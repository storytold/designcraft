//! Files: the program itself, strings run as code, decoding filters and `eexec` sections.
//!
//! Filters decode as they are read, so a filter made over `currentfile` before the operator
//! that consumes it (`currentfile /ASCII85Decode filter … image`) reads the data that follows
//! that operator. A filter takes from the file beneath only what its encoding needs: bytes a
//! decompressor fetched past its end are pushed back, so the program carries on after the data.

use std::collections::VecDeque;
use std::rc::Rc;

/// The most bytes one filter may decode to, and one reusable stream may hold.
const MAX_DECODED: usize = 256 << 20;
/// How deep files may sit on one another.
const MAX_CHAIN: usize = 16;

pub enum Dec {
    Hex {
        hi: Option<u8>,
    },
    A85 {
        group: [u8; 5],
        n: usize,
        first: bool,
    },
    RunLength,
    Lzw {
        dec: Box<weezl::decode::Decoder>,
        pending: Vec<u8>,
    },
    Flate {
        state: Box<miniz_oxide::inflate::stream::InflateState>,
        pending: Vec<u8>,
    },
    SubFile {
        eod: Vec<u8>,
        count: usize,
        left: Option<usize>,
        window: VecDeque<u8>,
    },
    /// JPEG data: only images take it (whole, as it is).
    Dct,
}

pub enum Kind {
    Bytes(Rc<Vec<u8>>),
    /// A window onto a string's storage, read in place (`token`, executing a string).
    Shared {
        buf: Rc<std::cell::RefCell<Vec<u8>>>,
        off: usize,
        len: usize,
    },
    /// Decrypting `eexec` over another file. `hex` is decided by the first four bytes.
    Eexec {
        up: usize,
        r: u16,
        hex: Option<bool>,
        skipped: u8,
    },
    Filter {
        up: usize,
        dec: Dec,
        out: VecDeque<u8>,
        done: bool,
        produced: usize,
    },
}

pub struct Source {
    pub kind: Kind,
    pub pos: usize,
    pub closed: bool,
    /// Raw JPEG data for a DCTDecode filter, once taken.
    pub dct: Option<Rc<Vec<u8>>>,
    /// The last byte given, and bytes pushed back (read again first; the last pushed first).
    pub last: Option<u8>,
    pub back: Vec<u8>,
    /// A PNG or TIFF predictor under Flate or LZW.
    pub predictor: Option<Predictor>,
}

impl Source {
    pub fn bytes(data: Vec<u8>) -> Self {
        Self::shared(Rc::new(data))
    }

    pub fn shared(data: Rc<Vec<u8>>) -> Self {
        Self::of(Kind::Bytes(data))
    }

    /// A string read in place, without copying it.
    pub fn string(s: &crate::obj::PStr) -> Self {
        Self::of(Kind::Shared { buf: s.buf.clone(), off: s.off, len: s.len })
    }

    pub fn eexec(up: usize) -> Self {
        Self::of(Kind::Eexec { up, r: 55665, hex: None, skipped: 0 })
    }

    fn of(kind: Kind) -> Self {
        Source { kind, pos: 0, closed: false, dct: None, last: None, back: Vec::new(), predictor: None }
    }
}

fn is_ws(c: u8) -> bool {
    matches!(c, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct FilterParams {
    pub early_change: bool,
    pub predictor: i64,
    pub colors: i64,
    pub bpc: i64,
    pub columns: i64,
    pub eod_count: usize,
    pub eod_string: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Predictor {
    kind: i64,
    bpp: usize,
    row: usize,
    prev: Vec<u8>,
    cur: Vec<u8>,
}

/// The sources, read through one another (`eexec` over the program, filters over both).
pub struct Sources {
    pub list: Vec<Source>,
    /// Bytes read so far, at every level (a filter's input and its output both count): the
    /// interpreter charges them as work.
    pub read: u64,
    /// The most bytes that may be read; past it every file is at its end and `exhausted` is set.
    pub max_read: u64,
    pub exhausted: bool,
}

impl Default for Sources {
    fn default() -> Self {
        Sources { list: Vec::new(), read: 0, max_read: u64::MAX, exhausted: false }
    }
}

impl Sources {
    pub fn add(&mut self, s: Source) -> usize {
        self.list.push(s);
        self.list.len() - 1
    }

    pub fn read(&mut self, id: usize) -> Option<u8> {
        self.read_depth(id, 0)
    }

    fn read_depth(&mut self, id: usize, depth: usize) -> Option<u8> {
        if self.read >= self.max_read {
            self.exhausted = true;
            return None;
        }
        let c = self.read_inner(id, depth);
        if c.is_some() {
            self.read += 1;
        }
        c
    }

    fn read_inner(&mut self, id: usize, depth: usize) -> Option<u8> {
        if depth > MAX_CHAIN {
            return None;
        }
        let src = self.list.get_mut(id)?;
        if src.closed {
            return None;
        }
        if let Some(c) = src.back.pop() {
            src.last = Some(c);
            return Some(c);
        }
        let c = match &mut src.kind {
            Kind::Bytes(data) => {
                let c = data.get(src.pos).copied();
                if c.is_some() {
                    src.pos += 1;
                }
                return c;
            }
            Kind::Shared { buf, off, len } => {
                let c = if src.pos < *len { off.checked_add(src.pos).and_then(|i| buf.borrow().get(i).copied()) } else { None };
                if c.is_some() {
                    src.pos += 1;
                }
                return c;
            }
            Kind::Eexec { .. } => self.read_eexec(id, depth),
            Kind::Filter { .. } => self.read_filter(id, depth),
        };
        if let Some(s) = self.list.get_mut(id) {
            s.last = c;
            if c.is_some() {
                s.pos += 1;
            }
        }
        c
    }

    fn read_eexec(&mut self, id: usize, depth: usize) -> Option<u8> {
        let Some(Source { kind: Kind::Eexec { up, hex, .. }, .. }) = self.list.get(id) else { return None };
        let (up, hex) = (*up, *hex);
        let hex = match hex {
            Some(h) => h,
            None => {
                let h = self.leading_hex(up, depth)?;
                if let Some(Source { kind: Kind::Eexec { hex, .. }, .. }) = self.list.get_mut(id) {
                    *hex = Some(h);
                }
                h
            }
        };
        loop {
            let cipher = if hex {
                let mut digits = [0u8; 2];
                let mut n = 0;
                while n < 2 {
                    let c = self.read_depth(up, depth + 1)?;
                    if is_ws(c) {
                        continue;
                    }
                    digits[n] = hex_val(c)?;
                    n += 1;
                }
                digits[0] << 4 | digits[1]
            } else {
                self.read_depth(up, depth + 1)?
            };
            let Some(Source { kind: Kind::Eexec { r, skipped, .. }, .. }) = self.list.get_mut(id) else { return None };
            let plain = cipher ^ (*r >> 8) as u8;
            *r = (u16::from(cipher).wrapping_add(*r)).wrapping_mul(52845).wrapping_add(22719);
            // The first four plain bytes are random.
            if *skipped < 4 {
                *skipped += 1;
                continue;
            }
            return Some(plain);
        }
    }

    /// Whether the next four bytes after whitespace are hex digits (the whitespace is consumed
    /// when they are; the four bytes are pushed back either way).
    fn leading_hex(&mut self, up: usize, depth: usize) -> Option<bool> {
        let mut skipped = Vec::new();
        let first = loop {
            let c = self.read_depth(up, depth + 1)?;
            if !is_ws(c) {
                break c;
            }
            skipped.push(c);
        };
        let mut four = vec![first];
        while four.len() < 4 {
            match self.read_depth(up, depth + 1) {
                Some(c) => four.push(c),
                None => break,
            }
        }
        let hex = four.len() == 4 && four.iter().all(|c| hex_val(*c).is_some());
        self.unread_many(up, &four);
        if !hex {
            self.unread_many(up, &skipped);
        }
        Some(hex)
    }

    /// Push bytes back so they are read again, in order.
    pub fn unread_many(&mut self, id: usize, bytes: &[u8]) {
        let Some(src) = self.list.get_mut(id) else { return };
        match src.kind {
            Kind::Bytes(_) | Kind::Shared { .. } if src.back.is_empty() => src.pos = src.pos.saturating_sub(bytes.len()),
            _ => src.back.extend(bytes.iter().rev()),
        }
    }

    /// Step back over the byte just read (the scanner's look-ahead).
    pub fn unread(&mut self, id: usize) {
        let Some(src) = self.list.get_mut(id) else { return };
        match src.kind {
            Kind::Bytes(_) | Kind::Shared { .. } if src.back.is_empty() => src.pos = src.pos.saturating_sub(1),
            _ => {
                if let Some(c) = src.last {
                    src.back.push(c);
                }
            }
        }
    }

    pub fn peek(&mut self, id: usize) -> Option<u8> {
        let c = self.read(id)?;
        self.unread(id);
        Some(c)
    }

    /// A decoding filter over `up`. Unsupported filters give `Err`.
    pub fn filter(&mut self, up: usize, name: &[u8], p: &FilterParams) -> Result<usize, String> {
        let dec = match name {
            b"ASCIIHexDecode" | b"AHx" => Dec::Hex { hi: None },
            b"ASCII85Decode" | b"A85" => Dec::A85 { group: [0; 5], n: 0, first: true },
            b"RunLengthDecode" | b"RL" => Dec::RunLength,
            b"LZWDecode" | b"LZW" => {
                let dec = if p.early_change {
                    weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
                } else {
                    weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8)
                };
                Dec::Lzw { dec: Box::new(dec), pending: Vec::new() }
            }
            b"FlateDecode" | b"Fl" => {
                Dec::Flate { state: miniz_oxide::inflate::stream::InflateState::new_boxed(miniz_oxide::DataFormat::Zlib), pending: Vec::new() }
            }
            b"DCTDecode" | b"DCT" => Dec::Dct,
            b"SubFileDecode" => Dec::SubFile {
                eod: p.eod_string.clone(),
                count: p.eod_count,
                // A byte count; with no count and no end string, everything passes through.
                left: (p.eod_string.is_empty() && p.eod_count > 0).then_some(p.eod_count),
                window: VecDeque::new(),
            },
            b"NullEncode" => Dec::SubFile { eod: Vec::new(), count: 0, left: None, window: VecDeque::new() },
            b"ReusableStreamDecode" => {
                // Reads its data when made, and can be read again (`resetfile`, positions).
                let mut data = Vec::new();
                while let Some(c) = self.read(up) {
                    data.push(c);
                    if data.len() > MAX_DECODED {
                        return Err("reusable stream too large".into());
                    }
                }
                return Ok(self.add(Source::bytes(data)));
            }
            _ => return Err(format!("unsupported filter {}", String::from_utf8_lossy(name))),
        };
        let mut s = Source::of(Kind::Filter { up, dec, out: VecDeque::new(), done: false, produced: 0 });
        if p.predictor > 1 && matches!(name, b"FlateDecode" | b"Fl" | b"LZWDecode" | b"LZW") {
            let colors = usize::try_from(p.colors.clamp(1, 32)).unwrap_or(1);
            let bpc = usize::try_from(p.bpc.clamp(1, 16)).unwrap_or(8);
            let columns = usize::try_from(p.columns.clamp(1, 1 << 20)).unwrap_or(1);
            let row = (colors * bpc * columns).div_ceil(8);
            s.predictor = Some(Predictor { kind: p.predictor, bpp: (colors * bpc).div_ceil(8).max(1), row, prev: vec![0; row], cur: Vec::new() });
        }
        Ok(self.add(s))
    }

    fn read_filter(&mut self, id: usize, depth: usize) -> Option<u8> {
        loop {
            let src = self.list.get_mut(id)?;
            let Kind::Filter { out, done, .. } = &mut src.kind else { return None };
            if let Some(c) = out.pop_front() {
                return Some(c);
            }
            if *done {
                return None;
            }
            if !self.fill(id, depth)
                && let Some(Source { kind: Kind::Filter { done, .. }, .. }) = self.list.get_mut(id)
            {
                // What the last step produced is still read first.
                *done = true;
            }
        }
    }

    /// Decode some more of filter `id`; false once its data has ended.
    fn fill(&mut self, id: usize, depth: usize) -> bool {
        let Some(Source { kind: Kind::Filter { up, dec, .. }, .. }) = self.list.get(id) else { return false };
        let up = *up;
        let mut produced: Vec<u8> = Vec::new();
        let mut more = true;
        match dec {
            Dec::Dct => more = false,
            Dec::Hex { hi } => {
                let mut hi = *hi;
                while produced.is_empty() {
                    let Some(c) = self.read_depth(up, depth + 1) else {
                        more = false;
                        break;
                    };
                    if c == b'>' {
                        more = false;
                        break;
                    }
                    let Some(v) = hex_val(c) else { continue };
                    match hi.take() {
                        Some(h) => produced.push(h << 4 | v),
                        None => hi = Some(v),
                    }
                }
                if !more && let Some(h) = hi.take() {
                    produced.push(h << 4);
                }
                if let Some(Source { kind: Kind::Filter { dec: Dec::Hex { hi: slot }, .. }, .. }) = self.list.get_mut(id) {
                    *slot = hi;
                }
            }
            Dec::A85 { group, n, first } => {
                let (mut group, mut n, mut first) = (*group, *n, *first);
                while produced.is_empty() {
                    let Some(c) = self.read_depth(up, depth + 1) else {
                        more = false;
                        break;
                    };
                    if is_ws(c) {
                        continue;
                    }
                    if c == b'<' && first {
                        // A leading `<~`.
                        if self.read_depth(up, depth + 1) == Some(b'~') {
                            first = false;
                            continue;
                        }
                        more = false;
                        break;
                    }
                    first = false;
                    if c == b'~' {
                        let _ = self.read_depth(up, depth + 1);
                        more = false;
                        break;
                    }
                    if c == b'z' && n == 0 {
                        produced.extend([0; 4]);
                        continue;
                    }
                    if !(b'!'..=b'u').contains(&c) {
                        more = false;
                        break;
                    }
                    if let Some(slot) = group.get_mut(n) {
                        *slot = c - b'!';
                    }
                    n += 1;
                    if n == 5 {
                        let v = group.iter().fold(0u64, |a, &d| a * 85 + u64::from(d)).min(u64::from(u32::MAX));
                        produced.extend(u32::try_from(v).unwrap_or(u32::MAX).to_be_bytes());
                        n = 0;
                    }
                }
                if !more && n > 1 {
                    for slot in group.iter_mut().skip(n) {
                        *slot = 84;
                    }
                    let v = group.iter().fold(0u64, |a, &d| a * 85 + u64::from(d)).min(u64::from(u32::MAX));
                    produced.extend(u32::try_from(v).unwrap_or(u32::MAX).to_be_bytes().iter().take(n - 1));
                    n = 0;
                }
                if let Some(Source { kind: Kind::Filter { dec: Dec::A85 { group: g, n: nn, first: f }, .. }, .. }) = self.list.get_mut(id) {
                    *g = group;
                    *nn = n;
                    *f = first;
                }
            }
            Dec::RunLength => match self.read_depth(up, depth + 1) {
                None | Some(128) => more = false,
                Some(len @ 0..=127) => {
                    for _ in 0..=len {
                        match self.read_depth(up, depth + 1) {
                            Some(c) => produced.push(c),
                            None => {
                                more = false;
                                break;
                            }
                        }
                    }
                }
                Some(len) => match self.read_depth(up, depth + 1) {
                    Some(c) => produced.extend(std::iter::repeat_n(c, 257 - usize::from(len))),
                    None => more = false,
                },
            },
            Dec::SubFile { eod, count, left, window } => {
                let (eod, mut count, mut left, mut window) = (eod.clone(), *count, *left, window.clone());
                if let Some(n) = left.as_mut() {
                    if *n == 0 {
                        more = false;
                    } else if let Some(c) = self.read_depth(up, depth + 1) {
                        *n -= 1;
                        produced.push(c);
                    } else {
                        more = false;
                    }
                } else if eod.is_empty() {
                    match self.read_depth(up, depth + 1) {
                        Some(c) => produced.push(c),
                        None => more = false,
                    }
                } else {
                    while produced.is_empty() && more {
                        let Some(c) = self.read_depth(up, depth + 1) else {
                            produced.extend(window.drain(..));
                            more = false;
                            break;
                        };
                        window.push_back(c);
                        if window.len() >= eod.len() && window.iter().skip(window.len() - eod.len()).eq(eod.iter()) {
                            if count == 0 {
                                let keep = window.len() - eod.len();
                                produced.extend(window.drain(..keep));
                                window.clear();
                                more = false;
                                break;
                            }
                            count -= 1;
                        }
                        while window.len() > eod.len() {
                            if let Some(b) = window.pop_front() {
                                produced.push(b);
                            }
                        }
                    }
                }
                if let Some(Source { kind: Kind::Filter { dec: Dec::SubFile { count: c, left: l, window: w, .. }, .. }, .. }) = self.list.get_mut(id)
                {
                    *c = count;
                    *l = left;
                    *w = window;
                }
            }
            Dec::Flate { .. } | Dec::Lzw { .. } => {
                // Feed what is pending, or a chunk from beneath; keep what wasn't used.
                let mut input = match self.list.get_mut(id).map(|s| &mut s.kind) {
                    Some(Kind::Filter { dec: Dec::Flate { pending, .. } | Dec::Lzw { pending, .. }, .. }) => std::mem::take(pending),
                    _ => Vec::new(),
                };
                let mut eof = false;
                if input.is_empty() {
                    while input.len() < 4096 {
                        match self.read_depth(up, depth + 1) {
                            Some(c) => input.push(c),
                            None => {
                                eof = true;
                                break;
                            }
                        }
                    }
                }
                let mut buf = vec![0u8; 64 * 1024];
                let (used, written, finished) = match self.list.get_mut(id).map(|s| &mut s.kind) {
                    Some(Kind::Filter { dec: Dec::Flate { state, .. }, .. }) => {
                        let r = miniz_oxide::inflate::stream::inflate(state, &input, &mut buf, miniz_oxide::MZFlush::None);
                        let finished = matches!(r.status, Ok(miniz_oxide::MZStatus::StreamEnd) | Err(_));
                        (r.bytes_consumed, r.bytes_written, finished)
                    }
                    Some(Kind::Filter { dec: Dec::Lzw { dec, .. }, .. }) => {
                        // One byte at a time: the decoder must not read past its end code.
                        let (mut used, mut written, mut finished) = (0, 0, false);
                        while used < input.len() && written + 4096 < buf.len() {
                            let r = dec.decode_bytes(input.get(used..used + 1).unwrap_or(&[]), buf.get_mut(written..).unwrap_or(&mut []));
                            used += r.consumed_in;
                            written += r.consumed_out;
                            if matches!(r.status, Ok(weezl::LzwStatus::Done) | Err(_)) {
                                finished = true;
                                break;
                            }
                            if r.consumed_in == 0 && r.consumed_out == 0 {
                                break;
                            }
                        }
                        (used, written, finished)
                    }
                    _ => (0, 0, true),
                };
                produced.extend_from_slice(buf.get(..written).unwrap_or(&[]));
                let rest = input.get(used..).unwrap_or(&[]).to_vec();
                if finished {
                    // The stream ended inside this chunk: what follows belongs to the file.
                    self.unread_many(up, &rest);
                    more = false;
                } else if used == 0 && written == 0 && (eof || input.is_empty()) {
                    more = false;
                } else if let Some(Source { kind: Kind::Filter { dec: Dec::Flate { pending, .. } | Dec::Lzw { pending, .. }, .. }, .. }) =
                    self.list.get_mut(id)
                {
                    *pending = rest;
                }
            }
        }
        // Predictors work on whole rows.
        let predicted = match self.list.get_mut(id).and_then(|s| s.predictor.as_mut()) {
            Some(pr) => unpredict(pr, &produced, !more),
            None => produced,
        };
        let Some(Source { kind: Kind::Filter { out, produced: total, .. }, .. }) = self.list.get_mut(id) else { return false };
        *total = total.saturating_add(predicted.len());
        if *total > MAX_DECODED {
            return false;
        }
        out.extend(predicted);
        more
    }

    /// The JPEG a DCTDecode filter reads: through its end marker when it sits on the program
    /// itself, all of the filter beneath otherwise.
    pub fn dct(&mut self, id: usize) -> Option<Rc<Vec<u8>>> {
        let src = self.list.get(id)?;
        if let Some(d) = &src.dct {
            return Some(d.clone());
        }
        let Kind::Filter { up, dec: Dec::Dct, .. } = &src.kind else { return None };
        let up = *up;
        let direct = match self.list.get(up) {
            Some(Source { kind: Kind::Bytes(data), pos, back, .. }) if back.is_empty() => Some((data.clone(), *pos)),
            _ => None,
        };
        let raw = match direct {
            Some((data, pos)) => {
                let rest = data.get(pos..).unwrap_or(&[]);
                let end = jpeg_end(rest).unwrap_or(rest.len());
                if let Some(s) = self.list.get_mut(up) {
                    s.pos = s.pos.saturating_add(end);
                }
                rest.get(..end).unwrap_or(&[]).to_vec()
            }
            None => {
                let mut raw = Vec::new();
                while let Some(c) = self.read(up) {
                    raw.push(c);
                    if raw.len() > MAX_DECODED {
                        break;
                    }
                }
                raw
            }
        };
        let raw = Rc::new(raw);
        if let Some(s) = self.list.get_mut(id) {
            s.dct = Some(raw.clone());
        }
        Some(raw)
    }

    pub fn is_filter(&self, id: usize) -> bool {
        matches!(self.list.get(id).map(|s| &s.kind), Some(Kind::Filter { .. }))
    }

    /// Read a filter to its end marker, so the file beneath carries on after the data.
    pub fn drain(&mut self, id: usize) {
        let mut n = 0usize;
        while n < MAX_DECODED && self.read(id).is_some() {
            n += 1;
        }
    }

    pub fn close(&mut self, id: usize) {
        if let Some(s) = self.list.get_mut(id) {
            s.closed = true;
        }
    }

    pub fn reset(&mut self, id: usize) {
        if let Some(s) = self.list.get_mut(id)
            && matches!(s.kind, Kind::Bytes(_) | Kind::Shared { .. })
        {
            s.pos = 0;
            s.back.clear();
            s.closed = false;
        }
    }

    pub fn position(&self, id: usize) -> Option<usize> {
        let s = self.list.get(id)?;
        Some(s.pos.saturating_sub(s.back.len()))
    }

    pub fn set_position(&mut self, id: usize, pos: usize) -> bool {
        match self.list.get_mut(id) {
            Some(s) if matches!(&s.kind, Kind::Bytes(d) if pos <= d.len()) || matches!(&s.kind, Kind::Shared { len, .. } if pos <= *len) => {
                s.pos = pos;
                s.back.clear();
                s.closed = false;
                true
            }
            _ => false,
        }
    }

    pub fn available(&self, id: usize) -> i64 {
        match self.list.get(id) {
            Some(s) if s.closed => -1,
            Some(Source { kind: Kind::Bytes(d), pos, back, .. }) => i64::try_from(d.len().saturating_sub(*pos) + back.len()).unwrap_or(i64::MAX),
            Some(Source { kind: Kind::Shared { len, .. }, pos, back, .. }) => {
                i64::try_from(len.saturating_sub(*pos) + back.len()).unwrap_or(i64::MAX)
            }
            Some(_) => 1,
            None => -1,
        }
    }
}

/// Undo a predictor on newly decoded bytes (`last`: no more will come).
fn unpredict(p: &mut Predictor, data: &[u8], last: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if p.kind == 2 {
        // TIFF: 8-bit horizontal differencing.
        for &b in data {
            p.cur.push(b);
            if p.cur.len() == p.row {
                for i in p.bpp..p.row {
                    let prev = p.cur.get(i - p.bpp).copied().unwrap_or(0);
                    if let Some(c) = p.cur.get_mut(i) {
                        *c = c.wrapping_add(prev);
                    }
                }
                out.append(&mut p.cur);
            }
        }
        if last {
            out.append(&mut p.cur);
        }
        return out;
    }
    // PNG: a filter byte before each row.
    for &b in data {
        p.cur.push(b);
        if p.cur.len() == p.row + 1 {
            let kind = p.cur.first().copied().unwrap_or(0);
            let mut row: Vec<u8> = p.cur.get(1..).unwrap_or(&[]).to_vec();
            for i in 0..row.len() {
                let a = if i >= p.bpp { row.get(i - p.bpp).copied().unwrap_or(0) } else { 0 };
                let up = p.prev.get(i).copied().unwrap_or(0);
                let c = if i >= p.bpp { p.prev.get(i - p.bpp).copied().unwrap_or(0) } else { 0 };
                let add = match kind {
                    1 => a,
                    2 => up,
                    3 => ((u16::from(a) + u16::from(up)) / 2) as u8,
                    4 => {
                        let pp = i16::from(a) + i16::from(up) - i16::from(c);
                        let (pa, pb, pc) = ((pp - i16::from(a)).abs(), (pp - i16::from(up)).abs(), (pp - i16::from(c)).abs());
                        if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            up
                        } else {
                            c
                        }
                    }
                    _ => 0,
                };
                if let Some(v) = row.get_mut(i) {
                    *v = v.wrapping_add(add);
                }
            }
            out.extend_from_slice(&row);
            p.prev = row;
            p.cur.clear();
        }
    }
    out
}

/// The length of the JPEG at the start of `d` (through its EOI marker).
fn jpeg_end(d: &[u8]) -> Option<usize> {
    if d.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    loop {
        while d.get(i)? == &0xFF && d.get(i + 1)? == &0xFF {
            i += 1;
        }
        if d.get(i)? != &0xFF {
            return None;
        }
        let m = *d.get(i + 1)?;
        i += 2;
        match m {
            0xD9 => return Some(i),
            0x01 | 0xD0..=0xD7 => continue,
            _ => {
                let len = usize::from(u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?]));
                i = i.checked_add(len)?;
                if m == 0xDA {
                    // Entropy-coded data: up to the next marker that isn't a stuffed byte or restart.
                    loop {
                        if d.get(i)? == &0xFF {
                            let n = *d.get(i + 1)?;
                            if n != 0 && !(0xD0..=0xD7).contains(&n) {
                                break;
                            }
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(s: &mut Sources, id: usize) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(c) = s.read(id) {
            out.push(c);
        }
        out
    }

    #[test]
    fn filters_stop_at_their_end_markers() {
        let mut s = Sources::default();
        let id = s.add(Source::bytes(b"48656c6c6f>rest".to_vec()));
        let f = s.filter(id, b"ASCIIHexDecode", &FilterParams::default()).unwrap();
        assert_eq!(all(&mut s, f), b"Hello");
        assert_eq!(all(&mut s, id), b"rest");
        let id = s.add(Source::bytes(b"87cURD]i,\"Ebo80~>rest".to_vec()));
        let f = s.filter(id, b"ASCII85Decode", &FilterParams::default()).unwrap();
        assert_eq!(all(&mut s, f), b"Hello World!");
        assert_eq!(all(&mut s, id), b"rest");
        let id = s.add(Source::bytes(vec![2, b'a', b'b', b'c', 254, b'x', 128, b'!']));
        let f = s.filter(id, b"RunLengthDecode", &FilterParams::default()).unwrap();
        assert_eq!(all(&mut s, f), b"abcxxx");
        assert_eq!(all(&mut s, id), b"!");
        let id = s.add(Source::bytes(b"data%%EndBinary tail".to_vec()));
        let p = FilterParams { eod_string: b"%%EndBinary".to_vec(), ..Default::default() };
        let f = s.filter(id, b"SubFileDecode", &p).unwrap();
        assert_eq!(all(&mut s, f), b"data");
        assert_eq!(all(&mut s, id), b" tail");
    }

    fn ascii85(data: &[u8]) -> String {
        let mut out = String::new();
        for chunk in data.chunks(4) {
            let mut b = [0u8; 4];
            b[..chunk.len()].copy_from_slice(chunk);
            let mut v = u32::from_be_bytes(b);
            let mut digits = [0u8; 5];
            for d in digits.iter_mut().rev() {
                *d = (v % 85) as u8 + b'!';
                v /= 85;
            }
            out.push_str(std::str::from_utf8(&digits[..chunk.len() + 1]).unwrap());
        }
        out
    }

    #[test]
    fn flate_and_lzw_read_only_their_streams() {
        let mut z = miniz_oxide::deflate::compress_to_vec_zlib(&b"abcabcabc".repeat(2000), 6);
        z.extend_from_slice(b"tail");
        let mut s = Sources::default();
        let id = s.add(Source::bytes(z));
        let f = s.filter(id, b"FlateDecode", &FilterParams::default()).unwrap();
        assert_eq!(all(&mut s, f), b"abcabcabc".repeat(2000));
        assert_eq!(all(&mut s, id), b"tail");
        // Flate over ASCII85 over the program: bytes pushed back return to the filter beneath.
        let z = miniz_oxide::deflate::compress_to_vec_zlib(b"xyz", 6);
        let mut s = Sources::default();
        let id = s.add(Source::bytes(format!("{}~> after", ascii85(&z)).into_bytes()));
        let a = s.filter(id, b"ASCII85Decode", &FilterParams::default()).unwrap();
        let f = s.filter(a, b"FlateDecode", &FilterParams::default()).unwrap();
        assert_eq!(all(&mut s, f), b"xyz");
        assert_eq!(all(&mut s, id), b" after");
        let mut enc = weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8);
        let mut l = enc.encode(b"lzw lzw lzw").unwrap();
        l.extend_from_slice(b"!");
        let mut s = Sources::default();
        let id = s.add(Source::bytes(l));
        let f = s.filter(id, b"LZWDecode", &FilterParams { early_change: true, ..Default::default() }).unwrap();
        assert_eq!(all(&mut s, f), b"lzw lzw lzw");
        assert_eq!(all(&mut s, id), b"!");
    }

    #[test]
    fn eexec_decrypts_binary_and_hex() {
        let encrypt = |plain: &[u8]| {
            let mut r: u16 = 55665;
            let mut out = Vec::new();
            for &p in [0u8, 0, 0, 0].iter().chain(plain) {
                let c = p ^ (r >> 8) as u8;
                r = (u16::from(c).wrapping_add(r)).wrapping_mul(52845).wrapping_add(22719);
                out.push(c);
            }
            out
        };
        let enc = encrypt(b"hello");
        for hex in [false, true] {
            let data = if hex { enc.iter().map(|b| format!("{b:02x}")).collect::<String>().into_bytes() } else { enc.clone() };
            let mut s = Sources::default();
            let up = s.add(Source::bytes(data));
            let e = s.add(Source::eexec(up));
            assert_eq!(s.peek(e), Some(b'h'), "look-ahead inside the section");
            let got: Vec<u8> = (0..5).filter_map(|_| s.read(e)).collect();
            assert_eq!(got, b"hello", "hex {hex}");
        }
    }

    #[test]
    fn broken_filter_data_ends_the_filter_not_the_program() {
        for (name, data) in [
            (&b"ASCIIHexDecode"[..], &b"zz>"[..]),
            (b"ASCII85Decode", b"\x7f~>"),
            (b"FlateDecode", b"\x78\x9c\xff\xff"),
            (b"RunLengthDecode", b"\x05a"),
            (b"LZWDecode", b"\xff\xff\xff"),
        ] {
            let mut s = Sources::default();
            let id = s.add(Source::bytes(data.to_vec()));
            let f = s.filter(id, name, &FilterParams::default()).unwrap();
            let _ = all(&mut s, f);
        }
        let mut s = Sources::default();
        let id = s.add(Source::bytes(b"\xff\xd8\xff".to_vec()));
        let f = s.filter(id, b"DCTDecode", &FilterParams::default()).unwrap();
        assert!(s.dct(f).is_some());
    }
}
