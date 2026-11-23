//! File › Export Imposed PDF: copies of pages n-up on press sheets, with bleed and crop marks
//! (`file.imposedLayout` tells where they go, `file.exportImposedPdf` writes the file).

use designcraft_geom::impose::{CropMarks, Rotation, Spec};
use designcraft_geom::units::PT_PER_MM;
use designcraft_geom::{Rect, Size, Unit, parse_measure};
use designcraft_pdf::{ImposeOptions, MAX_COPIES, PdfError};

use super::pdf::pages_param;
use super::{EngineError, Result, Session, Value, bad, json, str_param};

const MM: f64 = PT_PER_MM;

/// Press sheet presets: name, then width and height in points, portrait.
pub const SHEET_SIZES: &[(&str, f64, f64)] = &[
    ("A5", 148.0 * MM, 210.0 * MM),
    ("A4", 210.0 * MM, 297.0 * MM),
    ("A3", 297.0 * MM, 420.0 * MM),
    ("A2", 420.0 * MM, 594.0 * MM),
    ("SRA4", 225.0 * MM, 320.0 * MM),
    ("SRA3", 320.0 * MM, 450.0 * MM),
    ("SRA2", 450.0 * MM, 640.0 * MM),
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("Tabloid", 792.0, 1224.0),
    ("12 × 18 in", 864.0, 1296.0),
];

/// A length: a number of points or a measurement (`"3mm"`, `"1p6"`).
fn measure(id: &str, key: &str, v: &Value) -> Result<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => parse_measure(s, Unit::Points).ok(),
        _ => None,
    };
    n.filter(|n| n.is_finite()).ok_or_else(|| bad(id, format!("bad {key} `{v}`")))
}

/// One length for every side, or one per side.
fn sides<const N: usize>(id: &str, key: &str, p: &Value) -> Result<[f64; N]> {
    match p.get(key) {
        None | Some(Value::Null) => Ok([0.0; N]),
        Some(Value::Array(a)) if a.len() == N => {
            let mut out = [0.0; N];
            for (o, v) in out.iter_mut().zip(a) {
                *o = measure(id, key, v)?;
            }
            Ok(out)
        }
        Some(Value::Array(_)) => Err(bad(id, format!("{key} takes one length or {N}"))),
        Some(v) => Ok([measure(id, key, v)?; N]),
    }
}

fn sheet_size(id: &str, p: &Value) -> Result<(f64, f64)> {
    let (w, h) = match p.get("sheet") {
        None | Some(Value::Null) => (210.0 * MM, 297.0 * MM),
        Some(Value::String(name)) => SHEET_SIZES
            .iter()
            .find(|(n, ..)| n.eq_ignore_ascii_case(name))
            .map(|(_, w, h)| (*w, *h))
            .ok_or_else(|| bad(id, format!("unknown sheet `{name}` ({})", SHEET_SIZES.iter().map(|(n, ..)| *n).collect::<Vec<_>>().join(", "))))?,
        Some(Value::Array(a)) if a.len() == 2 => {
            let side = |i: usize| a.get(i).map_or_else(|| Err(bad(id, "sheet takes [width, height]")), |v| measure(id, "sheet", v));
            (side(0)?, side(1)?)
        }
        Some(v) => return Err(bad(id, format!("bad sheet `{v}`"))),
    };
    Ok(match str_param(p, "orientation") {
        None => (w, h),
        Some("portrait") => (w.min(h), w.max(h)),
        Some("landscape") => (w.max(h), w.min(h)),
        Some(o) => return Err(bad(id, format!("unknown orientation `{o}` (portrait, landscape)"))),
    })
}

/// The command's parameters as export settings.
fn impose_options(id: &str, s: &Session, p: &Value) -> Result<ImposeOptions> {
    let (w, h) = sheet_size(id, p)?;
    let rotation = match str_param(p, "rotation").unwrap_or("auto") {
        "auto" => Rotation::Auto,
        "off" => Rotation::Off,
        "on" => Rotation::On,
        r => return Err(bad(id, format!("unknown rotation `{r}` (auto, off, on)"))),
    };
    let crop_marks = match str_param(p, "cropMarks").unwrap_or("none") {
        "none" => CropMarks::None,
        "corners" => CropMarks::Corners,
        "border" => CropMarks::Border,
        m => return Err(bad(id, format!("unknown cropMarks `{m}` (none, corners, border)"))),
    };
    let copies = match p.get("copies") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_COPIES).contains(n))
            .ok_or_else(|| bad(id, format!("copies must be 1 to {MAX_COPIES}, not {v}")))?,
    };
    let length = |key: &str, default: f64| p.get(key).filter(|v| !v.is_null()).map_or(Ok(default), |v| measure(id, key, v));
    let doc = &s.doc()?.doc;
    Ok(ImposeOptions {
        spec: Spec {
            sheet: Size::new(w, h),
            margins: sides::<4>(id, "margins", p)?,
            gap: sides::<2>(id, "gap", p)?,
            rotation,
            center: p.get("center").and_then(Value::as_bool).unwrap_or(true),
        },
        pages: pages_param(id, p, doc.page_count())?,
        copies,
        bleed: p.get("bleed").and_then(Value::as_bool).unwrap_or(false),
        crop_marks,
        mark_weight: length("marksWeight", 0.25)?,
        mark_offset: length("marksOffset", 6.0)?,
        title: str_param(p, "title").map(str::to_owned),
    })
}

fn pdf_error(id: &str, e: PdfError) -> EngineError {
    match e {
        PdfError::Write(_) => EngineError::Other(e.to_string()),
        _ => bad(id, e.to_string()),
    }
}

fn rect(r: Rect) -> Value {
    json!([r.x0, r.y0, r.x1, r.y1])
}

/// `file.imposedLayout`: the grid, the sheet count, and the copies and crop marks of one sheet.
pub(super) fn imposed_layout(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.imposedLayout";
    let o = impose_options(ID, s, p)?;
    let im = designcraft_pdf::impose(&s.doc()?.doc, &o).map_err(|e| pdf_error(ID, e))?;
    let sheet = match p.get("sheetIndex") {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n < im.sheets)
            .ok_or_else(|| bad(ID, format!("sheetIndex {v}: there are {} sheets (0-based)", im.sheets)))?,
    };
    let copies: Vec<Value> = im
        .on_sheet(sheet)
        .map(|(abs, pl)| json!({"page": abs.saturating_add(1), "slot": pl.slot, "trim": rect(pl.trim), "clip": rect(pl.clip), "rotated": pl.rotated}))
        .collect();
    let marks: Vec<Value> = im.marks(sheet, o.crop_marks, o.mark_offset).iter().map(|l| json!([l.p0.x, l.p0.y, l.p1.x, l.p1.y])).collect();
    Ok(json!({
        "sheet": [im.spec.sheet.width, im.spec.sheet.height],
        "page": [im.page.width, im.page.height],
        "cols": im.grid.cols,
        "rows": im.grid.rows,
        "rotated": im.grid.rotated,
        "perSheet": im.grid.per_sheet(),
        "copies": im.copies.len(),
        "sheets": im.sheets,
        "bleed": im.bleed,
        "sheetIndex": sheet,
        "placements": copies,
        "marks": marks,
    }))
}

/// `file.exportImposedPdf`.
pub(super) fn export_imposed_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.exportImposedPdf";
    let o = impose_options(ID, s, p)?;
    let st = s.doc()?;
    let r = designcraft_pdf::export_imposed(&crate::cmd::interchange::with_pdf_boxes(&st.doc), &s.cache, &o).map_err(|e| pdf_error(ID, e))?;
    let per = designcraft_pdf::impose(&st.doc, &o).map(|im| im.grid.per_sheet()).unwrap_or(0);
    match str_param(p, "path") {
        Some(path) => {
            super::formats::write_file(path, &r.bytes)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "sheets": r.pages, "perSheet": per, "warnings": r.warnings}))
        }
        None => Ok(
            json!({"base64": crate::cmd::file::base64_encode(&r.bytes), "bytes": r.bytes.len(), "sheets": r.pages, "perSheet": per, "warnings": r.warnings}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn session(w: f64, h: f64, pages: usize) -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": pages, "facingPages": false, "width": w, "height": h})).unwrap();
        s
    }

    const MM: f64 = designcraft_geom::units::PT_PER_MM;

    #[test]
    fn layout_counts_copies_per_sheet_and_sheets() {
        // A6 on A4: four a sheet; eight copies take two sheets.
        let mut s = session(105.0 * MM, 148.0 * MM, 1);
        let r = s.execute("file.imposedLayout", &json!({"sheet": "A4"})).unwrap();
        assert_eq!((r["perSheet"].as_u64(), r["sheets"].as_u64(), r["rotated"].as_bool()), (Some(4), Some(1), Some(false)), "{r}");
        let r = s.execute("file.imposedLayout", &json!({"sheet": "A4", "copies": 8, "sheetIndex": 1})).unwrap();
        assert_eq!((r["sheets"].as_u64(), r["copies"].as_u64()), (Some(2), Some(8)));
        assert_eq!(r["placements"].as_array().map(Vec::len), Some(4));
        assert!(s.execute("file.imposedLayout", &json!({"sheet": "A4", "copies": 8, "sheetIndex": 2})).is_err());
        // Two pages, three copies each: page 1 fills the first three places, page 2 follows.
        let mut s = session(105.0 * MM, 148.0 * MM, 2);
        let r = s.execute("file.imposedLayout", &json!({"copies": 3})).unwrap();
        let pages: Vec<u64> = r["placements"].as_array().unwrap().iter().filter_map(|p| p["page"].as_u64()).collect();
        assert_eq!(pages, [1, 1, 1, 2]);
        assert_eq!(r["sheets"], 2);
        let r = s.execute("file.imposedLayout", &json!({"copies": 3, "pages": "2"})).unwrap();
        assert_eq!(r["sheets"], 1);
    }

    #[test]
    fn auto_rotation_picks_the_better_fit() {
        // 88 × 60 mm on A4 without gaps: 8 upright (2 × 4) or 9 rotated (3 × 3).
        let mut s = session(88.0 * MM, 60.0 * MM, 1);
        let r = s.execute("file.imposedLayout", &json!({"sheet": "A4"})).unwrap();
        assert_eq!((r["cols"].as_u64(), r["rows"].as_u64(), r["rotated"].as_bool()), (Some(3), Some(3), Some(true)));
        let r = s.execute("file.imposedLayout", &json!({"sheet": "A4", "rotation": "off"})).unwrap();
        assert_eq!((r["cols"].as_u64(), r["rows"].as_u64(), r["rotated"].as_bool()), (Some(2), Some(4), Some(false)));
        // Landscape A4 with 10 mm margins and 3 mm gaps, as measurements.
        let r =
            s.execute("file.imposedLayout", &json!({"sheet": "A4", "orientation": "landscape", "margins": "10mm", "gap": ["3mm", "3mm"]})).unwrap();
        let sheet = r["sheet"].as_array().unwrap().iter().filter_map(serde_json::Value::as_f64).collect::<Vec<_>>();
        assert!(sheet[0] > sheet[1], "{sheet:?}");
        // 277 × 190 mm: upright 3 × 3 = 9 (88·3 + 6 = 270, 60·3 + 6 = 186), turned 4 × 2 = 8.
        assert_eq!((r["perSheet"].as_u64(), r["rotated"].as_bool()), (Some(9), Some(false)), "{r}");
    }

    #[test]
    fn bad_parameters_are_refused() {
        let mut s = session(300.0, 200.0, 1);
        for p in [
            json!({"sheet": "B12"}),
            json!({"sheet": [100, 100]}),
            json!({"sheet": [100]}),
            json!({"rotation": "sideways"}),
            json!({"cropMarks": "all"}),
            json!({"copies": 0}),
            json!({"copies": 1_000_000}),
            json!({"margins": [1, 2]}),
            json!({"margins": -5}),
            json!({"gap": "wide"}),
            json!({"orientation": "upside"}),
            json!({"pages": "4"}),
        ] {
            assert!(s.execute("file.imposedLayout", &p).is_err(), "{p}");
            assert!(s.execute("file.exportImposedPdf", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn export_writes_a_page_per_sheet() {
        let mut s = session(105.0 * MM, 148.0 * MM, 1);
        let r = s.execute("file.exportImposedPdf", &json!({"sheet": "A4", "copies": 8, "cropMarks": "border"})).unwrap();
        assert_eq!((r["sheets"].as_u64(), r["perSheet"].as_u64()), (Some(2), Some(4)), "{r}");
        let bytes = crate::cmd::file::base64_decode(r["base64"].as_str().unwrap());
        let pdf = String::from_utf8_lossy(&bytes).into_owned();
        assert_eq!(pdf.matches("/MediaBox").count(), 2, "one page a sheet");
        let mb = pdf.find("/MediaBox").map(|i| pdf[i..i + 40].to_string()).unwrap_or_default();
        assert!(mb.contains("595.") && mb.contains("841.8"), "A4 sheets: {mb}");
    }
}
