use super::values::{finite, tint};
use super::{DirectionalFeather, DropShadow, Effects, El, GradientFeather, Importer, Stroke, names};

impl<'r> Importer<'r> {
    /// An item's object style chain: its applied style, the styles that one is based on, then
    /// `[None]` (cycle-safe).
    pub(super) fn object_style_chain(&self, e: &El) -> Vec<El> {
        let model = crate::xml::to_craft(e);
        let styles: Vec<_> = self.object_els.values().map(crate::xml::to_craft).collect();
        let values = craft_idml::model::Values::of(&model);
        let reference = values.get("AppliedObjectStyle");
        craft_idml::model::ObjectStyleStack::from_styles(&styles, reference, true).styles().iter().cloned().map(crate::xml::from_craft).collect()
    }

    /// Object style `id` and the styles it is based on, nearest first (cycle-safe, at most 16).
    /// The item's child element `name` laid over the same element of each style in its object style
    /// chain: attributes and nested elements merge one by one, nearer levels winning. IDML writes on an
    /// item only the values that differ from its style. Effects whose category the applied style turns
    /// off (`ObjectStyle*EffectsCategorySettings`) come from the item alone.
    pub(super) fn with_style(&self, e: &El, name: &str) -> Option<El> {
        let model = crate::xml::to_craft(e);
        let styles: Vec<_> = self.object_els.values().map(crate::xml::to_craft).collect();
        let values = craft_idml::model::Values::of(&model);
        let reference = values.get("AppliedObjectStyle");
        craft_idml::model::ObjectStyleStack::from_styles(&styles, reference, true).merged_child(&model, name).map(crate::xml::from_craft)
    }

    /// Reads the drop shadow, feather, gradient feather and directional feather of a
    /// `TransparencySetting` into `fx`.
    pub(super) fn effects_in(&mut self, t: &El, fx: &mut Effects) {
        let settings = craft_idml::model::TransparencySetting::from_el(&crate::xml::to_craft(t));
        self.effects_from_model(&settings, fx);
    }

    pub(super) fn effects_from_model(&mut self, settings: &craft_idml::model::TransparencySetting, fx: &mut Effects) {
        if let Some(ds) = settings.drop_shadow.as_ref().filter(|ds| ds.mode.as_deref() == Some("Drop")) {
            let x = ds.x_offset.unwrap_or(4.95);
            let y = ds.y_offset.unwrap_or(4.95);
            let def = DropShadow::default();
            fx.drop_shadow = DropShadow {
                on: true,
                color: ds.color.as_deref().map(|c| self.swatch_ref(c)).unwrap_or(def.color),
                opacity: ds.opacity.map(|o| (o / 100.0) as f32).unwrap_or(def.opacity),
                angle: names_angle(x, y),
                distance: x.hypot(y),
                size: ds.size.unwrap_or(def.size),
                spread: ds.spread.unwrap_or(def.spread),
                global_light: false,
                // Without the attribute (earlier DesignCraft exports) the shadow stays Normal, as it
                // rendered then; a new shadow's default is Multiply.
                mode: ds.blend_mode.as_deref().map_or(designcraft_color::BlendMode::Normal, names::blend_in),
            };
        }
        if let Some(f) = settings.feather.as_ref().filter(|f| f.mode.as_deref().is_some_and(|m| m != "None")) {
            fx.feather = f.width.unwrap_or(9.0);
            fx.feather_options.choke = finite(f.choke_amount).unwrap_or(0.0).clamp(0.0, 100.0);
            fx.feather_options.corners = match f.corner_type.as_deref() {
                Some("Sharp") => designcraft_doc::FeatherCorners::Sharp,
                Some("Rounded") => designcraft_doc::FeatherCorners::Rounded,
                _ => designcraft_doc::FeatherCorners::Diffused,
            };
        }
        if let Some(g) = settings.gradient_feather.as_ref().filter(|g| g.applied) {
            fx.gradient_feather = gradient_feather_in(g);
        }
        if let Some(df) = settings.directional_feather.as_ref().filter(|df| df.applied) {
            let w = |value| finite(value).unwrap_or(0.0).max(0.0);
            fx.directional_feather = DirectionalFeather { on: true, widths: df.widths.map(w) };
        }
    }

    /// An object style's drop shadow and feather. Each is read when the effects category turns it
    /// on (without category settings: when its setting is on), from the nearest style in the
    /// BasedOn chain that has the setting. `None` when the style sets neither.
    pub(super) fn style_effects(&mut self, e: &El) -> Option<Effects> {
        let model = crate::xml::to_craft(e);
        let mut styles: Vec<_> = self.object_els.values().map(crate::xml::to_craft).collect();
        let reference = craft_idml::model::Values::of(&model).self_id().map(str::to_owned);
        if !styles.iter().any(|style| craft_idml::model::Values::of(style).self_id() == reference.as_deref()) {
            styles.push(model.clone());
        }
        let t = craft_idml::model::ObjectStyleStack::from_styles(&styles, reference.as_deref(), false).enabled_shadow_feather()?;
        let t = crate::xml::from_craft(t);
        let mut fx = Effects::default();
        self.effects_in(&t, &mut fx);
        Some(fx)
    }

    pub(super) fn stroke_from(&mut self, e: &El, item: Option<&El>) -> Stroke {
        let model_elements = if item.is_some() {
            std::iter::once(e).chain(self.object_style_chain(e).iter()).map(crate::xml::to_craft).collect::<Vec<_>>()
        } else {
            vec![crate::xml::to_craft(e)]
        };
        let model_refs: Vec<_> = model_elements.iter().collect();
        let model = craft_idml::model::StrokeValues::from_elements(&model_refs);
        let mut s = Stroke::none();
        if let Some(c) = model.color.as_deref() {
            s.swatch = self.swatch_ref(c);
        }
        if let Some(w) = model.weight {
            s.weight = w;
        }
        if let Some(t) = model.tint {
            s.tint = tint(Some(t)).unwrap_or(1.0);
        }
        if let Some(t) = model.kind.as_deref() {
            s.kind = names::stroke_type_in(t);
        }
        if let Some(v) = model.alignment.as_deref() {
            s.align = names::stroke_align_in(v);
        }
        if let Some(v) = model.cap.as_deref() {
            s.cap = names::cap_in(v);
        }
        if let Some(v) = model.join.as_deref() {
            s.join = names::join_in(v);
        }
        if let Some(v) = model.miter_limit {
            s.miter_limit = v;
        }
        if let Some(v) = model.start.as_deref() {
            s.start = names::arrow_in(v);
        }
        if let Some(v) = model.end.as_deref() {
            s.end = names::arrow_in(v);
        }
        if let Some(v) = model.gap_color.as_deref() {
            s.gap_swatch = self.swatch_ref(v);
        }
        if let Some(t) = model.gap_tint {
            s.gap_tint = tint(Some(t)).unwrap_or(1.0);
        }
        s.overprint = model.overprint == Some(true);
        s.gap_overprint = model.gap_overprint == Some(true);
        s
    }
}

/// A `GradientFeatherSetting`. The model holds two opacity stops at the ends of the gradient: the
/// first and last `OpacityGradientStop` give them; with a linear vector, their `Location`s move its
/// ends.
fn gradient_feather_in(g: &craft_idml::model::GradientFeatherValues) -> GradientFeather {
    let def = GradientFeather::default();
    let angle = finite(g.angle).unwrap_or(0.0);
    let mut stops: Vec<(f64, f64)> =
        g.stops.iter().filter_map(|s| Some((finite(s.location)?.clamp(0.0, 100.0) / 100.0, finite(s.opacity)?.clamp(0.0, 100.0) / 100.0))).collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (first, last) = match (stops.first(), stops.last()) {
        (Some(&a), Some(&b)) => (a, b),
        _ => ((0.0, def.start as f64), (1.0, def.end as f64)),
    };
    let vector = match (g.start, finite(g.length)) {
        (Some([x, y]), Some(len)) if len > 0.0 => {
            let a = angle.to_radians();
            let (dx, dy) = (len * a.cos(), -len * a.sin());
            let (l0, l1) = if !g.radial && last.0 > first.0 { (first.0, last.0) } else { (0.0, 1.0) };
            Some([x + dx * l0, y + dy * l0, x + dx * l1, y + dy * l1])
        }
        _ => None,
    };
    GradientFeather { on: true, radial: g.radial, angle, start: first.1 as f32, end: last.1 as f32, vector }
}

fn names_angle(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 135.0;
    }
    let a = y.atan2(-x).to_degrees();
    if a < 0.0 { a + 360.0 } else { a }
}
