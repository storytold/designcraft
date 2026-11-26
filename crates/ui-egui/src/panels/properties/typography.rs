use super::{Align, DesignApp, FIELD_H, Tokens, Ui, Value, block, full_width, json, sub, text_attrs, units};

pub fn character_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    framework::character_panel(app, ui)
}

/// Opens the Typography panel, or brings it to the front when it is already open.
pub fn show_typography_panel(app: &mut DesignApp) {
    let _ = app.run("window.panel", json!({"panel": "typography"}));
}

/// The paragraph alignments in InDesign's order, with their icons.
const PARA_ALIGNS: [(Align, &str); 9] = [
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

/// Space Between Paragraphs Using Same Style's presets (pt), after Ignore.
const SAME_STYLE_SPACING: &[f64] = &[0.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0];

/// The Paragraph panel's indent and spacing rows: icon, name and attribute, left and right.
const PARA_ROWS: [((&str, &str, &str), (&str, &str, &str)); 3] = [
    (("indent-left", "Left Indent", "leftIndent"), ("indent-right", "Right Indent", "rightIndent")),
    (("indent-first", "First Line Left Indent", "firstLineIndent"), ("indent-last", "Last Line Right Indent", "lastLineIndent")),
    (("space-before", "Space Before", "spaceBefore"), ("space-after", "Space After", "spaceAfter")),
];

/// Shading and Border: the on attribute, the colour attribute, the label, the dialog button's icon, the dialog tab and
/// the colour field's name.
const PARA_DECORATIONS: [(&str, &str, &str, &str, &str, &str); 2] = [
    ("shadingOn", "shadingColor", "Shading", "para-shading", "shading", "Paragraph Shading"),
    ("borderOn", "borderColor", "Border", "para-border", "border", "Paragraph Border"),
];

/// Do Not Align and Align to Baseline Grid: icon, name, whether it stands for aligned, and the `gridAlign` it sets.
const GRID_BUTTONS: [(&str, &str, bool, &str); 2] =
    [("grid-none", "Do Not Align to Baseline Grid", false, "none"), ("grid-baseline", "Align to Baseline Grid", true, "allLines")];

/// The Paragraph panel, laid out as InDesign's: the alignment buttons in groups of 3, 4 and 2;
/// indents, spacing and drop cap fields by their icons in two columns; Shading and Border with
/// their colours; Hyphenate and the baseline grid buttons. The rest (composer, grid and ragged
/// lines options, the paragraph dialogs) is in the panel menu.
pub fn paragraph_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    framework::paragraph_panel(app, ui)
}

/// What a Paragraph panel menu row does: set paragraph attributes, or run a command (which opens its dialog).
#[derive(Clone, Debug, PartialEq)]
enum ParagraphAction {
    /// `type.para` with these attributes.
    Attrs(Value),
    /// The menu command with this id.
    Command(&'static str),
    /// Nothing (a row the app does not do yet).
    Nothing,
}

/// A Paragraph panel menu row: its label, check, whether it can run, its shortcut text and what it does.
struct ParagraphRow {
    label: &'static str,
    checked: Option<bool>,
    enabled: bool,
    shortcut: Option<String>,
    action: ParagraphAction,
}

/// The Paragraph panel menu, in the order of InDesign's, in groups set off by separators.
fn paragraph_menu_rows(app: &DesignApp, p: &Value) -> Vec<Vec<ParagraphRow>> {
    let row = |label, checked, enabled, action| ParagraphRow { label, checked, enabled, shortcut: None, action };
    let (single, [paragraph, single_line]) = composer_choice(p["composer"].as_str());
    let composers = vec![
        row("Paragraph Composer", Some(!single), true, ParagraphAction::Attrs(json!({"composer": paragraph}))),
        row("Single-line Composer", Some(single), true, ParagraphAction::Attrs(json!({"composer": single_line}))),
    ];
    let grid = p["gridAlign"].as_str().unwrap_or("none");
    let balance = p["balanceRagged"].as_bool().unwrap_or(false);
    // Story-wide Optical Margin Alignment is in the Story panel.
    let ignore = p["ignoreOpticalMargin"].as_bool().unwrap_or(false);
    let options = vec![
        row(
            "Only Align First Line to Grid",
            Some(grid == "firstLineOnly"),
            grid != "none",
            ParagraphAction::Attrs(json!({"gridAlign": if grid == "firstLineOnly" { "allLines" } else { "firstLineOnly" }})),
        ),
        row("Balance Ragged Lines", Some(balance), true, ParagraphAction::Attrs(json!({"balanceRagged": !balance}))),
        row("Ignore Optical Margin", Some(ignore), true, ParagraphAction::Attrs(json!({"ignoreOpticalMargin": !ignore}))),
    ];
    let dialogs = [
        ("Justification…", "app.justificationDialog"),
        ("Keep Options…", "app.keepOptionsDialog"),
        ("Span Columns…", "app.spanColumnsDialog"),
        ("Hyphenation…", "app.hyphenationDialog"),
        ("Drop Caps and Nested Styles…", "app.nestedStylesDialog"),
        ("GREP Styles…", "app.grepStylesDialog"),
        ("Paragraph Rules…", "app.paragraphRulesDialog"),
        ("Paragraph Borders and Shading…", "app.paragraphBordersDialog"),
    ]
    .into_iter()
    .map(|(label, id)| ParagraphRow {
        label,
        checked: None,
        enabled: true,
        shortcut: crate::menus::shortcut_of(app, id).map(|sc| crate::menus::shortcut_text(&sc)),
        action: ParagraphAction::Command(id),
    })
    .collect();
    // Restart Numbering at 1, or Continue Numbering from the paragraph before, for a numbered list.
    let numbered = p["listType"].as_str() == Some("numbers");
    let restarts = p["startAt"].is_u64();
    let restart = match (numbered, restarts) {
        (false, _) => "Restart/Continue Numbering",
        (true, false) => "Restart Numbering",
        (true, true) => "Continue Numbering",
    };
    let lists = vec![
        row("Bullets and Numbering…", None, true, ParagraphAction::Command("app.bulletsNumberingDialog")),
        row(restart, None, numbered, ParagraphAction::Attrs(json!({"startAt": if restarts { Value::Null } else { json!(1) }}))),
        row("Convert Bullets and Numbering to Text", None, false, ParagraphAction::Nothing),
        row("Define Lists…", None, true, ParagraphAction::Command("list.define")),
    ];
    // Paragraph Direction (World-Ready): switching also mirrors left and right alignment.
    let rtl = p["direction"] == "rightToLeft";
    let mirrored = match serde_json::from_value(p["align"].clone()).unwrap_or_default() {
        Align::Left => Align::Right,
        Align::Right => Align::Left,
        a => a,
    };
    let direction = [(!rtl, "Left to Right", "leftToRight"), (rtl, "Right to Left", "rightToLeft")]
        .into_iter()
        .map(|(on, label, dir)| {
            let action = if on { ParagraphAction::Nothing } else { ParagraphAction::Attrs(json!({"direction": dir, "align": mirrored})) };
            row(label, Some(on), true, action)
        })
        .collect();
    vec![composers, options, dialogs, lists, direction]
}

/// Runs what a Paragraph panel menu row does; as a menu choice, a command opens its dialog.
fn run_paragraph_action(app: &mut DesignApp, action: ParagraphAction) {
    match action {
        ParagraphAction::Attrs(attrs) => {
            let _ = app.run("type.para", json!({"attrs": attrs}));
        }
        ParagraphAction::Command(id) => crate::menus::activate(app, id, &Value::Null),
        ParagraphAction::Nothing => {}
    }
}

/// The axes a slider can show: a malformed font's axis whose minimum lies above its maximum (or
/// isn't a number) is left out, since no value fits its range.
pub(super) fn usable_axes(axes: Vec<(String, String, f32, f32, f32)>) -> Vec<(String, String, f32, f32, f32)> {
    axes.into_iter().filter(|a| a.2 <= a.4).collect()
}

/// The slider step and the field's decimals for an axis: whole units, or hundredths for an axis
/// whose whole range is a few units (Skia's weight runs 0.48–3.2).
pub(super) fn axis_step(min: f32, max: f32) -> (f32, usize) {
    if max - min >= 10.0 { (1.0, 0) } else { (0.01, 2) }
}

/// The style for the axis values: `base {tag:value,…}`, each value to at most two decimals.
pub(super) fn variable_font_style(base: &str, values: &[(String, f32)]) -> String {
    let spec = values
        .iter()
        .map(|(t, v)| {
            let v = format!("{v:.2}");
            format!("{t}:{}", v.trim_end_matches('0').trim_end_matches('.'))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{base} {{{spec}}}")
}

/// The width of the axis sliders' value fields, so the sliders end at the same place.
const PANEL_FIELD_W: f32 = 52.0;

/// The caption font of the Character panel's rows.
fn caption_font() -> egui::FontId {
    egui::FontId::proportional(13.0)
}

/// The width of the widest of `labels` in the interface language: a column's caption cells.
fn caption_width(app: &DesignApp, ui: &Ui, labels: &[&str]) -> f32 {
    let t = Tokens::get(ui.ctx());
    labels
        .iter()
        .map(|l| ui.painter().layout_no_wrap(crate::i18n::tr(&app.ui.language, l).to_string(), caption_font(), t.text).size().x)
        .fold(0.0, f32::max)
        // The framework's text takes its own room around a word: a caption cell as wide as the bare word wraps it.
        .ceil()
        + 4.0
}

/// Whether `composer` (a paragraph's `composer` attribute) is a single-line composer, and the
/// paragraph and single-line composers of its kind (Japanese or not), for the composer choice.
pub(crate) fn composer_choice(composer: Option<&str>) -> (bool, [&'static str; 2]) {
    let single = matches!(composer, Some("singleLine" | "japaneseSingleLine"));
    let kind = if composer.is_some_and(|c| c.starts_with("japanese")) { ["japanese", "japaneseSingleLine"] } else { ["paragraph", "singleLine"] };
    (single, kind)
}

/// The Digits choices: (value, caption, sample of 0–3). The values are the file format's:
/// `arabic` is 0123, `hindi` the Arabic-Indic ٠١٢٣ and `farsi` the Persian/Urdu ۰۱۲۳; the captions
/// name them by the script's users.
pub const DIGIT_OPTIONS: [(&str, &str, &str); 5] =
    [("default", "Default", ""), ("arabic", "Arabic", "0123"), ("hindi", "Hindi", "٠١٢٣"), ("farsi", "Farsi", "۰۱۲۳"), ("native", "Native", "")];

/// A Digits choice as shown: its translated caption and a sample of the figures.
pub fn digit_label(lang: &str, o: (&str, &str, &str)) -> String {
    let name = crate::i18n::tr(lang, o.1);
    if o.2.is_empty() { name.to_string() } else { format!("{name}  {}", o.2) }
}

/// The least value a paragraph field takes: the first and last line indents may be negative (a
/// hanging line, a last line past the right indent), as in InDesign; the others not.
pub(crate) fn para_field_min(key: &str) -> f64 {
    if matches!(key, "firstLineIndent" | "lastLineIndent") { -10_000.0 } else { 0.0 }
}

/// The Character and Paragraph panels on the craft UI framework: every control a framework component over the engine commands.
mod framework {
    use super::{
        Align, DIGIT_OPTIONS, DesignApp, FIELD_H, GRID_BUTTONS, PANEL_FIELD_W, PARA_ALIGNS, PARA_DECORATIONS, PARA_ROWS, ParagraphAction,
        SAME_STYLE_SPACING, Ui, Value, axis_step, block, caption_width, digit_label, full_width, json, para_field_min, paragraph_menu_rows,
        run_paragraph_action, show_typography_panel, sub, text_attrs, units, usable_axes, variable_font_style,
    };
    use crate::framework_transform::{
        Artwork, Letters, MenuRow, Number, Quantity, caption, check, choice, font_style_select, icon_button, icon_number, icon_toggle,
        letters_button, menu, number_field, swatch_select,
    };
    use crate::panels::properties::text::framework::{kerning_field, language_select};
    use craft_behavior::EditEvent;
    use craft_ui_widgets::buttons::IconButton;
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
    use craft_ui_widgets::controls::disclosure::Disclosure;
    use craft_ui_widgets::primitives::IconSpec;
    use craft_ui_widgets::value_drag::Slider;

    /// The type sizes the Leading list offers.
    const LEADING_SIZES: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 21.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];

    /// The id of the control `key` of the panel the `ui` shows.
    fn cid(ui: &Ui, key: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
        ui.scope_id().with(("typography-panels", key))
    }

    /// A caption in its cell: one line, centred on the field beside it, cut with an ellipsis where the cell is too narrow
    /// (a cell narrower than its word would otherwise wrap the word and show only its first line).
    fn cell_label(ui: &mut egui::Ui, at: Option<egui::Rect>, text: &str) {
        use craft_ui_widgets::display::{Elide, Text};
        match at {
            Some(at) => {
                Text::new(text.to_owned()).elide(Elide::End).show_at(ui, at);
            }
            None => caption(ui, None, text),
        }
    }

    /// A caption cell and a field after it on one row of `width` from `x` in block `b` at `y`: their rects.
    fn cells(b: egui::Rect, x: f32, y: f32, width: f32, label_w: f32) -> (egui::Rect, egui::Rect) {
        let field_w = (width - label_w - 6.0).max(40.0);
        (sub(b, x, y, label_w, FIELD_H), sub(b, x + label_w + 6.0, y, field_w, FIELD_H))
    }

    pub(super) fn character_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
        let Some(a) = text_attrs(app) else {
            caption(ui, None, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
            return;
        };
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let c = a["chars"].clone();
        let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
        let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
        let row = FIELD_H + 5.0;
        let b = block(ui, 2.0 * row);
        let id = cid(ui, "font-family");
        crate::panels::font_family_field(app, ui, id, &fam, sub(b, 0.0, 0.0, 220.0, FIELD_H));
        let id = cid(ui, "font-style");
        if let Some(style) = font_style_select(app, ui, Some(sub(b, 0.0, row, 220.0, FIELD_H)), id, &fam, &sty) {
            let _ = app.run("type.char", json!({"attrs": {"fontStyle": style}}));
        }
        variable_font_axes(app, ui, &fam, &sty);
        // Language (hyphenation, spelling and typographer's quotes follow it), and Digits (World-Ready): how the
        // figures 0-9 are drawn.
        let label_w = caption_width(app, ui, &["Language", "Digits"]);
        let b = block(ui, 2.0 * row);
        let current = c["language"].as_str().unwrap_or("English: USA").to_string();
        let (cap, field) = cells(b, 0.0, 0.0, label_w + 6.0 + 170.0, label_w);
        cell_label(ui, Some(cap), &tr("Language"));
        let id = cid(ui, "language");
        language_select(app, ui, Some(field), id, &current);
        let digits = c["digits"].as_str().unwrap_or("default").to_string();
        let (cap, field) = cells(b, 0.0, row, label_w + 6.0 + 150.0, label_w);
        cell_label(ui, Some(cap), &tr("Digits"));
        let choices: Vec<(&'static str, String)> = DIGIT_OPTIONS.iter().map(|o| (o.0, digit_label(&lang, *o))).collect();
        let held = DIGIT_OPTIONS.iter().find(|o| o.0 == digits).map_or(DIGIT_OPTIONS[0].0, |o| o.0);
        let id = cid(ui, "digits");
        if let Some(d) = choice(ui, Some(field), id, &tr("Digits"), Some(held), &choices, None) {
            let _ = app.run("type.char", json!({"attrs": {"digits": d}}));
        }
        // Two columns: Size, Kerning and Baseline on the left, Leading and Tracking on the right; a column's captions
        // share the width of its widest one.
        let auto = c["leading"]["kind"].as_str() != Some("points");
        let lv = if auto { c["size"].as_f64().map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
        let (left_w, right_w) = (caption_width(app, ui, &["Size", "Kerning", "Baseline"]), caption_width(app, ui, &["Leading", "Tracking"]));
        let col = (full_width(ui) - 10.0) / 2.0;
        // Two columns when each field keeps room for its value (automatic leading reads "(14.4 pt)"), else one column of rows.
        let two = col - left_w.max(right_w) - 6.0 >= 96.0;
        let wide = left_w.max(right_w);
        let b = block(ui, if two { 3.0 * row } else { 5.0 * row });
        let left = |y: f32| if two { cells(b, 0.0, y, col, left_w) } else { cells(b, 0.0, y, b.width().min(wide + 6.0 + 130.0), wide) };
        let right = |y: f32| if two { cells(b, col + 10.0, y, col, right_w) } else { cells(b, 0.0, y, b.width().min(wide + 6.0 + 130.0), wide) };
        // The rows' places: the one column puts Leading under Size, and Tracking after Baseline.
        let (size_y, leading_y, kerning_y, baseline_y, tracking_y) =
            if two { (0.0, 0.0, row, 2.0 * row, 2.0 * row) } else { (0.0, row, 2.0 * row, 3.0 * row, 4.0 * row) };
        for (k, (cell, name, value, quantity, attrs)) in [
            (left(size_y), "Size", c["size"].as_f64(), Quantity::Points, "size"),
            (left(baseline_y), "Baseline", c["baselineShift"].as_f64(), Quantity::Points, "baselineShift"),
            (right(leading_y), "Leading", lv, Quantity::Points, "leading"),
            (right(tracking_y), "Tracking", c["tracking"].as_f64(), Quantity::Plain { decimals: 0 }, "tracking"),
        ]
        .into_iter()
        .enumerate()
        {
            let (cap, field) = cell;
            let label = tr(name);
            cell_label(ui, Some(cap), &label);
            let id = cid(ui, ("char-field", k, auto));
            // Automatic leading shows the value it works out to in parentheses, with the type sizes as its list.
            let n = Number::new(&label, value, quantity);
            let n = if attrs == "leading" { n.presets(LEADING_SIZES).range(0.0, 5000.0).derived(auto) } else { n };
            if let Some(Some(v)) = number_field(ui, Some(field), id, n) {
                let v = if attrs == "leading" { json!({"kind": "points", "value": v}) } else { json!(v) };
                let _ = app.run("type.char", json!({"attrs": {attrs: v}}));
            }
        }
        // Kerning keeps a row of its own: its Metrics and Optical words need more room than a column's field has.
        let kerning_label_w = if two { left_w } else { wide };
        let (cap, field) = cells(b, 0.0, kerning_y, (kerning_label_w + 6.0 + 130.0).min(b.width()), kerning_label_w);
        cell_label(ui, Some(cap), &tr("Kerning"));
        let id = cid(ui, "kerning");
        kerning_field(app, ui, Some(field), id, &c["kerning"]);
        // The text colour.
        if let Some(doc) = app.session.active().map(|d| d.doc.clone()) {
            let label_w = caption_width(app, ui, &["Color:"]);
            let b = block(ui, row);
            let (cap, field) = cells(b, 0.0, 0.0, label_w + 6.0 + 180.0, label_w);
            cell_label(ui, Some(cap), &tr("Color:"));
            let id = cid(ui, "fill");
            if let Some(n) = swatch_select(ui, Some(field), id, &tr("Color:"), &doc, c["fill"].as_str(), c["fill"].as_str()) {
                let _ = app.run("type.char", json!({"attrs": {"fill": n}}));
            }
        }
        // Bold and Italic pick the family's own styles (style linking); then the case and line buttons.
        let (weight, italic) = crate::panels::fonts(app).traits_of(&fam, &sty);
        let tip = |app: &DesignApp, name: &str, id: &str| {
            let name = crate::i18n::tr(&app.ui.language, name);
            match crate::menus::shortcut_of(app, id) {
                Some(sc) => format!("{name} ({})", crate::menus::shortcut_text(&sc)),
                None => name.to_string(),
            }
        };
        let caps = c["capitalization"].as_str() == Some("allCaps");
        let ul = c["underline"].as_bool().unwrap_or(false);
        let st = c["strikethrough"].as_bool().unwrap_or(false);
        let sup = c["position"].as_str() == Some("superscript");
        let buttons: [(&'static str, bool, String, bool, &'static str, Value); 6] = [
            ("B", false, tip(app, "Bold", "type.bold"), weight >= 650.0, "type.bold", json!({})),
            ("I", false, tip(app, "Italic", "type.italic"), italic, "type.italic", json!({})),
            ("TT", false, tr("All Caps"), caps, "type.char", json!({"attrs": {"capitalization": if caps { "normal" } else { "allCaps" }}})),
            ("U", false, tr("Underline"), ul, "type.char", json!({"attrs": {"underline": !ul}})),
            ("S", true, tr("Strikethrough"), st, "type.char", json!({"attrs": {"strikethrough": !st}})),
            ("T¹", false, tr("Superscript"), sup, "type.char", json!({"attrs": {"position": if sup { "normal" } else { "superscript" }}})),
        ];
        // OpenType follows the six buttons when the panel has the room for its word, and takes a row below them when not.
        let strip = 6.0 * 30.0;
        let wanted = caption_width(app, ui, &["OpenType"]) + 24.0;
        let beside = full_width(ui) - strip >= wanted;
        let b = block(ui, if beside { 22.0 } else { 22.0 + 4.0 + 22.0 });
        for (k, (text, struck, label, on, cmd, params)) in buttons.into_iter().enumerate() {
            let (at, id) = (sub(b, k as f32 * 30.0, 0.0, 26.0, 22.0), cid(ui, ("char-button", k)));
            if letters_button(ui, at, id, Letters { text, struck }, &label, on) {
                let _ = app.run(cmd, params);
            }
        }
        let id = cid(ui, "opentype");
        let at = if beside { sub(b, strip, 0.0, b.width() - strip, 22.0) } else { sub(b, 0.0, 26.0, wanted.max(80.0), 22.0) };
        let opentype = craft_ui_widgets::buttons::Button::new(tr("OpenType")).id(id).tooltip(tr("Show the Typography panel")).show_at(ui, at);
        if opentype.clicked() {
            show_typography_panel(app);
        }
        // Underline and Strikethrough Options, while the line is on.
        for (key, title) in [("underline", "Underline Options"), ("strikethrough", "Strikethrough Options")] {
            if !c[key].as_bool().unwrap_or(false) {
                continue;
            }
            line_options(app, ui, &c, key, &tr(title));
        }
        // Room under the last row, so the panel does not end at the window's edge.
        ui.add_space(8.0);
    }

    /// The options of the underline or strikethrough `key`, under a disclosure titled `title`: weight, offset and
    /// colour.
    fn line_options(app: &mut DesignApp, ui: &mut Ui, c: &Value, key: &str, title: &str) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let open_id = cid(ui, ("line-options-open", key));
        let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        let b = block(ui, FIELD_H);
        let id = cid(ui, ("line-options", key));
        let toggled = Disclosure::new(Bind::given(open))
            .id(id)
            .label(title.to_owned())
            .show_at(ui, sub(b, 0.0, 0.0, 20.0, FIELD_H))
            .events
            .committed()
            .copied();
        cell_label(ui, Some(sub(b, 24.0, 0.0, b.width() - 24.0, FIELD_H)), title);
        if let Some(on) = toggled {
            ui.data_mut(|d| d.insert_temp(open_id, on));
        }
        if !open {
            return;
        }
        let size = c["size"].as_f64().unwrap_or(12.0);
        let auto_offset = if key == "underline" { size * 0.12 } else { size * 0.3 };
        let label_w = caption_width(app, ui, &["Weight", "Offset", "Color:"]);
        let row = FIELD_H + 5.0;
        let b = block(ui, 3.0 * row);
        for (k, (name, attr, value)) in [
            ("Weight", format!("{key}Weight"), c[format!("{key}Weight")].as_f64().unwrap_or(size / 14.0)),
            ("Offset", format!("{key}Offset"), c[format!("{key}Offset")].as_f64().unwrap_or(auto_offset)),
        ]
        .into_iter()
        .enumerate()
        {
            let (cap, field) = cells(b, 0.0, k as f32 * row, label_w + 6.0 + 60.0, label_w);
            let label = tr(name);
            cell_label(ui, Some(cap), &label);
            let id = cid(ui, (key, name));
            if let Some(Some(v)) = number_field(ui, Some(field), id, Number::new(&label, Some(value), Quantity::Points)) {
                let v = if name == "Weight" { v.max(0.0) } else { v };
                let _ = app.run("type.char", json!({"attrs": {attr: v}}));
            }
        }
        if let Some(doc) = app.session.active().map(|d| d.doc.clone()) {
            let (cap, field) = cells(b, 0.0, 2.0 * row, label_w + 6.0 + 180.0, label_w);
            cell_label(ui, Some(cap), &tr("Color:"));
            let current = c[format!("{key}Color")].as_str().filter(|s| !s.is_empty()).map(str::to_string);
            let text_color = tr("(Text Color)");
            let id = cid(ui, (key, "color"));
            if let Some(n) =
                swatch_select(ui, Some(field), id, &tr("Color:"), &doc, current.as_deref(), Some(current.as_deref().unwrap_or(&text_color)))
            {
                let _ = app.run("type.char", json!({"attrs": {format!("{key}Color"): n}}));
            }
        }
    }

    /// The Variable Font sliders: one per axis, each with a field for a typed value. A drag previews on the page and is
    /// one undo step once let go; a click, an arrow key or a typed value applies at once.
    fn variable_font_axes(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, style: &str) {
        if family.is_empty() {
            return;
        }
        let db = crate::panels::fonts(app);
        let axes = usable_axes(db.axes(family, style));
        if axes.is_empty() {
            return;
        }
        let base = designcraft_fonts::base_style(style).to_string();
        let face = db.face(family, style);
        let cur = |tag: &str, default: f32| face.coords.iter().find(|(t, _)| t == tag.as_bytes()).map_or(default, |c| c.1);
        let mut values: Vec<(String, f32)> = axes.iter().map(|a| (a.0.clone(), cur(&a.0, a.3))).collect();
        caption(ui, None, crate::i18n::tr(&app.ui.language, "Variable Font"));
        let (mut held, mut dragged, mut applied) = (false, false, false);
        let names: Vec<&str> = axes.iter().map(|a| a.1.as_str()).collect();
        let label_w = caption_width(app, ui, &names);
        let width = full_width(ui);
        for ((tag, name, min, _, max), value) in axes.iter().zip(values.iter_mut()) {
            // Caption, a slider filling the middle, then the value field: every slider is as long.
            let b = block(ui, FIELD_H);
            cell_label(ui, Some(sub(b, 0.0, 0.0, label_w, FIELD_H)), name);
            let slider_w = (width - label_w - PANEL_FIELD_W - 12.0).max(40.0);
            let (step, decimals) = axis_step(*min, *max);
            let id = cid(ui, ("axis", tag));
            let out = Slider::new(Bind::given(f64::from(value.1)), f64::from(*min)..=f64::from(*max))
                .id(id)
                .label(name.clone())
                .step(f64::from(step))
                .show_at(ui, sub(b, label_w + 6.0, 0.0, slider_w, FIELD_H));
            let pressed = out.response.is_pointer_button_down_on() || out.response.dragged();
            held |= pressed;
            for event in out.events {
                match event {
                    EditEvent::Update { value: v, .. } => {
                        value.1 = (v as f32).clamp(*min, *max);
                        dragged = true;
                    }
                    EditEvent::Commit { after, .. } | EditEvent::Set { after, .. } => {
                        value.1 = (after as f32).clamp(*min, *max);
                        applied = true;
                    }
                    _ => {}
                }
            }
            let id = cid(ui, ("axis-value", tag));
            let field = sub(b, label_w + 12.0 + slider_w, 0.0, PANEL_FIELD_W, FIELD_H);
            if let Some(Some(v)) = number_field(ui, Some(field), id, Number::new(name, Some(f64::from(value.1)), Quantity::Plain { decimals })) {
                value.1 = (v as f32).clamp(*min, *max);
                applied = true;
            }
        }
        let attrs = json!({"attrs": {"fontStyle": variable_font_style(&base, &values)}});
        if applied {
            app.end_live_preview();
            let _ = app.run("type.char", attrs);
        } else if dragged {
            app.live_preview("Variable Font", "type.char", attrs);
        } else if held {
            app.hold_live_preview();
        }
    }

    pub(super) fn paragraph_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
        let Some(a) = text_attrs(app) else {
            caption(ui, None, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
            return;
        };
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let u = units(app);
        let p = a["para"].clone();
        let fw = full_width(ui);
        let half = (fw - 10.0) / 2.0;
        let field_w = half - 24.0;
        // Alignment in groups of 3, 4 and 2, and the panel menu at the right.
        let cur: Align = serde_json::from_value(p["align"].clone()).unwrap_or_default();
        const GAP: f32 = 8.0;
        let pitch = ((fw - 24.0 - 2.0 * GAP) / 9.0).clamp(16.0, 24.0);
        let b = block(ui, 21.0);
        let mut x = 0.0;
        for (i, (al, artwork)) in PARA_ALIGNS.into_iter().enumerate() {
            if i == 3 || i == 7 {
                x += GAP;
            }
            let (at, id) = (sub(b, x, 0.0, pitch, 21.0), cid(ui, ("align", i)));
            if icon_toggle(ui, at, id, artwork, &tr(al.label()), cur == al) {
                let _ = app.run("type.para", json!({"attrs": {"align": al}}));
            }
            x += pitch;
        }
        let id = cid(ui, "paragraph-menu");
        let opener = IconButton::new(IconSpec::glyph(&Artwork("menu")))
            .id(id)
            .label(tr("Paragraph"))
            .tooltip(tr("Paragraph"))
            .popup(craft_ui_widgets::contract::PopupKind::Menu)
            .show_at(ui, sub(b, fw - 20.0, 0.0, 20.0, 21.0));
        paragraph_menu(app, ui, &opener, &p);
        ui.add_space(6.0);
        // Indents, spacing and drop cap, two to a row as in InDesign.
        let row = FIELD_H + 5.0;
        let b = block(ui, 5.0 * row - 5.0);
        let at = |right: bool, y: f32| {
            let x = if right { half + 10.0 } else { 0.0 };
            (sub(b, x, y + 1.0, 19.0, 19.0), sub(b, x + 24.0, y, field_w, FIELD_H))
        };
        for (i, (l, r)) in PARA_ROWS.into_iter().enumerate() {
            for (right, (artwork, tip, key)) in [(false, l), (true, r)] {
                let (icon_at, field_at) = at(right, i as f32 * row);
                let id = cid(ui, key);
                let label = tr(tip);
                let n = Number::new(&label, p[key].as_f64(), Quantity::Length(u)).range(para_field_min(key), 10_000.0);
                if let Some(Some(v)) = icon_number(ui, icon_at, field_at, id, artwork, n) {
                    let _ = app.run("type.para", json!({"attrs": {key: v}}));
                }
            }
        }
        // Space Between Paragraphs Using Same Style: Ignore (unset) or a space that replaces the space after and before
        // between paragraphs of the same style.
        let ignore = tr("Ignore");
        let label = tr("Space Between Paragraphs Using Same Style");
        let n = Number::new(&label, p["sameParaStyleSpacing"].as_f64(), Quantity::Length(u))
            .presets(SAME_STYLE_SPACING)
            .ignore(&ignore)
            .range(0.0, 10_000.0);
        let (icon_at, field_at) = at(false, 3.0 * row);
        let id = cid(ui, "sameParaStyleSpacing");
        if let Some(v) = icon_number(ui, icon_at, field_at, id, "space-same", n) {
            let _ = app.run("type.para", json!({"attrs": {"sameParaStyleSpacing": v}}));
        }
        for (right, artwork, tip, key, max) in [
            (false, "drop-lines", "Drop Cap Number of Lines", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES),
            (true, "drop-chars", "Drop Cap One or More Characters", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS),
        ] {
            let (icon_at, field_at) = at(right, 4.0 * row);
            let id = cid(ui, key);
            let label = tr(tip);
            let n = Number::new(&label, p[key].as_f64(), Quantity::Plain { decimals: 0 }).range(0.0, max as f64);
            if let Some(Some(v)) = icon_number(ui, icon_at, field_at, id, artwork, n) {
                let _ = app.run("type.para", json!({"attrs": {key: (v.max(0.0) as u64).min(max as u64)}}));
            }
        }
        // Shading and Border: on or off at the left; at the right the Paragraph Borders and Shading dialog on its tab
        // and the colour.
        ui.add_space(10.0);
        let doc = app.session.active().map(|d| d.doc.clone());
        let b = block(ui, 2.0 * row - 5.0);
        for (i, (on_key, color_key, label, artwork, tab, name)) in PARA_DECORATIONS.into_iter().enumerate() {
            let y = i as f32 * row;
            let id = cid(ui, on_key);
            let on = p[on_key].as_bool().unwrap_or(false);
            if let Some(on) = check(ui, Some(sub(b, 0.0, y, half, FIELD_H)), id, &tr(label), on, true) {
                let _ = app.run("type.para", json!({"attrs": {on_key: on}}));
            }
            let id = cid(ui, (on_key, "dialog"));
            if icon_button(ui, sub(b, half + 10.0, y, 23.0, FIELD_H), id, artwork, &tr("Paragraph Borders and Shading…"), false) {
                let _ = app.run("app.paragraphBordersDialog", json!({"tab": tab}));
            }
            if let Some(doc) = &doc {
                let color = p[color_key].as_str().unwrap_or("[Black]").to_string();
                let id = cid(ui, color_key);
                if let Some(sw) =
                    swatch_select(ui, Some(sub(b, half + 34.0, y, half - 24.0, FIELD_H)), id, &tr(name), doc, Some(&color), Some(&color))
                {
                    let _ = app.run("type.para", json!({"attrs": {color_key: sw}}));
                }
            }
        }
        // Hyphenate, and Do Not Align / Align to Baseline Grid.
        ui.add_space(10.0);
        let b = block(ui, FIELD_H);
        let id = cid(ui, "hyphenate");
        if let Some(on) = check(ui, Some(sub(b, 0.0, 0.0, half, FIELD_H)), id, &tr("Hyphenate"), p["hyphenate"].as_bool().unwrap_or(true), true) {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": on}}));
        }
        let grid = matches!(p["gridAlign"].as_str(), Some("allLines" | "firstLineOnly"));
        for (i, (artwork, tip, on, v)) in GRID_BUTTONS.into_iter().enumerate() {
            let on = on == grid;
            let (at, id) = (sub(b, fw - 48.0 + i as f32 * 25.0, 0.0, 23.0, FIELD_H), cid(ui, ("grid", i)));
            if icon_toggle(ui, at, id, artwork, &tr(tip), on) && !on {
                let _ = app.run("type.para", json!({"attrs": {"gridAlign": v}}));
            }
        }
        ui.add_space(4.0);
    }

    /// The Paragraph panel menu hung from `opener`, its rows those of [`paragraph_menu_rows`].
    fn paragraph_menu(app: &mut DesignApp, ui: &mut Ui, opener: &craft_ui_widgets::buttons::ButtonOutput, p: &Value) {
        let lang = app.ui.language.clone();
        let groups = paragraph_menu_rows(app, p);
        let labels: Vec<Vec<String>> = groups.iter().map(|g| g.iter().map(|r| crate::i18n::tr(&lang, r.label).to_owned()).collect()).collect();
        let rows: Vec<Vec<MenuRow<'_, ParagraphAction>>> = groups
            .iter()
            .zip(&labels)
            .map(|(g, l)| {
                g.iter()
                    .zip(l)
                    .map(|(r, label)| MenuRow { key: r.action.clone(), label, checked: r.checked, enabled: r.enabled, shortcut: r.shortcut.clone() })
                    .collect()
            })
            .collect();
        let id = cid(ui, "paragraph-menu-rows");
        if let Some(action) = menu(ui, id, crate::i18n::tr(&lang, "Paragraph"), opener, &rows) {
            run_paragraph_action(app, action);
        }
    }
}

/// The Character and Paragraph panels on the framework, driven with real pointer and key events.
#[cfg(test)]
mod framework_tests {
    use super::super::framework_test_kit::*;
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT as _;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    #[test]
    fn the_character_panels_underline_button_and_size_field_run_type_char_once_each() {
        let mut app = document();
        text_frame(&mut app, "Hello");
        let mut h = open(app, crate::panels::properties::character_panel);
        let steps = undo_steps(&h);
        click(&mut h, Role::Button, "Underline");
        assert_eq!(journal(&h, "type.char").last(), Some(&json!({"attrs": {"underline": true}})));
        assert_eq!(undo_steps(&h), steps + 1);
        assert!(h.query_all_by_label("Underline Options").next().is_some(), "an underline shows its options");
        type_into(&mut h, "Size", "18");
        assert_eq!(text_attrs(&mut h)["chars"]["size"].as_f64(), Some(18.0));
        assert_eq!(undo_steps(&h), steps + 2);
    }

    #[test]
    fn the_paragraph_panels_indent_field_and_menu_run_type_para() {
        let mut app = document();
        text_frame(&mut app, "Hello");
        let mut h = open(app, crate::panels::properties::paragraph_panel);
        // Picas: the field reads the app's own spelling.
        type_into(&mut h, "Left Indent", "1p0");
        assert_eq!(text_attrs(&mut h)["para"]["leftIndent"].as_f64(), Some(12.0));
        click(&mut h, Role::Button, "Paragraph");
        let at = h.query_all_by_label("Balance Ragged Lines").last().expect("the panel menu lists Balance Ragged Lines").rect().center();
        click_at(&mut h, at);
        assert_eq!(journal(&h, "type.para").last(), Some(&json!({"attrs": {"balanceRagged": true}})));
    }

    #[test]
    fn hyphenate_is_a_framework_check_box_on_the_paragraph_panel() {
        let mut app = document();
        text_frame(&mut app, "Hello");
        let mut h = open(app, crate::panels::properties::paragraph_panel);
        let on = text_attrs(&mut h)["para"]["hyphenate"].as_bool().unwrap_or(true);
        click(&mut h, Role::CheckBox, "Hyphenate");
        assert_eq!(journal(&h, "type.para").last(), Some(&json!({"attrs": {"hyphenate": !on}})));
    }
}
