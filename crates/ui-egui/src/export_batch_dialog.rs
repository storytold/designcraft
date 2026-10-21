//! File › Export Layouts and Versions…: pick the layouts, the versions (each a format and its
//! settings, written to a folder of its own) and the file names, see the files it will write,
//! and keep the setup as a preset.

use designcraft_engine::cmd::export_batch::{ExportFormat, ExportJob, ExportPreset, ExportVersion, builtin_presets, version_templates};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;

pub const ID: &str = "exportBatch";
/// The most files the preview lists.
const PREVIEW_ROWS: usize = 200;

/// Open the dialog: the last job used (or the Print + Screen preset), on `layouts` when given.
pub fn open(app: &mut DesignApp, layouts: Option<Vec<String>>) {
    let names: Vec<String> = app.session.active().map(|st| st.doc.layouts().into_iter().map(|l| l.name).collect()).unwrap_or_default();
    let (mut job, preset) = match app.ui.export_batch.clone() {
        Some(j) => (j, String::new()),
        None => {
            let p = builtin_presets().into_iter().next().unwrap_or_default();
            (p.job, p.name)
        }
    };
    if let Some(l) = layouts {
        job.layouts = l;
    }
    job.layouts.retain(|l| names.contains(l));
    if job.layouts.len() == names.len() {
        job.layouts.clear();
    }
    app.ui.dialog = Some(Dialog::new(ID, json!({"job": job, "preset": preset})));
}

fn job_of(d: &Dialog) -> ExportJob {
    d.fields.get("job").and_then(|j| serde_json::from_value(j.clone()).ok()).unwrap_or_default()
}

fn presets(app: &DesignApp) -> Vec<ExportPreset> {
    builtin_presets().into_iter().chain(app.session.prefs.export_presets.iter().cloned()).collect()
}

/// A PDF version's settings as one of the quick choices, or "custom".
fn pdf_kind(v: &ExportVersion) -> &'static str {
    let m = v.marks;
    let no_marks = !m.crop && !m.bleed && !m.page_info;
    let plain = v.standard == "none" && !v.spreads;
    match (v.bleed, m.crop && m.bleed && m.page_info, no_marks, v.compress_images, plain) {
        (true, true, _, false, true) => "print",
        (true, false, true, false, true) => "bleed",
        (false, false, true, true, true) => "screen",
        _ => "custom",
    }
}

fn set_pdf_kind(v: &mut ExportVersion, kind: &str) {
    let marks = designcraft_engine::cmd::export_batch::VersionMarks { crop: true, bleed: true, page_info: true };
    let none = designcraft_engine::cmd::export_batch::VersionMarks::default();
    (v.bleed, v.marks, v.compress_images, v.standard, v.spreads) = match kind {
        "print" => (true, marks, false, "none".into(), false),
        "bleed" => (true, none, false, "none".into(), false),
        "screen" => (false, none, true, "none".into(), false),
        _ => return,
    };
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let t = crate::theme::Tokens::get(ui.ctx());
    let mut job = job_of(d);
    let before = job.clone();
    let layout_names: Vec<String> = app.session.active().map(|st| st.doc.layouts().into_iter().map(|l| l.name).collect()).unwrap_or_default();
    let section = |ui: &mut egui::Ui, title: &str| {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(tr(title)).strong());
    };

    // ---- Preset.
    let all = presets(app);
    let mut preset = d.s("preset");
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Preset:"));
        let shown = if preset.is_empty() { tr("[Custom]") } else { preset.clone() };
        egui::ComboBox::from_id_salt("export_batch_preset").selected_text(shown).width(200.0).show_ui(ui, |ui| {
            for p in &all {
                if ui.selectable_label(p.name == preset, &p.name).clicked() {
                    job = p.job.clone();
                    preset = p.name.clone();
                }
            }
        });
        let saved = app.session.prefs.export_presets.iter().any(|p| p.name == preset);
        if saved && ui.button(crate::rtl::widget(ui, tr("Delete Preset"))).clicked() {
            match app.run("file.deleteExportPreset", json!({"name": preset})) {
                Ok(_) => preset.clear(),
                Err(e) => {
                    d.fields.insert("status".into(), json!(e));
                }
            }
        }
    });
    ui.horizontal(|ui| {
        let mut name = d.s("presetName");
        let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0).hint_text(tr("Preset name")));
        if field.changed() {
            d.fields.insert("presetName".into(), json!(name));
        }
        if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(crate::rtl::widget(ui, tr("Save Preset")))).clicked() {
            match app.run("file.saveExportPreset", json!({"name": name.trim(), "job": job})) {
                Ok(_) => {
                    preset = name.trim().to_string();
                    d.fields.insert("presetName".into(), json!(""));
                    d.fields.insert("status".into(), json!(tr("Preset saved.")));
                }
                Err(e) => {
                    d.fields.insert("status".into(), json!(e));
                }
            }
        }
    });

    // ---- Layouts.
    section(ui, "Layouts");
    if layout_names.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for name in &layout_names {
                let mut on = job.layouts.is_empty() || job.layouts.contains(name);
                if ui.checkbox(&mut on, name.as_str()).changed() {
                    let mut chosen: Vec<String> = if job.layouts.is_empty() { layout_names.clone() } else { job.layouts.clone() };
                    if on {
                        chosen.push(name.clone());
                    } else {
                        chosen.retain(|x| x != name);
                    }
                    // In document order; every layout chosen is "all" (it follows new layouts too).
                    let ordered: Vec<String> = layout_names.iter().filter(|n| chosen.contains(n)).cloned().collect();
                    job.layouts = if ordered.len() == layout_names.len() {
                        vec![]
                    } else if ordered.is_empty() {
                        vec![String::new()]
                    } else {
                        ordered
                    };
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.radio(!job.combine, crate::rtl::widget(ui, tr("A file per layout"))).clicked() {
                job.combine = false;
            }
            if ui.radio(job.combine, crate::rtl::widget(ui, tr("All layouts in one PDF"))).clicked() {
                job.combine = true;
            }
        });
    } else {
        crate::rtl::label(ui, egui::RichText::new(tr("The document has one layout: all its pages are exported.")).color(t.text_dim));
    }

    // ---- Versions.
    section(ui, "Versions");
    let mut remove = None;
    let mut duplicate = None;
    let mut expanded: Vec<u64> = d.fields.get("expanded").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    for (i, v) in job.versions.iter_mut().enumerate() {
        ui.push_id(("export_version", i), |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut v.name).desired_width(170.0).hint_text(tr("Folder name")));
                egui::ComboBox::from_id_salt("format").selected_text(v.format.ext().to_uppercase().replace("JPG", "JPEG")).width(70.0).show_ui(
                    ui,
                    |ui| {
                        for (f, label) in [(ExportFormat::Pdf, "PDF"), (ExportFormat::Png, "PNG"), (ExportFormat::Jpeg, "JPEG")] {
                            if ui.selectable_label(v.format == f, label).clicked() {
                                v.format = f;
                            }
                        }
                    },
                );
                match v.format {
                    ExportFormat::Pdf => {
                        let kind = if expanded.contains(&(i as u64)) { "custom" } else { pdf_kind(v) };
                        let label = |k: &str| match k {
                            "print" => tr("Print: marks + bleed"),
                            "bleed" => tr("Bleed, no marks"),
                            "screen" => tr("Screen"),
                            _ => tr("Custom"),
                        };
                        egui::ComboBox::from_id_salt("pdf_kind").selected_text(label(kind)).width(150.0).show_ui(ui, |ui| {
                            for k in ["print", "bleed", "screen", "custom"] {
                                if ui.selectable_label(kind == k, label(k)).clicked() {
                                    expanded.retain(|x| *x != i as u64);
                                    if k == "custom" {
                                        expanded.push(i as u64);
                                    } else {
                                        set_pdf_kind(v, k);
                                    }
                                }
                            }
                        });
                    }
                    ExportFormat::Png | ExportFormat::Jpeg => {
                        ui.add(egui::DragValue::new(&mut v.ppi).range(36.0..=1200.0).suffix(" ppi").speed(1.0));
                        if v.format == ExportFormat::Jpeg {
                            let mut q = u32::from(v.quality);
                            if ui.add(egui::DragValue::new(&mut q).range(1..=100).prefix(tr("Quality ")).speed(1.0)).changed() {
                                v.quality = u8::try_from(q.clamp(1, 100)).unwrap_or(85);
                            }
                        }
                    }
                }
                if ui.small_button(tr("Duplicate")).on_hover_text(tr("Duplicate version")).clicked() {
                    duplicate = Some(i);
                }
                if crate::icons::button(ui, "trash", 18.0, false, &tr("Remove version")).clicked() {
                    remove = Some(i);
                }
            });
            if v.format == ExportFormat::Pdf && (expanded.contains(&(i as u64)) || pdf_kind(v) == "custom") {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(12.0);
                    ui.checkbox(&mut v.bleed, crate::rtl::widget(ui, tr("Bleed")));
                    ui.checkbox(&mut v.marks.crop, crate::rtl::widget(ui, tr("Crop Marks")));
                    ui.checkbox(&mut v.marks.bleed, crate::rtl::widget(ui, tr("Bleed Marks")));
                    ui.checkbox(&mut v.marks.page_info, crate::rtl::widget(ui, tr("Page Information")));
                    ui.checkbox(&mut v.compress_images, crate::rtl::widget(ui, tr("Compress Images")));
                    ui.checkbox(&mut v.spreads, crate::rtl::widget(ui, tr("Spreads")));
                    egui::ComboBox::from_id_salt("standard")
                        .selected_text(match v.standard.as_str() {
                            "x4" => "PDF/X-4",
                            "a2b" => "PDF/A-2b",
                            _ => "No standard",
                        })
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for (s, l) in [("none", "No standard"), ("x4", "PDF/X-4"), ("a2b", "PDF/A-2b")] {
                                if ui.selectable_label(v.standard == s, l).clicked() {
                                    v.standard = s.into();
                                }
                            }
                        });
                });
            }
        });
    }
    if let Some(i) = duplicate
        && let Some(v) = job.versions.get(i).cloned()
    {
        let mut copy = v;
        copy.name = format!("{} copy", copy.name);
        job.versions.insert(i.saturating_add(1), copy);
        expanded.clear();
    }
    if let Some(i) = remove
        && i < job.versions.len()
    {
        job.versions.remove(i);
        expanded.clear();
    }
    d.fields.insert("expanded".into(), json!(expanded));
    ui.horizontal(|ui| {
        let add = ui.button(crate::rtl::widget(ui, tr("+ Add Version")));
        egui::Popup::menu(&add).show(|ui| {
            for v in version_templates() {
                if ui.button(&v.name).clicked() {
                    job.versions.push(v);
                    ui.close();
                }
            }
        });
        ui.checkbox(&mut job.folders, crate::rtl::widget(ui, tr("Put each version in its own folder")));
    });

    // ---- File names.
    section(ui, "File Names");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut job.file_name).desired_width(240.0).id_salt("export_batch_name"));
        for token in ["{document}", "{layout}", "{version}", "{date}"] {
            if ui.small_button(token).on_hover_text(tr("Add to the name")).clicked() {
                job.file_name.push_str(if job.file_name.is_empty() || job.file_name.ends_with(' ') { "" } else { " " });
                job.file_name.push_str(token);
            }
        }
    });

    // ---- Preview: the files it will write.
    if job != before {
        preset = all.iter().find(|p| p.job == job).map(|p| p.name.clone()).unwrap_or_default();
    }
    let key = serde_json::to_string(&job).unwrap_or_default();
    if d.s("previewFor") != key {
        let preview = if job.layouts.iter().any(String::is_empty) {
            Err(tr("Choose at least one layout."))
        } else {
            app.session.execute("file.exportBatch", &json!({"job": job, "dryRun": true})).map_err(|e| e.to_string())
        };
        let (files, error) = match preview {
            Ok(r) => {
                (r["files"].as_array().map(|a| a.iter().filter_map(|f| f["path"].as_str().map(str::to_string)).collect()).unwrap_or_default(), None)
            }
            Err(e) => (Vec::<String>::new(), Some(e)),
        };
        d.fields.insert("preview".into(), json!(files));
        d.fields.insert("previewError".into(), json!(error));
        d.fields.insert("previewFor".into(), json!(key));
    }
    let files: Vec<String> = d.fields.get("preview").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    ui.add_space(6.0);
    match d.fields.get("previewError").and_then(Value::as_str) {
        Some(e) => {
            crate::rtl::label(ui, egui::RichText::new(e).color(ui.visuals().error_fg_color));
        }
        None => {
            crate::rtl::label(ui, egui::RichText::new(tr("Files to export: {n}").replace("{n}", &files.len().to_string())).strong());
            egui::ScrollArea::vertical().id_salt("export_batch_preview").max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
                for f in files.iter().take(PREVIEW_ROWS) {
                    ui.label(egui::RichText::new(f).monospace().size(11.0).color(t.text_dim));
                }
                if files.len() > PREVIEW_ROWS {
                    ui.label(format!("… {}", files.len() - PREVIEW_ROWS));
                }
            });
        }
    }
    if let Some(status) = d.fields.get("status").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        crate::rtl::label(ui, egui::RichText::new(status).color(t.text_dim));
    }
    d.fields.insert("job".into(), json!(job));
    d.fields.insert("preset".into(), json!(preset));
}

/// Export: ask for a folder (the web downloads each file), write every file, remember the job.
pub fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let job = job_of(d);
    let reopen = |app: &mut DesignApp, e: &str| {
        let mut d = d.clone();
        d.fields.insert("status".into(), json!(e));
        app.ui.dialog = Some(d);
    };
    if job.layouts.iter().any(String::is_empty) {
        let e = crate::i18n::tr(&app.ui.language, "Choose at least one layout.").to_string();
        reopen(app, &e);
        return Err(e);
    }
    if job.versions.is_empty() {
        let e = crate::i18n::tr(&app.ui.language, "Add at least one version.").to_string();
        reopen(app, &e);
        return Err(e);
    }
    app.ui.export_batch = Some(job.clone());
    let title = crate::i18n::tr(&app.ui.language, "Export to Folder").to_string();
    let dir = match app.services.pick_folder.as_mut() {
        Some(pick) => match pick(&title) {
            Some(dir) => Some(dir),
            None => return Ok(Value::Null),
        },
        None => None,
    };
    let mut params = json!({"job": job});
    if let Some(dir) = &dir {
        params["dir"] = json!(dir);
    }
    let r = match app.run("file.exportBatch", params) {
        Ok(r) => r,
        Err(e) => {
            reopen(app, &e);
            return Err(e);
        }
    };
    let files = r["files"].as_array().cloned().unwrap_or_default();
    if dir.is_none() {
        // No folders on the web: each file downloads as "Folder - Name.ext".
        let write = app.services.write.as_mut().ok_or("no writer")?;
        for f in &files {
            let (Some(path), Some(b64)) = (f["path"].as_str(), f["base64"].as_str()) else { continue };
            write(&path.replace('/', " - "), &designcraft_engine::cmd::base64_decode(b64))?;
        }
    }
    app.ui.status = crate::i18n::tr(&app.ui.language, "Exported {n} files").replace("{n}", &files.len().to_string());
    Ok(r)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 2, "facingPages": false, "width": 300, "height": 400, "title": "Mag"})).unwrap();
        app.run("layout.createAlternate", json!({"name": "Tablet H", "width": 400, "height": 300, "fromName": "Print"})).unwrap();
        app
    }

    #[test]
    fn the_dialog_lists_the_files_and_edits_the_versions() {
        let mut app = app();
        app.run("app.exportBatch", json!({})).unwrap();
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.id.as_str(), d.s("preset").as_str()), (ID, "Print + Screen"));
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        h.run_steps(4);
        // Print + Screen: two PDFs a version, a PNG a page.
        assert!(h.query_by_label("Files to export: 8").is_some());
        assert!(h.query_by_label("PNG/Mag - Tablet H - 2.png").is_some());
        // Remove the PNG version, duplicate the first: the preset becomes [Custom].
        h.get_all_by_label("Remove version").last().unwrap().click();
        h.run_steps(3);
        h.get_all_by_label("Duplicate").next().unwrap().click();
        h.run_steps(3);
        let d = h.state().app.ui.dialog.clone().unwrap();
        let job = job_of(&d);
        assert_eq!(
            job.versions.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["PDF Print (marks + bleed)", "PDF Print (marks + bleed) copy", "PDF Screen"]
        );
        assert_eq!(d.s("preset"), "");
        assert!(h.query_by_label("Files to export: 6").is_some());
        // One layout only.
        h.get_by_label("Tablet H").click();
        h.run_steps(3);
        assert!(h.query_by_label("Files to export: 3").is_some());
        assert_eq!(job_of(h.state().app.ui.dialog.as_ref().unwrap()).layouts, ["Print"]);
    }

    #[test]
    fn export_writes_into_the_chosen_folder_and_remembers_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        let folder = dir.path().to_string_lossy().to_string();
        app.services.pick_folder = Some(Box::new(move |_| Some(folder.clone())));
        open(&mut app, Some(vec!["Tablet H".into()]));
        let r = crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(r["files"].as_array().unwrap().len(), 1 + 1 + 2);
        assert!(dir.path().join("PDF Print (marks + bleed)/Mag - Tablet H.pdf").is_file());
        assert!(dir.path().join("PNG/Mag - Tablet H - 1.png").is_file());
        assert_eq!(app.ui.export_batch.as_ref().unwrap().layouts, ["Tablet H"]);
        // Reopened, it is the same job.
        open(&mut app, None);
        assert_eq!(job_of(app.ui.dialog.as_ref().unwrap()).layouts, ["Tablet H"]);
    }

    #[test]
    fn a_cancelled_folder_writes_nothing_and_the_web_downloads_each_file() {
        let written = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut app = app();
        app.services.pick_folder = Some(Box::new(|_| None));
        let w = written.clone();
        app.services.write = Some(Box::new(move |p: &str, _: &[u8]| {
            w.lock().unwrap().push(p.to_string());
            Ok(())
        }));
        open(&mut app, None);
        assert_eq!(crate::dialogs::confirm(&mut app).unwrap(), Value::Null);
        assert!(written.lock().unwrap().is_empty());
        app.services.pick_folder = None;
        open(&mut app, Some(vec!["Print".into()]));
        crate::dialogs::confirm(&mut app).unwrap();
        let names = written.lock().unwrap().clone();
        assert_eq!(names.len(), 1 + 1 + 2);
        assert!(names.contains(&"PDF Screen - Mag - Print.pdf".to_string()), "{names:?}");
    }

    #[test]
    fn presets_are_saved_and_chosen() {
        let mut app = app();
        open(&mut app, None);
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        h.run_steps(3);
        if let Some(d) = h.state_mut().app.ui.dialog.as_mut() {
            d.fields.insert("presetName".into(), json!("Client"));
        }
        h.run_steps(2);
        h.get_by_label("Save Preset").click();
        h.run_steps(3);
        let app = &h.state().app;
        assert_eq!(app.session.prefs.export_presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Client"]);
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("preset"), "Client");
        assert!(h.query_by_label("Delete Preset").is_some());
    }

    #[test]
    fn print_with_no_layout_chosen_keeps_the_dialog_open_and_says_why() {
        let mut app = app();
        crate::dialogs::open_print(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("range".into(), json!("layouts"));
            d.fields.insert("layouts".into(), json!([]));
        }
        // Refused before anything is printed.
        assert!(crate::dialogs::confirm(&mut app).is_err());
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.id.as_str(), d.s("status").as_str()), ("print", "Choose at least one layout."));
    }

    #[test]
    fn clicking_a_layout_header_leaves_edit_parents() {
        let mut app = app();
        app.ui.dock_tab = "pages".into();
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        h.run_steps(3);
        h.get_by_label("Tablet H").click();
        h.run_steps(4);
        let app = &h.state().app;
        assert!(!app.session.active().unwrap().editing_parents);
        assert_eq!(crate::canvas::current_page(app), Some(2));
    }

    #[test]
    fn no_layout_chosen_keeps_the_dialog_open() {
        let mut app = app();
        open(&mut app, None);
        if let Some(d) = app.ui.dialog.as_mut() {
            let mut job = job_of(d);
            job.layouts = vec![String::new()];
            d.fields.insert("job".into(), json!(job));
        }
        assert!(crate::dialogs::confirm(&mut app).is_err());
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.id == ID));
    }
}
