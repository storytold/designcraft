//! Paragraph, character and object styles with based-on inheritance.

use serde::{Deserialize, Serialize};

use crate::attrs::{CharAttrs, CharProps, ParaAttrs, ParaProps};
use crate::item::{Fill, Stroke, TextFrameOptions};
use crate::story::{BASIC_PARAGRAPH, CharFormat, NO_CHAR_STYLE, ParaFormat};

pub const NO_PARA_STYLE: &str = "[No Paragraph Style]";
pub const BASIC_GRAPHICS_FRAME: &str = "[Basic Graphics Frame]";
pub const BASIC_TEXT_FRAME: &str = "[Basic Text Frame]";
pub const NO_OBJECT_STYLE: &str = "[None]";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParagraphStyle {
    /// Unique name; groups are expressed as `Group/Name`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_style: Option<String>,
    #[serde(default)]
    pub para: ParaAttrs,
    #[serde(default)]
    pub chars: CharAttrs,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shortcut: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterStyle {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default)]
    pub chars: CharAttrs,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shortcut: String,
}

/// Object style: each attribute group is optional ("not included in style" when `None`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObjectStyle {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraph_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_frame: Option<TextFrameOptions>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Styles {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composite_fonts: Vec<crate::cjk::CompositeFont>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mojikumi_tables: Vec<crate::cjk::MojikumiTable>,
    /// Export Tagging per style (`p:Name` or `c:Name`): the HTML tag and class for EPUB/HTML.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub export_tags: std::collections::BTreeMap<String, ExportTag>,
    pub paragraph: Vec<ParagraphStyle>,
    pub character: Vec<CharacterStyle>,
    pub object: Vec<ObjectStyle>,
    /// Cell and table styles (Table › Cell Styles / Table Styles).
    #[serde(default)]
    pub cell: Vec<CellStyle>,
    #[serde(default)]
    pub table: Vec<TableStyle>,
    /// Default styles for new text/frames (the style selected with nothing selected).
    pub default_paragraph: String,
    pub default_character: String,
    pub default_text_frame: String,
    pub default_graphic_frame: String,
}

/// A style's Export Tagging.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportTag {
    /// HTML element (`p`, `h1`…`h6`, `blockquote`, `pre`, `li` for paragraphs; `span`, `em`,
    /// `strong`, `code`, `sup`, `sub` for characters). Empty = automatic.
    pub tag: String,
    /// CSS class (empty = from the style name).
    pub class: String,
}

impl Styles {
    /// Export tagging of a paragraph (`character` false) or character style.
    pub fn export_tag(&self, name: &str, character: bool) -> Option<&ExportTag> {
        self.export_tags.get(&format!("{}:{name}", if character { "c" } else { "p" }))
    }
}

impl Default for Styles {
    fn default() -> Self {
        Styles {
            composite_fonts: Vec::new(),
            mojikumi_tables: Vec::new(),
            export_tags: Default::default(),
            paragraph: vec![
                ParagraphStyle {
                    name: NO_PARA_STYLE.into(),
                    based_on: None,
                    next_style: None,
                    para: ParaAttrs::default(),
                    chars: CharAttrs::default(),
                    shortcut: String::new(),
                },
                ParagraphStyle {
                    name: BASIC_PARAGRAPH.into(),
                    based_on: None,
                    next_style: Some(BASIC_PARAGRAPH.into()),
                    para: ParaAttrs::default(),
                    chars: CharAttrs::default(),
                    shortcut: String::new(),
                },
            ],
            character: vec![CharacterStyle { name: NO_CHAR_STYLE.into(), based_on: None, chars: CharAttrs::default(), shortcut: String::new() }],
            object: vec![
                ObjectStyle { name: NO_OBJECT_STYLE.into(), ..Default::default() },
                ObjectStyle { name: BASIC_GRAPHICS_FRAME.into(), stroke: Some(Stroke::default()), ..Default::default() },
                ObjectStyle {
                    name: BASIC_TEXT_FRAME.into(),
                    fill: Some(Fill::none()),
                    stroke: Some(Stroke::none()),
                    paragraph_style: Some(BASIC_PARAGRAPH.into()),
                    ..Default::default()
                },
            ],
            cell: vec![CellStyle { name: NO_CELL_STYLE.into(), ..Default::default() }],
            table: vec![TableStyle { name: BASIC_TABLE.into(), ..Default::default() }],
            default_paragraph: BASIC_PARAGRAPH.into(),
            default_character: NO_CHAR_STYLE.into(),
            default_text_frame: BASIC_TEXT_FRAME.into(),
            default_graphic_frame: BASIC_GRAPHICS_FRAME.into(),
        }
    }
}

pub const NO_CELL_STYLE: &str = "[None]";
pub const BASIC_TABLE: &str = "[Basic Table]";

/// A cell style: the cell attributes it sets (unset = left as is) and a paragraph style for the
/// cell's text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CellStyle {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_tint: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insets: Option<[f64; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vj: Option<crate::item::VerticalJustification>,
    /// All four edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<crate::table::CellStroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraph_style: Option<String>,
}

/// A table style: cell styles per region, the border, alternating row fills and spacing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TableStyle {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<crate::table::CellStroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_rows: Option<crate::table::AltFills>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f64>,
}

impl CellStyle {
    /// Write this style's attributes into `cell`.
    pub fn apply_to(&self, cell: &mut crate::table::Cell) {
        if let Some(v) = &self.fill {
            cell.fill = v.clone();
        }
        if let Some(v) = self.fill_tint {
            cell.fill_tint = v;
        }
        if let Some(v) = self.insets {
            cell.insets = v;
        }
        if let Some(v) = self.vj {
            cell.vj = v;
        }
        if let Some(v) = &self.stroke {
            cell.strokes = [v.clone(), v.clone(), v.clone(), v.clone()];
        }
        if let Some(ps) = &self.paragraph_style {
            for p in &mut cell.text.paras {
                p.style = ps.clone();
            }
            cell.text.rev += 1;
        }
        cell.style = if self.name == NO_CELL_STYLE { String::new() } else { self.name.clone() };
    }
}

impl TableStyle {
    /// Write this style into `t`: options, then each cell's region style.
    pub fn apply_to(&self, t: &mut crate::table::Table, cells: &[CellStyle]) {
        if let Some(b) = &self.border {
            t.options.border = b.clone();
        }
        if self.alt_rows.is_some() {
            t.options.alt_rows = self.alt_rows.clone();
        }
        if let Some(v) = self.space_before {
            t.options.space_before = v;
        }
        if let Some(v) = self.space_after {
            t.options.space_after = v;
        }
        let (nr, nc) = (t.rows.len(), t.columns.len());
        for r in 0..nr {
            let kind = t.rows[r].kind;
            for c in 0..nc {
                let visual_column = if t.options.direction == crate::TextDirection::RightToLeft { nc - 1 - c } else { c };
                let region = match kind {
                    crate::table::RowKind::Header => self.header.as_ref(),
                    crate::table::RowKind::Footer => self.footer.as_ref(),
                    _ if visual_column == 0 && self.left_column.is_some() => self.left_column.as_ref(),
                    _ if visual_column + 1 == nc && self.right_column.is_some() => self.right_column.as_ref(),
                    _ => self.body.as_ref(),
                };
                if let (Some(name), Some(cell)) = (region, t.cells.get_mut(r * nc + c))
                    && let Some(cs) = cells.iter().find(|s| s.name == *name)
                {
                    cs.apply_to(cell);
                }
            }
        }
        t.style = if self.name == BASIC_TABLE { String::new() } else { self.name.clone() };
    }
}

const MAX_DEPTH: usize = 32;

impl Styles {
    pub fn para(&self, name: &str) -> Option<&ParagraphStyle> {
        self.paragraph.iter().find(|s| s.name == name)
    }
    pub fn para_mut(&mut self, name: &str) -> Option<&mut ParagraphStyle> {
        self.paragraph.iter_mut().find(|s| s.name == name)
    }
    pub fn char_style(&self, name: &str) -> Option<&CharacterStyle> {
        self.character.iter().find(|s| s.name == name)
    }
    pub fn char_style_mut(&mut self, name: &str) -> Option<&mut CharacterStyle> {
        self.character.iter_mut().find(|s| s.name == name)
    }
    pub fn object_style(&self, name: &str) -> Option<&ObjectStyle> {
        self.object.iter().find(|s| s.name == name)
    }

    /// The paragraph style chain from root to `name` (cycle-safe): resolution starts at its first.
    pub fn para_chain(&self, name: &str) -> Vec<&ParagraphStyle> {
        let mut chain = Vec::new();
        let mut cur = self.para(name);
        while let Some(s) = cur {
            if chain.len() >= MAX_DEPTH || chain.iter().any(|c: &&ParagraphStyle| c.name == s.name) {
                break;
            }
            chain.push(s);
            cur = s.based_on.as_deref().and_then(|b| self.para(b));
        }
        chain.reverse();
        chain
    }

    fn char_chain(&self, name: &str) -> Vec<&CharacterStyle> {
        let mut chain = Vec::new();
        let mut cur = self.char_style(name);
        while let Some(s) = cur {
            if chain.len() >= MAX_DEPTH || chain.iter().any(|c: &&CharacterStyle| c.name == s.name) {
                break;
            }
            chain.push(s);
            cur = s.based_on.as_deref().and_then(|b| self.char_style(b));
        }
        chain.reverse();
        chain
    }

    /// Resolved paragraph attributes and the paragraph's base character attributes.
    pub fn resolve_para(&self, p: &ParaFormat) -> (ParaProps, CharProps) {
        let (mut pp, mut cp) = (ParaProps::default(), CharProps::default());
        for s in self.para_chain(&p.style) {
            pp.apply(&s.para);
            cp.apply(&s.chars);
        }
        pp.apply(&p.para);
        cp.apply(&p.chars);
        (pp, cp)
    }

    /// Paragraph-style-only resolution (no local overrides).
    pub fn resolve_para_style(&self, name: &str) -> (ParaProps, CharProps) {
        self.resolve_para(&ParaFormat { style: name.into(), ..Default::default() })
    }

    /// Character attributes for a run: paragraph base ← character style chain ← local overrides.
    pub fn resolve_char(&self, para_chars: &CharProps, f: &CharFormat) -> CharProps {
        let mut cp = para_chars.clone();
        for s in self.char_chain(&f.style) {
            cp.apply(&s.chars);
        }
        cp.apply(&f.over);
        cp
    }

    /// Would making `name` based on `parent` create a cycle?
    pub fn para_based_on_cycles(&self, name: &str, parent: &str) -> bool {
        name == parent || self.para_chain(parent).iter().any(|s| s.name == name)
    }
    pub fn char_based_on_cycles(&self, name: &str, parent: &str) -> bool {
        name == parent || self.char_chain(parent).iter().any(|s| s.name == name)
    }

    /// A free name `base`, `base 2`, `base 3`…
    pub fn unique_name(existing: impl Fn(&str) -> bool, base: &str) -> String {
        if !existing(base) {
            return base.to_string();
        }
        (2..=usize::MAX).map(|i| format!("{base} {i}")).find(|n| !existing(n)).unwrap_or_else(|| format!("{base} copy"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn based_on_chain_resolves_in_order() {
        let mut st = Styles::default();
        st.paragraph.push(ParagraphStyle {
            name: "Body".into(),
            based_on: Some(BASIC_PARAGRAPH.into()),
            next_style: None,
            para: ParaAttrs { space_after: Some(6.0), ..Default::default() },
            chars: CharAttrs { size: Some(10.0), ..Default::default() },
            shortcut: String::new(),
        });
        st.paragraph.push(ParagraphStyle {
            name: "Body First".into(),
            based_on: Some("Body".into()),
            next_style: Some("Body".into()),
            para: ParaAttrs { first_line_indent: Some(0.0), ..Default::default() },
            chars: CharAttrs { size: Some(11.0), ..Default::default() },
            shortcut: String::new(),
        });
        let (pp, cp) = st.resolve_para_style("Body First");
        assert_eq!(pp.space_after, 6.0);
        assert_eq!(cp.size, 11.0);
        let f = CharFormat { style: NO_CHAR_STYLE.into(), over: CharAttrs { tracking: Some(10.0), ..Default::default() } };
        let r = st.resolve_char(&cp, &f);
        assert_eq!((r.size, r.tracking), (11.0, 10.0));
    }

    #[test]
    fn cycles_are_detected_and_safe() {
        let mut st = Styles::default();
        for (n, b) in [("A", "B"), ("B", "A")] {
            st.paragraph.push(ParagraphStyle {
                name: n.into(),
                based_on: Some(b.into()),
                next_style: None,
                para: ParaAttrs::default(),
                chars: CharAttrs::default(),
                shortcut: String::new(),
            });
        }
        let _ = st.resolve_para_style("A"); // terminates
        assert!(st.para_based_on_cycles("A", "B"));
        assert!(!st.para_based_on_cycles(BASIC_PARAGRAPH, NO_PARA_STYLE));
    }

    #[test]
    fn unique_names() {
        let names = ["Body", "Body 2"];
        assert_eq!(Styles::unique_name(|n| names.contains(&n), "Body"), "Body 3");
        assert_eq!(Styles::unique_name(|n| names.contains(&n), "Head"), "Head");
    }
}
