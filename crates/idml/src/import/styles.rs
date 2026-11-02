use super::table::uniform_cell_stroke;
use super::values::tint;
use super::{
    CHAR_BUILTINS, CharacterStyle, El, Fill, Importer, OBJECT_BUILTINS, ObjectStyle, PARA_BUILTINS, ParagraphStyle, Rule, names, st, swatch,
    unescape_id,
};

impl<'r> Importer<'r> {
    // ---------- styles ----------

    pub(super) fn styles(&mut self, top: &[El]) {
        for e in top.iter().filter(|e| e.local() == "Condition") {
            let (Some(id), Some(name)) = (e.get("Self"), e.get("Name")) else { continue };
            // Indicator colours: "r g b" (0–255) or a UI colour name (kept as the default blue).
            let nums: Vec<u8> = e
                .get("IndicatorColor")
                .unwrap_or("")
                .split_whitespace()
                .filter_map(|x| x.parse::<f64>().ok())
                .map(|x| x.clamp(0.0, 255.0) as u8)
                .collect();
            let color = if nums.len() == 3 { [nums[0], nums[1], nums[2]] } else { [79, 153, 255] };
            self.condition_names.insert(id.to_string(), name.to_string());
            self.conditions.push(designcraft_doc::Condition { name: name.to_string(), color, visible: e.get("Visible") != Some("false") });
        }
        for e in top.iter().filter(|e| e.local() == "NumberingList") {
            let name = e.get("Name").unwrap_or("").to_string();
            if !name.is_empty() && !name.starts_with("$ID/") && !self.lists.iter().any(|l| l.name == name) {
                self.lists
                    .push(designcraft_doc::NumberedList { name, continue_across_stories: e.get("ContinueNumbersAcrossStories") == Some("true") });
            }
        }
        let mut paras: Vec<(String, El)> = Vec::new();
        let mut chars: Vec<(String, El)> = Vec::new();
        let mut objects: Vec<(String, El)> = Vec::new();
        let mut cell_styles: Vec<(String, El)> = Vec::new();
        let mut table_styles: Vec<(String, El)> = Vec::new();
        fn walk(e: &El, style: &str, group: &str, out: &mut Vec<(String, El)>) {
            for c in e.elements() {
                if c.local() == style {
                    out.push((c.get("Self").unwrap_or("").to_string(), c.clone()));
                } else if c.local() == group {
                    walk(c, style, group, out);
                }
            }
        }
        for e in top {
            match e.local() {
                "RootParagraphStyleGroup" => walk(e, "ParagraphStyle", "ParagraphStyleGroup", &mut paras),
                "RootCharacterStyleGroup" => walk(e, "CharacterStyle", "CharacterStyleGroup", &mut chars),
                "RootObjectStyleGroup" => walk(e, "ObjectStyle", "ObjectStyleGroup", &mut objects),
                "RootCellStyleGroup" => walk(e, "CellStyle", "CellStyleGroup", &mut cell_styles),
                "RootTableStyleGroup" => walk(e, "TableStyle", "TableStyleGroup", &mut table_styles),
                _ => {}
            }
        }
        for (id, e) in &paras {
            let n = names::style_name_in(PARA_BUILTINS, e.get("Name").unwrap_or(""));
            self.para_names.insert(id.clone(), n);
        }
        for (id, e) in &chars {
            let n = names::style_name_in(CHAR_BUILTINS, e.get("Name").unwrap_or(""));
            self.char_names.insert(id.clone(), n);
        }
        for (id, e) in &objects {
            let n = names::style_name_in(OBJECT_BUILTINS, e.get("Name").unwrap_or(""));
            self.object_names.insert(id.clone(), n);
            self.object_els.insert(id.clone(), e.clone());
        }
        for (id, e) in &paras {
            self.para_els.insert(id.clone(), e.clone());
        }
        for (_, e) in &paras {
            let name = names::style_name_in(PARA_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = if name == designcraft_doc::NO_PARA_STYLE { None } else { self.based_on(e, true) };
            let next_style = e.get("NextStyle").map(|n| self.para_style_ref(n)).filter(|n| *n != name);
            let para = self.para_attrs(e);
            let chars = self.char_attrs(e);
            let s = ParagraphStyle { name: name.clone(), based_on, next_style, para, chars, shortcut: String::new() };
            match self.styles.para_mut(&name) {
                Some(slot) => *slot = s,
                None => self.styles.paragraph.push(s),
            }
            // Split Document (EPUB only): on the style, and on its EPUB export tag map. Files write
            // `false` on every style, so either place saying `true` turns it on.
            let split = e.get("SplitDocument") == Some("true")
                || e.elements()
                    .any(|m| m.local() == "StyleExportTagMap" && m.get("ExportType") == Some("EPUB") && m.get("SplitDocument") == Some("true"));
            if split {
                self.styles.export_tags.entry(format!("p:{name}")).or_default().split = true;
            }
        }
        for (_, e) in &chars {
            let name = names::style_name_in(CHAR_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = if name == st::NO_CHAR_STYLE { None } else { self.based_on(e, false) };
            let chars = self.char_attrs(e);
            let s = CharacterStyle { name: name.clone(), based_on, chars, shortcut: String::new() };
            match self.styles.char_style_mut(&name) {
                Some(slot) => *slot = s,
                None => self.styles.character.push(s),
            }
        }
        // Keep built-ins as well: [Basic Table] is editable, and [No table style]
        // supplies the defaults used by named styles in ordinary IDML files.
        for (id, e) in &cell_styles {
            self.cell_names.insert(id.clone(), names::style_name_in(names::CELL_BUILTINS, e.get("Name").unwrap_or("")));
        }
        for (id, e) in &table_styles {
            self.table_names.insert(id.clone(), names::style_name_in(names::TABLE_BUILTINS, e.get("Name").unwrap_or("")));
        }
        for (_, e) in &cell_styles {
            let name = names::style_name_in(names::CELL_BUILTINS, e.get("Name").unwrap_or(""));
            let mut cs = designcraft_doc::CellStyle { name: name.clone(), ..Default::default() };
            cs.based_on =
                e.prop("BasedOn").map(|r| self.cell_style_ref(&r)).or_else(|| Some(designcraft_doc::NO_CELL_STYLE.into())).filter(|n| *n != name);
            cs.fill = e.prop("FillColor").map(|c| self.swatch_ref(&c));
            cs.fill_tint = tint(e.num("FillTint"));
            cs.inset_overrides = ["TopInset", "LeftInset", "BottomInset", "RightInset"].map(|k| e.num(k).map(|v| v.max(0.0)));
            if let [Some(top), Some(left), Some(bottom), Some(right)] = cs.inset_overrides {
                cs.insets = Some([top, left, bottom, right]);
                cs.inset_overrides = [None; 4];
            }
            cs.vj = e.get("VerticalJustification").map(names::vj_in);
            cs.first_baseline = e.get("FirstBaselineOffset").map(names::first_baseline_in);
            cs.first_baseline_min = e.num("MinimumFirstBaselineOffset").filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 8640.0));
            cs.strokes = ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].map(|side| self.cell_stroke_attrs(e, side));
            if let Some(stroke) = uniform_cell_stroke(&cs.strokes) {
                cs.stroke = Some(stroke);
                cs.strokes = Default::default();
            }
            cs.paragraph_style = e.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).filter(|n| n != designcraft_doc::NO_PARA_STYLE);
            match self.styles.cell.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = cs,
                None => self.styles.cell.push(cs),
            }
        }
        for (_, e) in &table_styles {
            let name = names::style_name_in(names::TABLE_BUILTINS, e.get("Name").unwrap_or(""));
            let mut ts = designcraft_doc::TableStyle { name: name.clone(), ..Default::default() };
            ts.based_on =
                e.prop("BasedOn").map(|r| self.table_style_ref(&r)).or_else(|| Some(designcraft_doc::NO_TABLE_STYLE.into())).filter(|n| *n != name);
            ts.header = e.get("HeaderRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.body = e.get("BodyRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.footer = e.get("FooterRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.left_column =
                e.get("LeftColumnRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.right_column =
                e.get("RightColumnRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.header_same_as_body = e.boolean("HeaderRegionSameAsBodyRegion");
            ts.footer_same_as_body = e.boolean("FooterRegionSameAsBodyRegion");
            ts.left_column_same_as_body = e.boolean("LeftColumnRegionSameAsBodyRegion");
            ts.right_column_same_as_body = e.boolean("RightColumnRegionSameAsBodyRegion");
            ts.borders = ["TopBorder", "LeftBorder", "BottomBorder", "RightBorder"].map(|side| self.cell_stroke_attrs(e, side));
            if let Some(border) = uniform_cell_stroke(&ts.borders) {
                ts.border = Some(border);
                ts.borders = Default::default();
            }
            ts.row_fills = self.alt_fill_attrs(e, "Row");
            ts.column_fills = self.alt_fill_attrs(e, "Column");
            ts.row_strokes = self.alt_stroke_attrs(e, "Row");
            ts.column_strokes = self.alt_stroke_attrs(e, "Column");
            ts.space_before = e.num("SpaceBefore");
            ts.space_after = e.num("SpaceAfter");
            match self.styles.table.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = ts,
                None => self.styles.table.push(ts),
            }
        }
        for (_, e) in &objects {
            let name = names::style_name_in(OBJECT_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = e.prop("BasedOn").map(|b| {
                self.object_names
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| names::style_name_in(OBJECT_BUILTINS, &unescape_id(b.trim_start_matches("ObjectStyle/"))))
            });
            let based_on = based_on.filter(|b| *b != designcraft_doc::NO_OBJECT_STYLE && *b != name);
            let enabled = |k: &str, attr: &str| match e.get(k) {
                Some(v) => v == "true",
                None => e.get(attr).is_some(),
            };
            let fill = enabled("EnableFill", "FillColor").then(|| {
                let sw = e.get("FillColor").map(|r| self.swatch_ref(r)).unwrap_or_else(|| swatch::NONE.into());
                Fill { swatch: sw, tint: tint(e.num("FillTint")).unwrap_or(1.0), ..Fill::none() }
            });
            let stroke = enabled("EnableStroke", "StrokeColor").then(|| self.stroke_from(e, None));
            let paragraph_style = enabled("EnableParagraphStyle", "AppliedParagraphStyle")
                .then(|| e.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)))
                .flatten();
            let text_frame = if e.get("EnableTextFrameGeneralOptions") == Some("true") {
                e.find("TextFramePreference").map(|t| self.frame_options(&[t]))
            } else {
                None
            };
            let effects = self.style_effects(e);
            let s = ObjectStyle { name: name.clone(), based_on, fill, stroke, paragraph_style, text_frame, effects };
            match self.styles.object.iter_mut().find(|o| o.name == name) {
                Some(slot) => *slot = s,
                None => self.styles.object.push(s),
            }
        }
    }

    fn based_on(&self, e: &El, para: bool) -> Option<String> {
        let b = e.prop("BasedOn")?;
        let b = b.trim();
        if para {
            if let Some(n) = self.para_names.get(b) {
                return Some(n.clone());
            }
            let n = names::style_name_in(PARA_BUILTINS, &unescape_id(b.trim_start_matches("ParagraphStyle/")));
            Some(n)
        } else {
            if let Some(n) = self.char_names.get(b) {
                return Some(n.clone()).filter(|n| n != st::NO_CHAR_STYLE);
            }
            let n = names::style_name_in(CHAR_BUILTINS, &unescape_id(b.trim_start_matches("CharacterStyle/")));
            Some(n).filter(|n| n != st::NO_CHAR_STYLE)
        }
    }

    pub(super) fn para_style_ref(&self, r: &str) -> String {
        self.para_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(PARA_BUILTINS, &unescape_id(r.trim_start_matches("ParagraphStyle/"))))
    }

    /// A paragraph style reference written as an element's text: as written when it names a
    /// style, otherwise without surrounding whitespace (see [`Self::listed_char_style_ref`]).
    pub(super) fn listed_para_style_ref(&self, r: &str) -> String {
        if self.para_names.contains_key(r) { self.para_style_ref(r) } else { self.para_style_ref(r.trim()) }
    }
    /// A character style reference written as an element's text (nested, line and GREP styles).
    /// A style name may end in a space, so the reference is used as written when it names a
    /// style; only otherwise is surrounding whitespace dropped.
    pub(super) fn listed_char_style_ref(&self, r: &str) -> String {
        if self.char_names.contains_key(r) { self.char_style_ref(r) } else { self.char_style_ref(r.trim()) }
    }

    pub(super) fn char_style_ref(&self, r: &str) -> String {
        self.char_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(CHAR_BUILTINS, &unescape_id(r.trim_start_matches("CharacterStyle/"))))
    }

    pub(super) fn rule(&mut self, e: &El, k: &str) -> Option<Rule> {
        // A style or range can set some of a rule's settings (such as only its colour); the others
        // come from its paragraph style chain, so the rule is whole.
        let fields = ["", "LineWeight", "Color", "Tint", "Width", "Offset", "LeftIndent", "RightIndent"];
        // Any of them, or of the type, gap and Keep In Frame settings, makes it an override (the
        // rest then come from the style chain, or the defaults: off, 1 pt).
        let more = ["Type", "GapColor", "GapTint"];
        let own = fields.iter().chain(&more).any(|f| e.prop(&format!("{k}{f}")).is_some())
            || (k == "RuleAbove" && e.prop("KeepRuleAboveInFrame").is_some());
        if !own {
            return None;
        }
        let v: Vec<Option<String>> = fields.iter().map(|f| self.para_setting(e, &format!("{k}{f}"))).collect();
        let get = |i: usize| v.get(i).cloned().flatten();
        let num = |i: usize| get(i).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite());
        let on = get(0).map(|v| v.trim() == "true");
        let weight = num(1);
        let mut r = Rule { on: on.unwrap_or(false), ..Rule::default() };
        if let Some(w) = weight {
            r.weight = w;
        }
        if let Some(t) = tint(num(3)) {
            r.tint = t;
        }
        r.column_width = get(4).is_none_or(|w| w.trim() != "TextWidth");
        r.offset = num(5).unwrap_or(0.0);
        r.left_indent = num(6).unwrap_or(0.0);
        r.right_indent = num(7).unwrap_or(0.0);
        // Type, gap colour and tint, Keep In Frame (rule above).
        if let Some(t) = self.para_setting(e, &format!("{k}Type")) {
            r.kind = names::stroke_type_in(t.trim());
        }
        if let Some(c) = self.para_setting(e, &format!("{k}GapColor"))
            && let Some(c) = self.rule_color_ref(&c)
        {
            r.gap_color = c;
        }
        if let Some(t) = tint(self.para_setting(e, &format!("{k}GapTint")).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())) {
            r.gap_tint = t;
        }
        if k == "RuleAbove" {
            r.keep_in_frame = self.para_setting(e, "KeepRuleAboveInFrame").is_some_and(|v| v.trim() == "true");
        }
        if let Some(c) = get(2).and_then(|c| self.rule_color_ref(&c)) {
            r.color = c;
        }
        Some(r)
    }

    /// A rule's colour or gap colour: Text Color (written `Text Color` or `$ID/Text Color`), or
    /// a swatch reference; anything else leaves the default.
    fn rule_color_ref(&mut self, c: &str) -> Option<String> {
        let c = c.trim();
        if c == designcraft_doc::TEXT_COLOR || c.strip_prefix("$ID/") == Some(designcraft_doc::TEXT_COLOR) {
            Some(designcraft_doc::TEXT_COLOR.to_string())
        } else {
            c.contains('/').then(|| self.swatch_ref(c))
        }
    }

    /// Paragraph setting `k` of `e` (a paragraph style or a range): its own, else from its
    /// paragraph style chain (`BasedOn` of a style, the applied style of a range).
    fn para_setting(&self, e: &El, k: &str) -> Option<String> {
        if let Some(v) = e.prop(k) {
            return Some(v);
        }
        let style_ref = |r: String| if r.starts_with("ParagraphStyle/") { r } else { format!("ParagraphStyle/{}", names::escape_id(&r)) };
        let mut next =
            if e.local() == "ParagraphStyle" { e.prop("BasedOn") } else { e.get("AppliedParagraphStyle").map(str::to_string) }.map(style_ref);
        for _ in 0..16 {
            let Some(id) = next else { break };
            let s = self.para_els.get(&id)?;
            if let Some(v) = s.prop(k) {
                return Some(v);
            }
            next = s.prop("BasedOn").map(style_ref);
        }
        None
    }

    pub(super) fn cell_style_ref(&self, reference: &str) -> String {
        let reference = reference.trim();
        self.cell_names
            .get(reference)
            .cloned()
            .unwrap_or_else(|| names::style_name_in(names::CELL_BUILTINS, &unescape_id(reference.trim_start_matches("CellStyle/"))))
    }

    pub(super) fn table_style_ref(&self, reference: &str) -> String {
        let reference = reference.trim();
        self.table_names
            .get(reference)
            .cloned()
            .unwrap_or_else(|| names::style_name_in(names::TABLE_BUILTINS, &unescape_id(reference.trim_start_matches("TableStyle/"))))
    }

    pub(super) fn cell_stroke_attrs(&mut self, el: &El, prefix: &str) -> designcraft_doc::CellStrokeAttrs {
        designcraft_doc::CellStrokeAttrs {
            weight: el.num(&format!("{prefix}StrokeWeight")).map(|v| v.max(0.0)),
            color: el.prop(&format!("{prefix}StrokeColor")).map(|v| self.swatch_ref(&v)),
            tint: tint(el.num(&format!("{prefix}StrokeTint"))),
            kind: el.prop(&format!("{prefix}StrokeType")).map(|v| names::stroke_type_in(&v)),
            gap_color: el.prop(&format!("{prefix}StrokeGapColor")).map(|v| self.swatch_ref(&v)),
            gap_tint: tint(el.num(&format!("{prefix}StrokeGapTint"))),
        }
    }

    pub(super) fn alt_fill_attrs(&mut self, el: &El, kind: &str) -> designcraft_doc::AltFillsAttrs {
        designcraft_doc::AltFillsAttrs {
            first: el.num(&format!("Start{kind}FillCount")).map(|v| v.max(0.0) as u32),
            first_color: el.prop(&format!("Start{kind}FillColor")).map(|v| self.swatch_ref(&v)),
            first_tint: tint(el.num(&format!("Start{kind}FillTint"))),
            next: el.num(&format!("End{kind}FillCount")).map(|v| v.max(0.0) as u32),
            next_color: el.prop(&format!("End{kind}FillColor")).map(|v| self.swatch_ref(&v)),
            next_tint: tint(el.num(&format!("End{kind}FillTint"))),
            skip_first: el.num(&format!("SkipFirstAlternatingFill{kind}s")).map(|v| v.max(0.0) as u32),
            skip_last: el.num(&format!("SkipLastAlternatingFill{kind}s")).map(|v| v.max(0.0) as u32),
        }
    }

    pub(super) fn alt_stroke_attrs(&mut self, el: &El, kind: &str) -> designcraft_doc::AltStrokesAttrs {
        let first_stroke = self.cell_stroke_attrs(el, &format!("Start{kind}"));
        let mut next_stroke = self.cell_stroke_attrs(el, &format!("End{kind}"));
        // IDML uses LineStyle for this one attribute (rather than StrokeType).
        if kind == "Column"
            && let Some(value) = el.prop("EndColumnLineStyle")
        {
            next_stroke.kind = Some(names::stroke_type_in(&value));
        }
        designcraft_doc::AltStrokesAttrs {
            first: el.num(&format!("Start{kind}StrokeCount")).map(|v| v.max(0.0) as u32),
            next: el.num(&format!("End{kind}StrokeCount")).map(|v| v.max(0.0) as u32),
            first_stroke,
            next_stroke,
            skip_first: el.num(&format!("SkipFirstAlternatingStroke{kind}s")).map(|v| v.max(0.0) as u32),
            skip_last: el.num(&format!("SkipLastAlternatingStroke{kind}s")).map(|v| v.max(0.0) as u32),
        }
    }
}
