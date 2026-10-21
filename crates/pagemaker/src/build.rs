//! A [`PmDoc`] as a DesignCraft [`Document`]: pages, master pages (the A-Parent), threaded text
//! frames with their stories' formatting, lines, boxes, ovals and frames for placed graphics.

use std::collections::BTreeMap;

use designcraft_doc::build::NewDocument;
use designcraft_doc::{
    Align as DcAlign, Asset, AssetId, Content, Document, Fill, Fitting, Graphic, Item, ItemId, Leading, Margins, ParaFormat, Shape, SpreadRef,
    Stroke, TextWrap,
};
use designcraft_geom::{Affine, Point, Rect, Unit, shapes};

use crate::PmError;
use crate::model::{Align, ObjKind, PmColor, PmDoc, PmStory};

/// The document, and what didn't come across.
pub fn build(pm: &PmDoc) -> Result<(Document, Vec<String>), PmError> {
    let mut warnings = pm.warnings.clone();
    let margins = pm.margins.map_or(Margins::uniform(36.0), |[top, bottom, inside, outside]| Margins { top, bottom, inside, outside });
    let nd = NewDocument {
        width: pm.width,
        height: pm.height,
        pages: pm.pages.len().max(1),
        facing_pages: pm.facing,
        margins,
        units: Unit::Inches,
        ..NewDocument::default()
    };
    let mut d = Document::new(&nd);
    let layer = d.default_layer();
    let swatches = add_colors(&mut d, &pm.colors);
    // Text blocks of each story, in block order: (block, spread, rect in spread space).
    let mut blocks: BTreeMap<u32, Vec<(u32, SpreadRef, Rect, Affine)>> = BTreeMap::new();
    let mut graphics = 0usize;
    // Embedded images already added: image index → (asset, natural size in points).
    let mut assets: BTreeMap<usize, (AssetId, (f64, f64))> = BTreeMap::new();
    let single_graphic =
        pm.pages.iter().chain(std::iter::once(&pm.masters)).flatten().filter(|o| matches!(o.kind, ObjKind::Graphic { .. })).count() == 1;
    let mut shapes_kept = 0usize;

    let mut place = |d: &mut Document, r: SpreadRef, origin: (f64, f64), obj: &crate::model::Obj| -> Result<(), PmError> {
        let [x0, y0, x1, y1] = obj.rect;
        let bounds = Rect::new(origin.0 + x0, origin.1 + y0, origin.0 + x1, origin.1 + y1);
        // A rotated object: its unrotated shape about the bounds' centre, then turned.
        let (rect, xf) = match obj.rotation {
            Some((deg, w, h)) => {
                let c = bounds.center();
                (Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0), Affine::rotate_about(deg.to_radians(), c))
            }
            None => (bounds, Affine::IDENTITY),
        };
        let (shape, path, stroke) = match obj.kind {
            ObjKind::Text { story, block } => {
                blocks.entry(story).or_default().push((block, r, rect, xf));
                return Ok(());
            }
            ObjKind::Line { a, b, weight } => {
                let at = |p: (f64, f64)| Point::new(origin.0 + p.0, origin.1 + p.1);
                let stroke = Stroke { weight: weight.unwrap_or(1.0), ..Stroke::default() };
                (Shape::GraphicLine, shapes::line(at(a), at(b)), stroke)
            }
            ObjKind::Box => (Shape::Rectangle, shapes::rectangle(rect), Stroke::default()),
            ObjKind::Oval => (Shape::Oval, shapes::ellipse(rect), Stroke::default()),
            ObjKind::Graphic { .. } => (Shape::Rectangle, shapes::rectangle(rect), Stroke::none()),
            ObjKind::Shape => {
                shapes_kept += 1;
                (Shape::Rectangle, shapes::rectangle(rect), Stroke::default())
            }
        };
        let mut it = Item::new(ItemId(d.alloc()), layer, shape, path);
        it.xf = xf;
        it.stroke = stroke;
        it.fill = Fill::none();
        if let ObjKind::Graphic { pixels } = obj.kind {
            // The image of that pixel size; with a single image and graphic, that one.
            let image = pixels
                .and_then(|(w, h)| pm.images.iter().position(|i| same_size((i.width, i.height), (w, h))))
                .or_else(|| (pm.images.len() == 1 && single_graphic).then_some(0));
            match image.and_then(|k| {
                assets.get(&k).copied().or_else(|| {
                    add_image(d, &pm.images, k).inspect(|a| {
                        assets.insert(k, *a);
                    })
                })
            }) {
                Some((asset, (w, h))) => {
                    // The image fills its frame, as PageMaker shows it.
                    let xf = Affine::translate((rect.x0, rect.y0)) * Affine::scale_non_uniform(rect.width() / w, rect.height() / h);
                    it.content = Content::Graphic(Graphic {
                        asset,
                        size: (w, h),
                        xf,
                        clip: None,
                        auto_fit: Fitting::None,
                        fit_align: 4,
                        crop: [0.0; 4],
                        wrap: TextWrap::default(),
                        fill: Fill::none(),
                    });
                }
                None => {
                    graphics += 1;
                    it.name = "PageMaker graphic".into();
                }
            }
        }
        d.insert_item(r, it, None).map_err(|e| PmError::Corrupt(e.to_string()))?;
        Ok(())
    };

    // PageMaker measures x from the spine (facing pages) or the page's centre: where that is on
    // a spread.
    let origin = |pages: &[designcraft_doc::Page]| -> Option<(f64, f64)> {
        let first = pages.first()?.bounds();
        let x = if !pm.facing {
            (first.x0 + first.x1) / 2.0
        } else {
            match pages.iter().find(|p| p.side == designcraft_doc::PageSide::Right) {
                Some(right) => right.bounds().x0,
                None => first.x1,
            }
        };
        Some((x, first.y0))
    };
    // The master spread is the A-Parent.
    if let Some(o) = d.parents.first().and_then(|p| origin(&p.pages)) {
        for obj in &pm.masters {
            place(&mut d, SpreadRef::Parent(0), o, obj)?;
        }
    }
    for (i, objs) in pm.pages.iter().enumerate() {
        let Some((si, _)) = d.page_loc(i) else { continue };
        let Some(o) = d.spreads.get(si).and_then(|s| origin(&s.pages)) else { continue };
        for obj in objs {
            place(&mut d, SpreadRef::Doc(si), o, obj)?;
        }
    }

    // Each story flows through its blocks, threaded in block order.
    let mut missing = 0usize;
    for (story, mut frames) in blocks {
        frames.sort_by_key(|f| f.0);
        let Some(st) = pm.stories.get(&story) else {
            missing += 1;
            continue;
        };
        let mut prev: Option<ItemId> = None;
        let mut sid = None;
        for (k, (_, r, rect, xf)) in frames.iter().enumerate() {
            let text = if k == 0 { story_text(st) } else { String::new() };
            let (id, s) = d.add_text_frame(*r, *rect, layer, &text, ParaFormat::default()).map_err(|e| PmError::Corrupt(e.to_string()))?;
            if let Some(it) = d.item_mut(id) {
                it.xf = *xf;
            }
            match prev {
                Some(p) => d.thread(p, id).map_err(|e| PmError::Corrupt(e.to_string()))?,
                None => sid = Some(s),
            }
            prev = Some(id);
        }
        if let Some(story) = sid.and_then(|s| d.story_mut(s)) {
            format_story(story, st, &swatches);
        }
    }
    if missing > 0 {
        warnings.push(format!("{missing} text block(s) name a story the file doesn't hold; they were left out"));
    }
    if graphics > 0 {
        warnings.push(format!(
            "{graphics} placed graphic(s) came in as empty frames: their images (metafiles, EPS, or ones the file doesn't hold) aren't imported yet"
        ));
    }
    if shapes_kept > 0 {
        warnings.push(format!("{shapes_kept} polygon(s) or other drawn shape(s) came in as rectangles of their bounds"));
    }
    warnings.extend(d.fix_parent_bases());
    d.check().map_err(|e| PmError::Corrupt(e.to_string()))?;
    Ok((d, warnings))
}

/// The colour table as swatches: each colour's swatch name, added to the document when it is
/// not one of the fixed ones. A colour named like an existing swatch keeps that swatch.
fn add_colors(d: &mut Document, colors: &[PmColor]) -> Vec<Option<String>> {
    use designcraft_color::{Color, swatch};
    colors
        .iter()
        .map(|c| {
            let (name, color) = match c {
                PmColor::None => return Some(swatch::NONE.to_string()),
                PmColor::Registration => return Some(swatch::REGISTRATION.to_string()),
                PmColor::Paper => return Some(swatch::PAPER.to_string()),
                PmColor::Black => return Some(swatch::BLACK.to_string()),
                PmColor::Cmyk(n, [c, m, y, k]) => (n, Color::cmyk(*c, *m, *y, *k)),
                PmColor::Rgb(n, [r, g, b]) => (n, Color::rgb(*r, *g, *b)),
            };
            if name.is_empty() {
                return None;
            }
            if !d.swatches.iter().any(|s| s.name == *name) {
                d.swatches.push(swatch::Swatch::color(name, color));
            }
            Some(name.clone())
        })
        .collect()
}

/// Whether a graphic that records pixel size `recorded` shows an image of size `image`: the same,
/// or a whole multiple of it (PageMaker 4 records some images at a higher resolution).
fn same_size(image: (u32, u32), recorded: (u32, u32)) -> bool {
    (1..=8u32).any(|k| image.0.checked_mul(k) == Some(recorded.0) && image.1.checked_mul(k) == Some(recorded.1))
}

/// Image `k` as an asset of `d` → its id and natural size in points (a pixel a point, as at
/// 72 ppi).
fn add_image(d: &mut Document, images: &[crate::images::Embedded], k: usize) -> Option<(AssetId, (f64, f64))> {
    let img = images.get(k)?;
    let asset = Asset {
        id: AssetId(d.alloc()),
        name: format!("PageMaker image {}.{}", k + 1, if img.mime == "image/bmp" { "bmp" } else { "tif" }),
        mime: img.mime.to_string(),
        data: std::sync::Arc::new(img.data.clone()),
        pixels: Some((img.width, img.height)),
        ..Asset::default()
    };
    Some((d.add_asset(asset), (f64::from(img.width), f64::from(img.height))))
}

/// The story's text without the last paragraph mark (a frame's story ends without one).
fn story_text(st: &PmStory) -> String {
    st.text.strip_suffix('\n').unwrap_or(&st.text).to_string()
}

/// Apply the character and paragraph runs. Run lengths count characters of the file's text.
fn format_story(story: &mut designcraft_doc::Story, st: &PmStory, swatches: &[Option<String>]) {
    // Byte offset of each character of the story text, and its end.
    let len = story.text.len();
    let mut at: Vec<usize> = story.text.char_indices().map(|(i, _)| i).collect();
    at.push(len);
    let byte = |c: usize| at.get(c).copied().unwrap_or(len);
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for run in &st.chars {
        let end = pos.saturating_add(run.len);
        ranges.push((byte(pos), byte(end), run.clone()));
        pos = end;
    }
    for (a, b, run) in ranges {
        if a >= b {
            continue;
        }
        story.format_chars(a..b, |cf| {
            let o = &mut cf.over;
            if let Some(f) = &run.font {
                o.font_family = Some(f.clone());
            }
            o.font_style = Some(
                match (run.bold, run.italic) {
                    (true, true) => "Bold Italic",
                    (true, false) => "Bold",
                    (false, true) => "Italic",
                    (false, false) => "Regular",
                }
                .into(),
            );
            if let Some(s) = run.size {
                o.size = Some(s);
            }
            o.leading = Some(run.leading.map_or(Leading::Auto, Leading::Points));
            if run.underline {
                o.underline = Some(true);
            }
            // Text in [None] would vanish: it keeps the default colour.
            if let Some(name) = run.color.and_then(|c| swatches.get(c)).and_then(Option::as_ref).filter(|n| *n != designcraft_color::swatch::NONE) {
                o.fill = Some(name.clone());
            }
        });
    }
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for run in &st.paras {
        let end = pos.saturating_add(run.len);
        ranges.push((byte(pos), byte(end.saturating_sub(1)).max(byte(pos)), run.clone()));
        pos = end;
    }
    for (a, b, run) in ranges {
        story.format_paras(a..b, |pf| {
            let p = &mut pf.para;
            for (v, field) in [
                (run.left, &mut p.left_indent),
                (run.first, &mut p.first_line_indent),
                (run.right, &mut p.right_indent),
                (run.before, &mut p.space_before),
                (run.after, &mut p.space_after),
            ] {
                if v != 0.0 {
                    *field = Some(v);
                }
            }
            p.align = Some(match run.align {
                Align::Left => DcAlign::Left,
                Align::Center => DcAlign::Center,
                Align::Right => DcAlign::Right,
                Align::Justify => DcAlign::LeftJustified,
            });
        });
    }
}
