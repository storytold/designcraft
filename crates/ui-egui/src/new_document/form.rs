//! File › New › Document on the craft framework's dialog host: the preset tabs over the cards, the chosen preset's
//! details beside them, and Preview with the status line in the footer. The form edits the same draft fields the
//! browser window does, so Create and Close go through the host to [`create`](super::create) and the dialog's cancel.

use super::{
    Card, DEFAULT_CARD, EDGES, NEW_DOCUMENT_UNITS, TABS, cards, link_edges, measure, paint_page, save_preset, select, set_units, size_text, units,
};
use crate::DesignApp;
use crate::dialogs::Dialog;
use crate::dialogs::framework::Form;
use craft_behavior::ColumnWidth;
use craft_behavior::{DisplayOrder, ItemSource, Mixed, Selection, SelectionChange, keyed};
use craft_ui_panels::sections::Section;
use craft_ui_widgets::buttons::{Button, IconButton, ToggleButton};
use craft_ui_widgets::choice::{Checkbox, Segmented};
use craft_ui_widgets::contract::{
    Bind, CollectionEvent, Entry, FieldLayout, Gap, Pad, Props as _, Scroll, SelectMode, Show as _, ShowCollection as _, ShowContainer as _,
};
use craft_ui_widgets::display::{Divider, Shade, Text};
use craft_ui_widgets::form::Field;
use craft_ui_widgets::layout::CellAlign;
use craft_ui_widgets::layout_components::{Grid, Row, Stack};
use craft_ui_widgets::list::ThumbAspect;
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::popover::Popover;
use craft_ui_widgets::primitives::{ContentPainterCx, Glyph, IconSpec};
use craft_ui_widgets::style::{SurfaceKind, TypeRole, UiTokens};
use craft_ui_widgets::tabs::{Tab, TabKind, TabStrip};
use craft_ui_widgets::text::{NumberField, StepArrows, TextField};
use craft_ui_widgets::tile_grid::{CardText, Tile, TileGrid};
use designcraft_geom::Unit;
use egui::{Color32, Rect, Ui};
use serde_json::{Value, json};
use std::borrow::Cow;

/// New Document on the framework's dialog host.
pub(crate) const FORM: Form = Form::new(show).width(960.0).opens_at_width().footer(footer);

/// Whether the Save Document Preset popover is open: a draft field, as the dialog's other transient state.
const PRESET_POPOVER: &str = "_presetPopover";
/// How high the presets and the details may grow before they scroll.
const PANE_ROOM: f32 = 560.0;
/// The least room the one column keeps in a very short window.
const LEAST_COLUMN_ROOM: f32 = 48.0;
/// The least room the panes keep in a short window, which scrolls the dialog's body instead.
const LEAST_PANE_ROOM: f32 = 120.0;
/// The dialog's title bar, footer and padding: what the panes' room leaves of the height the window gives a dialog.
const DIALOG_CHROME_H: f32 = 160.0;
/// The share of the window's height a framework dialog may take before its body scrolls (`Overlay::dialog`'s cap).
const DIALOG_HEIGHT_SHARE: f32 = 0.9;
/// The dialog's padding on its two sides (`Pad::Regular` each side).
const DIALOG_PAD_W: f32 = 16.0;
/// The details' column, as wide as the native one: two captioned columns and the link toggle beside the edges.
const DETAILS_W: f32 = 300.0;
/// The gap between the presets and the details (`Gap::Section`), and a little slack for the details' scroll bar.
const PANE_GAP: f32 = 20.0;
/// The category tabs over the inset, with the gap under them.
const TABS_H: f32 = 40.0;
/// The inset's padding on each side (`Pad::Regular`).
const INSET_PAD: f32 = 8.0;
/// The share of the window's width a framework dialog may take (`Overlay::dialog`'s cap).
const DIALOG_WIDTH_SHARE: f32 = 0.8;
/// The dialog width below which the presets go under the details, in one column the dialog's body scrolls, as the native
/// dialog did in a small window.
const TWO_PANES: f32 = 700.0;
/// The details' two columns of captioned controls, each as wide as the native dialog's.
const DETAIL_COLUMNS: &[ColumnWidth] = &[ColumnWidth::Fixed(124.0), ColumnWidth::Fixed(124.0)];
/// The preset cards' picture: this many points wide, and this much wider than high, so a card is
/// about 125 by 140 points with its name and size under the page, as the native cards were.
const CARD_SIDE: f32 = 108.0;
const CARD_ASPECT: f32 = 1.5;

/// One of the app's icons, by its name in `crate::icons`.
#[derive(craft_ui::contract::MemoProps)]
struct AppIcon(&'static str);

impl Glyph for AppIcon {
    fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, color: Color32, _mirrored: bool) {
        crate::icons::paint(painter, rect, self.0, color);
    }
}

static CLOCK: AppIcon = AppIcon("clock");
static PORTRAIT: AppIcon = AppIcon("orient-portrait");
static LANDSCAPE: AppIcon = AppIcon("orient-landscape");
static LINKED: AppIcon = AppIcon("link");
static UNLINKED: AppIcon = AppIcon("link-broken");
static PRESET_SAVE: AppIcon = AppIcon("preset-save");

fn tr(app: &DesignApp, s: &str) -> String {
    crate::i18n::tr(&app.ui.language, s).to_owned()
}

/// The cards of a tab as a display order keyed by position.
struct Cards<'a>(&'a [Card]);

impl DisplayOrder<usize> for Cards<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn key(&self, at: usize) -> usize {
        at
    }
    fn index_of(&self, key: &usize) -> Option<usize> {
        (*key < self.0.len()).then_some(*key)
    }
    fn text(&self, at: usize) -> Option<Cow<'_, str>> {
        self.0.get(at).map(|card| Cow::Borrowed(card.label.as_str()))
    }
}
impl ItemSource<usize> for Cards<'_> {}

/// The window's body: the presets beside the details. `Some(true)` when a card was activated (a double click or Enter
/// creates the document at once).
///
/// The dialog fits the window, as the native one did: the panes grow as high as the window lets the dialog be, up to
/// [`PANE_ROOM`], and a window too narrow for the two panes side by side shows the details, then the presets, in one
/// column that the dialog's body scrolls.
fn show(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let mut create = false;
    let window = ui.ctx().content_rect();
    if one_column(ui.ctx()) {
        // The column scrolls in the room the window leaves under the title and over the footer, so Close and Create stay
        // in view: a body that only the dialog's own scroll held grew past the window and took the footer with it.
        let room = (window.height() * DIALOG_HEIGHT_SHARE - DIALOG_CHROME_H).max(LEAST_COLUMN_ROOM);
        // `room_height` caps what the container offers its children, not the container itself: the column that scrolls
        // is the one child of a stack that offers it the room, so it is at most that tall and scrolls the rest.
        Stack::new().id_salt("new_document_column_room").fill_width().room_height(room).show(ui, |ui, _| {
            Stack::new().id_salt("new_document_column").fill_width().gap(Gap::Section).scroll(Scroll::vertical()).show(ui, |ui, _| {
                details(app, ui, d);
                tabs(app, ui, d);
                inset(ui, false, |ui| create = presets(app, ui, d));
                preview_check(app, ui, d);
            });
        });
        return create.then_some(true);
    }
    let room = (window.height() * DIALOG_HEIGHT_SHARE - DIALOG_CHROME_H).clamp(LEAST_PANE_ROOM, PANE_ROOM);
    // The dialog opens 960 points wide, or as wide as the window lets it be. A container's frame is as wide as its
    // content (the layer's least width widens the layer, not the frame), so each pane asks for its own width: the
    // details their native column, the presets the rest.
    let body = (FORM.width.min(window.width() * DIALOG_WIDTH_SHARE) - DIALOG_PAD_W).max(TWO_PANES - DIALOG_PAD_W);
    let presets_w = body - DETAILS_W - PANE_GAP;
    Row::new().id_salt("new_document_form").fill_width().gap(Gap::Section).show(ui, |ui, _| {
        Stack::new().id_salt("new_document_presets").room_height(room).gap(Gap::Item).show(ui, |ui, _| {
            tabs(app, ui, d);
            // The inset fills the column down to the footer: as wide as the column, as high as the room the tabs leave.
            let inset_h = room - TABS_H;
            inset(ui, true, |ui| {
                ui.set_min_width(presets_w - 2.0 * INSET_PAD);
                ui.set_min_height(inset_h - 2.0 * INSET_PAD);
                create = presets(app, ui, d);
            });
        });
        // The details scroll in the panes' room: the scrolling stack is the one child of a stack that offers it the room.
        Stack::new().id_salt("new_document_details_room").room_height(room).show(ui, |ui, _| {
            Stack::new().id_salt("new_document_details").gap(Gap::Item).scroll(Scroll::vertical()).show(ui, |ui, _| {
                ui.set_min_width(DETAILS_W);
                details(app, ui, d);
            });
        });
    });
    create.then_some(true)
}

/// The presets' inset: a darker, rounded panel with padding that holds the heading and the cards, and scrolls them.
/// Beside the details it scrolls the cards in the room the column gives it (`scrolls`).
fn inset(ui: &mut Ui, scrolls: bool, mut body: impl FnMut(&mut Ui)) {
    let stack = Stack::new().id_salt("new_document_presets_inset").surface(SurfaceKind::Sunken).pad(Pad::Regular).gap(Gap::Item);
    let stack = if scrolls { stack.scroll(Scroll::vertical()) } else { stack.fill_width() };
    stack.show(ui, |ui, _| body(ui));
}

/// Preview, and the reason Create was refused. Preview makes the document it shows here, after the check box.
fn footer(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let one_column = one_column(ui.ctx());
    Row::new().id_salt("new_document_footer").show(ui, |ui, _| {
        // In a small window the footer keeps only the buttons, so Close and Create stay inside it: Preview goes to the end
        // of the body ([`show`]).
        if !one_column {
            preview_check(app, ui, d);
        }
        if let Some(status) = d.fields.get("status").and_then(Value::as_str) {
            Text::new(status.to_owned()).shade(Shade::Muted).show(ui);
        }
    });
    let ctx = ui.ctx().clone();
    super::preview(app, d, &ctx);
}

/// The Preview check box.
fn preview_check(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let out = Checkbox::new(Bind::given(d.b("preview"))).id_salt("new_document_preview").label(tr(app, "Preview")).show(ui);
    if let Some(on) = out.events.committed() {
        d.fields.insert("preview".into(), json!(on));
    }
}

/// Whether the window is too small for the presets beside the details: too narrow for the two panes, or too short for
/// their least room. The details, then the presets, stand in one column that the dialog's body scrolls.
fn one_column(ctx: &egui::Context) -> bool {
    let window = ctx.content_rect();
    window.width() * DIALOG_WIDTH_SHARE < TWO_PANES || window.height() * DIALOG_HEIGHT_SHARE - DIALOG_CHROME_H < LEAST_PANE_ROOM
}

/// The preset tabs. As in InDesign, a tab shows its first preset's details (Web and Mobile in pixels).
fn tabs(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let current = d.s("tab");
    let keys: Vec<&'static str> = TABS.iter().map(|(key, _)| *key).collect();
    let shown = keys.iter().copied().find(|key| *key == current).unwrap_or(TABS[0].0);
    let source = keyed(&keys, |key: &&'static str| *key);
    let strip = TabStrip::new(tr(app, "New Document")).kind(TabKind::Text).large().shown(shown).natural_height(true).show_items(
        ui,
        &source,
        &mut |key, _| {
            let label = TABS.iter().find(|(held, _)| held == key).map_or("", |(_, label)| *label);
            let tab = Tab::new(tr(app, label));
            if *key == "recent" { tab.icon(IconSpec::glyph(&CLOCK)) } else { tab }
        },
    );
    for event in strip.response.events {
        if let CollectionEvent::Selection { change: SelectionChange::Set(keys), .. } = event
            && let Some(key) = keys.last()
            && *key != current
        {
            d.fields.insert("tab".into(), json!(key));
            if let Some(card) = cards(app, key).into_iter().next() {
                select(d, &card);
            }
        }
    }
}

/// The tile of a card: the page at the preset's proportions (crop marks for a custom size), its name and size.
fn tile(card: &Card, units: Unit) -> Tile<'static> {
    let (nd, custom) = (card.nd.clone(), card.custom);
    Tile::new(card.label.clone())
        .detail(size_text(&card.nd, units))
        .flat_thumbnail()
        // The picked preset is outlined in the accent, as it always was; the flat thumbnail marks a pick with a fill alone.
        .pick_ring()
        .thumb(ThumbAspect::new(1, 1), move |cx: &ContentPainterCx<'_>, rect: Rect| paint_page(&cx.painter, rect, &nd, custom, cx.ink()))
}

/// The cards of the current tab; true when one was activated. A click picks a card and fills the details; on Saved,
/// Delete Preset (or the Delete key) removes the picked preset.
fn presets(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let tab = d.s("tab");
    let list = cards(app, &tab);
    let heading = match tab.as_str() {
        "recent" => "Your Recent Items",
        "saved" => "Saved Presets",
        _ => "Blank Document Presets",
    };
    Text::new(format!("{} ({})", tr(app, heading).to_uppercase(), list.len())).role(TypeRole::SectionCaption).shade(Shade::Muted).show(ui);
    let current = d.s("preset");
    let card_units = units(d);
    let saved = tab == "saved";
    let selection = list.iter().position(|card| card.key == current).map_or_else(|| Selection::new(Vec::new(), None, None), Selection::single);
    let shown = TileGrid::<usize>::new(tr(app, heading))
        .id_salt(("new_document_cards", tab.as_str()))
        .card()
        .card_text(CardText::Preset)
        .detail_line()
        .select(SelectMode::Single)
        .deletable(saved)
        .rows_stamp(list.len() as u64)
        .card_size(CARD_SIDE)
        .aspect(CARD_ASPECT)
        .selection(Bind::given(selection))
        // The presets' inset scrolls; the grid takes its whole height inside it.
        .natural_height(true)
        .show_items(ui, &Cards(&list), &mut |at, _| tile(&list[*at], card_units));
    let mut create = false;
    let mut delete = None;
    for event in shown.response.events {
        match event {
            CollectionEvent::Selection { change: SelectionChange::Set(keys), .. } => {
                if let Some(card) = keys.last().and_then(|at| list.get(*at)) {
                    select(d, card);
                }
            }
            // A click on the card already picked applies it again, as a click on a card always did: its details
            // replace what was edited since.
            CollectionEvent::Picked { key, .. } => {
                if let Some(card) = list.get(key) {
                    select(d, card);
                }
            }
            CollectionEvent::Activate(at) => {
                if let Some(card) = list.get(at) {
                    select(d, card);
                    create = true;
                }
            }
            CollectionEvent::Delete(keys) => {
                delete = keys.iter().filter_map(|at| list.get(*at)).find(|card| card.key != DEFAULT_CARD).map(|card| card.nd.title.clone());
            }
            _ => {}
        }
    }
    if saved {
        let chosen = list.iter().find(|card| card.key == current && card.key != DEFAULT_CARD);
        let label = tr(app, "Delete Preset");
        if Button::new(label).id_salt("new_document_delete_preset").enabled(chosen.is_some()).show(ui).clicked() {
            delete = chosen.map(|card| card.nd.title.clone());
        }
    }
    if let Some(name) = delete
        && let Err(e) = app.run("file.deletePreset", json!({"name": name}))
    {
        app.status(e);
    }
    create
}

/// A text field over the string draft field `key`, named `label`. Returns whether the text changed.
fn text(ui: &mut Ui, d: &mut Dialog, key: &str, label: String, hint: Option<String>) -> bool {
    let before = d.s(key);
    let mut text = before.clone();
    let field = TextField::new(label, Bind::given(before.clone())).id_salt(("new_document_text", key)).limit(None::<usize>);
    let out = match hint {
        Some(hint) => field.placeholder(hint).show(ui),
        None => field.show(ui),
    };
    for event in out.events {
        event.apply(&mut text);
    }
    let changed = text != before;
    if changed {
        d.fields.insert(key.into(), Value::String(text));
    }
    changed
}

/// A measure over the string draft field `key`, named `label`, with the step halves before it as the dialog's number
/// fields have: a step writes the new length in the dialog's units. Returns whether the text changed.
fn measure_text(ui: &mut Ui, d: &mut Dialog, key: &str, label: String) -> bool {
    let unit = units(d);
    let (mut stepped, mut changed) = (None, false);
    Row::new().id_salt(("new_document_measure_row", key)).show(ui, |ui, _| {
        let id = ui.make_persistent_id(("new_document_measure_steps", key));
        stepped = crate::framework_transform::measure_steps(ui, id, &label, unit, true);
        changed = text(ui, d, key, label.clone(), None);
    });
    if let Some(by) = stepped {
        let value = measure(d, key).unwrap_or(0.0) + by;
        d.fields.insert(key.into(), Value::String(designcraft_geom::units::format_measure(value.max(0.0), unit)));
        return true;
    }
    changed
}

/// A whole number from 1 over the draft field `key` (Pages, Start #, Columns).
fn count_field(ui: &mut Ui, d: &mut Dialog, key: &str, label: String) {
    let value = d.n(key).filter(|v| v.is_finite());
    let out = NumberField::new(label, Bind::given(value))
        .id_salt(("new_document_count", key))
        .range(1.0..=9999.0)
        .step(1.0)
        .step_arrows(StepArrows::Leading)
        .fixed_decimals(0)
        .commit_decimals(0)
        .show(ui);
    if let Some(Some(v)) = out.edit.events.committed().copied() {
        d.fields.insert(key.into(), json!(v.round() as u64));
    }
}

/// A check box over the bool draft field `key`, under its caption as the native dialog set it: the caption names the
/// box (the field's label step), and the box paints no label of its own.
fn check(ui: &mut Ui, d: &mut Dialog, key: &str, label: String) {
    cell(ui, label, |ui| {
        let out = Checkbox::new(Bind::given(d.b(key))).id_salt(("new_document_check", key)).show(ui);
        if let Some(on) = out.events.committed() {
            d.fields.insert(key.into(), json!(on));
        }
    });
}

/// Captioned controls in the details' two columns, a row of two at a time, each caption above its control.
fn columns(ui: &mut Ui, salt: &'static str, body: impl FnMut(&mut Ui)) {
    let mut body = body;
    Grid::new(DETAIL_COLUMNS).id_salt(salt).cells(CellAlign::Top).show(ui, |ui, _| body(ui));
}

/// A captioned control over its caption, in one of the details' two columns.
fn cell(ui: &mut Ui, caption: String, add: impl FnOnce(&mut Ui)) {
    let mut add = Some(add);
    Field::new(caption.clone()).id_salt(("new_document_cell", caption)).layout(FieldLayout::Above).show(ui, |ui, _| {
        if let Some(add) = add.take() {
            add(ui);
        }
    });
}

/// The Save Document Preset button and its popover: the preset's name, then Save Preset (or Enter in the name).
fn save_preset_control(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let label = tr(app, "Save Document Preset");
    let button = IconButton::new(IconSpec::glyph(&PRESET_SAVE)).id_salt("new_document_save_preset").label(label.clone()).tooltip(label).show(ui);
    let was_open = d.b(PRESET_POPOVER);
    let mut open = was_open != button.clicked();
    let mut save = None;
    let title = tr(app, "Save Document Preset As:");
    Popover::new(title.clone(), Bind::bound(&mut open)).opened_by(&button.response).show(ui, |ui, _| {
        Text::new(title.clone()).show(ui);
        let mut name = d.s("presetName");
        let out = TextField::new(tr(app, "Preset Name"), Bind::given(name.clone()))
            .id_salt("new_document_preset_name")
            .placeholder(tr(app, "Custom"))
            .limit(None::<usize>)
            .submits()
            .show(ui);
        let submitted = out.submitted;
        for event in out.events {
            event.apply(&mut name);
        }
        if name != d.s("presetName") {
            d.fields.insert("presetName".into(), json!(name));
        }
        if Button::new(tr(app, "Save Preset")).id_salt("new_document_save_preset_go").show(ui).clicked() || submitted {
            save = Some(if name.trim().is_empty() { tr(app, "Custom") } else { name });
        }
    });
    if let Some(name) = save {
        match save_preset(app, d, &name) {
            Ok(()) => open = false,
            // Refused (a built-in name…): stays open to be corrected.
            Err(e) => app.status(e),
        }
    }
    if open != was_open {
        d.fields.insert(PRESET_POPOVER.into(), json!(open));
    }
}

/// Four edges (`{group}Top`…) beside a link that keeps them equal; linking makes every edge the top's.
fn edges(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, group: &str) {
    let linked_key = format!("{group}Linked");
    let linked = d.b(&linked_key);
    let labels = if d.b("facingPages") { EDGES } else { ["Top", "Bottom", "Left", "Right"] };
    let (mut changed, mut source) = (None, None);
    Row::new().id_salt(("new_document_edges", group)).show(ui, |ui, _| {
        Grid::new(DETAIL_COLUMNS).id_salt(("new_document_edge_fields", group)).cells(CellAlign::Top).show(ui, |ui, _| {
            for at in 0..4 {
                let key = format!("{group}{}", EDGES[at]);
                let caption = tr(app, labels[at]);
                cell(ui, caption.clone(), |ui| {
                    if measure_text(ui, d, &key, caption) {
                        changed = Some(key.clone());
                    }
                });
            }
        });
        let out = ToggleButton::new(tr(app, "Make all settings the same"), Bind::given(linked))
            .id_salt(("new_document_link", group))
            .icon(IconSpec::glyph(&UNLINKED))
            .on_icon(IconSpec::glyph(&LINKED))
            .icon_only()
            .show(ui);
        if let Some(on) = out.events.committed() {
            d.fields.insert(linked_key.clone(), json!(on));
            if *on {
                source = Some(format!("{group}Top"));
            }
        }
    });
    if linked && source.is_none() {
        source = changed;
    }
    if let Some(key) = source {
        link_edges(d, group, &key);
    }
}

/// The preset's details: the document name and Save Document Preset, the page size, units and orientation, pages,
/// columns, margins, bleed and slug.
fn details(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    Text::new(tr(app, "Preset Details").to_uppercase()).role(TypeRole::SectionCaption).shade(Shade::Muted).show(ui);
    Row::new().id_salt("new_document_name_row").show(ui, |ui, _| {
        let hint = app.session.next_untitled_title();
        text(ui, d, "name", tr(app, "Document Name"), Some(hint));
        save_preset_control(app, ui, d);
    });
    let u = units(d);
    // The page: two columns of captioned controls, each caption above its control, as the native dialog set them.
    columns(ui, "new_document_page", |ui| {
        cell(ui, tr(app, "Width"), |ui| {
            measure_text(ui, d, "width", tr(app, "Width"));
        });
        cell(ui, tr(app, "Units"), |ui| {
            // InDesign's eight; a preset in another unit keeps it on the list.
            let entries: Vec<Entry<Unit>> = NEW_DOCUMENT_UNITS
                .into_iter()
                .chain((!NEW_DOCUMENT_UNITS.contains(&u)).then_some(u))
                .map(|v| Entry::new(v, tr(app, v.label())))
                .collect();
            let out = Select::new(tr(app, "Units"), Bind::given(u), &entries).id_salt("new_document_units").show(ui);
            if let Some(next) = out.events.committed().copied().filter(|next| *next != u) {
                set_units(d, next);
            }
        });
        cell(ui, tr(app, "Height"), |ui| {
            measure_text(ui, d, "height", tr(app, "Height"));
        });
        cell(ui, tr(app, "Orientation"), |ui| {
            let portrait = !matches!((measure(d, "width"), measure(d, "height")), (Some(w), Some(h)) if w > h);
            let entries = [
                Entry::new(true, tr(app, "Portrait")).icon(IconSpec::glyph(&PORTRAIT)),
                Entry::new(false, tr(app, "Landscape")).icon(IconSpec::glyph(&LANDSCAPE)),
            ];
            let out =
                Segmented::new(tr(app, "Orientation"), Bind::given(Mixed::Same(portrait)), &entries).id_salt("new_document_orientation").show(ui);
            if out.events.committed().is_some_and(|next| *next != portrait)
                && let (Some(w), Some(h)) = (d.fields.get("width").cloned(), d.fields.get("height").cloned())
            {
                d.fields.insert("width".into(), h);
                d.fields.insert("height".into(), w);
            }
        });
        cell(ui, tr(app, "Pages"), |ui| count_field(ui, d, "pages", tr(app, "Pages")));
        check(ui, d, "facingPages", tr(app, "Facing Pages"));
        cell(ui, tr(app, "Start #"), |ui| count_field(ui, d, "startPage", tr(app, "Start #")));
        check(ui, d, "primaryTextFrame", tr(app, "Primary Text Frame"));
    });
    Divider::horizontal().show(ui);
    columns(ui, "new_document_columns", |ui| {
        cell(ui, tr(app, "Columns"), |ui| count_field(ui, d, "columns", tr(app, "Columns")));
        cell(ui, tr(app, "Column Gutter"), |ui| {
            measure_text(ui, d, "gutter", tr(app, "Column Gutter"));
        });
    });
    // The edges sit indented under their disclosure headings, as the native dialog set them.
    Section::new(tr(app, "Margins")).id_salt("new_document_margins").show(ui, |ui, _| {
        Stack::new().id_salt("new_document_margin_edges").indent(1).show(ui, |ui, _| edges(app, ui, d, "margin"));
    });
    Section::new(tr(app, "Bleed and Slug")).id_salt("new_document_bleed_slug").show(ui, |ui, _| {
        Stack::new().id_salt("new_document_bleed_slug_edges").indent(1).gap(Gap::Item).show(ui, |ui, _| {
            Text::new(tr(app, "Bleed")).shade(Shade::Muted).show(ui);
            edges(app, ui, d, "bleed");
            Text::new(tr(app, "Slug")).shade(Shade::Muted).show(ui);
            edges(app, ui, d, "slug");
        });
    });
}

#[cfg(test)]
mod tests {
    use super::super::params;
    use crate::DesignApp;
    use crate::theme::Tokens;
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use serde_json::{Value, json};

    /// New Document open over an installed framework, in English, shown by the app's dialog router every frame.
    fn harness() -> Harness<'static, DesignApp> {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("app.newDocumentDialog", json!({})).unwrap();
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        // Frames a sixtieth of a second apart, as the shipping window's: the harness's default quarter second puts the
        // two clicks of a double click further apart than egui's double-click delay.
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, DesignApp>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, DesignApp>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn press(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
    }

    fn click_at(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
        event(h, Event::PointerMoved(at));
        press(h, at);
        steps(h, 3);
    }

    fn click(h: &mut Harness<'_, DesignApp>, role: Role, label: &str) {
        let at = h.get_by_role_and_label(role, label).rect().center();
        click_at(h, at);
    }

    fn key(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        steps(h, 3);
    }

    fn type_into(h: &mut Harness<'_, DesignApp>, label: &str, text: &str) {
        click(h, Role::TextInput, label);
        key(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(text.to_owned()));
        steps(h, 3);
    }

    fn draft<'h>(h: &'h Harness<'_, DesignApp>) -> &'h crate::dialogs::Dialog {
        h.state().ui.dialog.as_ref().expect("New Document is open")
    }

    /// The centre of the preset card named `name`: its tile, whose accessible name is the name then its size
    /// (`{name}, {details}`), not the details' text of the same words.
    fn card(h: &Harness<'_, DesignApp>, name: &str) -> egui::Pos2 {
        let joined = format!("{name}, ");
        let tile = h
            .query_all_by(|node| {
                !matches!(node.role(), Role::Label | Role::Tab) && node.label().is_some_and(|label| label == name || label.starts_with(&joined))
            })
            .next()
            .expect("a card of that name");
        tile.rect().center()
    }

    fn journal(h: &Harness<'_, DesignApp>, command: &str) -> Vec<Value> {
        h.state().session.journal.iter().filter(|(id, _)| id == command).map(|(_, payload)| payload.clone()).collect()
    }

    /// A tab shows its first preset, keeping the typed name, and Create makes exactly the document the details
    /// describe, through the same `file.new` the browser window runs.
    #[test]
    fn framework_tab_shows_its_first_preset_and_create_makes_that_document() {
        let mut h = harness();
        type_into(&mut h, "Document Name", "Poster");
        click(&mut h, Role::Tab, "Web");
        let d = draft(&h);
        assert_eq!((d.s("tab"), d.s("preset")), ("web".into(), "Web 1920 \u{d7} 1080".into()));
        assert_eq!((d.s("width"), d.s("height")), ("1920 px".into(), "1080 px".into()), "Web is in pixels");
        assert_eq!(d.s("name"), "Poster", "the typed name stays");
        let expected = params(d);
        assert!(journal(&h, "file.new").is_empty(), "picking a tab creates nothing");
        click(&mut h, Role::Button, "Create");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(journal(&h, "file.new"), vec![expected]);
        assert_eq!(h.state().session.active().unwrap().doc.title, "Poster");
    }

    /// A double-clicked card creates its document at once; a single click only fills the details.
    #[test]
    fn framework_double_clicked_card_creates_its_document_at_once() {
        let mut h = harness();
        click(&mut h, Role::Tab, "Print");
        let at = card(&h, "Letter");
        click_at(&mut h, at);
        assert_eq!(draft(&h).s("preset"), "Letter");
        assert!(journal(&h, "file.new").is_empty(), "a click fills the details only");
        let expected = params(draft(&h));
        event(&mut h, Event::PointerMoved(at));
        press(&mut h, at);
        press(&mut h, at);
        steps(&mut h, 3);
        assert!(h.state().ui.dialog.is_none(), "the double click created the document");
        assert_eq!(journal(&h, "file.new"), vec![expected]);
    }

    /// Preview in the footer shows a document with the settings; Close removes it and leaves no document.
    #[test]
    fn framework_preview_shows_the_settings_and_close_removes_them() {
        let mut h = harness();
        assert!(h.state().session.new_document_preview().is_none());
        click(&mut h, Role::CheckBox, "Preview");
        steps(&mut h, 3);
        assert!(draft(&h).b("preview"));
        assert!(h.state().session.new_document_preview().is_some(), "the preview document shows");
        click(&mut h, Role::Button, "Close");
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().session.new_document_preview().is_none());
        assert!(h.state().session.documents().is_empty(), "closing the start window's preview leaves no document");
        assert!(journal(&h, "file.new").is_empty());
    }

    /// Enter in the preset's name saves the preset and shows it on Saved; the dialog stays open.
    #[test]
    fn framework_save_preset_popover_saves_by_name_and_keeps_the_dialog() {
        let mut h = harness();
        click(&mut h, Role::Button, "Save Document Preset");
        type_into(&mut h, "Preset Name", "Flyer");
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(journal(&h, "file.savePreset").len(), 1);
        assert!(h.state().session.prefs.document_presets.iter().any(|p| p.title == "Flyer"));
        let d = draft(&h);
        assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Flyer".into()));
        assert!(journal(&h, "file.new").is_empty(), "saving a preset creates no document");
    }
}
