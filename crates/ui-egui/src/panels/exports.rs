//! Window ▸ Exports: every background export, how far it is, and a Cancel button per running job.
//! "Every" includes jobs queued through the control channel: `DesignApp::run` registers each one,
//! so an agent's `file.exportPdf {background: true}` shows up here beside the menu's exports.

use serde_json::json;

use crate::DesignApp;
use crate::theme::{Tokens, semibold};

/// The Exports panel: what each background export is doing, and a Cancel button per running job.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let jobs = app.ui.export_jobs.clone();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::i18n::tr(&lang, "Exports")).font(semibold(12.5)).size(12.5).color(t.accent));
        if jobs.iter().any(|j| !j.busy) && ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Clear finished"))).clicked() {
            app.ui.export_jobs.retain(|j| j.busy);
        }
    });
    ui.add_space(4.0);
    if jobs.is_empty() {
        ui.label(egui::RichText::new(crate::i18n::tr(&lang, "No exports are running.")).size(11.0).color(t.text_dim));
        return;
    }
    egui::ScrollArea::vertical().id_salt("exports_panel").auto_shrink([false, false]).show(ui, |ui| {
        for job in &jobs {
            row(app, ui, job, &t, &lang);
            ui.add_space(6.0);
        }
    });
}

/// One job: its document, where it goes, how far it is, and Cancel or the result.
fn row(app: &mut DesignApp, ui: &mut egui::Ui, job: &crate::state::ExportJob, t: &Tokens, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&job.doc_title).font(semibold(12.0)).size(12.0).color(t.text));
            ui.label(egui::RichText::new(job.format.clone()).size(10.0).color(t.text_dim));
        });
        if !job.path.is_empty() {
            ui.label(egui::RichText::new(&job.path).size(10.0).color(t.text_dim));
        }
        if job.busy {
            let fraction = job.fraction();
            let text = if job.phase == "pages" && job.total > 0 {
                let percent = (fraction * 100.0).round() as i64;
                tr("Page {page} of {total} ({percent}%)")
                    .replace("{page}", &(job.done + 1).min(job.total).to_string())
                    .replace("{total}", &job.total.to_string())
                    .replace("{percent}", &percent.to_string())
            } else {
                phase_label(lang, &job.phase)
            };
            ui.add(egui::ProgressBar::new(fraction).desired_height(10.0).text(text));
            if job.phase == "pages" && !job.page.is_empty() {
                ui.label(egui::RichText::new(tr("Sheet {page}").replace("{page}", &job.page)).size(10.0).color(t.text_dim));
            }
            ui.horizontal(|ui| {
                if job.cancelling {
                    ui.label(egui::RichText::new(tr("Cancelling…")).size(10.5).color(t.text_dim));
                } else if ui.add(egui::Button::new(crate::rtl::widget(ui, tr("Cancel"))).small()).clicked() {
                    let _ = app.run("file.export.cancel", json!({"job": job.id}));
                    if let Some(j) = app.ui.export_jobs.iter_mut().find(|j| j.id == job.id) {
                        j.cancelling = true;
                    }
                }
            });
        } else if let Some(result) = &job.result {
            if let Some(e) = &result.error {
                ui.label(egui::RichText::new(tr("Failed: {error}").replace("{error}", e)).size(10.5).color(t.text));
            } else if result.cancelled {
                ui.label(egui::RichText::new(tr("Cancelled")).size(10.5).color(t.text_dim));
            } else {
                let mut line =
                    tr("Exported {bytes} · {pages} pages").replace("{bytes}", &bytes(result.bytes)).replace("{pages}", &result.pages.to_string());
                if !result.warnings.is_empty() {
                    line.push_str(" (");
                    line.push_str(&result.warnings.join("; "));
                    line.push(')');
                }
                ui.label(egui::RichText::new(line).size(10.5).color(t.text_dim));
            }
            if ui.small_button(crate::rtl::widget(ui, tr("Dismiss"))).clicked() {
                app.ui.export_jobs.retain(|j| j.id != job.id);
            }
        }
    });
}

/// A phase's name in the user's language, as the Exports panel and the status bar show it.
pub(crate) fn phase_label(lang: &str, phase: &str) -> String {
    let key = match phase {
        "preparing" => "Preparing",
        "flattening" => "Flattening",
        "rasterizing" => "Rasterizing",
        "pages" => "Pages",
        "finishing" => "Finishing",
        _ => return phase.to_string(),
    };
    crate::i18n::tr(lang, key).to_string()
}

/// A file size, as the Exports panel shows it.
fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}
