use super::typography::{para_field_min, show_typography_panel};
use super::{
    Align, DesignApp, FIELD_H, NumField, Rect, Ui, Value, block, full_width, json, section, set_temp_bool, show_panel, sub, temp_bool, text_attrs,
    units, vec2, widgets,
};

// ---------------------------------------------------------------- Text
//
// Each section here is a framework body (`framework` below) over the engine commands.

/// The text stroke weights the weight field's presets list offers, in points.
const TEXT_STROKE_WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 10.0];
/// The type sizes (and leadings) the size field's presets list offers, in points.
const TYPE_SIZES: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 21.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];
/// The trackings the tracking field's presets list offers, in 1/1000 em.
const TRACKING: &[f64] = &[-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 200.0];
/// The horizontal and vertical scales the scale fields' presets lists offer, in percent.
const SCALES: &[f64] = &[25.0, 50.0, 75.0, 80.0, 90.0, 100.0, 110.0, 120.0, 125.0, 150.0, 200.0];
/// The paragraph alignment buttons, in InDesign's order.
const PARA_ALIGN_BUTTONS: [(Align, &str); 9] = [
    (Align::Left, "palign-left"),
    (Align::Center, "palign-center"),
    (Align::Right, "palign-right"),
    (Align::LeftJustified, "palign-justify-left"),
    (Align::CenterJustified, "palign-justify-center"),
    (Align::RightJustified, "palign-justify-right"),
    (Align::FullyJustified, "palign-justify-all"),
    (Align::TowardsSpine, "palign-spine-towards"),
    (Align::AwayFromSpine, "palign-spine-away"),
];
/// The paragraph indent and spacing rows of Properties › Paragraph after •••: icon, name and attribute, left and right.
const PARA_SPACING_ROWS: [((&str, &str, &str), (&str, &str, &str)); 3] = [
    (("indent-left", "Left Indent", "leftIndent"), ("indent-right", "Right Indent", "rightIndent")),
    (("indent-first", "First Line Left Indent", "firstLineIndent"), ("indent-last", "Last Line Right Indent", "lastLineIndent")),
    (("space-before", "Space Before", "spaceBefore"), ("space-after", "Space After", "spaceAfter")),
];
/// The vertical justification buttons of Properties › Text Frame.
const VERTICAL_JUSTIFICATIONS: [(designcraft_doc::VerticalJustification, &str, &str); 4] = [
    (designcraft_doc::VerticalJustification::Top, "vjust-top", "Top"),
    (designcraft_doc::VerticalJustification::Center, "vjust-center", "Center"),
    (designcraft_doc::VerticalJustification::Bottom, "vjust-bottom", "Bottom"),
    (designcraft_doc::VerticalJustification::Justify, "vjust-justify", "Justify"),
];
/// The text frame inset fields: the inset's index in `[top, left, bottom, right]`, its key, icon and name, and where
/// its icon sits in the block.
const INSET_FIELDS: [(usize, &str, &str, &str, f32, f32); 4] = [
    (0, "tfInsetTop", "margin-top", "Top", 0.0, 0.0),
    (2, "tfInsetBottom", "margin-bottom", "Bottom", 0.0, FIELD_H + 5.0),
    (1, "tfInsetLeft", "margin-inside", "Left", 140.0, 0.0),
    (3, "tfInsetRight", "margin-outside", "Right", 140.0, FIELD_H + 5.0),
];
/// Change Case's rows: label and capitalization.
const CHANGE_CASE: [(&str, &str); 3] = [("UPPERCASE", "allCaps"), ("Small Caps", "smallCaps"), ("Normal", "normal")];

pub(super) fn text_style_section(app: &mut DesignApp, ui: &mut Ui) {
    framework::text_style_section(app, ui)
}

pub(super) fn text_appearance_section(app: &mut DesignApp, ui: &mut Ui) {
    framework::text_appearance_section(app, ui)
}

/// Character section: font family (with search), style, size/leading, kerning/tracking, •••.
pub(super) fn character_section(app: &mut DesignApp, ui: &mut Ui) {
    framework::character_section(app, ui)
}

/// Paragraph section: the 9 alignment buttons and •••.
pub(super) fn paragraph_section(app: &mut DesignApp, ui: &mut Ui) {
    framework::paragraph_section(app, ui)
}

pub(super) fn bullets_section(app: &mut DesignApp, ui: &mut Ui) {
    framework::bullets_section(app, ui)
}

pub(super) fn quick_actions_text(app: &mut DesignApp, ui: &mut Ui) {
    framework::quick_actions_text(app, ui)
}

/// The text frame the Text Frame section shows, as Text Frame Options shows it: the first selected
/// frame, the frame that shows the caret, or the first frame holding selected text. Its id, its
/// options, and whether its outline is not its own rectangle (an oval, a polygon or corner
/// options), where the text keeps one inset from the outline. Type on a path is not shaped.
pub(super) fn text_frame_target(app: &DesignApp) -> Option<(designcraft_doc::ItemId, designcraft_doc::TextFrameOptions, bool)> {
    let st = app.session.active()?;
    let fid = designcraft_engine::cmd::text_frame_options_frames(&app.session).first().copied()?;
    let item = st.doc.item(fid)?;
    let opts = item.text_frame()?.options.clone();
    Some((fid, opts, is_shaped(item)))
}

/// Whether a text frame's outline is not its own rectangle, where its text keeps one inset from
/// the outline. Type on a path is not shaped.
fn is_shaped(item: &designcraft_doc::Item) -> bool {
    item.text_frame().is_some_and(|tf| tf.options.path.is_none()) && designcraft_compose::FrameOutline::of_item(item, 0.0).is_some()
}

/// Text Frame section: columns, gutter, [Options]; Inset Spacing (Top and Bottom, the link that
/// makes all four the same, Left and Right; one Inset for a non-rectangular frame); the four
/// Vertical Justification buttons (neither for type on a path). With the Type tool it changes the
/// frame that shows the caret, or every frame that holds part of the selected text.
pub(super) fn text_frame_section(app: &mut DesignApp, ui: &mut Ui) {
    framework::text_frame_section(app, ui)
}

/// The `inset` a shaped frame's one Inset sets: the composer also bounds the text by the frame's rectangle less all
/// four insets, so when every frame it changes is shaped the one Inset sets all four. With a rectangular frame among
/// them (part of the same selected text), only the top inset changes and each frame keeps its other sides.
fn shaped_inset(app: &DesignApp, v: f64) -> Value {
    let all_shaped = app.session.active().is_some_and(|st| {
        designcraft_engine::cmd::text_frame_options_frames(&app.session)
            .iter()
            .filter_map(|id| st.doc.item(*id))
            .filter(|it| it.text_frame().is_some_and(|tf| tf.options.path.is_none()))
            .all(is_shaped)
    });
    if all_shaped { json!(v) } else { json!([v, null, null, null]) }
}

/// The key under which the inset link remembers its state, and whether it starts linked. The state belongs to the
/// frames the field changes in this document state: other frames, or frames with the same ids in another document or
/// after Revert, start linked when every one of them has four equal insets, so a frame with uneven sides keeps them
/// when one side changes.
fn inset_link(app: &DesignApp, fid: designcraft_doc::ItemId, inset: &[f64; 4]) -> (egui::Id, bool) {
    let doc_epoch = app.session.active().map_or(0, |st| st.identity_epoch);
    let targets = designcraft_engine::cmd::text_frame_options_frames(&app.session);
    let target_ids: Vec<u64> = targets.iter().map(|id| id.0).collect();
    let link_id = egui::Id::unique(("prop_tf_inset_linked", doc_epoch, fid.0, target_ids));
    let even = |i: &[f64; 4]| i.iter().all(|v| (v - i[0]).abs() < 1e-9);
    let same = even(inset)
        && app.session.active().is_none_or(|st| targets.iter().filter_map(|id| st.doc.item(*id)?.text_frame()).all(|tf| even(&tf.options.inset)));
    (link_id, same)
}

/// The insets an edit of the inset at `k` sets: linked, all four; else that side alone. Insets are
/// `[top, left, bottom, right]`.
fn inset_set(linked: bool, k: usize, v: f64) -> [Option<f64>; 4] {
    if linked {
        [Some(v); 4]
    } else {
        let mut o = [None; 4];
        o[k] = Some(v);
        o
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
    "Esperanto",
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

/// The text sections on the craft UI framework: every control a framework component over the engine commands.
pub(crate) mod framework {
    use super::{
        Align, CHANGE_CASE, DesignApp, FIELD_H, INSET_FIELDS, LANGUAGES, PARA_ALIGN_BUTTONS, PARA_SPACING_ROWS, Rect, SCALES, TEXT_STROKE_WEIGHTS,
        TRACKING, TYPE_SIZES, Ui, VERTICAL_JUSTIFICATIONS, Value, block, full_width, inset_link, inset_set, json, para_field_min, section,
        set_temp_bool, shaped_inset, show_panel, show_typography_panel, sub, temp_bool, text_attrs, text_frame_target, units, vec2, widgets,
    };
    use crate::framework_transform::{
        Letters, MenuRow, Number, Quantity, button, button_width, check, choice, font_style_select, icon_button, icon_number, icon_tip, icon_toggle,
        letters_button, menu, more_options, number_field,
    };
    use crate::panels::properties::object::framework::{SwatchFor, swatch_chip};
    use craft_behavior::Mixed;
    use craft_ui_widgets::choice::Segmented;
    use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::primitives::ContentPainterCx;
    use craft_ui_widgets::text::{NamedOr, NumberField, NumberName, NumberPick, link_toggle};

    /// The id of the control `key` of the text sections the panel's `ui` shows.
    fn cid(ui: &Ui, key: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
        ui.scope_id().with(("properties-text", key))
    }

    /// The `•••` toggle under a section: whether it was clicked this pass.
    fn more(app: &DesignApp, ui: &mut Ui, section: &'static str) -> bool {
        let id = cid(ui, ("more-options", section));
        more_options(ui, &app.ui.language, id)
    }

    pub(in crate::panels::properties) fn text_style_section(app: &mut DesignApp, ui: &mut Ui) {
        section(ui, crate::i18n::tr(&app.ui.language, "Text Style"), true);
        let Some(a) = text_attrs(app) else { return };
        let lang = app.ui.language.clone();
        let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
        let tab = app.ui.text_style_tab;
        let fw = full_width(ui);
        let tabs = [Entry::new(0u8, tr("Paragraph Styles")), Entry::new(1u8, tr("Character Styles"))];
        let b = block(ui, widgets::BUTTON_H);
        let id = cid(ui, "style-kind");
        let picked = Segmented::new(tr("Text Style"), Bind::given(tab), &tabs)
            .id(id)
            .show_at(ui, sub(b, 0.0, 0.0, fw, widgets::BUTTON_H))
            .events
            .committed()
            .copied();
        if let Some(k) = picked {
            app.ui.text_style_tab = k;
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
        // The current style shows `+` when the text overrides it, as InDesign's field does.
        let entries: Vec<Entry<String>> = names
            .iter()
            .map(|n| {
                let shown = crate::i18n::style_name(&lang, n).to_owned();
                let entry = Entry::new(n.clone(), shown.clone());
                if ov > 0 && *n == cur { entry.closed_label(format!("{shown}+")) } else { entry }
            })
            .collect();
        let held = names.iter().find(|n| **n == cur).cloned().map_or(Mixed::Mixed, Mixed::Same);
        let style_kind = tr(if tab == 0 { "Paragraph Style" } else { "Character Style" });
        let b = block(ui, 34.0);
        let id = cid(ui, ("style", tab));
        let pick = Select::new(style_kind, Bind::given(held), &entries)
            .id(id)
            // The "Ag" sample before the name, as the native field paints it.
            .picture(|_: &String, cx: &ContentPainterCx<'_>, rect: Rect| {
                cx.painter.rect_filled(rect, 1.0, egui::Color32::WHITE);
                cx.painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Ag", egui::FontId::proportional(13.0), egui::Color32::BLACK);
            })
            .show_at(ui, sub(b, 0.0, 0.0, fw, 34.0))
            .events
            .committed()
            .cloned();
        if let Some(n) = pick {
            if tab == 0 {
                let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": false}));
            } else {
                let _ = app.run("style.character.apply", json!({"name": n}));
            }
        }
        ui.add_space(4.0);
        let b = block(ui, FIELD_H);
        let panel = if tab == 0 { "paragraphStyles" } else { "characterStyles" };
        let id = cid(ui, "styles-panel");
        if icon_button(
            ui,
            sub(b, 0.0, 0.0, 23.0, 21.0),
            id,
            "pilcrow-menu",
            &tr(if tab == 0 { "Paragraph Styles panel" } else { "Character Styles panel" }),
            false,
        ) {
            show_panel(app, panel);
        }
        // At the row's end: Styles panel, Clear overrides, Create new style (reading from the end).
        let end = b.width() - 4.0;
        let id = cid(ui, "style-load");
        if icon_button(ui, sub(b, end - 3.0 * 23.0 - 2.0 * 12.0, 0.0, 23.0, 21.0), id, "style-load", &tr("Styles panel"), false) {
            show_panel(app, panel);
        }
        let id = cid(ui, "style-clear");
        if icon_button(ui, sub(b, end - 2.0 * 23.0 - 12.0, 0.0, 23.0, 21.0), id, "style-clear", &tr("Clear overrides"), false)
            && tab == 0
            && !cur.is_empty()
        {
            let _ = app.run("style.paragraph.apply", json!({"name": cur, "clearOverrides": true}));
        }
        let id = cid(ui, "style-new");
        if icon_button(ui, sub(b, end - 23.0, 0.0, 23.0, 21.0), id, "style-new", &tr("Create new style"), false) {
            let cmd = if tab == 0 { "style.paragraph.create" } else { "style.character.create" };
            let _ = app.run(cmd, json!({}));
        }
    }

    pub(in crate::panels::properties) fn text_appearance_section(app: &mut DesignApp, ui: &mut Ui) {
        let Some(a) = text_attrs(app) else { return };
        let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
        let lang = app.ui.language.clone();
        let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
        let c = &a["chars"];
        for (k, (key, label)) in [("fill", "Fill"), ("stroke", "Stroke")].into_iter().enumerate() {
            let sw = c[key].as_str().unwrap_or(designcraft_color::swatch::NONE).to_string();
            let b = block(ui, FIELD_H);
            swatch_chip(app, ui, sub(b, 4.0, 2.0, 17.0, 17.0), &doc, &sw, k != 0, SwatchFor::Text);
            if k == 0 {
                crate::framework_transform::caption(ui, Some(sub(b, 34.0, 0.0, 56.0, FIELD_H)), &tr(label));
            } else {
                let id = cid(ui, "stroke-link");
                if craft_ui_widgets::buttons::Link::new(tr(label)).id(id).show_at(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H)).clicked() {
                    show_panel(app, "stroke");
                }
                let id = cid(ui, "stroke-weight");
                let n = Number::new(crate::i18n::tr(&lang, "Stroke Weight"), c["strokeWeight"].as_f64(), Quantity::Points)
                    .presets(TEXT_STROKE_WEIGHTS)
                    .range(0.0, 800.0);
                if let Some(Some(v)) = number_field(ui, Some(sub(b, 96.0, 0.0, 69.0, FIELD_H)), id, n) {
                    let _ = app.run("type.char", json!({"attrs": {"strokeWeight": v}}));
                }
            }
            ui.add_space(4.0);
        }
        let behind = c["strokeBehindFill"].as_bool().unwrap_or(false);
        let id = cid(ui, "stroke-behind-fill");
        if let Some(on) = check(ui, None, id, &tr("Stroke Behind Fill"), behind, true) {
            let _ = app.run("type.char", json!({"attrs": {"strokeBehindFill": on}}));
        }
    }

    pub(in crate::panels::properties) fn character_section(app: &mut DesignApp, ui: &mut Ui) {
        let Some(a) = text_attrs(app) else { return };
        let lang = app.ui.language.clone();
        let c = a["chars"].clone();
        let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
        let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
        let fw = full_width(ui);
        let b = block(ui, FIELD_H);
        let id = cid(ui, "font-family");
        crate::panels::font_family_field(app, ui, id, &fam, sub(b, 0.0, 0.0, fw, FIELD_H));
        ui.add_space(1.0);
        let b = block(ui, FIELD_H);
        let id = cid(ui, "font-style");
        if let Some(style) = font_style_select(app, ui, Some(sub(b, 0.0, 0.0, fw, FIELD_H)), id, &fam, &sty) {
            let _ = app.run("type.char", json!({"attrs": {"fontStyle": style}}));
        }
        ui.add_space(1.0);
        let half = (fw - 10.0) / 2.0;
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        let size = c["size"].as_f64();
        let auto = c["leading"]["kind"].as_str() != Some("points");
        let lv = if auto { size.map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
        let field_w = half - 24.0;
        let id = cid(ui, "size");
        let n = Number::new(crate::i18n::tr(&lang, "Font Size"), size, Quantity::Points).presets(TYPE_SIZES).range(0.1, 1296.0);
        if let Some(Some(v)) = icon_number(ui, sub(b, 0.0, 1.0, 19.0, 19.0), sub(b, 24.0, 0.0, field_w, FIELD_H), id, "font-size", n) {
            let _ = app.run("type.char", json!({"attrs": {"size": v}}));
        }
        // Auto leading shows the leading it works out to, in parentheses: "(14.4 pt)".
        let id = cid(ui, ("leading", auto));
        let n = Number::new(crate::i18n::tr(&lang, "Leading"), lv.map(|v| (v * 100.0).round() / 100.0), Quantity::Points)
            .presets(TYPE_SIZES)
            .range(0.0, 5000.0)
            .derived(auto);
        if let Some(Some(v)) = icon_number(ui, sub(b, half + 10.0, 1.0, 19.0, 19.0), sub(b, half + 34.0, 0.0, field_w, FIELD_H), id, "leading", n) {
            let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
        }
        icon_tip(ui, sub(b, 0.0, 27.0, 19.0, 19.0), "kerning", crate::i18n::tr(&lang, "Kerning"), true);
        let id = cid(ui, "kerning");
        kerning_field(app, ui, Some(sub(b, 24.0, 26.0, field_w, FIELD_H)), id, &c["kerning"]);
        let id = cid(ui, "tracking");
        let n = Number::new(crate::i18n::tr(&lang, "Tracking"), c["tracking"].as_f64(), Quantity::Plain { decimals: 0 })
            .presets(TRACKING)
            .step(5.0)
            .range(-1000.0, 10000.0);
        if let Some(Some(v)) = icon_number(ui, sub(b, half + 10.0, 27.0, 19.0, 19.0), sub(b, half + 34.0, 26.0, field_w, FIELD_H), id, "tracking", n)
        {
            let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
        }
        // ••• shows the rest in place, as InDesign's Properties panel does.
        let shown = temp_bool(ui, "props_char_more", false);
        if shown {
            character_more(app, ui, &c, fw);
        }
        if more(app, ui, "character") {
            set_temp_bool(ui, "props_char_more", !shown);
        }
    }

    /// Properties › Character after •••.
    fn character_more(app: &mut DesignApp, ui: &mut Ui, c: &Value, fw: f32) {
        let lang = app.ui.language.clone();
        let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
        let half = (fw - 10.0) / 2.0;
        let field_w = half - 24.0;
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        let pct = |v: &Value| v.as_f64().map(|s| (s * 1000.0).round() / 10.0);
        let at = |right: bool, y: f32| {
            let x = if right { half + 10.0 } else { 0.0 };
            (sub(b, x, y + 1.0, 19.0, 19.0), sub(b, x + 24.0, y, field_w, FIELD_H))
        };
        for (right, key, artwork, tip) in [(false, "vScale", "vscale", "Vertical Scale"), (true, "hScale", "hscale", "Horizontal Scale")] {
            let (icon_at, field_at) = at(right, 0.0);
            let id = cid(ui, key);
            let label = tr(tip);
            let n = Number::new(&label, pct(&c[key]), Quantity::Percent).presets(SCALES).range(1.0, 1000.0);
            if let Some(Some(v)) = icon_number(ui, icon_at, field_at, id, artwork, n) {
                let _ = app.run("type.char", json!({"attrs": {key: v / 100.0}}));
            }
        }
        let (icon_at, field_at) = at(false, 26.0);
        let id = cid(ui, "baseline-shift");
        let label = tr("Baseline Shift");
        let n = Number::new(&label, c["baselineShift"].as_f64(), Quantity::Points).range(-5000.0, 5000.0);
        if let Some(Some(v)) = icon_number(ui, icon_at, field_at, id, "baseline-shift", n) {
            let _ = app.run("type.char", json!({"attrs": {"baselineShift": v}}));
        }
        let (icon_at, field_at) = at(true, 26.0);
        let id = cid(ui, "skew");
        let label = tr("Skew (False Italic)");
        let n = Number::new(&label, c["skew"].as_f64(), Quantity::Degrees).range(-85.0, 85.0);
        if let Some(Some(v)) = icon_number(ui, icon_at, field_at, id, "skew", n) {
            let _ = app.run("type.char", json!({"attrs": {"skew": v}}));
        }
        // Language (hyphenation, spelling and typographer's quotes follow it) and OpenType.
        ui.add_space(4.0);
        let current = c["language"].as_str().unwrap_or("English: USA").to_string();
        let row = block(ui, FIELD_H);
        let id = cid(ui, "language");
        language_select(app, ui, Some(sub(row, 0.0, 0.0, half, FIELD_H)), id, &current);
        let id = cid(ui, "opentype");
        if button(ui, Some(sub(row, half + 10.0, 0.0, half, FIELD_H)), id, &tr("OpenType"), true).clicked() {
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
            let id = cid(ui, ("case", i));
            let label = tr(tip);
            let clicked = if text == "small-caps" {
                icon_toggle(ui, r, id, "small-caps", &label, on)
            } else {
                letters_button(ui, r, id, Letters { text, struck: i == 5 }, &label, on)
            };
            if clicked {
                let _ = app.run("type.char", json!({"attrs": attrs}));
            }
        }
        ui.add_space(4.0);
    }

    pub(in crate::panels::properties) fn paragraph_section(app: &mut DesignApp, ui: &mut Ui) {
        let Some(a) = text_attrs(app) else { return };
        let lang = app.ui.language.clone();
        let cur: Align = serde_json::from_value(a["para"]["align"].clone()).unwrap_or_default();
        let fw = full_width(ui);
        let pitch = fw / 9.0;
        let b = block(ui, 21.0);
        for (k, (al, artwork)) in PARA_ALIGN_BUTTONS.into_iter().enumerate() {
            let (at, id) = (sub(b, k as f32 * pitch, 0.0, pitch.min(23.0), 21.0), cid(ui, ("align", k)));
            if icon_toggle(ui, at, id, artwork, crate::i18n::tr(&lang, al.label()), cur == al) {
                let _ = app.run("type.para", json!({"attrs": {"align": al}}));
            }
        }
        let shown = temp_bool(ui, "props_para_more", false);
        if shown {
            paragraph_more(app, ui, &a["para"], fw);
        }
        if more(app, ui, "paragraph") {
            set_temp_bool(ui, "props_para_more", !shown);
        }
    }

    /// Properties › Paragraph after •••.
    fn paragraph_more(app: &mut DesignApp, ui: &mut Ui, p: &Value, fw: f32) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let u = units(app);
        let half = (fw - 10.0) / 2.0;
        let field_w = half - 24.0;
        ui.add_space(4.0);
        let b = block(ui, 4.0 * FIELD_H + 15.0);
        let at = |right: bool, y: f32| {
            let x = if right { half + 10.0 } else { 0.0 };
            (sub(b, x, y + 1.0, 19.0, 19.0), sub(b, x + 24.0, y, field_w, FIELD_H))
        };
        for (i, (l, r)) in PARA_SPACING_ROWS.into_iter().enumerate() {
            let y = i as f32 * (FIELD_H + 5.0);
            for (right, (artwork, tip, key)) in [(false, l), (true, r)] {
                let (icon_at, field_at) = at(right, y);
                let id = cid(ui, key);
                let label = tr(tip);
                let n = Number::new(&label, p[key].as_f64(), Quantity::Length(u)).range(para_field_min(key), 10_000.0);
                if let Some(Some(v)) = icon_number(ui, icon_at, field_at, id, artwork, n) {
                    let _ = app.run("type.para", json!({"attrs": {key: v}}));
                }
            }
        }
        let y = 3.0 * (FIELD_H + 5.0);
        for (right, artwork, tip, key, max) in [
            (false, "drop-lines", "Drop Cap Number of Lines", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES),
            (true, "drop-chars", "Drop Cap One or More Characters", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS),
        ] {
            let (icon_at, field_at) = at(right, y);
            let id = cid(ui, key);
            let label = tr(tip);
            let n = Number::new(&label, p[key].as_f64(), Quantity::Plain { decimals: 0 }).range(0.0, max as f64);
            if let Some(Some(v)) = icon_number(ui, icon_at, field_at, id, artwork, n) {
                let _ = app.run("type.para", json!({"attrs": {key: (v.max(0.0) as u64).min(max as u64)}}));
            }
        }
        ui.add_space(4.0);
        let b = block(ui, FIELD_H);
        // First Line Only is aligned too: the button shows it pressed, and a click turns alignment off.
        let grid = matches!(p["gridAlign"].as_str(), Some("allLines" | "firstLineOnly"));
        let id = cid(ui, "grid-align");
        if icon_toggle(ui, sub(b, 0.0, 0.0, 23.0, 21.0), id, "grid-baseline", &tr("Align to Baseline Grid"), grid) {
            let _ = app.run("type.para", json!({"attrs": {"gridAlign": if grid { "none" } else { "allLines" }}}));
        }
        let id = cid(ui, "hyphenate");
        let hyphenate = p["hyphenate"].as_bool().unwrap_or(true);
        if let Some(on) = check(ui, Some(Rect::from_min_max(b.min + vec2(37.0, 0.0), b.max)), id, &tr("Hyphenate"), hyphenate, true) {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": on}}));
        }
        ui.add_space(4.0);
    }

    pub(in crate::panels::properties) fn bullets_section(app: &mut DesignApp, ui: &mut Ui) {
        if text_attrs(app).is_none() {
            return;
        }
        let lang = app.ui.language.clone();
        let b = block(ui, FIELD_H);
        use designcraft_doc::ListType;
        for (k, (artwork, kind, cmd, tip)) in
            [("bullets", ListType::Bullets, "list.bullets", "Bulleted List"), ("numbering", ListType::Numbers, "list.numbers", "Numbered List")]
                .into_iter()
                .enumerate()
        {
            // Pressed while every selected paragraph has the list, as Type › Apply Bullets shows it.
            let on = app.session.selection_has_list(kind);
            let (at, id) = (sub(b, k as f32 * 33.0, 0.0, 23.0, 21.0), cid(ui, ("list", cmd)));
            if icon_toggle(ui, at, id, artwork, crate::i18n::tr(&lang, tip), on) {
                let _ = app.run(cmd, json!({"on": !on}));
            }
        }
        // The Bullets and Numbering dialog for the selected paragraphs, at the row's end.
        let id = cid(ui, "bullets-options");
        let options = crate::i18n::tr(&lang, "Options");
        // As wide as its whole label takes, at least the old 66 pt.
        let w = button_width(ui, options).max(66.0);
        if button(ui, Some(sub(b, b.width() - 8.0 - w, 0.0, w, FIELD_H)), id, options, true).clicked() {
            let _ = app.run("app.bulletsNumberingDialog", json!({}));
        }
    }

    pub(in crate::panels::properties) fn quick_actions_text(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
        let full = full_width(ui);
        let half = (full - 10.0) / 2.0;
        let h = widgets::BUTTON_H;
        let b = block(ui, h);
        let id = cid(ui, "change-case");
        let opener = button(ui, Some(sub(b, 0.0, 0.0, half, h)), id, &tr("Change Case"), true);
        let labels: Vec<String> = CHANGE_CASE.iter().map(|(label, _)| tr(label)).collect();
        let rows: Vec<MenuRow<'_, &str>> =
            CHANGE_CASE.iter().zip(&labels).map(|((_, v), label)| MenuRow { key: *v, label, checked: None, enabled: true, shortcut: None }).collect();
        let id = cid(ui, "change-case-menu");
        if let Some(v) = menu(ui, id, &tr("Change Case"), &opener, &[rows]) {
            let _ = app.run("type.char", json!({"attrs": {"capitalization": v}}));
        }
        let id = cid(ui, "story-editor");
        if button(ui, Some(sub(b, half + 10.0, 0.0, half, h)), id, &tr("Story Editor"), true).clicked() {
            let _ = app.run("app.storyEditor", json!({}));
        }
        ui.add_space(4.0);
        let b = block(ui, h);
        let id = cid(ui, "placeholder");
        if button(ui, Some(sub(b, 0.0, 0.0, full, h)), id, &tr("Fill with Placeholder Text"), true).clicked() {
            let _ = app.run("type.fillWithPlaceholder", json!({}));
        }
    }

    pub(in crate::panels::properties) fn text_frame_section(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let u = units(app);
        let Some((fid, opts, shaped)) = text_frame_target(app) else { return };
        let b = block(ui, FIELD_H);
        let id = cid(ui, "columns");
        let label = crate::i18n::tr(&lang, "Number of Columns");
        let n = Number::new(label, Some(opts.columns as f64), Quantity::Plain { decimals: 0 }).range(1.0, 40.0);
        if let Some(Some(v)) = icon_number(ui, sub(b, 0.0, 1.0, 19.0, 19.0), sub(b, 22.0, 0.0, 54.0, FIELD_H), id, "text-columns", n) {
            let _ = app.run("object.textFrameOptions", json!({"columns": v.max(1.0) as u64}));
        }
        let id = cid(ui, "gutter");
        let label = crate::i18n::tr(&lang, "Gutter");
        let n = Number::new(label, Some(opts.gutter), Quantity::Length(u)).range(0.0, 1440.0);
        if let Some(Some(v)) = icon_number(ui, sub(b, 83.0, 1.0, 19.0, 19.0), sub(b, 105.0, 0.0, 54.0, FIELD_H), id, "text-gutter", n) {
            let _ = app.run("object.textFrameOptions", json!({"gutter": v}));
        }
        // From x 164 to the row's end, or as wide as its whole label takes when that is wider (moving left over the gap).
        let options = crate::i18n::tr(&lang, "Options");
        let ow = ((b.max.x - 8.0) - (b.min.x + 164.0)).max(button_width(ui, options));
        let ox = (b.width() - 8.0 - ow).min(164.0);
        let id = cid(ui, "text-frame-options");
        if button(ui, Some(sub(b, ox, 0.5, ow, 20.0)), id, options, true).clicked() {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
        }
        // Type on a path sets one line along the path: insets and vertical justification do not apply.
        if opts.path.is_some() {
            return;
        }
        text_frame_insets(app, ui, fid, &opts, shaped);
        ui.add_space(4.0);
        let cur = opts.vertical_justification;
        let b = block(ui, 21.0);
        for (k, (vj, artwork, tip)) in VERTICAL_JUSTIFICATIONS.into_iter().enumerate() {
            let (at, id) = (sub(b, k as f32 * 27.0, 0.0, 23.0, 21.0), cid(ui, ("vjust", k)));
            if icon_toggle(ui, at, id, artwork, crate::i18n::tr(&lang, tip), cur == vj) {
                let _ = app.run("object.textFrameOptions", json!({"verticalJustification": vj}));
            }
        }
    }

    /// Inset Spacing under the columns, as the native body lays it out.
    fn text_frame_insets(app: &mut DesignApp, ui: &mut Ui, fid: designcraft_doc::ItemId, opts: &designcraft_doc::TextFrameOptions, shaped: bool) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let u = units(app);
        let inset = opts.inset;
        ui.add_space(4.0);
        if shaped {
            let b = block(ui, FIELD_H);
            let id = cid(ui, "inset");
            let label = tr("Inset");
            let n = Number::new(&label, Some(inset[0]), Quantity::Length(u)).range(0.0, 15552.0);
            if let Some(Some(v)) = icon_number(ui, sub(b, 0.0, 2.0, 17.0, 17.0), sub(b, 20.0, 0.0, 76.0, FIELD_H), id, "margin-top", n) {
                let _ = app.run("object.textFrameOptions", json!({"inset": shaped_inset(app, v)}));
            }
            return;
        }
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        let (link_id, same) = inset_link(app, fid, &inset);
        let linked = ui.data(|d| d.get_temp::<bool>(link_id)).unwrap_or(same);
        let mut set = None;
        for (k, key, artwork, tip, x, y) in INSET_FIELDS {
            let id = cid(ui, key);
            let label = tr(tip);
            let n = Number::new(&label, Some(inset[k]), Quantity::Length(u)).range(0.0, 15552.0);
            if let Some(Some(v)) = icon_number(ui, sub(b, x, y + 2.0, 17.0, 17.0), sub(b, x + 20.0, y, 76.0, FIELD_H), id, artwork, n) {
                set = Some(inset_set(linked, k, v));
            }
        }
        let tip = tr("Make all settings the same");
        let id = cid(ui, "inset-link");
        let toggled = link_toggle(tip.clone(), Bind::given(linked))
            .id(id)
            .tooltip(tip)
            .show_at(ui, sub(b, 106.0, (FIELD_H + 5.0) / 2.0, 22.0, FIELD_H))
            .events
            .committed()
            .copied();
        if let Some(on) = toggled {
            ui.data_mut(|d| d.insert_temp(link_id, on));
        }
        if let Some(o) = set {
            let _ = app.run("object.textFrameOptions", json!({"inset": o}));
        }
    }

    /// The text's language select at `at` (or in the flow): picking one sets the language of the selected text.
    pub(crate) fn language_select(app: &mut DesignApp, ui: &mut Ui, at: Option<Rect>, id: egui::Id, current: &str) {
        let lang = app.ui.language.clone();
        let choices: Vec<(&'static str, String)> = LANGUAGES.iter().map(|l| (*l, crate::i18n::tr(&lang, l).to_owned())).collect();
        let held = LANGUAGES.iter().copied().find(|l| *l == current);
        if let Some(l) = choice(ui, at, id, crate::i18n::tr(&lang, "Language"), held, &choices, Some(crate::i18n::tr(&lang, current))) {
            let _ = app.run("type.char", json!({"attrs": {"language": l}}));
        }
    }

    /// The kerning words a kerning field offers beside a number.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Kern {
        /// The font's own pairs.
        Metrics,
        /// Kerning from the glyph shapes.
        Optical,
        /// No kerning.
        Zero,
    }

    impl NumberName for Kern {
        fn id(self) -> u32 {
            match self {
                Kern::Metrics => 1,
                Kern::Optical => 2,
                Kern::Zero => 3,
            }
        }

        fn from_id(id: u32) -> Option<Self> {
            match id {
                1 => Some(Kern::Metrics),
                2 => Some(Kern::Optical),
                3 => Some(Kern::Zero),
                _ => None,
            }
        }
    }

    /// Kerning for the text selection, or at a caret for the pair at the caret, at `at` (or in the flow): Metrics,
    /// Optical or 0 from its presets list, or a manual value in 1/1000 em typed or stepped.
    pub(crate) fn kerning_field(app: &mut DesignApp, ui: &mut Ui, at: Option<Rect>, id: egui::Id, k: &Value) {
        let lang = app.ui.language.clone();
        let kind = k["kind"].as_str().unwrap_or("metrics");
        let value = match (kind, k["value"].as_f64().filter(|v| v.is_finite())) {
            ("optical", _) => NamedOr::Named(Kern::Optical),
            ("none", _) => NamedOr::Named(Kern::Zero),
            ("manual", Some(v)) => NamedOr::Number(v),
            _ => NamedOr::Named(Kern::Metrics),
        };
        let rows = [
            Entry::new(NumberPick::Choice(Kern::Metrics.id()), crate::i18n::tr(&lang, "Metrics").to_owned()),
            Entry::new(NumberPick::Choice(Kern::Optical.id()), crate::i18n::tr(&lang, "Optical").to_owned()),
            Entry::new(NumberPick::Choice(Kern::Zero.id()), "0".to_owned()),
        ];
        let field = NumberField::named(crate::i18n::tr(&lang, "Kerning").to_owned(), Bind::given(value))
            .id(id)
            .presets(&rows)
            .step(5.0)
            .commit_decimals(0)
            .range(-1000.0..=10000.0);
        let out = match at {
            Some(at) => field.show_at(ui, at),
            None => field.show(ui),
        };
        let picked = out.edit.events.committed().copied().or_else(|| match out.preset {
            Some(NumberPick::Choice(id)) => Kern::from_id(id).map(NamedOr::Named),
            _ => None,
        });
        let kerning = match picked {
            Some(NamedOr::Named(Kern::Metrics)) => json!({"kind": "metrics"}),
            Some(NamedOr::Named(Kern::Optical)) => json!({"kind": "optical"}),
            Some(NamedOr::Named(Kern::Zero)) => json!({"kind": "none"}),
            Some(NamedOr::Number(v)) if v.is_finite() => json!({"kind": "manual", "value": v}),
            _ => return,
        };
        let _ = app.run("type.kernPair", json!({"kerning": kerning}));
    }
}

/// The text sections and Frame Fitting on the framework, in the real Properties panel of a real document, driven with
/// real pointer and key events: each control runs its engine command once, as one undo step.
#[cfg(test)]
mod framework_tests {
    use super::super::framework_test_kit::*;
    use egui::accesskit::Role;
    use serde_json::json;

    fn properties(app: &mut crate::DesignApp, ui: &mut egui::Ui) {
        crate::panels::properties::show(app, ui);
    }

    #[test]
    fn a_typed_type_size_is_one_type_char_step() {
        let mut app = document();
        text_frame(&mut app, "Hello");
        let mut h = open(app, properties);
        let steps = undo_steps(&h);
        type_into(&mut h, "Font Size", "24");
        assert_eq!(journal(&h, "type.char").last(), Some(&json!({"attrs": {"size": 24.0}})));
        assert_eq!(text_attrs(&mut h)["chars"]["size"].as_f64(), Some(24.0));
        assert_eq!(undo_steps(&h), steps + 1, "one undo step");
    }

    #[test]
    fn a_typed_kerning_sets_a_manual_pair_kerning() {
        let mut app = document();
        text_frame(&mut app, "AV");
        let mut h = open(app, properties);
        type_into(&mut h, "Kerning", "25");
        assert_eq!(journal(&h, "type.kernPair").last(), Some(&json!({"kerning": {"kind": "manual", "value": 25.0}})));
    }

    #[test]
    fn the_alignment_buttons_and_stroke_behind_fill_run_their_commands() {
        let mut app = document();
        let (_, story) = text_frame(&mut app, "Hello");
        // Text Appearance is the text selection's section.
        select_text(&mut app, story, 5);
        let mut h = open(app, properties);
        let center = designcraft_doc::Align::Center.label();
        click(&mut h, Role::Button, center);
        assert_eq!(text_attrs(&mut h)["para"]["align"], json!(designcraft_doc::Align::Center));
        let steps = undo_steps(&h);
        click(&mut h, Role::CheckBox, "Stroke Behind Fill");
        assert_eq!(journal(&h, "type.char").last(), Some(&json!({"attrs": {"strokeBehindFill": true}})));
        assert_eq!(undo_steps(&h), steps + 1);
    }

    #[test]
    fn the_bulleted_list_button_turns_bullets_on() {
        let mut app = document();
        let (_, story) = text_frame(&mut app, "One");
        // Bullets and Numbering is the text selection's section.
        select_text(&mut app, story, 3);
        let mut h = open(app, properties);
        click(&mut h, Role::Button, "Bulleted List");
        assert_eq!(journal(&h, "list.bullets").last(), Some(&json!({"on": true})));
        assert!(h.state().session.selection_has_list(designcraft_doc::ListType::Bullets));
    }

    #[test]
    fn the_text_frame_columns_field_sets_the_columns() {
        let mut app = document();
        let (id, story) = text_frame(&mut app, "Columns");
        // With the text selected the Text Frame section is near the top of the panel, not under Transform, Align and
        // Flex Layout.
        select_text(&mut app, story, 3);
        let mut h = open(app, properties);
        type_into(&mut h, "Number of Columns", "3");
        assert_eq!(journal(&h, "object.textFrameOptions").last(), Some(&json!({"columns": 3})));
        assert_eq!(item(&h, id).text_frame().expect("a text frame").options.columns, 3);
    }

    #[test]
    fn frame_fitting_buttons_and_auto_fit_run_their_commands() {
        let mut app = document();
        let id = graphic_frame(&mut app);
        let mut h = open(app, properties);
        let steps = undo_steps(&h);
        click(&mut h, Role::Button, "Fit Content Proportionally");
        assert_eq!(journal(&h, "object.fit").last(), Some(&json!({"mode": "fitProportionally"})));
        click(&mut h, Role::CheckBox, "Auto-Fit");
        assert_eq!(journal(&h, "object.fittingOptions").last(), Some(&json!({"autoFit": true})));
        assert!(item(&h, id).graphic().is_some_and(|g| g.auto_fit != designcraft_doc::Fitting::None));
        assert!(undo_steps(&h) >= steps + 1, "the edits are undoable");
        assert!(has(&h, Role::Button, "Options"), "Frame Fitting Options… is beside the buttons");
    }
}
