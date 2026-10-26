//! The rulers' zero point: one per document, an offset from the ruler origin (the spread's or
//! each page's top-left corner, or the spine), so every spread measures from the same relative
//! place. Moving it is not an undo step, and undo and redo leave it where it is.

use std::sync::Arc;

use designcraft_doc::Document;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, point_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo
            "view.zeroPoint",
            "Ruler Zero Point",
            [],
            None,
            "{at?: [x, y] (points right of and below the ruler origin — document.preferences rulerOrigin: spread|page|spine; kept on the pasteboard), reset?: bool} → {zeroPoint: [x, y], locked}; with neither, reports it. Not an undo step; refused while locked",
            has_doc,
            zero_point
        ),
        cmd!(noundo
            "view.lockZeroPoint",
            "Lock Zero Point",
            [],
            None,
            "{on?: bool (default: toggle)} → {locked} — a locked zero point can't be dragged, moved or reset",
            has_doc,
            lock
        ),
    ]
}

fn state(d: &Document) -> Value {
    json!({"zeroPoint": d.settings.zero_point, "locked": d.settings.zero_point_locked})
}

fn zero_point(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "view.zeroPoint";
    let reset = p.get("reset").and_then(Value::as_bool).unwrap_or(false);
    let at = match p.get("at") {
        None => None,
        Some(_) => Some(point_param(p, "at").ok_or_else(|| bad(ID, "at must be [x, y] in points"))?),
    };
    let st = s.doc()?;
    let to = match (reset, at) {
        (true, _) => [0.0, 0.0],
        (false, Some(a)) => st.doc.clamp_zero_point([a.x, a.y]).ok_or_else(|| bad(ID, "at must be finite"))?,
        (false, None) => return Ok(state(&st.doc)),
    };
    if st.doc.settings.zero_point_locked {
        return Err(bad(ID, "the zero point is locked"));
    }
    if st.doc.settings.zero_point == to {
        return Ok(state(&st.doc));
    }
    s.edit(|d, _| {
        d.settings.zero_point = to;
        Ok(state(d))
    })
}

fn lock(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = s.doc()?.doc.settings.zero_point_locked;
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(!cur);
    if on == cur {
        return Ok(json!({"locked": cur}));
    }
    s.edit(|d, _| {
        d.settings.zero_point_locked = on;
        Ok(json!({"locked": on}))
    })
}

/// Undo and redo swap whole documents; the zero point stays as it is now.
pub(crate) fn keep_zero_point(now: &Document, to: &mut Arc<Document>) {
    let (zp, locked) = (now.settings.zero_point, now.settings.zero_point_locked);
    if to.settings.zero_point != zp || to.settings.zero_point_locked != locked {
        let d = Arc::make_mut(to);
        d.settings.zero_point = zp;
        d.settings.zero_point_locked = locked;
    }
}

#[cfg(test)]
mod tests {
    use designcraft_doc::{ItemId, SpreadRef};
    use designcraft_geom::Point;
    use serde_json::json;

    use crate::Session;

    fn zp(s: &Session) -> [f64; 2] {
        s.doc().unwrap().doc.settings.zero_point
    }

    #[test]
    fn the_zero_point_moves_resets_and_locks_without_undo_steps() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 136, 136]})).unwrap();
        let undo_steps = s.doc().unwrap().history.undo.len();
        let r = s.execute("view.zeroPoint", &json!({"at": [72, 36]})).unwrap();
        assert_eq!(r["zeroPoint"], json!([72.0, 36.0]));
        assert_eq!(zp(&s), [72.0, 36.0]);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_steps, "not an undo step");
        // Undo takes back the frame, not the zero point.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(zp(&s), [72.0, 36.0]);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(zp(&s), [72.0, 36.0]);
        // Locked: moving and resetting are refused.
        assert_eq!(s.execute("view.lockZeroPoint", &json!({})).unwrap()["locked"], true);
        assert!(s.execute("view.zeroPoint", &json!({"at": [0, 0]})).is_err());
        assert!(s.execute("view.zeroPoint", &json!({"reset": true})).is_err());
        assert_eq!(s.execute("view.zeroPoint", &json!({})).unwrap()["locked"], true);
        s.execute("view.lockZeroPoint", &json!({"on": false})).unwrap();
        s.execute("view.zeroPoint", &json!({"reset": true})).unwrap();
        assert_eq!(zp(&s), [0.0, 0.0]);
        // Hostile input: non-numbers are refused, far-off points stay on the pasteboard.
        assert!(s.execute("view.zeroPoint", &json!({"at": "here"})).is_err());
        assert!(s.execute("view.zeroPoint", &json!({"at": [1]})).is_err());
        s.execute("view.zeroPoint", &json!({"at": [1e300, -1e300]})).unwrap();
        let [x, y] = zp(&s);
        assert!(x.is_finite() && y.is_finite() && x < 5000.0 && y > -5000.0, "{x} {y}");
    }

    #[test]
    fn x_and_y_measure_from_the_zero_point_on_every_spread() {
        let mut s = Session::new();
        // Page 1 alone on spread 0; pages 2–3 on spread 1.
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        s.execute("view.zeroPoint", &json!({"at": [100, 50]})).unwrap();
        s.prefs.dimensions_include_stroke = false;
        let d = s.doc().unwrap().doc.clone();
        let o0 = d.ruler_origin(SpreadRef::Doc(0), 0.0).unwrap();
        let o1 = d.ruler_origin(SpreadRef::Doc(1), 0.0).unwrap();
        // The same place relative to each spread's top-left page corner.
        let b0 = d.spreads[0].bounds();
        let b1 = d.spreads[1].bounds();
        assert_eq!(o0, Point::new(b0.x0 + 100.0, b0.y0 + 50.0));
        assert_eq!(o1, Point::new(b1.x0 + 100.0, b1.y0 + 50.0));
        // transform.set x/y places the frame's top-left at that distance from the zero point.
        // Spread 1: a frame on its right-hand page measures from the left-hand page's corner too,
        // as the ruler does.
        for (spread, x0, o) in [(0, 10, o0), (1, 700, o1)] {
            let r = s.execute("frame.create", &json!({"spread": spread, "rect": [x0, 10, x0 + 50, 60]})).unwrap();
            let id = ItemId(r["id"].as_u64().unwrap());
            s.execute("transform.set", &json!({"ids": [id.0], "x": 20, "y": 30, "ref": 0})).unwrap();
            let b = s.doc().unwrap().doc.item(id).unwrap().bounds();
            assert!((b.x0 - (o.x + 20.0)).abs() < 1e-6 && (b.y0 - (o.y + 30.0)).abs() < 1e-6, "spread {spread}: {b:?} from {o:?}");
        }
    }

    #[test]
    fn x_measures_from_the_chosen_ruler_origin() {
        let mut s = Session::new();
        // Spread 1: page 2 (x 0–612) on the left, page 3 (612–1224) on the right.
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        s.prefs.dimensions_include_stroke = false;
        s.execute("view.zeroPoint", &json!({"at": [10, 0]})).unwrap();
        let left = s.execute("frame.create", &json!({"spread": 1, "rect": [100, 10, 150, 60]})).unwrap()["id"].as_u64().unwrap();
        let right = s.execute("frame.create", &json!({"spread": 1, "rect": [700, 10, 750, 60]})).unwrap()["id"].as_u64().unwrap();
        let x_of = |s: &Session, id: u64| {
            let d = &s.doc().unwrap().doc;
            let b = d.item(ItemId(id)).unwrap().bounds();
            b.x0 - d.ruler_origin(SpreadRef::Doc(1), b.center().x).unwrap().x
        };
        assert_eq!(s.doc().unwrap().doc.settings.ruler_origin, designcraft_doc::RulerOrigin::Spread, "the default");
        for (origin, l, r) in [("spread", 90.0, 690.0), ("page", 90.0, 78.0), ("spine", -522.0, 78.0)] {
            s.execute("document.preferences", &json!({"rulerOrigin": origin})).unwrap();
            assert_eq!((x_of(&s, left), x_of(&s, right)), (l, r), "{origin}");
            // The X field sets what it shows.
            s.execute("transform.set", &json!({"ids": [right], "x": 5, "ref": 0})).unwrap();
            assert!((x_of(&s, right) - 5.0).abs() < 1e-6, "{origin}");
            s.execute("transform.set", &json!({"ids": [right], "x": r, "ref": 0})).unwrap();
        }
        // A page origin restarts the horizontal ruler on each page.
        s.execute("document.preferences", &json!({"rulerOrigin": "page"})).unwrap();
        let pieces = s.doc().unwrap().doc.ruler_pieces(SpreadRef::Doc(1));
        let starts: Vec<f64> = pieces.iter().map(|p| p.2.x).collect();
        assert_eq!(starts, [0.0, 612.0]);
        assert!(pieces[0].0.is_infinite() && pieces[0].1 == 612.0 && pieces[1].1.is_infinite());
        assert!(s.execute("document.preferences", &json!({"rulerOrigin": "elsewhere"})).is_err());
    }

    #[test]
    fn old_documents_read_with_the_zero_point_at_the_corner() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let json = serde_json::to_string(&*d).unwrap();
        assert!(!json.contains("zeroPoint"), "the default isn't written");
        let back: designcraft_doc::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.settings.zero_point, [0.0, 0.0]);
        assert!(!back.settings.zero_point_locked);
        s.execute("view.zeroPoint", &json!({"at": [12, 34]})).unwrap();
        s.execute("view.lockZeroPoint", &json!({"on": true})).unwrap();
        let json = serde_json::to_string(&*s.doc().unwrap().doc).unwrap();
        let back: designcraft_doc::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.settings.zero_point, [12.0, 34.0]);
        assert!(back.settings.zero_point_locked);
        // A file can hold any numbers; rulers and X/Y read them kept on the pasteboard.
        let far = json.replace("[12.0,34.0]", "[1e300,-1e300]");
        let back: designcraft_doc::Document = serde_json::from_str(&far).unwrap();
        let [x, y] = back.zero_point();
        assert!(x.is_finite() && y.is_finite() && x < 1e4 && y > -1e4, "{x} {y}");
    }
}
