use egui::{pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::test_window::{self, wheel};

/// The last tool group's button and the Fill proxy, near the bottom of the panel.
const LAST_TOOL: &str = "Zoom Tool (Z)";
const FILL: &str = "Fill (X)";
/// Status bar height: the Tools panel ends above it.
const STATUS_H: f32 = 17.0;

fn app(double: bool) -> crate::DesignApp {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.tools_double_column = double;
    app
}

fn column_width(cols: f32) -> f32 {
    8.0 + super::BTN * cols + 4.0 * (cols - 1.0) + 8.0
}

fn rect(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> egui::Rect {
    h.get_by_label(label).rect()
}

/// As InDesign's: a tool group's flyout closes on Esc and on a click outside it (the fill
/// swatch, a panel), not only on the canvas.
#[test]
fn a_tool_flyout_closes_on_escape_and_on_a_click_elsewhere() {
    use egui::{Event, Modifiers, PointerButton};
    use egui_kittest::kittest::Queryable;
    let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
    let open = |h: &mut egui_kittest::Harness<'static, crate::test_window::Window>| {
        let at = h.get_by_label("Rectangle Frame Tool (F)").rect().center();
        h.input_mut().events.push(Event::PointerMoved(at));
        for pressed in [true, false] {
            h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        }
        h.run_steps(3);
        assert!(h.state().app.ui.flyout.is_some(), "a right-click opens the flyout");
        let _ = h.get_by_label("Ellipse Frame Tool");
    };
    open(&mut h);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().app.ui.flyout.is_none(), "Esc closes it");
    // A click on a panel closes it.
    open(&mut h);
    crate::test_window::click_at(&mut h, egui::pos2(1300.0, 700.0));
    assert!(h.state().app.ui.flyout.is_none(), "a click elsewhere closes it");
    // Choosing a tool in it closes it and selects the tool.
    open(&mut h);
    h.get_by_label("Ellipse Frame Tool").click();
    h.run_steps(3);
    assert_eq!((h.state().app.session.tool_id(), h.state().app.ui.flyout), ("ellipseFrame", None));
}

/// A flyout opened by holding the group button stays open after the release, and Esc closes
/// it without switching the active tool.
#[test]
fn a_tool_flyout_opened_by_holding_stays_open_and_esc_keeps_the_tool() {
    use egui::{Event, Modifiers, PointerButton};
    use egui_kittest::kittest::Queryable;
    let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let mut h = crate::test_window::open_stepped(app, vec2(1400.0, 900.0), 1.0, 1.0 / 60.0);
    h.state_mut().app.select_tool("rectangleFrame");
    h.run_steps(2);
    let at = h.get_by_label("Rectangle Frame Tool (F)").rect().center();
    h.input_mut().events.push(Event::PointerMoved(at));
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    // Held for half a second: shorter than egui's longest click.
    h.run_steps(30);
    assert!(h.state().app.ui.flyout.is_some(), "holding opens the flyout");
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert!(h.state().app.ui.flyout.is_some(), "the release keeps it open");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!((h.state().app.session.tool_id(), h.state().app.ui.flyout), ("rectangleFrame", None));
}

#[test]
fn a_short_window_reaches_every_tool() {
    for double in [false, true] {
        let (w, ht) = (1000.0, 500.0);
        let mut h = test_window::open(app(double), vec2(w, ht));
        let panel = test_window::panel_rect(&h, "tools");
        wheel(&mut h, panel.center(), vec2(0.0, -2000.0));
        let visible = egui::Rect::from_min_max(pos2(0.0, panel.min.y), pos2(w, ht - STATUS_H));
        for label in [LAST_TOOL, FILL] {
            let r = rect(&h, label);
            assert!(visible.contains_rect(r), "double column {double}: {label} at {r:?} is outside {visible:?}");
        }
        assert_eq!(h.state().app.ui.tools_double_column, double, "the preference is the user's");
    }
}

#[test]
fn tools_take_two_columns_while_one_does_not_fit() {
    let (w, ht) = (1086.0, 612.0);
    let mut h = test_window::open(app(false), vec2(w, ht));
    assert_eq!(test_window::panel_rect(&h, "tools").width(), column_width(2.0));
    let visible = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(w, ht - STATUS_H));
    for label in [LAST_TOOL, FILL] {
        let r = rect(&h, label);
        assert!(visible.contains_rect(r), "{label} at {r:?} is outside {visible:?} without scrolling");
    }
    assert!(!h.state().app.ui.tools_double_column);
    // A taller window brings the single column back.
    h.set_size(vec2(1400.0, 900.0));
    h.run_steps(4);
    assert_eq!(test_window::panel_rect(&h, "tools").width(), column_width(1.0));
    assert!(!h.state().app.ui.tools_double_column);
}

/// Runs `app` in a window and double-clicks the Tools panel's Fill box.
fn double_click_fill(app: crate::DesignApp) -> egui_kittest::Harness<'static, crate::DesignApp> {
    // Frames 1 ms apart, so two clicks make a double-click.
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(0.001).build_ui_state(
        |ui, app: &mut crate::DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    let at = h.get_by_label(FILL).rect().center();
    h.hover_at(at);
    h.step();
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }
    h.run_steps(3);
    h
}

#[test]
fn double_clicking_the_fill_box_opens_the_color_picker_with_the_selections_colour() {
    let mut app = app(false);
    app.run("file.new", serde_json::json!({})).unwrap();
    let id = app.run("frame.create", serde_json::json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].clone();
    app.run("object.color", serde_json::json!({"color": "#456daf", "ids": [id]})).unwrap();
    app.run("selection.set", serde_json::json!({"ids": [id]})).unwrap();
    let h = double_click_fill(app);
    let d = h.state().ui.dialog.clone().expect("the Color Picker opened");
    assert_eq!(d.id, "colorPicker");
    assert_eq!((d.fields["target"].as_str(), d.fields["hex"].as_str()), (Some("fill"), Some("#456daf")), "{:?}", d.fields);
}

/// With Formatting Affects Text on and a text frame selected, the picker opened from the Fill
/// box colours the frame's text, not the frame.
#[test]
fn the_color_picker_from_the_fill_box_follows_formatting_affects_text() {
    let mut app = app(false);
    app.run("file.new", serde_json::json!({})).unwrap();
    let r = app.run("frame.create", serde_json::json!({"rect": [72, 72, 300, 200], "content": "text", "text": "words", "caret": false})).unwrap();
    let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
    app.run("selection.set", serde_json::json!({"ids": [id.0]})).unwrap();
    app.ui.formatting_affects_text = true;
    let mut h = double_click_fill(app);
    let app = h.state_mut();
    let d = app.ui.dialog.as_mut().expect("the Color Picker opened");
    assert_eq!(d.fields["text"], true, "{:?}", d.fields);
    // What the picker's fields hold after #456daf is typed.
    for (k, v) in [("hex", serde_json::json!("#456daf")), ("rgb", serde_json::json!([0x45, 0x6d, 0xaf])), ("model", serde_json::json!("rgb"))] {
        d.fields.insert(k.into(), v);
    }
    d.fields.insert("edited".into(), serde_json::json!(true));
    crate::dialogs::confirm(app).unwrap();
    let st = app.session.active().unwrap();
    let it = st.doc.item(id).unwrap();
    assert_eq!(it.fill.swatch, "[None]", "the frame keeps its fill");
    let story = st.doc.stories.values().find(|s| s.text.starts_with("words")).unwrap();
    let fill = story.runs().next().unwrap().1.over.fill.clone().expect("the text has a fill");
    let rgb = st.doc.resolve_color(&fill, 1.0).unwrap().to_rgb();
    assert!((rgb[0] - 0x45 as f32 / 255.0).abs() < 0.01 && (rgb[2] - 0xaf as f32 / 255.0).abs() < 0.01, "{rgb:?}");
}

#[test]
fn a_tall_window_keeps_one_column_and_does_not_scroll() {
    let mut h = test_window::open(app(false), vec2(1400.0, 900.0));
    let panel = test_window::panel_rect(&h, "tools");
    assert_eq!(panel.width(), column_width(1.0));
    let before = (rect(&h, LAST_TOOL), rect(&h, FILL));
    assert!(panel.contains_rect(before.0) && panel.contains_rect(before.1), "{before:?} outside {panel:?}");
    wheel(&mut h, panel.center(), vec2(0.0, -2000.0));
    assert_eq!((rect(&h, LAST_TOOL), rect(&h, FILL)), before);
}
#[test]
fn the_tools_panel_lays_out_like_indesigns() {
    // Chris Nielsen's "The Adobe InDesign Toolbox" (InDesign 2020): Gradient Swatch and Gradient
    // Feather are tools of their own; in two columns Content Collector and Placer, Apply
    // Color / Gradient / None and Normal / Preview each get a button.
    for double in [false, true] {
        let h = test_window::open(app(double), vec2(1400.0, 900.0));
        let _ = rect(&h, "Gradient Swatch Tool (G)");
        let shortcut = crate::menus::tool_shortcut_label(&h.state().app, "gradientFeather").unwrap();
        let _ = rect(&h, &format!("Gradient Feather Tool ({shortcut})"));
        let _ = rect(&h, "View Options");
        let split = ["Content Placer Tool (B)", "Apply Color (,)", "Apply Gradient (.)", "Normal"];
        for label in split {
            assert_eq!(h.query_by_label(label).is_some(), double, "double column {double}: {label}");
        }
        let _ = rect(&h, "Content Collector Tool (B)");
        // One column: the Apply well names what is in front (no fill: Apply None).
        let _ = rect(&h, "Apply None (/)");
        if double {
            let (a, b) = (rect(&h, "Content Collector Tool (B)"), rect(&h, "Content Placer Tool (B)"));
            assert!((a.center().y - b.center().y).abs() < 1.0 && a.center().x < b.center().x, "side by side: {a:?} {b:?}");
            let (n, p) = (rect(&h, "Normal"), rect(&h, "Preview (W)"));
            assert!((n.center().y - p.center().y).abs() < 1.0, "Normal and Preview side by side");
        }
    }
}

#[test]
fn apply_color_and_gradient_reuse_the_last_ones() {
    use serde_json::json;
    let mut app = app(true);
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200], "content": "unassigned"})).unwrap()["id"].clone();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let fill = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap();
        let it = st.doc.item(st.selection.items[0]).unwrap();
        (it.fill.swatch.clone(), it.stroke.swatch.clone())
    };
    // Apply Color: [Black] at first, then the colour last applied.
    app.run("color.applyColor", json!({})).unwrap();
    assert_eq!(fill(&app).0, "[Black]");
    crate::panels::apply_swatch(&mut app, false, "C=100 M=0 Y=0 K=0", None).unwrap();
    app.run("color.applyNone", json!({})).unwrap();
    assert_eq!(fill(&app).0, "[None]");
    app.run("color.applyColor", json!({})).unwrap();
    assert_eq!(fill(&app).0, "C=100 M=0 Y=0 K=0");
    // Apply Gradient: white to black when there's none yet, then that one again.
    app.run("color.applyGradient", json!({})).unwrap();
    let g = fill(&app).0;
    let doc = &app.session.active().unwrap().doc;
    let grad = designcraft_color::swatch::resolve_gradient(&doc.swatches, &g).expect("a gradient");
    assert_eq!((grad.stops[0].color.to_rgb(), grad.stops[1].color.to_rgb()), ([1.0; 3], [0.0; 3]));
    app.run("color.applyColor", json!({})).unwrap();
    // With the stroke in front, it goes to the stroke; the fill keeps its colour.
    app.ui.stroke_front = true;
    app.run("color.applyGradient", json!({})).unwrap();
    assert_eq!(fill(&app), ("C=100 M=0 Y=0 K=0".to_string(), g));
    // The shortcuts: , and .
    for (id, sc) in [("color.applyColor", ","), ("color.applyGradient", ".")] {
        assert_eq!(crate::menus::ui_label(id).and_then(|c| c.1), Some(sc));
    }
}

/// A new default gradient on the stroke in front is one undo step; the fill keeps its swatch and tint.
#[test]
fn apply_gradient_to_the_stroke_is_one_undo_step() {
    use serde_json::json;
    let mut app = app(true);
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200], "content": "unassigned"})).unwrap()["id"].clone();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.run("object.fill", json!({"swatch": "[Paper]", "tint": 0.5})).unwrap();
    app.run("object.stroke", json!({"swatch": "[Black]", "tint": 0.7})).unwrap();
    let paints = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap();
        let it = st.doc.item(st.selection.items[0]).unwrap();
        (it.fill.swatch.clone(), it.fill.tint, it.stroke.swatch.clone(), it.stroke.tint)
    };
    let before = paints(&app);
    app.ui.stroke_front = true;
    app.run("color.applyGradient", json!({})).unwrap();
    let after = paints(&app);
    assert_eq!((&after.0, after.1), (&before.0, before.1), "the fill is untouched");
    assert_ne!(after.2, before.2, "the stroke is the gradient");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(paints(&app), before, "one undo restores the stroke");
}

/// With nothing selected, the Color Picker and Apply None set the colours of the next shape
/// (the course's poster: a scarlet fill picked before drawing), and the proxy shows them.
#[test]
fn with_nothing_selected_the_proxy_sets_the_next_shapes_colours() {
    use serde_json::json;
    let mut app = app(false);
    app.run("file.new", json!({})).unwrap();
    // Frames 1 ms apart, so two clicks make a double-click.
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(0.001).build_ui_state(
        |ui, app: &mut crate::DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    // Double-click the Fill box: the picker opens; OK with nothing selected keeps the colour.
    let at = h.get_by_label(FILL).rect().center();
    h.hover_at(at);
    h.step();
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }
    h.run_steps(3);
    let app = h.state_mut();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("colorPicker"));
    let d = app.ui.dialog.as_mut().unwrap();
    // What the picker's fields hold after #7c1315 is typed.
    for (k, v) in [("hex", json!("#7c1315")), ("rgb", json!([0x7c, 0x13, 0x15])), ("model", json!("rgb")), ("edited", json!(true))] {
        d.fields.insert(k.into(), v);
    }
    crate::dialogs::confirm(app).unwrap();
    // Apply None with the stroke in front.
    app.ui.stroke_front = true;
    app.run("color.applyNone", json!({})).unwrap();
    let red = app.session.active().unwrap().doc.settings.new_object.fill.clone();
    assert_eq!(app.session.active().unwrap().doc.resolve_color(&red, 1.0).unwrap().to_hex(), "#7c1315");
    // The next rectangle has them.
    let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    let st = app.session.active().unwrap();
    let it = st.doc.item(designcraft_doc::ItemId(id)).unwrap();
    assert_eq!((it.fill.swatch.as_str(), it.stroke.swatch.as_str()), (red.as_str(), "[None]"));
}

/// The Fill/Stroke proxy's keys run on the selection and stay out of text being typed.
#[test]
fn fill_stroke_proxy_keys() {
    use egui::{Event, Key, Modifiers};
    use serde_json::json;
    fn press(app: &mut crate::DesignApp, ctx: &egui::Context, key: Key, modifiers: Modifiers) {
        let events = [true, false].map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).to_vec();
        ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| crate::menus::shortcuts(app, ui.ctx())).textures_delta.clear();
    }
    fn colors(app: &crate::DesignApp) -> (String, f32, String, f32) {
        let st = app.session.active().unwrap();
        let it = st.doc.item(st.selection.items[0]).unwrap();
        (it.fill.swatch.clone(), it.fill.tint, it.stroke.swatch.clone(), it.stroke.tint)
    }
    let mut app = app(false);
    let ctx = egui::Context::default();
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"})).unwrap();
    app.run("selection.set", json!({"ids": [f["id"]]})).unwrap();
    app.run("object.fill", json!({"swatch": "[Paper]", "tint": 0.5})).unwrap();
    app.run("object.stroke", json!({"swatch": "[Black]", "tint": 1.0})).unwrap();

    press(&mut app, &ctx, Key::X, Modifiers::SHIFT);
    assert_eq!(colors(&app), ("[Black]".into(), 1.0, "[Paper]".into(), 0.5), "Shift+X swaps, tints included");
    press(&mut app, &ctx, Key::D, Modifiers::NONE);
    assert_eq!(colors(&app), ("[None]".into(), 1.0, "[Black]".into(), 1.0), "D: no fill, black stroke");
    press(&mut app, &ctx, Key::X, Modifiers::NONE);
    assert!(app.ui.stroke_front, "X brings the stroke to the front");
    press(&mut app, &ctx, Key::Slash, Modifiers::NONE);
    assert_eq!(colors(&app).2, "[None]", "/ applies None to the stroke in front");

    // Formatting Affects Text (J) with the frame selected: the keys colour its text, as the
    // proxy shows it, and leave the frame alone.
    press(&mut app, &ctx, Key::X, Modifiers::NONE);
    press(&mut app, &ctx, Key::J, Modifiers::NONE);
    assert!(crate::panels::colors_affect_text(&app));
    let frame = colors(&app);
    app.run("type.char", json!({"attrs": {"fill": "[Paper]", "fillTint": 0.5, "stroke": "[Black]", "strokeTint": 1.0}})).unwrap();
    let text = |app: &mut crate::DesignApp| crate::panels::color_target(app).unwrap();
    press(&mut app, &ctx, Key::X, Modifiers::SHIFT);
    assert_eq!(text(&mut app), ("[Black]".into(), 1.0, "[Paper]".into(), 0.5), "Shift+X swaps the text's colours");
    press(&mut app, &ctx, Key::D, Modifiers::NONE);
    assert_eq!(text(&mut app), ("[Black]".into(), 1.0, "[None]".into(), 1.0), "D: text gets a black fill and no stroke");
    press(&mut app, &ctx, Key::Slash, Modifiers::NONE);
    assert_eq!(text(&mut app).0, "[None]", "/ applies None to the text's fill in front");
    assert_eq!(colors(&app), frame, "the frame keeps its colours");

    // Typing in a story: the keys are text, not shortcuts.
    app.session.execute("text.select", &json!({"story": f["story"], "anchor": 5})).unwrap();
    app.select_tool("type");
    assert!(app.session.wants_text());
    let front = app.ui.stroke_front;
    press(&mut app, &ctx, Key::X, Modifiers::NONE);
    assert_eq!(app.ui.stroke_front, front, "X while typing leaves the proxy alone");
}
