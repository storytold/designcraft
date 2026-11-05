use super::common::{bool_s, pct, uid};
use super::{Affine, CharFormat, Content, El, Ex, Item, ParaFormat, Shape, names, num};

impl<'a> Ex<'a> {
    pub(super) fn cell_stroke_attrs(&self, el: &mut El, prefix: &str, s: &designcraft_doc::CellStroke) {
        el.set(&format!("{prefix}StrokeWeight"), num(s.weight));
        el.set(&format!("{prefix}StrokeColor"), self.sw(&s.color));
        el.set(&format!("{prefix}StrokeTint"), pct(s.tint as f64));
        el.set(&format!("{prefix}StrokeType"), names::stroke_type_out(&s.kind));
        if s.gap_color != designcraft_color::swatch::NONE || s.gap_tint != 1.0 {
            el.set(&format!("{prefix}StrokeGapColor"), self.sw(&s.gap_color));
            el.set(&format!("{prefix}StrokeGapTint"), pct(s.gap_tint as f64));
        }
    }

    pub(super) fn partial_cell_stroke_attrs(&self, el: &mut El, prefix: &str, attrs: &designcraft_doc::CellStrokeAttrs) {
        if let Some(value) = attrs.weight {
            el.set(&format!("{prefix}StrokeWeight"), num(value));
        }
        if let Some(value) = &attrs.color {
            el.set(&format!("{prefix}StrokeColor"), self.sw(value));
        }
        if let Some(value) = attrs.tint {
            el.set(&format!("{prefix}StrokeTint"), pct(value as f64));
        }
        if let Some(value) = &attrs.kind {
            el.set(&format!("{prefix}StrokeType"), names::stroke_type_out(value));
        }
        if let Some(value) = &attrs.gap_color {
            el.set(&format!("{prefix}StrokeGapColor"), self.sw(value));
        }
        if let Some(value) = attrs.gap_tint {
            el.set(&format!("{prefix}StrokeGapTint"), pct(value as f64));
        }
    }

    pub(super) fn partial_alt_fill_attrs(&self, el: &mut El, kind: &str, attrs: &designcraft_doc::AltFillsAttrs) {
        if let Some(value) = attrs.first {
            el.set(&format!("Start{kind}FillCount"), value);
        }
        if let Some(value) = &attrs.first_color {
            el.set(&format!("Start{kind}FillColor"), self.sw(value));
        }
        if let Some(value) = attrs.first_tint {
            el.set(&format!("Start{kind}FillTint"), pct(value as f64));
        }
        if let Some(value) = attrs.next {
            el.set(&format!("End{kind}FillCount"), value);
        }
        if let Some(value) = &attrs.next_color {
            el.set(&format!("End{kind}FillColor"), self.sw(value));
        }
        if let Some(value) = attrs.next_tint {
            el.set(&format!("End{kind}FillTint"), pct(value as f64));
        }
        if let Some(value) = attrs.skip_first {
            el.set(&format!("SkipFirstAlternatingFill{kind}s"), value);
        }
        if let Some(value) = attrs.skip_last {
            el.set(&format!("SkipLastAlternatingFill{kind}s"), value);
        }
    }

    pub(super) fn partial_alt_stroke_attrs(&self, el: &mut El, kind: &str, attrs: &designcraft_doc::AltStrokesAttrs) {
        if let Some(value) = attrs.first {
            el.set(&format!("Start{kind}StrokeCount"), value);
        }
        if let Some(value) = attrs.next {
            el.set(&format!("End{kind}StrokeCount"), value);
        }
        self.partial_cell_stroke_attrs(el, &format!("Start{kind}"), &attrs.first_stroke);
        self.partial_cell_stroke_attrs(el, &format!("End{kind}"), &attrs.next_stroke);
        if kind == "Column"
            && let Some(value) = &attrs.next_stroke.kind
        {
            el.attrs.retain(|(key, _)| key != "EndColumnStrokeType");
            el.set("EndColumnLineStyle", names::stroke_type_out(value));
        }
        if let Some(value) = attrs.skip_first {
            el.set(&format!("SkipFirstAlternatingStroke{kind}s"), value);
        }
        if let Some(value) = attrs.skip_last {
            el.set(&format!("SkipLastAlternatingStroke{kind}s"), value);
        }
    }

    /// `<Table>` with rows, columns and cells (cell names are `column:row`).
    pub(super) fn table_el(&mut self, t: &designcraft_doc::Table) -> El {
        let id = uid(t.id);
        let (h, f) = (t.header_rows(), t.footer_rows());
        let mut el = El::new("Table")
            .attr("Self", &id)
            .attr("HeaderRowCount", h)
            .attr("FooterRowCount", f)
            .attr("BodyRowCount", t.nrows() - h - f)
            .attr("ColumnCount", t.ncols())
            .attr(
                "AppliedTableStyle",
                names::style_self("TableStyle", names::TABLE_BUILTINS, if t.style.is_empty() { designcraft_doc::BASIC_TABLE } else { &t.style }),
            )
            .attr(
                "TableDirection",
                if t.options.direction == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
            )
            .attr("SpaceBefore", num(t.options.space_before))
            .attr("SpaceAfter", num(t.options.space_after))
            .attr("HeaderBehavior", if t.options.repeat_header { "RepeatOnEachTextColumn" } else { "RepeatOnce" })
            .attr("FooterBehavior", if t.options.repeat_footer { "RepeatOnEachTextColumn" } else { "RepeatOnce" });
        for (i, side) in ["Top", "Left", "Bottom", "Right"].iter().enumerate() {
            self.cell_stroke_attrs(&mut el, &format!("{side}Border"), t.options.border_for(i));
        }
        for (kind, alt) in [("Row", &t.options.alt_rows), ("Column", &t.options.alt_cols)] {
            if let Some(a) = alt {
                el.set(&format!("Start{kind}FillColor"), self.sw(&a.first_color));
                el.set(&format!("Start{kind}FillCount"), a.first);
                el.set(&format!("Start{kind}FillTint"), pct(a.first_tint as f64));
                el.set(&format!("End{kind}FillColor"), self.sw(&a.next_color));
                el.set(&format!("End{kind}FillCount"), a.next);
                el.set(&format!("End{kind}FillTint"), pct(a.next_tint as f64));
                el.set(&format!("SkipFirstAlternatingFill{kind}s"), a.skip_first);
                el.set(&format!("SkipLastAlternatingFill{kind}s"), a.skip_last);
            } else {
                // A local off value must override any enabled pattern in the named style.
                el.set(&format!("Start{kind}FillCount"), 0);
                el.set(&format!("End{kind}FillCount"), 0);
            }
        }
        for (kind, strokes) in [("Row", &t.options.row_strokes), ("Column", &t.options.column_strokes)] {
            if let Some(strokes) = strokes {
                el.set(&format!("Start{kind}StrokeCount"), strokes.first);
                el.set(&format!("End{kind}StrokeCount"), strokes.next);
                self.cell_stroke_attrs(&mut el, &format!("Start{kind}"), &strokes.first_stroke);
                self.cell_stroke_attrs(&mut el, &format!("End{kind}"), &strokes.next_stroke);
                if kind == "Column" {
                    el.attrs.retain(|(key, _)| key != "EndColumnStrokeType");
                    el.set("EndColumnLineStyle", names::stroke_type_out(&strokes.next_stroke.kind));
                }
                el.set(&format!("SkipFirstAlternatingStroke{kind}s"), strokes.skip_first);
                el.set(&format!("SkipLastAlternatingStroke{kind}s"), strokes.skip_last);
            } else {
                el.set(&format!("Start{kind}StrokeCount"), 0);
                el.set(&format!("End{kind}StrokeCount"), 0);
            }
        }
        for (r, row) in t.rows.iter().enumerate() {
            let exact = row.mode == designcraft_doc::RowHeightMode::Exactly;
            el.push(
                El::new("Row")
                    .attr("Self", format!("{id}Row{r}"))
                    .attr("Name", r)
                    .attr("SingleRowHeight", num(row.height.max(3.0)))
                    .attr("MinimumHeight", num(row.height.max(0.0)))
                    .attr("AutoGrow", bool_s(!exact)),
            );
        }
        for (c, col) in t.columns.iter().enumerate() {
            el.push(El::new("Column").attr("Self", format!("{id}Column{c}")).attr("Name", c).attr("SingleColumnWidth", num(col.width)));
        }
        let owners = t.owners();
        for r in 0..t.nrows() {
            for c in 0..t.ncols() {
                if owners[r * t.ncols() + c] != (r, c) {
                    continue;
                }
                let Some(cell) = t.cell(r, c) else { continue };
                let vj = match cell.vj {
                    designcraft_doc::VerticalJustification::Top => "TopAlign",
                    designcraft_doc::VerticalJustification::Center => "CenterAlign",
                    designcraft_doc::VerticalJustification::Bottom => "BottomAlign",
                    designcraft_doc::VerticalJustification::Justify => "JustifyAlign",
                };
                let mut ce = El::new("Cell")
                    .attr("Self", format!("{id}i{}", r * t.ncols() + c))
                    .attr("Name", format!("{c}:{r}"))
                    .attr("RowSpan", cell.row_span)
                    .attr("ColumnSpan", cell.col_span)
                    .attr(
                        "AppliedCellStyle",
                        names::style_self(
                            "CellStyle",
                            names::CELL_BUILTINS,
                            if cell.style.is_empty() { designcraft_doc::NO_CELL_STYLE } else { &cell.style },
                        ),
                    )
                    .attr("FillColor", self.sw(&cell.fill))
                    .attr("FillTint", pct(cell.fill_tint as f64))
                    .attr("TopInset", num(cell.insets[0]))
                    .attr("LeftInset", num(cell.insets[1]))
                    .attr("BottomInset", num(cell.insets[2]))
                    .attr("RightInset", num(cell.insets[3]))
                    .attr("VerticalJustification", vj)
                    .attr("FirstBaselineOffset", names::first_baseline_out(cell.first_baseline))
                    .attr("MinimumFirstBaselineOffset", num(cell.first_baseline_min))
                    .attr("RotationAngle", num(cell.rotation));
                for (i, side) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().enumerate() {
                    // Do not turn absent native defaults into explicit shared-edge candidates.
                    if cell.stroke_defined[i]
                        || cell.border_overrides[i]
                        || cell.stroke_priorities[i] != 0
                        || cell.strokes[i] != designcraft_doc::CellStroke::default()
                    {
                        self.cell_stroke_attrs(&mut ce, side, &cell.strokes[i]);
                    }
                    let priority = if cell.border_overrides[i] && !cell.stroke_defined[i] {
                        cell.stroke_priorities[i].max(1)
                    } else {
                        cell.stroke_priorities[i]
                    };
                    if priority != 0 {
                        ce.set(&format!("{side}StrokePriority"), priority);
                    }
                }
                let mut paras = self.story_paras(&cell.text);
                // A graphic cell holds its image as an inline object in the cell's first paragraph.
                if let Some(g) = &cell.graphic
                    && let Some(object) = self.cell_graphic_el(g)
                {
                    // The cell clips its graphic on screen and in PDF; IDML does so only when asked.
                    ce.set("ClipContentToCell", bool_s(true));
                    let mut range = self.csr_el(&CharFormat::default());
                    range.push(object);
                    match paras.first_mut() {
                        Some(first) => first.push(range),
                        None => {
                            let mut psr = self.psr_el(&ParaFormat::default());
                            psr.push(range);
                            paras.push(psr);
                        }
                    }
                }
                for psr in paras {
                    ce.push(psr);
                }
                el.push(ce);
            }
        }
        el
    }

    /// The frame of a table cell's graphic: an inline `Rectangle` around the shown image, or `None`
    /// when the graphic's asset is missing.
    fn cell_graphic_el(&mut self, g: &designcraft_doc::Graphic) -> Option<El> {
        self.d.assets.get(&g.asset)?;
        let shown = g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
        let layer = self.d.layers.first().map(|l| l.id).unwrap_or(designcraft_doc::ids::LayerId(0));
        let mut item = Item::new(designcraft_doc::ids::ItemId(0), layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(shown));
        item.content = Content::Graphic(g.clone());
        let mut el = self.item_el(&item, Affine::IDENTITY);
        el.set("Self", self.fresh());
        el.push(El::new("AnchoredObjectSetting").attr("AnchoredPosition", "InlinePosition").attr("AnchorYoffset", "0"));
        Some(el)
    }
}
