//! Window › Output › Separations Preview: the plates the print PDF of the spread nearest the
//! view center puts on film, each ink's percentage under the pointer, one plate or all inks,
//! and the ink limit.

use craft_prepress::{Alternate, Ink, Process};
use serde_json::json;

use crate::DesignApp;
use crate::i18n::tr;
use crate::theme::Tokens;

/// A swatch for an ink's row.
fn ink_color(ink: &Ink) -> egui::Color32 {
    match ink {
        Ink::Process(Process::Cyan) => egui::Color32::from_rgb(0, 174, 239),
        Ink::Process(Process::Magenta) => egui::Color32::from_rgb(236, 0, 140),
        Ink::Process(Process::Yellow) => egui::Color32::from_rgb(255, 242, 0),
        Ink::Process(Process::Black) => egui::Color32::from_rgb(35, 31, 32),
        Ink::Spot(s) => {
            let rgb = match s.alternate {
                Alternate::Lab([l, a, b]) => designcraft_color::cms::lab::lab_to_srgb(designcraft_color::cms::Lab { l, a, b }),
                Alternate::Cmyk([c, m, y, k]) => designcraft_color::Color::Cmyk { c, m, y, k }.to_rgb(),
            };
            let [r, g, b] = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            egui::Color32::from_rgb(r, g, b)
        }
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let sep = app.ui.separations.clone();
    let mut run = None;
    ui.horizontal(|ui| {
        let limit = sep.as_ref().and_then(|s| s.ink_limit);
        let modes = [
            ("Off", sep.is_none(), json!({"plate": null, "inkLimit": null})),
            ("Separations", sep.is_some() && limit.is_none(), json!({"plate": "all", "inkLimit": null})),
            ("Ink Limit", limit.is_some(), json!({"inkLimit": limit.unwrap_or(300.0)})),
        ];
        for (label, on, params) in modes {
            if ui.selectable_label(on, crate::rtl::widget(ui, tr(&lang, label))).clicked() && !on {
                run = Some(params);
            }
        }
    });
    if let Some(mut limit) = sep.as_ref().and_then(|s| s.ink_limit) {
        ui.horizontal(|ui| {
            ui.label(crate::rtl::widget(ui, tr(&lang, "Ink Limit")));
            if ui.add(egui::DragValue::new(&mut limit).range(0.0..=400.0).suffix("%")).changed() {
                run = Some(json!({"inkLimit": limit}));
            }
        });
    }
    if let Some(p) = run.take() {
        if let Err(e) = app.run("view.separations", p) {
            app.status_error("Separations Preview", e);
        }
        return;
    }
    let Some(sep) = sep else {
        ui.label(egui::RichText::new(tr(&lang, "Shows the plates the exported PDF prints.")).size(11.0).color(t.text_dim));
        return;
    };
    ui.separator();
    let Some(designcraft_doc::SpreadRef::Doc(si)) = crate::canvas::current_spread(app) else { return };
    let sheet = match app.canvas.separations.sheet(si) {
        None => {
            ui.label(egui::RichText::new(tr(&lang, "Separating…")).size(11.0).color(t.text_dim));
            return;
        }
        Some(Err(e)) => {
            ui.label(egui::RichText::new(e.as_str()).size(11.0).color(t.text_dim));
            return;
        }
        Some(Ok(sheet)) => sheet.clone(),
    };
    // Each ink's percentage under the pointer.
    let k = f64::from(sheet.plates.dpi) / 72.0;
    let coverage = app.canvas.separations.pointer.filter(|(psi, _)| *psi == si).and_then(|(_, p)| {
        let (x, y) = ((p.x - sheet.area.x0) * k, (p.y - sheet.area.y0) * k);
        (x >= 0.0 && y >= 0.0).then(|| sheet.plates.coverage_at(x as u32, y as u32, &[])).flatten()
    });
    let pct = |v: Option<f32>| v.map_or_else(String::new, |v| format!("{v:.0}%"));
    let row = |ui: &mut egui::Ui, selected: bool, color: Option<egui::Color32>, name: &str, value: String| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            if let Some(c) = color {
                ui.painter().rect_filled(r, 1.0, c);
            }
            let clicked = ui.selectable_label(selected, name).clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(value).size(11.0)));
            clicked
        })
        .inner
    };
    if row(ui, sep.plate.is_none(), None, tr(&lang, "All Inks"), pct(coverage.as_ref().map(|c| c.total))) {
        run = Some(json!({"plate": "all"}));
    }
    for (id, ink) in sheet.plates.inks.iter() {
        let selected = sep.plate.as_deref().and_then(|n| sheet.ink(n)) == Some(id);
        let value = pct(coverage.as_ref().and_then(|c| c.inks.get(id.index()).copied()));
        if row(ui, selected, Some(ink_color(ink)), ink.name(), value) {
            run = Some(json!({"plate": if selected { "all" } else { ink.name() }}));
        }
    }
    for name in &sheet.plates.unseparated {
        ui.label(egui::RichText::new(format!("{name} ({})", tr(&lang, "not separated"))).size(11.0).color(t.text_dim));
    }
    if let Some(p) = run
        && let Err(e) = app.run("view.separations", p)
    {
        app.status_error("Separations Preview", e);
    }
}
