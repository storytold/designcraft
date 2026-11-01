//! Separations Preview: the plates the print PDF puts on film, read back from that PDF.
//!
//! A spread is exported as Export PDF writes it (one PDF page for the spread, with the
//! document bleed) into memory, and `craft_prepress::plates` separates that page: one 8-bit
//! plane per ink, the four process inks and every spot ink the page paints, after the Ink
//! Manager's aliases and spots to process. The preview shows those plates, so it shows what
//! the PDF prints, not what the document model intends.

use std::sync::Arc;

use craft_prepress::{CmsConversion, Fallback, InkId, Options, Plates, Process};
use designcraft_compose::Cache;
use designcraft_doc::{Document, PageSide};
use designcraft_geom::Rect;
use designcraft_pdf::PdfOptions;
pub use designcraft_pdf::Standard;

use crate::{EngineError, Result};

/// The highest resolution a preview separates at, in pixels per inch. craft-prepress further
/// lowers it to fit 4 megapixels per plate.
pub const MAX_DPI: f32 = 300.0;
/// The lowest resolution a preview separates at.
pub const MIN_DPI: f32 = 9.0;
/// The color over-limit areas show in, sRGB.
pub const OVER_LIMIT: [u8; 3] = [230, 40, 40];

/// A spread separated into plates.
pub struct Sheet {
    pub spread: usize,
    /// The PDF page's area in spread coordinates: the spread's trim plus the document bleed.
    pub area: Rect,
    pub plates: Plates,
    /// The printing condition the composite converts process plates through.
    cms: CmsConversion,
    /// Export warnings.
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for Sheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sheet").field("spread", &self.spread).field("area", &self.area).field("plates", &self.plates).finish_non_exhaustive()
    }
}

/// The area spread `si`'s print PDF page covers, in spread coordinates (the trim and the bleed,
/// as the PDF writer lays out a spread sheet).
fn sheet_area(doc: &Document, si: usize) -> Option<Rect> {
    let sp = doc.spreads.get(si)?;
    let b = doc.settings.bleed.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    // Bleed is top, bottom, inside, outside; single-sided pages read inside as left.
    let lr = |side: PageSide| if side == PageSide::Left { (b[3], b[2]) } else { (b[2], b[3]) };
    let outer = sp.outer_page_indices();
    let l = outer.and_then(|(l, _)| sp.pages.get(l)).map_or(b[2], |p| lr(p.side).0);
    let r = outer.and_then(|(_, r)| sp.pages.get(r)).map_or(b[3], |p| lr(p.side).1);
    let trim = sp.bounds();
    Some(Rect::new(trim.x0 - l, trim.y0 - b[0], trim.x1 + r, trim.y1 + b[1]))
}

/// The print profile: the working CMYK space's output intent, as PDF/X-4 export embeds it.
fn printing_condition() -> (Fallback, CmsConversion) {
    let Ok(intent) = designcraft_color::cms::active().output_intent() else { return (Fallback::Fogra39, CmsConversion::fogra39()) };
    match CmsConversion::new(&intent.icc) {
        Ok(cms) => (Fallback::Profile(Arc::from(intent.icc.as_ref())), cms),
        Err(_) => (Fallback::Fogra39, CmsConversion::fogra39()),
    }
}

/// The Export PDF settings that change what a page prints.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Print {
    pub standard: Standard,
    pub compress_images: bool,
    /// The transparency flattener's resolution, in ppi.
    pub flatten: Option<f64>,
}

/// Exports spread `si` as Export PDF prints it with `print` (one page for the spread, with
/// the document bleed, without printer's marks) and separates it at `dpi`, clamped to
/// [`MIN_DPI`]–[`MAX_DPI`].
pub fn separate(doc: &Document, cache: &Cache, si: usize, dpi: f32, print: &Print) -> Result<Sheet> {
    let sp = doc.spreads.get(si).ok_or_else(|| EngineError::Other(format!("spread {si} does not exist")))?;
    let first = doc.first_page_of_spread(si);
    let pages: Vec<usize> = (first..first.saturating_add(sp.pages.len())).collect();
    let area = sheet_area(doc, si).ok_or_else(|| EngineError::Other(format!("spread {si} does not exist")))?;
    let opts = PdfOptions {
        pages: Some(pages),
        spreads: true,
        bleed: true,
        standard: print.standard,
        compress_images: print.compress_images,
        ..PdfOptions::default()
    };
    let report = crate::cmd::print_pdf(doc, cache, &opts, print.flatten)?;
    let dpi = if dpi.is_finite() { dpi.clamp(MIN_DPI, MAX_DPI) } else { MIN_DPI };
    let (fallback, cms) = printing_condition();
    let plates = craft_prepress::plates(&report.bytes, 0, dpi, &Options { overprint: true, fallback })
        .map_err(|e| EngineError::Other(format!("separations: {e}")))?;
    Ok(Sheet { spread: si, area, plates, cms, warnings: report.warnings })
}

impl Sheet {
    /// The ink a plate name denotes: a process ink by its name in any case (`cyan`), or a spot
    /// ink by name.
    pub fn ink(&self, name: &str) -> Option<InkId> {
        Process::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name)).map(InkId::process).or_else(|| self.plates.inks.lookup(name))
    }

    /// Every plate: name and whether it's a spot ink, process inks first.
    pub fn inks(&self) -> Vec<(String, bool)> {
        self.plates.inks.iter().map(|(_, ink)| (ink.name().to_string(), ink.is_spot())).collect()
    }

    /// The preview as opaque RGBA8 (`plates.width` × `plates.height`): every ink composited,
    /// or with `plate` one ink alone as gray film (black is 100%). With `ink_limit` (percent of
    /// total ink) the image is gray and the areas over the limit show in [`OVER_LIMIT`].
    pub fn image(&self, plate: Option<InkId>, ink_limit: Option<f32>) -> Vec<u8> {
        let p = &self.plates;
        let mut rgb = match plate.and_then(|id| p.plane(id)) {
            Some(plane) => plane.iter().flat_map(|t| [255 - t; 3]).collect(),
            None => p.composite(&[], &self.cms),
        };
        if let Some(limit) = ink_limit.filter(|l| l.is_finite()) {
            for px in rgb.as_chunks_mut::<3>().0 {
                let [r, g, b] = px.map(u32::from);
                let y = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
                *px = [y; 3];
            }
            p.above(limit, &[]).paint(&mut rgb, OVER_LIMIT);
        }
        rgb.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect()
    }

    /// Converts a pixel position to spread coordinates.
    pub fn to_spread(&self, x: u32, y: u32) -> designcraft_geom::Point {
        let k = 72.0 / f64::from(self.plates.dpi.max(f32::MIN_POSITIVE));
        designcraft_geom::Point::new(self.area.x0 + f64::from(x) * k, self.area.y0 + f64::from(y) * k)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Session;

    /// A letter page with a Gold spot square at (72, 72), a cyan square at (300, 72) and a
    /// 400% square at (72, 400), each 100 pt.
    fn document() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("swatch.create", &json!({"name": "Gold", "spot": true, "color": {"c": 0, "m": 20, "y": 80, "k": 10}})).unwrap();
        s.execute("swatch.create", &json!({"name": "Rich", "color": {"c": 100, "m": 100, "y": 100, "k": 100}})).unwrap();
        for (rect, swatch) in [([72, 72, 172, 172], "Gold"), ([300, 72, 400, 172], "C=100 M=0 Y=0 K=0"), ([72, 400, 172, 500], "Rich")] {
            let id = s.execute("frame.create", &json!({"rect": rect})).unwrap()["id"].clone();
            s.execute("object.fill", &json!({"swatch": swatch, "ids": [id]})).unwrap();
        }
        s
    }

    fn sheet(s: &Session) -> Sheet {
        separate(&s.doc().unwrap().doc, &s.cache, 0, 36.0, &Print::default()).unwrap()
    }

    /// The tint of `ink` at spread point (`x`, `y`).
    fn tint(sheet: &Sheet, ink: &str, x: f64, y: f64) -> u8 {
        let k = f64::from(sheet.plates.dpi) / 72.0;
        let (px, py) = (((x - sheet.area.x0) * k) as u32, ((y - sheet.area.y0) * k) as u32);
        sheet.plates.tint(sheet.ink(ink).unwrap(), px, py).unwrap()
    }

    #[test]
    fn the_plates_are_the_process_inks_and_the_spot_and_the_spot_plate_shows_only_the_spot() {
        let s = document();
        let sheet = sheet(&s);
        let names: Vec<(String, bool)> = sheet.inks();
        let expected: Vec<(String, bool)> =
            [("Cyan", false), ("Magenta", false), ("Yellow", false), ("Black", false), ("Gold", true)].map(|(n, sp)| (n.to_string(), sp)).into();
        assert_eq!(names, expected);
        assert!(tint(&sheet, "Gold", 122.0, 122.0) > 250, "Gold under the Gold square");
        assert_eq!(tint(&sheet, "Gold", 350.0, 122.0), 0, "no Gold under the cyan square");
        assert_eq!(tint(&sheet, "Gold", 122.0, 450.0), 0, "no Gold under the 400% square");
        assert_eq!(tint(&sheet, "cyan", 122.0, 122.0), 0, "the spot isn't separated into process");
        assert!(tint(&sheet, "cyan", 350.0, 122.0) > 250);

        // The Gold plate alone: film only where the Gold square is.
        let img = sheet.image(sheet.ink("Gold"), None);
        let at = |x: f64, y: f64| {
            let k = f64::from(sheet.plates.dpi) / 72.0;
            let i = (((y - sheet.area.y0) * k) as usize * sheet.plates.width as usize + ((x - sheet.area.x0) * k) as usize) * 4;
            [img[i], img[i + 1], img[i + 2]]
        };
        assert!(at(122.0, 122.0).iter().all(|v| *v < 5), "{:?}", at(122.0, 122.0));
        assert_eq!(at(350.0, 122.0), [255; 3]);
        assert_eq!(at(122.0, 450.0), [255; 3]);
        let inked = img.as_chunks::<4>().0.iter().filter(|px| px[0] < 128).count();
        let square = (100.0 * f64::from(sheet.plates.dpi) / 72.0).powi(2);
        assert!((inked as f64 - square).abs() < square * 0.1, "{inked} dark pixels, the square has {square}");
    }

    #[test]
    fn ink_limit_flags_the_400_percent_area() {
        let mut s = document();
        let r = s.execute("ink.separations", &json!({"dpi": 36, "inkLimit": 300})).unwrap();
        assert!(r["maxTotal"].as_f64().unwrap() > 399.0, "{r}");
        let regions = r["overLimit"]["regions"].as_array().unwrap();
        assert_eq!(regions.len(), 1, "{r}");
        let (x, y) = (regions[0]["x"].as_f64().unwrap(), regions[0]["y"].as_f64().unwrap());
        assert!((x - 72.0).abs() < 3.0 && (y - 400.0).abs() < 3.0, "the 400% square: {r}");
        assert!(r["inks"].as_array().unwrap().iter().any(|i| i["name"] == "Gold" && i["spot"] == true));

        let sheet = sheet(&s);
        let img = sheet.image(None, Some(300.0));
        let k = f64::from(sheet.plates.dpi) / 72.0;
        let at = |x: f64, y: f64| {
            let i = (((y - sheet.area.y0) * k) as usize * sheet.plates.width as usize + ((x - sheet.area.x0) * k) as usize) * 4;
            [img[i], img[i + 1], img[i + 2]]
        };
        assert_eq!(at(122.0, 450.0), OVER_LIMIT);
        let gray = at(350.0, 122.0);
        assert!(gray[0] == gray[1] && gray[1] == gray[2], "under the limit the preview is gray: {gray:?}");
    }

    #[test]
    fn a_spot_aliased_to_another_spot_prints_on_its_plate() {
        let mut s = document();
        s.execute("swatch.create", &json!({"name": "Silver", "spot": true, "color": {"c": 0, "m": 0, "y": 0, "k": 40}})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [300, 400, 400, 500]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "Silver", "ids": [id]})).unwrap();
        assert!(sheet(&s).ink("Silver").is_some());
        s.execute("ink.options", &json!({"ink": "Gold", "alias": "Silver"})).unwrap();
        let sheet = sheet(&s);
        assert!(sheet.ink("Gold").is_none(), "{:?}", sheet.inks());
        assert!(tint(&sheet, "Silver", 122.0, 122.0) > 250, "Gold's square is on the Silver plate");
        assert!(tint(&sheet, "Silver", 350.0, 450.0) > 250);
    }

    #[test]
    fn a_saved_document_with_ink_settings_loads_and_its_preview_applies_them() {
        let s = document();
        let mut v = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
        // Ink Manager settings as documents save them.
        v["inks"] = json!({"allToProcess": false, "toProcess": ["Gold"], "aliases": []});
        let doc: Document = serde_json::from_value(v).unwrap();
        assert_eq!(doc.inks.to_process, ["Gold"]);
        let sheet = separate(&doc, &s.cache, 0, 36.0, &Print::default()).unwrap();
        assert!(sheet.ink("Gold").is_none(), "Gold prints as process: {:?}", sheet.inks());
        assert!(tint(&sheet, "Yellow", 122.0, 122.0) > 150, "Gold's yellow");
    }
}
