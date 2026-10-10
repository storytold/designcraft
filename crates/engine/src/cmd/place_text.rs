//! File › Place of text files (plain text, Word, RTF): into the text insertion point, the
//! selected frame, or a new frame on the page; with autoflow, pages and threaded frames are added
//! until the whole story fits (InDesign's Shift-click with a loaded text cursor).

use designcraft_doc::{CharacterStyle, Content, Document, ParaFormat, ParagraphStyle, SpreadRef, Story, StoryId, TextSel};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::bad;
use crate::{Result, Session};

/// Add the imported styles the document doesn't have yet (existing styles win, as in InDesign's
/// default style-conflict option).
/// How imported styles that share a name with the document's are handled (Word Import Options).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conflict {
    /// Use the document's definition.
    UseExisting,
    /// The imported definition replaces the document's.
    Redefine,
    /// Import under a new name (`Name_wrd_1`).
    AutoRename,
}

/// Map imported style names (`styleMap`) and resolve name clashes in the import before it lands.
fn map_styles(d: &Document, imp: &mut designcraft_textimport::Imported, map: &std::collections::HashMap<String, String>, conflict: Conflict) {
    let mut rename: std::collections::HashMap<String, String> = map.clone();
    if conflict == Conflict::AutoRename {
        let st = &d.styles;
        for s in imp.para_styles.iter().filter(|s| !map.contains_key(&s.name) && st.para(&s.name).is_some()) {
            if let Some(n) = (1..=st.paragraph.len() + 1).map(|i| format!("{}_wrd_{i}", s.name)).find(|n| st.para(n).is_none()) {
                rename.insert(s.name.clone(), n);
            }
        }
        for s in imp.char_styles.iter().filter(|s| !map.contains_key(&s.name) && st.char_style(&s.name).is_some()) {
            if let Some(n) = (1..=st.character.len() + 1).map(|i| format!("{}_wrd_{i}", s.name)).find(|n| st.char_style(n).is_none()) {
                rename.insert(s.name.clone(), n);
            }
        }
    }
    if rename.is_empty() {
        return;
    }
    // Mapped styles aren't imported; renamed ones come in under their new name.
    imp.para_styles.retain(|s| !map.contains_key(&s.name));
    imp.char_styles.retain(|s| !map.contains_key(&s.name));
    for s in imp.para_styles.iter_mut().chain(imp.char_styles.iter_mut()) {
        if let Some(n) = rename.get(&s.name) {
            s.name = n.clone();
        }
        if let Some(b) = s.based_on.as_ref().and_then(|b| rename.get(b)) {
            s.based_on = Some(b.clone());
        }
    }
    let n = imp.story.len();
    imp.story.format_paras(0..n, |p| {
        if let Some(m) = rename.get(&p.style) {
            p.style = m.clone();
        }
    });
    imp.story.format_chars(0..n, |f| {
        if let Some(m) = rename.get(&f.style) {
            f.style = m.clone();
        }
    });
}

fn add_styles(d: &mut Document, imp: &designcraft_textimport::Imported, conflict: Conflict) {
    let st = d.styles_mut();
    if conflict == Conflict::Redefine {
        for s in &imp.para_styles {
            if let Some(ps) = st.paragraph.iter_mut().find(|p| p.name == s.name) {
                (ps.para, ps.chars) = (s.para.clone(), s.chars.clone());
            }
        }
        for s in &imp.char_styles {
            if let Some(cs) = st.character.iter_mut().find(|c| c.name == s.name) {
                cs.chars = s.chars.clone();
            }
        }
    }
    for s in &imp.para_styles {
        if st.para(&s.name).is_none() {
            st.paragraph.push(ParagraphStyle {
                name: s.name.clone(),
                based_on: s.based_on.clone().or_else(|| Some(designcraft_doc::story::BASIC_PARAGRAPH.into())),
                next_style: None,
                para: s.para.clone(),
                chars: s.chars.clone(),
                shortcut: String::new(),
            });
        }
    }
    for s in &imp.char_styles {
        if !st.character.iter().any(|c| c.name == s.name) {
            st.character.push(CharacterStyle { name: s.name.clone(), based_on: s.based_on.clone(), chars: s.chars.clone(), shortcut: String::new() });
        }
    }
}

/// "Remove styles and formatting": every paragraph [Basic Paragraph], no local formatting.
fn strip(story: &mut Story) {
    let n = story.len();
    story.format_paras(0..n, |p| *p = ParaFormat { table: p.table, ..Default::default() });
    story.format_chars(0..n, |f| *f = Default::default());
}

/// Is the story overset in its frames, and how much text do its full frames hold (bytes, frames)?
/// A frame a column, frame or page break ends early is left out: it says nothing about how much
/// a frame holds.
fn fit(d: &Document, sid: StoryId) -> (Option<usize>, usize, usize) {
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    let text = d.story(sid).map_or("", |s| s.text.as_str());
    let is_break = |c: char| matches!(c, designcraft_doc::COLUMN_BREAK | designcraft_doc::FRAME_BREAK | designcraft_doc::PAGE_BREAK);
    let (mut held, mut full) = (0, 0);
    for f in &cs.frames {
        let Some(shown) = text.get(f.range.clone()) else { continue };
        if !shown.is_empty() && !shown.chars().any(is_break) {
            held += shown.len();
            full += 1;
        }
    }
    (cs.overset_at, held, full)
}

/// Thread new frames on new pages (margin rectangles) until the story fits. Returns pages added.
pub(crate) fn autoflow(d: &mut Document, sid: StoryId, max_pages: usize) -> Result<usize> {
    let mut added = 0;
    loop {
        let (overset, held, full) = fit(d, sid);
        let Some(at) = overset else { return Ok(added) };
        if added >= max_pages {
            return Err(bad("file.place", format!("autoflow stopped after {max_pages} pages")));
        }
        let len = d.story(sid).map_or(0, |s| s.len());
        // Frames still needed at the rate the full ones are filled (at least one); with no full
        // frame to go by yet, one more page.
        let want = held.checked_div(full).map_or(1, |per_frame| len.saturating_sub(at).div_ceil(per_frame.max(1))).clamp(1, max_pages - added);
        let last = *d.story(sid).and_then(|s| s.frames.last()).ok_or_else(|| bad("file.place", "the story has no frame"))?;
        let mut page = d.page_of_item(last).unwrap_or(d.page_count().saturating_sub(1));
        let parent = d.page_loc(page).and_then(|(si, pi)| d.spreads[si].pages[pi].parent);
        d.insert_pages(Some(page), want, parent)?;
        let layer = d.default_layer();
        let mut prev = last;
        for _ in 0..want {
            page += 1;
            let (si, pi) = d.page_loc(page).ok_or_else(|| bad("file.place", "page insert failed"))?;
            let r = d.spreads[si].pages[pi].margin_rect();
            let (fid, _) = d.add_text_frame(SpreadRef::Doc(si), r, layer, "", ParaFormat::default())?;
            d.thread(prev, fid)?;
            prev = fid;
        }
        added += want;
    }
}

pub(super) fn place_text(s: &mut Session, p: &Value, name: &str, bytes: &[u8]) -> Result<Value> {
    let mut imp = designcraft_textimport::import(name, bytes).map_err(|e| bad("file.place", e.to_string()))?;
    let remove = p.get("removeStyles").and_then(Value::as_bool).unwrap_or(false);
    if remove {
        strip(&mut imp.story);
        imp.para_styles.clear();
        imp.char_styles.clear();
    }
    let conflict = match p.get("styleConflicts").and_then(Value::as_str).unwrap_or("useExisting") {
        "useExisting" => Conflict::UseExisting,
        "redefine" => Conflict::Redefine,
        "autoRename" => Conflict::AutoRename,
        c => return Err(bad("file.place", format!("unknown styleConflicts `{c}` (useExisting, redefine, autoRename)"))),
    };
    let map: std::collections::HashMap<String, String> = p
        .get("styleMap")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    map_styles(&s.doc()?.doc, &mut imp, &map, conflict);
    let autoflow_on = p.get("autoflow").and_then(Value::as_bool).unwrap_or(false);
    let st = s.doc()?;
    let caret = st.selection.text.filter(|t| t.cell.is_none());
    let frame = super::id_param(p, "frame").or_else(|| {
        st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| matches!(it.content, Content::Unassigned | Content::Text(_))))
    });
    let page = p.get("page").and_then(Value::as_u64).map(|v| (v as usize).saturating_sub(1));
    let rect = super::rect_param(p, "rect");
    let warnings = imp.warnings.clone();
    s.edit(|d, sel| {
        add_styles(d, &imp, conflict);
        let sid = if let Some(t) = caret {
            // Into the text at the insertion point (replacing the selection).
            let story = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
            let r = t.range();
            story.delete(r.clone());
            let end = story.insert_story(r.start, &imp.story);
            sel.text = Some(TextSel { anchor: end, focus: end, ..t });
            t.story
        } else {
            let fid = match frame {
                Some(f) => f,
                None => {
                    let pg = page.unwrap_or(0).min(d.page_count().saturating_sub(1));
                    let (si, pi) = d.page_loc(pg).ok_or_else(|| bad("file.place", "no such page"))?;
                    let r: Rect = rect.unwrap_or_else(|| d.spreads[si].pages[pi].margin_rect());
                    let layer = d.default_layer();
                    d.add_text_frame(SpreadRef::Doc(si), r, layer, "", ParaFormat::default())?.0
                }
            };
            // Empty frames become text frames.
            let sid = match d.item(fid).map(|i| i.content.clone()) {
                Some(Content::Text(t)) => t.story,
                Some(_) => {
                    let sid = StoryId(d.alloc());
                    let mut ns = Story::new(sid);
                    ns.direction = d.new_story_direction();
                    ns.frames = vec![fid];
                    d.stories.insert(sid, std::sync::Arc::new(ns));
                    if let Some(it) = d.item_mut(fid) {
                        it.content = Content::Text(designcraft_doc::TextFrame { story: sid, options: Default::default() });
                    }
                    sid
                }
                None => return Err(bad("file.place", "no such frame")),
            };
            let story = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
            let n = story.len();
            story.delete(0..n);
            story.insert_story(0, &imp.story);
            if let Some(first) = imp.story.paras.first() {
                story.paras[0] = ParaFormat { table: story.paras[0].table, ..first.clone() };
            }
            *sel = designcraft_doc::Selection::items(vec![fid]);
            sid
        };
        let pages = if autoflow_on { autoflow(d, sid, 2000)? } else { 0 };
        let overset = fit(d, sid).0.is_some();
        Ok(json!({"story": sid.0, "pagesAdded": pages, "overset": overset, "warnings": warnings}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_text_file_with_autoflow() {
        let dir = std::env::temp_dir().join(format!("dc-place-text-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        let para = "The quick brown fox jumps over the lazy dog, again and again, to fill the page. ".repeat(12);
        std::fs::write(&path, vec![para.as_str(); 60].join("\n")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "autoflow": true})).unwrap();
        assert!(r["pagesAdded"].as_u64().unwrap() >= 2, "{r}");
        assert_eq!(r["overset"], false);
        let d = &s.doc().unwrap().doc;
        let sid = StoryId(r["story"].as_u64().unwrap());
        assert_eq!(d.story(sid).unwrap().frames.len(), d.page_count());
        assert_eq!(d.story(sid).unwrap().paras.len(), 60);
        // Into the insertion point of another frame.
        let fr = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Start  end"})).unwrap();
        let sid2 = fr["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid2, "anchor": 6, "focus": 6})).unwrap();
        let small = dir.join("small.rtf");
        std::fs::write(&small, br"{\rtf1 {\b middle}}").unwrap();
        s.execute("file.place", &json!({"path": small.to_string_lossy()})).unwrap();
        let st = s.doc().unwrap().doc.story(StoryId(sid2)).unwrap().clone();
        assert_eq!(st.text, "Start middle end");
        assert_eq!(st.format_after(6).over.font_style.as_deref(), Some("Bold"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_adds_only_the_pages_breaks_ask_for() {
        // Regression (#262): a frame a break ends early made autoflow take it for a frame that
        // holds a few characters, and add a page per few characters left (33 for this text).
        let dir = std::env::temp_dir().join(format!("dc-place-breaks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, rtf, pages) in [
            ("lead.rtf", r"{\rtf1 \page\par A Short Heading\par First short paragraph.\par Second short paragraph.\par}", 2),
            ("each.rtf", r"{\rtf1 One\par\page\par Two\par\page\par Three\par\page\par Four\par}", 4),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, rtf).unwrap();
            let mut s = Session::new();
            s.execute("file.new", &json!({"pages": 1})).unwrap();
            let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "autoflow": true})).unwrap();
            assert_eq!(r["overset"], false, "{name}: {r}");
            let d = &s.doc().unwrap().doc;
            assert_eq!(d.page_count(), pages, "{name}: {r}");
            assert_eq!(d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames.len(), pages, "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod place_pdf_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_pdf_renders_and_exports_as_vector() {
        let dir = std::env::temp_dir().join(format!("dc-place-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("ad.pdf");
        // A PDF made by our own exporter: a black square on a page.
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
        s.execute("file.exportPdf", &json!({"path": src.to_string_lossy()})).unwrap();
        // Place it in a new document.
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("file.place", &json!({"path": src.to_string_lossy(), "x": 100, "y": 100})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let it = d.spreads[0].items.last().unwrap();
        let b = it.bounds();
        assert!((b.width() - 200.0).abs() < 1.0 && (b.height() - 100.0).abs() < 1.0, "sized by the PDF page: {b:?} {r}");
        let asset = d.assets.values().next().unwrap();
        assert_eq!(asset.mime, "application/pdf");
        // On screen: the square is black, the rest of the placed page white.
        let mut rr = designcraft_render::Renderer::new();
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        assert!(img.pixel(150, 150)[0] < 90, "{:?}", img.pixel(150, 150));
        assert!(img.pixel(250, 150)[0] > 200, "{:?}", img.pixel(250, 150));
        // Exported: still a valid PDF, larger than a page without the placed PDF would be.
        let out = dir.join("out.pdf");
        s.execute("file.exportPdf", &json!({"path": out.to_string_lossy()})).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let pdf = hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.clone())).expect("valid pdf");
        assert_eq!(pdf.pages().len(), 1);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Subtype /Form") || text.contains("/Subtype/Form"), "embedded as a form XObject");
        assert!(!text.contains("/Subtype /Image") && !text.contains("/Subtype/Image"), "not rasterized");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn place_svg_on_screen_and_as_vectors_in_pdf() {
        let dir = std::env::temp_dir().join(format!("dc-place-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("logo.svg");
        std::fs::write(
            &src,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="80"><rect width="80" height="80" fill="#0000ff"/></svg>"##,
        )
        .unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": src.to_string_lossy(), "x": 100, "y": 100})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let b = d.spreads[0].items.last().unwrap().bounds();
        assert!((b.width() - 120.0).abs() < 1.0 && (b.height() - 60.0).abs() < 1.0, "160×80 px = 120×60 pt: {b:?}");
        assert_eq!(d.assets.values().next().unwrap().mime, "image/svg+xml");
        let mut rr = designcraft_render::Renderer::new();
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let p = img.pixel(130, 130);
        assert!(p[2] > 200 && p[0] < 60, "blue square on screen: {p:?}");
        let out = dir.join("out.pdf");
        s.execute("file.exportPdf", &json!({"path": out.to_string_lossy()})).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.clone())).expect("valid pdf");
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("/Subtype /Image") && !text.contains("/Subtype/Image"), "not rasterized");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod reflow_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn smart_text_reflow_adds_and_removes_pages() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "primaryTextFrame": true})).unwrap();
        let sid = s.doc().unwrap().doc.settings.primary_story.expect("primary story").0;
        assert_eq!(s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().frames.len(), 1);
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        let long = "Words keep coming and the page fills up quickly with them. ".repeat(250);
        s.execute("text.insert", &json!({"text": long})).unwrap();
        let pages = s.doc().unwrap().doc.page_count();
        assert!(pages >= 3, "pages added: {pages}");
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(designcraft_doc::StoryId(sid)).unwrap().frames.len(), pages);
        // One undo takes the typing and its pages back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
        s.execute("edit.redo", &json!({})).unwrap();
        // Delete the text: the empty pages at the end go.
        let n = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().len();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": n})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
        s.doc().unwrap().doc.check().unwrap();
        // Off: no pages added.
        s.execute("prefs.set", &json!({"smartTextReflow": false})).unwrap();
        s.execute("text.insert", &json!({"text": long})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
    }
}

#[cfg(test)]
mod pdf_page_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_a_chosen_pdf_page() {
        let dir = std::env::temp_dir().join(format!("dc-pdf-page-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("two.pdf");
        // Page 1 white, page 2 a different size with a black square.
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false, "width": 200, "height": 100})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [2], "width": 300, "height": 150})).unwrap();
        let id = s.execute("frame.create", &json!({"spread": 1, "rect": [0, 0, 300, 150]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
        s.execute("file.exportPdf", &json!({"path": src.to_string_lossy()})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("file.place", &json!({"path": src.to_string_lossy(), "pdfPage": 3})).is_err());
        s.execute("file.place", &json!({"path": src.to_string_lossy(), "pdfPage": 2, "x": 100, "y": 100})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let it = d.spreads[0].items.last().unwrap();
        let b = it.bounds();
        assert!((b.width() - 300.0).abs() < 1.0 && (b.height() - 150.0).abs() < 1.0, "page 2's size: {b:?}");
        assert_eq!(d.assets.values().next().unwrap().page, 1);
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        assert!(img.pixel(250, 170)[0] < 60, "page 2 (black) shows: {:?}", img.pixel(250, 170));
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /Form") || String::from_utf8_lossy(&pdf).contains("/Subtype/Form"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn word_file() -> Vec<u8> {
        use std::io::Write;
        const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let styles = format!(
            r#"<w:styles {W}><w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
            <w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:rPr><w:sz w:val="56"/></w:rPr></w:style>
            <w:style w:type="paragraph" w:styleId="Body"><w:name w:val="Body"/><w:rPr><w:sz w:val="20"/></w:rPr></w:style></w:styles>"#
        );
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Head</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Body"/></w:pPr><w:r><w:t>Text</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in [("word/document.xml", doc), ("word/styles.xml", styles)] {
            w.start_file(n, o).unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn word_import_options_map_and_rename_styles() {
        let b64 = crate::cmd::base64_encode(&word_file());
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 9}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Headline", "chars": {"size": 30}})).unwrap();
        let info = s.execute("place.styles", &json!({"base64": b64, "name": "a.docx"})).unwrap();
        assert_eq!(info["conflicts"], json!(["Body"]));
        // Title → the document's Headline; Body clashes → imported as Body_wrd_1.
        let r = s
            .execute("file.place", &json!({"base64": b64, "name": "a.docx", "styleMap": {"Title": "Headline"}, "styleConflicts": "autoRename"}))
            .unwrap();
        let d = &s.doc().unwrap().doc;
        let st = d.story(designcraft_doc::StoryId(r["story"].as_u64().unwrap())).unwrap();
        assert_eq!(st.paras.iter().map(|p| p.style.as_str()).collect::<Vec<_>>(), ["Headline", "Body_wrd_1"]);
        assert!(d.styles.para("Title").is_none(), "mapped styles aren't imported");
        assert_eq!(d.styles.para("Body").unwrap().chars.size, Some(9.0), "the document's Body is untouched");
        // Redefine: the imported definition replaces the document's.
        s.execute("file.place", &json!({"base64": b64, "name": "a.docx", "styleConflicts": "redefine", "rect": [72, 400, 300, 500]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(10.0));
    }
}
