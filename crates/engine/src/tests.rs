use serde_json::json;

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2})).unwrap();
    s
}

#[test]
fn create_frame_type_and_undo() {
    let mut s = session();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.insert", &json!({"text": "Hello, world"})).unwrap();
    let st = s.execute("story.get", &json!({"story": sid})).unwrap();
    assert_eq!(st["text"], "Hello, world");
    s.execute("text.delete", &json!({})).unwrap();
    assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "Hello, worl");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "Hello, world");
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.stories.is_empty());
}

#[test]
fn undo_restores_the_selection_from_before_the_command() {
    let mut s = session();
    let a = s.execute("frame.create", &json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": [a]})).unwrap();
    let b = s.execute("frame.create", &json!({"rect": [200, 36, 300, 100]})).unwrap()["id"].as_u64().unwrap();
    let sel = |s: &Session| s.doc().unwrap().selection.items.iter().map(|i| i.0).collect::<Vec<_>>();
    assert_eq!(sel(&s), vec![b]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(sel(&s), vec![a]);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(sel(&s), vec![b]);
}

#[test]
fn smart_quotes_and_formatting() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
    s.execute("text.insert", &json!({"text": "\"hi\" it's"})).unwrap();
    let t = s.execute("document.inspect", &json!({})).unwrap();
    let sid = t["stories"][0]["id"].as_u64().unwrap();
    assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "\u{201C}hi\u{201D} it\u{2019}s");
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 4})).unwrap();
    s.execute("type.char", &json!({"attrs": {"size": 24, "fontStyle": "Bold"}})).unwrap();
    let a = s.execute("type.selectionAttrs", &json!({})).unwrap();
    assert_eq!(a["chars"]["size"], 24.0);
    s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["para"]["align"], "center");
}

/// Issue #919: with Cmd held, a press with the Type (or any drawing) tool selects and drags like
/// the Selection tool, and the chosen tool stays.
#[test]
fn cmd_held_press_borrows_the_selection_tool_for_one_gesture() {
    use designcraft_tools::{Mods, PointerEvent, PointerKind};
    let mut s = session();
    let v = ViewInfo::at_zoom(1.0);
    let r = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200], "content": "text", "text": "Hello"})).unwrap();
    let fid = r["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": []})).unwrap();
    s.set_tool("type");
    let cmd = Mods { cmd: true, ..Mods::default() };
    // Cmd-press on the frame selects it as an object (no caret) and a Cmd-drag moves it.
    s.pointer(&PointerEvent::new(PointerKind::Down, 150.0, 150.0).with_mods(cmd), v).unwrap();
    assert_eq!(s.doc().unwrap().selection.items, vec![designcraft_doc::ItemId(fid)]);
    assert!(s.doc().unwrap().selection.text.is_none(), "no caret: the Selection tool was borrowed");
    s.pointer(&PointerEvent::new(PointerKind::Drag, 170.0, 150.0).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 200.0, 150.0).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 200.0, 150.0).with_mods(cmd), v).unwrap();
    assert_eq!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(fid)).unwrap().bounds().x0, 150.0, "moved by the drag");
    assert_eq!(s.tool_id(), "type", "the chosen tool stays");
    assert!(s.temp_tool.is_none(), "the borrowed tool is gone on release");
    // The next plain press is the Type tool's again: a caret in the frame.
    s.pointer(&PointerEvent::new(PointerKind::Down, 200.0, 150.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 200.0, 150.0), v).unwrap();
    assert!(s.doc().unwrap().selection.text.is_some(), "a plain press places the caret");
    // Cmd with the Selection tool itself borrows nothing (Cmd-click selects through, as before).
    s.set_tool("selection");
    s.pointer(&PointerEvent::new(PointerKind::Down, 200.0, 150.0).with_mods(cmd), v).unwrap();
    assert!(s.temp_tool.is_none());
    s.pointer(&PointerEvent::new(PointerKind::Up, 200.0, 150.0).with_mods(cmd), v).unwrap();
    s.set_tool("pen");
    s.pointer(&PointerEvent::new(PointerKind::Down, 200.0, 150.0).with_mods(cmd), v).unwrap();
    assert_eq!(s.temp_tool.as_ref().map(|t| t.id()), Some("selection"));
    s.pointer(&PointerEvent::new(PointerKind::Up, 200.0, 150.0).with_mods(cmd), v).unwrap();
    assert_eq!(s.tool_id(), "pen");
}

/// Cmd borrows whichever selection tool was chosen last, whatever the chosen tool is.
#[test]
fn cmd_borrows_the_last_used_selection_tool() {
    use designcraft_tools::{Mods, PointerEvent, PointerKind};
    let mut s = session();
    let v = ViewInfo::at_zoom(1.0);
    let cmd = Mods { cmd: true, ..Mods::default() };
    let borrowed = |s: &mut Session| {
        s.pointer(&PointerEvent::new(PointerKind::Down, 400.0, 400.0).with_mods(cmd), v).unwrap();
        let id = s.temp_tool.as_ref().map(|t| t.id());
        s.pointer(&PointerEvent::new(PointerKind::Up, 400.0, 400.0).with_mods(cmd), v).unwrap();
        id
    };
    s.set_tool("directSelection");
    s.set_tool("rectangle");
    assert_eq!(borrowed(&mut s), Some("directSelection"));
    s.set_tool("selection");
    s.set_tool("rectangle");
    assert_eq!(borrowed(&mut s), Some("selection"));
    s.set_tool("directSelection");
    s.set_tool("pen");
    assert_eq!(borrowed(&mut s), Some("directSelection"));
}

#[test]
fn tool_gesture_creates_one_undo_step() {
    let mut s = session();
    s.set_tool("rectangle");
    let v = ViewInfo::at_zoom(1.0);
    use designcraft_tools::{PointerEvent, PointerKind};
    s.pointer(&PointerEvent::new(PointerKind::Down, 10.0, 10.0), v).unwrap();
    for i in 1..10 {
        s.pointer(&PointerEvent::new(PointerKind::Drag, 10.0 + i as f64 * 10.0, 10.0 + i as f64 * 5.0), v).unwrap();
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, 100.0, 55.0), v).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), 1);
    assert_eq!(st.doc.spreads[0].items.len(), 1);
    assert_eq!(st.doc.spreads[0].items[0].bounds(), designcraft_geom::Rect::new(10.0, 10.0, 100.0, 55.0));
    // Move with the selection tool.
    s.set_tool("selection");
    s.pointer(&PointerEvent::new(PointerKind::Down, 50.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 70.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 90.0, 40.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 90.0, 40.0), v).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), 2);
    assert_eq!(st.doc.spreads[0].items[0].bounds().x0, 50.0);
    // A move that ends near the margin snaps to it.
    s.pointer(&PointerEvent::new(PointerKind::Down, 60.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 48.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 48.0, 30.0), v).unwrap();
    // The rectangle tool's 1 pt centered stroke sits half a point outside the path.
    // The visible edge lands on the 36 pt margin.
    let it = &s.doc().unwrap().doc.spreads[0].items[0];
    assert!((it.visible_bounds().x0 - 36.0).abs() < 1e-6, "visible {:?} path {:?}", it.visible_bounds(), it.bounds());
}

/// A command that runs another one (Cut runs Clear) recorded an undo step for each, so the
/// second Undo did nothing. One command is one step, whatever it runs inside.
#[test]
fn a_command_that_runs_another_is_one_undo_step() {
    let mut s = session();
    let one_step = |s: &mut Session, id: &str, p: Value, label: &str| {
        let (before, steps) = {
            let st = s.doc().unwrap();
            (st.doc.clone(), st.history.undo.len())
        };
        s.execute(id, &p).unwrap();
        let st = s.doc().unwrap();
        assert_ne!(st.doc, before, "{id} changes the document");
        assert_eq!(st.history.undo.len(), steps + 1, "{id}");
        assert_eq!(st.history.undo.last().unwrap().label, label);
        let after = st.doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before, "one Undo takes {id} back");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, after, "one Redo brings {id} back");
        s.execute("edit.undo", &json!({})).unwrap();
    };
    // Pasting text runs Type.
    let story = s.execute("frame.create", &json!({"rect": [36, 300, 300, 400], "content": "text", "text": "ab"})).unwrap()["story"].clone();
    s.execute("text.select", &json!({"story": story, "anchor": 2})).unwrap();
    one_step(&mut s, "edit.paste", json!({"text": "c"}), "Paste");
    let a = s.execute("frame.create", &json!({"rect": [36, 36, 136, 136]})).unwrap()["id"].clone();
    s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap();
    for (id, label) in
        [("edit.duplicate", "Duplicate"), ("object.bringToFront", "Bring to Front"), ("edit.cut", "Cut"), ("edit.pasteInPlace", "Paste in Place")]
    {
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        one_step(&mut s, id, json!({}), label);
    }
    // A script that runs Cut is three commands deep and still one step.
    let script = format!("selection.set {{\"ids\": [{a}]}}\nedit.cut {{}}");
    one_step(&mut s, "script.run", json!({"text": script}), "Run Script");
}

/// A command that fails partway strands no edits without an undo step: a script that stops at
/// a bad step rolls its earlier steps back (#709). A command that leaves its partial edit in the
/// document records an undo step for it (next test).
#[test]
fn a_command_that_fails_partway_strands_no_edits() {
    let mut s = session();
    let before = s.doc().unwrap().doc.clone();
    let script = "frame.create {\"rect\": [36, 36, 136, 136]}\nnope.command";
    assert!(s.execute("script.run", &json!({"text": script})).is_err());
    let st = s.doc().unwrap();
    assert_eq!(st.doc, before, "the first step was rolled back");
    assert!(st.history.undo.is_empty());
    assert!(s.execute("edit.undo", &json!({})).is_err(), "nothing was left to undo");
    // The next command records its step as usual.
    s.execute("frame.create", &json!({"rect": [36, 36, 136, 136]})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), 1);
}

/// The same for a command that fails after a command it ran changed the document: that one
/// leaves the step to it, so it has to record one.
#[test]
fn a_command_that_fails_after_the_one_it_ran_can_be_undone() {
    let mut s = session();
    let empty = s.execute("frame.create", &json!({"rect": [36, 36, 136, 136], "content": "unassigned"})).unwrap()["id"].clone();
    let text = s.execute("frame.create", &json!({"rect": [36, 300, 300, 400], "content": "text", "text": "abc"})).unwrap()["id"].clone();
    s.execute("selection.set", &json!({"ids": [text]})).unwrap();
    let (before, steps) = {
        let st = s.doc().unwrap();
        (st.doc.clone(), st.history.undo.len())
    };
    // Fill with Placeholder Text turns the empty frame into a text frame, then finds no such story.
    assert!(s.execute("type.fillWithPlaceholder", &json!({"frame": empty, "story": 99999})).is_err());
    let st = s.doc().unwrap();
    assert_ne!(st.doc, before, "the frame was converted");
    assert_eq!(st.history.undo.len(), steps + 1);
    assert_eq!(st.history.undo.last().unwrap().label, "Fill with Placeholder Text");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
}

/// A script's steps still record as it runs: an Undo step takes back the step before it, not
/// what was done before the script. The script stays one step.
#[test]
fn a_script_that_undoes_its_own_step_is_one_undo_step() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 136, 136]})).unwrap();
    let script = "frame.create {\"rect\": [200, 36, 300, 136]}\nframe.create {\"rect\": [400, 36, 500, 136]}\nedit.undo {}";
    s.execute("script.run", &json!({"text": script})).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.doc.spreads[0].items.len(), 2, "the frame from before the script and the script's first");
    assert_eq!(st.history.undo.len(), 2);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), 1);
}

/// A command that is no undo step itself (placing the caret) leaves the steps of the commands
/// it runs alone: turning the empty frame into a text frame can be undone.
#[test]
fn a_command_without_an_undo_step_keeps_the_steps_of_those_it_runs() {
    let mut s = session();
    let id = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "unassigned"})).unwrap()["id"].clone();
    s.execute("text.placeCaret", &json!({"frame": id, "point": [50, 50]})).unwrap();
    let labels: Vec<&str> = s.doc().unwrap().history.undo.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["Create Frame", "Content"]);
}

#[test]
fn shift_move_does_not_take_the_other_axis() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [80.0, 100.0, 140.0, 140.0], "content": "unassigned"})).unwrap();
    s.execute("guide.add", &json!({"orientation": "horizontal", "position": 102.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let shift = designcraft_tools::Mods { shift: true, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 120.0).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 160.0, 121.0).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 160.0, 121.0).with_mods(shift), v).unwrap();
    let b = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    assert!((b.y0 - 100.0).abs() < 1e-6, "locked axis jumped to {b:?}");
    assert!(b.x0 > 80.0);
}

#[test]
fn cmd_move_does_not_snap() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [80.0, 100.0, 140.0, 140.0], "content": "unassigned"})).unwrap();
    s.execute("guide.add", &json!({"orientation": "horizontal", "position": 102.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let cmd = designcraft_tools::Mods { cmd: true, shift: false, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, 110.0, 120.0).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 160.0, 121.0).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 160.0, 121.0).with_mods(cmd), v).unwrap();
    let b = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    assert!((b.y0 - 101.0).abs() < 1e-6, "cmd move snapped to {b:?}");
    assert!(b.x0 > 80.0);
}

#[test]
fn cmd_resize_still_snaps_to_a_guide() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [100.0, 100.0, 200.0, 180.0], "content": "unassigned"})).unwrap();
    s.execute("guide.add", &json!({"orientation": "vertical", "position": 250.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let b0 = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    let right = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Point::new(b0.x1, b0.center().y));
    let cmd = designcraft_tools::Mods { cmd: true, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, right.x, right.y).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, right.x + 47.0, right.y).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, right.x + 47.0, right.y).with_mods(cmd), v).unwrap();
    let it = &s.doc().unwrap().doc.spreads[0].items[0];
    let b = it.bounds();
    assert!((b.x1 - 250.0).abs() < 1e-6, "cmd resize did not snap the moving edge: {b:?}");
    assert!((b.x0 - b0.x0).abs() < 1e-6 && (b.y0 - b0.y0).abs() < 1e-6 && (b.y1 - b0.y1).abs() < 1e-6, "{b:?}");
    let expect = (b.x1 - b.x0).abs() / b0.width() * (b.y1 - b.y0).abs() / b0.height();
    assert!((it.stroke.weight - expect.sqrt()).abs() < 1e-6, "content scale weight {} want {}", it.stroke.weight, expect.sqrt());
}

#[test]
fn shift_move_on_a_turned_spread_stays_on_the_line() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [80.0, 100.0, 140.0, 140.0], "content": "unassigned"})).unwrap();
    s.execute("layout.rotateSpreadView", &json!({"angle": 90})).unwrap();
    // Vertical guide sits on the spread axis that a screen-horizontal Shift lock must not offer.
    s.execute("guide.add", &json!({"orientation": "vertical", "position": 82.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let b0 = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    let c = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), b0.center());
    let shift = designcraft_tools::Mods { shift: true, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, c.x, c.y).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, c.x + 60.0, c.y + 1.0).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, c.x + 60.0, c.y + 1.0).with_mods(shift), v).unwrap();
    let b = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    assert!((b.x0 - b0.x0).abs() < 1e-6, "locked screen axis jumped to {b:?}");
    assert!((b.y0 - b0.y0).abs() > 1.0, "free axis did not move: {b:?}");
}

#[test]
fn pages_and_layers() {
    let mut s = session();
    s.execute("layout.pages.insert", &json!({"count": 3})).unwrap();
    assert_eq!(s.doc().unwrap().doc.page_count(), 5);
    s.execute("layout.pages.delete", &json!({"pages": [0]})).unwrap();
    assert_eq!(s.doc().unwrap().doc.page_count(), 4);
    s.execute("layer.new", &json!({"name": "Text"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.layers[0].name, "Text");
}

#[test]
fn sample_document_is_valid_and_composes() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    let st = s.doc().unwrap();
    st.doc.check().unwrap();
    assert_eq!(st.doc.page_count(), 4);
    let i = s.execute("document.inspect", &json!({})).unwrap();
    assert!(i["stories"].as_array().unwrap().len() >= 8);
    // Save/load round trip.
    let bytes = cmd::file_bytes(&s.doc().unwrap().doc).unwrap();
    let back = cmd::file_from(&bytes).unwrap();
    assert_eq!(back.page_count(), 4);
    assert_eq!(back.assets.len(), s.doc().unwrap().doc.assets.len());
    assert!(back.assets.values().all(|a| !a.data.is_empty()));
}

#[test]
fn every_command_has_metadata() {
    let mut ids = std::collections::HashSet::new();
    for c in command_specs() {
        assert!(ids.insert(c.id), "duplicate command {}", c.id);
        assert!(!c.label.is_empty() && !c.params.is_empty(), "{}", c.id);
    }
    assert!(command_specs().len() > 60);
}

#[test]
fn align_and_distribute() {
    let mut s = session();
    let mut ids = vec![];
    for (x, w) in [(10.0, 20.0), (100.0, 40.0), (300.0, 10.0)] {
        let r = s.execute("frame.create", &json!({"rect": [x, 50.0 + x / 10.0, x + w, 100.0], "content": "unassigned"})).unwrap();
        ids.push(r["id"].as_u64().unwrap());
    }
    s.execute("object.align", &json!({"ids": ids, "edge": "top"})).unwrap();
    let st = s.doc().unwrap();
    let tops: Vec<f64> = ids.iter().map(|i| st.doc.item(designcraft_doc::ItemId(*i)).unwrap().bounds().y0).collect();
    assert!(tops.iter().all(|t| (*t - 51.0).abs() < 1e-9), "{tops:?}");
    s.execute("object.distribute", &json!({"ids": ids, "by": "spacing"})).unwrap();
    let st = s.doc().unwrap();
    let b: Vec<designcraft_geom::Rect> = ids.iter().map(|i| st.doc.item(designcraft_doc::ItemId(*i)).unwrap().bounds()).collect();
    assert!(((b[1].x0 - b[0].x1) - (b[2].x0 - b[1].x1)).abs() < 1e-9);
    s.execute("object.align", &json!({"ids": [ids[0]], "edge": "left", "to": "margins"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(ids[0])).unwrap().bounds().x0, 36.0);
}

#[test]
fn pen_draws_and_direct_selection_edits() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    let v = ViewInfo::at_zoom(1.0);
    s.set_tool("pen");
    let click = |s: &mut Session, x: f64, y: f64| {
        s.pointer(&PointerEvent::new(PointerKind::Down, x, y), v).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Up, x, y), v).unwrap();
    };
    click(&mut s, 100.0, 100.0);
    click(&mut s, 200.0, 100.0);
    // A smooth point (drag).
    s.pointer(&PointerEvent::new(PointerKind::Down, 200.0, 200.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 240.0, 200.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 240.0, 200.0), v).unwrap();
    click(&mut s, 100.0, 100.0); // close
    let st = s.doc().unwrap();
    let it = &st.doc.spreads[0].items[0];
    assert_eq!(it.path.subpaths[0].anchors.len(), 3);
    assert!(it.path.subpaths[0].closed);
    assert!(it.path.subpaths[0].anchors[2].has_out());
    let id = it.id;
    // Direct Selection: drag the first anchor.
    s.set_tool("directSelection");
    s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 100.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 90.0, 80.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 90.0, 80.0), v).unwrap();
    let a = s.doc().unwrap().doc.item(id).unwrap().path.subpaths[0].anchors[0].p;
    // The pen placed this anchor on the baseline at y 96. Dragging toward (90, 80)
    // proposes y 76, and the baseline at 72 is inside the zone, so the anchor lands at (90, 72).
    assert_eq!(a, designcraft_geom::Point::new(90.0, 72.0));
}

#[test]
fn rotate_scale_shear_and_eyedropper() {
    let mut s = session();
    let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
    let b = s.execute("frame.create", &json!({"rect": [300, 100, 340, 140], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("transform.rotate", &json!({"ids": [a], "angle": 90})).unwrap();
    let r = s.doc().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().bounds();
    assert!((r.width() - 50.0).abs() < 1e-6 && (r.height() - 100.0).abs() < 1e-6, "{r:?}");
    s.execute("transform.scale", &json!({"ids": [b], "sx": 2.0})).unwrap();
    let r = s.doc().unwrap().doc.item(designcraft_doc::ItemId(b)).unwrap().bounds();
    assert!((r.width() - 80.0).abs() < 1e-6, "{r:?}");
    s.execute("selection.set", &json!({"ids": [b]})).unwrap();
    s.execute("object.matchAttributes", &json!({"from": a})).unwrap();
    assert_eq!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(b)).unwrap().fill.swatch, "C=100 M=0 Y=0 K=0");
    s.execute("transform.shear", &json!({"ids": [b], "angle": 20})).unwrap();
}

#[test]
fn place_gun_click_drag_and_into_frame() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    let b64 = cmd::base64_encode(&png);
    s.execute("place.load", &json!({"base64": b64, "name": "a.png"})).unwrap();
    assert_eq!(s.tool_id(), "placeGun");
    let v = ViewInfo::at_zoom(1.0);
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 100.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 180.0, 300.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 180.0, 300.0), v).unwrap();
    let st = s.doc().unwrap();
    let it = st.doc.spreads[0].items.last().unwrap();
    // Dragged 80×200 → fit proportionally: 80×40.
    assert_eq!(it.bounds(), designcraft_geom::Rect::new(100.0, 100.0, 180.0, 140.0));
    assert_eq!(s.tool_id(), "selection");
    // Into an empty frame.
    let f = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    s.execute("place.load", &json!({"base64": b64})).unwrap();
    s.execute("place.drop", &json!({"frame": f})).unwrap();
    let graphic_xf = |s: &Session| match &s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().content {
        designcraft_doc::Content::Graphic(g) => g.xf,
        _ => panic!("not a graphic"),
    };
    let placed = graphic_xf(&s);
    // Dragging the top edge down crops the top only: the image stays where it is.
    s.execute("selection.set", &json!({"ids": [f]})).unwrap();
    s.execute("transform.resize", &json!({"from": [300, 300, 400, 400], "to": [300, 340, 400, 400]})).unwrap();
    assert_eq!(graphic_xf(&s), placed);
}

#[test]
fn preflight_reports_overset_and_missing_fonts() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 100, 50], "content": "text", "text": "This text will not fit in such a tiny frame at all."}))
        .unwrap();
    s.execute("text.select", &json!({"story": s.doc().unwrap().doc.stories.keys().next().unwrap().0, "anchor": 0, "focus": 4})).unwrap();
    s.execute("type.char", &json!({"attrs": {"fontFamily": "Nonexistent Sans"}})).unwrap();
    let r = s.execute("preflight.run", &json!({})).unwrap();
    let kinds: Vec<&str> = r["issues"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"overset") && kinds.contains(&"missingFont"), "{kinds:?}");
    assert_eq!(r["errors"], 2);
}

#[test]
fn hyperlinks_and_bookmarks_reach_the_pdf() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    s.execute("selection.set", &json!({"ids": [39]})).unwrap();
    s.execute("hyperlink.create", &json!({"url": "https://example.com/designcraft"})).unwrap();
    let d = s.execute("document.inspect", &json!({})).unwrap();
    let sid = d["stories"][0]["id"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
    s.execute("hyperlink.create", &json!({"page": 3})).unwrap();
    s.execute("bookmark.add", &json!({"name": "Cover", "page": 1})).unwrap();
    s.execute("bookmark.add", &json!({"name": "Feature", "page": 2})).unwrap();
    assert_eq!(s.execute("hyperlink.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
    let r = s.execute("file.exportPdf", &json!({})).unwrap();
    let bytes = cmd::base64_decode(r["base64"].as_str().unwrap());
    let pdf = hayro_syntax::Pdf::new(bytes).expect("valid pdf");
    assert_eq!(pdf.pages().len(), 4);
    // Round trip through the native format keeps them.
    let back = cmd::file_from(&cmd::file_bytes(&s.doc().unwrap().doc).unwrap()).unwrap();
    assert_eq!(back.hyperlinks.len(), 2);
    assert_eq!(back.bookmarks.len(), 2);
}

#[test]
fn serialized_document_opens_again() {
    // The web app saves (downloads) what `file.serialize` returns: it must be the file's exact bytes.
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Saved on the web"})).unwrap();
    let r = s.execute("file.serialize", &json!({})).unwrap();
    let bytes = cmd::base64_decode(r["base64"].as_str().expect("base64"));
    assert_eq!(bytes, cmd::to_bytes(&s.doc().unwrap().doc).unwrap());
    s.execute("file.openBytes", &json!({"name": "copy", "base64": r["base64"]})).unwrap();
    let d = s.execute("document.inspect", &json!({})).unwrap();
    assert_eq!(d["stories"][0]["preview"], "Saved on the web");
}

#[test]
fn active_layer_survives_undoing_new_layer() {
    let mut s = session();
    let layer_of = |s: &mut Session, id: u64| {
        let d = s.execute("document.inspect", &json!({})).unwrap();
        let items = d["spreads"].as_array().unwrap().iter().flat_map(|sp| sp["items"].as_array().unwrap().clone()).collect::<Vec<_>>();
        let it = items.into_iter().find(|i| i["id"] == id).unwrap();
        let layers: Vec<Value> = d["layers"].as_array().unwrap().iter().map(|l| l["id"].clone()).collect();
        (it["layer"].clone(), layers)
    };
    // New Layer, then Undo: the new layer is gone, so it can't stay the active one.
    s.execute("layer.new", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Visible"})).unwrap();
    let (layer, layers) = layer_of(&mut s, f["id"].as_u64().unwrap());
    assert!(layers.contains(&layer), "frame on layer {layer} not in {layers:?}");
    // Delete Unused Layers removing the (empty) active layer.
    s.execute("layer.new", &json!({})).unwrap();
    s.execute("layer.deleteUnused", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect": [36, 236, 300, 400], "content": "text", "text": "Also visible"})).unwrap();
    let (layer, layers) = layer_of(&mut s, f["id"].as_u64().unwrap());
    assert!(layers.contains(&layer), "frame on layer {layer} not in {layers:?}");
}

#[test]
fn step_and_repeat_grid() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 66, 66], "content": "unassigned"})).unwrap();
    let r = s.execute("edit.stepAndRepeat", &json!({"rows": 3, "columns": 4, "dx": 40, "dy": 40})).unwrap();
    assert_eq!(r["created"], 11);
    assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), 12);
    let last = s.doc().unwrap().doc.spreads[0].items.last().unwrap().bounds();
    assert_eq!(last.x0, 36.0 + 3.0 * 40.0);
    assert_eq!(last.y0, 36.0 + 2.0 * 40.0);
}

/// rows × columns once overflowed: a panic in debug builds, and in release builds a product that
/// wrapped to 0 passed the 1000-copy limit and started a grid of 2⁶⁴ copies.
#[test]
fn step_and_repeat_with_a_huge_grid_makes_no_grid() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 66, 66], "content": "unassigned"})).unwrap();
    let items = |s: &Session| s.doc().unwrap().doc.spreads[0].items.len();
    let before = items(&s);
    // Counts past the limit are an error, and nothing is copied.
    assert!(s.execute("edit.stepAndRepeat", &json!({"rows": 4_294_967_296_u64, "columns": 4_294_967_296_u64})).is_err());
    assert_eq!(items(&s), before);
}

/// The column count is the caller's and sizes a list every time the page's columns are laid out.
#[test]
fn new_document_caps_the_column_count() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"columns": 4_000_000_000_u64})).unwrap();
    assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].columns.count, 216);
    // Past u32 the count wrapped (2³² + 3 became 3).
    s.execute("file.new", &json!({"columns": 4_294_967_299_u64})).unwrap();
    assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].columns.count, 216);
}

#[test]
fn snippets_roundtrip_between_documents() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    s.execute("selection.set", &json!({"ids": [39]})).unwrap();
    let snip = s.execute("snippet.export", &json!({})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("snippet.place", &json!({"base64": snip["base64"], "x": 36.0, "y": 36.0})).unwrap();
    let id = designcraft_doc::ItemId(r["ids"][0].as_u64().unwrap());
    let st = s.doc().unwrap();
    let it = st.doc.item(id).unwrap();
    assert_eq!(it.bounds().x0, 36.0);
    let sid = it.text_frame().unwrap().story;
    assert!(st.doc.story(sid).unwrap().text.starts_with("Every page begins"));
    assert!(st.doc.styles.para("Body").is_some());
    st.doc.check().unwrap();
}

#[test]
fn object_styles_create_and_apply() {
    let mut s = session();
    let a = s.execute("frame.create", &json!({"rect": [36, 36, 136, 136], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.fill", &json!({"swatch": "C=0 M=100 Y=0 K=0"})).unwrap();
    s.execute("object.stroke", &json!({"weight": 4.0})).unwrap();
    s.execute("style.object.create", &json!({"name": "Magenta Box"})).unwrap();
    let b = s.execute("frame.create", &json!({"rect": [200, 36, 300, 136], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("style.object.apply", &json!({"name": "Magenta Box"})).unwrap();
    let d = &s.doc().unwrap().doc;
    let (ia, ib) = (d.item(designcraft_doc::ItemId(a)).unwrap(), d.item(designcraft_doc::ItemId(b)).unwrap());
    assert_eq!(ia.fill, ib.fill);
    assert_eq!(ib.stroke.weight, 4.0);
    assert_eq!(ib.object_style, "Magenta Box");
}

#[test]
fn object_style_effects_follow_the_style_until_overridden() {
    use designcraft_doc::{BASIC_GRAPHICS_FRAME, ItemId};
    let mut s = session();
    let frame = |s: &mut Session, x: f64| {
        let r = s.execute("frame.create", &json!({"rect": [x, 36, x + 100.0, 136], "content": "unassigned"})).unwrap();
        r["id"].as_u64().unwrap()
    };
    let item = |s: &Session, id: u64| s.doc().unwrap().doc.item(ItemId(id)).unwrap().clone();
    let shadow = |s: &Session, id: u64| item(s, id).effects.drop_shadow;
    let a = frame(&mut s, 36.0);
    s.execute("object.dropShadow", &json!({"on": true, "distance": 0, "size": 12, "opacity": 0.28, "ids": [a]})).unwrap();
    s.execute("selection.set", &json!({"ids": [a]})).unwrap();
    s.execute("style.object.create", &json!({"name": "Photo", "fromSelection": true})).unwrap();
    let b = frame(&mut s, 200.0);
    let c = frame(&mut s, 360.0);
    s.execute("style.object.apply", &json!({"name": "Photo", "ids": [a, b, c]})).unwrap();
    assert_eq!(shadow(&s, b), shadow(&s, a));
    assert!(shadow(&s, b).on && shadow(&s, b).size == 12.0);
    s.execute("object.dropShadow", &json!({"on": true, "size": 30, "ids": [c]})).unwrap();

    // Editing the style updates the items that follow it; `c`'s local shadow stays.
    let r = s.execute("style.object.edit", &json!({"name": "Photo", "dropShadow": {"size": 20}})).unwrap();
    assert_eq!(r["updated"], 2);
    assert_eq!((shadow(&s, a).size, shadow(&s, b).size, shadow(&s, c).size), (20.0, 20.0, 30.0));
    assert_eq!(shadow(&s, a).opacity, 0.28);
    s.execute("style.object.clearOverrides", &json!({"ids": [c]})).unwrap();
    assert_eq!(shadow(&s, c), shadow(&s, a));

    // Saved and opened again, the style keeps its effects.
    let bytes = s.execute("file.serialize", &json!({})).unwrap();
    let mut back = Session::new();
    back.execute("file.openBytes", &json!({"name": "copy", "base64": bytes["base64"]})).unwrap();
    let d = &back.doc().unwrap().doc;
    assert_eq!(d.styles.object_style("Photo").unwrap().effects.as_ref().unwrap().drop_shadow.size, 20.0);
    assert!(d.styles.object_style(BASIC_GRAPHICS_FRAME).unwrap().effects.is_none());

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(shadow(&s, c).size, 30.0);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!((shadow(&s, a).size, shadow(&s, b).size), (12.0, 12.0));
    assert_eq!(s.doc().unwrap().doc.styles.object_style("Photo").unwrap().effects.as_ref().unwrap().drop_shadow.size, 12.0);

    // A style that didn't set effects gives them to the items using it that have no effects of
    // their own, and to new frames that get it by default; `null` stops setting them.
    let e = frame(&mut s, 36.0);
    let f = frame(&mut s, 200.0);
    s.execute("style.object.apply", &json!({"name": BASIC_GRAPHICS_FRAME, "ids": [e, f]})).unwrap();
    s.execute("object.dropShadow", &json!({"on": true, "ids": [e]})).unwrap();
    let r = s.execute("style.object.edit", &json!({"name": BASIC_GRAPHICS_FRAME, "feather": 4})).unwrap();
    assert_eq!(r["updated"], 1);
    assert!(item(&s, e).effects.drop_shadow.on && item(&s, e).effects.feather == 0.0);
    assert_eq!(item(&s, f).effects, designcraft_doc::Effects { feather: 4.0, ..Default::default() });
    let g = s.execute("frame.create", &json!({"rect": [36, 300, 136, 400], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    assert_eq!((item(&s, g).object_style.as_str(), item(&s, g).effects.feather), (BASIC_GRAPHICS_FRAME, 4.0));
    s.execute("style.object.edit", &json!({"name": BASIC_GRAPHICS_FRAME, "effects": null})).unwrap();
    assert!(s.doc().unwrap().doc.styles.object_style(BASIC_GRAPHICS_FRAME).unwrap().effects.is_none());
    assert_eq!(item(&s, f).effects.feather, 4.0);

    // Nothing to change: no edit, no undo step.
    let rev = s.doc().unwrap().revision;
    s.execute("style.object.edit", &json!({"name": BASIC_GRAPHICS_FRAME, "effects": null})).unwrap();
    s.execute("style.object.clearOverrides", &json!({"ids": [a, b]})).unwrap();
    assert_eq!(s.doc().unwrap().revision, rev);
}

#[test]
fn object_style_effects_reach_grouped_and_anchored_items() {
    use designcraft_doc::ItemId;
    let mut s = session();
    let frame = |s: &mut Session, x: f64| {
        s.execute("frame.create", &json!({"rect": [x, 36, x + 50.0, 86], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap()
    };
    let (a, b, c) = (frame(&mut s, 36.0), frame(&mut s, 100.0), frame(&mut s, 200.0));
    s.execute("style.object.create", &json!({"name": "Photo", "fromSelection": false, "dropShadow": {"on": true}})).unwrap();
    s.execute("style.object.apply", &json!({"name": "Photo", "ids": [a, b, c]})).unwrap();
    s.execute("object.group", &json!({"ids": [a, b]})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [36, 200, 400, 400], "content": "text", "text": "Before after."})).unwrap();
    let sid = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
    s.execute("anchored.insert", &json!({"ids": [c], "story": sid.0, "pos": 7})).unwrap();

    let r = s.execute("style.object.edit", &json!({"name": "Photo", "dropShadow": {"size": 25}})).unwrap();
    assert_eq!(r["updated"], 3);
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.item(ItemId(a)).unwrap().effects.drop_shadow.size, 25.0);
    assert_eq!(d.story(sid).unwrap().objects[0].item.effects.drop_shadow.size, 25.0);

    // Values that aren't finite numbers are refused (an f32 opacity of 1e39 is infinite); the
    // others are clamped, and the document still saves and opens.
    assert!(s.execute("style.object.edit", &json!({"name": "Photo", "dropShadow": {"opacity": 1e39}})).is_err());
    assert!(s.execute("object.dropShadow", &json!({"opacity": 1e39, "ids": [b]})).is_err());
    assert!(s.execute("style.object.edit", &json!({"name": "Photo", "dropShadow": {"size": "big"}})).is_err());
    s.execute("style.object.edit", &json!({"name": "Photo", "dropShadow": {"opacity": 3, "size": -5}})).unwrap();
    s.execute("object.innerShadow", &json!({"on": true, "distance": -2, "ids": [b]})).unwrap();
    let d = &s.doc().unwrap().doc;
    let ds = &d.styles.object_style("Photo").unwrap().effects.as_ref().unwrap().drop_shadow;
    assert_eq!((ds.opacity, ds.size), (1.0, 0.0));
    assert_eq!(d.item(ItemId(b)).unwrap().effects.inner_shadow.distance, 0.0);
    let bytes = s.execute("file.serialize", &json!({})).unwrap();
    let mut back = Session::new();
    back.execute("file.openBytes", &json!({"name": "copy", "base64": bytes["base64"]})).unwrap();
    assert_eq!(back.doc().unwrap().doc.styles.object_style("Photo").unwrap().effects.as_ref().unwrap().drop_shadow.opacity, 1.0);
}

/// Unicode text per page, through hayro's interpreter (ToUnicode / ActualText).
fn pdf_text(bytes: &[u8]) -> Vec<String> {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    struct Ex(String);
    impl Device<'_> for Ex {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, _: &kurbo::BezPath, _: kurbo::Affine, _: &Paint<'_>, _: &PathDrawMode) {}
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, g: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {
            match g.as_unicode() {
                Some(hayro_cmap::BfString::Char(c)) => self.0.push(c),
                Some(hayro_cmap::BfString::String(s)) => self.0.push_str(&s),
                None => self.0.push('\u{FFFD}'),
            }
        }
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("valid PDF");
    let cache = InterpreterCache::new();
    pdf.pages()
        .iter()
        .map(|page| {
            let mut ctx =
                Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), &cache, pdf.xref(), InterpreterSettings::default());
            let mut ex = Ex(String::new());
            interpret_page(page, &mut ctx, &mut ex);
            ex.0
        })
        .collect()
}

#[test]
fn export_pdf_of_the_sample() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    let r = s.execute("file.exportPdf", &json!({"bleed": true, "marks": true})).unwrap();
    assert_eq!(r["pages"], 4);
    let bytes = cmd::base64_decode(r["base64"].as_str().unwrap());
    assert!(bytes.starts_with(b"%PDF-"));
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).expect("parse");
    assert_eq!(pdf.pages().len(), 4);
    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.contains("/FontFile2") || raw.contains("/FontFile3"), "fonts embedded");
    let text = pdf_text(&bytes);
    let all: String = text.join("\n");
    assert!(all.contains("Notes on the Grid"), "{all}");
    // Page range + spreads + file output.
    let dir = std::env::temp_dir().join(format!("dc-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.pdf");
    let r = s.execute("file.exportPdf", &json!({"path": path.to_string_lossy(), "pages": "2-3", "spreads": true})).unwrap();
    assert_eq!(r["pages"], 1, "pages 2–3 are one spread");
    assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-"));
    std::fs::remove_dir_all(&dir).ok();
    assert!(s.execute("file.exportPdf", &json!({"pages": "9"})).is_err());
    assert!(s.execute("file.exportPdf", &json!({"standard": "bogus"})).is_err());
}

#[test]
fn object_set_flags_and_wrap_invert() {
    let mut s = session();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200]})).unwrap();
    let id = r["id"].as_u64().unwrap();
    let item = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).cloned().unwrap();
    s.execute("object.setFlags", &json!({"ids": [id], "hidden": true})).unwrap();
    assert!(item(&s).hidden);
    s.execute("object.setFlags", &json!({"ids": [id], "hidden": false, "locked": true})).unwrap();
    assert!(!item(&s).hidden && item(&s).locked);
    s.execute("object.setFlags", &json!({"ids": [id], "locked": false})).unwrap();
    assert!(!item(&s).locked);
    s.execute("selection.set", &json!({"ids": [id]})).unwrap();
    s.execute("object.textWrap", &json!({"mode": "boundingBox", "invert": true})).unwrap();
    assert!(item(&s).wrap.invert);
    // Side and contour type keep the mode; contour types that need the image aren't accepted.
    s.execute("object.textWrap", &json!({"mode": "contour"})).unwrap();
    s.execute("object.textWrap", &json!({"side": "largestArea", "contour": "graphicFrame"})).unwrap();
    let w = item(&s).wrap;
    assert_eq!(
        (w.mode, w.side, w.contour),
        (designcraft_doc::WrapMode::Contour, designcraft_doc::WrapSide::LargestArea, designcraft_doc::ContourType::GraphicFrame)
    );
    assert!(s.execute("object.textWrap", &json!({"contour": "detectEdges"})).is_err());
    assert!(s.execute("object.textWrap", &json!({"side": "middle"})).is_err());
}

#[test]
fn text_wrap_on_selected_content_sets_the_graphic_wrap() {
    let mut s = session();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    let f = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    s.execute("place.load", &json!({"base64": cmd::base64_encode(&png)})).unwrap();
    s.execute("place.drop", &json!({"frame": f})).unwrap();
    let item = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).cloned().unwrap();
    s.execute("selection.set", &json!({"ids": [f], "content": true})).unwrap();
    s.execute("object.textWrap", &json!({"mode": "boundingBox", "offset": 4})).unwrap();
    let it = item(&s);
    assert_eq!(it.wrap.mode, designcraft_doc::WrapMode::None, "the frame keeps no wrap");
    let g = it.graphic().unwrap();
    assert_eq!((g.wrap.mode, g.wrap.offsets), (designcraft_doc::WrapMode::BoundingBox, [4.0; 4]));
    // The frame selected: the frame's own wrap.
    s.execute("selection.set", &json!({"ids": [f]})).unwrap();
    s.execute("object.textWrap", &json!({"mode": "jumpObject"})).unwrap();
    let it = item(&s);
    assert_eq!((it.wrap.mode, it.graphic().unwrap().wrap.mode), (designcraft_doc::WrapMode::JumpObject, designcraft_doc::WrapMode::BoundingBox));
}

#[test]
fn custom_wrap_path_follows_resize_and_yields_to_offsets() {
    let mut s = session();
    let r = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap();
    let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
    s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
    s.execute("object.textWrap", &json!({"mode": "boundingBox"})).unwrap();
    let inner = s.doc().unwrap().doc.item(id).unwrap().path.clone();
    let custom = designcraft_geom::PathData::single(designcraft_geom::SubPath::polyline(
        &[designcraft_geom::Point::new(0.0, 0.0), designcraft_geom::Point::new(40.0, 0.0), designcraft_geom::Point::new(0.0, 100.0)],
        true,
    ));
    s.edit(|d, _| {
        if let Some(it) = d.item_mut(id) {
            it.wrap.path = Some(custom.clone());
        }
        Ok(())
    })
    .unwrap();
    let wrap_path = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().wrap.path.clone();
    // Side and mode keep the boundary.
    s.execute("object.textWrap", &json!({"side": "rightSide"})).unwrap();
    s.execute("object.textWrap", &json!({"mode": "contour"})).unwrap();
    assert_eq!(wrap_path(&s), Some(custom.clone()));
    // Resizing scales it with the frame's path.
    let b = inner.bounds().unwrap();
    let to = [b.x0, b.y0, b.x0 + 2.0 * b.width(), b.y1];
    s.execute("transform.resize", &json!({"from": [b.x0, b.y0, b.x1, b.y1], "to": to})).unwrap();
    let w = wrap_path(&s).unwrap().bounds().unwrap();
    assert!((w.width() - 80.0).abs() < 1e-6 && (w.height() - 100.0).abs() < 1e-6, "{w:?}");
    // An offset rebuilds the wrap from the frame.
    s.execute("object.textWrap", &json!({"offset": 6})).unwrap();
    assert_eq!(wrap_path(&s), None);
}

#[test]
fn gridify_while_drawing_frames() {
    use designcraft_tools::{Mods, PointerEvent, PointerKind, ToolKey};
    let mut s = session();
    let n0 = s.doc().unwrap().doc.spreads[0].items.len();
    s.set_tool("rectangleFrame");
    let v = ViewInfo::at_zoom(1.0);
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 100.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 312.0, 212.0), v).unwrap();
    for k in [ToolKey::Right, ToolKey::Right, ToolKey::Up, ToolKey::Left] {
        assert!(s.tool_key(k, Mods::default(), v).unwrap(), "arrow keys gridify while dragging");
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, 312.0, 212.0), v).unwrap();
    let d = &s.doc().unwrap().doc;
    let new: Vec<_> = d.spreads[0].items[n0..].iter().map(|i| i.bounds()).collect();
    assert_eq!(new.len(), 4, "2 columns × 2 rows");
    assert!((new[0].width() - 100.0).abs() < 1e-6 && (new[1].x0 - 212.0).abs() < 1e-6, "12 pt gutters: {new:?}");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), n0, "one undo step");
}

#[test]
fn gap_tool_drag_moves_the_gap() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 300]})).unwrap()["id"].as_u64().unwrap();
    s.set_tool("gap");
    let v = ViewInfo::at_zoom(1.0);
    s.pointer(&PointerEvent::new(PointerKind::Down, 250.0, 200.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 270.0, 205.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 270.0, 205.0), v).unwrap();
    // The gap between the frame and the page's right edge: the frame's right side moved 20.
    let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().bounds();
    assert_eq!(b.x1, 220.0, "{b:?}");
    assert_eq!(s.doc().unwrap().history.undo.len(), 2, "create + one gap move");
}

/// Parameters reach commands from the control channel, MCP and scripts as any JSON value.
/// Commands that add a key to their parameters (`q["fixedLayout"] = …`) panicked on an array.
#[test]
fn commands_take_non_object_params_without_panicking() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    for p in [json!([1, 2]), json!("text"), json!(3), Value::Null] {
        let r = s.execute("file.exportFixedEpub", &p);
        assert!(r.is_ok(), "{p}: {r:?}");
    }
}

/// Load the text from a frame's out port with the Selection tool and click on the pasteboard:
/// a new frame is threaded there and the overset text flows into it.
#[test]
fn threading_with_the_loaded_text_cursor_on_the_sample() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let (sid, first) = d.stories.values().find(|st| st.frames.len() >= 2).map(|st| (st.id, st.frames[0])).expect("a threaded story");
    // Break the thread so the first frame is overset, and select it.
    s.execute("text.unthread", &json!({"frame": first.0})).unwrap();
    assert!(s.cache.get(&s.doc().unwrap().doc, sid, None).is_overset());
    s.execute("selection.set", &json!({"ids": [first.0]})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let layout = s.layout();
    let port = designcraft_tools::thread::port_center(&s.doc().unwrap().doc, &layout, first, true, v.zoom).unwrap();
    let ev = |k, p: designcraft_geom::Point| PointerEvent::new(k, p.x, p.y);
    s.pointer(&ev(PointerKind::Down, port), v).unwrap();
    s.pointer(&ev(PointerKind::Up, port), v).unwrap();
    assert_eq!(s.cursor(port + designcraft_geom::Vec2::new(-2000.0, 0.0), Default::default(), v), designcraft_tools::Cursor::LoadedText);
    // On the pasteboard left of the first spread.
    let b = layout.slots[0].bounds;
    let at = designcraft_geom::Point::new(b.x0 - 300.0, b.y0 + 100.0);
    let undo = s.doc().unwrap().history.undo.len();
    s.pointer(&ev(PointerKind::Down, at), v).unwrap();
    s.pointer(&ev(PointerKind::Up, at), v).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    let frames = st.doc.story(sid).unwrap().frames.clone();
    assert_eq!(frames.len(), 2);
    let new = frames[1];
    assert_eq!(st.selection.items, [new]);
    let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
    assert!(cs.frames.iter().any(|f| f.frame == new && !f.range.is_empty()), "the text flows into the new frame");
    let nb = s.doc().unwrap().doc.item(new).unwrap().bounds();
    assert!((nb.y0 - layout.to_spread(designcraft_doc::SpreadRef::Doc(0), at).y).abs() < 1e-6, "{nb:?}");
}

/// Drag a selection handle of item 0 on spread 0 from the edge point `at` by `by` (canvas units).
fn drag_handle(s: &mut Session, at: designcraft_geom::Point, by: designcraft_geom::Vec2, v: ViewInfo) {
    use designcraft_tools::{PointerEvent, PointerKind};
    let p = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), at);
    s.pointer(&PointerEvent::new(PointerKind::Down, p.x, p.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, p.x + by.x / 2.0, p.y + by.y / 2.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, p.x + by.x, p.y + by.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, p.x + by.x, p.y + by.y), v).unwrap();
}

#[test]
fn text_frame_resize_snaps_to_the_margins() {
    use designcraft_geom::{Point, Vec2};
    for zoom in [1.0, 0.5] {
        let mut s = Session::new();
        s.execute("file.newSample", &json!({})).unwrap();
        let margin = s.doc().unwrap().doc.spreads[0].pages[0].margin_rect();
        let (x0, y0) = (margin.x0 + 40.0, margin.y0 + 40.0);
        s.execute("frame.create", &json!({"rect": [x0, y0, x0 + 200.0, y0 + 200.0], "content": "text"})).unwrap();
        s.set_tool("selection");
        let v = ViewInfo::at_zoom(zoom);
        let b0 = s.doc().unwrap().doc.spreads[0].items.last().unwrap().bounds();
        // Right handle to 1.5 pt short of the right margin.
        drag_handle(&mut s, Point::new(b0.x1, b0.center().y), Vec2::new(margin.x1 - 1.5 - b0.x1, 0.0), v);
        let b = s.doc().unwrap().doc.spreads[0].items.last().unwrap().bounds();
        assert!((b.x1 - margin.x1).abs() < 1e-6, "zoom {zoom}: right edge {} not on the margin {}", b.x1, margin.x1);
        // Bottom handle to 1.5 pt past the bottom margin.
        drag_handle(&mut s, Point::new(b.center().x, b.y1), Vec2::new(0.0, margin.y1 + 1.5 - b.y1), v);
        let b = s.doc().unwrap().doc.spreads[0].items.last().unwrap().bounds();
        assert!((b.y1 - margin.y1).abs() < 1e-6, "zoom {zoom}: bottom edge {} not on the margin {}", b.y1, margin.y1);
        // Top-left corner handle onto the top and left margins.
        drag_handle(&mut s, Point::new(b.x0, b.y0), Vec2::new(margin.x0 + 1.0 - b.x0, margin.y0 - 1.0 - b.y0), v);
        let b = s.doc().unwrap().doc.spreads[0].items.last().unwrap().bounds();
        assert!((b.x0 - margin.x0).abs() < 1e-6 && (b.y0 - margin.y0).abs() < 1e-6, "zoom {zoom}: corner {b:?} not on the margins {margin:?}");
    }
}

#[test]
fn type_tool_frames_snap_to_the_margins() {
    use designcraft_geom::Point;
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    let margin = s.doc().unwrap().doc.spreads[0].pages[0].margin_rect();
    s.set_tool("type");
    let v = ViewInfo::at_zoom(1.0);
    let c = |s: &Session, x: f64, y: f64| s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), Point::new(x, y));
    // From 1.5 pt off the top-left margin corner to 1.5 pt off the right margin.
    let a = c(&s, margin.x0 + 1.5, margin.y0 + 1.5);
    let b = c(&s, margin.x1 - 1.5, margin.y0 + 101.5);
    let before = s.doc().unwrap().doc.spreads[0].items.len();
    s.pointer(&PointerEvent::new(PointerKind::Down, a.x, a.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, (a.x + b.x) / 2.0, b.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, b.x, b.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, b.x, b.y), v).unwrap();
    let items = &s.doc().unwrap().doc.spreads[0].items;
    assert_eq!(items.len(), before + 1, "a frame was drawn");
    let r = items.last().unwrap().bounds();
    assert!((r.x0 - margin.x0).abs() < 1e-6 && (r.y0 - margin.y0).abs() < 1e-6, "press point {r:?} vs {margin:?}");
    assert!((r.x1 - margin.x1).abs() < 1e-6, "right edge {} vs {}", r.x1, margin.x1);
}

#[test]
fn preflight_distinguishes_unplaced_stories_and_locates_footnote_overset() {
    use designcraft_doc::{AnchorPosition, AnchoredObject, ParaFormat, SpreadRef, Story, StoryId};
    use designcraft_geom::Rect;
    let mut s = session();
    let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
    let layer = doc.default_layer();
    let (host, body) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 20.0, 400.0, 600.0), layer, "Body", ParaFormat::default()).unwrap();
    let (frame, _) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 5.0, 2.0), layer, "Overflow", ParaFormat::default()).unwrap();
    let item = doc.item(frame).unwrap().clone();
    doc.spread_mut(SpreadRef::Doc(0)).unwrap().items.retain(|i| i.id != frame);
    let story = doc.story_mut(body).unwrap();
    story.insert_note(4, "Note", ParaFormat::default());
    Arc::make_mut(&mut story.notes[0]).text.insert_object(0, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: 0.0 }));
    let unplaced = StoryId(doc.alloc());
    doc.stories.insert(unplaced, Arc::new(Story::with_text(unplaced, "Unplaced", ParaFormat::default())));
    doc.check().unwrap();
    let issues = s.execute("preflight.run", &json!({})).unwrap();
    let issues = issues["issues"].as_array().unwrap();
    let overset: Vec<_> = issues.iter().filter(|i| i["kind"] == "overset").collect();
    assert_eq!(overset.len(), 1, "{issues:?}");
    assert_eq!(overset[0]["item"], host.0);
    assert_eq!(overset[0]["page"], 0);
    let unplaced = issues.iter().find(|i| i["kind"] == "unplacedStory").unwrap();
    assert_eq!(unplaced["severity"], "warning");
    assert!(unplaced.get("page").is_none());
}

#[test]
fn preflight_mojikumi_uses_the_same_supported_rules_as_composition() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "中文 A1"})).unwrap();
    let story = s.doc().unwrap().doc.stories.keys().next().unwrap().0;
    s.execute("text.select", &json!({"story": story, "anchor": 0, "focus": 1})).unwrap();
    for (name, unsupported) in [
        ("SimpChineseDefault", false),
        ("LineEndAllOneHalfEmEnum", false),
        ("LineEndAllOneEmEnum", false),
        ("UnknownPreset", true),
        ("Nothing", false),
    ] {
        s.execute("type.para", &json!({"attrs": {"mojikumi": name, "kinsokuType": "KinsokuPushInFirst"}})).unwrap();
        let result = s.execute("preflight.run", &json!({})).unwrap();
        let warnings: Vec<_> = result["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "unsupportedTypography").collect();
        assert_eq!(!warnings.is_empty(), unsupported, "{name}: {warnings:?}");
    }
}

#[test]
fn paragraph_style_edit_persists_attrs_and_rename() {
    let mut s = session();
    let name =
        s.execute("style.paragraph.create", &json!({"name": "Body Copy", "para": {"align": "left"}})).unwrap()["name"].as_str().unwrap().to_string();
    s.execute("style.paragraph.edit", &json!({"name": name, "para": {"align": "center"}})).unwrap();
    {
        let st = s.doc().unwrap();
        let style = st.doc.styles.para(&name).expect("style still exists");
        assert_eq!(style.para.align, Some(designcraft_doc::Align::Center), "edited attribute persisted");
    }
    // Renaming updates the style definition itself, and every place it's referenced.
    s.execute("style.paragraph.edit", &json!({"name": name, "rename": "Body Text"})).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.styles.para("Body Text").is_some(), "renamed style exists under the new name");
    assert!(st.doc.styles.para(&name).is_none(), "old name is gone");
}

#[test]
fn paragraph_style_edit_rejects_based_on_cycle() {
    let mut s = session();
    let a = s.execute("style.paragraph.create", &json!({"name": "A"})).unwrap()["name"].as_str().unwrap().to_string();
    let b = s.execute("style.paragraph.create", &json!({"name": "B", "basedOn": a})).unwrap()["name"].as_str().unwrap().to_string();
    let r = s.execute("style.paragraph.edit", &json!({"name": a, "basedOn": b}));
    assert!(r.is_err(), "A based on B based on A must be rejected");
}

#[test]
fn paragraph_style_delete_reassigns_affected_text() {
    let mut s = session();
    let name = s.execute("style.paragraph.create", &json!({"name": "Caption"})).unwrap()["name"].as_str().unwrap().to_string();
    let sid = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap()["story"].as_u64().unwrap();
    s.execute("text.insert", &json!({"text": "Hello, world"})).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    s.execute("style.paragraph.apply", &json!({"name": name})).unwrap();
    {
        let st = s.doc().unwrap();
        let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).expect("story exists");
        assert_eq!(story.paras[0].style, name, "style applied before delete");
    }
    s.execute("style.paragraph.delete", &json!({"name": name, "replaceWith": designcraft_doc::BASIC_PARAGRAPH})).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.styles.para(&name).is_none(), "deleted style is gone");
    let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).expect("story exists");
    assert_eq!(story.paras[0].style, designcraft_doc::BASIC_PARAGRAPH, "affected paragraph reassigned to the replacement style");
}

#[test]
fn paragraph_style_delete_rejects_builtin_styles() {
    let mut s = session();
    let r = s.execute("style.paragraph.delete", &json!({"name": designcraft_doc::BASIC_PARAGRAPH}));
    assert!(r.is_err(), "built-in styles can't be deleted");
}

#[test]
fn character_style_edit_persists_attrs_and_rename() {
    let mut s = session();
    let name = s.execute("style.character.create", &json!({"name": "Key Term", "chars": {"fontStyle": "Bold"}})).unwrap()["name"]
        .as_str()
        .unwrap()
        .to_string();
    s.execute("style.character.edit", &json!({"name": name, "chars": {"tracking": 20.0}})).unwrap();
    {
        let st = s.doc().unwrap();
        let style = st.doc.styles.char_style(&name).expect("style still exists");
        assert_eq!(style.chars.tracking, Some(20.0), "edited attribute persisted");
    }
    // Renaming updates the style definition itself, and every place it's referenced.
    s.execute("style.character.edit", &json!({"name": name, "rename": "Emphasis"})).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.styles.char_style("Emphasis").is_some(), "renamed style exists under the new name");
    assert!(st.doc.styles.char_style(&name).is_none(), "old name is gone");
}

#[test]
fn character_style_edit_rejects_based_on_cycle() {
    let mut s = session();
    let a = s.execute("style.character.create", &json!({"name": "A"})).unwrap()["name"].as_str().unwrap().to_string();
    let b = s.execute("style.character.create", &json!({"name": "B", "basedOn": a})).unwrap()["name"].as_str().unwrap().to_string();
    let r = s.execute("style.character.edit", &json!({"name": a, "basedOn": b}));
    assert!(r.is_err(), "A based on B based on A must be rejected");
}

#[test]
fn character_style_delete_reassigns_affected_text() {
    let mut s = session();
    let name = s.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.insert", &json!({"text": "Hello, world"})).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
    s.execute("style.character.apply", &json!({"name": name})).unwrap();
    {
        let st = s.doc().unwrap();
        let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).expect("story exists");
        assert_eq!(story.chars[0].format.style, name, "style applied before delete");
    }
    s.execute("style.character.delete", &json!({"name": name, "replaceWith": designcraft_doc::NO_CHAR_STYLE})).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.styles.char_style(&name).is_none(), "deleted style is gone");
    let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).expect("story exists");
    assert_eq!(story.chars[0].format.style, designcraft_doc::NO_CHAR_STYLE, "affected run reassigned to the replacement style");
}

#[test]
fn character_style_delete_rejects_builtin_styles() {
    let mut s = session();
    let r = s.execute("style.character.delete", &json!({"name": designcraft_doc::NO_CHAR_STYLE}));
    assert!(r.is_err(), "built-in styles can't be deleted");
}

#[test]
fn direct_selection_drag_and_arrow_keys_move_placed_content() {
    use designcraft_tools::{Mods, PointerEvent, PointerKind, SnapView, ToolKey};
    let mut s = session();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    s.execute("place.load", &json!({"base64": cmd::base64_encode(&png)})).unwrap();
    s.execute("place.drop", &json!({"frame": f})).unwrap();
    let xfs = |s: &Session| {
        let it = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().clone();
        (it.xf, it.graphic().unwrap().xf.translation())
    };
    let (frame0, g0) = xfs(&s);
    let v = ViewInfo { snap: SnapView::OFF, ..ViewInfo::at_zoom(1.0) };
    let at = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Point::new(150.0, 150.0));
    s.set_tool("directSelection");
    s.pointer(&PointerEvent::new(PointerKind::Down, at.x, at.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, at.x + 20.0, at.y + 10.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, at.x + 30.0, at.y + 10.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, at.x + 30.0, at.y + 10.0), v).unwrap();
    assert!(s.doc().unwrap().selection.content);
    let (frame1, g1) = xfs(&s);
    assert_eq!(frame1, frame0, "the frame stays");
    assert_eq!(g1 - g0, designcraft_geom::Vec2::new(30.0, 10.0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(xfs(&s).1, g0, "one undo step");
    // Arrow keys nudge the selected content; Shift nudges ten times as far.
    s.set_tool("selection");
    s.tool_key(ToolKey::Right, Mods::default(), v).unwrap();
    s.tool_key(ToolKey::Down, Mods { shift: true, ..Default::default() }, v).unwrap();
    let (frame2, g2) = xfs(&s);
    assert_eq!(frame2, frame0);
    assert_eq!(g2 - g0, designcraft_geom::Vec2::new(1.0, 10.0));
}

#[test]
fn preflight_notes_an_assumed_mojikumi_base_set_without_calling_the_table_unapplied() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "中文 A1"})).unwrap();
    let story = s.doc().unwrap().doc.stories.keys().next().unwrap().0;
    let d = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
    std::sync::Arc::make_mut(&mut d.styles)
        .mojikumi_tables
        .push(designcraft_doc::cjk::MojikumiTable { name: "Baseless".into(), ..Default::default() });
    s.execute("text.select", &json!({"story": story, "anchor": 0, "focus": 1})).unwrap();
    s.execute("type.para", &json!({"attrs": {"mojikumi": "MojikumiTable/Baseless"}})).unwrap();
    let result = s.execute("preflight.run", &json!({})).unwrap();
    let messages: Vec<_> =
        result["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "unsupportedTypography").filter_map(|i| i["message"].as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("Baseless") && m.contains("no base set")), "{messages:?}");
    assert!(!messages.iter().any(|m| m.contains("not applied")), "{messages:?}");
}

#[test]
fn live_corner_gestures_are_one_undo_step_each() {
    use designcraft_geom::corners::CornerShape;
    use designcraft_tools::{Mods, PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [100, 100, 300, 250], "content": "graphic"})).unwrap();
    let id = s.doc().unwrap().doc.spreads[0].items[0].id;
    s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let undo = |s: &Session| s.doc().unwrap().history.undo.len();
    let corners = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().corners.corners;
    let before = undo(&s);
    // Click the widget, then drag the top-left diamond 20 pt toward the centre.
    s.pointer(&PointerEvent::new(PointerKind::Down, 300.0, 111.5), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 300.0, 111.5), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Down, 112.0, 100.0), v).unwrap();
    for x in [116.0, 124.0, 132.0] {
        s.pointer(&PointerEvent::new(PointerKind::Drag, x, 100.0), v).unwrap();
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, 132.0, 100.0), v).unwrap();
    assert_eq!(undo(&s), before + 1);
    assert!(corners(&s).iter().all(|c| c.shape == CornerShape::Rounded && (c.size - 20.0).abs() < 1e-9), "{:?}", corners(&s));
    // Alt-click the top-left diamond, now 20 pt along the edge.
    let alt = Mods { alt: true, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, 120.0, 100.0).with_mods(alt), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 120.0, 100.0).with_mods(alt), v).unwrap();
    assert_eq!(undo(&s), before + 2);
    assert_eq!(corners(&s)[0].shape, CornerShape::InverseRounded);
    assert_eq!(corners(&s)[1].shape, CornerShape::Rounded);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(corners(&s)[0].shape, CornerShape::Rounded);
    // Malformed per-corner parameters are errors, not silent defaults.
    let c = json!({"shape": "rounded", "size": 5});
    assert!(s.execute("object.cornerOptions", &json!({"corners": [c, c, c]})).is_err());
    assert!(s.execute("object.cornerOptions", &json!({"corners": [c, c, c, {"shape": "rounded", "size": -1}]})).is_err());
    assert!(s.execute("object.cornerOptions", &json!({"corners": "rounded"})).is_err());
}

#[test]
fn alt_arrows_kern_at_the_caret_and_track_a_selection() {
    use designcraft_doc::{Kerning, StoryId};
    use designcraft_tools::{Mods, ToolKey};
    let mut s = session();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello world"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.set_tool("type");
    let v = ViewInfo::at_zoom(1.0);
    let alt = Mods { alt: true, ..Default::default() };
    let story = |s: &Session| s.doc().unwrap().doc.story(StoryId(sid)).unwrap().clone();
    let caret = |s: &mut Session, at: usize| s.execute("text.select", &json!({"story": sid, "anchor": at, "focus": at})).unwrap();
    // Caret between "e" and "l": Alt+Right kerns the pair by the increment; the caret stays.
    caret(&mut s, 2);
    assert!(s.tool_key(ToolKey::Right, alt, v).unwrap());
    assert_eq!(story(&s).format_after(1).over.kerning, Some(Kerning::Manual(20.0)));
    assert_eq!(s.doc().unwrap().selection.text.unwrap().focus, 2);
    s.tool_key(ToolKey::Right, alt, v).unwrap();
    assert_eq!(story(&s).format_after(1).over.kerning, Some(Kerning::Manual(40.0)));
    // One undo step per press.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(story(&s).format_after(1).over.kerning, Some(Kerning::Manual(20.0)));
    // Alt+Cmd (Alt+Ctrl elsewhere) steps five increments.
    caret(&mut s, 4);
    s.tool_key(ToolKey::Left, Mods { cmd: true, ..alt }, v).unwrap();
    assert_eq!(story(&s).format_after(3).over.kerning, Some(Kerning::Manual(-100.0)));
    // The increment is the document's Kerning/Tracking preference.
    s.execute("document.preferences", &json!({"kerningIncrement": 10})).unwrap();
    caret(&mut s, 2);
    s.tool_key(ToolKey::Right, alt, v).unwrap();
    assert_eq!(story(&s).format_after(1).over.kerning, Some(Kerning::Manual(30.0)));
    // A selection: tracking of the selected text.
    s.execute("text.select", &json!({"story": sid, "anchor": 6, "focus": 11})).unwrap();
    s.tool_key(ToolKey::Right, alt, v).unwrap();
    let st = story(&s);
    assert_eq!((st.format_after(6).over.tracking, st.format_after(10).over.tracking), (Some(10.0), Some(10.0)));
    assert_eq!(st.format_after(4).over.tracking, None);
    s.tool_key(ToolKey::Left, Mods { cmd: true, ctrl: true, ..alt }, v).unwrap();
    assert_eq!(story(&s).format_after(8).over.tracking, Some(-40.0));
    // Words: Cmd+Right (Ctrl+Right elsewhere); Alt+Shift+Right extends by a word.
    caret(&mut s, 0);
    s.tool_key(ToolKey::Right, Mods { cmd: true, ..Default::default() }, v).unwrap();
    assert_eq!(s.doc().unwrap().selection.text.unwrap().focus, 5);
    caret(&mut s, 0);
    s.tool_key(ToolKey::Right, Mods { alt: true, shift: true, ..Default::default() }, v).unwrap();
    let t = s.doc().unwrap().selection.text.unwrap();
    assert_eq!((t.anchor, t.focus), (0, 5));
}

#[test]
fn character_style_delete_rejects_missing_unknown_and_self_replacement() {
    let mut s = session();
    let name = s.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
    assert!(s.execute("style.character.delete", &json!({"name": ""})).is_err(), "empty name is rejected");
    assert!(s.execute("style.character.delete", &json!({"name": "No Such Style"})).is_err(), "unknown name is rejected");
    assert!(s.execute("style.character.delete", &json!({"name": name, "replaceWith": "No Such Style"})).is_err(), "unknown replaceWith is rejected");
    assert!(s.execute("style.character.delete", &json!({"name": name, "replaceWith": name})).is_err(), "replaceWith can't be the deleted style");
}

#[test]
fn paragraph_style_delete_rejects_missing_unknown_and_self_replacement() {
    let mut s = session();
    let name = s.execute("style.paragraph.create", &json!({"name": "Caption"})).unwrap()["name"].as_str().unwrap().to_string();
    assert!(s.execute("style.paragraph.delete", &json!({"name": ""})).is_err(), "empty name is rejected");
    assert!(s.execute("style.paragraph.delete", &json!({"name": "No Such Style"})).is_err(), "unknown name is rejected");
    assert!(s.execute("style.paragraph.delete", &json!({"name": name, "replaceWith": "No Such Style"})).is_err(), "unknown replaceWith is rejected");
    assert!(s.execute("style.paragraph.delete", &json!({"name": name, "replaceWith": name})).is_err(), "replaceWith can't be the deleted style");
}

#[test]
fn deleting_a_style_never_leaves_its_replacement_based_on_itself() {
    let mut s = session();
    let para = |s: &Session, n: &str| s.doc().unwrap().doc.styles.para(n).unwrap().based_on.clone();
    s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Sub", "basedOn": "Head"})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Minor", "basedOn": "Sub"})).unwrap();
    // Minor is based on Sub; deleting Sub in favour of Minor gives Minor Sub's parent.
    s.execute("style.paragraph.delete", &json!({"name": "Sub", "replaceWith": "Minor"})).unwrap();
    assert_eq!(para(&s, "Minor").as_deref(), Some("Head"));
    s.execute("style.character.create", &json!({"name": "Strong"})).unwrap();
    s.execute("style.character.create", &json!({"name": "Stronger", "basedOn": "Strong"})).unwrap();
    s.execute("style.character.delete", &json!({"name": "Strong", "replaceWith": "Stronger"})).unwrap();
    let based = s.doc().unwrap().doc.styles.character.iter().find(|c| c.name == "Stronger").unwrap().based_on.clone();
    assert_ne!(based.as_deref(), Some("Stronger"));
}

#[test]
fn character_style_delete_reassigns_every_kind_of_reference() {
    let mut s = session();
    let name = s.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
    let repl = s.execute("style.character.create", &json!({"name": "Emphasis"})).unwrap()["name"].as_str().unwrap().to_string();
    // Another character style's basedOn.
    let based = s.execute("style.character.create", &json!({"name": "Strong", "basedOn": name})).unwrap()["name"].as_str().unwrap().to_string();
    // A paragraph style's nested, nested line, GREP and drop cap styles, and the footnote reference style.
    s.execute(
        "style.paragraph.create",
        &json!({"name": "Body", "para": {
            "nestedStyles": [{"style": name, "through": true, "count": 1, "until": {"kind": "words"}}],
            "grepStyles": [{"style": name, "pattern": "x"}],
            "nestedLineStyles": [{"style": name, "lines": 1}],
            "dropCapStyle": name}}),
    )
    .unwrap();
    s.execute("footnote.options", &json!({"refCharStyle": name})).unwrap();
    s.execute("style.character.delete", &json!({"name": name, "replaceWith": repl})).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.styles.char_style(&name).is_none(), "deleted style is gone");
    assert_eq!(st.doc.styles.char_style(&based).unwrap().based_on.as_deref(), Some(repl.as_str()), "basedOn redirected");
    let body = st.doc.styles.para("Body").unwrap();
    assert_eq!(body.para.nested_styles.as_ref().unwrap()[0].style, repl, "nested style redirected");
    assert_eq!(body.para.nested_line_styles.as_ref().unwrap()[0].style, repl, "nested line style redirected");
    assert_eq!(body.para.grep_styles.as_ref().unwrap()[0].style, repl, "grep style redirected");
    assert_eq!(body.para.drop_cap_style.as_deref(), Some(repl.as_str()), "drop cap style redirected");
    assert_eq!(st.doc.footnote_options.ref_char_style, repl, "footnote reference style redirected");
}

#[test]
fn page_tool_selects_a_page_and_its_handles_preview_or_resize_it() {
    use designcraft_tools::{CanvasLayout, Mods, PointerEvent, PointerKind};
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 612, "height": 792, "facingPages": false})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [400, 100, 500, 200]})).unwrap()["id"].as_u64().unwrap();
    let frame = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
    let width = |s: &Session| s.doc().unwrap().doc.spreads[0].pages[0].width;
    // The page on the canvas (a single page is centred on the spine).
    let page = |s: &Session| {
        let d = &s.doc().unwrap().doc;
        CanvasLayout::new(d, false).page_rect(d, 0).unwrap()
    };
    s.set_tool("page");
    let v = ViewInfo::at_zoom(1.0);
    let ev = |kind, x: f64, y: f64, alt: bool| PointerEvent { mods: Mods { alt, ..Default::default() }, ..PointerEvent::new(kind, x, y) };
    let drag = |s: &mut Session, from: (f64, f64), by: f64, alt: bool, check: &dyn Fn(&Session)| {
        s.pointer(&ev(PointerKind::Down, from.0, from.1, alt), v).unwrap();
        s.pointer(&ev(PointerKind::Drag, from.0 + by, from.1, alt), v).unwrap();
        check(s);
        s.pointer(&ev(PointerKind::Up, from.0 + by, from.1, alt), v).unwrap();
    };
    // A click selects the page.
    let c = page(&s).center();
    s.pointer(&ev(PointerKind::Down, c.x, c.y, false), v).unwrap();
    s.pointer(&ev(PointerKind::Up, c.x, c.y, false), v).unwrap();
    assert_eq!(s.doc().unwrap().selection.page, Some(0));
    // A plain drag of the right handle previews the page's liquid rule and springs back.
    s.execute("liquid.pageRule", &json!({"rule": "scale"})).unwrap();
    let undo = s.doc().unwrap().history.undo.len();
    let r = page(&s);
    drag(&mut s, (r.x1, r.center().y), 612.0, false, &|s| {
        assert_eq!(width(s), 1224.0, "previewed");
        // Scale keeps proportions: only wider, so the frame is centred on the wider page.
        assert_eq!(frame(s).x0, 706.0, "the Scale rule in the preview: {:?}", frame(s));
    });
    assert_eq!((width(&s), frame(&s)), (612.0, designcraft_geom::Rect::new(400.0, 100.0, 500.0, 200.0)), "sprung back");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo, "nothing to undo");
    // Alt-drag resizes; with the rule off the frame stays where it is.
    s.execute("liquid.pageRule", &json!({"rule": "off"})).unwrap();
    let r = page(&s);
    drag(&mut s, (r.x1, r.center().y), 100.0, true, &|_| {});
    assert_eq!(width(&s), 712.0);
    assert_eq!(frame(&s).x0, 400.0);
    // The left handle grows the page to the left: the frame keeps its place by the right edge.
    let r = page(&s);
    drag(&mut s, (r.x0, r.center().y), -100.0, true, &|_| {});
    let pg = s.doc().unwrap().doc.spreads[0].pages[0].clone();
    assert_eq!(pg.width, 812.0);
    assert_eq!(frame(&s).x0 - pg.x, 500.0, "100 pt further from the page's left edge");
    // A click on the pasteboard deselects the page.
    let r = page(&s);
    s.pointer(&ev(PointerKind::Down, r.x0 - 200.0, r.center().y, false), v).unwrap();
    assert_eq!(s.doc().unwrap().selection.page, None);
}

#[test]
fn page_size_keeps_objects_by_its_anchor() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 400, "height": 400, "facingPages": false})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let frame = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
    // Bottom-right stays put: 100 wider and 50 taller moves the frame 100 right and 50 down.
    s.execute("layout.pageSize", &json!({"pages": [1], "width": 500, "height": 450, "anchor": 8})).unwrap();
    assert_eq!(frame(&s), designcraft_geom::Rect::new(200.0, 150.0, 300.0, 250.0));
    // The centre: half of it.
    s.execute("layout.pageSize", &json!({"pages": [1], "width": 700, "height": 450, "anchor": 4})).unwrap();
    assert_eq!(frame(&s).x0, 300.0);
    assert!(s.execute("layout.pageSize", &json!({"pages": [1], "width": 700, "anchor": 9})).is_err());
    assert!(s.execute("layout.selectPage", &json!({"page": 2})).is_err());
    assert_eq!(s.execute("layout.selectPage", &json!({"page": 1})).unwrap()["page"], 1);
}

#[test]
fn controlled_by_parent_takes_the_parent_pages_rule() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 400, "height": 400, "facingPages": false})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let frame = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
    // The page follows its parent; the parent page re-centres.
    s.execute("liquid.pageRule", &json!({"rule": "controlledByParent", "pages": [1]})).unwrap();
    s.edit(|d, _| {
        std::sync::Arc::make_mut(&mut d.parents[0]).pages[0].liquid = designcraft_doc::LiquidRule::ReCenter;
        Ok(())
    })
    .unwrap();
    s.execute("layout.pageSize", &json!({"pages": [1], "width": 600})).unwrap();
    assert_eq!(frame(&s).x0, 200.0, "re-centred: {:?}", frame(&s));
    // Without a parent (or a parent that's also controlled by its parent) nothing moves.
    s.execute("layout.pages.applyParent", &json!({"pages": [0], "parent": null})).unwrap();
    s.execute("layout.pageSize", &json!({"pages": [1], "width": 800})).unwrap();
    assert_eq!(frame(&s).x0, 200.0);
    // Show Parent Page Overlay is a page option.
    s.execute("layout.pageOptions", &json!({"pages": [1], "showParentOverlay": true})).unwrap();
    assert!(s.doc().unwrap().doc.spreads[0].pages[0].show_parent_overlay);
    assert!(s.execute("layout.pageOptions", &json!({"pages": [2], "showParentOverlay": true})).is_err());
}

#[test]
fn page_tool_pins_and_locks_objects_on_object_based_pages() {
    use designcraft_tools::{CanvasLayout, Mods, PointerEvent, PointerKind};
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 612, "height": 792, "facingPages": false})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [256, 72, 356, 132]})).unwrap()["id"].as_u64().unwrap();
    s.execute("liquid.pageRule", &json!({"rule": "objectBased"})).unwrap();
    s.execute("edit.deselectAll", &json!({})).unwrap();
    s.set_tool("page");
    let v = ViewInfo::at_zoom(1.0);
    let ev =
        |kind, p: designcraft_geom::Point, alt: bool| PointerEvent { mods: Mods { alt, ..Default::default() }, ..PointerEvent::new(kind, p.x, p.y) };
    let canvas = |s: &Session| {
        let d = &s.doc().unwrap().doc;
        let l = CanvasLayout::new(d, false);
        (
            l.page_rect(d, 0).unwrap(),
            l.xf(designcraft_doc::SpreadRef::Doc(0)).transform_rect_bbox(d.item(designcraft_doc::ItemId(id)).unwrap().bounds()),
        )
    };
    let click = |s: &mut Session, p: designcraft_geom::Point| {
        s.pointer(&ev(PointerKind::Down, p, false), v).unwrap();
        s.pointer(&ev(PointerKind::Up, p, false), v).unwrap();
    };
    let liquid = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().liquid;
    // A click on the object selects it with its page; its pins and locks show.
    let (_, o) = canvas(&s);
    click(&mut s, o.center());
    let sel = s.doc().unwrap().selection.clone();
    assert_eq!((sel.page, sel.items.clone()), (Some(0), vec![designcraft_doc::ItemId(id)]));
    assert!(
        s.overlays(v)
            .iter()
            .any(|o| matches!(o, designcraft_tools::Overlay::LiquidPins { pins: [false, false, false, false], resize: [true, true], .. }))
    );
    // The right pin (14 px outside the right edge) and the width lock (10 px right of the centre).
    click(&mut s, designcraft_geom::Point::new(o.x1 + 14.0, o.center().y));
    click(&mut s, designcraft_geom::Point::new(o.center().x + 10.0, o.center().y));
    let l = liquid(&s).unwrap();
    assert!(l.pin_right && !l.resize_width && l.resize_height && !l.pin_left, "{l:?}");
    // Alt-drag the page 100 pt wider: the pinned, width-locked frame keeps its size by the right edge.
    let (p, _) = canvas(&s);
    let right = designcraft_geom::Point::new(p.x1, p.center().y);
    s.pointer(&ev(PointerKind::Down, right, true), v).unwrap();
    s.pointer(&ev(PointerKind::Drag, right + designcraft_geom::Vec2::new(100.0, 0.0), true), v).unwrap();
    s.pointer(&ev(PointerKind::Up, right + designcraft_geom::Vec2::new(100.0, 0.0), true), v).unwrap();
    let d = &s.doc().unwrap().doc;
    let (pg, b) = (&d.spreads[0].pages[0], d.item(designcraft_doc::ItemId(id)).unwrap().bounds());
    assert_eq!(pg.width, 712.0);
    assert!((b.width() - 100.0).abs() < 1e-6 && (pg.x + pg.width - b.x1 - 256.0).abs() < 1e-6, "{b:?}");
    // Hovering a pin shows its tooltip.
    let (_, o) = canvas(&s);
    s.pointer(&ev(PointerKind::Move, designcraft_geom::Point::new(o.x1 + 14.0, o.center().y), false), v).unwrap();
    assert!(s.overlays(v).iter().any(|o| matches!(o, designcraft_tools::Overlay::Tip { text, .. } if text.starts_with("Object is pinned"))));
}

#[test]
fn duplicating_a_page_to_an_index_is_one_undo_step() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"facingPages": false})).unwrap();
    s.execute("layout.pages.insert", &json!({"count": 2})).unwrap();
    for (page, width) in [(1, 300), (2, 400), (3, 500)] {
        s.execute("layout.pageSize", &json!({"pages": [page], "width": width, "height": 600})).unwrap();
    }
    let widths = |s: &Session| {
        let d = &s.doc().unwrap().doc;
        (0..d.page_count()).map(|i| d.page_loc(i).map(|(si, pi)| d.spreads[si].pages[pi].width)).collect::<Option<Vec<_>>>().unwrap()
    };
    // Backwards: the last page copied to the front.
    let r = s.execute("layout.pages.duplicateSpread", &json!({"spread": 2, "to": 0})).unwrap();
    assert_eq!(r["page"], 0);
    assert_eq!(widths(&s), [500.0, 300.0, 400.0, 500.0]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(widths(&s), [300.0, 400.0, 500.0]);
    // Forwards: the first page copied to the end.
    s.execute("layout.pages.duplicateSpread", &json!({"spread": 0, "to": 3})).unwrap();
    assert_eq!(widths(&s), [300.0, 400.0, 500.0, 300.0]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(widths(&s), [300.0, 400.0, 500.0]);
    // Past the end: refused, nothing changes.
    assert!(s.execute("layout.pages.duplicateSpread", &json!({"spread": 0, "to": 4})).is_err());
    assert_eq!(widths(&s), [300.0, 400.0, 500.0]);
}

#[test]
fn duplicating_a_page_keeps_its_own_size_rule_guides_and_objects() {
    // Chris Cicchino's Page Tool video (2:06): the letterhead page is duplicated, then resized.
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 252, "height": 144, "facingPages": false})).unwrap();
    s.execute("layout.pages.insert", &json!({"count": 1})).unwrap();
    s.execute("layout.pageSize", &json!({"pages": [2], "width": 612, "height": 792})).unwrap();
    s.execute("liquid.pageRule", &json!({"rule": "scale", "pages": [2]})).unwrap();
    s.execute("guide.add", &json!({"spread": 1, "page": 0, "orientation": "vertical", "position": 300})).unwrap();
    s.execute("frame.create", &json!({"spread": 1, "rect": [100, 100, 200, 200]})).unwrap();
    let r = s.execute("layout.pages.duplicateSpread", &json!({"spread": 1})).unwrap();
    assert_eq!(r["page"], 2);
    let d = s.doc().unwrap().doc.clone();
    let (si, pi) = d.page_loc(2).unwrap();
    let pg = &d.spreads[si].pages[pi];
    assert_eq!((pg.width, pg.height, pg.liquid), (612.0, 792.0, designcraft_doc::LiquidRule::Scale));
    assert_eq!(pg.guides.len(), 1);
    assert_eq!(d.spreads[si].items.len(), 1);
    // At the end of the document (the New Page button): the business card, not the last page's size.
    let r = s.execute("layout.pages.duplicateSpread", &json!({"spread": 0, "atEnd": true})).unwrap();
    assert_eq!(r["page"], 3);
    let d = s.doc().unwrap().doc.clone();
    let (si, pi) = d.page_loc(3).unwrap();
    assert_eq!((d.spreads[si].pages[pi].width, d.spreads[si].pages[pi].height), (252.0, 144.0));
}

#[test]
fn the_type_tool_draws_a_text_frame_over_a_locked_frame() {
    // Adobe's "Set up a modern magazine layout" (4:03): with the page frame locked (Cmd+L), the
    // Type tool draws text frames on top of it; the locked frame stays an empty frame.
    use designcraft_tools::{PointerEvent, PointerKind, SnapView};
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 612, "height": 792, "facingPages": false})).unwrap();
    let big = s.execute("frame.create", &json!({"rect": [36, 36, 576, 756], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.stroke", &json!({"ids": [big], "swatch": "[Black]", "weight": 6})).unwrap();
    s.execute("object.lock", &json!({"ids": [big]})).unwrap();
    s.execute("edit.deselectAll", &json!({})).unwrap();
    s.set_tool("type");
    let v = ViewInfo { snap: SnapView::OFF, ..ViewInfo::at_zoom(1.0) };
    let layout = s.layout();
    let at = |x: f64, y: f64| layout.to_canvas(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Point::new(x, y));
    let (a, b) = (at(100.0, 100.0), at(300.0, 200.0));
    s.pointer(&PointerEvent::new(PointerKind::Down, a.x, a.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 200.0 + a.x - 100.0, a.y + 50.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, b.x, b.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, b.x, b.y), v).unwrap();
    let d = &s.doc().unwrap().doc;
    let it = d.item(designcraft_doc::ItemId(big)).unwrap();
    assert!(matches!(it.content, designcraft_doc::Content::Unassigned), "the locked frame stays empty");
    let frames: Vec<_> = d.spreads[0].items.iter().filter(|i| i.is_text_frame()).collect();
    assert_eq!(frames.len(), 1, "a new text frame");
    let r = frames[0].bounds();
    assert!((r.x0 - 100.0).abs() < 1e-6 && (r.y1 - 200.0).abs() < 1e-6, "{r:?}");
    // The engine refuses to turn a locked empty frame into a text frame.
    assert!(s.execute("text.placeCaret", &json!({"frame": big, "point": [300, 400]})).is_err());
    // Unlocked, a press inside converts it, as before.
    s.execute("object.unlockAll", &json!({})).unwrap();
    s.execute("text.placeCaret", &json!({"frame": big, "point": [300, 400]})).unwrap();
    assert!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(big)).unwrap().is_text_frame());
}

/// A line is set when its baseline fits in the frame; its descenders may hang below the frame, as
/// in InDesign. A 103 pt heading in a 78.5 pt frame (Cap Height first baseline) was overset.
#[test]
fn a_line_fits_when_its_baseline_does() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let overset = |s: &mut Session, height: f64| {
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 400, 36.0 + height], "content": "text", "text": "Gyp"})).unwrap();
        s.execute("object.textFrameOptions", &json!({"ids": [r["id"]], "firstBaseline": "capHeight", "inset": 0})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 3})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 103}})).unwrap();
        let st = s.execute("story.get", &json!({"story": r["story"]})).unwrap();
        !st["overset"].is_null()
    };
    // Cap height of the default font at 103 pt is about 70 pt: the baseline fits in 78.5 pt, the
    // descent (about 25 pt more) doesn't.
    assert!(!overset(&mut s, 78.5), "the line is set; its descenders hang below the frame");
    assert!(overset(&mut s, 50.0), "a baseline below the frame still oversets the line");
}
