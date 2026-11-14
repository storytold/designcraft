use super::common::{color_value, pct};
use super::{Color, ColorType, Ex, GradientKind, SwatchValue, escape_id, num, swatch};
use craft_idml::model::{
    ColorDefinition, ColorGroupDefinition, ColorGroupMember, GradientDefinition, GradientStopDefinition, GraphicPart, InkDefinition,
    StrokeDefinition, TintDefinition,
};

impl<'a> Ex<'a> {
    /// A `Gradient` element; stop colours missing from the swatches become anonymous colours.
    fn gradient_el(
        &mut self,
        s: &designcraft_color::Swatch,
        gradient: &designcraft_color::Gradient,
        colors: &mut Vec<ColorDefinition>,
        by_value: &mut Vec<(Color, String)>,
    ) -> GradientDefinition {
        let id = self.sw(&s.name);
        let mut stops = Vec::new();
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
            stops.push(GradientStopDefinition {
                self_id: format!("{}GradientStop{i}", id.replace(['/', ' '], "_")),
                color_id: cid,
                location: pct(stop.offset as f64),
                midpoint: (i > 0).then(|| pct(gradient.stops[i - 1].midpoint as f64)),
            });
        }
        GradientDefinition { self_id: id, name: s.name.clone(), radial: gradient.kind == GradientKind::Radial, stops }
    }

    // ---------- Graphic.xml ----------

    pub(super) fn graphic_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let mut graphic = GraphicPart::default();
        // Specials (always present).
        graphic.colors.push(color_el("Color/Black", "Black", "Process", &Color::cmyk(0.0, 0.0, 0.0, 1.0), "Specialblack", (false, false, true)));
        let paper = match d.swatch(swatch::PAPER).map(|s| &s.value) {
            Some(SwatchValue::Paper { color }) => *color,
            _ => Color::cmyk(0.0, 0.0, 0.0, 0.0),
        };
        let paper = if paper == Color::WHITE { Color::cmyk(0.0, 0.0, 0.0, 0.0) } else { paper };
        graphic.colors.push(color_el("Color/Paper", "Paper", "Process", &paper, "Specialpaper", (true, false, true)));
        graphic.colors.push(color_el(
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
            graphic.inks.push(InkDefinition {
                self_id: format!("Ink/$ID/{}", ink.0),
                name: format!("$ID/{}", ink.0),
                alias: None,
                angle: ink.1.to_string(),
                convert_to_process: false,
                frequency: "70".to_string(),
                neutral_density: num(ink.2),
                trap_order: i + 1,
                ink_type: "Normal".to_string(),
            });
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
                    graphic.colors.push(color_el(&id, &s.name, model, color, "Normal", (true, true, true)));
                    by_value.push((*color, id.clone()));
                    if *color_type == ColorType::Spot {
                        let alias = d.inks.aliases.iter().find(|(a, _)| *a == s.name).map(|(_, to)| to.clone());
                        let convert = d.inks.all_to_process || d.inks.to_process.contains(&s.name);
                        graphic.inks.push(InkDefinition {
                            self_id: format!("Ink/{}", escape_id(&s.name)),
                            name: s.name.clone(),
                            alias,
                            angle: "45".to_string(),
                            convert_to_process: convert,
                            frequency: "70".to_string(),
                            neutral_density: "0.5".to_string(),
                            trap_order: graphic.inks.len() + 1,
                            ink_type: "Normal".to_string(),
                        });
                    }
                }
                SwatchValue::Tint { base, tint } => {
                    graphic.tints.push(TintDefinition { self_id: id, value: pct(*tint as f64), base_color: self.sw(base), name: s.name.clone() });
                }
                _ => {}
            }
        }
        for s in &d.swatches {
            let SwatchValue::Gradient { gradient } = &s.value else { continue };
            graphic.gradients.push(self.gradient_el(s, gradient, &mut graphic.colors, &mut by_value));
        }
        for (gi, g) in d.color_groups.iter().enumerate() {
            let mut members = Vec::new();
            for (k, n) in g.swatches.iter().enumerate() {
                if let Some(id) = self.swatch_ids.get(n) {
                    members.push(ColorGroupMember { self_id: format!("u{gi}ColorGroupSwatch{k}"), swatch_id: id.clone() });
                }
            }
            graphic.color_groups.push(ColorGroupDefinition { self_id: format!("ColorGroup/{}", escape_id(&g.name)), name: g.name.clone(), members });
        }
        for st in &d.stroke_styles {
            let id = format!("CustomStrokeStyle/{}", st.name);
            let pcts = |v: &[f64]| v.iter().map(|x| num(x * 100.0)).collect::<Vec<_>>().join(" ");
            let el = match &st.kind {
                designcraft_doc::StrokeType::Stripes { bands } => {
                    let flat: Vec<f64> = bands.iter().flat_map(|(a, w)| [*a, a + w]).collect();
                    Some(StrokeDefinition::Striped { self_id: id.clone(), name: st.name.clone(), array: pcts(&flat) })
                }
                designcraft_doc::StrokeType::Dashed { pattern } => Some(StrokeDefinition::Dashed {
                    self_id: id.clone(),
                    name: st.name.clone(),
                    array: pattern.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" "),
                }),
                designcraft_doc::StrokeType::Dotted => {
                    Some(StrokeDefinition::Dotted { self_id: id.clone(), name: st.name.clone(), array: "0 200".to_string() })
                }
                _ => None,
            };
            if let Some(style) = el {
                graphic.stroke_styles.push(style);
            }
        }
        for s in STROKE_STYLES {
            graphic.stroke_styles.push(StrokeDefinition::Named { self_id: format!("StrokeStyle/$ID/{s}"), name: format!("$ID/{s}") });
        }
        craft_idml::part_xml(&graphic.to_el())
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
fn color_el(self_id: &str, name: &str, model: &str, c: &Color, over: &str, (editable, removable, visible): (bool, bool, bool)) -> ColorDefinition {
    let (space, val) = color_value(c);
    ColorDefinition {
        self_id: self_id.to_string(),
        name: name.to_string(),
        model: model.to_string(),
        space: space.to_string(),
        value: val,
        override_kind: over.to_string(),
        editable,
        removable,
        visible,
    }
}
