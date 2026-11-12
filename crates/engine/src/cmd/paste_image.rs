//! Paste a bitmap from the system clipboard (a screenshot, an image copied in a web browser or an
//! image editor, an image file copied in a file manager) as a placed image. The pixels are
//! embedded in the document, so a save and reopen or an IDML export keeps them.

use designcraft_doc::Content;
use designcraft_geom::Point;
use serde_json::{Value, json};

use super::{CommandSpec, Result, Session, bad, cmd, has_doc, spread_param};

const ID: &str = "edit.pasteImage";

/// The most pixels a pasted image may have (8,000 × 8,000).
pub const MAX_PASTED_PIXELS: u64 = 64_000_000;

/// The largest pasted image file, in bytes.
const MAX_PASTED_BYTES: usize = 256 << 20;

/// The extensions a pasted image's name may end in.
pub const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp"];

/// The name a pasted bitmap gets in the Links panel when it comes with none.
pub const PASTED_IMAGE_NAME: &str = "Pasted image.png";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "edit.pasteImage",
        "Paste Image",
        [],
        None,
        "{base64: an image file (PNG/JPEG/GIF/WebP/TIFF/BMP) as base64, name?: its name in the Links panel ending in an image extension (default \"Pasted image.png\"), spread?, at?: [x, y] (spread coords of the image's center; default the first page's center)} → {id, asset} — embedded; a selected empty frame takes the image, otherwise a new frame holds it at one point per pixel, as File › Place sizes an image, and at most 60% of the page width",
        has_doc,
        paste_image
    )]
}

/// Whether a `width × height` bitmap is small enough to paste; the error says why not.
pub fn check_pasted_size(width: u32, height: u32) -> std::result::Result<(), String> {
    if width == 0 || height == 0 {
        return Err("the image on the clipboard has no pixels".into());
    }
    if u64::from(width) * u64::from(height) > MAX_PASTED_PIXELS {
        return Err(format!(
            "the image on the clipboard is {width} × {height} pixels; images of up to 64 million pixels can be pasted (save it and use File › Place)"
        ));
    }
    Ok(())
}

/// Whether an image file given as base64 is small enough to paste, checked before it is
/// decoded; the error says why not.
pub fn check_pasted_base64(b64: &str) -> std::result::Result<(), String> {
    check_pasted_base64_len(b64.len())
}

fn check_pasted_base64_len(len: usize) -> std::result::Result<(), String> {
    if len / 4 * 3 > MAX_PASTED_BYTES {
        return Err(format!("the image is larger than {} MB", MAX_PASTED_BYTES >> 20));
    }
    Ok(())
}

/// Unpremultiplied RGBA pixels from the clipboard, rows top to bottom, as a PNG file for
/// [`edit.pasteImage`](specs).
pub fn pasted_png(width: u32, height: u32, rgba: Vec<u8>) -> std::result::Result<Vec<u8>, String> {
    check_pasted_size(width, height)?;
    if (rgba.len() as u64) != u64::from(width) * u64::from(height) * 4 {
        return Err("the image on the clipboard has the wrong number of pixels".into());
    }
    designcraft_images::png_from_rgba(width, height, rgba).ok_or_else(|| "the image on the clipboard can't be stored".to_string())
}

/// An image file's pixels: (width, height, unpremultiplied RGBA). `None` when it is not a raster
/// DesignCraft reads or has more pixels than [`MAX_PASTED_PIXELS`].
pub fn pasted_pixels(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = raster_size(bytes)?;
    check_pasted_size(w, h).ok()?;
    let img = designcraft_images::decode_rgba(bytes)?;
    Some((img.width(), img.height(), img.into_raw()))
}

/// A raster's pixel size; `None` for vector formats (PDF, SVG, EPS) and unreadable data.
fn raster_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if designcraft_images::is_pdf(bytes) || designcraft_images::is_svg(bytes) || designcraft_images::is_eps(bytes) {
        return None;
    }
    designcraft_images::pixel_size(bytes)
}

fn paste_image(s: &mut Session, p: &Value) -> Result<Value> {
    let b64 = match p.get("base64") {
        Some(Value::String(b)) if !b.is_empty() => b,
        Some(_) => return Err(bad(ID, "`base64` must be a non-empty string")),
        None => return Err(bad(ID, "missing base64")),
    };
    check_pasted_base64(b64).map_err(|e| bad(ID, e))?;
    let name = match p.get("name") {
        None | Some(Value::Null) => PASTED_IMAGE_NAME,
        Some(Value::String(n)) if !n.trim().is_empty() => n.as_str(),
        Some(_) => return Err(bad(ID, "`name` must be a non-empty string")),
    };
    // File › Place picks text and layout import by the name's extension: keep it an image's.
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(bad(ID, format!("`name` must end in an image extension ({})", IMAGE_EXTENSIONS.join(", "))));
    }
    let bytes = super::base64_decode(b64);
    let (pw, ph) = raster_size(&bytes).ok_or_else(|| bad(ID, "the clipboard doesn't hold an image DesignCraft can read"))?;
    check_pasted_size(pw, ph).map_err(|e| bad(ID, e))?;
    // The header can be whole while the pixels after it are cut off: decode the whole raster so
    // a damaged image is refused instead of stored.
    if designcraft_images::decode_rgba(&bytes).is_none() {
        return Err(bad(ID, "the image on the clipboard is damaged and can't be read"));
    }
    let sr = spread_param(p, ID)?;
    let at = match p.get("at") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let at = super::point_param(p, "at").ok_or_else(|| bad(ID, "`at` must be [x, y]"))?;
            if !(at.x.is_finite() && at.y.is_finite()) {
                return Err(bad(ID, "`at` must be finite"));
            }
            Some(at)
        }
    };
    // A selected empty frame takes the image, as File › Place fills it.
    let st = s.doc()?;
    let empty_frame = match st.selection.items.as_slice() {
        [only] if st.doc.item(*only).is_some_and(|it| !it.locked && it.path.encloses_area() && matches!(it.content, Content::Unassigned)) => {
            Some(only.0)
        }
        _ => None,
    };
    if let Some(frame) = empty_frame {
        return place(s, &json!({"base64": b64, "name": name, "frame": frame}));
    }
    let d = &s.doc()?.doc;
    let sp = d.spread(sr).ok_or_else(|| bad(ID, "no such spread"))?;
    let center = match at {
        Some(at) => at,
        None => sp.pages.first().map_or(Point::ZERO, |pg| pg.bounds().center()),
    };
    // The size File › Place gives it: one point per pixel (`designcraft_images::natural_size`), at
    // most 60% of the page width.
    let (nw, nh) = designcraft_render::natural_size(&bytes, 0).unwrap_or((f64::from(pw), f64::from(ph)));
    let w = nw.min(d.settings.page_width * 0.6);
    let h = nh * w / nw;
    let spread = p.get("spread").cloned().unwrap_or(Value::Null);
    place(s, &json!({"base64": b64, "name": name, "spread": spread, "x": center.x - w / 2.0, "y": center.y - h / 2.0}))
}

/// Runs `file.place` inside this command. Only `edit.pasteImage` stays in the journal, so a
/// replay of the journal places the image once.
fn place(s: &mut Session, params: &Value) -> Result<Value> {
    let journal = s.journal.len();
    let r = s.execute("file.place", params);
    s.journal.truncate(journal);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::{base64_decode, base64_encode};
    use designcraft_doc::{ItemId, SpreadRef};

    /// A `w × h` PNG whose pixels count up, so a changed pixel shows.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
        pasted_png(w, h, rgba).unwrap()
    }

    fn doc() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s
    }

    fn placed_png(s: &Session, id: ItemId) -> Vec<u8> {
        let d = &s.doc().unwrap().doc;
        let Content::Graphic(g) = &d.item(id).unwrap().content else { panic!("not a graphic") };
        let a = &d.assets[&g.asset];
        assert_eq!(a.link, None, "embedded, not linked");
        a.data.to_vec()
    }

    #[test]
    fn a_pasted_bitmap_is_embedded_at_the_center_and_kept_by_save_and_idml() {
        let mut s = doc();
        let png = png(40, 30);
        let undo = s.doc().unwrap().history.undo.len();
        let r = s.execute(ID, &json!({"base64": base64_encode(&png), "at": [300, 400]})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
        let id = ItemId(r["id"].as_u64().unwrap());
        let st = s.doc().unwrap();
        assert_eq!(st.selection.items, [id], "the pasted image is selected");
        let b = st.doc.item(id).unwrap().bounds();
        assert!(
            (b.x0 - 280.0).abs() < 1e-9 && (b.y0 - 385.0).abs() < 1e-9 && (b.width() - 40.0).abs() < 1e-9 && (b.height() - 30.0).abs() < 1e-9,
            "{b:?}"
        );
        assert_eq!(placed_png(&s, id), png);
        assert_eq!(s.doc().unwrap().doc.assets[&designcraft_doc::AssetId(r["asset"].as_u64().unwrap())].name, PASTED_IMAGE_NAME);

        let saved = s.execute("file.serialize", &json!({})).unwrap();
        let mut t = Session::new();
        t.execute("file.openBytes", &json!({"name": "a.designcraft", "base64": saved["base64"]})).unwrap();
        assert_eq!(placed_png(&t, id), png, "saved with the document");

        let idml = s.execute("file.exportIdml", &json!({})).unwrap();
        let imported = designcraft_idml::import_idml(&base64_decode(idml["base64"].as_str().unwrap())).unwrap();
        assert!(imported.assets.values().any(|a| *a.data == png), "the IDML package carries the pixels");
    }

    #[test]
    fn without_a_point_the_image_lands_in_the_middle_of_the_first_page_and_shrinks_to_the_page() {
        let mut s = doc();
        let r = s.execute(ID, &json!({"base64": base64_encode(&png(1000, 10))})).unwrap();
        let d = &s.doc().unwrap().doc;
        let b = d.item(ItemId(r["id"].as_u64().unwrap())).unwrap().bounds();
        let page = d.spread(SpreadRef::Doc(0)).unwrap().pages[0].bounds();
        let w = d.settings.page_width * 0.6;
        assert!((b.width() - w).abs() < 1e-9 && (b.height() - w / 100.0).abs() < 1e-9, "{b:?}");
        assert!((b.center().x - page.center().x).abs() < 1e-9 && (b.center().y - page.center().y).abs() < 1e-9, "{b:?}");
    }

    #[test]
    fn a_selected_empty_frame_takes_the_image() {
        let mut s = doc();
        let frame = s.execute("frame.create", &json!({"rect": [100, 100, 200, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [frame]})).unwrap();
        let items = s.doc().unwrap().doc.spreads[0].items.len();
        let r = s.execute(ID, &json!({"base64": base64_encode(&png(10, 10)), "at": [500, 500]})).unwrap();
        assert_eq!(r["id"].as_u64(), Some(frame));
        assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), items, "no new frame");
        // A frame that already holds a graphic gets a new frame beside it instead.
        let r = s.execute(ID, &json!({"base64": base64_encode(&png(10, 10)), "at": [500, 500]})).unwrap();
        assert_ne!(r["id"].as_u64(), Some(frame));
        assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), items + 1);
    }

    #[test]
    fn bad_input_is_refused_and_leaves_no_undo_step() {
        let mut s = doc();
        let ok = base64_encode(&png(2, 2));
        let undo = s.doc().unwrap().history.undo.len();
        for (p, want) in [
            (json!({}), "missing base64"),
            (json!({"base64": 5}), "`base64` must be a non-empty string"),
            (json!({"base64": ""}), "`base64` must be a non-empty string"),
            (json!({"base64": base64_encode(b"just text")}), "doesn't hold an image"),
            (json!({"base64": base64_encode(b"<svg xmlns='http://www.w3.org/2000/svg'/>")}), "doesn't hold an image"),
            (json!({"base64": ok, "name": ""}), "`name` must be a non-empty string"),
            (json!({"base64": ok, "name": 3}), "`name` must be a non-empty string"),
            (json!({"base64": ok, "name": "notes.txt"}), "`name` must end in an image extension"),
            (json!({"base64": ok, "name": "page.idml"}), "`name` must end in an image extension"),
            (json!({"base64": ok, "name": "png"}), "`name` must end in an image extension"),
            (json!({"base64": ok, "at": [1]}), "`at` must be [x, y]"),
            (json!({"base64": ok, "spread": 9}), "no such spread"),
        ] {
            let e = s.execute(ID, &p).unwrap_err().to_string();
            assert!(e.contains(want), "{p}: {e}");
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), undo, "nothing changed");
    }

    #[test]
    fn base64_past_the_file_limit_is_refused_by_its_length() {
        let limit = MAX_PASTED_BYTES / 3 * 4;
        assert!(check_pasted_base64_len(limit).is_ok());
        let e = check_pasted_base64_len(limit + 4).unwrap_err();
        assert!(e.contains("larger than 256 MB"), "{e}");
    }

    #[test]
    fn a_damaged_image_with_a_whole_header_is_refused() {
        let mut s = doc();
        let whole = png(40, 30);
        let cut = &whole[..whole.len() / 2];
        assert_eq!(raster_size(cut), Some((40, 30)), "the header still reads");
        let (undo, assets) = (s.doc().unwrap().history.undo.len(), s.doc().unwrap().doc.assets.len());
        let e = s.execute(ID, &json!({"base64": base64_encode(cut)})).unwrap_err().to_string();
        assert!(e.contains("damaged"), "{e}");
        let st = s.doc().unwrap();
        assert_eq!((st.history.undo.len(), st.doc.assets.len()), (undo, assets), "nothing stored");
    }

    #[test]
    fn each_paste_is_one_journal_entry_and_replays_once() {
        let mut s = doc();
        let frame = s.execute("frame.create", &json!({"rect": [100, 100, 200, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [frame]})).unwrap();
        let start = s.journal.len();
        let b64 = base64_encode(&png(10, 10));
        s.execute(ID, &json!({"base64": b64, "at": [500, 500]})).unwrap();
        s.execute(ID, &json!({"base64": b64, "at": [500, 500]})).unwrap();
        let entries = s.journal[start..].to_vec();
        assert_eq!(entries.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), [ID, ID], "the frame and the new-frame paths");
        let items = |s: &Session| s.doc().unwrap().doc.spreads[0].items.len();
        let mut t = doc();
        t.execute("frame.create", &json!({"rect": [100, 100, 200, 300]})).unwrap();
        t.execute("selection.set", &json!({"ids": [frame]})).unwrap();
        for (id, p) in &entries {
            t.execute(id, p).unwrap();
        }
        assert_eq!(items(&t), items(&s), "the replay places each image once");
    }

    #[test]
    fn images_beyond_the_pixel_limit_are_refused_before_they_are_encoded() {
        let e = pasted_png(9000, 8000, Vec::new()).unwrap_err();
        assert!(e.contains("9000 × 8000 pixels") && e.contains("64 million"), "{e}");
        assert!(pasted_png(0, 5, Vec::new()).is_err());
        assert!(pasted_png(2, 2, vec![0; 15]).unwrap_err().contains("wrong number of pixels"));
        assert_eq!(pasted_pixels(&png(3, 2)).map(|(w, h, px)| (w, h, px.len())), Some((3, 2, 24)));
        assert_eq!(pasted_pixels(b"%PDF-1.4"), None, "vector files are not pasted as pixels");
    }
}
