//! Ruler guides: add (dragged from the rulers), move, delete, list; Layout › Create Guides.
//!
//! Guides are stored on their page in spread coordinates; a spread guide (dropped on the
//! pasteboard) lives on the spread's first page with `spread: true` and crosses the pasteboard.

use designcraft_doc::{Document, Guide, Orientation, SpreadRef};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

const MAX_GUIDE_DIVISIONS: u64 = 1000;
const MAX_GUIDES_CREATED: usize = 10_000;
const MAX_GUIDE_GUTTER: f64 = 1_000_000.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "guide.add",
            "New Guide",
            [],
            None,
            "{orientation: horizontal|vertical, position (spread coordinate), spread?, page? (index in the spread; default: the page under `at` or the position), at? (the other coordinate), spreadGuide?: bool} → {page, index}; {orientation: both, point: [x, y], …} adds a horizontal guide through y and a vertical guide through x as one undo step → {guides: [{page, index}, {page, index}]}",
            has_doc,
            add
        ),
        cmd!("guide.move", "Move Guide", [], None, "{spread?, page, index, position}", has_doc, move_guide),
        cmd!("guide.delete", "Delete Guide", [], None, "{spread?, page, index}", has_doc, delete),
        cmd!("guide.deleteAll", "Delete All Guides on Spread", ["View", "Grids & Guides"], None, "{spread?}", has_doc, |s, p| {
            let r = guide_spread(p, "guide.deleteAll")?;
            s.edit(|d, _| {
                let sp = d.spread_mut(r).ok_or_else(|| bad("guide.deleteAll", "no such spread"))?;
                let mut n = 0;
                for pg in &mut sp.pages {
                    n += pg.guides.len();
                    pg.guides.clear();
                }
                Ok(json!({"deleted": n}))
            })
        }),
        cmd!(query "guide.list", "Guides", [], None, "{spread?} → [{page, index, orientation, position, spread}]", has_doc, |s, p| {
            let d = &s.doc()?.doc;
            let sp = d.spread(guide_spread(p, "guide.list")?).ok_or_else(|| bad("guide.list", "no such spread"))?;
            let mut out = Vec::new();
            for (pi, pg) in sp.pages.iter().enumerate() {
                for (i, g) in pg.guides.iter().enumerate() {
                    out.push(json!({"page": pi, "index": i, "orientation": g.orientation, "position": g.position, "spread": g.spread}));
                }
            }
            Ok(Value::Array(out))
        }),
        cmd!(
            "layout.createGuides",
            "Create Guides…",
            ["Layout"],
            None,
            "{rows?: 0..1000, columns?: 0..1000, rowGutter?: 12, columnGutter?: 12, fitTo?: margins|page, removeExisting?: false, spread?, page? (index in the spread; default all pages)} — at most 10000 guides per command",
            has_doc,
            create_guides
        ),
    ]
}

fn checked_usize(value: &Value, key: &str, command: &str) -> Result<usize> {
    let n = value.as_u64().ok_or_else(|| bad(command, format!("`{key}` must be a non-negative integer")))?;
    usize::try_from(n).map_err(|_| bad(command, format!("`{key}` is too large for this platform")))
}

fn optional_usize(p: &Value, key: &str, command: &str) -> Result<Option<usize>> {
    p.get(key).map(|value| checked_usize(value, key, command)).transpose()
}

/// Numeric spread shorthand or the serialized `SpreadRef` form, without lossy/defaulting casts.
fn guide_spread(p: &Value, command: &str) -> Result<SpreadRef> {
    let Some(value) = p.get("spread") else { return Ok(SpreadRef::Doc(0)) };
    if value.is_number() {
        return Ok(SpreadRef::Doc(checked_usize(value, "spread", command)?));
    }
    let object = value.as_object().ok_or_else(|| bad(command, "`spread` must be a non-negative integer or a spread reference"))?;
    let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| bad(command, "`spread.kind` must be `doc` or `parent`"))?;
    let index = object.get("index").ok_or_else(|| bad(command, "missing `spread.index`"))?;
    let index = checked_usize(index, "spread.index", command)?;
    match kind {
        "doc" => Ok(SpreadRef::Doc(index)),
        "parent" => Ok(SpreadRef::Parent(index)),
        _ => Err(bad(command, "`spread.kind` must be `doc` or `parent`")),
    }
}

fn finite_number(p: &Value, key: &str, command: &str) -> Result<Option<f64>> {
    let Some(value) = p.get(key) else { return Ok(None) };
    let number = value.as_f64().ok_or_else(|| bad(command, format!("`{key}` must be a finite number")))?;
    if !number.is_finite() {
        return Err(bad(command, format!("`{key}` must be a finite number")));
    }
    Ok(Some(number))
}

fn orientation(p: &Value) -> Result<Orientation> {
    match str_param(p, "orientation") {
        Some("horizontal") => Ok(Orientation::Horizontal),
        Some("vertical") => Ok(Orientation::Vertical),
        _ => Err(bad("guide", "orientation: horizontal|vertical")),
    }
}

/// The page of a spread at a spread x coordinate (nearest page).
fn page_at(d: &Document, r: SpreadRef, x: f64) -> usize {
    d.spread(r).and_then(|sp| sp.page_at_x(x)).unwrap_or(0)
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let r = guide_spread(p, "guide.add")?;
    let spread_guide = p.get("spreadGuide").and_then(Value::as_bool).unwrap_or(false);
    let page = optional_usize(p, "page", "guide.add")?;
    // (orientation, position, x used to find the page)
    let guides: Vec<(Orientation, f64, f64)> = if str_param(p, "orientation") == Some("both") {
        let point = p.get("point").and_then(Value::as_array).ok_or_else(|| bad("guide.add", "orientation `both` needs point: [x, y]"))?;
        let coord = |i: usize| point.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
        let (Some(x), Some(y), 2) = (coord(0), coord(1), point.len()) else {
            return Err(bad("guide.add", "`point` must be two finite numbers [x, y]"));
        };
        vec![(Orientation::Horizontal, y, x), (Orientation::Vertical, x, x)]
    } else {
        let o = orientation(p)?;
        let pos = finite_number(p, "position", "guide.add")?.ok_or_else(|| bad("guide.add", "missing position"))?;
        let x = match o {
            Orientation::Vertical => pos,
            Orientation::Horizontal => finite_number(p, "at", "guide.add")?.unwrap_or(0.0),
        };
        vec![(o, pos, x)]
    };
    let d = &s.doc()?.doc;
    let placed: Vec<(Orientation, f64, usize)> = guides
        .into_iter()
        .map(|(o, pos, x)| {
            let pi = match page {
                Some(pi) => pi,
                None if spread_guide => 0,
                None => page_at(d, r, x),
            };
            (o, pos, pi)
        })
        .collect();
    let layer = s.doc()?.active_layer;
    s.edit(|d, _| {
        let sp = d.spread_mut(r).ok_or_else(|| bad("guide.add", "no such spread"))?;
        let mut out = Vec::new();
        for (o, pos, pi) in placed {
            let pg = sp.pages.get_mut(pi).ok_or_else(|| bad("guide.add", "no such page"))?;
            pg.guides.push(Guide { orientation: o, position: pos, spread: spread_guide, locked: false, layer: Some(layer), liquid: false });
            out.push(json!({"page": pi, "index": pg.guides.len() - 1}));
        }
        match <[Value; 1]>::try_from(out) {
            Ok([one]) => Ok(one),
            Err(out) => Ok(json!({"guides": out})),
        }
    })
}

fn guide_ref(p: &Value, cmd: &str) -> Result<(SpreadRef, usize, usize)> {
    let pi = optional_usize(p, "page", cmd)?.ok_or_else(|| bad(cmd, "missing page"))?;
    let i = optional_usize(p, "index", cmd)?.ok_or_else(|| bad(cmd, "missing index"))?;
    Ok((guide_spread(p, cmd)?, pi, i))
}

/// Commands must use the same guide/layer protection as pointer editing.
fn check_editable(d: &Document, r: SpreadRef, pi: usize, i: usize, command: &str) -> Result<()> {
    let g = d.spread(r).and_then(|sp| sp.pages.get(pi)).and_then(|pg| pg.guides.get(i)).ok_or_else(|| bad(command, "no such guide"))?;
    if !g.editable_in(d) {
        return Err(bad(command, "the guide or its layer is locked or hidden"));
    }
    Ok(())
}

fn move_guide(s: &mut Session, p: &Value) -> Result<Value> {
    let (r, pi, i) = guide_ref(p, "guide.move")?;
    let pos = finite_number(p, "position", "guide.move")?.ok_or_else(|| bad("guide.move", "missing position"))?;
    s.edit(|d, _| {
        check_editable(d, r, pi, i, "guide.move")?;
        let g = d
            .spread_mut(r)
            .and_then(|sp| sp.pages.get_mut(pi))
            .and_then(|pg| pg.guides.get_mut(i))
            .ok_or_else(|| bad("guide.move", "no such guide"))?;
        g.position = pos;
        Ok(Value::Null)
    })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let (r, pi, i) = guide_ref(p, "guide.delete")?;
    s.edit(|d, _| {
        check_editable(d, r, pi, i, "guide.delete")?;
        let pg = d.spread_mut(r).and_then(|sp| sp.pages.get_mut(pi)).ok_or_else(|| bad("guide.delete", "no such page"))?;
        if i >= pg.guides.len() {
            return Err(bad("guide.delete", "no such guide"));
        }
        pg.guides.remove(i);
        Ok(Value::Null)
    })
}

fn guide_divisions(p: &Value, key: &str) -> Result<u64> {
    let Some(value) = p.get(key) else { return Ok(0) };
    let count = value.as_u64().ok_or_else(|| bad("layout.createGuides", format!("`{key}` must be an integer")))?;
    if count > MAX_GUIDE_DIVISIONS {
        return Err(bad("layout.createGuides", format!("`{key}` must not exceed {MAX_GUIDE_DIVISIONS}")));
    }
    Ok(count)
}

fn guide_gutter(p: &Value, key: &str) -> Result<f64> {
    let Some(value) = p.get(key) else { return Ok(12.0) };
    let gutter = value.as_f64().ok_or_else(|| bad("layout.createGuides", format!("`{key}` must be a finite number")))?;
    if !gutter.is_finite() || !(0.0..=MAX_GUIDE_GUTTER).contains(&gutter) {
        return Err(bad("layout.createGuides", format!("`{key}` must be between 0 and {MAX_GUIDE_GUTTER}")));
    }
    Ok(gutter)
}

fn guide_count(divisions: u64, gutter: f64) -> Result<usize> {
    let edges = divisions.saturating_sub(1);
    let count = if gutter > 0.0 { edges.checked_mul(2) } else { Some(edges) }
        .ok_or_else(|| bad("layout.createGuides", "the requested guide count is too large"))?;
    usize::try_from(count).map_err(|_| bad("layout.createGuides", "the requested guide count is too large for this platform"))
}

/// Evenly spaced rows/columns with gutters inside `area`: the guide positions.
fn grid_positions(a: f64, b: f64, n: u64, gutter: f64) -> Result<Vec<f64>> {
    if n < 2 {
        return Ok(vec![]);
    }
    if !a.is_finite() || !b.is_finite() || !gutter.is_finite() || n > MAX_GUIDE_DIVISIONS {
        return Err(bad("layout.createGuides", "guide geometry must be finite and within the supported range"));
    }
    let cell = ((b - a) - gutter * (n - 1) as f64) / n as f64;
    if !cell.is_finite() {
        return Err(bad("layout.createGuides", "guide geometry is outside the supported range"));
    }
    let mut v = Vec::with_capacity(guide_count(n, gutter)?);
    for k in 1..n {
        let edge = a + k as f64 * cell + (k - 1) as f64 * gutter;
        if !edge.is_finite() || (gutter > 0.0 && !(edge + gutter).is_finite()) {
            return Err(bad("layout.createGuides", "guide geometry is outside the supported range"));
        }
        v.push(edge);
        if gutter > 0.0 {
            v.push(edge + gutter);
        }
    }
    Ok(v)
}

fn create_guides(s: &mut Session, p: &Value) -> Result<Value> {
    let rows = guide_divisions(p, "rows")?;
    let cols = guide_divisions(p, "columns")?;
    let rg = guide_gutter(p, "rowGutter")?;
    let cg = guide_gutter(p, "columnGutter")?;
    let to_page = str_param(p, "fitTo") == Some("page");
    let remove = p.get("removeExisting").and_then(Value::as_bool).unwrap_or(false);
    let r = guide_spread(p, "layout.createGuides")?;
    let only = optional_usize(p, "page", "layout.createGuides")?;
    let page_count = {
        let sp = s.doc()?.doc.spread(r).ok_or_else(|| bad("layout.createGuides", "no such spread"))?;
        match only {
            Some(page) => {
                if sp.pages.get(page).is_none() {
                    return Err(bad("layout.createGuides", "no such page"));
                }
                1
            }
            None => sp.pages.len(),
        }
    };
    let per_page = guide_count(rows, rg)?
        .checked_add(guide_count(cols, cg)?)
        .ok_or_else(|| bad("layout.createGuides", "the requested guide count is too large"))?;
    per_page
        .checked_mul(page_count)
        .filter(|count| *count <= MAX_GUIDES_CREATED)
        .ok_or_else(|| bad("layout.createGuides", format!("a single command may create at most {MAX_GUIDES_CREATED} guides")))?;
    let layer = s.doc()?.active_layer;
    s.edit(|d, _| {
        let sp = d.spread_mut(r).ok_or_else(|| bad("layout.createGuides", "no such spread"))?;
        let mut n = 0usize;
        for (pi, pg) in sp.pages.iter_mut().enumerate() {
            if only.is_some_and(|o| o != pi) {
                continue;
            }
            if remove {
                pg.guides.clear();
            }
            let area: Rect = if to_page { pg.bounds() } else { pg.margin_rect() };
            for y in grid_positions(area.y0, area.y1, rows, rg)? {
                pg.guides.push(Guide {
                    orientation: Orientation::Horizontal,
                    position: y,
                    spread: false,
                    locked: false,
                    layer: Some(layer),
                    liquid: false,
                });
                n += 1;
            }
            for x in grid_positions(area.x0, area.x1, cols, cg)? {
                pg.guides.push(Guide {
                    orientation: Orientation::Vertical,
                    position: x,
                    spread: false,
                    locked: false,
                    layer: Some(layer),
                    liquid: false,
                });
                n += 1;
            }
        }
        Ok(json!({"guides": n}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_add_move_delete_and_grid() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        // Spread 1 holds pages 2–3 side by side from x = 0.
        let r = s.execute("guide.add", &json!({"spread": 1, "orientation": "vertical", "position": 100.0})).unwrap();
        assert_eq!(r["page"], 0);
        let r = s.execute("guide.add", &json!({"spread": 1, "orientation": "horizontal", "position": 200.0, "at": 700.0})).unwrap();
        assert_eq!(r["page"], 1);
        s.execute("guide.move", &json!({"spread": 1, "page": 1, "index": 0, "position": 250.0})).unwrap();
        let l = s.execute("guide.list", &json!({"spread": 1})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 2);
        assert_eq!(l[1]["position"], 250.0);
        s.execute("guide.delete", &json!({"spread": 1, "page": 0, "index": 0})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({"spread": 1})).unwrap().as_array().unwrap().len(), 1);
        // 3 columns with 12 pt gutters inside the margins: 4 guides; 2 rows without gutter: 1 guide.
        let r = s.execute("layout.createGuides", &json!({"spread": 0, "columns": 3, "rows": 2, "rowGutter": 0.0})).unwrap();
        assert_eq!(r["guides"], 5);
        let d = &s.doc().unwrap().doc;
        let pg = &d.spreads[0].pages[0];
        let m = pg.margin_rect();
        let v: Vec<f64> = pg.guides.iter().filter(|g| g.orientation == Orientation::Vertical).map(|g| g.position).collect();
        let cell = (m.width() - 24.0) / 3.0;
        assert!((v[0] - (m.x0 + cell)).abs() < 1e-9 && (v[1] - (m.x0 + cell + 12.0)).abs() < 1e-9);
        s.execute("guide.deleteAll", &json!({"spread": 0})).unwrap();
        assert!(s.doc().unwrap().doc.spreads[0].pages[0].guides.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].guides.len(), 5);
    }

    #[test]
    fn both_orientations_are_one_undo_step() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("guide.add", &json!({"orientation": "both", "point": [120.0, 80.0], "spreadGuide": true})).unwrap();
        assert_eq!(r["guides"].as_array().unwrap().len(), 2);
        let l = s.execute("guide.list", &json!({})).unwrap();
        assert_eq!((l[0]["orientation"].as_str(), l[0]["position"].as_f64()), (Some("horizontal"), Some(80.0)));
        assert_eq!((l[1]["orientation"].as_str(), l[1]["position"].as_f64()), (Some("vertical"), Some(120.0)));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap(), json!([]));
        assert!(s.execute("guide.add", &json!({"orientation": "both", "point": [1.0]})).is_err());
        assert!(s.execute("guide.add", &json!({"orientation": "both", "position": 1.0})).is_err());
    }

    #[test]
    fn create_guides_validates_generation_limits() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("layout.createGuides", &json!({"columns": MAX_GUIDE_DIVISIONS + 1})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"columns": 2, "columnGutter": -1})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"columns": "many"})).is_err());
        let r = s.execute("layout.createGuides", &json!({"columns": MAX_GUIDE_DIVISIONS, "columnGutter": 0})).unwrap();
        assert_eq!(r["guides"], MAX_GUIDE_DIVISIONS - 1);
    }

    #[test]
    fn mutating_guide_commands_reject_invalid_indices() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100})).unwrap();

        assert!(s.execute("guide.add", &json!({"orientation": "vertical", "position": 100, "page": -1})).is_err());
        assert!(s.execute("guide.move", &json!({"page": 0.5, "index": 0, "position": 200})).is_err());
        assert!(s.execute("guide.delete", &json!({"page": 0, "index": -1})).is_err());
        assert!(s.execute("guide.deleteAll", &json!({"spread": "0"})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"spread": -1, "columns": 2})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"page": u64::MAX, "columns": 2})).is_err());
        assert_eq!(s.execute("guide.list", &json!({})).unwrap().as_array().unwrap().len(), 1);

        s.execute("guide.add", &json!({"spread": {"kind": "doc", "index": 0}, "page": 0, "orientation": "horizontal", "position": 150})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
    }
}

#[cfg(test)]
mod layer_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn guide_commands_respect_layer_locks() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let layer = s.execute("layer.new", &json!({"name": "Grid"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": layer})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100})).unwrap();
        s.execute("layer.set", &json!({"id": layer, "locked": true})).unwrap();
        for command in ["guide.move", "guide.delete"] {
            assert!(s.execute(command, &json!({"page": 0, "index": 0, "position": 200})).is_err(), "{command} changed a locked layer");
            assert_eq!(s.execute("guide.list", &json!({})).unwrap()[0]["position"], 100.0);
        }
        s.execute("layer.set", &json!({"id": layer, "locked": false})).unwrap();
        s.execute("guide.move", &json!({"page": 0, "index": 0, "position": 200})).unwrap();
        s.execute("guide.delete", &json!({"page": 0, "index": 0})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap(), json!([]));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap()[0]["position"], 200.0);
    }

    #[test]
    fn guides_follow_their_layer() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let l = s.execute("layer.new", &json!({"name": "Grid"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": l})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100})).unwrap();
        let g = |s: &Session| s.doc().unwrap().doc.spreads[0].pages[0].guides[0].clone();
        assert_eq!(g(&s).layer.map(|x| x.0), Some(l));
        assert!(g(&s).visible_in(&s.doc().unwrap().doc));
        s.execute("layer.set", &json!({"id": l, "visible": false})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(!g(&s).visible_in(&d) && !g(&s).editable_in(&d));
        s.execute("layer.set", &json!({"id": l, "visible": true, "locked": true})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(g(&s).visible_in(&d) && !g(&s).editable_in(&d));
    }
}
