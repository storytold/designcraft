use super::target::{edit_table, target};
use super::{CellRange, CellStroke, Document, Result, Session, StrokeType, Table, Value, VerticalJustification, bad, compose, json, ok, str_param};

/// Cell edges chosen for a stroke edit: the target range's four outer sides (physical left and
/// right) and the lines between its rows and columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EdgeSel {
    top: bool,
    left: bool,
    bottom: bool,
    right: bool,
    inner_h: bool,
    inner_v: bool,
}

impl EdgeSel {
    const ALL: EdgeSel = EdgeSel { top: true, left: true, bottom: true, right: true, inner_h: true, inner_v: true };

    fn parse(v: Option<&Value>, cmd: &str) -> Result<EdgeSel> {
        let names: Vec<&Value> = match v {
            None | Some(Value::Null) => return Ok(EdgeSel::ALL),
            Some(Value::Array(a)) => a.iter().collect(),
            Some(v) => vec![v],
        };
        let mut e = EdgeSel::default();
        for n in names {
            match n.as_str() {
                Some("all") => e = EdgeSel::ALL,
                Some("outer") => (e.top, e.left, e.bottom, e.right) = (true, true, true, true),
                Some("inner") => (e.inner_h, e.inner_v) = (true, true),
                Some("top") => e.top = true,
                Some("left") => e.left = true,
                Some("bottom") => e.bottom = true,
                Some("right") => e.right = true,
                Some("innerHorizontal") => e.inner_h = true,
                Some("innerVertical") => e.inner_v = true,
                _ => return Err(bad(cmd, format!("unknown edge {n}; use all|outer|inner|top|left|bottom|right|innerHorizontal|innerVertical"))),
            }
        }
        if e == EdgeSel::default() {
            return Err(bad(cmd, "no edges chosen"));
        }
        Ok(e)
    }

    /// Which sides (top, left, bottom, right) of the cell at `(r, c)` with `spans` are chosen
    /// within `rg`. Left and right are physical sides, also in RTL tables.
    fn sides(&self, rg: CellRange, (r, c): (usize, usize), spans: (u32, u32), rtl: bool) -> [bool; 4] {
        let last_row = r.saturating_add((spans.0.max(1) - 1) as usize);
        let last_col = c.saturating_add((spans.1.max(1) - 1) as usize);
        let (start, end) = (c <= rg.c0, last_col >= rg.c1);
        let (left_outer, right_outer) = if rtl { (end, start) } else { (start, end) };
        [
            if r <= rg.r0 { self.top } else { self.inner_h },
            if left_outer { self.left } else { self.inner_v },
            if last_row >= rg.r1 { self.bottom } else { self.inner_h },
            if right_outer { self.right } else { self.inner_v },
        ]
    }
}

/// A finite number in `lo..=hi`, or an error naming `key`.
fn num_in(v: &Value, key: &str, lo: f64, hi: f64, cmd: &str) -> Result<Option<f64>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(x) => match x.as_f64() {
            Some(n) if n.is_finite() && (lo..=hi).contains(&n) => Ok(Some(n)),
            _ => Err(bad(cmd, format!("{key} must be a number from {lo} to {hi}"))),
        },
    }
}

/// A swatch of the document (or [None]), or an error naming `key`.
fn swatch_in(doc: &Document, v: &Value, keys: &[&str], cmd: &str) -> Result<Option<String>> {
    let Some((key, x)) = keys.iter().find_map(|k| v.get(*k).filter(|x| !x.is_null()).map(|x| (*k, x))) else { return Ok(None) };
    let name = x.as_str().ok_or_else(|| bad(cmd, format!("{key} must be a swatch name")))?;
    if name != designcraft_color::swatch::NONE && doc.swatch(name).is_none() {
        return Err(bad(cmd, format!("no swatch `{name}`")));
    }
    Ok(Some(name.to_string()))
}

/// The cell stroke type names of `type`.
const CELL_STROKE_TYPES: &[&str] =
    &["solid", "dashed", "dotted", "thickThin", "thinThick", "thinThin", "thickThick", "thinThickThin", "thickThinThick", "wavy", "hashed"];

/// The attributes a stroke edit sets. A plain `"dashed"` gets a pattern scaled to the
/// resulting weight of each edge.
pub(super) struct StrokeEdit {
    attrs: designcraft_doc::CellStrokeAttrs,
    scaled_dashes: bool,
}

impl StrokeEdit {
    pub(super) fn parse(doc: &Document, v: &Value, cmd: &str) -> Result<StrokeEdit> {
        if !v.is_object() {
            return Err(bad(cmd, "stroke must be an object"));
        }
        let mut attrs = designcraft_doc::CellStrokeAttrs {
            weight: num_in(v, "weight", 0.0, 1000.0, cmd)?,
            color: swatch_in(doc, v, &["color"], cmd)?,
            tint: num_in(v, "tint", 0.0, 1.0, cmd)?.map(|t| t as f32),
            gap_color: swatch_in(doc, v, &["gapColor", "gapSwatch"], cmd)?,
            gap_tint: num_in(v, "gapTint", 0.0, 1.0, cmd)?.map(|t| t as f32),
            kind: None,
        };
        let mut scaled_dashes = false;
        match v.get("type") {
            None | Some(Value::Null) => {}
            Some(Value::String(name)) if name == "dashed" => {
                scaled_dashes = true;
                attrs.kind = Some(StrokeType::Dashed { pattern: Vec::new() });
            }
            Some(Value::String(name)) if CELL_STROKE_TYPES.contains(&name.as_str()) => {
                attrs.kind = Some(serde_json::from_value(json!({"kind": name})).map_err(|e| bad(cmd, e.to_string()))?);
            }
            Some(t @ Value::Object(_)) => {
                let kind: StrokeType = serde_json::from_value(t.clone()).map_err(|e| bad(cmd, format!("bad stroke type: {e}")))?;
                let finite = |v: &[f64]| v.len() <= 64 && v.iter().all(|x| x.is_finite() && *x >= 0.0);
                match &kind {
                    StrokeType::Dashed { pattern } if !StrokeType::valid_dash_pattern(pattern) => {
                        return Err(bad(cmd, "a dash pattern needs 1 to 64 lengths, each 0 or 0.1 to 10000 pt, not all 0"));
                    }
                    StrokeType::Stripes { bands } if bands.len() > 64 || !bands.iter().all(|(a, b)| finite(&[*a, *b])) => {
                        return Err(bad(cmd, "stripes need up to 64 bands of non-negative numbers"));
                    }
                    StrokeType::Style { name } if !doc.stroke_styles.iter().any(|s| s.name == *name) => {
                        return Err(bad(cmd, format!("no stroke style `{name}`")));
                    }
                    _ => {}
                }
                attrs.kind = Some(kind);
            }
            Some(other) => return Err(bad(cmd, format!("unknown stroke type {other}"))),
        }
        Ok(StrokeEdit { attrs, scaled_dashes })
    }

    pub(super) fn apply(&self, mut base: CellStroke) -> CellStroke {
        self.attrs.apply_to(&mut base);
        if self.scaled_dashes {
            let w = base.weight.max(1.0);
            base.kind = StrokeType::Dashed { pattern: vec![w * 3.0, w * 2.0] };
        }
        base
    }
}

/// The stroke drawn on side `side` of the owner cell at `(r, c)` (where that side meets its
/// first neighbour), as rendering resolves it; the cell's own copy if the side has no stretch.
fn drawn_side(t: &Table, owners: &[(usize, usize)], (r, c): (usize, usize), side: usize) -> Option<CellStroke> {
    let drawn = compose::table::cell_side_strokes(t, owners, (r, c), side).first().map(|s| s.stroke.clone());
    drawn.or_else(|| t.cell(r, c).and_then(|cell| cell.strokes.get(side).cloned()))
}

/// Cell Options › Text › First Baseline: `firstBaseline` (offset method) and `firstBaselineMin` (pt).
pub(super) fn first_baseline_params(p: &Value, id: &str) -> Result<(Option<designcraft_doc::FirstBaseline>, Option<f64>)> {
    let fb = match p.get("firstBaseline").filter(|v| !v.is_null()) {
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| bad(id, format!("firstBaseline: {e}")))?),
        None => None,
    };
    let min = match p.get("firstBaselineMin").filter(|v| !v.is_null()) {
        Some(v) => match v.as_f64().filter(|m| m.is_finite() && (0.0..=8640.0).contains(m)) {
            Some(m) => Some(m),
            None => return Err(bad(id, "firstBaselineMin: a number of points from 0 to 8640")),
        },
        None => None,
    };
    Ok((fb, min))
}

fn mirror_edited_cell_edges(
    table: &mut Table,
    range: CellRange,
    table_size: (usize, usize, bool),
    stroke: Option<&(StrokeEdit, EdgeSel)>,
    edited: &std::collections::HashSet<(usize, usize, usize)>,
    owners: &[(usize, usize)],
) {
    let (rows, columns, rtl) = table_size;
    let mirror = |table: &mut Table, from: (usize, usize), edge: usize, to: (usize, usize), opposite: usize| {
        if edited.contains(&(from.0, from.1, edge)) {
            mirror_cell_edge(table, owners, from, edge, to, opposite);
        }
    };
    let Some((_, edges)) = stroke else { return };
    if edges.top
        && let Some(row) = range.r0.checked_sub(1)
    {
        for column in range.c0..=range.c1 {
            mirror(table, (range.r0, column), 0, (row, column), 2);
        }
    }
    if edges.bottom && range.r1.saturating_add(1) < rows {
        for column in range.c0..=range.c1 {
            mirror(table, (range.r1, column), 2, (range.r1 + 1, column), 0);
        }
    }
    if (if rtl { edges.right } else { edges.left })
        && let Some(column) = range.c0.checked_sub(1)
    {
        for row in range.r0..=range.r1 {
            mirror(table, (row, range.c0), if rtl { 3 } else { 1 }, (row, column), if rtl { 1 } else { 3 });
        }
    }
    if (if rtl { edges.left } else { edges.right }) && range.c1.saturating_add(1) < columns {
        for row in range.r0..=range.r1 {
            mirror(table, (row, range.c1), if rtl { 1 } else { 3 }, (row, range.c1 + 1), if rtl { 3 } else { 1 });
        }
    }
}

pub(super) fn set_cell(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.setCell";
    let g = target(s, p, ID)?;
    let doc = &s.doc()?.doc;
    let fill = swatch_in(doc, p, &["fill"], ID)?;
    let tint = num_in(p, "tint", 0.0, 1.0, ID)?.map(|t| t as f32);
    let insets: Option<[f64; 4]> = match p.get("insets") {
        Some(Value::Number(n)) => n.as_f64().map(|v| [v.max(0.0); 4]),
        Some(Value::Array(a)) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0).max(0.0)).collect();
            Some([v[0], v[1], v[2], v[3]])
        }
        _ => None,
    };
    let vj = match str_param(p, "vj").or_else(|| str_param(p, "verticalJustification")) {
        Some("top") => Some(VerticalJustification::Top),
        Some("center") => Some(VerticalJustification::Center),
        Some("bottom") => Some(VerticalJustification::Bottom),
        Some("justify") => Some(VerticalJustification::Justify),
        Some(o) => return Err(bad(ID, format!("unknown vj `{o}`"))),
        None => None,
    };
    let (first_baseline, first_baseline_min) = first_baseline_params(p, ID)?;
    // Cell Options › Text › Rotation. The value is kept and written back to IDML; only 0 is
    // composed so far, which the IDML import reports (#1468).
    let rotation = match p.get("rotation").filter(|v| !v.is_null()) {
        Some(v) => match v.as_f64().map(|d| d.rem_euclid(360.0)) {
            Some(d) if d == 0.0 || d == 90.0 || d == 180.0 || d == 270.0 => Some(d),
            _ => return Err(bad(ID, "rotation: 0, 90, 180 or 270 degrees")),
        },
        None => None,
    };
    let text = str_param(p, "text").map(|_| crate::cmd::text_param(p, "text"));
    let stroke = match p.get("stroke").filter(|v| !v.is_null()) {
        Some(sv) => Some((StrokeEdit::parse(doc, sv, ID)?, EdgeSel::parse(sv.get("edges"), ID)?)),
        None => None,
    };
    edit_table(s, &g, ID, |t| {
        let owners = t.owners();
        let nc = t.ncols();
        let nr = t.nrows();
        let rtl = t.options.direction == designcraft_doc::TextDirection::RightToLeft;
        // Local edits outrank previously imported edges, including explicit zero/None.
        let priority = if stroke.is_some() { next_cell_stroke_priority(t) } else { 0 };
        let rg = g.range;
        // Edges are inner or outer within the selection grown to whole merged cells, so a
        // merged cell selected through one of its grid positions keeps its own outline.
        let outline = t.expand_range(rg);
        // Each chosen edge starts from the stroke drawn there, which both copies of a shared
        // edge then take, so the edit changes only the attributes it names.
        let mut bases = std::collections::HashMap::new();
        if let Some((_, edges)) = &stroke {
            for (r, c) in t.selected_owners(rg) {
                let Some(cell) = t.cell(r, c) else { continue };
                for (i, on) in edges.sides(outline, (r, c), (cell.row_span, cell.col_span), rtl).into_iter().enumerate() {
                    if let Some(base) = drawn_side(t, &owners, (r, c), i).filter(|_| on) {
                        bases.insert((r, c, i), base);
                    }
                }
            }
        }
        let mut edited = std::collections::HashSet::new();
        for (r, c) in t.selected_owners(rg) {
            let Some(cell) = t.cell_mut(r, c) else { continue };
            if let Some(f) = &fill {
                cell.fill = f.clone();
            }
            if let Some(ti) = tint {
                cell.fill_tint = ti;
            }
            if let Some(i) = insets {
                cell.insets = i;
            }
            if let Some(v) = vj {
                cell.vj = v;
            }
            if let Some(v) = first_baseline {
                cell.first_baseline = v;
            }
            if let Some(v) = first_baseline_min {
                cell.first_baseline_min = v;
            }
            if let Some(v) = rotation {
                cell.rotation = v;
            }
            if let Some(tx) = &text {
                let len = cell.text.len();
                cell.text.replace(0..len, tx);
            }
            if let Some((edit, _)) = &stroke {
                for i in 0..4 {
                    if let Some(base) = bases.remove(&(r, c, i)) {
                        edited.insert((r, c, i));
                        cell.strokes[i] = edit.apply(base);
                        cell.stroke_defined[i] = true;
                        cell.stroke_priorities[i] = priority;
                        cell.border_overrides[i] = true;
                    }
                }
            }
        }
        // Keep both copies of unmerged shared boundaries consistent, including in RTL tables.
        mirror_edited_cell_edges(t, rg, (nr, nc, rtl), stroke.as_ref(), &edited, &owners);
        ok()
    })
}

/// The shown stroke attributes of the chosen edges of the target cells, and their fill; values
/// that differ are null.
pub(super) fn get_cell_stroke(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.getCellStroke";
    let g = target(s, p, ID)?;
    let edges = EdgeSel::parse(p.get("edges"), ID)?;
    let st = s.doc()?;
    let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad(ID, "no table"))?;
    let owners = t.owners();
    let outline = t.expand_range(g.range);
    let rtl = t.options.direction == designcraft_doc::TextDirection::RightToLeft;
    // One value per key, or null once two edges (cells) disagree.
    fn merge(slot: &mut Option<Value>, v: Value) {
        match slot {
            None => *slot = Some(v),
            Some(cur) if *cur != v => *cur = Value::Null,
            _ => {}
        }
    }
    let keys = ["weight", "color", "tint", "type", "gapColor", "gapTint"];
    let mut stroke: [Option<Value>; 6] = Default::default();
    let (mut fill, mut fill_tint) = (None, None);
    // Each drawn edge once: a shared edge is a side of both its cells.
    let mut seen = std::collections::HashSet::new();
    for (r, c) in t.selected_owners(g.range) {
        let Some(cell) = t.cell(r, c) else { continue };
        merge(&mut fill, json!(cell.fill));
        merge(&mut fill_tint, json!(cell.fill_tint));
        for (i, on) in edges.sides(outline, (r, c), (cell.row_span, cell.col_span), rtl).into_iter().enumerate() {
            if !on {
                continue;
            }
            for side in compose::table::cell_side_strokes(t, &owners, (r, c), i) {
                let (x, y) = (Some((r, c)), side.neighbor);
                if !seen.insert((i % 2, side.boundary, x.min(y), x.max(y))) {
                    continue;
                }
                let e = side.stroke;
                let values = [json!(e.weight), json!(e.color), json!(e.tint), json!(e.kind), json!(e.gap_color), json!(e.gap_tint)];
                for (slot, v) in stroke.iter_mut().zip(values) {
                    merge(slot, v);
                }
            }
        }
    }
    let mut out = serde_json::Map::new();
    for (k, v) in keys.iter().zip(stroke) {
        out.insert((*k).into(), v.unwrap_or(Value::Null));
    }
    out.insert("fill".into(), fill.unwrap_or(Value::Null));
    out.insert("fillTint".into(), fill_tint.unwrap_or(Value::Null));
    out.insert("edges".into(), json!(seen.len()));
    Ok(Value::Object(out))
}

/// Only saturation needs rebasing. Preserve every distinct priority's order and keep
/// zero/negative priorities untouched; the work is bounded by the table's edges.
fn next_cell_stroke_priority(t: &mut Table) -> i32 {
    let highest = t.cells.iter().flat_map(|cell| cell.stroke_priorities).max().unwrap_or(0).max(0);
    if highest < i32::MAX {
        return highest + 1;
    }
    let values: std::collections::BTreeSet<_> = t.cells.iter().flat_map(|cell| cell.stroke_priorities).filter(|p| *p > 0).collect();
    let mut ranks = std::collections::BTreeMap::new();
    let mut highest = 0i32;
    for value in values {
        highest = highest.saturating_add(1);
        ranks.insert(value, highest);
    }
    for cell in &mut t.cells {
        for priority in &mut cell.stroke_priorities {
            if let Some(rank) = ranks.get(priority) {
                *priority = *rank;
            }
        }
    }
    highest.saturating_add(1)
}

fn mirror_cell_edge(t: &mut Table, owners: &[(usize, usize)], from: (usize, usize), edge: usize, to: (usize, usize), opposite: usize) {
    let owner = |pos: (usize, usize)| owners.get(pos.0.checked_mul(t.ncols())?.checked_add(pos.1)?).copied();
    if owner(from) != Some(from) || owner(to) != Some(to) {
        return;
    }
    // A merged cell's edge may border several separately editable cells. Mirroring
    // a segment onto that whole edge would change unselected neighbors; priorities
    // instead resolve each segment without altering the merged cell's formatting.
    let merged = |cell: &designcraft_doc::Cell| cell.row_span > 1 || cell.col_span > 1;
    if t.cell(from.0, from.1).is_some_and(merged) || t.cell(to.0, to.1).is_some_and(merged) {
        return;
    }
    let source = t
        .cell(from.0, from.1)
        .map(|cell| (cell.strokes[edge].clone(), cell.stroke_defined[edge], cell.stroke_priorities[edge], cell.border_overrides[edge]));
    if let (Some((stroke, defined, priority, border_override)), Some(neighbor)) = (source, t.cell_mut(to.0, to.1)) {
        neighbor.strokes[opposite] = stroke;
        neighbor.stroke_defined[opposite] = defined;
        neighbor.stroke_priorities[opposite] = priority;
        neighbor.border_overrides[opposite] = border_override;
    }
}

#[cfg(test)]
mod arabic_tests {
    use super::*;
    #[test]
    fn arabic_table_direction_and_physical_left_insertion_support_undo() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 400, 300], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
        s.execute("table.setCell", &json!({"rows": [0, 0], "cols": [0, 0], "text": "Right"})).unwrap();
        s.execute("table.options", &json!({"direction": "rightToLeft"})).unwrap();
        s.execute("table.select", &json!({"rows": [0, 0], "cols": [0, 0]})).unwrap();
        s.execute("table.insertColumnLeft", &json!({})).unwrap();
        let t = s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
        assert_eq!(t.ncols(), 3);
        assert_eq!(t.cell(0, 0).unwrap().text.text, "Right");
        assert!(t.cell(0, 1).unwrap().text.text.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        let t = s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
        assert_eq!(t.ncols(), 2);
        assert_eq!(t.options.direction, designcraft_doc::TextDirection::RightToLeft);
    }

    /// A text frame whose paragraphs have a paragraph style with `direction`, with the story's
    /// text selected. Returns the session and the story id.
    fn styled_frame(direction: &str, text: &str) -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "A4", "pages": 1})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Dir", "para": {"direction": direction}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [40, 330, 555, 420], "content": "text", "text": text, "caret": true})).unwrap();
        s.execute("edit.selectAll", &json!({})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Dir"})).unwrap();
        s.execute("edit.selectAll", &json!({})).unwrap();
        (s, r["story"].as_u64().unwrap())
    }

    fn only_table(s: &Session) -> std::sync::Arc<designcraft_doc::Table> {
        s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap().clone()
    }

    /// Text converted to a table in a right-to-left paragraph laid its columns out left to right
    /// (#100): the first column belongs on the right.
    #[test]
    fn tables_take_the_direction_of_their_paragraph() {
        use designcraft_doc::TextDirection::{LeftToRight, RightToLeft};
        let (mut s, sid) = styled_frame("rightToLeft", "١\t٢\t٣\nأ\tب\tج");
        s.execute("table.convertFromText", &json!({"columnSeparator": "tab"})).unwrap();
        assert_eq!(only_table(&s).options.direction, RightToLeft);
        let st = s.doc().unwrap();
        let cs = s.cache.get(&st.doc, designcraft_doc::StoryId(sid), None);
        let placed = &cs.frames[0].tables[0];
        let (first, last) = (placed.cell(0, 0).unwrap().rect, placed.cell(0, 2).unwrap().rect);
        assert!(first.x0 > last.x0, "cell ١ is the rightmost: {first:?} {last:?}");

        let (mut s, _) = styled_frame("leftToRight", "1\t2\t3\na\tb\tc");
        s.execute("table.convertFromText", &json!({"columnSeparator": "tab"})).unwrap();
        assert_eq!(only_table(&s).options.direction, LeftToRight);

        for (direction, expected) in [("rightToLeft", RightToLeft), ("leftToRight", LeftToRight)] {
            let (mut s, _) = styled_frame(direction, "x");
            s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
            assert_eq!(only_table(&s).options.direction, expected, "table.insert in a {direction} paragraph");
        }
    }
}
