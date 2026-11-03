//! Place PDF (Image Import Options for a multi-page PDF): which pages, the box each shows and a
//! crop into it, and where the pages go: into the place cursor, or onto document pages from a
//! starting page. A preview shows the page being chosen with what the crop leaves of it.

use designcraft_geom::{Unit, units::parse_measure};
use serde_json::{Value, json};

use super::{Dialog, check, text_field};
use crate::DesignApp;
use crate::i18n::tr;

/// The fields a new Place PDF dialog starts with (the file's own fields come with it).
pub(super) fn defaults() -> Value {
    json!({
        "pageChoice": "previewed",
        "range": "",
        "crop": "crop",
        "insetTop": "0",
        "insetLeft": "0",
        "insetBottom": "0",
        "insetRight": "0",
        "placeOn": "cursor",
        "startPage": "1",
        "fit": "none",
        "addPages": true,
    })
}

/// Longest side of the preview, in points.
const PREVIEW: f32 = 180.0;

/// The crop inset fields, in the order `file.place {pdfCropInset}` takes them.
const INSET: [(&str, &str); 4] = [("insetTop", "Top:"), ("insetLeft", "Left:"), ("insetBottom", "Bottom:"), ("insetRight", "Right:")];

pub(super) fn show(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = &app.ui.language;
    let count = d.n("pages").filter(|n| n.is_finite() && *n >= 1.0).unwrap_or(1.0) as u64;
    ui.label(egui::RichText::new(if d.b("async") { d.s("name") } else { d.s("path") }).size(11.0));
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| preview(app, ui, d, count));
        ui.add_space(8.0);
        ui.vertical(|ui| options(app, ui, d, count));
    });
    if let Some(status) = d.fields.get("status").and_then(Value::as_str) {
        ui.label(egui::RichText::new(tr(lang, status)).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

/// The page shown in the preview and placed as the Previewed Page (1-based), when the field
/// holds one of the file's pages.
fn previewed(d: &Dialog, count: u64) -> Option<u64> {
    d.s("page").trim().parse::<u64>().ok().filter(|n| (1..=count).contains(n))
}

/// The page with the part the crop leaves outlined and the rest dimmed, and the arrows that go
/// through the pages.
fn preview(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, count: u64) {
    let lang = &app.ui.language;
    let t = crate::theme::Tokens::get(ui.ctx());
    let (area, _) = ui.allocate_exact_size(egui::vec2(PREVIEW, PREVIEW), egui::Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 2.0, t.measure_bg);
    let page = previewed(d, count);
    let shown = page.and_then(|page| page_preview(app, ui.ctx(), page, &d.s("crop")));
    match shown {
        Some((tex, aspect, frac)) => {
            let (w, h) = if aspect >= 1.0 { (PREVIEW - 8.0, (PREVIEW - 8.0) / aspect) } else { ((PREVIEW - 8.0) * aspect, PREVIEW - 8.0) };
            let r = egui::Rect::from_center_size(area.center(), egui::vec2(w, h));
            // A page draws on paper: what it leaves unpainted is white, as in a PDF reader.
            painter.rect_filled(r, 0.0, egui::Color32::WHITE);
            painter.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            if let Some(frac) = frac.map(|(frac, size)| inset_frac(frac, size, inset(d).unwrap_or([0.0; 4]))) {
                let at = |x: f64, y: f64| egui::pos2(r.min.x + x as f32 * r.width(), r.min.y + y as f32 * r.height());
                let keep = egui::Rect::from_min_max(at(frac[0], frac[1]), at(frac[2], frac[3]));
                let dim = egui::Color32::from_black_alpha(110);
                for cut in [
                    egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, keep.min.y)),
                    egui::Rect::from_min_max(egui::pos2(r.min.x, keep.max.y), r.max),
                    egui::Rect::from_min_max(egui::pos2(r.min.x, keep.min.y), egui::pos2(keep.min.x, keep.max.y)),
                    egui::Rect::from_min_max(egui::pos2(keep.max.x, keep.min.y), egui::pos2(r.max.x, keep.max.y)),
                ] {
                    if cut.is_positive() {
                        painter.rect_filled(cut.intersect(r), 0.0, dim);
                    }
                }
                painter.rect_stroke(keep, 0.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Middle);
            }
        }
        None => {
            let message = if page.is_some() { "No preview" } else { "Enter a page to preview" };
            painter.text(area.center(), egui::Align2::CENTER_CENTER, tr(lang, message), egui::FontId::proportional(11.0), t.text_dim);
        }
    }
    ui.horizontal(|ui| {
        let current = page.unwrap_or(1);
        if ui.add_enabled(current > 1, egui::Button::new("◀")).on_hover_text(tr(lang, "Previous Page")).clicked() {
            d.fields.insert("page".into(), json!((current - 1).to_string()));
        }
        text_field(ui, d, "page", 44.0);
        if ui.add_enabled(current < count, egui::Button::new("▶")).on_hover_text(tr(lang, "Next Page")).clicked() {
            d.fields.insert("page".into(), json!((current + 1).to_string()));
        }
    });
    crate::rtl::label(ui, tr(lang, "Total pages: {count}").replace("{count}", &count.to_string()));
}

/// A preview of a page: its texture, its width over its height and where the crop box sits on it
/// (fractions of the page, and its size in points).
type Preview = (egui::TextureHandle, f32, Option<([f64; 4], (f64, f64))>);

/// The preview of PDF page `page` (1-based) of the file the dialog places, cropped to `crop`.
/// Only the last one shown is kept (turning the pages doesn't pile up textures); finding a
/// Bounding Box crop interprets the page's drawing, so it isn't redone every frame.
fn page_preview(app: &DesignApp, ctx: &egui::Context, page: u64, crop: &str) -> Option<Preview> {
    let bytes = app.pdf_import_bytes()?;
    let index = usize::try_from(page.checked_sub(1)?).ok()?;
    let crop = designcraft_doc::PdfCrop::from_name(crop).unwrap_or_default();
    let slot = egui::Id::unique("pdf_import_preview");
    let key = egui::Id::unique((bytes.as_ptr() as usize, bytes.len(), index, crop));
    if let Some((shown, found)) = ctx.data(|m| m.get_temp::<(egui::Id, Option<Preview>)>(slot))
        && shown == key
    {
        return found;
    }
    let made = (|| {
        // Twice the preview's size in pixels, for high-density screens.
        let pm = designcraft_render::render_pdf_page(bytes, index, (2.0 * PREVIEW) as u32)?;
        let (pw, ph) = (usize::from(pm.width()), usize::from(pm.height()));
        if pw == 0 || ph == 0 {
            return None;
        }
        let image = egui::ColorImage::from_rgba_premultiplied([pw, ph], pm.data_as_u8_slice());
        let tex = ctx.load_texture("pdf_import_preview", image, egui::TextureOptions::LINEAR);
        Some((tex, pw as f32 / ph as f32, designcraft_render::pdf_crop_box(bytes, index, crop)))
    })();
    ctx.data_mut(|m| m.insert_temp(slot, (key, made.clone())));
    made
}

/// The part of the page an `inset` (top, left, bottom, right, in points) leaves of the crop box
/// `frac` (fractions of the page) of `size` points.
fn inset_frac(frac: [f64; 4], size: (f64, f64), inset: [f64; 4]) -> [f64; 4] {
    let [l, t, r, b] = frac;
    let (fx, fy) = ((r - l) / size.0.max(1e-9), (b - t) / size.1.max(1e-9));
    let [it, il, ib, ir] = inset;
    let (x0, x1) = (l + il * fx, r - ir * fx);
    let (y0, y1) = (t + it * fy, b - ib * fy);
    // An inset past the middle leaves nothing: draw the box as a line, not inside out.
    let (x0, x1) = if x0 <= x1 { (x0, x1) } else { ((x0 + x1) / 2.0, (x0 + x1) / 2.0) };
    let (y0, y1) = if y0 <= y1 { (y0, y1) } else { ((y0 + y1) / 2.0, (y0 + y1) / 2.0) };
    [x0, y0, x1, y1]
}

fn options(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, count: u64) {
    let lang = &app.ui.language;
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Pages")).font(crate::theme::semibold(12.0)));
    let choice = d.s("pageChoice");
    for (value, label) in [("previewed", "Previewed Page"), ("all", "All")] {
        if ui.radio(choice == value, crate::rtl::widget(ui, tr(lang, label))).clicked() {
            d.fields.insert("pageChoice".into(), json!(value));
        }
    }
    ui.horizontal(|ui| {
        if ui.radio(choice == "range", crate::rtl::widget(ui, tr(lang, "Range:"))).clicked() {
            d.fields.insert("pageChoice".into(), json!("range"));
        }
        let before = d.s("range");
        text_field(ui, d, "range", 110.0);
        // Typing a range chooses it.
        if d.s("range") != before {
            d.fields.insert("pageChoice".into(), json!("range"));
        }
    });
    if choice == "range" {
        ui.label(egui::RichText::new(tr(lang, "For example 1-3, 5, 8- (1–{count})").replace("{count}", &count.to_string())).size(10.0));
    }
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Options")).font(crate::theme::semibold(12.0)));
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr(lang, "Crop to:"));
        let cur = d.s("crop");
        let label = |v: &str| match v {
            "contentVisible" => tr(lang, "Bounding Box (Visible Layers Only)"),
            "contentAll" => tr(lang, "Bounding Box (All Layers)"),
            "trim" => tr(lang, "Trim"),
            "bleed" => tr(lang, "Bleed"),
            "art" => tr(lang, "Art"),
            "media" => tr(lang, "Media"),
            _ => tr(lang, "Crop"),
        };
        craft_ui::ComboBox::from_id_salt("pdf_crop").selected_text(crate::rtl::widget(ui, label(&cur))).show_ui(ui, |ui| {
            for v in designcraft_doc::PdfCrop::ALL.map(designcraft_doc::PdfCrop::name) {
                if ui.selectable_label(label(&cur) == label(v), label(v)).clicked() {
                    d.fields.insert("crop".into(), json!(v));
                }
            }
        });
    });
    crate::rtl::label(ui, tr(lang, "Crop Amount"));
    egui::Grid::new("pdf_inset").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
        for row in INSET.chunks(2) {
            for (key, label) in row {
                crate::rtl::label(ui, tr(lang, label));
                text_field(ui, d, key, 56.0);
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Place")).font(crate::theme::semibold(12.0)));
    let on = d.s("placeOn");
    if ui.radio(on == "cursor", crate::rtl::widget(ui, tr(lang, "In the Place Cursor (a page per click)"))).clicked() {
        d.fields.insert("placeOn".into(), json!("cursor"));
    }
    ui.horizontal(|ui| {
        if ui.radio(on == "pages", crate::rtl::widget(ui, tr(lang, "On Document Pages, Starting at:"))).clicked() {
            d.fields.insert("placeOn".into(), json!("pages"));
        }
        text_field(ui, d, "startPage", 44.0);
    });
    ui.add_enabled_ui(on == "pages", |ui| {
        ui.horizontal(|ui| {
            crate::rtl::label(ui, tr(lang, "Fit:"));
            let cur = d.s("fit");
            let label = |v: &str| match v {
                "page" => tr(lang, "Fit to Page"),
                "margins" => tr(lang, "Fit to Margins"),
                "bleed" => tr(lang, "Fit to Bleed"),
                _ => tr(lang, "Actual Size, Centered"),
            };
            craft_ui::ComboBox::from_id_salt("pdf_fit").selected_text(crate::rtl::widget(ui, label(&cur))).show_ui(ui, |ui| {
                for v in ["none", "page", "margins", "bleed"] {
                    if ui.selectable_label(cur == v, label(v)).clicked() {
                        d.fields.insert("fit".into(), json!(v));
                    }
                }
            });
        });
        check(ui, d, "addPages", tr(lang, "Add Pages as Needed"));
    });
}

/// The crop amount fields in points, top, left, bottom, right (a bare number is points; a blank
/// field is none), or the message for one that isn't a measurement of at least 0.
fn inset(d: &Dialog) -> Result<[f64; 4], &'static str> {
    let mut out = [0.0; 4];
    for (to, (key, _)) in out.iter_mut().zip(INSET) {
        let text = d.s(key);
        *to = match text.trim() {
            "" => 0.0,
            t => parse_measure(t, Unit::Points)
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .ok_or("Enter each crop amount as a measurement of 0 or more")?,
        };
    }
    Ok(out)
}

/// The `file.place` parameters the dialog's choices stand for, the file aside, or the message for
/// the field to correct. The Previewed Page into the place cursor is one page placed as before
/// (`pdfPage`); anything else is `pdfPages`.
pub(super) fn params(d: &Dialog) -> Result<Value, String> {
    let crop = match d.s("crop") {
        c if c.is_empty() => "crop".to_string(),
        c => c,
    };
    let mut p = json!({"pdfCrop": crop});
    let inset = inset(d)?;
    if inset != [0.0; 4] {
        p["pdfCropInset"] = json!(inset);
    }
    let on_pages = d.s("placeOn") == "pages";
    let pages = match d.s("pageChoice").as_str() {
        "all" => json!("all"),
        "range" => {
            let range = d.s("range");
            let range = range.trim();
            // Checked here, so the dialog stays open to correct it.
            let count = d.n("pages").filter(|n| n.is_finite() && *n >= 1.0).unwrap_or(1.0) as usize;
            match designcraft_engine::cmd::page_range(range, count) {
                Ok(pages) if !range.is_empty() && !pages.is_empty() => json!(range),
                Ok(_) => return Err("Enter the pages to place, such as 1-3, 5".into()),
                Err(e) => return Err(format!("{e}: this PDF has {count} page(s)")),
            }
        }
        _ => {
            // One page number, 1 or more; a blank field is page 1. A range or list ("4-5", "2,5"),
            // a fraction or a negative is refused rather than placing some other page.
            let raw = if d.fields.get("page").is_none_or(Value::is_null) { String::new() } else { d.s("page") };
            let page = match raw.trim() {
                "" => 1,
                t => t.parse::<u64>().ok().filter(|n| *n >= 1).ok_or("Enter one page number, or choose All or Range for several")?,
            };
            if !on_pages {
                p["pdfPage"] = json!(page);
                return Ok(p);
            }
            json!([page])
        }
    };
    p["pdfPages"] = pages;
    p["pdfPlace"] = json!(if on_pages { "pages" } else { "cursor" });
    if on_pages {
        let start = d.s("startPage").trim().parse::<u64>().ok().filter(|n| *n >= 1).ok_or("Enter the document page the PDF starts on")?;
        let fit = match d.s("fit") {
            f if f.is_empty() => "none".to_string(),
            f => f,
        };
        p["startPage"] = json!(start);
        p["fit"] = json!(fit);
        p["addPages"] = json!(d.b("addPages"));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(fields: Value) -> Dialog {
        let mut d = Dialog::new("pdfImport", json!({"name": "three.pdf", "async": true, "page": "1", "pages": 3}));
        if let Value::Object(f) = fields {
            d.fields.extend(f);
        }
        d
    }

    #[test]
    fn the_previewed_page_into_the_cursor_is_one_page_as_before() {
        assert_eq!(params(&dialog(json!({}))).unwrap(), json!({"pdfCrop": "crop", "pdfPage": 1}));
        assert_eq!(params(&dialog(json!({"page": " 2 ", "crop": "trim"}))).unwrap(), json!({"pdfCrop": "trim", "pdfPage": 2}));
        assert_eq!(params(&dialog(json!({"page": ""}))).unwrap()["pdfPage"], 1, "blank is page 1");
        for page in ["4-5", "2,5", "2.5", "-3", "0", "x"] {
            assert!(params(&dialog(json!({"page": page}))).is_err(), "{page}");
        }
    }

    #[test]
    fn all_or_a_range_go_to_the_cursor_or_onto_pages() {
        let all = params(&dialog(json!({"pageChoice": "all"}))).unwrap();
        assert_eq!(all, json!({"pdfCrop": "crop", "pdfPages": "all", "pdfPlace": "cursor"}));
        let range =
            params(&dialog(json!({"pageChoice": "range", "range": " 3, 1-2 ", "placeOn": "pages", "startPage": "4", "fit": "bleed"}))).unwrap();
        assert_eq!(range, json!({"pdfCrop": "crop", "pdfPages": "3, 1-2", "pdfPlace": "pages", "startPage": 4, "fit": "bleed", "addPages": true}));
        // The previewed page onto a page of its own.
        let one = params(&dialog(json!({"page": "3", "placeOn": "pages", "addPages": false}))).unwrap();
        assert_eq!(one["pdfPages"], json!([3]));
        assert_eq!(one["addPages"], false);
        assert!(one.get("pdfPage").is_none());
        // The range is checked against the file by `file.place`; an empty one is refused here.
        assert!(params(&dialog(json!({"pageChoice": "range", "range": "  "}))).is_err());
        for start in ["", "0", "-1", "1.5", "two"] {
            assert!(params(&dialog(json!({"pageChoice": "all", "placeOn": "pages", "startPage": start}))).is_err(), "{start}");
        }
    }

    #[test]
    fn crop_amounts_are_points_or_measurements() {
        let p = params(&dialog(json!({"insetTop": "12", "insetLeft": "1p", "insetBottom": "", "insetRight": "25.4 mm"}))).unwrap();
        let inset: Vec<f64> = p["pdfCropInset"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        for (got, want) in inset.iter().zip([12.0, 12.0, 0.0, 72.0]) {
            assert!((got - want).abs() < 1e-9, "{inset:?}");
        }
        // No inset: the parameter isn't sent.
        assert!(params(&dialog(json!({}))).unwrap().get("pdfCropInset").is_none());
        for bad in ["-1", "abc", "NaN", "1e999"] {
            assert!(params(&dialog(json!({"insetLeft": bad}))).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_preview_outline_follows_the_box_and_the_inset() {
        // A trim box 10 % in on a 200 × 100 pt page, 160 × 80 pt: 16 pt off its left and 8 pt off
        // its top are a tenth of it, 8 % of the page.
        let frac = inset_frac([0.1, 0.1, 0.9, 0.9], (160.0, 80.0), [8.0, 16.0, 0.0, 0.0]);
        let want = [0.18, 0.18, 0.9, 0.9];
        assert!(frac.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9), "{frac:?}");
        // An inset past the middle draws a line.
        let frac = inset_frac([0.0, 0.0, 1.0, 1.0], (100.0, 100.0), [0.0, 80.0, 0.0, 80.0]);
        assert!((frac[0] - frac[2]).abs() < 1e-9 && frac[1] == 0.0 && frac[3] == 1.0, "{frac:?}");
    }
}
