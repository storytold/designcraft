//! The New Calendar and Insert Calendar dialog: a gallery of the styles drawn with the dates
//! chosen, the options (look overrides start from the chosen style's own values), and a live
//! preview of exactly what OK makes. `file.newCalendar` and `layout.insertCalendar` do the work.

use designcraft_engine::cmd::calendar::{STYLES, look_defaults, placeholder_frame, preview};
use serde_json::{Map, Value, json};

use super::{Dialog, check, text_field, text_field_named, translated_combo};
use crate::DesignApp;

pub(crate) const ID: &str = "newCalendar";
const GALLERY_PX: u32 = 132;
const PREVIEW_PX: u32 = 440;

/// The dialog's starting fields; `mode` is `new` (File › New Calendar) or `insert`.
pub(crate) fn defaults(mode: &str) -> Value {
    json!({"mode": mode, "year": designcraft_engine::cmd::calendar::this_year().to_string(), "startMonth": "1",
        "months": if mode == "insert" { "1" } else { "12" }, "layout": "month", "style": "classic", "weekStart": "sunday",
        "size": "letter", "orientation": "auto", "photo": false, "notes": false, "weekNumbers": false, "adjacentDays": true,
        "group": true, "look": {}})
}

pub(crate) fn title(d: &Dialog) -> &'static str {
    if d.s("mode") == "insert" { "Insert Calendar" } else { "New Calendar" }
}

/// Where Insert Calendar puts the calendar: the selected empty frame (which it replaces), or the
/// margins of the page in view. (placeholder, 0-based page, size in points)
fn target(app: &DesignApp) -> Option<(bool, usize, (f64, f64))> {
    let st = app.session.active()?;
    // The command's own test, so a frame it won't fill is never shown as filled.
    if let Some((_, _, b)) = placeholder_frame(&st.doc, &st.selection.items) {
        return Some((true, 0, (b.width(), b.height())));
    }
    let page = crate::canvas::current_page(app).unwrap_or(0);
    let r = st.doc.page(page)?.margin_rect();
    Some((false, page, (r.width(), r.height())))
}

fn number_or_text(d: &Dialog, k: &str) -> Value {
    let s = d.s(k);
    s.trim().parse::<i64>().map_or(Value::String(s), |n| json!(n))
}

/// The params the calendar commands take from the dialog, look overrides included (`look: false`
/// leaves them out, for the style gallery).
fn params(d: &Dialog, look: bool) -> Value {
    let mut p = json!({"style": d.s("style"), "layout": d.s("layout"), "weekStart": d.s("weekStart"),
        "year": number_or_text(d, "year"), "startMonth": number_or_text(d, "startMonth"), "months": number_or_text(d, "months"),
        "photo": d.b("photo"), "notes": d.b("notes"), "weekNumbers": d.b("weekNumbers"), "adjacentDays": d.b("adjacentDays")});
    if d.s("mode") != "insert" {
        p["size"] = json!(d.s("size"));
        if d.s("orientation") != "auto" {
            p["orientation"] = json!(d.s("orientation"));
        }
    }
    if look && let (Some(Value::Object(o)), Value::Object(out)) = (d.fields.get("look"), &mut p) {
        for (k, v) in o {
            out.insert(k.clone(), v.clone());
        }
    }
    p
}

fn preview_params(app: &DesignApp, d: &Dialog, look: bool) -> Value {
    let mut p = params(d, look);
    if d.s("mode") == "insert"
        && let Some((_, _, (w, h))) = target(app)
    {
        // The preview draws blocks up to 5000 pt a side: a bigger frame is previewed scaled down,
        // keeping its shape (the calendar itself still fills the frame).
        let fit = (5000.0 / w.max(h).max(1.0)).min(1.0);
        p["previewSize"] = json!([(w * fit).max(36.0), (h * fit).max(36.0)]);
    }
    p
}

/// The month count each layout starts with: Insert Calendar places one month by default, but a
/// year at a glance is twelve.
fn default_months(mode: &str, layout: &str) -> &'static str {
    if mode == "insert" && layout == "month" { "1" } else { "12" }
}

/// After the layout changed from `was`: a month count still at the old layout's default
/// follows to the new one's, so Year at a Glance doesn't show a single month. A count the
/// user typed stays.
fn layout_changed(d: &mut Dialog, was: &str) {
    let (mode, now) = (d.s("mode"), d.s("layout"));
    if now != was && d.s("months").trim() == default_months(&mode, was) {
        d.fields.insert("months".into(), json!(default_months(&mode, &now)));
    }
}

/// The rendered preview of `p`, kept per `slot` until the params change.
fn texture(ui: &egui::Ui, app: &DesignApp, slot: &str, p: &Value, px: u32) -> Result<egui::TextureHandle, String> {
    type Cached = (String, Result<egui::TextureHandle, String>);
    let key = p.to_string();
    let id = egui::Id::unique(("calendar_preview", slot, px));
    if let Some((k, t)) = ui.ctx().data(|d| d.get_temp::<Cached>(id))
        && k == key
    {
        return t;
    }
    let t = preview(&app.session.cache, p, px).map_err(|e| e.to_string()).and_then(|r| {
        let size = [usize::try_from(r.width).unwrap_or(0), usize::try_from(r.height).unwrap_or(0)];
        // ColorImage panics on a buffer of another size.
        if size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(4)) != Some(r.pixels.len()) {
            return Err("the preview could not be drawn".to_string());
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &r.pixels);
        Ok(ui.ctx().load_texture(format!("calendar_preview:{slot}"), image, egui::TextureOptions::LINEAR))
    });
    ui.ctx().data_mut(|d| d.insert_temp::<Cached>(id, (key, t.clone())));
    t
}

/// A look value: the override, else the style's own.
fn look_value(d: &Dialog, defaults: &Value, k: &str) -> Value {
    d.fields.get("look").and_then(|l| l.get(k)).cloned().unwrap_or_else(|| defaults.get(k).cloned().unwrap_or(Value::Null))
}

fn set_look(d: &mut Dialog, k: &str, v: Value) {
    let look = d.fields.entry("look").or_insert_with(|| Value::Object(Map::new()));
    if !look.is_object() {
        *look = Value::Object(Map::new());
    }
    if let Value::Object(m) = look {
        m.insert(k.into(), v);
    }
}

fn hex_rgb(s: &str) -> [u8; 3] {
    let h = s.trim().trim_start_matches('#');
    let p = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok()).unwrap_or(0);
    if h.len() == 6 { [p(0), p(2), p(4)] } else { [0, 0, 0] }
}

pub(crate) fn body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(420.0);
            settings(app, ui, d, &lang);
        });
        ui.separator();
        ui.vertical(|ui| {
            crate::rtl::label(ui, egui::RichText::new(tr("Style")).strong());
            ui.horizontal_wrapped(|ui| {
                let current = d.s("style");
                for style in STYLES {
                    let mut p = preview_params(app, d, false);
                    p["style"] = json!(style);
                    ui.vertical(|ui| {
                        ui.set_width(GALLERY_PX as f32 * 0.72);
                        let selected = current == style;
                        let resp = match texture(ui, app, &format!("gallery_{style}"), &p, GALLERY_PX) {
                            Ok(t) => {
                                let size = t.size_vec2() * 0.72;
                                let resp = ui.add(egui::Image::new((t.id(), size)).sense(egui::Sense::click()));
                                let stroke = if selected {
                                    egui::Stroke::new(2.5, ui.visuals().selection.stroke.color)
                                } else if resp.hovered() {
                                    ui.visuals().widgets.hovered.bg_stroke
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke
                                };
                                ui.painter().rect_stroke(resp.rect.expand(1.0), 2.0, stroke, egui::StrokeKind::Outside);
                                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                            }
                            Err(_) => ui.button("?"),
                        };
                        let label = match style {
                            "modern" => tr("Modern"),
                            "minimal" => tr("Minimal"),
                            "bold" => tr("Bold"),
                            "pastel" => tr("Pastel"),
                            _ => tr("Classic"),
                        };
                        let text = if selected { egui::RichText::new(label).strong() } else { egui::RichText::new(label) };
                        if resp.clicked() || ui.selectable_label(selected, text).clicked() {
                            d.fields.insert("style".into(), json!(style));
                            // A style starts from its own look; the options below then adjust it.
                            d.fields.insert("look".into(), json!({}));
                        }
                    });
                }
            });
            ui.add_space(8.0);
            crate::rtl::label(ui, egui::RichText::new(tr("Preview")).strong());
            let p = preview_params(app, d, true);
            match texture(ui, app, "main", &p, PREVIEW_PX) {
                Ok(t) => {
                    egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).show(ui, |ui| {
                        ui.image((t.id(), t.size_vec2()));
                    });
                }
                Err(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
            }
            // Why OK was refused (set by `dialogs::confirm`), until the next try.
            if let Some(Value::String(error)) = d.fields.get("status") {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if d.s("mode") == "insert" {
                let text = match target(app) {
                    Some((true, _, (w, h))) => format!("{} ({w:.0} × {h:.0} pt)", tr("Fills the selected frame")),
                    Some((false, page, _)) => format!("{} {}", tr("Inside the margins of page"), page + 1),
                    None => tr("No document open").to_string(),
                };
                ui.weak(text);
            }
        });
    });
}

fn settings(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    let insert = d.s("mode") == "insert";
    egui::CollapsingHeader::new(tr("Dates and Layout")).default_open(true).show(ui, |ui| {
        let months: Vec<(String, &str)> = (1..=12)
            .map(|m| m.to_string())
            .zip(["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"])
            .collect();
        let months: Vec<(&str, &str)> = months.iter().map(|(v, l)| (v.as_str(), *l)).collect();
        egui::Grid::new("calendar_dates").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, tr("Start:"));
            ui.horizontal(|ui| {
                translated_combo(lang, ui, d, "startMonth", &months);
                text_field_named(ui, d, "year", 60.0, tr("Year"));
            });
            ui.end_row();
            crate::rtl::label(ui, tr("Number of Months:"));
            text_field(ui, d, "months", 60.0);
            ui.end_row();
            crate::rtl::label(ui, tr("Layout:"));
            let was = d.s("layout");
            translated_combo(lang, ui, d, "layout", &[("month", "One Month per Page"), ("year", "Year at a Glance")]);
            layout_changed(d, &was);
            ui.end_row();
            crate::rtl::label(ui, tr("Week Starts On:"));
            translated_combo(lang, ui, d, "weekStart", &[("sunday", "Sunday"), ("monday", "Monday")]);
            ui.end_row();
            if insert {
                crate::rtl::label(ui, tr("Placement:"));
                check(ui, d, "group", tr("Group Each Calendar"));
                ui.end_row();
            } else {
                crate::rtl::label(ui, tr("Page Size:"));
                translated_combo(lang, ui, d, "size", &[("letter", "Letter"), ("tabloid", "Tabloid"), ("a5", "A5"), ("a4", "A4"), ("a3", "A3")]);
                ui.end_row();
                crate::rtl::label(ui, tr("Orientation:"));
                translated_combo(lang, ui, d, "orientation", &[("auto", "Automatic"), ("portrait", "Portrait"), ("landscape", "Landscape")]);
                ui.end_row();
            }
        });
        if insert {
            ui.weak(tr("More months than one continue on the following pages, adding pages as needed."));
        }
        // Year at a Glance has no room for a picture per month.
        let year = d.s("layout") == "year";
        ui.add_enabled_ui(!year, |ui| check(ui, d, "photo", tr("Picture Frame Above Each Month")))
            .response
            .on_disabled_hover_text(tr("Only for One Month per Page"));
        check(ui, d, "notes", tr("Notes Area"));
        check(ui, d, "weekNumbers", tr("Week Numbers"));
        check(ui, d, "adjacentDays", tr("Show Days of Adjacent Months"));
    });
    let defaults = look_defaults(&d.s("style"));
    egui::CollapsingHeader::new(tr("Colors")).default_open(true).show(ui, |ui| {
        egui::Grid::new("calendar_colors").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            for (i, (k, label)) in
                [("accent", "Accent:"), ("ink", "Text:"), ("gridColor", "Grid:"), ("background", "Day Cells:")].into_iter().enumerate()
            {
                crate::rtl::label(ui, tr(label));
                let mut c = hex_rgb(look_value(d, &defaults, k).as_str().unwrap_or("#000000"));
                if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
                    set_look(d, k, json!(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])));
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    });
    egui::CollapsingHeader::new(tr("Type")).default_open(false).show(ui, |ui| {
        egui::Grid::new("calendar_fonts").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
            for (k, label) in [("title", "Month Name:"), ("number", "Day Numbers:"), ("body", "Other Text:")] {
                let fk = format!("{k}Font");
                let sk = format!("{k}FontStyle");
                crate::rtl::label(ui, tr(label));
                let family = look_value(d, &defaults, &fk).as_str().unwrap_or("").to_string();
                if let Some(pick) = crate::panels::font_combo(app, ui, ("calendar_font", k), &family, 150.0) {
                    let style = crate::panels::pick_style(app, &pick);
                    set_look(d, &fk, json!(pick.family));
                    set_look(d, &sk, json!(style));
                }
                let style = look_value(d, &defaults, &sk).as_str().unwrap_or("").to_string();
                let styles = crate::panels::fonts(app).styles(&family);
                egui::ComboBox::from_id_salt(("calendar_font_style", k)).selected_text(&style).width(110.0).show_ui(ui, |ui| {
                    for s in &styles {
                        if ui.selectable_label(*s == style, s).clicked() {
                            set_look(d, &sk, json!(s));
                        }
                    }
                });
                ui.end_row();
            }
        });
        slider(ui, d, &defaults, "fontScale", tr("Type Size:"), 0.5..=2.5, "×");
        look_check(ui, d, &defaults, "uppercase", tr("All Caps Headings"));
    });
    egui::CollapsingHeader::new(tr("Layout Details")).default_open(false).show(ui, |ui| {
        egui::Grid::new("calendar_layout").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            look_combo(
                ui,
                d,
                &defaults,
                lang,
                "numberPosition",
                tr("Day Number:"),
                &[
                    ("topLeft", "Top Left"),
                    ("topCenter", "Top Center"),
                    ("topRight", "Top Right"),
                    ("center", "Center"),
                    ("bottomLeft", "Bottom Left"),
                    ("bottomRight", "Bottom Right"),
                ],
            );
            look_combo(
                ui,
                d,
                &defaults,
                lang,
                "weekdayFormat",
                tr("Day Names:"),
                &[("auto", "Automatic"), ("full", "Full (Monday)"), ("short", "Short (Mon)"), ("initial", "Initial (M)")],
            );
            look_combo(ui, d, &defaults, lang, "titleAlign", tr("Month Name:"), &[("left", "Left"), ("center", "Center"), ("right", "Right")]);
        });
        slider(ui, d, &defaults, "gridWeight", tr("Grid Weight:"), 0.0..=6.0, " pt");
        slider(ui, d, &defaults, "cellGap", tr("Space Between Days:"), 0.0..=12.0, " pt");
        slider(ui, d, &defaults, "cellPadding", tr("Inset in Days:"), 0.0..=16.0, " pt");
        slider(ui, d, &defaults, "weekendTint", tr("Weekend Shading:"), 0.0..=0.6, "");
        for (k, label) in [
            ("showTitle", "Month Name"),
            ("showYear", "Year"),
            ("headerBand", "Color Band Behind the Month"),
            ("weekdayStrip", "Color Strip Behind Day Names"),
            ("weekendAccent", "Weekend Numbers in Accent"),
        ] {
            look_check(ui, d, &defaults, k, tr(label));
        }
    });
    if d.fields.get("look").is_some_and(|l| l.as_object().is_some_and(|m| !m.is_empty())) && ui.button(tr("Reset to Style")).clicked() {
        d.fields.insert("look".into(), json!({}));
    }
}

fn slider(ui: &mut egui::Ui, d: &mut Dialog, defaults: &Value, k: &str, label: &str, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        crate::rtl::label(ui, label);
        let mut v = look_value(d, defaults, k).as_f64().unwrap_or(*range.start());
        if ui.add(egui::Slider::new(&mut v, range).suffix(suffix).max_decimals(2)).changed() {
            set_look(d, k, json!(v));
        }
    });
}

fn look_check(ui: &mut egui::Ui, d: &mut Dialog, defaults: &Value, k: &str, label: &str) {
    let mut b = look_value(d, defaults, k).as_bool().unwrap_or(false);
    if ui.checkbox(&mut b, crate::rtl::widget(ui, label)).changed() {
        set_look(d, k, json!(b));
        if k == "weekendTint" || (k == "weekendShading" && !b) {
            set_look(d, "weekendTint", json!(0.0));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn look_combo(ui: &mut egui::Ui, d: &mut Dialog, defaults: &Value, lang: &str, k: &str, label: &str, opts: &[(&str, &str)]) {
    crate::rtl::label(ui, label);
    let cur = look_value(d, defaults, k).as_str().unwrap_or("").to_string();
    let shown = opts.iter().find(|o| o.0 == cur).map_or(cur.as_str(), |o| crate::i18n::tr(lang, o.1)).to_string();
    egui::ComboBox::from_id_salt(("calendar_look", k)).selected_text(crate::rtl::widget(ui, &shown)).width(170.0).show_ui(ui, |ui| {
        for (v, l) in opts {
            if ui.selectable_label(cur == *v, crate::rtl::widget(ui, crate::i18n::tr(lang, l))).clicked() {
                set_look(d, k, json!(v));
            }
        }
    });
    ui.end_row();
}

pub(crate) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let mut p = params(d, true);
    if d.s("mode") != "insert" {
        return app.run("file.newCalendar", p);
    }
    p["group"] = json!(d.b("group"));
    match target(app) {
        // The command finds the selected frame itself.
        Some((true, _, _)) => {}
        Some((false, page, _)) => p["page"] = json!(page + 1),
        None => return Err("no document open".into()),
    }
    app.run("layout.insertCalendar", p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// File › New Calendar…: the dialog's choices reach file.newCalendar, a custom colour
    /// becomes the calendar's swatch, and picking a style starts its look afresh.
    #[test]
    fn new_calendar_dialog_makes_the_calendar_it_shows() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.newCalendar", json!({})).unwrap();
        let d = app.ui.dialog.as_mut().unwrap();
        assert_eq!((d.id.as_str(), d.s("mode").as_str()), (ID, "new"));
        for (k, v) in [("year", json!("2027")), ("startMonth", json!("6")), ("months", json!("3")), ("style", json!("bold"))] {
            d.fields.insert(k.into(), v);
        }
        set_look(d, "accent", json!("#0080ff"));
        set_look(d, "showYear", json!(false));
        let p = params(d, true);
        assert_eq!((p["year"].clone(), p["showYear"].clone(), p["style"].clone()), (json!(2027), json!(false), json!("bold")));
        assert!(params(d, false).get("accent").is_none(), "the gallery shows the styles' own looks");
        super::super::confirm(&mut app).unwrap();
        let doc = &app.session.doc().unwrap().doc;
        assert_eq!(doc.page_count(), 3);
        assert!(doc.stories.values().any(|s| s.text == "June") && doc.stories.values().any(|s| s.text == "August"));
        assert!(!doc.stories.values().any(|s| s.text == "2027"));
        let accent = doc.swatch("Calendar Accent").unwrap();
        assert!(matches!(&accent.value, designcraft_color::SwatchValue::Color { color, .. } if color.to_hex() == "#0080ff"));
    }

    /// Layout › Insert Calendar…: on the page in view, or into the selected empty frame, and the
    /// preview is sized to where it goes.
    #[test]
    fn insert_calendar_dialog_places_on_the_page_or_into_the_frame() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert!(!crate::menus::menu_enabled(&app, "app.insertCalendar"), "needs a document");
        app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
        assert!(crate::menus::menu_enabled(&app, "app.insertCalendar"));
        let f = app.run("frame.create", json!({"rect": [100, 100, 400, 300], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [f]})).unwrap();
        app.run("app.insertCalendar", json!({})).unwrap();
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!(preview_params(&app, &d, true)["previewSize"], json!([300.0, 200.0]));
        let r = super::super::confirm(&mut app).unwrap();
        assert_eq!(r["pages"], json!([1]));
        let doc = &app.session.doc().unwrap().doc;
        assert!(doc.item(designcraft_doc::ItemId(f)).is_none());
        let g = doc.item(designcraft_doc::ItemId(r["ids"][0].as_u64().unwrap())).unwrap();
        assert!(g.is_group());
        // Nothing selected: the page in view, inside its margins.
        app.run("selection.set", json!({"ids": []})).unwrap();
        app.run("app.insertCalendar", json!({})).unwrap();
        let r = super::super::confirm(&mut app).unwrap();
        assert_eq!(r["styles"], "Calendar 2");
        assert!(preview(&app.session.cache, &json!({"style": "minimal", "previewSize": [300, 200]}), 200).is_ok());
    }

    /// Insert Calendar starts at one month; switching to Year at a Glance made a year grid of a
    /// single month. The count follows the layout while it is still the default, and a count
    /// the user typed stays.
    #[test]
    fn the_month_count_follows_the_layout_until_edited() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 1})).unwrap();
        app.run("app.insertCalendar", json!({})).unwrap();
        let d = app.ui.dialog.as_mut().unwrap();
        let switch = |d: &mut Dialog, to: &str| {
            let was = d.s("layout");
            d.fields.insert("layout".into(), json!(to));
            layout_changed(d, &was);
        };
        switch(d, "year");
        assert_eq!(d.s("months"), "12");
        switch(d, "month");
        assert_eq!(d.s("months"), "1");
        d.fields.insert("months".into(), json!("3"));
        switch(d, "year");
        assert_eq!(d.s("months"), "3", "a typed count stays");
        // The year layout really draws twelve months.
        d.fields.insert("months".into(), json!("1"));
        switch(d, "month");
        switch(d, "year");
        super::super::confirm(&mut app).unwrap();
        let doc = &app.session.doc().unwrap().doc;
        let minis = doc.stories.values().filter(|st| st.paras.first().is_some_and(|p| p.style == "Calendar Mini Month")).count();
        assert_eq!(minis, 12);
        // A new calendar starts at twelve either way.
        let mut d = Dialog::new(ID, json!({}));
        switch(&mut d, "year");
        assert_eq!(d.s("months"), "12");
    }

    /// A frame past the preview's 5000-point limit is previewed scaled down in its own shape,
    /// not as the Letter page the preview fell back to.
    #[test]
    fn a_huge_frame_is_previewed_in_its_shape() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 5000, "height": 5000})).unwrap();
        let f = app.run("frame.create", json!({"rect": [-2000, 0, 6000, 2000], "content": "unassigned"})).unwrap()["id"].clone();
        app.run("selection.set", json!({"ids": [f]})).unwrap();
        app.run("app.insertCalendar", json!({})).unwrap();
        let d = app.ui.dialog.clone().unwrap();
        let p = preview_params(&app, &d, true);
        assert_eq!(p["previewSize"], json!([5000.0, 1250.0]));
        let r = preview(&app.session.cache, &p, 400).unwrap();
        assert!(r.width == 400 && r.height < 120, "{}×{}", r.width, r.height);
    }

    /// An empty frame the command won't replace (here, inside a group) isn't shown as filled:
    /// the calendar goes on the page in view, as the dialog says.
    #[test]
    fn a_grouped_empty_frame_is_not_offered_as_the_place() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 1})).unwrap();
        let a = app.run("frame.create", json!({"rect": [100, 100, 300, 300], "content": "unassigned"})).unwrap()["id"].clone();
        let b = app.run("frame.create", json!({"rect": [320, 100, 500, 300], "content": "unassigned"})).unwrap()["id"].clone();
        app.run("object.group", json!({"ids": [a, b]})).unwrap();
        app.run("selection.set", json!({"ids": [a]})).unwrap();
        assert!(matches!(target(&app), Some((false, 0, _))), "{:?}", target(&app));
        app.run("app.insertCalendar", json!({})).unwrap();
        let r = super::super::confirm(&mut app).unwrap();
        assert_eq!(r["pages"], json!([1]));
        assert!(app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(a.as_u64().unwrap())).is_some(), "the grouped frame stays");
    }

    /// OK with settings the command refuses keeps the dialog open, with every setting and the
    /// reason, instead of closing it and losing the choices.
    #[test]
    fn a_refused_calendar_keeps_the_dialog_open() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.newCalendar", json!({})).unwrap();
        let d = app.ui.dialog.as_mut().unwrap();
        d.fields.insert("months".into(), json!("500"));
        d.fields.insert("style".into(), json!("pastel"));
        set_look(d, "accent", json!("#123456"));
        assert!(super::super::confirm(&mut app).is_err());
        let d = app.ui.dialog.as_ref().expect("the dialog stays open");
        assert_eq!((d.s("style").as_str(), d.s("months").as_str()), ("pastel", "500"));
        assert_eq!(look_value(d, &json!({}), "accent"), json!("#123456"));
        assert!(d.s("status").contains("months"), "{}", d.s("status"));
        assert!(app.session.active().is_none(), "nothing was made");
    }
}
