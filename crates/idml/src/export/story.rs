use super::common::{change_type, uid, with_props, xref_format_self};
use super::{CHAR_BUILTINS, CharFormat, El, Ex, ListType, NO_CHAR_ID, Node, PARA_BUILTINS, ParaAttrs, ParaFormat, Story, TOC_STYLE_ID, names, st};

impl<'a> Ex<'a> {
    /// `<GraphicLayerOption>` of a placed graphic: its layer attributes and nested layer tree.
    pub(super) fn layer_option_el(&mut self, options: &designcraft_doc::GraphicLayerOptions) -> El {
        let model = craft_idml::model::GraphicLayerOptions {
            attributes: options.attributes.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
            layers: options.layers.iter().map(|layer| layer.iter().map(|(key, value)| (key.clone(), value.clone())).collect()).collect(),
            parents: options.parents.clone(),
        };
        let self_id = self.fresh();
        crate::xml::from_craft(model.to_el(&self_id, || self.fresh()))
    }

    /// `<Index>`: nested topics with their See / See also cross-references.
    pub(super) fn index_el(&mut self) -> El {
        let model = craft_idml::model::IndexValues {
            topics: self.index_topics.clone(),
            cross_references: self
                .index_xrefs
                .iter()
                .map(|(from, reference_type, target)| craft_idml::model::IndexCrossReferenceValues {
                    from: from.clone(),
                    reference_type: (*reference_type).into(),
                    target: target.clone(),
                })
                .collect(),
        };
        crate::xml::from_craft(model.to_el(|| self.fresh()))
    }

    pub(super) fn footnote_option_el(&self) -> El {
        let o = &self.d.footnote_options;
        let marker = if o.ref_char_style == st::NO_CHAR_STYLE {
            NO_CHAR_ID.to_string()
        } else {
            names::style_self("CharacterStyle", CHAR_BUILTINS, &o.ref_char_style)
        };
        let restart = names::NOTE_RESTART.iter().find(|r| r.0 == o.restart).map_or("DontRestart", |r| r.1);
        let affix = names::NOTE_AFFIX.iter().find(|r| r.0 == o.affix_in).map_or("NoPrefixSuffix", |r| r.1);
        let values = craft_idml::model::FootnotePreferences {
            start_at: Some(f64::from(o.start_at)),
            prefix: Some(o.prefix.clone()),
            suffix: Some(o.suffix.clone()),
            paragraph_style: Some(names::style_self("ParagraphStyle", PARA_BUILTINS, &o.para_style)),
            marker_style: Some(marker),
            separator: Some(o.separator.clone()),
            space_between: Some(o.space_between),
            space_before: Some(o.space_before),
            first_baseline: Some(names::first_baseline_out(o.first_baseline).into()),
            first_baseline_minimum: Some(o.first_baseline_min),
            span_columns: Some(o.span_columns),
            rule_on: Some(o.rule.on),
            rule_weight: Some(o.rule.weight),
            rule_tint: Some(f64::from(o.rule.tint) * 100.0),
            rule_left_indent: Some(o.rule.left_indent),
            rule_width: Some(o.rule.width),
            rule_offset: Some(o.rule.offset),
            rule_color: Some(self.sw(&o.rule.color)),
            numbering_style: Some(names::number_style_out(o.style).into()),
            restart_numbering: Some(restart.into()),
            show_prefix_suffix: Some(affix.into()),
            marker_positioning: Some(names::note_marker_out(o.ref_position).into()),
        };
        crate::xml::from_craft(values.to_el())
    }

    /// `<Footnote>`: the footnote's paragraphs, the first starting with the footnote number marker
    /// and the separator (as InDesign stores them).
    fn footnote_el(&mut self, note: &Story) -> El {
        let mut el = El::new("Footnote");
        let mut paras = self.story_paras(note);
        let lead = vec![Node::Pi("ACE".into(), "4".into()), Node::Text(self.d.footnote_options.separator.clone())];
        if let Some(Node::El(csr)) =
            paras.first_mut().and_then(|p| p.children.iter_mut().find(|n| matches!(n, Node::El(e) if e.name == "CharacterStyleRange")))
        {
            match csr.children.iter_mut().find_map(|n| match n {
                Node::El(c) if c.name == "Content" => Some(c),
                _ => None,
            }) {
                Some(c) => {
                    c.children.splice(0..0, lead);
                }
                None => {
                    let mut c = El::new("Content");
                    c.children = lead;
                    csr.children.insert(0, Node::El(c));
                }
            }
        }
        for p in paras {
            el.push(p);
        }
        el
    }

    // ---------- stories ----------

    pub(super) fn story_el(&mut self, s: &Story) -> El {
        let toc = self.d.toc.as_ref().is_some_and(|t| t.story == s.id);
        let mut el = El::new("Story")
            .attr("Self", uid(s.id.0))
            .attr("AppliedTOCStyle", if toc { TOC_STYLE_ID } else { "n" })
            .attr("TrackChanges", "false")
            .attr("StoryTitle", "$ID/");
        el.push(crate::xml::from_craft(
            craft_idml::model::StoryPreference {
                optical_margin_alignment: s.optical_margin.on,
                optical_margin_size: designcraft_doc::OpticalMargin::clamp_size(s.optical_margin.size)
                    .unwrap_or(designcraft_doc::OpticalMargin::DEFAULT_SIZE),
                vertical: s.vertical,
                right_to_left: s.direction == designcraft_doc::TextDirection::RightToLeft,
            }
            .to_el(),
        ));
        for psr in self.story_paras(s) {
            el.push(psr);
        }
        el
    }

    /// Write an index reference and record any cross-reference it declares.
    fn index_reference(&mut self, s: &Story, k: usize) -> Option<El> {
        if let Some(r) = s.index_refs.get(k).filter(|r| !r.topics.is_empty()) {
            use designcraft_doc::index::IndexRange as R;
            for n in 1..=r.topics.len() {
                // A shared topic has one key: keep the first reference's
                // value (even empty), rather than inventing a merged key.
                self.index_topics.entry(r.topics[..n].to_vec()).or_insert_with(|| r.sort.get(n - 1).cloned().unwrap_or_default());
            }
            let (kind, limit) = match &r.range {
                R::CurrentPage => (Some("CurrentPage"), None),
                R::ToEndOfStory => (Some("ToEndOfStory"), None),
                R::NextParagraphs(n) => (Some("ForNextNParagraphs"), Some(*n)),
                R::SuppressPageRange => (Some("SuppressPageNumbers"), None),
                R::See(t) => {
                    self.index_xrefs.push((r.topics.clone(), "See", t.clone()));
                    (None, None)
                }
                R::SeeAlso(t) => {
                    self.index_xrefs.push((r.topics.clone(), "SeeAlso", t.clone()));
                    (Some("CurrentPage"), None)
                }
            };
            if let Some(kind) = kind {
                let model = craft_idml::model::PageReference::to_el(
                    &self.fresh(),
                    kind,
                    &craft_idml::model::IndexValues::topic_self(&r.topics),
                    k + 1,
                    limit,
                );
                return Some(crate::xml::from_craft(model));
            }
        }
        None
    }

    fn anchored_object(&mut self, s: &Story, k: usize) -> Option<El> {
        if let Some(o) = s.objects.get(k) {
            let mut e = self.item_el(&o.item, o.item.xf);
            use designcraft_doc::anchored::AnchorAlign as A;
            let setting = match &o.position {
                designcraft_doc::AnchorPosition::Inline { y_offset } => craft_idml::model::AnchoredObjectSetting::inline_position(*y_offset).to_el(),
                designcraft_doc::AnchorPosition::AboveLine { align, space_before, space_after } => {
                    craft_idml::model::AnchoredObjectSetting::above_line(
                        match align {
                            A::Left => "LeftAlign",
                            A::Center => "CenterAlign",
                            A::Right => "RightAlign",
                        },
                        *space_before,
                        *space_after,
                    )
                    .to_el()
                }
                designcraft_doc::AnchorPosition::Custom {
                    x_relative,
                    y_relative,
                    x_offset,
                    y_offset,
                    object_point,
                    ref_point,
                    keep_within_column,
                    spine_relative,
                } => craft_idml::model::AnchoredObjectSetting::anchored(
                    names::anchor_out(*object_point),
                    names::anchor_rel_out(*x_relative, false),
                    names::anchor_rel_out(*y_relative, true),
                    ["LeftAlign", "CenterAlign", "RightAlign"][(*ref_point % 3) as usize],
                    ["TopAlign", "CenterAlign", "BottomAlign"][(*ref_point / 3).min(2) as usize],
                    // IDML measures a left-aligned object's X offset leftwards.
                    if *ref_point % 3 == 0 { 0.0 - *x_offset } else { *x_offset },
                    *y_offset,
                    *keep_within_column,
                    *spine_relative,
                )
                .to_el(),
            };
            e.push(crate::xml::from_craft(setting));
            return Some(e);
        }
        None
    }

    fn apply_tracked_changes(
        &mut self,
        f: &CharFormat,
        csrs: &mut [El],
        para_br: bool,
        runs: &[(std::ops::Range<usize>, &CharFormat)],
        para_end: usize,
    ) {
        // Tracked changes wrap the text they mark.
        if let Some(kind) = change_type(f.over.change) {
            for c in csrs.iter_mut().filter(|c| c.name == "CharacterStyleRange" && !c.children.is_empty()) {
                let mut ch = El::new("Change").attr("Self", self.fresh()).attr("ChangeType", kind);
                ch.children = std::mem::take(&mut c.children);
                c.push(ch);
            }
        }
        if para_br && let Some(c) = csrs.last_mut() {
            let nl = runs
                .get(runs.partition_point(|(rr, _)| rr.end <= para_end))
                .filter(|(rr, _)| rr.start <= para_end)
                .and_then(|(_, nf)| change_type(nf.over.change));
            match nl {
                None => c.push(El::new("Br")),
                Some(kind) => match c.children.last_mut() {
                    Some(Node::El(ch)) if ch.name == "Change" && ch.get("ChangeType") == Some(kind) => ch.push(El::new("Br")),
                    _ => {
                        let mut ch = El::new("Change").attr("Self", self.fresh()).attr("ChangeType", kind);
                        ch.push(El::new("Br"));
                        c.push(ch);
                    }
                },
            }
        }
    }

    /// `ParagraphStyleRange`s of a story (or a table cell's story).
    pub(super) fn story_paras(&mut self, s: &Story) -> Vec<El> {
        let ranges = s.para_ranges();
        let n = ranges.len();
        // The runs and the marker offsets, once: each paragraph (and each marker) searched them from the start.
        let runs: Vec<(std::ops::Range<usize>, &CharFormat)> = s.runs().collect();
        let marks = Marks::of(s);
        ranges.into_iter().enumerate().map(|(pi, r)| self.story_para(s, pi, r, pi + 1 == n, &runs, &marks)).collect()
    }

    fn paragraph_runs(
        s: &Story,
        r: &std::ops::Range<usize>,
        runs: &[(std::ops::Range<usize>, &CharFormat)],
    ) -> Vec<(std::ops::Range<usize>, CharFormat)> {
        // Character runs intersecting the paragraph (they ascend: the first is a search away).
        let mut segs: Vec<(std::ops::Range<usize>, CharFormat)> = Vec::new();
        for (rr, f) in &runs[runs.partition_point(|(rr, _)| rr.end <= r.start)..] {
            if rr.start >= r.end {
                break;
            }
            let a = rr.start.max(r.start);
            let b = rr.end.min(r.end);
            if a < b {
                segs.push((a..b, (*f).clone()));
            }
        }
        if segs.is_empty() {
            let f = s.char_format_at(r.start).clone();
            segs.push((r.start..r.start, f));
        }
        segs
    }

    /// A cross-reference source's text: what its format generates, or the text typed over it, its
    /// page numbers as InDesign's live cross-reference page number (as InDesign writes them, so
    /// generated text reads back as generated).
    fn xref_content(&mut self, c: &mut El, x: &designcraft_doc::CrossRef) {
        let t = match &x.text {
            Some(t) => t.clone(),
            None => self.d.generated_xref_text(x.target, &x.format).unwrap_or_else(|| "??".into()),
        };
        let page = x.page.clone().unwrap_or_else(|| "?".into());
        for (n, part) in t.split(designcraft_doc::xref::PAGE_NUM_BLOCK).enumerate() {
            if n > 0 {
                c.push(
                    El::new("TextVariableInstance")
                        .attr("Self", self.fresh())
                        .attr("Name", "<?AID 001b?>TV XRefPageNumber")
                        .attr("ResultText", &page)
                        .attr("AssociatedTextVariable", "dTextVariablen<?AID 001b?>TV XRefPageNumber"),
                );
            }
            if !part.is_empty() {
                let mut content = El::new("Content");
                content.children.push(Node::Text(part.to_string()));
                c.push(content);
            }
        }
    }

    fn story_para(
        &mut self,
        s: &Story,
        pi: usize,
        r: std::ops::Range<usize>,
        last: bool,
        runs: &[(std::ops::Range<usize>, &CharFormat)],
        marks: &Marks,
    ) -> El {
        let pf = s.paras.get(pi).cloned().unwrap_or_default();
        let mut psr = self.psr_el(&pf);
        let ends_with_break = s.text.get(r.clone()).and_then(|t| t.chars().next_back()).is_some_and(st::is_break_char);
        let segs = Self::paragraph_runs(s, &r, runs);
        // A hyperlink on a run of text is written as a `HyperlinkTextSource` around the
        // ranges it covers, so the runs are cut at its edges first (#1446).
        let (links, segs) = self.cut_runs_at_links(s, &r, segs);
        let nseg = segs.len();
        // Each segment's ranges, with the link they belong to; wrapped once the paragraph is
        // built so that consecutive ranges of one link share a single source element.
        let mut psr_csrs: Vec<(Option<usize>, El)> = Vec::new();
        for (k, (rr, f)) in segs.into_iter().enumerate() {
            let seg_start = rr.start;
            let text = &s.text[rr];
            // Split at break characters.
            let mut cur = String::new();
            let mut pending: Vec<Node> = Vec::new();
            let mut csrs: Vec<El> = Vec::new();
            let flush_text = |cur: &mut String, pending: &mut Vec<Node>| {
                if !cur.is_empty() {
                    pending.push(Node::Text(std::mem::take(cur)));
                }
            };
            let flush_content = |pending: &mut Vec<Node>, out: &mut Vec<Node>| {
                if !pending.is_empty() {
                    let mut c = El::new("Content");
                    c.children = std::mem::take(pending);
                    out.push(Node::El(c));
                }
            };
            let mut out: Vec<Node> = Vec::new();
            for (ci, ch) in text.char_indices() {
                let ace = match ch {
                    st::PAGE_NUMBER | st::NEXT_PAGE_NUMBER | st::PREV_PAGE_NUMBER => Some("18"),
                    st::SECTION_MARKER => Some("19"),
                    st::INDENT_HERE => Some("7"),
                    st::RIGHT_INDENT_TAB => Some("8"),
                    _ => None,
                };
                let brk = match ch {
                    st::COLUMN_BREAK => Some("NextColumn"),
                    st::FRAME_BREAK => Some("NextFrame"),
                    st::PAGE_BREAK => Some("NextPage"),
                    st::ODD_PAGE_BREAK => Some("NextOddPage"),
                    st::EVEN_PAGE_BREAK => Some("NextEvenPage"),
                    _ => None,
                };
                if let Some(code) = ace {
                    flush_text(&mut cur, &mut pending);
                    pending.push(Node::Pi("ACE".into(), code.into()));
                } else if ch == st::TABLE_ANCHOR {
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    if let Some(t) = s.para_table(pi) {
                        out.push(Node::El(self.table_el(t)));
                    }
                } else if ch == designcraft_doc::ANCHOR_MARK {
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    let k = marks.anchor.partition_point(|&at| at < seg_start + ci);
                    if let Some(a) = s.anchors.get(k) {
                        out.push(Node::El(crate::xml::from_craft(craft_idml::model::StoryAnchor::destination_el(a.id, &a.name))));
                    }
                } else if ch == designcraft_doc::OBJECT_MARK {
                    // An anchored object: the item inside the range with its anchoring settings.
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    let k = marks.object.partition_point(|&at| at < seg_start + ci);
                    if let Some(el) = self.anchored_object(s, k) {
                        out.push(Node::El(el));
                    }
                } else if ch == designcraft_doc::INDEX_MARK {
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    let k = marks.index.partition_point(|&at| at < seg_start + ci);
                    if let Some(el) = self.index_reference(s, k) {
                        out.push(Node::El(el));
                    }
                } else if ch == designcraft_doc::XREF_MARK {
                    // A source wrapping its current text (InDesign regenerates it on update).
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    if !out.is_empty() {
                        let mut c = self.csr_el(&f);
                        c.children.append(&mut out);
                        csrs.push(c);
                    }
                    let k = marks.xref.partition_point(|&at| at < seg_start + ci);
                    if let Some(x) = s.xrefs.get(k) {
                        let me = self.fresh();
                        let fmt = self.d.xref_formats.iter().position(|ff| ff.name == x.format).map_or("n".to_string(), xref_format_self);
                        let mut c = self.csr_el(&f);
                        self.xref_content(&mut c, x);
                        let mut src = El::new("CrossReferenceSource")
                            .attr("Self", &me)
                            .attr("AppliedFormat", fmt)
                            .attr("Name", format!("Cross-Reference {}", self.xref_sources.len() + 1))
                            .attr("Hidden", "false");
                        src.push(c);
                        csrs.push(src);
                        self.xref_sources.push((me, x.target));
                    }
                } else if let Some(vi) = designcraft_doc::vars::var_index(ch) {
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    if let Some(v) = self.d.text_variables.get(vi) {
                        let result = self.d.variable_value(vi, None).unwrap_or_else(|| format!("{}{}", v.before, v.after));
                        out.push(Node::El(
                            El::new("TextVariableInstance")
                                .attr("Self", self.fresh())
                                .attr("Name", &v.name)
                                .attr("ResultText", result)
                                .attr("AssociatedTextVariable", format!("dTextVariablen{}", v.name)),
                        ));
                    }
                } else if ch == designcraft_doc::NOTE_MARK {
                    // Editorial notes aren't written to IDML yet.
                } else if ch == designcraft_doc::ENDNOTE_REF {
                    // Endnote structures aren't written to IDML yet: the endnote frame keeps their
                    // text (numbers included), and the reference keeps its number as superscript
                    // text, so the endnote's place isn't lost (#2753).
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    if !out.is_empty() {
                        let mut c = self.csr_el(&f);
                        c.children.append(&mut out);
                        csrs.push(c);
                    }
                    let k = u32::try_from(s.endnotes_before(seg_start + ci)).unwrap_or(u32::MAX);
                    let n = self.d.endnote_start(s.id).saturating_add(k);
                    let mut c = self.csr_el(&f);
                    c.set("Position", names::position_out(designcraft_doc::Position::Superscript));
                    let mut content = El::new("Content");
                    content.children.push(Node::Text(self.d.endnote_options.label(n)));
                    c.push(content);
                    csrs.push(c);
                } else if ch == designcraft_doc::FOOTNOTE_REF {
                    // The reference: its own range carrying the reference position.
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    if !out.is_empty() {
                        let mut c = self.csr_el(&f);
                        c.children.append(&mut out);
                        csrs.push(c);
                    }
                    if let Some(n) = s.notes.get(s.notes_before(seg_start + ci)) {
                        let mut c = self.csr_el(&f);
                        let pos = self.d.footnote_options.ref_position;
                        if pos != designcraft_doc::Position::Normal {
                            c.set("Position", names::position_out(pos));
                        }
                        c.push(self.footnote_el(&n.text));
                        csrs.push(c);
                    }
                } else if let Some(bt) = brk {
                    flush_text(&mut cur, &mut pending);
                    flush_content(&mut pending, &mut out);
                    if !out.is_empty() {
                        let mut c = self.csr_el(&f);
                        c.children.append(&mut out);
                        csrs.push(c);
                    }
                    let mut c = self.csr_el(&f);
                    c.set("ParagraphBreakType", bt);
                    c.push(El::new("Br"));
                    csrs.push(c);
                } else {
                    cur.push(ch);
                }
            }
            flush_text(&mut cur, &mut pending);
            flush_content(&mut pending, &mut out);
            let is_last_seg = k + 1 == nseg;
            // A break character that ends the paragraph was written as its `Br`.
            let para_br = is_last_seg && !last && !ends_with_break;
            if !out.is_empty() || csrs.is_empty() || para_br {
                let mut c = self.csr_el(&f);
                c.children.extend(out);
                csrs.push(c);
            }
            self.apply_tracked_changes(&f, &mut csrs, para_br, runs, r.end);
            let link = links.iter().find(|(_, l)| l.contains(&seg_start) || (l.start == seg_start && l.start == l.end)).map(|(i, _)| *i);
            for c in csrs {
                psr_csrs.push((link, c));
            }
        }
        self.wrap_link_sources(&mut psr, psr_csrs);
        psr
    }

    /// The hyperlinks on story text that fall in the paragraph `r` (each link's index and its
    /// range clipped to `r`), and the paragraph's runs `segs` cut at their edges, so a link's
    /// ranges can be wrapped on their own (#1446).
    fn cut_runs_at_links(
        &self,
        s: &Story,
        r: &std::ops::Range<usize>,
        segs: Vec<(std::ops::Range<usize>, CharFormat)>,
    ) -> (Vec<(usize, std::ops::Range<usize>)>, Vec<(std::ops::Range<usize>, CharFormat)>) {
        let links: Vec<(usize, std::ops::Range<usize>)> = self
            .d
            .hyperlinks
            .iter()
            .enumerate()
            .filter_map(|(i, h)| match &h.source {
                designcraft_doc::HyperlinkSource::Text { story, start, end } if *story == s.id => {
                    let (a, b) = ((*start).max(r.start), (*end).min(r.end));
                    (a < b).then_some((i, a..b))
                }
                _ => None,
            })
            .collect();
        let mut cuts: Vec<usize> = links.iter().flat_map(|(_, l)| [l.start, l.end]).collect();
        cuts.sort_unstable();
        cuts.dedup();
        let segs = segs.into_iter().fold(Vec::new(), |mut out: Vec<(std::ops::Range<usize>, CharFormat)>, (rr, f)| {
            let mut at = rr.start;
            for c in cuts.iter().copied().filter(|c| rr.start < *c && *c < rr.end) {
                out.push((at..c, f.clone()));
                at = c;
            }
            out.push((at..rr.end, f));
            out
        });
        (links, segs)
    }

    /// Push the paragraph's character ranges into `psr`, with each run of consecutive ranges of
    /// one link inside a single `HyperlinkTextSource` (#1446).
    fn wrap_link_sources(&mut self, psr: &mut El, psr_csrs: Vec<(Option<usize>, El)>) {
        let mut groups: Vec<(Option<usize>, Vec<El>)> = Vec::new();
        for (link, c) in psr_csrs {
            match groups.last_mut() {
                Some((l, v)) if *l == link => v.push(c),
                _ => groups.push((link, vec![c])),
            }
        }
        for (link, cs) in groups {
            match link.and_then(|i| self.d.hyperlinks.get(i).map(|h| (i, h.name.clone()))) {
                None => {
                    for c in cs {
                        psr.push(c);
                    }
                }
                Some((i, name)) => {
                    let me = self.fresh();
                    let mut src = El::new("HyperlinkTextSource").attr("Self", &me).attr("Name", name).attr("Hidden", "false");
                    for c in cs {
                        src.push(c);
                    }
                    psr.push(src);
                    self.link_text_sources.push((me, i));
                }
            }
        }
    }

    pub(super) fn psr_el(&mut self, pf: &ParaFormat) -> El {
        let mut el = El::new("ParagraphStyleRange").attr("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, &pf.style));
        let mut props = Vec::new();
        let styles = &self.d.styles;
        let pa = with_list_tab(pf.para.clone(), || (styles.resolve_para(pf).0, styles.resolve_para_style(&pf.style).0));
        self.para_attrs(&mut el, &mut props, &pa, Some(&pf.style));
        self.char_attrs(&mut el, &mut props, &pf.chars);
        with_props(el, props)
    }

    pub(super) fn csr_el(&mut self, f: &CharFormat) -> El {
        let id = if f.style.is_empty() || f.style == st::NO_CHAR_STYLE {
            NO_CHAR_ID.to_string()
        } else {
            names::style_self("CharacterStyle", CHAR_BUILTINS, &f.style)
        };
        let mut el = El::new("CharacterStyleRange").attr("AppliedCharacterStyle", id);
        let mut props = Vec::new();
        self.char_attrs(&mut el, &mut props, &f.over);
        with_props(el, props)
    }
}

/// The left stop the composer adds for a list's Tab Position, if it adds one.
fn list_tab_stop(pp: &designcraft_doc::ParaProps) -> Option<f64> {
    if !matches!(pp.list_type, ListType::Bullets | ListType::Numbers) {
        return None;
    }
    pp.list_tab.filter(|t| t.is_finite() && *t >= 0.0 && !pp.tabs.iter().any(|s| (s.position - t).abs() < 1e-6))
}

/// IDML has no attribute for the bullets-and-numbering Tab Position, so it is written as the left
/// stop the composer adds for it, in the TabList of each level (style or paragraph) that sets the
/// list type, the tab position or the tabs. `resolved` gives this level's resolved attributes and
/// those it inherits; the stop comes back on import as an ordinary tab stop.
pub(super) fn with_list_tab(mut a: ParaAttrs, resolved: impl FnOnce() -> (designcraft_doc::ParaProps, designcraft_doc::ParaProps)) -> ParaAttrs {
    if a.list_tab.is_none() && a.tabs.is_none() && a.list_type.is_none() {
        return a;
    }
    let (here, parent) = resolved();
    let stop = list_tab_stop(&here);
    // An inherited TabList holds the parent's stop; a level that ends the list writes its tabs without it.
    if stop.is_none() && list_tab_stop(&parent).is_none() {
        return a;
    }
    let mut tabs = here.tabs;
    if let Some(t) = stop {
        tabs.push(designcraft_doc::TabStop { position: t, align: designcraft_doc::TabAlign::Left, leader: String::new(), align_on: String::new() });
        tabs.sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    a.tabs = Some(tabs);
    a
}

/// Byte offsets of a story's anchor, object, index and cross-reference markers, in text order.
pub(super) struct Marks {
    anchor: Vec<usize>,
    object: Vec<usize>,
    index: Vec<usize>,
    xref: Vec<usize>,
}

impl Marks {
    fn of(s: &Story) -> Self {
        let at = |c: char| s.text.match_indices(c).map(|(i, _)| i).collect::<Vec<_>>();
        Marks {
            anchor: at(designcraft_doc::ANCHOR_MARK),
            object: at(designcraft_doc::OBJECT_MARK),
            index: at(designcraft_doc::INDEX_MARK),
            xref: at(designcraft_doc::XREF_MARK),
        }
    }
}
