use std::sync::Arc;

use designcraft_doc::{Content, Document, ItemId, PdfCrop};
use designcraft_geom::{Point, Rect};
use serde_json::{Value, json};

use crate::Session;

/// Three 200 × 100 pt pages with a 10 pt bleed all round (crop and media boxes 220 × 120, trim
/// boxes 200 × 100 inside them). Page n's left half, out to the bleed, is the n-th of cyan,
/// magenta and yellow.
fn three_pages() -> String {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100, "pages": 3, "facingPages": false, "bleed": 10})).unwrap();
    for (spread, swatch) in ["C=100 M=0 Y=0 K=0", "C=0 M=100 Y=0 K=0", "C=0 M=0 Y=100 K=0"].into_iter().enumerate() {
        let id = s.execute("frame.create", &json!({"spread": spread, "rect": [-10, -10, 100, 110]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"ids": [id], "swatch": swatch})).unwrap();
    }
    s.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap().to_string()
}

/// A document of `pages` single 400 × 300 pt pages, 20 pt margins and a 5 pt bleed.
fn session(pages: usize) -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 400, "height": 300, "pages": pages, "facingPages": false, "margins": 20, "bleed": 5})).unwrap();
    s
}

fn place(s: &mut Session, pdf: &str, mut params: Value) -> crate::Result<Value> {
    params["base64"] = json!(pdf);
    params["name"] = json!("three.pdf");
    s.execute("file.place", &params)
}

fn ids(v: &Value) -> Vec<ItemId> {
    v.as_array().unwrap().iter().map(|id| ItemId(id.as_u64().unwrap())).collect()
}

/// The asset and graphic of frame `id`.
fn placed(d: &Document, id: ItemId) -> (Arc<designcraft_doc::Asset>, designcraft_doc::Graphic) {
    let g = d.item(id).unwrap().graphic().unwrap().clone();
    (d.assets[&g.asset].clone(), g)
}

fn near(a: Rect, b: Rect) -> bool {
    [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1].iter().all(|d| d.abs() < 1e-6)
}

/// A `w` × `h` rect centred on the 400 × 300 page.
fn centred(w: f64, h: f64) -> Rect {
    Rect::new(200.0 - w / 2.0, 150.0 - h / 2.0, 200.0 + w / 2.0, 150.0 + h / 2.0)
}

/// Where graphic-space point `p` of frame `id`'s graphic lands on its spread.
fn on_spread(d: &Document, id: ItemId, p: Point) -> Point {
    let it = d.item(id).unwrap();
    (it.xf * it.graphic().unwrap().xf) * p
}

#[test]
fn a_range_goes_onto_pages_in_its_order_adding_pages_in_one_undo_step() {
    let pdf = three_pages();
    let mut s = session(2);
    let r = place(&mut s, &pdf, json!({"pdfPages": "3, 1-2", "startPage": 2})).unwrap();
    assert_eq!(r["pages"], json!([2, 3, 4]));
    assert_eq!(r["pdfPages"], json!([3, 1, 2]));
    assert_eq!(r["added"], 2);
    let frames = ids(&r["ids"]);
    let st = s.doc().unwrap();
    assert_eq!(st.selection.items, frames, "the placed frames are selected");
    let d = st.doc.clone();
    assert_eq!(d.page_count(), 4);
    let (first, _) = placed(&d, frames[0]);
    for ((&id, pdf_page), doc_page) in frames.iter().zip([2_u32, 0, 1]).zip([1_usize, 2, 3]) {
        let (asset, g) = placed(&d, id);
        assert_eq!(asset.page, pdf_page);
        assert_eq!(asset.pdf_crop, PdfCrop::Crop);
        assert!(Arc::ptr_eq(&asset.data, &first.data), "every page shares the file's bytes");
        assert_eq!(d.page_of_item(id), Some(doc_page));
        // Actual size (the crop box, bleed included), centred on its page.
        assert!(near(d.item(id).unwrap().bounds(), centred(220.0, 120.0)), "{:?}", d.item(id).unwrap().bounds());
        assert!((g.size.0 - 220.0).abs() < 1e-6 && (g.size.1 - 120.0).abs() < 1e-6, "{:?}", g.size);
    }
    // One undo step takes the pages, frames and assets away; redo brings them back.
    s.execute("edit.undo", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.page_count(), 2);
    assert!(d.assets.is_empty() && d.spreads.iter().all(|sp| sp.items.is_empty()));
    s.execute("edit.redo", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.page_count(), 4);
    assert_eq!(d.assets.len(), 3);
}

#[test]
fn start_page_after_the_last_appends_and_the_default_starts_at_page_one() {
    let pdf = three_pages();
    let mut s = session(2);
    let r = place(&mut s, &pdf, json!({"pdfPages": [1, 2], "startPage": 3})).unwrap();
    assert_eq!(r["pages"], json!([3, 4]));
    assert_eq!(r["added"], 2);
    let mut s = session(3);
    let r = place(&mut s, &pdf, json!({"pdfPages": "all", "addPages": false})).unwrap();
    assert_eq!(r["pages"], json!([1, 2, 3]));
    assert_eq!(r["added"], 0);
    // A single number is one page, placed as the others are.
    let r = place(&mut s, &pdf, json!({"pdfPages": 2, "startPage": 3})).unwrap();
    assert_eq!(r["pdfPages"], json!([2]));
    assert_eq!(s.doc().unwrap().doc.page_count(), 3);
}

#[test]
fn fit_scales_each_page_to_its_page_its_margins_or_its_bleed() {
    let pdf = three_pages();
    // The 220 × 120 crop box, proportionally: into 400 × 300 (× 400/220), the 360 × 260 inside the
    // margins (× 360/220) and the 410 × 310 with the bleed (× 410/220).
    for (fit, k) in [("none", 1.0), ("page", 400.0 / 220.0), ("margins", 360.0 / 220.0), ("bleed", 410.0 / 220.0)] {
        let mut s = session(1);
        let r = place(&mut s, &pdf, json!({"pdfPages": [2], "fit": fit})).unwrap();
        let id = ids(&r["ids"])[0];
        let d = s.doc().unwrap().doc.clone();
        let frame = d.item(id).unwrap().bounds();
        assert!(near(frame, centred(220.0 * k, 120.0 * k)), "{fit}: {frame:?}");
        // The graphic fills its frame exactly: no crop, no gap.
        let (_, g) = placed(&d, id);
        let it = d.item(id).unwrap();
        assert!(near((it.xf * g.xf).transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1)), frame), "{fit}");
    }
}

#[test]
fn every_page_shows_its_crop_box_less_the_inset() {
    let pdf = three_pages();
    let mut s = session(3);
    // The 200 × 100 trim box, cut 10 from the top, 20 from the left, 30 from the bottom and 40
    // from the right: 140 × 60 shows.
    let r = place(&mut s, &pdf, json!({"pdfPages": "all", "pdfCrop": "trim", "pdfCropInset": [10, 20, 30, 40]})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    for id in ids(&r["ids"]) {
        assert!(near(d.item(id).unwrap().bounds(), centred(140.0, 60.0)), "{:?}", d.item(id).unwrap().bounds());
        let (asset, g) = placed(&d, id);
        assert_eq!(asset.pdf_crop, PdfCrop::Trim);
        let want = [10.0 / 220.0, 10.0 / 120.0, 210.0 / 220.0, 110.0 / 120.0];
        assert!(asset.shown_box().unwrap().iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4), "{:?}", asset.pdf_box);
        assert!((g.size.0 - 200.0).abs() < 1e-3 && (g.size.1 - 100.0).abs() < 1e-3, "{:?}", g.size);
        // The inset's corners are the frame's.
        let frame = centred(140.0, 60.0);
        for (p, at) in [(Point::new(20.0, 10.0), Point::new(frame.x0, frame.y0)), (Point::new(160.0, 70.0), Point::new(frame.x1, frame.y1))] {
            assert!((on_spread(&d, id, p) - at).hypot() < 1e-6, "{p:?}");
        }
    }
    // One number cuts every side.
    let mut s = session(1);
    let r = place(&mut s, &pdf, json!({"pdfPages": [1], "pdfCropInset": 10})).unwrap();
    assert!(near(s.doc().unwrap().doc.item(ids(&r["ids"])[0]).unwrap().bounds(), centred(200.0, 100.0)));
}

#[test]
fn the_place_cursor_holds_the_pages_and_each_click_places_the_next() {
    let pdf = three_pages();
    let mut s = session(1);
    let r = place(&mut s, &pdf, json!({"pdfPages": "2-3, 1", "loadCursor": true, "pdfCropInset": 5})).unwrap();
    assert_eq!(r["loaded"], 3);
    assert_eq!(r["assets"].as_array().unwrap().len(), 3);
    assert!(r.get("id").is_none(), "nothing is placed yet");
    assert_eq!(s.tool_id(), "placeGun");
    assert!(s.doc().unwrap().doc.spreads[0].items.is_empty());
    for (left, pdf_page) in [(2, 1_u32), (1, 2), (0, 0)] {
        let r = s.execute("place.drop", &json!({"x": 10, "y": 20})).unwrap();
        assert_eq!(r["loaded"], left);
        let id = ItemId(r["id"].as_u64().unwrap());
        let d = s.doc().unwrap().doc.clone();
        // A click places it at its size, less the 5 pt inset all round.
        assert!(near(d.item(id).unwrap().bounds(), Rect::new(10.0, 20.0, 220.0, 130.0)), "{:?}", d.item(id).unwrap().bounds());
        assert!((on_spread(&d, id, Point::new(5.0, 5.0)) - Point::new(10.0, 20.0)).hypot() < 1e-6);
        assert_eq!(placed(&d, id).0.page, pdf_page);
        assert_eq!(s.tool_id(), if left > 0 { "placeGun" } else { "selection" });
    }
    assert!(s.loaded.is_none() && s.queued.is_empty());
    // `pdfPlace` says so explicitly, without loadCursor. (Nothing selected: the frame just placed
    // would take the first page.)
    s.execute("edit.deselectAll", &json!({})).unwrap();
    let r = place(&mut s, &pdf, json!({"pdfPages": "all", "pdfPlace": "cursor"})).unwrap();
    assert_eq!(r["loaded"], 3);
}

#[test]
fn a_drag_with_the_cursor_fits_what_the_inset_leaves() {
    let pdf = three_pages();
    let mut s = session(1);
    place(&mut s, &pdf, json!({"pdfPages": [1, 2], "pdfPlace": "cursor", "pdfCrop": "trim", "pdfCropInset": [10, 20, 30, 40]})).unwrap();
    let loaded = s.loaded.unwrap();
    assert_eq!(loaded.inset, [10.0, 20.0, 30.0, 40.0]);
    // 70 × 300 holds the 140 × 60 that shows at half size.
    let r = s.execute("place.drop", &json!({"x": 0, "y": 0, "rect": [0, 0, 70, 300]})).unwrap();
    let id = ItemId(r["id"].as_u64().unwrap());
    let d = s.doc().unwrap().doc.clone();
    assert!(near(d.item(id).unwrap().bounds(), Rect::new(0.0, 0.0, 70.0, 30.0)), "{:?}", d.item(id).unwrap().bounds());
    assert!((on_spread(&d, id, Point::new(20.0, 10.0)) - Point::new(0.0, 0.0)).hypot() < 1e-6);
    // Into a frame: what shows fills it, proportionally.
    let f = s.execute("frame.create", &json!({"rect": [100, 100, 380, 220], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    s.execute("place.drop", &json!({"frame": f})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let f = ItemId(f);
    assert!((on_spread(&d, f, Point::new(20.0, 10.0)) - Point::new(100.0, 100.0)).hypot() < 1e-6);
    assert!((on_spread(&d, f, Point::new(160.0, 70.0)) - Point::new(380.0, 220.0)).hypot() < 1e-6);
}

#[test]
fn escape_skips_the_page_in_the_cursor_and_unload_all_empties_it() {
    use designcraft_tools::{Mods, ToolKey};
    let pdf = three_pages();
    let mut s = session(1);
    place(&mut s, &pdf, json!({"pdfPages": "all", "loadCursor": true})).unwrap();
    let view = crate::ViewInfo::at_zoom(1.0);
    assert!(s.tool_key(ToolKey::Escape, Mods::default(), view).unwrap());
    assert_eq!(s.tool_id(), "placeGun", "two pages still wait");
    let asset = s.loaded.unwrap().asset;
    assert_eq!(s.doc().unwrap().doc.assets[&asset].page, 1);
    let r = s.execute("place.drop", &json!({"x": 0, "y": 0})).unwrap();
    assert_eq!(placed(&s.doc().unwrap().doc, ItemId(r["id"].as_u64().unwrap())).0.page, 1);
    assert_eq!(s.execute("place.unload", &json!({"all": true})).unwrap()["loaded"], 0);
    assert_eq!(s.tool_id(), "selection");
    assert!(s.loaded.is_none() && s.queued.is_empty());
    // Another tool unloads them all as well. (Nothing selected: a selected graphic frame would
    // take the first page.)
    s.execute("edit.deselectAll", &json!({})).unwrap();
    place(&mut s, &pdf, json!({"pdfPages": "all", "loadCursor": true})).unwrap();
    s.set_tool("hand");
    assert!(s.loaded.is_none() && s.queued.is_empty());
    // The last one: Escape brings the Selection tool back, as with a single graphic.
    place(&mut s, &pdf, json!({"pdfPages": [3], "loadCursor": true})).unwrap();
    s.tool_key(ToolKey::Escape, Mods::default(), view).unwrap();
    assert_eq!(s.tool_id(), "selection");
    assert!(s.loaded.is_none());
}

#[test]
fn with_a_frame_selected_the_first_page_fills_it_and_the_rest_wait() {
    let pdf = three_pages();
    let mut s = session(1);
    let f = s.execute("frame.create", &json!({"rect": [0, 0, 110, 60], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    let r = place(&mut s, &pdf, json!({"pdfPages": "all", "loadCursor": true})).unwrap();
    assert_eq!(r["id"], f);
    assert_eq!(r["loaded"], 2);
    let d = s.doc().unwrap().doc.clone();
    let f = ItemId(f);
    assert_eq!(placed(&d, f).0.page, 0);
    // 220 × 120 fills 110 × 60 at half size.
    assert!(near(d.item(f).unwrap().bounds(), Rect::new(0.0, 0.0, 110.0, 60.0)));
    assert!((on_spread(&d, f, Point::new(220.0, 120.0)) - Point::new(110.0, 60.0)).hypot() < 1e-6);
    assert_eq!(s.tool_id(), "placeGun");
    let waiting: Vec<u32> = s.loaded.iter().chain(&s.queued).map(|l| d.assets[&l.asset].page).collect();
    assert_eq!(waiting, [1, 2]);
    // One page into the frame leaves nothing to load: the tool stays as it was.
    s.execute("place.unload", &json!({"all": true})).unwrap();
    s.execute("selection.set", &json!({"ids": [f.0]})).unwrap();
    let r = place(&mut s, &pdf, json!({"pdfPages": [3], "loadCursor": true})).unwrap();
    assert_eq!(r["loaded"], 0);
    assert_eq!(s.tool_id(), "selection");
    assert_eq!(placed(&s.doc().unwrap().doc, f).0.page, 2);
}

#[test]
fn pages_undone_out_of_the_document_leave_the_cursor() {
    let pdf = three_pages();
    let mut s = session(1);
    place(&mut s, &pdf, json!({"pdfPages": "all", "loadCursor": true})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.assets.is_empty());
    assert!(s.execute("place.drop", &json!({"x": 0, "y": 0})).is_err());
    assert!(s.loaded.is_none() && s.queued.is_empty(), "no page left to place");
    assert!(s.doc().unwrap().doc.spreads[0].items.is_empty());
}

#[test]
fn bad_choices_are_refused_and_change_nothing() {
    let pdf = three_pages();
    let mut s = session(2);
    for params in [
        json!({"pdfPages": "0"}),
        json!({"pdfPages": "4"}),
        json!({"pdfPages": "2-1"}),
        json!({"pdfPages": "x"}),
        json!({"pdfPages": " , "}),
        json!({"pdfPages": []}),
        json!({"pdfPages": [0]}),
        json!({"pdfPages": [4]}),
        json!({"pdfPages": [1, "2"]}),
        json!({"pdfPages": -1}),
        json!({"pdfPages": 1.5}),
        json!({"pdfPages": true}),
        json!({"pdfPages": {}}),
        json!({"pdfPages": "all", "pdfPage": 1}),
        json!({"pdfPages": "all", "pdfPlace": "elsewhere"}),
        json!({"pdfPages": "all", "pdfPlace": 1}),
        json!({"pdfPages": "all", "fit": "stretch"}),
        json!({"pdfPages": "all", "fit": true}),
        json!({"pdfPages": "all", "startPage": 0}),
        json!({"pdfPages": "all", "startPage": 4}),
        json!({"pdfPages": "all", "startPage": -1}),
        json!({"pdfPages": "all", "startPage": "1"}),
        json!({"pdfPages": "all", "startPage": u64::MAX}),
        json!({"pdfPages": "all", "addPages": "yes"}),
        json!({"pdfPages": "all", "addPages": false}),
        json!({"pdfPages": "all", "pdfCrop": "nope"}),
        json!({"pdfPages": "all", "pdfCropInset": -1}),
        json!({"pdfPages": "all", "pdfCropInset": "5"}),
        json!({"pdfPages": "all", "pdfCropInset": [1, 2, 3]}),
        json!({"pdfPages": "all", "pdfCropInset": [1, 2, 3, null]}),
        json!({"pdfPages": "all", "pdfCropInset": 1e308}),
        // Nothing would be left across, of the 220 pt wide crop box.
        json!({"pdfPages": "all", "pdfCropInset": [0, 110, 0, 110]}),
        json!({"pdfPages": "all", "pdfPlace": "cursor", "frame": 999_999}),
        // One page placed alone is cropped the same way: nothing would be left down.
        json!({"pdfPage": 1, "pdfCropInset": [60, 0, 60, 0]}),
        json!({"pdfPage": 1, "pdfCropInset": [0, 0, 0, -1]}),
    ] {
        let before = s.doc().unwrap();
        let (doc, revision, selection) = (before.doc.clone(), before.revision, before.selection.clone());
        assert!(place(&mut s, &pdf, params.clone()).is_err(), "{params}");
        let after = s.doc().unwrap();
        assert!(Arc::ptr_eq(&doc, &after.doc), "{params}");
        assert_eq!((after.revision, &after.selection), (revision, &selection), "{params}");
        assert!(s.loaded.is_none() && s.queued.is_empty(), "{params}");
        assert_eq!(s.tool_id(), "selection", "{params}");
    }
}

#[test]
fn placed_from_a_path_each_page_links_to_the_file() {
    let pdf = crate::cmd::base64_decode(&three_pages());
    let file = tempfile::Builder::new().suffix(".pdf").tempfile().unwrap();
    std::fs::write(file.path(), &pdf).unwrap();
    let mut s = session(1);
    let path = file.path().to_string_lossy().to_string();
    s.execute("file.place", &json!({"path": path, "pdfPages": "1, 3"})).unwrap();
    let links = s.execute("links.list", &json!({})).unwrap();
    let rows: Vec<&Value> = links.as_array().unwrap().iter().filter(|row| row.get("asset").is_some()).collect();
    assert_eq!(rows.len(), 2, "{links}");
    for row in rows {
        assert_eq!(row["path"], json!(path));
        assert_eq!(row["status"], "ok", "{row}");
    }
}

#[test]
fn exported_pdf_draws_the_page_each_frame_holds() {
    let pdf = three_pages();
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100, "pages": 1, "facingPages": false})).unwrap();
    // The trim boxes at actual size: each page of the PDF covers a page of the document.
    place(&mut s, &pdf, json!({"pdfPages": "3, 2, 1", "pdfCrop": "trim"})).unwrap();
    let out = s.execute("file.exportPdf", &json!({})).unwrap();
    let bytes = crate::cmd::base64_decode(out["base64"].as_str().unwrap());
    assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(3));
    let is = |(r, g, b): (u8, u8, u8), kind: &str| match kind {
        "cyan" => r < 90 && g > 120 && b > 180,
        "magenta" => r > 180 && g < 90 && b > 60,
        _ => r > 180 && g > 180 && b < 90,
    };
    for (page, colour) in [(0, "yellow"), (1, "magenta"), (2, "cyan")] {
        let pm = designcraft_render::render_pdf_page(&bytes, page, 200).unwrap();
        let (w, h) = (usize::from(pm.width()), usize::from(pm.height()));
        assert_eq!((w, h), (200, 100));
        let at = |x: usize, y: usize| {
            let c = pm.data()[y * w + x];
            (c.r, c.g, c.b)
        };
        assert!(is(at(50, 50), colour), "page {}: {:?}", page + 1, at(50, 50));
        assert!(!is(at(150, 50), colour), "page {}: the right half is paper", page + 1);
    }
}

#[test]
fn placed_pages_survive_saving() {
    let pdf = three_pages();
    let mut s = session(3);
    place(&mut s, &pdf, json!({"pdfPages": "all", "pdfCrop": "trim", "pdfCropInset": 10})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let bytes = crate::cmd::to_bytes(&doc).unwrap();
    // The file holds the PDF once, not once per page.
    let zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(zip.file_names().filter(|n| n.starts_with("assets/")).count(), 1);
    let back = crate::cmd::from_bytes(&bytes).unwrap();
    let mut pages: Vec<(u32, PdfCrop)> = back.assets.values().map(|a| (a.page, a.pdf_crop)).collect();
    pages.sort_by_key(|(page, _)| *page);
    assert_eq!(pages, [(0, PdfCrop::Trim), (1, PdfCrop::Trim), (2, PdfCrop::Trim)]);
    let mut buffers = back.assets.values().map(|a| a.data.clone());
    let first = buffers.next().unwrap();
    assert!(buffers.all(|b| Arc::ptr_eq(&b, &first)), "the pages share one buffer again when opened");
    for sp in &back.spreads {
        for it in &sp.items {
            assert!(matches!(it.content, Content::Graphic(_)));
            assert!(near(it.bounds(), centred(180.0, 80.0)), "{:?}", it.bounds());
        }
    }
}

/// Facing pages: the bleed runs round the outer edges of each spread, never at the spine.
#[test]
fn a_page_bleeds_only_where_its_spread_ends() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 100, "height": 200, "pages": 3, "facingPages": true, "bleed": [1, 2, 3, 4]})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let bleed = d.settings.bleed;
    let rect = |abs: usize| {
        let (si, pi) = d.page_loc(abs).unwrap();
        (super::page_bleed(&d.spreads[si], pi, bleed).unwrap(), d.spreads[si].pages[pi].bounds())
    };
    // Page 1, a right page alone: the inside bleed at the spine edge, as the bleed guide draws it.
    let (b, p) = rect(0);
    assert!(near(b, Rect::new(p.x0 - 3.0, p.y0 - 1.0, p.x1 + 4.0, p.y1 + 2.0)), "{b:?} {p:?}");
    // Pages 2 and 3 face each other: no bleed at the spine between them.
    let (b, p) = rect(1);
    assert!(near(b, Rect::new(p.x0 - 4.0, p.y0 - 1.0, p.x1, p.y1 + 2.0)), "{b:?} {p:?}");
    let (b, p) = rect(2);
    assert!(near(b, Rect::new(p.x0, p.y0 - 1.0, p.x1 + 4.0, p.y1 + 2.0)), "{b:?} {p:?}");
}

/// A PDF of `count` blank 10 × 10 pt pages.
fn blank_pages(count: usize) -> String {
    let kids: Vec<String> = (0..count).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_string(), format!("<< /Type /Pages /Kids [{}] /Count {count} >>", kids.join(" "))];
    objects.extend((0..count).map(|_| "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_string()));
    let mut pdf = "%PDF-1.4\n".to_string();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
    crate::cmd::base64_encode(pdf.as_bytes())
}

/// More pages than one placement takes are refused before anything is read into the document.
#[test]
fn a_placement_takes_at_most_the_page_limit() {
    let mut s = session(1);
    let pdf = blank_pages(super::MAX_PLACED_PAGES + 1);
    let e = place(&mut s, &pdf, json!({"pdfPages": "all"})).unwrap_err().to_string();
    assert!(e.contains("at most 2000 pages"), "{e}");
    assert!(s.doc().unwrap().doc.assets.is_empty());
    // Under it, every page loads.
    let r = place(&mut s, &blank_pages(40), json!({"pdfPages": "all", "pdfPlace": "cursor"})).unwrap();
    assert_eq!(r["loaded"], 40);
}

/// A list places its pages in its order, each once, as a range does.
#[test]
fn a_list_names_each_page_once_in_its_order() {
    let pdf = three_pages();
    let mut s = session(1);
    let r = place(&mut s, &pdf, json!({"pdfPages": [2, 3, 2, 1, 3]})).unwrap();
    assert_eq!(r["pdfPages"], json!([2, 3, 1]));
    assert_eq!(r["pages"], json!([1, 2, 3]));
}
