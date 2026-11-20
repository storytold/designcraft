use super::common::{bool_s, p, pct, pkg_root, with_props};
use super::item::text_frame_pref;
use super::story::with_list_tab;
use super::{CHAR_BUILTINS, El, Ex, NO_PARA_ID, OBJECT_BUILTINS, PARA_BUILTINS, Rule, TOC_STYLE_ID, TOC_STYLE_NAME, document, names, num, st};

impl<'a> Ex<'a> {
    // ---------- Styles.xml ----------

    pub(super) fn styles_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let styles = d.styles.clone();
        let mut root = pkg_root("Styles");

        // Character styles.
        let mut cg = craft_idml::model::StyleGroupWriter::new(craft_idml::model::StyleKind::Character, self.fresh());
        for cs in &styles.character {
            let id = names::style_self("CharacterStyle", CHAR_BUILTINS, &cs.name);
            let mut el =
                El::new("CharacterStyle").attr("Self", &id).attr("Imported", "false").attr("Name", names::style_name_out(CHAR_BUILTINS, &cs.name));
            let mut props = Vec::new();
            if cs.name != st::NO_CHAR_STYLE {
                let based = cs.based_on.as_deref().filter(|b| *b != st::NO_CHAR_STYLE);
                match based {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[No character style]")),
                }
                self.char_attrs(&mut el, &mut props, &cs.chars);
            }
            let mut el = with_props(el, props);
            if let Some(t) = styles.export_tag(&cs.name, true) {
                el.push(self.export_tag_map(t));
            }
            cg.insert(&cs.name, crate::xml::to_craft(&el));
        }
        root.push(crate::xml::from_craft(cg.to_el()));

        // Paragraph styles.
        let mut pg = craft_idml::model::StyleGroupWriter::new(craft_idml::model::StyleKind::Paragraph, self.fresh());
        for ps in &styles.paragraph {
            let id = names::style_self("ParagraphStyle", PARA_BUILTINS, &ps.name);
            let mut el =
                El::new("ParagraphStyle").attr("Self", &id).attr("Name", names::style_name_out(PARA_BUILTINS, &ps.name)).attr("Imported", "false");
            let mut props = Vec::new();
            let (pa, ca) = if ps.name == designcraft_doc::NO_PARA_STYLE {
                // The root style carries the complete attribute set.
                let mut pa = designcraft_doc::ParaProps::common([&designcraft_doc::ParaProps::default()]);
                pa.merge(&ps.para);
                let mut ca = designcraft_doc::CharProps::common([&designcraft_doc::CharProps::default()]);
                ca.merge(&ps.chars);
                (pa, ca)
            } else {
                let next = ps.next_style.as_deref().unwrap_or(&ps.name);
                el.set("NextStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, next));
                match ps.based_on.as_deref().filter(|b| *b != designcraft_doc::NO_PARA_STYLE) {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("ParagraphStyle", PARA_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[No paragraph style]")),
                }
                (ps.para.clone(), ps.chars.clone())
            };
            let parent = (ps.name != designcraft_doc::NO_PARA_STYLE).then(|| ps.based_on.as_deref().unwrap_or(designcraft_doc::NO_PARA_STYLE));
            let parent_props = match ps.based_on.as_deref() {
                Some(b) if ps.name != designcraft_doc::NO_PARA_STYLE => styles.resolve_para_style(b).0,
                _ => designcraft_doc::ParaProps::default(),
            };
            let pa = with_list_tab(pa, || (styles.resolve_para_style(&ps.name).0, parent_props));
            self.para_attrs(&mut el, &mut props, &pa, parent);
            self.char_attrs(&mut el, &mut props, &ca);
            let tag = styles.export_tag(&ps.name, false);
            if tag.is_some_and(|t| t.split) {
                el.set("SplitDocument", "true");
            }
            let mut el = with_props(el, props);
            if let Some(t) = tag {
                el.push(self.export_tag_map(t));
            }
            pg.insert(&ps.name, crate::xml::to_craft(&el));
        }
        root.push(crate::xml::from_craft(pg.to_el()));

        self.append_cell_styles(&mut root, &styles);
        self.append_table_styles(&mut root, &styles);
        // Named numbered lists.
        for l in &self.d.settings.lists {
            root.push(crate::xml::from_craft(
                craft_idml::model::NumberingListValues { name: l.name.clone(), continue_across_stories: l.continue_across_stories }.to_el(),
            ));
        }

        // Object styles.
        let mut og = craft_idml::model::StyleGroupWriter::new(craft_idml::model::StyleKind::Object, self.fresh());
        let mut have: Vec<&str> = styles.object.iter().map(|o| o.name.as_str()).collect();
        let defaults = designcraft_doc::Styles::default();
        let mut all: Vec<designcraft_doc::ObjectStyle> = styles.object.clone();
        for b in &defaults.object {
            if !have.contains(&b.name.as_str()) {
                all.push(b.clone());
                have.push(&b.name);
            }
        }
        for os in &all {
            let id = names::style_self("ObjectStyle", OBJECT_BUILTINS, &os.name);
            let mut el = El::new("ObjectStyle").attr("Self", &id).attr("Name", names::style_name_out(OBJECT_BUILTINS, &os.name));
            let mut props = Vec::new();
            if os.name != designcraft_doc::NO_OBJECT_STYLE {
                match os.based_on.as_deref().filter(|b| *b != designcraft_doc::NO_OBJECT_STYLE) {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("ObjectStyle", OBJECT_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[None]")),
                }
            }
            el.set("EnableFill", bool_s(os.fill.is_some()));
            el.set("EnableStroke", bool_s(os.stroke.is_some()));
            el.set("EnableParagraphStyle", bool_s(os.paragraph_style.is_some()));
            el.set("EnableTextFrameGeneralOptions", bool_s(os.text_frame.is_some()));
            el.set("EnableStrokeAndCornerOptions", bool_s(os.corners.is_some()));
            if let Some(c) = &os.corners {
                for (n, c) in ["TopLeft", "TopRight", "BottomRight", "BottomLeft"].iter().zip(c.corners) {
                    el.set(&format!("{n}CornerOption"), names::corner_out(c.shape));
                    el.set(&format!("{n}CornerRadius"), num(c.size));
                }
            }
            if let Some(f) = &os.fill {
                el.set("FillColor", self.sw(&f.swatch));
                el.set("FillTint", pct(f.tint as f64));
            }
            if let Some(s) = &os.stroke {
                self.stroke_attrs(&mut el, s);
            }
            if let Some(ps) = &os.paragraph_style {
                el.set("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, ps));
                el.set("ApplyNextParagraphStyle", bool_s(os.apply_next_style));
            }
            let mut el = with_props(el, props);
            if let Some(e) = &os.effects {
                let mut t = El::new("TransparencySetting");
                for c in self.effect_els(e, true) {
                    t.push(c);
                }
                el.push(t);
            }
            if let Some(tf) = &os.text_frame {
                el.push(text_frame_pref(tf, &self.sw(&tf.column_rule_color)));
            }
            // Effects: a style with effects sets the drop shadow and feather, never blending.
            let has_fx = os.effects.is_some();
            el.push(crate::xml::from_craft(craft_idml::model::object_style_effects_category_settings(has_fx, has_fx)));
            og.insert(&os.name, crate::xml::to_craft(&el));
        }
        root.push(crate::xml::from_craft(og.to_el()));
        if let Some(toc) = d.toc.as_ref().filter(|t| d.story(t.story).is_some()) {
            root.push(self.toc_style_el(toc));
        }
        document(&root)
    }

    /// A style's Export Tagging as its EPUB export tag map.
    fn export_tag_map(&mut self, t: &designcraft_doc::ExportTag) -> El {
        let model = craft_idml::model::EpubExportTagMap { tag: t.tag.clone(), class: t.class.clone(), split_document: t.split };
        crate::xml::from_craft(model.to_el(&self.fresh()))
    }

    fn cell_stroke_values(&self, s: &designcraft_doc::CellStroke) -> craft_idml::model::StrokeValues {
        let mut out = craft_idml::model::StrokeValues {
            weight: Some(s.weight),
            color: Some(self.sw(&s.color)),
            tint: Some(f64::from(s.tint) * 100.0),
            kind: Some(names::stroke_type_out(&s.kind)),
            ..craft_idml::model::StrokeValues::default()
        };
        if s.gap_color != designcraft_color::swatch::NONE || s.gap_tint != 1.0 {
            out.gap_color = Some(self.sw(&s.gap_color));
            out.gap_tint = Some(f64::from(s.gap_tint) * 100.0);
        }
        out
    }

    fn apply_partial_cell_stroke(&self, out: &mut craft_idml::model::StrokeValues, attrs: &designcraft_doc::CellStrokeAttrs) {
        if let Some(value) = attrs.weight {
            out.weight = Some(value);
        }
        if let Some(value) = &attrs.color {
            out.color = Some(self.sw(value));
        }
        if let Some(value) = attrs.tint {
            out.tint = Some(f64::from(value) * 100.0);
        }
        if let Some(value) = &attrs.kind {
            out.kind = Some(names::stroke_type_out(value));
        }
        if let Some(value) = &attrs.gap_color {
            out.gap_color = Some(self.sw(value));
        }
        if let Some(value) = attrs.gap_tint {
            out.gap_tint = Some(f64::from(value) * 100.0);
        }
    }

    fn map_alt_fill_attrs(&self, attrs: &designcraft_doc::AltFillsAttrs) -> craft_idml::model::AlternatingFillValues {
        craft_idml::model::AlternatingFillValues {
            first_count: attrs.first.map(f64::from),
            first_color: attrs.first_color.as_deref().map(|c| self.sw(c)),
            first_tint: attrs.first_tint.map(|t| f64::from(t) * 100.0),
            next_count: attrs.next.map(f64::from),
            next_color: attrs.next_color.as_deref().map(|c| self.sw(c)),
            next_tint: attrs.next_tint.map(|t| f64::from(t) * 100.0),
            skip_first: attrs.skip_first.map(f64::from),
            skip_last: attrs.skip_last.map(f64::from),
        }
    }

    fn map_alt_stroke_attrs(&self, attrs: &designcraft_doc::AltStrokesAttrs) -> craft_idml::model::AlternatingStrokeValues {
        let mut first = craft_idml::model::StrokeValues::default();
        self.apply_partial_cell_stroke(&mut first, &attrs.first_stroke);
        let mut next = craft_idml::model::StrokeValues::default();
        self.apply_partial_cell_stroke(&mut next, &attrs.next_stroke);
        craft_idml::model::AlternatingStrokeValues {
            first_count: attrs.first.map(f64::from),
            next_count: attrs.next.map(f64::from),
            first,
            next,
            skip_first: attrs.skip_first.map(f64::from),
            skip_last: attrs.skip_last.map(f64::from),
        }
    }

    fn append_cell_styles(&mut self, root: &mut El, styles: &designcraft_doc::Styles) {
        let mut cells = El::new("RootCellStyleGroup")
            .attr("Self", self.fresh())
            .child(El::new("CellStyle").attr("Self", "CellStyle/$ID/[None]").attr("AppliedParagraphStyle", NO_PARA_ID).attr("Name", "$ID/[None]"));
        for cs in styles.cell.iter().filter(|c| c.name != designcraft_doc::NO_CELL_STYLE) {
            let mut insets = cs.insets.map_or([None; 4], |i| i.map(Some));
            for (slot, value) in insets.iter_mut().zip(cs.inset_overrides) {
                if let Some(value) = value {
                    *slot = Some(value);
                }
            }
            let mut edge_strokes = [
                craft_idml::model::StrokeValues::default(),
                craft_idml::model::StrokeValues::default(),
                craft_idml::model::StrokeValues::default(),
                craft_idml::model::StrokeValues::default(),
            ];
            if let Some(st) = &cs.stroke {
                for slot in &mut edge_strokes {
                    *slot = self.cell_stroke_values(st);
                }
            }
            for (slot, attrs) in edge_strokes.iter_mut().zip(&cs.strokes) {
                self.apply_partial_cell_stroke(slot, attrs);
            }
            let base = cs.based_on.as_deref().unwrap_or(designcraft_doc::NO_CELL_STYLE);
            let model = craft_idml::model::CellStyleValues {
                self_id: Some(names::style_self("CellStyle", names::CELL_BUILTINS, &cs.name)),
                name: Some(names::style_name_out(names::CELL_BUILTINS, &cs.name)),
                based_on: (base != designcraft_doc::NO_CELL_STYLE).then(|| names::style_self("CellStyle", names::CELL_BUILTINS, base)),
                fill: cs.fill.as_deref().map(|f| self.sw(f)),
                fill_tint: cs.fill_tint.map(|t| f64::from(t) * 100.0),
                insets,
                vertical_justification: cs.vj.map(|vj| names::vj_out(vj).into()),
                first_baseline: cs.first_baseline.map(|fb| names::first_baseline_out(fb).into()),
                first_baseline_min: cs.first_baseline_min,
                paragraph_style: cs.paragraph_style.as_deref().map(|ps| names::style_self("ParagraphStyle", PARA_BUILTINS, ps)),
                edge_strokes,
            };
            cells = cells.child(crate::xml::from_craft(model.to_el()));
        }
        root.push(cells);
    }

    fn append_table_styles(&mut self, root: &mut El, styles: &designcraft_doc::Styles) {
        let mut tables = El::new("RootTableStyleGroup").attr("Self", self.fresh());
        if !styles.table.iter().any(|s| s.name == designcraft_doc::NO_TABLE_STYLE) {
            tables.push(El::new("TableStyle").attr("Self", "TableStyle/$ID/[No table style]").attr("Name", "$ID/[No table style]"));
        }
        if !styles.table.iter().any(|s| s.name == designcraft_doc::BASIC_TABLE) {
            tables.push(with_props(
                El::new("TableStyle").attr("Self", "TableStyle/$ID/[Basic Table]").attr("Name", "$ID/[Basic Table]"),
                vec![p("BasedOn", "string", "$ID/[No table style]")],
            ));
        }
        for ts in &styles.table {
            let cell_ref = |opt: &Option<String>| opt.as_deref().map(|n| names::style_self("CellStyle", names::CELL_BUILTINS, n));
            let mut border_strokes = [
                craft_idml::model::StrokeValues::default(),
                craft_idml::model::StrokeValues::default(),
                craft_idml::model::StrokeValues::default(),
                craft_idml::model::StrokeValues::default(),
            ];
            if let Some(b) = &ts.border {
                for slot in &mut border_strokes {
                    *slot = self.cell_stroke_values(b);
                }
            }
            for (slot, attrs) in border_strokes.iter_mut().zip(&ts.borders) {
                self.apply_partial_cell_stroke(slot, attrs);
            }
            let mut row_fills = craft_idml::model::AlternatingFillValues::default();
            if let Some(a) = &ts.alt_rows {
                row_fills.first_color = Some(self.sw(&a.first_color));
                row_fills.first_count = Some(f64::from(a.first));
                row_fills.first_tint = Some(f64::from(a.first_tint) * 100.0);
                row_fills.next_color = Some(self.sw(&a.next_color));
                row_fills.next_count = Some(f64::from(a.next));
                row_fills.next_tint = Some(f64::from(a.next_tint) * 100.0);
            }
            let overlay = self.map_alt_fill_attrs(&ts.row_fills);
            row_fills.first_count = overlay.first_count.or(row_fills.first_count);
            row_fills.first_color = overlay.first_color.or(row_fills.first_color);
            row_fills.first_tint = overlay.first_tint.or(row_fills.first_tint);
            row_fills.next_count = overlay.next_count.or(row_fills.next_count);
            row_fills.next_color = overlay.next_color.or(row_fills.next_color);
            row_fills.next_tint = overlay.next_tint.or(row_fills.next_tint);
            row_fills.skip_first = overlay.skip_first.or(row_fills.skip_first);
            row_fills.skip_last = overlay.skip_last.or(row_fills.skip_last);
            let based_on = (ts.name != designcraft_doc::NO_TABLE_STYLE).then(|| {
                let base = ts.based_on.as_deref().unwrap_or(designcraft_doc::NO_TABLE_STYLE);
                if base == designcraft_doc::NO_TABLE_STYLE {
                    "$ID/[No table style]".to_string()
                } else {
                    names::style_self("TableStyle", names::TABLE_BUILTINS, base)
                }
            });
            let model = craft_idml::model::TableStyleValues {
                name: Some(names::style_name_out(names::TABLE_BUILTINS, &ts.name)),
                based_on,
                header_cell_style: cell_ref(&ts.header),
                body_cell_style: cell_ref(&ts.body),
                footer_cell_style: cell_ref(&ts.footer),
                left_column_cell_style: cell_ref(&ts.left_column),
                right_column_cell_style: cell_ref(&ts.right_column),
                header_same_as_body: ts.header_same_as_body,
                footer_same_as_body: ts.footer_same_as_body,
                left_column_same_as_body: ts.left_column_same_as_body,
                right_column_same_as_body: ts.right_column_same_as_body,
                space_before: ts.space_before,
                space_after: ts.space_after,
                border_strokes,
                row_fills,
                column_fills: self.map_alt_fill_attrs(&ts.column_fills),
                row_strokes: self.map_alt_stroke_attrs(&ts.row_strokes),
                column_strokes: self.map_alt_stroke_attrs(&ts.column_strokes),
            };
            let self_id = names::style_self("TableStyle", names::TABLE_BUILTINS, &ts.name);
            tables = tables.child(crate::xml::from_craft(model.to_el(&self_id)));
        }
        root.push(tables);
    }

    /// The document's table of contents as a TOC style; its story names it in `AppliedTOCStyle`.
    fn toc_style_el(&mut self, toc: &designcraft_doc::Toc) -> El {
        let para = |n: &str| names::style_self("ParagraphStyle", PARA_BUILTINS, n);
        let chars = |n: Option<&str>| names::style_self("CharacterStyle", CHAR_BUILTINS, n.unwrap_or(designcraft_doc::NO_CHAR_STYLE));
        let entries = toc
            .entries
            .iter()
            .map(|e| {
                let format = e.entry_style.clone().unwrap_or_else(|| format!("TOC Level {}", e.level));
                let position = match e.page_number {
                    _ if !toc.page_numbers => "NoPageNumber",
                    designcraft_doc::TocPageNumber::After => "AfterEntry",
                    designcraft_doc::TocPageNumber::Before => "BeforeEntry",
                    designcraft_doc::TocPageNumber::None => "NoPageNumber",
                };
                craft_idml::model::TocStyleEntryValues {
                    style: Some(names::style_name_out(PARA_BUILTINS, &e.style)),
                    level: Some(e.level),
                    page_number_position: Some(position.into()),
                    separator: Some(names::toc_separator_out(e.separator.as_deref().unwrap_or("\t"))),
                    entry_style: Some(para(&format)),
                    page_number_style: Some(chars(e.page_number_style.as_deref())),
                    separator_style: Some(chars(e.separator_style.as_deref())),
                    sort: Some(e.sort),
                }
            })
            .collect();
        let model = craft_idml::model::TocStyleValues {
            self_id: Some(TOC_STYLE_ID.into()),
            title: Some(toc.title.clone()),
            title_style: Some(para(toc.title_style.as_deref().unwrap_or("TOC Title"))),
            create_bookmarks: Some(toc.create_bookmarks),
            entries,
        };
        crate::xml::from_craft(model.to_el(TOC_STYLE_NAME, || self.fresh()))
    }

    pub(super) fn rule(&mut self, el: &mut El, props: &mut Vec<El>, k: &str, r: &Rule) {
        el.set(k, bool_s(r.on));
        el.set(&format!("{k}LineWeight"), num(r.weight));
        el.set(&format!("{k}Tint"), pct(r.tint as f64));
        el.set(&format!("{k}Width"), if r.column_width { "ColumnWidth" } else { "TextWidth" });
        el.set(&format!("{k}Offset"), num(r.offset));
        el.set(&format!("{k}LeftIndent"), num(r.left_indent));
        el.set(&format!("{k}RightIndent"), num(r.right_indent));
        el.set(&format!("{k}Type"), names::stroke_type_out(&r.kind));
        // A gap in Text Color is a string property, as the rule's own colour is.
        if r.gap_color == designcraft_doc::TEXT_COLOR {
            props.push(p(&format!("{k}GapColor"), "string", designcraft_doc::TEXT_COLOR));
        } else {
            el.set(&format!("{k}GapColor"), self.sw(&r.gap_color));
        }
        el.set(&format!("{k}GapTint"), pct(r.gap_tint as f64));
        if k == "RuleAbove" {
            el.set("KeepRuleAboveInFrame", bool_s(r.keep_in_frame));
        }
        props.push(if r.color == designcraft_doc::TEXT_COLOR {
            p(&format!("{k}Color"), "string", designcraft_doc::TEXT_COLOR)
        } else {
            p(&format!("{k}Color"), "object", self.sw(&r.color))
        });
    }

    pub(super) fn stroke_attrs(&self, el: &mut El, s: &designcraft_doc::Stroke) {
        el.set("StrokeColor", self.sw(&s.swatch));
        el.set("StrokeWeight", num(if s.is_none() { 0.0 } else { s.weight }));
        el.set("StrokeTint", pct(s.tint as f64));
        el.set("StrokeType", names::stroke_type_out(&s.kind));
        el.set("StrokeAlignment", names::stroke_align_out(s.align));
        el.set("EndCap", names::cap_out(s.cap));
        el.set("EndJoin", names::join_out(s.join));
        el.set("MiterLimit", num(s.miter_limit));
        el.set("LeftLineEnd", names::arrow_out(s.start));
        el.set("RightLineEnd", names::arrow_out(s.end));
        el.set("GapColor", self.sw(&s.gap_swatch));
        el.set("GapTint", pct(s.gap_tint as f64));
        el.set("OverprintStroke", bool_s(s.overprint));
        el.set("OverprintGap", bool_s(s.gap_overprint));
    }
}
