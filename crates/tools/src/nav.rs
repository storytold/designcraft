//! Hand (H) and Zoom (Z) tools.

use designcraft_geom::Point;
use serde_json::json;

use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext};

/// Hand tool: drag to pan. Alt-press starts Power Zoom: the view zooms out to the spread, the
/// pointer aims a rectangle of the previous view, and releasing zooms back in there.
#[derive(Default)]
pub struct HandTool {
    last: Option<Point>,
    power: bool,
}

impl Tool for HandTool {
    fn id(&self) -> &'static str {
        "hand"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down if ev.mods.alt => {
                self.power = true;
                self.last = Some(ev.pos);
                vec![Action::View(json!({"powerZoom": "start", "at": [ev.pos.x, ev.pos.y]}))]
            }
            PointerKind::Down => {
                self.last = Some(ev.pos);
                vec![]
            }
            PointerKind::Drag if self.power => vec![Action::View(json!({"powerZoom": "move", "at": [ev.pos.x, ev.pos.y]}))],
            PointerKind::Up if self.power => {
                self.power = false;
                self.last = None;
                vec![Action::View(json!({"powerZoom": "end", "at": [ev.pos.x, ev.pos.y]}))]
            }
            PointerKind::Drag => {
                // Canvas positions shift as we pan, so convert the delta to screen pixels once.
                let Some(l) = self.last else { return vec![] };
                let d = (ev.pos - l) * cx.zoom;
                vec![Action::View(json!({"pan": [d.x, d.y]}))]
            }
            PointerKind::Up => {
                self.last = None;
                vec![]
            }
            _ => vec![],
        }
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        if self.last.is_some() { Cursor::HandGrab } else { Cursor::Hand }
    }
    fn busy(&self) -> bool {
        self.last.is_some()
    }
}

/// Zoom tool: click zooms in (Alt: out); dragging left or right zooms out or in continuously
/// around the press point (scrubby zoom).
#[derive(Default)]
pub struct ZoomTool {
    /// Press point (canvas), zoom at the press, whether it has been dragged.
    press: Option<(Point, f64, bool)>,
}

/// Screen pixels of horizontal drag per e-fold of zoom.
const SCRUB_PX: f64 = 150.0;

impl Tool for ZoomTool {
    fn id(&self) -> &'static str {
        "zoom"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                self.press = Some((ev.pos, cx.zoom, false));
                vec![]
            }
            PointerKind::Drag => {
                let Some((start, z0, _)) = self.press else { return vec![] };
                // The press point stays put on screen, so this is the on-screen distance.
                let dx = (ev.pos.x - start.x) * cx.zoom;
                if dx.abs() < 3.0 && self.press.is_some_and(|p| !p.2) {
                    return vec![];
                }
                self.press = Some((start, z0, true));
                let want = (z0 * (dx / SCRUB_PX).exp()).clamp(0.05, 40.0);
                vec![Action::View(json!({"zoomAt": [start.x, start.y], "factor": want / cx.zoom.max(1e-9)}))]
            }
            PointerKind::Up => {
                let dragged = self.press.take().is_some_and(|p| p.2);
                if dragged {
                    return vec![];
                }
                let f = if ev.mods.alt { 0.5 } else { 2.0 };
                vec![Action::View(json!({"zoomAt": [ev.pos.x, ev.pos.y], "factor": f}))]
            }
            _ => vec![],
        }
    }
    fn busy(&self) -> bool {
        self.press.is_some_and(|p| p.2)
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, m: Mods) -> Cursor {
        if m.alt { Cursor::ZoomOut } else { Cursor::ZoomIn }
    }
}

/// The loaded place cursor: click places at actual size, drag draws the frame, click on an empty
/// frame places into it.
#[derive(Default)]
pub struct PlaceGun {
    start: Option<Point>,
    cur: Point,
}

impl Tool for PlaceGun {
    fn id(&self) -> &'static str {
        "placeGun"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                self.start = Some(ev.pos);
                self.cur = ev.pos;
                vec![]
            }
            PointerKind::Drag => {
                self.cur = ev.pos;
                vec![]
            }
            PointerKind::Up => {
                let Some(a) = self.start.take() else { return vec![] };
                let Some((sr, sa)) = cx.layout.spread_at(a) else { return vec![] };
                let b = cx.layout.to_spread(sr, ev.pos);
                if (ev.pos - a).hypot() < cx.tol(3.0)
                    && let Some((_, id)) = cx.hit(a)
                    && cx
                        .doc
                        .item(id)
                        .is_some_and(|i| matches!(i.content, designcraft_doc::Content::Unassigned | designcraft_doc::Content::Graphic(_)))
                {
                    return vec![Action::Exec("place.drop".into(), json!({"frame": id.0}))];
                }
                let r = designcraft_geom::Rect::from_points(sa, b);
                vec![Action::Exec(
                    "place.drop".into(),
                    json!({"spread": crate::spread_json(sr), "x": sa.x, "y": sa.y, "rect": [r.x0, r.y0, r.x1, r.y1]}),
                )]
            }
            _ => vec![],
        }
    }
    fn overlays(&self, _cx: &ToolContext) -> Vec<crate::Overlay> {
        match self.start {
            Some(a) => vec![crate::Overlay::Marquee(designcraft_geom::Rect::from_points(a, self.cur))],
            None => vec![],
        }
    }
    fn key(&mut self, _cx: &ToolContext, key: crate::ToolKey, _mods: Mods) -> Vec<Action> {
        match key {
            // Escape unloads the place cursor.
            crate::ToolKey::Escape => vec![Action::SwitchTool("selection".into())],
            _ => vec![],
        }
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::LoadedGraphic
    }
}

/// Page tool (Shift+P): click a page to select it; drag one of its handles to preview its liquid
/// page rule (the page springs back on release), Alt-drag to resize it, Shift to keep its
/// proportions; double-click a page for exact sizes.
#[derive(Default)]
pub struct PageTool {
    drag: Option<PageDrag>,
    /// The pin or lock under the pointer (for its tooltip).
    hover: Option<(LiquidControl, Point)>,
}

/// An object's liquid control on an object-based page: a side's pin (top, right, bottom, left) or
/// the width/height lock.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LiquidControl {
    Pin(usize),
    Width,
    Height,
}

impl LiquidControl {
    /// The `liquid.object` parameter it toggles.
    fn key(self) -> &'static str {
        match self {
            LiquidControl::Pin(0) => "pinTop",
            LiquidControl::Pin(1) => "pinRight",
            LiquidControl::Pin(2) => "pinBottom",
            LiquidControl::Pin(_) => "pinLeft",
            LiquidControl::Width => "resizeWidth",
            LiquidControl::Height => "resizeHeight",
        }
    }

    /// InDesign's tooltip for the control in its current state.
    fn tip(self, l: &designcraft_doc::ObjectLiquid) -> &'static str {
        match self {
            LiquidControl::Pin(i) if [l.pin_top, l.pin_right, l.pin_bottom, l.pin_left][i.min(3)] => {
                "Object is pinned relative to the page edge. Click to release."
            }
            LiquidControl::Pin(_) => "Click to pin the object relative to the page edge.",
            LiquidControl::Width if l.resize_width => "The object changes width as the page changes width. Click to lock the object width.",
            LiquidControl::Width => "The object's width is locked. Click to let the object change width as the page changes width.",
            LiquidControl::Height if l.resize_height => "The object changes height as the page changes height. Click to lock the object height.",
            LiquidControl::Height => "The object's height is locked. Click to let the object change height as the page changes height.",
        }
    }

    fn value(self, l: &designcraft_doc::ObjectLiquid) -> bool {
        match self {
            LiquidControl::Pin(0) => l.pin_top,
            LiquidControl::Pin(1) => l.pin_right,
            LiquidControl::Pin(2) => l.pin_bottom,
            LiquidControl::Pin(_) => l.pin_left,
            LiquidControl::Width => l.resize_width,
            LiquidControl::Height => l.resize_height,
        }
    }
}

/// Screen pixels between an object's edge and its pin (the canvas draws them there too).
pub const PIN_OFFSET_PX: f64 = 14.0;
/// Screen pixels between an object's centre and its width/height locks.
pub const LOCK_OFFSET_PX: f64 = 10.0;

/// Where an object's liquid controls sit (canvas): pins outside each edge's middle, the width lock
/// right of the centre, the height lock above it.
fn liquid_control_points(cx: &ToolContext, r: designcraft_geom::Rect) -> [(LiquidControl, Point); 6] {
    let (c, o, k) = (r.center(), cx.tol(PIN_OFFSET_PX), cx.tol(LOCK_OFFSET_PX));
    [
        (LiquidControl::Pin(0), Point::new(c.x, r.y0 - o)),
        (LiquidControl::Pin(1), Point::new(r.x1 + o, c.y)),
        (LiquidControl::Pin(2), Point::new(c.x, r.y1 + o)),
        (LiquidControl::Pin(3), Point::new(r.x0 - o, c.y)),
        (LiquidControl::Width, Point::new(c.x + k, c.y)),
        (LiquidControl::Height, Point::new(c.x, c.y - k)),
    ]
}

/// A page handle being dragged.
#[derive(Clone, Copy, Debug)]
struct PageDrag {
    /// Absolute page index.
    page: usize,
    /// Reference-point index of the handle (0–8, row-major; 4 is the centre and has none).
    handle: usize,
    start: Point,
    cur: Point,
    /// The page's size when the drag began.
    size: (f64, f64),
    alt: bool,
}

/// The eight handles of a page rect: (reference-point index, canvas point).
fn page_handles(r: designcraft_geom::Rect) -> impl Iterator<Item = (usize, Point)> {
    (0..9).filter(|i| *i != 4).map(move |i| (i, Point::new(r.x0 + r.width() * (i % 3) as f64 / 2.0, r.y0 + r.height() * (i / 3) as f64 / 2.0)))
}

/// The liquid page rule as the Page tool tags a page.
pub fn rule_label(rule: designcraft_doc::LiquidRule) -> &'static str {
    use designcraft_doc::LiquidRule as R;
    match rule {
        R::Off => "Off",
        R::Scale => "Scale",
        R::ReCenter => "Re-center",
        R::ObjectBased => "Object-Based",
        R::GuideBased => "Guide-Based",
        R::ControlledByParent => "Controlled by Parent",
    }
}

/// Whether page `abs` follows the object-based rule (its own, or its parent page's).
fn object_based(doc: &designcraft_doc::Document, abs: usize) -> bool {
    use designcraft_doc::LiquidRule as R;
    let Some((si, pi)) = doc.page_loc(abs) else { return false };
    match doc.spreads[si].pages[pi].liquid {
        R::ObjectBased => true,
        R::ControlledByParent => {
            doc.parent_page_for(abs).and_then(|(ppi, ppg)| doc.parents.get(ppi)?.pages.get(ppg).map(|p| p.liquid == R::ObjectBased)).unwrap_or(false)
        }
        _ => false,
    }
}

impl PageTool {
    /// The selected object's liquid settings, canvas bounds and page bounds when the Page tool
    /// shows its pins: one object selected on the selected page, whose rule is object-based.
    fn liquid_target(
        cx: &ToolContext,
    ) -> Option<(designcraft_doc::ItemId, designcraft_doc::ObjectLiquid, designcraft_geom::Rect, designcraft_geom::Rect)> {
        let page = cx.selection.page?;
        let [id] = cx.selection.items.as_slice() else { return None };
        let (si, _) = cx.doc.page_loc(page)?;
        if !object_based(cx.doc, page) || cx.doc.page_of_item(*id) != Some(page) {
            return None;
        }
        let it = cx.doc.item(*id)?;
        let l = it.liquid.unwrap_or(designcraft_doc::ObjectLiquid { resize_width: true, resize_height: true, ..Default::default() });
        let r = cx.layout.xf(designcraft_doc::SpreadRef::Doc(si)).transform_rect_bbox(it.bounds());
        Some((*id, l, r, cx.layout.page_rect(cx.doc, page)?))
    }

    /// The selected object's pin or lock under `p`.
    fn control_at(cx: &ToolContext, p: Point) -> Option<(designcraft_doc::ItemId, designcraft_doc::ObjectLiquid, LiquidControl)> {
        let (id, l, r, _) = Self::liquid_target(cx)?;
        liquid_control_points(cx, r).into_iter().find(|(_, q)| (q.x - p.x).hypot(q.y - p.y) <= cx.tol(6.0)).map(|(c, _)| (id, l, c))
    }

    /// The selected page's handle under `p`.
    fn handle_at(&self, cx: &ToolContext, p: Point) -> Option<(usize, usize)> {
        let page = cx.selection.page?;
        let r = cx.layout.page_rect(cx.doc, page)?;
        page_handles(r).find(|(_, h)| (h.x - p.x).abs() <= cx.tol(5.0) && (h.y - p.y).abs() <= cx.tol(5.0)).map(|(i, _)| (page, i))
    }

    /// The new page size for the drag at `p`: the handle's edges follow the pointer (Shift: in
    /// proportion).
    fn size_at(cx: &ToolContext, d: &PageDrag, p: Point, shift: bool) -> (f64, f64) {
        let si = cx.doc.page_loc(d.page).map_or(0, |l| l.0);
        let v = cx.layout.delta_to_spread(designcraft_doc::SpreadRef::Doc(si), p - d.start);
        let (col, row) = (d.handle % 3, d.handle / 3);
        let dx = match col {
            0 => -v.x,
            2 => v.x,
            _ => 0.0,
        };
        let dy = match row {
            0 => -v.y,
            2 => v.y,
            _ => 0.0,
        };
        let (w0, h0) = d.size;
        let (mut w, mut h) = ((w0 + dx).max(1.0), (h0 + dy).max(1.0));
        if shift {
            // The axis being dragged (the larger change at a corner) sets the scale.
            let k = match (col == 1, row == 1) {
                (true, _) => h / h0,
                (_, true) => w / w0,
                _ if (w / w0 - 1.0).abs() >= (h / h0 - 1.0).abs() => w / w0,
                _ => h / h0,
            };
            (w, h) = ((w0 * k).max(1.0), (h0 * k).max(1.0));
        }
        (w, h)
    }
}

impl Tool for PageTool {
    fn id(&self) -> &'static str {
        "page"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                if let Some((page, handle)) = self.handle_at(cx, ev.pos) {
                    let Some((si, pi)) = cx.doc.page_loc(page) else { return vec![] };
                    let pg = &cx.doc.spreads[si].pages[pi];
                    self.drag = Some(PageDrag { page, handle, start: ev.pos, cur: ev.pos, size: (pg.width, pg.height), alt: ev.mods.alt });
                    return vec![Action::Begin("Resize Page".into())];
                }
                // An object-based page's object: a click on a pin or lock toggles it.
                if let Some((id, l, c)) = Self::control_at(cx, ev.pos) {
                    return vec![Action::Exec("liquid.object".into(), json!({"ids": [id.0], c.key(): !c.value(&l)}))];
                }
                // A click selects the page under the pointer (none on the pasteboard), and on an
                // object-based page the object clicked too.
                let page = (0..cx.doc.page_count()).find(|p| cx.layout.page_rect(cx.doc, *p).is_some_and(|r| r.contains(ev.pos)));
                let object = page
                    .filter(|p| object_based(cx.doc, *p))
                    .and_then(|p| cx.hit(ev.pos).map(|(_, id)| id).filter(|id| cx.doc.page_of_item(*id) == Some(p)));
                let ids: Vec<u64> = object.map(|id| id.0).into_iter().collect();
                if page == cx.selection.page && cx.selection.items.iter().map(|i| i.0).eq(ids.iter().copied()) {
                    return vec![];
                }
                vec![Action::Exec("layout.selectPage".into(), json!({"page": page.map(|p| p + 1), "ids": ids}))]
            }
            PointerKind::Drag => {
                let Some(d) = self.drag.as_mut() else { return vec![] };
                d.cur = ev.pos;
                d.alt |= ev.mods.alt;
                let d = *d;
                let (w, h) = Self::size_at(cx, &d, ev.pos, ev.mods.shift);
                vec![Action::Preview("layout.pageSize".into(), json!({"pages": [d.page + 1], "width": w, "height": h, "anchor": 8 - d.handle}))]
            }
            PointerKind::Up => match self.drag.take() {
                // Alt-drag resizes; a plain drag previews the liquid rule and springs back.
                Some(d) if d.alt || ev.mods.alt => vec![Action::Commit],
                Some(_) => vec![Action::Cancel],
                None => vec![],
            },
            PointerKind::DoubleClick => {
                // Exact sizes: the Page Size dialog for the page.
                let Some(page) = (0..cx.doc.page_count()).find(|p| cx.layout.page_rect(cx.doc, *p).is_some_and(|r| r.contains(ev.pos))) else {
                    return vec![];
                };
                let Some((si, pi)) = cx.doc.page_loc(page) else { return vec![] };
                let pg = &cx.doc.spreads[si].pages[pi];
                vec![Action::Dialog("cmd:layout.pageSize".into(), json!({"pages": [page + 1], "width": pg.width, "height": pg.height}))]
            }
            PointerKind::Move => {
                self.hover = Self::control_at(cx, ev.pos).map(|(_, _, c)| (c, ev.pos));
                vec![]
            }
        }
    }
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let Some(page) = cx.selection.page else { return vec![] };
        let Some(r) = cx.layout.page_rect(cx.doc, page) else { return vec![] };
        let Some((si, pi)) = cx.doc.page_loc(page) else { return vec![] };
        let pg = &cx.doc.spreads[si].pages[pi];
        let mut v = vec![Overlay::PageHandles(r), Overlay::Tag { p: Point::new(r.x0, r.y0 - cx.tol(6.0)), text: rule_label(pg.liquid).into() }];
        if let Some((_, l, object, page)) = Self::liquid_target(cx) {
            v.push(Overlay::LiquidPins {
                object,
                page,
                pins: [l.pin_top, l.pin_right, l.pin_bottom, l.pin_left],
                resize: [l.resize_width, l.resize_height],
            });
            if let Some((c, p)) = self.hover {
                v.push(Overlay::Tip { p, text: c.tip(&l).into() });
            }
        }
        if let Some(d) = self.drag {
            v.push(Overlay::Measure {
                p: d.cur,
                text: format!(
                    "W: {}\nH: {}",
                    designcraft_geom::format_measure(pg.width, cx.unit),
                    designcraft_geom::format_measure(pg.height, cx.unit)
                ),
            });
        }
        v
    }
    fn cursor(&self, cx: &ToolContext, p: Point, _m: Mods) -> Cursor {
        if self.drag.is_none() && Self::control_at(cx, p).is_some() {
            return Cursor::Hand;
        }
        let handle = self.drag.map(|d| d.handle).or_else(|| self.handle_at(cx, p).map(|(_, h)| h));
        match handle {
            Some(1 | 7) => Cursor::ResizeV,
            Some(3 | 5) => Cursor::ResizeH,
            Some(0 | 8) => Cursor::ResizeNwSe,
            Some(_) => Cursor::ResizeNeSw,
            None => Cursor::Arrow,
        }
    }
    fn busy(&self) -> bool {
        self.drag.is_some()
    }
}

/// Type on a Path tool (Shift+T): click a path to put type on it; typing goes there.
#[derive(Default)]
pub struct PathTypeTool;

impl Tool for PathTypeTool {
    fn id(&self) -> &'static str {
        "typeOnPath"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        if ev.kind != PointerKind::Up {
            return vec![];
        }
        let Some((_, id)) = cx.hit(ev.pos) else { return vec![] };
        let Some(it) = cx.doc.item(id) else { return vec![] };
        match &it.content {
            designcraft_doc::Content::Unassigned => vec![Action::Exec("type.onPath".into(), json!({"id": id.0})), Action::SwitchTool("type".into())],
            _ => vec![],
        }
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Text
    }
}

/// Gap tool (U): drag the space between objects (or between an object and the page edge); the
/// objects on both sides resize so the gap moves.
#[derive(Default)]
pub struct GapTool {
    /// Spread, its spread → canvas transform and the press point in spread coordinates.
    start: Option<(designcraft_doc::SpreadRef, designcraft_geom::Affine, Point)>,
    active: bool,
}

impl Tool for GapTool {
    fn id(&self) -> &'static str {
        "gap"
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                if let Some((sr, p)) = cx.layout.spread_at(ev.pos) {
                    self.start = Some((sr, cx.layout.xf(sr), p));
                }
                self.active = false;
                vec![]
            }
            PointerKind::Drag => {
                let Some((sr, xf, p)) = self.start else { return vec![] };
                let d = (xf.inverse() * ev.pos) - p;
                let mut out = vec![];
                if !self.active {
                    if d.hypot() < cx.tol(2.0) {
                        return vec![];
                    }
                    self.active = true;
                    out.push(Action::Begin("Move Gap".into()));
                }
                out.push(Action::Preview(
                    "gap.move".into(),
                    serde_json::json!({"spread": crate::spread_json(sr), "at": [p.x, p.y], "dx": d.x, "dy": d.y}),
                ));
                out
            }
            PointerKind::Up => {
                self.start = None;
                if std::mem::take(&mut self.active) { vec![Action::Commit] } else { vec![] }
            }
            _ => vec![],
        }
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Crosshair
    }

    fn busy(&self) -> bool {
        self.active
    }
}

/// Content Collector (B): click objects to put copies on the conveyor. Content Placer: click to
/// place the next collected object there.
pub struct ConveyorTool {
    placer: bool,
}

impl ConveyorTool {
    pub fn new(placer: bool) -> Self {
        Self { placer }
    }
}

impl Tool for ConveyorTool {
    fn id(&self) -> &'static str {
        if self.placer { "contentPlacer" } else { "contentCollector" }
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        if ev.kind != PointerKind::Down {
            return vec![];
        }
        if self.placer {
            let Some((sr, p)) = cx.layout.spread_at(ev.pos) else { return vec![] };
            return vec![Action::Exec("conveyor.place".into(), json!({"spread": crate::spread_json(sr), "x": p.x, "y": p.y, "keep": ev.mods.alt}))];
        }
        match cx.hit(ev.pos) {
            Some((_, id)) => vec![Action::Exec("conveyor.collect".into(), json!({"ids": [id.0]}))],
            None => vec![],
        }
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Crosshair
    }
}
