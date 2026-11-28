use super::frame::{Prefs, text_frame_options};
use super::values::lab_to_color;
use super::{
    Color, ColorType, El, Gradient, GradientKind, GradientStop, HashMap, HashSet, Importer, Styles, Swatch, SwatchValue, TextFrameOptions,
    TopLevelNode, names, swatch,
};

impl<'r> Importer<'r> {
    // ---------- swatches ----------

    fn color_of(e: &craft_idml::model::Swatch) -> Option<(Color, ColorType)> {
        let v = e.color_value();
        let c = match e.space() {
            craft_idml::model::ColorSpace::Rgb if v.len() >= 3 => Color::rgb((v[0] / 255.0) as f32, (v[1] / 255.0) as f32, (v[2] / 255.0) as f32),
            craft_idml::model::ColorSpace::Lab if v.len() >= 3 => lab_to_color(v[0] as f32, v[1] as f32, v[2] as f32),
            _ if v.len() >= 4 => Color::cmyk((v[0] / 100.0) as f32, (v[1] / 100.0) as f32, (v[2] / 100.0) as f32, (v[3] / 100.0) as f32),
            _ => return None,
        };
        // Huge values (1e300) overflow `f32` to infinity.
        let parts = match c {
            Color::Rgb { r, g, b } => [r, g, b, 0.0],
            Color::Cmyk { c, m, y, k } => [c, m, y, k],
            Color::Gray { k } => [k, 0.0, 0.0, 0.0],
        };
        if !parts.iter().all(|v| v.is_finite()) {
            return None;
        }
        let ty = if e.spot() { ColorType::Spot } else { ColorType::Process };
        Some((c, ty))
    }

    /// Custom stroke styles.
    fn read_stroke_styles(&mut self, top: &[TopLevelNode]) {
        let top: Vec<_> = top.iter().map(TopLevelNode::to_el).collect();
        for e in craft_idml::model::stroke_styles(&top) {
            let (Some(id), Some(name)) = (e.values.self_id(), e.values.name()) else { continue };
            let kind = match e.kind {
                craft_idml::model::StrokeStyleKind::Striped => designcraft_doc::StrokeType::Stripes {
                    bands: e.stripe_array().as_chunks::<2>().0.iter().map(|c| (c[0] / 100.0, (c[1] - c[0]) / 100.0)).collect(),
                },
                craft_idml::model::StrokeStyleKind::Dashed => designcraft_doc::StrokeType::Dashed { pattern: e.dash_array() },
                craft_idml::model::StrokeStyleKind::Dotted => designcraft_doc::StrokeType::Dotted,
            };
            let _ = id;
            self.stroke_styles.push(designcraft_doc::StrokeStyleDef { name: name.to_string(), kind });
        }
    }

    /// Ink Manager settings: inks converted to process and ink aliases.
    fn read_inks(&mut self, top: &[TopLevelNode]) {
        let model_top: Vec<_> = top.iter().map(TopLevelNode::to_el).collect();
        for e in craft_idml::model::inks(&model_top) {
            let Some(name) =
                e.name().filter(|n| !n.starts_with("$ID/") && !["Process Cyan", "Process Magenta", "Process Yellow", "Process Black"].contains(n))
            else {
                continue;
            };
            if e.convert_to_process() {
                self.inks.to_process.push(name.to_string());
            }
            if let Some(a) = e.alias().filter(|a| !a.is_empty() && *a != name && !a.starts_with("$ID/")) {
                self.inks.aliases.push((name.to_string(), a.to_string()));
            }
        }
    }

    pub(super) fn graphics(&mut self, top: &[TopLevelNode]) {
        let model_top: Vec<_> = top.iter().map(TopLevelNode::to_el).collect();
        let swatches = craft_idml::model::swatches(&model_top);
        let mut order: Vec<(String, Swatch)> = Vec::new();
        // Colours.
        for e in swatches.iter().filter(|e| e.kind == craft_idml::model::SwatchKind::Color) {
            let Some(id) = e.values.self_id() else { continue };
            let Some((c, ty)) = Self::color_of(e) else { continue };
            match id {
                "Color/Paper" => {
                    if let Some(s) = self.swatches.iter_mut().find(|s| s.name == swatch::PAPER) {
                        s.value = SwatchValue::Paper { color: if c == Color::cmyk(0.0, 0.0, 0.0, 0.0) { Color::WHITE } else { c } };
                    }
                    continue;
                }
                "Color/Black" | "Color/Registration" => continue,
                _ => {}
            }
            let name = e.values.name().unwrap_or("");
            if !e.values.visible() || name.is_empty() || name == "$ID/" || e.values.get("Model") == Some("Registration") {
                self.hidden_colors.insert(id.to_string(), c);
                continue;
            }
            let named = !name.starts_with("C=") && !name.starts_with("R=");
            self.swatch_names.insert(id.to_string(), name.to_string());
            order.push((
                id.to_string(),
                Swatch { name: name.to_string(), value: SwatchValue::Color { color: c, color_type: ty }, locked: false, named, hidden: false },
            ));
        }
        // Tints (after colours: they reference them).
        for e in swatches.iter().filter(|e| e.kind == craft_idml::model::SwatchKind::Tint) {
            let Some(id) = e.values.self_id() else { continue };
            let base = e.base_color().map(|b| self.swatch_ref(b)).unwrap_or_else(|| swatch::BLACK.to_string());
            let t = e.tint_value().unwrap_or(100.0);
            let name = match e.values.name() {
                Some(n) if !n.is_empty() && n != "$ID/" => n.to_string(),
                _ => format!("{base} {}%", names::num(t)),
            };
            self.swatch_names.insert(id.to_string(), name.clone());
            order.push((
                id.to_string(),
                Swatch { name, value: SwatchValue::Tint { base, tint: (t / 100.0) as f32 }, locked: false, named: true, hidden: false },
            ));
        }
        self.read_stroke_styles(top);
        self.read_inks(top);
        for e in swatches.iter().filter(|e| e.kind == craft_idml::model::SwatchKind::Gradient) {
            let Some(id) = e.values.self_id() else { continue };
            let mut stops: Vec<GradientStop> = Vec::new();
            for s in e.stops_in_order() {
                let r = s.color.as_deref();
                let c = r.map(|r| self.color_for_ref(r, &order)).unwrap_or(Color::BLACK);
                // A stop on a named swatch (a spot, a tint, [Paper]) keeps it for its inks;
                // unnamed colors stay plain. The id becomes the swatch's name below, once
                // repeated names are numbered.
                let swatch = r.filter(|r| self.swatch_names.get(*r).is_some_and(|n| n != swatch::NONE)).map(str::to_string);
                let loc = s.location.unwrap_or(0.0) / 100.0;
                if let (Some(prev), Some(m)) = (stops.last_mut(), s.midpoint) {
                    prev.midpoint = (m / 100.0) as f32;
                }
                stops.push(GradientStop { swatch, ..GradientStop::new(loc as f32, c) });
            }
            let kind = if e.radial() { GradientKind::Radial } else { GradientKind::Linear };
            let unnamed = !e.values.visible() || e.values.name().is_none_or(|n| n.is_empty() || n == "$ID/");
            let name = if unnamed { "Gradient".to_string() } else { e.values.name().unwrap_or_default().to_string() };
            let sw =
                Swatch { name, value: SwatchValue::Gradient { gradient: Gradient { kind, stops } }, locked: false, named: !unnamed, hidden: false };
            if unnamed {
                // Unnamed gradients become swatches only when something uses them.
                self.hidden_swatches.insert(id.to_string(), sw);
            } else {
                self.swatch_names.insert(id.to_string(), sw.name.clone());
                order.push((id.to_string(), sw));
            }
        }
        // Panel order from the root colour group.
        let groups = craft_idml::model::color_groups(&model_top);
        let group_order: Vec<String> = groups.iter().filter(|g| g.is_root()).flat_map(|g| g.members().into_iter().map(str::to_string)).collect();
        if !group_order.is_empty() {
            order.sort_by_key(|(id, _)| group_order.iter().position(|g| g == id).unwrap_or(usize::MAX));
        }
        let mut names: HashSet<String> = self.swatches.iter().map(|s| s.name.clone()).collect();
        let first_new = self.swatches.len();
        let mut next_suffix: HashMap<String, usize> = HashMap::new();
        for (id, mut s) in order {
            if names.contains(&s.name) {
                let base = s.name.clone();
                // Resume after the last suffix given to this base, so N duplicates cost O(N), not O(N^2).
                let from = next_suffix.get(&base).copied().unwrap_or(2);
                let (i, name) = (from..).map(|i| (i, format!("{base} {i}"))).find(|(_, n)| !names.contains(n)).unwrap_or((from, base.clone()));
                next_suffix.insert(base, i.saturating_add(1));
                s.name = name;
                self.swatch_names.insert(id, s.name.clone());
            }
            names.insert(s.name.clone());
            self.swatches.push(s);
        }
        let stop_names = |stops: &mut Vec<GradientStop>, ids: &HashMap<String, String>| {
            for stop in stops {
                stop.swatch = stop.swatch.take().and_then(|id| ids.get(&id).cloned());
            }
        };
        for s in self.swatches.iter_mut().skip(first_new).chain(self.hidden_swatches.values_mut()) {
            if let SwatchValue::Gradient { gradient } = &mut s.value {
                stop_names(&mut gradient.stops, &self.swatch_names);
            }
        }
        // Colour groups other than the root one become Swatches panel folders.
        for g in groups.iter().filter(|g| !g.is_root()) {
            let swatches = g.members().into_iter().filter_map(|r| self.swatch_names.get(r).cloned()).collect();
            self.color_groups.push(designcraft_doc::ColorGroup { name: g.values.name().unwrap_or("Color Group").to_string(), swatches });
        }
    }

    fn color_for_ref(&self, r: &str, pending: &[(String, Swatch)]) -> Color {
        if let Some(c) = self.hidden_colors.get(r) {
            return *c;
        }
        match r {
            "Color/Black" => return Color::cmyk(0.0, 0.0, 0.0, 1.0),
            "Color/Paper" => return Color::WHITE,
            "Color/Registration" => return Color::cmyk(1.0, 1.0, 1.0, 1.0),
            _ => {}
        }
        if let Some((_, s)) = pending.iter().find(|(id, _)| id == r) {
            match &s.value {
                SwatchValue::Color { color, .. } => return *color,
                SwatchValue::Tint { base, tint } => {
                    let mut all: Vec<Swatch> = self.swatches.clone();
                    all.extend(pending.iter().map(|(_, s)| s.clone()));
                    return designcraft_color::swatch::resolve(&all, base, *tint).unwrap_or(Color::BLACK);
                }
                _ => {}
            }
        }
        Color::BLACK
    }

    /// A `TextFramePreference`, with its column rule colour resolved to a swatch.
    pub(super) fn frame_options(&mut self, prefs: &[&El]) -> TextFrameOptions {
        let mut o = text_frame_options(prefs);
        if let Some(r) = Prefs(prefs).prop("ColumnRuleStrokeColor") {
            o.column_rule_color = self.swatch_ref(&r);
        }
        o
    }

    /// A swatch reference (Self id) → our swatch name; unnamed colours become value-named swatches.
    pub(super) fn swatch_ref(&mut self, r: &str) -> String {
        if let Some(n) = self.swatch_names.get(r) {
            return n.clone();
        }
        if r == "n" || r.is_empty() {
            return swatch::NONE.into();
        }
        if let Some(mut sw) = self.hidden_swatches.remove(r) {
            sw.name = Styles::unique_name(|n| self.swatches.iter().any(|s| s.name == n), &sw.name);
            self.swatch_names.insert(r.to_string(), sw.name.clone());
            let name = sw.name.clone();
            self.swatches.push(sw);
            return name;
        }
        if let Some(c) = self.hidden_colors.get(r).copied() {
            let name = match c {
                Color::Cmyk { c, m, y, k } => designcraft_color::swatch::cmyk_name(c, m, y, k),
                Color::Rgb { r, g, b } => format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round()),
                Color::Gray { k } => format!("K={}", (k * 100.0).round()),
            };
            if !self.swatches.iter().any(|s| s.name == name) {
                self.swatches.push(Swatch {
                    name: name.clone(),
                    value: SwatchValue::Color { color: c, color_type: ColorType::Process },
                    locked: false,
                    named: false,
                    hidden: false,
                });
            }
            self.swatch_names.insert(r.to_string(), name.clone());
            return name;
        }
        swatch::NONE.into()
    }
}
