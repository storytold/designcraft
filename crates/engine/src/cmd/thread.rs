//! Threading text frames: thread a frame after another (their stories merge), break a thread,
//! take a frame out of its thread. The Selection tool's loaded text cursor runs these.

use designcraft_doc::{Document, ItemId, ParaFormat, Selection, StoryId};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, id_param, rect_param, spread_param, str_param};
use crate::{Result, Session};

/// Pages Shift-click autoflow may add before it gives up.
const AUTOFLOW_MAX_PAGES: usize = 2000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "text.thread",
            "Thread Text Frames",
            [],
            None,
            "{from?, to?, rect?: [x0,y0,x1,y1] (spread coords), spread?, autoflow?: bool} — threads frame `to` right after frame `from` (between `from` and the frame that followed it, if any). `to` is an empty frame or a text frame nothing threads into yet; its text joins the end of the story. Give `rect` in place of `from` or `to` to thread a new text frame made there on `spread`: `{from, rect}` adds it after `from`, `{rect, to}` in front of `to`. autoflow: then add pages with threaded frames until the story fits. Selects the frame that joined the thread. → {story, from, to, overset, pagesAdded}",
            has_doc,
            thread
        ),
        cmd!(
            "text.unthread",
            "Break Thread",
            [],
            None,
            "{frame?, side?: after|before} — breaks the thread after (or before) `frame` (default: the selected text frame). The frames from the break on stay threaded to each other but become empty; the text stays in the story, overset if the earlier frames can't hold it",
            has_doc,
            unthread
        ),
        cmd!(
            "text.removeFromThread",
            "Remove Frame from Thread",
            [],
            None,
            "{frame?} — takes `frame` (default: the selected text frame) out of its thread: the frames before and after it join up and keep the text, `frame` stays empty",
            has_doc,
            remove_from_thread
        ),
    ]
}

/// `frame` param, or the first selected text frame.
fn frame_param(s: &Session, p: &Value, cmd: &str) -> Result<ItemId> {
    let st = s.doc()?;
    let id = id_param(p, "frame")
        .or_else(|| st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| it.is_text_frame())))
        .ok_or_else(|| bad(cmd, "no text frame given or selected"))?;
    if !st.doc.item(id).is_some_and(|it| it.is_text_frame()) {
        return Err(bad(cmd, format!("{id} is not a text frame")));
    }
    Ok(id)
}

/// A rect param made safe for a frame: finite, ordered, at least half a point each way.
fn frame_rect(p: &Value) -> Result<Option<Rect>> {
    let Some(r) = rect_param(p, "rect") else { return Ok(None) };
    if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) {
        return Err(bad("text.thread", "rect must be finite"));
    }
    let (x0, x1) = (r.x0.min(r.x1), r.x0.max(r.x1));
    let (y0, y1) = (r.y0.min(r.y1), r.y0.max(r.y1));
    Ok(Some(Rect::new(x0, y0, x1.max(x0 + 0.5), y1.max(y0 + 0.5))))
}

fn thread(s: &mut Session, p: &Value) -> Result<Value> {
    let (from, to, rect) = (id_param(p, "from"), id_param(p, "to"), frame_rect(p)?);
    let sr = spread_param(p, "spread");
    let autoflow = bool_or(p, "autoflow", false);
    let layer = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let mut new_frame = || -> Result<ItemId> {
            let r = rect.ok_or_else(|| bad("text.thread", "give two of from, to and rect"))?;
            let para = ParaFormat { style: d.styles.default_paragraph.clone(), ..Default::default() };
            Ok(d.add_text_frame(sr, r, layer, "", para)?.0)
        };
        let (from, to, joined) = match (from, to) {
            (Some(f), Some(t)) if rect.is_none() => (f, t, t),
            (Some(f), None) => {
                let t = new_frame()?;
                (f, t, t)
            }
            (None, Some(t)) => {
                let f = new_frame()?;
                (f, t, f)
            }
            _ => return Err(bad("text.thread", "give two of from, to and rect")),
        };
        d.check_thread(from, to)?;
        d.thread_keeping_story(from, to)?;
        let sid = d.item(from).and_then(|i| i.text_frame()).map(|t| t.story).ok_or_else(|| bad("text.thread", "threading lost the story"))?;
        let pages = if autoflow { super::place_text_autoflow(d, sid, AUTOFLOW_MAX_PAGES)? } else { 0 };
        *sel = Selection::items(vec![joined]);
        Ok(json!({"story": sid.0, "from": from.0, "to": to.0, "overset": overset(d, sid), "pagesAdded": pages}))
    })
}

fn overset(d: &Document, sid: StoryId) -> bool {
    designcraft_compose::compose_story(d, sid, &Default::default()).overset_at.is_some()
}

fn unthread(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "text.unthread";
    let frame = frame_param(s, p, CMD)?;
    let d0 = &s.doc()?.doc;
    let at = match str_param(p, "side").unwrap_or("after") {
        "after" => d0.next_frame(frame).map(|_| frame).ok_or_else(|| bad(CMD, format!("no frame follows {frame} in its thread")))?,
        "before" => d0.prev_frame(frame).ok_or_else(|| bad(CMD, format!("no frame comes before {frame} in its thread")))?,
        other => return Err(bad(CMD, format!("unknown side `{other}` (after, before)"))),
    };
    s.edit(|d, _| {
        d.unthread_after(at)?;
        let sid = d.item(at).and_then(|i| i.text_frame()).map(|t| t.story).ok_or_else(|| bad(CMD, "no story"))?;
        Ok(json!({"story": sid.0, "overset": overset(d, sid)}))
    })
}

fn remove_from_thread(s: &mut Session, p: &Value) -> Result<Value> {
    let frame = frame_param(s, p, "text.removeFromThread")?;
    s.edit(|d, _| {
        d.remove_from_thread(frame)?;
        Ok(json!({}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        s
    }

    fn text_frame(s: &mut Session, rect: [f64; 4], text: &str) -> (u64, u64) {
        let r = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text, "caret": false})).unwrap();
        (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap())
    }

    fn story(s: &mut Session, frame: u64) -> serde_json::Value {
        s.execute("story.get", &json!({"frame": frame})).unwrap()
    }

    fn frames(s: &mut Session, frame: u64) -> Vec<u64> {
        story(s, frame)["frames"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
    }

    const LONG: &str = "Threaded text keeps flowing from one frame into the next one. ";

    #[test]
    fn thread_into_an_empty_frame_and_undo() {
        let mut s = session();
        let (a, sid) = text_frame(&mut s, [72.0, 72.0, 200.0, 120.0], &LONG.repeat(10));
        let b = s.execute("frame.create", &json!({"rect": [72, 300, 200, 400], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        assert!(!story(&mut s, a)["overset"].is_null(), "the first frame is overset");
        let r = s.execute("text.thread", &json!({"from": a, "to": b})).unwrap();
        assert_eq!(r["story"].as_u64(), Some(sid));
        assert_eq!(frames(&mut s, a), [a, b]);
        let d = &s.doc().unwrap().doc;
        assert!(d.item(designcraft_doc::ItemId(b)).unwrap().is_text_frame());
        assert_eq!(s.doc().unwrap().selection.items, [designcraft_doc::ItemId(b)]);
        // The text now reaches the second frame.
        let cs = designcraft_compose::compose_story(&s.doc().unwrap().doc, designcraft_doc::StoryId(sid), &Default::default());
        assert!(cs.frames.iter().any(|f| f.frame.0 == b && !f.range.is_empty()), "text flows into the new frame");
        s.doc().unwrap().doc.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(frames(&mut s, a), [a]);
        assert!(!s.doc().unwrap().doc.item(designcraft_doc::ItemId(b)).unwrap().is_text_frame());
    }

    #[test]
    fn thread_into_a_text_frame_merges_its_story() {
        let mut s = session();
        let (a, sa) = text_frame(&mut s, [72.0, 72.0, 200.0, 200.0], "First");
        let (b, sb) = text_frame(&mut s, [72.0, 300.0, 200.0, 400.0], "Second");
        s.execute("text.thread", &json!({"from": a, "to": b})).unwrap();
        let st = story(&mut s, b);
        assert_eq!(st["id"].as_u64(), Some(sa));
        assert_eq!(st["text"], "First\nSecond");
        assert!(s.doc().unwrap().doc.story(designcraft_doc::StoryId(sb)).is_none());
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn thread_into_the_middle_of_a_thread() {
        let mut s = session();
        let (a, _) = text_frame(&mut s, [72.0, 72.0, 200.0, 120.0], &LONG.repeat(8));
        let (c, _) = text_frame(&mut s, [72.0, 500.0, 200.0, 600.0], "");
        s.execute("text.thread", &json!({"from": a, "to": c})).unwrap();
        let (b, _) = text_frame(&mut s, [72.0, 300.0, 200.0, 400.0], "");
        s.execute("text.thread", &json!({"from": a, "to": b})).unwrap();
        assert_eq!(frames(&mut s, a), [a, b, c]);
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn bad_threads_are_errors_and_change_nothing() {
        let mut s = session();
        let (a, _) = text_frame(&mut s, [72.0, 72.0, 200.0, 120.0], "a");
        let (b, _) = text_frame(&mut s, [72.0, 300.0, 200.0, 400.0], "b");
        let (x, _) = text_frame(&mut s, [300.0, 72.0, 400.0, 120.0], "x");
        let (y, _) = text_frame(&mut s, [300.0, 300.0, 400.0, 400.0], "y");
        let g = s.execute("line.create", &json!({"a": [300, 500], "b": [400, 600]})).unwrap()["id"].as_u64().unwrap();
        let group = {
            let p = s.execute("frame.create", &json!({"rect": [500, 500, 550, 550], "content": "unassigned"})).unwrap()["id"].clone();
            let q = s.execute("frame.create", &json!({"rect": [560, 500, 600, 550], "content": "unassigned"})).unwrap()["id"].clone();
            s.execute("selection.set", &json!({"ids": [p, q]})).unwrap();
            s.execute("object.group", &json!({})).unwrap();
            s.doc().unwrap().selection.items[0].0
        };
        s.execute("text.thread", &json!({"from": a, "to": b})).unwrap();
        s.execute("text.thread", &json!({"from": x, "to": y})).unwrap();
        let undo = s.doc().unwrap().history.undo.len();
        let doc = s.doc().unwrap().doc.clone();
        for p in [
            json!({"from": a, "to": a}),
            json!({"from": a, "to": g}),
            json!({"from": g, "to": a}),
            json!({"from": a, "to": group}),
            // Earlier in the same thread.
            json!({"from": b, "to": a}),
            // Text already flows into y from x.
            json!({"from": a, "to": y}),
            json!({"from": a, "to": 999_999}),
            json!({"from": a}),
            json!({"from": a, "to": b, "rect": [0, 0, 10, 10]}),
            json!({"rect": [0, 0, 10, 10]}),
            json!({"from": a, "rect": [0, 0, 10, 10], "spread": 99}),
            json!({"from": "a", "to": [1]}),
            json!([1, 2]),
        ] {
            assert!(s.execute("text.thread", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert_eq!(*s.doc().unwrap().doc, *doc);
    }

    #[test]
    fn thread_a_new_frame_after_or_in_front() {
        let mut s = session();
        let (a, sa) = text_frame(&mut s, [72.0, 72.0, 200.0, 120.0], &LONG.repeat(8));
        let r = s.execute("text.thread", &json!({"from": a, "rect": [72, 300, 200, 400]})).unwrap();
        let n = r["to"].as_u64().unwrap();
        assert_eq!(frames(&mut s, a), [a, n]);
        assert_eq!(s.doc().unwrap().selection.items, [designcraft_doc::ItemId(n)]);
        let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(n)).unwrap().bounds();
        assert_eq!(b, Rect::new(72.0, 300.0, 200.0, 400.0));
        // In front of the first frame: the story keeps its id and text.
        let r = s.execute("text.thread", &json!({"rect": [300, 72, 400, 120], "to": a})).unwrap();
        let m = r["from"].as_u64().unwrap();
        assert_eq!(r["story"].as_u64(), Some(sa));
        assert_eq!(frames(&mut s, a), [m, a, n]);
        assert_eq!(story(&mut s, a)["text"], LONG.repeat(8));
        s.doc().unwrap().doc.check().unwrap();
        // One undo step each.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(frames(&mut s, a), [a, n]);
    }

    use designcraft_geom::Rect;

    #[test]
    fn thread_with_autoflow_adds_pages_until_the_text_fits() {
        let mut s = session();
        let (a, _) = text_frame(&mut s, [72.0, 72.0, 300.0, 200.0], &LONG.repeat(300));
        let r = s.execute("text.thread", &json!({"from": a, "rect": [72, 300, 300, 700], "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false);
        assert!(r["pagesAdded"].as_u64().unwrap() >= 1, "{r}");
        assert!(frames(&mut s, a).len() >= 3);
        s.doc().unwrap().doc.check().unwrap();
        // One undo removes the frames and the pages.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(frames(&mut s, a), [a]);
        assert_eq!(s.doc().unwrap().doc.page_count(), 2);
    }

    #[test]
    fn unthread_after_and_before() {
        let mut s = session();
        let text = LONG.repeat(12);
        let (a, sa) = text_frame(&mut s, [72.0, 72.0, 200.0, 120.0], &text);
        let (b, _) = text_frame(&mut s, [72.0, 200.0, 200.0, 250.0], "");
        let (c, _) = text_frame(&mut s, [72.0, 300.0, 200.0, 350.0], "");
        s.execute("text.thread", &json!({"from": a, "to": b})).unwrap();
        s.execute("text.thread", &json!({"from": b, "to": c})).unwrap();
        // Break after the first frame: b and c stay threaded to each other, empty.
        let r = s.execute("text.unthread", &json!({"frame": a})).unwrap();
        assert_eq!(r["story"].as_u64(), Some(sa));
        assert_eq!(r["overset"], true);
        assert_eq!(frames(&mut s, a), [a]);
        assert_eq!(story(&mut s, a)["text"], text);
        assert_eq!(frames(&mut s, b), [b, c]);
        assert_eq!(story(&mut s, b)["text"], "");
        s.doc().unwrap().doc.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(frames(&mut s, a), [a, b, c]);
        // Break before the last frame, with the frame selected.
        s.execute("selection.set", &json!({"ids": [c]})).unwrap();
        s.execute("text.unthread", &json!({"side": "before"})).unwrap();
        assert_eq!(frames(&mut s, a), [a, b]);
        assert_eq!(frames(&mut s, c), [c]);
        // Nothing to break.
        assert!(s.execute("text.unthread", &json!({"frame": b})).is_err());
        assert!(s.execute("text.unthread", &json!({"frame": a, "side": "before"})).is_err());
        assert!(s.execute("text.unthread", &json!({"frame": a, "side": "sideways"})).is_err());
        assert!(s.execute("text.unthread", &json!({"frame": 999_999})).is_err());
    }

    #[test]
    fn deleting_or_removing_a_threaded_frame_keeps_the_text_flowing() {
        let mut s = session();
        let text = LONG.repeat(12);
        let (a, _) = text_frame(&mut s, [72.0, 72.0, 200.0, 120.0], &text);
        let (b, _) = text_frame(&mut s, [72.0, 200.0, 200.0, 250.0], "");
        let (c, _) = text_frame(&mut s, [72.0, 300.0, 200.0, 350.0], "");
        s.execute("text.thread", &json!({"from": a, "to": b})).unwrap();
        s.execute("text.thread", &json!({"from": b, "to": c})).unwrap();
        s.execute("text.removeFromThread", &json!({"frame": b})).unwrap();
        assert_eq!(frames(&mut s, a), [a, c]);
        assert_eq!(story(&mut s, a)["text"], text);
        assert_eq!(story(&mut s, b)["text"], "");
        assert!(s.execute("text.removeFromThread", &json!({"frame": b})).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.clear", &json!({"ids": [b]})).unwrap();
        assert_eq!(frames(&mut s, a), [a, c]);
        assert_eq!(story(&mut s, a)["text"], text);
        s.doc().unwrap().doc.check().unwrap();
    }
}
