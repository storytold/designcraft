//! Window › Output › Separations Preview, for automation: the plates a spread's print PDF
//! prints, with ink coverage and the areas over an ink limit.

use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

const ID: &str = "ink.separations";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(query "ink.separations", "Separations Preview", [], None,
        "{page?: 1-based page position (default 1; its spread is separated), dpi?: 9–300 (default 72), inkLimit?: percent, standard?: none|x4|a2b, compressImages?: bool, flatten?: high|medium|low|ppi (as file.exportPdf)} — the spread exported as Export PDF prints it and separated → {spread, dpi, width, height, conversion, inks: [{name, spot, max, mean, area} (percent)], unseparated: [spot names past 32], maxTotal, maxTotalAt?: [x, y] (pt, spread), overLimit?: {limit, pixels, regions: [{x, y, width, height}] (pt, spread)}, warnings}",
        has_doc, separations)]
}

fn separations(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let d = &st.doc;
    let page = match p.get("page") {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=d.page_count()).contains(n))
            .map(|n| n - 1)
            .ok_or_else(|| bad(ID, format!("bad page {v}")))?,
    };
    let (si, _) = d.page_loc(page).ok_or_else(|| bad(ID, "the document has no pages"))?;
    let dpi = match p.get("dpi") {
        None | Some(Value::Null) => 72.0,
        Some(v) => v
            .as_f64()
            .filter(|v| (f64::from(crate::separations::MIN_DPI)..=f64::from(crate::separations::MAX_DPI)).contains(v))
            .ok_or_else(|| bad(ID, "`dpi`: 9–300"))? as f32,
    };
    let limit = match p.get("inkLimit") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| bad(ID, "`inkLimit`: a percentage"))? as f32),
    };
    let standard = match super::str_param(p, "standard") {
        Some(v) => designcraft_pdf::Standard::parse(v).ok_or_else(|| bad(ID, format!("unknown standard `{v}` (none, x4, a2b)")))?,
        None => designcraft_pdf::Standard::None,
    };
    let print = crate::separations::Print {
        standard,
        compress_images: p.get("compressImages").and_then(Value::as_bool).unwrap_or(false),
        flatten: super::export::flatten_ppi(ID, p)?,
    };
    let sheet = crate::separations::separate(d, &s.cache, si, dpi, &print)?;
    let pl = &sheet.plates;
    let stats = pl.stats(&[]);
    let inks: Vec<Value> = pl
        .inks
        .iter()
        .zip(&stats.inks)
        .map(|((_, ink), st)| json!({"name": ink.name(), "spot": ink.is_spot(), "max": st.max, "mean": st.mean, "area": st.area}))
        .collect();
    let conversion = match &pl.conversion {
        craft_prepress::Conversion::OutputIntent { profile, .. } => format!("output intent: {profile}"),
        craft_prepress::Conversion::Fallback { profile } => profile.clone(),
        craft_prepress::Conversion::DeviceFormulas => "device formulas".into(),
        // `Conversion` is non-exhaustive; a later craft-prepress conversion reports its debug form.
        other => format!("{other:?}"),
    };
    let pt = |x: u32, y: u32| {
        let p = sheet.to_spread(x, y);
        [p.x, p.y]
    };
    let mut out = json!({
        "spread": si,
        "dpi": pl.dpi,
        "width": pl.width,
        "height": pl.height,
        "conversion": conversion,
        "inks": inks,
        "unseparated": pl.unseparated,
        "maxTotal": stats.max_total,
        "maxTotalAt": stats.max_total_at.map(|(x, y)| pt(x, y)),
        "warnings": sheet.warnings,
    });
    if let Some(limit) = limit {
        let above = pl.above(limit, &[]);
        let k = 72.0 / f64::from(pl.dpi.max(f32::MIN_POSITIVE));
        let regions: Vec<Value> = above
            .regions()
            .iter()
            .map(|r| {
                let [x, y] = pt(r.x, r.y);
                json!({"x": x, "y": y, "width": f64::from(r.width) * k, "height": f64::from(r.height) * k})
            })
            .collect();
        out["overLimit"] = json!({"limit": limit, "pixels": above.count, "regions": regions});
    }
    Ok(out)
}
