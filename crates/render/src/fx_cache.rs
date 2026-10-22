//! Soft effects kept between renders (drop shadows, glows, inner shadows, bevels, satin and
//! feather masks). Drawn into a multithreaded context, each is a blur on an offscreen image of its
//! own; redrawing those for every frame made scrolling a page of shadowed headings slow (26 text
//! shadows took 44 of a frame's 50 ms). Each is now drawn once per object and zoom and placed
//! again while nothing it depends on changes: the object (its transform within its parent too),
//! its composed text, the zoom and rotation, the render options, and the document's swatches,
//! inks, stroke styles, layers, settings, images and the colour settings. Panning only moves it.

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_color::Swatch;
use designcraft_color::cms::Cms;
use designcraft_compose::{ComposedStory, ShownOn};
use designcraft_doc::{Asset, AssetId, DocSettings, Document, InkManager, Item, ItemId, Layer, StrokeStyleDef};
use designcraft_geom::{Affine, Rect, Vec2};
use vello_cpu::Pixmap;

use crate::RenderOptions;

/// Pixels one kept image may have (a larger one is drawn each time, as before).
pub(crate) const MAX_ENTRY_PIXELS: f64 = 16_777_216.0;
/// Pixels all kept images may have together (128 MiB).
const MAX_PIXELS: u64 = 32 << 20;
/// Renders an image not drawn in is kept for (it may scroll back into view).
const KEEP_FRAMES: u64 = 2;

/// Which effect of which object, shown where, drawn for which plate (the CMYK blend passes draw
/// each object twice, once per plate).
pub(crate) type Key = (ItemId, ShownOn, Slot, Option<u8>);

/// The effects of one object an image may be kept for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Slot {
    OuterGlow,
    DropShadow,
    Feather,
    InnerShadow,
    InnerGlow,
    Satin,
    BevelHighlight,
    BevelShadow,
}

/// What a kept image was drawn from, compared with what is drawn now.
pub(crate) struct Source<'a> {
    pub item: &'a Item,
    /// The parent transform the object was drawn with.
    pub xf: Affine,
    /// Spread space to device pixels.
    pub view: Affine,
    pub px: f64,
    /// The composed stories of the text frames in the object.
    pub stories: Vec<Arc<ComposedStory>>,
    /// The device area the image must cover now (a large object's image covers only what was on
    /// screen when it was drawn, and a margin).
    pub needed: Rect,
}

struct Entry {
    opts: RenderOptions,
    item: Item,
    xf: Affine,
    linear: [f64; 4],
    px: f64,
    translation: Vec2,
    stories: Vec<Arc<ComposedStory>>,
    /// Where the image went on the device, and how many device pixels one of its pixels covers.
    x0: f64,
    y0: f64,
    scale: f64,
    image: FxImage,
    used: u64,
}

/// An effect image: one or more bands of rows, drawn on their own (on several cores) and placed
/// side by side.
#[derive(Clone)]
pub(crate) struct FxImage {
    /// Its width (pixels).
    pub width: u16,
    /// Its height (rows) in all.
    pub height: u32,
    pub bands: Vec<Band>,
}

/// Rows `core` of an [`FxImage`], from `image` whose first row is the image's row `top`. The
/// image reaches past the core by the blur's spread, so each band is blurred as the whole is.
#[derive(Clone)]
pub(crate) struct Band {
    pub image: Arc<Pixmap>,
    pub top: u32,
    pub core: (u32, u32),
}

impl FxImage {
    fn pixels(&self) -> u64 {
        self.bands.iter().map(|b| u64::from(b.image.width()) * u64::from(b.image.height())).sum()
    }
}

/// What every kept image depends on beyond its own object.
struct Deps {
    cms: Arc<Cms>,
    swatches: Vec<Swatch>,
    settings: DocSettings,
    inks: InkManager,
    stroke_styles: Vec<StrokeStyleDef>,
    layers: Vec<Layer>,
    assets: Vec<(AssetId, Arc<Asset>)>,
}

impl Deps {
    fn of(doc: &Document) -> Self {
        Deps {
            cms: designcraft_color::cms::active(),
            swatches: doc.swatches.clone(),
            settings: doc.settings.clone(),
            inks: doc.inks.clone(),
            stroke_styles: doc.stroke_styles.clone(),
            layers: doc.layers.clone(),
            assets: doc.assets.iter().map(|(id, a)| (*id, a.clone())).collect(),
        }
    }

    fn matches(&self, doc: &Document) -> bool {
        Arc::ptr_eq(&self.cms, &designcraft_color::cms::active())
            && self.swatches == doc.swatches
            && self.settings == doc.settings
            && self.inks == doc.inks
            && self.stroke_styles == doc.stroke_styles
            && self.layers == doc.layers
            && self.assets.len() == doc.assets.len()
            && self.assets.iter().zip(&doc.assets).all(|((id, a), (id2, b))| id == id2 && Arc::ptr_eq(a, b))
    }
}

/// An image being drawn, to keep once it is ([`FxCache::prepare`], [`FxCache::put`]).
pub(crate) struct Prepared {
    key: Key,
    opts: RenderOptions,
    item: Item,
    xf: Affine,
    linear: [f64; 4],
    px: f64,
    translation: Vec2,
    stories: Vec<Arc<ComposedStory>>,
    x0: f64,
    y0: f64,
    scale: f64,
}

/// What [`FxCache::get`] found.
pub(crate) enum Lookup {
    Hit(Placed),
    /// Nothing to place; `zoomed` when an image was kept for the object at another zoom or rotation
    /// in the last render (it is being zoomed: the next render draws it anew too).
    Miss {
        zoomed: bool,
    },
}

/// A kept image to place: the image, its top left on the device now, and its pixel scale.
pub(crate) struct Placed {
    pub image: FxImage,
    pub x0: f64,
    pub y0: f64,
    pub scale: f64,
}

#[derive(Default)]
pub(crate) struct FxCache {
    entries: HashMap<Key, Entry>,
    deps: Option<Deps>,
    pixels: u64,
    frame: u64,
}

fn linear(a: Affine) -> [f64; 4] {
    let [a, b, c, d, _, _] = a.as_coeffs();
    [a, b, c, d]
}

impl FxCache {
    /// A render of `doc` begins: images not drawn in the last few are let go, and all of them
    /// once something every one depends on changed.
    pub(crate) fn next_frame(&mut self, doc: &Document) {
        if !self.deps.as_ref().is_some_and(|d| d.matches(doc)) {
            self.clear();
            self.deps = Some(Deps::of(doc));
        }
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        let before = self.entries.len();
        self.entries.retain(|_, e| e.used.saturating_add(KEEP_FRAMES) >= frame);
        if self.entries.len() != before {
            self.pixels = self.entries.values().map(|e| e.image.pixels()).sum();
        }
    }

    /// Forget everything.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.pixels = 0;
    }

    /// The kept image for `key` where `src` is what it was drawn from, placed for `src.view`.
    pub(crate) fn get(&mut self, key: Key, src: &Source, opts: &RenderOptions) -> Lookup {
        let Some(e) = self.entries.get_mut(&key) else { return Lookup::Miss { zoomed: false } };
        if e.linear != linear(src.view) || e.px != src.px {
            return Lookup::Miss { zoomed: e.used.saturating_add(1) >= self.frame };
        }
        let same = e.opts == *opts
            && e.xf == src.xf
            && e.stories.len() == src.stories.len()
            && e.stories.iter().zip(&src.stories).all(|(a, b)| Arc::ptr_eq(a, b))
            && e.item == *src.item;
        if !same {
            return Lookup::Miss { zoomed: false };
        }
        // Panning moves the device position only, as long as the image still covers what shows.
        let t = src.view.translation();
        let (x0, y0) = (e.x0 + (t.x - e.translation.x), e.y0 + (t.y - e.translation.y));
        let (w, h) = (f64::from(e.image.width) * e.scale, f64::from(e.image.height) * e.scale);
        let n = src.needed;
        if n.x0 < x0 - 0.5 || n.y0 < y0 - 0.5 || n.x1 > x0 + w + 0.5 || n.y1 > y0 + h + 0.5 {
            return Lookup::Miss { zoomed: false };
        }
        e.used = self.frame;
        Lookup::Hit(Placed { image: e.image.clone(), x0, y0, scale: e.scale })
    }

    /// What to keep of an image about to be drawn from `src` with its top left at (`x0`, `y0`) on
    /// the device; [`Self::put`] keeps it once it is drawn.
    pub(crate) fn prepare(key: Key, src: Source, opts: &RenderOptions, x0: f64, y0: f64, scale: f64) -> Prepared {
        Prepared {
            key,
            opts: opts.clone(),
            item: src.item.clone(),
            xf: src.xf,
            linear: linear(src.view),
            px: src.px,
            translation: src.view.translation(),
            stories: src.stories,
            x0,
            y0,
            scale,
        }
    }

    /// Keep `image`, drawn as `p` describes.
    pub(crate) fn put(&mut self, p: Prepared, image: FxImage) {
        let size = image.pixels();
        if let Some(old) = self.entries.remove(&p.key) {
            self.pixels = self.pixels.saturating_sub(old.image.pixels());
        }
        if self.pixels.saturating_add(size) > MAX_PIXELS {
            // Make room: first what wasn't drawn lately, then everything else.
            let frame = self.frame;
            self.entries.retain(|_, e| e.used.saturating_add(KEEP_FRAMES) >= frame);
            self.pixels = self.entries.values().map(|e| e.image.pixels()).sum();
            if self.pixels.saturating_add(size) > MAX_PIXELS {
                self.clear();
            }
        }
        if size > MAX_PIXELS {
            return;
        }
        self.pixels += size;
        let Prepared { key, opts, item, xf, linear, px, translation, stories, x0, y0, scale } = p;
        self.entries.insert(key, Entry { opts, item, xf, linear, px, translation, stories, x0, y0, scale, image, used: self.frame });
    }
}
