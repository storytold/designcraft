use super::values::tint;
use super::{BaselineGrid, El, GridRelative, Importer, PARA_BUILTINS, names};

/// IDML stores a sort key on each topic, rather than on its page references.
#[derive(Clone, Default)]
pub(super) struct IndexTopic {
    pub(super) path: Vec<String>,
    pub(super) sort: Vec<String>,
}

impl IndexTopic {
    fn child(&self, topic: &El) -> Self {
        let mut path = self.path.clone();
        path.push(topic.get("Name").unwrap_or("").to_string());
        let mut sort = self.sort.clone();
        if let Some(key) = topic.get("SortOrder").filter(|key| !key.is_empty()) {
            // Keep empty positions before a child's key, but omit trailing defaults.
            sort.resize(self.path.len(), String::new());
            sort.push(key.to_string());
        }
        Self { path, sort }
    }
}

impl<'r> Importer<'r> {
    pub(super) fn doc_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        if let Some(v) = e.get("OverprintBlack") {
            s.overprint_black = v != "false";
        }
        if let Some(v) = e.num("PageWidth") {
            s.page_width = v;
        }
        if let Some(v) = e.num("PageHeight") {
            s.page_height = v;
        }
        if let Some(v) = e.boolean("FacingPages") {
            s.facing_pages = v;
        }
        if let Some(v) = e.get("PageBinding") {
            s.right_to_left_binding = v == "RightToLeft";
        }
        for (i, k) in ["DocumentBleedTopOffset", "DocumentBleedBottomOffset", "DocumentBleedInsideOrLeftOffset", "DocumentBleedOutsideOrRightOffset"]
            .iter()
            .enumerate()
        {
            if let Some(v) = e.num(k) {
                s.bleed[i] = v;
            }
        }
        for (i, k) in ["SlugTopOffset", "SlugBottomOffset", "SlugInsideOrLeftOffset", "SlugRightOrOutsideOffset"].iter().enumerate() {
            if let Some(v) = e.num(k) {
                s.slug[i] = v;
            }
        }
        if let Some(i) = e.get("Intent") {
            s.intent = match i {
                "WebIntent" => designcraft_doc::Intent::Web,
                "MobileIntent" => designcraft_doc::Intent::Mobile,
                _ => designcraft_doc::Intent::Print,
            };
        }
        if let Some(v) = e.boolean("CreatePrimaryTextFrame") {
            s.primary_text_frame = v;
        }
        if let Some(v) = e.boolean("ColumnGuideLocked") {
            s.column_guides_locked = v;
        }
    }

    pub(super) fn grid_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        let bg: &mut BaselineGrid = &mut s.baseline_grid;
        if let Some(v) = e.num("BaselineStart") {
            bg.start = v;
        }
        if let Some(v) = e.num("BaselineDivision") {
            bg.increment = v;
        }
        if let Some(v) = e.num("BaselineViewThreshold") {
            bg.view_threshold = v / 100.0;
        }
        if let Some(v) = e.get("BaselineGridRelativeOption") {
            bg.relative_to = if v.contains("Margin") { GridRelative::TopMargin } else { GridRelative::TopOfPage };
        }
        if let Some(v) = e.num("HorizontalGridlineDivision") {
            s.grid.horizontal = v;
        }
        if let Some(v) = e.num("VerticalGridlineDivision") {
            s.grid.vertical = v;
        }
        if let Some(v) = e.num("HorizontalGridSubdivision") {
            s.grid.subdivisions = v.max(1.0) as u32;
        }
        if let Some(v) = e.boolean("GridsInBack") {
            s.grid.in_back = v;
        }
    }

    /// Index topics (recursively); at the root, their See / See also cross-references too.
    pub(super) fn index_topics_of(&mut self, e: &El, prefix: &IndexTopic) {
        for t in e.find_all("Topic") {
            let topic = prefix.child(t);
            if let Some(me) = t.get("Self") {
                self.index_topics.insert(me.to_string(), topic.clone());
            }
            self.index_topics_of(t, &topic);
        }
        if !prefix.path.is_empty() {
            return;
        }
        let mut stack = vec![(e, IndexTopic::default())];
        while let Some((el, prefix)) = stack.pop() {
            for t in el.find_all("Topic") {
                let topic = prefix.child(t);
                for x in t.find_all("CrossReference") {
                    let target = x.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).map(|t| t.path.join(": ")).unwrap_or_default();
                    use designcraft_doc::index::IndexRange as R;
                    let range = match x.get("CrossReferenceType") {
                        Some("SeeAlso") | Some("SeeAlsoHerein") => R::SeeAlso(target),
                        _ => R::See(target),
                    };
                    self.index_xrefs.push(designcraft_doc::IndexRef { topics: topic.path.clone(), sort: topic.sort.clone(), range });
                }
                stack.push((t, topic));
            }
        }
    }

    /// `CrossReferenceFormat`: building blocks back to a format definition.
    /// A `TextVariable` definition. Types that aren't computed here keep their settings and show
    /// the result of their first instance.
    pub(super) fn text_variable(&mut self, e: &El) {
        use designcraft_doc::vars::{TextVariable, Use, VAR_MAX, VarKind};
        let ty = e.get("VariableType").unwrap_or("");
        // Cross-reference internals; export writes its own. Their page numbers are recognised
        // inside cross-reference text.
        if ty == "XrefPageNumberType"
            && let Some(id) = e.get("Self")
        {
            self.xref_page_vars.insert(id.to_string());
        }
        if ty.is_empty() || ty.starts_with("Xref") {
            return;
        }
        let vars = self.text_vars.get_or_insert_with(Vec::new);
        if vars.len() >= VAR_MAX {
            return;
        }
        let pref = e.elements().find(|c| c.local().ends_with("Preference"));
        let attr = |k: &str| pref.and_then(|p| p.get(k)).unwrap_or("");
        let date = || Some(attr("Format")).filter(|f| !f.is_empty()).unwrap_or("MM/dd/yy").to_string();
        let kind = match ty {
            "CustomTextType" => Some(VarKind::Custom { text: attr("Contents").into() }),
            "LastPageNumberType" => Some(VarKind::LastPageNumber { section: attr("Scope") == "SectionScope" }),
            "ChapterNumberType" => Some(VarKind::ChapterNumber),
            "FileNameType" if attr("IncludePath") != "true" => Some(VarKind::FileName { extension: attr("IncludeExtension") == "true" }),
            "CreationDateType" => Some(VarKind::CreationDate { format: date() }),
            "ModificationDateType" => Some(VarKind::ModificationDate { format: date() }),
            "OutputDateType" => Some(VarKind::OutputDate { format: date() }),
            "MatchParagraphStyleType" | "MatchCharacterStyleType" => {
                let character = ty == "MatchCharacterStyleType";
                let r = attr(if character { "AppliedCharacterStyle" } else { "AppliedParagraphStyle" });
                let use_ = if attr("SearchStrategy") == "LastOnPage" { Use::LastOnPage } else { Use::FirstOnPage };
                // Change Case and Delete End Punctuation aren't modelled: the header shows the text as set.
                (!r.is_empty()).then(|| VarKind::RunningHeader {
                    style: if character { self.char_style_ref(r) } else { self.para_style_ref(r) },
                    use_,
                    character,
                })
            }
            _ => None,
        };
        let name = e.get("Name").unwrap_or("").to_string();
        let v = match kind {
            Some(kind) => {
                let mut v = TextVariable::new(&name, kind);
                v.before = attr("TextBefore").into();
                v.after = attr("TextAfter").into();
                v
            }
            None => TextVariable::new(
                &name,
                VarKind::Imported {
                    variable_type: ty.into(),
                    result: String::new(),
                    settings_element: pref.map(|p| p.name.clone()).unwrap_or_default(),
                    settings: pref.map(|p| p.attrs.clone()).unwrap_or_default(),
                },
            ),
        };
        let vars = self.text_vars.get_or_insert_with(Vec::new);
        if let Some(s) = e.get("Self") {
            self.text_var_ids.insert(s.to_string(), vars.len());
        }
        vars.push(v);
    }

    /// A `TextVariableInstance`: the variable's character, or its recorded text when the file
    /// doesn't define the variable.
    pub(super) fn text_variable_instance(&mut self, e: &El) -> String {
        use designcraft_doc::vars::{VarKind, var_char};
        let result = e.get("ResultText").unwrap_or("");
        let found = e.get("AssociatedTextVariable").and_then(|r| self.text_var_ids.get(r)).copied();
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
        let name = e.get("Name").unwrap_or("Format").to_string();
        let mut def = String::new();
        for b in e.find_all("BuildingBlock") {
            let text = |k: &str| b.get(k).filter(|v| *v != "$ID/").unwrap_or("").to_string();
            match b.get("BlockType").unwrap_or("") {
                "CustomStringBuildingBlock" => def.push_str(&text("CustomText")),
                "FullParagraphBuildingBlock" => def.push_str("<fullPara />"),
                "ParagraphTextBuildingBlock" => def.push_str("<paraText />"),
                "ParagraphNumberBuildingBlock" => def.push_str("<paraNum />"),
                "PageNumberBuildingBlock" => def.push_str("<pageNum />"),
                "BookmarkNameBuildingBlock" => def.push_str("<txtAnchrName />"),
                "ChapterNumberBuildingBlock" => def.push_str("<chapNum />"),
                "FileNameBuildingBlock" => def.push_str("<fileName />"),
                "PartialParagraphBuildingBlock" => def.push_str(&format!(
                    "<partialPara delim=\"{}\" includeDelim=\"{}\" />",
                    text("AppliedDelimiter"),
                    b.get("IncludeDelimiter") == Some("true")
                )),
                _ => {}
            }
        }
        if let Some(me) = e.get("Self") {
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
        if e.boolean("BreakDocument") != Some(true) {
            return;
        }
        let name = e.get("ParagraphStyleName").unwrap_or("$ID/NormalParagraphStyle");
        self.settings.epub_split = if name.trim().is_empty() {
            designcraft_doc::EpubSplit::ExportTags
        } else {
            let style = self.para_style_ref(name);
            if self.styles.para(&style).is_some() { designcraft_doc::EpubSplit::Style(style) } else { designcraft_doc::EpubSplit::Off }
        };
    }

    /// Preferences › `FootnoteOption` (after styles: it names the footnote text and marker styles).
    pub(super) fn footnote_option(&mut self, e: &El) {
        let mut o = designcraft_doc::FootnoteOptions::default();
        if let Some(v) = e.num("StartAt") {
            o.start_at = v.max(0.0) as u32;
        }
        o.prefix = e.get("Prefix").unwrap_or("").into();
        o.suffix = e.get("Suffix").unwrap_or("").into();
        if let Some(r) = e.get("FootnoteTextStyle") {
            o.para_style = self.para_style_ref(r);
        }
        if let Some(r) = e.get("FootnoteMarkerStyle") {
            o.ref_char_style = self.char_style_ref(r);
        }
        if let Some(v) = e.get("SeparatorText") {
            o.separator = v.into();
        }
        o.space_between = e.num("SpaceBetween").unwrap_or(0.0);
        o.space_before = e.num("Spacer").unwrap_or(0.0);
        if let Some(v) = e.prop("FootnoteFirstBaselineOffset") {
            o.first_baseline = names::first_baseline_in(v.trim());
        }
        o.first_baseline_min = e.num("FootnoteMinimumFirstBaselineOffset").unwrap_or(0.0);
        o.span_columns = e.boolean("EnableStraddling").unwrap_or(false);
        o.rule.on = e.boolean("RuleOn").unwrap_or(true);
        o.rule.weight = e.num("RuleLineWeight").unwrap_or(1.0);
        o.rule.tint = tint(e.num("RuleTint")).unwrap_or(1.0);
        o.rule.left_indent = e.num("RuleLeftIndent").unwrap_or(0.0);
        o.rule.width = e.num("RuleWidth").unwrap_or(72.0);
        o.rule.offset = e.num("RuleOffset").unwrap_or(0.0);
        if let Some(c) = e.prop("RuleColor") {
            o.rule.color = self.swatch_ref(c.trim());
        }
        if let Some(v) = e.prop("FootnoteNumberingStyle") {
            o.style = names::note_style_in(v.trim());
        }
        if let Some(v) = e.prop("RestartNumbering") {
            o.restart = names::NOTE_RESTART.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = e.prop("ShowPrefixSuffix") {
            o.affix_in = names::NOTE_AFFIX.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = e.prop("MarkerPositioning") {
            o.ref_position = names::note_marker_in(v.trim());
        }
        self.footnote_options = o;
    }

    /// The table of contents: the TOC style a story is generated from (`AppliedTOCStyle`), with
    /// its entries. Styles the document doesn't have fall back to the generated TOC styles.
    pub(super) fn toc_of(&self, top: &[El]) -> Option<designcraft_doc::Toc> {
        use designcraft_doc::{TocEntry, TocPageNumber};
        let (ts, story) = top.iter().filter(|e| e.local() == "TOCStyle").find_map(|ts| {
            let id = ts.get("Self")?;
            let story = top.iter().find(|e| e.local() == "Story" && e.get("AppliedTOCStyle") == Some(id))?;
            Some((ts, *self.story_ids.get(story.get("Self")?)?))
        })?;
        // Property children are element text: formatting whitespace around a reference is dropped
        // unless the reference as written names a style.
        let known = |r: Option<&str>| r.map(|r| self.listed_para_style_ref(r)).filter(|n| self.styles.para(n).is_some());
        // Character styles the document has; [None] is stored as no style.
        let known_chars = |r: Option<&str>| {
            r.map(|r| self.listed_char_style_ref(r)).filter(|n| n != designcraft_doc::NO_CHAR_STYLE && self.styles.char_style(n).is_some())
        };
        let entries: Vec<TocEntry> = ts
            .elements()
            .filter(|e| e.local() == "TOCStyleEntry")
            .filter_map(|e| {
                let style = names::style_name_in(PARA_BUILTINS, e.get("Name")?);
                if style.is_empty() || style.chars().count() > 256 {
                    return None;
                }
                let level = e.get("Level").and_then(|l| l.trim().parse::<u8>().ok()).unwrap_or(1).clamp(1, 9);
                let page_number = match e.get("PageNumberPosition") {
                    Some("BeforeEntry") => TocPageNumber::Before,
                    Some("NoPageNumber") => TocPageNumber::None,
                    _ => TocPageNumber::After,
                };
                let separator = e.get("Separator").map(crate::names::toc_separator_in).filter(|s| s != "\t" && s.chars().count() <= 64);
                // InDesign writes these as `Properties` children; attributes are read too.
                let entry_style = known(e.prop("FormatStyle").as_deref()).filter(|n| *n != format!("TOC Level {level}"));
                let page_number_style = known_chars(e.prop("PageNumberStyle").as_deref());
                let separator_style = known_chars(e.prop("SeparatorStyle").as_deref());
                let sort = e.get("Sort").is_some_and(|v| v.trim() == "true");
                Some(TocEntry { style, level, entry_style, page_number, separator, page_number_style, separator_style, sort })
            })
            .take(64)
            .collect();
        if entries.is_empty() {
            return None;
        }
        let title: String = ts.get("Title").unwrap_or("").chars().filter(|c| !matches!(c, '\n' | '\r' | '\u{2029}')).take(1024).collect();
        let title_style = known(ts.get("TitleStyle")).filter(|n| n != "TOC Title");
        let create_bookmarks = ts.get("CreateBookmarks").or_else(|| ts.get("CreatePdfBookmarks")) == Some("true");
        Some(designcraft_doc::Toc { story, title, entries, page_numbers: true, title_style, create_bookmarks })
    }
}
