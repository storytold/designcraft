//! Successors, on the framework form, of the native Package dialog tests the wave 4 cleanup parked: the Summary and Links and Images
//! pages leading through Printing Instructions to the package, and the IDML and PDF options. They run the real dialog host over an
//! installed framework (`table_layout_kit`). `enter_in_a_multiline_instruction_field_is_a_new_line` and the native multi-line Enter flag
//! test are covered by the existing `sdk_package_natural_tab_and_multiline_enter_do_not_continue`.

use super::*;
use crate::dialogs::table_layout_kit as kit;
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use serde_json::{Value, json};

/// A document with a text frame and an image whose file has gone, in a temporary folder.
fn app_with_missing_link(dir: &std::path::Path) -> DesignApp {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".to_owned();
    app.run("file.new", json!({"title": "Packed"})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
    let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
    let img = dir.join("gone.png");
    std::fs::write(&img, &png).unwrap();
    app.run("file.place", json!({"path": img.to_string_lossy(), "x": 40, "y": 260, "width": 40})).unwrap();
    std::fs::remove_file(&img).unwrap();
    app
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-successor-{tag}-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn st(h: &mut egui_kittest::Harness<'static, DesignApp>) -> &mut DesignApp {
    h.state_mut()
}

/// Successor of `package_shows_a_summary_then_the_printing_instructions` (the course's chapter 27): File > Package opens the Package
/// dialog. Summary counts the fonts, links (one missing, with a warning) and inks; Links and Images lists the link, and Show Problems
/// Only keeps it; Create Printing Instructions leads to Printing Instructions, whose Continue asks for the folder and writes the
/// package with the instructions.
#[test]
fn package_form_shows_a_summary_then_the_printing_instructions() {
    let dir = temp_dir("summary");
    let mut app = app_with_missing_link(&dir);
    let out = dir.join("Packed Folder");
    let picked = out.to_string_lossy().to_string();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
    crate::menus::activate(&mut app, "app.packageDialog", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(ID));
    let mut h = kit::harness_sized(app, vec2(1100.0, 1000.0));
    assert!(kit::shows(&h, "Scope:") || h.query_all_by_label_contains("Scope").next().is_some());
    assert!(h.query_all_by_label_contains("Links and Images: 1 linked, 0 changed, 1 not found").next().is_some());
    assert!(h.query_all_by_label_contains("Colors and Inks: 4 process, 0 spot").next().is_some());
    let fonts = h.state().ui.dialog.as_ref().unwrap().fields["fonts"].as_array().unwrap().len();
    assert!(h.query_all_by_label_contains(&format!("Fonts: {fonts} in use, 0 unavailable")).next().is_some());
    // Links and Images: the link, missing; Show Problems Only keeps it.
    kit::click(&mut h, &[Role::ListItem], "Links and Images", 0);
    assert!(kit::shows(&h, "gone.png"));
    assert!(h.query_all_by_label_contains("Missing").next().is_some());
    kit::click(&mut h, &[Role::CheckBox], "Show Problems Only", 0);
    assert!(kit::shows(&h, "gone.png"));
    // Back on Summary: Create Printing Instructions, then Package...
    kit::click(&mut h, &[Role::ListItem], "Summary", 0);
    kit::click(&mut h, &[Role::CheckBox], "Create Printing Instructions", 0);
    kit::click(&mut h, &[Role::Button], "Package…", 0);
    st(&mut h).settle_dialogs();
    kit::steps(&mut h, 3);
    assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some(INSTRUCTIONS));
    for (key, value) in [("contact", "Dave"), ("email", "dave@example.com"), ("instructions", "Call me if anything is wrong.")] {
        st(&mut h).ui.dialog.as_mut().unwrap().fields.insert(key.into(), json!(value));
    }
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[Role::Button], "Continue", 0);
    st(&mut h).settle_dialogs();
    kit::steps(&mut h, 3);
    assert!(h.state().ui.dialog.is_none());
    let report = std::fs::read_to_string(out.join("Instructions.txt")).unwrap();
    assert!(
        report.contains("Contact: Dave") && report.contains("Email: dave@example.com") && report.contains("Call me if anything is wrong."),
        "{report}"
    );
    assert!(out.join("Packed.idml").exists(), "Include IDML is on by default");
    assert!(!out.join("Packed.pdf").exists(), "Include PDF is off by default");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Successor of `package_options_choose_the_idml_and_pdf`: without instructions, Package... goes straight to the folder; Include PDF
/// adds the PDF, made with the PDF preset chosen, and Include IDML off leaves the IDML out.
#[test]
fn package_form_options_choose_the_idml_and_pdf() {
    let dir = temp_dir("options");
    let mut app = app_with_missing_link(&dir);
    let out = dir.join("Packed Folder");
    let picked = out.to_string_lossy().to_string();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
    open(&mut app).unwrap();
    let mut h = kit::harness_sized(app, vec2(1100.0, 1000.0));
    kit::click(&mut h, &[Role::CheckBox], "Include IDML", 0);
    kit::click(&mut h, &[Role::CheckBox], "Include PDF (Print)", 0);
    // The PDF's preset: PDF/X-4.
    kit::pick(&mut h, "PDF Preset:", "PDF/X-4");
    assert_eq!(kit::draft(&h).s("pdfPreset"), "PDF/X-4");
    kit::click(&mut h, &[Role::Button], "Package…", 0);
    st(&mut h).settle_dialogs();
    kit::steps(&mut h, 3);
    assert!(h.state().ui.dialog.is_none());
    assert!(out.join("Packed.pdf").exists() && !out.join("Packed.idml").exists());
    let pdf = std::fs::read(out.join("Packed.pdf")).unwrap();
    assert!(pdf.windows(8).any(|w| w == b"GTS_PDFX"), "the PDF/X-4 preset's PDF");
    std::fs::remove_dir_all(dir).unwrap();
}
