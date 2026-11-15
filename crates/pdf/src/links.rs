//! Interactive PDF: hyperlink annotations (frames and text ranges → URLs, e-mail, pages) and the
//! bookmark outline.

use craft_pdf::write::backend as krilla;
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
                let m: Affine = doc.parent_xf(&loc) * doc.text_xf(it);
                for l in &ft.lines {
                    if l.range.end <= *start || l.range.start >= *end {
                        continue;
                    }
                    // Every glyph the source's characters produced, not only the ones that own
                    // their bytes: the glyphs of generated text — a cross-reference's — all point
                    // at the one marker character, and only the first of them owns it, so asking
                    // for owned bytes measured the first letter and nothing else (#1446).
                    //
                    // The bullet or number a paragraph's list adds is not the paragraph's text,
                    // but the composer gives it the first character's offset and no length, so a
                    // link starting at that character took the label in and the rectangle
                    // covered text outside the link. So `list_label` glyphs are left out, and so are the zero-length glyphs that are not
                    // generated text (kenten marks, ruby, kashidas, tab leaders, a cluster's later glyphs): they
                    // sit over or inside text that is counted already (noted in review).
                    for (x0, x1) in visual_link_spans(l.glyphs.iter().filter_map(|g| {
                        // Decorative marks do not partition a text row. Generated cross-reference
                        // glyphs do count even when only their first glyph owns the source byte.
                        if g.list_label || (g.len == 0 && g.generated_text.is_none()) {
                            return None;
                        }
                        let stacked = cs.styles.get(g.style as usize).is_some_and(|style| style.warichu);
                        Some((link_row(stacked, g.y), g.x, g.x + g.adv, g.byte >= *start && g.byte < *end))
                    })) {
                        let r = Rect::new(x0, l.baseline - l.ascent, x1, l.baseline + l.descent);
                        out.push(m.transform_rect_bbox(r));
                    }
                }
            }
            out
        }
    }
}

/// Selected horizontal spans, split by intervening unselected text, not by empty
/// letter spacing. The composer retains logical glyph order after bidi positioning;
/// sort the positioned text by row and x instead. Separate rows (warichu, for example)
/// must not act as separators for one another. Non-finite or zero-width glyphs cannot
/// define clickable coverage or a visible separator.
fn link_row(stacked: bool, y: f64) -> f64 {
    if stacked { y } else { 0.0 }
}

fn visual_link_spans(glyphs: impl Iterator<Item = (f64, f64, f64, bool)>) -> Vec<(f64, f64)> {
    let mut glyphs: Vec<_> = glyphs.filter(|(y, x0, x1, _)| y.is_finite() && x0.is_finite() && x1.is_finite() && x1 > x0).collect();
    glyphs.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    let mut out = Vec::new();
    let mut first = 0;
    while first < glyphs.len() {
        let row = glyphs[first].0;
        let end = first + glyphs[first..].iter().take_while(|g| g.0 == row).count();
        let items = &glyphs[first..end];
        let selected = items.iter().filter(|g| g.3);
        let left = selected.clone().map(|g| g.1).fold(f64::INFINITY, f64::min);
        let right = selected.map(|g| g.2).fold(f64::NEG_INFINITY, f64::max);
        let mut cursor = left;
        // Subtract unselected advances even when they overlap selected glyphs. Prefer
        // a smaller clickable region over assigning another text cluster this link.
        for (_, x0, x1, _) in items.iter().filter(|g| !g.3) {
            if *x1 <= cursor || *x0 >= right {
                continue;
            }
            if *x0 > cursor {
                out.push((cursor, *x0));
            }
            cursor = cursor.max(*x1);
        }
        if cursor < right {
            out.push((cursor, right));
        }
        first = end;
    }
    out
}

#[cfg(test)]
mod visual_link_tests {
    use super::{link_row, visual_link_spans};

    #[test]
    fn bidi_selection_does_not_cover_intervening_unselected_text() {
        // Logical order differs from visual order: selected Hebrew at the right,
        // then a selected digit at the left, with unselected digits between them.
        let glyphs = [(0.0, 30.0, 40.0, true), (0.0, 0.0, 10.0, true), (0.0, 10.0, 20.0, false), (0.0, 20.0, 30.0, false)];
        assert_eq!(visual_link_spans(glyphs.into_iter()), [(0.0, 10.0), (30.0, 40.0)]);
    }

    #[test]
    fn letter_spacing_and_zero_width_marks_do_not_split_selected_text() {
        let glyphs = [(0.0, 0.0, 10.0, true), (0.0, 12.0, 22.0, true), (0.0, 11.0, 11.0, false)];
        assert_eq!(visual_link_spans(glyphs.into_iter()), [(0.0, 22.0)]);
    }

    #[test]
    fn baseline_shifted_unselected_text_is_still_a_separator() {
        let glyphs = [(link_row(false, 0.0), 0.0, 10.0, true), (link_row(false, -5.0), 10.0, 20.0, false), (link_row(false, 0.0), 20.0, 30.0, true)];
        assert_eq!(visual_link_spans(glyphs.into_iter()), [(0.0, 10.0), (20.0, 30.0)]);
    }

    #[test]
    fn overlapping_unselected_advance_is_not_clickable() {
        let glyphs = [(0.0, 0.0, 20.0, true), (0.0, 10.0, 15.0, false), (0.0, 15.0, 25.0, true)];
        assert_eq!(visual_link_spans(glyphs.into_iter()), [(0.0, 10.0), (15.0, 25.0)]);
    }

    #[test]
    fn another_text_row_is_not_a_separator() {
        let glyphs = [(0.0, 0.0, 10.0, true), (0.0, 20.0, 30.0, true), (link_row(true, 5.0), 10.0, 20.0, false)];
        assert_eq!(visual_link_spans(glyphs.into_iter()), [(0.0, 30.0)]);
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
    for (xf, it) in crate::export::exported_items(doc, sh.spread) {
        let Some(action) = &it.button else { continue };
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
