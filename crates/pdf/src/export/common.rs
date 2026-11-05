use super::*;

pub(super) enum TagEntry {
    Story(designcraft_doc::StoryId),
    Figure(TagGroup),
}

pub(crate) fn tf(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32)
}

pub(crate) fn to_path(bp: &BezPath) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for el in bp.elements() {
        match *el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, p) => pb.quad_to(a.x as f32, a.y as f32, p.x as f32, p.y as f32),
            PathEl::CurveTo(a, b, p) => pb.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

pub(crate) fn norm(v: f32) -> NormalizedF32 {
    NormalizedF32::new(if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 }).unwrap_or(NormalizedF32::ONE)
}

pub(crate) fn q(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn regular(c: &Color, rgb_only: bool) -> krilla::color::RegularColor {
    use krilla::color::RegularColor as R;
    if rgb_only && matches!(c, Color::Cmyk { .. }) {
        let [r, g, b] = c.to_rgb();
        return R::Rgb(rgb::Color::new(q(r), q(g), q(b)));
    }
    match *c {
        Color::Rgb { r, g, b } => R::Rgb(rgb::Color::new(q(r), q(g), q(b))),
        Color::Cmyk { c, m, y, k } => R::Cmyk(cmyk::Color::new(q(c), q(m), q(y), q(k))),
        // DesignCraft grey is ink coverage (0 = white); PDF DeviceGray is lightness.
        Color::Gray { k } => R::Luma(luma::Color::new(q(1.0 - k))),
    }
}

/// A process colour in its own device space (CMYK → DeviceCMYK, RGB → DeviceRGB, Gray → DeviceGray).
/// `rgb_only` converts CMYK to RGB (PDF/A without a CMYK output profile).
pub(crate) fn device(c: &Color, rgb_only: bool) -> krilla::color::Color {
    match regular(c, rgb_only) {
        krilla::color::RegularColor::Rgb(v) => v.into(),
        krilla::color::RegularColor::Cmyk(v) => v.into(),
        krilla::color::RegularColor::Luma(v) => v.into(),
    }
}

/// `[Registration]` prints on every plate: `/Separation /All`.
pub(crate) fn registration(tint: f32, rgb_only: bool) -> krilla::color::Color {
    use krilla::color::separation::{Color as SepColor, SeparationColorant, SeparationSpace};
    let alt = regular(&Color::cmyk(1.0, 1.0, 1.0, 1.0), rgb_only);
    SepColor::new(q(tint), SeparationSpace::new(SeparationColorant::AllColorants, alt)).into()
}

pub(super) fn blend(b: DcBlend) -> krilla::blend::BlendMode {
    use krilla::blend::BlendMode as K;
    match b {
        DcBlend::Normal => K::Normal,
        DcBlend::Darken => K::Darken,
        DcBlend::Multiply => K::Multiply,
        DcBlend::ColorBurn => K::ColorBurn,
        DcBlend::Lighten => K::Lighten,
        DcBlend::Screen => K::Screen,
        DcBlend::ColorDodge => K::ColorDodge,
        DcBlend::Overlay => K::Overlay,
        DcBlend::SoftLight => K::SoftLight,
        DcBlend::HardLight => K::HardLight,
        DcBlend::Difference => K::Difference,
        DcBlend::Exclusion => K::Exclusion,
        DcBlend::Hue => K::Hue,
        DcBlend::Saturation => K::Saturation,
        DcBlend::Color => K::Color,
        DcBlend::Luminosity => K::Luminosity,
    }
}

pub(super) fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

pub(crate) fn solid_fill(c: krilla::color::Color, opacity: f32) -> Fill {
    Fill { paint: c.into(), opacity: norm(opacity), rule: FillRule::NonZero }
}

impl Exporter<'_> {
    /// A print document, or a PDF/X file (whose output intent is CMYK).
    pub(crate) fn for_print(&self) -> bool {
        self.doc.settings.intent == designcraft_doc::Intent::Print || self.opts.standard.is_pdfx()
    }

    /// Whether a paint overprints: its own attribute, or 100% [Black] when Preferences ›
    /// Appearance of Black overprints it (not in PDF/A, whose RGB output has no overprint).
    pub(crate) fn overprints(&self, swatch: &str, tint: f32, flag: bool) -> bool {
        flag || (!self.rgb_only && self.doc.settings.overprint_black && swatch == designcraft_color::swatch::BLACK && tint >= 0.999)
    }

    /// Overprint for what is painted until the matching pop: `/op` for fills, `/OP` for strokes,
    /// and `/OPM 1`, so a CMYK paint's zero components leave the inks beneath it alone. Returns
    /// whether it pushed. PDF/A exports are RGB and leave overprint out.
    pub(crate) fn push_overprint(&mut self, s: &mut Surface, fill: bool, stroke: bool) -> bool {
        if !fill && !stroke {
            return false;
        }
        if self.rgb_only {
            self.warn("PDF/A: overprint was left out");
            return false;
        }
        s.push_overprint(krilla::blend::Overprint { fill, stroke, nonzero: true });
        true
    }

    pub(crate) fn warn(&mut self, w: impl Into<String>) {
        let w = w.into();
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    /// A swatch + tint as a PDF colour: spot swatches (and tints of them) become `/Separation`
    /// with the swatch's values as the alternate space; process colours stay in their device space.
    pub(crate) fn swatch_color(&self, name: &str, tint: f32) -> Option<krilla::color::Color> {
        use krilla::color::separation::{Color as SepColor, SeparationColorant, SeparationSpace};
        let mut n = name;
        let mut t = tint;
        for _ in 0..8 {
            let sw = self.doc.swatch(n)?;
            match &sw.value {
                SwatchValue::None => return None,
                // [Paper] prints no ink (it knocks out what lies below); its colour is only what
                // the screen shows. Written as RGB white (`1 1 1 rg`) it would put DeviceRGB in a
                // CMYK print file.
                SwatchValue::Paper { .. } if self.for_print() => return Some(device(&Color::cmyk(0.0, 0.0, 0.0, 0.0), self.rgb_only)),
                SwatchValue::Tint { base, tint: bt } => {
                    t *= bt;
                    n = base;
                }
                SwatchValue::Registration => return Some(registration(t, self.rgb_only)),
                SwatchValue::Color { color: _, color_type: ColorType::Spot } => {
                    // Ink Manager: an alias prints on another ink; converted inks go to process.
                    let (ink, process) = crate::inks::spot_output(self.doc, &self.inks, &sw.name);
                    let target =
                        self.doc.swatch(ink).filter(|w| matches!(w.value, SwatchValue::Color { color_type: ColorType::Spot, .. })).unwrap_or(sw);
                    let SwatchValue::Color { color, .. } = &target.value else { break };
                    if process {
                        return Some(device(&designcraft_color::swatch::apply_tint(*color, t), self.rgb_only));
                    }
                    let space = SeparationSpace::new(SeparationColorant::Custom(target.name.clone()), regular(color, self.rgb_only));
                    return Some(SepColor::new(q(t), space).into());
                }
                _ => break,
            }
        }
        self.doc.resolve_color(name, tint).map(|c| device(&c, self.rgb_only))
    }
}
