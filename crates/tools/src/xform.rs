//! Rotate (R), Scale (S) and Shear (O) tools: drag around the selection centre (Alt-click sets
//! the reference point elsewhere in a later iteration); Eyedropper (I): click an object to apply
//! its attributes to the selection, or a placed image to colour the selection (text or objects)
//! with the image's colour under the pointer.

use designcraft_doc::SpreadRef;
use designcraft_geom::{Point, Rect};
use serde_json::json;

use crate::{Action, Cursor, Gesture, Mods, Overlay, PointerEvent, PointerKind, SnapRequest, Tool, ToolContext};

pub struct XformTool {
    id: &'static str,
    drag: Option<(Point, Point)>,
    guides: Vec<Overlay>,
    /// First selected item's rotation at pointer down. Rotate only. Later samples see the preview.
    start_rotation: Option<f64>,
}

impl XformTool {
    pub fn new(id: &'static str) -> Self {
        Self { id, drag: None, guides: Vec::new(), start_rotation: None }
    }
}

/// Top-level selection on one spread, in that spread's coordinates.
fn selection_spread_bounds(cx: &ToolContext) -> Option<(SpreadRef, Rect)> {
    let mut spread = None;
    let mut rect: Option<Rect> = None;
    for id in &cx.selection.items {
        let loc = cx.doc.find(*id)?;
        if loc.path.len() != 1 {
            return None;
        }
        if let Some(have) = spread
            && have != loc.spread
        {
            return None;
        }
        spread = Some(loc.spread);
        let b = cx.doc.item(*id)?.bounds();
        if !(b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite()) {
            return None;
        }
        rect = Some(match rect {
            Some(r) => r.union(b),
            None => b,
        });
    }
    Some((spread?, rect?))
}

fn edge_toward(pointer: f64, center: f64) -> [bool; 3] {
    if pointer >= center { [false, false, true] } else { [true, false, false] }
}

impl Tool for XformTool {
    fn id(&self) -> &'static str {
        self.id
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                self.guides.clear();
                self.start_rotation = None;
                let Some(b) = cx.selection_bounds() else { return vec![] };
                if self.id == "rotate" {
                    self.start_rotation = crate::snap::reference_rotation(cx);
                }
                self.drag = Some((b.center(), ev.pos));
                vec![Action::Begin(match self.id {
                    "rotate" => "Rotate".into(),
                    "scale" => "Scale".into(),
                    _ => "Shear".into(),
                })]
            }
            PointerKind::Drag => {
                let Some((c, start)) = self.drag else { return vec![] };
                let (v0, v1) = (start - c, ev.pos - c);
                self.guides.clear();
                match self.id {
                    "rotate" => {
                        let mut a = (v1.atan2() - v0.atan2()).to_degrees();
                        if ev.mods.shift {
                            // 45 degree steps. The rotation pass does not run while Shift is held.
                            a = (a / 45.0).round() * 45.0;
                        } else if let Some(start) = self.start_rotation {
                            let raw = -a;
                            if let Some(resulting) = crate::snap::folded_rotation(start + raw) {
                                let spread =
                                    cx.selection.items.first().and_then(|id| cx.doc.find(*id)).map(|loc| loc.spread).unwrap_or(SpreadRef::Doc(0));
                                let exclude = cx.selection.items.clone();
                                // The rect is unused by the angle pass. It only has to be finite.
                                let hit = crate::snap::snap(
                                    cx,
                                    SnapRequest {
                                        spread,
                                        gesture: Gesture::Rotate,
                                        rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                                        x_edges: [false, false, false],
                                        y_edges: [false, false, false],
                                        exclude: &exclude,
                                        exclude_guides: &[],
                                        copying: false,
                                        lengths: [None, None],
                                        angle: Some(resulting),
                                        radius: (ev.pos - c).hypot(),
                                        pointer: cx.layout.to_spread(spread, ev.pos),
                                    },
                                );
                                if let Some(snapped) = hit.angle
                                    && let Some(delta) = crate::snap::rotation_command_delta(start, snapped, raw)
                                {
                                    a = -delta;
                                }
                                self.guides = hit.guides;
                            }
                        }
                        vec![Action::Preview("transform.rotate".into(), json!({"angle": -a}))]
                    }
                    "scale" => {
                        let (sx0, sy0) = (v1.x / v0.x.abs().max(1e-6) * v0.x.signum(), v1.y / v0.y.abs().max(1e-6) * v0.y.signum());
                        let x_drives = !ev.mods.shift || sx0.abs() >= sy0.abs();
                        let (mut sx, mut sy) = (sx0, sy0);
                        if ev.mods.shift {
                            let s = sx.abs().max(sy.abs());
                            sx = s;
                            sy = s;
                        }
                        if cx.snap.any()
                            && sx.is_finite()
                            && sy.is_finite()
                            && let Some((spread, from)) = selection_spread_bounds(cx)
                        {
                            let (sign_x, sign_y) = (if sx < 0.0 { -1.0 } else { 1.0 }, if sy < 0.0 { -1.0 } else { 1.0 });
                            let (w0, h0) = (from.width().abs(), from.height().abs());
                            if w0 > 1e-9 && h0 > 1e-9 {
                                let center = from.center();
                                let (mut w, mut h) = (w0 * sx.abs(), h0 * sy.abs());
                                let pre = Rect::new(center.x - w / 2.0, center.y - h / 2.0, center.x + w / 2.0, center.y + h / 2.0);
                                let pointer = cx.layout.to_spread(spread, ev.pos);
                                let x_edges = if x_drives { edge_toward(pointer.x, center.x) } else { [false, false, false] };
                                let y_edges = if x_drives && ev.mods.shift { [false, false, false] } else { edge_toward(pointer.y, center.y) };
                                let exclude = cx.selection.items.clone();
                                let (pad_x, pad_y) = scale_stroke_pad(cx);
                                let mut hit = crate::snap::snap(
                                    cx,
                                    SnapRequest {
                                        spread,
                                        gesture: Gesture::Resize,
                                        rect: pre,
                                        x_edges,
                                        y_edges,
                                        exclude: &exclude,
                                        exclude_guides: &[],
                                        copying: false,
                                        lengths: [length_on(x_edges, w, pad_x), length_on(y_edges, h, pad_y)],
                                        angle: None,
                                        radius: 0.0,
                                        pointer,
                                    },
                                );
                                if let Some(diff) = hit.length_delta[0] {
                                    w += diff;
                                } else if x_edges[2] {
                                    w += 2.0 * hit.delta.x;
                                } else if x_edges[0] {
                                    w -= 2.0 * hit.delta.x;
                                }
                                if let Some(diff) = hit.length_delta[1] {
                                    h += diff;
                                } else if y_edges[2] {
                                    h += 2.0 * hit.delta.y;
                                } else if y_edges[0] {
                                    h -= 2.0 * hit.delta.y;
                                }
                                if ev.mods.shift {
                                    if x_drives {
                                        h = h0 * (w / w0);
                                    } else {
                                        w = w0 * (h / h0);
                                    }
                                }
                                if w.is_finite() && h.is_finite() {
                                    let committed = Rect::new(center.x - w / 2.0, center.y - h / 2.0, center.x + w / 2.0, center.y + h / 2.0);
                                    crate::snap::lay_dimension_guides(
                                        &mut hit.guides,
                                        cx.layout.xf(spread),
                                        pre,
                                        committed,
                                        x_edges,
                                        y_edges,
                                        hit.length_delta,
                                    );
                                    if w.abs() > 1e-9 && h.abs() > 1e-9 {
                                        sx = (w / w0) * sign_x;
                                        sy = (h / h0) * sign_y;
                                    }
                                }
                                self.guides = hit.guides;
                            }
                        }
                        vec![Action::Preview("transform.scale".into(), json!({"sx": sx, "sy": sy}))]
                    }
                    _ => {
                        let a = ((v1.x - v0.x) / v0.y.abs().max(20.0)).atan().to_degrees();
                        vec![Action::Preview("transform.shear".into(), json!({"angle": a}))]
                    }
                }
            }
            PointerKind::Up => {
                self.guides.clear();
                self.start_rotation = None;
                if self.drag.take().is_some() { vec![Action::Commit] } else { vec![] }
            }
            _ => vec![],
        }
    }
    fn overlays(&self, _cx: &ToolContext) -> Vec<Overlay> {
        self.guides.clone()
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        if self.id == "rotate" { Cursor::Rotate } else { Cursor::Crosshair }
    }
    fn busy(&self) -> bool {
        self.drag.is_some()
    }
}

fn length_on(edges: [bool; 3], size: f64, pad: f64) -> Option<f64> {
    if edges == [false, false, false] {
        return None;
    }
    let len = size.abs() + pad;
    len.is_finite().then_some(len)
}

fn scale_stroke_pad(cx: &ToolContext) -> (f64, f64) {
    if cx.selection.items.len() != 1 {
        return (0.0, 0.0);
    }
    let Some(id) = cx.selection.items.first() else { return (0.0, 0.0) };
    let Some(it) = cx.doc.item(*id) else { return (0.0, 0.0) };
    let path = it.bounds();
    let vis = it.visible_bounds();
    (positive_pad(vis.width() - path.width()), positive_pad(vis.height() - path.height()))
}

fn positive_pad(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

/// The spread and spread point under `p` when a placed raster shows there. Other graphic formats
/// still block an image behind them, but use attribute matching instead of pixel sampling.
fn image_under(cx: &ToolContext, p: Point) -> Option<(SpreadRef, Point)> {
    use designcraft_doc::{Content, Item};
    fn shows(d: &designcraft_doc::Document, it: &Item, outer: designcraft_geom::Affine, at: Point, depth: usize) -> Option<bool> {
        if it.hidden || depth > 64 {
            return None;
        }
        let xf = outer * it.xf;
        if let Content::Group { .. } = &it.content {
            // Only the shown children count: a multi-state object shows just its active state.
            let shown: Vec<&Item> = it.shown_children().map(|c| &**c).collect();
            return shown.into_iter().rev().find_map(|c| shows(d, c, xf, at, depth + 1));
        }
        let graphic = matches!(it.content, Content::Graphic(_));
        // Unfilled frames (a text frame over the photo) let the image show through.
        if !graphic && it.fill.is_none() {
            return None;
        }
        if !designcraft_doc::edit_hit(it, outer.inverse() * at, 0.0) {
            return None;
        }
        Some(match &it.content {
            Content::Graphic(g) => d.assets.get(&g.asset).is_some_and(|a| a.is_raster()),
            _ => false,
        })
    }
    let (sr, at) = cx.layout.spread_at(p)?;
    let sp = cx.doc.spread(sr)?;
    let mut items: Vec<&Item> = sp.items.iter().map(|i| &**i).collect();
    items.sort_by_key(|i| cx.doc.layer_rank(i.layer));
    let image = items
        .into_iter()
        .rev()
        .filter(|it| cx.doc.layer(it.layer).is_none_or(|l| l.visible))
        .find_map(|it| shows(cx.doc, it, designcraft_geom::Affine::IDENTITY, at, 0))?;
    image.then_some((sr, at))
}

#[derive(Default)]
pub struct EyedropperTool;

/// The palette tool only requests a preview. Adding the swatches is a separate command.
pub struct ColorThemeTool;

impl Tool for ColorThemeTool {
    fn id(&self) -> &'static str {
        "colorTheme"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        if ev.kind != PointerKind::Down {
            return vec![];
        }
        let Some((_, id)) = cx.hit(ev.pos) else { return vec![] };
        if cx.doc.item(id).and_then(|it| it.graphic()).is_none() {
            return vec![];
        }
        vec![Action::Dialog("colorTheme".into(), json!({"id": id.0}))]
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Eyedropper
    }
}

impl Tool for EyedropperTool {
    fn id(&self) -> &'static str {
        "eyedropper"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        if ev.kind != PointerKind::Down {
            return vec![];
        }
        let selected = !cx.selection.items.is_empty() || cx.selection.text.is_some() || cx.selection.cells.is_some();
        let hit = cx.hit(ev.pos);
        // A placed image under the pointer is sampled (its layer may be locked: sampling changes nothing).
        if selected && let Some((sr, at)) = image_under(cx, ev.pos) {
            // Where the image shows no opaque pixel, the engine matches the clicked object instead.
            let mut p = json!({"at": [at.x, at.y], "spread": sr});
            if let Some((_, id)) = hit {
                p["matchFrom"] = json!(id.0);
            }
            // To the fill or stroke in front in the toolbox, as in InDesign.
            if cx.stroke_in_front {
                p["target"] = json!("stroke");
            }
            return vec![Action::Exec("color.sample".into(), p)];
        }
        let Some((sr, id)) = hit else { return vec![] };
        if !selected {
            return vec![Action::Exec("selection.set".into(), json!({"ids": [id.0]}))];
        }
        // With text selected, clicking text picks up its formatting (InDesign's Eyedropper).
        if cx.selection.text.is_some() && cx.doc.item(id).is_some_and(|it| it.is_text_frame()) {
            let p = cx.layout.to_spread(sr, ev.pos);
            return vec![Action::Exec("type.pickUpFormat".into(), json!({"frame": id.0, "point": [p.x, p.y]}))];
        }
        vec![Action::Exec("object.matchAttributes".into(), json!({"from": id.0}))]
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Eyedropper
    }
}
