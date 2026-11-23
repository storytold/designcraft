//! Successors, on the installed framework, of the native canvas tests for the context menu: the same steps and checks in the
//! shipping window with the craft UI runtime, whose context menus are the framework's.

use designcraft_geom::Unit;
use egui_kittest::{Harness, kittest::Queryable};

use super::*;

/// The shapes the canvas paints for a new document with every spread in view, after two frames
/// (the first lays out).
fn canvas_shapes(app: &mut DesignApp) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = canvas_frame_shapes(app, &ctx);
        if let Some(rect) = app.canvas_rect {
            fit(app, rect, "all");
        }
    }
    shapes
}

/// One frame of the canvas alone in a 1440 by 900 window: the shapes it paints.
fn canvas_frame_shapes(app: &mut DesignApp, ctx: &egui::Context) -> Vec<egui::epaint::ClippedShape> {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| show(app, ui));
    out.textures_delta.clear();
    out.shapes
}

/// The spine-coloured vertical hairlines at screen x `x` that run the full height of `page`
/// (the spine of one spread shares its x with the pages of the spreads above and below it).
fn vertical_lines_at(shapes: &[egui::epaint::ClippedShape], x: f32, page: Rect) -> usize {
    let spine = page_decor::PAGE_EDGE;
    shapes
        .iter()
        .filter(|c| match &c.shape {
            egui::Shape::LineSegment { points: [a, b], stroke } => {
                stroke.color == spine
                    && (a.x - x).abs() < 0.01
                    && (b.x - x).abs() < 0.01
                    && a.y.min(b.y) <= page.min.y + 0.01
                    && a.y.max(b.y) >= page.max.y - 0.01
            }
            _ => false,
        })
        .count()
}

/// Build the four-fps app harness used by canvas tests.
fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    harness_with_step(app, 1.0 / 4.0)
}

/// A window whose frames last `step_dt` seconds. Wheel motion is tested at a realistic frame
/// rate: egui ends a scroll action after 150 ms without wheel events, so quarter-second
/// frames would split every motion into separate actions.
fn harness_with_step(app: DesignApp, step_dt: f32) -> Harness<'static, DesignApp> {
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(1440.0, 900.0)).with_step_dt(step_dt).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

/// A primary-button drag from `a` to `b` over a few frames.
fn drag(h: &mut Harness<'_, DesignApp>, a: Pos2, b: Pos2) {
    h.hover_at(a);
    h.step();
    h.event(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    for k in 1..=4 {
        h.hover_at(a + (b - a) * (k as f32 / 4.0));
        h.step();
    }
    h.event(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
}

fn right_click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    h.hover_at(p);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() });
    }
    h.run_steps(3);
}

fn new_document() -> Harness<'static, DesignApp> {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    harness(app)
}

/// [`new_document`] at 60 frames a second, the cadence a wheel motion really arrives at.
fn new_document_60fps() -> Harness<'static, DesignApp> {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    harness_with_step(app, 1.0 / 60.0)
}

fn held(modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::ModifiersChanged(modifiers)
}

/// Construct a point-unit vertical mouse-wheel event with an unknown source.
fn wheel(phase: egui::TouchPhase, up: f32, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, up), phase, modifiers, source: egui::MouseWheelSource::Unknown }
}

// Plays `events` with the pointer at `at`, a frame each, and lets the motion settle.
fn play(h: &mut Harness<'_, DesignApp>, at: Pos2, events: &[egui::Event]) -> View {
    h.hover_at(at);
    for event in events {
        h.event(event.clone());
        h.step();
    }
    h.run_steps(30);
    *h.state().view().unwrap()
}

fn units(h: &Harness<'_, DesignApp>) -> (Unit, Unit) {
    let s = &h.state().session.doc().unwrap().doc.settings;
    (s.horizontal_units, s.vertical_units)
}

fn zoom_app(tool: &str) -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    app.select_tool(tool);
    app.ui.rulers = false;
    app
}

/// Run one app frame with the supplied modifiers, install fonts, draw the canvas when ready and return the cursor icon.
fn canvas_frame(app: &mut DesignApp, ctx: &egui::Context, m: egui::Modifiers, mut events: Vec<egui::Event>) -> egui::CursorIcon {
    events.insert(0, egui::Event::ModifiersChanged(m));
    let mut output =
        ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, ..Default::default() }, |ui| {
            app.logic(ui.ctx());
            // Fonts are installed by the first real UI pass, just as in the full application.
            app.ui_setup(ui.ctx());
            if app.fonts_ready {
                show(app, ui);
            }
        });
    output.textures_delta.clear();
    output.platform_output.cursor_icon
}

/// The cursor icon the canvas sets for `c` on this platform (bitmap and painted cursors
/// report their own icon).
fn shown(c: Cursor) -> egui::CursorIcon {
    crate::cursors::show(&egui::Context::default(), &mut crate::cursors::Images::default(), c, Pos2::ZERO)
}

fn space_key(pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers }
}

fn press(pos: Pos2, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers }
}

fn hover(app: &mut DesignApp, ctx: &egui::Context, p: Pos2) {
    for _ in 0..3 {
        canvas_frame(app, ctx, egui::Modifiers::NONE, vec![egui::Event::PointerMoved(p)]);
    }
}

const CMD: egui::Modifiers = egui::Modifiers { command: true, mac_cmd: true, ..egui::Modifiers::NONE };

const ART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50" width="100pt" height="50pt"><path d="M0,0 C30,0 70,50 100,50" fill="none" stroke="#000" stroke-width="2"/></svg>"##;

/// A fake system clipboard: its text, a bitmap of a given size and the change count that every
/// copy raises.
#[derive(Default)]
struct FakeClip {
    text: Option<String>,
    image: Option<(u32, u32)>,
    change: i64,
}

type FakeClipboard = std::rc::Rc<std::cell::RefCell<FakeClip>>;

/// Another application copies `text` and an image of `image` pixels.
fn copy_elsewhere(clip: &FakeClipboard, text: Option<&str>, image: Option<(u32, u32)>) {
    let mut c = clip.borrow_mut();
    (c.text, c.image, c.change) = (text.map(str::to_string), image, c.change + 1);
}

fn fake_clipboard(text: Option<&str>, image: Option<(u32, u32)>) -> (FakeClipboard, crate::Services) {
    let clip = FakeClipboard::default();
    copy_elsewhere(&clip, text, image);
    let (t, i, c) = (clip.clone(), clip.clone(), clip.clone());
    let services = crate::Services {
        clipboard_text: Some(Box::new(move || t.borrow().text.clone())),
        clipboard_image: Some(Box::new(move || {
            let (width, height) = i.borrow().image?;
            // Sizes past the paste limit are refused before their pixels are read.
            let len = if u64::from(width) * u64::from(height) > designcraft_engine::cmd::MAX_PASTED_PIXELS { 0 } else { width * height * 4 };
            Some(crate::ClipboardImage::Pixels { width, height, rgba: vec![90; len as usize] })
        })),
        clipboard_change: Some(Box::new(move || Some(c.borrow().change))),
        ..Default::default()
    };
    (clip, services)
}

/// The embedded image the selected frame shows: its pixel size and the asset's name.
fn pasted_image(app: &DesignApp) -> Option<((u32, u32), String)> {
    let st = app.session.doc().unwrap();
    let id = *st.selection.items.first()?;
    let designcraft_doc::Content::Graphic(g) = &st.doc.item(id)?.content else { return None };
    let asset = &st.doc.assets[&g.asset];
    assert_eq!(asset.link, None, "embedded");
    Some((asset.pixels?, asset.name.clone()))
}

fn out_frame(doc: &Document, id: u64) -> &Item {
    doc.item(designcraft_doc::ItemId(id)).unwrap()
}

/// A harness whose frames are 1 ms apart, so two clicks make a double-click.
fn fast_harness(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(1440.0, 900.0)).with_step_dt(0.001).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn primary(h: &mut Harness<'_, DesignApp>, pos: Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
    h.run_steps(2);
}

fn corner_drag(h: &mut Harness<'_, DesignApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    primary(h, from, true);
    h.event(egui::Event::PointerMoved(from + (to - from) / 2.0));
    h.run_steps(2);
    h.event(egui::Event::PointerMoved(to));
    h.run_steps(2);
    primary(h, to, false);
}

fn corner_click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    h.hover_at(p);
    h.step();
    primary(h, p, true);
    primary(h, p, false);
}

/// Where the rulers measure from (canvas coordinates): the document's zero point on the
/// spread in view (its first page, for a page origin).
fn ruler_zero(app: &DesignApp, doc: &Document, layout: &CanvasLayout) -> Option<Point> {
    ruler_pieces(app, doc, layout, doc.zero_point()).first().map(|p| p.2)
}

fn zero_point(h: &Harness<'_, DesignApp>) -> [f64; 2] {
    h.state().session.doc().unwrap().doc.settings.zero_point
}

/// Screen position of spread 0's point (x, y).
fn screen_of(h: &Harness<'_, DesignApp>, x: f64, y: f64) -> Pos2 {
    let app = h.state();
    let st = app.session.active().unwrap();
    let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    xf.to_screen(CanvasLayout::new(&st.doc, st.editing_parents).xf(SpreadRef::Doc(0)) * Point::new(x, y))
}

/// A menu row: a button that isn't a toggle, named by its label, then its shortcut if it has
/// one. Toggle buttons with the same name (the Task Bar's icons) don't match, nor do buttons
/// right of the canvas (the Properties panel's Arrange).
fn menu_row<'h>(h: &'h Harness<'_, DesignApp>, label: &'h str) -> Option<egui_kittest::Node<'h>> {
    let right = f64::from(h.state().canvas_rect.map_or(f32::INFINITY, |r| r.max.x));
    h.query_by(move |n| {
        matches!(n.role(), egui::accesskit::Role::Button | egui::accesskit::Role::MenuItem | egui::accesskit::Role::MenuItemRadio)
            && n.toggled().is_none()
            && n.bounding_box().is_none_or(|b| b.x0 < right)
            && n.label().is_some_and(|l| l == label || l.strip_prefix(label).and_then(|r| r.strip_prefix(' ')).is_some_and(|r| !r.contains(' ')))
    })
}

fn enabled(h: &Harness<'_, DesignApp>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    !menu_row(h, label).unwrap_or_else(|| panic!("no menu row {label}")).accesskit_node().is_disabled()
}

fn spread_items(h: &Harness<'_, DesignApp>) -> Vec<u64> {
    h.state().session.doc().unwrap().doc.spreads[0].items.iter().map(|i| i.id.0).collect()
}

fn selected(h: &Harness<'_, DesignApp>) -> Vec<u64> {
    h.state().session.doc().unwrap().selection.items.iter().map(|i| i.0).collect()
}

/// Successor of `right_clicking_an_object_selects_it_and_its_menu_arranges_it` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a Button, a MenuItem or a MenuItemRadio node.
#[test]
fn right_clicking_an_object_selects_it_and_its_menu_arranges_it() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [150, 150, 300, 300]})).unwrap()["id"].as_u64().unwrap();
    let mut h = harness(app);
    assert_eq!(selected(&h), [b]);
    let on_a = screen_of(&h, 60.0, 60.0);
    right_click(&mut h, on_a);
    assert_eq!(selected(&h), [a], "the object under the pointer is selected first");
    // An empty frame's menu: Content, but no Fitting (nothing to fit) and no text frame entries.
    assert!(menu_row(&h, "Content").is_some());
    assert!(menu_row(&h, "Fitting").is_none() && menu_row(&h, "Text Frame Options…").is_none());
    // Paste is available with nothing copied here: the system clipboard may hold another app's text.
    assert!(enabled(&h, "Copy") && enabled(&h, "Paste"));
    menu_row(&h, "Arrange").unwrap().click();
    h.run_steps(3);
    menu_row(&h, "Bring to Front").unwrap().click();
    h.run_steps(3);
    assert_eq!(spread_items(&h), [b, a]);
    assert!(menu_row(&h, "Arrange").is_none(), "choosing an entry closes the menu");
}

/// Successor of `right_clicking_empty_canvas_offers_paste_after_a_copy` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a Button, a MenuItem or a MenuItemRadio node.
#[test]
fn right_clicking_empty_canvas_offers_paste_after_a_copy() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("edit.copy", json!({})).unwrap();
    app.select_tool("rectangleFrame");
    let mut h = harness(app);
    let empty = screen_of(&h, 400.0, 500.0);
    right_click(&mut h, empty);
    assert_eq!(selected(&h), [a], "a right-click on nothing keeps the selection");
    assert!(menu_row(&h, "Arrange").is_none(), "the page menu, not the object menu");
    assert!(enabled(&h, "Paste in Place"));
    assert!(menu_row(&h, "✓ Show/Hide Rulers").is_some(), "toggles show their state");
    // Escape closes the menu and does nothing else: the tool would deselect, the shortcuts
    // would switch to the Selection tool.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(menu_row(&h, "Paste in Place").is_none());
    assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
    right_click(&mut h, empty);
    menu_row(&h, "Paste").unwrap().click();
    h.run_steps(3);
    assert_eq!(spread_items(&h).len(), 2);
}

/// Successor of `right_clicking_another_object_while_the_menu_is_open_reopens_it_there` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a Button, a MenuItem or a MenuItemRadio node.
#[test]
fn right_clicking_another_object_while_the_menu_is_open_reopens_it_there() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [36, 600, 200, 760]})).unwrap()["id"].as_u64().unwrap();
    let mut h = harness(app);
    // The menu opens down from A, with the pointer on its first row (no submenu open); B is below it.
    let (on_a, on_b) = (screen_of(&h, 60.0, 60.0), screen_of(&h, 50.0, 700.0));
    right_click(&mut h, on_a);
    assert_eq!(selected(&h), [a]);
    assert!(menu_row(&h, "Arrange").is_some());
    right_click(&mut h, on_b);
    assert_eq!(selected(&h), [b]);
    assert!(menu_row(&h, "Arrange").is_some(), "the menu reopens at the new click");
}

/// Successor of `a_context_menu_in_one_pane_keeps_the_keys_from_the_other` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a Button, a MenuItem or a MenuItemRadio node.
#[test]
fn a_context_menu_in_one_pane_keeps_the_keys_from_the_other() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
    app.select_tool("rectangleFrame");
    app.split = true;
    let mut h = harness(app);
    // The focused pane is the left one; this point is near its right edge, away from the frame.
    let left = h.state().canvas_rect.unwrap();
    right_click(&mut h, pos2(left.max.x - 40.0, left.center().y));
    assert!(menu_row(&h, "Paste in Place").is_some());
    // Escape closes the left pane's menu before the right pane handles the keys.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(menu_row(&h, "Paste in Place").is_none());
    assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
}

/// Successor of `right_clicking_text_keeps_the_selected_text_or_places_the_caret` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a Button, a MenuItem or a MenuItemRadio node.
#[test]
fn right_clicking_text_keeps_the_selected_text_or_places_the_caret() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 400, 200], "content": "text"})).unwrap();
    app.select_tool("type");
    app.run("text.insert", json!({"text": "Hello world"})).unwrap();
    let sid = *app.session.doc().unwrap().doc.stories.keys().next().unwrap();
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
    let mut h = harness(app);
    let text_sel = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().selection.text.as_ref().map(|t| (t.anchor, t.focus));
    // Inside "Hello": the selection stays.
    let inside = screen_of(&h, 45.0, 44.0);
    right_click(&mut h, inside);
    assert_eq!(text_sel(&h), Some((0, 5)));
    assert!(menu_row(&h, "Paste without Formatting").is_some(), "the text menu");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // Past the end of the line: the caret goes there.
    let past_end = screen_of(&h, 300.0, 44.0);
    right_click(&mut h, past_end);
    assert_eq!(text_sel(&h), Some((11, 11)));
    assert!(menu_row(&h, "Paste without Formatting").is_some());
}

/// Successor of `rows_hidden_in_the_menu_bar_are_hidden_in_context_menus` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a Button, a MenuItem or a MenuItemRadio node.
#[test]
fn rows_hidden_in_the_menu_bar_are_hidden_in_context_menus() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap();
    app.ui.hidden_menu_items.push(crate::menus::menu_key("Object/Arrange", "Bring to Front"));
    let mut h = harness(app);
    let on_frame = screen_of(&h, 60.0, 60.0);
    right_click(&mut h, on_frame);
    menu_row(&h, "Arrange").unwrap().click();
    h.run_steps(3);
    assert!(menu_row(&h, "Bring to Front").is_none());
    assert!(menu_row(&h, "Send to Back").is_some());
    assert!(h.query_by_label("Show All Menu Items").is_some());
}

/// A text frame holding `text`, the Type tool, and a harness stepping at 60 fps (clicks in a
/// row must land within the double-click delay).
fn typing(text: &str) -> (Harness<'static, DesignApp>, u64) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": text})).unwrap();
    let id = r["id"].as_u64().unwrap();
    app.run("tool.select", json!({"tool": "type"})).unwrap();
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    (h, id)
}

/// The screen point over the first line of frame `id` where a click puts the caret at a byte
/// in `bytes`.
fn screen_at(h: &mut Harness<'_, DesignApp>, id: u64, bytes: std::ops::Range<usize>) -> Pos2 {
    let mut x = 74.0;
    let p = loop {
        assert!(x < 400.0, "no point on the first line hits {bytes:?}");
        h.state_mut().run("text.placeCaret", json!({"frame": id, "point": [x, 80.0]})).unwrap();
        if h.state().session.doc().unwrap().selection.text.is_some_and(|t| bytes.contains(&t.focus)) {
            break Point::new(x, 80.0);
        }
        x += 1.0;
    };
    h.state_mut().run("edit.deselectAll", json!({})).unwrap();
    let app = h.state();
    let c = app.session.layout().to_canvas(SpreadRef::Doc(0), p);
    Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(c)
}

fn click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }
}

fn selected_range(h: &Harness<'_, DesignApp>) -> std::ops::Range<usize> {
    h.state().session.doc().unwrap().selection.text.map(|t| t.range()).unwrap_or_default()
}

/// One frame of the whole app with `events`; returns what it asks of the system IME.
fn ime_frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Option<egui::output::IMEOutput> {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    out.platform_output.ime
}

/// The Type tool's caret at the end of a new text frame holding `text`, the app's first frames
/// drawn; returns the story id.
fn typing_app(app: &mut DesignApp, ctx: &egui::Context, text: &str) -> Value {
    app.run("file.new", json!({})).unwrap();
    let f = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": text})).unwrap();
    let sid = f["story"].clone();
    app.session.execute("text.select", &json!({"story": sid, "anchor": text.len(), "focus": text.len()})).unwrap();
    app.select_tool("type");
    for _ in 0..4 {
        ime_frame(app, ctx, vec![]);
    }
    sid
}

fn story_text(app: &mut DesignApp, sid: &Value) -> String {
    app.session.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string()
}

/// Spread point `p` on screen.
fn on_screen(app: &DesignApp, p: Point) -> Pos2 {
    let st = app.session.active().unwrap();
    let c = CanvasLayout::new(&st.doc, st.editing_parents).to_canvas(SpreadRef::Doc(0), p);
    Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(c)
}

fn preedit(t: &str, c: usize) -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Preedit { text: t.into(), active_range_chars: Some(c..c) })
}

/// Extract `(surrounding text, cursor, anchor)` from an IME output.
fn ime_payload(ime: &egui::output::IMEOutput) -> Option<(&str, usize, usize)> {
    ime.surrounding_text.available().map(|p| (p.text.as_str(), p.cursor, p.anchor))
}

/// Set an engine text selection for a test fixture.
fn text_selection(app: &mut DesignApp, sid: &Value, anchor: usize, focus: usize) {
    app.session.execute("text.select", &json!({"story": sid, "anchor": anchor, "focus": focus})).unwrap();
}

struct NativeClickFixture {
    app: DesignApp,
    ctx: egui::Context,
    target: Value,
    old: Value,
    at: Pos2,
    focus: usize,
}

/// Build the old and target stories, measure an interior target caret, and leave the old story selected before the click.
fn native_gui_click_fixture() -> NativeClickFixture {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    let ctx = egui::Context::default();
    let old = typing_app(&mut app, &ctx, "Old story");
    let frame = app.run("frame.create", json!({"rect": [36, 280, 300, 400], "content": "text", "text": "Target story"})).unwrap();
    let target = frame["story"].clone();
    // Measure an interior first-word caret through the engine, rather than guessing a
    // font-dependent screen pixel or clicking the frame's resize border.
    let (point, focus) = (50..180)
        .find_map(|x| {
            app.run("text.placeCaret", json!({"frame": frame["id"], "point": [x, 290]})).unwrap();
            let sel = app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!(sel.story.0, target.as_u64().unwrap());
            (1..5).contains(&sel.focus).then_some((Point::new(f64::from(x), 290.0), sel.focus))
        })
        .expect("an interior point in the target's first word");
    text_selection(&mut app, &old, "Old story".len(), "Old story".len());
    for _ in 0..4 {
        ime_frame(&mut app, &ctx, vec![]);
    }
    let at = on_screen(&app, point);
    assert!(app.canvas_rect.unwrap().contains(at));
    ime_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    assert_eq!(app.session.doc().unwrap().selection.text.unwrap().story.0, old.as_u64().unwrap());
    NativeClickFixture { app, ctx, target, old, at, focus }
}

/// Build a primary pointer press or release event at a position.
fn native_gui_button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
}

/// The 37-byte Devanagari line the native run had in the previous story.
const NATIVE_GUI_OLD_DEVANAGARI: &str = "नमस्ते दुनिया";

/// The click fixture with the non-empty selection in the previous story that the native
/// Windows run had (37-byte Devanagari, 0..37). A non-empty selection shows the contextual
/// task bar; keep it present but clear of the target click.
fn native_gui_selected_old_click_fixture() -> NativeClickFixture {
    let mut f = native_gui_click_fixture();
    text_selection(&mut f.app, &f.old, 0, "Old story".len());
    f.app.run("text.insert", json!({"text": NATIVE_GUI_OLD_DEVANAGARI})).unwrap();
    text_selection(&mut f.app, &f.old, 0, NATIVE_GUI_OLD_DEVANAGARI.len());
    for _ in 0..4 {
        ime_frame(&mut f.app, &f.ctx, vec![]);
    }
    let sel = f.app.session.doc().unwrap().selection.text.unwrap();
    assert_eq!((sel.story.0, sel.anchor, sel.focus), (f.old.as_u64().unwrap(), 0, NATIVE_GUI_OLD_DEVANAGARI.len()));
    assert_eq!(NATIVE_GUI_OLD_DEVANAGARI.len(), 37, "the native fixture's Devanagari line was 37 UTF-8 bytes");
    assert!(!sel.is_caret(), "the previous story's selection is non-empty");
    assert!(!context_menu_open(&f.ctx), "no context menu owns this frame's keys");
    assert!(f.app.ui.dialog.is_none() && f.app.ui.palette.is_none(), "no modal owns the keyboard");
    assert!(!f.ctx.text_edit_focused(), "no text field owns the keyboard");
    assert_eq!(f.app.pane, f.app.focus_pane, "the canvas pane takes the keyboard");
    let task_bar = egui::Id::unique(("task_bar", f.ctx.viewport_id(), f.app.pane));
    let bar = f.ctx.memory(|m| m.area_rect(task_bar)).expect("the old selection shows the floating task bar");
    assert!(!bar.contains(f.at), "the floating task bar does not cover the target click at {bar:?}");
    f
}

/// Run one raw event batch, optionally force a discarded pass, and return the pointer edges observed by each pass.
fn native_gui_input(f: &mut NativeClickFixture, events: Vec<egui::Event>, discard: bool) -> Vec<(bool, bool, bool)> {
    let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
    // The host runs the app's input hook before every pass, as eframe does.
    f.app.raw_input_hook(&f.ctx, &mut raw);
    let mut edges = Vec::new();
    let mut out = f.ctx.run_ui(raw, |ui| {
        edges.push(ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin().is_some())));
        f.app.logic(ui.ctx());
        f.app.ui(ui);
        if discard && edges.len() == 1 {
            ui.ctx().request_discard("native same-batch click exactly-once regression");
        }
    });
    if discard {
        assert_eq!(out.platform_output.num_completed_passes, 2, "the requested discarded pass actually ran");
    }
    out.textures_delta.clear();
    edges
}

/// Assert that the primary button, canvas press state and tool gesture are all released.
fn native_gui_released(f: &NativeClickFixture) {
    assert!(!f.ctx.input(|i| i.pointer.primary_down()), "the native primary button is up");
    let down = f.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("canvas_pointer_down", f.app.pane)))).unwrap_or(false);
    assert!(!down, "the canvas retained a released pointer");
    assert!(!f.app.session.tool_busy(), "a released click must not leave a tool gesture");
}

/// Create an independently measured second-story caret farther apart than the double-click distance.
fn native_gui_second_click_target(f: &mut NativeClickFixture) -> (Value, Pos2, usize) {
    let frame = f.app.run("frame.create", json!({"rect": [330, 280, 575, 400], "content": "text", "text": "Second story"})).unwrap();
    let story = frame["story"].clone();
    let (point, focus) = (345..465)
        .find_map(|x| {
            f.app.run("text.placeCaret", json!({"frame": frame["id"], "point": [x, 290]})).unwrap();
            let sel = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!(sel.story.0, story.as_u64().unwrap());
            (1..5).contains(&sel.focus).then_some((Point::new(f64::from(x), 290.0), sel.focus))
        })
        .expect("an independently measured interior caret in the second story");
    text_selection(&mut f.app, &f.old, "Old story".len(), "Old story".len());
    for _ in 0..4 {
        ime_frame(&mut f.app, &f.ctx, vec![]);
    }
    let at = on_screen(&f.app, point);
    assert!(f.app.canvas_rect.unwrap().contains(f.at));
    assert!(f.app.canvas_rect.unwrap().contains(at));
    let max_click_dist = f.ctx.options(|o| o.input_options.max_click_dist);
    assert!((at - f.at).length() > max_click_dist, "the two target presses must not form a double click");
    (story, at, focus)
}

#[test]
#[allow(deprecated)] // `ImeEvent::Enabled` / `Disabled`: egui no longer sends them, other integrations may.
fn ime_events_compose_in_the_story_with_the_candidate_window_at_the_caret() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "曲:");
    // At a text caret the IME is allowed, its window on the caret as the canvas draws it.
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("IME allowed at a text caret");
    let (top, bottom) = {
        let st = app.session.active().unwrap();
        let cs = app.session.cache.get(&st.doc, designcraft_doc::StoryId(sid.as_u64().unwrap()), None);
        let (fi, x, bl, asc, desc) = compose::caret(&cs, "曲:".len()).unwrap();
        let fid = cs.frames[fi].frame;
        let (a, _) = item_canvas_xf(&st.doc, &CanvasLayout::new(&st.doc, st.editing_parents), fid).unwrap();
        let m = a * st.doc.text_xf(st.doc.item(fid).unwrap());
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        (xf.to_screen(m * Point::new(x, bl - asc)), xf.to_screen(m * Point::new(x, bl + desc)))
    };
    assert!(ime.rect.expand(1.0).contains(top) && ime.rect.expand(1.0).contains(bottom), "{:?} vs {top:?}–{bottom:?}", ime.rect);
    assert_eq!(ime.cursor_rect, ime.rect);
    assert!(!ime.should_interrupt_composition);
    assert!(app.canvas_ime(), "the web build keeps whole words typed at the caret");
    // "ni" → に → 日, with the events winit sends.
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Enabled), preedit("ni", 2)]);
    assert_eq!(story_text(&mut app, &sid), "曲:ni", "marked text shows in place");
    assert!(app.session.tool_composing());
    // Keys, plain text, the clipboard and Undo (the native menu's ⌘Z) wait for the IME.
    let steps = app.session.doc().unwrap().history.undo.len();
    let key = |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
    ime_frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Backspace, true), key(egui::Key::Backspace, false), egui::Event::Text("x".into()), egui::Event::Paste("y".into())],
    );
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    assert!(!crate::menus::enabled(&app, "edit.undo"));
    assert_eq!(story_text(&mut app, &sid), "曲:ni");
    let ime2 = ime_frame(&mut app, &ctx, vec![preedit("に", 1)]).expect("IME allowed while composing");
    assert_eq!(story_text(&mut app, &sid), "曲:に");
    assert!(ime2.rect.center().x > ime.rect.center().x, "the window follows the IME's cursor: {:?} {:?}", ime2.rect, ime.rect);
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into())), egui::Event::Ime(egui::ImeEvent::Disabled)]);
    assert_eq!(story_text(&mut app, &sid), "曲:日");
    assert!(!app.session.tool_composing());
    assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "the commit is one undo step");
    assert_eq!(app.session.journal.last(), Some(&("text.insert".to_string(), json!({"text": "日"}))));
    // After the commit, typing and Undo work again.
    ime_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]);
    assert_eq!(story_text(&mut app, &sid), "曲:日!");
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    assert_eq!(story_text(&mut app, &sid), "曲:");
    // Without a text caret the IME is off: single-key tool shortcuts keep working.
    app.select_tool("selection");
    assert!(ime_frame(&mut app, &ctx, vec![]).is_none());
    assert!(!app.canvas_ime());
}

/// Successor of `enter_or_tab_in_a_panel_field_leave_the_selected_text_alone` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a Button, a MenuItem or a MenuItemRadio node.
#[test]
fn enter_or_tab_in_a_panel_field_leave_the_selected_text_alone() {
    for key in [egui::Key::Enter, egui::Key::Tab] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.framework_opacity = true;
        app.framework_installed = true;
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        app.select_tool("type");
        app.run("text.select", json!({"story": f["story"], "anchor": 1, "focus": 4})).unwrap();
        assert!(app.session.wants_text());
        let mut h = harness(app);
        let size = h.get_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.value().as_deref() == Some("12 pt")).unwrap();
        size.focus();
        h.run_steps(1);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(1);
        h.event(egui::Event::Text("18".into()));
        h.run_steps(1);
        h.key_press(key);
        h.run_steps(3);
        let text = h.state_mut().session.execute("story.get", &json!({"story": f["story"]})).unwrap()["text"].clone();
        assert_eq!(text, "Hello", "{key:?} confirms the field and leaves the story alone");
        let size = crate::panels::text_attrs(h.state_mut()).unwrap()["chars"]["size"].clone();
        assert_eq!(size, 18.0, "{key:?} applies the typed size");
    }
}
