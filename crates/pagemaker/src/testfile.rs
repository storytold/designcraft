//! Synthetic PageMaker files of each version family, for tests here and in the engine. Not part
//! of the API.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// A version family's header code, format number and record sizes.
#[derive(Clone, Copy)]
pub struct Fam {
    code: u16,
    fmt: u16,
    object: usize,
    char_run: usize,
    para_run: usize,
    font: usize,
    bounds: usize,
    block_id: usize,
    page_rects: usize,
}

pub const PM4: Fam = Fam { code: 0x0203, fmt: 42, object: 58, char_run: 26, para_run: 70, font: 144, bounds: 4, block_id: 26, page_rects: 40 };
pub const PM5: Fam = Fam { code: 0x0206, fmt: 47, object: 78, char_run: 30, para_run: 76, font: 94, bounds: 6, block_id: 32, page_rects: 42 };
pub const PM6: Fam = Fam { code: 0x0209, fmt: 50, object: 136, char_run: 30, para_run: 80, font: 94, bounds: 6, block_id: 32, page_rects: 54 };
pub const PM65: Fam = Fam { code: 0x0209, fmt: 51, object: 258, char_run: 30, para_run: 80, font: 94, bounds: 6, block_id: 32, page_rects: 54 };

/// Writes a PageMaker stream: records appended after the header, then the index.
struct Writer {
    f: Fam,
    le: bool,
    out: Vec<u8>,
    index: Vec<(u8, u16, u32)>,
}

impl Writer {
    fn new(f: Fam, le: bool) -> Self {
        Writer { f, le, out: vec![0; 256], index: vec![] }
    }
    fn u16(&self, v: u16) -> [u8; 2] {
        if self.le { v.to_le_bytes() } else { v.to_be_bytes() }
    }
    fn u32(&self, v: u32) -> [u8; 4] {
        if self.le { v.to_le_bytes() } else { v.to_be_bytes() }
    }
    fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
        buf[at..at + bytes.len()].copy_from_slice(bytes);
    }
    /// Append `records` (already sized) as one index entry of `kind`.
    fn list(&mut self, kind: u8, records: &[Vec<u8>]) {
        while !self.out.len().is_multiple_of(64) {
            self.out.push(0);
        }
        let at = self.out.len() as u32;
        for r in records {
            self.out.extend(r);
        }
        self.index.push((kind, records.len() as u16, if records.is_empty() { 0 } else { at }));
    }
    fn i16s(&self, v: [i16; 4]) -> Vec<u8> {
        v.iter().flat_map(|x| self.u16(*x as u16)).collect()
    }
    fn finish(mut self) -> Vec<u8> {
        while !self.out.len().is_multiple_of(64) {
            self.out.push(0);
        }
        let at = self.out.len() as u32;
        for (kind, count, ptr) in self.index.clone() {
            let mut e = vec![0u8; 16];
            e[1] = kind;
            Self::put(&mut e, 2, &self.u16(count));
            Self::put(&mut e, 4, &self.u32(ptr));
            e[8] = 0xFF;
            e[9] = 0xFF;
            e[10] = 1;
            e[11] = kind;
            self.out.extend(e);
        }
        let n = self.index.len() as u16;
        let mut h = std::mem::take(&mut self.out);
        Self::put(&mut h, 6, if self.le { &[0xFF, 0x99] } else { &[0x99, 0xFF] });
        Self::put(&mut h, 10, &self.u16(self.f.code));
        Self::put(&mut h, 42, &self.u16(self.f.fmt));
        Self::put(&mut h, 46, &self.u16(n));
        Self::put(&mut h, 48, &self.u32(at));
        h
    }
}

/// A two-page facing document: a story threaded over both pages ("Hello" in bold 18 pt
/// Helvetica, teal, centred, then "world" in 10 pt Times, black), a box on page 1 and a line on
/// the right master.
pub fn sample(f: Fam, le: bool) -> Vec<u8> {
    let mut w = Writer::new(f, le);
    // Document record: left page (-6120..0) and right page (0..6120), 8.5×11 in.
    let mut doc = vec![0u8; 128];
    Writer::put(&mut doc, f.page_rects, &w.i16s([-12240, -7920, 0, 7920]));
    Writer::put(&mut doc, f.page_rects + 8, &w.i16s([0, -7920, 12240, 7920]));
    w.list(0x18, &[doc]);
    let font = |name: &str| {
        let mut r = vec![0u8; f.font];
        r[..name.len()].copy_from_slice(name.as_bytes());
        r
    };
    w.list(0x13, &[font("Helvetica"), font("Times")]);
    // Colours: the fixed ones, then "Teal" (C=100 M=0 Y=50 K=0). PageMaker 6 and later start with None.
    let modern = f.fmt >= 50;
    let size = match f.code {
        0x0203 => 64,
        0x0206 => 86,
        _ => 210,
    };
    let named = |w: &Writer, name: &str, cmyk: Option<[u16; 4]>| {
        let mut r = vec![0u8; size];
        r[..name.len()].copy_from_slice(name.as_bytes());
        if let Some(v) = cmyk {
            let at = if modern { 38 } else { 34 };
            if modern {
                Writer::put(&mut r, 34, &w.u16(0x08));
            }
            for (k, x) in v.iter().enumerate() {
                Writer::put(&mut r, at + 2 * k, &w.u16(*x));
            }
        }
        r
    };
    let mut colors = vec![];
    if modern {
        colors.push(named(&w, "None", None));
    }
    colors.extend([
        named(&w, "Registration", None),
        named(&w, "Paper", None),
        named(&w, "Black", None),
        named(&w, "Teal", Some([65535, 0, 32768, 0])),
    ]);
    let black = if modern { 3 } else { 2 };
    w.list(0x15, &colors);
    // The story.
    // The story record: its length (two 16-bit words, low first) and its id.
    let mut st = vec![0u8; 24];
    Writer::put(&mut st, 0, &w.u16(12));
    Writer::put(&mut st, 20, &w.u32(7));
    w.list(0x1B, &[st]);
    w.list(0x0D, &b"Hello\rworld\r".iter().map(|c| vec![*c]).collect::<Vec<_>>());
    let run = |w: &Writer, len: u16, font: u16, size: u16, style: u16, color: u16| {
        let mut r = vec![0u8; f.char_run];
        Writer::put(&mut r, 0, &w.u16(len));
        Writer::put(&mut r, 2, &w.u16(font));
        Writer::put(&mut r, 4, &w.u16(size));
        Writer::put(&mut r, 6, &w.u16(0xFFFF));
        Writer::put(&mut r, 8, &w.u16(color));
        Writer::put(&mut r, 10, &w.u16(style));
        r
    };
    let runs = [run(&w, 6, 0, 180, 1, black + 1), run(&w, 6, 1, 100, 0, black)];
    w.list(0x1C, &runs);
    // Paragraphs: centred with a 36 pt left indent, an 18 pt first-line indent and 6 pt before;
    // then plain.
    let para = |w: &Writer, len: u16, align: u8, indents: [u16; 5]| {
        let mut r = vec![0u8; f.para_run];
        Writer::put(&mut r, 0, &w.u16(len));
        r[3] = align;
        for (k, v) in indents.iter().enumerate() {
            Writer::put(&mut r, 10 + 2 * k, &w.u16(*v));
        }
        r
    };
    let paras = [para(&w, 6, 2, [720, 360, 0, 120, 0]), para(&w, 6, 0, [0; 5])];
    w.list(0x0B, &paras);
    // Objects: one list off the pages (two from PageMaker 6.5), the left and right masters, page
    // 1 (right) and page 2 (left); positions are from the spine.
    let obj = |w: &Writer, t: u8, bx: [i16; 4], ids: Option<(u32, u32)>| {
        let mut r = vec![0u8; f.object];
        r[0] = t;
        Writer::put(&mut r, f.bounds, &w.i16s(bx));
        if let Some((block, story)) = ids {
            Writer::put(&mut r, f.block_id, &w.u32(block));
            Writer::put(&mut r, f.block_id + 4, &w.u32(story));
        }
        r
    };
    w.list(0x19, &[]);
    if f.fmt >= 51 {
        w.list(0x19, &[]);
    }
    w.list(0x19, &[]);
    // A 2 pt line across the right master, 36 pt from the top (weights from PageMaker 5).
    let mut line = obj(&w, 3, [1440, -7200, 10800, -7180], None);
    let ends = if f.code == 0x0203 { 30 } else { 40 };
    Writer::put(&mut line, ends, &w.i16s([1440, -7190, 10800, -7190]));
    if f.code != 0x0203 {
        line[35] = 20;
    }
    w.list(0x19, &[line]);
    let p1 = [obj(&w, 1, [1440, -6480, 7200, -5040], Some((1, 7))), obj(&w, 4, [1440, 0, 4320, 2880], None)];
    w.list(0x19, &p1);
    let p2 = [obj(&w, 1, [-10800, -6480, -5040, -5040], Some((2, 7)))];
    w.list(0x19, &p2);
    w.finish()
}
