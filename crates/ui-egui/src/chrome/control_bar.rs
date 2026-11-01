use super::{Align, Color32, DesignApp, NumField, Sense, Stroke, Tokens, Unit, caption, icons, json, measure, panels, semibold, vec2};

fn vsep(ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 46.0), Sense::hover());
    ui.painter().line_segment([r.center_top() + vec2(0.0, 4.0), r.center_bottom() - vec2(0.0, 4.0)], Stroke::new(1.0, t.divider));
}

/// Two-row context-sensitive Control panel.
pub fn control_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("control_bar")
        .exact_size(59.0)
        .frame(
            egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin { left: 8, right: 8, top: 5, bottom: 3 }).stroke(Stroke::new(1.0, t.divider)),
        )
        .show(ui, |ui| {
            // Wider than the window: the bar scrolls sideways.
            crate::widgets::overflow_scrolling(ui);
            egui::ScrollArea::horizontal().id_salt("control_bar_scroll").auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let text_mode =
                        matches!(app.session.tool_id(), "type" | "verticalType") || app.session.active().is_some_and(|d| d.selection.text.is_some());
                    if app.session.tool_id() == "page" {
                        control_page(app, ui);
                    } else if text_mode {
                        control_text(app, ui);
                    } else {
                        crate::object_control::show(app, ui);
                    }
                });
            });
        });
}

/// Page-size preset menu and the portrait / landscape toggles of the Control panel.
fn page_preset_and_orientation(app: &mut DesignApp, ui: &mut egui::Ui, lang: &str, size: (f64, f64), n: usize, anchor: u8) {
    let landscape = size.0 > size.1;
    ui.vertical(|ui| {
        let cur = panels::properties::preset_name(size.0, size.1);
        craft_ui::ComboBox::from_id_salt("ctl_page_preset").selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, cur))).width(130.0).show_ui(
            ui,
            |ui| {
                for (name, w, h) in panels::properties::PAGE_PRESETS {
                    if ui.selectable_label(cur == *name, crate::rtl::widget(ui, crate::i18n::tr(lang, name))).clicked() {
                        let (w, h) = if landscape { (h, w) } else { (w, h) };
                        let _ = app.run("layout.pageSize", json!({"pages": [n], "width": w, "height": h, "anchor": anchor}));
                    }
                }
            },
        );
        ui.horizontal(|ui| {
            for (icon, tip, want) in [("orient-portrait", "Portrait", false), ("orient-landscape", "Landscape", true)] {
                if crate::widgets::icon_toggle(ui, icon, landscape == want, crate::i18n::tr(lang, tip)).clicked() && landscape != want {
                    let _ = app.run("layout.pageSize", json!({"pages": [n], "width": size.1, "height": size.0, "anchor": anchor}));
                }
            }
        });
    });
}

/// The Control panel with the Page tool, as InDesign's: reference point, X/Y, W/H, page-size
/// preset, orientation, Liquid Page Rule and Show Parent Page Overlay, for
/// the page the Page tool selected (else the page in view). W/H keep the reference point in place.
fn control_page(app: &mut DesignApp, ui: &mut egui::Ui) {
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let units = st.doc.settings.horizontal_units;
    let Some(page) = st.selection.page.or_else(|| crate::canvas::current_page(app)) else { return };
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let Some((si, pi)) = d.page_loc(page) else { return };
    let pg = d.spreads[si].pages[pi].clone();
    let n = page + 1;
    // Reference point: W/H edits keep it in place.
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 44.0), Sense::hover());
    if let Some(p) = panels::properties::ref_point_proxy(ui, egui::Rect::from_center_size(r.center(), vec2(27.0, 27.0)), app.ui.ref_point) {
        app.ui.ref_point = p;
    }
    let anchor = app.ui.ref_point.min(8);
    let (fx, fy) = ((anchor % 3) as f64 / 2.0, (anchor / 3) as f64 / 2.0);
    // The reference point's position: from the spine across, from the top down.
    let (x, y) = (pg.x - d.spreads[si].spine_x() + fx * pg.width, fy * pg.height);
    egui::Grid::new("ctl_page_xywh").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&lang, "X:"));
        // Pages don't move within their spread yet.
        ui.add_enabled_ui(false, |ui| measure(ui, "cpx", Some(x), units, 64.0));
        caption(ui, crate::i18n::tr(&lang, "W:"));
        if let Some(v) = measure(ui, "cpw", Some(pg.width), units, 64.0) {
            let _ = app.run("layout.pageSize", json!({"pages": [n], "width": v, "anchor": anchor}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&lang, "Y:"));
        ui.add_enabled_ui(false, |ui| measure(ui, "cpy", Some(y), units, 64.0));
        caption(ui, crate::i18n::tr(&lang, "H:"));
        if let Some(v) = measure(ui, "cph", Some(pg.height), units, 64.0) {
            let _ = app.run("layout.pageSize", json!({"pages": [n], "height": v, "anchor": anchor}));
        }
        ui.end_row();
    });
    vsep(ui);
    // Page-size preset and orientation.
    page_preset_and_orientation(app, ui, &lang, (pg.width, pg.height), n, anchor);
    vsep(ui);
    ui.vertical(|ui| {
        caption(ui, crate::i18n::tr(&lang, "Liquid Page Rule:"));
        let label = panels::interactive::liquid_rule_label(pg.liquid);
        craft_ui::ComboBox::from_id_salt("ctl_page_rule").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).width(150.0).show_ui(
            ui,
            |ui| {
                for (r, l) in panels::interactive::LIQUID_RULES {
                    if ui.selectable_label(pg.liquid == r, crate::rtl::widget(ui, crate::i18n::tr(&lang, l))).clicked() && pg.liquid != r {
                        let _ = app.run("liquid.pageRule", json!({"rule": r, "pages": [n]}));
                    }
                }
            },
        );
    });
    vsep(ui);
    ui.vertical(|ui| {
        let mut overlay = pg.show_parent_overlay;
        if ui.checkbox(&mut overlay, crate::rtl::widget(ui, crate::i18n::tr(&lang, "Show Parent Page Overlay"))).changed() {
            let _ = app.run("layout.pageOptions", json!({"pages": [n], "showParentOverlay": overlay}));
        }
    });
}

/// The Control panel's font sizes (and leadings), as InDesign lists them.
const SIZES: &[f64] = &[6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];

/// The Control panel's tracking values (1/1000 em).
const TRACKING: &[f64] = &[-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 200.0];

/// Whether the text Control panel shows Paragraph Formatting Controls (¶) rather than Character
/// Formatting Controls (A).
const PARA_MODE: &str = "ctl_text_para_mode";

/// An icon and a field after it, as the Control panel lays its fields out.
pub(crate) fn icon_field(ui: &mut egui::Ui, icon: &str, tip: &str, field: NumField) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    icons::paint(ui.painter(), r, icon, t.icon);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tip));
    let label = resp.id;
    resp.on_hover_text(tip);
    field.spinner().labelled_by(label).show(ui)
}

/// A column of two rows (the Control panel's two-row groups).
fn rows(ui: &mut egui::Ui, mut row: impl FnMut(&mut egui::Ui, usize)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| row(ui, 0));
        ui.horizontal(|ui| row(ui, 1));
    });
}

/// A small toggle button showing `text` (the case and position buttons).
fn text_toggle(ui: &mut egui::Ui, text: &str, strike: bool, on: bool, tip: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 20.0), Sense::click());
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
    if strike {
        ui.painter().line_segment([r.center() + vec2(-6.0, 0.5), r.center() + vec2(6.0, 0.5)], Stroke::new(1.2, col));
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, tip));
    resp.on_hover_text(tip).clicked()
}

/// The text Control panel, as InDesign's: A shows the Character Formatting Controls (font, size
/// and leading, the case and position buttons, kerning and tracking, the scales, baseline shift
/// and skew, the character style and language); ¶ the Paragraph Formatting Controls (alignment,
/// the indents, space before and after, the drop cap, the paragraph style, Hyphenate and Align to
/// Baseline Grid).
fn control_text(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let attrs = panels::text_attrs(app);
    let chars = attrs.as_ref().map(|a| a["chars"].clone()).unwrap_or_default();
    let para = attrs.as_ref().map(|a| a["para"].clone()).unwrap_or_default();
    let mode_id = egui::Id::unique(PARA_MODE);
    let para_mode = ui.data(|d| d.get_temp::<bool>(mode_id)).unwrap_or(false);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        for (para, text, tip) in [(false, "A", "Character Formatting Controls"), (true, "¶", "Paragraph Formatting Controls")] {
            let on = para_mode == para;
            let (r, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
            if on {
                ui.painter().rect_filled(r, 2.0, t.tool_active);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 2.0, t.hover);
            }
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, semibold(15.0), if on { t.text_strong } else { t.text_dim });
            let tip = tr(tip);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, &tip));
            if resp.on_hover_text(&tip).clicked() {
                ui.data_mut(|d| d.insert_temp(mode_id, para));
            }
        }
    });
    vsep(ui);
    if para_mode {
        control_paragraph(app, ui, &attrs, &para, &tr);
    } else {
        control_character(app, ui, &attrs, &chars, &para, &tr);
    }
}

/// Draw the character-mode controls and issue their corresponding text commands.
fn control_character(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    attrs: &Option<serde_json::Value>,
    chars: &serde_json::Value,
    para: &serde_json::Value,
    tr: &dyn Fn(&str) -> String,
) {
    ui.vertical(|ui| {
        let fam = chars["fontFamily"].as_str().unwrap_or("").to_string();
        let sty = chars["fontStyle"].as_str().unwrap_or("").to_string();
        panels::font_family_picker(app, ui, &fam, 170.0);
        panels::font_style_picker(app, ui, &fam, &sty, 170.0);
    });
    vsep(ui);
    // Size and leading: a spinner on the left and a list of common values on the right;
    // automatic leading in parentheses.
    rows(ui, |ui, row| {
        if row == 0 {
            if let Some(v) = icon_field(
                ui,
                "font-size",
                &tr("Font Size"),
                NumField::number("csize", chars["size"].as_f64(), " pt", 2).width(84.0).presets(SIZES).range(0.1, 1296.0),
            ) {
                let _ = app.run("type.char", json!({"attrs": {"size": v}}));
            }
        } else {
            let auto = chars["leading"]["kind"].as_str() != Some("points");
            let lead = if auto {
                chars["size"].as_f64().map(|s| s * para["autoLeading"].as_f64().unwrap_or(1.2))
            } else {
                chars["leading"]["value"].as_f64()
            };
            let lead = lead.map(|v| (v * 100.0).round() / 100.0);
            let key = if auto { "cleadauto" } else { "clead" };
            if let Some(v) = icon_field(
                ui,
                "leading",
                &tr("Leading"),
                NumField::number(key, lead, " pt", 2).width(84.0).presets(SIZES).range(0.0, 5000.0).parens(auto),
            ) {
                let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
            }
        }
    });
    vsep(ui);
    // All Caps, Superscript, Underline / Small Caps, Subscript, Strikethrough.
    let caps = chars["capitalization"].as_str().unwrap_or("normal").to_string();
    let pos = chars["position"].as_str().unwrap_or("normal").to_string();
    let (ul, st) = (chars["underline"].as_bool().unwrap_or(false), chars["strikethrough"].as_bool().unwrap_or(false));
    let mut set = None;
    rows(ui, |ui, row| {
        if row == 0 {
            if text_toggle(ui, "TT", false, caps == "allCaps", &tr("All Caps")) {
                set = Some(json!({"capitalization": if caps == "allCaps" { "normal" } else { "allCaps" }}));
            }
            if text_toggle(ui, "T¹", false, pos == "superscript", &tr("Superscript")) {
                set = Some(json!({"position": if pos == "superscript" { "normal" } else { "superscript" }}));
            }
            if text_toggle(ui, "T̲", false, ul, &tr("Underline")) {
                set = Some(json!({"underline": !ul}));
            }
        } else {
            if text_toggle(ui, "small-caps", false, caps == "smallCaps", &tr("Small Caps")) {
                set = Some(json!({"capitalization": if caps == "smallCaps" { "normal" } else { "smallCaps" }}));
            }
            if text_toggle(ui, "T₁", false, pos == "subscript", &tr("Subscript")) {
                set = Some(json!({"position": if pos == "subscript" { "normal" } else { "subscript" }}));
            }
            if text_toggle(ui, "T", true, st, &tr("Strikethrough")) {
                set = Some(json!({"strikethrough": !st}));
            }
        }
    });
    if let Some(a) = set {
        let _ = app.run("type.char", json!({"attrs": a}));
    }
    vsep(ui);
    rows(ui, |ui, row| {
        if row == 0 {
            let t = Tokens::get(ui.ctx());
            let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            icons::paint(ui.painter(), r, "kerning", t.icon);
            resp.on_hover_text(tr("Kerning"));
            crate::panels::properties::kerning_control(app, ui, "ckern", &chars["kerning"], 92.0);
        } else {
            if let Some(v) = icon_field(
                ui,
                "tracking",
                &tr("Tracking"),
                NumField::number("ctrack", chars["tracking"].as_f64(), "", 0).width(92.0).presets(TRACKING).step(5.0).range(-1000.0, 10000.0),
            ) {
                let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
            }
        }
    });
    vsep(ui);
    const SCALES: &[f64] = &[25.0, 50.0, 75.0, 80.0, 90.0, 100.0, 110.0, 120.0, 125.0, 150.0, 200.0];
    let pct = |v: &serde_json::Value| v.as_f64().map(|s| (s * 1000.0).round() / 10.0);
    rows(ui, |ui, row| {
        if row == 0 {
            if let Some(v) = icon_field(
                ui,
                "vscale",
                &tr("Vertical Scale"),
                NumField::number("cvs", pct(&chars["vScale"]), "%", 1).width(84.0).presets(SCALES).range(1.0, 1000.0),
            ) {
                let _ = app.run("type.char", json!({"attrs": {"vScale": v / 100.0}}));
            }
        } else {
            if let Some(v) = icon_field(
                ui,
                "hscale",
                &tr("Horizontal Scale"),
                NumField::number("chs", pct(&chars["hScale"]), "%", 1).width(84.0).presets(SCALES).range(1.0, 1000.0),
            ) {
                let _ = app.run("type.char", json!({"attrs": {"hScale": v / 100.0}}));
            }
        }
    });
    vsep(ui);
    rows(ui, |ui, row| {
        if row == 0 {
            if let Some(v) = icon_field(
                ui,
                "baseline-shift",
                &tr("Baseline Shift"),
                NumField::number("cbs", chars["baselineShift"].as_f64(), " pt", 2).width(76.0).range(-5000.0, 5000.0),
            ) {
                let _ = app.run("type.char", json!({"attrs": {"baselineShift": v}}));
            }
        } else {
            if let Some(v) = icon_field(
                ui,
                "skew",
                &tr("Skew (False Italic)"),
                NumField::number("csk", chars["skew"].as_f64(), "°", 1).width(76.0).range(-85.0, 85.0),
            ) {
                let _ = app.run("type.char", json!({"attrs": {"skew": v}}));
            }
        }
    });
    vsep(ui);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        // The character style, then the language.
        let cs = attrs.as_ref().and_then(|a| a["characterStyle"].as_str()).unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_string();
        let names: Vec<String> = app.session.active().map(|d| d.doc.styles.character.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        let mut pick = None;
        craft_ui::ComboBox::from_id_salt("ctl_char_style")
            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &cs)))
            .width(150.0)
            .show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(*n == cs, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                        pick = Some(n.clone());
                    }
                }
            });
        if let Some(n) = pick {
            let _ = app.run("style.character.apply", json!({"name": n}));
        }
        let lang = chars["language"].as_str().unwrap_or("English: USA").to_string();
        craft_ui::ComboBox::from_id_salt("ctl_language").selected_text(crate::rtl::widget(ui, tr(&lang))).width(150.0).show_ui(ui, |ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| crate::panels::properties::language_menu(app, ui, &lang));
        });
    });
}

fn control_paragraph(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    attrs: &Option<serde_json::Value>,
    para: &serde_json::Value,
    tr: &dyn Fn(&str) -> String,
) {
    let cur: Align = serde_json::from_value(para["align"].clone()).unwrap_or_default();
    if let Some(a) = align_buttons(ui, cur, tr) {
        let _ = app.run("type.para", json!({"attrs": {"align": a}}));
    }
    vsep(ui);
    let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    if let Some((k, v)) = indent_space_fields(ui, para, units, tr) {
        let _ = app.run("type.para", json!({"attrs": {k: v}}));
    }
    if let Some((k, v)) = drop_cap_fields(ui, para, tr) {
        let _ = app.run("type.para", json!({"attrs": {k: v.max(0.0) as u64}}));
    }
    vsep(ui);
    // Bulleted List over Numbered List, as InDesign's (a list with the default hanging indent, or,
    // pressed, none). Pressed when every selected paragraph has it, as Type › Apply Bullets shows.
    use designcraft_doc::ListType;
    let pressed = [ListType::Bullets, ListType::Numbers].map(|k| app.session.selection_has_list(k));
    let mut list = None;
    rows(ui, |ui, row| {
        let (icon, cmd, tip) = if row == 0 { ("bullets", "list.bullets", "Bulleted List") } else { ("numbering", "list.numbers", "Numbered List") };
        let on = pressed.get(row).copied().unwrap_or(false);
        if crate::widgets::icon_toggle(ui, icon, on, &tr(tip)).clicked() {
            list = Some((cmd, !on));
        }
    });
    if let Some((cmd, on)) = list {
        let _ = app.run(cmd, json!({"on": on}));
    }
    vsep(ui);
    let (shade, dialog) = shading_buttons(app, ui, para, tr);
    if let Some(attrs) = shade {
        let _ = app.run("type.para", json!({ "attrs": attrs }));
    }
    if let Some(tab) = dialog {
        let _ = app.run("app.paragraphBordersDialog", json!({ "tab": tab }));
    }
    vsep(ui);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        let ps = attrs.as_ref().and_then(|a| a["paragraphStyle"].as_str()).unwrap_or("").to_string();
        let ov = attrs.as_ref().and_then(|a| a["paraOverrides"].as_u64()).unwrap_or(0)
            + attrs.as_ref().and_then(|a| a["charOverrides"].as_u64()).unwrap_or(0);
        panels::para_style_picker(app, ui, &ps, ov > 0, 170.0);
        ui.horizontal(|ui| {
            let mut h = para["hyphenate"].as_bool().unwrap_or(true);
            if crate::widgets::checkbox(ui, &mut h, &tr("Hyphenate")).changed() {
                let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
            }
            // First Line Only is aligned too: the button shows it pressed, and a click turns alignment off.
            let grid = matches!(para["gridAlign"].as_str(), Some("allLines" | "firstLineOnly"));
            if crate::widgets::icon_toggle(ui, "grid-baseline", grid, &tr("Align to Baseline Grid")).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"gridAlign": if grid { "none" } else { "allLines" }}}));
            }
        });
    });
}

/// The paragraph alignment buttons, in two rows: the alignment picked this frame, if any.
fn align_buttons(ui: &mut egui::Ui, cur: Align, tr: &dyn Fn(&str) -> String) -> Option<Align> {
    let mut align = None;
    rows(ui, |ui, row| {
        if row == 0 {
            for (a, icon) in [
                (Align::Left, "palign-left"),
                (Align::Center, "palign-center"),
                (Align::Right, "palign-right"),
                (Align::TowardsSpine, "palign-spine-towards"),
            ] {
                if icons::button(ui, icon, 20.0, cur == a, &tr(a.label())).clicked() {
                    align = Some(a);
                }
            }
        } else {
            for (a, icon) in [
                (Align::LeftJustified, "palign-justify-left"),
                (Align::CenterJustified, "palign-justify-center"),
                (Align::RightJustified, "palign-justify-right"),
                (Align::FullyJustified, "palign-justify-all"),
                (Align::AwayFromSpine, "palign-spine-away"),
            ] {
                if icons::button(ui, icon, 20.0, cur == a, &tr(a.label())).clicked() {
                    align = Some(a);
                }
            }
        }
    });
    align
}

/// The indents and space before and after, two per column. Returns the one field changed this
/// frame, with its paragraph attribute and value.
fn indent_space_fields(ui: &mut egui::Ui, para: &serde_json::Value, units: Unit, tr: &dyn Fn(&str) -> String) -> Option<(&'static str, f64)> {
    let mut set = None;
    for ((ia, ta, ka), (ib, tb, kb)) in [
        (("indent-left", "Left Indent", "leftIndent"), ("indent-first", "First Line Left Indent", "firstLineIndent")),
        (("indent-right", "Right Indent", "rightIndent"), ("indent-last", "Last Line Right Indent", "lastLineIndent")),
        (("space-before", "Space Before", "spaceBefore"), ("space-after", "Space After", "spaceAfter")),
    ] {
        rows(ui, |ui, row| {
            if row == 0 {
                if let Some(v) = icon_field(ui, ia, &tr(ta), NumField::measure(ka, para[ka].as_f64(), units).width(84.0).range(0.0, 10_000.0)) {
                    set = Some((ka, v));
                }
            } else {
                let min = crate::panels::properties::para_field_min(kb);
                if let Some(v) = icon_field(ui, ib, &tr(tb), NumField::measure(kb, para[kb].as_f64(), units).width(84.0).range(min, 10_000.0)) {
                    set = Some((kb, v));
                }
            }
        });
        vsep(ui);
    }
    set
}

/// The drop cap's line and character counts. Returns the one changed this frame, with its
/// paragraph attribute and value.
fn drop_cap_fields(ui: &mut egui::Ui, para: &serde_json::Value, tr: &dyn Fn(&str) -> String) -> Option<(&'static str, f64)> {
    let mut drop = None;
    rows(ui, |ui, row| {
        if row == 0 {
            let max = designcraft_compose::MAX_DROP_CAP_LINES as f64;
            if let Some(v) = icon_field(
                ui,
                "drop-lines",
                &tr("Drop Cap Number of Lines"),
                NumField::number("cdl", para["dropCapLines"].as_f64(), "", 0).width(64.0).range(0.0, max),
            ) {
                drop = Some(("dropCapLines", v));
            }
        } else {
            let max = designcraft_compose::MAX_DROP_CAP_CHARS as f64;
            if let Some(v) = icon_field(
                ui,
                "drop-chars",
                &tr("Drop Cap One or More Characters"),
                NumField::number("cdc", para["dropCapChars"].as_f64(), "", 0).width(64.0).range(0.0, max),
            ) {
                drop = Some(("dropCapChars", v));
            }
        }
    });
    drop
}

/// Shading over Border: the on/off checkboxes, the colour swatches and the Paragraph Borders and
/// Shading buttons. Returns the attributes to set and the dialog tab to open, if any.
fn shading_buttons(
    app: &DesignApp,
    ui: &mut egui::Ui,
    para: &serde_json::Value,
    tr: &dyn Fn(&str) -> String,
) -> (Option<serde_json::Value>, Option<&'static str>) {
    // On or off, the Paragraph Borders and Shading dialog on its tab, and the colour, as InDesign's.
    let doc = app.session.active().map(|d| d.doc.clone());
    let mut shade = None;
    let mut dialog = None;
    // Both checkboxes as wide as the wider label, so the icons and colours line up in columns.
    let check_w = ["Shading", "Border"]
        .iter()
        .map(|l| ui.fonts_mut(|f| f.layout_no_wrap(tr(l), egui::FontId::proportional(11.5), Color32::WHITE).size().x))
        .fold(0.0, f32::max)
        + 28.0;
    rows(ui, |ui, row| {
        let (on_key, color_key, label, icon, tab) = if row == 0 {
            ("shadingOn", "shadingColor", "Shading", "para-shading", "shading")
        } else {
            ("borderOn", "borderColor", "Border", "para-border", "border")
        };
        let mut on = para[on_key].as_bool().unwrap_or(false);
        ui.allocate_ui_with_layout(vec2(check_w, crate::widgets::FIELD_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(check_w);
            if crate::widgets::checkbox(ui, &mut on, &tr(label)).changed() {
                shade = Some(json!({ on_key: on }));
            }
        });
        if crate::widgets::icon_toggle(ui, icon, false, &tr("Paragraph Borders and Shading…")).clicked() {
            dialog = Some(tab);
        }
        if let Some(doc) = &doc {
            let cur = para[color_key].as_str().unwrap_or("[Black]").to_string();
            if let Some(w) = swatch_field(ui, doc, &cur, 96.0, &tr(if row == 0 { "Paragraph Shading" } else { "Paragraph Border" })) {
                shade = Some(json!({ color_key: w }));
            }
        }
    });
    (shade, dialog)
}

/// A swatch field of the Control panel (Shading and Border colours): the swatch's chip and name,
/// and the document's swatches in its menu. Returns the swatch picked.
pub(crate) fn swatch_field(ui: &mut egui::Ui, doc: &designcraft_doc::Document, cur: &str, width: f32, label: &str) -> Option<String> {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, crate::widgets::FIELD_H), Sense::click());
    let resp = resp.on_hover_text(label);
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label);
        info.current_text_value = Some(cur.to_string());
        info
    });
    let p = ui.painter();
    p.rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.input });
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    let seg = egui::Rect::from_min_max(egui::pos2(r.max.x - crate::widgets::SEG_W, r.min.y), r.max);
    p.line_segment([seg.left_top(), seg.left_bottom()], Stroke::new(1.0, t.field_border));
    icons::paint(p, egui::Rect::from_center_size(seg.center(), vec2(12.0, 12.0)), "chevron-down", t.icon);
    let (c, g) = crate::widgets::swatch_colors(doc, cur, 1.0);
    let chip = egui::Rect::from_min_size(egui::pos2(r.min.x + 4.0, r.center().y - 6.0), vec2(12.0, 12.0));
    crate::widgets::paint_chip(p, chip, c, g);
    let clip = egui::Rect::from_min_max(r.min, egui::pos2(seg.min.x - 2.0, r.max.y));
    p.with_clip_rect(clip).text(egui::pos2(chip.max.x + 5.0, r.center().y), egui::Align2::LEFT_CENTER, cur, egui::FontId::proportional(11.5), t.text);
    let mut picked = None;
    egui::Popup::menu(&resp).show(|ui| {
        picked = crate::panels::swatch_list(ui, doc, cur);
    });
    picked
}
