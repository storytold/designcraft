//! Page items: frames (text / graphic / unassigned), paths, lines and groups.

use std::sync::Arc;

use designcraft_color::BlendMode;
use designcraft_geom::corners::CornerOptions;
use designcraft_geom::{Affine, PathData, Point, Rect};
use serde::{Deserialize, Serialize};

use crate::ids::{AssetId, ItemId, LayerId, StoryId};

mod content;
mod effects;
mod graphic;
mod paint;
mod text_frame;

pub use content::{Content, Shape};
pub use effects::{
    Bevel, DirectionalFeather, DropShadow, Effects, FeatherCorners, FeatherOptions, GlowTechnique, GradientFeather, InnerGlow, InnerShadow,
    OuterGlow, Satin,
};
pub use graphic::{
    ButtonAction, ClippingKind, ClippingPath, ExportOptions, FieldKind, Fitting, FormField, GRAPHIC_LAYER_RENDERING_WARNING, Graphic,
    GraphicLayerOptions, LiveCaption, MAX_CLIP_ANCHORS, MediaOptions, ObjectLiquid, media_kind, media_mime,
};
pub use paint::{
    Arrowhead, Cap, Fill, Join, MAX_DASH, MAX_DASH_ENTRIES, MAX_DASH_PERIODS, MIN_DASH, Stroke, StrokeAlign, StrokeStyleDef, StrokeType,
    path_length_bound,
};
pub use text_frame::{
    AutoSize, ColumnsKind, ContourType, FirstBaseline, FrameBaselineGrid, FrameGridRelative, PathAlign, PathType, TextFrame, TextFrameOptions,
    TextWrap, VerticalJustification, WrapMode, WrapSide, vertical_text_xf,
};

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: ItemId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub layer: LayerId,
    #[serde(default)]
    pub shape: Shape,
    /// Geometry in the item's inner coordinate space.
    pub path: PathData,
    /// Inner space → spread space.
    #[serde(default = "identity")]
    pub xf: Affine,
    #[serde(default)]
    pub fill: Fill,
    #[serde(default = "Stroke::none")]
    pub stroke: Stroke,
    #[serde(default)]
    pub corners: CornerOptions,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default)]
    pub effects: Effects,
    #[serde(default)]
    pub wrap: TextWrap,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub nonprinting: bool,
    #[serde(default)]
    pub object_style: String,
    #[serde(default)]
    pub content: Content,
    /// For items on a document page that override a parent-page item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<ItemId>,
    /// Script Label (for scripts and agents).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Alternative text (Object Export Options) for tagged PDF and EPUB.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt_text: String,
    /// Liquid Layout (object-based pages): pins and resize permissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquid: Option<ObjectLiquid>,
    /// Object Export Options (beyond alt text).
    #[serde(default, skip_serializing_if = "ExportOptions::is_default")]
    pub export_options: ExportOptions,
    /// Buttons and Forms: an interactive PDF form field drawn in this frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_field: Option<FormField>,
    /// Live caption: this text frame shows `template` filled from the source object's metadata,
    /// kept current as the source changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_caption: Option<LiveCaption>,
    /// Object › Object Layer Options: layers of a placed PDF that are hidden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pdf_hidden_layers: Vec<String>,
    /// Explicit placed-PDF visibility, including an all-on override with no hidden names.
    /// False with an empty hidden list retains the file's own settings (legacy behavior).
    #[serde(default, skip_serializing_if = "is_false")]
    pub pdf_layer_override: bool,
    /// Window › Interactive › Media: options of a placed video or sound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaOptions>,
    /// Buttons and Forms: what clicking this object does in an interactive PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<ButtonAction>,
    /// Object States: a group whose children are states (named here); only `active_state` shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub active_state: usize,
    /// XML tag (Tags panel); its content is the element's content in XML export/import.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub xml_tag: String,
    /// Groups: Isolate Blending (blend modes only within the group) and Knockout Group (the
    /// group's objects don't show through each other).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub isolate: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub knockout: bool,
    /// Flex Layout: this frame lays out the objects nested in it (see [`crate::flex`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex: Option<crate::flex::FlexLayout>,
}

fn identity() -> Affine {
    Affine::IDENTITY
}
fn one() -> f32 {
    1.0
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Item {
    pub fn has_pdf_layer_override(&self) -> bool {
        self.pdf_layer_override || !self.pdf_hidden_layers.is_empty()
    }

    pub fn new(id: ItemId, layer: LayerId, shape: Shape, path: PathData) -> Self {
        Item {
            id,
            name: String::new(),
            layer,
            shape,
            path,
            xf: Affine::IDENTITY,
            fill: Fill::none(),
            stroke: Stroke::none(),
            corners: CornerOptions::default(),
            opacity: 1.0,
            blend: BlendMode::default(),
            effects: Effects::default(),
            wrap: TextWrap::default(),
            locked: false,
            hidden: false,
            nonprinting: false,
            object_style: crate::styles::NO_OBJECT_STYLE.into(),
            content: Content::Unassigned,
            overrides: None,
            label: String::new(),
            alt_text: String::new(),
            xml_tag: String::new(),
            button: None,
            pdf_hidden_layers: Vec::new(),
            pdf_layer_override: false,
            live_caption: None,
            form_field: None,
            export_options: ExportOptions::default(),
            media: None,
            liquid: None,
            states: Vec::new(),
            active_state: 0,
            isolate: false,
            knockout: false,
            flex: None,
        }
    }

    pub fn is_text_frame(&self) -> bool {
        matches!(self.content, Content::Text(_))
    }
    pub fn text_frame(&self) -> Option<&TextFrame> {
        match &self.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }
    pub fn text_frame_mut(&mut self) -> Option<&mut TextFrame> {
        match &mut self.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }
    /// Does this item, or the graphic it holds, wrap text?
    pub fn has_wrap(&self) -> bool {
        self.wrap.mode != WrapMode::None || self.graphic().is_some_and(|g| g.wrap.mode != WrapMode::None)
    }
    pub fn graphic(&self) -> Option<&Graphic> {
        match &self.content {
            Content::Graphic(g) => Some(g),
            _ => None,
        }
    }
    pub fn children(&self) -> &[Arc<Item>] {
        match &self.content {
            Content::Group { items } => items,
            _ => &[],
        }
    }
    /// The children that show: all of them, or just the active state of a multi-state object.
    pub fn shown_children(&self) -> impl Iterator<Item = &Arc<Item>> {
        let only = (!self.states.is_empty()).then_some(self.active_state);
        self.children().iter().enumerate().filter(move |(i, _)| only.is_none_or(|a| a == *i)).map(|(_, c)| c)
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Arc<Item>>> {
        match &mut self.content {
            Content::Group { items } => Some(items),
            _ => None,
        }
    }

    /// Inner-space bounds of the path (the frame's "geometric bounds" before transform).
    pub fn inner_bounds(&self) -> Rect {
        self.path.bounds().unwrap_or(Rect::ZERO)
    }

    /// The box a gradient feather spans, in item space: the path's bounds, or a group's visible
    /// objects' (a group has no path of its own). Only the active states contribute, including in
    /// nested groups. Walk iteratively so deeply nested documents don't exhaust the call stack.
    pub fn feather_bounds(&self) -> Rect {
        if !self.is_group() {
            return self.inner_bounds();
        }
        let mut stack: Vec<_> = self.shown_children().map(|c| (c.as_ref(), c.xf)).collect();
        let mut bounds: Option<Rect> = None;
        while let Some((it, xf)) = stack.pop() {
            if it.hidden {
                continue;
            }
            if it.is_group() {
                stack.extend(it.shown_children().map(|c| (c.as_ref(), xf * c.xf)));
            } else {
                let b = xf.transform_rect_bbox(it.inner_bounds());
                bounds = Some(bounds.map_or(b, |have| have.union(b)));
            }
        }
        bounds.unwrap_or(Rect::ZERO)
    }

    /// Spread-space geometric bounds (axis-aligned box of the transformed path; groups: union).
    /// A group (its bounds are its children's). A frame holding pasted-into items is not.
    pub fn is_group(&self) -> bool {
        self.shape == Shape::Group && matches!(self.content, Content::Group { .. })
    }

    /// Does this object (or anything in it) involve transparency (opacity, a blend mode, effects,
    /// knockout)?
    pub fn involves_transparency(&self) -> bool {
        self.opacity < 1.0
            || self.blend != Default::default()
            || self.effects.any()
            || self.knockout
            || self.children().iter().any(|c| c.involves_transparency())
    }

    /// A frame with items pasted into it (Edit › Paste Into): drawn clipped to its path.
    pub fn has_nested_items(&self) -> bool {
        self.shape != Shape::Group && matches!(&self.content, Content::Group { items } if !items.is_empty())
    }

    pub fn bounds(&self) -> Rect {
        self.bounds_under(Affine::IDENTITY).unwrap_or(Rect::ZERO)
    }

    /// Bounds of the outline under `outer` (then this item's own transform). A group's are its
    /// children's outlines transformed by the full transform, which a rotated group's box of its
    /// children's boxes would overstate.
    fn bounds_under(&self, outer: Affine) -> Option<Rect> {
        let xf = outer * self.xf;
        if self.shape == Shape::Group
            && let Content::Group { items } = &self.content
        {
            return items.iter().filter_map(|c| c.bounds_under(xf)).reduce(|a, b| a.union(b));
        }
        self.path.transformed(xf).bounds()
    }

    /// Visible bounds including stroke weight (approximate for inside/outside alignment).
    pub fn visible_bounds(&self) -> Rect {
        let b = self.bounds();
        if self.stroke.is_none() {
            return b;
        }
        let w = match self.stroke.align {
            StrokeAlign::Center => self.stroke.weight / 2.0,
            StrokeAlign::Inside => 0.0,
            StrokeAlign::Outside => self.stroke.weight,
        };
        // Arrowheads on open paths.
        let w = if self.path.is_closed() { w } else { w.max(self.stroke.extent() - self.stroke.weight) };
        b.inflate(w, w)
    }

    /// The text frame's text area (inner space): inside the part of a visible stroke that lies
    /// within the frame, then the inset, on each edge.
    pub fn text_area(&self) -> Rect {
        let r = self.inner_bounds();
        let s = self.stroke_inside();
        let inset = self.text_frame().map(|t| t.options.inset).unwrap_or([0.0; 4]).map(|v| v + s);
        Rect::new(r.x0 + inset[1], r.y0 + inset[0], (r.x1 - inset[3]).max(r.x0 + inset[1]), (r.y1 - inset[2]).max(r.y0 + inset[0]))
    }

    /// How far a visible stroke reaches inside the path: half its weight centred, all of it inside,
    /// none outside.
    fn stroke_inside(&self) -> f64 {
        let w = self.stroke.weight;
        if self.stroke.is_none() || !w.is_finite() {
            return 0.0;
        }
        match self.stroke.align {
            StrokeAlign::Center => w / 2.0,
            StrokeAlign::Inside => w,
            StrokeAlign::Outside => 0.0,
        }
    }

    /// Spread-space centre.
    pub fn center(&self) -> Point {
        self.bounds().center()
    }

    /// The Layers-panel label: `<rectangle>`, the first words of a text frame, the placed file name…
    pub fn default_label(&self) -> &'static str {
        match (&self.content, self.shape) {
            // A frame with items pasted into it is named by its shape, as in InDesign.
            (Content::Group { .. }, Shape::Group) => "<group>",
            (Content::Text(_), _) => "<text frame>",
            (Content::Graphic(_), _) => "<image>",
            (_, Shape::Rectangle) => "<rectangle>",
            (_, Shape::Oval) => "<ellipse>",
            (_, Shape::Polygon) => "<polygon>",
            (_, Shape::GraphicLine) => "<line>",
            (_, Shape::Path) | (_, Shape::Group) => "<path>",
        }
    }

    /// Whether the item's own geometry (path, transform, a placed graphic's size and transform)
    /// is finite. JSON has no infinity or NaN, so a document holding one can't be saved.
    pub fn geometry_is_finite(&self) -> bool {
        let pt = |p: Point| p.x.is_finite() && p.y.is_finite();
        let affine = |a: &Affine| a.as_coeffs().iter().all(|c| c.is_finite());
        let path = self.path.subpaths.iter().flat_map(|s| &s.anchors).all(|a| pt(a.p) && pt(a.h_in) && pt(a.h_out));
        let graphic = match &self.content {
            Content::Graphic(g) => g.size.0.is_finite() && g.size.1.is_finite() && affine(&g.xf),
            _ => true,
        };
        path && affine(&self.xf) && graphic
    }

    /// Visit this item and descendants (pre-order).
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Item)) {
        f(self);
        for c in self.children() {
            c.walk(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::shapes;

    /// Shadows and glows saved before they had a blend mode read Normal, as they were drawn;
    /// new ones start as InDesign's: Multiply for shadows, Screen for the outer glow.
    #[test]
    fn effect_blend_modes_default_as_indesign_and_keep_old_documents() {
        let old: DropShadow =
            serde_json::from_str(r#"{"on": true, "color": "[Black]", "opacity": 0.75, "angle": 120, "distance": 7, "size": 5, "spread": 0}"#)
                .unwrap();
        assert_eq!(old.mode, BlendMode::Normal);
        let old: OuterGlow = serde_json::from_str(r#"{"on": true, "color": "[Black]", "opacity": 0.75, "size": 7, "spread": 0}"#).unwrap();
        assert_eq!(old.mode, BlendMode::Normal);
        assert_eq!(DropShadow::default().mode, BlendMode::Multiply);
        assert_eq!(InnerShadow::default().mode, BlendMode::Multiply);
        assert_eq!(OuterGlow::default().mode, BlendMode::Screen);
        let saved = serde_json::to_value(DropShadow { mode: BlendMode::Screen, ..Default::default() }).unwrap();
        assert_eq!(serde_json::from_value::<DropShadow>(saved).unwrap().mode, BlendMode::Screen, "round trip");
    }

    #[test]
    fn graphics_without_a_clipping_path_load_and_save_as_before() {
        let g = Graphic {
            layer_options: None,
            asset: AssetId(3),
            size: (40.0, 30.0),
            xf: Affine::translate((5.0, 5.0)),
            clip: None,
            auto_fit: Fitting::None,
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        };
        let v = serde_json::to_value(&g).unwrap();
        assert!(v.get("layerOptions").is_none(), "legacy graphics keep their existing representation");
        assert!(v.get("clip").is_none(), "{v}");
        assert_eq!(serde_json::from_value::<Graphic>(v.clone()).unwrap(), g);
        // A clip written with only its type and path gets the default options.
        let mut v = v;
        v["clip"] = serde_json::json!({"kind": "alphaChannel", "path": {"subpaths": []}});
        let c = serde_json::from_value::<Graphic>(v).unwrap().clip.unwrap();
        assert_eq!(*c, ClippingPath::new(ClippingKind::AlphaChannel, PathData::default()));
    }

    #[test]
    fn frame_baseline_grid_saves_as_start_then_increment() {
        // A file saved before Relative To existed: the grid starts from the top of the page and
        // saves back unchanged.
        let v = serde_json::json!({"baselineGrid": [6.0, 14.0]});
        let o = serde_json::from_value::<TextFrameOptions>(v).unwrap();
        assert_eq!(o.baseline_grid, Some(FrameBaselineGrid { start: 6.0, increment: 14.0, relative_to: FrameGridRelative::TopOfPage }));
        assert_eq!(serde_json::to_value(&o).unwrap()["baselineGrid"], serde_json::json!([6.0, 14.0]));
        for r in [FrameGridRelative::TopMargin, FrameGridRelative::TopOfFrame, FrameGridRelative::TopInset] {
            let o = TextFrameOptions {
                baseline_grid: Some(FrameBaselineGrid { relative_to: r, ..FrameBaselineGrid::new(6.0, 14.0) }),
                ..Default::default()
            };
            let v = serde_json::to_value(&o).unwrap();
            assert_eq!(v["baselineGrid"], serde_json::json!([6.0, 14.0, r.name()]));
            assert_eq!(serde_json::from_value::<TextFrameOptions>(v).unwrap().baseline_grid, o.baseline_grid);
        }
        for bad in [serde_json::json!([6.0]), serde_json::json!([6.0, 14.0, "frame", 1]), serde_json::json!([6.0, 14.0, "spine"])] {
            assert!(serde_json::from_value::<TextFrameOptions>(serde_json::json!({"baselineGrid": bad})).is_err());
        }
    }

    #[test]
    fn bounds_and_text_area() {
        let mut it = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 50.0)));
        it.xf = Affine::translate((10.0, 20.0));
        assert_eq!(it.bounds(), Rect::new(10.0, 20.0, 110.0, 70.0));
        it.content = Content::Text(TextFrame { story: StoryId(2), options: TextFrameOptions { inset: [5.0, 6.0, 7.0, 8.0], ..Default::default() } });
        assert_eq!(it.text_area(), Rect::new(6.0, 5.0, 92.0, 43.0));
        it.stroke = Stroke { weight: 4.0, ..Stroke::default() };
        assert_eq!(it.visible_bounds(), Rect::new(8.0, 18.0, 112.0, 72.0));
    }

    #[test]
    fn text_area_starts_inside_the_stroke() {
        let mut it = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 50.0)));
        it.content = Content::Text(TextFrame { story: StoryId(2), options: TextFrameOptions { inset: [5.0, 6.0, 7.0, 8.0], ..Default::default() } });
        let unstroked = Rect::new(6.0, 5.0, 92.0, 43.0);
        let stroked = |align: StrokeAlign, swatch: &str| {
            let mut it = it.clone();
            it.stroke = Stroke { weight: 4.0, align, swatch: swatch.into(), ..Stroke::default() };
            it.text_area()
        };
        let black = designcraft_color::swatch::BLACK;
        // The part of the stroke inside the frame, on every edge, then the inset.
        assert_eq!(stroked(StrokeAlign::Center, black), Rect::new(8.0, 7.0, 90.0, 41.0));
        assert_eq!(stroked(StrokeAlign::Inside, black), Rect::new(10.0, 9.0, 88.0, 39.0));
        assert_eq!(stroked(StrokeAlign::Outside, black), unstroked);
        // A stroke without a colour doesn't count.
        assert_eq!(stroked(StrokeAlign::Inside, designcraft_color::swatch::NONE), unstroked);
        // Nor does a weight that isn't a number.
        it.stroke = Stroke { weight: f64::NAN, ..Stroke::default() };
        assert_eq!(it.text_area(), unstroked);
        // A stroke wider than the frame leaves no room for text.
        it.stroke = Stroke { weight: 200.0, align: StrokeAlign::Inside, ..Stroke::default() };
        let a = it.text_area();
        assert!(a.width() == 0.0 && a.height() == 0.0, "{a:?}");
    }

    #[test]
    fn group_bounds_union_children() {
        let a = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        let b = Item::new(ItemId(2), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(20.0, 20.0, 30.0, 40.0)));
        let mut g = Item::new(ItemId(3), LayerId(1), Shape::Group, PathData::default());
        g.content = Content::Group { items: vec![Arc::new(a), Arc::new(b)] };
        assert_eq!(g.bounds(), Rect::new(0.0, 0.0, 30.0, 40.0));
        let mut n = 0;
        g.walk(&mut |_| n += 1);
        assert_eq!(n, 3);
    }

    #[test]
    fn rotated_group_bounds_follow_the_children_outlines() {
        let rects = [Rect::new(0.0, 0.0, 120.0, 20.0), Rect::new(10.0, 40.0, 200.0, 70.0)];
        let child_xf = Affine::rotate(-20f64.to_radians());
        let items = rects
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut c = Item::new(ItemId(i as u64 + 1), LayerId(1), Shape::Rectangle, shapes::rectangle(*r));
                c.xf = child_xf;
                Arc::new(c)
            })
            .collect();
        let mut g = Item::new(ItemId(9), LayerId(1), Shape::Group, PathData::default());
        g.content = Content::Group { items };
        g.xf = Affine::translate((50.0, 60.0)) * Affine::rotate(30f64.to_radians());
        let full = g.xf * child_xf;
        let corners = rects.iter().flat_map(|r| [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)]).map(|(x, y)| full * Point::new(x, y));
        let want = corners.fold(None::<Rect>, |acc, p| Some(acc.map_or(Rect::from_points(p, p), |r| r.union_pt(p)))).unwrap();
        let got = g.bounds();
        for (a, b) in [(got.x0, want.x0), (got.y0, want.y0), (got.x1, want.x1), (got.y1, want.y1)] {
            assert!((a - b).abs() < 1e-9, "{got:?} != {want:?}");
        }
        // The box of the children's rotated boxes is larger.
        let boxes = rects.iter().map(|r| g.xf.transform_rect_bbox(child_xf.transform_rect_bbox(*r))).reduce(|a, b| a.union(b)).unwrap();
        assert!(boxes.height() > got.height() + 1.0);
    }

    #[test]
    fn feather_bounds_follow_visible_states_in_nested_groups() {
        let a = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(10.0, 20.0, 30.0, 40.0)));
        let mut b = a.clone();
        b.id = ItemId(2);
        b.xf = Affine::translate((500.0, 500.0));
        let mut state = Item::new(ItemId(3), LayerId(1), Shape::Group, PathData::default());
        state.content = Content::Group { items: vec![Arc::new(a), Arc::new(b)] };
        state.states = vec!["Visible".into(), "Inactive".into()];
        assert_eq!(state.feather_bounds(), Rect::new(10.0, 20.0, 30.0, 40.0));
        state.active_state = 1;
        assert_eq!(state.feather_bounds(), Rect::new(510.0, 520.0, 530.0, 540.0));
        state.active_state = 0;
        state.xf = Affine::translate((5.0, 10.0)) * Affine::scale(2.0);
        let mut hidden = state.clone();
        hidden.id = ItemId(4);
        hidden.hidden = true;
        hidden.xf = Affine::translate((-1000.0, -1000.0));
        let mut outer = Item::new(ItemId(5), LayerId(1), Shape::Group, PathData::default());
        outer.content = Content::Group { items: vec![Arc::new(state), Arc::new(hidden)] };
        assert_eq!(outer.feather_bounds(), Rect::new(25.0, 50.0, 65.0, 90.0));
    }

    #[test]
    fn serde_roundtrip() {
        let mut it = Item::new(ItemId(1), LayerId(1), Shape::Oval, shapes::ellipse(Rect::new(0.0, 0.0, 10.0, 10.0)));
        it.fill = Fill::swatch("[Black]");
        it.content = Content::Text(TextFrame { story: StoryId(9), options: TextFrameOptions::default() });
        let s = serde_json::to_string(&it).unwrap();
        let back: Item = serde_json::from_str(&s).unwrap();
        assert_eq!(it, back);
    }
}
