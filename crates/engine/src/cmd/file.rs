//! File menu: new, open, save, place, export, close.

use std::sync::Arc;

use designcraft_doc::build::{NewDocument, PRESETS};
use designcraft_doc::{Asset, AssetId, Content, Document, Graphic, Item, ItemId, Selection, Shape};
use designcraft_geom::{Affine, Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, f64_or, has_doc, ok, str_param};
use crate::{DocState, EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.new", "Document…", ["File", "New"], Some("Cmd+N"),
            "{preset?: \"Letter\"|\"A4\"|…, width?, height?, pages?, facingPages?, columns?, gutter?, margins?: number|{top,bottom,inside,outside}, bleed?, title?}",
            always, file_new),
        cmd!(noundo "file.newSample", "Sample Document", ["Help"], None, "{} — a multi-page magazine sample", always, file_sample),
        cmd!(query "file.presets", "Document Presets", [], None, "{}", always, |_, _| Ok(serde_json::to_value(PRESETS).unwrap_or_default())),
        cmd!(noundo "file.open", "Open…", ["File"], Some("Cmd+O"),
            "{path} — .designcraft or .idml; the fonts in a `Document Fonts` folder beside it load first → {index, documentFonts: faces loaded, warnings: font files skipped (and, for .idml, package parts missing)}",
            always, file_open),
        cmd!(noundo "file.openBytes", "Open Bytes", [], None, "{name, base64} — DesignCraft JSON or an IDML package", always, file_open_bytes),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?}", has_doc, file_save),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path}", has_doc, file_save),
        cmd!(noundo "file.saveACopy", "Save a Copy…", ["File"], None, "{path} — writes the document without changing which file it is or its unsaved state", has_doc, |s, p| {
            let path = str_param(p, "path").ok_or_else(|| bad("file.saveACopy", "missing `path`"))?.to_string();
            let bytes = to_bytes(&s.doc()?.doc);
            #[cfg(not(target_arch = "wasm32"))]
            std::fs::write(&path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }),
        cmd!(noundo "file.revert", "Revert", ["File"], None, "{} — back to the last saved version (not undoable)", can_revert, file_revert),
        cmd!(query "file.recovery.list", "Recovery Data", [], None, "{dir?} → [{uid, path, title, saved}] unsaved documents left by a crash (default: the session's recovery folder)", always, |s, p| {
            let dir = recovery_dir(s, p)?;
            Ok(Value::Array(crate::recovery::list(&dir).into_iter().map(|(uid, mut m)| { m["uid"] = json!(uid); m }).collect()))
        }),
        cmd!(noundo "file.recovery.save", "Save Recovery Data", [], None, "{dir?} — write every unsaved document to the recovery folder (the app does this on a timer)", always, |s, p| {
            let dir = recovery_dir(s, p)?;
            Ok(json!({"saved": crate::recovery::save(s, &dir)?}))
        }),
        cmd!(noundo "file.recovery.open", "Recover Documents", [], None, "{dir?} — reopen documents left in the recovery folder (unsaved)", always, |s, p| {
            let dir = recovery_dir(s, p)?;
            Ok(json!({"opened": crate::recovery::open(s, &dir)?}))
        }),
        cmd!(query "file.serialize", "Serialize", [], None, "{} → {base64, bytes} the .designcraft file", has_doc, |s, _| {
            let st = s.doc()?;
            // Preview fill is session state. Serialize the unfilled template.
            let doc = st.preview_stash.as_deref().unwrap_or(st.doc.as_ref());
            let bytes = to_bytes(doc);
            Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()}))
        }),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{index?}", has_doc, |s, p| {
            let i = p.get("index").and_then(Value::as_u64).map(|v| v as usize).or(s.active_index()).unwrap_or(0);
            s.close_document(i);
            ok()
        }),
        cmd!(noundo "file.activate", "Activate Document", [], None, "{index}", has_doc, |s, p| {
            s.set_active(p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize);
            ok()
        }),
        cmd!(query "snippet.export", "Export Selection as Snippet", ["File", "Export"], None,
            "{path?} — the selected items (with their stories, styles and images) as a .designcraft snippet; no path: {base64}", super::has_selection, snippet_export),
        cmd!(query "place.styles", "Import Options: Styles", [], None,
        "{path | base64, name} → {paragraph: [name], character: [name], conflicts: [name]} — the styles a Word/RTF file brings (for styleMap / styleConflicts)",
        has_doc, |s, p| {
            let (bytes, name, _) = read_source(p)?;
            let imp = designcraft_textimport::import(&name, &bytes).map_err(|e| bad("place.styles", e.to_string()))?;
            let st = &s.doc()?.doc.styles;
            let conflicts: Vec<&str> = imp
                .para_styles
                .iter()
                .filter(|x| st.para(&x.name).is_some())
                .chain(imp.char_styles.iter().filter(|x| st.char_style(&x.name).is_some()))
                .map(|x| x.name.as_str())
                .collect();
            Ok(json!({"paragraph": imp.para_styles.iter().map(|x| &x.name).collect::<Vec<_>>(), "character": imp.char_styles.iter().map(|x| &x.name).collect::<Vec<_>>(), "conflicts": conflicts}))
        }),
        cmd!(
            "snippet.place",
            "Place Snippet",
            [],
            None,
            "{path | base64, spread?, x?, y?} — items keep their positions unless x/y given (top-left)",
            has_doc,
            snippet_place
        ),
        cmd!(
            "place.load",
            "Load Place Cursor",
            [],
            None,
            "{path | base64, name?} — load a graphic into the place cursor (then click/drag with the placeGun tool)",
            has_doc,
            place_load
        ),
        cmd!("place.drop", "Place Loaded Graphic", [], None, "{spread?, x, y, rect?: [x0,y0,x1,y1], frame?: id}", has_doc, place_drop),
        cmd!(
            "file.place",
            "Place…",
            ["File"],
            Some("Cmd+D"),
            "{path?|base64?, name?, frame?: id (place into), spread?, x?, y?, width?, pdfPage?: n (1-based, Image Import Options), pdfCrop?: crop|trim|bleed|art|media, layoutPage?: n (IDML / .designcraft: that page's objects as a group)} — places an image (into the selected empty frame if any); text files (.txt, .docx, .rtf, .md) and Excel workbooks (.xlsx, as a table) go into the insertion point, the selected frame or a new frame on `page`/`rect` — {autoflow?: adds pages with threaded frames until the text fits, removeStyles?, styleMap?: {imported name: document style}, styleConflicts?: useExisting|redefine|autoRename}",
            has_doc,
            file_place
        ),
    ]
}

pub fn to_bytes(d: &Document) -> Vec<u8> {
    designcraft_format::save(d).unwrap_or_default()
}

pub fn from_bytes(b: &[u8]) -> Result<Document> {
    designcraft_format::load(b).map_err(|e| EngineError::Other(e.to_string()))
}

fn file_new(s: &mut Session, p: &Value) -> Result<Value> {
    let mut nd = match str_param(p, "preset") {
        Some(name) => NewDocument::from_preset(name).ok_or_else(|| bad("file.new", format!("unknown preset `{name}`")))?,
        None => NewDocument::default(),
    };
    nd.width = f64_or(p, "width", nd.width);
    nd.height = f64_or(p, "height", nd.height);
    nd.pages = p.get("pages").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(nd.pages).clamp(1, 9999);
    nd.facing_pages = p.get("facingPages").and_then(Value::as_bool).unwrap_or(nd.facing_pages);
    nd.columns = p.get("columns").and_then(Value::as_u64).map(|v| v.clamp(1, 216) as u32).unwrap_or(nd.columns);
    nd.gutter = f64_or(p, "gutter", nd.gutter);
    nd.primary_text_frame = p.get("primaryTextFrame").and_then(Value::as_bool).unwrap_or(false);
    match p.get("margins") {
        Some(Value::Number(n)) => nd.margins = designcraft_doc::Margins::uniform(n.as_f64().unwrap_or(36.0)),
        Some(v @ Value::Object(_)) => {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                nd.margins = m;
            }
        }
        _ => {}
    }
    if let Some(b) = p.get("bleed").and_then(Value::as_f64) {
        nd.bleed = [b; 4];
    }
    s.untitled += 1;
    nd.title = str_param(p, "title").map(str::to_string).unwrap_or_else(|| format!("Untitled-{}", s.untitled));
    if nd.width <= 0.0 || nd.height <= 0.0 || nd.width > 15552.0 || nd.height > 15552.0 {
        return Err(bad("file.new", "page size out of range (0 < size ≤ 216 in)"));
    }
    let d = Document::new(&nd);
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}

fn file_sample(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = crate::sample::magazine();
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}

fn file_open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.open", "missing `path`"))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        if path.to_ascii_lowercase().ends_with(".idml") {
            return super::interchange::open_idml(s, p);
        }
        let bytes = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        let mut d = from_bytes(&bytes)?;
        super::interchange::resolve_packaged_links(&mut d, std::path::Path::new(path).parent());
        super::datamerge::resolve_sources_on_open(&mut d, Some(std::path::Path::new(path)));
        let (fonts, faces, warnings) = load_document_fonts(&mut d, path);
        let mut st = DocState::new(d, Some(path.to_string()));
        st.fonts = fonts;
        let i = s.add_document(st);
        Ok(json!({"index": i, "documentFonts": faces, "warnings": warnings}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path);
        Err(EngineError::Other("use file.openBytes on the web".into()))
    }
}

/// Load the fonts in the `Document Fonts` folder beside the document file at `path` for `d` alone
/// (`d.font_scope`) → (the scope, for the document's state to hold; faces; a warning for each
/// font file skipped). Nothing on the web (no folders).
pub(crate) fn load_document_fonts(d: &mut Document, path: &str) -> (Option<Arc<crate::FontScope>>, usize, Vec<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
        let Some(folder) = std::path::Path::new(path).parent().map(document_fonts_folder) else { return (None, 0, Vec::new()) };
        let r = designcraft_fonts::FontDb::global().load_document_fonts(&folder);
        d.font_scope = r.scope;
        let scope = (r.scope != 0).then(|| Arc::new(crate::FontScope(r.scope)));
        (scope, r.faces, r.skipped.into_iter().map(|s| format!("{DOCUMENT_FONTS_FOLDER}: {s}")).collect())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (d, path);
        (None, 0, Vec::new())
    }
}

/// The `Document Fonts` folder in `dir`, matched without regard to case: packages name it
/// `Document fonts`, and file systems on Linux are case-sensitive. The exact name wins.
#[cfg(not(target_arch = "wasm32"))]
fn document_fonts_folder(dir: &std::path::Path) -> std::path::PathBuf {
    use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
    let exact = dir.join(DOCUMENT_FONTS_FOLDER);
    if exact.is_dir() {
        return exact;
    }
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| e.file_name().to_str().is_some_and(|n| n.eq_ignore_ascii_case(DOCUMENT_FONTS_FOLDER)) && e.path().is_dir())
        .map_or(exact, |e| e.path())
}

fn file_open_bytes(s: &mut Session, p: &Value) -> Result<Value> {
    let b = base64_decode(str_param(p, "base64").unwrap_or(""));
    // IDML packages are recognised by their stored `mimetype` first entry.
    if designcraft_idml::is_idml(&b) {
        return super::interchange::open_idml(s, p);
    }
    let mut d = from_bytes(&b)?;
    super::datamerge::resolve_sources_on_open(&mut d, None);
    if let Some(n) = str_param(p, "name") {
        d.title = n.to_string();
    }
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}

fn file_save(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc_mut()?;
    let path = str_param(p, "path")
        .map(str::to_string)
        .or_else(|| st.path.clone())
        .ok_or_else(|| bad("file.save", "missing `path` (document has never been saved)"))?;
    {
        let mut d = (*st.doc).clone();
        super::datamerge::refresh_relative_paths(&mut d, std::path::Path::new(&path));
        if d.data_merge != st.doc.data_merge {
            st.doc = Arc::new(d);
            st.revision = st.revision.saturating_add(1);
        }
    }
    let bytes = to_bytes(&st.doc);
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::write(&path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    st.path = Some(path.clone());
    st.saved_revision = st.revision;
    st.saved_doc = st.doc.clone();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dir) = &s.recovery_dir {
        let uid = s.doc()?.uid;
        crate::recovery::discard(dir, uid);
    }
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

fn file_place(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name, link) = if let Some(b) = str_param(p, "base64") {
        (base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None)
    } else if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let b = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = vec![];
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
        (b, name, Some(path.to_string()))
    } else {
        s.ui_requests.push(crate::UiRequest::Pick { purpose: "place".into(), params: json!({}) });
        return ok();
    };
    // A page of another layout (IDML or DesignCraft): its objects, as one group.
    let lname = name.to_lowercase();
    if lname.ends_with(".idml") || lname.ends_with(".designcraft") {
        return place_layout_page(s, p, &name, &bytes);
    }
    // EPS: shown and printed through its preview (or a placeholder) at its bounding box size.
    let (bytes, eps_size) = if designcraft_images::is_eps(&bytes) {
        let (proxy, size) = designcraft_images::eps_proxy(&bytes).ok_or_else(|| bad("file.place", "the EPS has no bounding box"))?;
        (proxy, Some(size))
    } else {
        (bytes, None)
    };
    // Video and sound: a media frame (Window › Interactive › Media).
    if let Some(mime) = designcraft_doc::media_mime(&name) {
        return super::media::place_media(s, p, name, mime, bytes, link);
    }
    // Text files (plain, Word, RTF) flow into frames.
    if designcraft_textimport::is_text_file(&name) {
        return super::place_text::place_text(s, p, &name, &bytes);
    }
    // Image Import Options: which page of a PDF (1-based).
    let pdf_page = match p.get("pdfPage").and_then(Value::as_u64) {
        Some(n) if designcraft_render::is_pdf(&bytes) => {
            let count = designcraft_render::pdf_page_count(&bytes).unwrap_or(1);
            let invalid_page = || bad("file.place", format!("the PDF has {count} page(s)"));
            let one_based = usize::try_from(n).map_err(|_| invalid_page())?;
            if one_based == 0 || one_based > count {
                return Err(invalid_page());
            }
            let zero_based = one_based.checked_sub(1).ok_or_else(invalid_page)?;
            u32::try_from(zero_based).map_err(|_| invalid_page())?
        }
        _ => 0,
    };
    let (pw, ph) = match pdf_page {
        0 => designcraft_render::image_size(&bytes).ok_or_else(|| bad("file.place", "unsupported or corrupt image"))?,
        n => designcraft_render::pdf_page_size(&bytes, n as usize)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
            .ok_or_else(|| bad("file.place", "can't read that PDF page"))?,
    };
    // 72 ppi by default unless the file says otherwise; scale so it fits the page when huge.
    let (nw, nh) = eps_size.unwrap_or((pw as f64, ph as f64));
    // Image Import Options › Crop to: the frame shows that box of the PDF page.
    let crop_box = match str_param(p, "pdfCrop") {
        Some(k) if designcraft_render::is_pdf(&bytes) => {
            if !["crop", "trim", "bleed", "art", "media"].contains(&k) {
                return Err(bad("file.place", format!("unknown pdfCrop `{k}` (crop, trim, bleed, art, media)")));
            }
            designcraft_render::pdf_page_box(&bytes, pdf_page as usize, k).filter(|b| b.2 > 0.0 && b.3 > 0.0)
        }
        Some(_) => None,
        None => None,
    };
    let target_frame = super::id_param(p, "frame").or_else(|| {
        let st = s.active()?;
        st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| matches!(it.content, Content::Unassigned | Content::Graphic(_))))
    });
    let spread = super::spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    let px = p.get("x").and_then(Value::as_f64);
    let py = p.get("y").and_then(Value::as_f64);
    let want_w = p.get("width").and_then(Value::as_f64);
    s.edit(|d, sel| {
        let aid = AssetId(d.alloc());
        let mime = designcraft_render::image_mime(&bytes).to_string();
        d.assets.insert(aid, Arc::new(Asset { page: pdf_page, id: aid, name, mime, link, data: Arc::new(bytes), pixels: Some((pw, ph)) }));
        // The width the whole page (or image) gets; the height follows the proportions.
        let w = match want_w {
            Some(w) => w,
            None => {
                let page_w = d.settings.page_width * 0.6;
                nw.min(page_w)
            }
        };
        let id = if let Some(fid) = target_frame {
            let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
            let r = it.inner_bounds();
            // Fill frame proportionally.
            let k = (r.width() / nw).max(r.height() / nh);
            let (gw, gh) = (nw * k, nh * k);
            it.content = Content::Graphic(Graphic {
                asset: aid,
                size: (nw, nh),
                xf: Affine::translate((r.x0 + (r.width() - gw) / 2.0, r.y0 + (r.height() - gh) / 2.0)) * Affine::scale(k),
                auto_fit: designcraft_doc::Fitting::FillProportionally,
                fit_align: 4,
                crop: [0.0; 4],
            });
            fid
        } else {
            let sp = d.spread(spread).ok_or_else(|| bad("file.place", "no such spread"))?;
            let pr = sp.pages.first().map(|pg| pg.margin_rect()).unwrap_or(Rect::new(36.0, 36.0, 300.0, 300.0));
            let (x, y) = (px.unwrap_or(pr.x0), py.unwrap_or(pr.y0));
            let id = ItemId(d.alloc());
            let k = w / nw;
            let (bx, by, bw, bh) = crop_box.unwrap_or((0.0, 0.0, nw, nh));
            let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(Rect::new(x, y, x + bw * k, y + bh * k)));
            it.object_style = d.styles.default_graphic_frame.clone();
            it.content = Content::Graphic(Graphic {
                asset: aid,
                size: (nw, nh),
                xf: Affine::translate((x - bx * k, y - by * k)) * Affine::scale(k),
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
            });
            d.insert_item(spread, it, None)?;
            id
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0, "asset": aid.0}))
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

pub(crate) fn read_source(p: &Value) -> Result<(Vec<u8>, String, Option<String>)> {
    if let Some(b) = str_param(p, "base64") {
        return Ok((base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None));
    }
    let path = str_param(p, "path").ok_or_else(|| bad("place", "missing `path` or `base64`"))?;
    #[cfg(not(target_arch = "wasm32"))]
    let b = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    #[cfg(target_arch = "wasm32")]
    let b: Vec<u8> = vec![];
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
    Ok((b, name, Some(path.to_string())))
}

fn place_load(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name, link) = read_source(p)?;
    let (pw, ph) = designcraft_render::image_size(&bytes).ok_or_else(|| bad("place.load", "unsupported or corrupt image"))?;
    let aid = s.edit(|d, _| {
        let aid = AssetId(d.alloc());
        let mime = designcraft_render::image_mime(&bytes).to_string();
        d.assets.insert(
            aid,
            Arc::new(Asset { page: 0, id: aid, name: name.clone(), mime, link: link.clone(), data: Arc::new(bytes), pixels: Some((pw, ph)) }),
        );
        Ok(aid)
    })?;
    s.loaded = Some((aid, (pw as f64, ph as f64)));
    s.set_tool("placeGun");
    Ok(json!({"asset": aid.0, "name": name, "width": pw, "height": ph}))
}

fn place_drop(s: &mut Session, p: &Value) -> Result<Value> {
    let (aid, (nw, nh)) = s.loaded.ok_or_else(|| bad("place.drop", "nothing loaded in the place cursor"))?;
    let sr = super::spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    let frame = super::id_param(p, "frame");
    let rect = super::rect_param(p, "rect");
    let (x, y) = (super::f64_or(p, "x", 0.0), super::f64_or(p, "y", 0.0));
    let r = s.edit(|d, sel| {
        let id = match frame {
            Some(fid) => {
                let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
                let r = it.inner_bounds();
                let k = (r.width() / nw).max(r.height() / nh);
                it.content = Content::Graphic(Graphic {
                    asset: aid,
                    size: (nw, nh),
                    xf: Affine::translate((r.x0 + (r.width() - nw * k) / 2.0, r.y0 + (r.height() - nh * k) / 2.0)) * Affine::scale(k),
                    auto_fit: designcraft_doc::Fitting::FillProportionally,
                    fit_align: 4,
                    crop: [0.0; 4],
                });
                fid
            }
            None => {
                // Drag: fit proportionally into the dragged rect; click: actual size at the point.
                let (frame_r, k) = match rect {
                    Some(r) if r.width() > 2.0 && r.height() > 2.0 => {
                        let k = (r.width() / nw).min(r.height() / nh);
                        (Rect::new(r.x0, r.y0, r.x0 + nw * k, r.y0 + nh * k), k)
                    }
                    _ => (Rect::new(x, y, x + nw, y + nh), 1.0),
                };
                let id = ItemId(d.alloc());
                let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(frame_r));
                it.object_style = d.styles.default_graphic_frame.clone();
                it.content = Content::Graphic(Graphic {
                    asset: aid,
                    size: (nw, nh),
                    xf: Affine::translate((frame_r.x0, frame_r.y0)) * Affine::scale(k),
                    auto_fit: Default::default(),
                    fit_align: 4,
                    crop: [0.0; 4],
                });
                d.insert_item(sr, it, None)?;
                id
            }
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })?;
    s.loaded = None;
    s.set_tool("selection");
    Ok(r)
}

/// The selection as `.designcraft` snippet bytes.
pub(crate) fn snippet_bytes(s: &mut Session) -> Result<Vec<u8>> {
    Ok(to_bytes(&super::edit::clip_doc(s)?))
}

fn snippet_export(s: &mut Session, p: &Value) -> Result<Value> {
    let d = super::edit::clip_doc(s)?;
    let bytes = to_bytes(&d);
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            std::fs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            Ok(json!({"path": path, "bytes": bytes.len(), "items": d.spreads.first().map(|s| s.items.len()).unwrap_or(0)}))
        }
        None => Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()})),
    }
}

/// File › Place of an IDML or DesignCraft document: the objects of one page (`layoutPage`,
/// 1-based) grouped, top-left at x/y (default: the source position on the first page).
fn place_layout_page(s: &mut Session, p: &Value, name: &str, bytes: &[u8]) -> Result<Value> {
    const ID: &str = "file.place";
    let src = if name.to_lowercase().ends_with(".idml") {
        designcraft_idml::import_idml(bytes).map_err(|e| bad(ID, e.to_string()))?
    } else {
        from_bytes(bytes)?
    };
    let n = src.page_count();
    let page = p.get("layoutPage").and_then(Value::as_u64).unwrap_or(1) as usize;
    if page == 0 || page > n {
        return Err(bad(ID, format!("the document has {n} page(s)")));
    }
    let (si, pi) = src.page_loc(page - 1).ok_or_else(|| bad(ID, "no such page"))?;
    let sp = &src.spreads[si];
    let pr = sp.pages[pi].bounds();
    let ids: Vec<ItemId> = sp.items.iter().filter(|it| !it.hidden && sp.page_at_x(it.bounds().center().x) == Some(pi)).map(|it| it.id).collect();
    if ids.is_empty() {
        return Err(bad(ID, format!("page {page} has no objects")));
    }
    let to = super::spread_param(p, "spread");
    let off = match (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => designcraft_geom::Vec2::new(x - pr.x0, y - pr.y0),
        _ => designcraft_geom::Vec2::new(-pr.x0, -pr.y0),
    };
    let placed = s.edit(|d, sel| {
        for sw in &src.swatches {
            if d.swatch(&sw.name).is_none() {
                d.swatches.push(sw.clone());
            }
        }
        let missing_p: Vec<_> = src.styles.paragraph.iter().filter(|ps| d.styles.para(&ps.name).is_none()).cloned().collect();
        let missing_c: Vec<_> = src.styles.character.iter().filter(|cs| d.styles.char_style(&cs.name).is_none()).cloned().collect();
        if !missing_p.is_empty() || !missing_c.is_empty() {
            let st = d.styles_mut();
            st.paragraph.extend(missing_p);
            st.character.extend(missing_c);
        }
        let new = super::object::duplicate_from(d, &src, &ids, to, off)?;
        *sel = Selection::items(new.clone());
        Ok(new)
    })?;
    if placed.len() > 1 {
        let g = s.execute("object.group", &json!({"ids": placed.iter().map(|i| i.0).collect::<Vec<_>>()}))?;
        return Ok(json!({"id": g["id"], "items": placed.len()}));
    }
    Ok(json!({"id": placed[0].0, "items": 1}))
}

pub(crate) fn snippet_place(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, _, _) = read_source(p)?;
    let src = from_bytes(&bytes)?;
    let ids: Vec<ItemId> = src.spreads.first().map(|sp| sp.items.iter().map(|i| i.id).collect()).unwrap_or_default();
    if ids.is_empty() {
        return Err(bad("snippet.place", "the snippet is empty"));
    }
    let sr = super::spread_param(p, "spread");
    let bounds = ids.iter().filter_map(|i| src.item(*i).map(|it| it.bounds())).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
    let off = match (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => designcraft_geom::Vec2::new(x - bounds.x0, y - bounds.y0),
        _ => designcraft_geom::Vec2::ZERO,
    };
    s.edit(|d, sel| {
        // Bring over styles and swatches the snippet uses that this document lacks.
        for sw in &src.swatches {
            if d.swatch(&sw.name).is_none() {
                d.swatches.push(sw.clone());
            }
        }
        let missing_p: Vec<_> = src.styles.paragraph.iter().filter(|ps| d.styles.para(&ps.name).is_none()).cloned().collect();
        let missing_c: Vec<_> = src.styles.character.iter().filter(|cs| d.styles.char_style(&cs.name).is_none()).cloned().collect();
        if !missing_p.is_empty() || !missing_c.is_empty() {
            let st = d.styles_mut();
            st.paragraph.extend(missing_p);
            st.character.extend(missing_c);
        }
        let new = super::object::duplicate_from(d, &src, &ids, sr, off)?;
        *sel = Selection::items(new.clone());
        Ok(json!({"ids": new.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

fn recovery_dir(s: &Session, p: &Value) -> Result<std::path::PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(EngineError::Other("no recovery folder on the web".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    str_param(p, "dir")
        .map(std::path::PathBuf::from)
        .or_else(|| s.recovery_dir.clone())
        .ok_or_else(|| bad("file.recovery", "no recovery folder (give `dir`)"))
}

fn can_revert(s: &Session) -> std::result::Result<(), String> {
    match s.active() {
        Some(d) if d.path.is_some() && d.is_dirty() => Ok(()),
        Some(d) if d.path.is_none() => Err("the document has never been saved".into()),
        Some(_) => Err("no changes since the last save".into()),
        None => Err("no document open".into()),
    }
}

fn file_revert(s: &mut Session, _: &Value) -> Result<Value> {
    let path = s.doc()?.path.clone().ok_or_else(|| bad("file.revert", "the document has never been saved"))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut d = if path.to_ascii_lowercase().ends_with(".idml") {
            let bytes = std::fs::read(&path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            designcraft_idml::import_idml(&bytes).map_err(|e| EngineError::Other(e.to_string()))?
        } else {
            from_bytes(&std::fs::read(&path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?)?
        };
        // As opened again: the fonts beside it now (unchanged files are shared, not read again).
        let (fonts, _, _) = load_document_fonts(&mut d, &path);
        let uid = s.doc()?.uid;
        let st = s.doc_mut()?;
        let mut fresh = DocState::new(d, Some(path.clone()));
        fresh.fonts = fonts;
        fresh.uid = uid;
        fresh.revision = st.revision + 1;
        *st = fresh;
        if let Some(dir) = &s.recovery_dir {
            crate::recovery::discard(dir, uid);
        }
        Ok(json!({"path": path}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path);
        Err(EngineError::Other("revert isn't available on the web".into()))
    }
}

#[cfg(test)]
mod pdf_crop_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_pdf_cropped_to_its_trim_box() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
        let b64 = s.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap().to_string();
        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        let size = |t: &mut Session, crop: &str| {
            t.execute("edit.deselectAll", &json!({})).unwrap();
            let r = t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": crop, "width": 630, "x": 0, "y": 0})).unwrap();
            let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
            t.doc().unwrap().doc.item(id).unwrap().bounds()
        };
        let full = size(&mut t, "crop");
        let trim = size(&mut t, "trim");
        assert!((full.width() - 630.0).abs() < 1e-6, "{full:?}");
        assert!((trim.width() - 612.0).abs() < 1e-3 && (trim.height() - 792.0).abs() < 1e-3, "{trim:?}");
        assert!(t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": "nope"})).is_err());
    }
}

#[cfg(test)]
mod layout_place_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_a_page_of_an_idml_file_as_a_group() {
        let mut a = Session::new();
        a.execute("file.new", &json!({"pages": 2})).unwrap();
        a.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "text", "text": "From page one", "caret": false})).unwrap();
        a.execute("frame.create", &json!({"rect": [300, 400, 350, 450]})).unwrap();
        let idml = designcraft_idml::export_idml(&a.doc().unwrap().doc);
        let b64 = super::base64_encode(&idml);
        let mut b = Session::new();
        b.execute("file.new", &json!({})).unwrap();
        let r = b.execute("file.place", &json!({"base64": b64, "name": "src.idml", "layoutPage": 1, "x": 0, "y": 0})).unwrap();
        assert_eq!(r["items"], 2);
        let d = &b.doc().unwrap().doc;
        let g = d.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap();
        assert_eq!(g.children().len(), 2, "one group");
        let bb = g.bounds();
        assert!((bb.x0 - 100.0).abs() < 1e-6 && (bb.y0 - 100.0).abs() < 1e-6, "page top-left at 0,0: {bb:?}");
        assert!(d.stories.values().any(|st| st.text == "From page one"), "stories come along");
        assert!(b.execute("file.place", &json!({"base64": b64, "name": "src.idml", "layoutPage": 2})).is_err(), "page 2 is empty");
    }
}

#[cfg(test)]
mod document_fonts_tests {
    use std::path::{Path, PathBuf};

    use designcraft_fonts::testing::font_with;
    use designcraft_fonts::{DOCUMENT_FONTS_FOLDER, FaceRef, FontDb, FontSource};
    use serde_json::{Value, json};

    use crate::Session;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dc-open-docfonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(DOCUMENT_FONTS_FOLDER)).unwrap();
        dir
    }

    /// A new document in `s` with a text frame holding `text` set in `family`.
    fn new_document(s: &mut Session, family: &str, text: &str) {
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": text.len()})).unwrap();
        s.execute("type.char", &json!({"fontFamily": family, "fontStyle": "Regular"})).unwrap();
    }

    /// A document whose text ("Hello") is set in `family`, written to `path` (`.idml`: as IDML).
    fn write_document(path: &Path, family: &str) {
        write_document_with(path, family, "Hello");
    }

    fn write_document_with(path: &Path, family: &str, text: &str) {
        let mut s = Session::new();
        new_document(&mut s, family, text);
        if path.extension().is_some_and(|e| e == "idml") {
            std::fs::write(path, designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        } else {
            s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        }
    }

    fn font_entry(s: &mut Session, family: &str) -> Value {
        let l = s.execute("font.list", &json!({})).unwrap();
        l.as_array().unwrap().iter().find(|f| f["family"] == family).cloned().unwrap_or_else(|| panic!("{family} in {l}"))
    }

    /// Each character of the active document's text with the face it is drawn in.
    fn glyph_faces(s: &Session) -> Vec<(char, FaceRef)> {
        let st = s.doc().unwrap();
        let mut out = Vec::new();
        for (sid, story) in &st.doc.stories {
            let cs = s.cache.get(&st.doc, *sid, None);
            for g in cs.frames.iter().flat_map(|ft| &ft.lines).flat_map(|l| &l.glyphs).filter(|g| g.visible && g.len > 0) {
                out.push((story.text[g.byte..].chars().next().unwrap(), g.face));
            }
        }
        out
    }

    fn open(s: &mut Session, path: &Path) -> Value {
        s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap()
    }

    #[test]
    fn opening_a_document_loads_its_document_fonts() {
        const FAMILY: &str = "DocFont Open Test";
        let dir = temp_dir("designcraft");
        let fonts = dir.join(DOCUMENT_FONTS_FOLDER);
        std::fs::write(fonts.join("open.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        std::fs::write(fonts.join("broken.otf"), b"not a font").unwrap();
        let path = dir.join("Brochure.designcraft");
        write_document(&path, FAMILY);
        assert!(!FontDb::global().has_family(FAMILY));

        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!(r["documentFonts"], 1, "{r}");
        assert!(r["warnings"].to_string().contains("broken.otf"), "a damaged font is reported, not fatal: {r}");
        let f = font_entry(&mut s, FAMILY);
        assert_eq!((f["missing"].clone(), f["source"].clone()), (json!(false), json!("document")), "{f}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_an_idml_file_loads_its_document_fonts() {
        const FAMILY: &str = "DocFont IDML Test";
        let dir = temp_dir("idml");
        std::fs::write(dir.join(DOCUMENT_FONTS_FOLDER).join("idml.ttf"), font_with(FAMILY, &['H']).unwrap()).unwrap();
        let path = dir.join("Brochure.idml");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!(r["documentFonts"], 1, "{r}");
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_document_fonts_folder_is_found_whatever_its_case() {
        const FAMILY: &str = "DocFont Case Test";
        let dir = std::env::temp_dir().join(format!("dc-open-docfonts-case-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fonts = dir.join("Document fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        std::fs::write(fonts.join("case.ttf"), font_with(FAMILY, &['H']).unwrap()).unwrap();
        let path = dir.join("Brochure.idml");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        let r = open(&mut s, &path);
        assert_eq!(r["documentFonts"], 1, "{r}");
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_document_without_the_folder_opens_as_before() {
        let dir = std::env::temp_dir().join(format!("dc-open-docfonts-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Plain.designcraft");
        write_document(&path, designcraft_fonts::DEFAULT_FAMILY);
        let r = Session::new().execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!((r["documentFonts"].clone(), r["warnings"].clone()), (json!(0), json!([])), "{r}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn document_fonts_belong_to_their_document() {
        const FAMILY: &str = "DocFont Scoped";
        let (a, b) = (temp_dir("scoped-a"), temp_dir("scoped-b"));
        std::fs::write(a.join(DOCUMENT_FONTS_FOLDER).join("scoped.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        write_document(&a.join("A.designcraft"), FAMILY);
        write_document(&b.join("B.designcraft"), FAMILY);

        let mut s = Session::new();
        assert_eq!(open(&mut s, &a.join("A.designcraft"))["documentFonts"], 1);
        assert_eq!(open(&mut s, &b.join("B.designcraft"))["documentFonts"], 0);
        // B, open at the same time, doesn't have A's font.
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], true);
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family != FAMILY), "{:?}", glyph_faces(&s));
        s.execute("file.activate", &json!({"index": 0})).unwrap();
        assert_eq!(font_entry(&mut s, FAMILY)["source"], "document");
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family == FAMILY), "{:?}", glyph_faces(&s));
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn each_document_sets_its_own_font_of_a_name() {
        const FAMILY: &str = "DocFont Same";
        let (a, b) = (temp_dir("same-a"), temp_dir("same-b"));
        let (a_font, b_font) = (a.join(DOCUMENT_FONTS_FOLDER).join("same.ttf"), b.join(DOCUMENT_FONTS_FOLDER).join("same.ttf"));
        // A's font has both letters, B's only `b`.
        std::fs::write(&a_font, font_with(FAMILY, &['a', 'b']).unwrap()).unwrap();
        std::fs::write(&b_font, font_with(FAMILY, &['b']).unwrap()).unwrap();
        // The same words in both: the word cache must not hand one document's shaping to the other.
        write_document_with(&a.join("A.designcraft"), FAMILY, "ab ab ab");
        write_document_with(&b.join("B.designcraft"), FAMILY, "ab ab ab");

        let mut s = Session::new();
        open(&mut s, &a.join("A.designcraft"));
        open(&mut s, &b.join("B.designcraft"));
        // Fallback fonts for the `a` B's font lacks: never A's font.
        s.execute("document.preferences", &json!({"glyphFallback": true})).unwrap();
        let in_b = glyph_faces(&s);
        s.execute("file.activate", &json!({"index": 0})).unwrap();
        let in_a = glyph_faces(&s);
        assert_eq!((in_a.len(), in_b.len()), (8, 8), "{in_a:?} {in_b:?}");
        for (c, f) in &in_a {
            assert_eq!(f.source, FontSource::Document(a_font.clone()), "`{c}` in A");
        }
        for (c, f) in &in_b {
            match c {
                'a' => assert!(f.family != FAMILY, "B's font lacks `a`, and A's isn't B's: {f:?}"),
                _ => assert_eq!(f.source, FontSource::Document(b_font.clone()), "`{c}` in B"),
            }
        }
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn closing_a_document_forgets_its_fonts_and_reopening_reuses_them() {
        const FAMILY: &str = "DocFont Closed";
        let dir = temp_dir("closed");
        std::fs::write(dir.join(DOCUMENT_FONTS_FOLDER).join("closed.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        let path = dir.join("Closed.designcraft");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        open(&mut s, &path);
        let first = glyph_faces(&s)[0].1;
        assert_eq!(first.family, FAMILY);
        s.execute("file.close", &json!({})).unwrap();
        // A new document can't use the closed document's font.
        new_document(&mut s, FAMILY, "Hello");
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], true);
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family != FAMILY));
        // Reopened, the document has its font again: the face loaded the first time.
        assert_eq!(open(&mut s, &path)["documentFonts"], 1);
        assert!(glyph_faces(&s).iter().all(|(_, f)| *f == first), "{:?}", glyph_faces(&s));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reverted_document_keeps_its_fonts() {
        const FAMILY: &str = "DocFont Reverted";
        let dir = temp_dir("revert");
        std::fs::write(dir.join(DOCUMENT_FONTS_FOLDER).join("revert.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        let path = dir.join("Revert.designcraft");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        open(&mut s, &path);
        s.execute("frame.create", &json!({"rect": [72, 400, 200, 500], "content": "text", "text": "x"})).unwrap();
        s.execute("file.revert", &json!({})).unwrap();
        assert_eq!(font_entry(&mut s, FAMILY)["source"], "document");
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family == FAMILY));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
