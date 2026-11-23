//! Successors, on the framework forms, of the native dialog tests the wave 4 cleanup parked (`native_tests_pending.rs`): the
//! Justification dialog's Auto Leading and Single Word Justification, the Kenten and composition settings of the style dialogs,
//! and the font style select's named instances. They run the real dialog host over an installed framework (`table_layout_kit`).
//!
//! The two checks of the old font style tests that need a font database of their own (a closed popup does not preload a catalogued font; a
//! missing family is not retried every frame, but reopening, hiding or a new scope retries it) are in `panels/successor_tests.rs`, over
//! `framework_transform::font_style_select_in`, which takes the database as a parameter.

use super::table_layout_kit as kit;
use super::*;
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use serde_json::{Value, json};

/// The Auto Leading stored for a percent typed into its field: the whole percent the field shows.
fn auto_leading_typed(pct: f64) -> f64 {
    // Adding 0.0 turns a rounded -0 into 0.
    (pct.round() + 0.0) / 100.0
}

fn st<'a>(h: &'a mut egui_kittest::Harness<'static, DesignApp>) -> &'a mut DesignApp {
    h.state_mut()
}

/// Successor of `justification_dialog_sets_auto_leading_and_single_word_justification`: the Justification dialog shows Auto Leading
/// and Single Word Justification, OK sets them on the selected paragraph as one undo step, bad values keep the dialog open with
/// the reason, and both values survive saving and opening the native file and an IDML export.
#[test]
fn justification_form_sets_auto_leading_and_single_word_justification() {
    let (mut app, story) = kit::two_paragraphs();
    app.run("app.justificationDialog", json!({})).unwrap();
    let d = app.ui.dialog.clone().unwrap();
    let current = d.fields["current"].clone();
    assert_eq!((current["autoLeading"].clone(), current["singleWordJustify"].clone()), (json!(1.2), json!("fullyJustified")));
    let mut h = kit::harness_sized(app, vec2(1000.0, 800.0));
    assert!(kit::shows(&h, "Auto Leading:") && kit::shows(&h, "Single Word Justification:"));
    // The dropdown shows the paragraph's Full Justify. Picking Full Justify again is no edit.
    kit::pick(&mut h, "Single Word Justification:", "Full Justify");
    assert!(!kit::draft(&h).fields.contains_key("singleWordJustify"), "the paragraph's own choice is no edit");
    // Pick Align Center.
    kit::pick(&mut h, "Single Word Justification:", "Align Center");
    assert_eq!(kit::draft(&h).fields["singleWordJustify"], "center", "the choice is the edit");

    // Out of range or not a choice: an error, the dialog stays open, nothing changes.
    let d = kit::draft(&h).clone();
    let before = kit::undo_steps(&h);
    let bad = [
        ("autoLeading", json!(5.01)),
        ("autoLeading", json!(-0.01)),
        ("autoLeading", json!(f64::NAN)),
        ("singleWordJustify", json!("leftJustified")),
    ];
    for (key, v) in bad {
        st(&mut h).ui.dialog = Some(d.clone());
        st(&mut h).ui.dialog.as_mut().unwrap().fields.insert(key.into(), v.clone());
        assert!(confirm(st(&mut h)).is_err(), "{key} = {v}");
        assert!(st(&mut h).ui.dialog.as_ref().is_some_and(|d| d.fields.contains_key("justifyError")), "{key}: the dialog shows why");
    }
    assert_eq!(kit::undo_steps(&h), before);

    // A typed percent is stored as the whole percent the field shows, so 500.4% (shown as 500%) and -0.4% (shown as 0%) are accepted.
    for (typed, stored) in [(500.4, 5.0), (-0.4, 0.0), (149.6, 1.5)] {
        assert_eq!(auto_leading_typed(typed), stored, "{typed}%");
        let mut probe = d.clone();
        probe.fields.insert("autoLeading".into(), json!(auto_leading_typed(typed)));
        assert_eq!(check_justification(&probe, "", &current), Ok(()), "{typed}%");
    }

    st(&mut h).ui.dialog = Some(d);
    st(&mut h).ui.dialog.as_mut().unwrap().fields.insert("autoLeading".into(), json!(1.5));
    kit::steps(&mut h, 3);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(kit::undo_steps(&h), before + 1, "one undo step");
    let story_value = json!(story);
    let mut both = |h: &mut egui_kittest::Harness<'static, DesignApp>, story: &Value, at: usize| {
        let story = story.as_u64().unwrap();
        (kit::para_attr(h, story, at, "autoLeading"), kit::para_attr(h, story, at, "singleWordJustify"))
    };
    assert_eq!(both(&mut h, &story_value, 0), (json!(1.5), json!("center")));
    assert_eq!(both(&mut h, &story_value, 4), (json!(1.2), json!("fullyJustified")), "only the selected paragraph");

    // Both paragraphs selected: the fields that differ show blank, and OK with no edit adds no undo step.
    st(&mut h).run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
    kit::reopen(&mut h, "app.justificationDialog");
    let current = kit::draft(&h).fields["current"].clone();
    assert_eq!((current["autoLeading"].clone(), current["singleWordJustify"].clone()), (Value::Null, Value::Null));
    let before = kit::undo_steps(&h);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert_eq!(kit::undo_steps(&h), before);

    // Saved and opened again, natively and through IDML.
    let native = st(&mut h).run("file.serialize", json!({})).unwrap();
    st(&mut h).run("file.openBytes", json!({"name": "justify.designcraft", "base64": native["base64"]})).unwrap();
    assert_eq!(both(&mut h, &story_value, 0), (json!(1.5), json!("center")));
    let idml = st(&mut h).run("file.exportIdml", json!({})).unwrap();
    st(&mut h).run("file.openBytes", json!({"name": "justify.idml", "base64": idml["base64"]})).unwrap();
    let doc = &h.state().session.doc().unwrap().doc;
    let sid = doc.stories.iter().find(|(_, s)| s.text == "One\nTwo").map(|(id, _)| *id).expect("the story comes back");
    let story = serde_json::to_value(sid).unwrap();
    assert_eq!(both(&mut h, &story, 0), (json!(1.5), json!("center")));
    assert_eq!(both(&mut h, &story, 4), (json!(1.2), json!("fullyJustified")));
}

/// A document with paragraph style `Body` applied to two characters of text.
fn body_style_app() -> DesignApp {
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name": "Body"})).unwrap();
    app.run("style.character.create", json!({"name": "Emphasis", "chars": {"fontStyle": "Italic"}})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "漢字かな"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 2})).unwrap();
    app.run("style.paragraph.apply", json!({"name": "Body"})).unwrap();
    app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
    app
}

/// Successor of `kenten_and_japanese_composition_reach_the_style_and_its_paragraph`: Kenten, the Kinsoku set, the Mojikumi set and
/// the hanging type reach the paragraph style and the paragraph that follows it, Default Rules replaces the set, and a character
/// style sets only the kenten attributes.
#[test]
fn style_dialogs_kenten_and_japanese_composition_reach_the_style_and_its_paragraph() {
    use designcraft_doc::cjk_settings::KentenKind;
    let mut app = body_style_app();
    if let Some(st) = app.session.active_mut() {
        let table = designcraft_doc::cjk::MojikumiTable { name: "Body Text".into(), ..Default::default() };
        std::sync::Arc::make_mut(&mut st.doc).styles_mut().mojikumi_tables.push(table);
    }
    let doc = app.session.active().unwrap().doc.clone();
    let hard = super::style_cjk::kinsoku_sets(&doc).into_iter().find(|k| k.name == "HardKinsoku").unwrap();
    let moji = super::style_cjk::mojikumi_sets(&doc, "").into_iter().find(|s| s.1 == "Body Text").unwrap().0;
    assert_eq!(moji, "MojikumiTable/Body Text");

    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.kenten"})));
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    kit::pick(&mut h, "Kenten Type:", "Black Circle");
    {
        let d = st(&mut h).ui.dialog.as_mut().unwrap();
        d.fields.insert("section".into(), json!("cjk.composition"));
        d.fields.insert("p.kinsoku".into(), serde_json::to_value(&hard).unwrap());
        d.fields.insert("p.mojikumi".into(), json!(moji));
        d.fields.insert("p.kinsokuHang".into(), json!("regular"));
    }
    kit::steps(&mut h, 4);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());

    let doc = h.state().session.active().unwrap().doc.clone();
    let style = doc.styles.para("Body").unwrap();
    assert_eq!(style.para.kinsoku, Some(Some(hard)));
    assert_eq!(style.para.mojikumi.as_deref(), Some("MojikumiTable/Body Text"));
    assert_eq!(style.para.kinsoku_hang, Some(designcraft_doc::cjk::KinsokuHang::Regular));
    assert_eq!((style.chars.kenten, style.chars.kenten_kind), (Some(true), Some(KentenKind::BlackCircle)));
    assert_eq!(style.chars.kenten_character.as_deref(), Some("\u{25CF}"));
    let a = st(&mut h).run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!(a["para"]["kinsoku"]["name"], "HardKinsoku", "the paragraph follows its style");
    assert_eq!(a["para"]["mojikumi"], "MojikumiTable/Body Text");
    assert_eq!(a["chars"]["kenten"], true);
    assert_eq!(a["chars"]["kentenKind"], "blackCircle");

    // Default Rules (`null`) replaces the set.
    st(&mut h).ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.composition", "p.kinsoku": null})));
    kit::steps(&mut h, 4);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert_eq!(h.state().session.active().unwrap().doc.styles.para("Body").unwrap().para.kinsoku, Some(None));
    assert!(st(&mut h).run("type.selectionAttrs", json!({})).unwrap()["para"]["kinsoku"].is_null());

    // A character style sets only the kenten attributes.
    st(&mut h).ui.dialog = Some(Dialog::new("characterStyleOptions", json!({"name": "Emphasis", "section": "cjk.kenten"})));
    kit::steps(&mut h, 4);
    kit::pick(&mut h, "Kenten Type:", "Sesame Dot");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let style = h.state().session.active().unwrap().doc.styles.char_style("Emphasis").cloned().unwrap();
    assert_eq!((style.chars.kenten, style.chars.kenten_kind), (Some(true), Some(KentenKind::SesameDot)));
    assert!(style.chars.ruby_alignment.is_none() && style.chars.shatai_angle.is_none(), "nothing else is set");
}

/// A variable font with the named instances Regular and SemiBold, catalogued in the installed fonts, and a document with a
/// paragraph style `Synthetic UI` set in it, open in Paragraph Style Options on Basic Character Formats.
fn synthetic_style_dialog(family: &str) -> egui_kittest::Harness<'static, DesignApp> {
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::variable_font(family).unwrap());
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name": "Synthetic UI", "chars": {"fontFamily": family, "fontStyle": "Regular"}})).unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Synthetic UI", "section": "chars"})));
    kit::harness_sized(app, vec2(1100.0, 900.0))
}

/// Successor of `paragraph_style_formats_select_a_synthetic_named_instance`: the Font Style select of Basic Character Formats lists a
/// catalogued variable font's named instance, and picking it and OK sets it on the paragraph style.
#[test]
fn paragraph_style_options_form_selects_a_synthetic_named_instance() {
    let mut h = synthetic_style_dialog("Synthetic Paragraph Wiring");
    kit::pick_named_like(&mut h, "Font Style", 0, "SemiBold");
    assert_eq!(kit::draft(&h).fields["c.fontStyle"], "SemiBold");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(h.state().session.doc().unwrap().doc.styles.resolve_para_style("Synthetic UI").1.font_style, "SemiBold");
}

/// Successor of `cold_style_popup_discovers_instances_and_selects_them` and `cold_style_combo_contains_named_instances` (which needs the
/// pinned fixture in a fresh process), on a synthetic font: the Font Style select offers the named instance the first time it opens.
#[test]
fn paragraph_style_options_form_offers_the_named_instances_of_a_catalogued_font() {
    let mut h = synthetic_style_dialog("Synthetic Cold Style");
    kit::click_named_like(&mut h, &[Role::ComboBox], "Font Style", 0);
    assert!(kit::shows(&h, "SemiBold"), "the Style select must offer named instances");
    let at = h.query_all_by_label("SemiBold").last().unwrap().rect().center();
    kit::click_at(&mut h, at);
    assert_eq!(kit::draft(&h).fields["c.fontStyle"], "SemiBold");
}

/// Successor of `named_instance_style_widget_and_api_agree` (which needs the pinned Bricolage fixture), on a synthetic font: a named
/// instance picked in the dialog previews on every use without an undo step, Preview off and Cancel put the document back, OK is
/// one undo step, undo and redo walk it, the inherited family stays inherited, and the style the dialog makes is the style
/// `style.paragraph.edit` makes, also after saving and opening the file.
#[test]
fn style_options_form_named_instance_widget_and_api_agree() {
    let family = "Synthetic Widget Agreement";
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::variable_font(family).unwrap());
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name": "Base VF", "chars": {"fontFamily": family, "fontStyle": "Regular"}})).unwrap();
    app.run("style.paragraph.create", json!({"name": "UI VF", "basedOn": "Base VF"})).unwrap();
    app.run("style.paragraph.create", json!({"name": "API VF", "basedOn": "Base VF"})).unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "UI VF", "section": "chars"})));
    let mut h = kit::harness_sized(app, vec2(1100.0, 900.0));
    let font_style =
        |h: &egui_kittest::Harness<'static, DesignApp>, name: &str| h.state().session.doc().unwrap().doc.styles.resolve_para_style(name).1.font_style;
    {
        let app = st(&mut h);
        let before = app.session.doc().unwrap().doc.clone();
        let undo = app.session.doc().unwrap().history.undo.len();
        let d = app.ui.dialog.as_mut().unwrap();
        d.fields.insert("preview".into(), json!(true));
        d.fields.insert("c.fontStyle".into(), json!("SemiBold"));
        kit::steps(&mut h, 3);
        assert_eq!(font_style(&h, "UI VF"), "SemiBold");
        assert_eq!(kit::undo_steps(&h), undo);
        st(&mut h).ui.dialog.as_mut().unwrap().fields.insert("preview".into(), json!(false));
        kit::steps(&mut h, 3);
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        st(&mut h).ui.dialog.as_mut().unwrap().fields.insert("preview".into(), json!(true));
        kit::steps(&mut h, 3);
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        st(&mut h).ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "UI VF", "section": "chars"})));
        let d = st(&mut h).ui.dialog.as_mut().unwrap();
        d.fields.insert("preview".into(), json!(true));
        d.fields.insert("c.fontStyle".into(), json!("SemiBold"));
        kit::steps(&mut h, 3);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert_eq!(kit::undo_steps(&h), undo + 1);
        st(&mut h).run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        st(&mut h).run("edit.redo", json!({})).unwrap();
        assert_eq!(font_style(&h, "UI VF"), "SemiBold");
        st(&mut h).run("edit.undo", json!({})).unwrap();
    }
    let mut current = "Regular";
    for selected in ["SemiBold", "Bold", "Regular"] {
        st(&mut h).ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "UI VF", "section": "chars"})));
        kit::steps(&mut h, 3);
        if font_style(&h, "UI VF") != selected {
            kit::pick_named_like(&mut h, "Font Style", 0, selected);
        }
        let _ = current;
        let d = kit::draft(&h);
        assert!(!d.fields.contains_key("c.fontFamily"), "inherited family remains inherited");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        let app = st(&mut h);
        app.run("style.paragraph.edit", json!({"name": "API VF", "chars": {"fontStyle": selected}})).unwrap();
        let styles = &app.session.doc().unwrap().doc.styles;
        assert_eq!(styles.para("UI VF").unwrap().chars, styles.para("API VF").unwrap().chars);
        assert_eq!(styles.resolve_para_style("UI VF").1.font_style, selected);
        assert_eq!(styles.para("UI VF").unwrap().based_on.as_deref(), Some("Base VF"));
        current = selected;
        let native = app.run("file.serialize", json!({})).unwrap();
        app.run("file.openBytes", json!({"name": "vf.designcraft", "base64": native["base64"]})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.styles.resolve_para_style("UI VF").1.font_style, selected);
    }
}
