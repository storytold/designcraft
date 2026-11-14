//! DOCX to DesignCraft story adapter. Package, XML, styles and numbering are read by craft-docx;
//! this module maps the neutral Word model into the app's story and style model.

use std::collections::{HashMap, HashSet};

use craft_wordmodel::Block;
use craft_wordmodel::numbering::{Level, LevelSuffix};
use craft_wordmodel::para::{InlineObject, NoteKind, Paragraph};
use craft_wordmodel::props::{Align as WordAlign, CharProps as WordCharProps, ParaProps as WordParaProps, Underline, VertAlign};
use craft_wordmodel::section::NumFormat;
use craft_wordmodel::styles::{StyleKind, StyleSheet};
use designcraft_doc::story::{BASIC_PARAGRAPH, NO_CHAR_STYLE};
use designcraft_doc::{CharAttrs, CharFormat, ListCounter, ListType, NumberStyle, ParaAttrs, ParaFormat, Story, StoryId, Table};

use crate::{ImportError, Imported, ImportedStyle};

const MAX_NAME_CHARS: usize = 256;
const MAX_NOTE_BYTES: usize = 8 << 20;
const MAX_WARNINGS: usize = 100;

/// Word counts one abstract list across its instances, except a numbering instance with a
/// `startOverride`, which owns a separate counter. Keep that document behavior while mapping the
/// shared neutral definitions to the app's list model.
#[derive(Default)]
struct WordListCounter {
    values: HashMap<String, [Option<u32>; 9]>,
}

impl WordListCounter {
    fn next(&mut self, numbering: &craft_wordmodel::numbering::Numbering, num_id: u32, level: u8) -> Option<[u32; 9]> {
        let num = numbering.num(num_id)?;
        numbering.abstract_of(num_id)?;
        let level_index = usize::from(level);
        if level_index >= 9 || !numbering.level_is_defined(num_id, level) || numbering.level(num_id, level).is_none() {
            return None;
        }
        let own_counter = !num.start_overrides.is_empty();
        let key = if own_counter { format!("n:{num_id}") } else { format!("a:{}", num.abstract_id) };
        let start = |i: usize| {
            (own_counter)
                .then(|| num.start_overrides.iter().find(|(l, _)| usize::from(*l) == i).map(|(_, n)| *n))
                .flatten()
                .or_else(|| numbering.level(num_id, i as u8).map(|l| l.start))
                .unwrap_or(1)
        };
        let counters = self.values.entry(key).or_default();
        let current = counters[level_index].map_or_else(|| start(level_index), |n| n.saturating_add(1));
        counters[level_index] = Some(current);
        for (i, value) in counters.iter_mut().enumerate().skip(level_index + 1) {
            let restart =
                numbering.level(num_id, i as u8).is_none_or(|l| l.restart && l.restart_after.is_none_or(|after| level_index < usize::from(after)));
            if restart {
                *value = None;
            }
        }
        Some(std::array::from_fn(|i| counters[i].unwrap_or_else(|| start(i))))
    }
}

fn capped_name(name: &str) -> String {
    name.chars().take(MAX_NAME_CHARS).collect()
}

fn display_style_name(style: &craft_wordmodel::styles::Style) -> String {
    // The shared Word model canonicalizes built-in heading names for document editing. Text
    // import historically keeps the source's lowercase heading spelling in the app style map.
    if style.builtin
        && style.id.strip_prefix("Heading").is_some_and(|n| (1..=9).any(|level| n == level.to_string()))
        && style.name.starts_with("Heading ")
    {
        style.name.to_ascii_lowercase()
    } else {
        style.name.clone()
    }
}

fn style_names(styles: &StyleSheet) -> HashMap<String, String> {
    styles.styles.iter().map(|s| (s.id.clone(), capped_name(&display_style_name(s)))).collect()
}

fn char_attrs(p: &WordCharProps) -> CharAttrs {
    CharAttrs {
        font_family: p.font.as_ref().map(|s| capped_name(s)),
        font_style: match (p.bold, p.italic) {
            (Some(true), Some(true)) => Some("Bold Italic".into()),
            (Some(true), _) => Some("Bold".into()),
            (_, Some(true)) => Some("Italic".into()),
            (Some(false), Some(false)) => Some("Regular".into()),
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

fn paragraph_numbering(p: &WordParaProps, doc: &craft_wordmodel::Document) -> Option<craft_wordmodel::props::NumRef> {
    let numbering = if p.style.is_some() { doc.styles.resolve_para(p).numbering } else { p.numbering }?;
    (numbering.num != 0).then_some(numbering)
}

fn paragraph_level(p: &WordParaProps, doc: &craft_wordmodel::Document) -> Option<(craft_wordmodel::props::NumRef, Level)> {
    let numbering = paragraph_numbering(p, doc)?;
    if !doc.numbering.level_is_defined(numbering.num, numbering.level) {
        return None;
    }
    let level = doc.numbering.level(numbering.num, numbering.level)?.clone();
    Some((numbering, level))
}

fn suffix_text(suffix: LevelSuffix) -> &'static str {
    match suffix {
        LevelSuffix::Tab => "\t",
        LevelSuffix::Space => " ",
        LevelSuffix::Nothing => "",
    }
}

fn bullet_char(level: &Level) -> String {
    let font = level.chr.font.as_deref().unwrap_or_default();
    let font_lower = font.to_ascii_lowercase();
    level
        .text
        .chars()
        .take(64)
        .map(|c| {
            if let Some(mapped) = craft_wordmodel::symbol_fonts::symbol_char(font, c) {
                return mapped;
            }
            if font_lower.contains("wingdings") {
                let low = if ('\u{F000}'..='\u{F0FF}').contains(&c) { u32::from(c) - 0xF000 } else { u32::from(c) };
                return match low {
                    0x6C => '●',
                    0x6E | 0xA7 => '▪',
                    0x71 => '❑',
                    0x76 => '❖',
                    0xD8 => '➢',
                    0xFC => '✓',
                    _ => '•',
                };
            }
            match c {
                'o' | 'O' => '◦',
                '\u{E000}'..='\u{F8FF}' => '•',
                c => c,
            }
        })
        .filter(|c| *c != '\n' && *c != '\r')
        .collect()
}

fn number_expressions(
    level: &Level,
    numbering: &craft_wordmodel::numbering::Numbering,
    num: u32,
    own_level: u8,
    values: &[u32; 9],
) -> (String, String) {
    let suffix = match level.suffix {
        LevelSuffix::Tab => "^t",
        LevelSuffix::Space => " ",
        LevelSuffix::Nothing => "",
    };
    let mut model = String::new();
    let mut fixed = String::new();
    let mut chars = level.text.chars().take(64).peekable();
    while let Some(c) = chars.next() {
        if c == '%'
            && let Some(d) = chars.peek().and_then(|d| d.to_digit(10)).filter(|d| (1..=9).contains(d))
        {
            chars.next();
            let parent = (d - 1) as u8;
            if parent == own_level {
                model.push_str("^#");
                fixed.push_str("^#");
                continue;
            }
            let parent_format = numbering.level(num, parent).map(|p| p.format).unwrap_or(NumFormat::None);
            let text = if matches!(parent_format, NumFormat::Bullet | NumFormat::None) {
                String::new()
            } else {
                let format = if level.legal { NumFormat::Decimal } else { parent_format };
                format.format(values.get(usize::from(parent)).copied().unwrap_or(1))
            };
            if !level.legal && !matches!(parent_format, NumFormat::Bullet | NumFormat::None) {
                model.push('^');
                model.push(char::from_digit(d, 10).unwrap_or('1'));
            } else {
                model.push_str(&text);
            }
            fixed.push_str(&text);
        } else if c == '^' {
            model.push_str("^^");
            fixed.push_str("^^");
        } else {
            model.push(c);
            fixed.push(c);
        }
    }
    model.push_str(suffix);
    fixed.push_str(suffix);
    (model, fixed)
}

fn render_word_label(level: &Level, numbering: &craft_wordmodel::numbering::Numbering, num: u32, values: &[u32; 9]) -> String {
    let mut out = String::new();
    let mut chars = level.text.chars().take(64).peekable();
    while let Some(c) = chars.next() {
        if c == '%'
            && let Some(d) = chars.peek().and_then(|d| d.to_digit(10)).filter(|d| (1..=9).contains(d))
        {
            chars.next();
            let i = (d - 1) as usize;
            if let Some(parent) = numbering.level(num, i as u8) {
                let format =
                    if level.legal && !matches!(parent.format, NumFormat::None | NumFormat::Bullet) { NumFormat::Decimal } else { parent.format };
                out.push_str(&format.format(values.get(i).copied().unwrap_or(parent.start)));
            }
        } else {
            out.push(c);
        }
    }
    out.push_str(suffix_text(level.suffix));
    out
}

fn para_attrs(p: &WordParaProps, doc: &craft_wordmodel::Document) -> ParaAttrs {
    let mut out = ParaAttrs {
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
    };
    if let Some((numbering, level)) = paragraph_level(p, doc) {
        out.list_level = Some(u32::from(numbering.level) + 1);
        out.left_indent = Some(f64::from(level.indent));
        out.first_line_indent = Some(-f64::from(level.hanging));
        if level.format == NumFormat::Bullet {
            out.list_type = Some(ListType::Bullets);
            out.bullet_char = Some(bullet_char(&level));
            out.list_separator = Some(suffix_text(level.suffix).into());
        } else if level.format == NumFormat::None {
            out.list_type = Some(ListType::Bullets);
            out.bullet_char = Some(level.text.chars().take(64).collect());
            out.list_separator = Some(suffix_text(level.suffix).into());
        } else {
            out.list_type = Some(ListType::Numbers);
            out.number_style = Some(if level.legal {
                NumberStyle::Arabic
            } else {
                match level.format {
                    NumFormat::UpperRoman => NumberStyle::UpperRoman,
                    NumFormat::LowerRoman => NumberStyle::LowerRoman,
                    NumFormat::UpperLetter => NumberStyle::UpperLetters,
                    NumFormat::LowerLetter => NumberStyle::LowerLetters,
                    NumFormat::DecimalZero => NumberStyle::ArabicLeadingZero,
                    _ => NumberStyle::Arabic,
                }
            });
        }
    }
    out
}

fn char_format(p: &WordCharProps, names: &HashMap<String, String>) -> CharFormat {
    let style = p.style.as_ref().and_then(|id| names.get(id)).cloned().unwrap_or_else(|| NO_CHAR_STYLE.into());
    CharFormat { style, over: char_attrs(p) }
}

fn style_records(styles: &StyleSheet, doc: &craft_wordmodel::Document) -> (Vec<ImportedStyle>, Vec<ImportedStyle>, HashMap<String, String>) {
    let names = style_names(styles);
    let mut paras = Vec::new();
    let mut chars = Vec::new();
    for style in &styles.styles {
        let name = names.get(&style.id).cloned().unwrap_or_else(|| capped_name(&style.name));
        let based_on = style.based_on.as_ref().and_then(|id| names.get(id)).cloned();
        let mut para = para_attrs(&style.para, doc);
        let mut char_values = char_attrs(&style.chr);
        if based_on.is_none() {
            let mut defaults = para_attrs(&styles.default_para, doc);
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

/// Plain text under the importer's accepted-changes policy, with inline objects expanded. The
/// neutral Word model retains tracked deletions for editing, while text import treats changes as
/// accepted, just like [`append_paragraph`].
fn accepted_plain_text(p: &Paragraph) -> String {
    let mut out = String::with_capacity(p.text.len());
    let mut object_i = 0usize;
    let mut runs = p.run_ranges().peekable();
    for (at, c) in p.text.char_indices() {
        while runs.peek().is_some_and(|(range, _)| range.end <= at) {
            runs.next();
        }
        let deleted = runs.peek().is_some_and(|(range, props)| range.contains(&at) && props.del.is_some());
        if deleted {
            if c == craft_wordmodel::para::OBJ {
                object_i = object_i.saturating_add(1);
            }
            continue;
        }
        if c == craft_wordmodel::para::OBJ {
            if let Some(object) = p.objects.get(object_i) {
                out.push_str(object.plain_text());
            }
            object_i = object_i.saturating_add(1);
        } else {
            out.push(c);
        }
    }
    out
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
                out.push_str(&accepted_plain_text(p));
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
    default_para_style: Option<String>,
    word_numbering: WordListCounter,
    model_numbering: ListCounter,
    warned_objects: bool,
    notes_done: HashSet<u32>,
    note_bytes: usize,
    warnings: Vec<String>,
}

fn append_paragraph(ctx: &mut ImportContext<'_>, p: &Paragraph, st: &mut Story) {
    let para_at = st.len();
    let style = match p.props.style.as_ref() {
        Some(id) => ctx.names.get(id).cloned().unwrap_or_else(|| BASIC_PARAGRAPH.into()),
        None => ctx.default_para_style.clone().unwrap_or_else(|| BASIC_PARAGRAPH.into()),
    };
    let mut attrs = para_attrs(&p.props, ctx.doc);
    let has_numbering_reference = paragraph_numbering(&p.props, ctx.doc).is_some();
    let list = paragraph_level(&p.props, ctx.doc).and_then(|(num, level)| {
        let values = ctx.word_numbering.next(&ctx.doc.numbering, num.num, num.level)?;
        let number = values.get(usize::from(num.level)).copied().unwrap_or(level.start);
        let label = render_word_label(&level, &ctx.doc.numbering, num.num, &values);
        Some((num, level, number, values, label))
    });
    if has_numbering_reference && list.is_none() {
        add_warning(&mut ctx.warnings, "Some list information in the Word file could not be read.".into());
    }
    let mut fixed_expression = None;
    if let Some((num, level, _, values, _)) = &list
        && level.format != NumFormat::Bullet
        && level.format != NumFormat::None
    {
        let (model, fixed) = number_expressions(level, &ctx.doc.numbering, num.num, num.level, values);
        attrs.number_expression = Some(model);
        fixed_expression = Some(fixed);
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
        // The neutral DOCX model retains tracked deletions so callers that edit documents can
        // accept or reject them. Text import has always treated changes as accepted: preserve
        // insertions and move destinations, but omit deleted runs and move sources. Keep consuming
        // object indices even for deleted OBJ markers so later inline objects stay aligned.
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
    let empty = st.len() == para_at;
    if let Some((_, level, actual_number, _, word_label)) = list {
        if level.format != NumFormat::Bullet && level.format != NumFormat::None && !empty {
            let mut para = designcraft_doc::ParaProps::default();
            para.apply(&attrs);
            let mut model = ctx.model_numbering.clone();
            let calculated = model.advance(&para);
            if calculated != actual_number.max(1) {
                let start = actual_number.max(1);
                attrs.start_at = Some(Some(start));
                para.start_at = Some(start);
                model = ctx.model_numbering.clone();
                model.advance(&para);
            }
            if para.number_label_in(&model) != word_label
                && let Some(expression) = fixed_expression.take()
            {
                attrs.number_expression = Some(expression);
            }
            ctx.model_numbering = model;
        }
    } else {
        // The app counter restarts after a paragraph outside a Word list. Word's shared counter
        // remains independent and may resume an earlier list id later in the document.
        ctx.model_numbering = ListCounter::default();
    }
    st.format_paras(para_at..para_at, |fmt| fmt.para = attrs.clone());
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
    let (mut para_styles, mut char_styles, names) = style_records(&doc.styles, &doc);
    let default_para_style = doc
        .styles
        .styles
        .iter()
        .find(|style| style.kind == StyleKind::Paragraph && style.default)
        .or_else(|| {
            doc.styles.styles.iter().find(|style| {
                style.kind == StyleKind::Paragraph
                    && style.id == "Normal"
                    && (!style.builtin
                        || style.priority != Some(0)
                        || !style.quick
                        || style.para != WordParaProps::default()
                        || style.chr != WordCharProps::default())
            })
        })
        .and_then(|style| names.get(&style.id).cloned());
    let mut story = Story::new(StoryId(0));
    let mut ctx = ImportContext {
        doc: &doc,
        names: &names,
        default_para_style,
        word_numbering: WordListCounter::default(),
        model_numbering: ListCounter::default(),
        warned_objects: false,
        notes_done: HashSet::new(),
        note_bytes: 0,
        warnings: Vec::new(),
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
