//! File › Place of several pages of a PDF (Image Import Options › Pages: All or a Range,
//! `file.place {pdfPages}`): into the place cursor, the next page for each click, or one page per
//! document page from `startPage` on, adding pages at the end as needed, as one undo step. Each
//! page is an asset of its own; they all share the file's bytes, read once.

use designcraft_doc::{PageSide, Spread, SpreadRef};

use super::place::{fill_xf, graphic, graphic_frame, load_queue, pdf_crop_param, shown_size, target_frame, validate_placement};
use super::{Affine, Arc, Asset, AssetId, Content, ItemId, PdfCrop, Rect, Result, Selection, Session, Value, bad, json};
use crate::LoadedGraphic;

const ID: &str = "file.place";

/// Most pages one File › Place takes: each is a frame, and onto pages perhaps a page of its own.
const MAX_PLACED_PAGES: usize = 2000;

/// The least of a page's box a crop inset may leave, across and down, in points.
const MIN_SHOWN: f64 = 1.0;

/// Where the pages go (`pdfPlace`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The place cursor: each click or drag places the next page, in order.
    Cursor,
    /// One page per document page, from `startPage` on.
    Pages,
}

/// How a page sits on its document page (`fit`): centred on it, at its size or scaled to fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    /// At its size.
    None,
    /// As large as fits the page, keeping its proportions.
    Page,
    /// As large as fits inside the page's margins.
    Margins,
    /// As large as fits the page with the document's bleed around it.
    Bleed,
}

/// A string option: `None` when absent or null, an error when it isn't a string.
fn choice<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(v) => Err(bad(ID, format!("`{key}` must be a string, not {v}"))),
    }
}

/// `pdfCropInset`: how far a placed PDF's frame is cut into the page's box from each side, in
/// points of the box: one number for every side, or [top, left, bottom, right]; none negative.
pub(super) fn crop_inset(p: &Value) -> Result<[f64; 4]> {
    let side = |v: &Value| v.as_f64().filter(|n| n.is_finite() && *n >= 0.0);
    let invalid = || bad(ID, "`pdfCropInset` must be a number or [top, left, bottom, right], each finite and at least 0");
    match p.get("pdfCropInset") {
        None | Some(Value::Null) => Ok([0.0; 4]),
        Some(Value::Array(a)) if a.len() == 4 => {
            let mut inset = [0.0; 4];
            for (to, v) in inset.iter_mut().zip(a) {
                *to = side(v).ok_or_else(invalid)?;
            }
            Ok(inset)
        }
        Some(v) => side(v).map(|n| [n; 4]).ok_or_else(invalid),
    }
}

/// Refuses an `inset` that leaves less than [`MIN_SHOWN`] of PDF page `page`'s box (1-based)
/// of `size` points, across or down. No inset leaves the whole box, however small.
pub(super) fn check_inset(inset: [f64; 4], size: (f64, f64), page: usize) -> Result<()> {
    let (w, h) = shown_size(size, inset);
    if inset == [0.0; 4] || (w >= MIN_SHOWN && h >= MIN_SHOWN) {
        return Ok(());
    }
    Err(bad(ID, format!("`pdfCropInset` leaves nothing of page {page}: its box is {:.2} × {:.2} pt", size.0, size.1)))
}

/// File › Place of the pages of the PDF `bytes` that `pdfPages` names: see the module docs.
pub(super) fn place_pdf_pages(s: &mut Session, p: &Value, bytes: Vec<u8>, name: String, link: Option<String>) -> Result<Value> {
    if p.get("pdfPage").is_some_and(|v| !v.is_null()) {
        return Err(bad(ID, "give `pdfPage` (one page) or `pdfPages`, not both"));
    }
    let count = designcraft_render::pdf_page_count(&bytes).ok_or_else(|| bad(ID, "can't read the PDF"))?;
    let pages = crate::cmd::export::page_list(ID, p.get("pdfPages"), count)
        .map_err(|e| match e {
            crate::EngineError::BadParams { cmd, msg } => {
                crate::EngineError::BadParams { cmd, msg: format!("`pdfPages`: {msg} (the PDF has {count} page(s))") }
            }
            other => other,
        })?
        .unwrap_or_default();
    if pages.is_empty() {
        return Err(bad(ID, "`pdfPages` names no page"));
    }
    if pages.len() > MAX_PLACED_PAGES {
        return Err(bad(ID, format!("place at most {MAX_PLACED_PAGES} pages at once ({} chosen)", pages.len())));
    }
    let crop = pdf_crop_param(p)?;
    let inset = crop_inset(p)?;
    let target = match choice(p, "pdfPlace")? {
        Some("cursor") => Target::Cursor,
        Some("pages") => Target::Pages,
        Some(other) => return Err(bad(ID, format!("unknown pdfPlace `{other}` (cursor, pages)"))),
        // File › Place loads the cursor; a script places onto the pages.
        None if p.get("loadCursor").and_then(Value::as_bool) == Some(true) => Target::Cursor,
        None => Target::Pages,
    };
    let assets = page_assets(Arc::new(bytes), &pages, crop, &name, link)?;
    for (asset, size) in &assets {
        check_inset(inset, *size, usize::try_from(asset.page).unwrap_or(usize::MAX).saturating_add(1))?;
    }
    match target {
        Target::Cursor => into_cursor(s, p, assets, inset),
        Target::Pages => onto_pages(s, p, assets, inset),
    }
}

/// An asset for each of `pages` (0-based) of the PDF `data`, all sharing it, each with its
/// natural size: the `crop` box in points. A page whose `crop` box is unusable shows its crop
/// box, as one placed alone does, and its asset says so.
fn page_assets(data: Arc<Vec<u8>>, pages: &[usize], crop: PdfCrop, name: &str, link: Option<String>) -> Result<Vec<(Asset, (f64, f64))>> {
    let unreadable = || bad(ID, "can't read the PDF");
    let mut placed: Vec<_> =
        designcraft_render::pdf_placed_pages(&data, pages, crop).ok_or_else(unreadable)?.into_iter().map(|g| (g, crop)).collect();
    let fallback: Vec<usize> = pages.iter().zip(&placed).filter(|(_, (g, _))| g.is_none()).map(|(page, _)| *page).collect();
    if crop != PdfCrop::Crop && !fallback.is_empty() {
        // Read once more for all of them, not once each.
        let mut whole = designcraft_render::pdf_placed_pages(&data, &fallback, PdfCrop::Crop).ok_or_else(unreadable)?.into_iter();
        for slot in placed.iter_mut().filter(|(g, _)| g.is_none()) {
            *slot = (whole.next().flatten(), PdfCrop::Crop);
        }
    }
    let mime = designcraft_render::image_mime(&data).to_string();
    pages
        .iter()
        .zip(placed)
        .map(|(&page, (geometry, crop))| {
            let one_based = page.saturating_add(1);
            let g = geometry.ok_or_else(|| bad(ID, format!("can't read page {one_based} of the PDF")))?;
            let (w, h) = g.size;
            let asset = Asset {
                name: name.to_string(),
                mime: mime.clone(),
                link: link.clone(),
                data: data.clone(),
                pixels: Some((w.round().max(1.0) as u32, h.round().max(1.0) as u32)),
                page: u32::try_from(page).map_err(|_| bad(ID, format!("can't place page {one_based}")))?,
                pdf_crop: crop,
                // The crop box is the whole page: no box to keep.
                pdf_box: (crop != PdfCrop::Crop).then_some(g.frac),
                ..Default::default()
            };
            // The crop box is the page as it displays (no thinner than a PDF reader shows it).
            let size = if crop == PdfCrop::Crop { (w.max(1e-3), h.max(1e-3)) } else { g.box_size };
            Ok((asset, size))
        })
        .collect()
}

/// Adds `asset` to `d` under a fresh id and returns the id to place it by (the document may
/// already hold the same file). `data` becomes the buffer the document keeps for the file, so the
/// next page added finds it by address instead of comparing the bytes again.
fn add_page_asset(d: &mut designcraft_doc::Document, asset: Asset, data: &mut Option<Arc<Vec<u8>>>) -> AssetId {
    let asset = match data {
        Some(shared) => Asset { data: shared.clone(), ..asset },
        None => asset,
    };
    let fresh = AssetId(d.alloc());
    let id = d.add_asset(Asset { id: fresh, ..asset });
    if data.is_none() {
        *data = d.assets.get(&id).map(|a| a.data.clone());
    }
    id
}

/// The pages into the place cursor, in order. With a frame to fill (`frame`, or the selected
/// empty or graphic frame), the first page fills it and the rest wait in the cursor.
fn into_cursor(s: &mut Session, p: &Value, assets: Vec<(Asset, (f64, f64))>, inset: [f64; 4]) -> Result<Value> {
    let frame = target_frame(s, p);
    let (ids, filled) = s.edit(|d, sel| {
        let mut data = None;
        let ids: Vec<(AssetId, (f64, f64))> = assets.into_iter().map(|(a, size)| (add_page_asset(d, a, &mut data), size)).collect();
        let filled = match (frame, ids.first()) {
            (Some(fid), Some(&(aid, size))) => {
                let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
                let r = it.inner_bounds();
                let xf = fill_xf(r, size, inset);
                validate_placement(size, r, xf, ID)?;
                it.content = Content::Graphic(graphic(aid, size, xf));
                *sel = Selection::items(vec![fid]);
                Some(fid)
            }
            _ => None,
        };
        Ok((ids, filled))
    })?;
    let waiting: Vec<LoadedGraphic> =
        ids.iter().skip(usize::from(filled.is_some())).map(|&(asset, size)| LoadedGraphic { asset, size, inset }).collect();
    let loaded = waiting.len();
    if loaded > 0 {
        load_queue(s, waiting);
    }
    let mut out = json!({"assets": ids.iter().map(|(a, _)| a.0).collect::<Vec<_>>(), "loaded": loaded});
    if let Some(fid) = filled {
        out["id"] = json!(fid.0);
    }
    Ok(out)
}

/// One page per document page from `startPage` (1-based; default 1) on, each in a new frame
/// centred on its page and sized by `fit`, on the active layer. With `addPages` (the default),
/// pages are added at the end of the document for the pages past its last.
fn onto_pages(s: &mut Session, p: &Value, assets: Vec<(Asset, (f64, f64))>, inset: [f64; 4]) -> Result<Value> {
    let fit = match choice(p, "fit")? {
        None | Some("none") => Fit::None,
        Some("page") => Fit::Page,
        Some("margins") => Fit::Margins,
        Some("bleed") => Fit::Bleed,
        Some(other) => return Err(bad(ID, format!("unknown fit `{other}` (none, page, margins, bleed)"))),
    };
    let add_pages = match p.get("addPages") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(v) => return Err(bad(ID, format!("`addPages` must be true or false, not {v}"))),
    };
    let st = s.doc()?;
    let (count, layer) = (st.doc.page_count(), st.active_layer);
    // The page after the last starts the PDF at the end of the document.
    let start = match p.get("startPage") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=count.saturating_add(1)).contains(n))
            .ok_or_else(|| bad(ID, format!("`startPage` must be a page from 1 to {}, not {v}", count.saturating_add(1))))?,
    };
    let first = start.saturating_sub(1);
    let n = assets.len();
    let missing = first.saturating_add(n).saturating_sub(count);
    if missing > 0 && !add_pages {
        return Err(bad(ID, format!("{n} page(s) from page {start} need {missing} more page(s) than the document has (addPages)")));
    }
    s.edit(|d, sel| {
        if missing > 0 {
            let last = count.checked_sub(1);
            let parent = last.and_then(|l| d.page(l)).and_then(|pg| pg.parent);
            d.insert_pages(last, missing, parent)?;
        }
        let bleed = d.settings.bleed;
        let mut data = None;
        let (mut ids, mut placed_assets, mut doc_pages, mut pdf_pages) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (i, (asset, size)) in assets.into_iter().enumerate() {
            let abs = first + i;
            let pdf_page = u64::from(asset.page) + 1;
            let (si, pi) = d.page_loc(abs).ok_or_else(|| bad(ID, "adding pages failed"))?;
            let (r, k) = frame_on_page(&d.spreads[si], pi, bleed, fit, size, inset)
                .ok_or_else(|| bad(ID, format!("page {} has no room for page {pdf_page} of the PDF", abs + 1)))?;
            let [top, left, ..] = inset;
            let xf = Affine::translate((r.x0 - left * k, r.y0 - top * k)) * Affine::scale(k);
            validate_placement(size, r, xf, ID)?;
            let aid = add_page_asset(d, asset, &mut data);
            let id = ItemId(d.alloc());
            let it = graphic_frame(d, id, layer, r, graphic(aid, size, xf));
            d.insert_item(SpreadRef::Doc(si), it, None)?;
            ids.push(id);
            placed_assets.push(aid.0);
            doc_pages.push(abs + 1);
            pdf_pages.push(pdf_page);
        }
        *sel = Selection::items(ids.clone());
        Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(), "assets": placed_assets, "pages": doc_pages, "pdfPages": pdf_pages, "added": missing}))
    })
}

/// The frame a graphic of natural `size`, cut `inset`, gets on page `pi` of `sp`, with the scale
/// it's placed at: what the inset leaves, centred on the page, at its size or scaled by `fit`.
/// `None` when `fit` leaves no room.
fn frame_on_page(sp: &Spread, pi: usize, bleed: [f64; 4], fit: Fit, size: (f64, f64), inset: [f64; 4]) -> Option<(Rect, f64)> {
    let page = sp.pages.get(pi)?;
    let area = match fit {
        Fit::None | Fit::Page => page.bounds(),
        Fit::Margins => page.margin_rect(),
        Fit::Bleed => page_bleed(sp, pi, bleed)?,
    };
    let (w, h) = shown_size(size, inset);
    let k = match fit {
        Fit::None => 1.0,
        _ => (area.width() / w).min(area.height() / h),
    };
    if !(k.is_finite() && k > 0.0) {
        return None;
    }
    let (c, half) = (area.center(), (w * k / 2.0, h * k / 2.0));
    Some((Rect::new(c.x - half.0, c.y - half.1, c.x + half.0, c.y + half.1), k))
}

/// Page `pi` of `sp` with the document's `bleed` (top, bottom, inside, outside) around it on the
/// sides at the spread's outer edges, as the bleed guide runs: none at a spine between pages.
fn page_bleed(sp: &Spread, pi: usize, bleed: [f64; 4]) -> Option<Rect> {
    let page = sp.pages.get(pi)?;
    let [top, bottom, inside, outside] = bleed.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let (leftmost, rightmost) = sp.outer_page_indices()?;
    let left = match page.side {
        _ if pi != leftmost => 0.0,
        PageSide::Left => outside,
        _ => inside,
    };
    let right = match page.side {
        _ if pi != rightmost => 0.0,
        PageSide::Right | PageSide::Single => outside,
        PageSide::Left => inside,
    };
    let b = page.bounds();
    Some(Rect::new(b.x0 - left, b.y0 - top, b.x1 + right, b.y1 + bottom))
}

#[cfg(test)]
mod tests;
