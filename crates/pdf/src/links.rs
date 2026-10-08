//! Interactive PDF: hyperlink annotations (frames and text ranges → URLs, e-mail, pages) and the
//! bookmark outline.

use designcraft_doc::{Bookmark, Document, HyperlinkDest, HyperlinkSource, SpreadRef};
use designcraft_geom::{Affine, Rect};
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::destination::XyzDestination;
use krilla::outline::{Outline, OutlineNode};

use crate::export::Sheet;

/// Index of the output sheet showing absolute page `abs`.
pub(crate) fn sheet_of_page(doc: &Document, sheets: &[Sheet], abs: usize) -> Option<usize> {
    let (si, pi) = doc.page_loc(abs)?;
    let page = doc.spreads.get(si)?.pages.get(pi)?.bounds();
    sheets.iter().position(|s| s.spread == si && s.trim.x0 <= page.center().x && page.center().x <= s.trim.x1)
}

/// Spread-space rectangles covered by a hyperlink source on spread `si`.
fn source_rects(doc: &Document, cache: &designcraft_compose::Cache, src: &HyperlinkSource, si: usize) -> Vec<Rect> {
    match src {
        HyperlinkSource::Item { id } => match (doc.find(*id), doc.item(*id)) {
            (Some(loc), Some(it)) if loc.spread == SpreadRef::Doc(si) => vec![doc.parent_xf(&loc).transform_rect_bbox(it.bounds())],
            _ => vec![],
        },
        HyperlinkSource::Text { story, start, end } => {
            let cs = cache.get(doc, *story, None);
            let mut out = Vec::new();
            for ft in &cs.frames {
                let (Some(loc), Some(it)) = (doc.find(ft.frame), doc.item(ft.frame)) else { continue };
                if loc.spread != SpreadRef::Doc(si) {
                    continue;
                }
                let m: Affine = doc.parent_xf(&loc) * it.text_xf();
                for l in &ft.lines {
                    if l.range.end <= *start || l.range.start >= *end {
                        continue;
                    }
                    let gs: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte >= *start && g.byte < *end).collect();
                    let (Some(a), Some(b)) = (gs.first(), gs.last()) else { continue };
                    let r = Rect::new(a.x, l.baseline - l.ascent, b.x + b.adv, l.baseline + l.descent);
                    out.push(m.transform_rect_bbox(r));
                }
            }
            out
        }
    }
}

/// Link annotations for the sheet at `idx`.
pub(crate) fn annotations(doc: &Document, cache: &designcraft_compose::Cache, sheets: &[Sheet], idx: usize) -> Vec<Annotation> {
    let sh = &sheets[idx];
    let mut out = Vec::new();
    for h in &doc.hyperlinks {
        for r in source_rects(doc, cache, &h.source, sh.spread) {
            let r = r.intersect(sh.bleed);
            if r.width() <= 0.0 || r.height() <= 0.0 {
                continue;
            }
            let rect = krilla::geom::Rect::from_ltrb(
                (r.x0 - sh.media.x0) as f32,
                (r.y0 - sh.media.y0) as f32,
                (r.x1 - sh.media.x0) as f32,
                (r.y1 - sh.media.y0) as f32,
            );
            let Some(rect) = rect else { continue };
            let target = match &h.dest {
                HyperlinkDest::Url(u) => Target::Action(Action::Link(LinkAction::new(u.clone()))),
                HyperlinkDest::Email(e) => Target::Action(Action::Link(LinkAction::new(format!("mailto:{e}")))),
                HyperlinkDest::Page(p) => match sheet_of_page(doc, sheets, *p) {
                    Some(i) => Target::Destination(XyzDestination::new(i, krilla::geom::Point::from_xy(0.0, 0.0)).into()),
                    None => continue,
                },
            };
            out.push(Annotation::new_link(LinkAnnotation::new(rect, target), Some(h.name.clone())));
        }
    }
    // Buttons: their bounds act on release.
    let here = doc.spreads.get(sh.spread).and_then(|sp| {
        let first = doc.spreads[..sh.spread].iter().map(|s| s.pages.len()).sum::<usize>();
        sp.pages.iter().position(|p| p.bounds().intersect(sh.trim).area() > 0.0).map(|i| first + i)
    });
    let mut stack: Vec<(Affine, &std::sync::Arc<designcraft_doc::Item>)> =
        doc.spreads.get(sh.spread).map(|sp| sp.items.iter().map(|it| (Affine::IDENTITY, it)).collect()).unwrap_or_default();
    while let Some((xf, it)) = stack.pop() {
        stack.extend(it.shown_children().map(|c| (xf * it.child_space(), c)));
        let Some(action) = &it.button else { continue };
        if it.hidden {
            continue;
        }
        let r = xf.transform_rect_bbox(it.bounds()).intersect(sh.bleed);
        let rect = krilla::geom::Rect::from_ltrb(
            (r.x0 - sh.media.x0) as f32,
            (r.y0 - sh.media.y0) as f32,
            (r.x1 - sh.media.x0) as f32,
            (r.y1 - sh.media.y0) as f32,
        );
        let Some(rect) = rect.filter(|_| r.width() > 0.0 && r.height() > 0.0) else { continue };
        use designcraft_doc::ButtonAction as B;
        let total = doc.page_count();
        let page = match action {
            B::GoToUrl { url } => {
                out.push(Annotation::new_link(
                    LinkAnnotation::new(rect, Target::Action(Action::Link(LinkAction::new(url.clone())))),
                    Some(it.name.clone()),
                ));
                continue;
            }
            B::GoToPage { page } => Some(page.to_owned()),
            B::GoToFirstPage => Some(0),
            B::GoToLastPage => total.checked_sub(1),
            B::GoToNextPage => here.map(|h| h + 1).filter(|p| *p < total),
            B::GoToPreviousPage => here.and_then(|h| h.checked_sub(1)),
        };
        let Some(i) = page.and_then(|p| sheet_of_page(doc, sheets, p)) else { continue };
        let target = Target::Destination(XyzDestination::new(i, krilla::geom::Point::from_xy(0.0, 0.0)).into());
        out.push(Annotation::new_link(LinkAnnotation::new(rect, target), Some(it.name.clone())));
    }
    // Cross-references link to their destination's page.
    if doc.stories.values().any(|st| !st.xrefs.is_empty()) {
        let index = cache.xref_index(doc);
        for st in doc.stories.values().filter(|st| !st.xrefs.is_empty()) {
            for ((pos, _), x) in st.text.match_indices(designcraft_doc::XREF_MARK).zip(&st.xrefs) {
                let Some(i) = index.page_index(x.target).and_then(|p| sheet_of_page(doc, sheets, p)) else { continue };
                let src = HyperlinkSource::Text { story: st.id, start: pos, end: pos + designcraft_doc::XREF_MARK.len_utf8() };
                for r in source_rects(doc, cache, &src, sh.spread) {
                    let r = r.intersect(sh.bleed);
                    let rect = krilla::geom::Rect::from_ltrb(
                        (r.x0 - sh.media.x0) as f32,
                        (r.y0 - sh.media.y0) as f32,
                        (r.x1 - sh.media.x0) as f32,
                        (r.y1 - sh.media.y0) as f32,
                    );
                    let Some(rect) = rect.filter(|_| r.width() > 0.0 && r.height() > 0.0) else { continue };
                    let target = Target::Destination(XyzDestination::new(i, krilla::geom::Point::from_xy(0.0, 0.0)).into());
                    out.push(Annotation::new_link(LinkAnnotation::new(rect, target), Some(format!("Cross-reference: {}", x.format))));
                }
            }
        }
    }
    out
}

/// The document outline from the bookmarks (bookmarks to pages that weren't exported are skipped).
pub(crate) fn outline(doc: &Document, sheets: &[Sheet]) -> Option<Outline> {
    fn node(doc: &Document, sheets: &[Sheet], b: &Bookmark) -> Option<OutlineNode> {
        let i = sheet_of_page(doc, sheets, b.page)?;
        let mut n = OutlineNode::new(b.name.clone(), XyzDestination::new(i, krilla::geom::Point::from_xy(0.0, 0.0)));
        for c in &b.children {
            if let Some(cn) = node(doc, sheets, c) {
                n.push_child(cn);
            }
        }
        Some(n)
    }
    if doc.bookmarks.is_empty() {
        return None;
    }
    let mut o = Outline::new();
    for b in &doc.bookmarks {
        if let Some(n) = node(doc, sheets, b) {
            o.push_child(n);
        }
    }
    Some(o)
}
