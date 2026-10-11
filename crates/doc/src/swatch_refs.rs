//! Renaming a swatch: every place the document names a swatch follows the new name.

use std::sync::Arc;

use designcraft_color::SwatchValue;

use crate::Document;
use crate::attrs::{CharAttrs, ParaAttrs};
use crate::item::{Fill, Item, Stroke, TextFrameOptions};
use crate::story::Story;
use crate::styles::{AltFillsAttrs, AltStrokesAttrs, CellStrokeAttrs};
use crate::table::{AltFills, AltStrokes, CellStroke, Table};

struct Names<'a> {
    from: &'a str,
    to: &'a str,
}

impl Names<'_> {
    fn s(&self, v: &mut String) {
        if v == self.from {
            *v = self.to.to_string();
        }
    }
    fn o(&self, v: &mut Option<String>) {
        if let Some(v) = v {
            self.s(v);
        }
    }
    fn fill(&self, f: &mut Fill) {
        self.s(&mut f.swatch);
    }
    fn stroke(&self, s: &mut Stroke) {
        self.s(&mut s.swatch);
        self.s(&mut s.gap_swatch);
    }
    fn frame(&self, o: &mut TextFrameOptions) {
        self.s(&mut o.column_rule_color);
    }
    fn chars(&self, c: &mut CharAttrs) {
        for v in [&mut c.fill, &mut c.stroke, &mut c.underline_color, &mut c.strikethrough_color] {
            self.o(v);
        }
    }
    fn para(&self, p: &mut ParaAttrs) {
        for r in [&mut p.rule_above, &mut p.rule_below].into_iter().flatten() {
            self.s(&mut r.color);
        }
        self.o(&mut p.shading_color);
        self.o(&mut p.border_color);
    }
    fn cell_stroke(&self, s: &mut CellStroke) {
        self.s(&mut s.color);
    }
    fn alt_fills(&self, f: &mut AltFills) {
        self.s(&mut f.first_color);
        self.s(&mut f.next_color);
    }
    fn alt_strokes(&self, s: &mut AltStrokes) {
        self.cell_stroke(&mut s.first_stroke);
        self.cell_stroke(&mut s.next_stroke);
    }
    fn stroke_attrs(&self, s: &mut CellStrokeAttrs) {
        self.o(&mut s.color);
    }
    fn alt_fill_attrs(&self, f: &mut AltFillsAttrs) {
        self.o(&mut f.first_color);
        self.o(&mut f.next_color);
    }
    fn alt_stroke_attrs(&self, s: &mut AltStrokesAttrs) {
        self.stroke_attrs(&mut s.first_stroke);
        self.stroke_attrs(&mut s.next_stroke);
    }
    /// An item and everything inside it (group members). Depth-bounded for hostile documents.
    fn item(&self, it: &mut Item, depth: u32) {
        if depth > 64 {
            return;
        }
        self.fill(&mut it.fill);
        self.stroke(&mut it.stroke);
        let e = &mut it.effects;
        for c in [&mut e.drop_shadow.color, &mut e.inner_shadow.color, &mut e.outer_glow.color, &mut e.inner_glow.color, &mut e.satin.color] {
            self.s(c);
        }
        if let Some(tf) = it.text_frame_mut() {
            self.frame(&mut tf.options);
        }
        if let Some(kids) = it.children_mut() {
            for k in kids {
                self.item(Arc::make_mut(k), depth + 1);
            }
        }
    }
    fn table(&self, t: &mut Table) {
        let o = &mut t.options;
        self.cell_stroke(&mut o.border);
        for b in o.borders.iter_mut().flatten() {
            self.cell_stroke(b);
        }
        for f in [&mut o.alt_rows, &mut o.alt_cols].into_iter().flatten() {
            self.alt_fills(f);
        }
        for s in [&mut o.row_strokes, &mut o.column_strokes].into_iter().flatten() {
            self.alt_strokes(s);
        }
        for c in &mut t.cells {
            self.s(&mut c.fill);
            for s in &mut c.strokes {
                self.cell_stroke(s);
            }
        }
    }
    /// One story's own text, objects and tables (cell and note text are visited separately).
    fn story(&self, st: &mut Story) {
        for f in &mut st.paras {
            self.para(&mut f.para);
            self.chars(&mut f.chars);
        }
        for r in &mut st.chars {
            self.chars(&mut r.format.over);
        }
        for o in &mut st.objects {
            self.item(&mut Arc::make_mut(o).item, 0);
        }
        for t in st.tables.values_mut() {
            self.table(Arc::make_mut(t));
        }
        st.rev += 1;
    }
}

impl Document {
    /// Point every reference to swatch `from` at `to`: tints based on it, colour groups, inks,
    /// styles, page items (fills, strokes, gaps, effects, column rules), text colours, rules,
    /// shading and borders, tables and the footnote rule. The swatch itself is renamed by the
    /// caller.
    pub fn rename_swatch_refs(&mut self, from: &str, to: &str) {
        if from == to {
            return;
        }
        let n = Names { from, to };
        for w in &mut self.swatches {
            if let SwatchValue::Tint { base, .. } = &mut w.value {
                n.s(base);
            }
        }
        for g in &mut self.color_groups {
            for w in &mut g.swatches {
                n.s(w);
            }
        }
        for w in &mut self.inks.to_process {
            n.s(w);
        }
        for (a, b) in &mut self.inks.aliases {
            n.s(a);
            n.s(b);
        }
        n.s(&mut self.footnote_options.rule.color);
        let styles = self.styles_mut();
        for p in &mut styles.paragraph {
            n.para(&mut p.para);
            n.chars(&mut p.chars);
        }
        for c in &mut styles.character {
            n.chars(&mut c.chars);
        }
        for o in &mut styles.object {
            if let Some(f) = &mut o.fill {
                n.fill(f);
            }
            if let Some(s) = &mut o.stroke {
                n.stroke(s);
            }
            if let Some(tf) = &mut o.text_frame {
                n.frame(tf);
            }
        }
        for c in &mut styles.cell {
            n.o(&mut c.fill);
            if let Some(s) = &mut c.stroke {
                n.cell_stroke(s);
            }
            for s in &mut c.strokes {
                n.stroke_attrs(s);
            }
        }
        for t in &mut styles.table {
            if let Some(b) = &mut t.border {
                n.cell_stroke(b);
            }
            for b in &mut t.borders {
                n.stroke_attrs(b);
            }
            if let Some(f) = &mut t.alt_rows {
                n.alt_fills(f);
            }
            n.alt_fill_attrs(&mut t.row_fills);
            n.alt_fill_attrs(&mut t.column_fills);
            n.alt_stroke_attrs(&mut t.row_strokes);
            n.alt_stroke_attrs(&mut t.column_strokes);
        }
        for sp in self.spreads.iter_mut().chain(self.parents.iter_mut()) {
            for it in &mut Arc::make_mut(sp).items {
                n.item(Arc::make_mut(it), 0);
            }
        }
        for sid in self.stories.keys().copied().collect::<Vec<_>>() {
            if let Some(story) = self.story_mut(sid) {
                story.for_each_text_mut(&mut |st| n.story(st));
            }
        }
    }
}
