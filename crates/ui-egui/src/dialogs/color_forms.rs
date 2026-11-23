//! Framework forms of the colour dialogs: the Color Picker, Swatch Options, Color Group Options, Layer Options and
//! Color Theme.
//!
//! Each form edits the same `Dialog` draft its native body edits, and OK and Cancel go through the host's `confirm` and
//! `cancel`, so the commands, the document and its undo steps are the native windows'. The Color Picker is the
//! framework's own `ColorPicker` in its Full layout (InDesign's and Photoshop's picker), with CMYK through the
//! document's working space. The native bodies stay in `dialogs.rs` and `options.rs` for the browser build.

use super::framework::Form;
use super::options::{dialog_color, set_color};
use super::{PickComp, Picked, nums, picked_of, store_picked};
use crate::DesignApp;
use crate::dialogs::Dialog;
use crate::effects_dialog::form::{check, choose, edited};
use craft_behavior::{ColumnWidth, Mixed};
use craft_color_ui::AreaMode;
use craft_color_ui::ui::{ColorPicker, ColorPickerLayout, ColorPopover, PickedColor, ScreenPickGesture};
use craft_theme::{Channel, ColorModel, DefaultOn, HsbModel, LabModel, MAX_CHANNELS, ModelValues, RgbModel, Srgba};
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::Swatch;
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::{Field, SliderField, SliderFieldLayout};
use craft_ui_widgets::layout_components::{Grid, Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::runtime::env::UnitKind;
use craft_ui_widgets::text::{NumberField, TextField};
use designcraft_color::Color;
use egui::Ui;
use serde_json::{Value, json};

/// The Color Picker: the area and strip, the new colour over the one it replaces, every model's channels, the hex
/// field and the eyedropper; Add CMYK Swatch under them.
pub(crate) const COLOR_PICKER: Form = Form::new(color_picker).width(580.0);
/// Swatch Options: the swatch's name and what its kind lets the user change.
pub(crate) const SWATCH_OPTIONS: Form = Form::new(swatch_options);
/// Color Group Options: the group's name.
pub(crate) const COLOR_GROUP_OPTIONS: Form = Form::new(color_group_options);
/// Layer Options: the layer's name, colour and flags.
pub(crate) const LAYER_SETTINGS: Form = Form::new(layer_settings);
/// Color Theme: the theme's name and its colours, added to the swatches on OK.
pub(crate) const COLOR_THEME: Form = Form::new(color_theme);

/// `s` in the interface language.
fn tr(app: &DesignApp, s: &str) -> String {
    crate::i18n::tr(&app.ui.language, s).to_owned()
}

/// A text field over the string draft field `key`, named `label`.
fn text_field(ui: &mut Ui, d: &mut Dialog, key: &str, label: String, enabled: bool) {
    let text = d.s(key);
    let out = TextField::new(label, Bind::given(text.clone())).id_salt(("color_text", key.to_owned())).limit(None::<usize>).enabled(enabled).show(ui);
    if let Some(next) = edited(out.events, text) {
        d.fields.insert(key.into(), Value::String(next));
    }
}

/// A check box over the bool draft field `key`.
fn flag(ui: &mut Ui, d: &mut Dialog, key: &str, label: String) {
    if let Some(on) = check(ui, ("color_check", key.to_owned()), &label, d.b(key)) {
        d.fields.insert(key.into(), json!(on));
    }
}

/// Why OK was refused, until the next try.
fn status(ui: &mut Ui, d: &Dialog) {
    let s = d.s("status");
    if !s.is_empty() {
        Text::new(s).show(ui);
    }
}

/// An encoded sRGB colour from 0..1 channels.
fn srgb(rgb: [f32; 3]) -> Srgba {
    Srgba::rgb(rgb[0], rgb[1], rgb[2])
}

/// The document's CMYK as the Color Picker shows it: inks through the working CMYK space with the working intent,
/// as the native picker's fields and its OK convert them, so the inks typed are the inks applied.
struct WorkingCmyk;

const CMYK_CHANNELS: [Channel; 4] = [
    Channel { name: "Cyan", qualified: "CMYK cyan", label: "C", min: 0.0, max: 100.0, unit: "%", decimals: 0, step: 1.0, track_stops: 9 },
    Channel { name: "Magenta", qualified: "CMYK magenta", label: "M", min: 0.0, max: 100.0, unit: "%", decimals: 0, step: 1.0, track_stops: 9 },
    Channel { name: "Yellow", qualified: "CMYK yellow", label: "Y", min: 0.0, max: 100.0, unit: "%", decimals: 0, step: 1.0, track_stops: 9 },
    Channel { name: "Black", qualified: "CMYK black", label: "K", min: 0.0, max: 100.0, unit: "%", decimals: 0, step: 1.0, track_stops: 9 },
];

impl ColorModel for WorkingCmyk {
    fn id(&self) -> &'static str {
        "cmyk"
    }

    fn name(&self) -> &'static str {
        "CMYK"
    }

    fn channels(&self) -> &[Channel] {
        &CMYK_CHANNELS
    }

    fn from_srgb(&self, color: Srgba) -> ModelValues {
        ModelValues::new(self.id(), &working_cmyk([color.r, color.g, color.b]))
    }

    fn to_srgb(&self, values: &[f32; MAX_CHANNELS]) -> Srgba {
        let [c, m, y, k] = [values[0], values[1], values[2], values[3]].map(|v| v / 100.0);
        srgb(Color::Cmyk { c, m, y, k }.to_rgb().map(|v| v.clamp(0.0, 1.0)))
    }

    fn default_on(&self) -> DefaultOn {
        DefaultOn { full: false, compact: false }
    }
}

/// The inks, in percent, of the display colour `rgb` in the working CMYK space.
fn working_cmyk(rgb: [f32; 3]) -> [f32; 4] {
    Color::Rgb { r: rgb[0], g: rgb[1], b: rgb[2] }.to_cmyk_managed(designcraft_color::cms::active_settings().intent).map(|v| v * 100.0)
}

/// What the area shows for the picker's component key, and back.
const AREA_MODES: [(&str, AreaMode); 9] = [
    ("h", AreaMode::Hue),
    ("s", AreaMode::Saturation),
    ("b", AreaMode::Brightness),
    ("r", AreaMode::Red),
    ("g", AreaMode::Green),
    ("bl", AreaMode::Blue),
    ("l", AreaMode::LabL),
    ("la", AreaMode::LabA),
    ("lb", AreaMode::LabB),
];

/// The model a kept value names, as the picker's models spell it.
fn model_id(id: &str) -> Option<&'static str> {
    ["hsb", "rgb", "lab", "cmyk"].into_iter().find(|m| *m == id)
}

/// The values the picker keeps with the draft's colour: those of the model the user last edited it in (`_kept`), else
/// the CMYK inks of a colour opened or typed as CMYK.
fn kept(d: &Dialog) -> Option<ModelValues> {
    if let Some(k) = d.fields.get("_kept")
        && let Some(model) = k.get("model").and_then(Value::as_str).and_then(model_id)
    {
        let values: Vec<f32> = k.get("values").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_f64).map(|v| v as f32).collect();
        return Some(ModelValues::new(model, &values));
    }
    (d.s("model") == "cmyk").then(|| nums::<4>(d, "cmyk")).flatten().map(|c| ModelValues::new("cmyk", &c.map(|v| v as f32)))
}

/// The draft after the picker set `color`: its colour in every space, as the native picker stores it, and the values of
/// the model it was edited in. Red, green or blue makes the colour RGB and typed inks make it CMYK, as the native fields
/// do.
fn take(d: &mut Dialog, color: PickedColor, hue: f32) {
    let p = Picked::from_rgb([color.color.r, color.color.g, color.color.b], hue);
    store_picked(d, &p);
    let Some(values) = color.model else {
        d.fields.remove("_kept");
        return;
    };
    let v = values.values;
    match values.model {
        "cmyk" => {
            d.fields.insert("cmyk".into(), json!(v));
            d.fields.insert("model".into(), json!("cmyk"));
        }
        "rgb" => {
            d.fields.insert("rgb".into(), json!([v[0], v[1], v[2]]));
            d.fields.insert("model".into(), json!("rgb"));
        }
        "hsb" => {
            d.fields.insert("_hsb".into(), json!([v[0], v[1] / 100.0, v[2] / 100.0]));
        }
        "lab" => {
            d.fields.insert("_lab".into(), json!([v[0], v[1], v[2]]));
        }
        _ => {}
    }
    d.fields.insert("_kept".into(), json!({"model": values.model, "values": v}));
}

/// The Color Picker. Its colour is the draft's (`hex`, the kept HSB and Lab values, the inks); the area shows the
/// component last chosen in any picker. The eyedropper, dragged onto the page and let go there, takes the colour the
/// page shows under it.
fn color_picker(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let old = d.s("hex");
    d.fields.entry("old").or_insert_with(|| json!(old));
    // The component the picker was last left on, unless the opener chose one.
    d.fields.entry("component").or_insert_with(|| json!(app.ui.color_picker_component));
    let p = picked_of(d);
    d.fields.entry("rgb").or_insert_with(|| json!(p.rgb.map(|x| x * 255.0)));
    if !d.fields.contains_key("cmyk") {
        d.fields.insert("cmyk".into(), json!(working_cmyk(p.rgb)));
    }
    let shown = match kept(d) {
        Some(values) => PickedColor::in_model(srgb(p.rgb), values),
        None => PickedColor::rgb(srgb(p.rgb)),
    };
    let component = PickComp::from_key(&d.s("component")).key();
    let before = AREA_MODES.iter().find(|(key, _)| *key == component).map_or(AreaMode::Red, |(_, mode)| *mode);
    let mut mode = before;
    let replaced = Color::from_hex(&d.s("old")).map_or([0.0; 3], |c| c.to_rgb_uncalibrated());
    let models: [&dyn ColorModel; 4] = [&HsbModel, &RgbModel, &LabModel, &WorkingCmyk];
    let out = ColorPicker::new(tr(app, "Color Picker"), Bind::given(shown))
        .layout(ColorPickerLayout::Full)
        .models(models)
        .mode(Bind::bound(&mut mode))
        .compare(Mixed::Same(PickedColor::rgb(srgb(replaced))))
        .eyedropper(true)
        .show(ui);
    if let Some(color) = edited(out.events, shown) {
        take(d, color, p.hsb[0]);
    }
    // The eyedropper takes nothing let go over the picker itself, though a page may lie under it.
    if let Some(ScreenPickGesture::Released { at, .. }) = out.screen_pick
        && !out.response.rect.contains(at)
        && let Some(rgb) = crate::canvas::color_at(app, ui.ctx(), at)
    {
        let hue = picked_of(d).hsb[0];
        take(d, PickedColor::rgb(srgb(rgb.map(|v| f32::from(v) / 255.0))), hue);
    }
    if mode != before
        && let Some((key, _)) = AREA_MODES.iter().find(|(_, m)| *m == mode)
    {
        d.fields.insert("component".into(), json!(key));
        app.ui.color_picker_component = (*key).to_owned();
    }
    if Button::new(tr(app, "Add CMYK Swatch")).id_salt("color_picker_add_swatch").show(ui).clicked() {
        // The inks the fields show: as typed or given, else the colour converted.
        let inks = nums::<4>(d, "cmyk").map_or_else(|| working_cmyk(picked_of(d).rgb).map(f64::from), |c| c);
        let [c, m, y, k] = inks.map(f64::round);
        let added = tr(app, "Added swatch");
        match app.run("swatch.create", json!({"color": {"c": c, "m": m, "y": y, "k": k}})) {
            Ok(r) => app.status(format!("{added} {}", r["name"].as_str().unwrap_or(""))),
            Err(e) => app.status(e),
        }
    }
    None
}

/// A colour channel's number field over the draft field `key`, named `label`, from 0 to `max`.
fn channel(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, max: f64, percent: bool) {
    let value = d.n(key).unwrap_or(0.0);
    Field::new(label.to_owned()).id_salt(("swatch_channel_field", key.to_owned())).show(ui, |ui, _| {
        let field =
            NumberField::new(label.to_owned(), Bind::given(Some(value))).id_salt(("swatch_channel", key.to_owned())).range(0.0..=max).step(1.0);
        let field = if percent { field.unit_kind(UnitKind::Percent) } else { field };
        let out = field.commit_decimals(1).show(ui);
        if let Some(Some(v)) = edited(out.edit.events, Some(value)).filter(|v| v.is_some_and(f64::is_finite)) {
            d.fields.insert(key.into(), json!(v));
        }
    });
}

/// Swatch Options: the name (named with its value, or fixed for a system swatch), and the colour type, mode and
/// channels of a colour, the tint of a tint.
fn swatch_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let kind = d.s("kind");
    let auto = kind == "color" && d.b("nameWithValue");
    if auto && let Some(c) = dialog_color(&d.fields) {
        d.fields.insert("swatchName".into(), json!(designcraft_color::swatch::value_name(c)));
    }
    Stack::new().id_salt("swatch_options").fill_width().show(ui, |ui, _| {
        let editable = !d.b("locked") && !auto;
        Field::new(tr(app, "Swatch Name:")).id_salt("swatch_name_field").show(ui, |ui, _| {
            text_field(ui, d, "swatchName", tr(app, "Swatch Name"), editable);
        });
        if kind == "color" {
            flag(ui, d, "nameWithValue", tr(app, "Name with Color Value"));
        }
        match kind.as_str() {
            "color" | "paper" => {
                if kind == "color" {
                    Field::new(tr(app, "Color Type:")).id_salt("swatch_color_type_field").show(ui, |ui, _| {
                        let entries = [Entry::new("process".to_owned(), tr(app, "Process")), Entry::new("spot".to_owned(), tr(app, "Spot"))];
                        let cur = if d.s("colorType") == "spot" { "spot" } else { "process" }.to_owned();
                        if let Some(t) = choose(ui, "swatch_color_type", &tr(app, "Color Type"), cur, &entries) {
                            d.fields.insert("colorType".into(), json!(t));
                        }
                    });
                }
                Field::new(tr(app, "Color Mode:")).id_salt("swatch_color_mode_field").show(ui, |ui, _| {
                    let entries = [("cmyk", "CMYK"), ("rgb", "RGB"), ("gray", "Gray")].map(|(m, l)| Entry::new(m.to_owned(), tr(app, l)));
                    let cur = match d.s("mode").as_str() {
                        m @ ("rgb" | "gray") => m.to_owned(),
                        _ => "cmyk".to_owned(),
                    };
                    if let Some(m) = choose(ui, "swatch_color_mode", &tr(app, "Color Mode"), cur, &entries)
                        && let Some(c) = dialog_color(&d.fields)
                    {
                        set_color(&mut d.fields, c, Some(&m));
                    }
                });
                match d.s("mode").as_str() {
                    "rgb" => ["r", "g", "b"].into_iter().for_each(|k| channel(ui, d, k, &k.to_uppercase(), 255.0, false)),
                    "gray" => channel(ui, d, "gray", "K", 100.0, true),
                    _ => ["c", "m", "y", "k"].into_iter().for_each(|k| channel(ui, d, k, &k.to_uppercase(), 100.0, true)),
                }
                if let Some(c) = dialog_color(&d.fields) {
                    Swatch::new(tr(app, "Color"), srgb(c.to_rgb())).id_salt("swatch_options_color").show(ui);
                }
            }
            "tint" => {
                Field::new(tr(app, "Base Color:")).id_salt("swatch_base_field").value(d.s("base")).show(ui, |ui, _| {
                    Text::new(d.s("base")).show(ui);
                });
                Field::new(tr(app, "Tint:")).id_salt("swatch_tint_field").show(ui, |ui, _| {
                    let tint = d.n("tint").unwrap_or(100.0);
                    let out = SliderField::new(tr(app, "Tint"), Bind::given(tint), 0.0..=100.0)
                        .id_salt("swatch_tint")
                        .layout(SliderFieldLayout::Inline)
                        .unit_kind(UnitKind::Percent)
                        .step(1.0)
                        .show(ui);
                    if let Some(v) = edited(out.edit.events, tint) {
                        d.fields.insert("tint".into(), json!(v.round()));
                    }
                });
            }
            "gradient" => {
                Text::new(tr(app, "Edit the gradient's stops in the Gradient panel.")).show(ui);
            }
            _ => {
                Text::new(tr(app, "This swatch can't be edited.")).show(ui);
            }
        }
        status(ui, d);
    });
    None
}

/// Color Group Options: the group's new name.
fn color_group_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    Stack::new().id_salt("color_group_options").fill_width().show(ui, |ui, _| {
        Field::new(tr(app, "Name:")).id_salt("color_group_name_field").show(ui, |ui, _| text_field(ui, d, "groupName", tr(app, "Name"), true));
        status(ui, d);
    });
    None
}

/// The layer's flags as InDesign lays them out: the layer's own on the left, its guides' on the right.
const FLAG_COLUMNS: [ColumnWidth; 2] = [ColumnWidth::Content, ColumnWidth::Content];

/// Layer Options: name, colour (InDesign's named layer colours, or any colour from the well's picker) and the flags.
fn layer_settings(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let rgb: [u8; 3] = d.fields.get("rgb").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or([0, 0, 0]);
    let named = designcraft_doc::LAYER_COLORS.iter().find(|(_, c)| *c == rgb).map(|(n, _)| (*n).to_owned());
    let well = srgb(rgb.map(|v| f32::from(v) / 255.0));
    Stack::new().id_salt("layer_options").fill_width().show(ui, |ui, _| {
        Field::new(tr(app, "Name:")).id_salt("layer_name_field").show(ui, |ui, _| text_field(ui, d, "name", tr(app, "Name"), true));
        Field::new(tr(app, "Color:")).id_salt("layer_color_field").show(ui, |ui, _| {
            Row::new().id_salt("layer_color_row").show(ui, |ui, _| {
                let entries: Vec<Entry<String>> = designcraft_doc::LAYER_COLORS
                    .iter()
                    .map(|(n, c)| Entry::new((*n).to_owned(), tr(app, n)).swatch(srgb(c.map(|v| f32::from(v) / 255.0))))
                    .collect();
                let out = Select::new(tr(app, "Color"), Bind::given(named.clone().unwrap_or_default()), &entries)
                    .id_salt("layer_color")
                    .placeholder(tr(app, "Custom"))
                    .show(ui);
                if let Some(name) = edited(out.events, named.clone().unwrap_or_default())
                    && let Some((_, c)) = designcraft_doc::LAYER_COLORS.iter().find(|(n, _)| *n == name)
                {
                    d.fields.insert("rgb".into(), json!(c));
                }
                let picked = PickedColor::rgb(well);
                let out = ColorPopover::new(tr(app, "Custom Color"), Bind::given(picked)).show(ui);
                if let Some(c) = edited(out.events, picked) {
                    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    d.fields.insert("rgb".into(), json!([q(c.color.r), q(c.color.g), q(c.color.b)]));
                }
            });
        });
        Grid::new(&FLAG_COLUMNS).id_salt("layer_flags").show(ui, |ui, _| {
            flag(ui, d, "visible", tr(app, "Show Layer"));
            flag(ui, d, "showGuides", tr(app, "Show Guides"));
            flag(ui, d, "locked", tr(app, "Lock Layer"));
            flag(ui, d, "lockGuides", tr(app, "Lock Guides"));
            flag(ui, d, "printable", tr(app, "Print Layer"));
        });
        flag(ui, d, "suppressWrapWhenHidden", tr(app, "Suppress Text Wrap When Layer is Hidden"));
        status(ui, d);
    });
    None
}

/// Color Theme: the theme's name and the colours it adds, each a well over its hex code.
fn color_theme(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let colors: Vec<String> =
        d.fields.get("colors").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).take(5).map(str::to_owned).collect();
    Stack::new().id_salt("color_theme").fill_width().show(ui, |ui, _| {
        Field::new(tr(app, "Name:")).id_salt("color_theme_name_field").show(ui, |ui, _| text_field(ui, d, "name", tr(app, "Name"), true));
        Row::new().id_salt("color_theme_colors").show(ui, |ui, _| {
            for (at, hex) in colors.iter().enumerate() {
                Stack::new().id_salt(("color_theme_color", at)).show(ui, |ui, _| {
                    if let Some(c) = Color::from_hex(hex) {
                        Swatch::new(hex.clone(), srgb(c.to_rgb_uncalibrated())).id_salt(("color_theme_well", at)).show(ui);
                    }
                    Text::new(hex.clone()).shade(Shade::Muted).show(ui);
                });
            }
        });
        status(ui, d);
    });
    None
}

#[cfg(test)]
mod tests {
    //! The colour dialogs in the framework's dialog host, driven with real pointer and key events: each form edits its
    //! draft only, and OK runs the command the native dialog ran, as one undo step.

    use super::*;
    use egui::accesskit::{Node, Role, TreeUpdate};
    use egui::{Event, Key, Modifiers, PointerButton};

    struct Window {
        app: DesignApp,
        ctx: egui::Context,
        ax: Option<TreeUpdate>,
        time: f64,
    }

    impl Window {
        fn new(mut app: DesignApp) -> Self {
            app.ui.language = "en".into();
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            app.install_framework(&ctx);
            app.raw_input_hook(&ctx, &mut egui::RawInput::default());
            assert!(app.framework_installed);
            let out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1100.0))),
                    max_texture_side: Some(8192),
                    ..Default::default()
                },
                |ui| {
                    crate::theme::install_fonts(ui.ctx(), "");
                },
            );
            out.drop_without_applying_deltas();
            let mut w = Window { app, ctx, ax: None, time: 1.0 };
            w.idle();
            w
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.05;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1100.0))),
                max_texture_side: Some(8192),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.app.raw_input_hook(&self.ctx, &mut input);
            let ctx = self.ctx.clone();
            let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(&mut self.app, ui.ctx()));
            self.ax = out.platform_output.accesskit_update.take();
            out.drop_without_applying_deltas();
        }

        fn idle(&mut self) {
            for _ in 0..8 {
                self.frame(Vec::new());
            }
        }

        fn key_with(&mut self, key: Key, modifiers: Modifiers) {
            for pressed in [true, false] {
                self.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }]);
            }
            self.frame(Vec::new());
        }

        /// The one accessibility node of `role` that `test` picks, and its bounds.
        fn node(&self, role: Role, what: &str, test: impl Fn(&Node) -> bool) -> (bool, egui::Rect) {
            let matches: Vec<_> = self.ax.as_ref().unwrap().nodes.iter().filter(|(_, n)| n.role() == role && test(n)).collect();
            assert_eq!(matches.len(), 1, "exactly one {role:?} {what}");
            let b = matches[0].1.bounds().unwrap();
            (matches[0].1.is_disabled(), egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32)))
        }

        fn named(&self, role: Role, name: &str) -> (bool, egui::Rect) {
            self.node(role, name, |n| n.label() == Some(name))
        }

        fn click_at(&mut self, at: egui::Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            for pressed in [true, false] {
                self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
            }
            self.idle();
        }

        fn click(&mut self, role: Role, name: &str) {
            let at = self.named(role, name).1.center();
            self.click_at(at);
        }

        /// Replaces the text of the field at `at` with `text` and leaves it with Tab, which commits it.
        fn retype_at(&mut self, at: egui::Pos2, text: &str) {
            self.click_at(at);
            self.key_with(Key::A, Modifiers::COMMAND);
            self.frame(vec![Event::Text(text.into())]);
            self.key_with(Key::Tab, Modifiers::NONE);
            self.idle();
        }

        /// Replaces the text of the text field showing `old`.
        fn retype(&mut self, old: &str, text: &str) {
            let at = self.node(Role::TextInput, old, |n| n.value() == Some(old)).1.center();
            self.retype_at(at, text);
        }

        fn draft(&self) -> &Dialog {
            self.app.ui.dialog.as_ref().expect("the dialog is open")
        }

        fn undo_steps(&self) -> usize {
            self.app.session.active().unwrap().history.undo.len()
        }
    }

    fn app() -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app
    }

    fn doc(w: &Window) -> &designcraft_doc::Document {
        &w.app.session.active().unwrap().doc
    }

    #[test]
    fn swatch_options_renames_a_swatch_and_its_objects_follow_in_one_step() {
        let mut app = app();
        app.run("swatch.create", json!({"name": "Brand", "color": {"c": 0, "m": 80, "y": 100, "k": 0}})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("object.fill", json!({"swatch": "Brand", "ids": [id]})).unwrap();
        crate::dialogs::open_swatch_options(&mut app, "Brand").unwrap();
        let mut w = Window::new(app);
        let steps = w.undo_steps();
        w.retype("Brand", "Signal Orange");
        assert_eq!(w.draft().s("swatchName"), "Signal Orange");
        assert!(doc(&w).swatch("Brand").is_some(), "the field edits the draft only");
        assert_eq!(w.undo_steps(), steps);
        w.click(Role::Button, "OK");
        assert!(w.app.ui.dialog.is_none());
        assert!(doc(&w).swatch("Signal Orange").is_some() && doc(&w).swatch("Brand").is_none());
        assert_eq!(doc(&w).item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, "Signal Orange");
        assert_eq!(w.undo_steps(), steps + 1, "one undo step");
        w.app.run("edit.undo", json!({})).unwrap();
        assert!(doc(&w).swatch("Brand").is_some());
    }

    #[test]
    fn a_system_swatchs_name_cant_be_edited_and_cancel_changes_nothing() {
        let mut app = app();
        crate::dialogs::open_swatch_options(&mut app, "[Black]").unwrap();
        let mut w = Window::new(app);
        let before = doc(&w).clone();
        let (disabled, _) = w.node(Role::TextInput, "[Black]", |n| n.value() == Some("[Black]"));
        assert!(disabled, "the name field is disabled");
        w.click(Role::Button, "Cancel");
        assert!(w.app.ui.dialog.is_none());
        assert_eq!(doc(&w), &before);
    }

    #[test]
    fn color_group_options_rename_the_group() {
        let mut app = app();
        app.run("swatch.create", json!({"name": "Brand", "color": {"c": 0, "m": 80, "y": 100, "k": 0}})).unwrap();
        app.run("swatch.newColorGroup", json!({"name": "Brand Colors", "swatches": ["Brand"]})).unwrap();
        crate::dialogs::open_color_group_options(&mut app, "Brand Colors").unwrap();
        let mut w = Window::new(app);
        let steps = w.undo_steps();
        w.retype("Brand Colors", "Identity");
        assert!(doc(&w).color_groups.iter().any(|g| g.name == "Brand Colors"), "the field edits the draft only");
        w.click(Role::Button, "OK");
        assert!(w.app.ui.dialog.is_none());
        let names: Vec<&str> = doc(&w).color_groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Identity"]);
        assert_eq!(w.undo_steps(), steps + 1);
    }

    #[test]
    fn layer_options_rename_the_layer_and_set_its_flags() {
        let mut app = app();
        app.run("layer.new", json!({"name": "Art"})).unwrap();
        let id = app.session.active().unwrap().doc.layers.iter().find(|l| l.name == "Art").unwrap().id.0;
        crate::dialogs::open_layer_options(&mut app, Some(id)).unwrap();
        let mut w = Window::new(app);
        let steps = w.undo_steps();
        let (lock, guides) = (w.named(Role::CheckBox, "Lock Layer").1, w.named(Role::CheckBox, "Lock Guides").1);
        assert!(guides.left() > lock.right() && (lock.center().y - guides.center().y).abs() < 2.0, "Lock Guides is beside Lock Layer");
        w.retype("Art", "Artwork");
        w.click(Role::CheckBox, "Print Layer");
        w.click(Role::CheckBox, "Lock Guides");
        assert!(!w.draft().b("printable") && w.draft().b("lockGuides"));
        assert!(doc(&w).layers.iter().any(|l| l.name == "Art" && l.printable), "the controls edit the draft only");
        w.click(Role::Button, "OK");
        assert!(w.app.ui.dialog.is_none());
        let layer = doc(&w).layers.iter().find(|l| l.name == "Artwork").expect("renamed");
        assert!(!layer.printable && layer.lock_guides && !layer.locked);
        assert_eq!(w.undo_steps(), steps + 1);
    }

    #[test]
    fn color_theme_adds_its_colours_under_the_name_given() {
        let mut app = app();
        app.ui.dialog = Some(Dialog::new("colorTheme", json!({"name": "Color Theme", "colors": ["#102030", "#a0b0c0"]})));
        let mut w = Window::new(app);
        w.retype("Color Theme", "Sunset");
        assert!(doc(&w).color_groups.iter().all(|g| g.name != "Sunset"), "the field edits the draft only");
        w.click(Role::Button, "Add to Swatches");
        assert!(w.app.ui.dialog.is_none());
        let group = doc(&w).color_groups.iter().find(|g| g.name == "Sunset").expect("the theme's group");
        assert_eq!(group.swatches.len(), 2);
    }

    /// The fill's colour as the page shows it, in 8-bit RGB.
    fn fill_rgb(w: &Window, id: u64) -> [u8; 3] {
        let item = doc(w).item(designcraft_doc::ItemId(id)).unwrap().clone();
        let [r, g, b, _] = doc(w).resolve_color(&item.fill.swatch, item.fill.tint).unwrap().to_rgba8(1.0);
        [r, g, b]
    }

    #[test]
    fn the_color_picker_edits_the_draft_applies_once_and_adds_a_cmyk_swatch() {
        let mut app = app();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
        app.run("object.color", json!({"color": Color::from_hex("#7c1315").unwrap(), "target": "fill"})).unwrap();
        app.ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": "#7c1315", "text": false})));
        let mut w = Window::new(app);
        let before = fill_rgb(&w, id);
        for name in ["Hue", "Saturation", "Brightness", "Red", "Green", "Blue", "Lightness", "Cyan", "Magenta", "Yellow", "Black"] {
            w.named(Role::SpinButton, name);
        }
        // Add CMYK Swatch makes a swatch of the inks shown and keeps the picker open.
        let swatches = doc(&w).swatches.len();
        w.click(Role::Button, "Add CMYK Swatch");
        assert!(w.app.ui.dialog.is_some());
        assert_eq!(doc(&w).swatches.len(), swatches + 1);
        let steps = w.undo_steps();
        // Red typed: the colour becomes RGB, in the draft alone.
        let red = w.named(Role::SpinButton, "Red").1.center();
        w.retype_at(red, "0");
        let d = w.draft();
        assert!(d.b("edited"));
        assert_eq!(d.s("model"), "rgb");
        assert_eq!(d.s("hex"), "#001315");
        assert_eq!(fill_rgb(&w, id), before, "the picker edits the draft only");
        assert_eq!(w.undo_steps(), steps);
        // OK: the fill takes the colour, as one undo step.
        w.click(Role::Button, "OK");
        assert!(w.app.ui.dialog.is_none());
        let [r, g, b] = fill_rgb(&w, id);
        assert!(r <= 1 && g.abs_diff(0x13) <= 1 && b.abs_diff(0x15) <= 1, "{:?}", [r, g, b]);
        assert_eq!(w.undo_steps(), steps + 1);
        // Typed inks stay as typed, and Cancel applies nothing.
        w.app.ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": "#001315", "text": false})));
        w.idle();
        let cyan = w.named(Role::SpinButton, "Cyan").1.center();
        w.retype_at(cyan, "50");
        let d = w.draft();
        assert_eq!((d.s("model"), d.fields["cmyk"][0].as_f64()), ("cmyk".to_owned(), Some(50.0)));
        let applied = fill_rgb(&w, id);
        w.click(Role::Button, "Cancel");
        assert!(w.app.ui.dialog.is_none());
        assert_eq!(fill_rgb(&w, id), applied);
        assert_eq!(w.undo_steps(), steps + 1);
    }
}
