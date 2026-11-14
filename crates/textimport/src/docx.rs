//! DOCX to DesignCraft story adapter. Package, XML, styles and numbering are read by craft-docx;
//! this module maps the neutral Word model into the app's story and style model.

use std::collections::{HashMap, HashSet};

use craft_wordmodel::Block;
use craft_wordmodel::numbering::LevelSuffix;
use craft_wordmodel::para::{InlineObject, NoteKind, Paragraph};
use craft_wordmodel::props::{Align as WordAlign, CharProps as WordCharProps, NumRef, ParaProps as WordParaProps, Underline, VertAlign};
use craft_wordmodel::section::NumFormat;
use craft_wordmodel::styles::{StyleKind, StyleSheet};
use designcraft_doc::story::{BASIC_PARAGRAPH, NO_CHAR_STYLE};
use designcraft_doc::{CharAttrs, CharFormat, ListCounter, ListType, NumberStyle, ParaAttrs, ParaFormat, ParaProps, Story, StoryId, Table};

use crate::{ImportError, Imported, ImportedStyle};

const MAX_NAME_CHARS: usize = 256;
const MAX_NOTE_BYTES: usize = 8 << 20;
const MAX_WARNINGS: usize = 100;

fn capped_name(name: &str) -> String {
    name.chars().take(MAX_NAME_CHARS).collect()
}

fn style_names(styles: &StyleSheet) -> HashMap<String, String> {
    styles.styles.iter().map(|s| (s.id.clone(), capped_name(&s.name))).collect()
}

fn char_attrs(p: &WordCharProps) -> CharAttrs {
    CharAttrs {
        font_family: p.font.as_ref().map(|s| capped_name(s)),
        font_style: match (p.bold, p.italic) {
            (Some(true), Some(true)) => Some("Bold Italic".into()),
            (Some(true), _) => Some("Bold".into()),
            (_, Some(true)) => Some("Italic".into()),
            (Some(false), Some(false)) | (Some(false), None) | (None, Some(false)) => Some("Regular".into()),
            _ => None,
        },
        size: p.size.map(f64::from).and_then(designcraft_doc::attrs::imported_type_size),
        underline: p.underline.map(|u| u != Underline::None),
        strikethrough: p.strike.or(p.double_strike),
        capitalization: p.caps.filter(|&on| on).map(|_| designcraft_doc::Capitalization::AllCaps),
        position: p.vert_align.map(|v| match v {
            VertAlign::Superscript => designcraft_doc::Position::Superscript,
            VertAlign::Subscript => designcraft_doc::Position::Subscript,
            VertAlign::Baseline => designcraft_doc::Position::Normal,
        }),
        ..Default::default()
    }
}

fn number_style(fmt: NumFormat) -> Option<NumberStyle> {
    match fmt {
        NumFormat::UpperRoman => Some(NumberStyle::UpperRoman),
        NumFormat::LowerRoman => Some(NumberStyle::LowerRoman),
        NumFormat::UpperLetter => Some(NumberStyle::UpperLetters),
        NumFormat::LowerLetter => Some(NumberStyle::LowerLetters),
        NumFormat::DecimalZero => Some(NumberStyle::ArabicLeadingZero),
        NumFormat::Chicago => Some(NumberStyle::Symbols),
        NumFormat::Bullet | NumFormat::None => None,
        _ => Some(NumberStyle::Arabic),
    }
}

fn bullet_char(text: &str, font: Option<&str>) -> String {
    let font = font.unwrap_or_default().to_ascii_lowercase();
    let symbol = font.contains("symbol");
    let wingdings = font.contains("wingdings");
    text.chars()
        .map(|c| {
            let code = u32::from(c);
            let low = if (0xF000..=0xF0FF).contains(&code) { code - 0xF000 } else { code };
            if wingdings {
                return match low {
                    0x6C => '●',
                    0x6E => '■',
                    0x71 => '❑',
                    0x76 => '❖',
                    0xA7 => '▪',
                    0xD8 => '➢',
                    0xFC => '✓',
                    _ => '•',
                };
            }
            if symbol {
                return match low {
                    0x2D => '–',
                    0x6F => '◦',
                    _ => '•',
                };
            }
            match c {
                'o' => '◦',
                '\u{E000}'..='\u{F8FF}' => '•',
                c => c,
            }
        })
        .filter(|c| *c != '\n' && *c != '\r')
        .collect()
}

fn style_numbering(style_id: &str, doc: &craft_wordmodel::Document) -> Option<NumRef> {
    let mut cur = Some(style_id);
    for _ in 0..32 {
        let id = cur?;
        let s = doc.styles.styles.iter().find(|s| s.id == id)?;
        if let Some(num) = s.para.numbering {
            return Some(num);
        }
        cur = s.based_on.as_deref();
    }
    None
}

#[derive(Default)]
struct NumberingState {
    counters: HashMap<String, [Option<u32>; 9]>,
    model_counter: ListCounter,
}

impl NumberingState {
    fn next_para(&mut self, num_ref: NumRef, doc: &craft_wordmodel::Document) -> Option<ParaAttrs> {
        let lv = usize::from(num_ref.level);
        if lv > 8 {
            return None;
        }
        let num = doc.numbering.num(num_ref.num)?;
        let abs = doc.numbering.abstract_of(num_ref.num)?;
        let level = |k: usize| doc.numbering.level(num_ref.num, k as u8);
        let def = level(lv)?;
        if def.text.is_empty() && def.format != NumFormat::Bullet && def.format != NumFormat::None {
            return None;
        }
        let own = !num.start_overrides.is_empty();
        let key = if own { format!("n:{}", num.id) } else { format!("a:{}", abs.id) };
        let start = |k: usize| {
            let o = if own { num.start_overrides.iter().find(|(l, _)| *l as usize == k).map(|(_, s)| *s) } else { None };
            o.or_else(|| level(k).map(|l| l.start)).unwrap_or(1)
        };
        let counters = self.counters.entry(key).or_default();
        let n = counters.get(lv).copied().flatten().map_or_else(|| start(lv), |v| v.saturating_add(1));
        if let Some(c) = counters.get_mut(lv) {
            *c = Some(n);
        }
        for (m, c) in counters.iter_mut().enumerate().skip(lv + 1) {
            let restart = match level(m) {
                None => true,
                Some(l) => {
                    if !l.restart {
                        false
                    } else {
                        match l.restart_after {
                            None => true,
                            Some(r) => lv < r as usize,
                        }
                    }
                }
            };
            if restart {
                *c = None;
            }
        }

        let parent = |k: usize| -> String {
            let v = counters.get(k).copied().flatten().unwrap_or_else(|| start(k));
            match level(k).map(|l| l.format) {
                Some(fmt) if !def.legal => number_style(fmt).map_or_else(String::new, |s| s.format(v)),
                Some(_) => v.to_string(),
                _ => String::new(),
            }
        };

        let mut parents = Vec::new();
        let mut label = |own_level: &str, escape: bool, refs: bool| -> String {
            let mut out = String::new();
            let mut it = def.text.chars().peekable();
            while let Some(c) = it.next() {
                if c == '%'
                    && let Some(k) = it.peek().and_then(|d| d.to_digit(10)).filter(|d| (1..=9).contains(d))
                {
                    it.next();
                    let k = k as usize - 1;
                    let is_number = matches!(level(k).map(|l| l.format), Some(f) if f != NumFormat::Bullet && f != NumFormat::None);
                    if k == lv {
                        out.push_str(own_level);
                    } else if refs && is_number && !def.legal {
                        out.push('^');
                        out.push_str(&(k + 1).to_string());
                        parents.push((k as u32 + 1, parent(k)));
                    } else {
                        let p = parent(k);
                        out.push_str(&if escape { p.replace('^', "^^") } else { p });
                    }
                } else if escape && c == '^' {
                    out.push_str("^^");
                } else if c != '\n' && c != '\r' {
                    out.push(c);
                }
            }
            out
        };

        let (number_sep, bullet_sep) = match def.suffix {
            LevelSuffix::Tab => ("^t", "\t"),
            LevelSuffix::Space => (" ", " "),
            LevelSuffix::Nothing => ("", ""),
        };
        let mut a = ParaAttrs::default();
        let mut fixed_expression = None;
        if def.format == NumFormat::Bullet {
            a.list_type = Some(ListType::Bullets);
            a.bullet_char = Some(bullet_char(&def.text, def.chr.font.as_deref()));
            a.list_separator = Some(bullet_sep.into());
        } else if def.format == NumFormat::None {
            a.list_type = Some(ListType::Bullets);
            a.bullet_char = Some(label("", false, false));
            a.list_separator = Some(bullet_sep.into());
        } else {
            a.list_type = Some(ListType::Numbers);
            a.number_style = number_style(def.format);
            a.number_expression = Some(format!("{}{number_sep}", label("^#", true, true)));
            fixed_expression = Some(format!("{}{number_sep}", label("^#", true, false)));
        }
        a.left_indent = designcraft_doc::attrs::imported_measure(f64::from(def.indent));
        a.first_line_indent = designcraft_doc::attrs::imported_measure(-f64::from(def.hanging));
        a.list_level = Some(u32::from(num_ref.level) + 1);
        if !def.restart {
            a.restart_numbers = Some(false);
        } else if let Some(r) = def.restart_after {
            a.restart_after_level = Some(u32::from(r));
        }

        if a.list_type == Some(ListType::Numbers) {
            let want = n.max(1);
            let mut pp = ParaProps::default();
            pp.apply(&a);
            let mut model = self.model_counter.clone();
            let start = (model.advance(&pp) != want).then_some(want);
            if start.is_some() {
                model = self.model_counter.clone();
                pp.start_at = start;
                model.advance(&pp);
            }
            let level_text = |k: u32| ParaProps { number_expression: format!("^{k}"), ..ParaProps::default() }.number_label_in(&model);
            let expression = parents.iter().any(|(k, text)| level_text(*k) != *text).then(|| fixed_expression.clone()).flatten();
            self.model_counter = model;
            if let Some(s) = start {
                a.start_at = Some(Some(s));
            }
            if let Some(e) = expression {
                a.number_expression = Some(e);
            }
        }

        Some(a)
    }

    fn reset_model_counter(&mut self) {
        self.model_counter = ListCounter::default();
    }
}

fn para_attrs(p: &WordParaProps) -> ParaAttrs {
    ParaAttrs {
        align: p.align.map(|a| match a {
            WordAlign::Center => designcraft_doc::Align::Center,
            WordAlign::Right => designcraft_doc::Align::Right,
            WordAlign::Justify | WordAlign::Distribute => designcraft_doc::Align::LeftJustified,
            WordAlign::Left => designcraft_doc::Align::Left,
        }),
        left_indent: p.indent_left.map(f64::from).and_then(designcraft_doc::attrs::imported_measure),
        right_indent: p.indent_right.map(f64::from).and_then(designcraft_doc::attrs::imported_measure),
        first_line_indent: p.indent_first.map(f64::from).and_then(designcraft_doc::attrs::imported_measure),
        space_before: p.space_before.map(f64::from).and_then(designcraft_doc::attrs::imported_measure),
        space_after: p.space_after.map(f64::from).and_then(designcraft_doc::attrs::imported_measure),
        ..Default::default()
    }
}

fn char_format(p: &WordCharProps, names: &HashMap<String, String>) -> CharFormat {
    let style = p.style.as_ref().and_then(|id| names.get(id)).cloned().unwrap_or_else(|| NO_CHAR_STYLE.into());
    CharFormat { style, over: char_attrs(p) }
}

fn style_records(styles: &StyleSheet) -> (Vec<ImportedStyle>, Vec<ImportedStyle>, HashMap<String, String>) {
    let names = style_names(styles);
    let mut paras = Vec::new();
    let mut chars = Vec::new();
    for style in &styles.styles {
        let name = names.get(&style.id).cloned().unwrap_or_else(|| capped_name(&style.name));
        let based_on = style.based_on.as_ref().and_then(|id| names.get(id)).cloned();
        let mut para = para_attrs(&style.para);
        let mut char_values = char_attrs(&style.chr);
        if based_on.is_none() {
            let mut defaults = para_attrs(&styles.default_para);
            defaults.merge(&para);
            para = defaults;
            let mut defaults = char_attrs(&styles.default_chr);
            defaults.merge(&char_values);
            char_values = defaults;
        }
        let record = ImportedStyle { name: name.clone(), based_on, para, chars: char_values };
        match style.kind {
            StyleKind::Paragraph => paras.push(record),
            StyleKind::Character if name != NO_CHAR_STYLE => chars.push(record),
            _ => {}
        }
    }
    (paras, chars, names)
}

fn plain_blocks(blocks: &[std::sync::Arc<Block>]) -> String {
    let mut out = String::new();
    let mut first = true;
    for block in blocks {
        match block.as_ref() {
            Block::Para(p) => {
                if !first {
                    out.push('\n');
                }
                first = false;
                let text = p.final_text();
                let is_break = |c: char| matches!(c, designcraft_doc::PAGE_BREAK | designcraft_doc::COLUMN_BREAK);
                out.push_str(&text.chars().map(|c| if is_break(c) { '\n' } else { c }).collect::<String>());
            }
            Block::Table(t) => {
                for row in &t.rows {
                    if !first {
                        out.push('\n');
                    }
                    first = false;
                    out.push_str(&row.cells.iter().map(|c| plain_blocks(&c.blocks)).collect::<Vec<_>>().join("\t"));
                }
            }
        }
    }
    out
}

fn word_text(c: char) -> char {
    use craft_wordmodel::para::{COLUMN_BREAK, LINE_BREAK, PAGE_BREAK};
    match c {
        LINE_BREAK => designcraft_doc::FORCED_LINE_BREAK,
        PAGE_BREAK => designcraft_doc::PAGE_BREAK,
        COLUMN_BREAK => designcraft_doc::COLUMN_BREAK,
        c => c,
    }
}

struct ImportContext<'a> {
    doc: &'a craft_wordmodel::Document,
    names: &'a HashMap<String, String>,
    default_para: String,
    warned_objects: bool,
    notes_done: HashSet<u32>,
    note_bytes: usize,
    warnings: Vec<String>,
    numbering: NumberingState,
}

fn append_paragraph(ctx: &mut ImportContext<'_>, p: &Paragraph, st: &mut Story) {
    let para_at = st.len();
    let style = p.props.style.as_ref().and_then(|id| ctx.names.get(id)).cloned().unwrap_or_else(|| ctx.default_para.clone());

    let mut attrs = para_attrs(&p.props);
    let num_ref = match p.props.numbering {
        Some(n) if n.num == 0 => None,
        Some(n) => Some(n),
        None => p.props.style.as_deref().and_then(|id| style_numbering(id, ctx.doc)),
    };
    let list_attrs = num_ref.and_then(|nr| ctx.numbering.next_para(nr, ctx.doc));
    if let Some(la) = list_attrs {
        let direct = attrs;
        attrs = la;
        attrs.merge(&direct);
    } else {
        ctx.numbering.reset_model_counter();
    }

    st.format_paras(para_at..para_at, |fmt| *fmt = ParaFormat { style: style.clone(), para: attrs.clone(), ..Default::default() });

    let mut object_i = 0usize;
    let mut last_format: Option<CharFormat> = None;
    let mut chunk = String::new();
    let flush = |st: &mut Story, fmt: &mut Option<CharFormat>, text: &mut String| {
        if !text.is_empty() {
            st.insert_with(st.len(), text, fmt.clone().unwrap_or_default());
            text.clear();
        }
    };
    for (at, c) in p.text.char_indices() {
        let run = p
            .runs
            .iter()
            .scan(0usize, |offset, r| {
                let range = *offset..offset.saturating_add(r.len);
                *offset = range.end;
                Some((range, r))
            })
            .find(|(range, _)| range.contains(&at))
            .map(|(_, r)| r);

        if run.is_some_and(|r| r.props.del.is_some()) {
            if c == craft_wordmodel::para::OBJ {
                object_i = object_i.saturating_add(1);
            }
            continue;
        }

        let fmt = run.map(|r| char_format(&r.props, ctx.names)).unwrap_or_default();
        if last_format.as_ref().is_some_and(|prev| prev != &fmt) {
            flush(st, &mut last_format, &mut chunk);
        }
        last_format = Some(fmt);
        if c == craft_wordmodel::para::OBJ {
            flush(st, &mut last_format, &mut chunk);
            if let Some(object) = p.objects.get(object_i) {
                match object {
                    InlineObject::NoteRef { kind: NoteKind::Footnote, id, .. } => {
                        let mut text = ctx.doc.parts.get(id).map(|part| plain_blocks(&part.blocks)).unwrap_or_default();
                        if ctx.notes_done.contains(id) {
                            text.clear();
                            add_warning(
                                &mut ctx.warnings,
                                format!("Footnote {id} is referenced more than once in the Word file: the text is kept at its first reference."),
                            );
                        } else if ctx.note_bytes.saturating_add(text.len()) > MAX_NOTE_BYTES {
                            text.clear();
                            add_warning(
                                &mut ctx.warnings,
                                format!("The Word file's footnotes are longer than {} MB: the rest were imported empty.", MAX_NOTE_BYTES / (1 << 20)),
                            );
                        } else {
                            ctx.notes_done.insert(*id);
                            ctx.note_bytes = ctx.note_bytes.saturating_add(text.len());
                        }
                        let _ = st.insert_note(st.len(), &text, ParaFormat::default());
                    }
                    InlineObject::NoteRef { .. } => {
                        ctx.warned_objects = true;
                        let _ = st.insert_note(st.len(), "", ParaFormat::default());
                    }
                    other => {
                        let text = other.plain_text();
                        if text.is_empty() {
                            ctx.warned_objects = true;
                        } else {
                            st.insert_with(st.len(), text, last_format.clone().unwrap_or_default());
                        }
                    }
                }
            } else {
                ctx.warned_objects = true;
            }
            object_i = object_i.saturating_add(1);
        } else {
            chunk.push(word_text(c));
        }
    }
    flush(st, &mut last_format, &mut chunk);
}

fn append_blocks(ctx: &mut ImportContext<'_>, blocks: &[std::sync::Arc<Block>], st: &mut Story) {
    let mut first = st.is_empty();
    for block in blocks {
        match block.as_ref() {
            Block::Para(p) => {
                if !first && !st.text.ends_with('\n') {
                    st.insert(st.len(), "\n");
                }
                first = false;
                append_paragraph(ctx, p, st);
            }
            Block::Table(table) => {
                let rows: Vec<Vec<String>> = table.rows.iter().map(|r| r.cells.iter().map(|c| plain_blocks(&c.blocks)).collect()).collect();
                if !rows.is_empty() {
                    let id = st.tables.keys().next_back().copied().unwrap_or(0).saturating_add(1);
                    let t = Table::from_strings(id, &rows, 300.0, &ParaFormat::default(), &CharFormat::default());
                    st.insert_table(st.len(), t);
                    first = st.text.ends_with('\n');
                }
            }
        }
    }
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let doc = craft_docx::read(bytes).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let (mut para_styles, mut char_styles, names) = style_records(&doc.styles);
    let builtin_styles = craft_wordmodel::styles::StyleSheet::builtin();
    let is_default_stylesheet = doc.styles.as_ref() == &builtin_styles;
    let is_explicit = |s: &craft_wordmodel::styles::Style| {
        if is_default_stylesheet {
            return false;
        }
        if s.id == "Normal" && builtin_styles.get("Normal") == Some(s) {
            return doc.styles.styles.first().is_some_and(|first| first.id == "Normal");
        }
        builtin_styles.get(&s.id).is_none_or(|b| b != s)
    };
    let default_para = doc
        .styles
        .styles
        .iter()
        .filter(|s| is_explicit(s))
        .find(|s| s.kind == StyleKind::Paragraph && s.default)
        .or_else(|| doc.styles.styles.iter().filter(|s| is_explicit(s)).find(|s| s.kind == StyleKind::Paragraph && s.id == "Normal"))
        .and_then(|s| names.get(&s.id))
        .cloned()
        .unwrap_or_else(|| BASIC_PARAGRAPH.into());
    let mut story = Story::new(StoryId(0));
    let mut ctx = ImportContext {
        doc: &doc,
        names: &names,
        default_para,
        warned_objects: false,
        notes_done: HashSet::new(),
        note_bytes: 0,
        warnings: Vec::new(),
        numbering: NumberingState::default(),
    };
    append_blocks(&mut ctx, &doc.body, &mut story);
    story.end_paragraphs_at_breaks();
    if ctx.warned_objects {
        add_warning(&mut ctx.warnings, "Images, drawings and endnotes in the Word file were not imported (place them separately).".into());
    }
    let used_p: HashSet<String> = story.paras.iter().map(|p| p.style.clone()).collect();
    let used_c: HashSet<String> = story.runs().map(|(_, f)| f.style.clone()).collect();
    let keep = |list: &mut Vec<ImportedStyle>, used: &HashSet<String>| {
        let mut want = used.clone();
        let mut todo: Vec<String> = used.iter().cloned().collect();
        while let Some(name) = todo.pop() {
            for base in list.iter().filter(|s| s.name == name).filter_map(|s| s.based_on.as_ref()) {
                if want.insert(base.clone()) {
                    todo.push(base.clone());
                }
            }
        }
        list.retain(|s| want.contains(&s.name));
    };
    keep(&mut para_styles, &used_p);
    keep(&mut char_styles, &used_c);
    Ok(Imported { story, para_styles, char_styles, warnings: ctx.warnings })
}

fn add_warning(warnings: &mut Vec<String>, message: String) {
    if warnings.contains(&message) || warnings.len() > MAX_WARNINGS {
        return;
    }
    if warnings.len() == MAX_WARNINGS {
        warnings.push(format!("The Word file has more than {MAX_WARNINGS} kinds of problem: the rest are not listed."));
    } else {
        warnings.push(message);
    }
}

#[cfg(test)]
mod tests;
