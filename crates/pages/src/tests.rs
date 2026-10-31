use std::io::Write;

use designcraft_doc::item::{Content, VerticalJustification};
use designcraft_doc::{Align, Leading};
use designcraft_geom::Point;

use super::*;
use crate::proto::write::{M, varint};

const NO_FONTS: FontLookup = &|_| None;

/// A raw Snappy block holding `data` as literals.
fn snappy_literal(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    varint(&mut out, data.len() as u64);
    for chunk in data.chunks(65536) {
        out.push(61 << 2);
        out.extend_from_slice(&((chunk.len() - 1) as u16).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

/// An `.iwa` stream of `objects` (identifier, type, message).
fn iwa(objects: &[(u64, u32, M)]) -> Vec<u8> {
    frame(&records(objects))
}

/// The unpacked records of `objects`.
fn records(objects: &[(u64, u32, M)]) -> Vec<u8> {
    let mut raw = Vec::new();
    for (id, ty, m) in objects {
        let info = M::new().uint(1, *id).msg(2, M::new().uint(1, u64::from(*ty)).uint(3, m.0.len() as u64));
        varint(&mut raw, info.0.len() as u64);
        raw.extend_from_slice(&info.0);
        raw.extend_from_slice(&m.0);
    }
    raw
}

/// `raw` as one Snappy-framed `.iwa` chunk.
fn frame(raw: &[u8]) -> Vec<u8> {
    let block = snappy_literal(raw);
    let mut out = vec![0];
    out.extend_from_slice(&(block.len() as u32).to_le_bytes()[..3]);
    out.extend_from_slice(&block);
    out
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        z.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn color(r: f32, g: f32, b: f32) -> M {
    M::new().uint(1, 1).f32(3, r).f32(4, g).f32(5, b).f32(6, 1.0)
}

fn geometry(x: f32, y: f32, w: f32, h: f32, angle: f32) -> M {
    M::new().pair(1, x, y).pair(2, w, h).uint(3, 3).f32(4, angle)
}

fn storage(text: &str, para_styles: &[(u64, u64)], char_styles: &[(u64, Option<u64>)]) -> M {
    let mut paras = M::new();
    for (i, s) in para_styles {
        paras = paras.msg(1, M::new().uint(1, *i).reference(2, *s));
    }
    let mut chars = M::new();
    for (i, s) in char_styles {
        let e = M::new().uint(1, *i);
        chars = chars.msg(1, if let Some(s) = s { e.reference(2, *s) } else { e });
    }
    M::new().bytes(3, text.as_bytes()).msg(5, paras).msg(8, chars)
}

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#;

/// A two-page A4 document: page 1 has a rotated text box with an orange fill, page 2 an image
/// whose mask keeps the coordinates of an older size.
fn sample() -> Vec<u8> {
    let root = M::new().reference(3, 10).reference(4, 40).f32(30, 595.28).f32(31, 841.89).f32(32, 50.0).f32(33, 40.0).f32(34, 30.0).f32(35, 20.0);
    let group = M::new().msg(1, M::new().uint(1, 0).msg(4, M::new().reference(1, 20))).msg(1, M::new().uint(1, 1).msg(4, M::new().reference(1, 30)));
    let shape = M::new().msg(1, M::new().msg(1, M::new().msg(1, geometry(100.0, 200.0, 200.0, 100.0, 350.0))).reference(2, 50)).reference(2, 21);
    let text = storage("Hello\u{2028}world\nSecond 😀 para\n", &[(0, 60), (12, 61)], &[(0, None), (6, Some(62)), (11, None)]);
    let image = M::new()
        .msg(1, M::new().msg(1, geometry(10.0, 20.0, 400.0, 200.0, 0.0)))
        .reference(5, 31)
        .reference(3, 70)
        .msg(11, M::new().uint(1, 7))
        .pair(9, 40.0, 20.0);
    let mask_path = M::new()
        .msg(1, M::new().uint(1, 1).msg(2, M::new().f32(1, 0.0).f32(2, 0.0)))
        .msg(1, M::new().uint(1, 2).msg(2, M::new().f32(1, 4.0).f32(2, 0.0)))
        .msg(1, M::new().uint(1, 2).msg(2, M::new().f32(1, 4.0).f32(2, 2.0)))
        .msg(1, M::new().uint(1, 2).msg(2, M::new().f32(1, 0.0).f32(2, 2.0)))
        .msg(1, M::new().uint(1, 5));
    let mask = M::new()
        .msg(1, M::new().msg(1, geometry(0.0, 0.0, 400.0, 200.0, 0.0)))
        .msg(2, M::new().msg(5, M::new().pair(2, 400.0, 200.0).msg(3, mask_path)));
    let graphic = M::new()
        .msg(1, M::new().msg(1, M::new().reference(3, 51)).msg(11, M::new().msg(1, M::new().msg(1, color(1.0, 0.5, 0.0))).f32(3, 0.8)))
        .msg(11, M::new().uint(2, 1).msg(6, M::new().f32(1, 5.0).f32(2, 6.0).f32(3, 7.0).f32(4, 8.0)));
    // The parent gives a black 2 pt stroke.
    let graphic_parent = M::new().msg(1, M::new().msg(11, M::new().msg(2, M::new().msg(1, color(0.0, 0.0, 0.0)).f32(2, 2.0))));
    let body = M::new()
        .msg(1, M::new().reference(3, 63))
        .msg(11, M::new().f32(3, 20.0).bool(1, true))
        .msg(12, M::new().uint(1, 2).msg(13, M::new().uint(1, 2).f32(2, 30.0)).f32(20, 6.0));
    let second = M::new().msg(1, M::new().reference(3, 60)).msg(12, M::new().uint(1, 1).f32(11, 10.0).f32(7, 30.0));
    let emphasis = M::new().msg(11, M::new().bool(2, true).msg(7, color(0.0, 0.0, 1.0)));
    let root_para = M::new().msg(11, M::new().str(5, "HelveticaNeue").f32(3, 11.0));
    let media = M::new().msg(11, M::new().f32(2, 0.25));
    let metadata = M::new().msg(4, M::new().uint(1, 7).str(3, "pic.svg").str(4, "pic-7.svg"));
    let doc = iwa(&[
        (1, 10000, root),
        (10, 10010, group),
        (20, 2011, shape),
        (21, 2001, text),
        (30, 3005, image),
        (31, 3006, mask),
        (40, 2001, storage("", &[], &[])),
        (50, 2025, graphic),
        (51, 2025, graphic_parent),
        (60, 2022, body),
        (61, 2022, second),
        (62, 2021, emphasis),
        (63, 2022, root_para),
        (70, 3016, media),
    ]);
    let meta = iwa(&[(2, 11006, metadata)]);
    zip(&[("Index/Document.iwa", &doc), ("Index/Metadata.iwa", &meta), ("Data/pic-7.svg", SVG)])
}

impl M {
    fn bool(self, f: u32, v: bool) -> Self {
        self.uint(f, u64::from(v))
    }
    fn str(self, f: u32, s: &str) -> Self {
        self.bytes(f, s.as_bytes())
    }
}

#[test]
fn recognises_pages_packages() {
    assert!(is_pages(&sample()));
    assert!(!is_pages(b"PK\x03\x04 not really"));
    assert!(!is_pages(&zip(&[("mimetype", b"application/vnd.adobe.indesign-idml-package")])));
}

#[test]
fn imports_pages_with_their_objects() {
    let im = import(&sample(), NO_FONTS).unwrap();
    let d = &im.document;
    assert_eq!(d.page_count(), 2);
    assert!((d.settings.page_width - 595.28).abs() < 0.01 && (d.settings.page_height - 841.89).abs() < 0.01);
    let page = &d.spreads[0].pages[0];
    assert_eq!((page.margins.top, page.margins.bottom, page.margins.inside, page.margins.outside), (30.0, 20.0, 50.0, 40.0));
    let items = d.spreads.iter().map(|s| s.items.len()).collect::<Vec<_>>();
    assert_eq!(items, vec![1, 1]);
    for st in d.stories.values() {
        st.check().unwrap();
    }
}

#[test]
fn shapes_keep_their_box_rotation_and_style() {
    let im = import(&sample(), NO_FONTS).unwrap();
    let d = &im.document;
    let item = &d.spreads[0].items[0];
    // The centre stays where the unrotated box's centre was; 350° turns it 10° clockwise.
    let c = item.xf * Point::new(100.0, 50.0);
    assert!((c.x - 200.0).abs() < 1e-3 && (c.y - 250.0).abs() < 1e-3, "{c:?}");
    let corner = item.xf * Point::new(0.0, 0.0);
    let expected = Point::new(200.0, 250.0) + (Point::new(-100.0, -50.0).to_vec2().rotate_by(10f64.to_radians()));
    assert!((corner - expected).hypot() < 1e-3, "{corner:?} vs {expected:?}");
    assert_eq!(item.fill.swatch, "R=255 G=128 B=0");
    assert_eq!(item.stroke.swatch, "R=0 G=0 B=0");
    assert_eq!(item.stroke.weight, 2.0);
    assert!((item.opacity - 0.8).abs() < 1e-6);
    assert!(d.swatches.iter().any(|s| s.name == "R=255 G=128 B=0"));
    let Content::Text(tf) = &item.content else { panic!("not a text frame: {:?}", item.content) };
    assert_eq!(tf.options.inset, [6.0, 5.0, 8.0, 7.0]);
    assert_eq!(tf.options.vertical_justification, VerticalJustification::Center);
}

trait RotateBy {
    fn rotate_by(self, a: f64) -> Self;
}

impl RotateBy for designcraft_geom::Vec2 {
    fn rotate_by(self, a: f64) -> Self {
        designcraft_geom::Vec2::new(self.x * a.cos() - self.y * a.sin(), self.x * a.sin() + self.y * a.cos())
    }
}

#[test]
fn text_keeps_paragraph_and_character_formatting() {
    let im = import(&sample(), NO_FONTS).unwrap();
    let d = &im.document;
    let Content::Text(tf) = &d.spreads[0].items[0].content else { panic!() };
    let st = &d.stories[&tf.story];
    assert_eq!(st.text, "Hello\u{2028}world\nSecond 😀 para");
    assert_eq!(st.paras.len(), 2);
    assert_eq!(st.paras[0].para.align, Some(Align::Center));
    assert_eq!(st.paras[0].para.space_after, Some(6.0));
    // The second style inherits from the first: its own alignment, indents measured Pages' way.
    assert_eq!(st.paras[1].para.align, Some(Align::Right));
    assert_eq!(st.paras[1].para.left_indent, Some(10.0));
    assert_eq!(st.paras[1].para.first_line_indent, Some(20.0));
    assert_eq!(st.paras[1].para.space_after, Some(6.0));
    let run_at = |byte: usize| {
        let mut at = 0;
        st.chars.iter().find(|r| {
            at += r.len;
            at > byte
        })
    };
    let first = &run_at(0).unwrap().format.over;
    assert_eq!(first.font_family.as_deref(), Some("Helvetica Neue"));
    assert_eq!(first.font_style.as_deref(), Some("Bold"));
    assert_eq!(first.size, Some(20.0));
    assert_eq!(first.leading, Some(Leading::Points(30.0)));
    // "world" (UTF-16 6..11, after the line break) has the character style.
    let world = &run_at(st.text.find("world").unwrap()).unwrap().format.over;
    assert_eq!(world.font_style.as_deref(), Some("Bold Italic"));
    assert_eq!(world.fill.as_deref(), Some("R=0 G=0 B=255"));
    let second = &run_at(st.text.find("Second").unwrap()).unwrap().format.over;
    assert_eq!(second.fill, None);
}

#[test]
fn images_use_their_mask_and_media_style() {
    let im = import(&sample(), NO_FONTS).unwrap();
    let d = &im.document;
    let item = &d.spreads[1].items[0];
    let Content::Graphic(g) = &item.content else { panic!("not a graphic: {:?}", item.content) };
    let asset = &d.assets[&g.asset];
    assert_eq!(asset.name, "pic.svg");
    assert!(g.size.0 > 0.0 && (g.size.0 / g.size.1 - 2.0).abs() < 1e-9, "{:?}", g.size);
    // The mask's 4 × 2 path is stretched over its 400 × 200 box: the whole image shows.
    let b = designcraft_geom::Shape::bounding_box(&item.path.to_bezpath());
    assert!((b.width() - 400.0).abs() < 1e-6 && (b.height() - 200.0).abs() < 1e-6, "{b:?}");
    assert!((g.xf * Point::new(g.size.0, g.size.1) - Point::new(400.0, 200.0)).hypot() < 1e-9);
    assert!((item.opacity - 0.25).abs() < 1e-6);
    let origin = d.spreads[1].pages[0].x;
    assert_eq!(item.xf * Point::new(0.0, 0.0), Point::new(origin + 10.0, 20.0));
}

#[test]
fn package_folders_import_like_files() {
    let bytes = sample();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut files = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut b).unwrap();
        files.push((f.name().to_string(), b));
    }
    let im = import_files(files, NO_FONTS).unwrap();
    assert_eq!(im.document.page_count(), 2);
}

#[test]
fn installed_fonts_are_named_by_their_postscript_name() {
    let lookup = |ps: &str| (ps == "HelveticaNeue").then(|| ("Helvetica Neue".to_string(), "Regular".to_string()));
    let im = import(&sample(), &lookup).unwrap();
    assert!(im.document.stories.values().any(|s| s.chars.iter().any(|r| r.format.over.font_family.as_deref() == Some("Helvetica Neue"))));
}

#[test]
fn text_mapping_counts_utf16_units() {
    let (text, map) = text::map_text("a😀\u{FFFC}b\u{2029}c\u{1}");
    assert_eq!(text, "a😀b\nc");
    // a(1) 😀(2) FFFC(1) b(1) 2029(1) c(1) 01(1), and one past the end.
    assert_eq!(map, vec![0, 1, 1, 5, 5, 6, 7, 8, 8]);
}

#[test]
fn snappy_copies_and_rejects_bad_offsets() {
    // "abcd" then a 1-byte-offset copy of 8 bytes at offset 4: "abcdabcdabcd".
    let mut block = vec![12, 3 << 2];
    block.extend_from_slice(b"abcd");
    block.extend_from_slice(&[((8 - 4) << 2 | 1) as u8, 4]);
    let mut out = Vec::new();
    snappy::decompress_block(&block, &mut out, 1 << 20).unwrap();
    assert_eq!(out, b"abcdabcdabcd");
    for bad in [vec![12, 3 << 2, b'a', b'b', b'c', b'd', (4 << 2 | 1) as u8, 9], vec![4, (4 << 2 | 1) as u8, 1], vec![100, 0, b'x'], vec![0xFF; 11]] {
        let mut out = Vec::new();
        assert!(snappy::decompress_block(&bad, &mut out, 1 << 20).is_err(), "{bad:?}");
    }
    // Past the caller's limit.
    assert!(snappy::decompress_block(&snappy_literal(&[1; 100]), &mut Vec::new(), 10).is_err());
}

#[test]
fn hostile_packages_fail_or_import_without_panicking() {
    // Not ZIPs, empty archives, garbage archives, truncated chunks.
    for bytes in
        [Vec::new(), b"PK".to_vec(), zip(&[]), zip(&[("Index/Document.iwa", b"\x00\xff\xff\xff")]), zip(&[("Index/Document.iwa", &[1, 2, 3])])]
    {
        assert!(import(&bytes, NO_FONTS).is_err());
    }
    // Every truncation of a real archive.
    let good = sample();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(good)).unwrap();
    let mut doc = Vec::new();
    std::io::Read::read_to_end(&mut z.by_name("Index/Document.iwa").unwrap(), &mut doc).unwrap();
    for cut in 0..doc.len() {
        let _ = import(&zip(&[("Index/Document.iwa", &doc[..cut])]), NO_FONTS);
    }
    // Every single-byte change of the decompressed objects.
    let raw = snappy::unframe(&doc, 1 << 20).unwrap();
    for i in (0..raw.len()).step_by(3) {
        let mut r = raw.clone();
        r[i] ^= 0x5A;
        let block = snappy_literal(&r);
        let mut framed = vec![0];
        framed.extend_from_slice(&(block.len() as u32).to_le_bytes()[..3]);
        framed.extend_from_slice(&block);
        let _ = import(&zip(&[("Index/Document.iwa", &framed)]), NO_FONTS);
    }
}

#[test]
fn style_and_group_cycles_end() {
    let root = M::new().reference(3, 10).f32(30, 100.0).f32(31, 100.0);
    let group = M::new().msg(1, M::new().uint(1, 0).msg(4, M::new().reference(1, 20)));
    // A group holding itself, and a shape whose style is its own parent.
    let g = M::new().msg(1, M::new().msg(1, geometry(0.0, 0.0, 10.0, 10.0, 0.0))).msg(2, M::new().uint(1, 20)).msg(2, M::new().uint(1, 21));
    let shape = M::new().msg(1, M::new().msg(1, M::new().msg(1, geometry(1.0, 1.0, 5.0, 5.0, 0.0))).reference(2, 50));
    let style = M::new().msg(1, M::new().msg(1, M::new().reference(3, 50)).msg(11, M::new().msg(1, M::new().msg(1, color(0.0, 1.0, 0.0)))));
    let doc = iwa(&[(1, 10000, root), (10, 10010, group), (20, 3008, g), (21, 2011, shape), (50, 2025, style)]);
    let im = import(&zip(&[("Index/Document.iwa", &doc)]), NO_FONTS).unwrap();
    assert_eq!(im.document.spreads[0].items.len(), 1);
}

#[test]
fn unsupported_objects_are_named_in_warnings() {
    let root = M::new().reference(3, 10).f32(30, 100.0).f32(31, 100.0);
    let group = M::new().msg(1, M::new().uint(1, 0).msg(4, M::new().reference(1, 20)));
    let doc = iwa(&[(1, 10000, root), (10, 10010, group), (20, 6000, M::new())]);
    let im = import(&zip(&[("Index/Document.iwa", &doc)]), NO_FONTS).unwrap();
    assert!(im.warnings.iter().any(|w| w.contains("tables")), "{:?}", im.warnings);
}

#[test]
fn skipped_inline_objects_are_named_in_warnings() {
    let root = M::new().reference(4, 40).f32(30, 100.0).f32(31, 100.0);
    let doc = iwa(&[(1, 10000, root), (40, 2001, storage("See \u{FFFC} here", &[], &[]))]);
    let im = import(&zip(&[("Index/Document.iwa", &doc)]), NO_FONTS).unwrap();
    assert!(im.warnings.iter().any(|w| w.contains("inline objects")), "{:?}", im.warnings);
}

/// A shape at `x` on page 1.
fn shape_at(x: f32) -> M {
    M::new().msg(1, M::new().msg(1, M::new().msg(1, geometry(x, 0.0, 10.0, 10.0, 0.0))))
}

#[test]
fn drawables_keep_the_document_paint_order() {
    // The page lists 20, 21, 22; the document paints 22, then 20, then 21 (back to front).
    let root = M::new().reference(3, 10).reference(20, 11).f32(30, 100.0).f32(31, 100.0);
    let mut entry = M::new().uint(1, 0);
    for id in [20, 21, 22] {
        entry = entry.msg(4, M::new().reference(1, id));
    }
    let order = M::new().reference(1, 22).reference(1, 20).reference(1, 21);
    let doc = iwa(&[
        (1, 10000, root),
        (10, 10010, M::new().msg(1, entry)),
        (11, 10015, order),
        (20, 2011, shape_at(20.0)),
        (21, 2011, shape_at(21.0)),
        (22, 2011, shape_at(22.0)),
    ]);
    let im = import(&zip(&[("Index/Document.iwa", &doc)]), NO_FONTS).unwrap();
    let xs: Vec<f64> = im.document.spreads[0].items.iter().map(|i| (i.xf * Point::new(0.0, 0.0)).x).collect();
    assert_eq!(xs, [22.0, 20.0, 21.0]);
}

#[test]
fn a_damaged_record_after_good_objects_rejects_the_package() {
    let mut raw = records(&[(1, 10000, M::new().f32(30, 100.0).f32(31, 100.0))]);
    // A record header that claims more bytes than follow.
    raw.extend_from_slice(&[40, 1, 2]);
    let doc = frame(&raw);
    assert!(matches!(import(&zip(&[("Index/Document.iwa", &doc)]), NO_FONTS), Err(PagesError::Invalid(_))));
}

#[test]
fn images_without_a_readable_size_are_named_in_warnings() {
    let root = M::new().reference(3, 10).f32(30, 100.0).f32(31, 100.0);
    let group = M::new().msg(1, M::new().uint(1, 0).msg(4, M::new().reference(1, 20)));
    let image = M::new().msg(1, M::new().msg(1, geometry(0.0, 0.0, 50.0, 50.0, 0.0))).msg(11, M::new().uint(1, 7));
    let metadata = M::new().msg(4, M::new().uint(1, 7).str(3, "art.pdf").str(4, "art-7.pdf"));
    let doc = iwa(&[(1, 10000, root), (10, 10010, group), (20, 3005, image)]);
    let meta = iwa(&[(2, 11006, metadata)]);
    let pdf: &[u8] = b"%PDF-1.4\n%%EOF\n";
    let im = import(&zip(&[("Index/Document.iwa", &doc), ("Index/Metadata.iwa", &meta), ("Data/art-7.pdf", pdf)]), NO_FONTS).unwrap();
    assert!(im.document.spreads[0].items.is_empty());
    assert!(im.warnings.iter().any(|w| w.contains("PDFs")), "{:?}", im.warnings);
}

#[test]
fn keynote_files_are_not_pages() {
    let doc = iwa(&[(1, 1, M::new())]);
    assert!(matches!(import(&zip(&[("Index/Document.iwa", &doc)]), NO_FONTS), Err(PagesError::NotPages(_))));
}

#[test]
fn groups_sharing_children_stay_within_the_visit_budget() {
    // Each group holds the next one twice: 2^30 placements if every visit were followed.
    let levels = 30u64;
    let root = M::new().reference(3, 10).f32(30, 100.0).f32(31, 100.0);
    let page = M::new().msg(1, M::new().uint(1, 0).msg(4, M::new().reference(1, 100)));
    let mut objects = vec![(1, 10000, root), (10, 10010, page)];
    for l in 0..levels {
        let next = 100 + l + 1;
        let g = M::new().msg(1, M::new().msg(1, geometry(0.0, 0.0, 10.0, 10.0, 0.0))).msg(2, M::new().uint(1, next)).msg(2, M::new().uint(1, next));
        objects.push((100 + l, 3008, g));
    }
    objects.push((100 + levels, 2011, M::new().msg(1, M::new().msg(1, M::new().msg(1, geometry(1.0, 1.0, 5.0, 5.0, 0.0))))));
    let doc = iwa(&objects);
    let started = std::time::Instant::now();
    let im = import(&zip(&[("Index/Document.iwa", &doc)]), NO_FONTS).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(30), "{:?}", started.elapsed());
    assert!(im.warnings.iter().any(|w| w.contains("objects were imported")), "{:?}", im.warnings);
}

#[test]
fn the_longest_literal_length_fails_without_overflowing() {
    // A literal with a 4-byte length of 0xFFFFFFFF: one more than that overflows a 32-bit usize.
    let block = [10, 63 << 2, 0xFF, 0xFF, 0xFF, 0xFF, b'x'];
    assert!(snappy::decompress_block(&block, &mut Vec::new(), 1 << 20).is_err());
}
