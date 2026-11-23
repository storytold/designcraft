#![cfg(not(target_arch = "wasm32"))]

//! What the framework tests of the Properties panel and its small panels share: a document, a panel drawn under the
//! installed framework, and real pointer and key events found through AccessKit.

use crate::DesignApp;
use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, vec2};
use egui_kittest::kittest::NodeT as _;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::{Value, json};

pub(super) type Window = Harness<'static, DesignApp>;

/// A new document, with the interface in English.
pub(super) fn document() -> DesignApp {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = String::new();
    app.run("file.new", json!({"title": "Framework panels"})).expect("a new document");
    app
}

/// A selected text frame holding `text`, with the caret's story selected whole; its id and story.
pub(super) fn text_frame(app: &mut DesignApp, text: &str) -> (u64, u64) {
    let f = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": text})).expect("a text frame");
    let (id, story) = (f["id"].as_u64().expect("an id"), f["story"].as_u64().expect("a story"));
    app.run("selection.set", json!({"ids": [id]})).expect("the frame is selected");
    (id, story)
}

/// Selects the first `len` characters of `story` with the Type tool's text selection, so the Properties panel shows
/// the text's sections (Appearance, Bullets and Numbering, Text Frame) as it does while editing text.
pub(super) fn select_text(app: &mut DesignApp, story: u64, len: usize) {
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": len})).expect("the text is selected");
}

/// A selected rectangle; its id.
pub(super) fn rectangle(app: &mut DesignApp) -> u64 {
    let id = app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).expect("a frame")["id"].as_u64().expect("an id");
    app.run("selection.set", json!({"ids": [id]})).expect("the frame is selected");
    id
}

/// `panel` drawn each pass on a context that has the framework's runtime installed.
pub(super) fn open(mut app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui)) -> Window {
    let ctx = egui::Context::default();
    app.framework_transform = false;
    app.framework_opacity = true;
    app.install_framework(&ctx);
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    assert!(app.framework_installed, "the framework bodies need the installed runtime");
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(320.0, 2000.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            panel(app, ui);
            crate::dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    steps(&mut h, 3);
    h
}

pub(super) fn event(h: &mut Window, event: Event) {
    let mut raw = std::mem::take(h.input_mut());
    raw.events.push(event);
    let ctx = h.ctx.clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

pub(super) fn steps(h: &mut Window, count: usize) {
    for _ in 0..count {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
}

pub(super) fn key(h: &mut Window, key: Key) {
    for pressed in [true, false] {
        event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    steps(h, 3);
}

pub(super) fn click_at(h: &mut Window, at: egui::Pos2) {
    event(h, Event::PointerMoved(at));
    for pressed in [true, false] {
        event(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    steps(h, 3);
}

/// Clicks the first control of `role` named `label`.
pub(super) fn click(h: &mut Window, role: Role, label: &str) {
    let at = h.query_all_by_role_and_label(role, label).next().unwrap_or_else(|| panic!("no {role:?} named {label}")).rect().center();
    click_at(h, at);
}

/// Whether a control of `role` named `label` is shown.
pub(super) fn has(h: &Window, role: Role, label: &str) -> bool {
    h.query_all_by_role_and_label(role, label).next().is_some()
}

/// Chooses `option` in the select named `label`: opens it, then clicks the listed entry.
pub(super) fn pick(h: &mut Window, label: &str, option: &str) {
    click(h, Role::ComboBox, label);
    let at = h.query_all_by_label(option).last().unwrap_or_else(|| panic!("{option} is listed")).rect().center();
    click_at(h, at);
}

/// Types `text` over what the field named `label` holds (a number field or a measure's text field), then Enter.
pub(super) fn type_into(h: &mut Window, label: &str, text: &str) {
    let node = h
        .query_all_by(|n| matches!(n.role(), Role::SpinButton | Role::TextInput) && n.label().as_deref() == Some(label))
        .next()
        .unwrap_or_else(|| panic!("no field named {label}"));
    let at = node.rect().center();
    click_at(h, at);
    for pressed in [true, false] {
        event(h, Event::Key { key: Key::A, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND });
    }
    event(h, Event::Text(text.into()));
    event(h, Event::ModifiersChanged(Modifiers::NONE));
    key(h, Key::Enter);
}

/// The commands `command` ran with, oldest first.
pub(super) fn journal(h: &Window, command: &str) -> Vec<Value> {
    h.state().session.journal.iter().filter(|(id, _)| id == command).map(|(_, params)| params.clone()).collect()
}

/// The undo steps the document holds.
pub(super) fn undo_steps(h: &Window) -> usize {
    h.state().session.active().expect("a document").history.undo.len()
}

/// The item `id` of the active document.
pub(super) fn item(h: &Window, id: u64) -> designcraft_doc::Item {
    h.state().session.active().expect("a document").doc.item(designcraft_doc::ItemId(id)).expect("the item").clone()
}

/// A selected graphic frame holding an embedded picture; its id.
pub(super) fn graphic_frame(app: &mut DesignApp) -> u64 {
    use designcraft_doc::{Asset, AssetData, AssetId, Content, Graphic, ItemId, Storage};
    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#4488cc"/></svg>"##;
    let item = ItemId(app.run("frame.create", json!({"rect": [72, 72, 144, 144]})).expect("a frame")["id"].as_u64().expect("an id"));
    let doc = std::sync::Arc::make_mut(&mut app.session.doc_mut().expect("a document").doc);
    let asset = AssetId(doc.alloc());
    doc.assets.insert(
        asset,
        std::sync::Arc::new(Asset {
            id: asset,
            name: "embedded.svg".into(),
            mime: "image/svg+xml".into(),
            link: None,
            storage: Storage::Embedded,
            data: AssetData::embedded(SVG.to_vec()),
            pixels: Some((32, 32)),
            ..Default::default()
        }),
    );
    doc.item_mut(item).expect("the frame").content = Content::Graphic(Graphic {
        layer_options: None,
        asset,
        size: (72.0, 72.0),
        xf: designcraft_geom::Affine::IDENTITY,
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    app.run("selection.set", json!({"ids": [item.0]})).expect("the frame is selected");
    item.0
}

/// The text attributes at the selection, as the panels read them.
pub(super) fn text_attrs(h: &mut Window) -> Value {
    h.state_mut().session.execute("type.selectionAttrs", &json!({})).map(|v| serde_json::to_value(v).expect("json")).expect("text attributes")
}
