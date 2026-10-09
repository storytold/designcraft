//! Frame grids and Japanese composition settings: Object › Frame Grid Options, Object › Frame
//! Type, Edit › Apply Grid Format, the frame grid defaults, and the paragraph mojikumi and kinsoku
//! sets (JLREQ; see `designcraft_compose::jlreq`).

use designcraft_doc::{CharAttrs, Document, FrameGrid, Item, ItemId, Shape, Story};
use designcraft_geom::{Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, has_doc, has_selection, has_text_or_frames, str_param, targets};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "object.frameGridOptions",
            "Frame Grid Options…",
            ["Object"],
            None,
            "{fontFamily?, fontStyle?, size?, hScale?, vScale?, charAki?, lineAki?, lineAlign?: left|center|right|leftJustified|…|null (paragraphs' own), gridAlign?: none|romanBaseline|emTop|emCenter|emBottom|icfTop|icfBottom, charAlign? (same values), count?: none|top|bottom|left|right, countSize?, view?: grid|nZ|outline, mojikumi?, chars?, lines?, columns?, gutter?, applyFormat?: bool (default: when the font, size or scale change), ids?} → {grids} — text frames become frame grids; the frame is resized to its cells",
            has_text_or_frames,
            frame_grid_options
        ),
        cmd!(
            query "object.frameGridInfo",
            "Frame Grid Info",
            [],
            None,
            "{ids?} → {grids: [{id, grid, chars, lines, columns, gutter, vertical, count}]} — the frame grids among the selected (or given) frames; count is the characters the frame shows",
            has_doc,
            frame_grid_info
        ),
        cmd!(
            "object.frameType",
            "Frame Type",
            ["Object"],
            None,
            "{type: text|grid, ids?} — Object › Frame Type › Text Frame / Frame Grid",
            has_selection,
            frame_type
        ),
        cmd!(
            "type.applyGridFormat",
            "Apply Grid Format",
            ["Edit"],
            Some("Cmd+Alt+E"),
            "{} — the selected text (or the stories of the selected frame grids) takes its frame grid's font, size and scale",
            has_text_or_frames,
            apply_grid_format
        ),
        cmd!(
            "type.mojikumi",
            "Mojikumi",
            [],
            None,
            "{set: lineEndHalf|halfWidth|fullWidth|\"\" (none: set solid)} — the paragraphs' mojikumi set (JLREQ spacing between character classes)",
            has_text_or_frames,
            |s, p| {
                let set = str_param(p, "set").unwrap_or("");
                if !set.is_empty() && designcraft_compose::jlreq::mojikumi_set(set).is_none() {
                    return Err(bad("type.mojikumi", format!("unknown mojikumi set `{set}`")));
                }
                super::text::format_paras(s, &json!({ "mojikumi": set }))
            }
        ),
        cmd!(query "type.mojikumiSets", "Mojikumi Sets", [], None, "{} → [{id, label}] (the built-in sets; \"\" sets text solid)", always, |_, _| {
            Ok(Value::Array(designcraft_compose::jlreq::MOJIKUMI_SETS.iter().map(|m| json!({"id": m.id, "label": m.label})).collect()))
        }),
        cmd!(
            "type.kinsoku",
            "Kinsoku",
            [],
            None,
            "{set: hard|soft|none|default} — the paragraphs' kinsoku set: hard (small kana and the prolonged sound mark don't start lines either), soft, none (no kinsoku), default (the built-in rules)",
            has_text_or_frames,
            |s, p| {
                let k = match str_param(p, "set").unwrap_or("default") {
                    "hard" => json!(designcraft_doc::cjk::Kinsoku::named("HardKinsoku")),
                    "soft" => json!(designcraft_doc::cjk::Kinsoku::named("SoftKinsoku")),
                    "none" => json!(designcraft_doc::cjk::Kinsoku { name: "None".into(), ..Default::default() }),
                    "default" => Value::Null,
                    other => return Err(bad("type.kinsoku", format!("unknown kinsoku set `{other}`"))),
                };
                super::text::format_paras(s, &json!({ "kinsoku": k }))
            }
        ),
        cmd!(
            "document.frameGridDefaults",
            "Frame Grid Defaults",
            [],
            None,
            "{the grid fields of object.frameGridOptions} → the grid new frame grids get",
            has_doc,
            |s, p| {
                let base = s.doc()?.doc.settings.frame_grid.clone();
                let g = grid_with(&base, p, "document.frameGridDefaults")?;
                let out = json!(g);
                s.edit(move |d, _| {
                    d.settings.frame_grid = g;
                    Ok(out)
                })
            }
        ),
    ]
}

/// Keys of a grid in command parameters.
const GRID_KEYS: [&str; 14] = [
    "fontFamily",
    "fontStyle",
    "size",
    "hScale",
    "vScale",
    "charAki",
    "lineAki",
    "lineAlign",
    "gridAlign",
    "charAlign",
    "count",
    "countSize",
    "view",
    "mojikumi",
];

/// `base` with the grid fields of `p` (validated, numbers sanitized).
pub(crate) fn grid_with(base: &FrameGrid, p: &Value, id: &str) -> Result<FrameGrid> {
    let mut v = serde_json::to_value(base).map_err(|e| bad(id, e.to_string()))?;
    if let (Some(o), Some(m)) = (v.as_object_mut(), p.as_object()) {
        for k in GRID_KEYS {
            if let Some(x) = m.get(k) {
                o.insert(k.into(), x.clone());
            }
        }
    }
    let g: FrameGrid = serde_json::from_value(v).map_err(|e| bad(id, e.to_string()))?;
    if !g.mojikumi.is_empty() && designcraft_compose::jlreq::mojikumi_set(&g.mojikumi).is_none() {
        return Err(bad(id, format!("unknown mojikumi set `{}`", g.mojikumi)));
    }
    Ok(g.sanitized())
}

/// The character attributes Apply Grid Format gives text in grid `g`.
pub(crate) fn grid_format(g: &FrameGrid) -> CharAttrs {
    CharAttrs {
        font_family: (!g.font_family.is_empty()).then(|| g.font_family.clone()),
        font_style: (!g.font_style.is_empty()).then(|| g.font_style.clone()),
        size: Some(g.size),
        h_scale: Some(g.h_scale),
        v_scale: Some(g.v_scale),
        ..Default::default()
    }
}

/// Merge `a` into the characters of `range` of `st` (the typing format when the story is empty).
pub(crate) fn format_story_chars(st: &mut Story, range: std::ops::Range<usize>, a: &CharAttrs) {
    if st.is_empty() {
        if let Some(r) = st.chars.first_mut() {
            r.format.over.merge(a);
        }
        st.rev += 1;
    } else {
        st.format_chars(range, |f| f.over.merge(a));
    }
}

/// The text area of a frame in composition space (lines along x): the turned box of a vertical
/// frame.
fn composition_area(d: &Document, it: &Item) -> (Rect, bool) {
    let vertical = d.frame_vertical(it);
    let a = it.text_area();
    (if vertical { Rect::new(0.0, 0.0, a.height(), a.width()) } else { a }, vertical)
}

/// Characters per line and lines of grid `g` in frame `it`.
fn counts(d: &Document, it: &Item, g: &FrameGrid) -> (u32, u32) {
    let (area, vertical) = composition_area(d, it);
    let o = it.text_frame().map(|t| (t.options.columns, t.options.gutter)).unwrap_or((1, 0.0));
    g.counts_in(area, o.0, o.1, vertical)
}

/// Resize rectangular frame `it` to `chars` × `lines` cells of `g` in `columns` columns: a
/// horizontal frame keeps its top-left corner, a vertical one its top-right (where its text starts).
fn fit_frame(it: &mut Item, g: &FrameGrid, chars: u32, lines: u32, columns: u32, gutter: f64, vertical: bool) {
    if it.shape != Shape::Rectangle {
        return;
    }
    let inset = it.text_frame().map(|t| t.options.inset).unwrap_or([0.0; 4]);
    let (w, h) = g.area_size(chars, lines, columns, gutter, vertical);
    let (w, h) = (w + inset[1] + inset[3], h + inset[0] + inset[2]);
    let r = it.inner_bounds();
    let x0 = if vertical { r.x1 - w } else { r.x0 };
    it.path = shapes::rectangle(Rect::new(x0, r.y0, x0 + w, r.y0 + h));
}

/// The text frames a command acts on: the given or selected frames, else the frames of the story
/// with the text selection.
fn text_frames(s: &Session, p: &Value) -> Result<Vec<ItemId>> {
    let st = s.doc()?;
    let mut ids: Vec<ItemId> = targets(s, p)?.into_iter().filter(|id| st.doc.item(*id).is_some_and(|i| i.text_frame().is_some())).collect();
    if ids.is_empty()
        && let Some(t) = st.selection.text
    {
        ids = st.doc.story(t.story).map(|s| s.frames.clone()).unwrap_or_default();
    }
    Ok(ids)
}

fn count_param(p: &Value, k: &str) -> Option<u32> {
    p.get(k)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .map(|v| v.round().clamp(1.0, f64::from(designcraft_doc::framegrid::MAX_GRID_COUNT)) as u32)
}

fn frame_grid_options(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.frameGridOptions";
    let ids = text_frames(s, p)?;
    if ids.is_empty() {
        return Err(bad(ID, "select a text frame or frame grid"));
    }
    let p = p.clone();
    let format_keys = ["fontFamily", "fontStyle", "size", "hScale", "vScale"];
    let resize_keys = ["size", "hScale", "vScale", "charAki", "lineAki", "chars", "lines", "columns", "gutter"];
    s.edit(move |d, _| {
        let mut out = Vec::new();
        for id in &ids {
            let Some(it) = d.item(*id) else { continue };
            let Some(tf) = it.text_frame() else { continue };
            if tf.options.path.is_some() {
                continue;
            }
            let vertical = d.frame_vertical(it);
            let had = tf.options.frame_grid.clone();
            let old = had.clone().unwrap_or_else(|| d.settings.frame_grid.clone());
            let g = grid_with(&old, &p, ID)?;
            let (c0, l0) = counts(d, it, if had.is_some() { &old } else { &g });
            let columns = p.get("columns").and_then(Value::as_u64).map_or(tf.options.columns, |v| v.clamp(1, 40) as u32);
            let gutter = p.get("gutter").and_then(Value::as_f64).filter(|v| v.is_finite()).map_or(tf.options.gutter, |v| v.max(0.0));
            let chars = count_param(&p, "chars").unwrap_or(c0.max(1));
            let lines = count_param(&p, "lines").unwrap_or(l0.max(1));
            let resize = had.is_none() || resize_keys.iter().any(|k| p.get(*k).is_some());
            let apply = p.get("applyFormat").and_then(Value::as_bool).unwrap_or(had.is_none() || format_keys.iter().any(|k| p.get(*k).is_some()));
            let story = tf.story;
            let Some(it) = d.item_mut(*id) else { continue };
            if let Some(tf) = it.text_frame_mut() {
                tf.options.frame_grid = Some(g.clone());
                tf.options.columns = columns;
                tf.options.gutter = gutter;
            }
            if resize {
                fit_frame(it, &g, chars, lines, columns, gutter, vertical);
            }
            if apply && let Some(st) = d.story_mut(story) {
                let n = st.len();
                format_story_chars(st, 0..n, &grid_format(&g));
            }
            out.push(json!({"id": id.0, "grid": g, "chars": chars, "lines": lines, "columns": columns, "gutter": gutter, "vertical": vertical}));
        }
        Ok(json!({ "grids": out }))
    })
}

fn frame_grid_info(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = text_frames(s, p)?;
    let st = s.doc()?;
    let d = &st.doc;
    let mut out = Vec::new();
    for id in ids {
        let Some(it) = d.item(id) else { continue };
        let Some(tf) = it.text_frame() else { continue };
        let Some(g) = &tf.options.frame_grid else { continue };
        let g = g.sanitized();
        let (chars, lines) = counts(d, it, &g);
        let cs = s.cache.get(d, tf.story, None);
        let count = cs.frame(id).map_or(0, |f| f.lines.iter().flat_map(|l| &l.glyphs).filter(|g| g.visible && g.len > 0).count());
        out.push(json!({
            "id": id.0, "grid": g, "chars": chars, "lines": lines, "columns": tf.options.columns, "gutter": tf.options.gutter,
            "vertical": d.frame_vertical(it), "count": count,
        }));
    }
    Ok(json!({ "grids": out }))
}

fn frame_type(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.frameType";
    let grid = match str_param(p, "type") {
        Some("grid") => true,
        Some("text") => false,
        _ => return Err(bad(ID, "type must be text or grid")),
    };
    let ids = text_frames(s, p)?;
    if ids.is_empty() {
        return Err(bad(ID, "select a text frame"));
    }
    if grid {
        let mut q = p.clone();
        if let Some(o) = q.as_object_mut() {
            o.remove("type");
        }
        // Frames that already are grids keep theirs.
        let plain: Vec<Value> = {
            let d = &s.doc()?.doc;
            ids.iter()
                .filter(|id| d.item(**id).and_then(Item::text_frame).is_some_and(|t| t.options.frame_grid.is_none()))
                .map(|id| json!(id.0))
                .collect()
        };
        if plain.is_empty() {
            return Ok(json!({ "changed": 0 }));
        }
        let n = plain.len();
        let mut q = super::with_param(&q, "ids", Value::Array(plain));
        if let Some(o) = q.as_object_mut() {
            o.remove("id");
        }
        frame_grid_options(s, &q)?;
        return Ok(json!({ "changed": n }));
    }
    s.edit(|d, _| {
        let mut n = 0;
        for id in &ids {
            if let Some(tf) = d.item_mut(*id).and_then(Item::text_frame_mut)
                && tf.options.frame_grid.take().is_some()
            {
                n += 1;
            }
        }
        Ok(json!({ "changed": n }))
    })
}

fn apply_grid_format(s: &mut Session, _p: &Value) -> Result<Value> {
    const ID: &str = "type.applyGridFormat";
    let targets = super::text::format_targets_pub(s);
    let st = s.doc()?;
    let frame = st.selection.text.and_then(|t| t.frame);
    // The grid of the frame showing each target (the selection's frame first).
    let mut jobs = Vec::new();
    for t in targets {
        if t.cell.is_some() {
            continue;
        }
        let Some(story) = st.doc.story(t.story) else { continue };
        let grid = frame
            .into_iter()
            .chain(story.frames.iter().copied())
            .filter_map(|id| st.doc.item(id).and_then(Item::text_frame).filter(|tf| tf.story == t.story).and_then(|tf| tf.options.frame_grid.clone()))
            .next();
        if let Some(g) = grid {
            jobs.push((t.story, t.range, grid_format(&g.sanitized())));
        }
    }
    if jobs.is_empty() {
        return Err(bad(ID, "the text isn't in a frame grid"));
    }
    s.edit(|d, _| {
        for (sid, range, a) in &jobs {
            if let Some(st) = d.story_mut(*sid) {
                format_story_chars(st, range.clone(), a);
            }
        }
        Ok(json!({ "changed": jobs.len() }))
    })
}

/// After a resize of frame grid `it` (inner bounds `before`), round its size to whole cells: the
/// nearest number of characters per line and lines (at least one), keeping the edges the resize
/// didn't move. Only rectangles are snapped.
pub(crate) fn snap_grid_frame(it: &mut Item, before: Rect, vertical: bool) {
    let Some(o) = it.text_frame().map(|t| t.options.clone()) else { return };
    let Some(g) = o.frame_grid.as_ref().map(FrameGrid::sanitized) else { return };
    if it.shape != Shape::Rectangle {
        return;
    }
    let r = it.inner_bounds();
    let (w, h) = (r.width() - o.inset[1] - o.inset[3], r.height() - o.inset[0] - o.inset[2]);
    let (along, across) = if vertical { (h, w) } else { (w, h) };
    let cols = o.columns.clamp(1, 40);
    let col = (along - o.gutter.max(0.0) * f64::from(cols - 1)) / f64::from(cols);
    let n = |len: f64, pitch: f64, aki: f64| {
        let v = ((len + aki) / pitch).round();
        if v.is_finite() { v.clamp(1.0, f64::from(designcraft_doc::framegrid::MAX_GRID_COUNT)) as u32 } else { 1 }
    };
    let chars = n(col, g.char_pitch(vertical), g.char_aki);
    let lines = n(across, g.line_pitch(vertical), g.line_aki);
    let (aw, ah) = g.area_size(chars, lines, cols, o.gutter, vertical);
    let (nw, nh) = (aw + o.inset[1] + o.inset[3], ah + o.inset[0] + o.inset[2]);
    // Keep the edge that stayed (the left edge unless only it moved; likewise the top).
    let moved = |a: f64, b: f64| (a - b).abs() > 1e-6;
    let x0 = if moved(r.x0, before.x0) && !moved(r.x1, before.x1) { r.x1 - nw } else { r.x0 };
    let y0 = if moved(r.y0, before.y0) && !moved(r.y1, before.y1) { r.y1 - nh } else { r.y0 };
    it.path = shapes::rectangle(Rect::new(x0, y0, x0 + nw, y0 + nh));
}

/// A frame grid's text area for a drag `rect` (spread space): whole cells, at least one, the
/// nearest count to the drag; anchored at its top-left (top-right for vertical text).
pub(crate) fn snap_grid_rect(g: &FrameGrid, rect: Rect, vertical: bool) -> Rect {
    let (along, across) = if vertical { (rect.height(), rect.width()) } else { (rect.width(), rect.height()) };
    let n = |len: f64, pitch: f64, aki: f64| {
        let v = ((len + aki) / pitch).round();
        if v.is_finite() { v.clamp(1.0, f64::from(designcraft_doc::framegrid::MAX_GRID_COUNT)) as u32 } else { 1 }
    };
    let chars = n(along, g.char_pitch(vertical), g.char_aki);
    let lines = n(across, g.line_pitch(vertical), g.line_aki);
    let (w, h) = g.area_size(chars, lines, 1, 0.0, vertical);
    if vertical { Rect::new(rect.x1 - w, rect.y0, rect.x1, rect.y0 + h) } else { Rect::new(rect.x0, rect.y0, rect.x0 + w, rect.y0 + h) }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s
    }

    fn grid_of(s: &Session, id: u64) -> designcraft_doc::FrameGrid {
        let d = &s.active().unwrap().doc;
        d.item(designcraft_doc::ItemId(id)).and_then(|i| i.text_frame()).and_then(|t| t.options.frame_grid.clone()).unwrap()
    }

    #[test]
    fn frame_grids_are_drawn_in_whole_cells_and_take_the_grid_format() {
        let mut s = session();
        s.execute("document.frameGridDefaults", &json!({"size": 10, "charAki": 2, "lineAki": 5, "fontFamily": "Source Serif 4"})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 96, 66], "content": "text", "grid": true})).unwrap();
        let id = r["id"].as_u64().unwrap();
        let d = &s.active().unwrap().doc;
        let it = d.item(designcraft_doc::ItemId(id)).unwrap();
        // 60 pt ≈ 5 cells of 12 (58 pt), 30 pt ≈ 2 lines of 15 (25 pt).
        let b = it.bounds();
        assert!((b.width() - 58.0).abs() < 1e-6 && (b.height() - 25.0).abs() < 1e-6, "{b:?}");
        assert_eq!(grid_of(&s, id).size, 10.0);
        s.execute("text.insert", &json!({"text": "漢字"})).unwrap();
        let info = s.execute("object.frameGridInfo", &json!({"ids": [id]})).unwrap();
        assert_eq!(info["grids"][0]["chars"], 5);
        assert_eq!(info["grids"][0]["lines"], 2);
        let d = &s.active().unwrap().doc;
        let st = d.story(designcraft_doc::StoryId(r["story"].as_u64().unwrap())).unwrap();
        let f = d.styles.resolve_char(&d.styles.resolve_para(&st.paras[0]).1, st.char_format_at(1));
        assert_eq!((f.size, f.font_family.as_str()), (10.0, "Source Serif 4"));
    }

    #[test]
    fn frame_grid_options_resize_by_counts_and_undo() {
        let mut s = session();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "caret": false})).unwrap();
        let id = r["id"].as_u64().unwrap();
        s.execute("object.frameType", &json!({"ids": [id], "type": "grid"})).unwrap();
        assert!(grid_of(&s, id).size > 0.0);
        let out = s
            .execute(
                "object.frameGridOptions",
                &json!({"ids": [id], "size": 9, "lineAki": 4.5, "chars": 20, "lines": 10, "columns": 2, "gutter": 18}),
            )
            .unwrap();
        assert_eq!(out["grids"][0]["chars"], 20);
        let d = &s.active().unwrap().doc;
        let b = d.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((b.width() - (2.0 * 180.0 + 18.0)).abs() < 1e-6, "{b:?}");
        assert!((b.height() - (10.0 * 13.5 - 4.5)).abs() < 1e-6, "{b:?}");
        s.execute("edit.undo", &json!({})).unwrap();
        let d = &s.active().unwrap().doc;
        assert!(d.item(designcraft_doc::ItemId(id)).unwrap().bounds().width() < 300.0);
        s.execute("object.frameType", &json!({"ids": [id], "type": "text"})).unwrap();
        let d = &s.active().unwrap().doc;
        assert!(d.item(designcraft_doc::ItemId(id)).and_then(|i| i.text_frame()).unwrap().options.frame_grid.is_none());
    }

    #[test]
    fn hostile_grid_parameters_are_rejected_or_clamped() {
        let mut s = session();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "grid": {"size": -5, "charAki": 1e300}})).unwrap();
        let id = r["id"].as_u64().unwrap();
        let g = grid_of(&s, id);
        assert!(g.size > 0.0 && g.char_aki.is_finite() && g.char_aki < 1e6);
        assert!(s.execute("object.frameGridOptions", &json!({"ids": [id], "gridAlign": "sideways"})).is_err());
        assert!(s.execute("object.frameGridOptions", &json!({"ids": [id], "mojikumi": "vendor table"})).is_err());
        s.execute("object.frameGridOptions", &json!({"ids": [id], "chars": 1e12, "lines": -3})).unwrap();
        assert!(s.execute("type.mojikumi", &json!({"set": "nope"})).is_err());
    }

    #[test]
    fn mojikumi_and_kinsoku_sets_reach_the_paragraphs() {
        let mut s = session();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "漢字、かな。"})).unwrap();
        s.execute("type.mojikumi", &json!({"set": "lineEndHalf"})).unwrap();
        s.execute("type.kinsoku", &json!({"set": "soft"})).unwrap();
        let st = s.active().unwrap();
        let story = st.doc.stories.values().next().unwrap();
        assert_eq!(story.paras[0].para.mojikumi.as_deref(), Some("lineEndHalf"));
        assert_eq!(story.paras[0].para.kinsoku.clone().flatten().map(|k| k.name), Some("SoftKinsoku".to_string()));
        let sets = s.execute("type.mojikumiSets", &json!({})).unwrap();
        assert!(sets.as_array().unwrap().iter().any(|m| m["id"] == "lineEndHalf"));
    }

    #[test]
    fn resizing_a_frame_grid_snaps_to_whole_cells() {
        let mut s = session();
        s.execute("document.frameGridDefaults", &json!({"size": 10, "charAki": 2, "lineAki": 5})).unwrap();
        // 5 × 2 cells: 58 × 25 at (36, 36).
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 96, 66], "content": "text", "grid": true, "caret": false})).unwrap();
        let id = r["id"].as_u64().unwrap();
        let bounds = |s: &Session| s.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        // Drag the bottom-right corner to (130, 101): 94 pt = 8 cells (8 × 12 − 2), 65 pt ≈ 5 lines (70).
        s.execute("transform.resize", &json!({"ids": [id], "from": [36, 36, 94, 61], "to": [36, 36, 130, 101]})).unwrap();
        let b = bounds(&s);
        assert!((b.x0 - 36.0).abs() < 1e-6 && (b.y0 - 36.0).abs() < 1e-6, "{b:?}");
        assert!((b.width() - 94.0).abs() < 1e-6 && (b.height() - 70.0).abs() < 1e-6, "{b:?}");
        // Drag the left edge out: the right edge stays, the width is whole cells.
        s.execute("transform.resize", &json!({"ids": [id], "from": [36, 36, 130, 106], "to": [11, 36, 130, 106]})).unwrap();
        let b = bounds(&s);
        assert!((b.x1 - 130.0).abs() < 1e-6, "{b:?}");
        assert!(((b.width() + 2.0) / 12.0 - ((b.width() + 2.0) / 12.0).round()).abs() < 1e-9, "{b:?}");
        // The Control panel's width field snaps too.
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        s.execute("transform.set", &json!({"width": 50, "ref": 0})).unwrap();
        let w = bounds(&s).width();
        assert!(((w + 2.0) / 12.0 - ((w + 2.0) / 12.0).round()).abs() < 1e-9, "{w}");
        let b = bounds(&s);
        // Shrinking past one cell keeps one.
        s.execute("transform.resize", &json!({"ids": [id], "from": [b.x0, b.y0, b.x1, b.y1], "to": [b.x0, b.y0, b.x0 + 1.0, b.y0 + 1.0]})).unwrap();
        let b = bounds(&s);
        assert!((b.width() - 10.0).abs() < 1e-6 && (b.height() - 10.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn grid_rects_snap_to_whole_cells() {
        let g = designcraft_doc::FrameGrid { size: 10.0, ..Default::default() };
        let r = super::snap_grid_rect(&g, designcraft_geom::Rect::new(0.0, 0.0, 101.0, 2.0), false);
        assert_eq!((r.width(), r.height()), (100.0, 10.0));
        let r = super::snap_grid_rect(&g, designcraft_geom::Rect::new(0.0, 0.0, 40.0, 101.0), true);
        assert_eq!((r.x1, r.height()), (40.0, 100.0));
    }
}
