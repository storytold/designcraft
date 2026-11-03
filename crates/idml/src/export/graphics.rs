use super::common::{bool_s, color_value, pct, pkg_root};
use super::{Color, ColorType, El, Ex, GradientKind, SwatchValue, document, escape_id, num, swatch};

// Retained IDML attributes have unprefixed ASCII names. Native JSON can be
// edited independently: never let its map keys become XML syntax or Self IDs.
pub(super) fn graphic_layer_attribute_name(key: &str) -> bool {
    !key.is_empty()
        && key != "Self"
        && key != "xmlns"
        && key.bytes().enumerate().all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && (b.is_ascii_digit() || matches!(b, b'.' | b'-'))))
}

impl<'a> Ex<'a> {
    /// A `Gradient` element; stop colours missing from the swatches become anonymous colours.
    fn gradient_el(
        &mut self,
        s: &designcraft_color::Swatch,
        gradient: &designcraft_color::Gradient,
        colors: &mut Vec<El>,
        by_value: &mut Vec<(Color, String)>,
    ) -> El {
        let id = self.sw(&s.name);
        let mut g = El::new("Gradient")
            .attr("Self", &id)
            .attr("Type", if gradient.kind == GradientKind::Radial { "Radial" } else { "Linear" })
            .attr("Name", &s.name)
            .attr("ColorEditable", "true")
            .attr("ColorRemovable", "true")
            .attr("Visible", "true")
            .attr("SwatchCreatorID", "7937");
        let d = self.d;
        for (i, stop) in gradient.stops.iter().enumerate() {
            // A stop on a swatch names it; a plain color names an equal color swatch or
            // an unnamed color.
            let named =
                stop.swatch.as_deref().filter(|n| d.swatch(n).is_some_and(|w| !matches!(w.value, SwatchValue::Gradient { .. } | SwatchValue::None)));
            let cid = match (named, by_value.iter().find(|(c, _)| *c == stop.color)) {
                (Some(n), _) => self.sw(n),
                (None, Some((_, id))) => id.clone(),
                (None, None) => {
                    let cid = format!("Color/{}", self.fresh());
                    colors.push(color_el(&cid, "$ID/", "Process", &stop.color, "Normal", (true, true, false)));
                    by_value.push((stop.color, cid.clone()));
                    cid
                }
            };
            let mut e = El::new("GradientStop")
                .attr("Self", format!("{}GradientStop{i}", id.replace(['/', ' '], "_")))
                .attr("StopColor", cid)
                .attr("Location", pct(stop.offset as f64));
            if i > 0 {
                e.set("Midpoint", pct(gradient.stops[i - 1].midpoint as f64));
            }
            g.push(e);
        }
        g
    }

    // ---------- Graphic.xml ----------

    pub(super) fn graphic_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let mut root = pkg_root("Graphic");
        let mut colors: Vec<El> = Vec::new();
        let mut tints: Vec<El> = Vec::new();
        let mut gradients: Vec<El> = Vec::new();
        let mut inks: Vec<El> = Vec::new();
        // Specials (always present).
        colors.push(color_el("Color/Black", "Black", "Process", &Color::cmyk(0.0, 0.0, 0.0, 1.0), "Specialblack", (false, false, true)));
        let paper = match d.swatch(swatch::PAPER).map(|s| &s.value) {
            Some(SwatchValue::Paper { color }) => *color,
            _ => Color::cmyk(0.0, 0.0, 0.0, 0.0),
        };
        let paper = if paper == Color::WHITE { Color::cmyk(0.0, 0.0, 0.0, 0.0) } else { paper };
        colors.push(color_el("Color/Paper", "Paper", "Process", &paper, "Specialpaper", (true, false, true)));
        colors.push(color_el(
            "Color/Registration",
            "Registration",
            "Registration",
            &Color::cmyk(1.0, 1.0, 1.0, 1.0),
            "Specialregistration",
            (false, false, true),
        ));
        for (i, ink) in
            [("Process Cyan", 75, 0.61), ("Process Magenta", 15, 0.76), ("Process Yellow", 0, 0.16), ("Process Black", 45, 1.7)].iter().enumerate()
        {
            inks.push(
                El::new("Ink")
                    .attr("Self", format!("Ink/$ID/{}", ink.0))
                    .attr("Name", format!("$ID/{}", ink.0))
                    .attr("Angle", ink.1)
                    .attr("ConvertToProcess", "false")
                    .attr("Frequency", "70")
                    .attr("NeutralDensity", num(ink.2))
                    .attr("PrintInk", "true")
                    .attr("TrapOrder", i + 1)
                    .attr("InkType", "Normal"),
            );
        }
        // Colour swatches, by value, for gradient stop lookup.
        let mut by_value: Vec<(Color, String)> = Vec::new();
        for s in &d.swatches {
            if s.is_special() {
                continue;
            }
            let id = self.sw(&s.name);
            match &s.value {
                SwatchValue::Color { color, color_type } => {
                    let model = if *color_type == ColorType::Spot { "Spot" } else { "Process" };
                    colors.push(color_el(&id, &s.name, model, color, "Normal", (true, true, true)));
                    by_value.push((*color, id.clone()));
                    if *color_type == ColorType::Spot {
                        let mut ink = El::new("Ink");
                        if let Some((_, to)) = d.inks.aliases.iter().find(|(a, _)| *a == s.name) {
                            ink.set("AliasInkName", to);
                        }
                        let convert = d.inks.all_to_process || d.inks.to_process.contains(&s.name);
                        inks.push(
                            ink.attr("Self", format!("Ink/{}", escape_id(&s.name)))
                                .attr("Name", &s.name)
                                .attr("Angle", "45")
                                .attr("ConvertToProcess", bool_s(convert))
                                .attr("Frequency", "70")
                                .attr("NeutralDensity", "0.5")
                                .attr("PrintInk", "true")
                                .attr("TrapOrder", inks.len() + 1)
                                .attr("InkType", "Normal"),
                        );
                    }
                }
                SwatchValue::Tint { base, tint } => {
                    tints.push(
                        El::new("Tint")
                            .attr("Self", &id)
                            .attr("TintValue", pct(*tint as f64))
                            .attr("BaseColor", self.sw(base))
                            .attr("Name", &s.name)
                            .attr("ColorOverride", "Normal")
                            .attr("AlternateSpace", "NoAlternateColor")
                            .attr("AlternateColorValue", "")
                            .attr("ColorEditable", "true")
                            .attr("ColorRemovable", "true")
                            .attr("Visible", "true")
                            .attr("SwatchCreatorID", "7937"),
                    );
                }
                _ => {}
            }
        }
        for s in &d.swatches {
            let SwatchValue::Gradient { gradient } = &s.value else { continue };
            gradients.push(self.gradient_el(s, gradient, &mut colors, &mut by_value));
        }
        for c in colors {
            root.push(c);
        }
        for i in inks {
            root.push(i);
        }
        for t in tints {
            root.push(t);
        }
        root.push(
            El::new("Swatch")
                .attr("Self", "Swatch/None")
                .attr("Name", "None")
                .attr("ColorEditable", "false")
                .attr("ColorRemovable", "false")
                .attr("Visible", "true")
                .attr("SwatchCreatorID", "7937"),
        );
        for g in gradients {
            root.push(g);
        }
        for (gi, g) in d.color_groups.iter().enumerate() {
            let mut el = El::new("ColorGroup")
                .attr("Self", format!("ColorGroup/{}", escape_id(&g.name)))
                .attr("Name", &g.name)
                .attr("IsRootColorGroup", "false");
            for (k, n) in g.swatches.iter().enumerate() {
                if let Some(id) = self.swatch_ids.get(n) {
                    el.push(El::new("ColorGroupSwatch").attr("Self", format!("u{gi}ColorGroupSwatch{k}")).attr("SwatchItemRef", id));
                }
            }
            root.push(el);
        }
        for st in &d.stroke_styles {
            let id = format!("CustomStrokeStyle/{}", st.name);
            let pcts = |v: &[f64]| v.iter().map(|x| num(x * 100.0)).collect::<Vec<_>>().join(" ");
            let el = match &st.kind {
                designcraft_doc::StrokeType::Stripes { bands } => {
                    let flat: Vec<f64> = bands.iter().flat_map(|(a, w)| [*a, a + w]).collect();
                    Some(El::new("StripedStrokeStyle").attr("StripeArray", pcts(&flat)))
                }
                designcraft_doc::StrokeType::Dashed { pattern } => {
                    Some(El::new("DashedStrokeStyle").attr("DashArray", pattern.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" ")))
                }
                designcraft_doc::StrokeType::Dotted => Some(El::new("DottedStrokeStyle").attr("DotArray", "0 200")),
                _ => None,
            };
            if let Some(el) = el {
                root.push(el.attr("Self", id).attr("Name", &st.name));
            }
        }
        for s in STROKE_STYLES {
            root.push(El::new("StrokeStyle").attr("Self", format!("StrokeStyle/$ID/{s}")).attr("Name", format!("$ID/{s}")));
        }
        document(&root)
    }
}

const STROKE_STYLES: &[&str] = &[
    "Triple_Stroke",
    "ThickThinThick",
    "ThinThickThin",
    "ThickThick",
    "ThickThin",
    "ThinThick",
    "ThinThin",
    "Japanese Dots",
    "White Diamond",
    "Left Slant Hash",
    "Right Slant Hash",
    "Straight Hash",
    "Wavy",
    "Canned Dotted",
    "Canned Dashed 3x2",
    "Canned Dashed 4x4",
    "Dashed",
    "Solid",
];

/// A `Color` element. `flags` is (editable, removable, visible).
fn color_el(self_id: &str, name: &str, model: &str, c: &Color, over: &str, (editable, removable, visible): (bool, bool, bool)) -> El {
    let (space, val) = color_value(c);
    El::new("Color")
        .attr("Self", self_id)
        .attr("Model", model)
        .attr("Space", space)
        .attr("ColorValue", val)
        .attr("ColorOverride", over)
        .attr("AlternateSpace", "NoAlternateColor")
        .attr("AlternateColorValue", "")
        .attr("Name", name)
        .attr("ColorEditable", bool_s(editable))
        .attr("ColorRemovable", bool_s(removable))
        .attr("Visible", bool_s(visible))
        .attr("SwatchCreatorID", "7937")
}
