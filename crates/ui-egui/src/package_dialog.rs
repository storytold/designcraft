//! File › Package…: InDesign's Package dialog. Summary counts what the package carries (fonts,
//! links and images, inks), with a warning where something is missing or modified; Fonts, Links
//! and Images and Colors and Inks list them (Show Problems Only). Package… asks for the folder to
//! create and writes the package (`file.package`), after the Printing Instructions when Create
//! Printing Instructions is on.

use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::dialogs::Dialog;
use crate::theme::semibold;

/// The Package dialog's id.
pub const ID: &str = "package";
/// The Printing Instructions dialog's id.
pub const INSTRUCTIONS: &str = "printingInstructions";

/// The dialog's pages: (key, label).
const PAGES: [(&str, &str); 4] = [("summary", "Summary"), ("fonts", "Fonts"), ("links", "Links and Images"), ("inks", "Colors and Inks")];

/// The process inks every package prints with.
const PROCESS_INKS: [&str; 4] = ["Process Cyan", "Process Magenta", "Process Yellow", "Process Black"];

/// The Printing Instructions' fields: (key, label, lines).
const INSTRUCTION_FIELDS: [(&str, &str, usize); 7] = [
    ("contact", "Contact:", 1),
    ("company", "Company:", 1),
    ("address", "Address:", 3),
    ("phone", "Phone:", 1),
    ("fax", "Fax:", 1),
    ("email", "Email:", 1),
    ("instructions", "Instructions:", 4),
];

/// Open the Package dialog on its Summary, with the document's fonts, links and spot inks.
pub fn open(app: &mut DesignApp) -> Result<(), String> {
    if app.session.active().is_none() {
        return Err("no document open".into());
    }
    let fonts: Vec<Value> = app
        .session
        .execute("font.list", &json!({}))
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|f| {
            let status = font_status(f);
            let name = format!("{} {}", f["family"].as_str().unwrap_or(""), f["style"].as_str().unwrap_or("")).trim().to_string();
            json!({"name": name, "source": f["source"].as_str().unwrap_or(""), "status": status})
        })
        .collect();
    let links: Vec<Value> = app
        .session
        .execute("links.list", &json!({}))
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|l| {
            let page = l["uses"].as_array().and_then(|u| u.iter().find_map(|u| u["page"].as_str())).unwrap_or("");
            json!({"name": l["name"].as_str().unwrap_or(""), "page": page, "status": l["status"].as_str().unwrap_or("ok")})
        })
        .collect();
    // Spot inks print on their own plates unless the Ink Manager makes them process.
    let inks = app.session.execute("ink.list", &json!({})).unwrap_or_default();
    let all_process = inks["allToProcess"].as_bool().unwrap_or(false);
    let spot: Vec<Value> = inks["inks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|i| !all_process && !i["process"].as_bool().unwrap_or(false))
                .filter_map(|i| i["name"].as_str())
                .map(|n| json!(n))
                .collect()
        })
        .unwrap_or_default();
    // The packaged PDF's preset: the one last used for Export PDF, as InDesign suggests it.
    let last = app.ui.pdf_export.preset.clone();
    let preset = crate::dialogs::pdf_presets().find(|p| *p == last).unwrap_or("Desktop Printing");
    let doc = app.session.active().map(|d| d.uid);
    app.ui.dialog = Some(Dialog::new(
        ID,
        json!({"page": "summary", "doc": doc, "fonts": fonts, "links": links, "spot": spot, "instructions": false, "idml": true, "pdf": false,
            "pdfPreset": preset, "problemsOnly": false}),
    ));
    Ok(())
}

/// A `font.list` entry's status here: a missing font, or a style its family lacks, is missing
/// (as `file.package` counts them); a style it stands in for is substituted.
fn font_status(f: &Value) -> &'static str {
    if f["missing"].as_bool().unwrap_or(false) || f["styleMissing"].as_bool().unwrap_or(false) {
        "missing"
    } else if f["matchStatus"] == "styleSubstitute" {
        "substituted"
    } else {
        "ok"
    }
}

/// The document the dialog was opened on is still the active one (automation can switch
/// documents while it is open, and the package must be of the one summarised).
fn same_document(app: &DesignApp, d: &Dialog) -> Result<(), String> {
    match app.session.active() {
        Some(doc) if d.fields.get("doc").and_then(Value::as_u64) == Some(doc.uid) => Ok(()),
        _ => Err("the active document changed after the Package dialog opened".into()),
    }
}

fn rows(d: &Dialog, key: &str) -> Vec<Value> {
    d.fields.get(key).and_then(Value::as_array).cloned().unwrap_or_default()
}

/// How many of `rows` have one of `statuses`.
fn count(rows: &[Value], statuses: &[&str]) -> usize {
    rows.iter().filter(|r| r["status"].as_str().is_some_and(|s| statuses.contains(&s))).count()
}

/// The Summary's lines: (text, whether it warns).
fn summary_lines(lang: &str, d: &Dialog) -> Vec<(String, bool)> {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let (fonts, links) = (rows(d, "fonts"), rows(d, "links"));
    let missing_fonts = count(&fonts, &["missing"]);
    let (modified, missing) = (count(&links, &["modified"]), count(&links, &["missing"]));
    vec![
        (
            tr("Fonts: {used} in use, {missing} unavailable")
                .replace("{used}", &fonts.len().to_string())
                .replace("{missing}", &missing_fonts.to_string()),
            missing_fonts > 0,
        ),
        (
            tr("Links and Images: {found} linked, {modified} changed, {missing} not found")
                .replace("{found}", &links.len().to_string())
                .replace("{modified}", &modified.to_string())
                .replace("{missing}", &missing.to_string()),
            modified + missing > 0,
        ),
        (tr("Embedded images: {embedded}").replace("{embedded}", &count(&links, &["embedded"]).to_string()), false),
        (
            tr("Colors and Inks: {process} process, {spot} spot")
                .replace("{process}", &PROCESS_INKS.len().to_string())
                .replace("{spot}", &rows(d, "spot").len().to_string()),
            false,
        ),
    ]
}

/// How a status is shown: coloured with the theme's warning or error colour.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Level {
    Warn,
    Bad,
}

impl Level {
    fn color(self, ui: &egui::Ui) -> egui::Color32 {
        match self {
            Level::Warn => ui.visuals().warn_fg_color,
            Level::Bad => ui.visuals().error_fg_color,
        }
    }
}

fn status_text(lang: &str, status: &str) -> (String, Option<Level>) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    match status {
        "missing" => (format!("\u{26A0} {}", tr("Missing")), Some(Level::Bad)),
        "modified" => (format!("\u{25B2} {}", tr("Modified")), Some(Level::Warn)),
        "substituted" => (tr("Substituted"), Some(Level::Warn)),
        "embedded" => (tr("Embedded"), None),
        _ => (tr("OK"), None),
    }
}

/// A table: a header row and `cells` rows (each with an optional level colouring its last cell).
fn table(ui: &mut egui::Ui, id: &str, header: &[String], cells: &[(Vec<String>, Option<Level>)]) {
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.field_border)).inner_margin(6.0).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(id).max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new(id).num_columns(header.len()).spacing([18.0, 4.0]).striped(true).show(ui, |ui| {
                for h in header {
                    crate::rtl::label(ui, egui::RichText::new(h).font(semibold(11.5)));
                }
                ui.end_row();
                for (row, color) in cells {
                    for (i, c) in row.iter().enumerate() {
                        let mut text = egui::RichText::new(c);
                        if i + 1 == row.len()
                            && let Some(color) = color
                        {
                            text = text.color(color.color(ui));
                        }
                        crate::rtl::label(ui, text);
                    }
                    ui.end_row();
                }
            });
        });
    });
}

/// The Package dialog: the pages on the left, the page on the right.
pub fn show(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let page = d.fields.get("page").and_then(Value::as_str).unwrap_or("summary").to_string();
    ui.horizontal_top(|ui| {
        // A fixed height, as Preferences': the divider between the pages and the page fills it,
        // and the dialog keeps its size from page to page.
        ui.set_min_height(320.0);
        ui.set_max_height(320.0);
        ui.vertical(|ui| {
            ui.set_width(150.0);
            for (key, label) in PAGES {
                if ui.selectable_label(page == key, crate::rtl::widget(ui, tr(label))).clicked() {
                    d.fields.insert("page".into(), json!(key));
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_width(460.0);
            let title = PAGES.iter().find(|p| p.0 == page).map_or("Summary", |p| p.1);
            crate::rtl::label(ui, egui::RichText::new(tr(title)).font(semibold(13.0)));
            ui.add_space(6.0);
            let problems_only = d.b("problemsOnly");
            match page.as_str() {
                "fonts" => {
                    let fonts = rows(d, "fonts");
                    let cells: Vec<_> = fonts
                        .iter()
                        .filter(|f| !problems_only || f["status"] != "ok")
                        .map(|f| {
                            let (status, color) = status_text(&lang, f["status"].as_str().unwrap_or("ok"));
                            let source = match f["source"].as_str().unwrap_or("") {
                                "bundled" => tr("Bundled"),
                                "installed" => tr("Installed"),
                                "document" => tr("Document"),
                                "added" => tr("Added"),
                                _ => String::new(),
                            };
                            (vec![f["name"].as_str().unwrap_or("").to_string(), source, status], color)
                        })
                        .collect();
                    table(ui, "package_fonts", &[tr("Name"), tr("Source"), tr("Status")], &cells);
                }
                "links" => {
                    let links = rows(d, "links");
                    let cells: Vec<_> = links
                        .iter()
                        .filter(|l| !problems_only || matches!(l["status"].as_str(), Some("missing" | "modified")))
                        .map(|l| {
                            let (status, color) = status_text(&lang, l["status"].as_str().unwrap_or("ok"));
                            (vec![l["name"].as_str().unwrap_or("").to_string(), l["page"].as_str().unwrap_or("").to_string(), status], color)
                        })
                        .collect();
                    table(ui, "package_links", &[tr("Name"), tr("Page"), tr("Status")], &cells);
                }
                "inks" => {
                    let mut cells: Vec<_> = PROCESS_INKS.iter().map(|n| (vec![tr(n), tr("Process")], None)).collect();
                    cells.extend(rows(d, "spot").iter().filter_map(Value::as_str).map(|n| (vec![n.to_string(), tr("Spot")], None)));
                    // The ink's kind: the "Type:" key (the "Type" key is the menu's, which reads Text).
                    let kind = tr("Type:").trim_end_matches([':', '：']).trim_end().to_string();
                    table(ui, "package_inks", &[tr("Name"), kind], &cells);
                }
                _ => {
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, tr("Scope:"));
                        crate::rtl::label(ui, tr("Entire Publication"));
                    });
                    ui.add_space(6.0);
                    for (line, warn) in summary_lines(&lang, d) {
                        ui.horizontal(|ui| {
                            let sign = egui::RichText::new(if warn { "\u{26A0}" } else { " " }).color(Level::Warn.color(ui));
                            ui.add_sized([14.0, 16.0], egui::Label::new(sign));
                            crate::rtl::label(ui, line);
                        });
                    }
                    ui.add_space(10.0);
                    for (key, label) in [("instructions", "Create Printing Instructions"), ("idml", "Include IDML"), ("pdf", "Include PDF (Print)")] {
                        let mut on = d.b(key);
                        if ui.checkbox(&mut on, crate::rtl::widget(ui, tr(label))).changed() {
                            d.fields.insert(key.into(), json!(on));
                        }
                    }
                    // The PDF's preset, as InDesign's Select PDF Preset.
                    ui.add_enabled_ui(d.b("pdf"), |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            let name = crate::rtl::label(ui, tr("PDF Preset:"));
                            let cur = d.s("pdfPreset");
                            craft_ui::ComboBox::from_id_salt("package_pdf_preset")
                                .selected_text(crate::rtl::widget(ui, tr(&cur)))
                                .width(180.0)
                                .show_ui(ui, |ui| {
                                    for p in crate::dialogs::pdf_presets() {
                                        if ui.selectable_label(cur == p, crate::rtl::widget(ui, tr(p))).clicked() {
                                            d.fields.insert("pdfPreset".into(), json!(p));
                                        }
                                    }
                                })
                                .response
                                .labelled_by(name.id);
                        });
                    });
                }
            }
            if matches!(page.as_str(), "fonts" | "links") {
                ui.add_space(4.0);
                let mut on = problems_only;
                if ui.checkbox(&mut on, crate::rtl::widget(ui, tr("Show Problems Only"))).changed() {
                    d.fields.insert("problemsOnly".into(), json!(on));
                }
            }
        });
    });
}

/// The `pdf` of `file.package`: false, or the options of the chosen PDF preset.
fn pdf_param(d: &Dialog) -> Value {
    if d.b("pdf") { crate::dialogs::pdf_preset_params(&d.s("pdfPreset")) } else { json!(false) }
}

/// Package…: the Printing Instructions first when asked for, else the folder.
pub fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    same_document(app, d)?;
    let (idml, pdf) = (d.b("idml"), pdf_param(d));
    if d.b("instructions") {
        app.ui.dialog = Some(Dialog::new(INSTRUCTIONS, json!({"idml": idml, "pdf": pdf, "doc": d.fields.get("doc")})));
        return Ok(Value::Null);
    }
    package_to_folder(app, idml, pdf, None, d.clone())
}

/// The Printing Instructions dialog, as InDesign's: who to contact and what to do.
pub fn show_instructions(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    egui::Grid::new("printing_instructions").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (key, label, lines) in INSTRUCTION_FIELDS {
            let name = crate::rtl::label(ui, crate::i18n::tr(&lang, label));
            let mut text = d.s(key);
            let edit = if lines > 1 { egui::TextEdit::multiline(&mut text).desired_rows(lines) } else { egui::TextEdit::singleline(&mut text) };
            // Named by its label, for screen readers.
            let resp = ui.add(edit.desired_width(320.0)).labelled_by(name.id);
            if resp.changed() {
                d.fields.insert(key.into(), json!(text));
            }
            // Enter there makes a new line: the footer mustn't take it for Continue.
            if lines > 1 && resp.has_focus() {
                crate::dialogs::set_multiline_focus(ui.ctx());
            }
            ui.end_row();
        }
    });
}

/// The instructions as they go into the package's Instructions.txt: the contact lines, then the
/// instructions. Empty fields are left out.
fn instructions_text(d: &Dialog) -> String {
    let mut out = String::new();
    for (key, label) in
        [("contact", "Contact"), ("company", "Company"), ("address", "Address"), ("phone", "Phone"), ("fax", "Fax"), ("email", "Email")]
    {
        let v = d.s(key);
        let v = v.trim();
        if !v.is_empty() {
            out += &format!("{label}: {}\n", v.replace('\n', "\n  "));
        }
    }
    let ins = d.s("instructions");
    if !ins.trim().is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out += ins.trim();
        out.push('\n');
    }
    out
}

/// Continue: the folder, with the instructions.
pub fn confirm_instructions(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    same_document(app, d)?;
    let text = instructions_text(d);
    package_to_folder(app, d.b("idml"), d.fields.get("pdf").cloned().unwrap_or(json!(false)), Some(text).filter(|t| !t.is_empty()), d.clone())
}

/// Ask for the folder to create ("<title> Folder") and package the document into it once it is
/// chosen (Cancel packages nothing). Packaging that fails reopens `asking`, the dialog that asked,
/// with its options. `pdf`:
/// false, true or the PDF's `file.exportPdf` options.
pub fn package_to_folder(app: &mut DesignApp, idml: bool, pdf: Value, instructions: Option<String>, asking: Dialog) -> Result<Value, String> {
    let name = app.session.active().map(|d| format!("{} Folder", d.doc.title)).unwrap_or_default();
    app.ask_save(&name, Bound::Document, move |app, dir| {
        let mut p = json!({"dir": dir, "idml": idml, "pdf": pdf});
        if let Some(t) = instructions {
            p["instructions"] = json!(t);
        }
        let r = app.run("file.package", p);
        match &r {
            Ok(v) => app.status(
                crate::i18n::tr(&app.ui.language, "Packaged {count} files into {dir}")
                    .replace("{count}", &v["files"].as_array().map_or(0, |f| f.len()).to_string())
                    .replace("{dir}", v["dir"].as_str().unwrap_or("")),
            ),
            Err(_) if app.ui.dialog.is_none() => app.ui.dialog = Some(asking),
            Err(_) => {}
        }
        r
    })
}

#[cfg(test)]
mod tests {
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    /// A document with a text frame and an image whose file has gone, in a temporary folder.
    fn app_with_missing_link(dir: &std::path::Path) -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "Packed"})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
        let img = dir.join("gone.png");
        std::fs::write(&img, &png).unwrap();
        app.run("file.place", json!({"path": img.to_string_lossy(), "x": 40, "y": 260, "width": 40})).unwrap();
        std::fs::remove_file(&img).unwrap();
        app
    }

    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        Harness::builder().with_size(egui::vec2(1100.0, 760.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("package_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        )
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("dc-package-dialog-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The course's chapter 27: File › Package opens InDesign's Package dialog. Summary counts the
    /// fonts, links (one missing, with a warning) and inks; Links and Images lists the link, and
    /// Show Problems Only keeps it; Create Printing Instructions leads to Printing Instructions,
    /// whose Continue asks for the folder and writes the package with the instructions.
    #[test]
    fn package_shows_a_summary_then_the_printing_instructions() {
        let dir = temp_dir("summary");
        let mut app = app_with_missing_link(&dir);
        let out = dir.join("Packed Folder");
        let picked = out.to_string_lossy().to_string();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
        crate::menus::activate(&mut app, "app.packageDialog", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(ID));
        let mut h = harness(app);
        h.run_steps(3);
        assert!(h.query_by_label("Scope:").is_some());
        assert!(h.query_by_label("Links and Images: 1 linked, 0 changed, 1 not found").is_some());
        assert!(h.query_by_label("Colors and Inks: 4 process, 0 spot").is_some());
        let fonts = h.state().ui.dialog.as_ref().unwrap().fields["fonts"].as_array().unwrap().len();
        assert!(h.query_by_label(&format!("Fonts: {fonts} in use, 0 unavailable")).is_some());
        // Links and Images: the link, missing; Show Problems Only keeps it.
        h.get_by_label("Links and Images").click();
        h.run_steps(2);
        assert!(h.query_by_label("gone.png").is_some());
        assert!(h.query_by_label("\u{26A0} Missing").is_some());
        h.get_by_label("Show Problems Only").click();
        h.run_steps(2);
        assert!(h.query_by_label("gone.png").is_some());
        // Back on Summary: Create Printing Instructions, then Package….
        h.get_by_label("Summary").click();
        h.run_steps(2);
        h.get_by_label("Create Printing Instructions").click();
        h.run_steps(2);
        h.get_by_label("Package…").click();
        h.run_steps(3);
        h.state_mut().settle_dialogs();
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some(INSTRUCTIONS));
        for (key, value) in [("contact", "Dave"), ("email", "dave@example.com"), ("instructions", "Call me if anything is wrong.")] {
            h.state_mut().ui.dialog.as_mut().unwrap().fields.insert(key.into(), json!(value));
        }
        h.run_steps(2);
        h.get_by_label("Continue").click();
        h.run_steps(3);
        h.state_mut().settle_dialogs();
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

    /// Without instructions, Package… goes straight to the folder; Include PDF adds the PDF, made
    /// with the PDF preset chosen, and Include IDML off leaves the IDML out.
    #[test]
    fn package_options_choose_the_idml_and_pdf() {
        let dir = temp_dir("options");
        let mut app = app_with_missing_link(&dir);
        let out = dir.join("Packed Folder");
        let picked = out.to_string_lossy().to_string();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
        open(&mut app).unwrap();
        let mut h = harness(app);
        h.run_steps(3);
        h.get_by_label("Include IDML").click();
        h.run_steps(2);
        h.get_by_label("Include PDF (Print)").click();
        h.run_steps(2);
        // The PDF's preset: PDF/X-4.
        h.get_by_value("Desktop Printing").click();
        h.run_steps(2);
        h.get_by_label("PDF/X-4").click();
        h.run_steps(2);
        h.get_by_label("Package…").click();
        h.run_steps(3);
        h.state_mut().settle_dialogs();
        assert!(h.state().ui.dialog.is_none());
        assert!(out.join("Packed.pdf").exists() && !out.join("Packed.idml").exists());
        let pdf = std::fs::read(out.join("Packed.pdf")).unwrap();
        assert!(pdf.windows(8).any(|w| w == b"GTS_PDFX"), "the PDF/X-4 preset's PDF");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Packaging that fails (here, into a folder under a file) keeps the dialog open with its
    /// options, and the status line says why. Without a document there's nothing to open.
    #[test]
    fn a_failed_package_keeps_the_dialog_open() {
        let dir = temp_dir("fails");
        let mut app = app_with_missing_link(&dir);
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"a file, not a folder").unwrap();
        let picked = blocker.join("Packed Folder").to_string_lossy().to_string();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
        open(&mut app).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("pdf".into(), json!(true));
        crate::dialogs::confirm(&mut app).unwrap();
        // The folder is chosen on a later frame; packaging into it fails then.
        app.settle_dialogs();
        let d = app.ui.dialog.as_ref().expect("the dialog opens again");
        assert_eq!((d.id.as_str(), d.b("pdf")), (ID, true), "with its options");
        assert!(!app.ui.status.is_empty());
        app.run("file.close", json!({})).unwrap();
        app.ui.dialog = None;
        assert!(app.run("app.packageDialog", json!({})).is_err());
        assert!(app.ui.dialog.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A style the font's family lacks counts as missing, as `file.package` counts it.
    #[test]
    fn a_missing_font_style_is_a_missing_font() {
        assert_eq!(font_status(&json!({"missing": true})), "missing");
        assert_eq!(font_status(&json!({"missing": false, "styleMissing": true})), "missing");
        assert_eq!(font_status(&json!({"missing": false, "matchStatus": "styleSubstitute"})), "substituted");
        assert_eq!(font_status(&json!({"missing": false})), "ok");
    }

    /// The package is of the document the dialog summarised: when another document became active
    /// meanwhile, Package… and Continue refuse, and nothing is written.
    #[test]
    fn package_refuses_when_the_active_document_changed() {
        let dir = temp_dir("switched");
        let mut app = app_with_missing_link(&dir);
        let out = dir.join("Packed Folder");
        let picked = out.to_string_lossy().to_string();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
        open(&mut app).unwrap();
        let d = app.ui.dialog.take().unwrap();
        app.run("file.new", json!({"title": "Other"})).unwrap();
        assert!(confirm(&mut app, &d).is_err());
        let ins = Dialog::new(INSTRUCTIONS, json!({"idml": true, "pdf": false, "doc": d.fields["doc"]}));
        assert!(confirm_instructions(&mut app, &ins).is_err());
        assert!(!out.exists(), "nothing packaged");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Enter in Address or Instructions makes a new line there; in a one-line field it continues.
    #[test]
    fn enter_in_a_multiline_instruction_field_is_a_new_line() {
        let dir = temp_dir("enter");
        let mut app = app_with_missing_link(&dir);
        let doc = app.session.active().unwrap().uid;
        app.ui.dialog = Some(Dialog::new(INSTRUCTIONS, json!({"idml": true, "pdf": false, "doc": doc})));
        let mut h = harness(app);
        h.run_steps(3);
        let field = |h: &Harness<'static, DesignApp>, n: usize| {
            h.query_all_by(|x| x.role() == egui::accesskit::Role::TextInput || x.role() == egui::accesskit::Role::MultilineTextInput)
                .nth(n)
                .unwrap()
                .rect()
                .center()
        };
        // Address is the third field.
        let address = field(&h, 2);
        h.event(egui::Event::PointerButton { pos: address, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.event(egui::Event::PointerButton { pos: address, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some(INSTRUCTIONS), "still open");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
