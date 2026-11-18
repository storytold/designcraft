use super::values::tint;
use super::{BaselineGrid, El, GridRelative, Importer, PARA_BUILTINS, TopKind, TopLevelNode, names};

impl<'r> Importer<'r> {
    pub(super) fn doc_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        let values = craft_idml::model::DocumentPreferences::from_el(&crate::xml::to_craft(e));
        if let Some(v) = values.overprint_black {
            s.overprint_black = v;
        }
        if let Some(v) = values.page_width {
            s.page_width = v;
        }
        if let Some(v) = values.page_height {
            s.page_height = v;
        }
        if let Some(v) = values.facing_pages {
            s.facing_pages = v;
        }
        if let Some(v) = values.page_binding.as_deref() {
            s.right_to_left_binding = v == "RightToLeft";
        }
        for (i, value) in values.bleed.into_iter().enumerate() {
            if let Some(v) = value {
                s.bleed[i] = v;
            }
        }
        for (i, value) in values.slug.into_iter().enumerate() {
            if let Some(v) = value {
                s.slug[i] = v;
            }
        }
        if let Some(i) = values.intent.as_deref() {
            s.intent = match i {
                "WebIntent" => designcraft_doc::Intent::Web,
                "MobileIntent" => designcraft_doc::Intent::Mobile,
                _ => designcraft_doc::Intent::Print,
            };
        }
        if let Some(v) = values.primary_text_frame {
            s.primary_text_frame = v;
        }
        if let Some(v) = values.column_guides_locked {
            s.column_guides_locked = v;
        }
    }

    pub(super) fn grid_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        let bg: &mut BaselineGrid = &mut s.baseline_grid;
        let values = craft_idml::model::GridPreferences::from_el(&crate::xml::to_craft(e));
        if let Some(v) = values.baseline_start {
            bg.start = v;
        }
        if let Some(v) = values.baseline_division {
            bg.increment = v;
        }
        if let Some(v) = values.baseline_view_threshold {
            bg.view_threshold = v / 100.0;
        }
        if let Some(v) = values.baseline_relative_option.as_deref() {
            bg.relative_to = if v.contains("Margin") { GridRelative::TopMargin } else { GridRelative::TopOfPage };
        }
        if let Some(v) = values.horizontal_division {
            s.grid.horizontal = v;
        }
        if let Some(v) = values.vertical_division {
            s.grid.vertical = v;
        }
        if let Some(v) = values.horizontal_subdivision {
            s.grid.subdivisions = v.max(1.0) as u32;
        }
        if let Some(v) = values.grids_in_back {
            s.grid.in_back = v;
        }
    }

    /// `CrossReferenceFormat`: building blocks back to a format definition.
    /// A `TextVariable` definition. Types that aren't computed here keep their settings and show
    /// the result of their first instance.
    pub(super) fn text_variable(&mut self, e: &El) {
        use designcraft_doc::vars::{TextVariable, Use, VAR_MAX, VarKind};
        let values = craft_idml::model::TextVariableValues::from_el(&crate::xml::to_craft(e));
        // Cross-reference internals; export writes its own. Their page numbers are recognised
        // inside cross-reference text.
        if let craft_idml::model::TextVariableKind::CrossReference { page_number } = &values.kind {
            if *page_number && let Some(id) = values.self_id.as_ref() {
                self.xref_page_vars.insert(id.clone());
            }
            return;
        }
        let kind = match values.kind {
            craft_idml::model::TextVariableKind::CustomText { text } => VarKind::Custom { text },
            craft_idml::model::TextVariableKind::LastPageNumber { section } => VarKind::LastPageNumber { section },
            craft_idml::model::TextVariableKind::ChapterNumber => VarKind::ChapterNumber,
            craft_idml::model::TextVariableKind::FileName { extension } => VarKind::FileName { extension },
            craft_idml::model::TextVariableKind::Date { kind, format } => match kind {
                craft_idml::model::TextDateKind::Creation => VarKind::CreationDate { format },
                craft_idml::model::TextDateKind::Modification => VarKind::ModificationDate { format },
                craft_idml::model::TextDateKind::Output => VarKind::OutputDate { format },
            },
            craft_idml::model::TextVariableKind::RunningHeader { style, character, last_on_page } => VarKind::RunningHeader {
                style: if character { self.char_style_ref(&style) } else { self.para_style_ref(&style) },
                use_: if last_on_page { Use::LastOnPage } else { Use::FirstOnPage },
                character,
            },
            craft_idml::model::TextVariableKind::Imported { variable_type, settings_element, settings } => {
                if variable_type.is_empty() {
                    return;
                }
                VarKind::Imported { variable_type, result: String::new(), settings_element, settings }
            }
            craft_idml::model::TextVariableKind::CrossReference { .. } => return,
        };
        let vars = self.text_vars.get_or_insert_with(Vec::new);
        if vars.len() >= VAR_MAX {
            return;
        }
        let pref = e.elements().find(|c| c.local().ends_with("Preference"));
        let attr = |k: &str| pref.and_then(|p| p.get(k)).unwrap_or("");
        let mut variable = TextVariable::new(&values.name, kind);
        variable.before = if !values.before.is_empty() { values.before } else { attr("TextBefore").to_string() };
        variable.after = if !values.after.is_empty() { values.after } else { attr("TextAfter").to_string() };
        if let Some(id) = values.self_id {
            self.text_var_ids.insert(id, vars.len());
        }
        vars.push(variable);
    }

    /// A `TextVariableInstance`: the variable's character, or its recorded text when the file
    /// doesn't define the variable.
    pub(super) fn text_variable_instance(&mut self, values: &craft_idml::model::Values) -> String {
        use designcraft_doc::vars::{VarKind, var_char};
        let result = values.get("ResultText").unwrap_or("");
        let found = values.get("AssociatedTextVariable").and_then(|r| self.text_var_ids.get(r)).copied();
        let var = found.and_then(|i| Some((i, self.text_vars.as_mut()?.get_mut(i)?)));
        match var.and_then(|(i, v)| Some((var_char(i)?, v))) {
            Some((c, v)) => {
                if let VarKind::Imported { result: r, .. } = &mut v.kind
                    && r.is_empty()
                {
                    *r = result.to_string();
                }
                c.to_string()
            }
            None => result.to_string(),
        }
    }

    pub(super) fn xref_format(&mut self, e: &El) {
        let values = craft_idml::model::CrossReferenceFormatValues::from_el(&crate::xml::to_craft(e));
        let name = values.name;
        let mut def = String::new();
        for block in values.blocks {
            use craft_idml::model::CrossReferenceBuildingBlock as Block;
            match block {
                Block::CustomText(text) => def.push_str(&text),
                Block::FullParagraph => def.push_str("<fullPara />"),
                Block::ParagraphText => def.push_str("<paraText />"),
                Block::ParagraphNumber => def.push_str("<paraNum />"),
                Block::PageNumber => def.push_str("<pageNum />"),
                Block::BookmarkName => def.push_str("<txtAnchrName />"),
                Block::ChapterNumber => def.push_str("<chapNum />"),
                Block::FileName => def.push_str("<fileName />"),
                Block::PartialParagraph { delimiter, include_delimiter } => {
                    def.push_str(&format!("<partialPara delim=\"{}\" includeDelim=\"{}\" />", delimiter, include_delimiter))
                }
            }
        }
        if let Some(me) = values.self_id {
            self.xref_format_names.insert(me.to_string(), name.clone());
        }
        match self.xref_formats.iter_mut().find(|f| f.name == name) {
            Some(f) => f.definition = def,
            None => self.xref_formats.push(designcraft_doc::XrefFormat { name, definition: def }),
        }
    }

    /// Preferences › `EPubExportPreference`: Split Document (after styles: it can name one). An
    /// empty style name splits on the styles' export tags; a style the file lacks splits nothing.
    pub(super) fn epub_split(&mut self, e: &El) {
        let values = craft_idml::model::EpubSplitValues::from_el(&crate::xml::to_craft(e));
        if !values.break_document {
            return;
        }
        let name = values.paragraph_style.as_deref().unwrap_or("$ID/NormalParagraphStyle");
        self.settings.epub_split = if name.trim().is_empty() {
            designcraft_doc::EpubSplit::ExportTags
        } else {
            let style = self.para_style_ref(name);
            if self.styles.para(&style).is_some() { designcraft_doc::EpubSplit::Style(style) } else { designcraft_doc::EpubSplit::Off }
        };
    }

    /// Preferences › `FootnoteOption` (after styles: it names the footnote text and marker styles).
    pub(super) fn footnote_option(&mut self, e: &El) {
        let values = craft_idml::model::FootnotePreferences::from_el(&crate::xml::to_craft(e));
        let mut o = designcraft_doc::FootnoteOptions::default();
        if let Some(v) = values.start_at {
            o.start_at = v.max(0.0) as u32;
        }
        o.prefix = values.prefix.unwrap_or_default();
        o.suffix = values.suffix.unwrap_or_default();
        if let Some(r) = values.paragraph_style.as_deref() {
            o.para_style = self.para_style_ref(r);
        }
        if let Some(r) = values.marker_style.as_deref() {
            o.ref_char_style = self.char_style_ref(r);
        }
        if let Some(v) = values.separator {
            o.separator = v;
        }
        o.space_between = values.space_between.unwrap_or(0.0);
        o.space_before = values.space_before.unwrap_or(0.0);
        if let Some(v) = values.first_baseline.as_deref() {
            o.first_baseline = names::first_baseline_in(v.trim());
        }
        o.first_baseline_min = values.first_baseline_minimum.unwrap_or(0.0);
        o.span_columns = values.span_columns.unwrap_or(false);
        o.rule.on = values.rule_on.unwrap_or(true);
        o.rule.weight = values.rule_weight.unwrap_or(1.0);
        o.rule.tint = tint(values.rule_tint).unwrap_or(1.0);
        o.rule.left_indent = values.rule_left_indent.unwrap_or(0.0);
        o.rule.width = values.rule_width.unwrap_or(72.0);
        o.rule.offset = values.rule_offset.unwrap_or(0.0);
        if let Some(c) = values.rule_color.as_deref() {
            o.rule.color = self.swatch_ref(c.trim());
        }
        if let Some(v) = values.numbering_style.as_deref() {
            o.style = names::note_style_in(v.trim());
        }
        if let Some(v) = values.restart_numbering.as_deref() {
            o.restart = names::NOTE_RESTART.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = values.show_prefix_suffix.as_deref() {
            o.affix_in = names::NOTE_AFFIX.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = values.marker_positioning.as_deref() {
            o.ref_position = names::note_marker_in(v.trim());
        }
        self.footnote_options = o;
    }

    /// The table of contents: the TOC style a story is generated from (`AppliedTOCStyle`), with
    /// its entries. Styles the document doesn't have fall back to the generated TOC styles.
    pub(super) fn toc_of(&self, top: &[TopLevelNode]) -> Option<designcraft_doc::Toc> {
        use designcraft_doc::{TocEntry, TocPageNumber};
        let (toc, story) = top.iter().filter(|node| node.kind == TopKind::TocStyle).find_map(|node| {
            let toc = craft_idml::model::TocStyleValues::from_el(&node.to_el());
            let id = toc.self_id.as_deref()?;
            let story = top.iter().find(|node| node.kind == TopKind::Story && node.values().get("AppliedTOCStyle") == Some(id))?;
            Some((toc, *self.story_ids.get(story.values().self_id()?)?))
        })?;
        // Property children are element text: formatting whitespace around a reference is dropped
        // unless the reference as written names a style.
        let known = |r: Option<&str>| r.map(|r| self.listed_para_style_ref(r)).filter(|n| self.styles.para(n).is_some());
        // Character styles the document has; [None] is stored as no style.
        let known_chars = |r: Option<&str>| {
            r.map(|r| self.listed_char_style_ref(r)).filter(|n| n != designcraft_doc::NO_CHAR_STYLE && self.styles.char_style(n).is_some())
        };
        let entries: Vec<TocEntry> = toc
            .entries
            .iter()
            .filter_map(|entry| {
                let style = names::style_name_in(PARA_BUILTINS, entry.style.as_deref()?);
                if style.is_empty() || style.chars().count() > 256 {
                    return None;
                }
                let level = entry.level.unwrap_or(1).clamp(1, 9);
                let page_number = match entry.page_number_position.as_deref() {
                    Some("BeforeEntry") => TocPageNumber::Before,
                    Some("NoPageNumber") => TocPageNumber::None,
                    _ => TocPageNumber::After,
                };
                let separator = entry.separator.as_deref().map(crate::names::toc_separator_in).filter(|s| s != "\t" && s.chars().count() <= 64);
                // InDesign writes these as `Properties` children; the shared parser accepts both spellings.
                let entry_style = known(entry.entry_style.as_deref()).filter(|n| *n != format!("TOC Level {level}"));
                let page_number_style = known_chars(entry.page_number_style.as_deref());
                let separator_style = known_chars(entry.separator_style.as_deref());
                let sort = entry.sort == Some(true);
                Some(TocEntry { style, level, entry_style, page_number, separator, page_number_style, separator_style, sort })
            })
            .collect();
        if entries.is_empty() {
            return None;
        }
        let title: String = toc.title.as_deref().unwrap_or("").chars().filter(|c| !matches!(c, '\n' | '\r' | '\u{2029}')).take(1024).collect();
        let title_style = known(toc.title_style.as_deref()).filter(|n| n != "TOC Title");
        let create_bookmarks = toc.create_bookmarks == Some(true);
        Some(designcraft_doc::Toc { story, title, entries, page_numbers: true, title_style, create_bookmarks })
    }
}
