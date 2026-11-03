use super::values::{finite, tint};
use super::{Corner, DirectionalFeather, DropShadow, Effects, El, GradientFeather, HashMap, Importer, Node, Stroke, names};

/// InDesign's corner size where a file gives a corner shape without one.
pub(super) const INDESIGN_CORNER_SIZE: f64 = 12.0;

/// The four corners of a paragraph border or shading (`prefix` `ParagraphBorder` /
/// `ParagraphShading`): top left, top right, bottom right, bottom left. Unset when none is given.
pub(super) fn corners(e: &El, prefix: &str) -> Option<[Corner; 4]> {
    let keys = ["TopLeft", "TopRight", "BottomRight", "BottomLeft"];
    let shapes = keys.map(|n| e.prop(&format!("{prefix}{n}CornerOption")));
    let sizes = keys.map(|n| e.num(&format!("{prefix}{n}CornerRadius")));
    if shapes.iter().all(Option::is_none) && sizes.iter().all(Option::is_none) {
        return None;
    }
    let mut out = [Corner::default(); 4];
    for (c, (shape, size)) in out.iter_mut().zip(shapes.iter().zip(sizes)) {
        c.shape = shape.as_deref().map_or(designcraft_geom::corners::CornerShape::None, |s| names::corner_in(s.trim()));
        c.size = size.filter(|v| v.is_finite()).map_or(12.0, |v| v.max(0.0));
    }
    Some(out)
}

impl<'r> Importer<'r> {
    /// An item's object style chain: its applied style, the styles that one is based on, then
    /// `[None]` (cycle-safe).
    pub(super) fn object_style_chain(&self, e: &El) -> Vec<&El> {
        let mut chain = self.object_style_bases(e.get("AppliedObjectStyle").map(str::to_string));
        if let Some(none) = self.object_els.get("ObjectStyle/$ID/[None]")
            && !chain.iter().any(|c| std::ptr::eq(*c, none))
        {
            chain.push(none);
        }
        chain
    }

    /// Object style `id` and the styles it is based on, nearest first (cycle-safe, at most 16).
    fn object_style_bases(&self, id: Option<String>) -> Vec<&El> {
        let mut chain: Vec<&El> = Vec::new();
        let mut os = id;
        while let Some(id) = os {
            let Some(s) = self.object_els.get(&id) else { break };
            if chain.len() >= 16 || chain.iter().any(|c| std::ptr::eq(*c, s)) {
                break;
            }
            chain.push(s);
            os = s.prop("BasedOn").map(|b| {
                let b = b.trim();
                if b.starts_with("ObjectStyle/") { b.to_string() } else { format!("ObjectStyle/{}", names::escape_id(b)) }
            });
        }
        chain
    }

    /// Resolve an item attribute, falling back to its object style chain.
    pub(super) fn attr_or_style(&self, e: &El, k: &str) -> Option<String> {
        e.get(k).or_else(|| self.object_style_chain(e).into_iter().find_map(|s| s.get(k))).map(str::to_string)
    }

    /// The item's child element `name` laid over the same element of each style in its object style
    /// chain: attributes and nested elements merge one by one, nearer levels winning. IDML writes on an
    /// item only the values that differ from its style. Effects whose category the applied style turns
    /// off (`ObjectStyle*EffectsCategorySettings`) come from the item alone.
    pub(super) fn with_style(&self, e: &El, name: &str) -> Option<El> {
        let chain = self.object_style_chain(e);
        let category = match name {
            "TransparencySetting" => Some("ObjectStyleObjectEffectsCategorySettings"),
            "StrokeTransparencySetting" => Some("ObjectStyleStrokeEffectsCategorySettings"),
            "FillTransparencySetting" => Some("ObjectStyleFillEffectsCategorySettings"),
            "ContentTransparencySetting" => Some("ObjectStyleContentEffectsCategorySettings"),
            _ => None,
        };
        let enabled = |effect: &str| {
            let Some(flag) = category.and_then(|_| effect_flag(effect)) else { return true };
            chain.iter().find_map(|s| category.and_then(|c| s.find(c)).and_then(|c| c.get(flag))) != Some("false")
        };
        let mut out: Option<El> = None;
        for s in chain.iter().rev() {
            let Some(c) = s.find(name) else { continue };
            let mut c = c.clone();
            c.children.retain(|n| !matches!(n, Node::El(x) if !enabled(x.local())));
            match &mut out {
                Some(o) => overlay(o, &c, 0),
                None => out = Some(c),
            }
        }
        if let Some(c) = e.find(name) {
            match &mut out {
                Some(o) => overlay(o, c, 0),
                None => out = Some(c.clone()),
            }
        }
        out
    }

    /// Reads the drop shadow, feather, gradient feather and directional feather of a
    /// `TransparencySetting` into `fx`.
    pub(super) fn effects_in(&mut self, t: &El, fx: &mut Effects) {
        if let Some(ds) = t.find("DropShadowSetting")
            && ds.get("Mode") == Some("Drop")
        {
            let x = ds.num("XOffset").unwrap_or(4.95);
            let y = ds.num("YOffset").unwrap_or(4.95);
            let def = DropShadow::default();
            fx.drop_shadow = DropShadow {
                on: true,
                color: ds.get("EffectColor").map(|c| self.swatch_ref(c)).unwrap_or(def.color),
                opacity: ds.num("Opacity").map(|o| (o / 100.0) as f32).unwrap_or(def.opacity),
                angle: names_angle(x, y),
                distance: x.hypot(y),
                size: ds.num("Size").unwrap_or(def.size),
                spread: ds.num("Spread").unwrap_or(def.spread),
                global_light: false,
                // Without the attribute (earlier DesignCraft exports) the shadow stays Normal, as it
                // rendered then; a new shadow's default is Multiply.
                mode: ds.get("BlendMode").map_or(designcraft_color::BlendMode::Normal, names::blend_in),
            };
        }
        if let Some(f) = t.find("FeatherSetting")
            && f.get("Mode").is_some_and(|m| m != "None")
        {
            fx.feather = f.num("Width").unwrap_or(9.0);
            fx.feather_options.choke = finite(f.num("ChokeAmount")).unwrap_or(0.0).clamp(0.0, 100.0);
            fx.feather_options.corners = match f.get("CornerType") {
                Some("Sharp") => designcraft_doc::FeatherCorners::Sharp,
                Some("Rounded") => designcraft_doc::FeatherCorners::Rounded,
                _ => designcraft_doc::FeatherCorners::Diffused,
            };
        }
        if let Some(g) = t.find("GradientFeatherSetting")
            && g.boolean("Applied") == Some(true)
        {
            fx.gradient_feather = gradient_feather_in(g);
        }
        if let Some(df) = t.find("DirectionalFeatherSetting")
            && df.boolean("Applied") == Some(true)
        {
            let w = |k: &str| finite(df.num(k)).unwrap_or(0.0).max(0.0);
            fx.directional_feather = DirectionalFeather { on: true, widths: [w("TopWidth"), w("LeftWidth"), w("BottomWidth"), w("RightWidth")] };
        }
    }

    /// An object style's drop shadow and feather. Each is read when the effects category turns it
    /// on (without category settings: when its setting is on), from the nearest style in the
    /// BasedOn chain that has the setting. `None` when the style sets neither.
    pub(super) fn style_effects(&mut self, e: &El) -> Option<Effects> {
        let mut chain = self.object_style_bases(e.get("Self").map(str::to_string));
        if chain.is_empty() {
            chain.push(e);
        }
        let category = chain.iter().find_map(|s| s.find("ObjectStyleObjectEffectsCategorySettings")).cloned();
        let setting = |name: &str| chain.iter().find_map(|s| s.find("TransparencySetting")?.find(name)).cloned();
        let mut t = El::new("TransparencySetting");
        let mut any = false;
        for (name, flag) in [("DropShadowSetting", "EnableDropShadow"), ("FeatherSetting", "EnableFeather")] {
            let el = setting(name);
            let on = match &category {
                Some(c) => c.get(flag) == Some("true"),
                None => el.as_ref().and_then(|x| x.get("Mode")).is_some_and(|m| m != "None"),
            };
            any |= on;
            if let (true, Some(el)) = (on, el) {
                t.push(el);
            }
        }
        if !any {
            return None;
        }
        let mut fx = Effects::default();
        self.effects_in(&t, &mut fx);
        Some(fx)
    }

    pub(super) fn stroke_from(&mut self, e: &El, item: Option<&El>) -> Stroke {
        let g = |s: &Self, k: &str| -> Option<String> { if item.is_some() { s.attr_or_style(e, k) } else { e.get(k).map(str::to_string) } };
        let color = g(self, "StrokeColor");
        let mut s = Stroke::none();
        if let Some(c) = color {
            s.swatch = self.swatch_ref(&c);
        }
        if let Some(w) = g(self, "StrokeWeight").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()) {
            s.weight = w;
        }
        if let Some(t) = g(self, "StrokeTint").and_then(|v| v.parse().ok()) {
            s.tint = tint(Some(t)).unwrap_or(1.0);
        }
        if let Some(t) = g(self, "StrokeType") {
            s.kind = names::stroke_type_in(&t);
        }
        if let Some(v) = g(self, "StrokeAlignment") {
            s.align = names::stroke_align_in(&v);
        }
        if let Some(v) = g(self, "EndCap") {
            s.cap = names::cap_in(&v);
        }
        if let Some(v) = g(self, "EndJoin") {
            s.join = names::join_in(&v);
        }
        if let Some(v) = g(self, "MiterLimit").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()) {
            s.miter_limit = v;
        }
        if let Some(v) = g(self, "LeftLineEnd") {
            s.start = names::arrow_in(&v);
        }
        if let Some(v) = g(self, "RightLineEnd") {
            s.end = names::arrow_in(&v);
        }
        if let Some(v) = g(self, "GapColor") {
            s.gap_swatch = self.swatch_ref(&v);
        }
        if let Some(t) = g(self, "GapTint").and_then(|v| v.parse().ok()) {
            s.gap_tint = tint(Some(t)).unwrap_or(1.0);
        }
        s.overprint = g(self, "OverprintStroke").as_deref() == Some("true");
        s.gap_overprint = g(self, "OverprintGap").as_deref() == Some("true");
        s
    }
}

/// The object style category flag that turns an effect element on or off.
fn effect_flag(effect: &str) -> Option<&'static str> {
    Some(match effect {
        "BlendingSetting" => "EnableTransparency",
        "DropShadowSetting" => "EnableDropShadow",
        "FeatherSetting" => "EnableFeather",
        "InnerShadowSetting" => "EnableInnerShadow",
        "OuterGlowSetting" => "EnableOuterGlow",
        "InnerGlowSetting" => "EnableInnerGlow",
        "BevelAndEmbossSetting" => "EnableBevelEmboss",
        "SatinSetting" => "EnableSatin",
        "DirectionalFeatherSetting" => "EnableDirectionalFeather",
        "GradientFeatherSetting" => "EnableGradientFeather",
        _ => return None,
    })
}

/// Lays `top` over `base`: attributes one by one, child elements matched by name and merged the same
/// way, text replaced. A property given in one form (attribute or `Properties` child) replaces the
/// other form in `base`.
fn overlay(base: &mut El, top: &El, depth: usize) {
    if depth > 32 {
        *base = top.clone();
        return;
    }
    for (k, v) in &top.attrs {
        base.set(k, v);
        if let Some(Node::El(p)) = base.children.iter_mut().find(|n| matches!(n, Node::El(p) if p.local() == "Properties")) {
            p.children.retain(|n| !matches!(n, Node::El(x) if x.local() == k));
        }
    }
    if let Some(p) = top.find("Properties") {
        let names: Vec<&str> = p.elements().map(El::local).collect();
        base.attrs.retain(|(k, _)| !names.contains(&k.as_str()));
    }
    let has_text = top.children.iter().any(|n| matches!(n, Node::Text(t) | Node::CData(t) if !t.trim().is_empty()));
    if has_text {
        base.children.retain(|n| !matches!(n, Node::Text(_) | Node::CData(_)));
    }
    // The k-th child of a name merges with the k-th child of that name in `base` (repeated elements
    // such as gradient stops pair up in order).
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for n in &top.children {
        match n {
            Node::El(c) => {
                let k = seen.entry(c.local()).or_insert(0);
                let slot = base
                    .children
                    .iter_mut()
                    .filter_map(|b| match b {
                        Node::El(b) if b.local() == c.local() => Some(b),
                        _ => None,
                    })
                    .nth(*k);
                *k += 1;
                match slot {
                    Some(b) => overlay(b, c, depth + 1),
                    None => base.children.push(n.clone()),
                }
            }
            Node::Text(_) | Node::CData(_) if has_text => base.children.push(n.clone()),
            _ => {}
        }
    }
}

/// A `GradientFeatherSetting`. The model holds two opacity stops at the ends of the gradient: the
/// first and last `OpacityGradientStop` give them; with a linear vector, their `Location`s move its
/// ends.
fn gradient_feather_in(g: &El) -> GradientFeather {
    let def = GradientFeather::default();
    let angle = finite(g.num("Angle")).unwrap_or(0.0);
    let mut stops: Vec<(f64, f64)> = g
        .find_all("OpacityGradientStop")
        .filter_map(|s| Some((finite(s.num("Location"))?.clamp(0.0, 100.0) / 100.0, finite(s.num("Opacity"))?.clamp(0.0, 100.0) / 100.0)))
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (first, last) = match (stops.first(), stops.last()) {
        (Some(&a), Some(&b)) => (a, b),
        _ => ((0.0, def.start as f64), (1.0, def.end as f64)),
    };
    let start: Option<[f64; 2]> = g
        .prop("GradientStart")
        .and_then(|s| s.split_whitespace().map(|v| v.parse().ok().filter(|v: &f64| v.is_finite())).collect::<Option<Vec<f64>>>())
        .and_then(|v| v.try_into().ok());
    let radial = g.get("Type") == Some("Radial");
    let vector = match (start, finite(g.num("Length"))) {
        (Some([x, y]), Some(len)) if len > 0.0 => {
            let a = angle.to_radians();
            let (dx, dy) = (len * a.cos(), -len * a.sin());
            let (l0, l1) = if !radial && last.0 > first.0 { (first.0, last.0) } else { (0.0, 1.0) };
            Some([x + dx * l0, y + dy * l0, x + dx * l1, y + dy * l1])
        }
        _ => None,
    };
    GradientFeather { on: true, radial, angle, start: first.1 as f32, end: last.1 as f32, vector }
}

fn names_angle(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 135.0;
    }
    let a = y.atan2(-x).to_degrees();
    if a < 0.0 { a + 360.0 } else { a }
}
