// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Microsoft Publisher (`.pub`) import: [`import_pub`] turns a Publisher 2002–2021 publication
//! into an editable [`Document`] — pages, text frames with their formatted stories, shapes with
//! fills and strokes, and pictures — and reports what it could not bring over.
//! [`read_publication`] decodes the publication without converting it.
//!
//! ## Format
//! A `.pub` file is an OLE2 compound file (MS-CFB). Publisher 2002 and later (all one storage
//! family, `Contents` header `E8 AC 2C 00`) keep:
//! - `Contents`: chunks found through a trailer directory by sequence number — the document
//!   (page size, page order), pages (their shapes, master names) and shapes (story ids).
//! - `Quill/QuillSub/CONTENTS`: the stories' UTF-16 text and character/paragraph formatting.
//! - `Escher/EscherStm` and `Escher/EscherDelayStm`: Office Drawing records (MS-ODRAW) with
//!   every shape's geometry, fill and line, and the pictures.
//!
//! Publisher 97/98/2000 files (header `E8 AC 22 00`) are recognised and refused with an
//! explanation.
//!
//! ## Provenance
//! The Contents, Quill and bounded-reader modules (`contents.rs`, `quill.rs`, `read.rs`) were
//! rewritten from scratch, without access to the earlier reader, from the format notes in
//! `docs/pub-format/` and the allowed sample files (the controlled Publisher samples, never any
//! other reader's source code). The notes reference Microsoft's public MS-CFB and MS-ODRAW
//! specifications. The drawing reader (`escher.rs`) follows MS-ODRAW. See
//! `docs/publisher-import.md` (Provenance). Publisher `truth/*.json` exports are used only to
//! score imports, never to infer format encoding.
//!
//! ## Not supported yet
//! Tables (their text comes over as a text frame); WMF, EMF and PICT pictures (an empty graphic
//! frame keeps their place); non-default WordArt warps and exact stretching (default upward arch
//! uses measured editable path text; other warps stay straight); pattern/texture fills
//! (approximated by the fill colour); picture crops other than rectangular; custom tab stops,
//! numbering and unrecognized paragraph formatting. Bullets and exact/multiple line spacing are
//! imported. Groups (nested too) stay groups. Shaded fills become gradient swatches (centre and
//! outline shades as radial approximations); dashes and arrowheads come over.
//! Each omission is reported in [`Imported::warnings`].
//!
//! Linked text frames are supported: a story shown in several frames is imported once and
//! threaded through them in chain order (`tests::linked_frames_share_one_story_in_chain_order`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod contents;
mod escher;
mod overset;
mod paths;
mod presets;
mod quill;
mod read;
// Table records are decoded; their conversion follows in a later change.
#[allow(dead_code)]
mod tables;
mod wordart;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use designcraft_doc::build::NewDocument;
use designcraft_doc::{
    Arrowhead, Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, LayerId, ParaFormat, Shape, SpreadRef, Stroke, StrokeType,
    VerticalJustification,
};
use designcraft_geom::{Affine, BezPath, PathData, Point, Rect, shapes};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum PubError {
    #[error("not a Microsoft Publisher file: {0}")]
    NotPub(String),
    #[error("unsupported Publisher file: {0}")]
    Unsupported(String),
    #[error("damaged Publisher file: data ends early")]
    Truncated,
    #[error("damaged Publisher file: {0}")]
    Malformed(String),
}

pub type Result<T> = std::result::Result<T, PubError>;

/// Always display for an opened Publisher conversion, independently of diagnostics.
pub const CONVERSION_NOTICE: &str = "This is a converted copy. It can look different from what Publisher shows; some features are lost or simplified, and not every loss gives a warning. Compare it with your original.";

/// An imported publication and what the import could not bring over.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    /// One line per kind of problem that did not stop the import.
    pub warnings: Vec<String>,
    /// Informational diagnostics for unidentified formatting settings; field IDs are retained.
    pub notes: Vec<String>,
}

const CFB_SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const MAX_FILE: usize = 512 * 1024 * 1024;
const MAX_STREAM: u64 = 256 * 1024 * 1024;
const EMU_PER_PT: f64 = 12_700.0;
/// Groups nested deeper than this are skipped (the drawing reader stops at the same depth).
const MAX_GROUP_DEPTH: usize = 16;

/// True for an OLE2 compound file holding a Publisher `Contents` stream.
pub fn is_pub(bytes: &[u8]) -> bool {
    bytes.starts_with(&CFB_SIGNATURE)
        && cfb::CompoundFile::open(std::io::Cursor::new(bytes)).is_ok_and(|mut cf| {
            let mut head = [0u8; 2];
            cf.open_stream("/Contents").is_ok_and(|mut s| s.read_exact(&mut head).is_ok() && head == [0xe8, 0xac])
        })
}

fn stream(cf: &mut cfb::CompoundFile<std::io::Cursor<&[u8]>>, path: &str) -> Result<Option<Vec<u8>>> {
    if !cf.is_stream(path) {
        return Ok(None);
    }
    let s = cf.open_stream(path).map_err(|e| PubError::Malformed(format!("{path}: {e}")))?;
    let mut out = Vec::new();
    s.take(MAX_STREAM + 1).read_to_end(&mut out).map_err(|e| PubError::Malformed(format!("{path}: {e}")))?;
    if out.len() as u64 > MAX_STREAM {
        return Err(PubError::Unsupported(format!("{path} is larger than {MAX_STREAM} bytes")));
    }
    Ok(Some(out))
}

/// A decoded publication: its structure, text store and drawing records.
pub struct Publication {
    contents: contents::Contents,
    quill: quill::Quill,
    drawing: escher::Drawing,
}

impl Publication {
    /// Page width and height in points, as stored.
    pub fn page_size(&self) -> (f64, f64) {
        (pt(f64::from(self.contents.width_emu)), pt(f64::from(self.contents.height_emu)))
    }
    /// Ordinary pages (masters and the scratch-area placeholders are not counted).
    pub fn page_count(&self) -> usize {
        self.contents.pages.len()
    }
    /// Master pages.
    pub fn master_count(&self) -> usize {
        self.contents.masters.len()
    }
    /// Stories in the text store.
    pub fn story_count(&self) -> usize {
        self.quill.stories.len()
    }
    /// A story's text, UTF-16 decoded as stored (paragraphs end with CR).
    pub fn story_text(&self, id: u32) -> Option<String> {
        self.quill.stories.get(&id).map(|s| String::from_utf16_lossy(&s.units))
    }
    /// Story ids, in ascending order.
    pub fn story_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.quill.stories.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
    /// Top-level drawing records (groups count once).
    pub fn shape_count(&self) -> usize {
        self.drawing.shapes.len()
    }
    /// Pictures in the drawing's picture store.
    pub fn picture_count(&self) -> usize {
        self.drawing.pictures.len()
    }
    /// What the readers could not decode, one line per kind.
    pub fn warnings(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in self.contents.warnings.iter().chain(&self.quill.warnings).chain(&self.drawing.warnings) {
            if !out.contains(w) {
                out.push(w.clone());
            }
        }
        out
    }
}

/// Read a Publisher publication's streams with bounded readers.
pub fn read_publication(bytes: &[u8]) -> Result<Publication> {
    if bytes.len() > MAX_FILE {
        return Err(PubError::Unsupported("the file is larger than 512 MB".into()));
    }
    if !bytes.starts_with(&CFB_SIGNATURE) {
        return Err(PubError::NotPub("not an OLE2 compound file".into()));
    }
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(bytes)).map_err(|e| PubError::Malformed(format!("compound file: {e}")))?;
    let contents = stream(&mut cf, "/Contents")?.ok_or_else(|| PubError::NotPub("no Contents stream".into()))?;
    match contents.get(..4) {
        Some([0xe8, 0xac, 0x2c, 0x00]) => {}
        Some([0xe8, 0xac, 0x22, 0x00]) => {
            return Err(PubError::Unsupported(
                "Publisher 97/98/2000 files are not supported yet; open and save the file in Publisher 2002 or later".into(),
            ));
        }
        _ => return Err(PubError::NotPub("unrecognised Contents header".into())),
    }
    let quill = stream(&mut cf, "/Quill/QuillSub/CONTENTS")?.ok_or_else(|| PubError::Malformed("no Quill text stream".into()))?;
    let escher = stream(&mut cf, "/Escher/EscherStm")?.ok_or_else(|| PubError::Malformed("no Escher drawing stream".into()))?;
    let delay = stream(&mut cf, "/Escher/EscherDelayStm")?.unwrap_or_default();

    let c = contents::parse(&contents)?;
    let mut q = quill::parse(&quill)?;
    tables::recover_single_table_text(&c, &mut q, &quill)?;
    let dr = escher::parse(&escher, &delay)?;
    Ok(Publication { contents: c, quill: q, drawing: dr })
}

/// Import a Publisher publication.
pub fn import_pub(bytes: &[u8]) -> Result<Imported> {
    let p = read_publication(bytes)?;
    let (c, q, dr) = (&p.contents, &p.quill, &p.drawing);
    let mut b = Builder::new(c, q, dr)?;
    b.warnings.extend(c.warnings.iter().cloned());
    b.warnings.extend(q.warnings.iter().cloned());
    b.warnings.extend(dr.warnings.iter().cloned());
    b.build()?;
    let mut warnings: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    let mut seen = HashSet::new();
    for w in b.warnings {
        if seen.insert(w.clone()) {
            if let Some((kind, rest)) = w.strip_prefix("Publisher ").and_then(|w| w.split_once(" formatting is unsupported for fields: "))
                && matches!(kind, "character" | "paragraph")
                && let Some((fields, detail)) = rest.split_once(". Supported attributes were retained.")
            {
                notes.push(format!(
                    "A formatting setting this version does not understand was skipped; the text is kept. Publisher {kind} fields: {fields}."
                ));
                if !detail.trim().is_empty() {
                    warnings.push(format!("Publisher {kind} formatting loss: {}", detail.trim()));
                }
            } else {
                warnings.push(w);
            }
        }
    }
    notes.extend(overset::notes(&b.doc));
    Ok(Imported { document: b.doc, warnings, notes })
}

struct Builder<'a> {
    c: &'a contents::Contents,
    q: &'a quill::Quill,
    dr: &'a escher::Drawing,
    doc: Document,
    layer: LayerId,
    page_w: f64,
    page_h: f64,
    /// Master page sequence number → parent spread index.
    parents: HashMap<u32, usize>,
    pictures: HashMap<u32, Option<AssetId>>,
    /// Text id → the frames showing it, with their position in the chain of linked frames.
    chains: BTreeMap<u32, Vec<(u32, ItemId)>>,
    /// Linked frames after the first, created empty: their insets and vertical justification,
    /// applied once they are threaded into the story.
    followers: Vec<(ItemId, [f64; 4], VerticalJustification)>,
    warnings: Vec<String>,
}

fn pt(emu: f64) -> f64 {
    emu / EMU_PER_PT
}

impl<'a> Builder<'a> {
    fn new(c: &'a contents::Contents, q: &'a quill::Quill, dr: &'a escher::Drawing) -> Result<Self> {
        let (page_w, page_h) = (pt(f64::from(c.width_emu)), pt(f64::from(c.height_emu)));
        if !(1.0..=15_552.0).contains(&page_w) || !(1.0..=15_552.0).contains(&page_h) {
            return Err(PubError::Malformed(format!("page size {page_w}×{page_h} pt is out of range")));
        }
        let pages = c.pages.len().max(1);
        if pages > 10_000 {
            return Err(PubError::Unsupported("more than 10,000 pages".into()));
        }
        let nd = NewDocument {
            title: String::new(),
            width: page_w,
            height: page_h,
            pages,
            facing_pages: false,
            primary_text_frame: false,
            ..Default::default()
        };
        let doc = Document::new(&nd);
        let layer = doc.layers.first().map(|l| l.id).ok_or_else(|| PubError::Malformed("new document has no layer".into()))?;
        Ok(Builder {
            c,
            q,
            dr,
            doc,
            layer,
            page_w,
            page_h,
            parents: HashMap::new(),
            pictures: HashMap::new(),
            chains: BTreeMap::new(),
            followers: Vec::new(),
            warnings: Vec::new(),
        })
    }

    fn build(&mut self) -> Result<()> {
        // Masters become parent spreads; the first takes the new document's A-Parent.
        for (i, m) in self.c.masters.iter().enumerate() {
            let idx = if i == 0 {
                0
            } else {
                let prefix = self.doc.next_parent_prefix();
                let Some(margins) = self.doc.parents.first().and_then(|p| p.pages.first()).map(|p| p.margins) else { continue };
                let id = self.doc.add_parent(&prefix, "Parent", 1, 12.0, margins);
                self.doc.parent_index(id).unwrap_or(0)
            };
            self.parents.insert(m.seq, idx);
        }
        for (abs, page) in self.c.pages.iter().enumerate() {
            let parent = page.applied_master.and_then(|m| self.parents.get(&m)).and_then(|i| self.doc.parents.get(*i)).map(|p| p.id);
            self.doc.apply_parent(&[abs], parent).map_err(|e| PubError::Malformed(e.to_string()))?;
        }
        // Shapes, in drawing (stacking) order, onto the page or master that lists them.
        let mut owner: HashMap<u32, SpreadRef> = HashMap::new();
        for (abs, page) in self.c.pages.iter().enumerate() {
            let (si, _) = self.doc.page_loc(abs).ok_or_else(|| PubError::Malformed("page out of range".into()))?;
            for s in &page.shapes {
                owner.insert(*s, SpreadRef::Doc(si));
            }
        }
        for m in &self.c.masters {
            if let Some(i) = self.parents.get(&m.seq) {
                for s in &m.shapes {
                    owner.insert(*s, SpreadRef::Parent(*i));
                }
            }
        }
        let shapes = &self.dr.shapes;
        let mut placed = 0usize;
        for sh in shapes {
            let Some(r) = sh.seq.and_then(|s| owner.get(&s)).copied() else { continue };
            self.place(sh, r, 0)?;
            placed += 1;
        }
        self.thread_chains();
        let listed: BTreeSet<u32> = owner.keys().copied().collect();
        let drawn: BTreeSet<u32> = shapes.iter().filter_map(|s| s.seq).collect();
        if listed.difference(&drawn).next().is_some() {
            self.warnings.push("Some objects listed on pages have no drawing record and were skipped.".into());
        }
        log::debug!("pub import: {placed} top-level objects");
        Ok(())
    }

    /// Thread each chain of linked frames, in chain order, through the story its first-drawn
    /// frame holds.
    fn thread_chains(&mut self) {
        for (_, mut frames) in std::mem::take(&mut self.chains) {
            frames.sort_by_key(|f| f.0);
            let holder = frames.iter().position(|f| matches!(self.doc.item(f.1).map(|i| &i.content), Some(Content::Text(_))));
            let Some(p) = holder.filter(|_| frames.len() > 1) else { continue };
            // Frames after the holder join its story in order; frames before it are put in front.
            let after = frames.get(p..).unwrap_or_default().windows(2).map(|w| (w, false));
            let before = frames.get(..=p).unwrap_or_default().windows(2).rev().map(|w| (w, true));
            for (w, in_front) in after.chain(before) {
                let [(_, from), (_, to)] = w else { continue };
                let res = if in_front { self.doc.thread_keeping_story(*from, *to) } else { self.doc.thread(*from, *to) };
                if res.is_err() {
                    self.warnings.push("Some linked text frames could not be connected; their text stays in the other frames.".into());
                    break;
                }
            }
        }
        for (id, inset, vj) in std::mem::take(&mut self.followers) {
            if let Some(t) = self.doc.item_mut(id).and_then(Item::text_frame_mut) {
                t.options.inset = inset;
                t.options.vertical_justification = vj;
            }
        }
    }

    /// Page-space rectangle of an anchor (page-centre-relative EMU), offset by the page's position
    /// in its spread.
    fn rect(&self, a: escher::Emu, r: SpreadRef, rotation: f64) -> Rect {
        let x0 = match r {
            SpreadRef::Doc(si) => self.doc.spreads.get(si).and_then(|s| s.pages.first()).map_or(0.0, |p| p.x),
            SpreadRef::Parent(pi) => self.doc.parents.get(pi).and_then(|s| s.pages.first()).map_or(0.0, |p| p.x),
        };
        let (l, t, rr, b) = (pt(a.l.min(a.r)), pt(a.t.min(a.b)), pt(a.l.max(a.r)), pt(a.t.max(a.b)));
        let mut rect = Rect::new(l + self.page_w / 2.0 + x0, t + self.page_h / 2.0, rr + self.page_w / 2.0 + x0, b + self.page_h / 2.0);
        // Office stores the frame of a shape turned 45°–135° (or 225°–315°) with width and height
        // swapped; restore the unrotated frame around the same centre.
        let n = rotation.rem_euclid(180.0);
        if (45.0..135.0).contains(&n) {
            let c = rect.center();
            rect = Rect::from_center_size(c, (rect.height(), rect.width()));
        }
        rect
    }

    fn place(&mut self, sh: &escher::EShape, r: SpreadRef, depth: usize) -> Result<()> {
        let rotation = sh.prop(0x0004).map_or(0.0, |v| f64::from(v as i32) / 65_536.0);
        if sh.is_group && !sh.children.is_empty() {
            return self.place_group(sh, r, depth, rotation);
        }
        let rect = self.rect(sh.anchor, r, rotation);
        if !(rect.width() >= 0.0 && rect.height() >= 0.0 && rect.x0.is_finite() && rect.y0.is_finite()) {
            self.warnings.push("Objects with invalid geometry were skipped.".into());
            return Ok(());
        }
        let info = sh.seq.and_then(|s| self.c.shapes.get(&s)).cloned().unwrap_or_default();
        let text_id = info.text_id.filter(|id| self.q.stories.contains_key(id));
        let wordart = sh.complex.get(&0x00c0).map(|raw| utf16z(raw));
        let picture = sh.prop(0x0104).filter(|p| *p > 0);

        // Tables and straight WordArt keep rectangular frames. Curved WordArt uses its baseline path.
        let curved = wordart.as_ref().and_then(|text| Self::wordart_path(sh, rect, text));
        let (shape_kind, path) = if let Some(path) = curved {
            (Shape::Path, path)
        } else if text_id.is_some() && (info.is_table || sh.kind == 201) || wordart.is_some() {
            (Shape::Rectangle, shapes::rectangle(rect))
        } else {
            self.geometry(sh, rect)
        };
        // A text shown in several linked frames is imported once and threaded through them.
        let follower = text_id.is_some_and(|t| self.chains.contains_key(&t));
        let id = if let Some(tid) = text_id.filter(|_| !follower) {
            if info.table.is_some() {
                self.warnings.push("Publisher tables are not converted yet; their text was imported as a text frame.".into());
            } else if info.is_table {
                self.warnings.push("Unsupported Publisher table geometry was retained as text.".into());
            }
            self.text_frame(r, rect, tid)?
        } else if let Some(text) = &wordart {
            self.wordart_frame(r, rect, sh, text)?
        } else {
            let id = ItemId(self.doc.alloc());
            let mut item = Item::new(id, self.layer, shape_kind, path.clone());
            if let Some(pib) = picture.filter(|_| !follower) {
                item.content = self.picture_content(pib, rect, sh);
            }
            self.doc.insert_item(r, item, None).map_err(|e| PubError::Malformed(e.to_string()))?;
            id
        };
        if let Some(tid) = text_id {
            self.chains.entry(tid).or_default().push((info.chain_index, id));
        }
        let (shape_kind, path) = if wordart.is_some() && self.doc.item(id).and_then(Item::text_frame).is_some_and(|tf| tf.options.path.is_none()) {
            (Shape::Rectangle, shapes::rectangle(rect))
        } else {
            (shape_kind, path)
        };
        // WordArt's fill and line colour its letters (set by `wordart_frame`), not its frame.
        let (fill, stroke) =
            if text_id.is_none() && wordart.is_some() { (Fill::none(), Stroke::none()) } else { (self.fill(sh, picture.is_some()), self.stroke(sh)) };
        let insets = text_id.map(|_| {
            let v = |k: u16| sh.prop(k).map_or(0.0, |e| pt(f64::from(e as i32)).max(0.0));
            [v(0x0082), v(0x0081), v(0x0084), v(0x0083)]
        });
        let xf = transform(rect, rotation, sh.flip_h, sh.flip_v);
        if let Some(item) = self.doc.item_mut(id) {
            item.shape = shape_kind;
            item.path = path;
            item.fill = fill;
            item.stroke = stroke;
            item.xf = xf;
            let vj = match info.valign {
                Some(1) => VerticalJustification::Center,
                Some(2) => VerticalJustification::Bottom,
                _ => VerticalJustification::Top,
            };
            match (insets, &mut item.content) {
                (Some(inset), Content::Text(t)) => {
                    t.options.inset = inset;
                    t.options.vertical_justification = vj;
                }
                (Some(inset), _) if follower => self.followers.push((id, inset, vj)),
                _ => {}
            }
        }
        if wordart.is_none() {
            self.shadow(id, sh);
        }
        Ok(())
    }

    fn wordart_path(sh: &escher::EShape, rect: Rect, text: &str) -> Option<PathData> {
        // Non-default adjusts have no confirmed Publisher-to-ECMA unit conversion.
        // Multi-line geometry text remains unsupported.
        if sh.kind != 144 || (0x0147..=0x0150).any(|id| sh.prop(id).is_some()) || text.contains(['\r', '\n']) {
            return None;
        }
        wordart::default_path(sh.kind, rect)
    }

    /// WordArt (MS-ODRAW Geometry Text) as an editable text frame: its font, bold/italic,
    /// underline and strikethrough, alignment, the shape's fill and line as the letters' fill and
    /// stroke, and the shadow as a drop shadow. Default upward arch uses measured editable
    /// path text. Circle and other warps stay straight and centred.
    fn wordart_frame(&mut self, r: SpreadRef, rect: Rect, sh: &escher::EShape, raw: &str) -> Result<ItemId> {
        let text: String = raw.replace("\r\n", "\n").replace('\r', "\n").chars().filter(|c| *c == '\n' || !c.is_control()).collect();
        let curved = Self::wordart_path(sh, rect, &text);
        let (id, sid) = self.doc.add_text_frame(r, rect, self.layer, &text, ParaFormat::default()).map_err(|e| PubError::Malformed(e.to_string()))?;
        let fill = self.fill(sh, false);
        let stroke = self.stroke(sh);
        // Geometry Text booleans: each flag (bit n) counts only with its "use" bit (n + 16).
        let flags = sh.prop(0x00ff).unwrap_or(0);
        let flag = |bit: u32| flags & (1 << (bit + 16)) != 0 && flags & (1 << bit) != 0;
        let face = match (flag(5), flag(4)) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            _ => "Regular",
        };
        // Straight size: frame height per line, capped at an estimated 0.7 em per letter.
        let inset = 2.88;
        let lines = text.split('\n').count().max(1) as f64;
        let letters = text.split('\n').map(|l| l.chars().count()).max().unwrap_or(0).max(1);
        let longest = letters as f64;
        let by_height = (rect.height() - 2.0 * inset).max(0.0) / lines * 0.8;
        let by_width = (rect.width() - 2.0 * inset).max(0.0) / (longest * 0.7);
        let mut ops: Vec<(&'static str, serde_json::Value)> = vec![("fontStyle", json!(face))];
        if let Some(font) = sh.complex.get(&0x00c5).map(|f| utf16z(f)).filter(|f| !f.trim().is_empty()) {
            ops.push(("fontFamily", json!(font)));
        }
        let size = by_height.min(by_width);
        if size.is_finite() && size >= 0.1 {
            ops.push(("size", json!(size.min(1296.0))));
        }
        ops.push(("fill", json!(if fill.swatch.is_empty() { designcraft_color::swatch::NONE.to_string() } else { fill.swatch.clone() })));
        if stroke.weight > 0.0 && !stroke.swatch.is_empty() && stroke.swatch != designcraft_color::swatch::NONE {
            ops.push(("stroke", json!(stroke.swatch)));
            ops.push(("strokeWeight", json!(stroke.weight)));
        }
        if flag(3) {
            ops.push(("underline", json!(true)));
        }
        if flag(0) {
            ops.push(("strikethrough", json!(true)));
        }
        // gtextAlign: 2 left, 3 right; centred otherwise (stretch and justify included).
        let align = match sh.prop(0x00c2) {
            Some(2) => "left",
            Some(3) => "right",
            _ => "center",
        };
        let mut failed = false;
        if let Some(st) = self.doc.story_mut(sid) {
            let end = text.len();
            st.format_chars(0..end, |cf| {
                for (k, v) in &ops {
                    failed |= cf.over.set_json(k, v).is_err();
                }
            });
            st.format_paras(0..end, |pf| failed |= pf.para.set_json("align", &json!(align)).is_err());
        }
        if failed {
            self.warnings.push("Some WordArt formatting could not be applied.".into());
        }
        if let Some(item) = self.doc.item_mut(id)
            && let Content::Text(t) = &mut item.content
        {
            t.options.inset = [inset; 4];
            t.options.vertical_justification = VerticalJustification::Center;
        }
        let fit = curved.as_ref().and_then(|path| wordart::measure(&self.doc, id, path));
        if let Some(fit) = fit {
            if let Some(story) = self.doc.story_mut(sid) {
                story.format_chars(0..text.len(), |cf| {
                    cf.over.size = Some(fit.size);
                    cf.over.baseline_shift = Some(fit.baseline_shift);
                });
            }
            if let Some(tf) = self.doc.item_mut(id).and_then(Item::text_frame_mut) {
                tf.options.path = Some(designcraft_doc::PathType::default());
                tf.options.vertical_justification = VerticalJustification::Top;
                tf.options.inset = [0.0; 4];
            }
            self.warnings.push("Curved WordArt uses measured font advances; its appearance can differ from Publisher.".into());
        } else if sh.kind != 136 {
            self.warnings.push("WordArt shapes (arches, circles, waves) were imported as straight text.".into());
        }

        self.shadow(id, sh);
        Ok(id)
    }

    /// The confirmed simple OfficeArt shadow, shared by shapes and geometry text.
    fn shadow(&mut self, id: ItemId, sh: &escher::EShape) {
        if !sh.prop(0x023f).is_some_and(|v| v & 0x0002_0000 != 0 && v & 0x2 != 0) {
            return;
        }
        let kind = sh.prop(0x0200).unwrap_or(0);
        if kind != 0 {
            self.warnings.push(format!("Publisher shadow type {kind} is unconfirmed; the shadow was not imported."));
            return;
        }
        if sh.props.keys().any(|k| (0x0200..=0x023e).contains(k) && !matches!(k, 0x0200 | 0x0201 | 0x0204 | 0x0205 | 0x0206)) {
            self.warnings.push("Publisher shadow has unconfirmed transform or formatting fields; the shadow was not imported.".into());
            return;
        }
        let off = |k: u16| sh.prop(k).map_or(2.0, |v| pt(f64::from(v as i32)));
        let (dx, dy) = (off(0x0205), off(0x0206));
        if dx.abs() > 1_000.0 || dy.abs() > 1_000.0 {
            self.warnings.push("Publisher shadow offsets exceeded the 1000-point limit and were clipped.".into());
        }
        let (dx, dy) = (dx.clamp(-1_000.0, 1_000.0), dy.clamp(-1_000.0, 1_000.0));
        let Some(color) = self.color(sh.prop(0x0201).unwrap_or(0x0080_8080)) else { return };
        if let Some(item) = self.doc.item_mut(id) {
            let ds = &mut item.effects.drop_shadow;
            ds.on = true;
            ds.color = color;
            ds.opacity = sh.prop(0x0204).map_or(1.0, |o| (f64::from(o) / 65_536.0).clamp(0.0, 1.0) as f32);
            ds.distance = dx.hypot(dy);
            ds.angle = dy.atan2(-dx).to_degrees();
            ds.size = 0.0;
            ds.spread = 0.0;
        }
    }

    /// A group: its members (already in page coordinates, unrotated) placed as usual, then moved
    /// into one group item that carries the group's rotation and flips.
    fn place_group(&mut self, sh: &escher::EShape, r: SpreadRef, depth: usize, rotation: f64) -> Result<()> {
        if depth >= MAX_GROUP_DEPTH {
            self.warnings.push("Groups nested too deeply were skipped.".into());
            return Ok(());
        }
        let first = self.doc.spread_mut(r).map_or(0, |s| s.items.len());
        for c in &sh.children {
            self.place(c, r, depth + 1)?;
        }
        let frame = self.rect(sh.anchor, r, rotation);
        let Some(sp) = self.doc.spread_mut(r) else { return Ok(()) };
        let kids: Vec<Arc<Item>> = sp.items.drain(first.min(sp.items.len())..).collect();
        if kids.is_empty() {
            return Ok(());
        }
        let mut g = Item::new(ItemId(self.doc.alloc()), self.layer, Shape::Group, PathData::default());
        if frame.x0.is_finite() && frame.y0.is_finite() && frame.x1.is_finite() && frame.y1.is_finite() {
            g.xf = transform(frame, rotation, sh.flip_h, sh.flip_v);
        }
        g.fill = Fill::none();
        g.stroke = Stroke::none();
        g.content = Content::Group { items: kids };
        let id = g.id;
        self.doc.insert_item(r, g, None).map_err(|e| PubError::Malformed(e.to_string()))?;
        self.shadow(id, sh);
        Ok(())
    }

    /// Path in page space (before the item transform) for an Office shape type.
    fn geometry(&mut self, sh: &escher::EShape, r: Rect) -> (Shape, PathData) {
        let poly = |pts: &[(f64, f64)]| {
            let mut bp = BezPath::new();
            for (i, (x, y)) in pts.iter().enumerate() {
                let p = Point::new(r.x0 + x * r.width(), r.y0 + y * r.height());
                if i == 0 { bp.move_to(p) } else { bp.line_to(p) }
            }
            bp.close_path();
            (Shape::Polygon, PathData::from_bezpath(&bp))
        };
        let adj = |k: u16, default: f64| sh.prop(k).map_or(default, |v| f64::from(v as i32)) / 21_600.0;
        match sh.kind {
            1 | 75 | 202 => (Shape::Rectangle, shapes::rectangle(r)),
            2 => {
                let radius = r.width().min(r.height()) * adj(0x0147, 3_600.0);
                (Shape::Rectangle, shapes::rounded_rectangle(r, radius))
            }
            3 => (Shape::Oval, shapes::ellipse(r)),
            4 => poly(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
            5 => {
                let a = adj(0x0147, 10_800.0).clamp(0.0, 1.0);
                poly(&[(a, 0.0), (1.0, 1.0), (0.0, 1.0)])
            }
            6 => poly(&[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            7 => {
                let a = adj(0x0147, 5_400.0).clamp(0.0, 1.0);
                poly(&[(a, 0.0), (1.0, 0.0), (1.0 - a, 1.0), (0.0, 1.0)])
            }
            9 => {
                let a = adj(0x0147, 5_400.0).clamp(0.0, 0.5);
                poly(&[(a, 0.0), (1.0 - a, 0.0), (1.0, 0.5), (1.0 - a, 1.0), (a, 1.0), (0.0, 0.5)])
            }
            10 => {
                let a = adj(0x0147, 6_326.0).clamp(0.0, 0.5);
                poly(&[(a, 0.0), (1.0 - a, 0.0), (1.0, a), (1.0, 1.0 - a), (1.0 - a, 1.0), (a, 1.0), (0.0, 1.0 - a), (0.0, a)])
            }
            12 => {
                let mut pts = Vec::new();
                for i in 0..10 {
                    let ang = std::f64::consts::PI * (f64::from(i) / 5.0) - std::f64::consts::FRAC_PI_2;
                    let rad = if i % 2 == 0 { 0.5 } else { 0.5 * 0.381_966 };
                    pts.push((0.5 + rad * ang.cos(), 0.5 + rad * ang.sin()));
                }
                poly(&pts)
            }
            13 => {
                let (a, b2) = (adj(0x0147, 16_200.0).clamp(0.0, 1.0), adj(0x0148, 5_400.0).clamp(0.0, 0.5));
                poly(&[(0.0, b2), (a, b2), (a, 0.0), (1.0, 0.5), (a, 1.0), (a, 1.0 - b2), (0.0, 1.0 - b2)])
            }
            20 => {
                let (a, b) = (Point::new(r.x0, r.y0), Point::new(r.x1, r.y1));
                (Shape::GraphicLine, shapes::line(a, b))
            }
            74 => match presets::heart(r) {
                Ok(path) => (Shape::Path, path),
                Err(reason) => {
                    self.warnings.push(format!("The heart outline could not be drawn: {reason}; it was imported as a rectangle."));
                    (Shape::Rectangle, shapes::rectangle(r))
                }
            },
            0 => match paths::freeform(sh, r, &mut self.warnings) {
                Some(p) => (Shape::Path, p),
                None => {
                    self.warnings.push("Freeform shapes with unsupported segments were imported as rectangles.".into());
                    (Shape::Rectangle, shapes::rectangle(r))
                }
            },
            other => {
                self.warnings.push(format!("Shape type {other} is not supported yet; it was imported as a rectangle of the same size."));
                (Shape::Rectangle, shapes::rectangle(r))
            }
        }
    }

    fn color(&mut self, office: u32) -> Option<String> {
        let rgb = self.rgb(office);
        Some(self.swatch(rgb))
    }

    fn rgb(&mut self, office: u32) -> [u8; 3] {
        match office >> 24 {
            0x00 => contents::rgb_of(office),
            0x08 => match self.c.palette.get((office & 0xff_ffff) as usize) {
                Some(c) => *c,
                None => {
                    self.warnings.push("A colour referred to a missing palette entry; black was used.".into());
                    [0, 0, 0]
                }
            },
            _ => {
                self.warnings.push("Scheme and system colours were approximated with black.".into());
                [0, 0, 0]
            }
        }
    }

    fn swatch(&mut self, [r, g, b]: [u8; 3]) -> String {
        let name = format!("R={r} G={g} B={b}");
        if !self.doc.swatches.iter().any(|s| s.name == name) {
            let mut s = designcraft_color::Swatch::color(&name, designcraft_color::Color::rgb8(r, g, b));
            s.named = false;
            self.doc.swatches.push(s);
        }
        name
    }

    fn fill(&mut self, sh: &escher::EShape, picture: bool) -> Fill {
        // fillStyle booleans: fFilled (bit 4) with its "use" bit (bit 20).
        let filled = sh.prop(0x01bf).map_or(!picture && sh.prop(0x0181).is_some(), |v| v & 0x0010_0000 == 0 || v & 0x10 != 0);
        if !filled || picture {
            return Fill::none();
        }
        match sh.prop(0x0180).unwrap_or(0) {
            0 => {}
            3 => self.warnings.push("Picture fills were not imported.".into()),
            kind @ 4..=8 => return self.gradient_fill(sh, kind),
            _ => self.warnings.push("Pattern and texture fills were approximated with their first colour.".into()),
        }
        if sh.prop(0x0182).is_some_and(|o| o < 0x1_0000) {
            if sh.prop(0x0180).unwrap_or(0) == 0 {
                // A uniform gradient retains fill-only opacity; Item.opacity would also fade
                // the border and text. See docs/pub-format/drawing-details.md.
                let rgb = self.rgb(sh.prop(0x0181).unwrap_or(0x00ff_ffff));
                let opacity = sh.prop(0x0182).map_or(1.0, |o| o as f32 / 65_536.0);
                let value = designcraft_color::SwatchValue::Gradient {
                    gradient: designcraft_color::Gradient {
                        kind: designcraft_color::GradientKind::Linear,
                        stops: [0.0, 1.0]
                            .into_iter()
                            .map(|offset| designcraft_color::GradientStop {
                                offset,
                                color: designcraft_color::Color::rgb8(rgb[0], rgb[1], rgb[2]),
                                opacity,
                                midpoint: 0.5,
                                swatch: None,
                            })
                            .collect(),
                    },
                };
                if let Some(swatch) = self.doc.swatches.iter().find(|s| s.value == value) {
                    return Fill::swatch(&swatch.name);
                }
                let name = format!("Publisher Transparent Fill {}", self.doc.swatches.len());
                self.doc.swatches.push(designcraft_color::Swatch { name: name.clone(), value, locked: false, named: true, hidden: false });
                return Fill::swatch(&name);
            }
            self.warnings.push("Fill transparency was not imported.".into());
        }
        let c = sh.prop(0x0181).unwrap_or(0x00ff_ffff);
        self.color(c).map_or_else(Fill::none, |s| Fill::swatch(&s))
    }

    /// MS-ODRAW shaded fills (fillType 4–8) as a gradient swatch. fillFocus is where the last
    /// colour (fillBackColor) sits, -100..100 % along the shade; negative swaps the colours.
    /// fillShadeColors, when present, gives all the stops.
    fn gradient_fill(&mut self, sh: &escher::EShape, kind: u32) -> Fill {
        let opacity = |k: u16| sh.prop(k).map_or(1.0, |o| (f64::from(o) / 65_536.0).clamp(0.0, 1.0) as f32);
        let first = (self.rgb(sh.prop(0x0181).unwrap_or(0x00ff_ffff)), opacity(0x0182));
        let last = (self.rgb(sh.prop(0x0183).unwrap_or(0x00ff_ffff)), opacity(0x0184));
        let mut stops: Vec<(f32, [u8; 3], f32)> = Vec::new();
        if let Some(raw) = sh.complex.get(&0x0197) {
            for e in array(raw).unwrap_or_default().into_iter().take(64) {
                let (Ok(c), Ok(at)) = (read::u32_at(e, 0), read::i32_at(e, 4)) else { continue };
                stops.push(((f64::from(at) / 65_536.0).clamp(0.0, 1.0) as f32, self.rgb(c), 1.0));
            }
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        if stops.len() < 2 {
            let focus = sh.prop(0x018c).map_or(0, |f| (f as i32).clamp(-100, 100));
            let (a, b) = if focus < 0 { (last, first) } else { (first, last) };
            let at = focus.unsigned_abs() as f32 / 100.0;
            stops = if at == 0.0 { vec![(0.0, b.0, b.1), (1.0, a.0, a.1)] } else { vec![(0.0, a.0, a.1), (at, b.0, b.1)] };
            if at > 0.0 && at < 1.0 {
                stops.push((1.0, a.0, a.1));
            }
        }
        let radial = !matches!(kind, 4 | 7);
        if radial {
            self.warnings.push("Gradients from the centre or the shape outline were approximated with radial gradients.".into());
        }
        let gradient = designcraft_color::Gradient {
            kind: if radial { designcraft_color::GradientKind::Radial } else { designcraft_color::GradientKind::Linear },
            stops: stops
                .iter()
                .map(|&(offset, [r, g, b], opacity)| designcraft_color::GradientStop {
                    offset,
                    color: designcraft_color::Color::rgb8(r, g, b),
                    opacity,
                    midpoint: 0.5,
                    swatch: None,
                })
                .collect(),
        };
        let value = designcraft_color::SwatchValue::Gradient { gradient };
        let name = match self.doc.swatches.iter().find(|s| s.value == value) {
            Some(s) => s.name.clone(),
            None => {
                let n = self.doc.swatches.iter().filter(|s| matches!(s.value, designcraft_color::SwatchValue::Gradient { .. })).count();
                let name = format!("Publisher Gradient {}", n + 1);
                self.doc.swatches.push(designcraft_color::Swatch { name: name.clone(), value, locked: false, named: true, hidden: false });
                name
            }
        };
        // fillAngle: 0° runs bottom to top (DesignCraft's 90°); Office angles turn clockwise.
        let angle = sh.prop(0x018b).map_or(0.0, |a| f64::from(a as i32) / 65_536.0);
        let mut fill = Fill::swatch(&name);
        if !radial && angle.is_finite() {
            fill.gradient_angle = Some((90.0 - angle).rem_euclid(360.0));
        }
        fill
    }

    fn stroke(&mut self, sh: &escher::EShape) -> Stroke {
        // lineStyle booleans: fLine (bit 3) with its "use" bit (bit 19).
        let line = sh.prop(0x01ff).map_or(sh.kind == 20, |v| v & 0x0008_0000 == 0 || v & 0x8 != 0);
        if !line {
            return Stroke::none();
        }
        let weight = sh.prop(0x01cb).map_or(0.75, |w| pt(f64::from(w as i32)).clamp(0.0, 1_000.0));
        let swatch = self.color(sh.prop(0x01c0).unwrap_or(0)).unwrap_or_else(|| designcraft_color::swatch::BLACK.into());
        // lineDashing (MSOLINEDASHING): dash and gap lengths in line widths.
        let units: &[f64] = match sh.prop(0x01ce).unwrap_or(0) {
            0 => &[],
            1 => &[3.0, 1.0],
            2 => &[1.0, 1.0],
            3 => &[3.0, 1.0, 1.0, 1.0],
            4 => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            5 => &[1.0, 3.0],
            6 => &[4.0, 3.0],
            7 => &[8.0, 3.0],
            8 => &[4.0, 3.0, 1.0, 3.0],
            9 => &[8.0, 3.0, 1.0, 3.0],
            10 => &[8.0, 3.0, 1.0, 3.0, 1.0, 3.0],
            _ => {
                self.warnings.push("Unknown dash styles were imported as solid lines.".into());
                &[]
            }
        };
        let kind =
            if units.is_empty() { StrokeType::Solid } else { StrokeType::Dashed { pattern: units.iter().map(|u| u * weight.max(0.25)).collect() } };
        let start = self.arrowhead(sh.prop(0x01d0), sh.prop(0x01d2));
        let end = self.arrowhead(sh.prop(0x01d1), sh.prop(0x01d4));
        Stroke { swatch, weight, kind, start, end, ..Stroke::default() }
    }

    /// lineStart/EndArrowhead (MSOLINEEND) with its width (0 narrow, 1 medium, 2 wide).
    fn arrowhead(&mut self, head: Option<u32>, width: Option<u32>) -> Arrowhead {
        let wide = width == Some(2);
        match head.unwrap_or(0) {
            0 => Arrowhead::None,
            1 if wide => Arrowhead::TriangleWide,
            1 => Arrowhead::Triangle,
            2 => Arrowhead::Barbed,
            3 => {
                self.warnings.push("Diamond arrowheads were imported as squares.".into());
                Arrowhead::SquareSolid
            }
            4 => Arrowhead::CircleSolid,
            5 if wide => Arrowhead::SimpleWide,
            5 => Arrowhead::Simple,
            _ => {
                self.warnings.push("Unknown arrowheads were not imported.".into());
                Arrowhead::None
            }
        }
    }

    fn picture_content(&mut self, pib: u32, frame: Rect, sh: &escher::EShape) -> Content {
        let asset = match self.pictures.get(&pib) {
            Some(a) => *a,
            None => {
                let a = self.picture_asset(pib);
                self.pictures.insert(pib, a);
                a
            }
        };
        let Some(asset) = asset else { return Content::Unassigned };
        let Some(px) = self.doc.assets.get(&asset).and_then(|a| a.pixels) else { return Content::Unassigned };
        let size = (f64::from(px.0.max(1)), f64::from(px.1.max(1)));
        // Crops (cropFrom{Top,Bottom,Left,Right}): 16.16 fractions of the picture.
        let frac = |k: u16| sh.prop(k).map_or(0.0, |v| f64::from(v as i32) / 65_536.0);
        let (ct, cb, cl, cr) = (frac(0x0100), frac(0x0101), frac(0x0102), frac(0x0103));
        let shown_w = (1.0 - cl - cr).max(1e-3);
        let shown_h = (1.0 - ct - cb).max(1e-3);
        let sx = frame.width() / (size.0 * shown_w);
        let sy = frame.height() / (size.1 * shown_h);
        let xf = Affine::translate((frame.x0 - cl * size.0 * sx, frame.y0 - ct * size.1 * sy)) * Affine::scale_non_uniform(sx, sy);
        Content::Graphic(Graphic {
            layer_options: None,
            asset,
            size,
            xf,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Fill::none(),
        })
    }

    fn picture_asset(&mut self, pib: u32) -> Option<AssetId> {
        let Some(p) = self.dr.pictures.get(&pib) else {
            self.warnings.push("A picture referred to a missing picture record; its frame was kept empty.".into());
            return None;
        };
        let data = match p.kind {
            0xF01F => dib_to_bmp(&p.data)?,
            0xF01A | 0xF01B => {
                self.warnings.push("WMF and EMF pictures are not converted yet; their frames were kept empty.".into());
                return None;
            }
            0xF01C => {
                self.warnings.push("PICT pictures are not supported yet; their frames were kept empty.".into());
                return None;
            }
            _ => p.data.clone(),
        };
        let mime = designcraft_images::mime(&data);
        let Some(pixels) = designcraft_images::pixel_size(&data) else {
            self.warnings.push("A picture could not be decoded; its frame was kept empty.".into());
            return None;
        };
        let id = AssetId(self.doc.alloc());
        let ext = mime.rsplit('/').next().unwrap_or("bin");
        Some(self.doc.add_asset(Asset {
            id,
            name: format!("picture-{pib}.{ext}"),
            mime: mime.into(),
            data: Arc::new(data),
            pixels: Some(pixels),
            ..Default::default()
        }))
    }

    fn text_frame(&mut self, r: SpreadRef, rect: Rect, tid: u32) -> Result<ItemId> {
        let Some(qs) = self.q.stories.get(&tid) else { return Err(PubError::Malformed("missing story".into())) };
        // UTF-16 → UTF-8, keeping each unit's byte offset; Publisher ends every paragraph with CR.
        let mut text = String::new();
        let mut at = Vec::with_capacity(qs.units.len() + 1);
        let mut units = qs.units.clone();
        if units.last() == Some(&0x0d) {
            units.pop();
        }
        let mut skipped = false;
        for res in char::decode_utf16(units.iter().copied()) {
            let len16 = res.as_ref().map_or(1, |c| c.len_utf16());
            for _ in 0..len16 {
                at.push(text.len());
            }
            match res {
                Ok('\r') => text.push('\n'),
                Ok('\u{000b}') => text.push(designcraft_doc::FORCED_LINE_BREAK),
                Ok('\t') => text.push('\t'),
                Ok(c) if (c as u32) < 0x20 => skipped = true,
                Ok(c) => text.push(c),
                Err(_) => text.push(char::REPLACEMENT_CHARACTER),
            }
        }
        at.push(text.len());
        if skipped {
            self.warnings.push("Fields and special characters (page numbers, dates) were left out of the text.".into());
        }
        let (id, sid) = self.doc.add_text_frame(r, rect, self.layer, &text, ParaFormat::default()).map_err(|e| PubError::Malformed(e.to_string()))?;
        let byte = |u: usize| at.get(u.min(at.len().saturating_sub(1))).copied().unwrap_or(0);
        let defaults = self.q.default_char.clone();
        let mut char_ops = Vec::new();
        let runs: Vec<(std::ops::Range<usize>, quill::CharStyle)> =
            if qs.chars.is_empty() { vec![(0..units.len(), defaults.clone())] } else { qs.chars.clone() };
        for (range, style) in runs {
            let (a, z) = (byte(range.start), byte(range.end));
            if z <= a {
                continue;
            }
            let mut ops: Vec<(&'static str, serde_json::Value)> = Vec::new();
            let font = style.font.or(defaults.font).and_then(|f| self.q.fonts.get(f as usize)).cloned();
            if let Some(f) = font.filter(|f| !f.is_empty()) {
                ops.push(("fontFamily", json!(f)));
            }
            let (bold, italic) = (style.bold.or(defaults.bold).unwrap_or(false), style.italic.or(defaults.italic).unwrap_or(false));
            let face = match (bold, italic) {
                (true, true) => "Bold Italic",
                (true, false) => "Bold",
                (false, true) => "Italic",
                _ => "Regular",
            };
            ops.push(("fontStyle", json!(face)));
            if let Some(s) = style.size_pt.or(defaults.size_pt).filter(|s| (0.1..=1296.0).contains(s)) {
                ops.push(("size", json!(s)));
            }
            if style.underline.or(defaults.underline).unwrap_or(false) {
                ops.push(("underline", json!(true)));
            }
            if let Some(ci) = style.color.or(defaults.color)
                && let Some(office) = self.q.colors.get(ci as usize).copied()
                && let Some(sw) = self.color(office)
            {
                ops.push(("fill", json!(sw)));
            }
            if let Some(value) = style.capitalization.or(defaults.capitalization) {
                ops.push(("capitalization", json!(value)));
            }
            if let Some(value) = style.position.or(defaults.position) {
                ops.push(("position", json!(value)));
            }
            if let Some(value) = style.tracking.or(defaults.tracking).filter(|v| v.abs() <= 10_000.0) {
                ops.push(("tracking", json!(value)));
            }
            char_ops.push((a..z, ops));
        }
        let defaults_p = self.q.default_para.clone();
        let mut para_ops = Vec::new();
        let para_runs = if qs.paras.is_empty() { vec![(0..units.len(), defaults_p.clone())] } else { qs.paras.clone() };
        for (range, style) in &para_runs {
            let (a, z) = (byte(range.start), byte(range.end.saturating_sub(1)).max(byte(range.start)));
            let align = match style.align.or(defaults_p.align) {
                Some(1) => "right",
                Some(2) => "center",
                Some(6) => "leftJustified",
                _ => "left",
            };
            let mut ops: Vec<(&'static str, serde_json::Value)> = vec![("align", json!(align))];
            for (k, v) in [
                ("spaceBefore", style.space_before_pt.or(defaults_p.space_before_pt)),
                ("spaceAfter", style.space_after_pt.or(defaults_p.space_after_pt)),
                ("firstLineIndent", style.first_indent_pt.or(defaults_p.first_indent_pt)),
                ("leftIndent", style.left_indent_pt.or(defaults_p.left_indent_pt)),
                ("rightIndent", style.right_indent_pt.or(defaults_p.right_indent_pt)),
            ] {
                if let Some(v) = v.filter(|v| v.is_finite() && v.abs() < 10_000.0) {
                    ops.push((k, json!(v)));
                }
            }
            if let Some(bullet) = style.bullet.as_ref().or(defaults_p.bullet.as_ref()) {
                ops.push(("listType", json!("bullets")));
                if let Some(character) = bullet.character.or_else(|| defaults_p.bullet.as_ref().and_then(|b| b.character)) {
                    ops.push(("bulletChar", json!(character.to_string())));
                }
                if let Some(font) = bullet.font.or_else(|| defaults_p.bullet.as_ref().and_then(|b| b.font)).and_then(|f| self.q.fonts.get(f as usize))
                {
                    ops.push(("bulletFont", json!(font)));
                }
            }
            let spacing = style.line_spacing.or(defaults_p.line_spacing);
            if let Some(quill::LineSpacing::Multiple(factor)) = spacing {
                ops.push(("autoLeading", json!(factor)));
            }
            para_ops.push((a..z, ops, spacing));
        }
        let mut failed = false;
        if let Some(st) = self.doc.story_mut(sid) {
            for (range, ops) in char_ops {
                st.format_chars(range, |cf| {
                    for (k, v) in &ops {
                        failed |= cf.over.set_json(k, v).is_err();
                    }
                });
            }
            for (range, ops, spacing) in para_ops {
                st.format_paras(range, |pf| {
                    if let Some(spacing) = spacing {
                        pf.chars.leading = Some(match spacing {
                            quill::LineSpacing::Exact(points) => designcraft_doc::Leading::Points(points),
                            quill::LineSpacing::Multiple(_) => designcraft_doc::Leading::Auto,
                        });
                    }
                    for (k, v) in &ops {
                        failed |= pf.para.set_json(k, v).is_err();
                    }
                });
            }
        }
        if failed {
            self.warnings.push("Some text formatting could not be applied.".into());
        }
        Ok(id)
    }
}

/// Rotation (clockwise degrees) and flips about the frame's centre.
fn transform(r: Rect, rotation: f64, flip_h: bool, flip_v: bool) -> Affine {
    let c = r.center();
    let mut xf = Affine::IDENTITY;
    if flip_h || flip_v {
        xf = Affine::translate(c.to_vec2())
            * Affine::scale_non_uniform(if flip_h { -1.0 } else { 1.0 }, if flip_v { -1.0 } else { 1.0 })
            * Affine::translate(-c.to_vec2());
    }
    if rotation != 0.0 && rotation.is_finite() {
        xf = Affine::rotate_about(rotation.to_radians(), c) * xf;
    }
    xf
}

/// An MS-ODRAW array property: u16 count, u16 count in memory, u16 element size (0xFFF0 = 4),
/// then the elements.
fn array(raw: &[u8]) -> Option<Vec<&[u8]>> {
    let n = usize::from(read::u16_at(raw, 0).ok()?);
    let mut size = usize::from(read::u16_at(raw, 4).ok()?);
    if size == 0xfff0 {
        size = 4;
    }
    if size == 0 || n > 1_000_000 {
        return None;
    }
    (0..n).map(|i| raw.get(i.checked_mul(size)?.checked_add(6)?..).and_then(|r| r.get(..size))).collect()
}

fn utf16z(raw: &[u8]) -> String {
    let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    String::from_utf16_lossy(&units)
}

/// A packed DIB (no file header) as a BMP file.
fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    let header = read::u32_at(dib, 0).ok()? as usize;
    let bits = read::u16_at(dib, 14).ok()?;
    let mut colors = read::u32_at(dib, 32).ok()? as usize;
    if colors == 0 && bits <= 8 {
        colors = 1usize << bits;
    }
    // Checked: `header` is read from the file (a 32-bit `usize` overflows on wasm32).
    let offset = header.checked_add(14 + 4 * colors.min(256))?;
    let total = dib.len().checked_add(14)?;
    let mut out = Vec::with_capacity(total);
    out.extend(b"BM");
    out.extend(u32::try_from(total).ok()?.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(u32::try_from(offset).ok()?.to_le_bytes());
    out.extend(dib);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_outline_errors_and_unknown_types_keep_a_warned_rectangle() {
        let c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        let q = quill::Quill::default();
        let dr = escher::Drawing::default();
        let mut builder = Builder::new(&c, &q, &dr).unwrap();
        let heart = escher::EShape { kind: 74, ..Default::default() };
        let frame = Rect::new(10.0, 20.0, 106.0, 140.0);
        assert_eq!(builder.geometry(&heart, frame).0, Shape::Path);
        assert!(builder.warnings.is_empty());
        assert_eq!(builder.geometry(&heart, Rect::new(10.0, 20.0, 0.0, 140.0)).0, Shape::Rectangle);
        assert!(builder.warnings.last().unwrap().contains("heart outline could not be drawn"));
        let unknown = escher::EShape { kind: 8, ..Default::default() };
        let (shape, path) = builder.geometry(&unknown, frame);
        assert_eq!(shape, Shape::Rectangle);
        assert_eq!(path, shapes::rectangle(frame));
        assert_eq!(builder.warnings.last().unwrap(), "Shape type 8 is not supported yet; it was imported as a rectangle of the same size.");
    }

    fn frame(seq: u32, left_pt: f64) -> escher::EShape {
        let e = |p: f64| p * EMU_PER_PT;
        escher::EShape {
            seq: Some(seq),
            kind: 202,
            anchor: escher::Emu { l: e(left_pt), t: e(-100.0), r: e(left_pt + 90.0), b: e(100.0) },
            ..Default::default()
        }
    }

    #[test]
    fn linked_frames_share_one_story_in_chain_order() {
        let mut c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        c.pages.push(contents::PageInfo { seq: 10, shapes: vec![1, 2, 3], ..Default::default() });
        // Drawn out of chain order: the middle frame first, then the first, then the last.
        for (seq, chain_index) in [(1, 1), (2, 0), (3, 2)] {
            c.shapes.insert(seq, contents::ShapeInfo { text_id: Some(7), chain_index, ..Default::default() });
        }
        let mut q = quill::Quill::default();
        q.stories.insert(7, quill::QStory { units: "One\rTwo\rThree\r".encode_utf16().collect(), ..Default::default() });
        // Chain order left to right: seq 2 at -250 pt, seq 1 at -50 pt, seq 3 at 150 pt.
        let dr = escher::Drawing { shapes: vec![frame(1, -50.0), frame(2, -250.0), frame(3, 150.0)], ..Default::default() };
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        b.build().unwrap();
        let d = b.doc;
        let frames: Vec<&Item> = d.spreads.iter().flat_map(|s| s.items.iter().map(|i| &**i)).filter(|i| i.is_text_frame()).collect();
        assert_eq!(frames.len(), 3);
        let stories: HashSet<_> = frames.iter().filter_map(|i| i.text_frame().map(|t| t.story)).collect();
        assert_eq!(stories.len(), 1, "linked frames must share one story");
        let story = d.stories.get(stories.iter().next().unwrap()).unwrap();
        assert_eq!(story.text, "One\nTwo\nThree");
        let xs: Vec<f64> = story.frames.iter().map(|id| d.item(*id).unwrap().inner_bounds().x0).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "frames out of chain order: {xs:?}");
    }

    #[test]
    fn groups_import_as_groups_with_their_rotation() {
        let mut c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        c.pages.push(contents::PageInfo { seq: 10, shapes: vec![5, 9], ..Default::default() });
        let e = |p: f64| p * EMU_PER_PT;
        let shape = |seq: u32, l: f64, t: f64, r: f64, b: f64| escher::EShape {
            seq: Some(seq),
            kind: 1,
            anchor: escher::Emu { l: e(l), t: e(t), r: e(r), b: e(b) },
            ..Default::default()
        };
        // An outer group turned 30° holding a rectangle and an inner group of two rectangles.
        let mut inner = shape(0, 0.0, 0.0, 100.0, 50.0);
        inner.is_group = true;
        inner.children = vec![shape(0, 0.0, 0.0, 40.0, 50.0), shape(0, 60.0, 0.0, 100.0, 50.0)];
        let mut outer = shape(5, -200.0, -100.0, 100.0, 50.0);
        outer.is_group = true;
        outer.props.insert(0x0004, 30 << 16);
        outer.children = vec![shape(0, -200.0, -100.0, -150.0, -50.0), inner];
        let lone = shape(9, 150.0, 150.0, 200.0, 200.0);
        let dr = escher::Drawing { shapes: vec![outer, lone], ..Default::default() };
        let q = quill::Quill::default();
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        b.build().unwrap();
        assert!(!b.warnings.iter().any(|w| w.contains("Group")), "{:?}", b.warnings);
        let top: Vec<&Item> = b.doc.spreads.iter().flat_map(|s| s.items.iter().map(|i| &**i)).collect();
        assert_eq!(top.len(), 2, "the group and the lone rectangle");
        let Content::Group { items } = &top[0].content else { panic!("first item is not a group") };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1].content, Content::Group { items } if items.len() == 2), "nested group kept");
        // The group turns 30° clockwise about the centre of its frame (106..406 × 296..446 pt).
        let centre = Point::new(306.0 - 50.0, 396.0 - 25.0);
        let want = Affine::rotate_about(30f64.to_radians(), centre);
        let got = top[0].xf.as_coeffs();
        assert!(got.iter().zip(want.as_coeffs()).all(|(a, b)| (a - b).abs() < 1e-6), "{got:?}");
        assert!(matches!(top[1].content, Content::Unassigned));
    }

    fn styled(props: &[(u16, u32)]) -> escher::EShape {
        escher::EShape { kind: 1, props: props.iter().copied().collect(), ..Default::default() }
    }

    #[test]
    fn simple_shape_shadows_keep_color_opacity_and_signed_offsets() {
        let c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        let (q, dr) = (quill::Quill::default(), escher::Drawing::default());
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        for (dx, dy) in [(2.0, 2.0), (-3.0, 4.0), (0.0, 0.0), (6.0, -6.0)] {
            let mut sh = frame(1, -100.0);
            sh.kind = 1;
            sh.props = [(0x023f, 0x0002_0002), (0x0201, 0x0000_00ff), (0x0204, 0x8000)].into();
            if (dx, dy) != (2.0, 2.0) {
                sh.props.insert(0x0205, (dx * 12_700.0) as i32 as u32);
                sh.props.insert(0x0206, (dy * 12_700.0) as i32 as u32);
            }
            b.place(&sh, SpreadRef::Doc(0), 0).unwrap();
            let item = b.doc.spreads[0].items.last().unwrap();
            let ds = &item.effects.drop_shadow;
            assert!(ds.on);
            assert_eq!(ds.color, "R=255 G=0 B=0");
            assert_eq!(ds.opacity, 0.5);
            assert_eq!((ds.size, ds.spread), (0.0, 0.0));
            assert!((-ds.angle.to_radians().cos() * ds.distance - dx).abs() < 1e-9);
            assert!((ds.angle.to_radians().sin() * ds.distance - dy).abs() < 1e-9);
            assert_eq!(item.opacity, 1.0);
        }
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    }

    #[test]
    fn unconfirmed_shape_shadows_warn_and_disabled_shadows_do_not() {
        let c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        let (q, dr) = (quill::Quill::default(), escher::Drawing::default());
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        for (props, warning) in [
            (vec![(0x023f, 0x0002_0002), (0x0200, 1)], true),
            (vec![(0x023f, 0x0002_0002), (0x0200, u32::MAX)], true),
            (vec![(0x023f, 0x0002_0002), (0x0207, 0)], true),
            (vec![(0x023f, 0x0002_0000), (0x0200, 1)], false),
            (vec![(0x023f, 2), (0x0200, 1)], false),
        ] {
            let mut sh = frame(1, -100.0);
            sh.kind = 1;
            sh.props = props.into_iter().collect();
            b.warnings.clear();
            b.place(&sh, SpreadRef::Doc(0), 0).unwrap();
            assert!(!b.doc.spreads[0].items.last().unwrap().effects.drop_shadow.on);
            assert_eq!(b.warnings.len(), usize::from(warning));
        }
        let mut sh = frame(1, -100.0);
        sh.kind = 1;
        sh.props = [(0x023f, 0x0002_0002), (0x0205, i32::MAX as u32), (0x0206, i32::MIN as u32)].into();
        b.place(&sh, SpreadRef::Doc(0), 0).unwrap();
        let ds = &b.doc.spreads[0].items.last().unwrap().effects.drop_shadow;
        assert!(ds.on && ds.distance.is_finite());
        assert!((ds.distance - 1_000f64.hypot(1_000.0)).abs() < 1e-9);
        assert!(b.warnings[0].contains("clipped"));
    }

    #[test]
    fn solid_fill_opacity_preserves_color_without_fading_border() {
        let c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        let (q, dr) = (quill::Quill::default(), escher::Drawing::default());
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        for opacity in [0, 0x8000, 0xffff] {
            let sh = styled(&[(0x0181, 0x00ff_0000), (0x0182, opacity), (0x01ff, 0x0008_0008)]);
            let f = b.fill(&sh, false);
            let gradient = designcraft_color::swatch::resolve_gradient(&b.doc.swatches, &f.swatch).unwrap();
            assert_eq!(gradient.kind, designcraft_color::GradientKind::Linear);
            assert_eq!(gradient.stops.len(), 2);
            for (stop, at) in gradient.stops.iter().zip([0.0, 1.0]) {
                assert_eq!(stop.offset, at);
                assert_eq!(stop.color.to_rgba8(1.0), [0, 0, 255, 255]);
                assert_eq!(stop.opacity, opacity as f32 / 65_536.0);
            }
            let count = b.doc.swatches.len();
            assert_eq!(b.fill(&sh, false), f);
            assert_eq!(b.doc.swatches.len(), count);
            let stroke = b.stroke(&sh);
            assert_ne!(stroke.swatch, designcraft_color::swatch::NONE);
        }
        for opacity in [None, Some(0x1_0000)] {
            let mut sh = styled(&[(0x0181, 0x00ff_0000)]);
            if let Some(opacity) = opacity {
                sh.props.insert(0x0182, opacity);
            }
            let f = b.fill(&sh, false);
            assert!(designcraft_color::swatch::resolve_gradient(&b.doc.swatches, &f.swatch).is_none());
        }
        let sh = styled(&[(0x0181, 0x00ff_0000), (0x0182, 0x8000), (0x01bf, 0x0010_0000)]);
        assert_eq!(b.fill(&sh, false), Fill::none());
        assert_eq!(b.fill(&sh, true), Fill::none());
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    }

    #[test]
    fn shaded_fills_become_gradient_swatches() {
        let c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        let (q, dr) = (quill::Quill::default(), escher::Drawing::default());
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        // pubsmith fill__gradient: shadeScale, blue to red, focus 100 %, angle 0.
        let f = b.fill(&styled(&[(0x0180, 7), (0x0181, 0x00ff_0000), (0x0183, 0x0000_00ff), (0x018c, 100)]), false);
        assert_eq!(f.gradient_angle, Some(90.0));
        let g = designcraft_color::swatch::resolve_gradient(&b.doc.swatches, &f.swatch).unwrap();
        let stops: Vec<(f32, [u8; 4])> = g.stops.iter().map(|s| (s.offset, s.color.to_rgba8(1.0))).collect();
        assert_eq!(stops, vec![(0.0, [0, 0, 255, 255]), (1.0, [255, 0, 0, 255])]);
        // Focus 50 % reflects; focus 0 starts with the last colour; the same gradient is reused.
        let f2 = b.fill(&styled(&[(0x0180, 4), (0x0181, 0x00ff_0000), (0x0183, 0x0000_00ff), (0x018c, 50), (0x018b, 45 << 16)]), false);
        assert_eq!(f2.gradient_angle, Some(45.0));
        let g2 = designcraft_color::swatch::resolve_gradient(&b.doc.swatches, &f2.swatch).unwrap();
        assert_eq!(g2.stops.iter().map(|s| s.offset).collect::<Vec<_>>(), vec![0.0, 0.5, 1.0]);
        let f3 = b.fill(&styled(&[(0x0180, 7), (0x0181, 0x00ff_0000), (0x0183, 0x0000_00ff), (0x018c, 100)]), false);
        assert_eq!(f3.swatch, f.swatch);
        // fillShadeColors: three stops from the complex property.
        let mut sh = styled(&[(0x0180, 4), (0x0181, 0)]);
        let mut raw = vec![3, 0, 3, 0, 8, 0];
        for (c, at) in [(0x0000_00ffu32, 0i32), (0x0000_ff00, 0x8000), (0x00ff_0000, 0x1_0000)] {
            raw.extend(c.to_le_bytes());
            raw.extend(at.to_le_bytes());
        }
        sh.complex.insert(0x0197, raw);
        let f4 = b.fill(&sh, false);
        let g4 = designcraft_color::swatch::resolve_gradient(&b.doc.swatches, &f4.swatch).unwrap();
        assert_eq!(g4.stops.len(), 3);
        assert_eq!(g4.stops[1].color.to_rgba8(1.0), [0, 255, 0, 255]);
        assert!(!b.warnings.iter().any(|w| w.contains("approximated")), "{:?}", b.warnings);
    }

    #[test]
    fn dashes_and_arrowheads_come_over() {
        let c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        let (q, dr) = (quill::Quill::default(), escher::Drawing::default());
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        // 2 pt dashGEL line with a triangle at the end and a wide open arrow at the start.
        let s = b.stroke(&styled(&[(0x01ff, 0x0008_0008), (0x01cb, 25_400), (0x01ce, 6), (0x01d1, 1), (0x01d0, 5), (0x01d2, 2)]));
        assert_eq!(s.kind, StrokeType::Dashed { pattern: vec![8.0, 6.0] });
        assert_eq!((s.start, s.end), (Arrowhead::SimpleWide, Arrowhead::Triangle));
        let dot = b.stroke(&styled(&[(0x01ff, 0x0008_0008), (0x01ce, 2)]));
        assert_eq!(dot.kind, StrokeType::Dashed { pattern: vec![0.75, 0.75] });
        let plain = b.stroke(&styled(&[(0x01ff, 0x0008_0008)]));
        assert_eq!((plain.kind, plain.start, plain.end), (StrokeType::Solid, Arrowhead::None, Arrowhead::None));
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    }
    #[test]
    fn wordart_becomes_styled_editable_text() {
        let c = contents::Contents { width_emu: 612 * 12_700, height_emu: 792 * 12_700, ..Default::default() };
        let (q, dr) = (quill::Quill::default(), escher::Drawing::default());
        let mut b = Builder::new(&c, &q, &dr).unwrap();
        // Red fill, 1.5 pt white line, bold + italic, shadow 3 pt right and 4 pt down, curved shape.
        let mut sh = styled(&[
            (0x00ff, 0x0030_0030),
            (0x0181, 0x0000_00c8),
            (0x01ff, 0x0008_0008),
            (0x01c0, 0x00ff_ffff),
            (0x01cb, 19_050),
            (0x023f, 0x0002_0002),
            (0x0201, 0x0086_8686),
            (0x0205, 38_100),
            (0x0206, 50_800),
        ]);
        sh.kind = 145;
        let font: Vec<u8> = "Arial Black".encode_utf16().flat_map(u16::to_le_bytes).collect();
        sh.complex.insert(0x00c5, font);
        let id = b.wordart_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 333.0, 124.0), &sh, "Sample Text!").unwrap();
        let item = b.doc.item(id).unwrap().clone();
        let Content::Text(t) = &item.content else { panic!("not a text frame") };
        let story = b.doc.story(t.story).unwrap();
        assert_eq!(story.text, "Sample Text!");
        let (_, cf) = story.runs().next().unwrap();
        let over = serde_json::to_value(&cf.over).unwrap();
        assert_eq!(over["fontFamily"], "Arial Black");
        assert_eq!(over["fontStyle"], "Bold Italic");
        assert_eq!(over["fill"], "R=200 G=0 B=0");
        assert_eq!(over["stroke"], "R=255 G=255 B=255");
        assert_eq!(over["strokeWeight"], 1.5);
        // 255 pt of usable width for 12 letters at 0.7 em: about 30 pt.
        let size = over["size"].as_f64().unwrap();
        assert!((29.0..32.0).contains(&size), "{size}");
        let ds = &item.effects.drop_shadow;
        assert!(ds.on && ds.color == "R=134 G=134 B=134");
        assert!((ds.distance - 5.0).abs() < 1e-9 && (ds.angle - 4f64.atan2(-3.0).to_degrees()).abs() < 1e-9);
        assert_eq!(b.warnings, vec!["WordArt shapes (arches, circles, waves) were imported as straight text.".to_string()]);
    }
    #[test]
    fn unconfirmed_wordart_inputs_keep_the_straight_fallback() -> std::result::Result<(), String> {
        let rect = Rect::new(10.0, 20.0, 110.0, 120.0);
        let mut sh = escher::EShape { kind: 144, ..Default::default() };
        if Builder::wordart_path(&sh, rect, "Text").is_none() {
            return Err("default arch declined".into());
        }
        sh.props.insert(0x0147, 0);
        if Builder::wordart_path(&sh, rect, "Text").is_some() {
            return Err("unconfirmed adjust accepted".into());
        }
        sh.props.clear();
        if Builder::wordart_path(&sh, rect, "Text\nTwo").is_some() {
            return Err("multiple rows accepted".into());
        }
        for kind in [145, 146, 147, 156, 157, 158, 159] {
            sh.kind = kind;
            if Builder::wordart_path(&sh, rect, "Text").is_some() {
                return Err("unsupported warp accepted".into());
            }
        }
        Ok(())
    }
}
