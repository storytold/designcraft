//! Local, deterministic palettes from placed raster images. No network service is involved.
use designcraft_color::{Color, ColorType, Swatch, SwatchValue};
use designcraft_doc::{ColorGroup, ItemId, Styles};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "color.extractTheme", "Extract Color Theme", [], None,
            "{id?: selected image frame} → {from, colors: [up to five RGB hex colours]} — samples the source raster, ignoring transparent pixels; at most 16 million pixels",
            has_doc, extract),
        cmd!(
            "swatch.addColorTheme",
            "Color Theme…",
            [],
            None,
            "{colors: [one to five RGB hex colours], name?: group name} → {name, swatches} — adds the previewed palette as one undoable swatch group",
            has_doc,
            add
        ),
    ]
}

fn extract(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "color.extractTheme";
    let st = s.doc()?;
    let id = match p.get("id") {
        Some(v) => ItemId(v.as_u64().ok_or_else(|| bad(ID, "id must be an image frame id"))?),
        None => {
            if st.selection.items.len() != 1 {
                return Err(bad(ID, "select one placed raster image"));
            }
            st.selection.items.first().copied().ok_or_else(|| bad(ID, "select one placed raster image"))?
        }
    };
    let graphic = st.doc.item(id).and_then(|it| it.graphic()).ok_or_else(|| bad(ID, "select one placed raster image"))?;
    let asset = st.doc.assets.get(&graphic.asset).filter(|a| a.is_raster()).ok_or_else(|| bad(ID, "a raster image is required"))?;
    let (width, height) = designcraft_images::pixel_size(&asset.data).ok_or_else(|| bad(ID, "cannot read image dimensions"))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(bad(ID, "the image must contain between 1 and 16 million pixels"));
    }
    let img = designcraft_images::decode_rgba(&asset.data).ok_or_else(|| bad(ID, "cannot decode this image"))?;
    // A regular 128 × 128 grid bounds work and includes both axes even for very thin images.
    let (nx, ny) = (width.min(128), height.min(128));
    let mut samples = Vec::with_capacity((nx * ny) as usize);
    for y in 0..ny {
        for x in 0..nx {
            let px = ((u64::from(x) * 2 + 1) * u64::from(width) / (u64::from(nx) * 2)) as u32;
            let py = ((u64::from(y) * 2 + 1) * u64::from(height) / (u64::from(ny) * 2)) as u32;
            if let Some(pixel) = img.get_pixel_checked(px, py) {
                let [r, g, b, a] = pixel.0;
                if a > 0 {
                    samples.push(([r, g, b], u64::from(a)));
                }
            }
        }
    }
    let colors = palette(samples);
    if colors.is_empty() {
        return Err(bad(ID, "the image has no visible sampled pixels"));
    }
    Ok(json!({"from": id.0, "colors": colors}))
}

type Sample = ([u8; 3], u64);

/// Weighted median cut: transparent padding never becomes a colour; partial alpha weights it.
fn palette(samples: Vec<Sample>) -> Vec<String> {
    if samples.is_empty() {
        return Vec::new();
    }
    let extent = |bucket: &[Sample]| {
        (0..3)
            .map(|axis| {
                let lo = bucket.iter().filter_map(|(rgb, _)| rgb.get(axis)).min().copied().unwrap_or(0);
                let hi = bucket.iter().filter_map(|(rgb, _)| rgb.get(axis)).max().copied().unwrap_or(0);
                (hi.saturating_sub(lo), axis)
            })
            .max()
            .unwrap_or((0, 0))
    };
    let mut buckets = vec![samples];
    while buckets.len() < 5 {
        let Some((index, (_, axis))) = buckets
            .iter()
            .enumerate()
            .map(|(i, b)| (i, extent(b)))
            .filter(|(_, (range, _))| *range > 0)
            .max_by_key(|(i, (range, _))| (u64::from(*range) * buckets.get(*i).map_or(0, |b| b.iter().map(|(_, w)| w).sum::<u64>()), *i))
        else {
            break;
        };
        let mut bucket = buckets.remove(index);
        bucket.sort_by_key(|(rgb, _)| rgb.get(axis).copied().unwrap_or(0));
        let half = bucket.iter().map(|(_, w)| w).sum::<u64>() / 2;
        let mut accumulated = 0;
        let mut split = 1;
        for (i, (_, weight)) in bucket.iter().enumerate().take(bucket.len().saturating_sub(1)) {
            accumulated += weight;
            split = i + 1;
            if accumulated >= half {
                break;
            }
        }
        let other = bucket.split_off(split);
        buckets.push(bucket);
        buckets.push(other);
    }
    let mut colors = Vec::new();
    for bucket in buckets {
        let weight = bucket.iter().map(|(_, w)| w).sum::<u64>();
        if weight == 0 {
            continue;
        }
        let average = |axis| (bucket.iter().map(|(rgb, w)| u64::from(rgb.get(axis).copied().unwrap_or(0)) * w).sum::<u64>() / weight) as u8;
        let color = Color::rgb8(average(0), average(1), average(2)).to_hex();
        if !colors.contains(&color) {
            colors.push(color);
        }
    }
    colors.sort();
    colors
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swatch.addColorTheme";
    let input = p
        .get("colors")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty() && a.len() <= 5)
        .ok_or_else(|| bad(ID, "colors must contain one to five RGB hex colours"))?;
    let mut colors = Vec::with_capacity(input.len());
    for value in input {
        let hex = value
            .as_str()
            .filter(|s| s.len() == 7 && s.starts_with('#') && s.get(1..).is_some_and(|v| v.bytes().all(|b| b.is_ascii_hexdigit())))
            .ok_or_else(|| bad(ID, "each colour must be #rrggbb"))?;
        colors.push(super::style::parse_color(&json!(hex)).ok_or_else(|| bad(ID, "invalid colour"))?);
    }
    let base = str_param(p, "name").unwrap_or("Color Theme").trim();
    if base.is_empty() || base.len() > 256 {
        return Err(bad(ID, "the group name must contain 1 to 256 bytes"));
    }
    s.edit(|doc, _| {
        let name = Styles::unique_name(|n| doc.color_groups.iter().any(|g| g.name == n), base);
        let mut names = Vec::new();
        for (i, color) in colors.iter().enumerate() {
            let label = Styles::unique_name(|n| doc.swatch(n).is_some(), &format!("{name} {}", i + 1));
            doc.swatches.push(Swatch {
                name: label.clone(),
                value: SwatchValue::Color { color: *color, color_type: ColorType::Process },
                locked: false,
                named: true,
                hidden: false,
            });
            names.push(label);
        }
        doc.color_groups.push(ColorGroup { name: name.clone(), swatches: names.clone() });
        Ok(json!({"name": name, "swatches": names}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_source_pixels_without_mutating_and_ignores_transparent_padding() {
        let png = designcraft_render::Rendered { width: 3, height: 1, pixels: vec![255, 0, 0, 255, 0, 0, 255, 255, 0, 255, 0, 0] }.to_png();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let placed = s
            .execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name":"palette.png", "x":36, "y":36, "width":144}))
            .unwrap();
        let before = s.doc().unwrap().doc.clone();
        let result = s.execute("color.extractTheme", &json!({"id":placed["id"]})).unwrap();
        assert_eq!(result["colors"], json!(["#0000ff", "#ff0000"]));
        assert_eq!(s.doc().unwrap().doc, before);
        assert!(s.execute("color.extractTheme", &json!({"id":-1})).is_err());
        assert!(s.execute("color.extractTheme", &json!({"id":999999})).is_err());
    }

    #[test]
    fn palette_is_bounded_deterministic_and_keeps_solid_colors() {
        assert_eq!(palette(vec![([12, 45, 78], 255); 100]), vec!["#0c2d4e"]);
        let samples: Vec<_> = (0..=255).map(|n| ([n, 255 - n, 45], 255)).collect();
        let result = palette(samples.clone());
        assert_eq!(result.len(), 5);
        assert_eq!(result, palette(samples));
        assert!(palette(vec![]).is_empty());
    }

    #[test]
    fn adding_theme_is_atomic_undoable_and_names_do_not_collide() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let p = json!({"colors":["#123456", "#abcdef"], "name":"Fashion"});
        let first = s.execute("swatch.addColorTheme", &p).unwrap();
        assert_eq!(first["swatches"].as_array().unwrap().len(), 2);
        let added = s.doc().unwrap().doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, added);
        let second = s.execute("swatch.addColorTheme", &p).unwrap();
        assert_ne!(first["name"], second["name"]);
        let before = s.doc().unwrap().doc.clone();
        for p in [json!({"colors":[]}), json!({"colors":["#123456", "bad"]}), json!({"colors":["#123456"],"name":" "})] {
            assert!(s.execute("swatch.addColorTheme", &p).is_err());
            assert_eq!(s.doc().unwrap().doc, before);
        }
        assert!(s.execute("color.extractTheme", &json!({})).is_err());
    }
}
