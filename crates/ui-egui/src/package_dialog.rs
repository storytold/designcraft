//! File › Package…: InDesign's Package dialog. Summary counts what the package carries (fonts,
//! links and images, inks), with a warning where something is missing or modified; Fonts, Links
//! and Images and Colors and Inks list them (Show Problems Only). Package… asks for the folder to
//! create and writes the package (`file.package`), after the Printing Instructions when Create
//! Printing Instructions is on.

use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::dialogs::Dialog;

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

/// The page the dialog shows.
fn page_of(d: &Dialog) -> String {
    d.fields.get("page").and_then(Value::as_str).unwrap_or("summary").to_string()
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

/// A page's list: its id, column headers and rows (each with the level of its status, which is its last cell).
struct Listed {
    id: &'static str,
    header: Vec<String>,
    rows: Vec<(Vec<String>, Option<Level>)>,
}

/// The list the page `page` shows (Fonts, Links and Images, Colors and Inks); `None` for the Summary.
fn page_table(lang: &str, d: &Dialog, page: &str) -> Option<Listed> {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let problems_only = d.b("problemsOnly");
    Some(match page {
        "fonts" => {
            let rows = rows(d, "fonts")
                .iter()
                .filter(|f| !problems_only || f["status"] != "ok")
                .map(|f| {
                    let (status, level) = status_text(lang, f["status"].as_str().unwrap_or("ok"));
                    let source = match f["source"].as_str().unwrap_or("") {
                        "bundled" => tr("Bundled"),
                        "installed" => tr("Installed"),
                        "document" => tr("Document"),
                        "added" => tr("Added"),
                        _ => String::new(),
                    };
                    (vec![f["name"].as_str().unwrap_or("").to_string(), source, status], level)
                })
                .collect();
            Listed { id: "package_fonts", header: vec![tr("Name"), tr("Source"), tr("Status")], rows }
        }
        "links" => {
            let rows = rows(d, "links")
                .iter()
                .filter(|l| !problems_only || matches!(l["status"].as_str(), Some("missing" | "modified")))
                .map(|l| {
                    let (status, level) = status_text(lang, l["status"].as_str().unwrap_or("ok"));
                    (vec![l["name"].as_str().unwrap_or("").to_string(), l["page"].as_str().unwrap_or("").to_string(), status], level)
                })
                .collect();
            Listed { id: "package_links", header: vec![tr("Name"), tr("Page"), tr("Status")], rows }
        }
        "inks" => {
            let spot = rows(d, "spot");
            let mut inks: Vec<_> = PROCESS_INKS.iter().map(|n| (vec![tr(n), tr("Process")], None)).collect();
            inks.extend(spot.iter().filter_map(Value::as_str).map(|n| (vec![n.to_string(), tr("Spot")], None)));
            // The ink's kind: the "Type:" key (the "Type" key is the menu's, which reads Text).
            let kind = tr("Type:").trim_end_matches([':', '：']).trim_end().to_string();
            Listed { id: "package_inks", header: vec![tr("Name"), kind], rows: inks }
        }
        _ => return None,
    })
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

/// The Package dialog on the framework's dialog host.
pub(crate) const FORM: crate::dialogs::framework::Form = crate::dialogs::framework::Form::new(framework_form::package).width(720.0);
/// The Printing Instructions dialog on the framework's dialog host.
pub(crate) const INSTRUCTIONS_FORM: crate::dialogs::framework::Form = crate::dialogs::framework::Form::new(framework_form::instructions).width(520.0);

/// The framework forms of Package and Printing Instructions. They edit the dialog's draft fields;
/// Package… and Continue go through the host to [`confirm`] and [`confirm_instructions`], which alone ask for a folder.
mod framework_form {
    use super::{INSTRUCTION_FIELDS, PAGES, page_of, page_table, summary_lines};
    use crate::DesignApp;
    use crate::dialogs::Dialog;
    use craft_behavior::{ColumnKey, ColumnWidth, DisplayOrder, ItemSource, Selection};
    use craft_theme::variant::Tone;
    use craft_ui_widgets::choice::Checkbox;
    use craft_ui_widgets::contract::{
        Bind, ColumnSpec, Entry, Gap, Props as _, Scroll, SelectMode, Show as _, ShowCollection as _, ShowContainer as _, Sizing,
    };
    use craft_ui_widgets::display::{Divider, Elide, Text};
    use craft_ui_widgets::form::{Field, FieldGroup};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::{List, ListRow};
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::style::TypeRole;
    use craft_ui_widgets::table::Table;
    use craft_ui_widgets::text::TextField;
    use egui::Ui;
    use serde_json::{Value, json};

    /// The columns of a page's list, in order.
    const COLUMNS: [ColumnKey; 3] = [ColumnKey::new("name"), ColumnKey::new("second"), ColumnKey::new("status")];
    /// How high a page's list may grow before it scrolls.
    const LIST_ROOM: f32 = 220.0;

    /// Rows shown by their first cell's text.
    struct Texts(Vec<String>);

    impl DisplayOrder<usize> for Texts {
        fn len(&self) -> usize {
            self.0.len()
        }
        fn key(&self, at: usize) -> usize {
            at
        }
        fn index_of(&self, key: &usize) -> Option<usize> {
            (*key < self.0.len()).then_some(*key)
        }
        fn text(&self, at: usize) -> Option<std::borrow::Cow<'_, str>> {
            self.0.get(at).map(|text| std::borrow::Cow::Borrowed(text.as_str()))
        }
    }
    impl ItemSource<usize> for Texts {}

    fn tr(app: &DesignApp, s: &str) -> String {
        crate::i18n::tr(&app.ui.language, s).to_owned()
    }

    /// A check box over the bool draft field `key`.
    fn check(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
        let out = Checkbox::new(Bind::given(d.b(key))).id_salt(("package_check", key)).label(tr(app, label)).show(ui);
        if let Some(value) = out.events.committed() {
            d.fields.insert(key.into(), json!(value));
        }
    }

    /// The pages, one selected; picking one shows it.
    fn pages(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, page: &str) {
        let labels = Texts(PAGES.iter().map(|(_, label)| tr(app, label)).collect());
        let at = PAGES.iter().position(|(key, _)| *key == page).unwrap_or(0);
        let mut selected = Selection::single(at);
        let _ = List::<usize>::new(tr(app, "Package"))
            .id_salt("package_pages")
            .select(SelectMode::Single)
            .arrows_select(true)
            .natural_height(true)
            .sizing(Sizing::FillWidth)
            .selection(Bind::bound(&mut selected))
            .show_items(ui, &labels, &mut |key, _| ListRow::new(labels.0.get(*key).cloned().unwrap_or_default()));
        if let Some((key, _)) = selected.items.first().and_then(|at| PAGES.get(*at))
            && *key != page
        {
            d.fields.insert("page".into(), json!(key));
        }
    }

    /// The list of a page: its header over its rows, scrolling past [`LIST_ROOM`].
    fn list(ui: &mut Ui, id: &'static str, label: String, header: &[String], rows: &[(Vec<String>, Option<super::Level>)]) {
        let columns: Vec<ColumnSpec> =
            header.iter().zip(COLUMNS).map(|(title, key)| ColumnSpec::new(key, title.clone(), ColumnWidth::Content)).collect();
        let source = Texts(rows.iter().map(|(cells, _)| cells.first().cloned().unwrap_or_default()).collect());
        Stack::new().id_salt((id, "room")).fill_width().scroll(Scroll::vertical()).room_height(LIST_ROOM).show(ui, |ui, _| {
            let _ = Table::<usize>::new(label.clone(), columns.clone())
                .id_salt(id)
                .select(SelectMode::Single)
                .striped(true)
                .natural_height(true)
                .sizing(Sizing::FillWidth)
                .show_items(ui, &source, &mut |key, icx| {
                    let cells = &rows[*key].0;
                    let mut row = ListRow::new(cells.first().cloned().unwrap_or_default()).cells(icx.cells()).striped(icx.row.striped);
                    for (column, text) in COLUMNS.iter().zip(cells).skip(1) {
                        row = row.cell(column.clone(), text.clone());
                    }
                    row
                });
        });
    }

    /// The Package dialog: the pages beside the page shown.
    pub(super) fn package(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
        let page = page_of(d);
        // The document the dialog summarises and the active one: a change of either retires the old controls.
        let doc = d.fields.get("doc").and_then(Value::as_u64);
        let live = app.session.active().map(|st| (st.uid, st.identity_epoch));
        Row::new().id_salt(("package_form", doc, live)).fill_width().show(ui, |ui, _| {
            Stack::new().id_salt("package_pages_column").show(ui, |ui, _| pages(app, ui, d, &page));
            Divider::vertical().show(ui);
            Stack::new().id_salt("package_page").fill_width().show(ui, |ui, _| {
                let title = PAGES.iter().find(|p| p.0 == page).map_or("Summary", |p| p.1);
                Text::new(tr(app, title)).role(TypeRole::BodyStrong).show(ui);
                match page_table(&app.ui.language, d, &page) {
                    Some(listed) => list(ui, listed.id, tr(app, title), &listed.header, &listed.rows),
                    None => summary(app, ui, d),
                }
                if matches!(page.as_str(), "fonts" | "links") {
                    check(app, ui, d, "problemsOnly", "Show Problems Only");
                }
            });
        });
        None
    }

    /// The Summary: the scope, the counts (a warning sign on those that need attention), what the package includes
    /// and the PDF's preset.
    fn summary(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
        // Three blocks with the room of a section between them: the scope, the counts under it, and the choices.
        Stack::new().id_salt("package_summary_blocks").fill_width().gap(Gap::Section).show(ui, |ui, _| {
            Row::new().id_salt("package_scope").show(ui, |ui, _| {
                Text::new(tr(app, "Scope:")).show(ui);
                Text::new(tr(app, "Entire Publication")).show(ui);
            });
            // Each count is one whole line across the page, as InDesign's: it neither wraps at the dialog's paragraph
            // width nor is cut. A line that wrapped in the drawn run but not in the measuring one made the block taller
            // than it was measured, and cut its last line (Colors and Inks) off.
            Stack::new().id_salt("package_summary_counts").indent(2).fill_width().show(ui, |ui, _| {
                for (at, (line, warn)) in summary_lines(&app.ui.language, d).into_iter().enumerate() {
                    Row::new().id_salt(("package_summary", at)).fill_width().show(ui, |ui, _| {
                        if warn {
                            Text::new("\u{26A0}").tone(Tone::Warn).show(ui);
                        }
                        // The line's own text is its sample, so its least width is the whole line: the dialog's frame
                        // is as wide as its content, and a line that may shrink to the other rows' width is cut there.
                        Text::new(line.clone()).sample(line.clone()).elide(Elide::End).sizing(Sizing::FillWidth).show(ui);
                    });
                }
            });
            Stack::new().id_salt("package_summary_choices").fill_width().show(ui, |ui, _| {
                for (key, label) in [("instructions", "Create Printing Instructions"), ("idml", "Include IDML"), ("pdf", "Include PDF (Print)")] {
                    check(app, ui, d, key, label);
                }
                // The PDF's preset, as InDesign's Select PDF Preset, under the check box of the PDF.
                Stack::new().id_salt("package_pdf_preset_row").indent(2).fill_width().show(ui, |ui, _| {
                    let name = tr(app, "PDF Preset:");
                    Field::new(name.clone()).id_salt("package_pdf_preset_field").enabled(d.b("pdf")).show(ui, |ui, _| {
                        let current = d.s("pdfPreset");
                        let entries: Vec<_> = crate::dialogs::pdf_presets().map(|p| Entry::new(p.to_string(), tr(app, p))).collect();
                        let out =
                            Select::new(name.clone(), Bind::given(current), &entries).id_salt("package_pdf_preset").enabled(d.b("pdf")).show(ui);
                        if let Some(value) = out.events.committed() {
                            d.fields.insert("pdfPreset".into(), json!(value));
                        }
                    });
                });
            });
        });
    }

    /// Printing Instructions: who to contact and what to do. Address and Instructions take several lines, and Enter
    /// there makes a new line.
    pub(super) fn instructions(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
        // The opening document is part of the dialog: a refused change of the active document keeps the draft.
        let doc = d.fields.get("doc").and_then(Value::as_u64);
        let live = app.session.active().map(|st| (st.uid, st.identity_epoch));
        let status = d.fields.get("status").and_then(Value::as_str).map(str::to_owned);
        Stack::new().id_salt(("printing_instructions", doc, live)).fill_width().show(ui, |ui, _| {
            FieldGroup::new(tr(app, "Printing Instructions")).show(ui, |ui, _| {
                for (key, label, lines) in INSTRUCTION_FIELDS {
                    let label = tr(app, label);
                    Field::new(label.clone()).id_salt(("printing_instructions_field", key)).show(ui, |ui, _| {
                        let before = d.s(key);
                        let mut text = before.clone();
                        TextField::new(label.clone(), Bind::bound(&mut text))
                            .id_salt(key)
                            .lines(lines)
                            .limit(None::<usize>)
                            .sizing(Sizing::FillWidth)
                            .show(ui);
                        if text != before {
                            d.fields.insert(key.into(), json!(text));
                        }
                    });
                }
            });
            if let Some(status) = &status {
                Text::new(status.clone()).tone(Tone::Bad).show(ui);
            }
        });
        None
    }
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

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("dc-package-dialog-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    type History = Vec<(String, Arc<designcraft_doc::Document>, designcraft_doc::Selection)>;
    #[derive(Clone)]
    struct QuietDoc {
        uid: u64,
        epoch: u64,
        revision: u64,
        saved_revision: u64,
        history_limit: usize,
        document: Arc<designcraft_doc::Document>,
        saved: Arc<designcraft_doc::Document>,
        undo: History,
        redo: History,
        selection: designcraft_doc::Selection,
        dirty: bool,
    }
    #[derive(Clone)]
    struct Quiet {
        documents: Vec<QuietDoc>,
        active: Option<u64>,
        journal: Vec<(String, Value)>,
    }
    fn snapshot(app: &DesignApp) -> Quiet {
        let history = |entries: &[designcraft_engine::HistoryEntry]| -> History {
            entries.iter().map(|e| (e.label.clone(), e.doc.clone(), e.selection.clone())).collect()
        };
        Quiet {
            documents: app
                .session
                .documents()
                .iter()
                .map(|st| QuietDoc {
                    uid: st.uid,
                    epoch: st.identity_epoch,
                    revision: st.revision,
                    saved_revision: st.saved_revision,
                    history_limit: st.history.limit,
                    document: st.doc.clone(),
                    saved: st.saved_doc.clone(),
                    undo: history(&st.history.undo),
                    redo: history(&st.history.redo),
                    selection: st.selection.clone(),
                    dirty: st.is_dirty(),
                })
                .collect(),
            active: app.session.active().map(|st| st.uid),
            journal: app.session.journal.clone(),
        }
    }
    fn assert_quiet(h: &Harness<'_, DesignApp>, before: &Quiet) {
        let now = snapshot(h.state());
        assert_eq!(now.active, before.active);
        assert_eq!(now.journal, before.journal, "navigation, canceled folders and draft editing execute no commands");
        assert_eq!(now.documents.len(), before.documents.len());
        for (now, before) in now.documents.iter().zip(&before.documents) {
            assert_eq!(
                (now.uid, now.epoch, now.revision, now.saved_revision, now.history_limit, now.dirty),
                (before.uid, before.epoch, before.revision, before.saved_revision, before.history_limit, before.dirty)
            );
            assert_eq!(now.document.as_ref(), before.document.as_ref());
            assert_eq!(now.saved.as_ref(), before.saved.as_ref());
            assert_eq!(now.selection, before.selection);
            assert_eq!(now.undo, before.undo);
            assert_eq!(now.redo, before.redo);
            assert!(Arc::ptr_eq(&now.document, &before.document), "even the live allocation is unchanged for every document");
            assert!(Arc::ptr_eq(&now.saved, &before.saved));
        }
    }
    fn app() -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".into();
        app.run("file.new", json!({"title": "Package fixture"})).unwrap();
        open(&mut app).unwrap();
        app
    }
    fn instructions_fixture() -> DesignApp {
        let mut app = app();
        let doc = app.session.active().unwrap().uid;
        // Explicit second-stage DTO fixture; the full journey below reaches this stage via Package….
        app.ui.dialog = Some(Dialog::new(INSTRUCTIONS, json!({"doc": doc, "idml": true, "pdf": false})));
        app
    }
    fn cancelled_folders(app: &mut DesignApp) -> Arc<AtomicUsize> {
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        let mut answer = crate::app::file_dialogs::testing::answer_with(None);
        app.answer_dialogs(move |request| {
            if matches!(request, craft_dialogs::Request::File(_)) {
                observed.fetch_add(1, Ordering::SeqCst);
            }
            answer(request)
        });
        requests
    }
    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        let normal_ui = app.ui.dialog.as_ref().is_some_and(|d| d.id == INSTRUCTIONS);
        harness_stage(app, normal_ui, false)
    }
    fn normal_harness(app: DesignApp) -> Harness<'static, DesignApp> {
        harness_stage(app, true, false)
    }
    fn harness_stage(mut app: DesignApp, normal_ui: bool, disabled: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(1100.0, 1000.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if normal_ui {
                    if disabled {
                        ui.disable();
                    }
                    app.ui(ui);
                    return;
                }
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }
    fn event(h: &mut Harness<'_, DesignApp>, event: Event) {
        h.input_mut().events.push(event);
        idle(h, 1);
    }
    fn idle(h: &mut Harness<'_, DesignApp>, frames: usize) {
        for _ in 0..frames {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }
    fn key(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        idle(h, 3);
    }
    fn click_at(h: &mut Harness<'_, DesignApp>, pos: egui::Pos2) {
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        idle(h, 3);
    }
    fn click(h: &mut Harness<'_, DesignApp>, role: Role, name: &str) {
        let pos = h.get_by_role_and_label(role, name).rect().center();
        click_at(h, pos);
    }
    fn page(h: &mut Harness<'_, DesignApp>, name: &str) {
        click(h, Role::ListItem, name);
    }
    #[track_caller]
    fn draft<'a>(h: &'a Harness<'_, DesignApp>) -> &'a Dialog {
        h.state().ui.dialog.as_ref().unwrap()
    }
    fn field_role(label: &str) -> Role {
        match label {
            "Address:" | "Instructions:" => Role::MultilineTextInput,
            _ => Role::TextInput,
        }
    }
    fn type_field(h: &mut Harness<'_, DesignApp>, label: &str, text: &str) {
        click(h, field_role(label), label);
        key(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(text.into()));
        idle(h, 3);
    }
    fn pick_pdf(h: &mut Harness<'_, DesignApp>, preset: &str) {
        click(h, Role::ComboBox, "PDF Preset:");
        let pos = h.get_by_label(preset).rect().center();
        click_at(h, pos);
        assert_eq!(draft(h).s("pdfPreset"), preset);
    }
    fn diagnostic_fixture(app: &mut DesignApp) {
        // Synthetic display fixture, NOT a genuine missing-link/font or packaging-I/O proof.
        app.ui.dialog.as_mut().unwrap().fields.extend(
            json!({
                "fonts": [
                    {"name":"Fixture Installed Font", "source":"installed", "status":"ok"},
                    {"name":"Fixture Missing Font", "source":"document", "status":"missing"},
                    {"name":"Fixture Substituted Font", "source":"bundled", "status":"substituted"}
                ],
                "links": [
                    {"name":"fixture-ok.png", "page":"1", "status":"ok"},
                    {"name":"fixture-missing.png", "page":"2", "status":"missing"},
                    {"name":"fixture-modified.png", "page":"3", "status":"modified"},
                    {"name":"fixture-embedded.png", "page":"4", "status":"embedded"}
                ],
                "spot": ["Fixture Spot Blue", "Fixture Spot Orange"]
            })
            .as_object()
            .unwrap()
            .clone(),
        );
    }

    #[test]
    fn sdk_package_diagnostic_pages_and_problem_filters_are_draft_only() {
        let mut app = app();
        diagnostic_fixture(&mut app);
        let before = snapshot(&app);
        let mut h = harness(app);
        let expected = vec![
            ("Fonts: 3 in use, 1 unavailable".to_owned(), true),
            ("Links and Images: 4 linked, 1 changed, 1 not found".to_owned(), true),
            ("Embedded images: 1".to_owned(), false),
            ("Colors and Inks: 4 process, 2 spot".to_owned(), false),
        ];
        assert_eq!(summary_lines("en", draft(&h)), expected);
        for (line, _) in &expected {
            assert!(h.get_by_label(line).rect().is_positive());
        }
        // A table row is a list item; its first cell carries the same name.
        page(&mut h, "Fonts");
        assert_eq!(draft(&h).s("page"), "fonts");
        assert!(h.query_by_role_and_label(Role::ListItem, "Fixture Installed Font").is_some());
        click(&mut h, Role::CheckBox, "Show Problems Only");
        assert!(draft(&h).b("problemsOnly"));
        assert!(h.query_by_role_and_label(Role::ListItem, "Fixture Installed Font").is_none());
        for label in ["Fixture Missing Font", "Fixture Substituted Font"] {
            assert!(h.get_by_role_and_label(Role::ListItem, label).rect().is_positive());
        }
        page(&mut h, "Links and Images");
        for label in ["fixture-ok.png", "fixture-embedded.png"] {
            assert!(h.query_by_role_and_label(Role::ListItem, label).is_none());
        }
        for label in ["fixture-missing.png", "fixture-modified.png"] {
            assert!(h.get_by_role_and_label(Role::ListItem, label).rect().is_positive());
        }
        click(&mut h, Role::CheckBox, "Show Problems Only");
        for label in ["fixture-ok.png", "fixture-embedded.png"] {
            assert!(h.get_by_role_and_label(Role::ListItem, label).rect().is_positive());
        }
        page(&mut h, "Colors and Inks");
        for label in ["Process Cyan", "Process Magenta", "Process Yellow", "Process Black", "Fixture Spot Blue", "Fixture Spot Orange"] {
            assert!(h.get_by_role_and_label(Role::ListItem, label).rect().is_positive());
        }
        page(&mut h, "Summary");
        assert_eq!(draft(&h).s("page"), "summary");
        assert_quiet(&h, &before);
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_inclusion_flags_and_pdf_preset_preserve_draft_mapping() {
        let app = app();
        let before = snapshot(&app);
        let mut h = harness(app);
        assert!(draft(&h).b("idml"));
        assert!(!draft(&h).b("pdf"));
        let initial = draft(&h).s("pdfPreset");
        assert!(h.get_by_role_and_label(Role::ComboBox, "PDF Preset:").accesskit_node().is_disabled());
        click(&mut h, Role::ComboBox, "PDF Preset:");
        assert_eq!(draft(&h).s("pdfPreset"), initial);
        assert!(h.query_by_role_and_label(Role::ListBox, "PDF Preset:").is_none());
        click(&mut h, Role::CheckBox, "Include IDML");
        click(&mut h, Role::CheckBox, "Create Printing Instructions");
        assert!(!draft(&h).b("idml"));
        assert!(draft(&h).b("instructions"));
        click(&mut h, Role::CheckBox, "Include PDF (Print)");
        assert!(!h.get_by_role_and_label(Role::ComboBox, "PDF Preset:").accesskit_node().is_disabled());
        pick_pdf(&mut h, "PDF/X-4");
        assert_eq!(pdf_param(draft(&h)), crate::dialogs::pdf_preset_params("PDF/X-4"));
        assert_eq!(pdf_param(draft(&h))["standard"], "x4");
        click(&mut h, Role::CheckBox, "Include PDF (Print)");
        assert_eq!(draft(&h).s("pdfPreset"), "PDF/X-4");
        assert_eq!(pdf_param(draft(&h)), json!(false));
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_two_stage_all_seven_fields_continue_to_actual_cancelled_folder() {
        let mut app = app();
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let uid = app.session.active().unwrap().uid;
        let mut h = normal_harness(app);
        click(&mut h, Role::CheckBox, "Create Printing Instructions");
        click(&mut h, Role::CheckBox, "Include IDML");
        click(&mut h, Role::CheckBox, "Include PDF (Print)");
        pick_pdf(&mut h, "PDF/X-4");
        click(&mut h, Role::Button, "Package…");
        assert_eq!(draft(&h).id, INSTRUCTIONS);
        assert_eq!(draft(&h).fields["doc"], uid);
        assert!(!draft(&h).b("idml"));
        assert_eq!(draft(&h).fields["pdf"], crate::dialogs::pdf_preset_params("PDF/X-4"));
        assert_eq!(requests.load(Ordering::SeqCst), 0, "stage one asks no folder when instructions were requested");
        for (key_name, label, text) in [
            ("contact", "Contact:", "Dana"),
            ("company", "Company:", "Print Lab"),
            ("address", "Address:", "1 Studio Lane\nTown"),
            ("phone", "Phone:", "+1 408 0100"),
            ("fax", "Fax:", "+1 408 0101"),
            ("email", "Email:", "press@example.test"),
            ("instructions", "Instructions:", "Keep plates separate.\nCall first."),
        ] {
            type_field(&mut h, label, text);
            assert_eq!(draft(&h).s(key_name), text);
            assert_eq!(h.get_by_role_and_label(field_role(label), label).value().as_deref(), Some(text));
        }
        assert_eq!(
            instructions_text(draft(&h)),
            "Contact: Dana\nCompany: Print Lab\nAddress: 1 Studio Lane\n  Town\nPhone: +1 408 0100\nFax: +1 408 0101\nEmail: press@example.test\n\nKeep plates separate.\nCall first.\n"
        );
        assert_quiet(&h, &before);
        click(&mut h, Role::Button, "Continue");
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(requests.load(Ordering::SeqCst), 1, "no idle folder or package replay");
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_direct_folder_cancel_is_once_and_never_packages() {
        let mut app = app();
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let mut h = harness(app);
        assert!(!draft(&h).b("instructions"));
        click(&mut h, Role::Button, "Package…");
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_both_stages_reject_stale_document_without_folder_or_write() {
        for second_stage in [false, true] {
            let mut app = app();
            let stale = app.ui.dialog.take().unwrap();
            let old_uid = app.session.active().unwrap().uid;
            app.run("file.new", json!({"title":"Other document"})).unwrap();
            assert_ne!(app.session.active().unwrap().uid, old_uid);
            // Explicit stale pending-dialog fixture; actual footer and unchanged validator must refuse it.
            let pending = if second_stage {
                Dialog::new(INSTRUCTIONS, json!({"doc": old_uid, "idml": true, "pdf": false, "contact":"Retain me", "instructions":"Do not package"}))
            } else {
                stale
            };
            let fields = pending.fields.clone();
            app.ui.dialog = Some(pending);
            let requests = cancelled_folders(&mut app);
            let before = snapshot(&app);
            let mut h = harness(app);
            click(&mut h, Role::Button, if second_stage { "Continue" } else { "Package…" });
            assert_eq!(draft(&h).fields, fields, "refusal retains all draft fields");
            assert_eq!(h.state().ui.status, "the active document changed after the Package dialog opened");
            h.state_mut().settle_dialogs();
            idle(&mut h, 3);
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert_quiet(&h, &before);
            click(&mut h, Role::Button, "Cancel");
            assert!(h.state().ui.dialog.is_none());
            assert_quiet(&h, &before);
        }
    }

    #[test]
    fn sdk_package_natural_tab_and_multiline_enter_do_not_continue() {
        let mut app = instructions_fixture();
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let mut h = harness(app);
        click(&mut h, Role::TextInput, "Contact:");
        assert!(h.get_by_role_and_label(Role::TextInput, "Contact:").is_focused());
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert!(h.get_by_role_and_label(Role::TextInput, "Company:").is_focused());
        event(&mut h, Event::Text("Keyboard company".into()));
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert!(h.get_by_role_and_label(Role::MultilineTextInput, "Address:").is_focused());
        event(&mut h, Event::Text("Line one".into()));
        key(&mut h, Key::Enter, Modifiers::NONE);
        event(&mut h, Event::Text("Line two".into()));
        idle(&mut h, 3);
        assert_eq!(draft(&h).s("company"), "Keyboard company");
        assert_eq!(draft(&h).s("address"), "Line one\nLine two");
        assert_eq!(draft(&h).id, INSTRUCTIONS);
        type_field(&mut h, "Instructions:", "Proof");
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(draft(&h).s("instructions"), "Proof\n");
        assert_eq!(draft(&h).id, INSTRUCTIONS);
        h.state_mut().settle_dialogs();
        assert_eq!(requests.load(Ordering::SeqCst), 0, "real multiline Enter is local, never Continue");
        assert_quiet(&h, &before);
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
        // No host-Escape assertion: the shared footer/key adapter is outside this file's lease.
    }

    #[test]
    fn sdk_package_long_contact_company_and_instructions_retain_exact_text() {
        let mut app = instructions_fixture();
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let mut h = harness(app);
        let contact = "C".repeat(1300);
        let company = "D".repeat(1500);
        let instructions = format!("{}\nThen call.", "Keep inks separate. ".repeat(80));
        for (key_name, label, text) in
            [("contact", "Contact:", &contact), ("company", "Company:", &company), ("instructions", "Instructions:", &instructions)]
        {
            assert!(text.chars().count() > 1024);
            type_field(&mut h, label, text);
            assert_eq!(draft(&h).s(key_name), *text);
            assert_eq!(h.get_by_role_and_label(field_role(label), label).value().as_deref(), Some(text.as_str()));
        }
        assert_eq!(instructions_text(draft(&h)), format!("Contact: {contact}\nCompany: {company}\n\n{instructions}\n"));
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        h.state_mut().settle_dialogs();
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_normal_single_line_enter_continues_once_to_cancelled_folder() {
        let mut app = instructions_fixture();
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let mut h = normal_harness(app);
        click(&mut h, Role::TextInput, "Company:");
        assert!(h.get_by_role_and_label(Role::TextInput, "Company:").is_focused());
        event(&mut h, Event::Text("Keyboard company".into()));
        assert_eq!(draft(&h).s("company"), "Keyboard company");
        key(&mut h, Key::Enter, Modifiers::NONE);
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(requests.load(Ordering::SeqCst), 1, "untaken single-line Submit takes the real default Continue once");
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(requests.load(Ordering::SeqCst), 1, "released/idle input never requests a second folder");
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_normal_pointer_cancel_and_quiet_escape_are_no_io() {
        for pointer_cancel in [true, false] {
            let mut app = instructions_fixture();
            let requests = cancelled_folders(&mut app);
            let before = snapshot(&app);
            let mut h = normal_harness(app);
            if pointer_cancel {
                click(&mut h, Role::Button, "Cancel");
            } else {
                // Leave every clean field by natural Tab. Either footer button may be first
                // in the platform's display order; neither owns a text-edit Escape.
                click(&mut h, Role::TextInput, "Contact:");
                assert!(h.get_by_role_and_label(Role::TextInput, "Contact:").is_focused());
                for _ in 0..7 {
                    key(&mut h, Key::Tab, Modifiers::NONE);
                }
                assert!(
                    h.get_by_role_and_label(Role::Button, "Continue").is_focused() || h.get_by_role_and_label(Role::Button, "Cancel").is_focused(),
                    "quiet Escape starts on a real footer button, outside a text editor"
                );
                key(&mut h, Key::Escape, Modifiers::NONE);
            }
            assert!(h.state().ui.dialog.is_none());
            h.state_mut().settle_dialogs();
            idle(&mut h, 3);
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert_quiet(&h, &before);
        }
    }

    #[test]
    fn sdk_package_normal_disabled_instructions_cannot_continue_or_write() {
        let mut app = instructions_fixture();
        let requests = cancelled_folders(&mut app);
        let fields = app.ui.dialog.as_ref().unwrap().fields.clone();
        let before = snapshot(&app);
        let mut h = harness_stage(app, true, true);
        for (_, label, _) in INSTRUCTION_FIELDS {
            assert!(h.get_by_role_and_label(field_role(label), label).accesskit_node().is_disabled());
        }
        for label in ["Continue", "Cancel"] {
            assert!(h.get_by_role_and_label(Role::Button, label).accesskit_node().is_disabled());
        }
        click(&mut h, Role::Button, "Continue");
        assert_eq!(draft(&h).fields, fields, "disabled pointer activation keeps the complete draft");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_quiet(&h, &before);
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(draft(&h).fields, fields, "disabled Enter keeps the complete draft before settlement");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_quiet(&h, &before);
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(draft(&h).fields, fields);
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_normal_held_enter_does_not_replay_across_instructions_transition() {
        let mut app = app();
        // A valid Main-stage draft, without leaving a checkbox as the Enter owner.
        app.ui.dialog.as_mut().unwrap().fields.insert("instructions".into(), json!(true));
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let mut h = normal_harness(app);
        assert!(draft(&h).b("instructions"));
        event(&mut h, Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        assert_eq!(draft(&h).id, INSTRUCTIONS, "stage one enters Instructions, not the folder picker");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        idle(&mut h, 2);
        for _ in 0..3 {
            event(&mut h, Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE });
            assert_eq!(draft(&h).id, INSTRUCTIONS, "the held transition key is not a new Continue");
            assert_eq!(requests.load(Ordering::SeqCst), 0);
        }
        event(&mut h, Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        idle(&mut h, 3);
        assert_eq!(draft(&h).id, INSTRUCTIONS);
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_quiet(&h, &before);
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_normal_composition_enter_does_not_activate_continue() {
        let mut app = instructions_fixture();
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let mut h = normal_harness(app);
        click(&mut h, Role::TextInput, "Company:");
        assert!(h.get_by_role_and_label(Role::TextInput, "Company:").is_focused());
        event(&mut h, Event::Ime(egui::ImeEvent::Enabled));
        event(&mut h, Event::Ime(egui::ImeEvent::Preedit { text: "Composing company".into(), active_range_chars: None }));
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(draft(&h).id, INSTRUCTIONS);
        assert_eq!(requests.load(Ordering::SeqCst), 0, "composition owns Enter, not the default footer");
        assert_quiet(&h, &before);
        event(&mut h, Event::Ime(egui::ImeEvent::Commit("Composed company".into())));
        event(&mut h, Event::Ime(egui::ImeEvent::Disabled));
        idle(&mut h, 3);
        assert_eq!(draft(&h).s("company"), "Composed company");
        assert_eq!(h.get_by_role_and_label(Role::TextInput, "Company:").value().as_deref(), Some("Composed company"));
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_quiet(&h, &before);
    }

    #[test]
    fn sdk_package_normal_refusal_retains_long_draft_options_status_and_dialog_scope() {
        let mut app = instructions_fixture();
        let mut pending = app.ui.dialog.take().unwrap();
        pending.fields.insert("idml".into(), json!(false));
        pending.fields.insert("pdf".into(), crate::dialogs::pdf_preset_params("PDF/X-4"));
        // Same explicit stale-document fixture as the existing both-stage validator journey.
        app.run("file.new", json!({"title":"Other document"})).unwrap();
        app.ui.dialog = Some(pending);
        let requests = cancelled_folders(&mut app);
        let before = snapshot(&app);
        let mut h = normal_harness(app);
        let long_contact = format!("Contact {}", "C".repeat(2300));
        let long_company = format!("Company {}", "O".repeat(2400));
        let long_instructions = format!("Proof {}\nCall first.", "I".repeat(2600));
        for (label, text) in [("Contact:", &long_contact), ("Company:", &long_company), ("Instructions:", &long_instructions)] {
            type_field(&mut h, label, text);
            assert_eq!(h.get_by_role_and_label(field_role(label), label).value().as_deref(), Some(text.as_str()));
        }
        let fields = draft(&h).fields.clone();
        let modal_id = h.get_by_role_and_label(Role::Dialog, "Printing Instructions").accesskit_node().id();
        let accept_id = h.get_by_role_and_label(Role::Button, "Continue").accesskit_node().id();
        let assert_refusal = |h: &Harness<'_, DesignApp>| {
            assert_eq!(draft(h).fields, fields, "refusal keeps every unsaved field and option");
            assert_eq!(h.state().ui.status, "the active document changed after the Package dialog opened");
            assert_eq!(
                h.get_by_role_and_label(Role::Dialog, "Printing Instructions").accesskit_node().id(),
                modal_id,
                "refusal never closes/reopens the dialog"
            );
            assert_eq!(h.get_by_role_and_label(Role::Button, "Continue").accesskit_node().id(), accept_id);
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert_quiet(h, &before);
        };
        let modal_focus = |h: &Harness<'_, DesignApp>| {
            let modal = h.get_by_role_and_label(Role::Dialog, "Printing Instructions");
            modal.query_all_by(|node| node.is_focused()).map(|node| node.accesskit_node().id()).collect::<Vec<_>>()
        };

        // A press on a footer button keeps a dialog's focus inside it: the focused text field keeps it (next's press
        // rule in a trapping scope). Keep its genuine refusal, draft/status/identity/domain/IO coverage; do not count the
        // outer Window as modal focus.
        for _ in 0..2 {
            h.state_mut().ui.status = "pointer refusal invocation sentinel".into();
            click(&mut h, Role::Button, "Continue");
            assert_refusal(&h);
            h.state_mut().settle_dialogs();
            idle(&mut h, 3);
            assert_refusal(&h);
        }

        // Before ANY Tab after the pointer path, a fresh ordinary Enter must still reach
        // this existing modal's default/refusal route. A stale status cannot prove that call.
        // The pointer path left focus in the Instructions field, where Enter is a new line, as typed text: the Enter goes
        // from a single-line field, which the pointer puts focus in, as a person clicks back into the form.
        click(&mut h, Role::TextInput, "Contact:");
        assert!(h.get_by_role_and_label(Role::TextInput, "Contact:").is_focused());
        assert_refusal(&h);
        h.state_mut().ui.status = "modal default routing invocation sentinel".into();
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_refusal(&h);
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert_refusal(&h);

        // Natural navigation, never a focus request. Every Tab must land on a real modal
        // descendant, not an outside stop or the Window-root fallback. Bound the whole cycle.
        for _ in 0..INSTRUCTION_FIELDS.len() + 2 {
            if h.get_by_role_and_label(Role::Button, "Continue").is_focused() {
                break;
            }
            key(&mut h, Key::Tab, Modifiers::NONE);
            let focus = modal_focus(&h);
            assert_eq!(focus.len(), 1, "Tab stays on a real stop inside the existing modal");
            let all_focus: Vec<_> = h.query_all_by(|node| node.is_focused()).map(|node| node.accesskit_node().id()).collect();
            assert_eq!(all_focus, focus, "Tab must never escape to an outside control");
        }
        assert!(h.get_by_role_and_label(Role::Button, "Continue").is_focused(), "natural Tab reaches Continue within one modal cycle");
        // An already naturally focused Continue is a Keep target for pointer activation;
        // Enter on that same real stop is the separate keyboard-retention journey.
        for pointer in [true, false] {
            let retained_focus = modal_focus(&h);
            assert_eq!(retained_focus.len(), 1, "refusal starts with one real focused modal descendant");
            assert_eq!(retained_focus, vec![accept_id], "capture the actual focused Continue identity BEFORE either refusal");
            for _ in 0..2 {
                h.state_mut().ui.status = "focused refusal invocation sentinel".into();
                if pointer {
                    click(&mut h, Role::Button, "Continue");
                } else {
                    key(&mut h, Key::Enter, Modifiers::NONE);
                }
                assert_refusal(&h);
                let focus = modal_focus(&h);
                assert_eq!(focus.len(), 1, "the refused dialog retains real keyboard focus");
                assert_eq!(focus, retained_focus, "a repeated refusal keeps the existing focus, not a reopened scope");
                let all_focus: Vec<_> = h.query_all_by(|node| node.is_focused()).map(|node| node.accesskit_node().id()).collect();
                assert_eq!(all_focus, focus, "the retained keyboard stop is inside the modal, never outside");
                h.state_mut().settle_dialogs();
                idle(&mut h, 3);
                assert_refusal(&h);
                assert_eq!(modal_focus(&h), retained_focus, "settlement and idle preserve that same real modal stop");
            }
        }
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_quiet(&h, &before);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_tests;
