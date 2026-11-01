use super::common::{bool_s, p, pct, pkg_root, with_props};
use super::item::{Group, text_frame_pref};
use super::story::with_list_tab;
use super::{
    CHAR_BUILTINS, El, Ex, NO_PARA_ID, OBJECT_BUILTINS, PARA_BUILTINS, Rule, TOC_STYLE_ID, TOC_STYLE_NAME, document, escape_id, names, num, st,
};

impl<'a> Ex<'a> {
    // ---------- Styles.xml ----------

    pub(super) fn styles_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let styles = d.styles.clone();
        let mut root = pkg_root("Styles");

        // Character styles.
        let mut cg = Group::default();
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
            cg.insert(&cs.name, with_props(el, props));
        }
        let mut rc = El::new("RootCharacterStyleGroup").attr("Self", self.fresh());
        cg.emit(&mut rc, "CharacterStyleGroup", "");
        root.push(rc);

        // Paragraph styles.
        let mut pg = Group::default();
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
            if styles.export_tag(&ps.name, false).is_some_and(|t| t.split) {
                el.set("SplitDocument", "true");
            }
            pg.insert(&ps.name, with_props(el, props));
        }
        let mut rp = El::new("RootParagraphStyleGroup").attr("Self", self.fresh());
        pg.emit(&mut rp, "ParagraphStyleGroup", "");
        root.push(rp);

        self.append_cell_styles(&mut root, &styles);
        self.append_table_styles(&mut root, &styles);
        // Named numbered lists.
        for l in &self.d.settings.lists {
            root.push(
                El::new("NumberingList")
                    .attr("Self", format!("NumberingList/{}", escape_id(&l.name)))
                    .attr("Name", &l.name)
                    .attr("ContinueNumbersAcrossStories", bool_s(l.continue_across_stories))
                    .attr("ContinueNumbersAcrossDocuments", "false"),
            );
        }

        // Object styles.
        let mut og = Group::default();
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
            if let Some(f) = &os.fill {
                el.set("FillColor", self.sw(&f.swatch));
                el.set("FillTint", pct(f.tint as f64));
            }
            if let Some(s) = &os.stroke {
                self.stroke_attrs(&mut el, s);
            }
            if let Some(ps) = &os.paragraph_style {
                el.set("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, ps));
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
            let on = bool_s(os.effects.is_some());
            el.push(
                El::new("ObjectStyleObjectEffectsCategorySettings")
                    .attr("EnableTransparency", "false")
                    .attr("EnableDropShadow", on)
                    .attr("EnableFeather", on)
                    .attr("EnableInnerShadow", "false")
                    .attr("EnableOuterGlow", "false")
                    .attr("EnableInnerGlow", "false")
                    .attr("EnableBevelEmboss", "false")
                    .attr("EnableSatin", "false")
                    .attr("EnableDirectionalFeather", "false")
                    .attr("EnableGradientFeather", "false"),
            );
            og.insert(&os.name, el);
        }
        let mut ro = El::new("RootObjectStyleGroup").attr("Self", self.fresh());
        og.emit(&mut ro, "ObjectStyleGroup", "");
        root.push(ro);
        if let Some(toc) = d.toc.as_ref().filter(|t| d.story(t.story).is_some()) {
            root.push(self.toc_style_el(toc));
        }
        document(&root)
    }

    fn append_cell_styles(&mut self, root: &mut El, styles: &designcraft_doc::Styles) {
        let mut cells = El::new("RootCellStyleGroup")
            .attr("Self", self.fresh())
            .child(El::new("CellStyle").attr("Self", "CellStyle/$ID/[None]").attr("AppliedParagraphStyle", NO_PARA_ID).attr("Name", "$ID/[None]"));
        for cs in styles.cell.iter().filter(|c| c.name != designcraft_doc::NO_CELL_STYLE) {
            let mut el = El::new("CellStyle")
                .attr("Self", names::style_self("CellStyle", names::CELL_BUILTINS, &cs.name))
                .attr("Name", names::style_name_out(names::CELL_BUILTINS, &cs.name));
            if let Some(f) = &cs.fill {
                el.set("FillColor", self.sw(f));
            }
            if let Some(t) = cs.fill_tint {
                el.set("FillTint", pct(t as f64));
            }
            if let Some(i) = cs.insets {
                for (k, v) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().zip(i) {
                    el.set(k, num(v));
                }
            }
            for (key, value) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().zip(cs.inset_overrides) {
                if let Some(value) = value {
                    el.set(key, num(value));
                }
            }
            if let Some(vj) = cs.vj {
                el.set("VerticalJustification", names::vj_out(vj));
            }
            if let Some(fb) = cs.first_baseline {
                el.set("FirstBaselineOffset", names::first_baseline_out(fb));
            }
            if let Some(min) = cs.first_baseline_min {
                el.set("MinimumFirstBaselineOffset", num(min));
            }
            if let Some(st) = &cs.stroke {
                for side in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"] {
                    self.cell_stroke_attrs(&mut el, side, st);
                }
            }
            for (side, attrs) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().zip(&cs.strokes) {
                self.partial_cell_stroke_attrs(&mut el, side, attrs);
            }
            if let Some(ps) = &cs.paragraph_style {
                el.set("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, ps));
            }
            let base = cs.based_on.as_deref().unwrap_or(designcraft_doc::NO_CELL_STYLE);
            let based_on = if base == designcraft_doc::NO_CELL_STYLE {
                p("BasedOn", "string", "$ID/[None]")
            } else {
                p("BasedOn", "object", names::style_self("CellStyle", names::CELL_BUILTINS, base))
            };
            cells = cells.child(with_props(el, vec![based_on]));
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
            let mut el = El::new("TableStyle")
                .attr("Self", names::style_self("TableStyle", names::TABLE_BUILTINS, &ts.name))
                .attr("Name", names::style_name_out(names::TABLE_BUILTINS, &ts.name));
            for (k, v) in [
                ("HeaderRegionCellStyle", &ts.header),
                ("BodyRegionCellStyle", &ts.body),
                ("FooterRegionCellStyle", &ts.footer),
                ("LeftColumnRegionCellStyle", &ts.left_column),
                ("RightColumnRegionCellStyle", &ts.right_column),
            ] {
                if let Some(n) = v {
                    el.set(k, names::style_self("CellStyle", names::CELL_BUILTINS, n));
                }
            }
            for (key, value) in [
                ("HeaderRegionSameAsBodyRegion", ts.header_same_as_body),
                ("FooterRegionSameAsBodyRegion", ts.footer_same_as_body),
                ("LeftColumnRegionSameAsBodyRegion", ts.left_column_same_as_body),
                ("RightColumnRegionSameAsBodyRegion", ts.right_column_same_as_body),
            ] {
                if let Some(value) = value {
                    el.set(key, value);
                }
            }
            if let Some(b) = &ts.border {
                for side in ["Top", "Left", "Bottom", "Right"] {
                    self.cell_stroke_attrs(&mut el, &format!("{side}Border"), b);
                }
            }
            for (side, attrs) in ["TopBorder", "LeftBorder", "BottomBorder", "RightBorder"].iter().zip(&ts.borders) {
                self.partial_cell_stroke_attrs(&mut el, side, attrs);
            }
            if let Some(a) = &ts.alt_rows {
                el.set("StartRowFillColor", self.sw(&a.first_color));
                el.set("StartRowFillCount", a.first);
                el.set("StartRowFillTint", pct(a.first_tint as f64));
                el.set("EndRowFillColor", self.sw(&a.next_color));
                el.set("EndRowFillCount", a.next);
                el.set("EndRowFillTint", pct(a.next_tint as f64));
            }
            self.partial_alt_fill_attrs(&mut el, "Row", &ts.row_fills);
            self.partial_alt_fill_attrs(&mut el, "Column", &ts.column_fills);
            self.partial_alt_stroke_attrs(&mut el, "Row", &ts.row_strokes);
            self.partial_alt_stroke_attrs(&mut el, "Column", &ts.column_strokes);
            if let Some(v) = ts.space_before {
                el.set("SpaceBefore", num(v));
            }
            if let Some(v) = ts.space_after {
                el.set("SpaceAfter", num(v));
            }
            let mut props = Vec::new();
            if ts.name != designcraft_doc::NO_TABLE_STYLE {
                let base = ts.based_on.as_deref().unwrap_or(designcraft_doc::NO_TABLE_STYLE);
                props.push(if base == designcraft_doc::NO_TABLE_STYLE {
                    p("BasedOn", "string", "$ID/[No table style]")
                } else {
                    p("BasedOn", "object", names::style_self("TableStyle", names::TABLE_BUILTINS, base))
                });
            }
            tables = tables.child(with_props(el, props));
        }
        root.push(tables);
    }

    /// The document's table of contents as a TOC style; its story names it in `AppliedTOCStyle`.
    fn toc_style_el(&mut self, toc: &designcraft_doc::Toc) -> El {
        let para = |n: &str| names::style_self("ParagraphStyle", PARA_BUILTINS, n);
        let chars = |n: Option<&str>| names::style_self("CharacterStyle", CHAR_BUILTINS, n.unwrap_or(designcraft_doc::NO_CHAR_STYLE));
        let mut el = El::new("TOCStyle")
            .attr("Self", TOC_STYLE_ID)
            .attr("Name", TOC_STYLE_NAME)
            .attr("Title", &toc.title)
            .attr("TitleStyle", para(toc.title_style.as_deref().unwrap_or("TOC Title")))
            .attr("RunIn", "false")
            .attr("IncludeHidden", "false")
            .attr("IncludeBookDocuments", "false")
            .attr("CreateBookmarks", if toc.create_bookmarks { "true" } else { "false" });
        for e in &toc.entries {
            let format = e.entry_style.clone().unwrap_or_else(|| format!("TOC Level {}", e.level));
            let position = match e.page_number {
                _ if !toc.page_numbers => "NoPageNumber",
                designcraft_doc::TocPageNumber::After => "AfterEntry",
                designcraft_doc::TocPageNumber::Before => "BeforeEntry",
                designcraft_doc::TocPageNumber::None => "NoPageNumber",
            };
            el.push(
                El::new("TOCStyleEntry")
                    .attr("Self", self.fresh())
                    .attr("Name", names::style_name_out(PARA_BUILTINS, &e.style))
                    .attr("FormatStyle", para(&format))
                    .attr("Level", e.level)
                    .attr("PageNumberPosition", position)
                    .attr("Separator", names::toc_separator_out(e.separator.as_deref().unwrap_or("\t")))
                    .attr("PageNumberStyle", chars(e.page_number_style.as_deref()))
                    .attr("SeparatorStyle", chars(e.separator_style.as_deref()))
                    .attr("Sort", if e.sort { "true" } else { "false" }),
            );
        }
        el
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
