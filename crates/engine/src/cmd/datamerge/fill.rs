//! Fill one data-merge record into a document, and copy template items for later records.
//!
//! Placeholders are addressed through an id map so each record's copy can be filled from the
//! unfilled template. A placeholder whose frame or story is not in the map is left as it is.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use designcraft_doc::{
    Asset, AssetId, Content, DataField, Fill, Fitting, Graphic, Hyperlink, HyperlinkDest, HyperlinkSource, Item, ItemId, MergeOptions,
    PlaceholderAnchor, PlaceholderRole, Shape, SpreadRef, Story, StoryId,
};
use designcraft_geom::{Affine, Rect, Vec2};

use super::parse::image_candidates;
use crate::EngineError;

#[derive(Clone, Debug, Default)]
pub struct Maps {
    pub items: HashMap<ItemId, ItemId>,
    pub stories: HashMap<StoryId, StoryId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MissingImage {
    pub record: u32,
    pub field: String,
    pub path: String,
}

#[derive(Clone, Debug, Default)]
pub struct FillReport {
    pub missing: Vec<MissingImage>,
    pub warnings: Vec<String>,
}

/// Document-spread items and their stories, each mapped to itself.
pub fn identity_map(doc: &designcraft_doc::Document) -> Maps {
    let mut maps = Maps::default();
    for sp in &doc.spreads {
        for it in &sp.items {
            it.walk(&mut |i| {
                maps.items.insert(i.id, i.id);
                if let Content::Text(tf) = &i.content {
                    maps.stories.insert(tf.story, tf.story);
                }
            });
        }
    }
    maps
}

/// Deep-copy one top-level item. Children stay in their own space; only this item is translated.
pub fn copy_item(
    dst: &mut designcraft_doc::Document,
    src: &designcraft_doc::Document,
    it: &Item,
    to: SpreadRef,
    off: Vec2,
    maps: &mut Maps,
) -> Result<(), EngineError> {
    let mut copy = it.clone();
    copy.xf = Affine::translate(off) * copy.xf;
    renumber(dst, src, &mut copy, maps);
    if dst.layer(copy.layer).is_none() {
        copy.layer = dst.default_layer();
    }
    dst.insert_item(to, copy, None)?;
    Ok(())
}

fn renumber(dst: &mut designcraft_doc::Document, src: &designcraft_doc::Document, it: &mut Item, maps: &mut Maps) {
    let old = it.id;
    let new_id = ItemId(dst.alloc());
    maps.items.insert(old, new_id);
    it.id = new_id;
    match &mut it.content {
        Content::Text(tf) => {
            let new_sid = if let Some(n) = maps.stories.get(&tf.story) {
                *n
            } else {
                let n = StoryId(dst.alloc());
                let mut st = src.story(tf.story).cloned().unwrap_or_else(|| Story::new(n));
                st.id = n;
                st.frames.clear();
                dst.stories.insert(n, Arc::new(st));
                maps.stories.insert(tf.story, n);
                n
            };
            tf.story = new_sid;
            if let Some(st) = dst.story_mut(new_sid) {
                st.frames.push(new_id);
            }
        }
        Content::Graphic(g) => {
            if let Some(a) = src.assets.get(&g.asset) {
                dst.assets.entry(g.asset).or_insert_with(|| a.clone());
            }
        }
        Content::Group { items } => {
            for child in items.iter_mut() {
                renumber(dst, src, Arc::make_mut(child), maps);
            }
        }
        Content::Unassigned => {}
    }
}

struct Rep {
    story: StoryId,
    start: usize,
    end: usize,
    text: String,
}

/// Fill `row` (0-based) into the items `maps` points at. `record` is the 1-based source row.
pub fn fill_row(
    doc: &mut designcraft_doc::Document,
    fields: &[DataField],
    row: &[String],
    record: u32,
    options: &MergeOptions,
    maps: &Maps,
    data_dir: Option<&Path>,
    doc_dir: Option<&Path>,
) -> FillReport {
    let mut report = FillReport::default();
    let mut parent = false;
    let mut reps: Vec<Rep> = Vec::new();
    let mut links: Vec<(PlaceholderAnchor, HyperlinkDest, String)> = Vec::new();
    let placeholders = doc.data_merge.placeholders.clone();
    for ph in &placeholders {
        if !fields.iter().any(|f| f.name == ph.field) {
            continue;
        }
        let cell = cell_of(fields, row, &ph.field).to_string();
        match &ph.anchor {
            PlaceholderAnchor::Text { story, start, end } => {
                let Some(&mapped) = maps.stories.get(story) else {
                    if anchor_is_parent(doc, &ph.anchor) {
                        parent = true;
                    }
                    continue;
                };
                match ph.role {
                    PlaceholderRole::Text => reps.push(Rep { story: mapped, start: *start, end: *end, text: cell }),
                    PlaceholderRole::Hyperlink => {
                        if let Some(dest) = classify_link(&cell) {
                            links.push((PlaceholderAnchor::Text { story: mapped, start: *start, end: *end }, dest, ph.field.clone()));
                        }
                    }
                    PlaceholderRole::Image | PlaceholderRole::Qr => {}
                }
            }
            PlaceholderAnchor::Item { id } => {
                let Some(&mapped) = maps.items.get(id) else {
                    if anchor_is_parent(doc, &ph.anchor) {
                        parent = true;
                    }
                    continue;
                };
                match ph.role {
                    PlaceholderRole::Image => place_image(doc, mapped, &ph.field, &cell, record, options, data_dir, doc_dir, &mut report),
                    PlaceholderRole::Qr => place_qr(doc, mapped, &ph.field, &cell, record, &mut report),
                    PlaceholderRole::Hyperlink => {
                        if let Some(dest) = classify_link(&cell) {
                            links.push((PlaceholderAnchor::Item { id: mapped }, dest, ph.field.clone()));
                        }
                    }
                    PlaceholderRole::Text => {}
                }
            }
        }
    }
    typed_reps(doc, fields, row, maps, &placeholders, &mut reps);
    apply_reps(doc, &reps);
    for (anchor, dest, field) in links {
        let anchor = match anchor {
            PlaceholderAnchor::Text { story, start, end } => {
                let (start, end) = adjust_range(story, start, end, &reps);
                PlaceholderAnchor::Text { story, start, end }
            }
            other => other,
        };
        if !create_link(doc, anchor, dest) {
            report.warnings.push(format!("Record {record}, field {field}: the hyperlink could not be created."));
        }
    }
    if parent {
        report.warnings.push("A parent-page placeholder was left unfilled.".into());
    }
    report
}

fn typed_reps(
    doc: &designcraft_doc::Document,
    fields: &[DataField],
    row: &[String],
    maps: &Maps,
    placeholders: &[designcraft_doc::Placeholder],
    reps: &mut Vec<Rep>,
) {
    let mut stories: Vec<(StoryId, StoryId)> = maps.stories.iter().map(|(a, b)| (*a, *b)).collect();
    stories.sort_by_key(|(a, _)| a.0);
    stories.dedup();
    for (original, mapped) in stories {
        let Some(st) = doc.story(mapped) else { continue };
        let covers: Vec<(usize, usize)> = placeholders
            .iter()
            .filter_map(|ph| match (&ph.role, &ph.anchor) {
                (PlaceholderRole::Text, PlaceholderAnchor::Text { story, start, end }) if *story == original => Some((*start, *end)),
                _ => None,
            })
            .collect();
        for (start, end, name) in find_markers(&st.text) {
            if covers.iter().any(|(a, b)| start < *b && end > *a) {
                continue;
            }
            if !fields.iter().any(|f| f.name == name) {
                continue;
            }
            reps.push(Rep { story: mapped, start, end, text: cell_of(fields, row, &name).to_string() });
        }
    }
}

fn find_markers(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with("<<")
            && let Some(rel) = text[i + 2..].find(">>")
        {
            let name = &text[i + 2..i + 2 + rel];
            if !name.is_empty() && !name.contains(['<', '>', '\n']) {
                let end = i + 2 + rel + 2;
                out.push((i, end, name.to_string()));
                i = end;
                continue;
            }
        }
        i += text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out
}

fn apply_reps(doc: &mut designcraft_doc::Document, reps: &[Rep]) {
    let mut stories: Vec<StoryId> = reps.iter().map(|r| r.story).collect();
    stories.sort();
    stories.dedup();
    for sid in stories {
        let mut mine: Vec<&Rep> = reps.iter().filter(|r| r.story == sid).collect();
        mine.sort_by_key(|r| std::cmp::Reverse(r.start));
        let Some(st) = doc.story_mut(sid) else { continue };
        for r in mine {
            if r.start <= r.end && r.end <= st.text.len() {
                st.replace(r.start..r.end, &r.text);
            }
        }
    }
}

fn adjust_range(story: StoryId, start: usize, end: usize, reps: &[Rep]) -> (usize, usize) {
    let mut mine: Vec<&Rep> = reps.iter().filter(|r| r.story == story).collect();
    mine.sort_by_key(|r| r.start);
    let mut s = start;
    let mut e = end;
    for r in mine {
        let old = r.end.saturating_sub(r.start);
        let new = r.text.len();
        let delta = new as isize - old as isize;
        if r.end <= start {
            s = add_delta(s, delta);
            e = add_delta(e, delta);
        } else if r.start == start && r.end == end {
            e = s.saturating_add(new);
        } else if r.start >= end {
        } else if r.end <= end {
            e = add_delta(e, delta);
        }
    }
    (s, e)
}

pub fn add_delta(pos: usize, delta: isize) -> usize {
    if delta >= 0 { pos.saturating_add(delta as usize) } else { pos.saturating_sub(delta.unsigned_abs()) }
}

fn create_link(doc: &mut designcraft_doc::Document, anchor: PlaceholderAnchor, dest: HyperlinkDest) -> bool {
    let source = match anchor {
        PlaceholderAnchor::Item { id } => {
            if doc.item(id).is_none() {
                return false;
            }
            HyperlinkSource::Item { id }
        }
        PlaceholderAnchor::Text { story, start, end } => {
            let Some(st) = doc.story(story) else { return false };
            if start > end || end > st.text.len() {
                return false;
            }
            HyperlinkSource::Text { story, start, end }
        }
    };
    let id = doc.alloc();
    let name = match &dest {
        HyperlinkDest::Url(u) | HyperlinkDest::Email(u) => u.clone(),
        HyperlinkDest::Page(p) => format!("Page {}", p + 1),
    };
    doc.hyperlinks.push(Hyperlink { id, name, source, dest });
    true
}

fn place_image(
    doc: &mut designcraft_doc::Document,
    item: ItemId,
    field: &str,
    cell: &str,
    record: u32,
    options: &MergeOptions,
    data_dir: Option<&Path>,
    doc_dir: Option<&Path>,
    report: &mut FillReport,
) {
    let cands = image_candidates(cell, data_dir, doc_dir);
    let found = cands.iter().find_map(|p| {
        // Refuse huge files before reading them; a too-large image counts as missing.
        let len = std::fs::metadata(p).ok()?.len();
        if len == 0 || len > super::MAX_IMAGE_FILE_BYTES {
            return None;
        }
        std::fs::read(p).ok().filter(|b| !b.is_empty()).map(|b| (p.clone(), b))
    });
    let Some((path, bytes)) = found else {
        if let Some(it) = doc.item_mut(item) {
            it.content = Content::Unassigned;
        }
        report.missing.push(MissingImage { record, field: field.to_string(), path: cell.to_string() });
        report.warnings.push(format!("Record {record}, field {field}: missing image {cell}."));
        return;
    };
    let Some((pw, ph)) = designcraft_render::image_size(&bytes) else {
        if let Some(it) = doc.item_mut(item) {
            it.content = Content::Unassigned;
        }
        report.missing.push(MissingImage { record, field: field.to_string(), path: cell.to_string() });
        report.warnings.push(format!("Record {record}, field {field}: missing image {cell}."));
        return;
    };
    let Some(inner) = doc.item(item).map(|it| it.inner_bounds()) else {
        report.warnings.push(format!("Record {record}, field {field}: missing image {cell}."));
        report.missing.push(MissingImage { record, field: field.to_string(), path: cell.to_string() });
        return;
    };
    let (nw, nh) = (f64::from(pw), f64::from(ph));
    let mode = fitting_of(&options.fitting);
    let aid = AssetId(doc.alloc());
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| field.to_string());
    let link = options.link_images.then(|| path.to_string_lossy().into_owned());
    doc.assets.insert(
        aid,
        Arc::new(Asset {
            id: aid,
            name,
            mime: designcraft_render::image_mime(&bytes).to_string(),
            link,
            data: Arc::new(bytes),
            pixels: Some((pw, ph)),
            page: 0,
        }),
    );
    let mut g = Graphic {
        asset: aid,
        size: (nw, nh),
        xf: Affine::translate((inner.x0, inner.y0)),
        auto_fit: mode,
        fit_align: 0,
        crop: [0.0; 4],
        wrap: Default::default(),
    };
    if let Some(xf) = g.fitted(inner, mode) {
        g.xf = xf;
    }
    if options.center && nw > 0.0 && nh > 0.0 {
        let bb = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, nw, nh));
        let c = inner.center();
        let m = bb.center();
        g.xf = Affine::translate((c.x - m.x, c.y - m.y)) * g.xf;
    }
    if let Some(it) = doc.item_mut(item) {
        it.content = Content::Graphic(g);
    }
}

fn place_qr(doc: &mut designcraft_doc::Document, item: ItemId, field: &str, cell: &str, record: u32, report: &mut FillReport) {
    if cell.is_empty() {
        report.warnings.push(format!("Record {record}, field {field}: the QR cell is empty."));
        return;
    }
    let Some(it) = doc.item(item).cloned() else {
        report.warnings.push(format!("Record {record}, field {field}: the QR content does not fit."));
        return;
    };
    let Some(path) = designcraft_geom::qr::qr_path(cell, it.inner_bounds()) else {
        report.warnings.push(format!("Record {record}, field {field}: the QR content does not fit."));
        return;
    };
    let id = ItemId(doc.alloc());
    let mut code = Item::new(id, it.layer, Shape::Path, path);
    code.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    code.alt_text = format!("QR code: {cell}");
    if let Some(frame) = doc.item_mut(item) {
        frame.content = Content::Group { items: vec![Arc::new(code)] };
    }
}

fn fitting_of(s: &str) -> Fitting {
    match s {
        "fillProportionally" => Fitting::FillProportionally,
        "fitContentToFrame" => Fitting::FitContentToFrame,
        "none" => Fitting::None,
        _ => Fitting::FitProportionally,
    }
}

fn classify_link(cell: &str) -> Option<HyperlinkDest> {
    let value = cell.trim();
    if value.is_empty() {
        return None;
    }
    // An explicit scheme must be a web or mail one; `javascript:`, `file:` and the like stay text.
    if let Some((scheme, _)) = value.split_once(':') {
        let is_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if is_scheme {
            let scheme = scheme.to_ascii_lowercase();
            return matches!(scheme.as_str(), "http" | "https" | "ftp" | "mailto").then(|| HyperlinkDest::Url(value.to_string()));
        }
    }
    if value.contains("://") || value.chars().any(char::is_whitespace) {
        return Some(HyperlinkDest::Url(value.to_string()));
    }
    if value.matches('@').count() == 1 && !value.contains(':') {
        return Some(HyperlinkDest::Email(value.to_string()));
    }
    Some(HyperlinkDest::Url(value.to_string()))
}

fn anchor_is_parent(doc: &designcraft_doc::Document, anchor: &PlaceholderAnchor) -> bool {
    match anchor {
        PlaceholderAnchor::Item { id } => matches!(doc.find(*id).map(|l| l.spread), Some(SpreadRef::Parent(_))),
        PlaceholderAnchor::Text { story, .. } => {
            let Some(st) = doc.story(*story) else { return false };
            !st.frames.is_empty() && st.frames.iter().all(|fid| matches!(doc.find(*fid).map(|l| l.spread), Some(SpreadRef::Parent(_))))
        }
    }
}

fn cell_of<'a>(fields: &[DataField], row: &'a [String], name: &str) -> &'a str {
    fields.iter().position(|f| f.name == name).and_then(|i| row.get(i)).map(String::as_str).unwrap_or("")
}

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn only_web_and_mail_schemes_become_links() {
        for ok in ["https://example.com", "HTTP://example.com", "ftp://files.example.com", "mailto:a@example.com", "www.example.com"] {
            assert!(matches!(classify_link(ok), Some(HyperlinkDest::Url(_))), "{ok}");
        }
        assert!(matches!(classify_link("a@example.com"), Some(HyperlinkDest::Email(_))));
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "data:text/html,hi", "vbscript:x", ""] {
            assert!(classify_link(bad).is_none(), "{bad}");
        }
    }
}
