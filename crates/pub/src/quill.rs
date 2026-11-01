// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Quill text and formatting decoded from docs/pub-format/text-store.md.
use crate::{
    PubError, Result,
    read::{count, number, properties, record, slice, u16_at, u32_at},
};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};
/// File property keys are bytes: a fixed bitmap bounds and deduplicates diagnostics.
#[derive(Default, Clone, Copy)]
pub(crate) struct UnsupportedFields([u64; 4]);
impl UnsupportedFields {
    fn insert(&mut self, key: u8) {
        if let Some(word) = self.0.get_mut(usize::from(key) / 64) {
            *word |= 1u64 << (key % 64);
        }
    }
    fn is_empty(self) -> bool {
        self.0.iter().all(|word| *word == 0)
    }
    fn contains(self, key: u8) -> bool {
        self.0.get(usize::from(key) / 64).is_some_and(|word| word & (1u64 << (key % 64)) != 0)
    }
    fn merge(&mut self, other: Self) {
        for (word, value) in self.0.iter_mut().zip(other.0) {
            *word |= value;
        }
    }
    fn describe(self) -> String {
        let count: u32 = self.0.iter().map(|v| v.count_ones()).sum();
        if count == 0 {
            return "none".into();
        }
        let keys = (0..=u8::MAX).filter(|k| self.0.get(usize::from(*k) / 64).is_some_and(|w| w & (1u64 << (k % 64)) != 0));
        let mut text = keys.take(16).map(|k| format!("0x{k:02X}")).collect::<Vec<_>>().join(", ");
        if count > 16 {
            text.push_str(&format!(", plus {} more", count - 16));
        }
        text
    }
}
#[derive(Default, Clone)]
pub(crate) struct CharStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub size_pt: Option<f64>,
    pub font: Option<u32>,
    pub color: Option<u32>,
    pub capitalization: Option<designcraft_doc::Capitalization>,
    pub position: Option<designcraft_doc::Position>,
    pub tracking: Option<f64>,
    pub unsupported: UnsupportedFields,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LineSpacing {
    Exact(f64),
    Multiple(f64),
}
#[derive(Default, Clone)]
pub(crate) struct Bullet {
    pub character: Option<char>,
    pub font: Option<u32>,
}
#[derive(Default, Clone)]
pub(crate) struct ParaStyle {
    pub align: Option<u32>,
    pub bullet: Option<Bullet>,
    pub line_spacing: Option<LineSpacing>,
    pub unsupported: UnsupportedFields,
    pub space_before_pt: Option<f64>,
    pub space_after_pt: Option<f64>,
    pub first_indent_pt: Option<f64>,
    pub left_indent_pt: Option<f64>,
    pub right_indent_pt: Option<f64>,
}
#[derive(Default)]
pub(crate) struct QStory {
    pub units: Vec<u16>,
    pub cell_ranges: Option<Vec<Range<usize>>>,
    pub chars: Vec<(Range<usize>, CharStyle)>,
    pub paras: Vec<(Range<usize>, ParaStyle)>,
}
#[derive(Default)]
pub(crate) struct Quill {
    pub stories: HashMap<u32, QStory>,
    pub fonts: Vec<String>,
    pub colors: Vec<u32>,
    pub default_char: CharStyle,
    pub default_para: ParaStyle,
    pub warnings: Vec<String>,
}
fn char_style(b: &[u8]) -> Result<CharStyle> {
    let mut s = CharStyle::default();
    let mut color_descriptor = None;
    let fields = properties(b)?;
    // OBSERVATION: unknown-fields.md records these exact same-record copies.
    // Equality is a warning rule only; no meaning is assigned to either key.
    let matching_copy = |key, partner, kind| {
        let mut copies = fields.iter().filter(|p| p.key == key);
        let mut partners = fields.iter().filter(|p| p.key == partner);
        match (copies.next(), partners.next()) {
            (Some(a), Some(b)) => a.kind == kind && b.kind == kind && a.data == b.data && copies.next().is_none() && partners.next().is_none(),
            _ => false,
        }
    };
    let matching_39 = matching_copy(0x39, 0x0c, 0x22);
    let matching_48 = matching_copy(0x48, 0x44, 0x8a);
    for p in fields {
        match p.key {
            2 => s.bold = Some(true),
            3 => s.italic = Some(true),
            0x1e => s.underline = Some(p.number().is_none_or(|v| v != 0)),
            0x0c => s.size_pt = p.number().map(|v| f64::from(v) / 12_700.0),
            0x2e => s.color = p.number(),
            0x44 => s.color = number(&p.children()?, 0),
            0x24 => {
                if let Some(font) = p.children()?.iter().find(|v| v.kind == 0x88) {
                    s.font = font.children()?.first().and_then(|v| v.number());
                }
            }
            0x13 | 0x14 if p.kind == 0x0a => {
                s.capitalization =
                    Some(if p.key == 0x13 { designcraft_doc::Capitalization::SmallCaps } else { designcraft_doc::Capitalization::AllCaps });
            }
            0x0f if p.kind == 0x12 && p.number() == Some(1) => s.position = Some(designcraft_doc::Position::Superscript),
            0x1f if p.kind == 0x1a => s.tracking = p.number().map(|v| f64::from(v as u16 as i16) / 10.0),
            0x58 => {
                if color_descriptor.replace(p).is_some() {
                    s.unsupported.insert(p.key);
                }
            }
            0x39 if matching_39 => {}
            0x48 if matching_48 => {}
            0x12 | 0x3e => {}
            _ => s.unsupported.insert(p.key),
        }
    }
    if let Some(descriptor) = color_descriptor {
        let redundant = if descriptor.kind == 0x8a {
            let fields = descriptor.children()?;
            fields.len() == 3
                && (0..=2).all(|key| fields.iter().filter(|p| p.key == key).count() == 1)
                && fields.iter().all(|p| p.kind == if p.key == 0 { 0x12 } else { 0x22 })
                && number(&fields, 0) == Some(1)
                && number(&fields, 1) == s.color
                && s.color.is_some()
                && number(&fields, 2) == Some(100_000)
        } else {
            false
        };
        if !redundant {
            s.unsupported.insert(0x58);
        }
    }
    Ok(s)
}
fn para_style(b: &[u8]) -> Result<ParaStyle> {
    let p = properties(b)?;
    let pt = |key| number(&p, key).map(|v| f64::from(v as i32) / 12_700.0);
    let mut s = ParaStyle {
        align: number(&p, 4).map(|v| v & 0xff),
        space_before_pt: pt(0x12),
        space_after_pt: pt(0x13),
        first_indent_pt: pt(0x0c),
        left_indent_pt: pt(0x0d),
        right_indent_pt: pt(0x0e),
        ..Default::default()
    };
    for v in &p {
        match v.key {
            0x34 => {
                s.line_spacing = v.number().and_then(|raw| {
                    let payload = f64::from(raw & !3);
                    match raw & 3 {
                        1 if (0.1..=1296.0).contains(&(payload / 101_600.0)) => Some(LineSpacing::Exact(payload / 101_600.0)),
                        2 if (0.1..=10.0).contains(&(payload / 1_219_200.0)) => Some(LineSpacing::Multiple(payload / 1_219_200.0)),
                        _ => None,
                    }
                });
                if s.line_spacing.is_none() {
                    s.unsupported.insert(v.key);
                }
            }
            0x57 => {
                if v.kind != 0x8a {
                    s.unsupported.insert(v.key);
                    continue;
                }
                let descriptor = v.children()?;
                if number(&descriptor, 0) == Some(23) {
                    let character = number(&descriptor, 1).and_then(char::from_u32).filter(|c| !c.is_control());
                    if character.is_none() || number(&descriptor, 2).is_some_and(|n| n != 0) || descriptor.iter().any(|p| p.key > 2) {
                        s.unsupported.insert(v.key);
                    }
                    s.bullet = Some(Bullet { character, font: number(&p, 3) });
                } else {
                    s.unsupported.insert(v.key);
                }
            }
            // OBSERVATION: unknown-fields.md pairs 0x3C with imported alignment 6.
            // Its value remains unexplained; other alignments still warn numerically.
            0x3c if s.align == Some(6) => {}
            0x02 | 0x03 | 0x04 | 0x0c | 0x0d | 0x0e | 0x12 | 0x13 => {}
            _ => s.unsupported.insert(v.key),
        }
    }
    Ok(s)
}
struct Section<'a> {
    tag: &'a [u8],
    at: usize,
    data: &'a [u8],
}
fn sections(b: &[u8]) -> Result<Vec<Section<'_>>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut at = 0x18;
    while at != u32::MAX as usize {
        if seen.len() >= 64 || !seen.insert(at) {
            return Err(PubError::Malformed("Quill directory loop or depth limit".into()));
        }
        let header = slice(b, at, 8)?;
        let n = usize::from(u16_at(header, 2)?);
        let entries = slice(b, at.checked_add(8).ok_or(PubError::Truncated)?, n * 24)?;
        for e in entries.as_chunks::<24>().0 {
            let pos = u32_at(e, 16)? as usize;
            out.push(Section { tag: slice(e, 2, 4)?, at: pos, data: slice(b, pos, u32_at(e, 20)? as usize)? });
            if out.len() > 100_000 {
                return Err(PubError::Malformed("too many Quill sections".into()));
            }
        }
        at = u32_at(header, 4)? as usize;
    }
    Ok(out)
}
fn runs<T>(
    sections: &[Section<'_>],
    tag: &[u8],
    text_at: usize,
    text_len: usize,
    decode: impl Fn(&[u8]) -> Result<T>,
) -> Result<Vec<(Range<usize>, T)>> {
    let mut ends = Vec::new();
    for section in sections.iter().filter(|s| s.tag == tag) {
        for page in section.data.chunks(512) {
            let n = usize::from(u16_at(page, 0)?);
            slice(page, 8, n * 6)?;
            for i in 0..n {
                if ends.len() >= 100_000 {
                    return Err(PubError::Malformed("too many formatting runs".into()));
                }
                let end = (u32_at(page, 8 + 4 * i)? as usize).saturating_sub(text_at) / 2;
                let at = usize::from(u16_at(page, 8 + 4 * n + 2 * i)?);
                ends.push((end.min(text_len), decode(record(page, at)?)?));
            }
        }
    }
    ends.sort_by_key(|(end, _)| *end);
    let mut start = 0;
    Ok(ends
        .into_iter()
        .map(|(end, style)| {
            let range = start..end;
            start = end;
            (range, style)
        })
        .collect())
}
fn cut<T: Clone>(runs: &[(Range<usize>, T)], story: &Range<usize>) -> Vec<(Range<usize>, T)> {
    runs.iter()
        .skip(runs.partition_point(|(r, _)| r.end <= story.start))
        .take_while(|(r, _)| r.start < story.end)
        .filter_map(|(r, s)| {
            let a = r.start.max(story.start);
            let z = r.end.min(story.end);
            (a < z).then(|| (a - story.start..z - story.start, s.clone()))
        })
        .collect()
}
pub(crate) fn parse(b: &[u8]) -> Result<Quill> {
    if b.len() > 256 * 1024 * 1024 {
        return Err(PubError::Malformed("Quill stream exceeds limit".into()));
    }
    let sections = sections(b)?;
    let get = |tag: &[u8]| sections.iter().find(|s| s.tag == tag);
    let mut q = Quill::default();
    if let Some(s) = get(b"FONT") {
        let n = count(u32_at(s.data, 4)?, 10_000)?;
        let mut at = 20 + 4 * n;
        slice(s.data, 20, 4 * n)?;
        for _ in 0..n {
            let len = usize::from(u16_at(s.data, at)?) * 2;
            let units: Vec<_> = slice(s.data, at + 2, len)?.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
            q.fonts.push(String::from_utf16_lossy(&units));
            slice(s.data, at + 2 + len, 4)?;
            at += 6 + len;
        }
    }
    if let Some(s) = get(b"PL  ") {
        let n = count(u32_at(s.data, 0)?, 10_000)?;
        let mut at = 12;
        for _ in 0..n {
            let len = u32_at(s.data, at)? as usize;
            let value = if len < 4 { 0 } else { number(&properties(record(s.data, at)?)?, 1).unwrap_or(0) };
            q.colors.push(value);
            at = at.checked_add(len.max(4)).ok_or(PubError::Truncated)?;
        }
    }
    if let Some(s) = sections.iter().filter(|s| s.tag == b"STSH").nth(1) {
        let n = count(u32_at(s.data, 4)?, 100_000)?;
        slice(s.data, 20, n * 4)?;
        for i in 0..n.min(2) {
            let at = (u32_at(s.data, 20 + i * 4)? as usize).checked_add(22).ok_or(PubError::Truncated)?;
            let props = record(s.data, at)?;
            if i == 0 {
                q.default_char = char_style(props)?;
            } else {
                q.default_para = para_style(props)?;
            }
        }
    }
    // STSH records without TEXT do not describe any imported text.
    let Some(text) = get(b"TEXT") else { return Ok(q) };
    if text.data.len() / 2 > 16_000_000 {
        return Err(PubError::Malformed("text exceeds limit".into()));
    }
    let chars = runs(&sections, b"FDPC", text.at, text.data.len() / 2, char_style)?;
    let paras = runs(&sections, b"FDPP", text.at, text.data.len() / 2, para_style)?;
    if let (Some(ids), Some(lengths)) = (get(b"SYID"), get(b"STRS")) {
        let ni = count(u32_at(ids.data, 4)?, 100_000)?;
        let nl = count(u32_at(lengths.data, 0)?, 100_000)?;
        let lengths_at = (u32_at(lengths.data, 4)? as usize).checked_add(4).ok_or(PubError::Truncated)?;
        slice(ids.data, 8, ni * 4)?;
        slice(lengths.data, lengths_at, nl * 4)?;
        let mut start = 0usize;
        for i in 0..ni.min(nl) {
            let id = u32_at(ids.data, 8 + 4 * i)?;
            let len = u32_at(lengths.data, lengths_at + 4 * i)? as usize;
            let end = start.saturating_add(len).min(text.data.len() / 2);
            let range = start..end;
            let units = slice(text.data, start * 2, (end - start) * 2)?.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
            q.stories.insert(id, QStory { units, chars: cut(&chars, &range), paras: cut(&paras, &range), ..Default::default() });
            start = end;
        }
    }
    let mut used = false;
    let mut char_fields = UnsupportedFields::default();
    let mut para_fields = UnsupportedFields::default();
    for story in q.stories.values().filter(|s| !s.units.is_empty()) {
        used = true;
        for (_, style) in story.chars.iter().filter(|(r, _)| !r.is_empty()) {
            char_fields.merge(style.unsupported);
        }
        for (_, style) in story.paras.iter().filter(|(r, _)| !r.is_empty()) {
            para_fields.merge(style.unsupported);
        }
    }
    if used {
        for (kind, defaults, runs) in [("character", q.default_char.unsupported, char_fields), ("paragraph", q.default_para.unsupported, para_fields)]
        {
            if !defaults.is_empty() || !runs.is_empty() {
                let mut fields = defaults;
                fields.merge(runs);
                // Name only fields established by text-store.md §§11–12. Unknown list kinds,
                // tab stops, numbering and strikethrough stay numeric: their encodings are unconfirmed.
                let known: &[(u8, &str)] = if kind == "character" {
                    &[(0x13, "small caps"), (0x14, "all caps"), (0x0f, "superscript/position"), (0x1f, "letter spacing")]
                } else {
                    &[(0x34, "line spacing"), (0x57, "list marker (unconfirmed descriptor)")]
                };
                let dropped = known.iter().filter(|(key, _)| fields.contains(*key)).map(|(_, name)| *name).collect::<Vec<_>>();
                let detail = if dropped.is_empty() { String::new() } else { format!(" Not fully imported: {}.", dropped.join(", ")) };
                q.warnings.push(format!(
                    "Publisher {kind} formatting is unsupported for fields: defaults [{}]; runs [{}]. Supported attributes were retained.{detail}",
                    defaults.describe(),
                    runs.describe()
                ));
            }
        }
    }
    let tcd: Vec<_> = sections.iter().filter(|s| s.tag == b"TCD ").collect();
    if !tcd.is_empty() {
        if tcd.len() == 1 && q.stories.len() == 1 {
            if let Some(story) = q.stories.values_mut().next() {
                let data = tcd.first().ok_or(crate::PubError::Truncated)?.data;
                if u32_at(data, 4)? == 0 && u32_at(data, 8)? == 0xff00 {
                    story.cell_ranges = Some(crate::tables::text_ranges(data, &story.units)?);
                } else {
                    q.warnings.push("Unrecognized Publisher TCD cell-text header was not decoded.".into());
                }
            }
        } else {
            q.warnings.push("Publisher table text across multiple stories/TCD sections is not decoded.".into());
        }
    }
    Ok(q)
}

#[cfg(test)]
mod tests {
    fn stream(entries: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut b = vec![0; 32 + entries.len() * 24];
        b[26..28].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        b[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        for (i, (tag, data)) in entries.iter().enumerate() {
            let at = 32 + i * 24;
            b[at + 2..at + 6].copy_from_slice(*tag);
            let offset = b.len() as u32;
            b[at + 16..at + 20].copy_from_slice(&offset.to_le_bytes());
            b[at + 20..at + 24].copy_from_slice(&(data.len() as u32).to_le_bytes());
            b.extend(data);
        }
        b
    }

    #[test]
    fn maximal_directory_and_relative_style_offsets_are_rejected() {
        for offset in [u32::MAX - 1, u32::MAX - 7, 31] {
            let mut b = stream(&[]);
            b[28..32].copy_from_slice(&offset.to_le_bytes());
            assert!(sections(&b).is_err());
        }
        for offset in [u32::MAX, u32::MAX - 21, 24] {
            let mut style = vec![0; 24];
            style[4..8].copy_from_slice(&1u32.to_le_bytes());
            style[20..24].copy_from_slice(&offset.to_le_bytes());
            assert!(parse(&stream(&[(b"STSH", vec![]), (b"STSH", style)])).is_err());
            let lengths = [1u32.to_le_bytes(), offset.to_le_bytes()].concat();
            let ids = [0u32.to_le_bytes(), 1u32.to_le_bytes(), 1u32.to_le_bytes()].concat();
            assert!(parse(&stream(&[(b"TEXT", vec![0, 0]), (b"STRS", lengths), (b"SYID", ids)])).is_err());
        }
    }

    #[test]
    fn warning_3c_is_quiet_only_when_justified_alignment_is_imported() {
        for value in [0x4832u16, 0x0032] {
            let mut extra = vec![0x3c, 0x12];
            extra.extend(value.to_le_bytes());
            for reverse in [false, true] {
                let alignment = vec![4, 0x12, 6, 0];
                let bytes = if reverse { [extra.clone(), alignment].concat() } else { [alignment, extra.clone()].concat() };
                let style = para_style(&bytes).unwrap();
                assert_eq!(style.align, Some(6));
                assert!(style.unsupported.is_empty());
            }
            assert_eq!(para_style(&extra).unwrap().unsupported.describe(), "0x3C");
            for align in [0u8, 1, 2, 7] {
                let bytes = [vec![4, 0x12, align, 0], extra.clone()].concat();
                assert_eq!(para_style(&bytes).unwrap().unsupported.describe(), "0x3C");
            }
        }
        // A justified record cannot suppress a field in a different record.
        assert_eq!(para_style(&[4, 0x12, 6, 0]).unwrap().align, Some(6));
        assert_eq!(para_style(&[0x3c, 0x12, 0x32, 0]).unwrap().unsupported.describe(), "0x3C");
    }
    #[test]
    fn warning_copy_fields_require_equal_partners_in_the_same_record() {
        fn field(key: u8, kind: u8, value: u32) -> Vec<u8> {
            let mut b = vec![key, kind];
            if kind == 0x8a {
                b.extend(10u32.to_le_bytes());
                b.extend([0, 0x22]);
            }
            b.extend(value.to_le_bytes());
            b
        }
        for (key, partner, kind) in [(0x39, 0x0c, 0x22), (0x48, 0x44, 0x8a)] {
            let copy = field(key, kind, 127_000);
            let equal = field(partner, kind, 127_000);
            let different = field(partner, kind, 254_000);
            for bytes in [[copy.clone(), equal.clone()].concat(), [equal.clone(), copy.clone()].concat()] {
                assert!(char_style(&bytes).unwrap().unsupported.is_empty());
            }
            for bytes in
                [copy.clone(), [copy.clone(), different].concat(), [copy.clone(), equal.clone(), equal].concat(), [copy.clone(), copy].concat()]
            {
                assert_eq!(char_style(&bytes).unwrap().unsupported.describe(), format!("0x{key:02X}"));
            }
            // A partner in another record cannot suppress this record's warning.
            assert!(char_style(&field(partner, kind, 127_000)).unwrap().unsupported.is_empty());
            assert_eq!(char_style(&field(key, kind, 127_000)).unwrap().unsupported.describe(), format!("0x{key:02X}"));
            assert_eq!(char_style(&[key, 0x0a, partner, 0x0a]).unwrap().unsupported.describe(), format!("0x{key:02X}"));
        }
    }
    #[test]
    fn warning_redundant_colour_requires_exact_matching_descriptor() {
        fn props(index: u32, scale: u32, duplicate: bool, paired: bool, reverse: bool) -> Vec<u8> {
            let mut nested = vec![0, 0x12, 1, 0, 1, 0x22];
            nested.extend(index.to_le_bytes());
            nested.extend([2, 0x22]);
            nested.extend(scale.to_le_bytes());
            if duplicate {
                nested.extend([1, 0x22]);
                nested.extend(index.to_le_bytes());
            }
            let mut descriptor = vec![0x58, 0x8a];
            descriptor.extend(((nested.len() + 4) as u32).to_le_bytes());
            descriptor.extend(nested);
            let colour = vec![0x44, 0x8a, 10, 0, 0, 0, 0, 0x22, 2, 0, 0, 0];
            if !paired {
                descriptor
            } else if reverse {
                [descriptor, colour].concat()
            } else {
                [colour, descriptor].concat()
            }
        }
        for reverse in [true, false] {
            let style = char_style(&props(2, 100_000, false, true, reverse)).unwrap();
            assert!(style.unsupported.is_empty());
            assert_eq!(style.color, Some(2));
        }
        for bytes in [
            props(3, 100_000, false, true, false),
            props(2, 50_000, false, true, false),
            props(2, 100_000, true, true, false),
            props(2, 100_000, false, false, false),
        ] {
            assert_eq!(char_style(&bytes).unwrap().unsupported.describe(), "0x58");
        }
    }
    #[test]
    fn warning_field_sets_are_bounded_for_all_input_keys() {
        let bytes: Vec<_> = (0..=u8::MAX).flat_map(|key| [key, 0x0a]).collect();
        let fields = char_style(&bytes).unwrap().unsupported;
        let text = fields.describe();
        assert_eq!(text.matches("0x").count(), 16);
        assert!(text.contains("more"));
        assert!(text.len() < 150);
    }
    use super::*;
    #[test]
    fn directory_cycles_and_bad_section_ranges_are_errors() {
        let mut b = vec![0; 32];
        b[28..32].copy_from_slice(&24u32.to_le_bytes());
        assert!(parse(&b).is_err());
        b[26..28].copy_from_slice(&1u16.to_le_bytes());
        b[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&b).is_err());
    }
    #[test]
    fn character_formats_and_signed_paragraph_values() {
        let c = char_style(&[2, 0x0a, 3, 0x0a, 0x1e, 0x12, 0, 0, 0x44, 0x8a, 10, 0, 0, 0, 0, 0x22, 2, 0, 0, 0]).unwrap();
        assert_eq!(c.bold, Some(true));
        assert_eq!(c.italic, Some(true));
        assert_eq!(c.underline, Some(false));
        assert_eq!(c.color, Some(2));
        let mut bytes = vec![0x0c, 0x22];
        bytes.extend_from_slice(&(-228600i32).to_le_bytes());
        assert_eq!(para_style(&bytes).unwrap().first_indent_pt, Some(-18.0));
    }
    #[test]
    fn formatting_is_clipped_and_rebased_to_each_story() {
        let runs = vec![(0..6, 1), (6..13, 2), (13..25, 3)];
        assert_eq!(cut(&runs, &(10..20)), vec![(0..3, 2), (3..10, 3)]);
        assert!(cut(&runs, &(25..25)).is_empty());
    }
}
