// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Synthetic publications written from scratch for the tests: the compound file, the `Contents`
//! stream (docs/pub-format/contents-stream.md), the Quill text store (text-store.md) and the
//! drawing records (MS-ODRAW). `tests/data/hello.pub` is exactly [`hello`]'s output and
//! `tests/data/wmf-picture.pub` exactly [`wmf_picture`]'s.
#![allow(dead_code)]

use std::io::{Cursor, Write};

/// A property with a 4-byte number (kind 0x20).
pub fn number(key: u8, value: u32) -> Vec<u8> {
    let mut p = vec![key, 0x20];
    p.extend(value.to_le_bytes());
    p
}

/// A nested property list (`kind` 0x88, 0x90, 0xA0 …): self-counting length, then `data`.
pub fn list(key: u8, kind: u8, data: &[u8]) -> Vec<u8> {
    let mut p = vec![key, kind];
    p.extend(((data.len() + 4) as u32).to_le_bytes());
    p.extend(data);
    p
}

/// A length-prefixed list (a record or a style record).
pub fn record(properties: &[u8]) -> Vec<u8> {
    let mut b = ((properties.len() + 4) as u32).to_le_bytes().to_vec();
    b.extend(properties);
    b
}

/// One object of [`contents`]: its record type and properties.
pub struct Object {
    pub record_type: u16,
    pub properties: Vec<u8>,
}

/// A `Contents` stream with one document record, one page holding `objects` in order, and a
/// record directory whose first 256 entries are empty slots (as in Publisher-written files), so
/// the document is sequence number 256, the page 257 and the objects 258, 259, …
pub fn contents(width_emu: u32, height_emu: u32, objects: &[Object]) -> Vec<u8> {
    let first = 256u32;
    let page_seq = first + 1;
    let object_seqs: Vec<u32> = (0..objects.len() as u32).map(|i| first + 2 + i).collect();
    let page_entry = |seq: u32| {
        let mut e = vec![0x00, 0x70];
        e.extend(seq.to_le_bytes());
        e
    };
    let document =
        [list(0x02, 0xa0, &page_entry(page_seq)), list(0x12, 0x88, &[number(0x01, width_emu), number(0x02, height_emu)].concat())].concat();
    let page = list(0x02, 0xa0, &object_seqs.iter().flat_map(|s| page_entry(*s)).collect::<Vec<_>>());
    let mut records: Vec<(u16, Vec<u8>)> = vec![(0x44, document), (0x43, page)];
    records.extend(objects.iter().map(|o| (o.record_type, o.properties.clone())));

    let mut b = vec![0u8; 0x1e];
    b[..8].copy_from_slice(&[0xe8, 0xac, 0x2c, 0x00, 0xe8, 0x03, 0x0a, 0x00]);
    b[0x0c..0x10].copy_from_slice(&[0x1a, 0x00, 0x00, 0x01]);
    let mut directory = [0x00, 0x78].repeat(first as usize);
    for (ty, props) in &records {
        let at = b.len() as u32;
        b.extend(record(props));
        let mut fields = vec![0x02, 0x18];
        fields.extend(ty.to_le_bytes());
        fields.extend([0x04, 0xb8]);
        fields.extend(at.to_le_bytes());
        directory.extend(list(0x00, 0x88, &fields));
    }
    let entries = first + records.len() as u32;
    let trailer = [number(0x01, entries), number(0x02, entries - 1), list(0x03, 0x90, &directory)].concat();
    let at = b.len() as u32;
    b[0x1a..0x1e].copy_from_slice(&at.to_le_bytes());
    b.extend(record(&trailer));
    b
}

/// A Quill text store with one story (id 2) holding `text`, its formatting page carrying the
/// paragraph format record `props`, and `defaults` as the default paragraph format.
pub fn quill(props: &[u8], defaults: &[u8], text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let text_bytes: Vec<_> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
    let mut page = vec![0; 512];
    page[..2].copy_from_slice(&1u16.to_le_bytes());
    page[8..12].copy_from_slice(&(512u32 + text_bytes.len() as u32).to_le_bytes());
    let format = record(props);
    let at = 512 - format.len();
    page[12..14].copy_from_slice(&(at as u16).to_le_bytes());
    page[at..].copy_from_slice(&format);
    let mut fonts = vec![0; 24];
    fonts[4..8].copy_from_slice(&1u32.to_le_bytes());
    fonts.extend(6u16.to_le_bytes());
    fonts.extend("Symbol".encode_utf16().flat_map(u16::to_le_bytes));
    fonts.extend([0; 4]);
    let mut styles = vec![0; 28];
    styles[4..8].copy_from_slice(&2u32.to_le_bytes());
    styles[20..24].copy_from_slice(&8u32.to_le_bytes());
    styles[24..28].copy_from_slice(&14u32.to_le_bytes());
    styles.extend([0; 2]);
    styles.extend(record(&[]));
    styles.extend([0; 2]);
    styles.extend(record(defaults));
    let ids = [2u32, 1, 2].into_iter().flat_map(u32::to_le_bytes).collect();
    let lengths = [1u32, 8, 255, units.len() as u32].into_iter().flat_map(u32::to_le_bytes).collect();
    let sections =
        [(b"TEXT", text_bytes), (b"FDPP", page), (b"SYID", ids), (b"STRS", lengths), (b"FONT", fonts), (b"STSH", vec![0; 20]), (b"STSH", styles)];
    let mut quill = vec![0; 512];
    quill[..8].copy_from_slice(b"CHNKINK ");
    quill[26..28].copy_from_slice(&(sections.len() as u16).to_le_bytes());
    quill[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    for (i, (tag, data)) in sections.into_iter().enumerate() {
        let e = 32 + 24 * i;
        quill[e + 2..e + 6].copy_from_slice(tag);
        let offset = quill.len() as u32;
        quill[e + 16..e + 20].copy_from_slice(&offset.to_le_bytes());
        quill[e + 20..e + 24].copy_from_slice(&(data.len() as u32).to_le_bytes());
        quill.extend(data);
    }
    quill
}

/// An MS-ODRAW record: version, instance, type and data.
pub fn rec(ver: u16, inst: u16, ty: u16, data: &[u8]) -> Vec<u8> {
    let mut r = (ver | inst << 4).to_le_bytes().to_vec();
    r.extend(ty.to_le_bytes());
    r.extend((data.len() as u32).to_le_bytes());
    r.extend(data);
    r
}

/// Client anchor or client data pairs (u16 id, i32 value) after a u32 byte count.
pub fn pairs(p: &[(u16, i32)]) -> Vec<u8> {
    let mut d = ((4 + 6 * p.len()) as u32).to_le_bytes().to_vec();
    for (id, v) in p {
        d.extend(id.to_le_bytes());
        d.extend(v.to_le_bytes());
    }
    d
}

pub const PAGE_W: i32 = 612;
pub const PAGE_H: i32 = 792;

pub fn emu(pt: i32) -> i32 {
    pt * 12_700
}

/// A shape container: the shape type, its simple properties, a frame at (x, y, w, h) pt on the
/// page (page-centre-relative anchor) and its `Contents` sequence number.
pub fn shape(kind: u16, spid: u32, props: &[(u16, u32)], [x, y, w, h]: [i32; 4], seq: i32) -> Vec<u8> {
    let mut sp = rec(2, kind, 0xF00A, &[spid.to_le_bytes(), 0xA00u32.to_le_bytes()].concat());
    if !props.is_empty() {
        let data: Vec<u8> = props.iter().flat_map(|(id, v)| [id.to_le_bytes().to_vec(), v.to_le_bytes().to_vec()].concat()).collect();
        sp.extend(rec(3, props.len() as u16, 0xF00B, &data));
    }
    let (cx, cy) = (PAGE_W / 2, PAGE_H / 2);
    sp.extend(rec(0, 0, 0xF010, &pairs(&[(0x2001, emu(x - cx)), (0x2002, emu(y - cy)), (0x2003, emu(x + w - cx)), (0x2004, emu(y + h - cy))])));
    sp.extend(rec(0, 0, 0xF011, &pairs(&[(0x6801, seq)])));
    rec(0xF, 0, 0xF004, &sp)
}

/// The drawing stream: an optional drawing-group container (`dgg`, with the picture store) and
/// one drawing whose patriarch holds `shapes`.
pub fn escher(dgg: Option<&[u8]>, shapes: &[Vec<u8>]) -> Vec<u8> {
    let mut patriarch = rec(1, 0, 0xF009, &[0u8; 16]);
    patriarch.extend(rec(2, 0, 0xF00A, &[1024u32.to_le_bytes(), 0x5u32.to_le_bytes()].concat()));
    let mut spgr = rec(0xF, 0, 0xF004, &patriarch);
    for s in shapes {
        spgr.extend(s);
    }
    let last_spid = 1024 + shapes.len() as u32;
    let mut dg = rec(0, 1, 0xF008, &[(shapes.len() as u32 + 1).to_le_bytes(), last_spid.to_le_bytes()].concat());
    dg.extend(rec(0xF, 0, 0xF003, &spgr));
    let mut out = Vec::new();
    if let Some(dgg) = dgg {
        out.extend(rec(0xF, 0, 0xF000, dgg));
        out.extend(0u32.to_le_bytes()); // Publisher's u32 after the container
    }
    out.extend(rec(0xF, 0, 0xF002, &dg));
    out.extend(0u32.to_le_bytes());
    out
}

/// A compound file holding the three streams the importer reads.
pub fn compound(contents: &[u8], quill: &[u8], escher: &[u8]) -> Vec<u8> {
    let mut cf = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cf.create_stream("/Contents").unwrap().write_all(contents).unwrap();
    cf.create_storage("/Quill").unwrap();
    cf.create_storage("/Quill/QuillSub").unwrap();
    cf.create_stream("/Quill/QuillSub/CONTENTS").unwrap().write_all(quill).unwrap();
    cf.create_storage("/Escher").unwrap();
    cf.create_stream("/Escher/EscherStm").unwrap().write_all(escher).unwrap();
    // Storages carry times; fixed ones keep the file the same on every run.
    for storage in ["/Quill", "/Quill/QuillSub", "/Escher"] {
        cf.set_created_time(storage, std::time::UNIX_EPOCH).unwrap();
        cf.set_modified_time(storage, std::time::UNIX_EPOCH).unwrap();
    }
    cf.flush().unwrap();
    cf.into_inner().into_inner()
}

/// The text frame's `Contents` object record: story 2, and its stored size (0xAA, 0xAB).
fn text_object() -> Object {
    Object { record_type: 0x01, properties: [number(0x27, 2), number(0xaa, emu(288) as u32), number(0xab, emu(72) as u32)].concat() }
}

/// The text frame at (72, 72) pt, 288 × 72 pt.
pub fn text_shape() -> Vec<u8> {
    shape(202, 1025, &[], [72, 72, 288, 72], 258)
}

/// The text of [`hello`].
pub const HELLO_TEXT: &str = "Hello from DesignCraft";

/// [`hello`] with another text store.
pub fn publication(props: &[u8], defaults: &[u8], text: &str) -> Vec<u8> {
    let rectangle = shape(1, 1026, &[(0x0181, 0x00d0_6f1f), (0x01ff, 0x0008_0000)], [72, 216, 216, 144], 259);
    let objects = [text_object(), Object { record_type: 0x01, properties: Vec::new() }];
    compound(&contents(emu(PAGE_W) as u32, emu(PAGE_H) as u32, &objects), &quill(props, defaults, text), &escher(None, &[text_shape(), rectangle]))
}

/// One US Letter page with a text frame ("Hello from DesignCraft") and a blue rectangle at
/// (72, 216) pt, 216 × 144 pt, without a border.
pub fn hello() -> Vec<u8> {
    publication(&[], &[], HELLO_TEXT)
}

/// A small WMF (MS-WMF): a red triangle on a blue ellipse, a black pen and a text record.
fn wmf() -> Vec<u8> {
    fn wmf_rec(func: u16, params: &[i16]) -> Vec<u8> {
        let mut r = (3 + params.len() as u32).to_le_bytes().to_vec();
        r.extend(func.to_le_bytes());
        params.iter().for_each(|p| r.extend(p.to_le_bytes()));
        r
    }
    let mut body = Vec::new();
    body.extend(wmf_rec(0x020B, &[0, 0]));
    body.extend(wmf_rec(0x020C, &[200, 300]));
    body.extend(wmf_rec(0x02FC, &[0, 0, 0x00ff, 0])); // blue brush
    body.extend(wmf_rec(0x02FC, &[0, 0x00ff, 0, 0])); // red brush
    body.extend(wmf_rec(0x02FA, &[0, 4, 0, 0, 0])); // black pen, 4 units
    body.extend(wmf_rec(0x012D, &[0]));
    body.extend(wmf_rec(0x012D, &[2]));
    body.extend(wmf_rec(0x0418, &[200, 300, 0, 0]));
    body.extend(wmf_rec(0x012D, &[1]));
    body.extend(wmf_rec(0x0324, &[3, 150, 20, 270, 180, 30, 180]));
    body.extend(wmf_rec(0x0521, &[2, 0x6948, 10, 10]));
    body.extend(wmf_rec(0x0000, &[]));
    let mut wmf = Vec::new();
    [1u16, 9, 0x0300].iter().for_each(|v| wmf.extend(v.to_le_bytes()));
    wmf.extend(((18 + body.len()) as u32 / 2).to_le_bytes());
    wmf.extend(3u16.to_le_bytes());
    wmf.extend(7u32.to_le_bytes());
    wmf.extend(0u16.to_le_bytes());
    wmf.extend(body);
    wmf
}

/// [`hello`] with its rectangle replaced by a picture frame holding [`wmf`], stored compressed
/// in the drawing's picture store (MS-ODRAW OfficeArtBlipWMF in an OfficeArtFBSE).
pub fn wmf_picture() -> Vec<u8> {
    let wmf = wmf();
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(&wmf, 9);
    let mut blip = vec![0u8; 16]; // UID
    blip.extend((wmf.len() as u32).to_le_bytes());
    [0i32, 0, 300, 200, emu(216), emu(144)].iter().for_each(|v| blip.extend(v.to_le_bytes()));
    blip.extend((packed.len() as u32).to_le_bytes());
    blip.extend([0x00, 0xFE]);
    blip.extend(packed);
    let blip = rec(0, 0x216, 0xF01B, &blip);
    let mut bse = vec![3u8, 4];
    bse.extend([0u8; 16]);
    bse.extend(0xFFu16.to_le_bytes());
    bse.extend((blip.len() as u32).to_le_bytes());
    bse.extend(1u32.to_le_bytes());
    bse.extend(0u32.to_le_bytes());
    bse.extend([0u8; 4]);
    bse.extend(blip);
    let mut dgg = rec(0, 0, 0xF006, &[0u8; 16]);
    dgg.extend(rec(0xF, 1, 0xF001, &rec(2, 3, 0xF007, &bse)));
    let picture = shape(75, 1026, &[(0x4104, 1)], [72, 216, 216, 144], 259);
    let objects = [text_object(), Object { record_type: 0x01, properties: Vec::new() }];
    compound(&contents(emu(PAGE_W) as u32, emu(PAGE_H) as u32, &objects), &quill(&[], &[], HELLO_TEXT), &escher(Some(&dgg), &[text_shape(), picture]))
}
