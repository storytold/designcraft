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
fn page_preset_and_orientation(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    lang: &str,
    size: (f64, f64),
    anchor: u8,
    resize: impl Fn(&mut DesignApp, serde_json::Value),
) {
    let landscape = size.0 > size.1;
    ui.vertical(|ui| {
        let cur = panels::properties::preset_name(size.0, size.1);
        craft_ui_legacy::ComboBox::from_id_salt("ctl_page_preset")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, cur)))
            .width(130.0)
            .show_ui(ui, |ui| {
                for (name, w, h) in panels::properties::PAGE_PRESETS {
                    if ui.selectable_label(cur == *name, crate::rtl::widget(ui, crate::i18n::tr(lang, name))).clicked() {
                        let (w, h) = if landscape { (h, w) } else { (w, h) };
                        resize(app, json!({"width": w, "height": h, "anchor": anchor}));
                    }
                }
            })
            .response
            .accessible_name(crate::i18n::tr(lang, "Page Size"));
        ui.horizontal(|ui| {
            for (icon, tip, want) in [("orient-portrait", "Portrait", false), ("orient-landscape", "Landscape", true)] {
                if crate::widgets::icon_toggle(ui, icon, landscape == want, crate::i18n::tr(lang, tip)).clicked() && landscape != want {
                    resize(app, json!({"width": size.1, "height": size.0, "anchor": anchor}));
                }
            }
        });
    });
}

/// The Control panel with the Page tool, as InDesign's: reference point, X/Y, W/H, page-size
/// preset, orientation, Liquid Page Rule and Show Parent Page Overlay, for
/// the page the Page tool selected (else the page in view). W/H keep the reference point in place.
pub(super) fn control_page(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::page(app, ui);
        return;
    }
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let units = st.doc.settings.horizontal_units;
    let Some((spread, page)) = crate::canvas::page_target(app) else { return };
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let Some(sp) = d.spread(spread) else { return };
    let Some(pg) = sp.pages.get(page).cloned() else { return };
    let n = match spread {
        designcraft_doc::SpreadRef::Doc(si) => Some(d.first_page_of_spread(si) + page + 1),
        _ => None,
    };
    let resize = |app: &mut DesignApp, params| {
        if let Some((cmd, params)) = designcraft_tools::layout::page_size_command(&d, spread, page, params) {
            let _ = app.run(cmd, params);
        }
    };
    // Reference point: W/H edits keep it in place.
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 44.0), Sense::hover());
    if let Some(p) = panels::properties::ref_point_proxy(ui, egui::Rect::from_center_size(r.center(), vec2(27.0, 27.0)), app.ui.ref_point) {
        app.ui.ref_point = p;
    }
    let anchor = app.ui.ref_point.min(8);
    let (fx, fy) = ((anchor % 3) as f64 / 2.0, (anchor / 3) as f64 / 2.0);
    // The reference point's position: from the spine across, from the top down.
    let (x, y) = (pg.x - sp.spine_x() + fx * pg.width, fy * pg.height);
    egui::Grid::new("ctl_page_xywh").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&lang, "X:"));
        // Pages don't move within their spread yet.
        ui.add_enabled_ui(false, |ui| measure(ui, "cpx", Some(x), units, 64.0));
        caption(ui, crate::i18n::tr(&lang, "W:"));
        if let Some(v) = measure(ui, "cpw", Some(pg.width), units, 64.0) {
            resize(app, json!({"width": v, "anchor": anchor}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&lang, "Y:"));
        ui.add_enabled_ui(false, |ui| measure(ui, "cpy", Some(y), units, 64.0));
        caption(ui, crate::i18n::tr(&lang, "H:"));
        if let Some(v) = measure(ui, "cph", Some(pg.height), units, 64.0) {
            resize(app, json!({"height": v, "anchor": anchor}));
        }
        ui.end_row();
    });
    vsep(ui);
    // Page-size preset and orientation.
    page_preset_and_orientation(app, ui, &lang, (pg.width, pg.height), anchor, resize);
    vsep(ui);
    ui.add_enabled_ui(n.is_some(), |ui| {
        caption(ui, crate::i18n::tr(&lang, "Liquid Page Rule:"));
        let label = panels::interactive::liquid_rule_label(pg.liquid);
        craft_ui_legacy::ComboBox::from_id_salt("ctl_page_rule")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&lang, label)))
            .width(150.0)
            .show_ui(ui, |ui| {
                for (r, l) in panels::interactive::LIQUID_RULES {
                    if ui.selectable_label(pg.liquid == r, crate::rtl::widget(ui, crate::i18n::tr(&lang, l))).clicked()
                        && pg.liquid != r
                        && let Some(n) = n
                    {
                        let _ = app.run("liquid.pageRule", json!({"rule": r, "pages": [n]}));
                    }
                }
            })
            .response
            .accessible_name(crate::i18n::tr(&lang, "Liquid Page Rule:"));
    });
    vsep(ui);
    ui.add_enabled_ui(n.is_some(), |ui| {
        let mut overlay = pg.show_parent_overlay;
        if ui.checkbox(&mut overlay, crate::rtl::widget(ui, crate::i18n::tr(&lang, "Show Parent Page Overlay"))).changed()
            && let Some(n) = n
        {
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
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::text(app, ui);
        return;
    }
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
        craft_ui_legacy::ComboBox::from_id_salt("ctl_char_style")
            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &cs)))
            .width(150.0)
            .show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(*n == cs, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                        pick = Some(n.clone());
                    }
                }
            }).response.accessible_name(tr("Character Style"));
        if let Some(n) = pick {
            let _ = app.run("style.character.apply", json!({"name": n}));
        }
        let lang = chars["language"].as_str().unwrap_or("English: USA").to_string();
        craft_ui_legacy::ComboBox::from_id_salt("ctl_language").selected_text(crate::rtl::widget(ui, tr(&lang))).width(150.0).show_ui(ui, |ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| crate::panels::properties::language_menu(app, ui, &lang));
        }).response.accessible_name(tr("Language"));
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

/// The page and text Control panel on the framework's components, in fixed two-row groups as the object panel's: the
/// same fields, buttons and commands as the native panel above.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::{Align, DesignApp, PARA_MODE, SIZES, TRACKING, Unit, panels};
    use crate::framework_transform::{self as kit, Length, Letters, Number, Quantity};
    use crate::object_control::HEIGHT;
    use crate::object_control::framework::{cell, separator};
    use egui::{Rect, Sense, Ui, vec2};
    use serde_json::{Value, json};

    /// The scales the Control panel lists (percent).
    const SCALES: &[f64] = &[25.0, 50.0, 75.0, 80.0, 90.0, 100.0, 110.0, 120.0, 125.0, 150.0, 200.0];

    /// A group of the panel `width` wide, its two rows drawn by `draw` in its rect.
    fn group(ui: &mut Ui, key: &str, width: f32, draw: impl FnOnce(&mut Ui, Rect)) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
        ui.interact(rect, egui::Id::unique(("control_bar", key)), Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt(("control_bar", key)).max_rect(rect));
        draw(&mut child, rect);
    }

    /// The id of the control `key` of the panel.
    fn id(key: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
        egui::Id::new(("control-bar", key))
    }

    /// The width a label takes in the panel's caption type, rounded up.
    fn text_width(ui: &Ui, text: &str) -> f32 {
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::proportional(crate::widgets::LABEL_SIZE), egui::Color32::PLACEHOLDER)
            .size()
            .x
            .ceil()
    }

    /// The icon of a field and the field after it in row `row` of `rect`, as the native panel lays them: the value it
    /// committed this pass.
    fn icon_field(ui: &mut Ui, rect: Rect, row: usize, key: &str, artwork: &'static str, width: f32, n: Number<'_>) -> Option<f64> {
        let enabled = true;
        cell(ui, rect, 0.0, row, 22.0 + width, enabled, |ui, at| {
            let icon_at = Rect::from_min_size(at.min + vec2(0.0, 1.5), vec2(18.0, 18.0));
            let field_at = Rect::from_min_size(at.min + vec2(22.0, 0.0), vec2(width, at.height()));
            kit::icon_number(ui, icon_at, field_at, id(key), artwork, n).flatten()
        })
    }

    /// The Control panel with the Page tool: reference point, X/Y, W/H, page-size preset, orientation, Liquid Page
    /// Rule and Show Parent Page Overlay, for the page the Page tool selected (else the page in view).
    pub(super) fn page(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let Some(st) = app.session.active() else { return };
        let units = st.doc.settings.horizontal_units;
        let Some((spread, page)) = crate::canvas::page_target(app) else { return };
        let Some(st) = app.session.active() else { return };
        let d = st.doc.clone();
        let Some(sp) = d.spread(spread) else { return };
        let Some(pg) = sp.pages.get(page).cloned() else { return };
        let n = match spread {
            designcraft_doc::SpreadRef::Doc(si) => Some(d.first_page_of_spread(si) + page + 1),
            _ => None,
        };
        let resize = |app: &mut DesignApp, params| {
            if let Some((cmd, params)) = designcraft_tools::layout::page_size_command(&d, spread, page, params) {
                let _ = app.run(cmd, params);
            }
        };
        // Reference point: W/H edits keep it in place.
        group(ui, "page_reference", 30.0, |ui, rect| {
            let proxy = Rect::from_center_size(rect.center(), vec2(27.0, 27.0));
            if let Some(p) = panels::properties::reference_point(ui, proxy, id("page_reference"), &lang, app.ui.ref_point) {
                app.ui.ref_point = p;
            }
        });
        let anchor = app.ui.ref_point.min(8);
        let (fx, fy) = ((anchor % 3) as f64 / 2.0, (anchor / 3) as f64 / 2.0);
        // The reference point's position: from the spine across, from the top down.
        let (x, y) = (pg.x - sp.spine_x() + fx * pg.width, fy * pg.height);
        group(ui, "page_xywh", 168.0, |ui, rect| {
            for (row, cells) in [[("X:", "x", x, false), ("W:", "width", pg.width, true)], [("Y:", "y", y, false), ("H:", "height", pg.height, true)]]
                .into_iter()
                .enumerate()
            {
                for (column, (caption, key, value, enabled)) in cells.into_iter().enumerate() {
                    let at = column as f32 * 88.0;
                    cell(ui, rect, at, row, 14.0, true, |ui, r| kit::caption(ui, Some(r), &tr(caption)));
                    let name = tr(caption.trim_end_matches(':'));
                    // Pages don't move within their spread yet: X and Y show where the page is.
                    let committed = cell(ui, rect, at + 16.0, row, 64.0, enabled, |ui, r| {
                        kit::length_field(ui, r, id(("page", key)), &name, Length { value: Some(value), unit: units, range: None }, enabled).0
                    });
                    if let Some(v) = committed {
                        resize(app, json!({ key: v, "anchor": anchor }));
                    }
                }
            }
        });
        separator(ui);
        // Page-size preset and orientation.
        let size = (pg.width, pg.height);
        let landscape = size.0 > size.1;
        group(ui, "page_preset", 130.0, |ui, rect| {
            let cur = panels::properties::preset_name(size.0, size.1);
            let presets = panels::properties::PAGE_PRESETS;
            let choices: Vec<(usize, String)> = presets.iter().enumerate().map(|(k, (name, _, _))| (k, tr(name))).collect();
            let held = presets.iter().position(|(name, _, _)| *name == cur);
            let shown = tr(cur);
            let picked = cell(ui, rect, 0.0, 0, 130.0, true, |ui, r| {
                kit::choice(ui, Some(r), id("page_preset"), &tr("Page Size"), held, &choices, Some(shown.as_str()))
            });
            if let Some((_, w, h)) = picked.and_then(|k| presets.get(k)) {
                let (w, h) = if landscape { (*h, *w) } else { (*w, *h) };
                resize(app, json!({"width": w, "height": h, "anchor": anchor}));
            }
            for (k, (icon, tip, want)) in [("orient-portrait", "Portrait", false), ("orient-landscape", "Landscape", true)].into_iter().enumerate() {
                let on = landscape == want;
                if cell(ui, rect, k as f32 * 24.0, 1, 20.0, true, |ui, r| kit::icon_button(ui, r, id(icon), icon, &tr(tip), on)) && !on {
                    resize(app, json!({"width": size.1, "height": size.0, "anchor": anchor}));
                }
            }
        });
        separator(ui);
        group(ui, "page_rule", 150.0, |ui, rect| {
            let label = tr("Liquid Page Rule:");
            cell(ui, rect, 0.0, 0, 150.0, n.is_some(), |ui, r| kit::caption(ui, Some(r), &label));
            let rules = panels::interactive::LIQUID_RULES;
            let choices: Vec<(usize, String)> = rules.iter().enumerate().map(|(k, (_, l))| (k, tr(l))).collect();
            let held = rules.iter().position(|(r, _)| *r == pg.liquid);
            let shown = tr(panels::interactive::liquid_rule_label(pg.liquid));
            let picked = cell(ui, rect, 0.0, 1, 150.0, n.is_some(), |ui, r| {
                kit::choice(ui, Some(r), id("page_rule"), &label, held, &choices, Some(shown.as_str()))
            });
            if let Some((rule, _)) = picked.and_then(|k| rules.get(k))
                && *rule != pg.liquid
                && let Some(n) = n
            {
                let _ = app.run("liquid.pageRule", json!({"rule": rule, "pages": [n]}));
            }
        });
        separator(ui);
        let label = tr("Show Parent Page Overlay");
        let width = text_width(ui, &label) + 28.0;
        group(ui, "page_overlay", width, |ui, rect| {
            let toggled = cell(ui, rect, 0.0, 0, width, n.is_some(), |ui, r| {
                kit::check(ui, Some(r), id("page_overlay"), &label, pg.show_parent_overlay, n.is_some())
            });
            if let Some(overlay) = toggled
                && let Some(n) = n
            {
                let _ = app.run("layout.pageOptions", json!({"pages": [n], "showParentOverlay": overlay}));
            }
        });
    }

    /// The text Control panel: A shows the Character Formatting Controls, ¶ the Paragraph Formatting Controls, as the
    /// native panel's.
    pub(super) fn text(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let attrs = panels::text_attrs(app);
        let chars = attrs.as_ref().map(|a| a["chars"].clone()).unwrap_or_default();
        let para = attrs.as_ref().map(|a| a["para"].clone()).unwrap_or_default();
        let mode_id = egui::Id::unique(PARA_MODE);
        let para_mode = ui.data(|d| d.get_temp::<bool>(mode_id)).unwrap_or(false);
        group(ui, "text_mode", 24.0, |ui, rect| {
            for (row, (mode, text, tip)) in
                [(false, "A", "Character Formatting Controls"), (true, "¶", "Paragraph Formatting Controls")].into_iter().enumerate()
            {
                let on = para_mode == mode;
                let letters = Letters { text, struck: false };
                if cell(ui, rect, 0.0, row, 24.0, true, |ui, r| kit::letters_button(ui, r, id(("text_mode", row)), letters, &tr(tip), on)) {
                    ui.data_mut(|d| d.insert_temp(mode_id, mode));
                }
            }
        });
        separator(ui);
        if para_mode {
            paragraph(app, ui, &attrs, &para, &tr);
        } else {
            character(app, ui, &attrs, &chars, &para, &tr);
        }
    }

    /// The Character Formatting Controls and the text commands they run.
    fn character(app: &mut DesignApp, ui: &mut Ui, attrs: &Option<Value>, chars: &Value, para: &Value, tr: &dyn Fn(&str) -> String) {
        group(ui, "font", 170.0, |ui, rect| {
            let fam = chars["fontFamily"].as_str().unwrap_or("").to_string();
            let sty = chars["fontStyle"].as_str().unwrap_or("").to_string();
            cell(ui, rect, 0.0, 0, 170.0, true, |ui, _| panels::font_family_picker(app, ui, &fam, 170.0));
            cell(ui, rect, 0.0, 1, 170.0, true, |ui, _| panels::font_style_picker(app, ui, &fam, &sty, 170.0));
        });
        separator(ui);
        // Size and leading, each with the list of common values.
        group(ui, "size_leading", 106.0, |ui, rect| {
            let size_label = tr("Font Size");
            let size = Number::new(&size_label, chars["size"].as_f64(), Quantity::Points).presets(SIZES).range(0.1, 1296.0);
            if let Some(v) = icon_field(ui, rect, 0, "size", "font-size", 84.0, size) {
                let _ = app.run("type.char", json!({"attrs": {"size": v}}));
            }
            // Automatic leading shows its value; the native field puts it in parentheses.
            let auto = chars["leading"]["kind"].as_str() != Some("points");
            let lead = if auto {
                chars["size"].as_f64().map(|s| s * para["autoLeading"].as_f64().unwrap_or(1.2))
            } else {
                chars["leading"]["value"].as_f64()
            };
            let lead = lead.map(|v| (v * 100.0).round() / 100.0);
            let lead_label = tr("Leading");
            let leading = Number::new(&lead_label, lead, Quantity::Points).presets(SIZES).range(0.0, 5000.0);
            if let Some(v) = icon_field(ui, rect, 1, if auto { "leading_auto" } else { "leading" }, "leading", 84.0, leading) {
                let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
            }
        });
        separator(ui);
        // All Caps, Superscript, Underline / Small Caps, Subscript, Strikethrough.
        let caps = chars["capitalization"].as_str().unwrap_or("normal").to_string();
        let pos = chars["position"].as_str().unwrap_or("normal").to_string();
        let (ul, st) = (chars["underline"].as_bool().unwrap_or(false), chars["strikethrough"].as_bool().unwrap_or(false));
        let mut set = None;
        group(ui, "case", 76.0, |ui, rect| {
            let buttons: [[(&'static str, bool, bool, &str, Value); 3]; 2] = [
                [
                    ("TT", false, caps == "allCaps", "All Caps", json!({"capitalization": if caps == "allCaps" { "normal" } else { "allCaps" }})),
                    (
                        "T¹",
                        false,
                        pos == "superscript",
                        "Superscript",
                        json!({"position": if pos == "superscript" { "normal" } else { "superscript" }}),
                    ),
                    ("T̲", false, ul, "Underline", json!({"underline": !ul})),
                ],
                [
                    (
                        "small-caps",
                        false,
                        caps == "smallCaps",
                        "Small Caps",
                        json!({"capitalization": if caps == "smallCaps" { "normal" } else { "smallCaps" }}),
                    ),
                    ("T₁", false, pos == "subscript", "Subscript", json!({"position": if pos == "subscript" { "normal" } else { "subscript" }})),
                    ("T", true, st, "Strikethrough", json!({"strikethrough": !st})),
                ],
            ];
            for (row, buttons) in buttons.into_iter().enumerate() {
                for (k, (text, struck, on, tip, attrs)) in buttons.into_iter().enumerate() {
                    let tip = tr(tip);
                    let clicked = cell(ui, rect, k as f32 * 26.0, row, 24.0, true, |ui, r| {
                        // Small Caps has always been drawn, not lettered.
                        if text == "small-caps" {
                            kit::icon_button(ui, r, id(("case", row, k)), "small-caps", &tip, on)
                        } else {
                            kit::letters_button(ui, r, id(("case", row, k)), Letters { text, struck }, &tip, on)
                        }
                    });
                    if clicked {
                        set = Some(attrs);
                    }
                }
            }
        });
        if let Some(a) = set {
            let _ = app.run("type.char", json!({"attrs": a}));
        }
        separator(ui);
        group(ui, "kern_track", 114.0, |ui, rect| {
            let kerning = tr("Kerning");
            cell(ui, rect, 0.0, 0, 114.0, true, |ui, at| {
                kit::icon(ui, Rect::from_min_size(at.min + vec2(0.0, 1.5), vec2(18.0, 18.0)), "kerning", Some(kerning.as_str()), true);
                let field = Rect::from_min_size(at.min + vec2(22.0, 0.0), vec2(92.0, at.height()));
                panels::properties::kerning_field(app, ui, Some(field), id("kerning"), &chars["kerning"]);
            });
            let tracking_label = tr("Tracking");
            let tracking = Number::new(&tracking_label, chars["tracking"].as_f64(), Quantity::Plain { decimals: 0 })
                .presets(TRACKING)
                .step(5.0)
                .range(-1000.0, 10000.0);
            if let Some(v) = icon_field(ui, rect, 1, "tracking", "tracking", 92.0, tracking) {
                let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
            }
        });
        separator(ui);
        let pct = |v: &Value| v.as_f64().map(|s| (s * 1000.0).round() / 10.0);
        group(ui, "scales", 106.0, |ui, rect| {
            for (row, key, icon, tip) in [(0, "vScale", "vscale", "Vertical Scale"), (1, "hScale", "hscale", "Horizontal Scale")] {
                let label = tr(tip);
                let n = Number::new(&label, pct(&chars[key]), Quantity::Percent).presets(SCALES).range(1.0, 1000.0);
                if let Some(v) = icon_field(ui, rect, row, key, icon, 84.0, n) {
                    let _ = app.run("type.char", json!({"attrs": {key: v / 100.0}}));
                }
            }
        });
        separator(ui);
        group(ui, "shift_skew", 98.0, |ui, rect| {
            let shift_label = tr("Baseline Shift");
            let shift = Number::new(&shift_label, chars["baselineShift"].as_f64(), Quantity::Points).range(-5000.0, 5000.0);
            if let Some(v) = icon_field(ui, rect, 0, "baselineShift", "baseline-shift", 76.0, shift) {
                let _ = app.run("type.char", json!({"attrs": {"baselineShift": v}}));
            }
            let skew_label = tr("Skew (False Italic)");
            let skew = Number::new(&skew_label, chars["skew"].as_f64(), Quantity::Degrees).range(-85.0, 85.0);
            if let Some(v) = icon_field(ui, rect, 1, "skew", "skew", 76.0, skew) {
                let _ = app.run("type.char", json!({"attrs": {"skew": v}}));
            }
        });
        separator(ui);
        // The character style, then the language.
        group(ui, "char_style_language", 150.0, |ui, rect| {
            let cs = attrs.as_ref().and_then(|a| a["characterStyle"].as_str()).unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_string();
            let names: Vec<String> =
                app.session.active().map(|d| d.doc.styles.character.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
            let choices: Vec<(String, String)> = names.iter().map(|n| (n.clone(), crate::i18n::style_name(&app.ui.language, n).to_owned())).collect();
            let held = names.iter().find(|n| **n == cs).cloned();
            let shown = crate::i18n::style_name(&app.ui.language, &cs).to_owned();
            let label = tr("Character Style");
            let picked =
                cell(ui, rect, 0.0, 0, 150.0, true, |ui, r| kit::choice(ui, Some(r), id("char_style"), &label, held, &choices, Some(shown.as_str())));
            if let Some(n) = picked {
                let _ = app.run("style.character.apply", json!({"name": n}));
            }
            let language = chars["language"].as_str().unwrap_or("English: USA").to_string();
            cell(ui, rect, 0.0, 1, 150.0, true, |ui, r| panels::properties::language_select(app, ui, Some(r), id("language"), &language));
        });
    }

    /// The Paragraph Formatting Controls and the text commands they run.
    fn paragraph(app: &mut DesignApp, ui: &mut Ui, attrs: &Option<Value>, para: &Value, tr: &dyn Fn(&str) -> String) {
        let cur: Align = serde_json::from_value(para["align"].clone()).unwrap_or_default();
        let mut align = None;
        group(ui, "align", 116.0, |ui, rect| {
            let rows: [&[(Align, &'static str)]; 2] = [
                &[
                    (Align::Left, "palign-left"),
                    (Align::Center, "palign-center"),
                    (Align::Right, "palign-right"),
                    (Align::TowardsSpine, "palign-spine-towards"),
                ],
                &[
                    (Align::LeftJustified, "palign-justify-left"),
                    (Align::CenterJustified, "palign-justify-center"),
                    (Align::RightJustified, "palign-justify-right"),
                    (Align::FullyJustified, "palign-justify-all"),
                    (Align::AwayFromSpine, "palign-spine-away"),
                ],
            ];
            for (row, buttons) in rows.into_iter().enumerate() {
                for (k, (a, icon)) in buttons.iter().copied().enumerate() {
                    if cell(ui, rect, k as f32 * 24.0, row, 20.0, true, |ui, r| kit::icon_button(ui, r, id(icon), icon, &tr(a.label()), cur == a)) {
                        align = Some(a);
                    }
                }
            }
        });
        if let Some(a) = align {
            let _ = app.run("type.para", json!({"attrs": {"align": a}}));
        }
        separator(ui);
        let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
        // The indents and space before and after, two per group.
        for (k, ((ia, ta, ka), (ib, tb, kb))) in [
            (("indent-left", "Left Indent", "leftIndent"), ("indent-first", "First Line Left Indent", "firstLineIndent")),
            (("indent-right", "Right Indent", "rightIndent"), ("indent-last", "Last Line Right Indent", "lastLineIndent")),
            (("space-before", "Space Before", "spaceBefore"), ("space-after", "Space After", "spaceAfter")),
        ]
        .into_iter()
        .enumerate()
        {
            let mut set = None;
            group(ui, &format!("indents{k}"), 106.0, |ui, rect| {
                for (row, (icon, tip, key)) in [(ia, ta, ka), (ib, tb, kb)].into_iter().enumerate() {
                    let min = if row == 0 { 0.0 } else { crate::panels::properties::para_field_min(key) };
                    let label = tr(tip);
                    let n = Number::new(&label, para[key].as_f64(), Quantity::Length(units)).range(min, 10_000.0);
                    if let Some(v) = icon_field(ui, rect, row, key, icon, 84.0, n) {
                        set = Some((key, v));
                    }
                }
            });
            if let Some((key, v)) = set {
                let _ = app.run("type.para", json!({"attrs": {key: v}}));
            }
            separator(ui);
        }
        // The drop cap's line and character counts.
        let mut drop = None;
        group(ui, "drop_cap", 86.0, |ui, rect| {
            for (row, (icon, tip, key, max)) in [
                ("drop-lines", "Drop Cap Number of Lines", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES as f64),
                ("drop-chars", "Drop Cap One or More Characters", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS as f64),
            ]
            .into_iter()
            .enumerate()
            {
                let label = tr(tip);
                let n = Number::new(&label, para[key].as_f64(), Quantity::Plain { decimals: 0 }).range(0.0, max).step(1.0);
                if let Some(v) = icon_field(ui, rect, row, key, icon, 64.0, n) {
                    drop = Some((key, v));
                }
            }
        });
        if let Some((key, v)) = drop {
            let _ = app.run("type.para", json!({"attrs": {key: v.max(0.0) as u64}}));
        }
        separator(ui);
        // Bulleted List over Numbered List: pressed when every selected paragraph has it, as Type › Apply Bullets
        // shows.
        use designcraft_doc::ListType;
        let pressed = [ListType::Bullets, ListType::Numbers].map(|k| app.session.selection_has_list(k));
        let mut list = None;
        group(ui, "lists", 20.0, |ui, rect| {
            for (row, (icon, cmd, tip)) in
                [("bullets", "list.bullets", "Bulleted List"), ("numbering", "list.numbers", "Numbered List")].into_iter().enumerate()
            {
                let on = pressed[row];
                if cell(ui, rect, 0.0, row, 20.0, true, |ui, r| kit::icon_button(ui, r, id(icon), icon, &tr(tip), on)) {
                    list = Some((cmd, !on));
                }
            }
        });
        if let Some((cmd, on)) = list {
            let _ = app.run(cmd, json!({"on": on}));
        }
        separator(ui);
        // Shading over Border: on or off, the Paragraph Borders and Shading dialog on its tab, and the colour.
        let doc = app.session.active().map(|d| d.doc.clone());
        let check_w = ["Shading", "Border"].iter().map(|l| text_width(ui, &tr(l))).fold(0.0, f32::max) + 28.0;
        let mut shade = None;
        let mut dialog = None;
        group(ui, "shading_border", check_w + 4.0 + 20.0 + 4.0 + 96.0, |ui, rect| {
            for (row, (on_key, color_key, label, icon, tab, color_label)) in [
                ("shadingOn", "shadingColor", "Shading", "para-shading", "shading", "Paragraph Shading"),
                ("borderOn", "borderColor", "Border", "para-border", "border", "Paragraph Border"),
            ]
            .into_iter()
            .enumerate()
            {
                let on = para[on_key].as_bool().unwrap_or(false);
                if let Some(v) = cell(ui, rect, 0.0, row, check_w, true, |ui, r| kit::check(ui, Some(r), id(on_key), &tr(label), on, true)) {
                    shade = Some(json!({ on_key: v }));
                }
                let dialog_tip = tr("Paragraph Borders and Shading…");
                if cell(ui, rect, check_w + 4.0, row, 20.0, true, |ui, r| kit::icon_button(ui, r, id(icon), icon, &dialog_tip, false)) {
                    dialog = Some(tab);
                }
                if let Some(doc) = &doc {
                    let cur = para[color_key].as_str().unwrap_or("[Black]").to_string();
                    let name = tr(color_label);
                    let picked = cell(ui, rect, check_w + 28.0, row, 96.0, true, |ui, r| {
                        kit::swatch_select(ui, Some(r), id(color_key), &name, doc, Some(cur.as_str()), Some(cur.as_str()))
                    });
                    if let Some(w) = picked {
                        shade = Some(json!({ color_key: w }));
                    }
                }
            }
        });
        if let Some(attrs) = shade {
            let _ = app.run("type.para", json!({ "attrs": attrs }));
        }
        if let Some(tab) = dialog {
            let _ = app.run("app.paragraphBordersDialog", json!({ "tab": tab }));
        }
        separator(ui);
        // The paragraph style, then Hyphenate and Align to Baseline Grid.
        let hyphenate = tr("Hyphenate");
        let hyphenate_w = text_width(ui, &hyphenate) + 28.0;
        group(ui, "para_style", 170.0_f32.max(hyphenate_w + 24.0), |ui, rect| {
            let ps = attrs.as_ref().and_then(|a| a["paragraphStyle"].as_str()).unwrap_or("").to_string();
            let ov = attrs.as_ref().and_then(|a| a["paraOverrides"].as_u64()).unwrap_or(0)
                + attrs.as_ref().and_then(|a| a["charOverrides"].as_u64()).unwrap_or(0);
            cell(ui, rect, 0.0, 0, 170.0, true, |ui, _| panels::para_style_picker(app, ui, &ps, ov > 0, 170.0));
            let h = para["hyphenate"].as_bool().unwrap_or(true);
            if let Some(h) = cell(ui, rect, 0.0, 1, hyphenate_w, true, |ui, r| kit::check(ui, Some(r), id("hyphenate"), &hyphenate, h, true)) {
                let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
            }
            // First Line Only is aligned too: the button shows it pressed, and a click turns alignment off.
            let grid = matches!(para["gridAlign"].as_str(), Some("allLines" | "firstLineOnly"));
            let tip = tr("Align to Baseline Grid");
            if cell(ui, rect, hyphenate_w + 4.0, 1, 20.0, true, |ui, r| kit::icon_button(ui, r, id("grid-baseline"), "grid-baseline", &tip, grid)) {
                let _ = app.run("type.para", json!({"attrs": {"gridAlign": if grid { "none" } else { "allLines" }}}));
            }
        });
    }
}

/// The page and text Control panel with the framework installed: the same commands from the same controls as the
/// native panel's tests in `chrome/tests.rs` drive, through real pointer and key input on the shipping top panel.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_tests {
    use crate::DesignApp;
    use crate::theme::Tokens;
    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, PointerButton, Rect, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::{Value, json};

    struct Rig {
        app: DesignApp,
        bar: Option<Rect>,
    }

    fn step(h: &mut Harness<'_, Rig>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }
    fn event(h: &mut Harness<'_, Rig>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn click_at(h: &mut Harness<'_, Rig>, pos: egui::Pos2) {
        for pressed in [true, false] {
            event(h, Event::PointerMoved(pos));
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        idle(h, 3);
    }
    fn type_text(h: &mut Harness<'_, Rig>, text: &str) {
        event(h, Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
        event(h, Event::Key { key: Key::A, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::COMMAND });
        event(h, Event::Text(text.into()));
        for pressed in [true, false] {
            event(h, Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        idle(h, 3);
    }

    /// The Control panel alone, on a context with the framework installed, at a window wide enough for every group.
    fn harness(mut app: DesignApp) -> Harness<'static, Rig> {
        app.run("window.workspace", json!({"name": "Advanced"})).unwrap();
        assert!(app.ui.control_bar);
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        assert!(app.framework_installed);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(2400.0, 700.0)).build_ui_state(
            move |ui, state: &mut Rig| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(state.app.ui.brightness));
                    ready = true;
                    return;
                }
                let parent = ui.scope_id().with("control_bar");
                crate::chrome::control_bar(&mut state.app, ui);
                state.bar = egui::containers::panel::PanelState::load(ui.ctx(), parent).map(|panel| panel.outer_rect);
            },
            Rig { app, bar: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }

    /// A text frame with its text selected, with the Type tool.
    fn text_app(text: &str) -> (DesignApp, Value) {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let f = session.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
        session.execute("text.select", &json!({"story": f["story"], "anchor": 0, "focus": text.chars().count()})).unwrap();
        session.set_tool("type");
        (DesignApp::new_for_panel_tests(session, crate::Services::default()), f)
    }

    /// The control named `label` with one of `roles` in the Control panel, the `nth` in reading order.
    fn control(h: &Harness<'_, Rig>, label: &str, roles: &[Role], nth: usize) -> Rect {
        let bar = h.state().bar.expect("the shipping top panel persisted its rect");
        let mut found: Vec<Rect> = h
            .query_all_by_label(label)
            .filter(|n| roles.contains(&n.accesskit_node().role()))
            .map(|n| n.rect())
            .filter(|r| bar.contains(r.center()))
            .collect();
        found.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
        *found.get(nth).unwrap_or_else(|| panic!("no {label:?} ({roles:?}) #{nth} in the Control panel"))
    }
    fn has(h: &Harness<'_, Rig>, label: &str) -> bool {
        let bar = h.state().bar.unwrap();
        h.query_all_by_label(label).any(|n| bar.contains(n.rect().center()))
    }
    fn press(h: &mut Harness<'_, Rig>, label: &str, roles: &[Role], nth: usize) -> Vec<(String, Value)> {
        let start = h.state().app.session.journal.len();
        let at = control(h, label, roles, nth).center();
        click_at(h, at);
        h.state().app.session.journal[start..].to_vec()
    }
    fn button(h: &mut Harness<'_, Rig>, label: &str) -> Vec<(String, Value)> {
        press(h, label, &[Role::Button], 0)
    }
    fn attrs(h: &mut Harness<'_, Rig>) -> Value {
        crate::panels::text_attrs(&mut h.state_mut().app).unwrap()
    }
    fn undo_depth(h: &Harness<'_, Rig>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }
    const FIELDS: &[Role] = &[Role::SpinButton, Role::TextInput];

    #[test]
    fn installed_page_tool_control_panel_sets_orientation_and_parent_overlay() {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({"width": 252, "height": 144, "facingPages": false})).unwrap();
        session.set_tool("page");
        let mut h = harness(DesignApp::new_for_panel_tests(session, crate::Services::default()));
        control(&h, "Liquid Page Rule:", &[Role::ComboBox], 0);
        control(&h, "Page Size", &[Role::ComboBox], 0);
        assert!(!has(&h, "Objects Move with Page"));
        let ran = press(&mut h, "Show Parent Page Overlay", &[Role::CheckBox], 0);
        assert_eq!(ran, [("layout.pageOptions".to_owned(), json!({"pages": [1], "showParentOverlay": true}))]);
        assert!(h.state().app.session.doc().unwrap().doc.spreads[0].pages[0].show_parent_overlay);
        // Landscape is pressed for a wide page; Portrait swaps its sides about the reference point, in one step.
        assert!(button(&mut h, "Landscape").is_empty(), "the pressed orientation changes nothing");
        let before = undo_depth(&h);
        let ran = button(&mut h, "Portrait");
        assert_eq!(ran.len(), 1, "one page-size command: {ran:?}");
        let page = &h.state().app.session.doc().unwrap().doc.spreads[0].pages[0];
        assert_eq!((page.width, page.height), (144.0, 252.0));
        assert_eq!(undo_depth(&h), before + 1);
    }

    #[test]
    fn installed_type_control_panel_sets_size_and_fixes_automatic_leading() {
        let (app, f) = text_app("Headline");
        let mut h = harness(app);
        let story = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
        let format = |h: &Harness<'_, Rig>| h.state().app.session.doc().unwrap().doc.story(story).unwrap().format_after(0).over.clone();
        let at = control(&h, "Font Size", FIELDS, 0).center();
        click_at(&mut h, at);
        let start = h.state().app.session.journal.len();
        type_text(&mut h, "72");
        assert_eq!(&h.state().app.session.journal[start..], &[("type.char".to_owned(), json!({"attrs": {"size": 72.0}}))]);
        assert_eq!(format(&h).size, Some(72.0));
        // Typing the automatic value itself fixes the leading at it.
        let at = control(&h, "Leading", FIELDS, 0).center();
        click_at(&mut h, at);
        let start = h.state().app.session.journal.len();
        type_text(&mut h, "60");
        assert_eq!(
            &h.state().app.session.journal[start..],
            &[("type.char".to_owned(), json!({"attrs": {"leading": {"kind": "points", "value": 60.0}}}))]
        );
        assert_eq!(format(&h).leading, Some(designcraft_doc::Leading::Points(60.0)));
    }

    #[test]
    fn installed_type_control_panel_has_character_and_paragraph_modes() {
        let (app, _) = text_app("Bulbs");
        let mut h = harness(app);
        assert!(has(&h, "Small Caps") && has(&h, "Vertical Scale") && !has(&h, "Drop Cap Number of Lines"));
        assert_eq!(button(&mut h, "Small Caps"), [("type.char".to_owned(), json!({"attrs": {"capitalization": "smallCaps"}}))]);
        assert!(button(&mut h, "Paragraph Formatting Controls").is_empty(), "the mode is the panel's own, no command");
        assert!(has(&h, "Drop Cap Number of Lines") && has(&h, "Space Before") && !has(&h, "Small Caps"));
        assert_eq!(button(&mut h, "Align to Baseline Grid"), [("type.para".to_owned(), json!({"attrs": {"gridAlign": "allLines"}}))]);
        let center = crate::i18n::tr(&h.state().app.ui.language, designcraft_doc::Align::Center.label()).to_owned();
        assert_eq!(button(&mut h, &center), [("type.para".to_owned(), json!({"attrs": {"align": "center"}}))]);
        let a = attrs(&mut h);
        assert_eq!(
            (a["chars"]["capitalization"].as_str(), a["para"]["gridAlign"].as_str(), a["para"]["align"].as_str()),
            (Some("smallCaps"), Some("allLines"), Some("center"))
        );
        button(&mut h, "Character Formatting Controls");
        assert!(has(&h, "Small Caps"));
    }

    #[test]
    fn installed_paragraph_control_panel_has_lists_shading_and_border() {
        let (app, _) = text_app("4 eggs\n1 cup milk");
        let mut h = harness(app);
        button(&mut h, "Paragraph Formatting Controls");
        assert_eq!(button(&mut h, "Bulleted List"), [("list.bullets".to_owned(), json!({"on": true}))]);
        let a = attrs(&mut h);
        assert_eq!(
            (a["para"]["listType"].as_str(), a["para"]["leftIndent"].as_f64(), a["para"]["firstLineIndent"].as_f64()),
            (Some("bullets"), Some(18.0), Some(-18.0))
        );
        assert_eq!(button(&mut h, "Numbered List"), [("list.numbers".to_owned(), json!({"on": true}))]);
        assert_eq!(attrs(&mut h)["para"]["listType"], "numbers");
        assert_eq!(press(&mut h, "Shading", &[Role::CheckBox], 0), [("type.para".to_owned(), json!({"attrs": {"shadingOn": true}}))]);
        assert_eq!(press(&mut h, "Border", &[Role::CheckBox], 0), [("type.para".to_owned(), json!({"attrs": {"borderOn": true}}))]);
        control(&h, "Paragraph Shading", &[Role::ComboBox], 0);
        // The button beside Border opens the dialog on its Border tab.
        press(&mut h, "Paragraph Borders and Shading…", &[Role::Button], 1);
        let d = h.state().app.ui.dialog.clone().expect("the dialog");
        assert_eq!((d.id.as_str(), d.s("section").as_str()), ("paragraphLocal", "border"));
        h.state_mut().app.ui.dialog = None;
        idle(&mut h, 2);
        // The Last Line Right Indent takes a negative value, as the First Line Left Indent does.
        let at = control(&h, "Last Line Right Indent", FIELDS, 0).center();
        click_at(&mut h, at);
        type_text(&mut h, "-0p6");
        assert_eq!(attrs(&mut h)["para"]["lastLineIndent"].as_f64(), Some(-6.0));
    }

    #[test]
    fn installed_control_panel_grid_button_keeps_first_line_only() {
        let (mut app, _) = text_app("Bulbs");
        app.run("type.para", json!({"attrs": {"gridAlign": "firstLineOnly"}})).unwrap();
        let mut h = harness(app);
        button(&mut h, "Paragraph Formatting Controls");
        let bar = h.state().bar.unwrap();
        let pressed = h
            .query_all_by_label("Align to Baseline Grid")
            .find(|n| bar.contains(n.rect().center()) && n.accesskit_node().role() == Role::Button)
            .map(|n| n.accesskit_node().toggled());
        assert_eq!(pressed, Some(Some(egui::accesskit::Toggled::True)));
        assert_eq!(attrs(&mut h)["para"]["gridAlign"], "firstLineOnly");
        assert_eq!(button(&mut h, "Align to Baseline Grid"), [("type.para".to_owned(), json!({"attrs": {"gridAlign": "none"}}))]);
        assert_eq!(attrs(&mut h)["para"]["gridAlign"], "none");
    }

    #[test]
    fn installed_object_control_panel_sets_width_about_the_reference_point_in_one_step() {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let id = session.execute("frame.create", &json!({"rect": [20, 30, 80, 90]})).unwrap()["id"].as_u64().unwrap();
        session.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let mut h = harness(DesignApp::new_for_panel_tests(session, crate::Services::default()));
        let before = undo_depth(&h);
        let at = control(&h, "W", FIELDS, 0).center();
        click_at(&mut h, at);
        let start = h.state().app.session.journal.len();
        type_text(&mut h, "10p");
        let ran = h.state().app.session.journal[start..].to_vec();
        let reference = h.state().app.ui.ref_point.min(8);
        assert_eq!(ran, [("transform.set".to_owned(), json!({"width": 120.0, "ref": reference}))]);
        let bounds = h.state().app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((bounds.width() - 120.0).abs() < 1e-6, "{bounds:?}");
        assert_eq!(undo_depth(&h), before + 1);
    }
}
