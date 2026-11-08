//! Vertical metrics for upright glyphs in vertical text: advances down the line (`vmtx`), vertical
//! origins (`VORG`, else `vmtx` and the glyph's top) and the ideographic em box (`BASE`, else
//! OS/2 metrics). Fonts without them fall back to the em box: one em down the line, the origin at its top.

use skrifa::instance::Size;
use skrifa::metrics::GlyphMetrics;
use skrifa::raw::TableProvider;
use skrifa::raw::tables::{vmtx::Vmtx, vorg::Vorg};
use skrifa::raw::types::Tag;
use skrifa::{GlyphId, MetadataProvider};

use crate::FontFace;

/// A face's vertical metrics tables, read once for a run of lookups.
pub(crate) struct VMetrics<'a> {
    vmtx: Option<Vmtx<'a>>,
    vorg: Option<Vorg<'a>>,
    glyphs: Option<GlyphMetrics<'a>>,
    upem: f64,
    em_top: f64,
}

impl<'a> VMetrics<'a> {
    pub(crate) fn new(face: &'a FontFace) -> Self {
        let font = face.skrifa();
        VMetrics {
            vmtx: font.as_ref().and_then(|f| f.vmtx().ok()),
            vorg: font.as_ref().and_then(|f| f.vorg().ok()),
            glyphs: font.map(|f| f.glyph_metrics(Size::unscaled(), face.location())),
            upem: face.upem,
            em_top: face.em_box().0,
        }
    }

    /// Advance down the line in font units.
    pub(crate) fn advance(&self, gid: GlyphId) -> f64 {
        self.vmtx.as_ref().and_then(|t| t.advance(gid)).map_or(self.upem, f64::from)
    }

    /// Height of the vertical origin above the glyph's baseline in font units.
    pub(crate) fn origin(&self, gid: GlyphId) -> f64 {
        if let Some(t) = &self.vorg {
            return f64::from(t.vertical_origin_y(gid));
        }
        // TrueType outlines: the glyph's top plus its top side bearing.
        if let (Some(t), Some(g)) = (&self.vmtx, &self.glyphs)
            && let (Some(tsb), Some(b)) = (t.side_bearing(gid), g.bounds(gid))
            && t.advance(gid).is_some()
        {
            return f64::from(b.y_max) + f64::from(tsb);
        }
        self.em_top
    }
}

/// The ideographic em box (top, bottom) in font units, y up, one em tall: `BASE` `idtp`/`ideo`;
/// else, for a CJK font (its [`crate::FontGroup`] isn't Western), topped by the OS/2 typo ascender
/// when the typo ascender and descender span exactly one em; else centred on half the cap height
/// (OS/2 `sCapHeight`, else the `H` glyph's top); else 0.88 em above the baseline. The flag is
/// false for that last fallback.
pub(crate) fn em_box(face: &FontFace) -> ((f64, f64), bool) {
    let upem = face.upem;
    let fallback = ((upem * 0.88, upem * -0.12), false);
    let Some(font) = face.skrifa() else { return fallback };
    if let Some(b) = base_em_box(&font, upem) {
        return (b, true);
    }
    // Latin fonts with a one-em typo box (DejaVu) still centre on the cap height.
    let cjk = face.menu_group().0 != crate::FontGroup::Western;
    let typo_top = font.os2().ok().filter(|_| cjk).and_then(|t| {
        let (asc, desc) = (f64::from(t.s_typo_ascender()), f64::from(t.s_typo_descender()));
        (asc - desc == upem && asc > 0.0 && asc <= upem).then_some(asc)
    });
    let top = typo_top.or_else(|| {
        let declared = font.metrics(Size::unscaled(), face.location()).cap_height.map(f64::from);
        let cap = crate::fontdb::plausible(declared, upem).or_else(|| crate::fontdb::measured_top(&font, face.location(), &['H'], upem))?;
        Some((upem + cap) / 2.0)
    });
    top.map_or(fallback, |top| ((top, top - upem), true))
}

/// The ideographic character face (ICF) box (top, bottom) in font units, y up: `BASE`
/// `icft`/`icfb` (one of them alone is mirrored inside the em box), else the em box inset by 5% of
/// the em at the top and bottom.
pub(crate) fn icf_box(face: &FontFace) -> (f64, f64) {
    let (top, bottom) = face.em_box();
    let font = face.skrifa();
    let base = match font.as_ref().and_then(|f| base_box(f, b"icfb", b"icft")) {
        Some((Some(b), Some(t))) => Some((t, b)),
        Some((Some(b), None)) => Some((top - (b - bottom), b)),
        Some((None, Some(t))) => Some((t, bottom + (top - t))),
        _ => None,
    };
    base.filter(|(t, b)| t.is_finite() && b.is_finite() && t > b).unwrap_or_else(|| {
        let inset = (top - bottom) * 0.05;
        (top - inset, bottom + inset)
    })
}

/// The em box from the `BASE` table's horizontal axis: the Han script's values, else the default
/// script's, else the first script's that has them.
fn base_em_box(font: &skrifa::FontRef<'_>, upem: f64) -> Option<(f64, f64)> {
    match base_box(font, b"ideo", b"idtp")? {
        (Some(bottom), Some(top)) => (top > bottom).then_some((top, bottom)),
        (Some(bottom), None) => Some((bottom + upem, bottom)),
        (None, Some(top)) => Some((top, top - upem)),
        (None, None) => None,
    }
}

/// The `BASE` horizontal axis's `bottom` and `top` baselines in font units from the Han script's
/// values, else the default script's, else the first script's that has either.
fn base_box(font: &skrifa::FontRef<'_>, bottom_tag: &[u8; 4], top_tag: &[u8; 4]) -> Option<(Option<f64>, Option<f64>)> {
    let axis = font.base().ok()?.horiz_axis()?.ok()?;
    let tags = axis.base_tag_list()?.ok()?;
    let index = |tag: &[u8; 4]| tags.baseline_tags().iter().position(|t| t.get() == Tag::new(tag));
    let (ideo, idtp) = (index(bottom_tag), index(top_tag));
    if ideo.is_none() && idtp.is_none() {
        return None;
    }
    let scripts = axis.base_script_list().ok()?;
    let records = scripts.base_script_records();
    let rank = |t: Tag| [Tag::new(b"hani"), Tag::new(b"kana"), Tag::new(b"DFLT")].iter().position(|p| *p == t).unwrap_or(3);
    let mut order: Vec<_> = records.iter().collect();
    order.sort_by_key(|r| rank(r.base_script_tag()));
    order.into_iter().find_map(|r| {
        let values = r.base_script(scripts.offset_data()).ok()?.base_values()?.ok()?;
        let coord = |i: Option<usize>| i.and_then(|i| values.base_coords().get(i).ok()).map(|c| f64::from(c.coordinate()));
        let pair = (coord(ideo), coord(idtp));
        (pair.0.is_some() || pair.1.is_some()).then_some(pair)
    })
}

/// The shaper's vertical metrics callbacks: ours, so that its vertical advances and origins are
/// the ones glyphs are placed with.
pub(crate) struct VerticalFuncs<'a> {
    pub(crate) face: &'a FontFace,
    pub(crate) metrics: VMetrics<'a>,
}

impl VerticalFuncs<'_> {
    /// The vertical origin (x, y) in whole font units: centred across, [`VMetrics::origin`] up.
    pub(crate) fn origin(&self, gid: u32) -> (i32, i32) {
        ((self.face.advance(gid) / 2.0).round() as i32, self.metrics.origin(GlyphId::new(gid)).round() as i32)
    }
}

impl harfrust::font::FontFuncs for VerticalFuncs<'_> {
    fn advance_height(&mut self, _: &harfrust::font::BuiltinFontFuncs, glyph: harfrust::GlyphId) -> i32 {
        // The shaper's y axis points up: advancing down the line is negative.
        -(self.metrics.advance(GlyphId::new(glyph.to_u32())).round() as i32)
    }

    fn vertical_origin(&mut self, _: &harfrust::font::BuiltinFontFuncs, glyph: harfrust::GlyphId) -> (i32, i32) {
        self.origin(glyph.to_u32())
    }
}
