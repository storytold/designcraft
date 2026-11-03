use super::object::{open_color_picker, swatch_menu};
use super::typography::{para_field_min, show_typography_panel};
use super::{
    Align, DesignApp, FIELD_H, NumField, Rect, Sense, Stroke, StrokeKind, Tokens, Ui, Value, block, full_width, icon_toggle, icons, json, link_label,
    more_options, outline_button, place, pos2, section, set_temp_bool, show_panel, sub, temp_bool, text_attrs, units, vec2, widgets,
};

// ---------------------------------------------------------------- Text

pub(super) fn text_style_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    section(ui, crate::i18n::tr(&app.ui.language, "Text Style"), true);
    let Some(a) = text_attrs(app) else { return };
    let tab = app.ui.text_style_tab;
    let fw = full_width(ui);
    if let Some(k) = widgets::segmented(
        ui,
        &[crate::i18n::tr(&app.ui.language, "Paragraph Styles"), crate::i18n::tr(&app.ui.language, "Character Styles")],
        tab as usize,
        fw,
    ) {
        app.ui.text_style_tab = k as u8;
    }
    ui.add_space(5.0);
    let (names, cur): (Vec<String>, String) = {
        let Some(st) = app.session.active() else { return };
        if tab == 0 {
            (
                st.doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| n != designcraft_doc::NO_PARA_STYLE).collect(),
                a["paragraphStyle"].as_str().unwrap_or("").to_string(),
            )
        } else {
            (st.doc.styles.character.iter().map(|p| p.name.clone()).collect(), a["characterStyle"].as_str().unwrap_or("[None]").to_string())
        }
    };
    let ov = if tab == 0 { a["paraOverrides"].as_u64().unwrap_or(0) + a["charOverrides"].as_u64().unwrap_or(0) } else { 0 };
    let shown = crate::i18n::style_name(&app.ui.language, &cur);
    let label = if ov > 0 { format!("{shown}+") } else { shown.to_string() };
    let resp = widgets::dropdown_h(ui, "", fw, 34.0);
    // The row paints its own swatch and name, so the dropdown itself has no text to name it.
    let style_kind = crate::i18n::tr(&app.ui.language, if tab == 0 { "Paragraph Style" } else { "Character Style" }).to_owned();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &style_kind));
    // "Ag" swatch + style name.
    let ag = Rect::from_min_size(resp.rect.min + vec2(6.0, 6.0), vec2(22.0, 22.0));
    ui.painter().rect_filled(ag, 1.0, egui::Color32::WHITE);
    ui.painter().text(ag.center(), egui::Align2::CENTER_CENTER, "Ag", egui::FontId::new(13.0, egui::FontFamily::Proportional), egui::Color32::BLACK);
    crate::rtl::paint(
        &ui.painter().with_clip_rect(resp.rect.shrink2(vec2(20.0, 0.0))),
        pos2(ag.max.x + 8.0, resp.rect.center().y),
        egui::Align2::LEFT_CENTER,
        &label,
        egui::FontId::proportional(11.5),
        t.text,
    );
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(fw);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for n in &names {
                if ui.selectable_label(*n == cur, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                    pick = Some(n.clone());
                    ui.close();
                }
            }
        });
    });
    if let Some(n) = pick {
        if tab == 0 {
            let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": false}));
        } else {
            let _ = app.run("style.character.apply", json!({"name": n}));
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if icon_toggle(
            ui,
            "pilcrow-menu",
            false,
            crate::i18n::tr(&app.ui.language, if tab == 0 { "Paragraph Styles panel" } else { "Character Styles panel" }),
        )
        .clicked()
        {
            show_panel(app, if tab == 0 { "paragraphStyles" } else { "characterStyles" });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add_space(4.0);
            if icon_toggle(ui, "style-new", false, crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Create new style"))).clicked()
            {
                let cmd = if tab == 0 { "style.paragraph.create" } else { "style.character.create" };
                let _ = app.run(cmd, json!({}));
            }
            if icon_toggle(ui, "style-clear", false, crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Clear overrides")))
                .clicked()
                && tab == 0
                && !cur.is_empty()
            {
                let _ = app.run("style.paragraph.apply", json!({"name": cur, "clearOverrides": true}));
            }
            if icon_toggle(ui, "style-load", false, crate::i18n::tr(&app.ui.language, "Styles panel")).clicked() {
                show_panel(app, if tab == 0 { "paragraphStyles" } else { "characterStyles" });
            }
        });
    });
}

pub(super) fn text_appearance_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(a) = text_attrs(app) else { return };
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let c = &a["chars"];
    for (k, (key, label)) in [("fill", "Fill"), ("stroke", "Stroke")].into_iter().enumerate() {
        let sw = c[key].as_str().unwrap_or(designcraft_color::swatch::NONE).to_string();
        let b = block(ui, FIELD_H);
        let (col, g) = widgets::swatch_colors(&doc, &sw, 1.0);
        let chip = sub(b, 4.0, 2.0, 17.0, 17.0);
        if k == 0 {
            widgets::paint_chip(ui.painter(), chip, col, g);
        } else {
            widgets::paint_stroke_chip(ui.painter(), chip, col, egui::Color32::from_gray(20));
        }
        ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
        // The "T" marks text formatting.
        let tc = if col.is_some_and(|c| c.r() as u32 + c.g() as u32 + c.b() as u32 > 380) { egui::Color32::BLACK } else { egui::Color32::WHITE };
        if k == 0 {
            ui.painter().text(chip.center(), egui::Align2::CENTER_CENTER, "T", crate::theme::semibold(12.0), tc);
        }
        let resp = ui.interact(chip, ui.scope_id().with(("tchip", k)), Sense::click()).on_hover_text(format!("{label}: {sw}"));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, if k == 0 { "Text fill swatch" } else { "Text stroke swatch" }));
        swatch_menu(app, &resp, &doc, &sw, k != 0, false, true);
        if resp.double_clicked() {
            open_color_picker(app, &doc, &sw, k != 0, true);
        }
        if k == 0 {
            crate::rtl::paint(
                ui.painter(),
                b.min + vec2(34.0, FIELD_H / 2.0),
                egui::Align2::LEFT_CENTER,
                crate::i18n::tr(&app.ui.language, label),
                egui::FontId::proportional(11.5),
                t.text,
            );
        } else {
            if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, label), 56.0)).clicked() {
                show_panel(app, "stroke");
            }
            const WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 10.0];
            if let Some(v) = place(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H), |ui| {
                NumField::number("tsw", c["strokeWeight"].as_f64(), " pt", 3).width(69.0).spinner().presets(WEIGHTS).range(0.0, 100.0).show(ui)
            }) {
                let _ = app.run("type.char", json!({"attrs": {"strokeWeight": v}}));
            }
        }
        ui.add_space(4.0);
    }
    let mut behind = c["strokeBehindFill"].as_bool().unwrap_or(false);
    if ui.checkbox(&mut behind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Stroke Behind Fill"))).changed() {
        let _ = app.run("type.char", json!({"attrs": {"strokeBehindFill": behind}}));
    }
}

/// Character section: font family (with search), style, size/leading, kerning/tracking, •••.
pub(super) fn character_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(a) = text_attrs(app) else { return };
    let c = a["chars"].clone();
    let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
    let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
    let fw = full_width(ui);
    // Family dropdown with a search segment on the left.
    let fonts = crate::panels::fonts(app);
    let menu = crate::panels::font_menu(app);
    let resp = widgets::dropdown(ui, "", fw);
    // A painted search icon and family name, as in the font menus: name the field itself.
    let family_label = crate::i18n::tr(&app.ui.language, "Font Family:").to_owned();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &family_label));
    let r = resp.rect;
    icons::paint(ui.painter(), Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(14.0, 14.0)), "search", t.icon);
    widgets::chevron_down(ui.painter(), r.min + vec2(22.0, 10.5), t.icon);
    ui.painter().text(
        r.min + vec2(30.0, 10.5),
        egui::Align2::LEFT_CENTER,
        if fam.is_empty() { "—".to_string() } else { crate::panels::font_label(app, &menu, &fam) },
        egui::FontId::proportional(11.5),
        t.text,
    );
    if let Some(f) = crate::panels::font_popup(app, ui, &resp, &menu, &fam) {
        crate::panels::apply_font(app, &f);
    }
    ui.add_space(1.0);
    let styles = fonts.styles(&fam);
    let cur = styles.iter().position(|s| *s == sty);
    if let Some(k) = widgets::dropdown_list(ui, if sty.is_empty() { "—" } else { &sty }, fw, &styles, cur) {
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": styles[k]}}));
    }
    ui.add_space(1.0);
    const SIZES: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 21.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];
    let half = (fw - 10.0) / 2.0;
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    let size = c["size"].as_f64();
    let auto = c["leading"]["kind"].as_str() != Some("points");
    let lv = if auto { size.map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
    icons::paint(ui.painter(), sub(b, 0.0, 1.0, 19.0, 19.0), "font-size", t.icon);
    if let Some(v) = place(ui, sub(b, 24.0, 0.0, half - 24.0, FIELD_H), |ui| {
        NumField::number("csize", size, " pt", 2).width(half - 24.0).spinner().presets(SIZES).range(0.1, 1296.0).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"size": v}}));
    }
    icons::paint(ui.painter(), sub(b, half + 10.0, 1.0, 19.0, 19.0), "leading", t.icon);
    let lw = half - 24.0;
    let lshow = lv.map(|v| (v * 100.0).round() / 100.0);
    if let Some(v) = place(ui, sub(b, half + 34.0, 0.0, lw, FIELD_H), |ui| {
        let key = if auto { "cleadauto" } else { "clead" };
        // Auto leading is shown in parentheses, like "(14.4 pt)".
        NumField::number(key, lshow, " pt", 2).width(lw).spinner().presets(SIZES).range(0.0, 5000.0).parens(auto).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
    }
    // Kerning / tracking.
    icons::paint(ui.painter(), sub(b, 0.0, 27.0, 19.0, 19.0), "kerning", t.icon);
    place(ui, sub(b, 24.0, 26.0, half - 24.0, FIELD_H), |ui| kerning_control(app, ui, "ckern", &c["kerning"], half - 24.0));
    icons::paint(ui.painter(), sub(b, half + 10.0, 27.0, 19.0, 19.0), "tracking", t.icon);
    const TRACK: &[f64] = &[-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 200.0];
    if let Some(v) = place(ui, sub(b, half + 34.0, 26.0, lw, FIELD_H), |ui| {
        NumField::number("ctrack", c["tracking"].as_f64(), "", 0).width(lw).spinner().presets(TRACK).step(5.0).range(-1000.0, 10000.0).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
    }
    // ••• shows the rest in place, as InDesign's Properties panel does.
    let more = temp_bool(ui, "props_char_more", false);
    if more {
        character_more(app, ui, &c, fw);
    }
    if more_options(ui, &app.ui.language).clicked() {
        set_temp_bool(ui, "props_char_more", !more);
    }
}

/// A percentage or other number field in one half of a two-field row, with its icon.
#[allow(clippy::too_many_arguments)]
pub(super) fn half_field(ui: &mut Ui, b: Rect, right: bool, y: f32, half: f32, icon: &str, tip: &str, field: NumField) -> Option<f64> {
    half_field_or_clear(ui, b, right, y, half, icon, tip, field).flatten()
}

/// [`half_field`] for a value that may be unset: emptying the field commits `Some(None)`.
#[allow(clippy::too_many_arguments)]
pub(super) fn half_field_or_clear(
    ui: &mut Ui,
    b: Rect,
    right: bool,
    y: f32,
    half: f32,
    icon: &str,
    tip: &str,
    field: NumField,
) -> Option<Option<f64>> {
    let t = Tokens::get(ui.ctx());
    let x = if right { half + 10.0 } else { 0.0 };
    let ir = sub(b, x, y + 1.0, 19.0, 19.0);
    icons::paint(ui.painter(), ir, icon, t.icon);
    let resp = ui.interact(ir, ui.scope_id().with(("half_icon", icon)), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tip));
    let label = resp.id;
    resp.on_hover_text(tip);
    place(ui, sub(b, x + 24.0, y, half - 24.0, FIELD_H), |ui| field.width(half - 24.0).labelled_by(label).show_or_clear(ui))
}

/// Properties › Character after •••: vertical and horizontal scale, baseline shift and skew,
/// language and OpenType, then All Caps, Small Caps, Superscript, Subscript, Underline and
/// Strikethrough, as InDesign lays them out.
fn character_more(app: &mut DesignApp, ui: &mut Ui, c: &Value, fw: f32) {
    let lang_ui = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang_ui, s).to_string();
    let half = (fw - 10.0) / 2.0;
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    const SCALES: &[f64] = &[25.0, 50.0, 75.0, 80.0, 90.0, 100.0, 110.0, 120.0, 125.0, 150.0, 200.0];
    let pct = |v: &Value| v.as_f64().map(|s| (s * 1000.0).round() / 10.0);
    let field = |id, v, suffix, decimals| NumField::number(id, v, suffix, decimals).spinner();
    if let Some(v) = half_field(
        ui,
        b,
        false,
        0.0,
        half,
        "vscale",
        &tr("Vertical Scale"),
        field("cvscale", pct(&c["vScale"]), "%", 1).presets(SCALES).range(1.0, 1000.0),
    ) {
        let _ = app.run("type.char", json!({"attrs": {"vScale": v / 100.0}}));
    }
    if let Some(v) = half_field(
        ui,
        b,
        true,
        0.0,
        half,
        "hscale",
        &tr("Horizontal Scale"),
        field("chscale", pct(&c["hScale"]), "%", 1).presets(SCALES).range(1.0, 1000.0),
    ) {
        let _ = app.run("type.char", json!({"attrs": {"hScale": v / 100.0}}));
    }
    if let Some(v) = half_field(
        ui,
        b,
        false,
        26.0,
        half,
        "baseline-shift",
        &tr("Baseline Shift"),
        field("cbshift", c["baselineShift"].as_f64(), " pt", 2).range(-5000.0, 5000.0),
    ) {
        let _ = app.run("type.char", json!({"attrs": {"baselineShift": v}}));
    }
    if let Some(v) =
        half_field(ui, b, true, 26.0, half, "skew", &tr("Skew (False Italic)"), field("cskew", c["skew"].as_f64(), "°", 1).range(-85.0, 85.0))
    {
        let _ = app.run("type.char", json!({"attrs": {"skew": v}}));
    }
    // Language (hyphenation, spelling and typographer's quotes follow it) and OpenType.
    ui.add_space(4.0);
    let lang = c["language"].as_str().unwrap_or("English: USA").to_string();
    let label = tr("Language");
    let row = block(ui, FIELD_H);
    let resp = place(ui, sub(row, 0.0, 0.0, half, FIELD_H), |ui| widgets::dropdown(ui, crate::i18n::tr(&lang_ui, &lang), half)).on_hover_ui(|ui| {
        crate::rtl::label(ui, label.clone());
    });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &label));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(half);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| language_menu(app, ui, &lang));
    });
    if place(ui, sub(row, half + 10.0, 0.0, half, FIELD_H), |ui| widgets::outline_button(ui, &tr("OpenType"), half)).clicked() {
        show_typography_panel(app);
    }
    // The case and position buttons.
    ui.add_space(8.0);
    let caps = c["capitalization"].as_str().unwrap_or("normal").to_string();
    let pos = c["position"].as_str().unwrap_or("normal").to_string();
    let (ul, st) = (c["underline"].as_bool().unwrap_or(false), c["strikethrough"].as_bool().unwrap_or(false));
    let buttons: [(&str, &str, bool, Value); 6] = [
        ("TT", "All Caps", caps == "allCaps", json!({"capitalization": if caps == "allCaps" { "normal" } else { "allCaps" }})),
        ("small-caps", "Small Caps", caps == "smallCaps", json!({"capitalization": if caps == "smallCaps" { "normal" } else { "smallCaps" }})),
        ("T¹", "Superscript", pos == "superscript", json!({"position": if pos == "superscript" { "normal" } else { "superscript" }})),
        ("T₁", "Subscript", pos == "subscript", json!({"position": if pos == "subscript" { "normal" } else { "subscript" }})),
        ("T̲", "Underline", ul, json!({"underline": !ul})),
        ("T", "Strikethrough", st, json!({"strikethrough": !st})),
    ];
    let pitch = fw / 6.0;
    let row = block(ui, 22.0);
    for (i, (text, tip, on, attrs)) in buttons.into_iter().enumerate() {
        let r = Rect::from_center_size(row.min + vec2(pitch * (i as f32 + 0.5), 11.0), vec2(26.0, 22.0));
        let resp = ui.interact(r, ui.scope_id().with(("case", i)), Sense::click());
        let t = Tokens::get(ui.ctx());
        if on {
            ui.painter().rect_filled(r, 2.0, t.tool_active);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 2.0, t.hover);
        }
        let col = if on { t.text_strong } else { t.icon };
        if text == "small-caps" {
            crate::icons::paint(ui.painter(), r, "small-caps", col);
        } else {
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(13.0), col);
        }
        // Strikethrough: the T struck through (a combining stroke doesn't draw in every font).
        if i == 5 {
            ui.painter().line_segment([r.center() + vec2(-6.0, 0.5), r.center() + vec2(6.0, 0.5)], Stroke::new(1.2, col));
        }
        let tip = tr(tip);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, &tip));
        if resp.on_hover_text(&tip).clicked() {
            let _ = app.run("type.char", json!({"attrs": attrs}));
        }
    }
    ui.add_space(4.0);
}

/// Paragraph section: the 9 alignment buttons and •••.
pub(super) fn paragraph_section(app: &mut DesignApp, ui: &mut Ui) {
    let Some(a) = text_attrs(app) else { return };
    let cur: Align = serde_json::from_value(a["para"]["align"].clone()).unwrap_or_default();
    let fw = full_width(ui);
    let pitch = fw / 9.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (al, icon) in [
            (Align::Left, "palign-left"),
            (Align::Center, "palign-center"),
            (Align::Right, "palign-right"),
            (Align::LeftJustified, "palign-justify-left"),
            (Align::CenterJustified, "palign-justify-center"),
            (Align::RightJustified, "palign-justify-right"),
            (Align::FullyJustified, "palign-justify-all"),
            (Align::TowardsSpine, "palign-spine-towards"),
            (Align::AwayFromSpine, "palign-spine-away"),
        ] {
            if widgets::icon_toggle_sized(ui, icon, cur == al, al.label(), vec2(pitch.min(23.0), 21.0)).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"align": al}}));
            }
            if pitch > 23.0 {
                ui.add_space(pitch - 23.0);
            }
        }
    });
    let more = temp_bool(ui, "props_para_more", false);
    if more {
        paragraph_more(app, ui, &a["para"], fw);
    }
    if more_options(ui, &app.ui.language).clicked() {
        set_temp_bool(ui, "props_para_more", !more);
    }
}

/// Properties › Paragraph after •••: left and right indent, first line and last line indent,
/// space before and after, drop cap lines and characters, then Hyphenate and Align to Baseline
/// Grid, as InDesign lays them out.
fn paragraph_more(app: &mut DesignApp, ui: &mut Ui, p: &Value, fw: f32) {
    let lang_ui = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang_ui, s).to_string();
    let u = units(app);
    let half = (fw - 10.0) / 2.0;
    ui.add_space(4.0);
    let b = block(ui, 4.0 * FIELD_H + 15.0);
    let rows = [
        (("indent-left", "Left Indent", "leftIndent"), ("indent-right", "Right Indent", "rightIndent")),
        (("indent-first", "First Line Left Indent", "firstLineIndent"), ("indent-last", "Last Line Right Indent", "lastLineIndent")),
        (("space-before", "Space Before", "spaceBefore"), ("space-after", "Space After", "spaceAfter")),
    ];
    for (i, (l, r)) in rows.into_iter().enumerate() {
        let y = i as f32 * (FIELD_H + 5.0);
        for (right, (icon, tip, key)) in [(false, l), (true, r)] {
            let min = para_field_min(key);
            let field = NumField::measure(key, p[key].as_f64(), u).spinner().range(min, 10_000.0);
            if let Some(v) = half_field(ui, b, right, y, half, icon, &tr(tip), field) {
                let _ = app.run("type.para", json!({"attrs": {key: v}}));
            }
        }
    }
    let y = 3.0 * (FIELD_H + 5.0);
    for (right, icon, tip, key, max) in [
        (false, "drop-lines", "Drop Cap Number of Lines", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES),
        (true, "drop-chars", "Drop Cap One or More Characters", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS),
    ] {
        let field = NumField::number(key, p[key].as_f64(), "", 0).spinner().range(0.0, max as f64);
        if let Some(v) = half_field(ui, b, right, y, half, icon, &tr(tip), field) {
            let _ = app.run("type.para", json!({"attrs": {key: (v.max(0.0) as u64).min(max as u64)}}));
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // First Line Only is aligned too: the button shows it pressed, and a click turns alignment off.
        let grid = matches!(p["gridAlign"].as_str(), Some("allLines" | "firstLineOnly"));
        if widgets::icon_toggle(ui, "grid-baseline", grid, &tr("Align to Baseline Grid")).clicked() {
            let _ = app.run("type.para", json!({"attrs": {"gridAlign": if grid { "none" } else { "allLines" }}}));
        }
        ui.add_space(12.0);
        let mut h = p["hyphenate"].as_bool().unwrap_or(true);
        if widgets::checkbox(ui, &mut h, &tr("Hyphenate")).changed() {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
        }
    });
    ui.add_space(4.0);
}

pub(super) fn bullets_section(app: &mut DesignApp, ui: &mut Ui) {
    if text_attrs(app).is_none() {
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        // As InDesign's: a list with the default hanging indent, or (pressed: every selected
        // paragraph has it, as Type › Apply Bullets shows) no list.
        use designcraft_doc::ListType;
        for (icon, kind, cmd, tip) in
            [("bullets", ListType::Bullets, "list.bullets", "Bulleted List"), ("numbering", ListType::Numbers, "list.numbers", "Numbered List")]
        {
            let on = app.session.selection_has_list(kind);
            if icon_toggle(ui, icon, on, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run(cmd, json!({"on": !on}));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            // As InDesign's: the Bullets and Numbering dialog for the selected paragraphs.
            if outline_button(ui, crate::i18n::tr(&app.ui.language, "Options"), 66.0).clicked() {
                let _ = app.run("app.bulletsNumberingDialog", json!({}));
            }
        });
    });
}

pub(super) fn quick_actions_text(app: &mut DesignApp, ui: &mut Ui) {
    let fw = full_width(ui);
    let hw = (fw - 10.0) / 2.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Change Case"), hw);
        egui::Popup::menu(&r).show(|ui| {
            for (label, v) in [("UPPERCASE", "allCaps"), ("Small Caps", "smallCaps"), ("Normal", "normal")] {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("type.char", json!({"attrs": {"capitalization": v}}));
                    ui.close();
                }
            }
        });
        if outline_button(ui, crate::i18n::tr(&app.ui.language, "Story Editor"), hw).clicked() {
            let _ = app.run("app.storyEditor", json!({}));
        }
    });
    ui.add_space(4.0);
    if outline_button(ui, crate::i18n::tr(&app.ui.language, "Fill with Placeholder Text"), fw).clicked() {
        let _ = app.run("type.fillWithPlaceholder", json!({}));
    }
}

/// Text Frame section: columns, gutter, [Options].
pub(super) fn text_frame_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let u = units(app);
    let Some(st) = app.session.active() else { return };
    let fid = st.selection.items.first().copied().or_else(|| st.selection.text.and_then(|t| t.frame));
    let Some(opts) = fid.and_then(|f| st.doc.item(f)).and_then(|i| i.text_frame()).map(|t| t.options.clone()) else { return };
    let b = block(ui, FIELD_H);
    icons::paint(ui.painter(), sub(b, 0.0, 1.0, 19.0, 19.0), "text-columns", t.icon);
    if let Some(v) = place(ui, sub(b, 22.0, 0.0, 54.0, FIELD_H), |ui| {
        NumField::number("tfcols", Some(opts.columns as f64), "", 0).width(54.0).spinner().range(1.0, 40.0).show(ui)
    }) {
        let _ = app.run("object.textFrameOptions", json!({"columns": v.max(1.0) as u64}));
    }
    icons::paint(ui.painter(), sub(b, 83.0, 1.0, 19.0, 19.0), "text-gutter", t.icon);
    if let Some(v) = place(ui, sub(b, 105.0, 0.0, 54.0, FIELD_H), |ui| {
        NumField::measure("tfgut", Some(opts.gutter), u).width(54.0).spinner().range(0.0, 1440.0).show(ui)
    }) {
        let _ = app.run("object.textFrameOptions", json!({"gutter": v}));
    }
    let ow = (b.max.x - 8.0) - (b.min.x + 164.0);
    if place(ui, sub(b, 164.0, 0.5, ow, 20.0), |ui| outline_button(ui, crate::i18n::tr(&app.ui.language, "Options"), ow)).clicked() {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
    }
}

/// Languages offered for text (InDesign-style names).
pub(crate) const LANGUAGES: &[&str] = &[
    "[No Language]",
    "English: USA",
    "English: UK",
    "English: Canadian",
    "German: 2006 Reform",
    "German: Swiss 2006 Reform",
    "French",
    "French: Canadian",
    "Spanish",
    "Italian",
    "Portuguese",
    "Portuguese: Brazilian",
    "Dutch: 2005 Reform",
    "Danish",
    "Swedish",
    "Norwegian: Bokmål",
    "Finnish",
    "Polish",
    "Czech",
    "Hungarian",
    "Greek",
    "Russian",
    "Ukrainian",
    "Georgian",
    "Turkish",
    "Japanese",
    "Chinese",
    "Chinese: Simplified",
    "Chinese: Traditional",
    "Korean",
    "Arabic",
    "Hebrew",
];

/// The Language list (Character panel, Properties ▸ Character): picking one sets the language of
/// the selected text.
pub(crate) fn language_menu(app: &mut DesignApp, ui: &mut Ui, cur: &str) {
    for l in LANGUAGES {
        if ui.selectable_label(*l == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
            let _ = app.run("type.char", json!({"attrs": {"language": l}}));
            ui.close();
        }
    }
}

/// Kerning for the text selection, or at a caret for the pair at the caret: the label shows
/// Metrics, Optical, 0 or the manual value; its menu sets one of the first three or a manual value
/// in 1/1000 em (typed or stepped).
pub(crate) fn kerning_control(app: &mut DesignApp, ui: &mut egui::Ui, id: &str, k: &Value, width: f32) {
    let kind = k["kind"].as_str().unwrap_or("metrics");
    let manual = k["value"].as_f64().filter(|v| kind == "manual" && v.is_finite());
    let label = match (kind, manual) {
        ("optical", _) => "Optical".to_string(),
        ("none", _) => "0".to_string(),
        ("manual", Some(v)) => format!("{v}"),
        _ => "Metrics".to_string(),
    };
    let resp = widgets::dropdown(ui, crate::i18n::tr(&app.ui.language, &label), width);
    let name = crate::i18n::tr(&app.ui.language, "Kerning");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, name));
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        for (text, v) in [("Metrics", json!({"kind": "metrics"})), ("Optical", json!({"kind": "optical"})), ("0", json!({"kind": "none"}))] {
            if ui.selectable_label(label == text, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, text))).clicked() {
                let _ = app.run("type.kernPair", json!({"kerning": v}));
                ui.close();
            }
        }
        ui.separator();
        // A manual value in 1/1000 em: after each selected character, or between the two at a caret.
        let field = format!("{id}_manual");
        if let Some(v) = NumField::number(&field, Some(manual.unwrap_or(0.0)), "", 0)
            .name("Manual Kerning")
            .width(90.0)
            .spinner()
            .step(5.0)
            .range(-1000.0, 10000.0)
            .show(ui)
        {
            let _ = app.run("type.kernPair", json!({"kerning": {"kind": "manual", "value": v}}));
        }
    });
}
