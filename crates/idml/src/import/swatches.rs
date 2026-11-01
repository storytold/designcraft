use super::frame::{Prefs, text_frame_options};
use super::values::{lab_to_color, nums};
use super::{
    Color, ColorType, El, Gradient, GradientKind, GradientStop, HashMap, HashSet, Importer, Styles, Swatch, SwatchValue, TextFrameOptions, names,
    swatch,
};

impl<'r> Importer<'r> {
    // ---------- swatches ----------

    fn color_of(e: &El) -> Option<(Color, ColorType)> {
        let v = nums(e.get("ColorValue").unwrap_or(""));
        let space = e.get("Space").unwrap_or("CMYK");
        let c = match space {
            "RGB" if v.len() >= 3 => Color::rgb((v[0] / 255.0) as f32, (v[1] / 255.0) as f32, (v[2] / 255.0) as f32),
            "LAB" if v.len() >= 3 => lab_to_color(v[0] as f32, v[1] as f32, v[2] as f32),
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
        let ty = if e.get("Model") == Some("Spot") { ColorType::Spot } else { ColorType::Process };
        Some((c, ty))
    }

    /// Custom stroke styles.
    fn read_stroke_styles(&mut self, top: &[El]) {
        for e in top.iter().filter(|e| matches!(e.local(), "StripedStrokeStyle" | "DashedStrokeStyle" | "DottedStrokeStyle")) {
            let (Some(id), Some(name)) = (e.get("Self"), e.get("Name")) else { continue };
            let kind = match e.local() {
                "StripedStrokeStyle" => designcraft_doc::StrokeType::Stripes {
                    bands: nums(e.get("StripeArray").unwrap_or(""))
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| (c[0] / 100.0, (c[1] - c[0]) / 100.0))
                        .collect(),
                },
                "DashedStrokeStyle" => designcraft_doc::StrokeType::Dashed { pattern: nums(e.get("DashArray").unwrap_or("")) },
                _ => designcraft_doc::StrokeType::Dotted,
            };
            let _ = id;
            self.stroke_styles.push(designcraft_doc::StrokeStyleDef { name: name.to_string(), kind });
        }
    }

    /// Ink Manager settings: inks converted to process and ink aliases.
    fn read_inks(&mut self, top: &[El]) {
        for e in top.iter().filter(|e| e.local() == "Ink") {
            let Some(name) = e
                .get("Name")
                .filter(|n| !n.starts_with("$ID/") && !["Process Cyan", "Process Magenta", "Process Yellow", "Process Black"].contains(n))
            else {
                continue;
            };
            if e.get("ConvertToProcess") == Some("true") {
                self.inks.to_process.push(name.to_string());
            }
            if let Some(a) = e.get("AliasInkName").filter(|a| !a.is_empty() && *a != name && !a.starts_with("$ID/")) {
                self.inks.aliases.push((name.to_string(), a.to_string()));
            }
        }
    }

    pub(super) fn graphics(&mut self, top: &[El]) {
        let mut order: Vec<(String, Swatch)> = Vec::new();
        // Colours. The gradient tool writes a fresh, undocumented, machine-named swatch
        // ("r204g51b51", a trailing " 2" when InDesign had to dedupe it against itself) for every
        // process-colour stop it ever touched, so real files accumulate dozens of these; a later
        // one with the same cleaned name AND the same value merges into the first swatch of that
        // value instead of staying apart. A spot keeps its own name regardless (Ink Manager
        // settings and ink aliases, read below, are matched by name, and a spot's identity is the
        // ink it prints on, not its RGB preview). `auto_colors` maps the merge key to the kept
        // swatch's id and value; `auto_color_dup` records every id a later duplicate skipped, so
        // its name can be pointed at the kept swatch's own (possibly since-disambiguated) name.
        let mut auto_colors: HashMap<String, (String, Color)> = HashMap::new();
        let mut auto_color_dup: Vec<(String, String)> = Vec::new();
        for e in top.iter().filter(|e| e.local() == "Color") {
            let Some(id) = e.get("Self") else { continue };
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
            let name = e.get("Name").unwrap_or("");
            if e.get("Visible") == Some("false") || name.is_empty() || name == "$ID/" || e.get("Model") == Some("Registration") {
                self.hidden_colors.insert(id.to_string(), c);
                continue;
            }
            let cleaned = if ty == ColorType::Process { clean_auto_rgb_name(name) } else { None };
            let name = cleaned.as_deref().unwrap_or(name);
            if cleaned.is_some() {
                let key = format!("{ty:?}:{name}");
                match auto_colors.get(&key) {
                    Some((kept_id, kept_color)) if *kept_color == c => {
                        // A same-value duplicate: this id resolves to the kept swatch, whatever
                        // that swatch ends up named once disambiguation (below) settles it.
                        self.swatch_names.insert(id.to_string(), name.to_string());
                        auto_color_dup.push((id.to_string(), kept_id.clone()));
                        continue;
                    }
                    // Same cleaned name, different value: not this one's duplicate either. Keeps
                    // its own entry, disambiguated by name like any other collision, and leaves
                    // the earlier candidate as the one later same-value duplicates still merge
                    // into (replacing it here would lose that candidate for good).
                    Some(_) => {}
                    None => {
                        auto_colors.insert(key, (id.to_string(), c));
                    }
                }
            }
            let named = !name.starts_with("C=") && !name.starts_with("R=");
            self.swatch_names.insert(id.to_string(), name.to_string());
            order.push((
                id.to_string(),
                Swatch { name: name.to_string(), value: SwatchValue::Color { color: c, color_type: ty }, locked: false, named, hidden: false },
            ));
        }
        // Tints (after colours: they reference them). A tint's base id, so its stored `base` name
        // can be refreshed below if disambiguation (or an auto-named duplicate's merge) renames
        // the colour it was resolved to here.
        let mut tint_bases: Vec<(String, String)> = Vec::new();
        for e in top.iter().filter(|e| e.local() == "Tint") {
            let Some(id) = e.get("Self") else { continue };
            let base_id = e.get("BaseColor");
            let base = base_id.map(|b| self.swatch_ref(b)).unwrap_or_else(|| swatch::BLACK.to_string());
            if let Some(base_id) = base_id {
                tint_bases.push((id.to_string(), base_id.to_string()));
            }
            let t = e.num("TintValue").unwrap_or(100.0);
            let name = match e.get("Name") {
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
        for e in top.iter().filter(|e| e.local() == "Gradient") {
            let Some(id) = e.get("Self") else { continue };
            let mut stops: Vec<GradientStop> = Vec::new();
            for s in e.find_all("GradientStop") {
                let r = s.get("StopColor");
                // A merged duplicate's id has no `pending` entry of its own, and its name alone
                // can't tell it apart from an unrelated swatch that happens to share it (still
                // undisambiguated at this point) — resolve it to its kept swatch's id directly,
                // rather than leaving it to color_for_ref's id-or-name fallback.
                let resolved = r.map(|r| auto_color_dup.iter().find(|(dup, _)| dup.as_str() == r).map_or(r, |(_, kept)| kept.as_str()));
                let c = resolved.map(|r| self.color_for_ref(r, &order)).unwrap_or(Color::BLACK);
                // A stop on a named swatch (a spot, a tint, [Paper]) keeps it for its inks;
                // unnamed colors stay plain. The id becomes the swatch's name below, once
                // repeated names are numbered.
                let swatch = r.filter(|r| self.swatch_names.get(*r).is_some_and(|n| n != swatch::NONE)).map(str::to_string);
                let loc = s.num("Location").unwrap_or(0.0) / 100.0;
                if let (Some(prev), Some(m)) = (stops.last_mut(), s.num("Midpoint")) {
                    prev.midpoint = (m / 100.0) as f32;
                }
                stops.push(GradientStop { swatch, ..GradientStop::new(loc as f32, c) });
            }
            let kind = if e.get("Type") == Some("Radial") { GradientKind::Radial } else { GradientKind::Linear };
            let unnamed = e.get("Visible") == Some("false") || e.get("Name").is_none_or(|n| n.is_empty() || n == "$ID/");
            let name = if unnamed { "Gradient".to_string() } else { e.get("Name").unwrap_or_default().to_string() };
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
        let group_order: Vec<String> = top
            .iter()
            .filter(|e| e.local() == "ColorGroup")
            .flat_map(|g| g.find_all("ColorGroupSwatch").filter_map(|s| s.get("SwatchItemRef").map(str::to_string)).collect::<Vec<_>>())
            .collect();
        if !group_order.is_empty() {
            order.sort_by_key(|(id, _)| group_order.iter().position(|g| g == id).unwrap_or(usize::MAX));
        }
        let mut names: HashSet<String> = self.swatches.iter().map(|s| s.name.clone()).collect();
        let first_new = self.swatches.len();
        for (id, mut s) in order {
            if names.contains(&s.name) {
                let base = s.name.clone();
                s.name = Styles::unique_name(|n| names.contains(n), &base);
                self.swatch_names.insert(id, s.name.clone());
            }
            names.insert(s.name.clone());
            self.swatches.push(s);
        }
        // A merged duplicate's id still named itself after its own (pre-disambiguation) cleaned
        // name above; point it at the kept swatch's final one, in case disambiguation renamed it.
        for (dup_id, kept_id) in auto_color_dup {
            if let Some(final_name) = self.swatch_names.get(&kept_id).cloned() {
                self.swatch_names.insert(dup_id, final_name);
            }
        }
        // A tint's `base` above is the name its base colour had *then*; refresh it to that
        // colour's final one, in case disambiguation (or an auto-named duplicate's merge, both
        // above) renamed it since.
        for (tint_id, base_id) in tint_bases {
            let (Some(tint_name), Some(base_name)) = (self.swatch_names.get(&tint_id).cloned(), self.swatch_names.get(&base_id).cloned()) else {
                continue;
            };
            if let Some(SwatchValue::Tint { base, .. }) = self.swatches.iter_mut().skip(first_new).find(|s| s.name == tint_name).map(|s| &mut s.value)
            {
                *base = base_name;
            }
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
        for g in top.iter().filter(|e| e.local() == "ColorGroup" && e.get("IsRootColorGroup") != Some("true")) {
            let swatches: Vec<String> =
                g.find_all("ColorGroupSwatch").filter_map(|s| s.get("SwatchItemRef")).filter_map(|r| self.swatch_names.get(r).cloned()).collect();
            self.color_groups.push(designcraft_doc::ColorGroup { name: g.get("Name").unwrap_or("Color Group").to_string(), swatches });
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
        // An exact id match first: a colour's own entry, however many others share its (possibly
        // still pre-disambiguation, so non-unique) name. Only once that fails — a merged
        // auto-named duplicate's own id never got its own `pending` entry, since it shares the
        // kept swatch's — fall back to resolving by name.
        let name = self.swatch_names.get(r).map(String::as_str);
        let found = pending.iter().find(|(id, _)| id == r).or_else(|| pending.iter().find(|(_, s)| name == Some(s.name.as_str())));
        if let Some((_, s)) = found {
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

/// `r204g51b51` or `r204g51b51 2` (InDesign's own undocumented naming for a colour the Gradient
/// tool created, the trailing number from InDesign deduping its own name against itself): the
/// same `"R=.. G=.. B=.."` format this importer already gives value-named colours elsewhere.
/// `None` for anything else, including a real user name that happens to start with `r`.
fn clean_auto_rgb_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix('r')?;
    let (r, rest) = take_digits(rest)?;
    let rest = rest.strip_prefix('g')?;
    let (g, rest) = take_digits(rest)?;
    let rest = rest.strip_prefix('b')?;
    let (b, rest) = take_digits(rest)?;
    let rest = rest.trim();
    (rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit())).then(|| format!("R={r} G={g} B={b}"))
}

fn take_digits(s: &str) -> Option<(u16, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    (end > 0 && end <= 3).then(|| s[..end].parse().ok()).flatten().map(|n| (n, &s[end..]))
}
