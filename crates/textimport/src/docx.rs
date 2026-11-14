//! DOCX to DesignCraft story adapter. Package, XML, styles and numbering are read by craft-docx;
//! this module maps the neutral Word model into the app's story and style model.

use std::collections::{HashMap, HashSet};

use craft_wordmodel::Block;
use craft_wordmodel::para::{InlineObject, NoteKind, Paragraph};
use craft_wordmodel::props::{Align as WordAlign, CharProps as WordCharProps, ParaProps as WordParaProps, Underline, VertAlign};
use craft_wordmodel::section::NumFormat;
use craft_wordmodel::styles::{StyleKind, StyleSheet};
use designcraft_doc::story::{BASIC_PARAGRAPH, NO_CHAR_STYLE};
use designcraft_doc::{CharAttrs, CharFormat, ListType, NumberStyle, ParaAttrs, ParaFormat, Story, StoryId, Table};

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
    if let Some(numbering) = p.numbering.filter(|n| n.num != 0) {
        let definition = doc.numbering.nums.iter().find(|n| n.id == numbering.num).and_then(|n| {
            n.level_overrides.iter().find(|(level, _)| *level == numbering.level).map(|(_, def)| def).or_else(|| {
                let abs = doc.numbering.abstracts.iter().find(|a| a.id == n.abstract_id)?;
                abs.levels.get(usize::from(numbering.level))
            })
        });
        if let Some(level) = definition {
            if level.format == NumFormat::Bullet {
                out.list_type = Some(ListType::Bullets);
                out.bullet_char = Some(level.text.chars().next().unwrap_or('•').to_string());
            } else if level.format != NumFormat::None {
                out.list_type = Some(ListType::Numbers);
                out.number_style = Some(match level.format {
                    NumFormat::UpperRoman => NumberStyle::UpperRoman,
                    NumFormat::LowerRoman => NumberStyle::LowerRoman,
                    NumFormat::UpperLetter => NumberStyle::UpperLetters,
                    NumFormat::LowerLetter => NumberStyle::LowerLetters,
                    NumFormat::DecimalZero => NumberStyle::ArabicLeadingZero,
                    _ => NumberStyle::Arabic,
                });
                let mut expression = level.text.clone();
                for i in (1..=9).rev() {
                    expression = expression.replace(&format!("%{i}"), &format!("^{i}"));
                }
                out.number_expression = Some(expression);
                if level.start > 1 {
                    out.start_at = Some(Some(level.start));
                }
            }
            out.left_indent = Some(f64::from(level.indent));
            out.first_line_indent = Some(-f64::from(level.hanging));
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
                out.push_str(&p.plain_text());
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
    st.format_paras(para_at..para_at, |fmt| *fmt = ParaFormat { style: style.clone(), para: para_attrs(&p.props, ctx.doc), ..Default::default() });

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
    let (mut para_styles, mut char_styles, names) = style_records(&doc.styles, &doc);
    let default_para_style = doc
        .styles
        .styles
        .iter()
        .find(|style| style.kind == StyleKind::Paragraph && style.default)
        .or_else(|| doc.styles.styles.iter().find(|style| style.kind == StyleKind::Paragraph && style.id == "Normal"))
        .and_then(|style| names.get(&style.id).cloned());
    let mut story = Story::new(StoryId(0));
    let mut ctx = ImportContext {
        doc: &doc,
        names: &names,
        default_para_style,
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
