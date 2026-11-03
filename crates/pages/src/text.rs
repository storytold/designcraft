//! Text storages (`TSWP.StorageArchive`): the text in field 3 and attribute tables keyed by
//! UTF-16 index: paragraph styles (5), character styles (8) and languages (19). An entry without
//! a style means no character style; a paragraph entry without one keeps the previous style.

use designcraft_doc::story::{BASIC_PARAGRAPH, CharFormat, CharRun, FORCED_LINE_BREAK, NO_CHAR_STYLE, ParaFormat, Story};
use designcraft_doc::{Align, Capitalization, CharAttrs, Leading, ParaAttrs, Position, StoryId};

use crate::import::Importer;
use crate::proto::Msg;
use crate::style::{CharProps, ParaProps};

/// The text with Pages' special characters mapped, and the byte offset in it of every UTF-16
/// index of the original (one past the end included).
pub(crate) fn map_text(raw: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(raw.len());
    let mut map = Vec::with_capacity(raw.len() + 1);
    for c in raw.chars() {
        let at = out.len();
        map.extend(std::iter::repeat_n(at, c.len_utf16()));
        match c {
            // Paragraph, section, page and column ends all end the paragraph.
            '\n' | '\r' | '\u{2029}' | '\u{4}' | '\u{5}' | '\u{C}' => out.push('\n'),
            '\u{2028}' | '\u{B}' => out.push(FORCED_LINE_BREAK),
            '\t' => out.push('\t'),
            // Attachments (U+FFFC: inline objects, fields) and notes (U+000E) aren't read yet.
            '\u{FFFC}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    map.push(out.len());
    (out, map)
}

/// The entries of attribute table `field`: (byte offset, entry), in order.
fn table<'a>(storage: Msg<'a>, field: u32, map: &[usize], len: usize) -> Vec<(usize, Msg<'a>)> {
    let Some(t) = storage.msg(field) else { return Vec::new() };
    let mut out: Vec<(usize, Msg)> = t
        .msgs(1)
        .filter_map(|e| {
            let i = usize::try_from(e.uint(1)?).ok()?;
            Some((map.get(i).copied().unwrap_or(len).min(len), e))
        })
        .collect();
    out.sort_by_key(|(p, _)| *p);
    out
}

/// The last entry at or before `pos`.
fn at<T: Copy>(entries: &[(usize, T)], pos: usize) -> Option<T> {
    let n = entries.partition_point(|(p, _)| *p <= pos);
    entries.get(n.checked_sub(1)?).map(|(_, v)| *v)
}

impl Importer<'_> {
    /// The story of text storage `storage`, with id `id`.
    pub(crate) fn story(&mut self, id: StoryId, storage: Msg) -> Story {
        let raw: String =
            storage.all(3).filter_map(|v| if let crate::proto::Value::Bytes(b) = v { std::str::from_utf8(b).ok() } else { None }).collect();
        if raw.contains('\u{FFFC}') {
            self.warnings.insert("inline objects are not imported yet".into());
        }
        let (mut text, map) = map_text(&raw);
        if text.ends_with('\n') {
            text.pop();
        }
        let len = text.len();
        // Paragraph entries without a style keep the previous one.
        let mut paras: Vec<(usize, u64)> = Vec::new();
        for (p, e) in table(storage, 5, &map, len) {
            if let Some(s) = e.reference(2) {
                paras.push((p, s));
            }
        }
        let chars: Vec<(usize, Option<u64>)> = table(storage, 8, &map, len).into_iter().map(|(p, e)| (p, e.reference(2))).collect();
        let langs: Vec<(usize, Option<&str>)> = table(storage, 19, &map, len).into_iter().map(|(p, e)| (p, e.str(2))).collect();

        let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let mut story = Story::new(id);
        story.paras = starts.iter().map(|&s| self.para_format(at(&paras, s))).collect();

        let mut cuts: Vec<usize> =
            starts.iter().copied().chain(chars.iter().map(|c| c.0)).chain(langs.iter().map(|l| l.0)).filter(|&p| p < len).collect();
        cuts.push(0);
        cuts.sort_unstable();
        cuts.dedup();
        let mut runs: Vec<CharRun> = Vec::new();
        for (i, &from) in cuts.iter().enumerate() {
            let to = cuts.get(i + 1).copied().unwrap_or(len);
            let para_start = starts.get(starts.partition_point(|&s| s <= from).saturating_sub(1)).copied().unwrap_or(0);
            let over = self.char_attrs(at(&paras, para_start), at(&chars, from).flatten(), at(&langs, from).flatten());
            let format = CharFormat { style: NO_CHAR_STYLE.into(), over };
            match runs.last_mut() {
                Some(last) if last.format == format => last.len += to - from,
                _ => runs.push(CharRun { len: to - from, format }),
            }
        }
        if text.is_empty() {
            runs.truncate(1);
            if let Some(r) = runs.first_mut() {
                r.len = 0;
            }
        }
        story.text = text;
        if !runs.is_empty() {
            story.chars = runs;
        }
        story
    }

    fn para_format(&mut self, style: Option<u64>) -> ParaFormat {
        let p = style.map(|s| self.paragraph_style(s).1).unwrap_or_default();
        ParaFormat { style: BASIC_PARAGRAPH.into(), para: para_attrs(&p), ..Default::default() }
    }

    /// The overrides of a run in a paragraph of style `para` with character style `chr`.
    fn char_attrs(&mut self, para: Option<u64>, chr: Option<u64>, lang: Option<&str>) -> CharAttrs {
        let (mut c, p) = para.map(|s| self.paragraph_style(s)).unwrap_or_default();
        if let Some(cs) = chr {
            c.overlay(&self.character_style(cs));
        }
        let mut a = CharAttrs::default();
        if let Some(ps) = &c.font {
            let (family, style) = self.font(ps, c.bold == Some(true), c.italic == Some(true));
            a.font_family = Some(family);
            a.font_style = Some(style);
        }
        a.size = c.size;
        a.leading = leading(&c, &p).map(Leading::Points);
        a.tracking = c.tracking.map(|t| t * 1000.0);
        a.baseline_shift = c.shift;
        a.underline = c.underline;
        a.strikethrough = c.strike;
        a.capitalization = c.caps.and_then(|v| match v {
            0 => Some(Capitalization::Normal),
            1 => Some(Capitalization::AllCaps),
            2 => Some(Capitalization::SmallCaps),
            _ => None,
        });
        a.position = c.baseline.and_then(|v| match v {
            0 => Some(Position::Normal),
            1 => Some(Position::Superscript),
            2 => Some(Position::Subscript),
            _ => None,
        });
        a.fill = c.color.map(|rgba| self.swatch(rgba));
        a.language = lang.filter(|l| !l.is_empty()).map(str::to_string);
        a
    }
}

/// Leading in points, from the paragraph's line spacing and the run's size.
fn leading(c: &CharProps, p: &ParaProps) -> Option<f64> {
    let (mode, amount) = p.spacing?;
    let line = c.size.unwrap_or(12.0) * 1.2;
    let v = match mode.unwrap_or(0) {
        0 => amount * line,
        1 => amount.max(line),
        2 => amount,
        4 => line + amount,
        _ => return None,
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

fn para_attrs(p: &ParaProps) -> ParaAttrs {
    ParaAttrs {
        align: p.align.map(|v| match v {
            1 => Align::Right,
            2 => Align::Center,
            3 => Align::LeftJustified,
            _ => Align::Left,
        }),
        left_indent: p.left_indent,
        right_indent: p.right_indent,
        // Pages measures the first line from the margin, DesignCraft from the left indent.
        first_line_indent: p.first_indent.map(|f| f - p.left_indent.unwrap_or(0.0)),
        space_before: p.space_before,
        space_after: p.space_after,
        ..Default::default()
    }
}
