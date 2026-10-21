//! PageMaker 4–7 (PC and Mac): the record index into a [`PmDoc`].
//!
//! Record sizes and field offsets differ by version (see [`Layout`]); the meaning of each field
//! was worked out from sample files and is written up in `FORMAT.md`.

use std::collections::BTreeMap;

use crate::bytes::{self, Bytes};
use crate::model::{Align, CharRun, Obj, ObjKind, ParaRun, PmColor, PmDoc, PmStory};
use crate::records::{self, List, kind};
use crate::{PmError, Version};

/// Twips (1/1440 inch) per point.
const TWIPS: f64 = 20.0;
/// The most pages imported.
const MAX_PAGES: usize = 9_999;
/// The most characters in one story.
const MAX_STORY: usize = 64 << 20;

/// Where things are in the records of one version.
#[derive(Clone, Copy, Debug)]
struct Layout {
    object: usize,
    char_run: usize,
    para_run: usize,
    font: usize,
    /// Offset of an object's bounds.
    bounds: usize,
    /// Offset of a text block's block id; its story id follows 4 bytes on.
    block_id: usize,
    /// Offset of the left page's rectangle in the document record; the right page's follows.
    page_rects: usize,
    /// Offset of the left page's margin box in the document record, when there is one.
    margin_box: Option<usize>,
    /// Offset of an object's transform id (PageMaker 5 and later rotate and skew objects).
    transform: Option<usize>,
    /// Offset of a graphic's image size in pixels.
    pixels: usize,
    /// Offset of a line's two end points.
    line_ends: usize,
    /// Offset of a line's weight in tenths of a point, when the version has one.
    line_weight: Option<usize>,
}

impl Layout {
    fn of(version: Version) -> Layout {
        match version {
            Version::Pm4 => Layout {
                object: 58,
                char_run: 26,
                para_run: 70,
                font: 144,
                bounds: 4,
                block_id: 26,
                page_rects: 40,
                margin_box: Some(56),
                transform: None,
                pixels: 38,
                line_ends: 30,
                line_weight: None,
            },
            Version::Pm5 => Layout {
                object: 78,
                char_run: 30,
                para_run: 76,
                font: 94,
                bounds: 6,
                block_id: 32,
                page_rects: 42,
                margin_box: Some(58),
                transform: Some(28),
                pixels: 44,
                line_ends: 40,
                line_weight: Some(35),
            },
            Version::Pm6 => Layout {
                object: 136,
                char_run: 30,
                para_run: 80,
                font: 94,
                bounds: 6,
                block_id: 32,
                page_rects: 54,
                margin_box: None,
                transform: Some(28),
                pixels: 44,
                line_ends: 40,
                line_weight: Some(35),
            },
            _ => Layout {
                object: 258,
                char_run: 30,
                para_run: 80,
                font: 94,
                bounds: 6,
                block_id: 32,
                page_rects: 54,
                margin_box: None,
                transform: Some(28),
                pixels: 44,
                line_ends: 40,
                line_weight: Some(35),
            },
        }
    }
}

/// Read a PageMaker 4–7 stream (the whole file, or the `PageMaker` stream of a 6–7 file).
pub fn parse(data: &[u8], version: Version, mac: bool) -> Result<PmDoc, PmError> {
    let b = Bytes::new(data, !mac);
    let lay = Layout::of(version);
    let lists = records::index(&b);
    let src = records::Source::new(b);
    if lists.is_empty() {
        return Err(PmError::Corrupt("the document's record index is missing or damaged".into()));
    }
    let mut doc = PmDoc::default();
    let first = |k: u8| lists.iter().find(|l| l.kind == k && l.count() > 0);

    // Pages: the document record holds the left and right pages' rectangles (the same one twice
    // when pages don't face), in twips about the spread's centre.
    let docrec =
        first(kind::DOCUMENT).and_then(|l| l.records(&src, 128).first().copied()).ok_or_else(|| PmError::Corrupt("no document record".into()))?;
    let d = Bytes::new(docrec, !mac);
    let rect = |at: usize| -> Option<[i16; 4]> { Some([d.i16(at)?, d.i16(at + 2)?, d.i16(at + 4)?, d.i16(at + 6)?]) };
    let left = rect(lay.page_rects).ok_or_else(|| PmError::Corrupt("no page size".into()))?;
    let right = rect(lay.page_rects + 8).unwrap_or(left);
    let width = f64::from(left[2]) - f64::from(left[0]);
    let height = f64::from(left[3]) - f64::from(left[1]);
    if !(width > 0.0 && height > 0.0) {
        return Err(PmError::Corrupt(format!("bad page size {width}×{height} twips")));
    }
    doc.width = width / TWIPS;
    doc.height = height / TWIPS;
    doc.facing = left != right;
    if let Some(m) = lay.margin_box.and_then(rect) {
        // The left page's margin box: inside is the right edge, outside the left one.
        let [x0, y0, x1, y1] = m.map(f64::from);
        let margins = [y0 - f64::from(left[1]), f64::from(left[3]) - y1, f64::from(left[2]) - x1, x0 - f64::from(left[0])].map(|v| v / TWIPS);
        if margins.iter().all(|v| (0.0..doc.width.min(doc.height) / 2.0).contains(v)) {
            doc.margins = Some(margins);
        }
    }

    let fonts: Vec<String> = lists
        .iter()
        .filter(|l| l.kind == kind::FONTS)
        .flat_map(|l| l.records(&src, lay.font))
        .map(|r| bytes::name(r.get(..32).unwrap_or(r), mac))
        .collect();

    // Stories come in groups: a list of story records (an id and a length each), then the text,
    // character runs and paragraph runs of all of them, one after the other.
    let mut group: Option<Group> = None;
    for l in &lists {
        match l.kind {
            kind::STORY => {
                if let Some(g) = group.take() {
                    g.finish(&mut doc);
                }
                let heads = l.records(&src, 24).iter().filter_map(|r| story_head(r, mac)).collect();
                group = Some(Group { heads, ..Group::default() });
            }
            kind::TEXT => {
                if let Some(g) = group.as_mut() {
                    let raw = l.bytes(&src, MAX_STORY.saturating_sub(g.text.len()));
                    g.text.extend(raw.iter().take(MAX_STORY.saturating_sub(g.text.len())).map(|c| text_char(*c, mac)));
                }
            }
            kind::CHAR_RUNS => {
                if let Some(g) = group.as_mut() {
                    g.chars.extend(l.records(&src, lay.char_run).iter().map(|r| char_run(r, mac, &fonts)));
                }
            }
            kind::PARA_RUNS => {
                if let Some(g) = group.as_mut() {
                    g.paras.extend(l.records(&src, lay.para_run).iter().map(|r| para_run(r, mac)));
                }
            }
            _ => {}
        }
    }
    if let Some(g) = group.take() {
        g.finish(&mut doc);
    }

    // The colour table. Its first colours are the fixed ones; PageMaker 6 and later start with None.
    let (color_size, modern) = match version {
        Version::Pm4 => (64, false),
        Version::Pm5 => (86, false),
        _ => (210, true),
    };
    doc.colors = lists
        .iter()
        .filter(|l| l.kind == kind::COLORS)
        .flat_map(|l| l.records(&src, color_size))
        .enumerate()
        .map(|(i, r)| color(r, i, modern, mac))
        .collect();

    // Embedded images, in chunks.
    doc.images = lists
        .iter()
        .filter(|l| l.kind == kind::IMAGE_DATA)
        .filter_map(|l| crate::images::embedded(l.bytes(&src, crate::images::MAX_IMAGE.saturating_add(1))))
        .collect();

    // Transforms: rotation in thousandths of a degree (counter-clockwise), the unrotated box, an id.
    let transforms: BTreeMap<u32, (f64, f64, f64)> = lists
        .iter()
        .filter(|l| l.kind == kind::TRANSFORMS)
        .flat_map(|l| l.records(&src, 26))
        .filter_map(|r| {
            let t = Bytes::new(r, !mac);
            let (x0, y0, x1, y1) = (t.i16(10)?, t.i16(12)?, t.i16(14)?, t.i16(16)?);
            let w = (f64::from(x1) - f64::from(x0)).abs() / TWIPS;
            let h = (f64::from(y1) - f64::from(y0)).abs() / TWIPS;
            let deg = -f64::from(t.u32(0)? as i32) / 1000.0;
            Some((t.u32(22)?, (deg.rem_euclid(360.0), w, h)))
        })
        .collect();

    // Objects, a list per page: first one list (two from PageMaker 6.5) of objects off the
    // pages, then the left and right master pages, then each page. Positions are spread-wide.
    let object_lists: Vec<&List> = lists.iter().filter(|l| l.kind == kind::OBJECTS).collect();
    let special = if version == Version::Pm65 { 2 } else { 1 };
    let top = f64::from(left[1]);
    for (i, l) in object_lists.iter().enumerate().skip(special) {
        let objs = l.records(&src, lay.object).into_iter().filter_map(|r| object(r, &lay, mac, top, &transforms));
        if i < special + 2 {
            doc.masters.extend(objs);
        } else if doc.pages.len() < MAX_PAGES {
            doc.pages.push(objs.collect());
        } else {
            doc.warnings.push(format!("only the first {MAX_PAGES} pages were imported"));
            break;
        }
    }
    if doc.pages.is_empty() {
        doc.pages.push(vec![]);
    }
    // A file that names far more data than it holds (damage, or crafted to): what fit came in.
    if src.exhausted() {
        doc.warnings.push("the file refers to more data than it holds (it may be damaged): some of its content was left out".into());
    }
    Ok(doc)
}

/// A story record: its id and its length in characters. The length is a 32-bit count written
/// as two 16-bit words, low word first, in the file's byte order.
fn story_head(r: &[u8], mac: bool) -> Option<(u32, usize)> {
    let b = Bytes::new(r, !mac);
    let len = usize::from(b.u16(0)?) | usize::from(b.u16(2)?) << 16;
    Some((b.u32(20)?, len))
}

/// The stories of one group, until their text and runs are shared out.
#[derive(Default)]
struct Group {
    heads: Vec<(u32, usize)>,
    text: Vec<char>,
    chars: Vec<CharRun>,
    paras: Vec<ParaRun>,
}

impl Group {
    /// Give each story its text and the runs over it (a run that spans two stories is split).
    fn finish(self, doc: &mut PmDoc) {
        let lens: Vec<usize> = self.heads.iter().map(|h| h.1).collect();
        let mut chars = split_runs(self.chars, &lens, |r| &mut r.len).into_iter();
        let mut paras = split_runs(self.paras, &lens, |r| &mut r.len).into_iter();
        let mut text = self.text.into_iter();
        for (id, len) in self.heads {
            let st =
                PmStory { text: text.by_ref().take(len).collect(), chars: chars.next().unwrap_or_default(), paras: paras.next().unwrap_or_default() };
            doc.stories.insert(id, st);
        }
    }
}

/// Runs over joined stories of lengths `lens`, shared out story by story; a run crossing a
/// story's end is cut in two there.
fn split_runs<R: Clone>(runs: Vec<R>, lens: &[usize], len: impl Fn(&mut R) -> &mut usize) -> Vec<Vec<R>> {
    let mut out: Vec<Vec<R>> = vec![Vec::new(); lens.len()];
    let mut story = 0usize;
    let mut room = lens.first().copied().unwrap_or(0);
    for mut run in runs {
        let mut n = *len(&mut run);
        while n > 0 && story < lens.len() {
            if room == 0 {
                story += 1;
                room = lens.get(story).copied().unwrap_or(0);
                continue;
            }
            let take = n.min(room);
            let mut part = run.clone();
            *len(&mut part) = take;
            if let Some(v) = out.get_mut(story) {
                v.push(part);
            }
            n -= take;
            room -= take;
        }
    }
    out
}

/// Colour `i` of the colour table.
fn color(r: &[u8], i: usize, modern: bool, mac: bool) -> PmColor {
    let fixed = if modern {
        [PmColor::None, PmColor::Registration, PmColor::Paper, PmColor::Black].get(i).cloned()
    } else {
        [PmColor::Registration, PmColor::Paper, PmColor::Black].get(i).cloned()
    };
    if let Some(c) = fixed {
        return c;
    }
    let b = Bytes::new(r, !mac);
    let name = bytes::name(r.get(..32).unwrap_or(r), mac);
    let unit = |at: usize| f32::from(b.u16(at).unwrap_or(0)) / 65535.0;
    if modern && b.u16(34) == Some(0x18) {
        let byte = |at: usize| f32::from(b.u8(at).unwrap_or(0)) / 255.0;
        return PmColor::Rgb(name, [byte(38), byte(39), byte(40)]);
    }
    let at = if modern { 38 } else { 34 };
    PmColor::Cmyk(name, [unit(at), unit(at + 2), unit(at + 4), unit(at + 6)])
}

/// A character of story text. PageMaker marks paragraph ends with CR; 0x19 and 0x1A are its tab
/// and forced line break in stories (the PC writes tabs as 0x09 too).
fn text_char(c: u8, mac: bool) -> char {
    match c {
        0x0D => '\n',
        0x09 | 0x19 => '\t',
        0x1A | 0x0B => '\u{2028}',
        0x00..=0x1F => ' ',
        _ => bytes::char_of(c, mac),
    }
}

fn char_run(r: &[u8], mac: bool, fonts: &[String]) -> CharRun {
    let b = Bytes::new(r, !mac);
    let style = b.u16(10).unwrap_or(0);
    CharRun {
        len: usize::from(b.u16(0).unwrap_or(0)),
        font: b.u16(2).and_then(|f| fonts.get(usize::from(f))).filter(|f| !f.is_empty()).cloned(),
        size: b.u16(4).filter(|s| (10..=16_380).contains(s)).map(|s| f64::from(s) / 10.0),
        leading: b.u16(6).filter(|l| *l != 0xFFFF && (10..=32_000).contains(l)).map(|l| f64::from(l) / 10.0),
        bold: style & 1 != 0,
        italic: style & 2 != 0,
        underline: style & 4 != 0,
        color: b.u16(8).map(usize::from),
    }
}

fn para_run(r: &[u8], mac: bool) -> ParaRun {
    let b = Bytes::new(r, !mac);
    // Indents and spacing in twips; a value beyond a page width is damage, and counts as none.
    let twips = |at: usize| b.i16(at).map(f64::from).filter(|v| v.abs() <= 15_840.0).map_or(0.0, |v| v / TWIPS);
    ParaRun {
        len: usize::from(b.u16(0).unwrap_or(0)),
        left: twips(10),
        first: twips(12),
        right: twips(14),
        before: twips(16),
        after: twips(18),
        align: match b.u8(3) {
            Some(1) => Align::Right,
            Some(2) => Align::Center,
            Some(3 | 4) => Align::Justify,
            _ => Align::Left,
        },
    }
}

/// One object of a page's list, in spread space: x from the spine (or page centre), y from the
/// top of the page (`top`, in twips from the page's centre line).
fn object(r: &[u8], lay: &Layout, mac: bool, top: f64, transforms: &BTreeMap<u32, (f64, f64, f64)>) -> Option<Obj> {
    let b = Bytes::new(r, !mac);
    let t = b.u8(0)?;
    let at = lay.bounds;
    let [x0, y0, x1, y1] = [b.i16(at)?, b.i16(at + 2)?, b.i16(at + 4)?, b.i16(at + 6)?].map(f64::from);
    let rect = [x0.min(x1) / TWIPS, (y0.min(y1) - top) / TWIPS, x0.max(x1) / TWIPS, (y0.max(y1) - top) / TWIPS];
    let kind = match t {
        1 => ObjKind::Text { block: b.u32(lay.block_id)?, story: b.u32(lay.block_id + 4)? },
        3 => {
            // The end points, in twips like the bounds; the bounds' diagonal when they are damaged.
            let ends = [b.i16(lay.line_ends), b.i16(lay.line_ends + 2), b.i16(lay.line_ends + 4), b.i16(lay.line_ends + 6)];
            let [ax, ay, bx, by] = match ends {
                [Some(ax), Some(ay), Some(bx), Some(by)] => [ax, ay, bx, by].map(f64::from),
                _ => [x0, y0, x1, y1],
            };
            let inside = |x: f64, y: f64| x >= x0.min(x1) - 1.0 && x <= x0.max(x1) + 1.0 && y >= y0.min(y1) - 1.0 && y <= y0.max(y1) + 1.0;
            let [ax, ay, bx, by] = if inside(ax, ay) && inside(bx, by) { [ax, ay, bx, by] } else { [x0, y0, x1, y1] };
            let weight = lay.line_weight.and_then(|at| b.u8(at)).filter(|w| *w > 0).map(|w| f64::from(w) / 10.0);
            ObjKind::Line { a: (ax / TWIPS, (ay - top) / TWIPS), b: (bx / TWIPS, (by - top) / TWIPS), weight }
        }
        4 => ObjKind::Box,
        5 => ObjKind::Oval,
        6 => ObjKind::Graphic { pixels: Some((u32::from(b.u16(lay.pixels)?), u32::from(b.u16(lay.pixels + 2)?))).filter(|(w, h)| *w > 0 && *h > 0) },
        10 => ObjKind::Graphic { pixels: None },
        11 | 12 => ObjKind::Shape,
        _ => return None,
    };
    // A rotation turns the unrotated size about the bounds' centre (no rotation: none).
    let rotation = lay
        .transform
        .and_then(|at| b.u32(at))
        .filter(|id| *id != u32::MAX)
        .and_then(|id| transforms.get(&id).copied())
        .filter(|(deg, w, h)| deg.abs() > 1e-6 && *w > 0.0 && *h > 0.0);
    Some(Obj { kind, rect, rotation })
}
