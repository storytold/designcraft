#[cfg(not(target_arch = "wasm32"))]
use super::EngineError;
use super::bytes::{base64_encode, from_bytes, to_bytes};
use super::place::read_source;
use super::{ItemId, Rect, Result, Selection, Session, Value, bad, json, str_param};

/// The selection as `.designcraft` snippet bytes.
pub(crate) fn snippet_bytes(s: &mut Session) -> Result<Vec<u8>> {
    to_bytes(&crate::cmd::edit::clip_doc(s)?)
}

pub(super) fn snippet_export(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::edit::clip_doc(s)?;
    let bytes = to_bytes(&d)?;
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": bytes.len(), "items": d.spreads.first().map(|s| s.items.len()).unwrap_or(0)}))
        }
        None => Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()})),
    }
}

pub(crate) fn snippet_place(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, _, _) = read_source(p)?;
    let src = from_bytes(&bytes)?;
    let ids: Vec<ItemId> = src.spreads.first().map(|sp| sp.items.iter().map(|i| i.id).collect()).unwrap_or_default();
    if ids.is_empty() {
        return Err(bad("snippet.place", "the snippet is empty"));
    }
    let sr = crate::cmd::spread_param(p, "snippet.place")?;
    let bounds = ids.iter().filter_map(|i| src.item(*i).map(|it| it.bounds())).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
    let off = match (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => designcraft_geom::Vec2::new(x - bounds.x0, y - bounds.y0),
        _ => designcraft_geom::Vec2::ZERO,
    };
    s.edit(|d, sel| {
        // Bring over styles and swatches the snippet uses that this document lacks.
        for sw in &src.swatches {
            if d.swatch(&sw.name).is_none() {
                d.swatches.push(sw.clone());
            }
        }
        let missing_p: Vec<_> = src.styles.paragraph.iter().filter(|ps| d.styles.para(&ps.name).is_none()).cloned().collect();
        let missing_c: Vec<_> = src.styles.character.iter().filter(|cs| d.styles.char_style(&cs.name).is_none()).cloned().collect();
        if !missing_p.is_empty() || !missing_c.is_empty() {
            let st = d.styles_mut();
            st.paragraph.extend(missing_p);
            st.character.extend(missing_c);
        }
        let new = crate::cmd::object::duplicate_from(d, &src, &ids, sr, off)?;
        *sel = Selection::items(new.clone());
        Ok(json!({"ids": new.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}
