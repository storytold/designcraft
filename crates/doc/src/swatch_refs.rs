//! Renaming a swatch: every place the document names a swatch follows the new name. The same
//! walk lists the swatches that page items, stories and styles name.

use std::collections::BTreeSet;
use std::sync::Arc;

use designcraft_color::SwatchValue;

use crate::Document;
use crate::attrs::{CharAttrs, ParaAttrs};
use crate::item::{Fill, Item, Stroke, TextFrameOptions};
use crate::story::Story;
use crate::styles::{AltFillsAttrs, AltStrokesAttrs, CellStrokeAttrs, Styles};
use crate::table::{AltFills, AltStrokes, CellStroke, Table};

/// Every swatch name a walk meets goes through `visit`.
struct Names<'a> {
    visit: &'a mut dyn FnMut(&mut String),
}

impl Names<'_> {
    fn s(&mut self, v: &mut String) {
        (self.visit)(v);
    }
    fn o(&mut self, v: &mut Option<String>) {
        if let Some(v) = v {
            self.s(v);
        }
    }
    fn fill(&mut self, f: &mut Fill) {
        self.s(&mut f.swatch);
    }
    fn stroke(&mut self, s: &mut Stroke) {
        self.s(&mut s.swatch);
        self.s(&mut s.gap_swatch);
    }
    fn frame(&mut self, o: &mut TextFrameOptions) {
        self.s(&mut o.column_rule_color);
    }
    fn chars(&mut self, c: &mut CharAttrs) {
        for v in [
            &mut c.fill,
            &mut c.stroke,
            &mut c.underline_color,
            &mut c.strikethrough_color,
            &mut c.kenten_fill,
            &mut c.kenten_stroke,
            &mut c.ruby_fill,
            &mut c.ruby_stroke,
        ] {
            self.o(v);
        }
    }
    fn para(&mut self, p: &mut ParaAttrs) {
        for r in [&mut p.rule_above, &mut p.rule_below].into_iter().flatten() {
            self.s(&mut r.color);
        }
        self.o(&mut p.shading_color);
        self.o(&mut p.border_color);
        self.o(&mut p.border_gap_color);
    }
    fn cell_stroke(&mut self, s: &mut CellStroke) {
        self.s(&mut s.color);
        self.s(&mut s.gap_color);
    }
    fn alt_fills(&mut self, f: &mut AltFills) {
        self.s(&mut f.first_color);
        self.s(&mut f.next_color);
    }
    fn alt_strokes(&mut self, s: &mut AltStrokes) {
        self.cell_stroke(&mut s.first_stroke);
        self.cell_stroke(&mut s.next_stroke);
    }
    fn stroke_attrs(&mut self, s: &mut CellStrokeAttrs) {
        self.o(&mut s.color);
        self.o(&mut s.gap_color);
    }
    fn alt_fill_attrs(&mut self, f: &mut AltFillsAttrs) {
        self.o(&mut f.first_color);
        self.o(&mut f.next_color);
    }
    fn alt_stroke_attrs(&mut self, s: &mut AltStrokesAttrs) {
        self.stroke_attrs(&mut s.first_stroke);
        self.stroke_attrs(&mut s.next_stroke);
    }
    /// An item and everything inside it (group members). Depth-bounded for hostile documents.
    fn item(&mut self, it: &mut Item, depth: u32) {
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
    fn table(&mut self, t: &mut Table) {
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
    /// Every style's swatches.
    fn styles(&mut self, styles: &mut Styles) {
        for p in &mut styles.paragraph {
            self.para(&mut p.para);
            self.chars(&mut p.chars);
        }
        for c in &mut styles.character {
            self.chars(&mut c.chars);
        }
        for o in &mut styles.object {
            if let Some(f) = &mut o.fill {
                self.fill(f);
            }
            if let Some(s) = &mut o.stroke {
                self.stroke(s);
            }
            if let Some(tf) = &mut o.text_frame {
                self.frame(tf);
            }
        }
        for c in &mut styles.cell {
            self.o(&mut c.fill);
            if let Some(s) = &mut c.stroke {
                self.cell_stroke(s);
            }
            for s in &mut c.strokes {
                self.stroke_attrs(s);
            }
        }
        for t in &mut styles.table {
            if let Some(b) = &mut t.border {
                self.cell_stroke(b);
            }
            for b in &mut t.borders {
                self.stroke_attrs(b);
            }
            if let Some(f) = &mut t.alt_rows {
                self.alt_fills(f);
            }
            self.alt_fill_attrs(&mut t.row_fills);
            self.alt_fill_attrs(&mut t.column_fills);
            self.alt_stroke_attrs(&mut t.row_strokes);
            self.alt_stroke_attrs(&mut t.column_strokes);
        }
    }
    /// One story's own text, objects and tables (cell and note text are visited separately).
    fn story(&mut self, st: &mut Story) {
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
        st.rev = st.rev.wrapping_add(1);
    }
}

impl Document {
    /// Give gradient stops that name a color swatch that swatch's current color, after a
    /// swatch edit. Stops whose swatch is gone or isn't a color keep the color they have.
    pub fn refresh_gradient_stops(&mut self) {
        let mut fresh: Vec<(usize, usize, designcraft_color::Color)> = Vec::new();
        for (wi, w) in self.swatches.iter().enumerate() {
            let SwatchValue::Gradient { gradient } = &w.value else { continue };
            for (si, stop) in gradient.stops.iter().enumerate() {
                let Some(name) = &stop.swatch else { continue };
                if designcraft_color::swatch::resolve_gradient(&self.swatches, name).is_some() {
                    continue;
                }
                if let Some(c) = self.resolve_color(name, 1.0)
                    && c != stop.color
                {
                    fresh.push((wi, si, c));
                }
            }
        }
        for (wi, si, c) in fresh {
            if let Some(SwatchValue::Gradient { gradient }) = self.swatches.get_mut(wi).map(|w| &mut w.value)
                && let Some(stop) = gradient.stops.get_mut(si)
            {
                stop.color = c;
            }
        }
    }

    /// Turn gradient stops that name swatch `name` into plain colors (the swatch is being
    /// deleted); each keeps the color it shows.
    pub fn forget_gradient_stop_swatch(&mut self, name: &str) {
        for w in &mut self.swatches {
            let SwatchValue::Gradient { gradient } = &mut w.value else { continue };
            for stop in &mut gradient.stops {
                if stop.swatch.as_deref() == Some(name) {
                    stop.swatch = None;
                }
            }
        }
    }

    /// Point every reference to swatch `from` at `to`: tints based on it, colour groups, inks,
    /// styles, page items (fills, strokes, gaps, effects, column rules), text, kenten and ruby
    /// colours, rules, shading, borders and border gaps, tables (with stroke gaps), footnotes,
    /// gradient stops,
    /// endnotes and the footnote rule. The swatch itself is renamed by the
    /// caller.
    pub fn rename_swatch_refs(&mut self, from: &str, to: &str) {
        if from == to {
            return;
        }
        let mut rename = |v: &mut String| {
            if v == from {
                *v = to.to_string();
            }
        };
        let mut n = Names { visit: &mut rename };
        for w in &mut self.swatches {
            match &mut w.value {
                SwatchValue::Tint { base, .. } => n.s(base),
                SwatchValue::Gradient { gradient } => {
                    for stop in &mut gradient.stops {
                        n.o(&mut stop.swatch);
                    }
                }
                _ => {}
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
        n.styles(self.styles_mut());
        for sp in self.spreads.iter_mut().chain(self.parents.iter_mut()) {
            for it in &mut Arc::make_mut(sp).items {
                n.item(Arc::make_mut(it), 0);
            }
        }
        for sid in self.stories.keys().copied().collect::<Vec<_>>() {
            if let Some(story) = self.story_mut(sid) {
                story.for_each_text_mut(&mut |st| n.story(st));
                for e in &mut story.endnotes {
                    Arc::make_mut(e).text.for_each_text_mut(&mut |st| n.story(st));
                }
            }
        }
    }
}

/// The swatches that page items (and their groups' members), stories (with their tables, notes
/// and anchored objects) and styles name; a tint's base isn't included.
pub fn swatches_named_by<'a>(
    items: impl IntoIterator<Item = &'a Item>,
    stories: impl IntoIterator<Item = &'a Story>,
    styles: &Styles,
) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    let mut note = |v: &mut String| {
        if !v.is_empty() {
            used.insert(v.clone());
        }
    };
    let mut n = Names { visit: &mut note };
    for it in items {
        n.item(&mut it.clone(), 0);
    }
    for st in stories {
        let mut st = st.clone();
        st.for_each_text_mut(&mut |t| n.story(t));
        for e in &mut st.endnotes {
            Arc::make_mut(e).text.for_each_text_mut(&mut |t| n.story(t));
        }
    }
    n.styles(&mut styles.clone());
    used
}
