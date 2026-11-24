//! Modal dialogs. Each dialog keeps its fields in a JSON map so the control channel can set them
//! (`ui.dialog.set {field, value}`) and confirm them (`ui.dialog.confirm`) like a user would.

use designcraft_doc::SpreadRef;
use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::{Bound, with_path};

pub(crate) mod char_sections;
pub(crate) mod document_forms;
pub(crate) mod export_forms;
pub(crate) mod find_forms;
pub(crate) mod frame_forms;
pub(crate) mod import_forms;
mod cell_options_form;
mod color_forms;
pub(crate) mod framework;
mod layout_forms;
mod special_forms;
pub(crate) mod paragraph_forms;
mod command_form;
mod preferences;
mod table_forms;
pub(crate) mod text_forms;
#[cfg(test)]
pub(crate) mod table_layout_kit;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_find_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_dialog_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_pending_tests;
mod options;
mod style_cjk;
pub use options::{open_color_group_options, open_layer_options, open_swatch_options};

#[derive(Clone, Debug, Serialize)]
pub struct Dialog {
    pub id: String,
    pub fields: Map<String, Value>,
    /// The document's ruler unit: what a bare number such as `12` means in a measurement field.
    /// Set from the active document when the dialog is shown or confirmed (picas without one).
    #[serde(skip)]
    pub unit: Unit,
    /// The vertical ruler unit, for a dialog with a height field (the Rectangle dialog): kept
    /// from when it opened, so a bare `50` means what it did then. `None` falls back to `unit`.
    #[serde(skip)]
    pub vertical_unit: Option<Unit>,
    /// The document contents the dialog opened with (Diagonal Lines), held while it is open: an
    /// edit replaces them, so a dialog whose document holds other contents knows it was edited.
    #[serde(skip)]
    pub contents: Option<std::sync::Arc<designcraft_doc::Document>>,
}

impl Dialog {
    pub fn new(id: &str, params: Value) -> Self {
        let mut fields = match params {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let defaults = match id {
            "newDocument" => crate::new_document::defaults(),
            "contactSheet" => json!({"columns": 3, "rows": 4, "gutter": "6pt", "margin": "36pt"}),
            "goToPage" => json!({"page": "1"}),
            "epubExport" => json!({"split": false, "mode": "style", "style": designcraft_doc::BASIC_PARAGRAPH, "coverImage": ""}),
            "qrCode" => json!({"type": "url", "content": "https://", "color": "[Black]"}),
            "insertTable" => json!({"bodyRows": 4, "columns": 4, "headerRows": 0, "footerRows": 0}),
            "convertTextToTable" => json!({"columnSeparator": "tab", "rowSeparator": "paragraph", "columns": 1}),
            "convertTableToText" => json!({"columnSeparator": "tab", "rowSeparator": "paragraph"}),
            "insertXref" => json!({"linkTo": "paragraph", "style": "", "target": "", "format": ""}),
            "findChange" => json!({"find": "", "change": "", "grep": false, "caseSensitive": false, "wholeWord": false, "scope": "document"}),
            "documentSetup" => json!({}),
            "pdfExport" => {
                json!({
                    "preset": "Desktop Printing",
                    "standard": "none",
                    "compressImages": false,
                    "flatten": "",
                    "spreads": false,
                    "bleed": true,
                    "marksCrop": false,
                    "marksBleed": false,
                    "marksPageInfo": false,
                    "marksWeight": "0.25",
                    "marksOffset": "6",
                    "tagged": true
                })
            }
            _ => json!({}),
        };
        if let Value::Object(d) = defaults {
            for (k, v) in d {
                fields.entry(k).or_insert(v);
            }
        }
        if id == "footnoteOptions" {
            // Flatten the rule and show special characters as InDesign metacharacters.
            if let Some(Value::Object(r)) = fields.remove("rule") {
                for (k, v) in r {
                    fields.insert(format!("rule.{k}"), v);
                }
            }
            let sep = fields.get("separator").and_then(Value::as_str).unwrap_or("").to_string();
            fields.insert("separator".into(), json!(sep.replace('\t', "^t").replace('\u{2003}', "^m").replace('\u{2002}', "^>")));
            fields.entry("tab".to_string()).or_insert(json!("numbering"));
        }
        if id == "paragraphStyleOptions" || id == "characterStyleOptions" {
            fields.entry("section".to_string()).or_insert(json!("general"));
        }
        if id == "ruby" {
            fields.entry("section".to_string()).or_insert(json!("cjk.rubyPlacement"));
        }
        Dialog { id: id.into(), fields, unit: Unit::Picas, vertical_unit: None, contents: None }
    }
    /// Set a field and report a genuine Find domain/mode transition to the caller.
    pub(crate) fn set_field(&mut self, key: &str, value: Value) -> bool {
        let before =
            (self.id == "findChange" && matches!(key, "scope" | "story" | "objectMode")).then(|| (self.b("objectMode"), find_change_params(self)));
        self.fields.insert(key.into(), value);
        before.is_some_and(|(object_mode, params)| {
            object_mode != self.b("objectMode") || !designcraft_engine::cmd::same_find_domain(&params, &find_change_params(self))
        })
    }

    pub(crate) fn s(&self, k: &str) -> String {
        match self.fields.get(k) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }
    pub(crate) fn m(&self, k: &str) -> Option<f64> {
        self.m_in(k, self.unit)
    }
    /// A measure where a bare number is in `unit`.
    fn m_in(&self, k: &str, unit: Unit) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => self.exact(k, s, unit).or_else(|| parse_measure(s, unit).ok()),
            _ => None,
        }
    }
    /// A measure in points ("6", "6 pt", "0p6", "2 mm").
    fn pt(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => self.exact(k, s, self.unit).or_else(|| parse_measure(s, Unit::Points).ok()),
            _ => None,
        }
    }
    /// The points a measure field was committed as, while its text is still what was shown for them: the text is rounded for
    /// display, so reading the points back from it would lose what the field was given ("3 cm" in picas).
    fn exact(&self, k: &str, text: &str, unit: Unit) -> Option<f64> {
        let points = self.fields.get(&format!("_pt.{k}"))?.as_f64()?;
        (designcraft_geom::format_measure(points, unit) == text).then_some(points)
    }
    /// Commits the measure `points` to the field `k`: its text in the document's unit, and the exact points beside it.
    pub(crate) fn set_measure(&mut self, k: &str, points: f64) -> String {
        let shown = designcraft_geom::format_measure(points, self.unit);
        self.fields.insert(k.into(), json!(shown));
        self.fields.insert(format!("_pt.{k}"), json!(points));
        shown
    }
    pub(crate) fn b(&self, k: &str) -> bool {
        self.fields.get(k).and_then(Value::as_bool).unwrap_or(false)
    }
    pub(crate) fn n(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

/// Insert Pages…, after page `abs` and with that page's parent, as InDesign's.
pub(crate) fn open_insert_pages(app: &mut DesignApp, abs: usize) {
    let Some(st) = app.session.active() else { return };
    let abs = abs.min(st.doc.page_count().saturating_sub(1));
    let parent = st.doc.page(abs).and_then(|p| p.parent).map_or_else(String::new, |id| id.0.to_string());
    app.ui.dialog = Some(Dialog::new("insertPages", json!({"count": 1, "insert": "after", "page": abs, "parent": parent})));
}

/// Separators the Convert dialogs offer by name; anything else typed in the field is the
/// separator's own characters.
const SEPARATORS: [(&str, &str); 3] = [("tab", "Tab"), ("comma", "Comma"), ("paragraph", "Paragraph")];

/// The characters a separator field stands for (`table.convertFromText` resolves it the same
/// way), so "comma" and a typed "," are the same separator.
fn resolved_separator(v: &str) -> &str {
    match v {
        "tab" => "\t",
        "comma" => ",",
        "paragraph" => "\n",
        other => other,
    }
}

/// Whether the Convert dialog's two separators are the same (Number of Columns then applies).
fn same_separators(d: &Dialog) -> bool {
    resolved_separator(&d.s("columnSeparator")) == resolved_separator(&d.s("rowSeparator"))
}

/// A count field's value as the commands read it: a whole number is an integer (`as_u64` refuses `2.0`, which is how the
/// spinner stores it), anything else goes through unchanged for the command to refuse. One when unset.
fn whole_number(value: Option<&Value>) -> Value {
    match value {
        None => json!(1),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) if f >= 0.0 && f.fract() == 0.0 && f < u64::MAX as f64 => json!(f as u64),
            _ => Value::Number(n.clone()),
        },
        Some(other) => other.clone(),
    }
}

/// The dialog's chosen table style: none when unset, null or empty (a new table's [Basic Table]).
fn table_style_field(d: &Dialog) -> Option<String> {
    d.fields.get("tableStyle").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The dialog's `tableStyle` as the table commands take it: null for none, the style's name, or
/// a value that is neither (set through `ui.dialog.set`) as it is, for the command to refuse.
fn table_style_value(d: &Dialog) -> Value {
    match d.fields.get("tableStyle") {
        None | Some(Value::String(_)) => table_style_field(d).map_or(Value::Null, Value::String),
        Some(v) => v.clone(),
    }
}

/// Extract once when opened. Cancel is read-only; confirmation adds exactly these colours.
pub(crate) fn open_color_theme(app: &mut DesignApp, params: Value) {
    match app.run("color.extractTheme", params) {
        Ok(mut theme) => {
            theme["name"] = json!("Color Theme");
            app.ui.dialog = Some(Dialog::new("colorTheme", theme));
        }
        Err(message) => app.ui.dialog = Some(Dialog::new("alert", json!({"message": message}))),
    }
}

#[cfg(test)]
mod color_theme_tests {
    use super::*;

    /// A page past the end shows as the last page in the dialog, and Before Page puts the new
    /// page before that one rather than at the end.
    #[test]
    fn insert_pages_before_a_page_past_the_end_goes_before_the_last() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 4, "facingPages": false})).unwrap();
        let ids = |app: &DesignApp| {
            let d = &app.session.doc().unwrap().doc;
            (0..d.page_count()).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>()
        };
        let old = ids(&app);
        app.ui.dialog = Some(Dialog::new("insertPages", json!({"count": 1, "insert": "before", "page": 100, "parent": ""})));
        confirm(&mut app).unwrap();
        let now = ids(&app);
        assert_eq!(now.len(), 5);
        assert_eq!((now[..3].to_vec(), now[4]), (old[..3].to_vec(), old[3]), "the new page is fourth, before the old last page");
    }

    /// The course's chapter 25: Insert Pages as InDesign's. It opens after the page in view with
    /// that page's parent; At Start of Document with B-Cover puts a B page first, and Before Page
    /// with [None] puts a page without a parent before the page chosen.
    #[test]
    fn insert_pages_goes_before_a_page_or_at_the_start_with_the_chosen_parent() {
        use super::table_layout_kit as kit;
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};
        // Facing: [1] [2 3] [4].
        let mut app = kit::app(4, false);
        let b = app.run("layout.parents.new", json!({"prefix": "B", "name": "Cover"})).unwrap()["id"].as_u64().unwrap();
        let ids = |app: &DesignApp| -> Vec<u64> {
            let d = &app.session.doc().unwrap().doc;
            (0..d.page_count()).map(|i| d.page(i).unwrap().id.0).collect()
        };
        let old = ids(&app);
        crate::menus::activate(&mut app, "layout.pages.insert", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "insertPages");
        let mut h = kit::harness_sized(app, egui::vec2(1000.0, 700.0));
        for l in ["Pages:", "Insert:", "Parent:"] {
            assert!(h.query_all_by_label(l).next().is_some(), "{l}");
        }
        // The dropdown named `label`, then its item `choice`.
        let pick = |h: &mut Harness<'_, DesignApp>, label: &str, choice: &str| kit::pick(h, label, choice);
        // After page 1 with A, at first; then At Start of Document, with B-Cover.
        pick(&mut h, "Insert:", "At Start of Document");
        pick(&mut h, "Parent:", "B-Cover");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let now = ids(h.state());
        assert_eq!(now[1..], old[..], "the new page is first");
        let parent = |app: &DesignApp, i: usize| app.session.doc().unwrap().doc.page(i).unwrap().parent.map(|p| p.0);
        assert_eq!(parent(h.state(), 0), Some(b));
        // Before page 4, with [None].
        crate::dialogs::open_insert_pages(h.state_mut(), 0);
        kit::steps(&mut h, 3);
        pick(&mut h, "Insert:", "Before Page");
        pick(&mut h, "Page", "4");
        pick(&mut h, "Parent:", "[None]");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        let then = ids(h.state());
        assert_eq!(then.len(), 6);
        assert_eq!(then[..3], now[..3], "the pages before it stay");
        assert_eq!(then[4..], now[3..], "page 4 and those after it follow the new page");
        assert_eq!(parent(h.state(), 3), None);
    }

    /// The course's chapter 18: Text Frame Options with Preview on shows the columns and the
    /// inset on the page while the dialog is open; Cancel puts the frame back, OK sets them as one
    /// undo step. The Paragraph Spacing Limit is set from the dialog too.
    #[test]
    fn text_frame_options_preview_shows_the_change_until_ok_or_cancel() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        let opts = |app: &DesignApp| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
        let open = |app: &mut DesignApp| {
            let mut d = Dialog::new("textFrameOptions", json!({}));
            seed_text_frame_options(app, &mut d);
            d
        };
        let mut d = open(&mut app);
        d.fields.insert("columns".into(), json!("3"));
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 3, "previewed on the page");
        d.fields.insert("insetTop".into(), json!("1p0"));
        text_frame_preview(&mut app, &mut d);
        assert_eq!((opts(&app).columns, opts(&app).inset[0]), (3, 12.0), "redone when a field changes");
        d.fields.insert("preview".into(), json!(false));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 1, "Preview off: the page as it was");
        // A value OK would refuse: the page goes back while the status says why.
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 3);
        let gutter = d.fields.get("gutter").cloned().unwrap_or_default();
        d.fields.insert("gutter".into(), json!("wide"));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 1, "the page as it was");
        assert!(!d.fields.contains_key(PREVIEWED) && !d.s("status").is_empty());
        d.fields.insert("gutter".into(), gutter);
        d.fields.remove("status");
        // Cancel after a preview: back as it was.
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        app.ui.dialog = Some(d);
        cancel(&mut app);
        assert_eq!(opts(&app).columns, 1);
        // OK after a preview: set once, one undo step.
        let mut d = open(&mut app);
        d.fields.insert("columns".into(), json!("2"));
        d.fields.insert("verticalJustification".into(), json!("justify"));
        d.fields.insert("paragraphSpacingLimit".into(), json!("2p0"));
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        app.ui.dialog = Some(d);
        confirm(&mut app).unwrap();
        let o = opts(&app);
        assert_eq!((o.columns, o.vj_paragraph_spacing_limit), (2, 24.0));
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(opts(&app).columns, 1, "one undo step");
    }

    /// Opened with the caret on the last line of a frame, Text Frame Options keep acting on that
    /// frame while a preview pushes the caret's line into the next frame of the thread.
    #[test]
    fn text_frame_options_keep_their_frame_while_the_preview_reflows_the_caret() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
        let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
        let b = app.run("frame.create", json!({"rect": [36, 200, 300, 700], "content": "text", "text": "", "caret": false})).unwrap();
        let (a, b, sid) =
            (designcraft_doc::ItemId(a["id"].as_u64().unwrap()), designcraft_doc::ItemId(b["id"].as_u64().unwrap()), a["story"].as_u64().unwrap());
        app.run("text.thread", json!({"from": a.0, "to": b.0})).unwrap();
        let shown = |app: &DesignApp, f| {
            let cs = designcraft_compose::compose_story(&app.session.doc().unwrap().doc, designcraft_doc::StoryId(sid), &Default::default());
            cs.frame(f).unwrap().range.clone()
        };
        let caret = shown(&app, a).end - 1;
        app.run("text.select", json!({"story": sid, "anchor": caret})).unwrap();
        let inset = |app: &DesignApp, f| app.session.doc().unwrap().doc.item(f).unwrap().text_frame().unwrap().options.inset[0];
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(&app, &mut d);
        d.fields.insert("inset".into(), json!("2p0"));
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        assert!(shown(&app, b).contains(&caret), "the preview moved the caret's line into the next frame");
        d.fields.insert("columns".into(), json!("2"));
        text_frame_preview(&mut app, &mut d);
        assert_eq!((inset(&app, a), inset(&app, b)), (24.0, 0.0), "the preview stays on the first frame");
        app.ui.dialog = Some(d);
        confirm(&mut app).unwrap();
        assert_eq!((inset(&app, a), inset(&app, b)), (24.0, 0.0));
    }

    /// With the caret in the oval second frame of a thread whose first frame has uneven insets, the
    /// dialog shows the one Inset of the oval and keeps the link on: both follow the frame it changes.
    #[test]
    fn text_frame_options_from_the_type_tool_show_the_inset_of_the_caret_frame() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
        let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
        let b =
            app.run("frame.create", json!({"rect": [36, 200, 300, 700], "shape": "ellipse", "content": "text", "text": "", "caret": false})).unwrap();
        let (a, b, sid) =
            (designcraft_doc::ItemId(a["id"].as_u64().unwrap()), designcraft_doc::ItemId(b["id"].as_u64().unwrap()), a["story"].as_u64().unwrap());
        app.run("text.thread", json!({"from": a.0, "to": b.0})).unwrap();
        app.run("object.textFrameOptions", json!({"ids": [a.0], "inset": [1, 2, 3, 4]})).unwrap();
        let cs = designcraft_compose::compose_story(&app.session.doc().unwrap().doc, designcraft_doc::StoryId(sid), &Default::default());
        let caret = cs.frame(b).unwrap().range.start + 2;
        app.run("text.select", json!({"story": sid, "anchor": caret})).unwrap();
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(&app, &mut d);
        assert_eq!(d.fields.get("_targets"), Some(&json!([b.0])));
        assert!(d.b("_shaped"), "the oval shows one Inset");
        assert!(d.b("insetLinked"), "the first frame's uneven sides do not turn the link off");
    }

    /// A preview shown in one document goes when OK is refused, or Cancel is chosen, after another
    /// document became active.
    #[test]
    fn text_frame_options_preview_goes_from_its_document_after_another_becomes_active() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let make = |app: &mut crate::DesignApp| {
            app.run("file.new", json!({})).unwrap();
            let r = app.run("frame.create", json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Hello"})).unwrap();
            app.run("text.select", json!({"story": r["story"], "anchor": 2})).unwrap();
            designcraft_doc::ItemId(r["id"].as_u64().unwrap())
        };
        let a = make(&mut app);
        let first = |app: &DesignApp| {
            let st = &app.session.documents()[0];
            (st.doc.item(a).unwrap().text_frame().unwrap().options.columns, st.interaction.is_some())
        };
        let preview = |app: &mut DesignApp| {
            let mut d = Dialog::new("textFrameOptions", json!({}));
            seed_text_frame_options(app, &mut d);
            d.fields.insert("columns".into(), json!("3"));
            d.fields.insert("preview".into(), json!(true));
            text_frame_preview(app, &mut d);
            assert_eq!(first(app), (3, true), "previewed in the first document");
            d
        };
        let d = preview(&mut app);
        make(&mut app);
        app.ui.dialog = Some(d);
        let err = confirm(&mut app).unwrap_err().to_string();
        assert!(err.contains("no longer the active one"), "{err}");
        assert_eq!(first(&app), (1, false), "OK refused: the first document is as it was");
        app.ui.dialog = None;
        app.session.set_active(0);
        let d = preview(&mut app);
        app.session.set_active(1);
        app.ui.dialog = Some(d);
        cancel(&mut app);
        assert_eq!(first(&app), (1, false), "Cancel: the first document is as it was");
    }

    /// Undo puts back the document's id counter, so a frame made after Undo can take the id of a
    /// frame the dialog stored; OK is refused instead of changing the new frame.
    #[test]
    fn text_frame_options_from_the_type_tool_are_refused_after_undo_reuses_the_frame_id() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": "First"})).unwrap();
        app.run("text.select", json!({"story": a["story"], "anchor": 1})).unwrap();
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(&app, &mut d);
        d.fields.insert("columns".into(), json!("3"));
        app.run("edit.undo", json!({})).unwrap();
        let b = app.run("frame.create", json!({"rect": [36, 200, 300, 300], "content": "text", "text": "Second"})).unwrap();
        assert_eq!(a["id"], b["id"], "the new frame takes the undone frame's id");
        app.run("text.select", json!({"story": b["story"], "anchor": 1})).unwrap();
        app.ui.dialog = Some(d);
        let err = confirm(&mut app).unwrap_err().to_string();
        assert!(err.contains("changed after the dialog opened"), "{err}");
        let b = designcraft_doc::ItemId(b["id"].as_u64().unwrap());
        assert_eq!(app.session.doc().unwrap().doc.item(b).unwrap().text_frame().unwrap().options.columns, 1, "the new frame is unchanged");
    }

    #[test]
    fn toc_right_indent_separator_survives_the_dialog_and_update() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.newSample", json!({})).unwrap();
        open_toc(&mut app);
        app.ui.dialog.as_mut().unwrap().fields.insert(
            "included".into(),
            json!([
                {"style": "Headline", "level": 1, "separator": " ^y "}
            ]),
        );
        confirm(&mut app).unwrap();
        if app.session.tool_id() == "tocPlace" {
            app.run("toc.place", json!({"x":36,"y":36})).unwrap();
        }
        let separator = format!(" {} ", designcraft_doc::story::RIGHT_INDENT_TAB);
        let d = &app.session.doc().unwrap().doc;
        let toc = d.toc.as_ref().unwrap();
        assert_eq!(toc.entries[0].separator.as_deref(), Some(separator.as_str()));
        assert!(d.story(toc.story).unwrap().text.contains(&separator));
        app.run("toc.update", json!({})).unwrap();
        open_toc(&mut app);
        assert_eq!(toc_rows(app.ui.dialog.as_ref().unwrap())[0].separator, " ^y ");
        for text in ["^y", "^t^y", "^m^>", "unknown ^q and trailing ^", "a\t\u{E006}\u{2003}\u{2002}z"] {
            assert_eq!(toc_separator_typed(&toc_separator_shown(text)), text);
        }
        // The picker after a literal caret: the caret stays, and the code is the code.
        for (field, typed) in [("^", "^\u{E006}"), ("^^", "^\u{E006}"), ("a", "a\u{E006}"), ("^^^", "^^\u{E006}")] {
            let mut f = field.to_string();
            toc_separator_append(&mut f, "^y");
            assert_eq!(toc_separator_typed(&f), typed, "{field:?} → {f:?}");
        }
    }

    #[test]
    fn color_theme_cancel_is_read_only_and_confirm_adds_the_preview() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let before = app.session.active().unwrap().doc.clone();
        let theme = json!({"name":"Fashion", "colors":["#102030", "#f0a020"]});
        app.ui.dialog = Some(Dialog::new("colorTheme", theme.clone()));
        cancel(&mut app);
        assert_eq!(app.session.active().unwrap().doc, before);
        app.ui.dialog = Some(Dialog::new("colorTheme", theme));
        confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.color_groups.last().unwrap().name, "Fashion");
        assert_eq!(doc.color_groups.last().unwrap().swatches.len(), 2);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.active().unwrap().doc, before);
        app.ui.dialog = Some(Dialog::new("colorTheme", json!({"name":" ","colors":["#102030"]})));
        assert!(confirm(&mut app).is_err());
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "colorTheme");
        assert_eq!(app.session.active().unwrap().doc, before);
    }
}

/// File › Print, with the printers the system knows.
pub fn open_print(app: &mut DesignApp) {
    let p = app.session.execute("file.printers", &json!({})).unwrap_or_default();
    let printer = p["default"].as_str().map(str::to_string).or_else(|| p["printers"][0].as_str().map(str::to_string)).unwrap_or_default();
    app.ui.dialog = Some(Dialog::new(
        "print",
        json!({"printers": p["printers"], "printer": printer, "copies": 1, "range": "all", "pages": "1", "spreads": false, "marks": false, "bleed": false}),
    ));
}

/// File › Export PDF…: the "Export PDF" options dialog, seeded from the persisted settings
/// (`UiState.pdf_export`). `pages` is per-job (not persisted); `flatten` falls back to the global
/// flattener preset when the persisted value is empty. `title` starts empty: a title given to the
/// dialog is the PDF's title and names the file, and without one the file is named after the
/// document and the PDF takes its title from File Info.
pub fn open_pdf_export(app: &mut DesignApp) {
    let mut fields = serde_json::to_value(&app.ui.pdf_export).unwrap_or_else(|_| json!({}));
    if let Value::Object(m) = &mut fields {
        m.insert("pages".into(), json!("all"));
        if app.ui.pdf_export.flatten.is_empty() {
            m.insert("flatten".into(), json!(app.ui.flattener));
        }
    }
    app.ui.dialog = Some(Dialog::new("pdfExport", fields));
}

/// File › Export EPUB…: the EPUB Export Options dialog, seeded from the document's Split
/// Document setting.
pub fn open_epub_export(app: &mut DesignApp) {
    use designcraft_doc::EpubSplit;
    let split = app.session.active().map(|st| st.doc.settings.epub_split.clone()).unwrap_or_default();
    let fields = match split {
        EpubSplit::Off => json!({}),
        EpubSplit::Style(name) => json!({"split": true, "mode": "style", "style": name}),
        EpubSplit::ExportTags => json!({"split": true, "mode": "exportTags"}),
    };
    app.ui.dialog = Some(Dialog::new("epubExport", fields));
}

/// The Split Document setting the EPUB Export Options dialog's fields describe.
fn epub_export_split(d: &Dialog) -> designcraft_doc::EpubSplit {
    use designcraft_doc::EpubSplit;
    if !d.b("split") {
        EpubSplit::Off
    } else if d.s("mode") == "exportTags" {
        EpubSplit::ExportTags
    } else {
        EpubSplit::Style(d.s("style"))
    }
}

/// EPUB Export Options › Choose…: the image file picked goes into the dialog's Cover Image field
/// while it is open.
fn set_epub_cover(app: &mut DesignApp, path: String) -> Result<Value, String> {
    if let Some(d) = app.ui.dialog.as_mut().filter(|d| d.id == "epubExport") {
        d.fields.insert("coverImage".into(), json!(path));
    }
    Ok(Value::Null)
}

/// The presets offered by the Export PDF dialog, in display order. The stored value is the
/// English name; the caption is translated.
const PDF_EXPORT_PRESETS: &[&str] =
    &["Desktop Printing", "Commercial Printing", "Screen and Email", "PDF/X-1a:2003", "PDF/X-4", "PDF/A-2b", "Custom"];

/// The `file.exportPdf` options the Export PDF dialog's fields stand for (pages and title aside).
fn pdf_export_params(d: &Dialog) -> Value {
    let mut params = json!({
        "standard": d.s("standard"),
        "compressImages": d.b("compressImages"),
        "spreads": d.b("spreads"),
        "bleed": d.b("bleed"),
        "tagged": d.b("tagged"),
        "marks": {
            "crop": d.b("marksCrop"),
            "bleed": d.b("marksBleed"),
            "pageInfo": d.b("marksPageInfo"),
            "weight": d.pt("marksWeight").unwrap_or(0.25),
            "offset": d.pt("marksOffset").unwrap_or(6.0),
        },
    });
    let flatten = d.s("flatten");
    if !flatten.is_empty() {
        params["flatten"] = json!(flatten);
    }
    params
}

/// The PDF presets a document can be exported with, by name (Custom aside).
pub(crate) fn pdf_presets() -> impl Iterator<Item = &'static str> {
    PDF_EXPORT_PRESETS.iter().copied().filter(|p| *p != "Custom")
}

/// The `file.exportPdf` options of PDF preset `preset`, as the Export PDF dialog sets them.
pub(crate) fn pdf_preset_params(preset: &str) -> Value {
    let mut d = Dialog::new("pdfExport", json!({}));
    apply_pdf_preset(&mut d, preset);
    pdf_export_params(&d)
}

/// Apply a preset's dependent fields. Flatten is only touched by "Screen and Email" and
/// "PDF/X-1a:2003" (High Resolution); "Custom" (and any unknown value) changes nothing.
fn apply_pdf_preset(d: &mut Dialog, preset: &str) {
    let (standard, compress, crop, bleed, page_info, flatten) = match preset {
        "Desktop Printing" => ("none", false, false, false, false, None),
        "Commercial Printing" => ("none", true, true, true, false, None),
        "Screen and Email" => ("none", true, false, false, false, Some("low")),
        "PDF/X-1a:2003" => ("x1a", false, true, true, false, Some("high")),
        "PDF/X-4" => ("x4", false, true, true, false, None),
        "PDF/A-2b" => ("a2b", false, false, false, false, None),
        _ => return,
    };
    d.fields.insert("standard".into(), json!(standard));
    d.fields.insert("compressImages".into(), json!(compress));
    d.fields.insert("marksCrop".into(), json!(crop));
    d.fields.insert("marksBleed".into(), json!(bleed));
    d.fields.insert("marksPageInfo".into(), json!(page_info));
    if let Some(f) = flatten {
        d.fields.insert("flatten".into(), json!(f));
    }
}

/// The first frame Text Frame Options changes: the first selected item, the frame that shows the
/// caret, or the first frame holding selected text.
fn options_frame(app: &DesignApp) -> Option<designcraft_doc::ItemId> {
    designcraft_engine::cmd::text_frame_options_frames(&app.session).first().copied()
}

/// The selected text frame's width (its own, before rotation).
fn selected_frame_width(app: &DesignApp) -> Option<f64> {
    let st = app.session.active()?;
    Some(st.doc.item(options_frame(app)?)?.inner_bounds().width())
}

/// The options of the frame Text Frame Options shows.
fn selected_frame_options(app: &DesignApp) -> Option<designcraft_doc::TextFrameOptions> {
    let st = app.session.active()?;
    st.doc.item(options_frame(app)?)?.text_frame().map(|t| t.options.clone())
}

/// True when every text frame the dialog changes has the same inset on all four sides: the
/// selected items, else the frame that shows the caret or every frame holding selected text.
fn selected_frames_have_even_insets(app: &DesignApp) -> bool {
    let Some(st) = app.session.active() else {
        return true;
    };
    let ids = designcraft_engine::cmd::text_frame_options_frames(&app.session);
    ids.iter().filter_map(|id| st.doc.item(*id)?.text_frame()).all(|t| t.options.inset.iter().all(|v| (v - t.options.inset[0]).abs() < 1e-9))
}

/// Whether every text frame the dialog changes (the selected items, else the frame that shows the
/// caret or every frame holding selected text) has an outline that is not its own axis-aligned rectangle: an oval, a polygon, rounded or other corner
/// options. Its text keeps one inset from the outline, as compose sets it; type on a path doesn't.
/// A selection that mixes them with rectangular frames shows the four sides, so each frame keeps
/// the sides left as shown.
fn selected_frame_is_shaped(app: &DesignApp) -> bool {
    let Some(st) = app.session.active() else { return false };
    let shaped = |fid: designcraft_doc::ItemId| -> Option<bool> {
        let item = st.doc.item(fid)?;
        if item.text_frame()?.options.path.is_some() {
            return Some(false);
        }
        let bp = if item.corners.is_none() { item.path.to_bezpath() } else { designcraft_geom::corners::apply(&item.path, &item.corners) };
        Some(designcraft_compose::FrameOutline::new(&bp, 0.0).is_some())
    };
    let ids = designcraft_engine::cmd::text_frame_options_frames(&app.session);
    !ids.is_empty() && ids.iter().all(|&fid| shaped(fid).unwrap_or(false))
}

/// Object › Text Frame Options shows the selected frame's options. `_shown` keeps what it showed,
/// so OK sends only the fields the user changed. Fields given when the dialog opened count as
/// changed.
fn seed_text_frame_options(app: &DesignApp, d: &mut Dialog) {
    if d.fields.contains_key("_shown") {
        return;
    }
    let o = selected_frame_options(app).unwrap_or_default();
    let units = doc_unit(app);
    // The spinner fields show their measures in it and a bare number typed is in it (#2739):
    // kept from when the dialog opened, as the Rectangle dialog keeps its units.
    d.unit = units;
    let mut shown = text_frame_fields(&o, units, selected_frame_width(app), &document_grid(app));
    // A non-rectangular frame shows one Inset (its top inset, the one kept from the outline).
    let shaped = selected_frame_is_shaped(app);
    if shaped && let Value::Object(m) = &mut shown {
        m.insert("inset".into(), json!(format_measure(o.inset[0], units)));
    }
    d.fields.insert("_shaped".into(), json!(shaped));
    if let Some(targets) = typing_frame_ids(app) {
        d.fields.entry("_targets").or_insert(targets);
        if let Some(epoch) = app.session.active().map(|doc| doc.identity_epoch) {
            d.fields.entry("_targetsDoc").or_insert(json!(epoch));
        }
    }
    if let Value::Object(m) = &shown {
        for (k, v) in m {
            d.fields.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    // Linked only when no selected frame has uneven sides, so a typed side cannot overwrite them.
    d.fields.entry("insetLinked".to_string()).or_insert_with(|| json!(selected_frames_have_even_insets(app)));
    // The frame's width: Fixed Number shows each column's width from it as the fields change.
    if let Some(w) = selected_frame_width(app) {
        d.fields.insert("_frameWidth".into(), json!(w));
    }
    // Typing in a frame: the frames taken when the dialog opens, so a preview that reflows the
    // text into another frame does not change which frames the options apply to.
    if let Some(ids) = typing_frame_ids(app) {
        d.fields.insert("_targets".into(), ids);
        // The document state those frames are in: another document, or the document after
        // Revert, can reuse their ids.
        d.fields.insert("_targetsDoc".into(), json!(app.session.active().map(|doc| doc.identity_epoch)));
        // Undo puts back the document's id counter, so a frame made after it can take a stored id.
        d.fields.insert("_targetsRevision".into(), json!(app.session.active().map(base_revision)));
    }
    d.fields.insert("_shown".into(), shown);
}

/// The active document's baseline grid (the default one without a document).
pub(crate) fn document_grid(app: &DesignApp) -> designcraft_doc::BaselineGrid {
    app.session.active().map(|s| s.doc.settings.baseline_grid.clone()).unwrap_or_default()
}

/// Text frame options `o` as the Text Frame Options dialog's fields, measures in `units`; with the
/// frame's width, a Fixed Number column shows the width it gets. Without a custom baseline grid of
/// its own, the frame shows `doc_grid`'s start and increment.
pub(crate) fn text_frame_fields(
    o: &designcraft_doc::TextFrameOptions,
    units: Unit,
    frame_width: Option<f64>,
    doc_grid: &designcraft_doc::BaselineGrid,
) -> Value {
    let fm = |v: f64| json!(format_measure(v, units));
    // Fixed Number: the width each column gets in the frame (shown, as in InDesign).
    let inner = frame_width.map(|w| w - o.inset[1] - o.inset[3]).unwrap_or(0.0);
    let n = o.columns.max(1) as f64;
    let width = match o.columns_kind {
        designcraft_doc::ColumnsKind::FixedWidth if o.column_width > 0.0 => o.column_width,
        _ => ((inner - o.gutter * (n - 1.0)) / n).max(0.0),
    };
    let kind = |v: Value| v.as_str().unwrap_or_default().to_string();
    // A frame without a custom baseline grid shows the document grid's start and increment.
    let grid = o.baseline_grid.unwrap_or_else(|| {
        let g = doc_grid;
        let relative_to = match g.relative_to {
            designcraft_doc::GridRelative::TopOfPage => designcraft_doc::FrameGridRelative::TopOfPage,
            designcraft_doc::GridRelative::TopMargin => designcraft_doc::FrameGridRelative::TopMargin,
        };
        designcraft_doc::FrameBaselineGrid { start: g.start, increment: g.increment, relative_to }
    });
    json!({
        "columns": o.columns, "gutter": fm(o.gutter),
        "columnsKind": kind(serde_json::to_value(o.columns_kind).unwrap_or_default()), "columnWidth": fm(width),
        "balanceColumns": o.balance_columns, "ignoreWrap": o.ignore_wrap,
        "firstBaseline": kind(serde_json::to_value(o.first_baseline).unwrap_or_default()), "firstBaselineMin": fm(o.first_baseline_min),
        "insetTop": fm(o.inset[0]), "insetLeft": fm(o.inset[1]), "insetBottom": fm(o.inset[2]), "insetRight": fm(o.inset[3]),
        "verticalJustification": serde_json::to_value(o.vertical_justification).unwrap_or(Value::Null),
        "paragraphSpacingLimit": fm(o.vj_paragraph_spacing_limit),
        "columnRule": o.column_rule, "columnRuleWeight": format_measure(o.column_rule_weight, Unit::Points), "columnRuleColor": o.column_rule_color,
        "columnRuleTint": percent(o.column_rule_tint), "columnRuleTopInset": fm(o.column_rule_top_inset),
        "columnRuleBottomInset": fm(o.column_rule_bottom_inset), "columnRuleOffset": fm(o.column_rule_offset),
        "autoSize": serde_json::to_value(o.auto_size).unwrap_or(Value::Null),
        "autoSizeReference": designcraft_engine::autosize::REFERENCES.get(usize::from(o.auto_size_ref)).copied().unwrap_or("topCenter"),
        "useMinimumHeight": o.auto_size_use_min_height, "minimumHeight": fm(o.auto_size_min_height),
        "useMinimumWidth": o.auto_size_use_min_width, "minimumWidth": fm(o.auto_size_min_width),
        "noLineBreaks": o.auto_size_no_line_breaks,
        "customGrid": o.baseline_grid.is_some(), "gridStart": fm(grid.start), "gridIncrement": fm(grid.increment),
        "gridRelativeTo": grid.relative_to.name(),
    })
}

/// The document's revision without a preview on top: the one the open gesture began at.
fn base_revision(st: &designcraft_engine::DocState) -> u64 {
    st.interaction.as_ref().map_or(st.revision, |it| it.revision)
}

/// With the Type tool and no frame selected: the frame that shows the caret, or every frame that
/// holds part of the selected text.
fn typing_frame_ids(app: &DesignApp) -> Option<Value> {
    let st = app.session.active()?;
    if !st.selection.items.is_empty() || st.selection.text.is_none() {
        return None;
    }
    let ids: Vec<u64> = designcraft_engine::cmd::text_frame_options_frames(&app.session).iter().map(|f| f.0).collect();
    Some(json!(ids))
}

/// A 0..1 tint as a percentage ("40", "12.5").
fn percent(t: f32) -> String {
    let s = format!("{:.2}", f64::from(t) * 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Preview on: the options show on the page (redone when a field changes) without an undo step;
/// off, the page goes back.
fn text_frame_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            end_text_frame_preview(app, d);
            d.fields.remove("status");
        }
        return;
    }
    let params = match text_frame_params(app, d) {
        Ok(p) => Value::Object(p),
        Err(e) => {
            // Values OK would refuse don't stay on the page: it goes back to how it was.
            if shown.is_some() {
                end_text_frame_preview(app, d);
            }
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    // Nothing edited (yet, or any more): the page as it was.
    if params.as_object().is_some_and(|m| m.keys().all(|k| k == "ids")) {
        if shown.is_some() {
            end_text_frame_preview(app, d);
        }
        return;
    }
    if shown.is_none() {
        if let Err(e) = app.session.begin_interaction("Text Frame Options") {
            d.fields.insert("status".into(), json!(e.to_string()));
            return;
        }
        d.fields.insert(PREVIEW_DOC.into(), json!(app.session.active().map(|doc| doc.uid)));
    }
    match app.session.preview("object.textFrameOptions", params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// The preview goes from the document it was shown in, also when another document is active now.
fn end_text_frame_preview(app: &mut DesignApp, d: &mut Dialog) {
    d.fields.remove(PREVIEWED);
    match d.fields.remove(PREVIEW_DOC).as_ref().and_then(Value::as_u64) {
        Some(uid) => app.session.cancel_interaction_of(uid),
        None => app.session.cancel_interaction(),
    }
}

/// Whether the user changed `key` from what the dialog showed.
fn edited(d: &Dialog, key: &str) -> bool {
    let shown = match d.fields.get("_shown").and_then(|s| s.get(key)) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => return d.fields.contains_key(key),
    };
    d.fields.contains_key(key) && d.s(key).trim() != shown.trim()
}

/// The custom baseline grid of Text Frame Options: `None` if nothing about it was edited, else off
/// (null) or the fields the user changed.
fn text_frame_baseline_grid(d: &Dialog, bad: &dyn Fn(&str) -> String) -> Result<Option<Value>, String> {
    let grid_keys = ["customGrid", "gridStart", "gridIncrement", "gridRelativeTo"];
    if !grid_keys.iter().any(|k| edited(d, k)) {
        return Ok(None);
    }
    if !d.b("customGrid") {
        return Ok(Some(Value::Null));
    }
    let all = edited(d, "customGrid");
    let mut g = Map::new();
    if all || edited(d, "gridStart") {
        g.insert("start".into(), json!(d.m("gridStart").ok_or_else(|| bad("the baseline grid start is not a measure"))?));
    }
    if all || edited(d, "gridIncrement") {
        g.insert(
            "increment".into(),
            json!(d.m("gridIncrement").filter(|v| *v > 0.0).ok_or_else(|| bad("the baseline grid increment must be a measure greater than zero"))?),
        );
    }
    if all || edited(d, "gridRelativeTo") {
        g.insert("relativeTo".into(), json!(d.s("gridRelativeTo")));
    }
    Ok(Some(Value::Object(g)))
}

fn confirm_text_frame_options(app: &mut DesignApp, d: &mut Dialog) -> Result<Value, String> {
    seed_text_frame_options(app, d);
    // The preview goes; the options are set once, as one undo step.
    if d.fields.contains_key(PREVIEWED) {
        end_text_frame_preview(app, d);
    }
    let p = text_frame_params(app, d)?;
    if p.keys().all(|k| k == "ids") {
        return Ok(Value::Null);
    }
    app.run("object.textFrameOptions", Value::Object(p))
}

/// The `object.textFrameOptions` params of what the dialog changed.
fn text_frame_params(app: &DesignApp, d: &Dialog) -> Result<Map<String, Value>, String> {
    let mut p = Map::new();
    let bad = |what: &str| format!("Text Frame Options: {what}");
    if edited(d, "columns") {
        p.insert("columns".into(), json!(d.n("columns").ok_or_else(|| bad("the number of columns is not a number"))?.max(1.0) as u64));
    }
    if edited(d, "gutter") {
        p.insert("gutter".into(), json!(d.m("gutter").ok_or_else(|| bad("the gutter is not a measure"))?));
    }
    let sides = ["insetTop", "insetLeft", "insetBottom", "insetRight"];
    if edited(d, "inset") {
        p.insert("inset".into(), json!(d.m("inset").ok_or_else(|| bad("the inset is not a measure"))?));
    } else if sides.iter().any(|k| edited(d, k)) {
        // Sides left as shown are null: each selected frame keeps its own.
        let mut inset = [None; 4];
        for (v, k) in inset.iter_mut().zip(sides) {
            if edited(d, k) {
                *v = Some(d.m(k).ok_or_else(|| bad("an inset is not a measure"))?);
            }
        }
        p.insert("inset".into(), json!(inset));
    }
    if edited(d, "balanceColumns") {
        p.insert("balanceColumns".into(), json!(d.b("balanceColumns")));
    }
    if edited(d, "verticalJustification") {
        p.insert("verticalJustification".into(), json!(d.s("verticalJustification")));
    }
    for key in ["columnsKind", "firstBaseline"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.s(key)));
        }
    }
    // The width counts only for Fixed Width columns (Fixed Number shows it).
    if d.s("columnsKind") == "fixedWidth" && (edited(d, "columnWidth") || edited(d, "columnsKind")) {
        p.insert("columnWidth".into(), json!(d.m("columnWidth").ok_or_else(|| bad("the column width is not a measure"))?));
    }
    if edited(d, "firstBaselineMin") {
        p.insert("firstBaselineMin".into(), json!(d.m("firstBaselineMin").ok_or_else(|| bad("the first baseline minimum is not a measure"))?));
    }
    for key in ["balanceColumns", "ignoreWrap"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.b(key)));
        }
    }
    if edited(d, "columnRule") {
        p.insert("columnRule".into(), json!(d.b("columnRule")));
    }
    if edited(d, "columnRuleWeight") {
        p.insert("columnRuleWeight".into(), json!(d.pt("columnRuleWeight").ok_or_else(|| bad("the column rule weight is not a measure"))?));
    }
    if edited(d, "columnRuleColor") {
        p.insert("columnRuleColor".into(), json!(d.s("columnRuleColor")));
    }
    if edited(d, "columnRuleTint") {
        p.insert("columnRuleTint".into(), json!(d.n("columnRuleTint").ok_or_else(|| bad("the column rule tint is not a number"))? / 100.0));
    }
    for (key, what) in [
        ("columnRuleTopInset", "the column rule top inset is not a measure"),
        ("columnRuleBottomInset", "the column rule bottom inset is not a measure"),
        ("columnRuleOffset", "the column rule offset is not a measure"),
    ] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.m(key).ok_or_else(|| bad(what))?));
        }
    }
    for key in ["autoSize", "autoSizeReference"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.s(key)));
        }
    }
    for key in ["useMinimumHeight", "useMinimumWidth", "noLineBreaks"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.b(key)));
        }
    }
    for (key, what) in [("minimumHeight", "the minimum height is not a measure"), ("minimumWidth", "the minimum width is not a measure")] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.m(key).filter(|v| *v >= 0.0).ok_or_else(|| bad(what))?));
        }
    }
    // The custom baseline grid: off, or the fields the user changed (all of them when it is
    // turned on, since it shows the document grid's).
    if let Some(grid) = text_frame_baseline_grid(d, &bad)? {
        p.insert("baselineGrid".into(), grid);
    }
    if edited(d, "paragraphSpacingLimit") {
        p.insert(
            "paragraphSpacingLimit".into(),
            json!(d.m("paragraphSpacingLimit").filter(|v| *v >= 0.0).ok_or_else(|| bad("the paragraph spacing limit is not a measure"))?),
        );
    }
    if let Some(ids) = text_frame_targets(app, d).map_err(bad)? {
        p.insert("ids".into(), ids);
    }
    Ok(p)
}

/// Typing in a frame: the frame that showed the caret, or every frame that held part of the
/// selected text, when the dialog opened. Those ids are refused once another document is active,
/// once Revert has replaced the document, or once the document has changed (Undo can give a new
/// frame a stored id).
fn text_frame_targets(app: &DesignApp, d: &Dialog) -> Result<Option<Value>, &'static str> {
    let Some(ids) = d.fields.get("_targets").cloned() else { return Ok(typing_frame_ids(app)) };
    let active = app.session.active().map(|doc| doc.identity_epoch);
    if d.fields.get("_targetsDoc").and_then(Value::as_u64) != active {
        return Err("the document the dialog opened on is no longer the active one");
    }
    if d.fields.get("_targetsRevision").and_then(Value::as_u64) != app.session.active().map(base_revision) {
        return Err("the document changed after the dialog opened");
    }
    Ok(Some(ids))
}

/// In a native batch's `pdfImport` dialog: the paths chosen after its PDF, placed once it is confirmed.
pub(crate) const PDF_IMPORT_REST: &str = "rest";

/// File › Document Setup, filled from the document.
/// The active document's ruler unit (picas without a document).
fn doc_unit(app: &DesignApp) -> Unit {
    app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas)
}

/// Table › Go to Row…: opens on the target table's first selected row, numbered within its header,
/// body or footer rows.
pub fn open_go_to_row(app: &mut DesignApp) {
    let Ok(t) = app.run("table.get", json!({"cells": false})) else { return };
    let (h, f, r) = (count_at(&t, "headerRows"), count_at(&t, "footerRows"), count_at(&t["range"], "r0"));
    let nr = t["rows"].as_array().map_or(0, Vec::len);
    let (section, n) = if r < h {
        ("header", r + 1)
    } else if r >= nr.saturating_sub(f) {
        ("footer", r + 1 - nr.saturating_sub(f))
    } else {
        ("body", r + 1 - h)
    };
    app.ui.dialog = Some(Dialog::new(
        "goToRow",
        json!({
            "uid": app.session.active().map(|doc| doc.uid),
            "story": t["story"],
            "table": t["table"],
            "section": section,
            "row": n.to_string(),
            "hasHeader": h > 0,
            "hasFooter": f > 0
        }),
    ));
}

fn count_at(v: &Value, k: &str) -> usize {
    v[k].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0)
}

/// File › Place Contact Sheet…: bare numbers typed in the dialog are in the document's ruler unit
/// (the defaults keep their explicit `pt`).
pub fn open_contact_sheet(app: &mut DesignApp) {
    app.ui.dialog = Some(contact_sheet_dialog(app, json!({})));
}

/// The Place Contact Sheet dialog with `fields`, its bare numbers in the document's ruler unit,
/// for the menu and for `ui.dialog.open` alike.
pub fn contact_sheet_dialog(app: &DesignApp, fields: Value) -> Dialog {
    let mut d = Dialog::new("contactSheet", fields);
    d.unit = doc_unit(app);
    d
}

/// The document's vertical ruler unit: what a bare number means in a height field.
fn doc_vertical_unit(app: &DesignApp) -> Unit {
    app.session.active().map(|s| s.doc.settings.vertical_units).unwrap_or(Unit::Picas)
}

/// The Rectangle dialog a click with a frame tool opens: the width (points in `params`) shown in
/// the horizontal ruler unit, the height in the vertical one, as Preferences › Units &
/// Increments and the rulers' menus set them (#1964).
pub fn frame_size_dialog(app: &DesignApp, params: Value) -> Dialog {
    let mut d = Dialog::new("frameSize", params);
    for (k, unit) in [("width", doc_unit(app)), ("height", doc_vertical_unit(app))] {
        if let Some(v) = d.fields.get(k).and_then(Value::as_f64) {
            d.fields.insert(k.into(), json!(format_measure(v, unit)));
        }
    }
    d.unit = doc_unit(app);
    d.vertical_unit = Some(doc_vertical_unit(app));
    d
}

/// Layout › Create Guides…: guides go on every page of the spread in view (or of the parent
/// spread being edited), as InDesign's do; "This page" narrows them to
/// the page in view. The gutters are typed in the document's ruler unit.
pub fn open_create_guides(app: &mut DesignApp) {
    let unit = doc_unit(app);
    let Some((spread, page)) = crate::canvas::current_spread_page(app) else { return };
    let gutter = json!(format_measure(12.0, unit));
    let mut d = Dialog::new(
        "createGuides",
        json!({"rows": 0, "columns": 0, "rowGutter": gutter, "columnGutter": gutter, "fitTo": "margins", "removeExisting": false,
            "spread": serde_json::to_value(spread).unwrap_or(json!(0)), "page": page,
            // InDesign puts the guides on both pages of a facing spread (and a parent spread is
            // edited as a whole, so they show on every page that uses it).
            "target": "spread",
            "where": target_description(app, spread, page)}),
    );
    d.unit = unit;
    app.ui.dialog = Some(d);
}

/// "Page 4" / "Pages 4–5" / "Parent A-Parent": what a dialog's target choice refers to.
fn target_description(app: &DesignApp, spread: SpreadRef, page: usize) -> String {
    let Some(st) = app.session.active() else { return String::new() };
    match spread {
        SpreadRef::Doc(si) => {
            let first = st.doc.first_page_of_spread(si);
            format!("page {}", app.session.page_label(first + page))
        }
        SpreadRef::Parent(pi) => {
            let name = st.doc.parents.get(pi).and_then(|p| p.parent.as_ref()).map(|i| i.label()).unwrap_or_default();
            format!("parent {name}")
        }
    }
}

/// Layout › Margins and Columns…: the margins and columns of the page in view (or of the parent
/// spread while parents are edited), shown and typed in the document's ruler unit.
pub fn open_margins_and_columns(app: &mut DesignApp) {
    let unit = doc_unit(app);
    let parent = crate::canvas::current_parent_page(app);
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let Some(st) = app.session.active() else { return };
    let page = match parent {
        Some((pi, qi)) => st.doc.parents.get(pi).and_then(|s| s.pages.get(qi)),
        None => st.doc.page(cur),
    };
    let Some(pg) = page else { return };
    let fm = |v: f64| json!(format_measure(v, unit));
    // The pages of the spread in view: both pages of a facing spread.
    let spread_pages: Vec<usize> = match parent {
        Some((pi, _)) => (0..st.doc.parents.get(pi).map_or(0, |s| s.pages.len())).collect(),
        None => st.doc.page_loc(cur).map_or_else(Vec::new, |(si, _)| {
            let first = st.doc.first_page_of_spread(si);
            (first..first + st.doc.spreads.get(si).map_or(0, |s| s.pages.len())).collect()
        }),
    };
    let where_ = match parent {
        Some((pi, qi)) => target_description(app, SpreadRef::Parent(pi), qi),
        None => format!("page {}", app.session.page_label(cur)),
    };
    let mut f = json!({
        "columns": pg.columns.count, "gutter": fm(pg.columns.gutter),
        "marginTop": fm(pg.margins.top), "marginBottom": fm(pg.margins.bottom),
        "marginInside": fm(pg.margins.inside), "marginOutside": fm(pg.margins.outside),
        "adjustLayout": false, "page": cur, "spreadPages": spread_pages,
        "target": if parent.is_some() { "spread" } else { "page" }, "where": where_,
    });
    if let Some((pi, qi)) = parent {
        f["parent"] = json!(pi);
        f["parentPage"] = json!(qi);
    }
    let mut d = Dialog::new("marginsAndColumns", f);
    d.unit = unit;
    app.ui.dialog = Some(d);
}

/// The margin fields of Margins and Columns: (key, label on a facing page, label on a single one).
const MARGIN_FIELDS: [(&str, &str, &str); 4] = [
    ("marginTop", "Top:", "Top:"),
    ("marginBottom", "Bottom:", "Bottom:"),
    ("marginInside", "Inside:", "Left:"),
    ("marginOutside", "Outside:", "Right:"),
];

/// Linked margins: a margin changed since the dialog last looked (typed, or set over the control
/// channel) is all four.
fn link_margins(d: &mut Dialog) {
    let now: Vec<String> = MARGIN_FIELDS.iter().map(|(k, _, _)| d.s(k)).collect();
    let seen: Vec<String> = d.fields.get("_margins").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_else(|| now.clone());
    let mut now = now;
    if d.b("link")
        && let Some(i) = now.iter().zip(&seen).position(|(a, b)| a != b)
    {
        let v = now[i].clone();
        for (k, _, _) in &MARGIN_FIELDS {
            d.fields.insert((*k).into(), json!(v));
        }
        now = vec![v; MARGIN_FIELDS.len()];
    }
    d.fields.insert("_margins".into(), json!(now));
}

/// The command params of the Margins and Columns or Create Guides dialog. A measure that does
/// not parse is an error to show, never a silent 0.
fn layout_dialog_params(d: &Dialog) -> Result<Value, String> {
    let measure =
        |key: &str, what: &str| d.m(key).filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("Enter a valid {what}, e.g. 12 or 12 mm."));
    if d.id == "createGuides" {
        let (row_gutter, column_gutter) = (measure("rowGutter", "gutter")?, measure("columnGutter", "gutter")?);
        let mut p = json!({"rows": d.n("rows").unwrap_or(0.0).max(0.0) as u64, "columns": d.n("columns").unwrap_or(0.0).max(0.0) as u64,
            "rowGutter": row_gutter, "columnGutter": column_gutter, "fitTo": d.s("fitTo"),
            "removeExisting": d.b("removeExisting"), "spread": d.fields.get("spread").cloned().unwrap_or(json!(0))});
        // Without `page` the command covers every page of the spread.
        if d.s("target") != "spread" {
            p["page"] = d.fields.get("page").cloned().unwrap_or(json!(0));
        }
        return Ok(p);
    }
    let mut v = Map::new();
    for (k, key) in [("top", "marginTop"), ("bottom", "marginBottom"), ("inside", "marginInside"), ("outside", "marginOutside")] {
        v.insert(k.into(), json!(measure(key, &format!("{k} value"))?));
    }
    let gutter = measure("gutter", "gutter value")?;
    let mut p =
        json!({"margins": v, "columns": d.n("columns").unwrap_or(1.0).max(1.0) as u64, "gutter": gutter, "adjustLayout": d.b("adjustLayout")});
    let spread_pages: Vec<u64> =
        d.fields.get("spreadPages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    if let Some(pi) = d.fields.get("parent").and_then(Value::as_u64) {
        p["parent"] = json!(pi);
        p["pages"] =
            if d.s("target") == "page" { json!([d.fields.get("parentPage").and_then(Value::as_u64).unwrap_or(0)]) } else { json!(spread_pages) };
    } else {
        match d.s("target").as_str() {
            "all" => {}
            "spread" if !spread_pages.is_empty() => p["pages"] = json!(spread_pages),
            _ => p["pages"] = json!([d.fields.get("page").and_then(Value::as_u64).unwrap_or(0)]),
        }
    }
    Ok(p)
}

/// Preview on: the change shows on the page (redone when a field changes) without an undo step;
/// off, the page goes back.
fn layout_dialog_preview(app: &mut DesignApp, d: &mut Dialog, cmd: &str, label: &str) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let params = match layout_dialog_params(d) {
        Ok(p) => p,
        Err(e) => {
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction(label)
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview(cmd, params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

pub fn open_document_setup(app: &mut DesignApp) {
    let Ok(cur) = app.session.execute("layout.documentSetup", &json!({})) else { return };
    let units = app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let fm = |v: &Value| json!(format_measure(v.as_f64().unwrap_or(0.0), units));
    let mut f = json!({"intent": cur["intent"], "pages": cur["pages"], "startPage": cur["startPage"], "facingPages": cur["facingPages"],
        "binding": cur["binding"], "width": fm(&cur["width"]), "height": fm(&cur["height"]), "units": units,
        "primaryTextFrame": cur["primaryTextFrame"]});
    for (k, values) in [
        ("margin", ["top", "bottom", "inside", "outside"].map(|e| cur["margins"][e].clone())),
        ("bleed", [0, 1, 2, 3].map(|i| cur["bleed"][i].clone())),
        ("slug", [0, 1, 2, 3].map(|i| cur["slug"][i].clone())),
    ] {
        for (e, v) in SETUP_EDGES.iter().zip(&values) {
            f[format!("{k}{e}")] = fm(v);
        }
        let first = values[0].as_f64();
        f[format!("{k}Linked")] = json!(values.iter().all(|v| v.as_f64() == first));
    }
    // The margins as shown: OK applies them to every page only when they were edited.
    f["marginsShown"] = json!(SETUP_EDGES.map(|e| f[format!("margin{e}")].clone()));
    let mut d = Dialog::new("documentSetup", f);
    d.unit = units;
    app.ui.dialog = Some(d);
}

/// The fields of File › File Info, in the order the dialog shows them, with their labels.
const FILE_INFO_FIELDS: [(&str, &str); 6] = [
    ("documentTitle", "Document Title:"),
    ("author", "Author:"),
    ("description", "Description:"),
    ("keywords", "Keywords:"),
    ("copyrightNotice", "Copyright Notice:"),
    ("copyrightInfoUrl", "Copyright Info URL:"),
];

/// File › File Info, filled from the document. Keywords show as one line, separated by semicolons.
pub fn open_file_info(app: &mut DesignApp) {
    let Ok(cur) = app.session.execute("file.info.get", &json!({})) else { return };
    let mut f = Map::new();
    for (key, _) in FILE_INFO_FIELDS {
        let v = match &cur[key] {
            Value::Array(words) => words.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; "),
            v => v.as_str().unwrap_or_default().to_string(),
        };
        f.insert(key.into(), Value::String(v));
    }
    // The keywords as shown and as stored: OK sends them only when they were edited, and then a
    // stored keyword holding a semicolon or comma that is still there whole stays one keyword.
    let shown = f.get("keywords").cloned().unwrap_or_default();
    f.insert("keywordsShown".into(), shown);
    f.insert("keywordsStored".into(), cur["keywords"].clone());
    // The document the fields belong to, should another one become active before OK.
    f.insert("uid".into(), json!(app.session.active().map(|doc| doc.uid)));
    app.ui.dialog = Some(Dialog::new("fileInfo", Value::Object(f)));
}

/// File Info's OK: every field the dialog holds, as typed (an empty one clears it); the keywords
/// only when they were edited. A field that is not text (a keyword list, or `null` from
/// `ui.dialog.open`) goes to `file.info` as it is. The fields go to the document the dialog was
/// opened for, even when another one became active meanwhile, and the active document stays
/// active; when the dialog's document has closed, nothing is stored.
fn confirm_file_info(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let keywords_edited = d.fields.get("keywordsShown").and_then(Value::as_str) != Some(d.s("keywords").as_str());
    let stored: Vec<String> = match d.fields.get("keywordsStored") {
        Some(Value::Array(words)) => words.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let p: Map<String, Value> = FILE_INFO_FIELDS
        .iter()
        .filter(|(key, _)| d.fields.contains_key(*key))
        .filter(|(key, _)| *key != "keywords" || keywords_edited)
        .map(|(key, _)| {
            let v = match d.fields.get(*key) {
                Some(Value::String(s)) if *key == "keywords" => json!(designcraft_doc::file_info::parse_keywords_keeping(s, &stored)),
                Some(v) => v.clone(),
                None => Value::Null,
            };
            (key.to_string(), v)
        })
        .collect();
    let uid = d.fields.get("uid").and_then(Value::as_u64);
    let active = app.session.active().map(|doc| doc.uid);
    if uid.is_none() || uid == active {
        return app.run("file.info", Value::Object(p));
    }
    let index = |app: &DesignApp, uid: Option<u64>| app.session.documents().iter().position(|doc| Some(doc.uid) == uid);
    let Some(i) = index(app, uid) else { return Ok(Value::Null) };
    // Switching documents ends the active document's Data Merge preview; it is shown again after.
    let preview = app.session.active().and_then(|doc| doc.preview_record);
    app.run("file.activate", json!({"index": i}))?;
    let r = app.run("file.info", Value::Object(p));
    if let Some(back) = index(app, active) {
        app.run("file.activate", json!({"index": back}))?;
        if let Some(record) = preview {
            app.run("data.preview", json!({"record": record}))?;
        }
    }
    r
}

/// The edges of Document Setup's margins, bleed and slug (field suffixes).
const SETUP_EDGES: [&str; 4] = ["Top", "Bottom", "Inside", "Outside"];

/// The units Document Setup shows its measures in (the document's horizontal units).
fn setup_units(d: &Dialog) -> Unit {
    d.fields.get("units").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(Unit::Picas)
}

/// A Document Setup measure in points, read as every dialog reads its measures.
fn setup_measure(d: &Dialog, key: &str) -> Option<f64> {
    d.m(key).filter(|v| v.is_finite())
}

/// Document Setup's OK: `layout.documentSetup` with what was typed; a value that can't be read is
/// refused before anything changes. The margins (the first page's, shown) go to every page only
/// when they were edited, so pages keep their own margins otherwise.
fn confirm_document_setup(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let measure = |key: &str| setup_measure(d, key).ok_or_else(|| format!("{key}: not a measure"));
    let edges = |group: &str| -> Result<Vec<f64>, String> { SETUP_EDGES.iter().map(|e| measure(&format!("{group}{e}"))).collect() };
    let count = |key: &str| d.n(key).filter(|n| n.is_finite() && *n >= 1.0).map(|n| n as u64).ok_or_else(|| format!("{key}: a whole number from 1"));
    let margins = edges("margin")?;
    let shown = d.fields.get("marginsShown").and_then(Value::as_array);
    let edited = SETUP_EDGES.iter().enumerate().any(|(i, e)| shown.and_then(|s| s.get(i)) != d.fields.get(&format!("margin{e}")));
    let mut p = json!({"intent": d.s("intent"), "pages": count("pages")?, "startPage": count("startPage")?, "facingPages": d.b("facingPages"),
        "binding": d.s("binding"), "width": measure("width")?, "height": measure("height")?,
        "bleed": edges("bleed")?, "slug": edges("slug")?, "adjustLayout": d.b("adjustLayout")});
    if edited {
        p["margins"] = json!({"top": margins[0], "bottom": margins[1], "inside": margins[2], "outside": margins[3]});
    }
    app.run("layout.documentSetup", p)
}

/// The dialog's localized window title.
pub(crate) fn dialog_title(app: &DesignApp, d: &Dialog) -> String {
    let alert_title = d.s("title");
    let title = match d.id.as_str() {
        "alert" => crate::i18n::tr(&app.ui.language, &alert_title),
        "ruby" => crate::i18n::tr(&app.ui.language, "Ruby"),
        "newDocument" => crate::i18n::tr(&app.ui.language, "New Document"),
        "frameSize" => crate::i18n::tr(&app.ui.language, "Rectangle"),
        "goToRow" => crate::i18n::tr(&app.ui.language, "Go to Row"),
        "goToPage" => crate::i18n::tr(&app.ui.language, "Go to Page"),
        "insertPages" => crate::i18n::tr(&app.ui.language, "Insert Pages"),
        "insertTable" => crate::i18n::tr(&app.ui.language, "Insert Table…").trim_end_matches('…').trim_end(),
        // The menu items' translations (every language has them), without the ellipsis.
        "convertTextToTable" => crate::i18n::tr(&app.ui.language, "Convert Text to Table…").trim_end_matches('…').trim_end(),
        "convertTableToText" => crate::i18n::tr(&app.ui.language, "Convert Table to Text…").trim_end_matches('…').trim_end(),
        "cellOptions" | "diagonalLines" => crate::i18n::tr(&app.ui.language, "Cell Options"),
        "createGuides" => crate::i18n::tr(&app.ui.language, "Create Guides"),
        "marginsAndColumns" => crate::i18n::tr(&app.ui.language, "Margins and Columns"),
        "textFrameOptions" => crate::i18n::tr(&app.ui.language, "Text Frame Options"),
        "colorTheme" => crate::i18n::tr(&app.ui.language, "Color Theme"),
        "documentSetup" => crate::i18n::tr(&app.ui.language, "Document Setup"),
        "fileInfo" => crate::i18n::tr(&app.ui.language, "File Info"),
        // On a page that doesn't start a section yet, InDesign titles it New Section.
        "numberingSection" if d.b("newSection") => crate::i18n::tr(&app.ui.language, "New Section"),
        "numberingSection" => crate::i18n::tr(&app.ui.language, "Numbering & Section Options…").trim_end_matches('…'),
        "findChange" => crate::i18n::tr(&app.ui.language, "Find/Change"),
        "paragraphStyleOptions" if d.b("new") => crate::i18n::tr(&app.ui.language, "New Paragraph Style"),
        "paragraphStyleOptions" => crate::i18n::tr(&app.ui.language, "Paragraph Style Options"),
        "deleteParagraphStyle" => crate::i18n::tr(&app.ui.language, "Delete Paragraph Style"),
        "characterStyleOptions" if d.b("new") => crate::i18n::tr(&app.ui.language, "New Character Style"),
        "characterStyleOptions" => crate::i18n::tr(&app.ui.language, "Character Style Options"),
        "deleteCharacterStyle" => crate::i18n::tr(&app.ui.language, "Delete Character Style"),
        "footnoteOptions" => crate::i18n::tr(&app.ui.language, "Footnote Options"),
        "paragraphRules" => crate::i18n::tr(&app.ui.language, "Paragraph Rules"),
        "paragraphLocal" => crate::i18n::tr(
            &app.ui.language,
            match d.s("section").as_str() {
                "span" => "Span Columns",
                "bullets" => "Bullets and Numbering",
                "keep" => "Keep Options",
                "nested" => "Drop Caps and Nested Styles",
                "grep" => "GREP Styles",
                _ => "Paragraph Borders and Shading",
            },
        ),
        "justification" => crate::i18n::tr(&app.ui.language, "Justification"),
        "hyphenation" => crate::i18n::tr(&app.ui.language, "Hyphenation Settings"),
        "insertXref" => crate::i18n::tr(&app.ui.language, "New Cross-Reference"),
        "findFont" => crate::i18n::tr(&app.ui.language, "Find/Replace Font"),
        "polygonSettings" => crate::i18n::tr(&app.ui.language, "Polygon Settings"),
        "userDictionary" => crate::i18n::tr(&app.ui.language, "User Dictionary"),
        "newWorkspace" => crate::i18n::tr(&app.ui.language, "New Workspace"),
        "closeDocument" => crate::i18n::tr(&app.ui.language, "Unsaved Changes"),
        "deletePages" => crate::i18n::tr(&app.ui.language, "Warning"),
        "pagesPanelOptions" => crate::i18n::tr(&app.ui.language, "Panel Options"),
        "parentOptions" => crate::i18n::tr(&app.ui.language, "Parent Options"),
        "updateLink" => crate::i18n::tr(&app.ui.language, "Update Link"),
        "menus" => crate::i18n::tr(&app.ui.language, "Menu Customization"),
        "importOptions" => crate::i18n::tr(&app.ui.language, "Import Options"),
        "loadStyles" => crate::i18n::tr(&app.ui.language, "Load Styles"),
        "fittingOptions" => crate::i18n::tr(&app.ui.language, "Frame Fitting Options"),
        "qrCode" => crate::i18n::tr(&app.ui.language, "Generate QR Code"),
        "preferences" => crate::i18n::tr(&app.ui.language, "Preferences"),
        "print" => crate::i18n::tr(&app.ui.language, "Print"),
        "contactSheet" => crate::i18n::tr(&app.ui.language, "Place Contact Sheet"),
        "pdfImport" => crate::i18n::tr(&app.ui.language, "Place PDF"),
        "pdfExport" => crate::i18n::tr(&app.ui.language, "Export PDF"),
        "epubExport" => crate::i18n::tr(&app.ui.language, "EPUB Export Options"),
        "colorSettings" => crate::i18n::tr(&app.ui.language, "Color Settings"),
        "layerOptions" => crate::i18n::tr(&app.ui.language, "Object Layer Options"),
        "colorPicker" => crate::i18n::tr(&app.ui.language, "Color Picker"),
        "swatchOptions" => crate::i18n::tr(&app.ui.language, "Swatch Options"),
        "colorGroupOptions" => crate::i18n::tr(&app.ui.language, "Color Group Options"),
        "layerSettings" => crate::i18n::tr(&app.ui.language, "Layer Options"),
        crate::effects_dialog::ID => crate::i18n::tr(&app.ui.language, "Effects"),
        crate::export_batch_dialog::ID => crate::i18n::tr(&app.ui.language, "Export Layouts and Versions"),
        crate::package_dialog::ID => crate::i18n::tr(&app.ui.language, "Package"),
        crate::package_dialog::INSTRUCTIONS => crate::i18n::tr(&app.ui.language, "Printing Instructions"),
        crate::object_style_dialog::ID => crate::i18n::tr(&app.ui.language, crate::object_style_dialog::title(&d)),
        id => match id.strip_prefix("cmd:").and_then(designcraft_engine::find_command) {
            Some(c) => crate::i18n::tr(&app.ui.language, c.label.trim_end_matches('…')),
            None => crate::i18n::tr(&app.ui.language, "Dialog"),
        },
    };
    title.to_string()
}

/// The labels of the dialog's accept and cancel buttons.
pub(crate) fn footer_labels(app: &DesignApp, d: &Dialog) -> (String, &'static str) {
    let ok_label = match d.id.as_str() {
        "colorTheme" => crate::i18n::tr(&app.ui.language, "Add to Swatches").to_string(),
        "closeDocument" => crate::i18n::tr(&app.ui.language, "  Save  ").to_string(),
        "updateLink" => crate::i18n::tr(&app.ui.language, "  Update  ").to_string(),
        "findFont" => crate::i18n::tr(&app.ui.language, "Replace All").to_string(),
        "newDocument" => format!("  {}  ", crate::i18n::tr(&app.ui.language, "Create")),
        crate::export_batch_dialog::ID => format!("  {}  ", crate::i18n::tr(&app.ui.language, "Export…")),
        crate::package_dialog::ID => crate::i18n::tr(&app.ui.language, "Package…").to_string(),
        crate::package_dialog::INSTRUCTIONS => crate::i18n::tr(&app.ui.language, "Continue").to_string(),
        _ => crate::i18n::tr(&app.ui.language, "  OK  ").to_string(),
    };
    let cancel_label = match d.id.as_str() {
        "newDocument" => "Close",
        "findFont" => "Done",
        _ => "Cancel",
    };
    (ok_label, cancel_label)
}

/// Draw the active localized dialog; open popups own Enter/Escape, and result handling drives confirm or cancel.
pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    show_in(app, ctx, true);
}

/// [`show`] for an app whose Ui is `enabled` or not: a framework dialog of a disabled app takes no input.
pub fn show_in(app: &mut DesignApp, ctx: &egui::Context, enabled: bool) {
    let shortcuts = app.ui.dialog.as_ref().is_some_and(|d| d.id == crate::shortcuts_dialog::DIALOG_ID);
    if shortcuts || app.shortcut_dialog.is_some() {
        // Also the frames after the dialog was closed from outside, and after another dialog took its place (its edits are
        // cancelled): the framework dialog answers OK and Cancel inside a pass.
        crate::shortcuts_dialog::show(app, ctx);
        if shortcuts {
            return;
        }
    }
    if app.ui.dialog.as_ref().is_some_and(|d| d.id == "numberingSection") {
        crate::section_options::show_framework(app, ctx);
        return;
    }
    if let Some(form) = app.ui.dialog.as_ref().and_then(|d| framework::form(&d.id)) {
        framework::show(app, ctx, form, enabled);
    }
}

/// Cell Options keys (`table.getCellStroke` names) and whether each belongs to the stroke.
const CELL_OPTION_KEYS: [(&str, bool); 8] =
    [("weight", true), ("type", true), ("color", true), ("tint", true), ("gapColor", true), ("gapTint", true), ("fill", false), ("fillTint", false)];

/// Table › Cell Options › Strokes and Fills… for the target cells. The fields start from the
/// cells' values (`seed`); OK applies the fields that differ from it.
pub fn open_cell_options(app: &mut DesignApp) -> Result<Value, String> {
    let edges = crate::panels::cell_stroke::edges_param(&[true; 6]);
    let cur = app.session.execute("table.getCellStroke", &json!({"edges": edges})).map_err(|e| e.to_string())?;
    let mut fields = Map::new();
    for (k, _) in CELL_OPTION_KEYS {
        fields.insert(k.into(), cur[k].clone());
    }
    fields.insert("seed".into(), Value::Object(fields.clone()));
    fields.insert("edges".into(), edges);
    app.ui.dialog = Some(Dialog::new("cellOptions", Value::Object(fields)));
    Ok(Value::Null)
}

/// Cell Options with a new edge choice: the stroke fields the user hasn't changed show the
/// chosen edges' values, and every stroke field's seed becomes those values, so OK applies
/// exactly the fields that differ from what the chosen edges show.
fn cell_options_edges_changed(app: &mut DesignApp, d: &mut Dialog, edges: &crate::panels::cell_stroke::Edges) {
    d.fields.insert("edges".into(), crate::panels::cell_stroke::edges_param(edges));
    let cur = crate::panels::cell_stroke::current(app, edges);
    let mut seed = d.fields.get("seed").and_then(Value::as_object).cloned().unwrap_or_default();
    for (k, stroke) in CELL_OPTION_KEYS {
        if !stroke {
            continue;
        }
        let v = cur.get(k).cloned().unwrap_or(Value::Null);
        // Null shows mixed values; the user can't type it.
        let field = d.fields.get(k).filter(|f| !f.is_null());
        if field.is_none_or(|f| same_cell_value(seed.get(k), k, f)) {
            d.fields.insert(k.into(), v.clone());
        }
        seed.insert(k.into(), v);
    }
    d.fields.insert("seed".into(), Value::Object(seed));
}

/// Whether the dialog field `k` holds the cells' value `seed`. The Type list picks a stroke type
/// by name (`"solid"`), and the cells' value is the type itself (`{"kind": "solid"}`).
fn same_cell_value(seed: Option<&Value>, k: &str, v: &Value) -> bool {
    seed == Some(v) || (k == "type" && v.as_str().is_some_and(|name| seed == Some(&json!({"kind": name}))))
}

/// Apply the Cell Options fields that differ from the cells' values, as one `table.setCell`.
fn confirm_cell_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let seed = d.fields.get("seed").cloned().unwrap_or(Value::Null);
    let mut stroke = Map::new();
    let mut params = Map::new();
    for (k, is_stroke) in CELL_OPTION_KEYS {
        let Some(v) = d.fields.get(k).filter(|v| !v.is_null() && !same_cell_value(seed.get(k), k, v)) else { continue };
        match (is_stroke, k) {
            (true, _) => {
                stroke.insert(k.into(), v.clone());
            }
            (false, "fillTint") => {
                params.insert("tint".into(), v.clone());
            }
            _ => {
                params.insert(k.into(), v.clone());
            }
        }
    }
    if !stroke.is_empty() {
        stroke.insert("edges".into(), d.fields.get("edges").cloned().unwrap_or(json!("all")));
        params.insert("stroke".into(), Value::Object(stroke));
    }
    if params.is_empty() {
        return Ok(Value::Null);
    }
    app.run("table.setCell", Value::Object(params))
}

/// The line buttons of Cell Options › Diagonal Lines: the `diagonal.lines` name and its label.
const DIAGONAL_LINES: [(&str, &str); 4] = [
    ("none", "No Diagonal Lines"),
    ("topLeft", "Diagonal Line from Top Left to Bottom Right"),
    ("topRight", "Diagonal Line from Top Right to Bottom Left"),
    ("crossed", "Crossed Diagonal Lines"),
];

/// Cell Options › Diagonal Lines keys (`table.getCellDiagonal` names).
const DIAGONAL_KEYS: [&str; 8] = ["lines", "weight", "type", "color", "tint", "gapColor", "gapTint", "inFront"];

/// Table › Cell Options › Diagonal Lines… for the target cells. The fields start from the
/// cells' values (`seed`); OK applies the fields that differ from it. The dialog keeps the
/// document it opened in (`document`: uid and identity epoch, and `Dialog::contents`) and its
/// target cells (`cells`).
pub fn open_diagonal_lines(app: &mut DesignApp) -> Result<Value, String> {
    app.ui.dialog = Some(diagonal_lines_dialog(app, Value::Null)?);
    Ok(Value::Null)
}

/// The Diagonal Lines dialog for the target cells, with the `DIAGONAL_KEYS` values in `params`
/// (from `ui.dialog.open`) entered in place of the cells' values.
pub fn diagonal_lines_dialog(app: &mut DesignApp, params: Value) -> Result<Dialog, String> {
    let cur = app.session.execute("table.getCellDiagonal", &json!({})).map_err(|e| e.to_string())?;
    let st = app.session.active().ok_or("no document open")?;
    let document = json!({"uid": st.uid, "epoch": st.identity_epoch});
    let contents = st.doc.clone();
    let mut fields = Map::new();
    for k in DIAGONAL_KEYS {
        fields.insert(k.into(), cur[k].clone());
    }
    fields.insert("seed".into(), Value::Object(fields.clone()));
    for k in DIAGONAL_KEYS {
        if let Some(v) = params.get(k) {
            fields.insert(k.into(), v.clone());
        }
    }
    fields.insert("document".into(), document);
    fields.insert("cells".into(), json!({"story": cur["story"], "table": cur["table"], "rows": cur["rows"], "cols": cur["cols"]}));
    let mut d = Dialog::new("diagonalLines", Value::Object(fields));
    d.contents = Some(contents);
    Ok(d)
}

/// Where the document a Diagonal Lines dialog opened in is now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DiagonalLinesDocument {
    /// Active and unchanged since the dialog opened.
    Ready,
    /// Open, but another document is active.
    Inactive,
    /// Active, but edited since the dialog opened: the saved cell range may name other cells.
    Changed,
    /// Closed or reverted.
    Gone,
}

/// An edit gives the document new contents, and a selection change keeps them. The dialog holds
/// the contents it opened with, so no new contents can take their address while it is open.
fn diagonal_lines_document(app: &DesignApp, d: &Dialog) -> DiagonalLinesDocument {
    let saved = d.fields.get("document");
    let get = |k: &str| saved.and_then(|s| s.get(k)).and_then(Value::as_u64);
    let (Some(uid), Some(epoch), Some(contents)) = (get("uid"), get("epoch"), d.contents.as_ref()) else {
        return DiagonalLinesDocument::Gone;
    };
    let Some(i) = app.session.documents().iter().position(|st| st.uid == uid && st.identity_epoch == epoch) else {
        return DiagonalLinesDocument::Gone;
    };
    if app.session.active_index() != Some(i) {
        DiagonalLinesDocument::Inactive
    } else if !std::sync::Arc::ptr_eq(&app.session.documents()[i].doc, contents) {
        DiagonalLinesDocument::Changed
    } else {
        DiagonalLinesDocument::Ready
    }
}

/// Apply the Diagonal Lines fields that differ from the cells' values, as one `table.setCell`
/// on the cells the dialog opened for, in the document it opened in.
fn confirm_diagonal_lines(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    match diagonal_lines_document(app, d) {
        DiagonalLinesDocument::Ready => {}
        DiagonalLinesDocument::Inactive => return Err("Diagonal Lines: the document the dialog opened in is not the active document".into()),
        DiagonalLinesDocument::Changed => return Err("Diagonal Lines: the document changed while the dialog was open".into()),
        DiagonalLinesDocument::Gone => return Err("Diagonal Lines: the document the dialog opened in was closed or reverted".into()),
    }
    let Some(Value::Object(cells)) = d.fields.get("cells") else {
        return Err("Diagonal Lines: the dialog has no target cells".into());
    };
    let seed = d.fields.get("seed").cloned().unwrap_or(Value::Null);
    let mut diagonal = Map::new();
    for k in DIAGONAL_KEYS {
        if let Some(v) = d.fields.get(k).filter(|v| !v.is_null() && !same_cell_value(seed.get(k), k, v)) {
            diagonal.insert(k.into(), v.clone());
        }
    }
    if diagonal.is_empty() {
        return Ok(Value::Null);
    }
    let mut params = cells.clone();
    params.insert("diagonal".into(), Value::Object(diagonal));
    app.run("table.setCell", Value::Object(params))
}

/// Delete absolute pages `pages` as the Pages panel and Layout › Pages › Delete Page do: when
/// they hold objects, InDesign's warning asks first, unless it was told not to.
pub fn delete_pages(app: &mut DesignApp, pages: Vec<usize>) -> Result<Value, String> {
    let has_items = app.session.active().is_some_and(|st| pages.iter().any(|&abs| st.doc.page_has_items(abs)));
    if has_items && !app.ui.skip_delete_pages_warning {
        app.ui.dialog = Some(Dialog::new("deletePages", json!({"pages": pages})));
        return Ok(Value::Null);
    }
    app.run("layout.pages.delete", json!({"pages": pages}))
}

/// Close the open dialog without applying it (Cancel, Esc, the control channel): a previewing
/// dialog puts the document back as it was, and Preferences puts back the interface language it
/// opened with, since choosing one previews it at once.
pub fn cancel(app: &mut DesignApp) {
    if app.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange") {
        app.session.reset_find_scope();
    }
    let d = app.ui.dialog.take();
    if d.as_ref().is_some_and(|d| d.id == crate::shortcuts_dialog::DIALOG_ID) {
        let _ = crate::shortcuts_dialog::close(app, false);
    }
    crate::new_document::end_preview(app);
    if d.as_ref().is_some_and(|d| d.fields.contains_key(PREVIEWED)) {
        // Text Frame Options names the document its preview is in.
        match d.as_ref().and_then(|d| d.fields.get(PREVIEW_DOC)).and_then(Value::as_u64) {
            Some(uid) => app.session.cancel_interaction_of(uid),
            None => app.session.cancel_interaction(),
        }
    }
    app.cancel_pdf_import();
    if let Some(d) = d
        && d.id == "preferences"
    {
        if let Some(lang) = d.fields.get("openedLanguage").and_then(Value::as_str)
            && lang != app.ui.language
        {
            let _ = app.run("app.language", json!({"lang": lang}));
        }
        // The Color Theme shown while choosing goes back too.
        if let Some(b) = d.fields.get("openedBrightness").and_then(Value::as_str)
            && b != app.ui.brightness.id()
        {
            let _ = app.run("window.brightness", json!({"brightness": b}));
        }
    }
}

fn find_change_params(d: &Dialog) -> Value {
    let mut p = json!({
        "find": d.s("find"), "change": d.s("change"), "grep": d.b("grep"),
        "caseSensitive": d.b("caseSensitive"), "wholeWord": d.b("wholeWord"), "scope": d.s("scope"),
    });
    if let Some(story) = d.fields.get("story") {
        p["story"] = story.clone();
    }
    // Find Format: only text with it is found (with nothing in Find what, all text with it).
    if let Some(f) = d.fields.get(FIND_FORMAT).and_then(Value::as_object).filter(|f| !f.is_empty()) {
        p["findFormat"] = Value::Object(f.clone());
    }
    // Change Format: find.change applies it to the changed text (the other commands ignore it).
    if let Some(f) = d.fields.get(CHANGE_FORMAT) {
        for key in ["charStyle", "paraStyle"] {
            if let Some(v) = f.get(key).filter(|v| v.is_string()) {
                p[key] = v.clone();
            }
        }
        if let Some(chars) = f.get("chars").filter(|c| c.as_object().is_some_and(|o| !o.is_empty())) {
            p["attrs"] = chars.clone();
        }
    }
    p
}

/// Find/Change's Find Format and Change Format ({charStyle?, paraStyle?, chars?}), as their
/// settings set them; absent when there is none.
const FIND_FORMAT: &str = "findFormat";
const CHANGE_FORMAT: &str = "changeFormat";
/// The fields of Find or Change Format Settings while it is open (`for`: find|change, its
/// section, `charStyle`, `paraStyle` and `c.<attr>`), until OK makes them the format.
const FORMAT_EDIT: &str = "formatEdit";

/// Whether the dialog has a Find (`FIND_FORMAT`) or Change Format.
fn format_set(d: &Dialog, key: &str) -> bool {
    d.fields.get(key).and_then(Value::as_object).is_some_and(|f| !f.is_empty())
}

/// The sections of Find and Change Format Settings: Style Options, then those of Character Style
/// Options.
const FORMAT_SECTIONS: &[(&str, &str)] = &[
    ("style", "Style Options"),
    ("chars", "Basic Character Formats"),
    ("advanced", "Advanced Character Formats"),
    ("color", "Character Color"),
    ("openType", "OpenType Features"),
    ("underline", "Underline Options"),
    ("strikethrough", "Strikethrough Options"),
];

/// What a Find or Change Format box shows: "Character Style: eggs + …".
fn format_summary(lang: &str, format: Option<&Value>) -> String {
    let Some(f) = format else { return String::new() };
    let mut parts = Vec::new();
    for (key, label) in [("charStyle", "Character Style:"), ("paraStyle", "Paragraph Style:")] {
        if let Some(name) = f.get(key).and_then(Value::as_str) {
            parts.push(format!("{} {}", crate::i18n::tr(lang, label), crate::i18n::style_name(lang, name)));
        }
    }
    parts.extend(
        f.get("chars").and_then(Value::as_object).into_iter().flatten().map(|(k, v)| format!("{}: {}", setting_label(lang, k), setting_text(v))),
    );
    parts.join(" + ")
}

/// Apply the open dialog.
pub fn confirm(app: &mut DesignApp) -> Result<Value, String> {
    let Some(d) = app.ui.dialog.take() else { return Err("no dialog open".into()) };
    if d.id == crate::shortcuts_dialog::DIALOG_ID {
        return crate::shortcuts_dialog::close(app, true).map(|()| Value::Null);
    }
    if matches!(d.id.as_str(), "paragraphStyleOptions" | "characterStyleOptions") && d.fields.contains_key(PREVIEWED) {
        // Restore the original before the ordinary command commits one undoable edit.
        app.session.cancel_interaction();
    }
    if d.id != "pdfImport" || !d.b("async") {
        app.cancel_pdf_import();
    }
    confirm_part1(app, d)
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part1(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "colorTheme" => {
            let result = app.run("swatch.addColorTheme", json!({"name": d.s("name"), "colors": d.fields.get("colors")}));
            if let Err(error) = &result {
                let mut d = d;
                d.fields.insert("status".into(), json!(error));
                app.ui.dialog = Some(d);
            }
            result
        }
        "createGuides" | "marginsAndColumns" => {
            // The preview goes; the change is made once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let cmd = if d.id == "createGuides" { "layout.createGuides" } else { "layout.marginsAndColumns" };
            let r = layout_dialog_params(&d).and_then(|p| app.run(cmd, p));
            if let Err(e) = &r {
                // A value that does not parse, or a refused one, keeps the dialog open to be fixed.
                let mut d = d.clone();
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        "newDocument" => crate::new_document::create(app, d),
        "frameSize" => {
            let x = d.fields.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let y = d.fields.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            let w = d.m("width").unwrap_or(72.0);
            // In the units the dialog showed, not ones changed while it was open.
            let h = d.m_in("height", d.vertical_unit.unwrap_or(d.unit)).unwrap_or(72.0);
            app.run(
                "frame.create",
                json!({"spread": d.fields.get("spread").cloned().unwrap_or(json!(0)), "shape": d.s("shape"), "content": d.s("content"), "rect": [x, y, x + w, y + h]}),
            )
        }
        "ruby" => app.run("type.ruby", style_cjk::ruby_params(&d)),
        "goToRow" => {
            // The story and table belong to the document the dialog was opened for: that document
            // becomes active first, and when it has closed, nothing is selected.
            let uid = d.fields.get("uid").and_then(Value::as_u64);
            if uid.is_some() && uid != app.session.active().map(|doc| doc.uid) {
                let Some(i) = app.session.documents().iter().position(|doc| Some(doc.uid) == uid) else { return Ok(Value::Null) };
                app.run("file.activate", json!({"index": i}))?;
            }
            // Anything but a whole number goes as typed, and the command's message names the fault.
            let typed = d.s("row").trim().to_string();
            let row = typed.parse::<u64>().map_or_else(|_| json!(typed), |n| json!(n));
            let p = json!({"story": d.fields.get("story"), "table": d.fields.get("table"), "section": d.s("section"), "row": row});
            let r = app.run("table.goToRow", p);
            match &r {
                Ok(v) => {
                    // The view scrolls the row to its middle, at the same zoom, on the row's page or another.
                    let spread = serde_json::from_value::<designcraft_doc::SpreadRef>(v["spread"].clone()).ok();
                    let bounds = v["rowBounds"].as_array().and_then(|a| {
                        let n: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
                        (n.len() == 4).then(|| designcraft_geom::Rect::new(n[0], n[1], n[2], n[3]))
                    });
                    if let (Some(spread), Some(b)) = (spread, bounds) {
                        crate::canvas::go_to_spread_rect(app, spread, b);
                    }
                }
                Err(e) => {
                    let mut d = d.clone();
                    d.fields.insert("status".into(), json!(e));
                    app.ui.dialog = Some(d);
                }
            }
            r
        }
        "insertPages" => {
            let n = app.session.active().map_or(1, |st| st.doc.page_count());
            // The page the menu shows: a page past the end is the last.
            let page = (d.n("page").unwrap_or(0.0).max(0.0) as usize).min(n.saturating_sub(1));
            let mut p = json!({"count": d.n("count").unwrap_or(1.0).clamp(1.0, 9999.0) as u64});
            match d.s("insert").as_str() {
                "before" => p["before"] = json!(page),
                "start" => p["before"] = json!(0),
                "end" => p["after"] = json!(n.saturating_sub(1)),
                _ => p["after"] = json!(page),
            }
            // A parent's id, or [None].
            p["parent"] = d.s("parent").parse::<u64>().map_or(Value::Null, |id| json!(id));
            app.run("layout.pages.insert", p)
        }
        "goToPage" => {
            // A page name ("iv", "A-3"), a number, or "+n" for an absolute position.
            let typed = match d.fields.get("page") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => "1".into(),
            };
            match app.session.resolve_page(&typed) {
                Some(abs) => {
                    crate::canvas::go_to_page(app, abs);
                    Ok(Value::Null)
                }
                None => Err(format!("no page \"{typed}\"")),
            }
        }
        "convertTextToTable" | "convertTableToText" => {
            let r = if d.id == "convertTextToTable" {
                let same = same_separators(&d);
                let style = table_style_value(&d);
                app.run(
                    "table.convertFromText",
                    json!({"columnSeparator": d.s("columnSeparator"), "rowSeparator": d.s("rowSeparator"),
                        "columns": if same { whole_number(d.fields.get("columns")) } else { Value::Null }, "tableStyle": style}),
                )
            } else {
                app.run("table.convertToText", json!({"columnSeparator": d.s("columnSeparator"), "rowSeparator": d.s("rowSeparator")}))
            };
            // On an error (an emptied separator field) the dialog stays open to be corrected.
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "insertTable" => {
            // The table and its style in one step; a style that is gone leaves the document alone,
            // and the dialog stays open to be corrected.
            let style = table_style_value(&d);
            let r = app.run(
                "table.insert",
                json!({"rows": d.n("bodyRows").unwrap_or(4.0).max(1.0) as u64, "cols": d.n("columns").unwrap_or(4.0).max(1.0) as u64,
                    "headerRows": d.n("headerRows").unwrap_or(0.0).max(0.0) as u64, "footerRows": d.n("footerRows").unwrap_or(0.0).max(0.0) as u64,
                    "tableStyle": style}),
            );
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "numberingSection" => {
            // On an error the dialog stays open to be corrected.
            let r = crate::section_options::confirm(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "cellOptions" => {
            // On an error the dialog stays open to be corrected.
            let r = confirm_cell_options(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "diagonalLines" => {
            // On an error the dialog stays open to be corrected, unless the document it opened
            // in was closed, reverted or edited.
            let r = confirm_diagonal_lines(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                if matches!(diagonal_lines_document(app, &d), DiagonalLinesDocument::Ready | DiagonalLinesDocument::Inactive) {
                    app.ui.dialog = Some(d);
                }
            }
            r
        }
        "textFrameOptions" => {
            // On an error the dialog stays open to be corrected.
            let mut d = d;
            let r = confirm_text_frame_options(app, &mut d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "documentSetup" => {
            // On an error the dialog stays open to be corrected.
            let r = confirm_document_setup(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "fileInfo" => {
            let r = confirm_file_info(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "paragraphStyleOptions" => {
            let mut d = d;
            let r = confirm_paragraph_style_options(app, &mut d);
            if let Err(e) = &r {
                // Keep the dialog open with the reason, on the section it concerns.
                if e.contains("bad GREP") {
                    d.fields.insert("section".into(), json!("grep"));
                }
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        _ => confirm_part2(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part2(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "deleteParagraphStyle" => {
            let name = d.s("name");
            let replace = d.s("replaceWith");
            app.run(
                "style.paragraph.delete",
                json!({"name": name, "replaceWith": if replace.is_empty() { json!(designcraft_doc::BASIC_PARAGRAPH) } else { json!(replace) }}),
            )
        }
        "paragraphLocal" => {
            let attrs: Map<String, Value> = d.fields.iter().filter_map(|(k, v)| k.strip_prefix("p.").map(|a| (a.to_string(), v.clone()))).collect();
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs}))
        }
        "hyphenation" => {
            // The preview goes; the edits are made once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let attrs = hyphenation_edits(&d);
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs})).inspect_err(|e| {
                // The dialog stays open, with the reason, to be corrected.
                app.status(e.clone());
                let mut d = d;
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            })
        }
        "justification" => {
            let current = d.fields.get("current").cloned().unwrap_or_default();
            if let Err(e) = check_justification(&d, "", &current) {
                // The dialog stays open to be corrected.
                app.status(e.clone());
                let mut d = d;
                d.fields.insert("justifyError".into(), json!(e));
                app.ui.dialog = Some(d);
                return Err(e);
            }
            let attrs: Map<String, Value> = JUSTIFY_ATTRS.iter().filter_map(|k| d.fields.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs}))
        }
        "paragraphRules" => {
            let attrs = rule_edits(&d, "");
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs}))
        }
        "characterStyleOptions" => {
            let name = d.s("name");
            let mut chars = Map::new();
            for (k, v) in &d.fields {
                if let Some(a) = k.strip_prefix("c.") {
                    chars.insert(a.into(), v.clone());
                }
            }
            let rename = d.s("rename").trim().to_string();
            let based = d.fields.get("basedOn").and_then(Value::as_str).map(str::to_string);
            let r = if d.b("new") {
                // A new style sets only what was filled in.
                chars.retain(|_, v| !v.is_null());
                let mut params = json!({"name": if rename.is_empty() { "Character Style 1" } else { rename.as_str() }, "chars": chars});
                if let Some(b) = based.filter(|b| b != designcraft_doc::NO_CHAR_STYLE) {
                    params["basedOn"] = json!(b);
                }
                // Apply Style to Selection.
                app.run("style.character.create", params).and_then(|r| {
                    if d.b("apply") && app.session.active().is_some_and(|st| st.selection.text.is_some()) {
                        app.run("style.character.apply", json!({"name": r["name"]}))?;
                    }
                    Ok(r)
                })
            } else {
                let mut params = json!({"name": name, "chars": chars});
                if let Some(b) = based {
                    params["basedOn"] = if b == designcraft_doc::NO_CHAR_STYLE { Value::Null } else { json!(b) };
                }
                if !rename.is_empty() && rename != name {
                    params["rename"] = json!(rename);
                }
                app.run("style.character.edit", params)
            };
            if let Err(e) = &r {
                // Keep the dialog open with the reason (a taken name, a based-on loop).
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                d.fields.insert("section".into(), json!("general"));
                app.ui.dialog = Some(d);
            }
            r
        }
        "deleteCharacterStyle" => {
            let name = d.s("name");
            let replace = d.s("replaceWith");
            app.run(
                "style.character.delete",
                json!({"name": name, "replaceWith": if replace.is_empty() { json!(designcraft_doc::NO_CHAR_STYLE) } else { json!(replace) }}),
            )
        }
        "swatchOptions" | "colorGroupOptions" | "layerSettings" => {
            let r = match d.id.as_str() {
                "swatchOptions" => options::confirm_swatch_options(app, &d),
                "colorGroupOptions" => options::confirm_color_group_options(app, &d),
                _ => options::confirm_layer_options(app, &d),
            };
            if let Err(e) = &r {
                // Keep the dialog open with the reason (a taken or empty name).
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        _ => confirm_part3(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part3(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        // Nothing edited: OK leaves the fill or stroke as it was (its swatch and tint).
        "colorPicker" if !d.b("edited") => Ok(Value::Null),
        "colorPicker" => {
            let color = if d.s("model") == "cmyk" {
                let [c, m, y, k] = nums::<4>(&d, "cmyk").ok_or("no colour")?.map(|v| v / 100.0);
                json!({"model": "cmyk", "c": c, "m": m, "y": y, "k": k})
            } else {
                let [r, g, b] = nums::<3>(&d, "rgb").ok_or("no colour")?.map(|v| v / 255.0);
                json!({"model": "rgb", "r": r, "g": g, "b": b})
            };
            app.run("object.color", json!({"color": color, "target": d.s("target"), "text": d.b("text")}))
        }
        "layerOptions" => {
            let hidden: Vec<Value> = d
                .fields
                .get("layers")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter(|l| l["visible"] == false).map(|l| l["name"].clone()).collect())
                .unwrap_or_default();
            // A dialog opened without the menu's fields has no `useDefaults` at all: confirming it
            // with no layer unticked resets file defaults, as it did before all-on existed, rather
            // than making every layer explicitly visible. Only an explicit `false` means all-on.
            let use_defaults = d.fields.get("useDefaults").and_then(Value::as_bool).unwrap_or(hidden.is_empty());
            let hidden = if use_defaults { Vec::new() } else { hidden };
            app.run("object.layerOptions", json!({"hidden": hidden, "useDefaults": use_defaults}))
        }
        "colorSettings" => {
            let r = app.run("color.settings", json!({"rgb": d.s("rgb"), "cmyk": d.s("cmyk"), "intent": d.s("intent"), "bpc": d.b("bpc")}))?;
            app.ui.color_settings = Some(designcraft_color::cms::active_settings());
            app.canvas.shown = None;
            Ok(r)
        }
        "contactSheet" => {
            let result = (|| {
                // A bare number means the ruler unit, as typed or as a JSON number (`Dialog::m`
                // takes the latter as points).
                let measure = |k: &str| parse_measure(&d.s(k), d.unit).ok();
                let params = json!({
                    "columns": d.s("columns").trim().parse::<u64>().map_err(|_| "Columns must be a whole number")?,
                    "rows": d.s("rows").trim().parse::<u64>().map_err(|_| "Rows must be a whole number")?,
                    "gutter": measure("gutter").ok_or("Enter a valid gutter")?,
                    "margin": measure("margin").ok_or("Enter a valid margin")?,
                });
                app.run("place.contactSheet", params)
            })();
            // A mistake keeps the dialog, with what was typed and why it can't be placed.
            if let Err(error) = &result {
                let mut d = d;
                d.fields.insert("status".into(), json!(error));
                app.ui.dialog = Some(d);
            }
            result
        }
        "pdfImport" => {
            let crop = d.s("crop");
            // One page number, 1 or more; a blank field is page 1. A range or list ("4-5", "2,5"), a
            // fraction or a negative is refused rather than placing some other page.
            let raw = if d.fields.get("page").is_none_or(Value::is_null) { String::new() } else { d.s("page") };
            let page = match raw.trim() {
                "" => 1,
                t => match t.parse::<u64>() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        let e = "Enter a single page number; one page is placed at a time".to_string();
                        let mut d = d;
                        d.fields.insert("status".into(), json!(e));
                        app.ui.dialog = Some(d);
                        return Err(e);
                    }
                },
            };
            if d.b("async") {
                return app.confirm_pdf_import(page, if crop.is_empty() { "crop".into() } else { crop }, d.b("append"));
            }
            if let Some(target) = d.fields.get("target").and_then(Value::as_u64) {
                app.activate_import_target(&crate::ImportRequest { purpose: "place".into(), target: Some(target) })?;
            }
            let placed = app.place_file(
                json!({"path": d.s("path"), "pdfPage": page, "pdfCrop": if crop.is_empty() { "crop".to_string() } else { crop }, "append": d.b("append")}),
            )?;
            // The files chosen after this PDF, which waited for its options.
            let rest: Vec<String> = d
                .fields
                .get(PDF_IMPORT_REST)
                .and_then(Value::as_array)
                .map(|paths| paths.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            app.place_paths(rest)?;
            Ok(placed)
        }
        "print" => {
            let pages = if d.s("range") == "range" { json!(d.s("pages")) } else { Value::Null };
            let printer = d.s("printer");
            let mut params = json!({"printer": if printer.is_empty() { Value::Null } else { json!(printer) }, "copies": d.n("copies").unwrap_or(1.0).max(1.0) as u64,
                "pages": pages, "spreads": d.b("spreads"), "marks": d.b("marks"), "bleed": d.b("bleed")});
            if d.s("range") == "layouts" {
                let chosen = d.fields.get("layouts").cloned().unwrap_or_else(|| json!([]));
                if chosen.as_array().is_some_and(Vec::is_empty) {
                    // Keep the dialog open, saying why.
                    let e = crate::i18n::tr(&app.ui.language, "Choose at least one layout.").to_string();
                    let mut d = d.clone();
                    d.fields.insert("status".into(), json!(e));
                    app.ui.dialog = Some(d);
                    return Err(e);
                }
                params["layouts"] = chosen;
                params["separateJobs"] = json!(d.b("separateJobs"));
            }
            let r = match app.run("file.print", params) {
                Ok(r) => r,
                Err(e) => {
                    // Keep the dialog (and the layouts chosen) open, saying why.
                    let mut d = d;
                    d.fields.insert("status".into(), json!(e));
                    app.ui.dialog = Some(d);
                    return Err(e);
                }
            };
            let printer = r["printer"].as_str().unwrap_or_else(|| crate::i18n::tr(&app.ui.language, "the default printer"));
            app.status(
                crate::i18n::tr(&app.ui.language, "Sent {count} page(s) to {printer}")
                    .replace("{count}", &r["pages"].to_string())
                    .replace("{printer}", printer),
            );
            Ok(r)
        }
        "epubExport" => {
            let split = epub_export_split(&d);
            let stored = app.session.active().map(|st| st.doc.settings.epub_split.clone());
            if stored.as_ref() != Some(&split) {
                let v = serde_json::to_value(&split).map_err(|e| e.to_string())?;
                app.run("document.epubSplit", json!({"split": v}))?;
            }
            // A cover image file named in the dialog; empty, the first page is the cover.
            let cover = d.s("coverImage");
            let cover = cover.trim();
            let params = if cover.is_empty() { json!({}) } else { json!({"coverImage": cover}) };
            crate::menus::export_epub(app, &params)
        }
        "pdfExport" => {
            // Remember the last-used options; `pages` and `title` are per-job and not persisted.
            app.ui.pdf_export.preset = d.s("preset");
            app.ui.pdf_export.standard = d.s("standard");
            app.ui.pdf_export.compress_images = d.b("compressImages");
            app.ui.pdf_export.flatten = d.s("flatten");
            app.ui.pdf_export.spreads = d.b("spreads");
            app.ui.pdf_export.bleed = d.b("bleed");
            app.ui.pdf_export.marks_crop = d.b("marksCrop");
            app.ui.pdf_export.marks_bleed = d.b("marksBleed");
            app.ui.pdf_export.marks_page_info = d.b("marksPageInfo");
            app.ui.pdf_export.marks_weight = d.s("marksWeight");
            app.ui.pdf_export.marks_offset = d.s("marksOffset");
            app.ui.pdf_export.tagged = d.b("tagged");

            let mut params = pdf_export_params(&d);
            let pages = d.s("pages");
            if !pages.trim().is_empty() && pages != "all" {
                params["pages"] = json!(pages);
            }
            let title = d.s("title");
            // Without a title the document's name names the file, and the PDF's title comes from
            // File Info.
            let doc_name = app.session.active().map(|st| st.doc.title.clone()).unwrap_or_default();
            if !title.is_empty() {
                params["title"] = json!(title);
            }
            let stem = if title.is_empty() { doc_name } else { title };
            let name = if stem.is_empty() { "Export.pdf".to_string() } else { format!("{stem}.pdf") };
            app.ask_save(&name, Bound::Document, move |app, path| crate::menus::export_pdf(app, &with_path(&params, "path", path)))
        }
        _ => confirm_part4(app, d),
    }
}

/// Color Settings › Load Profile…: load the chosen profile and list it in the dialog if it is
/// still open.
fn load_color_profile(app: &mut DesignApp, path: String) -> Result<Value, String> {
    if let Err(e) = app.session.execute("color.loadProfile", &json!({"path": path})) {
        app.status_error("Color Settings", e);
        return Ok(Value::Null);
    }
    // Replayed in load order at start-up, so the latest load of a path comes last.
    app.ui.color_profiles.retain(|p| p != &path);
    app.ui.color_profiles.push(path);
    let f = app.session.execute("color.settings", &json!({})).unwrap_or_default();
    if let Some(d) = app.ui.dialog.as_mut().filter(|d| d.id == "colorSettings") {
        d.fields.insert("profiles".into(), f["profiles"].clone());
    }
    Ok(Value::Null)
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part4(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "preferences" => {
            let mut prefs = json!({"scaleStrokes": d.b("scaleStrokes"), "dimensionsIncludeStroke": d.b("dimensionsIncludeStroke"), "transformationsAreTotals": d.b("transformationsAreTotals"), "absolutePageNumbers": d.b("absolutePageNumbers"), "highlightHj": d.b("highlightHj"), "highlightKeeps": d.b("highlightKeeps"), "highlightCustomTracking": d.b("highlightCustomTracking"), "highlightSubstitutedFonts": d.b("highlightSubstitutedFonts"), "richBlackOutput": d.b("richBlackOutput"), "typographersQuotes": d.b("typographersQuotes"), "showFontNamesInEnglish": d.b("showFontNamesInEnglish"), "smartTextReflow": d.b("smartTextReflow"),
                    "autocorrect": d.b("autocorrect"), "showAddedText": d.b("showAddedText"), "showNoteAnchors": d.b("showNoteAnchors"),
                    "recoveryMinutes": d.n("recoveryMinutes").unwrap_or(0.5), "linkTextFiles": d.b("linkTextFiles"),
                    "recentFilesMax": d.n("recentFilesMax").unwrap_or(20.0).clamp(0.0, f64::from(designcraft_engine::MAX_RECENT_FILES)) as u32,
                    "autocorrectList": d.s("autocorrectText").lines().filter_map(|l| {
                        let (a, b) = l.split_once('→').or_else(|| l.split_once("->"))?;
                        let (a, b) = (a.trim().to_lowercase(), b.trim().to_string());
                        (!a.is_empty() && !b.is_empty()).then(|| json!([a, b]))
                    }).collect::<Vec<_>>()});
            if d.b("cjkFeatures") != d.b("cjkFeatures.initial") {
                prefs["cjkFeatures"] = json!(d.b("cjkFeatures"));
            }
            // No document open: the Units & Increments new documents start with, saved with the
            // rest in one `prefs.set` (which checks everything before it changes anything). A value
            // that doesn't read, or that it refuses, keeps the dialog open to be fixed, with no
            // preference changed.
            let saved = default_units_of(&d).and_then(|du| {
                if let Some(du) = du {
                    prefs["defaultUnits"] = du;
                }
                app.run("prefs.set", prefs)
            });
            if let Err(e) = saved {
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.status(e.clone());
                app.ui.dialog = Some(d);
                return Err(e);
            }
            app.ui.dynamic_spelling = d.b("dynamicSpelling");
            if d.fields.contains_key("userWords") {
                let words: Vec<String> = d.s("userWords").lines().map(str::to_string).collect();
                app.run("spelling.setWords", json!({"words": words}))?;
            }
            // The slider's range. A size that isn't finite is saved as `null`, and ui.json then fails to load.
            if let Some(size) = d.n("storyEditorSize").filter(|v| v.is_finite()) {
                app.ui.story_editor_size = size.clamp(8.0, 36.0) as f32;
            }
            // Highlight options change the screen view only.
            app.canvas.shown = None;
            let view = match d.s("displayQuality").as_str() {
                "fast" => "view.fastDisplay",
                "typical" => "view.typicalDisplay",
                _ => "view.highQualityDisplay",
            };
            app.run(view, json!({}))?;
            app.run("window.richBlack", json!({"on": d.b("richBlack")}))?;
            // Interface: the theme (already showing, unless set without a click) and the Tools panel.
            if let Some(b) = d.fields.get("brightness").and_then(Value::as_str).filter(|b| *b != app.ui.brightness.id()) {
                app.run("window.brightness", json!({"brightness": b}))?;
            }
            if d.fields.contains_key("toolsColumns") {
                app.run("window.toolsDoubleColumn", json!({"on": d.s("toolsColumns") == "double"}))?;
            }
            if let Some(v) = d.n("uiScale") {
                app.run("window.uiScale", json!({"scale": v / 100.0}))?;
            }
            if let Some(on) = d.fields.get("autoCollapseIconicPanels").and_then(Value::as_bool) {
                app.ui.dock_auto_collapse = on;
            }
            if d.fields.contains_key("snap.alignEdges") {
                let mut snap = json!({
                    "alignEdges": d.b("snap.alignEdges"),
                    "alignCenters": d.b("snap.alignCenters"),
                    "smartDimensions": d.b("snap.dimensions"),
                    "smartSpacing": d.b("snap.spacing"),
                });
                if let Some(zone) = d.n("snap.zone") {
                    snap["zone"] = json!(zone);
                }
                app.run("view.snapPreferences", snap)?;
            }
            if d.b("defaults") {
                // The default units went with the preferences above.
                return Ok(Value::Null);
            }
            let Some(horizontal) = d.fields.get("horizontalUnits") else { return Ok(Value::Null) };
            let mut doc = json!({"horizontalUnits": horizontal});
            // Fields can come from `ui.dialog.open`; rulers it doesn't name keep their units.
            if let Some(vertical) = d.fields.get("verticalUnits") {
                doc["verticalUnits"] = vertical.clone();
            }
            if let Some(o) = d.fields.get("rulerOrigin").filter(|o| o.is_string()) {
                doc["rulerOrigin"] = o.clone();
            }
            let mut bg = json!({"relativeTo": d.s("bg.relativeTo"), "color": d.fields.get("bg.color").cloned().unwrap_or(json!([140, 205, 230]))});
            if let Some(v) = d.m("bg.start") {
                bg["start"] = json!(v.max(0.0));
            }
            if let Some(v) = d.m("bg.increment").filter(|v| *v > 0.0) {
                bg["increment"] = json!(v);
            }
            if let Some(v) = d.n("bg.viewThreshold") {
                bg["viewThreshold"] = json!((v / 100.0).clamp(0.05, 40.0));
            }
            doc["baselineGrid"] = bg;
            let mut grid = json!({"inBack": d.b("grid.inBack"), "color": d.fields.get("grid.color").cloned().unwrap_or(json!([200, 200, 200]))});
            for k in ["horizontal", "vertical"] {
                if let Some(v) = d.m(&format!("grid.{k}")).filter(|v| *v > 0.0) {
                    grid[k] = json!(v);
                }
            }
            if let Some(v) = d.n("grid.subdivisions") {
                grid["subdivisions"] = json!(v.clamp(1.0, 100.0) as u32);
            }
            doc["grid"] = grid;
            for k in ["marginColor", "columnColor", "bleedColor", "slugColor"] {
                if let Some(v) = d.fields.get(k) {
                    doc[k] = v.clone();
                }
            }
            if let (Some(h), Some(v)) = (d.m("pasteboard.h"), d.m("pasteboard.v")) {
                doc["pasteboard"] = json!([h.max(0.0), v.max(0.0)]);
            }
            if let Some(v) = d.fields.get("keyboardIncrement").and_then(Value::as_str).and_then(|s| parse_measure(s, Unit::Points).ok()) {
                doc["keyboardIncrement"] = json!(v.max(0.001));
            }
            if let Some(v) = d.n("kerningIncrement").filter(|v| v.is_finite()) {
                doc["kerningIncrement"] = json!(v.clamp(1.0, 1000.0));
            }
            let mut adv = json!({});
            for k in ["superscriptSize", "superscriptPosition", "subscriptSize", "subscriptPosition"] {
                if let Some(v) = d.n(&format!("adv.{k}")) {
                    adv[k] = json!(if k.ends_with("Size") { v.clamp(1.0, 200.0) } else { v.clamp(-500.0, 500.0) });
                }
            }
            doc["advancedType"] = adv;
            doc["overprintBlack"] = json!(d.b("overprintBlack"));
            doc["glyphFallback"] = json!(d.b("glyphFallback"));
            app.run("document.preferences", doc)
        }
        _ => confirm_part5(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part5(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "newWorkspace" => app.run(
            "window.newWorkspace",
            json!({"name": d.s("name"), "panelLocations": d.b("panelLocations"), "menuCustomization": d.b("menuCustomization")}),
        ),
        "deletePages" => {
            if d.b("dontShowAgain") {
                app.ui.skip_delete_pages_warning = true;
            }
            app.run("layout.pages.delete", json!({"pages": d.fields.get("pages").cloned().unwrap_or_else(|| json!([]))}))
        }
        "parentOptions" => {
            let base = d.s("basedOn");
            let base = base.parse::<u64>().map_or(Value::Null, |id| json!(id));
            let parent = d.fields.get("parent").cloned().unwrap_or(Value::Null);
            let r = app.run("layout.parents.options", json!({"parent": parent, "prefix": d.s("prefix"), "name": d.s("name"), "basedOn": base}));
            if let Err(e) = &r {
                // A refused prefix or name: the dialog stays, with what was typed, saying why.
                let mut d = d;
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        "pagesPanelOptions" => {
            use crate::panels::pages::{IconSize, PanelResize};
            let o = &mut app.ui.pages_panel;
            o.page_size = IconSize::from_key(&d.s("pageSize")).unwrap_or(o.page_size);
            o.page_thumbnails = d.b("pageThumbnails");
            o.parent_size = IconSize::from_key(&d.s("parentSize")).unwrap_or(o.parent_size);
            o.parent_thumbnails = d.b("parentThumbnails");
            o.parents_vertical = d.b("parentsVertical");
            o.pages_on_top = d.s("layout") == "pagesOnTop";
            o.resize = PanelResize::from_key(&d.s("resize")).unwrap_or(o.resize);
            Ok(Value::Null)
        }
        "updateLink" => crate::panels::properties::run_update_links(app, d.fields.get("target").cloned().unwrap_or_else(|| json!({}))),
        "closeDocument" => {
            // The document may have moved in the tab row (or gone) while the dialog was open.
            let uid = d.fields.get("uid").and_then(Value::as_u64);
            if d.b("quitting") {
                return app.confirm_quit_document(uid.ok_or("Missing document in quit confirmation")?, d.b("discard"));
            }
            let index = move |app: &DesignApp| app.session.documents().iter().position(|doc| Some(doc.uid) == uid);
            let Some(i) = index(app) else { return Ok(Value::Null) };
            let discard = d.b("discard");
            // Closes the document once it is saved (or at once, for Don't Save). Still unsaved, the
            // save failed: the document stays open.
            let close = move |app: &mut DesignApp| match index(app) {
                Some(i) if discard || app.session.documents().get(i).is_some_and(|doc| !doc.is_dirty()) => app.run("file.close", json!({"index": i})),
                _ => Ok(Value::Null),
            };
            if discard {
                return close(app);
            }
            app.run("file.activate", json!({"index": i}))?;
            crate::menus::save_then(app, close)
        }
        "loadStyles" => {
            let checked = |key: &str| -> Vec<&str> {
                let rows = d.fields.get(key).and_then(Value::as_array).into_iter().flatten();
                rows.filter(|row| row["on"] != false).filter_map(|row| row["name"].as_str()).collect()
            };
            app.run(
                "style.load",
                json!({"path": d.s("path"), "paragraph": checked("paragraph"), "character": checked("character"), "conflicts": d.s("conflicts")}),
            )
        }
        "importOptions" => app.open_file(
            "file.place",
            json!({"path": d.s("path"), "removeStyles": d.b("removeStyles"), "styleConflicts": d.s("styleConflicts"), "styleMap": d.fields.get("map").cloned().unwrap_or(json!({}))}),
        ),
        "fittingOptions" => {
            let m = |k: &str| d.m(k).unwrap_or(0.0);
            app.run(
                "object.fittingOptions",
                json!({"autoFit": d.b("autoFit"), "fitting": d.s("fitting"), "align": d.n("align").unwrap_or(4.0) as u64,
                    "crop": [m("cropTop"), m("cropLeft"), m("cropBottom"), m("cropRight")]}),
            )
        }
        "qrCode" => {
            let mut p = Value::Object(d.fields.clone());
            // Nothing selected: a 2-inch code at the top left of the page in view.
            if let Some(st) = app.session.active().filter(|s| s.selection.items.is_empty()) {
                let abs = crate::canvas::current_page(app).unwrap_or(0);
                let (si, pi) = st.doc.page_loc(abs).unwrap_or((0, 0));
                let x = st.doc.spreads.get(si).and_then(|sp| sp.pages.get(pi)).map_or(0.0, |pg| pg.x);
                p["rect"] = json!([x + 36.0, 36.0, x + 180.0, 180.0]);
                p["spread"] = json!(si);
            }
            app.run("object.qrCode", p)
        }
        "polygonSettings" => app.run("tool.polygonSettings", json!({"sides": d.n("sides").unwrap_or(6.0) as u64, "starInset": d.n("starInset").unwrap_or(0.0)})),
        "findFont" => {
            let result = app.run("font.replace", json!({"family": d.s("family"), "style": d.s("style"), "toFamily": d.s("toFamily"), "toStyle": d.s("toStyle")}));
            let mut d = d;
            match &result {
                Ok(_) => {
                    d.fields.remove("_fonts");
                    d.fields.remove("family");
                    d.fields.remove("style");
                    d.fields.remove("error");
                }
                Err(error) => { d.fields.insert("error".into(), json!(error)); }
            }
            app.ui.dialog = Some(d);
            result
        }
        _ => confirm_part6(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part6(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "insertXref" => {
            let format = d.s("format");
            let target = d.s("target");
            let params = match target.split_once(':') {
                Some(("a", id)) => json!({"anchor": id.parse::<u64>().unwrap_or(0), "format": format}),
                Some((sid, pi)) => json!({"story": sid.parse::<u64>().unwrap_or(0), "para": pi.parse::<u64>().unwrap_or(0), "format": format}),
                None => {
                    let mut d = d.clone();
                    d.fields.insert("status".into(), json!("Choose a destination paragraph or text anchor."));
                    app.ui.dialog = Some(d);
                    return Err("no destination chosen".into());
                }
            };
            app.run("xref.insert", params)
        }
        "footnoteOptions" => {
            let pt = |k: &str| d.pt(k).map_or(Value::Null, |v| json!(v));
            let mut p = json!({
                "style": d.s("style"), "startAt": d.n("startAt").unwrap_or(1.0).max(0.0) as u32, "restart": d.s("restart"),
                "prefix": d.s("prefix"), "suffix": d.s("suffix"), "affixIn": d.s("affixIn"), "refPosition": d.s("refPosition"),
                "refCharStyle": d.s("refCharStyle"), "paraStyle": d.s("paraStyle"),
                "separator": d.s("separator").replace("^t", "\t").replace("^m", "\u{2003}").replace("^>", "\u{2002}"),
                "spaceBefore": pt("spaceBefore"), "spaceBetween": pt("spaceBetween"), "firstBaseline": d.s("firstBaseline"),
                "firstBaselineMin": pt("firstBaselineMin"), "spanColumns": d.b("spanColumns"),
                "rule": {"on": d.b("rule.on"), "weight": pt("rule.weight"), "color": d.s("rule.color"), "width": pt("rule.width"),
                    "offset": pt("rule.offset"), "leftIndent": pt("rule.leftIndent")},
            });
            // Unparsable measures keep their current values.
            if let Some(o) = p.as_object_mut() {
                o.retain(|_, v| !v.is_null());
            }
            if let Some(r) = p.get_mut("rule").and_then(Value::as_object_mut) {
                r.retain(|_, v| !v.is_null());
            }
            app.run("footnote.options", p)
        }
        "alert" => Ok(Value::Null),
        crate::effects_dialog::ID => crate::effects_dialog::confirm(app, &d),
        crate::export_batch_dialog::ID => crate::export_batch_dialog::confirm(app, &d),
        crate::package_dialog::ID | crate::package_dialog::INSTRUCTIONS => {
            let r = if d.id == crate::package_dialog::ID {
                crate::package_dialog::confirm(app, &d)
            } else {
                crate::package_dialog::confirm_instructions(app, &d)
            };
            // Packaging failed: the dialog stays open with its options and instructions, and the
            // status line says why.
            if let Err(e) = &r
                && app.ui.dialog.is_none()
            {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        crate::object_style_dialog::ID => {
            let r = crate::object_style_dialog::confirm(app, &d);
            if let Err(e) = &r {
                // Keep the dialog and its edits open with the reason (an empty or taken name).
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        "findChange" => {
            let result = if d.b("objectMode") { Ok(Value::Null) } else { app.run("find.change", find_change_params(&d)) };
            // Confirmation consumes retained bounds before closing, even on an error.
            app.session.reset_find_scope();
            result
        }
        CORNER_OPTIONS => {
            // The preview goes; the corners change once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let mut d = d;
            seed_corner_options(app, &mut d);
            link_corners(&mut d);
            let r = corner_options_params(&d).and_then(|p| app.run("object.cornerOptions", p));
            if let Err(e) = &r {
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        STEP_AND_REPEAT => {
            // The preview goes; the copies are made once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let r = step_and_repeat_params(app, &d).and_then(|p| app.run("edit.stepAndRepeat", p));
            if let Err(e) = &r {
                let mut d = d.clone();
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        TOC => {
            let r = app.run("toc.generate", toc_params(app, &d));
            if let Err(e) = &r {
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        id if id.starts_with("cmd:") => {
            let cid = &id[4..];
            let doc = designcraft_engine::find_command(cid).map_or("", |c| c.params);
            let mut p = Map::new();
            for f in command_fields(doc) {
                match d.fields.get(&f.key) {
                    Some(Value::String(v)) if !v.trim().is_empty() => {
                        let v = v.trim();
                        // Numbers, booleans, arrays and objects as JSON; anything else is a string.
                        let parsed = serde_json::from_str::<Value>(v).ok().filter(|x| !x.is_string());
                        p.insert(f.key, parsed.unwrap_or_else(|| json!(v)));
                    }
                    Some(Value::String(_)) | None => {}
                    Some(value) => {
                        // Control/MCP callers can supply JSON directly. Preserve its type,
                        // including explicit null, for the command to interpret.
                        p.insert(f.key, value.clone());
                    }
                }
            }
            if cid == "layout.alternate.options" {
                drop_unchanged_size(&d, &mut p);
            }
            let r = app.run(cid, Value::Object(p));
            if let Err(e) = &r {
                // Keep the dialog open with the error.
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        other => Err(format!("unknown dialog {other}")),
    }
}

/// Why `p` is not a pattern the GREP styles can use, if it isn't. (Checked as the user types.)
fn grep_error(p: &str) -> Option<String> {
    if p.is_empty() {
        return None;
    }
    designcraft_compose::grep::Grep::new(p, Default::default()).err().map(|e| e.to_string())
}

/// What paragraph style `style` resolves to when based on `based` (the Based On chosen in its
/// dialog, which may not be its own yet): `based`'s chain with the style's own settings over it.
fn style_over(
    styles: &designcraft_doc::Styles,
    based: &str,
    style: &designcraft_doc::ParagraphStyle,
) -> (designcraft_doc::ParaProps, designcraft_doc::CharProps) {
    styles.resolve_para(&designcraft_doc::ParaFormat {
        style: based.into(),
        para: style.para.clone(),
        chars: style.chars.clone(),
        ..Default::default()
    })
}

/// The Justification spacing check of Paragraph Style Options, against what the section showed:
/// the style's own values, or a new style's Based On.
fn check_paragraph_style_justification(app: &DesignApp, d: &Dialog) -> Result<(), String> {
    // After Reset To Base the style starts from its Based On (the one chosen here, if any).
    let base_of = |st: &designcraft_engine::DocState| {
        let chosen = d.s("basedOn");
        if chosen.is_empty() { st.doc.styles.para(&d.s("name")).and_then(|s| s.based_on.clone()).unwrap_or_default() } else { chosen }
    };
    let base = app
        .session
        .active()
        .map(|st| {
            let pp = if d.b("new") {
                st.doc.styles.resolve_para_style(&d.s("basedOn")).0
            } else if d.b("resetToBase") {
                st.doc.styles.resolve_para_style(&base_of(st)).0
            } else {
                // The style's own settings over the Based On chosen in the dialog.
                match st.doc.styles.para(&d.s("name")) {
                    Some(style) => style_over(&st.doc.styles, &base_of(st), style).0,
                    None => st.doc.styles.resolve_para_style(&d.s("name")).0,
                }
            };
            serde_json::to_value(pp).unwrap_or_default()
        })
        .unwrap_or_default();
    check_justification(d, "p.", &base)
}

/// The `p.*` and `c.*` dialog fields as the paragraph and character attribute maps a style
/// command takes. `p.ruleAbove.weight` is one field of a rule (the command keeps the others).
fn style_option_maps(d: &Dialog) -> (serde_json::Map<String, Value>, serde_json::Map<String, Value>) {
    let mut para = serde_json::Map::new();
    let mut chars = serde_json::Map::new();
    for (k, v) in &d.fields {
        if let Some(a) = k.strip_prefix("p.") {
            match a.split_once('.') {
                Some((attr, field)) => {
                    if let Value::Object(m) = para.entry(attr).or_insert_with(|| json!({})) {
                        m.insert(field.into(), v.clone());
                    }
                }
                None => {
                    para.insert(a.into(), v.clone());
                }
            }
        } else if let Some(a) = k.strip_prefix("c.") {
            chars.insert(a.into(), v.clone());
        }
    }
    (para, chars)
}

fn confirm_paragraph_style_options(app: &mut DesignApp, d: &mut Dialog) -> Result<Value, String> {
    let name = d.s("name");
    if let Err(e) = check_paragraph_style_justification(app, d) {
        // The caller shows the reason in the dialog's status line.
        d.fields.insert("section".into(), json!("justify"));
        return Err(e);
    }
    let (para, chars) = style_option_maps(d);
    let based = d.s("basedOn");
    let rename = d.s("rename").trim().to_string();
    let next = d.fields.get("nextStyle").cloned();
    let (r, style) = if d.b("new") {
        let mut params = json!({"name": if rename.is_empty() { "Paragraph Style 1" } else { rename.as_str() }, "para": para, "chars": chars});
        if !based.is_empty() && based != designcraft_doc::NO_PARA_STYLE {
            params["basedOn"] = json!(based);
        }
        if let Some(Value::String(n)) = &next {
            params["nextStyle"] = json!(n);
        }
        let r = app.run("style.paragraph.create", params)?;
        let style = r["name"].as_str().unwrap_or_default().to_string();
        (r, style)
    } else {
        let mut params = json!({"name": name, "para": para, "chars": chars});
        if let Some(n) = next {
            params["nextStyle"] = n;
        }
        if d.b("resetToBase") {
            params["resetToBase"] = json!(true);
        }
        if !based.is_empty() {
            params["basedOn"] = if based == designcraft_doc::NO_PARA_STYLE { Value::Null } else { json!(based) };
        }
        let renaming = !rename.is_empty() && rename != name;
        if renaming {
            params["rename"] = json!(rename);
        }
        let r = app.run("style.paragraph.edit", params).inspect_err(|_| {
            // A refused name is shown next to the name field.
            if renaming {
                d.fields.insert("section".into(), json!("general"));
            }
        })?;
        (r, if renaming { rename } else { name })
    };
    // The style now exists under `style`: if a later step fails, the dialog stays open on it and
    // a second OK edits it rather than creating another one.
    let created = d.b("new");
    d.fields.insert("new".into(), json!(false));
    d.fields.insert("name".into(), json!(&style));
    d.fields.insert("rename".into(), json!(&style));
    // Apply Style to Selection.
    if created && d.b("apply") && app.session.active().is_some_and(|st| st.selection.text.is_some()) {
        app.run("style.paragraph.apply", json!({"name": style}))?;
    }
    if d.fields.contains_key("x.tag") {
        let tag = d.s("x.tag");
        let tag = if tag == "[Automatic]" { String::new() } else { tag };
        app.run("style.exportTag", json!({"style": style, "tag": tag, "class": d.s("x.class"), "split": d.b("x.split")}))?;
    }
    Ok(r)
}

/// Copies the dialog's `c.*` (and, for a paragraph style, `p.*`) fields into the preview params.
fn fill_style_preview_fields(params: &mut Value, d: &Dialog, paragraph: bool) {
    for (key, value) in &d.fields {
        if let Some(attr) = key.strip_prefix("c.") {
            params["chars"][attr] = value.clone();
        } else if paragraph && let Some(attr) = key.strip_prefix("p.") {
            if let Some((attr, field)) = attr.split_once('.') {
                if params["para"][attr].is_null() {
                    params["para"][attr] = json!({});
                }
                if let Some(fields) = params["para"][attr].as_object_mut() {
                    fields.insert(field.into(), value.clone());
                }
            } else {
                params["para"][attr] = value.clone();
            }
        }
    }
}

/// Preview the changed style attributes through the same engine commands as OK. Renaming and
/// export tagging have no visual effect and wait for OK, so the dialog can keep looking up the
/// original style while the preview is visible. The interaction restores its starting document
/// before each update, including after a field is cleared or Preview is switched off.
fn style_options_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") || d.b("new") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let paragraph = d.id == "paragraphStyleOptions";
    // Values OK would refuse don't reach the page either: the page goes back and the status
    // line says why.
    if paragraph && let Err(e) = check_paragraph_style_justification(app, d) {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
        }
        d.fields.insert("status".into(), json!(e));
        return;
    }
    let mut params = json!({"name": d.s("name"), "chars": {}});
    if paragraph {
        params["para"] = json!({});
    }
    fill_style_preview_fields(&mut params, d, paragraph);
    if let Some(based) = d.fields.get("basedOn") {
        params["basedOn"] = match based.as_str() {
            Some(designcraft_doc::NO_PARA_STYLE | designcraft_doc::NO_CHAR_STYLE) => Value::Null,
            _ => based.clone(),
        };
    }
    // Reset To Base shows on the page too.
    if paragraph && d.b("resetToBase") {
        params["resetToBase"] = json!(true);
    }
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(error) = app.session.begin_interaction("Style Options")
    {
        d.fields.insert("status".into(), json!(error.to_string()));
        return;
    }
    let command = if paragraph { "style.paragraph.edit" } else { "style.character.edit" };
    match app.session.preview(command, params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(error) => d.fields.insert("status".into(), json!(error.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// Show what the style changes from its parent, including edits waiting for OK. Fully captured
/// styles contain every default, so comparing resolved values keeps the summary readable.
fn paragraph_style_summary(lang: &str, d: &Dialog, style: &designcraft_doc::ParagraphStyle, styles: &designcraft_doc::Styles) -> String {
    let based = d.fields.get("basedOn").and_then(Value::as_str).or(style.based_on.as_deref()).unwrap_or(designcraft_doc::NO_PARA_STYLE);
    let (base_para, base_chars) = styles.resolve_para_style(based);
    let (mut para, mut chars) = (base_para.clone(), base_chars.clone());
    para.apply(&style.para);
    chars.apply(&style.chars);
    for (key, value) in &d.fields {
        if let Some(key) = key.strip_prefix("c.") {
            let mut delta = designcraft_doc::CharAttrs::default();
            if delta.set_json(key, value).is_ok() {
                chars.apply(&delta);
            }
        } else if let Some(key) = key.strip_prefix("p.") {
            let (key, value) = key.split_once('.').map_or((key, value.clone()), |(attr, field)| (attr, json!({field:value})));
            let mut delta = designcraft_doc::ParaAttrs::default();
            if delta.set_json_over(key, &value, &para).is_ok() {
                para.apply(&delta);
            }
        }
    }
    let mut char_delta = base_chars.delta(&chars);
    if base_chars.font_family != chars.font_family {
        char_delta.font_style = Some(chars.font_style.clone());
    }
    let mut parts = vec![crate::i18n::style_name(lang, based).to_string()];
    for value in [serde_json::to_value(base_para.delta(&para)), serde_json::to_value(char_delta)] {
        if let Ok(Value::Object(attrs)) = value {
            for (key, value) in attrs {
                let suffix = if matches!(
                    key.as_str(),
                    "size" | "spaceBefore" | "spaceAfter" | "leftIndent" | "rightIndent" | "firstLineIndent" | "lastLineIndent"
                ) {
                    " pt"
                } else {
                    ""
                };
                parts.push(format!("{}: {}{suffix}", setting_label(lang, &key), setting_text(&value)));
            }
        }
    }
    parts.join(" + ")
}

/// Whether the four values of `key` are linked ("Make all settings the same"): as the user left
/// the link, else when the four are the same, as InDesign starts it.
fn linked(d: &Dialog, key: &str, same: bool) -> bool {
    d.fields.get(&format!("_linked.{key}")).and_then(Value::as_bool).unwrap_or(same)
}

/// Number styles as the list's Format menu shows them (`NumberStyle` values).
const NUMBER_STYLES: [(&str, &str); 9] = [
    ("arabic", "1, 2, 3, 4..."),
    ("upperRoman", "I, II, III, IV..."),
    ("lowerRoman", "i, ii, iii, iv..."),
    ("upperLetters", "A, B, C, D..."),
    ("lowerLetters", "a, b, c, d..."),
    ("arabicLeadingZero", "01, 02, 03..."),
    ("arabicThreeDigits", "001, 002, 003..."),
    ("arabicFourDigits", "0001, 0002, 0003..."),
    ("symbols", "*, †, ‡, §..."),
];

/// What the Bullets and Numbering section offers besides the paragraph's attributes: the
/// document's character styles, the font families for the bullet, the fonts, and the paragraph's
/// font (the bullet's when it has none of its own).
struct ListUi<'a> {
    char_styles: &'a [String],
    families: &'a [String],
    fonts: &'a designcraft_fonts::ScopedFonts<'static>,
    scope: u32,
    family: &'a str,
    style: &'a str,
}

impl ListUi<'_> {
    fn new_parts(app: &DesignApp) -> (Vec<String>, Vec<String>, designcraft_fonts::ScopedFonts<'static>, u32) {
        let styles = app.session.active().map(|s| s.doc.styles.character.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        let families = crate::panels::font_menu(app).into_iter().map(|f| f.family).collect();
        (styles, families, crate::panels::fonts(app), crate::panels::font_scope(app))
    }
}

/// Preferences with no document open: the Units & Increments new documents start with, as
/// `prefs.set`'s `defaultUnits` (None with a document open). An increment that doesn't read is an
/// error naming the field.
fn default_units_of(d: &Dialog) -> Result<Option<Value>, String> {
    if !d.b("defaults") {
        return Ok(None);
    }
    let mut du = json!({});
    for k in ["horizontalUnits", "verticalUnits", "rulerOrigin"] {
        if let Some(v) = d.fields.get(k).filter(|v| v.is_string()) {
            du[k] = v.clone();
        }
    }
    if let Some(t) = d.fields.get("keyboardIncrement").and_then(Value::as_str) {
        du["keyboardIncrement"] = json!(parse_measure(t, Unit::Points).map_err(|_| format!("Cursor Key: `{t}` is not a measurement"))?);
    }
    if let Some(v) = d.fields.get("kerningIncrement").filter(|v| !v.is_null()) {
        let n = d.n("kerningIncrement").filter(|v| v.is_finite());
        du["kerningIncrement"] = json!(n.ok_or_else(|| format!("Kerning/Tracking: `{}` is not a number", v.as_str().unwrap_or_default()))?);
    }
    Ok(Some(du))
}

/// "Paragraph Style n" / "Character Style n": the first name no style has.
fn next_style_name(styles: &designcraft_doc::Styles, para: bool) -> String {
    let (base, count) = if para { ("Paragraph Style", styles.paragraph.len()) } else { ("Character Style", styles.character.len()) };
    let taken = |n: &str| if para { styles.para(n).is_some() } else { styles.char_style(n).is_some() };
    // Among count + 1 numbers at least one is free.
    (1..=count + 1).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap_or_else(|| format!("{base} {}", count + 1))
}

/// Paragraph or Character Style Options for an existing style (a double-click on it in the styles
/// panel, or Edit in its menu). [None] and [No Paragraph Style] have no options.
pub fn open_style_options(app: &mut DesignApp, para: bool, name: &str) {
    if name == if para { designcraft_doc::NO_PARA_STYLE } else { designcraft_doc::NO_CHAR_STYLE } {
        return;
    }
    let id = if para { "paragraphStyleOptions" } else { "characterStyleOptions" };
    app.ui.dialog = Some(Dialog::new(id, json!({"name": name, "preview": app.ui.style_preview})));
}

/// New Paragraph Style… / New Character Style…: the style options for a style made on OK, named
/// with the next free "Paragraph Style n" / "Character Style n".
pub fn open_new_style(app: &mut DesignApp, para: bool) {
    let Some(st) = app.session.active() else { return };
    let name = next_style_name(&st.doc.styles, para);
    let has_text = st.selection.text.is_some();
    let (bp, bc) = st.doc.styles.resolve_para_style(designcraft_doc::NO_PARA_STYLE);
    let id = if para { "paragraphStyleOptions" } else { "characterStyleOptions" };
    let mut fields = json!({"new": true, "rename": name});
    // New Paragraph Style with text selected starts from it, as InDesign's: Based On [No Paragraph
    // Style] and every setting of the text that differs, applied to the selection on OK.
    if para
        && has_text
        && let Some(sel) = app.session.execute("type.selectionAttrs", &json!({})).ok().filter(|v| !v.is_null())
    {
        for (prefix, now, base) in
            [("p.", &sel["para"], serde_json::to_value(bp).unwrap_or_default()), ("c.", &sel["chars"], serde_json::to_value(bc).unwrap_or_default())]
        {
            for (k, v) in now.as_object().into_iter().flatten() {
                if !v.is_null() && base.get(k) != Some(v) && base.get(k).is_some() {
                    fields[format!("{prefix}{k}")] = v.clone();
                }
            }
        }
        fields["apply"] = json!(true);
    }
    // New Character Style with text selected starts from what the text has of its own over its
    // paragraph ("[None] + Bold + color"), applied to the selection on OK, as InDesign's.
    if !para
        && has_text
        && let Some(sel) = app.session.execute("type.selectionAttrs", &json!({})).ok().filter(|v| !v.is_null())
    {
        for (k, v) in sel["ownChars"].as_object().into_iter().flatten().filter(|(_, v)| !v.is_null()) {
            fields[format!("c.{k}")] = v.clone();
        }
        fields["apply"] = json!(true);
    }
    app.ui.dialog = Some(Dialog::new(id, fields));
}

/// An attribute's name as Style Settings shows it: the field's label where the name reads poorly.
fn setting_label(lang: &str, key: &str) -> String {
    match key {
        "hScale" => crate::i18n::tr(lang, "Horizontal Scale:").trim_end_matches([':', '：']).to_string(),
        "vScale" => crate::i18n::tr(lang, "Vertical Scale:").trim_end_matches([':', '：']).to_string(),
        "capitalization" => crate::i18n::tr(lang, "Case:").trim_end_matches([':', '：']).to_string(),
        "otfFeatures" => crate::i18n::tr(lang, "OpenType Features").to_string(),
        _ => crate::i18n::tr(lang, &humanize(key)).to_string(),
    }
}

/// An attribute value as Style Settings shows it.
fn setting_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |x| format!("{}", (x * 1000.0).round() / 1000.0)),
        Value::Bool(b) => if *b { "on" } else { "off" }.into(),
        Value::Array(a) => a.iter().map(setting_text).collect::<Vec<_>>().join(", "),
        Value::Object(o) => o.get("value").or_else(|| o.get("kind")).map(setting_text).unwrap_or_default(),
        Value::Null => String::new(),
    }
}

/// Where the character sections of the style options dialogs get the values they show: `base`
/// holds them by attribute name (resolved values for a paragraph style; for a character style the
/// attributes it sets, the others absent). `sparse` (a character style) lets a field be blank:
/// emptying it, or picking the blank entry, unsets the attribute.
struct CharFields<'a> {
    base: &'a Value,
    sparse: bool,
}

impl CharFields<'_> {
    /// The value shown for attribute `key`: the dialog's edit, else the base value.
    fn get(&self, d: &Dialog, key: &str) -> Value {
        d.fields.get(&format!("c.{key}")).cloned().unwrap_or_else(|| self.base.get(key).cloned().unwrap_or(Value::Null))
    }
    fn set(&self, d: &mut Dialog, key: &str, v: Value) {
        d.fields.insert(format!("c.{key}"), v);
    }
}

const RULES: [(&str, &str); 2] = [("ruleAbove", "Rule Above"), ("ruleBelow", "Rule Below")];

/// The paragraph attributes of the Justification controls.
pub const JUSTIFY_ATTRS: [&str; 14] = [
    "wordSpaceMin",
    "wordSpaceDesired",
    "wordSpaceMax",
    "letterSpaceMin",
    "letterSpaceDesired",
    "letterSpaceMax",
    "glyphScaleMin",
    "glyphScaleDesired",
    "glyphScaleMax",
    "autoLeading",
    "singleWordJustify",
    "composer",
    "kashidas",
    "koreanCharBreaks",
];

/// The spacing rows of the Justification controls: (label, attribute stem, allowed percent range).
const JUSTIFY_ROWS: [(&str, &str, (f64, f64)); 3] = [
    ("Word Spacing", "wordSpace", (0.0, 1000.0)),
    ("Letter Spacing", "letterSpace", (-100.0, 500.0)),
    ("Glyph Scaling", "glyphScale", (50.0, 200.0)),
];

/// The allowed Auto Leading, in percent of the type size.
const AUTO_LEADING_RANGE: (f64, f64) = (0.0, 500.0);

/// The Single Word Justification choices: (attribute value, label).
const SINGLE_WORD_CHOICES: [(&str, &str); 4] =
    [("left", "Align Left"), ("center", "Align Center"), ("right", "Align Right"), ("fullyJustified", "Full Justify")];

/// Checks the edited spacing values: each within its range, and minimum ≤ desired ≤ maximum
/// (values left mixed are not compared); an edited Auto Leading within its range, and an edited
/// Single Word Justification one of its choices.
fn check_justification(d: &Dialog, prefix: &str, current: &Value) -> Result<(), String> {
    if let Some(v) = d.fields.get(&format!("{prefix}autoLeading")) {
        let (lo, hi) = AUTO_LEADING_RANGE;
        let pct = v.as_f64().map(|x| x * 100.0).filter(|x| x.is_finite()).ok_or("Justification: Auto Leading is not a number")?;
        if !(lo..=hi).contains(&pct) {
            return Err(format!("Justification: Auto Leading must be between {lo}% and {hi}%"));
        }
    }
    if let Some(v) = d.fields.get(&format!("{prefix}singleWordJustify"))
        && !SINGLE_WORD_CHOICES.iter().any(|(k, _)| v.as_str() == Some(k))
    {
        return Err(format!("Justification: Single Word Justification cannot be {v}"));
    }
    for (label, stem, (lo, hi)) in JUSTIFY_ROWS {
        let mut edited = false;
        let mut known = Vec::new();
        for suffix in ["Min", "Desired", "Max"] {
            let key = format!("{stem}{suffix}");
            let v = match d.fields.get(&format!("{prefix}{key}")) {
                Some(v) => {
                    edited = true;
                    let pct =
                        v.as_f64().map(|x| x * 100.0).filter(|x| x.is_finite()).ok_or_else(|| format!("Justification: {label} is not a number"))?;
                    // The fields show whole percents; compare what was shown.
                    if pct.round() < lo || pct.round() > hi {
                        return Err(format!("Justification: {label} must be between {lo}% and {hi}%"));
                    }
                    Some(pct)
                }
                None => current[key.as_str()].as_f64().map(|x| x * 100.0),
            };
            known.extend(v);
        }
        if edited && known.windows(2).any(|w| w[0] > w[1] + 1e-9) {
            return Err(format!("Justification: {label} needs Minimum ≤ Desired ≤ Maximum"));
        }
    }
    Ok(())
}

/// Paragraph attributes the Hyphenation Settings dialog sets.
pub const HYPH_ATTRS: [&str; 10] = [
    "hyphenate",
    "hyphMinWord",
    "hyphAfterFirst",
    "hyphBeforeLast",
    "hyphLimit",
    "hyphZone",
    "hyphWeight",
    "hyphCapitalized",
    "hyphLastWord",
    "hyphAcrossColumn",
];

/// The Hyphenation dialog's edits, as `type.para` attrs.
fn hyphenation_edits(d: &Dialog) -> Map<String, Value> {
    HYPH_ATTRS.iter().filter_map(|k| d.fields.get(*k).map(|v| (k.to_string(), v.clone()))).collect()
}

/// Preview on: the edits show on the page (redone when a field changes) without an undo step;
/// off, the paragraphs go back.
fn hyphenation_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
        }
        return;
    }
    let params = json!({"attrs": hyphenation_edits(d)});
    if shown.as_ref() == Some(&params) || d.fields.get(PREVIEW_FAILED) == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction("Hyphenation")
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview("type.para", params.clone()) {
        Ok(_) => {
            d.fields.remove("status");
            d.fields.remove(PREVIEW_FAILED);
            d.fields.insert(PREVIEWED.into(), params);
        }
        // Rejected settings: the paragraphs go back, and the reason shows until they change.
        Err(e) => {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.insert("status".into(), json!(e.to_string()));
            d.fields.insert(PREVIEW_FAILED.into(), params);
        }
    }
}

/// The hyphenation settings a preview was refused for (not tried again until they change).
const PREVIEW_FAILED: &str = "previewFailed";

/// The rule fields edited in the dialog, as `type.para` attrs: `{ruleAbove?: {…}, ruleBelow?: {…}}`.
fn rule_edits(d: &Dialog, prefix: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for (which, _) in RULES {
        let edited = format!("{prefix}{which}.");
        let fields: Map<String, Value> = d.fields.iter().filter_map(|(k, v)| k.strip_prefix(&edited).map(|f| (f.to_string(), v.clone()))).collect();
        if !fields.is_empty() {
            out.insert(which.into(), Value::Object(fields));
        }
    }
    out
}

/// The built-in stroke types a paragraph rule can have (Paragraph Rules › Type).
fn rule_types() -> Vec<designcraft_doc::StrokeType> {
    use designcraft_doc::StrokeType as T;
    vec![
        T::Solid,
        T::ThickThick,
        T::ThickThin,
        T::ThickThinThick,
        T::ThinThick,
        T::ThinThickThin,
        T::ThinThin,
        T::Dashed { pattern: vec![12.0, 4.0] },
        T::Dotted,
        T::Hashed,
        T::Wavy,
    ]
}

/// One parameter of a command, parsed from its params documentation
/// (`{name, type?: a|b|c, count?, flag?: bool, …}`).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandField {
    pub key: String,
    pub optional: bool,
    /// Enumerated values (`a|b|c`).
    pub choices: Vec<String>,
    pub boolean: bool,
    /// The documented value spec (shown as a hint).
    pub hint: String,
}

/// Parameters of a command from its documentation string: the top-level keys of the first `{…}`.
pub fn command_fields(doc: &str) -> Vec<CommandField> {
    let Some(start) = doc.find('{') else { return vec![] };
    let mut depth = 0i32;
    let mut end = doc.len();
    for (i, c) in doc[start..].char_indices() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &doc[start + 1..end];
    let mut parts = Vec::new();
    let (mut depth, mut from) = (0i32, 0usize);
    for (i, c) in body.char_indices() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&body[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[from..]);
    let ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !s.starts_with(|c: char| c.is_ascii_digit());
    let mut out = Vec::new();
    for part in parts {
        // `key`, `key?`, `key: spec` or `key (spec)`: the key is the leading word.
        let part = part.trim();
        let k_end = part.find(|c: char| c.is_whitespace() || c == ':' || c == '(').unwrap_or(part.len());
        let (k, rest) = part.split_at(k_end);
        let rest = rest.trim_start();
        let spec = match rest.strip_prefix(':') {
            Some(v) => v.trim(),
            None => rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')).unwrap_or(rest).trim(),
        };
        let optional = k.ends_with('?');
        let key = k.trim_end_matches('?');
        if !ident(key) || out.iter().any(|f: &CommandField| f.key == key) {
            continue;
        }
        let word = spec.split_whitespace().next().unwrap_or("");
        let choices: Vec<String> =
            // `name|index` says how the value may be written, not what it is: a layout, a page or a style named in a text field.
            if word.contains('|') && word.split('|').all(ident) && !matches!(word, "name|index" | "index|name") {
                word.split('|').map(str::to_string).collect()
            } else {
                vec![]
            };
        let boolean = spec.starts_with("bool") || spec.starts_with("true|false") || spec.starts_with("true") && spec.len() <= 5;
        out.push(CommandField { key: key.into(), optional, choices: if boolean { vec![] } else { choices }, boolean, hint: spec.into() });
    }
    out
}

fn humanize(key: &str) -> String {
    let mut s = String::new();
    for (i, c) in key.chars().enumerate() {
        if i == 0 {
            s.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            s.push(' ');
            s.extend(c.to_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

/// Alternate Layout Options: the size fields start at the layout's first page's size (`sizeWas`),
/// and one left as it was isn't sent: it would resize every page of the layout to it.
fn drop_unchanged_size(d: &Dialog, p: &mut Map<String, Value>) {
    let num = |v: Option<&Value>| match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    let was = d.fields.get("sizeWas").and_then(Value::as_array);
    for (k, key) in ["width", "height"].into_iter().enumerate() {
        let before = num(was.and_then(|a| a.get(k)));
        if num(p.get(key)).zip(before).is_some_and(|(now, before)| (now - before).abs() < 1e-6) {
            p.remove(key);
        }
    }
}

const STEP_AND_REPEAT: &str = "cmd:edit.stepAndRepeat";
pub(crate) const CORNER_OPTIONS: &str = "cmd:object.cornerOptions";
const TOC: &str = "cmd:toc.generate";

/// One included style of the Table of Contents dialog, as `toc.generate` takes it, with the
/// separator in the dialog's notation (`^t` a tab).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct TocRow {
    style: String,
    level: u8,
    entry_style: String,
    page_number: String,
    separator: String,
    /// Character styles of the page number and the separator; "" is [None].
    page_number_style: String,
    separator_style: String,
    sort: bool,
}

fn toc_separator_shown(s: &str) -> String {
    designcraft_doc::story::toc_separator_notation(s)
}

fn toc_separator_typed(s: &str) -> String {
    designcraft_doc::story::toc_separator_text(s)
}

/// Appends the special-character `code` (such as `^y`) to the separator field's notation. A
/// trailing unescaped caret is a literal caret: it's escaped first, so it doesn't swallow the code.
fn toc_separator_append(field: &mut String, code: &str) {
    if field.chars().rev().take_while(|&c| c == '^').count() % 2 == 1 {
        field.push('^');
    }
    field.push_str(code);
}

fn toc_rows(d: &Dialog) -> Vec<TocRow> {
    d.fields.get("included").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// Layout › Table of Contents…: prefilled from the document's table of contents when it has one.
pub fn open_toc(app: &mut DesignApp) {
    let Some(st) = app.session.active() else { return };
    let d = &st.doc;
    let styles: Vec<&str> = d.styles.paragraph.iter().map(|p| p.name.as_str()).filter(|n| *n != designcraft_doc::NO_PARA_STYLE).collect();
    let char_styles: Vec<&str> = d.styles.character.iter().map(|c| c.name.as_str()).filter(|n| *n != designcraft_doc::NO_CHAR_STYLE).collect();
    let toc = d.toc.as_ref().filter(|t| d.story(t.story).is_some());
    let rows: Vec<TocRow> = toc
        .map(|t| {
            t.entries
                .iter()
                .map(|e| TocRow {
                    style: e.style.clone(),
                    level: e.level,
                    entry_style: e.entry_style.clone().unwrap_or_default(),
                    page_number: match (t.page_numbers, e.page_number) {
                        (false, _) | (_, designcraft_doc::TocPageNumber::None) => "none",
                        (_, designcraft_doc::TocPageNumber::Before) => "before",
                        _ => "after",
                    }
                    .into(),
                    separator: toc_separator_shown(e.separator.as_deref().unwrap_or("\t")),
                    page_number_style: e.page_number_style.clone().unwrap_or_default(),
                    separator_style: e.separator_style.clone().unwrap_or_default(),
                    sort: e.sort,
                })
                .collect()
        })
        .unwrap_or_default();
    let fields = json!({
        "title": toc.map_or("Contents", |t| t.title.as_str()),
        "titleStyle": toc.and_then(|t| t.title_style.clone()).unwrap_or_default(),
        "included": rows,
        "selected": 0,
        "styles": styles,
        "charStyles": char_styles,
        "hasToc": toc.is_some(),
        "replace": toc.is_some(),
        "createBookmarks": toc.is_none_or(|t| t.create_bookmarks),
    });
    app.ui.dialog = Some(Dialog::new(TOC, fields));
}

/// The dialog's fields as `toc.generate` params. A new frame goes on the page in view.
fn toc_params(app: &DesignApp, d: &Dialog) -> Value {
    let entries: Vec<Value> = toc_rows(d)
        .iter()
        .map(|r| {
            let mut e = json!({"style": r.style, "level": r.level, "separator": toc_separator_typed(&r.separator)});
            if !r.entry_style.is_empty() {
                e["entryStyle"] = json!(r.entry_style);
            }
            if !r.page_number.is_empty() {
                e["pageNumber"] = json!(r.page_number);
            }
            if !r.page_number_style.is_empty() {
                e["pageNumberStyle"] = json!(r.page_number_style);
            }
            if !r.separator_style.is_empty() {
                e["separatorStyle"] = json!(r.separator_style);
            }
            if r.sort {
                e["sort"] = json!(true);
            }
            e
        })
        .collect();
    let mut p = json!({"title": d.s("title"), "entries": entries, "replace": d.b("replace"), "createBookmarks": d.b("createBookmarks")});
    let title_style = d.s("titleStyle");
    if !title_style.is_empty() {
        p["titleStyle"] = json!(title_style);
    }
    if let Some(page) = crate::canvas::current_page(app) {
        p["page"] = json!(page + 1);
    }
    p
}

/// The params a previewing dialog shows on the page (its interaction is open while it is set).
const PREVIEWED: &str = "_previewed";
/// The uid of the document a Text Frame Options preview is shown in.
const PREVIEW_DOC: &str = "_previewDoc";

/// `edit.stepAndRepeat`'s params from the dialog: whole counts, offsets in the ruler units
/// (or any unit typed: "-0.75 in", "3p"). Empty fields are 1 copy and 1p0.
fn step_and_repeat_params(app: &DesignApp, d: &Dialog) -> Result<Value, String> {
    let (hu, vu) = app.session.active().map_or((Unit::Picas, Unit::Picas), |s| (s.doc.settings.horizontal_units, s.doc.settings.vertical_units));
    let invalid = |s: &str| format!("{}: {}", crate::i18n::tr(&app.ui.language, "Invalid value"), s.trim());
    // Fields not shown yet (a dialog confirmed over the control channel) take the defaults.
    let whole = |k: &str| {
        let s = d.s(k);
        if s.trim().is_empty() { Ok(1) } else { s.trim().parse::<u64>().map_err(|_| invalid(&s)) }
    };
    let measure = |k: &str, u: Unit| {
        let s = d.s(k);
        if s.trim().is_empty() {
            return Ok(12.0);
        }
        parse_measure(&s, u).ok().filter(|v| v.is_finite()).ok_or_else(|| invalid(&s))
    };
    let (dx, dy) = (measure("horizontal", hu)?, measure("vertical", vu)?);
    Ok(if d.b("grid") {
        json!({"rows": whole("rows")?, "columns": whole("columns")?, "dx": dx, "dy": dy})
    } else {
        json!({"count": whole("count")?, "dx": dx, "dy": dy})
    })
}

/// Preview on: the copies show on the page (redone when a field changes); off: they go.
fn step_and_repeat_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let params = match step_and_repeat_params(app, d) {
        Ok(p) => p,
        Err(e) => {
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction("Step and Repeat")
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview("edit.stepAndRepeat", params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// The corners of Object › Corner Options, in `object.cornerOptions` path order (top-left,
/// top-right, bottom-right, bottom-left), with the tip InDesign gives each.
const CORNERS: [(&str, &str); 4] = [("0", "Top Left:"), ("1", "Top Right:"), ("2", "Bottom Right:"), ("3", "Bottom Left:")];

/// The Corner Options dialog's fields from the first selected object's corners, once: each
/// corner's size (in the ruler units) and shape, linked when all four are the same.
fn seed_corner_options(app: &DesignApp, d: &mut Dialog) {
    use designcraft_geom::corners::Corner;
    let Some(st) = app.session.active() else { return };
    d.unit = st.doc.settings.horizontal_units;
    if d.fields.contains_key("_seeded") {
        return;
    }
    let corners: [Corner; 4] = st.selection.items.iter().find_map(|id| st.doc.item(*id)).map(|it| it.corners.corners).unwrap_or_default();
    for ((k, _), c) in CORNERS.iter().zip(corners) {
        d.fields.entry(format!("size{k}")).or_insert(json!(format_measure(c.size, d.unit)));
        d.fields.entry(format!("shape{k}")).or_insert(serde_json::to_value(c.shape).unwrap_or(json!("none")));
    }
    let first = corners[0];
    d.fields.entry("link").or_insert(json!(corners.iter().all(|c| *c == first)));
    d.fields.insert("_seed".into(), json!(corner_fields(d)));
    d.fields.insert("_seeded".into(), json!(true));
}

/// Each corner's size and shape fields, in `CORNERS` order.
fn corner_fields(d: &Dialog) -> Vec<(String, String)> {
    CORNERS.iter().map(|(k, _)| (d.s(&format!("size{k}")), d.s(&format!("shape{k}")))).collect()
}

/// The corners edited since the dialog opened: changed from the first object's, or set by a
/// linked edit (all four, even one that was already the value).
fn edited_corners(d: &Dialog) -> [bool; 4] {
    let touched: [bool; 4] = d.fields.get("_touched").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let seed: Vec<(String, String)> = d.fields.get("_seed").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let now = corner_fields(d);
    std::array::from_fn(|i| touched[i] || seed.get(i) != now.get(i))
}

/// Linked corners: a corner changed since the dialog last looked (size or shape, typed or set
/// over the control channel) is all four.
fn link_corners(d: &mut Dialog) {
    let now = corner_fields(d);
    let seen: Vec<(String, String)> = d.fields.get("_corners").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_else(|| now.clone());
    let mut now = now;
    let mut touched: [bool; 4] = d.fields.get("_touched").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    for (t, (a, b)) in touched.iter_mut().zip(now.iter().zip(&seen)) {
        *t |= a != b;
    }
    if d.b("link")
        && let Some(i) = now.iter().zip(&seen).position(|(a, b)| a != b)
        && let Some(changed) = now.get(i).cloned()
    {
        // The exact points go with the text they were shown as.
        let exact = CORNERS.get(i).and_then(|(k, _)| d.fields.get(&format!("_pt.size{k}")).cloned());
        for (k, _) in &CORNERS {
            d.fields.insert(format!("size{k}"), json!(changed.0));
            d.fields.insert(format!("shape{k}"), json!(changed.1));
            match &exact {
                Some(points) => d.fields.insert(format!("_pt.size{k}"), points.clone()),
                None => d.fields.remove(&format!("_pt.size{k}")),
            };
        }
        now = vec![changed; CORNERS.len()];
        touched = [true; 4];
    }
    d.fields.insert("_corners".into(), json!(now));
    d.fields.insert("_touched".into(), json!(touched));
}

/// `object.cornerOptions`' params from the dialog: the corners edited, the others null (each
/// selected object keeps its own). A size that does not parse, or a shape the dialog doesn't
/// offer, is an error to show.
fn corner_options_params(d: &Dialog) -> Result<Value, String> {
    use designcraft_geom::corners::CornerShape;
    let edited = edited_corners(d);
    let mut corners = Vec::with_capacity(CORNERS.len());
    for ((k, _), edited) in CORNERS.iter().zip(edited) {
        let key = format!("size{k}");
        let size =
            d.m(&key).filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| format!("Enter a valid corner size, e.g. 12 or 3 mm: {}", d.s(&key)))?;
        let shape: CornerShape = match d.fields.get(&format!("shape{k}")) {
            None => CornerShape::default(),
            Some(v) => serde_json::from_value(v.clone()).map_err(|_| format!("Choose a corner shape: {v} isn't one"))?,
        };
        corners.push(if edited { json!({"shape": shape, "size": size}) } else { Value::Null });
    }
    Ok(json!({"corners": corners}))
}

/// Preview on: the corners show on the frame (redone when a field changes); off: they go back.
fn corner_options_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let params = match corner_options_params(d) {
        Ok(p) => p,
        Err(e) => {
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction("Corner Options")
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview("object.cornerOptions", params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// `N` numbers from a dialog field holding a JSON array.
fn nums<const N: usize>(d: &Dialog, k: &str) -> Option<[f64; N]> {
    let a = d.fields.get(k)?.as_array()?;
    if a.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (o, v) in out.iter_mut().zip(a) {
        *o = v.as_f64().filter(|x| x.is_finite())?;
    }
    Some(out)
}

#[cfg(test)]
mod tests;

/// The Color Picker's components: the one picked runs along the slider, the other two of its
/// space across the field (InDesign's, as Photoshop's: R puts B across and G up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickComp {
    H,
    S,
    Br,
    L,
    A,
    Lb,
    R,
    G,
    Bl,
}

impl PickComp {
    const ALL: [PickComp; 9] =
        [PickComp::H, PickComp::S, PickComp::Br, PickComp::L, PickComp::A, PickComp::Lb, PickComp::R, PickComp::G, PickComp::Bl];

    fn key(self) -> &'static str {
        match self {
            PickComp::H => "h",
            PickComp::S => "s",
            PickComp::Br => "b",
            PickComp::L => "l",
            PickComp::A => "la",
            PickComp::Lb => "lb",
            PickComp::R => "r",
            PickComp::G => "g",
            PickComp::Bl => "bl",
        }
    }

    fn from_key(k: &str) -> PickComp {
        PickComp::ALL.into_iter().find(|c| c.key() == k).unwrap_or(PickComp::R)
    }

    fn lab(self) -> bool {
        matches!(self, PickComp::L | PickComp::A | PickComp::Lb)
    }
}

/// The colour the picker shows, in each of its spaces (HSB keeps its hue through greys).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Picked {
    pub rgb: [f32; 3],
    pub hsb: [f32; 3],
    pub lab: [f32; 3],
}

impl Picked {
    pub fn from_rgb(rgb: [f32; 3], hue: f32) -> Picked {
        let rgb = rgb.map(|v| v.clamp(0.0, 1.0));
        let c = designcraft_color::Color::Rgb { r: rgb[0], g: rgb[1], b: rgb[2] };
        let mut hsb = c.to_hsb();
        if hsb[1] <= 1e-4 || hsb[2] <= 1e-4 {
            hsb[0] = hue;
        }
        let l = designcraft_color::cms::lab::srgb_to_lab(rgb);
        Picked { rgb, hsb, lab: [l.l, l.a, l.b] }
    }

    fn from_hsb(hsb: [f32; 3]) -> Picked {
        let hsb = [hsb[0].rem_euclid(360.0), hsb[1].clamp(0.0, 1.0), hsb[2].clamp(0.0, 1.0)];
        let mut p = Picked::from_rgb(designcraft_color::Color::from_hsb(hsb[0], hsb[1], hsb[2]).to_rgb_uncalibrated(), hsb[0]);
        p.hsb = hsb;
        p
    }

    fn from_lab(lab: [f32; 3], hue: f32) -> Picked {
        let lab = [lab[0].clamp(0.0, 100.0), lab[1].clamp(-128.0, 127.0), lab[2].clamp(-128.0, 127.0)];
        let mut p = Picked::from_rgb(designcraft_color::cms::lab::lab_to_srgb(designcraft_color::cms::Lab::new(lab[0], lab[1], lab[2])), hue);
        p.lab = lab;
        p
    }

    /// (slider, across, up), each 0..1, for component `c`.
    pub fn coords(&self, c: PickComp) -> (f32, f32, f32) {
        let [r, g, b] = self.rgb;
        let [h, s, v] = self.hsb;
        let [l, la, lb] = self.lab;
        let (a, bb) = ((la + 128.0) / 255.0, (lb + 128.0) / 255.0);
        match c {
            PickComp::R => (r, b, g),
            PickComp::G => (g, b, r),
            PickComp::Bl => (b, r, g),
            PickComp::H => (h / 360.0, s, v),
            PickComp::S => (s, h / 360.0, v),
            PickComp::Br => (v, h / 360.0, s),
            PickComp::L => (l / 100.0, a, bb),
            PickComp::A => (a, bb, l / 100.0),
            PickComp::Lb => (bb, a, l / 100.0),
        }
    }

    /// The colour at (slider `t`, across `u`, up `v`) for component `c`.
    pub fn at(&self, c: PickComp, t: f32, u: f32, v: f32) -> Picked {
        let hue = self.hsb[0];
        let ab = |x: f32| x * 255.0 - 128.0;
        match c {
            PickComp::R => Picked::from_rgb([t, v, u], hue),
            PickComp::G => Picked::from_rgb([v, t, u], hue),
            PickComp::Bl => Picked::from_rgb([u, v, t], hue),
            PickComp::H => Picked::from_hsb([t * 360.0, u, v]),
            PickComp::S => Picked::from_hsb([u * 360.0, t, v]),
            PickComp::Br => Picked::from_hsb([u * 360.0, v, t]),
            PickComp::L => Picked::from_lab([t * 100.0, ab(u), ab(v)], hue),
            PickComp::A => Picked::from_lab([v * 100.0, ab(t), ab(u)], hue),
            PickComp::Lb => Picked::from_lab([v * 100.0, ab(u), ab(t)], hue),
        }
    }

    pub fn hex(&self) -> String {
        let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", q(self.rgb[0]), q(self.rgb[1]), q(self.rgb[2]))
    }
}

/// The colour in the dialog's fields: `hex`, with the HSB hue and the Lab values kept while
/// they round-trip (`_hsb`, `_lab`).
fn picked_of(d: &Dialog) -> Picked {
    let mut rgb = designcraft_color::Color::from_hex(&d.s("hex")).map_or([0.0; 3], |c| c.to_rgb_uncalibrated());
    if let Some(raw) = nums::<3>(d, "rgb") {
        let raw = raw.map(|x| (x / 255.0) as f32);
        if Picked::from_rgb(raw, 0.0).hex() == Picked::from_rgb(rgb, 0.0).hex() {
            rgb = raw;
        }
    }
    let hue = nums::<3>(d, "_hsb").map_or(0.0, |h| h[0] as f32);
    let mut p = Picked::from_rgb(rgb, hue);
    // The exact values the picker last set, while they still give this colour.
    if let Some(h) = nums::<3>(d, "_hsb").map(|h| h.map(|v| v as f32))
        && Picked::from_hsb(h).hex() == p.hex()
    {
        p.hsb = h;
    }
    if let Some(l) = nums::<3>(d, "_lab").map(|l| l.map(|v| v as f32))
        && Picked::from_lab(l, p.hsb[0]).hex() == p.hex()
    {
        p.lab = l;
    }
    p
}

fn store_picked(d: &mut Dialog, p: &Picked) {
    d.fields.insert("edited".into(), json!(true));
    d.fields.insert("rgb".into(), json!(p.rgb.map(|x| x * 255.0)));
    d.fields.insert(
        "cmyk".into(),
        json!(
            designcraft_color::Color::Rgb { r: p.rgb[0], g: p.rgb[1], b: p.rgb[2] }
                .to_cmyk_managed(designcraft_color::cms::active_settings().intent)
                .map(|x| x * 100.0)
        ),
    );
    d.fields.insert("hex".into(), json!(p.hex()));
    d.fields.insert("_hsb".into(), json!(p.hsb));
    d.fields.insert("_lab".into(), json!(p.lab));
}






