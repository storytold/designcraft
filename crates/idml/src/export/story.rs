use super::common::{anchor_self, bool_s, change_type, topic_self, uid, with_props, xref_format_self};
use super::graphics::graphic_layer_attribute_name;
use super::*;

impl<'a> Ex<'a> {
    /// `<GraphicLayerOption>` of a placed graphic: its layer attributes and the nested layer tree.
    pub(super) fn layer_option_el(&mut self, options: &designcraft_doc::GraphicLayerOptions) -> El {
        let mut option = El::new("GraphicLayerOption");
        for (key, value) in &options.attributes {
            if graphic_layer_attribute_name(key) {
                option.set(key, value);
            }
        }
        // Only an ancestor whose subtree is still open can parent the next layer.
        // Reopening a closed subtree would reorder native-edited lists on roundtrip.
        let mut open = Vec::new();
        let mut parents = Vec::with_capacity(options.layers.len());
        for i in 0..options.layers.len() {
            let parent = options.parents.get(i).copied().flatten();
            if let Some(at) = parent.and_then(|p| open.iter().position(|id| *id == p)).filter(|at| *at < 239) {
                open.truncate(at + 1);
                parents.push(parent);
            } else {
                open.clear();
                parents.push(None);
            }
            open.push(i);
        }
        let mut children: Vec<Vec<El>> = vec![Vec::new(); options.layers.len()];
        let mut roots = Vec::new();
        for (i, attributes) in options.layers.iter().enumerate().rev() {
            let mut layer = El::new("GraphicLayer").attr("Self", self.fresh());
            for (key, value) in attributes {
                if graphic_layer_attribute_name(key) {
                    layer.set(key, value);
                }
            }
            if let Some(nested) = children.get_mut(i) {
                for child in std::mem::take(nested).into_iter().rev() {
                    layer.push(child);
                }
            }
            if let Some(nested) = parents.get(i).copied().flatten().and_then(|parent| children.get_mut(parent)) {
                nested.push(layer);
            } else {
                roots.push(layer);
            }
        }
        for layer in roots.into_iter().rev() {
            option.push(layer);
        }
        option
    }

    /// `<Index>`: nested topics with their See / See also cross-references.
    pub(super) fn index_el(&mut self) -> El {
        let mut topics = self.index_topics.clone();
        for (_, _, target) in &self.index_xrefs {
            topics.entry(vec![target.clone()]).or_default();
        }
        fn build(ex: &mut Ex, topics: &BTreeMap<Vec<String>, String>, prefix: &[String]) -> Vec<El> {
            let mut out = Vec::new();
            for (t, sort) in topics.iter().filter(|(t, _)| t.len() == prefix.len() + 1 && t.starts_with(prefix)) {
                let mut el = El::new("Topic").attr("Self", topic_self(t)).attr("SortOrder", sort).attr("Name", t.last().map_or("", String::as_str));
                for (from, kind, target) in ex.index_xrefs.clone() {
                    if from == *t {
                        el.push(
                            El::new("CrossReference")
                                .attr("Self", ex.fresh())
                                .attr("ReferencedTopic", topic_self(std::slice::from_ref(&target)))
                                .attr("CrossReferenceType", kind)
                                .attr("CustomTypeString", ""),
                        );
                    }
                }
                for c in build(ex, topics, t) {
                    el.push(c);
                }
                out.push(el);
            }
            out
        }
        let mut el = El::new("Index").attr("Self", "ix");
        for t in build(self, &topics, &[]) {
            el.push(t);
        }
        el
    }

    pub(super) fn footnote_option_el(&self) -> El {
        let o = &self.d.footnote_options;
        let marker = if o.ref_char_style == st::NO_CHAR_STYLE {
            NO_CHAR_ID.to_string()
        } else {
            names::style_self("CharacterStyle", CHAR_BUILTINS, &o.ref_char_style)
        };
        let enumv = |name: &str, v: &str| {
            let mut e = El::new(name).attr("type", "enumeration");
            e.children.push(Node::Text(v.into()));
            e
        };
        let mut color = El::new("RuleColor").attr("type", "object");
        color.children.push(Node::Text(self.sw(&o.rule.color)));
        let mut el = El::new("FootnoteOption")
            .attr("StartAt", o.start_at)
            .attr("Prefix", &o.prefix)
            .attr("Suffix", &o.suffix)
            .attr("FootnoteTextStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, &o.para_style))
            .attr("FootnoteMarkerStyle", marker)
            .attr("SeparatorText", &o.separator)
            .attr("SpaceBetween", num(o.space_between))
            .attr("Spacer", num(o.space_before))
            .attr("FootnoteFirstBaselineOffset", names::first_baseline_out(o.first_baseline))
            .attr("FootnoteMinimumFirstBaselineOffset", num(o.first_baseline_min))
            .attr("EnableStraddling", bool_s(o.span_columns))
            .attr("RuleOn", bool_s(o.rule.on))
            .attr("RuleLineWeight", num(o.rule.weight))
            .attr("RuleTint", num(o.rule.tint as f64 * 100.0))
            .attr("RuleLeftIndent", num(o.rule.left_indent))
            .attr("RuleWidth", num(o.rule.width))
            .attr("RuleOffset", num(o.rule.offset));
        let restart = names::NOTE_RESTART.iter().find(|r| r.0 == o.restart).map_or("DontRestart", |r| r.1);
        let affix = names::NOTE_AFFIX.iter().find(|r| r.0 == o.affix_in).map_or("NoPrefixSuffix", |r| r.1);
        let mut props = El::new("Properties");
        props.push(enumv("FootnoteNumberingStyle", names::number_style_out(o.style)));
        props.push(enumv("RestartNumbering", restart));
        props.push(enumv("ShowPrefixSuffix", affix));
        props.push(enumv("MarkerPositioning", names::note_marker_out(o.ref_position)));
        props.push(color);
        el.push(props);
        el
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
        el.push(
            El::new("StoryPreference")
                .attr("OpticalMarginAlignment", bool_s(s.optical_margin.on))
                .attr(
                    "OpticalMarginSize",
                    num(designcraft_doc::OpticalMargin::clamp_size(s.optical_margin.size).unwrap_or(designcraft_doc::OpticalMargin::DEFAULT_SIZE)),
                )
                .attr("FrameType", "TextFrameType")
                .attr("StoryOrientation", if s.vertical { "Vertical" } else { "Horizontal" })
                .attr(
                    "StoryDirection",
                    if s.direction == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
                ),
        );
        for psr in self.story_paras(s) {
            el.push(psr);
        }
        el
    }

    /// `ParagraphStyleRange`s of a story (or a table cell's story).
    pub(super) fn story_paras(&mut self, s: &Story) -> Vec<El> {
        let mut paras = Vec::new();
        let ranges = s.para_ranges();
        let n = ranges.len();
        for (pi, r) in ranges.into_iter().enumerate() {
            let pf = s.paras.get(pi).cloned().unwrap_or_default();
            let mut psr = self.psr_el(&pf);
            let last = pi + 1 == n;
            let ends_with_break = s.text.get(r.clone()).and_then(|t| t.chars().next_back()).is_some_and(st::is_break_char);
            // Character runs intersecting the paragraph.
            let mut segs: Vec<(std::ops::Range<usize>, CharFormat)> = Vec::new();
            for (rr, f) in s.runs() {
                let a = rr.start.max(r.start);
                let b = rr.end.min(r.end);
                if a < b {
                    segs.push((a..b, f.clone()));
                }
            }
            if segs.is_empty() {
                let f = s.char_format_at(r.start).clone();
                segs.push((r.start..r.start, f));
            }
            // A hyperlink on a run of text is written as a `HyperlinkTextSource` around the
            // ranges it covers, so the runs are cut at its edges first (#1446).
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
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::ANCHOR_MARK).count();
                        if let Some(a) = s.anchors.get(k) {
                            out.push(Node::El(
                                El::new("HyperlinkTextDestination")
                                    .attr("Self", anchor_self(a.id))
                                    .attr("Name", &a.name)
                                    .attr("Hidden", "false")
                                    .attr("DestinationUniqueKey", a.id & 0xFFFF_FFFF),
                            ));
                        }
                    } else if ch == designcraft_doc::OBJECT_MARK {
                        // An anchored object: the item inside the range with its anchoring settings.
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::OBJECT_MARK).count();
                        if let Some(o) = s.objects.get(k) {
                            let mut e = self.item_el(&o.item, o.item.xf);
                            use designcraft_doc::anchored::AnchorAlign as A;
                            let setting = match &o.position {
                                designcraft_doc::AnchorPosition::Inline { y_offset } => {
                                    El::new("AnchoredObjectSetting").attr("AnchoredPosition", "InlinePosition").attr("AnchorYoffset", num(*y_offset))
                                }
                                designcraft_doc::AnchorPosition::AboveLine { align, space_before, space_after } => El::new("AnchoredObjectSetting")
                                    .attr("AnchoredPosition", "AboveLine")
                                    .attr(
                                        "HorizontalAlignment",
                                        match align {
                                            A::Left => "LeftAlign",
                                            A::Center => "CenterAlign",
                                            A::Right => "RightAlign",
                                        },
                                    )
                                    .attr("AnchorSpaceAbove", num(*space_before))
                                    .attr("AnchorYoffset", num(*space_after)),
                                designcraft_doc::AnchorPosition::Custom {
                                    x_relative,
                                    y_relative,
                                    x_offset,
                                    y_offset,
                                    object_point,
                                    ref_point,
                                    keep_within_column,
                                    spine_relative,
                                } => El::new("AnchoredObjectSetting")
                                    .attr("AnchoredPosition", "Anchored")
                                    .attr("AnchorPoint", names::anchor_out(*object_point))
                                    .attr("HorizontalReferencePoint", names::anchor_rel_out(*x_relative, false))
                                    .attr("VerticalReferencePoint", names::anchor_rel_out(*y_relative, true))
                                    .attr("HorizontalAlignment", ["LeftAlign", "CenterAlign", "RightAlign"][(*ref_point % 3) as usize])
                                    .attr("VerticalAlignment", ["TopAlign", "CenterAlign", "BottomAlign"][(*ref_point / 3).min(2) as usize])
                                    // IDML measures a left-aligned object's X offset leftwards.
                                    .attr("AnchorXoffset", num(if *ref_point % 3 == 0 { 0.0 - *x_offset } else { *x_offset }))
                                    .attr("AnchorYoffset", num(*y_offset))
                                    .attr("PinPosition", bool_s(*keep_within_column))
                                    .attr("SpineRelative", bool_s(*spine_relative)),
                            };
                            e.push(setting);
                            out.push(Node::El(e));
                        }
                    } else if ch == designcraft_doc::INDEX_MARK {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::INDEX_MARK).count();
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
                                let mut el = El::new("PageReference")
                                    .attr("Self", self.fresh())
                                    .attr("PageReferenceType", kind)
                                    .attr("ReferencedTopic", topic_self(&r.topics))
                                    .attr("Id", k + 1);
                                if let Some(n) = limit {
                                    el.set("PageReferenceLimit", n);
                                }
                                out.push(Node::El(el));
                            }
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
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::XREF_MARK).count();
                        if let Some(x) = s.xrefs.get(k) {
                            let me = self.fresh();
                            let fmt = self.d.xref_formats.iter().position(|ff| ff.name == x.format).map_or("n".to_string(), xref_format_self);
                            let text = self.d.xref_values(x.target).map_or("??".into(), |mut v| {
                                v.page = "?".into();
                                designcraft_doc::xref::expand(self.d.xref_format(&x.format).map_or("<fullPara />", |ff| ff.definition.as_str()), &v)
                            });
                            let mut c = self.csr_el(&f);
                            let mut content = El::new("Content");
                            content.children.push(Node::Text(text));
                            c.push(content);
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
                        // Endnotes aren't written to IDML yet: the endnote frame keeps their text
                        // (numbers included) and the references are left out.
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
                // Tracked changes wrap the text they mark.
                if let Some(kind) = change_type(f.over.change) {
                    for c in csrs.iter_mut().filter(|c| c.name == "CharacterStyleRange" && !c.children.is_empty()) {
                        let mut ch = El::new("Change").attr("Self", self.fresh()).attr("ChangeType", kind);
                        ch.children = std::mem::take(&mut c.children);
                        c.push(ch);
                    }
                }
                if para_br && let Some(c) = csrs.last_mut() {
                    let nl = s.runs().find(|(rr, _)| rr.contains(&r.end)).and_then(|(_, nf)| change_type(nf.over.change));
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
                let link = links.iter().find(|(_, l)| l.contains(&seg_start) || (l.start == seg_start && l.start == l.end)).map(|(i, _)| *i);
                for c in csrs {
                    psr_csrs.push((link, c));
                }
            }
            // Consecutive ranges of one link go inside one `HyperlinkTextSource`.
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
            paras.push(psr);
        }
        paras
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
