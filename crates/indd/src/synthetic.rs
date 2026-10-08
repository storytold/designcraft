//! Synthetic INDD documents built byte by byte, for tests in this and other crates.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeMap;

use crate::container::PAGE;

fn put_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put_f64(v: &mut Vec<u8>, x: f64) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// Object payload from `(implementation, data)` blocks.
fn blocks(list: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut v = Vec::new();
    for (imp, data) in list {
        put_u32(&mut v, *imp);
        put_u32(&mut v, data.len() as u32);
        v.extend_from_slice(data);
    }
    v
}

fn u32s(xs: &[u32]) -> Vec<u8> {
    let mut v = Vec::new();
    for x in xs {
        put_u32(&mut v, *x);
    }
    v
}

fn f64s(xs: &[f64]) -> Vec<u8> {
    let mut v = Vec::new();
    for x in xs {
        put_f64(&mut v, *x);
    }
    v
}

/// u16 count, u16 (0x4000 | len), 8-bit chars.
fn str8(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    put_u16(&mut v, s.len() as u16);
    put_u16(&mut v, 0x4000 | s.len() as u16);
    v.extend_from_slice(s.as_bytes());
    v
}

/// Typed value list with one value.
fn typed(t: u32, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    put_u16(&mut v, 1);
    put_u32(&mut v, t);
    put_u16(&mut v, data.len() as u16);
    v.extend_from_slice(data);
    v
}

/// Attribute list (`wide` = u32 count for graphic attributes).
fn attr_list(entries: &[(u32, Vec<u8>)], wide: bool) -> Vec<u8> {
    let mut v = Vec::new();
    if wide {
        put_u32(&mut v, entries.len() as u32);
    } else {
        put_u16(&mut v, entries.len() as u16);
    }
    for (id, data) in entries {
        put_u32(&mut v, *id);
        put_u16(&mut v, data.len() as u16);
        v.extend_from_slice(data);
    }
    v
}

pub struct Synth {
    objects: BTreeMap<u32, (u32, Vec<u8>)>,
    /// Objects stored as split records (first part on one page, rest on the next).
    pub split: Vec<u32>,
    /// Objects stored with one raw page plus a tail record.
    pub raw: Vec<u32>,
    /// `(uid, n)`: the location entries of `uid` are written `n` times (hostile files reuse them).
    pub repeat_locations: Vec<(u32, usize)>,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new()
    }
}

impl Synth {
    pub fn new() -> Self {
        Synth { objects: BTreeMap::new(), split: Vec::new(), raw: Vec::new(), repeat_locations: Vec::new() }
    }

    pub fn add(&mut self, uid: u32, cls: u32, payload: Vec<u8>) {
        self.objects.insert(uid, (cls, payload));
    }

    /// Assembles the file: pages 0 master, 1 root, 2 page map, 3.. B-tree leaves and object pages.
    pub fn build(&self) -> Vec<u8> {
        let mut pages: Vec<Vec<u8>> = Vec::new();
        let mut footers: Vec<(u32, u32)> = Vec::new();
        let mut l2p: Vec<(u32, u32)> = Vec::new(); // (logical, physical)
        let new_page = |pages: &mut Vec<Vec<u8>>, footers: &mut Vec<(u32, u32)>, t: u32, link: u32| {
            pages.push(vec![0u8; PAGE - 12]);
            footers.push((t, link));
            pages.len() as u32 - 1
        };
        new_page(&mut pages, &mut footers, 0, 0); // master
        new_page(&mut pages, &mut footers, 4, 0); // root
        new_page(&mut pages, &mut footers, 5, 0); // page map
        let mut next_logical = 4u32;
        let mut loc: Vec<[u32; 4]> = Vec::new();
        // Object pages: records packed until full.
        let mut cur: Option<(u32, u32, usize, u16)> = None; // (phys, logical, offset, next slot)
        let record = |pages: &mut Vec<Vec<u8>>,
                      footers: &mut Vec<(u32, u32)>,
                      l2p: &mut Vec<(u32, u32)>,
                      cur: &mut Option<(u32, u32, usize, u16)>,
                      next_logical: &mut u32,
                      body: &[u8],
                      flag: u16|
         -> (u32, u16) {
            let need = 4 + body.len().div_ceil(4) * 4;
            if cur.is_none_or(|(_, _, off, _)| off + need > PAGE - 12 - 64) {
                let logical = *next_logical;
                *next_logical += 1;
                let phys = new_page(pages, footers, 9, logical);
                l2p.push((logical, phys));
                *cur = Some((phys, logical, 0, 1));
            }
            let Some((phys, logical, off, slot)) = *cur else { return (0, 0) };
            let page = &mut pages[phys as usize];
            page[off..off + 2].copy_from_slice(&(need as u16).to_le_bytes());
            page[off + 2..off + 4].copy_from_slice(&(slot | flag).to_le_bytes());
            page[off + 4..off + 4 + body.len()].copy_from_slice(body);
            *cur = Some((phys, logical, off + need, slot + 1));
            (logical, slot)
        };
        for (&uid, (_, payload)) in &self.objects {
            if self.split.contains(&uid) {
                // InDesign splits at 4-byte boundaries (records are padded to 4).
                let (a, b) = payload.split_at(payload.len() / 8 * 4);
                // Second part goes on a fresh page so the pointer is known before writing the first.
                cur = None;
                let (l2, s2) = record(&mut pages, &mut footers, &mut l2p, &mut cur, &mut next_logical, b, 0);
                cur = None;
                let mut first = Vec::new();
                put_u16(&mut first, 0);
                put_u16(&mut first, s2);
                put_u32(&mut first, l2);
                first.extend_from_slice(a);
                let (l1, s1) = record(&mut pages, &mut footers, &mut l2p, &mut cur, &mut next_logical, &first, 0x8000);
                loc.push([uid, (u32::from(s1) << 16) | payload.len() as u32, l1, 1]);
            } else if self.raw.contains(&uid) {
                let (head, tail) = payload.split_at(payload.len().min(3000));
                let phys = new_page(&mut pages, &mut footers, 8, 0);
                pages[phys as usize][..head.len()].copy_from_slice(head);
                let (l, s) = record(&mut pages, &mut footers, &mut l2p, &mut cur, &mut next_logical, tail, 0);
                loc.push([uid, head.len() as u32, phys, 2]);
                loc.push([uid, (u32::from(s) << 16) | tail.len() as u32, l, 1]);
            } else {
                let (l, s) = record(&mut pages, &mut footers, &mut l2p, &mut cur, &mut next_logical, payload, 0);
                loc.push([uid, (u32::from(s) << 16) | payload.len() as u32, l, 1]);
            }
        }
        // B-tree leaves: locations, then classes (with the sentinel the reader expects last).
        let mut leaf = |pages: &mut Vec<Vec<u8>>, footers: &mut Vec<(u32, u32)>, l2p: &mut Vec<(u32, u32)>, entries: &[[u32; 4]]| {
            let logical = next_logical;
            next_logical += 1;
            let phys = new_page(pages, footers, 6, logical);
            l2p.push((logical, phys));
            let mut v = Vec::new();
            put_u32(&mut v, entries.len() as u32);
            put_u32(&mut v, 1);
            for e in entries {
                for x in e {
                    put_u32(&mut v, *x);
                }
            }
            pages[phys as usize][..v.len()].copy_from_slice(&v);
        };
        for &(uid, n) in &self.repeat_locations {
            let mine: Vec<[u32; 4]> = loc.iter().filter(|e| e[0] == uid).copied().collect();
            for _ in 1..n {
                loc.extend_from_slice(&mine);
            }
        }
        for chunk in loc.chunks(250) {
            leaf(&mut pages, &mut footers, &mut l2p, chunk);
        }
        let mut cls: Vec<[u32; 4]> = self.objects.iter().map(|(u, (c, _))| [*u, *c, 0, 0]).collect();
        cls.push([0x8000_0001, 0xC000_0000, 0, 0]);
        leaf(&mut pages, &mut footers, &mut l2p, &cls);
        // Master page, root and page map.
        let m = &mut pages[0];
        m[..16].copy_from_slice(&crate::container::GUID);
        m[16..24].copy_from_slice(b"DOCUMENT");
        m[24] = 1;
        m[29..33].copy_from_slice(&15u32.to_le_bytes());
        m[264..272].copy_from_slice(&3u64.to_le_bytes());
        m[0x3A8..0x3AC].copy_from_slice(&1u32.to_le_bytes());
        pages[1][128..132].copy_from_slice(&2u32.to_le_bytes());
        for (logical, phys) in &l2p {
            let o = (32 + *logical as usize) * 4;
            pages[2][o..o + 4].copy_from_slice(&phys.to_le_bytes());
        }
        let mut out = Vec::new();
        for (p, (t, link)) in pages.iter().zip(&footers) {
            out.extend_from_slice(p);
            put_u32(&mut out, *t);
            put_u32(&mut out, *link);
            put_u32(&mut out, 0);
        }
        out
    }
}

/// A one-page document: a red rectangle, and a text frame holding "Hello\rWorld" in "Body".
pub fn sample() -> Synth {
    let mut s = Synth::new();
    // Document: spread list, layer order.
    s.add(1, 0xE01, blocks(&[(0x501, u32s(&[1, 20])), (0x301, u32s(&[1, 10]))]));
    s.add(10, 0x302, blocks(&[(0x304, [vec![0, 0, 0], str8("Layer 1")].concat())]));
    s.add(
        11,
        0x1F05,
        blocks(&[(0x1F10, [vec![0, 2, 0], str8("Black")].concat()), (0x1F01, [u32s(&[6]), vec![4, 0], f64s(&[0.0, 0.0, 0.0, 1.0])].concat())]),
    );
    s.add(
        12,
        0x1F05,
        blocks(&[(0x1F10, [vec![0, 2, 0], str8("Red")].concat()), (0x1F01, [u32s(&[6]), vec![4, 0], f64s(&[0.0, 1.0, 1.0, 0.0])].concat())]),
    );
    // Spread with one content layer holding a page, a rectangle and a text frame.
    s.add(20, 0x501, blocks(&[(0x503, u32s(&[20, 0, 1, 21]))]));
    s.add(21, 0x301, blocks(&[(0x303, u32s(&[20, 20, 3, 22, 30, 40])), (0x302, [u32s(&[10]), vec![0, 0]].concat())]));
    s.add(22, 0x50F, blocks(&[(0x5DD, f64s(&[0.0, 0.0, 200.0, 100.0])), (0x5CC, f64s(&[1.0, 0.0, 0.0, 1.0, -100.0, -50.0]))]));
    let rect = rect_path(-20.0, -10.0, 20.0, 10.0);
    let fill = attr_list(&[(0x6E68, typed(0x117, &12u32.to_le_bytes()))], true);
    s.add(30, 0x6201, blocks(&[(0x162B, rect.clone()), (0x151, f64s(&[1.0, 0.0, 0.0, 1.0, -50.0, 0.0])), (0x6E03, fill)]));
    // Text frame: spline 40 → multi-column 41 → frame column 42 → frame-list strand 53.
    s.add(40, 0x6201, blocks(&[(0x162B, rect), (0x151, f64s(&[1.0, 0.0, 0.0, 1.0, 50.0, 0.0])), (0x15B, u32s(&[20, 21, 1, 41]))]));
    s.add(41, 0x263, blocks(&[(0x15B, u32s(&[20, 40, 1, 42]))]));
    s.add(42, 0x227, blocks(&[(0x220, u32s(&[53, 0]))]));
    // Story 50 with text (51), paragraph runs (52), character runs (54) and the frame list (53).
    let mut strands = u32s(&[11]);
    put_u16(&mut strands, 1);
    strands.extend_from_slice(&u32s(&[51, 3, 52, 53, 54]));
    s.add(50, 0x201, blocks(&[(0x223, strands)]));
    let strand = |data: u32| {
        let mut v = Vec::new();
        put_u16(&mut v, 1);
        put_u32(&mut v, 11);
        put_u32(&mut v, data);
        blocks(&[(0x261, v)])
    };
    s.add(51, 0x234, strand(61));
    s.add(52, 0x236, strand(62));
    s.add(53, 0x228, blocks(&[]));
    s.add(54, 0x235, strand(64));
    let mut text = u32s(&[0x202, 50]);
    put_u16(&mut text, 1);
    put_u32(&mut text, 4 + 2 + 11);
    put_u32(&mut text, 11);
    put_u16(&mut text, 0x4000 | 11);
    text.extend_from_slice(b"Hello\rWorld");
    s.add(61, 0x27E, blocks(&[(0x262, text)]));
    s.add(62, 0xCA17, blocks(&[(0x262, runs(&[(11, 70, attr_list(&[(0x1B7E, typed(0x1B20, &1u16.to_le_bytes()))], false))]))]));
    s.add(64, 0xCA18, blocks(&[(0x262, runs(&[(5, 71, attr_list(&[], false)), (6, 72, attr_list(&[], false))]))]));
    // Styles: paragraph root, a paragraph style, character root and a character style.
    s.add(70, 0x205, style("NormalParagraphStyle", 69, &[]));
    s.add(69, 0x205, style("[No paragraph style]", 0, &[(0x1B03, typed(0x1B28, &12.0f64.to_le_bytes()))]));
    s.add(71, 0x205, style("[No character style]", 0, &[]));
    s.add(72, 0x205, style("Accent", 71, &[(0x1B01, typed(0x1B05, &12u32.to_le_bytes())), (0x1B03, typed(0x1B28, &18.0f64.to_le_bytes()))]));
    s
}

/// Object payload from `(implementation, data)` blocks.
pub fn object(list: &[(u32, Vec<u8>)]) -> Vec<u8> {
    blocks(list)
}

/// Hierarchy block (0x15B) naming `kids` as the children of `parent`.
pub fn hierarchy(parent: u32, kids: &[u32]) -> (u32, Vec<u8>) {
    let mut v = u32s(&[20, parent, kids.len() as u32]);
    v.extend_from_slice(&u32s(kids));
    (0x15B, v)
}

pub fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<u8> {
    let mut v = u32s(&[1, 4]);
    for (x, y) in [(x0, y0), (x0, y1), (x1, y1), (x1, y0)] {
        put_u32(&mut v, 2);
        put_f64(&mut v, x);
        put_f64(&mut v, y);
    }
    put_u16(&mut v, 0);
    v
}

fn runs(list: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
    let mut v = u32s(&[0x203, 0]);
    put_u16(&mut v, list.len() as u16);
    for (len, style, attrs) in list {
        put_u32(&mut v, 8 + attrs.len() as u32);
        put_u32(&mut v, *len);
        put_u32(&mut v, *style);
        v.extend_from_slice(attrs);
    }
    v
}

fn style(name: &str, based_on: u32, attrs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut head = u32s(&[0, based_on]);
    head.resize(24, 0);
    head.extend_from_slice(&[0, 2, 0]);
    head.extend_from_slice(&str8(name));
    blocks(&[(0x230, head), (0x23F, attr_list(attrs, false))])
}
