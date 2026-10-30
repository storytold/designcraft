#[cfg(test)]
use super::bytes::base64_encode;
use super::{Arc, DocState, Document, NewDocument, Result, Session, Value, bad, f64_or, json, str_param};

/// The longest saved preset name, and how many presets are kept.
const MAX_PRESET_NAME: usize = 100;

const MAX_PRESETS: usize = 500;

/// A built-in document preset or a saved one, by name (case ignored).
fn find_preset(s: &Session, name: &str) -> Option<NewDocument> {
    NewDocument::from_preset(name).or_else(|| s.prefs.document_presets.iter().find(|p| p.title.eq_ignore_ascii_case(name.trim())).cloned())
}

/// The settings `file.new` makes a document with for `p`: its preset, then the settings over it.
fn new_document_settings(s: &Session, p: &Value) -> Result<NewDocument> {
    let mut nd = match str_param(p, "preset") {
        Some(name) => find_preset(s, name).ok_or_else(|| bad("file.new", format!("unknown preset `{name}`")))?,
        None => NewDocument::default(),
    };
    apply_settings(&mut nd, p);
    check_size("file.new", &nd)?;
    Ok(nd)
}

/// A document as `file.new` makes it. Its Basic Paragraph doesn't hyphenate capitalized words, a
/// paragraph's last word or across a column. Opened and imported files keep their own values,
/// which is why `ParaProps` keeps the IDML defaults (on).
fn new_document(nd: &NewDocument) -> Document {
    let mut d = Document::new(nd);
    if let Some(st) = Arc::make_mut(&mut d.styles).para_mut(designcraft_doc::BASIC_PARAGRAPH) {
        st.para.hyph_capitalized = Some(false);
        st.para.hyph_last_word = Some(false);
        st.para.hyph_across_column = Some(false);
    }
    d
}

pub(super) fn file_new(s: &mut Session, p: &Value) -> Result<Value> {
    let mut nd = new_document_settings(s, p)?;
    s.untitled += 1;
    nd.title = str_param(p, "title").map(str::to_string).unwrap_or_else(|| format!("Untitled-{}", s.untitled));
    let d = new_document(&nd);
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}

impl Session {
    /// New Document's Preview: a document made as `file.new` would make it with `p`, shown while
    /// the New Document window is open. It replaces the previous preview document in its tab and
    /// takes no Untitled number, no undo step and no recovery entry. Settings `file.new` refuses
    /// leave the previous preview as it was. Returns the preview document's uid.
    pub fn preview_new_document(&mut self, p: &Value) -> Result<u64> {
        let mut nd = new_document_settings(self, p)?;
        let previous = match self.new_document_preview {
            Some((_, previous)) => previous,
            None => self.active().map(|d| d.uid),
        };
        let shown = self.new_document_preview.and_then(|(old, _)| self.docs.iter().position(|d| d.uid == old));
        // A preview edited or saved by a menu or script while the window is open has become a
        // document of its own: it stays, and the preview gets a tab of its own.
        let replace = shown.filter(|&i| self.docs.get(i).is_some_and(|d| !d.is_dirty() && d.path.is_none()));
        if replace.is_none() && shown.is_some() {
            self.untitled += 1;
        }
        nd.title = str_param(p, "title").map(str::to_string).unwrap_or_else(|| self.next_untitled_title());
        let st = DocState::new(new_document(&nd), None);
        let uid = st.uid;
        match replace.and_then(|i| self.docs.get_mut(i).map(|d| (i, d))) {
            Some((i, d)) => {
                *d = st;
                self.active = Some(i);
            }
            None => {
                // Pushed without `add_document`: the document that was active stays as it is.
                self.docs.push(st);
                self.active = Some(self.docs.len() - 1);
            }
        }
        self.new_document_preview = Some((uid, previous));
        Ok(uid)
    }

    /// The uid of New Document's preview document, while one is shown.
    pub fn new_document_preview(&self) -> Option<u64> {
        self.new_document_preview.map(|(uid, _)| uid)
    }

    /// End New Document's preview. Its document goes, and if it was still the active document,
    /// the one that was active before it is active again. A preview that a menu or script has
    /// edited or saved stays open as a document, with an Untitled number. Returns the uid of the
    /// document removed. Without a preview it does nothing.
    pub fn end_new_document_preview(&mut self) -> Option<u64> {
        let (uid, previous) = self.new_document_preview.take()?;
        let i = self.docs.iter().position(|d| d.uid == uid)?;
        let d = self.docs.get(i)?;
        if d.is_dirty() || d.path.is_some() {
            self.untitled += 1;
            return None;
        }
        self.docs.remove(i);
        self.active = match self.active {
            Some(a) if a == i => previous.and_then(|p| self.docs.iter().position(|d| d.uid == p)).or_else(|| self.docs.len().checked_sub(1)),
            Some(a) if a > i => Some(a - 1),
            a => a,
        };
        Some(uid)
    }

    /// Create in the New Document window, with the settings the preview shows: the preview
    /// document becomes the new document, as `file.new` would make it (the next Untitled number,
    /// no undo history). Returns its index, or `None` when there is no unedited preview to keep.
    pub fn keep_new_document_preview(&mut self) -> Option<usize> {
        let (uid, _) = self.new_document_preview?;
        let i = self.docs.iter().position(|d| d.uid == uid && !d.is_dirty() && d.path.is_none())?;
        self.new_document_preview = None;
        self.untitled += 1;
        if let Some(d) = self.docs.get_mut(i) {
            d.history.undo.clear();
            d.history.redo.clear();
        }
        self.active = Some(i);
        Some(i)
    }
}

pub(super) fn save_preset(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("file.savePreset", "missing `name`"))?.to_string();
    if name.chars().count() > MAX_PRESET_NAME {
        return Err(bad("file.savePreset", format!("a preset name has at most {MAX_PRESET_NAME} characters")));
    }
    if NewDocument::from_preset(&name).is_some() {
        return Err(bad("file.savePreset", format!("`{name}` is a built-in preset")));
    }
    let mut nd = NewDocument::default();
    apply_settings(&mut nd, p);
    check_size("file.savePreset", &nd)?;
    nd.title = name.clone();
    let presets = &mut s.prefs.document_presets;
    presets.retain(|x| !x.title.eq_ignore_ascii_case(&name));
    if presets.len() >= MAX_PRESETS {
        return Err(bad("file.savePreset", format!("at most {MAX_PRESETS} saved presets")));
    }
    presets.push(nd);
    Ok(json!({"name": name}))
}

fn check_size(cmd: &str, nd: &NewDocument) -> Result<()> {
    if !(nd.width > 0.0 && nd.height > 0.0 && nd.width <= 15552.0 && nd.height <= 15552.0) {
        return Err(bad(cmd, "page size out of range (0 < size ≤ 216 in)"));
    }
    Ok(())
}

/// The document settings in `p` (as file.new takes them) over `nd`.
fn apply_settings(nd: &mut NewDocument, p: &Value) {
    if let Some(intent) = p.get("intent").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        nd.intent = intent;
    }
    if let Some(units) = p.get("units").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        nd.units = units;
    }
    nd.width = f64_or(p, "width", nd.width);
    nd.height = f64_or(p, "height", nd.height);
    nd.pages = p.get("pages").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(nd.pages).clamp(1, 9999);
    nd.start_page = p.get("startPage").and_then(Value::as_u64).map(|v| v.clamp(1, 99_999) as u32).unwrap_or(nd.start_page);
    nd.facing_pages = p.get("facingPages").and_then(Value::as_bool).unwrap_or(nd.facing_pages);
    nd.columns = p.get("columns").and_then(Value::as_u64).map(|v| v.clamp(1, 216) as u32).unwrap_or(nd.columns);
    nd.gutter = f64_or(p, "gutter", nd.gutter);
    nd.primary_text_frame = p.get("primaryTextFrame").and_then(Value::as_bool).unwrap_or(nd.primary_text_frame);
    match p.get("margins") {
        Some(Value::Number(n)) => nd.margins = designcraft_doc::Margins::uniform(n.as_f64().unwrap_or(36.0)),
        Some(v @ Value::Object(_)) => {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                nd.margins = m;
            }
        }
        _ => {}
    }
    if let Some(b) = edges(p, "bleed") {
        nd.bleed = b;
    }
    if let Some(b) = edges(p, "slug") {
        nd.slug = b;
    }
}

/// A bleed or slug parameter: one width for every edge, or [top, bottom, inside, outside].
fn edges(p: &Value, key: &str) -> Option<[f64; 4]> {
    let clean = |v: f64| if v.is_finite() { v.clamp(0.0, 1296.0) } else { 0.0 };
    match p.get(key)? {
        Value::Array(a) => match a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()?.as_slice() {
            [t, b, i, o] => Some([clean(*t), clean(*b), clean(*i), clean(*o)]),
            _ => None,
        },
        v => v.as_f64().map(|b| [clean(b); 4]),
    }
}

/// File › New Book Cover: a blank one-page flat cover (back, spine, front, left to right) sized for
/// a printer, with the bleed and safe margins set and guides on the two edges of the spine.
/// The spine comes from the printer's rule for `pages`, or is typed (`spine`, in points).
pub(super) fn file_new_cover(s: &mut Session, p: &Value) -> Result<Value> {
    use crate::cover::{Printer, spine_pt};
    const ID: &str = "file.newCover";
    let printer = str_param(p, "printer").and_then(Printer::parse).ok_or_else(|| bad(ID, "`printer`: lulu, hardback or other"))?;
    let size = |k: &str| {
        p.get(k)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && (18.0..=4000.0).contains(v))
            .ok_or_else(|| bad(ID, format!("`{k}` required: 18 to 4000 pt")))
    };
    let (trim_w, trim_h) = (size("trimWidth")?, size("trimHeight")?);
    let pages = p.get("pages").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    // A typed spine that is not a number is refused, not swapped for the printer's own rule.
    let typed_spine = match p.get("spine") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().ok_or_else(|| bad(ID, "`spine` must be a number of points"))?),
    };
    let spine = spine_pt(printer, pages, typed_spine).map_err(|e| bad(ID, e))?;
    // The safe margin and the bleed are lengths: finite, 0 to 1296 pt (the most a document edge takes),
    // and the margins must leave a live area.
    let length = |k: &str, default: f64| -> Result<f64> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(default),
            Some(v) => v
                .as_f64()
                .filter(|x| x.is_finite() && (0.0..=1296.0).contains(x))
                .ok_or_else(|| bad(ID, format!("`{k}` must be a length of 0 to 1296 pt"))),
        }
    };
    let (safe, bleed) = (length("safe", 36.0)?, length("bleed", 9.0)?);
    if 2.0 * safe >= trim_w.min(trim_h) {
        return Err(bad(ID, "`safe` is too large for the trim size"));
    }
    let settings = json!({
        "width": 2.0 * trim_w + spine, "height": trim_h, "pages": 1, "facingPages": false, "columns": 1, "primaryTextFrame": false,
        "margins": safe, "bleed": bleed,
    });
    let mut nd = new_document_settings(s, &settings)?;
    s.untitled += 1;
    nd.title = str_param(p, "title").map(str::to_string).unwrap_or_else(|| format!("Untitled-{}", s.untitled));
    let mut d = new_document(&nd);
    if let Some(page) = d.spreads.get_mut(0).and_then(|sp| Arc::make_mut(sp).pages.get_mut(0)) {
        for x in [trim_w, trim_w + spine].into_iter().take(if spine > 0.0 { 2 } else { 1 }) {
            page.guides.push(designcraft_doc::Guide {
                orientation: designcraft_doc::Orientation::Vertical,
                position: x,
                spread: false,
                locked: false,
                layer: None,
                liquid: false,
            });
        }
    }
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i, "width": 2.0 * trim_w + spine, "height": trim_h, "spine": spine}))
}

pub(super) fn file_sample(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = crate::sample::magazine();
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}

#[cfg(test)]
mod new_document_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn new_cover_makes_a_flat_one_page_cover_with_guides_on_the_spine() {
        let mut s = Session::new();
        let r = s.execute("file.newCover", &json!({"printer": "lulu", "pages": 224, "trimWidth": 612, "trimHeight": 792})).unwrap();
        let spine = 40.644;
        assert!((r["spine"].as_f64().unwrap() - spine).abs() < 1e-9);
        let st = s.doc().unwrap();
        assert!(!st.is_dirty() && st.history.undo.is_empty(), "a new cover is a clean document");
        let d = &st.doc;
        assert_eq!(d.page_count(), 1);
        let pg = d.page(0).unwrap();
        assert!((pg.width - (1224.0 + spine)).abs() < 1e-9 && pg.height == 792.0, "{}x{}", pg.width, pg.height);
        assert_eq!(d.settings.bleed, [9.0; 4]);
        assert_eq!((pg.margins.top, pg.margins.bottom), (36.0, 36.0));
        let at: Vec<f64> = pg.guides.iter().map(|g| g.position).collect();
        assert!(at.len() == 2 && (at[0] - 612.0).abs() < 1e-9 && (at[1] - (612.0 + spine)).abs() < 1e-9, "{at:?}");
    }

    #[test]
    fn new_cover_takes_a_hardback_a_typed_spine_a_bleed_and_refuses_nonsense() {
        let mut s = Session::new();
        let hb = s
            .execute("file.newCover", &json!({"printer": "hardback", "pages": 224, "trimWidth": 432, "trimHeight": 648, "bleed": 54, "safe": 18}))
            .unwrap();
        assert_eq!(hb["spine"].as_f64(), Some(58.5));
        assert_eq!(s.doc().unwrap().doc.settings.bleed, [54.0; 4]);
        let typed = s.execute("file.newCover", &json!({"printer": "other", "spine": 30, "trimWidth": 400, "trimHeight": 600})).unwrap();
        assert_eq!(typed["width"].as_f64(), Some(830.0));
        let none = s.execute("file.newCover", &json!({"printer": "other", "spine": 0, "trimWidth": 400, "trimHeight": 600})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page(0).unwrap().guides.len(), 1, "no spine, one centre guide: {none}");
        for bad in [
            json!({"printer": "kdp", "pages": 100, "trimWidth": 400, "trimHeight": 600}),
            json!({"printer": "other", "trimWidth": 400, "trimHeight": 600}),
            json!({"printer": "lulu", "pages": 100, "trimWidth": 5, "trimHeight": 600}),
            json!({"printer": "lulu", "pages": 100, "trimWidth": 400}),
            json!({"printer": "lulu", "pages": 0, "trimWidth": 400, "trimHeight": 600}),
        ] {
            let before = s.documents().len();
            assert!(s.execute("file.newCover", &bad).is_err(), "{bad}");
            assert_eq!(s.documents().len(), before, "a refused call opens nothing");
        }
    }

    #[test]
    fn new_cover_refuses_bad_margins_and_bleed_and_opens_nothing() {
        let mut s = Session::new();
        let base = json!({"printer": "other", "spine": 30, "trimWidth": 400, "trimHeight": 600});
        for (k, v) in [("safe", json!(-5)), ("bleed", json!(-1)), ("safe", json!("wide")), ("bleed", json!(true)), ("safe", json!(300))] {
            let mut p = base.clone();
            p[k] = v.clone();
            assert!(s.execute("file.newCover", &p).is_err(), "{k} = {v}");
        }
        assert_eq!(s.documents().len(), 0, "a refused call opens nothing");
        for (k, v) in [("bleed", json!(1296.5)), ("spine", json!("40")), ("spine", json!(true))] {
            let mut p = base.clone();
            p[k] = v.clone();
            assert!(s.execute("file.newCover", &p).is_err(), "{k} = {v}");
        }
        let mut lulu = json!({"printer": "lulu", "pages": 224, "trimWidth": 400, "trimHeight": 600, "spine": "40"});
        assert!(s.execute("file.newCover", &lulu).is_err(), "a typed spine that is not a number is not replaced by Lulu's rule");
        lulu["spine"] = serde_json::Value::Null;
        assert!(s.execute("file.newCover", &lulu).is_ok(), "a null spine means: use the printer's rule");
        let mut ok = base.clone();
        ok["safe"] = json!(0);
        ok["bleed"] = json!(0);
        assert!(s.execute("file.newCover", &ok).is_ok(), "zero is allowed");
    }

    #[test]
    fn saved_presets_make_documents_by_name() {
        let mut s = Session::new();
        let newsletter = json!({"name": "Newsletter", "units": "millimeters", "width": 595.2755905511812, "height": 841.8897637795277,
            "pages": 8, "facingPages": false, "columns": 3, "margins": {"top": 20, "bottom": 30, "inside": 40, "outside": 50},
            "bleed": [9, 9, 0, 9], "primaryTextFrame": true});
        assert_eq!(s.execute("file.savePreset", &newsletter).unwrap()["name"], "Newsletter");
        let listed = s.execute("file.presets", &json!({})).unwrap();
        let saved: Vec<_> = listed.as_array().unwrap().iter().filter(|p| p["saved"] == true).collect();
        assert_eq!(saved.len(), 1);
        assert_eq!((&saved[0]["name"], &saved[0]["units"]), (&json!("Newsletter"), &json!("millimeters")));
        // By name, case ignored; parameters still override the preset.
        s.execute("file.new", &json!({"preset": "newsletter", "pages": 4})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.page_count(), 4);
        assert!(!d.settings.facing_pages && d.settings.primary_text_frame);
        assert_eq!(d.settings.horizontal_units, designcraft_geom::Unit::Millimeters);
        assert_eq!(d.settings.bleed, [9.0, 9.0, 0.0, 9.0]);
        assert_eq!(d.page(0).unwrap().margins.outside, 50.0);
        assert!(d.title.starts_with("Untitled-"), "the preset's name isn't the document's");
        // The same name replaces it.
        s.execute("file.savePreset", &json!({"name": "NEWSLETTER", "pages": 2})).unwrap();
        assert_eq!(s.prefs.document_presets.len(), 1);
        s.execute("file.new", &json!({"preset": "Newsletter"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 2);
        // Built-in names, empty and overlong names and impossible sizes are refused.
        for bad in [json!({"name": "a4"}), json!({"name": "  "}), json!({}), json!({"name": "x".repeat(101)}), json!({"name": "Huge", "width": 1e9})]
        {
            assert!(s.execute("file.savePreset", &bad).is_err(), "{bad}");
        }
        s.execute("file.deletePreset", &json!({"name": "newsletter"})).unwrap();
        assert!(s.execute("file.deletePreset", &json!({"name": "newsletter"})).is_err());
        assert!(s.execute("file.new", &json!({"preset": "Newsletter"})).is_err());
    }

    #[test]
    fn new_document_takes_intent_units_start_page_bleed_and_slug() {
        let mut s = Session::new();
        let p = json!({"preset": "A4", "intent": "web", "units": "pixels", "startPage": 4, "bleed": [9, 0, 3, 6], "slug": 18});
        s.execute("file.new", &p).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.settings.intent, designcraft_doc::Intent::Web);
        assert_eq!(d.settings.horizontal_units, designcraft_geom::Unit::Pixels);
        assert_eq!(d.settings.bleed, [9.0, 0.0, 3.0, 6.0]);
        assert_eq!(d.settings.slug, [18.0; 4]);
        assert_eq!(d.page_name(0), "4");
        // Malformed edges are ignored, hostile ones bounded.
        s.execute("file.new", &json!({"bleed": [1, 2], "slug": [-5, 1e12, 0, 0]})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.settings.bleed, [0.0; 4]);
        assert_eq!(d.settings.slug, [0.0, 1296.0, 0.0, 0.0]);
    }

    #[test]
    fn a_new_document_preview_replaces_itself_and_leaves_the_session_as_it_was() {
        let dir = std::env::temp_dir().join(format!("dc-new-doc-preview-{}", std::process::id()));
        let mut s = Session::new();
        s.recovery_dir = Some(dir.clone());
        s.execute("file.new", &json!({})).unwrap();
        let first = s.doc().unwrap().uid;
        let a = s.preview_new_document(&json!({"width": 300})).unwrap();
        assert_eq!((s.doc().unwrap().uid, s.doc().unwrap().doc.title.as_str()), (a, "Untitled-2"));
        let b = s.preview_new_document(&json!({"width": 400, "columns": 3})).unwrap();
        assert_eq!(s.documents().len(), 2, "the preview replaces itself");
        assert!(s.preview_new_document(&json!({"width": 0})).is_err());
        let d = s.doc().unwrap();
        assert_eq!((d.uid, d.doc.settings.page_width, d.doc.page(0).unwrap().columns.count), (b, 400.0, 3), "a refused size keeps it");
        let listed = s.execute("document.list", &json!({})).unwrap();
        assert_eq!((listed[0].get("preview"), &listed[1]["preview"]), (None, &json!(true)));
        assert_eq!(s.end_new_document_preview(), Some(b));
        let d = s.doc().unwrap();
        assert_eq!((s.documents().len(), d.uid, d.is_dirty(), d.history.undo.len()), (1, first, false, 0));
        assert_eq!(s.end_new_document_preview(), None, "without a preview it does nothing");
        // Edited by a script while shown, it writes no recovery data while it is a preview, and
        // stays as a document when the window closes, with its Untitled number.
        let c = s.preview_new_document(&json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
        assert_eq!(crate::recovery::save(&s, &dir).unwrap(), 0);
        // The app's timer saves through the command.
        assert_eq!(s.execute("file.recovery.save", &json!({})).unwrap()["saved"], 0);
        assert!(crate::recovery::list(&dir).is_empty());
        assert_eq!(s.end_new_document_preview(), None);
        assert_eq!((s.documents().len(), s.doc().unwrap().uid, s.doc().unwrap().doc.title.as_str()), (2, c, "Untitled-2"));
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.title, "Untitled-3");
        // Create with the previewed settings keeps the preview as the document.
        let d = s.preview_new_document(&json!({"pages": 2})).unwrap();
        assert_eq!(s.keep_new_document_preview(), Some(3));
        let st = s.doc().unwrap();
        assert_eq!((st.uid, st.doc.title.as_str(), s.new_document_preview()), (d, "Untitled-4", None));
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.title, "Untitled-5");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod hyphenation_default_tests {
    use std::sync::Arc;

    use designcraft_doc::BASIC_PARAGRAPH;
    use designcraft_doc::build::NewDocument;
    use serde_json::json;

    use crate::Session;

    fn hyph_flags(d: &designcraft_doc::Document) -> (bool, bool, bool) {
        let (pp, _) = d.styles.resolve_para(&designcraft_doc::ParaFormat::default());
        (pp.hyph_capitalized, pp.hyph_last_word, pp.hyph_across_column)
    }

    #[test]
    fn new_documents_hyphenate_no_capitalized_last_or_column_end_words_and_imports_keep_theirs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(hyph_flags(&s.doc().unwrap().doc), (false, false, false));

        let mut d = designcraft_doc::Document::new(&NewDocument::default());
        let st = Arc::make_mut(&mut d.styles).para_mut(BASIC_PARAGRAPH).unwrap();
        st.para.hyph_capitalized = Some(true);
        st.para.hyph_last_word = Some(true);
        st.para.hyph_across_column = Some(true);
        let b64 = super::base64_encode(&designcraft_idml::export_idml(&d));
        s.execute("file.openIdml", &json!({"base64": b64, "name": "in.idml"})).unwrap();
        assert_eq!(hyph_flags(&s.doc().unwrap().doc), (true, true, true));
    }
}
