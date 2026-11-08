use super::{Affine, Cache, Document, Item, Rect, SpreadRef, fx};

/// How far (points) an object's soft effects reach outside its geometry.
pub fn effect_outset(it: &Item) -> f64 {
    fx::outset(it)
}

/// Largest side of a raster the renderer draws. vello_cpu keeps sizes in `u16` and rounds them
/// up to its tiles (4 px) and depth buckets (128 px), so a side near `u16::MAX` overflows inside
/// it: stay a few buckets below.
pub const MAX_SIDE: u32 = u16::MAX as u32 + 1 - 512;

/// Largest page raster [`Renderer::render_page`] makes (2 GiB of RGBA).
pub const MAX_PAGE_PIXELS: u64 = 1 << 29;

/// A rendered image (premultiplied RGBA8, row-major).
#[derive(Clone)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// What to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOptions {
    /// Canvas background (premultiplied RGBA8); `None` = transparent.
    pub background: Option<[u8; 4]>,
    /// Fill pages with paper colour.
    pub paper: bool,
    /// Skip non-printing items and layers (Preview mode, export).
    pub printing_only: bool,
    /// Items not to draw (live drag previews).
    pub hidden: Vec<designcraft_doc::ItemId>,
    /// Draw text smaller than this many pixels as grey bars ("greeking"); 0 = never.
    pub greek_below_px: f64,
    /// Drop shadow on pages (screen view).
    pub page_shadow: bool,
    /// Pink highlight behind text set in fonts that aren't installed (screen view).
    pub highlight_missing_fonts: bool,
    /// Preferences › Composition: yellow behind lines whose word spacing breaks the paragraph's
    /// limits (darker = worse), green behind text with custom tracking or kerning (screen view).
    pub highlight_hj: bool,
    pub highlight_keeps: bool,
    pub highlight_custom_tracking: bool,
    /// Conditional text indicators: an underline in each condition's colour (screen view).
    pub condition_indicators: bool,
    /// Editorial note anchors (screen view).
    pub note_indicators: bool,
    /// Track Changes: added text highlighted (screen view).
    pub change_markup: bool,
    /// View › Structure › Show Tag Markers: brackets around inline-tagged text (screen view).
    pub tag_markers: bool,
    /// The CMYK blend space's ink passes (internal): 3 draws black ink coverage as gray, 4 cyan,
    /// magenta and yellow coverage in red, green and blue, from each object's own color (CMYK as
    /// authored, others through the color settings); 0–2 draw cyan, magenta or yellow as gray.
    pub plate: Option<u8>,
    /// View › Display Performance.
    pub quality: DisplayQuality,
    /// Preferences › Appearance of Black: show 100% K as rich (pure) black instead of the
    /// accurate dark grey.
    pub rich_black: bool,
    /// View › Overprint Preview: overprinting inks mix with what's beneath.
    pub overprint_preview: bool,
    /// Screen: spreads with transparency are shown as blended in the document's blend space (a
    /// CMYK blend space keeps their colours in the working CMYK gamut).
    pub blend_space_view: bool,
    /// View › Show Document Grid: the area (spread coordinates) of each listed spread that the
    /// document grid covers, drawn behind or in front of the page items as the grid settings say
    /// (screen view). Empty draws no grid.
    pub document_grid: Vec<(SpreadRef, Rect)>,
}

/// View › Display Performance: how placed graphics and effects are drawn on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayQuality {
    /// Graphics as grey boxes, no transparency effects.
    Fast,
    /// Low-resolution proxies (72 ppi at 100%).
    Typical,
    /// Full resolution.
    #[default]
    High,
}

/// A spread placed on the canvas: canvas = xf × spread (a translation, turned for a rotated spread view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub spread: SpreadRef,
    pub xf: Affine,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub items: usize,
    pub glyphs: usize,
    pub micros: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct Frame<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) cache: &'a Cache,
    pub(crate) view: Affine,
    /// Visible area in spread space.
    pub(crate) visible: Rect,
    /// Size of a device pixel in points.
    pub(crate) px: f64,
    pub(crate) opts: &'a RenderOptions,
    /// Drawing into a multithreaded context (no filter layers).
    pub(crate) mt: bool,
}

pub fn default_threads() -> u16 {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(n) = std::env::var("DESIGNCRAFT_RENDER_THREADS").ok().and_then(|v| v.parse().ok()) {
            return n;
        }
        std::thread::available_parallelism().map(|n| (n.get().saturating_sub(1)).clamp(0, 8) as u16).unwrap_or(0)
    }
}
