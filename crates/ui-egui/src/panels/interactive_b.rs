//! The framework forms of the Interactive panels from Object States to Track Changes.
//!
//! Each panel draws only framework components and collects its commands while drawing; they run once the pass
//! has drawn, so the engine, the selection and Undo see the commands the native panels ran. The native bodies
//! stay in `interactive.rs` for the browser build and for a runtime that is not installed.

use super::{InteractiveDraft, InteractiveFields, LIQUID_RULES, ask_then_run, liquid_rule_label, object_based, track_identity, track_row_name};
use crate::DesignApp;
use craft_behavior::ColumnWidth;
use craft_behavior::{Flow, Metrics, Sense};
use craft_theme::ControlSize;
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
use craft_ui_widgets::contract::{
    Bind, Common, Component, Cx, Entry, MeasureCx, NoValue, Outcome, PaintUi, Pointer as _, Props as _, Role, Scroll, Show as _, ShowContainer as _,
    Sizing, Styled as _,
};
use craft_ui_widgets::display::{Divider, Shade, Text};
use craft_ui_widgets::layout::RowAlign;
use craft_ui_widgets::layout_components::{Grid, Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::primitives::{Span, TextRun, Wrap};
use craft_ui_widgets::style::{ComponentStyle, FocusLook, Stateful, States, StyleColor, StyleCx};
use craft_ui_widgets::style::{TypeRole, Variant};
use craft_ui_widgets::text::{NumberField, TextField};
use egui::Ui;
use serde_json::{Value, json};

/// The check boxes of a constraint group stand in two columns, as wide as their captions.
const CONSTRAINT_COLUMNS: [ColumnWidth; 2] = [ColumnWidth::Content, ColumnWidth::Content];

/// The room a long list gets before it scrolls.
const LIST_ROOM: f32 = 260.0;

/// `text` in the interface language.
fn tr(app: &DesignApp, text: &str) -> String {
    crate::i18n::tr(&app.ui.language, text).to_string()
}

/// A line of small muted text across the width of its column: a hint under the controls.
fn hint(ui: &mut Ui, salt: &'static str, text: String) {
    Text::new(text).id_salt(salt).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
}

/// A caption over the controls it names.
fn caption(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, text: String) {
    Text::new(text).id_salt(salt).role(TypeRole::SectionCaption).show(ui);
}

/// A push button; it reports a click only where the panel may edit.
fn action_button(ui: &mut Ui, controls: &InteractiveFields, key: &str, label: &str, enabled: bool, small: bool) -> bool {
    let enabled = enabled && InteractiveFields::editable(ui);
    let button = Button::new(label.to_owned()).id(controls.id(key)).enabled(enabled);
    let button = if small { button.size(ControlSize::Sm) } else { button.sizing(Sizing::FillWidth) };
    button.show(ui).clicked() && enabled
}

/// A menu of `options` named `name`: the option committed this pass, if it is not a refused one.
fn choice(ui: &mut Ui, controls: &InteractiveFields, key: &str, name: &str, current: usize, options: &[String]) -> Option<usize> {
    let enabled = InteractiveFields::editable(ui);
    let entries: Vec<_> = options.iter().enumerate().map(|(i, label)| Entry::new(i, label.clone())).collect();
    let out = Select::new(name.to_owned(), Bind::given(current), &entries).id(controls.id(key)).enabled(enabled).sizing(Sizing::FillWidth).show(ui);
    out.events.committed().copied().filter(|i| enabled && *i < options.len())
}

/// A text field over a draft; it reports where its edit ends. Enter, Tab and Escape end the draft without a
/// rollback, as the text editor it replaces did.
fn text_field(ui: &mut Ui, controls: &InteractiveFields, key: &str, name: &str, draft: &mut String) -> bool {
    let enabled = InteractiveFields::editable(ui);
    let out = TextField::new(name.to_owned(), Bind::given(draft.clone()))
        .id(controls.id(key))
        .enabled(enabled)
        .limit(None::<usize>)
        .keeps_on_escape()
        .sizing(Sizing::FillWidth)
        .show(ui);
    if enabled && let Some(value) = out.value() {
        *draft = value.clone();
    }
    enabled && out.events.committed().is_some()
}

/// A check box at `id`; it reports a toggle only where the panel may edit.
fn check_at(ui: &mut Ui, id: egui::Id, name: &str, value: &mut bool, enabled: bool) -> bool {
    let enabled = enabled && InteractiveFields::editable(ui);
    let before = *value;
    Checkbox::new(Bind::bound(value)).id(id).label(name.to_owned()).enabled(enabled).show(ui);
    enabled && *value != before
}

/// Object States: the states of the selected multi-state object, one to show, and Release to Objects.
pub(super) fn states(app: &mut DesignApp, ui: &mut Ui) {
    let n = app.session.active().map_or(0, |d| d.selection.items.len());
    let info = app.session.execute("states.list", &json!({})).ok();
    let list = info.as_ref().and_then(|i| i["states"].as_array().cloned()).unwrap_or_default();
    let controls = InteractiveFields::new(app, ui, ("states", list.iter().map(|v| v.as_str().unwrap_or("")).collect::<Vec<_>>()));
    let active = info.as_ref().and_then(|i| i["active"].as_u64()).unwrap_or(0) as usize;
    let (empty, create_label, group, release_label) = (
        tr(app, "Select two or more objects (or a group) to make a multi-state object."),
        tr(app, "Convert Selection to Multi-State Object"),
        tr(app, "Object States"),
        tr(app, "Release to Objects"),
    );
    let (mut create, mut picked, mut release) = (false, None, false);
    Stack::new().id_salt("interactive_states").fill_width().show(ui, |ui, _| {
        if list.is_empty() {
            hint(ui, "states_empty", empty.clone());
            create = action_button(ui, &controls, "create", &create_label, n > 0, false);
            return;
        }
        let out = RadioGroup::new(group.clone(), Bind::given(active)).id(controls.id("states")).enabled(InteractiveFields::editable(ui)).show(
            ui,
            |ui, _| {
                Stack::new().id_salt("states_list").show(ui, |ui, _| {
                    for (i, state) in list.iter().enumerate() {
                        let name = format!("{group}: {} ({})", state.as_str().unwrap_or(""), i + 1);
                        Radio::new(i).id(controls.id("state").with(i)).label(name).show(ui);
                    }
                });
            },
        );
        picked = out.output.events.committed().copied();
        Divider::horizontal().show(ui);
        release = action_button(ui, &controls, "release", &release_label, true, true);
    });
    if create && let Err(e) = app.run("states.create", json!({})) {
        app.status_error("Object States", e);
    }
    if let Some(i) = picked.filter(|i| *i < list.len() && InteractiveFields::editable(ui)) {
        let _ = app.run("states.show", json!({"index": i}));
    }
    if release {
        let _ = app.run("states.release", json!({}));
    }
}

/// Buttons and Forms: the selected objects' On Release action and form field. The drafts of the URL and of the
/// form field belong to the document, the selected objects and the kind they were typed under.
pub(super) fn buttons(app: &mut DesignApp, ui: &mut Ui) {
    use designcraft_doc::{ButtonAction as B, FieldKind as K};

    let url_key = ui.make_persistent_id("interactive-button-url-draft");
    let form_key = ui.make_persistent_id("interactive-form-draft");
    let Some(id) = app.session.active().and_then(|d| d.selection.items.first().copied()) else {
        if InteractiveFields::editable(ui) {
            ui.data_mut(|data| {
                data.remove::<InteractiveDraft<String>>(url_key);
                data.remove::<InteractiveDraft<(String, String, String)>>(form_key);
            });
        }
        let empty = tr(app, "Select an object to make it a button.");
        Stack::new().id_salt("interactive_buttons_empty").fill_width().show(ui, |ui, _| hint(ui, "buttons_empty", empty.clone()));
        return;
    };
    let cur = app.session.active().and_then(|d| d.doc.item(id)).and_then(|it| it.button.clone());
    let (kind, page, url) = match &cur {
        None => ("none", 0, String::new()),
        Some(B::GoToPage { page }) => ("page", *page, String::new()),
        Some(B::GoToFirstPage) => ("firstPage", 0, String::new()),
        Some(B::GoToLastPage) => ("lastPage", 0, String::new()),
        Some(B::GoToNextPage) => ("nextPage", 0, String::new()),
        Some(B::GoToPreviousPage) => ("previousPage", 0, String::new()),
        Some(B::GoToUrl { url }) => ("url", 0, url.clone()),
    };
    let controls = InteractiveFields::new(app, ui, ("button", kind));
    let labels = [
        ("none", "Not a button"),
        ("nextPage", "Go To Next Page"),
        ("previousPage", "Go To Previous Page"),
        ("firstPage", "Go To First Page"),
        ("lastPage", "Go To Last Page"),
        ("page", "Go To Page"),
        ("url", "Go To URL"),
    ];
    let options: Vec<_> = labels.iter().map(|(_, label)| tr(app, label)).collect();
    let total = app.session.active().map_or(1, |d| d.doc.page_count()).max(1);
    let ff = app.session.active().and_then(|d| d.doc.item(id)).and_then(|it| it.form_field.clone());
    let kinds = [
        (None, "Not a form field"),
        (Some(K::TextField), "Text Field"),
        (Some(K::CheckBox), "Check Box"),
        (Some(K::ComboBox), "Combo Box"),
        (Some(K::ListBox), "List Box"),
        (Some(K::Signature), "Signature Field"),
    ];
    let form_kind = ff.as_ref().map(|f| f.kind);
    let form_controls = InteractiveFields::new(app, ui, ("form", form_kind.map(|kind| kind as u8)));
    let form_options: Vec<_> = kinds.iter().map(|(_, label)| tr(app, label)).collect();
    let mut url_draft = (kind == "url").then(|| InteractiveDraft::read(ui, url_key, controls.identity, url.clone()));
    let mut form_draft =
        ff.as_ref().map(|f| InteractiveDraft::read(ui, form_key, form_controls.identity, (f.name.clone(), f.value.clone(), f.options.join(", "))));

    let (mut action_pick, mut page_pick, mut url_commit, mut kind_pick) = (None, None, false, None);
    let (mut form_sets, mut commit) = (Vec::<Value>::new(), false);
    Stack::new().id_salt("interactive_buttons").fill_width().show(ui, |ui, _| {
        caption(ui, "on_release", tr(app, "On Release"));
        action_pick = choice(ui, &controls, "action", &tr(app, "On Release Action"), labels.iter().position(|l| l.0 == kind).unwrap_or(0), &options);
        if kind == "page" {
            Row::new().id_salt("button_page").fill_width().show(ui, |ui, _| {
                Text::new(tr(app, "Page")).id_salt("button_page_caption").show(ui);
                let enabled = InteractiveFields::editable(ui);
                let out = NumberField::new(tr(app, "Button target page"), Bind::given(Some((page + 1) as f64)))
                    .id(controls.id("page"))
                    .enabled(enabled)
                    .range(1.0..=(total as f64))
                    .step(1.0)
                    .snap_typed(true)
                    .fixed_decimals(0)
                    .commit_decimals(0)
                    .sizing(Sizing::FillWidth)
                    .show(ui);
                if enabled
                    && let Some(Some(n)) = out.edit.events.committed().copied()
                    && n.is_finite()
                {
                    let shown = (n.round().max(1.0) as usize).min(total);
                    if shown != page + 1 {
                        page_pick = Some(shown);
                    }
                }
            });
        }
        if let Some(draft) = url_draft.as_mut() {
            url_commit = text_field(ui, &controls, "url", &tr(app, "Button URL"), &mut draft.value);
        }
        hint(ui, "buttons_note", tr(app, "Buttons act in interactive PDF export."));
        Divider::horizontal().show(ui);
        caption(ui, "form_field", tr(app, "Form Field"));
        kind_pick =
            choice(ui, &form_controls, "kind", &tr(app, "Form Field Type"), kinds.iter().position(|k| k.0 == form_kind).unwrap_or(0), &form_options);
        if kind_pick.is_some() {
            // The old fields may lose focus while the kind is picked. Their pending transaction belongs to the old kind.
            return;
        }
        let (Some(f), Some(draft)) = (&ff, form_draft.as_mut()) else { return };
        let (name, value, opts) = &mut draft.value;
        caption(ui, "form_name", tr(app, "Name"));
        commit |= text_field(ui, &form_controls, "name", &tr(app, "Form field name"), name);
        if f.kind == K::CheckBox {
            let mut on = !f.value.is_empty() && f.value != "Off";
            if check_at(ui, form_controls.id("checked"), &tr(app, "Checked by default"), &mut on, true) {
                form_sets.push(json!({"kind": f.kind, "value": if on { "On" } else { "" }}));
            }
        } else if f.kind != K::Signature {
            caption(ui, "form_default", tr(app, "Default"));
            commit |= text_field(ui, &form_controls, "default", &tr(app, "Form field default value"), value);
        }
        if matches!(f.kind, K::ComboBox | K::ListBox) {
            caption(ui, "form_choices", tr(app, "Choices"));
            commit |= text_field(ui, &form_controls, "choices", &tr(app, "Form field choices (separated by commas)"), opts);
        }
        let mut required = f.required;
        if check_at(ui, form_controls.id("required"), &tr(app, "Required"), &mut required, true) {
            form_sets.push(json!({"kind": f.kind, "required": required}));
        }
        if f.kind == K::TextField {
            let mut multiline = f.multiline;
            if check_at(ui, form_controls.id("multiline"), &tr(app, "Multiline"), &mut multiline, true) {
                form_sets.push(json!({"kind": f.kind, "multiline": multiline}));
            }
        }
    });

    let editable = InteractiveFields::editable(ui);
    let mut params = action_pick
        .and_then(|i| labels.get(i))
        .map(|(k, _)| json!({"action": k, "page": page, "url": if url.is_empty() { "https://" } else { &url }}));
    if let Some(shown) = page_pick {
        params = Some(json!({"action": "page", "page": shown - 1}));
    }
    if url_commit
        && action_pick.is_none()
        && let Some(draft) = url_draft.as_ref().filter(|draft| draft.value != url)
    {
        params = Some(json!({"action": "url", "url": draft.value.clone()}));
    }
    if editable {
        ui.data_mut(|data| match url_draft {
            Some(draft) => data.insert_temp(url_key, draft),
            None => {
                data.remove::<InteractiveDraft<String>>(url_key);
            }
        });
    }
    if let Some(p) = params
        && let Err(e) = app.run("button.set", p)
    {
        app.status_error("Buttons", e);
    }
    if let Some(i) = kind_pick
        && let Some((k, _)) = kinds.get(i)
    {
        let _ = match k {
            Some(k) => app.run("form.set", json!({"kind": k})),
            None => app.run("form.clear", json!({})),
        };
        ui.data_mut(|data| data.remove::<InteractiveDraft<(String, String, String)>>(form_key));
        return;
    }
    let (Some(f), Some(draft)) = (ff, form_draft) else {
        if editable {
            ui.data_mut(|data| data.remove::<InteractiveDraft<(String, String, String)>>(form_key));
        }
        return;
    };
    for params in form_sets {
        let _ = app.run("form.set", params);
    }
    if commit {
        let (name, value, opts) = &draft.value;
        let options: Vec<String> = opts.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
        let _ = app.run("form.set", json!({"kind": f.kind, "name": name, "value": value, "options": options}));
        ui.data_mut(|data| data.remove::<InteractiveDraft<(String, String, String)>>(form_key));
    } else if editable {
        ui.data_mut(|data| data.insert_temp(form_key, draft));
    }
}

/// The Liquid Page Rule menu: the rule committed this pass when it differs from the page's own.
fn rule_select(ui: &mut Ui, id: egui::Id, rule: designcraft_doc::LiquidRule, lang: &str) -> Option<designcraft_doc::LiquidRule> {
    let enabled = InteractiveFields::editable(ui);
    let current = LIQUID_RULES.iter().position(|(r, _)| *r == rule).unwrap_or(usize::MAX);
    let entries: Vec<_> = LIQUID_RULES.iter().enumerate().map(|(i, (_, text))| Entry::new(i, crate::i18n::tr(lang, text).to_string())).collect();
    let out = Select::new(crate::i18n::tr(lang, "Liquid Page Rule").to_string(), Bind::given(current), &entries)
        .id(id)
        .enabled(enabled)
        .placeholder(crate::i18n::tr(lang, liquid_rule_label(rule)).to_string())
        .sizing(Sizing::FillWidth)
        .show(ui);
    // Proposed entries are not page edits; Escape must leave the real rule and Undo alone.
    out.events.committed().and_then(|i| LIQUID_RULES.get(*i)).map(|(r, _)| *r).filter(|r| enabled && *r != rule)
}

/// Liquid Layout: the page's Liquid Page Rule, Content › Auto-Fit and Object Constraints for the selected object
/// on an object-based page, then the page's liquid guides.
pub(super) fn liquid(app: &mut DesignApp, ui: &mut Ui) {
    use designcraft_doc::{Content, Fitting, ObjectLiquid};

    let lang = app.ui.language.clone();
    let t = |text: &str| crate::i18n::tr(&lang, text).to_string();
    // The Page tool's page, the page in view, or (before the canvas lays out) the first.
    let Some(page) = app.session.active().map(|s| s.selection.page.or_else(|| crate::canvas::current_page(app)).unwrap_or(0)) else {
        Stack::new().id_salt("interactive_liquid_empty").fill_width().show(ui, |ui, _| hint(ui, "liquid_empty", t("No document.")));
        return;
    };
    let Some(d) = app.session.active().map(|d| d.doc.clone()) else { return };
    let Some((si, pi)) = d.page_loc(page) else { return };
    let pg = d.spreads[si].pages[pi].clone();
    let document = app.session.active().map(|st| (st.uid, st.identity_epoch));
    let targets = app.session.active().map(|st| st.selection.items.iter().map(|id| id.0).collect::<Vec<_>>());
    let inherited = d.parent_page_for(page).and_then(|(si, pi)| d.parents.get(si)?.pages.get(pi).map(|pg| (pg.id.0, pg.liquid as u8)));
    // Page IDs survive reordering; indices still bind the exact existing command payload.
    let identity = ui.make_persistent_id(("liquid-workflow", document, (page, si, pi, pg.id.0), targets, pg.liquid as u8, inherited));
    // The selected object (one on this page).
    let item = app
        .session
        .active()
        .and_then(|s| s.selection.items.first().copied())
        .and_then(|id| d.item(id))
        .filter(|it| d.page_of_item(it.id) == Some(page));
    let graphic = item.and_then(|it| if let Content::Graphic(g) = &it.content { Some(g.auto_fit) } else { None });
    let mut auto = graphic.is_some_and(|f| f != Fitting::None);
    let on = item.is_some() && object_based(&d, page);
    let l = item
        .filter(|_| on)
        .map(|it| it.liquid.unwrap_or(ObjectLiquid { resize_width: true, resize_height: true, ..Default::default() }))
        .unwrap_or_default();
    let groups: [(&str, Vec<(&str, &str, bool)>); 2] = [
        ("Resize with Page:", vec![("Height", "resizeHeight", l.resize_height), ("Width", "resizeWidth", l.resize_width)]),
        (
            "Pin:",
            vec![
                ("Top", "pinTop", l.pin_top),
                ("Left", "pinLeft", l.pin_left),
                ("Bottom", "pinBottom", l.pin_bottom),
                ("Right", "pinRight", l.pin_right),
            ],
        ),
    ];
    // Guides have no persistent ID. Bind to the complete structural row sequence,
    // raw coordinates and page identity, not a rounded caption or a shifted index.
    let rows: Vec<_> = pg.guides.iter().map(|g| (g.orientation as u8, g.position.to_bits(), g.spread, g.locked, g.layer.map(|id| id.0))).collect();
    let guide_identity = identity.with(("guides", rows));

    let mut rule_pick = None;
    let mut commands: Vec<(&'static str, Value)> = Vec::new();
    Stack::new().id_salt(("interactive_liquid", document)).fill_width().show(ui, |ui, _| {
        Row::new().id_salt("liquid_rule_header").fill_width().align(RowAlign::SpaceBetween).show(ui, |ui, _| {
            caption(ui, "liquid_rule_caption", t("Liquid Page Rule:"));
            caption(ui, "liquid_rule_page", format!("{} {}", t("Page:"), d.page_name(page)));
        });
        rule_pick = rule_select(ui, identity.with("rule"), pg.liquid, &lang);
        Divider::horizontal().show(ui);
        caption(ui, "liquid_content_caption", t("Content:"));
        Stack::new().id_salt("liquid_content").indent(1).show(ui, |ui, _| {
            if check_at(ui, identity.with("auto-fit"), &t("Auto-Fit"), &mut auto, graphic.is_some()) {
                commands.push(("object.fittingOptions", json!({"autoFit": auto})));
            }
        });
        caption(ui, "liquid_constraints_caption", t("Object Constraints:"));
        Stack::new().id_salt("liquid_constraints").indent(1).show(ui, |ui, _| {
            for (title, boxes) in &groups {
                let title: &str = title;
                caption(ui, ("liquid_group", title), t(title));
                Grid::new(&CONSTRAINT_COLUMNS).id_salt(("liquid_grid", title)).show(ui, |ui, _| {
                    for &(label, key, value) in boxes {
                        let mut v = value;
                        if check_at(ui, identity.with(("constraint", key)), &t(label), &mut v, on) {
                            commands.push(("liquid.object", json!({key: v})));
                        }
                    }
                });
            }
        });
        if pg.guides.is_empty() {
            return;
        }
        Divider::horizontal().show(ui);
        hint(ui, "liquid_guides", t("Liquid guides"));
        for (gi, g) in pg.guides.iter().enumerate() {
            let mut on = g.liquid;
            let label = format!("{} {}: {:?} at {} ({} {})", t("Liquid guide"), gi + 1, g.orientation, g.position, t("Page"), d.page_name(page));
            if check_at(ui, guide_identity.with(gi), &label, &mut on, true) {
                commands.push(("guide.liquid", json!({"spread": si, "page": pi, "index": gi, "on": on})));
            }
        }
    });
    if let Some(rule) = rule_pick {
        let _ = app.run("liquid.pageRule", json!({"rule": rule, "pages": [page + 1]}));
    }
    for (command, params) in commands {
        let _ = app.run(command, params);
    }
}

/// Media: the selected video or sound's playback options and poster.
pub(super) fn media(app: &mut DesignApp, ui: &mut Ui) {
    let Ok(m) = app.session.execute("media.get", &json!({})) else {
        let empty = tr(app, "Select a placed video or sound (File › Place a .mp4, .mov, .mp3, .wav …).");
        Stack::new().id_salt("interactive_media_empty").fill_width().show(ui, |ui, _| hint(ui, "media_empty", empty.clone()));
        return;
    };
    let identity = app.session.active().map(|st| (st.uid, st.identity_epoch));
    let title = format!("{} — {}", m["kind"].as_str().unwrap_or(""), m["name"].as_str().unwrap_or(""));
    let options = [("playOnPageLoad", "Play on Page Load"), ("loop", "Loop"), ("controls", "Show Controls")];
    let has_poster = !m["poster"].is_null();
    let editable = InteractiveFields::editable(ui);
    let (mut commands, mut choose, mut clear) = (Vec::<(&str, bool)>::new(), false, false);
    Stack::new().id_salt(("interactive_media", identity)).fill_width().show(ui, |ui, _| {
        Text::new(title.clone()).id_salt("media_title").role(TypeRole::BodyStrong).show(ui);
        for (key, label) in options {
            let on = m[key].as_bool().unwrap_or(false);
            let out = Checkbox::new(Bind::given(on)).id_salt(("media_option", key)).label(tr(app, label)).enabled(editable).show(ui);
            if let Some(next) = out.events.committed().copied().filter(|next| *next != on) {
                commands.push((key, next));
            }
        }
        Row::new().id_salt("media_poster").fill_width().show(ui, |ui, _| {
            Text::new(tr(app, "Poster")).id_salt("media_poster_caption").show(ui);
            choose = Button::new(tr(app, "Choose Image…")).id_salt("media_poster_choose").enabled(editable).show(ui).clicked();
            if has_poster {
                clear = Button::new(tr(app, "None")).id_salt("media_poster_none").size(ControlSize::Sm).enabled(editable).show(ui).clicked();
            }
        });
        hint(ui, "media_note", tr(app, "Plays in EPUB export; print and PDF show the poster."));
    });
    for (key, on) in commands {
        let _ = app.run("media.options", json!({key: on}));
    }
    if choose && editable {
        ask_then_run(app, "place", "Media", |path| ("media.options", json!({"poster": {"path": path}})));
    }
    if clear && editable {
        let _ = app.run("media.options", json!({"poster": null}));
    }
}

/// Page Transitions: the current spread's transition for interactive PDF.
pub(super) fn transitions(app: &mut DesignApp, ui: &mut Ui) {
    use designcraft_doc::TransitionKind as K;

    let Some(page) = crate::canvas::current_page(app) else { return };
    let Some(d) = app.session.active().map(|d| d.doc.clone()) else { return };
    let Some((si, _)) = d.page_loc(page) else { return };
    let cur = d.spreads[si].pages.first().and_then(|p| p.transition);
    let identity = app.session.active().map(|st| (st.uid, st.identity_epoch));
    let mut kind = cur.map(|c| c.kind);
    let mut duration = cur.map_or(1.0, |c| c.duration);
    let mut horizontal = cur.is_some_and(|c| c.horizontal);
    // Entry 0 is "None"; entry n + 1 is `K::ALL[n]`.
    let mut entries = vec![Entry::new(0usize, tr(app, "None"))];
    entries.extend(K::ALL.iter().enumerate().map(|(i, k)| Entry::new(i + 1, tr(app, k.label()))));
    let current = kind.and_then(|k| K::ALL.iter().position(|a| *a == k)).map_or(0, |i| i + 1);
    let editable = InteractiveFields::editable(ui);
    let (mut changed, mut all) = (false, false);
    Stack::new().id_salt(("interactive_transitions", identity)).fill_width().show(ui, |ui, _| {
        Text::new(format!("{} {}", tr(app, "Spread"), si + 1)).id_salt("transition_spread").role(TypeRole::BodyStrong).show(ui);
        let out = Select::new(tr(app, "Transition"), Bind::given(current), &entries)
            .id_salt("transition_kind")
            .enabled(editable)
            .sizing(Sizing::FillWidth)
            .show(ui);
        if let Some(&picked) = out.events.committed().filter(|picked| **picked != current) {
            kind = picked.checked_sub(1).and_then(|i| K::ALL.get(i)).copied();
            changed = editable;
        }
        if kind.is_some() {
            Row::new().id_salt("transition_speed").fill_width().show(ui, |ui, _| {
                Text::new(tr(app, "Speed")).id_salt("transition_speed_caption").show(ui);
                let out = NumberField::new(tr(app, "Speed"), Bind::given(Some(duration)))
                    .id_salt("transition_speed_field")
                    .enabled(editable)
                    .range(0.1..=10.0)
                    .step(0.1)
                    .unit(" s")
                    .sizing(Sizing::FillWidth)
                    .show(ui);
                if let Some(Some(next)) = out.edit.events.committed().copied().filter(|_| editable)
                    && next.is_finite()
                    && next != duration
                {
                    duration = next;
                    changed = true;
                }
            });
            if matches!(kind, Some(K::Blinds | K::Box | K::Split)) {
                let label = tr(app, if kind == Some(K::Box) { "Inward" } else { "Horizontal" });
                let out = Checkbox::new(Bind::given(horizontal)).id_salt("transition_horizontal").label(label).enabled(editable).show(ui);
                if let Some(&next) = out.events.committed().filter(|next| **next != horizontal) {
                    horizontal = next;
                    changed |= editable;
                }
            }
        }
        all = Button::new(tr(app, "Apply to All Spreads")).id_salt("transition_all").enabled(editable).show(ui).clicked();
        hint(ui, "transition_note", tr(app, "Transitions play in interactive PDF (full-screen mode)."));
    });
    let params = |all: bool| {
        let k = kind.map_or(json!("none"), |k| json!(k));
        if all {
            json!({"all": true, "kind": k, "duration": duration, "horizontal": horizontal})
        } else {
            json!({"spread": si, "kind": k, "duration": duration, "horizontal": horizontal})
        }
    };
    if changed {
        let _ = app.run("page.transition", params(false));
    }
    if all && editable {
        let _ = app.run("page.transition", params(true));
    }
}

/// The excerpt of a change in Track Changes: its kind and first forty characters, wrapped, struck through when the
/// change is a deletion as the text shows it. A click, Enter or Space shows the change in the text. Its name is the
/// full row name (`label`); `caption` is what it paints.
#[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
struct ChangeExcerpt {
    #[common]
    common: Common,
    caption: String,
    struck: bool,
    /// The context whose fonts lay out the struck caption.
    fonts: egui::Context,
}

/// The look of a change excerpt: caption-size text in the body colour, and the focus ring.
#[derive(Clone, Debug, PartialEq)]
struct ChangeExcerptStyle {
    focus: FocusLook,
    text: Stateful<StyleColor>,
}

impl ComponentStyle for ChangeExcerptStyle {
    fn focus(&self) -> Option<&FocusLook> {
        Some(&self.focus)
    }

    fn metrics(&self) -> Metrics {
        Metrics::new()
    }
}

/// What a change excerpt reports: its response and whether it was activated this pass.
struct ChangeExcerptOutput {
    response: egui::Response,
    clicked: bool,
}

impl Outcome for ChangeExcerptOutput {
    type Value = NoValue;

    fn response(&self) -> &egui::Response {
        &self.response
    }
}

impl ChangeExcerpt {
    fn new(name: String, caption: String, struck: bool, fonts: egui::Context) -> Self {
        ChangeExcerpt { common: Common::new(), caption, struck, fonts }.label(name)
    }

    fn run(&self) -> TextRun<'_> {
        TextRun::spans(vec![Span::new(self.caption.as_str())]).role(TypeRole::Caption).wrap(Wrap::Wrap)
    }
}

impl Component for ChangeExcerpt {
    const ROLE: Role = Role::BUTTON;
    const PARTS: bool = false;
    const OVERLAYS: bool = false;
    type Style = ChangeExcerptStyle;
    type Output = ChangeExcerptOutput;

    fn style(cx: &StyleCx<'_>, _: ChangeExcerptStyleKey) -> ChangeExcerptStyle {
        let colors = &cx.tokens.color;
        let normal = cx.color(colors.text.normal);
        ChangeExcerptStyle {
            focus: cx.focus_look(),
            text: Stateful::new(normal).off(States::new(normal, normal, normal, cx.color(colors.text.disabled))),
        }
    }

    fn sense(&self) -> Sense {
        Sense::CLICK | Sense::FOCUS
    }

    fn measure(&mut self, cx: &MeasureCx<'_, Self>) -> egui::Vec2 {
        cx.layout_at(&self.run(), cx.room.unwrap_or(f32::INFINITY)).size()
    }

    fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
        let color = cx.resolve(&cx.style.text);
        let draw = ui.draw();
        let mut layout = draw.layout(&self.run().color(color), cx.content_rect.width());
        if self.struck {
            // The framework's spans have no strike-through: the caption is laid out again with egui's.
            let mut job = (*layout.galley.job).clone();
            for section in &mut job.sections {
                section.format.strikethrough = egui::Stroke::new(1.0, color.0);
            }
            layout.galley = self.fonts.fonts_mut(|fonts| fonts.layout_job(job));
        }
        let line = Flow::new(cx.content_rect, cx.flow.is_rtl()).take_start(layout.size().x).0;
        draw.galley(egui::Pos2::new(line.left(), cx.content_rect.top()), &layout);
    }

    fn finish(self, cx: Cx<'_, Self>) -> ChangeExcerptOutput {
        ChangeExcerptOutput { response: cx.response.clone(), clicked: cx.activation().is_some() }
    }
}

/// A Track Changes action button: named in full, with its short caption painted.
fn track_action(ui: &mut Ui, id: egui::Id, name: &str, painted: &str, enabled: bool) -> bool {
    Button::new(name.to_owned()).painted(painted.to_owned()).id(id).enabled(enabled).size(ControlSize::Sm).variant(Variant::GHOST).show(ui).clicked()
        && enabled
}

/// Track Changes: tracking on or off, and each change with Accept and Reject.
pub(super) fn track_changes(app: &mut DesignApp, ui: &mut Ui) {
    let Some(on) = app.session.active().map(|d| d.doc.settings.track_changes) else { return };
    let lang = app.ui.language.clone();
    let t = |text: &str| crate::i18n::tr(&lang, text).to_string();
    let identity = track_identity(app);
    let root = ui.make_persistent_id(("track-changes-workflow", identity));
    let editable = InteractiveFields::editable(ui);
    let list = app.session.execute("changes.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let current = track_identity(app);
    let collection = Value::Array(list.clone()).to_string();
    // Full collection + document/replacement/revision, never a preview or row index.
    let actions = ui.make_persistent_id(("track-changes-actions", current, collection));
    let ready = editable && current == identity;
    let mut track = on;
    let mut commands: Vec<(&'static str, Value)> = Vec::new();
    Stack::new().id_salt(("interactive_track", identity.map(|(uid, epoch, _)| (uid, epoch)))).fill_width().show(ui, |ui, _| {
        check_at(ui, root.with("track"), &t("Track Changes in All Stories"), &mut track, true);
        Divider::horizontal().show(ui);
        if list.is_empty() {
            hint(ui, "track_empty", t("No changes."));
            return;
        }
        Stack::new().id_salt("track_changes").fill_width().scroll(Scroll::vertical()).room_height(LIST_ROOM).show(ui, |ui, _| {
            for c in &list {
                let (story, start) = (c["story"].clone(), c["start"].clone());
                let kind = c["kind"].as_str().unwrap_or("");
                let text: String = c["text"].as_str().unwrap_or("").chars().take(40).collect();
                let row = actions.with(("change", c.to_string()));
                let shown = format!("{} “{}”", t(if kind == "inserted" { "Added" } else { "Deleted" }), text.replace('\n', "¶"));
                // The excerpt takes the whole row; its two actions stand under it at the end edge.
                let excerpt = ChangeExcerpt::new(track_row_name(&t("Show in the text"), c), shown, kind == "deleted", ui.ctx().clone())
                    .id(row.with("show"))
                    .enabled(ready)
                    .tooltip(t("Show in the text"))
                    .sizing(Sizing::FillWidth)
                    .show(ui);
                if excerpt.clicked && ready {
                    commands.push(("text.select", json!({"story": story, "anchor": start, "focus": c["end"].clone()})));
                }
                Row::new().id_salt(("track_actions", c.to_string())).fill_width().align(RowAlign::End).show(ui, |ui, _| {
                    for (command, label, key) in [("changes.accept", "Accept", "accept"), ("changes.reject", "Reject", "reject")] {
                        if track_action(ui, row.with(key), &track_row_name(&t(label), c), &t(label), ready) {
                            commands.push((command, json!({"story": story, "start": start})));
                        }
                    }
                });
            }
        });
        Divider::horizontal().show(ui);
        Row::new().id_salt("track_bulk").fill_width().show(ui, |ui, _| {
            for (command, label, key) in [("changes.acceptAll", "Accept All", "accept-all"), ("changes.rejectAll", "Reject All", "reject-all")] {
                if track_action(ui, actions.with(key), &t(label), &t(label), ready) {
                    commands.push((command, json!({})));
                }
            }
        });
    });
    if track != on && editable && track_identity(app) == identity {
        let _ = app.run("changes.track", json!({"on": track}));
    }
    for (command, params) in commands {
        if track_identity(app) == current {
            let _ = app.run(command, params);
        }
    }
}
