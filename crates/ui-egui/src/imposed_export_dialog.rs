//! File › Export Imposed PDF…: the press sheet, the layout of the copies, the pages and copies,
//! bleed and crop marks, with a preview of the first sheet and "N per sheet, M sheets".

use designcraft_engine::cmd::{IMPOSE_MAX_COPIES, IMPOSE_SHEET_SIZES};
use designcraft_geom::{Unit, format_measure};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::{Bound, with_path};
use crate::dialogs::Dialog;

pub const ID: &str = "exportImposed";
const CUSTOM: &str = "Custom";
const LENGTHS: [&str; 8] = ["width", "height", "marginTop", "marginBottom", "marginLeft", "marginRight", "gapH", "gapV"];

/// Open the dialog: an A4 sheet, the document's pages once each.
pub fn open(app: &mut DesignApp) {
    let unit = app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let f = |v: f64| json!(format_measure(v, unit));
    let (w, h) = IMPOSE_SHEET_SIZES.iter().find(|(n, ..)| *n == "A4").map(|(_, w, h)| (*w, *h)).unwrap_or((595.276, 841.89));
    let mut d = Dialog::new(
        ID,
        json!({
            "sheet": "A4", "width": f(w), "height": f(h), "orientation": "portrait",
            "marginTop": f(0.0), "marginBottom": f(0.0), "marginLeft": f(0.0), "marginRight": f(0.0),
            "gapH": f(0.0), "gapV": f(0.0), "rotation": "auto", "center": true,
            "pages": "all", "copies": 1, "bleed": true, "cropMarks": "border",
        }),
    );
    d.unit = unit;
    app.ui.dialog = Some(d);
}

/// The engine parameters of the dialog's settings (lengths in points).
pub fn params(d: &Dialog) -> Result<Value, String> {
    let len = |k: &str| d.m(k).filter(|v| v.is_finite()).ok_or_else(|| format!("{k}: {}", d.s(k)));
    let [w, h, mt, mb, ml, mr, gh, gv] = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| len(LENGTHS[i]));
    let sheet = if d.s("sheet") == CUSTOM { json!([w?, h?]) } else { json!(d.s("sheet")) };
    let mut p = json!({
        "sheet": sheet,
        "orientation": d.s("orientation"),
        "margins": [mt?, mb?, ml?, mr?],
        "gap": [gh?, gv?],
        "rotation": d.s("rotation"),
        "center": d.b("center"),
        "copies": d.n("copies").map_or(1, |n| n.clamp(1.0, IMPOSE_MAX_COPIES as f64) as u64),
        "bleed": d.b("bleed"),
        "cropMarks": d.s("cropMarks"),
    });
    let pages = d.s("pages");
    if !pages.trim().is_empty() && pages.trim() != "all" {
        p["pages"] = json!(pages.trim());
    }
    Ok(p)
}

fn combo(ui: &mut egui::Ui, d: &mut Dialog, key: &str, lang: &str, opts: &[(&str, &str)]) {
    let cur = d.s(key);
    let shown = opts.iter().find(|(v, _)| *v == cur).map_or(cur.clone(), |(_, l)| crate::i18n::tr(lang, l).to_string());
    craft_ui::ComboBox::from_id_salt(("imposed", key)).selected_text(shown).width(130.0).show_ui(ui, |ui| {
        for (v, l) in opts {
            if ui.selectable_label(cur == *v, crate::i18n::tr(lang, l)).clicked() {
                d.fields.insert(key.into(), json!(*v));
            }
        }
    });
}

fn length(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str) {
    crate::rtl::label(ui, label);
    let mut s = d.s(key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(70.0)).accessible_name(label.to_owned()).changed() {
        d.fields.insert(key.into(), json!(s));
    }
}

fn check(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str) {
    let mut b = d.b(key);
    if ui.checkbox(&mut b, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(key.into(), json!(b));
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let t = crate::theme::Tokens::get(ui.ctx());
    let head = |ui: &mut egui::Ui, s: &str| {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(tr(s)).strong());
    };
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            head(ui, "Sheet");
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Size:"));
                let cur = d.s("sheet");
                let shown = if cur == CUSTOM { tr(CUSTOM) } else { cur.clone() };
                craft_ui::ComboBox::from_id_salt("imposed_sheet").selected_text(shown).width(130.0).show_ui(ui, |ui| {
                    for (name, w, h) in IMPOSE_SHEET_SIZES {
                        if ui.selectable_label(cur == *name, *name).clicked() {
                            let (w, h) = if d.s("orientation") == "landscape" { (w.max(*h), w.min(*h)) } else { (*w, *h) };
                            d.fields.insert("sheet".into(), json!(*name));
                            d.fields.insert("width".into(), json!(format_measure(w, d.unit)));
                            d.fields.insert("height".into(), json!(format_measure(h, d.unit)));
                        }
                    }
                    if ui.selectable_label(cur == CUSTOM, tr(CUSTOM)).clicked() {
                        d.fields.insert("sheet".into(), json!(CUSTOM));
                    }
                });
            });
            ui.horizontal(|ui| {
                let before = (d.s("width"), d.s("height"));
                length(ui, d, "width", &tr("Width:"));
                length(ui, d, "height", &tr("Height:"));
                if (d.s("width"), d.s("height")) != before {
                    d.fields.insert("sheet".into(), json!(CUSTOM));
                }
            });
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Orientation:"));
                let before = d.s("orientation");
                combo(ui, d, "orientation", &lang, &[("portrait", "Portrait"), ("landscape", "Landscape")]);
                if d.s("orientation") != before {
                    let (w, h) = (d.s("width"), d.s("height"));
                    let (a, b) = (d.m("width").unwrap_or(0.0), d.m("height").unwrap_or(0.0));
                    if (d.s("orientation") == "landscape") == (a < b) {
                        d.fields.insert("width".into(), json!(h));
                        d.fields.insert("height".into(), json!(w));
                    }
                }
            });
            head(ui, "Layout");
            ui.horizontal(|ui| {
                length(ui, d, "marginTop", &tr("Top:"));
                length(ui, d, "marginBottom", &tr("Bottom:"));
            });
            ui.horizontal(|ui| {
                length(ui, d, "marginLeft", &tr("Left:"));
                length(ui, d, "marginRight", &tr("Right:"));
            });
            ui.horizontal(|ui| {
                length(ui, d, "gapH", &tr("Horizontal Gap:"));
                length(ui, d, "gapV", &tr("Vertical Gap:"));
            });
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Rotation:"));
                combo(ui, d, "rotation", &lang, &[("auto", "Auto"), ("off", "Off"), ("on", "On")]);
            });
            check(ui, d, "center", &tr("Centre on Sheet"));
            head(ui, "Pages");
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Page Range:"));
                let mut s = d.s("pages");
                if ui.add(egui::TextEdit::singleline(&mut s).desired_width(110.0)).changed() {
                    d.fields.insert("pages".into(), json!(s));
                }
            });
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Copies:"));
                let mut n = d.n("copies").unwrap_or(1.0).clamp(1.0, IMPOSE_MAX_COPIES as f64) as u32;
                if ui.add(egui::DragValue::new(&mut n).range(1..=10_000).speed(1.0)).changed() {
                    d.fields.insert("copies".into(), json!(n));
                }
            });
            head(ui, "Marks and Bleeds");
            check(ui, d, "bleed", &tr("Include document bleed"));
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Crop Marks:"));
                combo(ui, d, "cropMarks", &lang, &[("none", "None"), ("corners", "At Each Copy"), ("border", "Along the Sheet Border")]);
            });
        });
        ui.add_space(12.0);
        ui.vertical(|ui| preview(app, ui, d, &t, &lang));
    });
    if let Some(status) = d.fields.get("status").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        crate::rtl::label(ui, egui::RichText::new(status).color(ui.visuals().error_fg_color));
    }
}

/// The first sheet as the engine lays it out, and the counts.
fn preview(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog, t: &crate::theme::Tokens, lang: &str) {
    let p = params(d);
    let key = p.as_ref().map(Value::to_string).unwrap_or_default();
    if d.s("previewFor") != key || !d.fields.contains_key("preview") {
        let r = p.and_then(|p| app.session.execute("file.imposedLayout", &p).map_err(|e| e.to_string()));
        let (layout, error) = match r {
            Ok(v) => (v, Value::Null),
            Err(e) => (Value::Null, json!(e)),
        };
        d.fields.insert("preview".into(), layout);
        d.fields.insert("previewError".into(), error);
        d.fields.insert("previewFor".into(), json!(key));
    }
    let size = egui::vec2(240.0, 300.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let l = d.fields.get("preview").cloned().unwrap_or(Value::Null);
    let num = |v: &Value| v.as_f64().unwrap_or(0.0) as f32;
    let sheet = l["sheet"].as_array().map(|a| egui::vec2(a.first().map_or(0.0, num), a.get(1).map_or(0.0, num)));
    if let Some(sw) = sheet.filter(|s| s.x > 0.0 && s.y > 0.0) {
        let k = (size.x / sw.x).min(size.y / sw.y) * 0.96;
        let o = rect.center() - sw * k / 2.0;
        let at = |x: f32, y: f32| o + egui::vec2(x, y) * k;
        let r4 = |v: &Value| {
            let a: Vec<f32> = v.as_array().map(|a| a.iter().map(num).collect()).unwrap_or_default();
            (a.len() == 4).then(|| egui::Rect::from_min_max(at(a[0], a[1]), at(a[2], a[3])))
        };
        painter.rect_filled(egui::Rect::from_min_size(o, sw * k), 0.0, egui::Color32::WHITE);
        painter.rect_stroke(egui::Rect::from_min_size(o, sw * k), 0.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
        for c in l["placements"].as_array().into_iter().flatten() {
            let Some(trim) = r4(&c["trim"]) else { continue };
            painter.rect_filled(trim, 0.0, t.accent.gamma_multiply(0.18));
            painter.rect_stroke(trim, 0.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
            painter.text(trim.center(), egui::Align2::CENTER_CENTER, c["page"].to_string(), egui::FontId::proportional(10.0), t.accent_strong);
        }
        for m in l["marks"].as_array().into_iter().flatten() {
            let a: Vec<f32> = m.as_array().map(|a| a.iter().map(num).collect()).unwrap_or_default();
            if a.len() == 4 {
                painter.line_segment([at(a[0], a[1]), at(a[2], a[3])], egui::Stroke::new(1.0, egui::Color32::BLACK));
            }
        }
    }
    match d.fields.get("previewError").and_then(Value::as_str) {
        Some(e) => {
            crate::rtl::label(ui, egui::RichText::new(e).color(ui.visuals().error_fg_color));
        }
        None => {
            let text = crate::i18n::tr(lang, "{n} per sheet, {m} sheets")
                .replace("{n}", &l["perSheet"].as_u64().unwrap_or(0).to_string())
                .replace("{m}", &l["sheets"].as_u64().unwrap_or(0).to_string());
            crate::rtl::label(ui, egui::RichText::new(text).strong());
        }
    }
}

/// Export: refuse settings the engine can't lay out (the dialog stays open, saying why), else ask
/// where to save and write the file.
pub fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let checked = params(d).and_then(|p| app.session.execute("file.imposedLayout", &p).map(|_| p).map_err(|e| e.to_string()));
    let p = match checked {
        Ok(p) => p,
        Err(e) => {
            let mut d = d.clone();
            d.fields.insert("status".into(), json!(e));
            app.ui.dialog = Some(d);
            return Err(e);
        }
    };
    let title = app.session.active().map(|st| st.doc.title.clone()).unwrap_or_default();
    let name = if title.is_empty() { "Imposed.pdf".to_string() } else { format!("{title} imposed.pdf") };
    app.ask_save(&name, Bound::Document, move |app, path| crate::menus::export_imposed(app, &with_path(&p, "path", path)))
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;

    use super::*;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mm = designcraft_geom::units::PT_PER_MM;
        app.run("file.new", json!({"pages": 1, "facingPages": false, "width": 105.0 * mm, "height": 148.0 * mm, "title": "Card"})).unwrap();
        app
    }

    #[test]
    fn the_dialog_previews_copies_per_sheet_and_sheets() {
        let mut app = app();
        app.run("app.exportImposedPdf", json!({})).unwrap();
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(ID));
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("copies".into(), json!(8));
        }
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        h.run_steps(4);
        assert!(h.query_by_label("4 per sheet, 2 sheets").is_some());
        let d = h.state().app.ui.dialog.clone().unwrap();
        assert_eq!(d.fields["preview"]["placements"].as_array().map(Vec::len), Some(4));
    }

    #[test]
    fn export_writes_the_file_and_bad_settings_keep_the_dialog_open() {
        let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
        let mut app = app();
        let w = written.clone();
        app.services.write = Some(Box::new(move |p: &str, b: &[u8]| {
            w.lock().unwrap().push((p.to_string(), b.to_vec()));
            Ok(())
        }));
        open(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("sheet".into(), json!(CUSTOM));
            d.fields.insert("width".into(), json!("50pt"));
        }
        assert!(crate::dialogs::confirm(&mut app).is_err());
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(d.id == ID && d.s("status").contains("doesn't fit"), "{}", d.s("status"));
        assert!(written.lock().unwrap().is_empty());
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some("/tmp/card.pdf".into())));
        open(&mut app);
        crate::dialogs::confirm(&mut app).unwrap();
        app.settle_dialogs();
        let files = written.lock().unwrap().clone();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "/tmp/card.pdf");
        assert!(files[0].1.starts_with(b"%PDF"));
    }
}
