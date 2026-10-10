//! Ruler guides: add (dragged from the rulers), move, delete, list; Layout › Create Guides.
//!
//! Guides are stored on their page in spread coordinates; a spread guide (dropped on the
//! pasteboard) lives on the spread's first page with `spread: true` and crosses the pasteboard.

use designcraft_doc::{Document, Guide, Orientation, SpreadRef};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, spread_param, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "guide.add",
            "New Guide",
            [],
            None,
            "{orientation: horizontal|vertical, position (spread coordinate), spread?, page? (index in the spread; default: the page under `at` or the position), at? (the other coordinate), spreadGuide?: bool} → {page, index}",
            has_doc,
            add
        ),
        cmd!("guide.move", "Move Guide", [], None, "{spread?, page, index, position}", has_doc, move_guide),
        cmd!("guide.delete", "Delete Guide", [], None, "{spread?, page, index}", has_doc, delete),
        cmd!("guide.deleteAll", "Delete All Guides on Spread", ["View", "Grids & Guides"], None, "{spread?}", has_doc, |s, p| {
            let r = spread_param(p, "spread");
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
            let sp = d.spread(spread_param(p, "spread")).ok_or_else(|| bad("guide.list", "no such spread"))?;
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
            "{rows?: 0, columns?: 0 (at most 100 each), rowGutter?: 12, columnGutter?: 12, fitTo?: margins|page, removeExisting?: false, spread?, page? (index in the spread; default all pages)}",
            has_doc,
            create_guides
        ),
    ]
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
    let o = orientation(p)?;
    let pos = p.get("position").and_then(Value::as_f64).ok_or_else(|| bad("guide.add", "missing position"))?;
    let r = spread_param(p, "spread");
    let spread_guide = p.get("spreadGuide").and_then(Value::as_bool).unwrap_or(false);
    let d = &s.doc()?.doc;
    let pi = match p.get("page").and_then(Value::as_u64) {
        Some(pi) => pi as usize,
        None if spread_guide => 0,
        None => {
            let x = match o {
                Orientation::Vertical => pos,
                Orientation::Horizontal => p.get("at").and_then(Value::as_f64).unwrap_or(0.0),
            };
            page_at(d, r, x)
        }
    };
    let layer = s.doc()?.active_layer;
    s.edit(|d, _| {
        let sp = d.spread_mut(r).ok_or_else(|| bad("guide.add", "no such spread"))?;
        let pg = sp.pages.get_mut(pi).ok_or_else(|| bad("guide.add", "no such page"))?;
        pg.guides.push(Guide { orientation: o, position: pos, spread: spread_guide, locked: false, layer: Some(layer), liquid: false });
        Ok(json!({"page": pi, "index": pg.guides.len() - 1}))
    })
}

fn guide_ref(p: &Value, cmd: &str) -> Result<(SpreadRef, usize, usize)> {
    let pi = p.get("page").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "missing page"))? as usize;
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "missing index"))? as usize;
    Ok((spread_param(p, "spread"), pi, i))
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
    let pos = p.get("position").and_then(Value::as_f64).ok_or_else(|| bad("guide.move", "missing position"))?;
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

/// Evenly spaced rows/columns with gutters inside `area`: the guide positions.
pub(crate) fn grid_positions(a: f64, b: f64, n: u32, gutter: f64) -> Vec<f64> {
    if n < 2 {
        return vec![];
    }
    let cell = ((b - a) - gutter * (n - 1) as f64) / n as f64;
    let mut v = Vec::new();
    for k in 1..n {
        let edge = a + k as f64 * cell + (k - 1) as f64 * gutter;
        v.push(edge);
        if gutter > 0.0 {
            v.push(edge + gutter);
        }
    }
    v
}

/// The most rows or columns Create Guides lays out. The count comes from the caller, and every
/// guide is stored in the document.
const MAX_GUIDE_GRID: u32 = 100;

fn create_guides(s: &mut Session, p: &Value) -> Result<Value> {
    let count = |key: &str| p.get(key).and_then(Value::as_u64).unwrap_or(0).min(u64::from(MAX_GUIDE_GRID)) as u32;
    let (rows, cols) = (count("rows"), count("columns"));
    let rg = p.get("rowGutter").and_then(Value::as_f64).unwrap_or(12.0);
    let cg = p.get("columnGutter").and_then(Value::as_f64).unwrap_or(12.0);
    let to_page = str_param(p, "fitTo") == Some("page");
    let remove = p.get("removeExisting").and_then(Value::as_bool).unwrap_or(false);
    let r = spread_param(p, "spread");
    let only = p.get("page").and_then(Value::as_u64).map(|v| v as usize);
    let layer = s.doc()?.active_layer;
    s.edit(|d, _| {
        let sp = d.spread_mut(r).ok_or_else(|| bad("layout.createGuides", "no such spread"))?;
        let mut n = 0;
        for (pi, pg) in sp.pages.iter_mut().enumerate() {
            if only.is_some_and(|o| o != pi) {
                continue;
            }
            if remove {
                pg.guides.clear();
            }
            let area: Rect = if to_page { pg.bounds() } else { pg.margin_rect() };
            for y in grid_positions(area.y0, area.y1, rows, rg) {
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
            for x in grid_positions(area.x0, area.x1, cols, cg) {
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

    /// A huge row or column count once made that many guides (2³² − 1 of them exhaust memory, which
    /// no guard catches), and a count past 2³² wrapped to a small one.
    #[test]
    fn create_guides_caps_the_row_and_column_counts() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let args = json!({"spread": 0, "rows": 4_294_967_298_u64, "columns": 3_000_000, "rowGutter": 0.0, "columnGutter": 0.0});
        let r = s.execute("layout.createGuides", &args).unwrap();
        let per_page = 2 * (u64::from(MAX_GUIDE_GRID) - 1);
        assert_eq!(r["guides"], per_page * s.doc().unwrap().doc.spreads[0].pages.len() as u64);
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
