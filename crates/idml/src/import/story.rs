use super::text_attrs::{ace_char, clear_story_drop_cap_styles, inherits_drop_cap_style};
use super::*;

impl<'r> Importer<'r> {
    // ---------- stories ----------

    pub(super) fn story(&mut self, id: StoryId, e: &El) -> Story {
        self.story_with_para(id, e, ParaFormat::default())
    }

    pub(super) fn story_with_para(&mut self, id: StoryId, e: &El, default_para: ParaFormat) -> Story {
        let mut b = StoryBuilder {
            text: String::new(),
            paras: vec![default_para.clone()],
            runs: Vec::new(),
            fresh: true,
            last: CharFormat::default(),
            tables: Vec::new(),
            after_table: None,
            notes: Vec::new(),
            anchors: Vec::new(),
            xrefs: Vec::new(),
            link_srcs: Vec::new(),
            index_refs: Vec::new(),
            objects: Vec::new(),
        };
        self.walk_story(e, &mut b, &default_para, &CharAttrs::default(), &CharFormat::default(), None);
        let StoryBuilder { text, mut paras, runs, last, tables: tbls, notes, anchors, xrefs, index_refs, objects, link_srcs, .. } = b;
        for (me, start, end) in link_srcs {
            // A cell's or a footnote's story is built under the `StoryId(0)` placeholder, so a
            // range recorded against it names no story of the document (noted in review).
            if id.0 == 0 {
                self.nested_link_srcs.insert(me);
            } else {
                self.link_text_srcs.insert(me, (id, start, end));
            }
        }
        let mut chars: Vec<CharRun> = runs.into_iter().filter(|r| r.len > 0).collect();
        if chars.is_empty() {
            chars.push(CharRun { len: 0, format: last });
        }
        let mut tables = BTreeMap::new();
        for (pi, t) in tbls {
            if let Some(p) = paras.get_mut(pi) {
                p.table = Some(t.id);
                tables.insert(t.id, Arc::new(t));
            }
        }
        let notes = notes.into_iter().enumerate().map(|(i, text)| Arc::new(designcraft_doc::Footnote { id: i as u64 + 1, text })).collect();
        let mut anchor_list = Vec::new();
        for (k, (me, name)) in anchors.into_iter().enumerate() {
            let aid = (id.0 << 24) | (k as u64 + 1);
            // Only a document story's anchors can be reached. A cell's or a footnote's story is
            // built under `StoryId(0)`, so its anchors take ids from the same low range as each
            // other and belong to no story `Document::stories` or the cross-reference index
            // holds -- a link to one resolved to an id nothing owns, and the PDF then left the
            // annotation out without saying so (noted in review).
            if id.0 == 0 {
                self.nested_anchors.insert(me);
            } else {
                self.anchor_ids.insert(me, aid);
            }
            anchor_list.push(Arc::new(designcraft_doc::TextAnchor { id: aid, name }));
        }
        let mut xref_list = Vec::new();
        for (k, (me, fmt)) in xrefs.into_iter().enumerate() {
            self.xref_srcs.insert(me, (id, k));
            let format = self.xref_format_names.get(&fmt).cloned().unwrap_or_else(|| "Full Paragraph & Page Number".into());
            xref_list.push(Arc::new(designcraft_doc::CrossRef { target: 0, format }));
        }
        let pref = e.find("StoryPreference");
        let optical_margin = designcraft_doc::OpticalMargin {
            on: pref.and_then(|p| p.boolean("OpticalMarginAlignment")).unwrap_or(false),
            size: pref
                .and_then(|p| p.num("OpticalMarginSize"))
                .and_then(designcraft_doc::OpticalMargin::clamp_size)
                .unwrap_or(designcraft_doc::OpticalMargin::DEFAULT_SIZE),
        };
        let mut st = Story {
            id,
            text,
            paras,
            chars,
            frames: vec![],
            direction: if e.find("StoryPreference").and_then(|p| p.get("StoryDirection")) == Some("RightToLeftDirection") {
                designcraft_doc::TextDirection::RightToLeft
            } else {
                designcraft_doc::TextDirection::LeftToRight
            },
            vertical: e.find("StoryPreference").and_then(|p| p.get("StoryOrientation")) == Some("Vertical"),
            optical_margin,
            rev: 0,
            tables,
            notes,
            endnotes: Vec::new(),
            anchors: anchor_list,
            xrefs: xref_list,
            index_refs: index_refs.into_iter().map(Arc::new).collect(),
            objects: objects.into_iter().map(Arc::new).collect(),
            editorial: Vec::new(),
            link: None,
            file_link: None,
        };
        st.fix_notes();
        st.fix_marks();
        // Drop anything inconsistent (e.g. a table sharing a paragraph with another).
        if st.check().is_err() {
            st.tables.retain(|_, _| false);
            for p in &mut st.paras {
                p.table = None;
            }
        }
        st
    }

    /// The format of the text in a `CharacterStyleRange`: its style, and its local attributes over
    /// the enclosing paragraph range's, inside a tracked change if `outer` (the enclosing format) is.
    fn range_format(&mut self, c: &El, pchars: &CharAttrs, outer: &CharFormat) -> CharFormat {
        let style = c.get("AppliedCharacterStyle").map(|r| self.char_style_ref(r)).unwrap_or_else(|| st::NO_CHAR_STYLE.into());
        let mut over = pchars.clone();
        over.merge(&self.char_attrs(c));
        over.change = over.change.or(outer.over.change);
        CharFormat { style, over }
    }

    fn walk_story(&mut self, e: &El, b: &mut StoryBuilder, pf: &ParaFormat, pchars: &CharAttrs, cf: &CharFormat, brk: Option<&str>) {
        for n in &e.children {
            match n {
                Node::El(c) => match c.local() {
                    "ParagraphStyleRange" => {
                        let style = c.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).unwrap_or_else(|| pf.style.clone());
                        let para = self.para_attrs(c);
                        let mut chars = self.char_attrs(c);
                        // A paragraph range inside a tracked change is part of that change.
                        chars.change = chars.change.or(cf.over.change);
                        let npf = ParaFormat { style, para, chars: CharAttrs::default(), table: None };
                        if b.fresh
                            && let Some(last) = b.paras.last_mut()
                        {
                            *last = npf.clone();
                        }
                        // Text and markers outside any character range (a destination or a
                        // cross-reference source at paragraph level) take the paragraph's format.
                        let pcf = CharFormat { style: st::NO_CHAR_STYLE.into(), over: chars.clone() };
                        self.walk_story(c, b, &npf, &chars, &pcf, None);
                    }
                    "CharacterStyleRange" => {
                        let ncf = self.range_format(c, pchars, cf);
                        let bt = c.get("ParagraphBreakType").filter(|t| *t != "Anywhere");
                        b.last = ncf.clone();
                        self.walk_story(c, b, pf, pchars, &ncf, bt);
                    }
                    "Content" => {
                        for t in &c.children {
                            match t {
                                Node::Text(s) | Node::CData(s) => {
                                    let s: String = s
                                        .chars()
                                        .filter(|c| *c != '\u{FEFF}')
                                        .map(|c| if c == '\n' || c == '\r' || c == '\u{2029}' { st::FORCED_LINE_BREAK } else { c })
                                        .collect();
                                    b.push(&s, cf);
                                }
                                Node::Pi(t, v) if t == "ACE" => {
                                    if let Some(ch) = ace_char(v) {
                                        b.push(&ch.to_string(), cf);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "Br" => {
                        // A column, frame or page break ends its paragraph like a return does.
                        if let Some(t) = brk {
                            let ch = match t {
                                "NextColumn" => st::COLUMN_BREAK,
                                "NextFrame" => st::FRAME_BREAK,
                                "NextOddPage" => st::ODD_PAGE_BREAK,
                                "NextEvenPage" => st::EVEN_PAGE_BREAK,
                                _ => st::PAGE_BREAK,
                            };
                            b.push(&ch.to_string(), cf);
                        }
                        b.push("\n", cf);
                        b.paras.push(pf.clone());
                        b.fresh = true;
                    }
                    "Table" => {
                        let t = self.table(c);
                        b.push_table(t, pf, cf);
                    }
                    // The source wraps the text the link covers: its children are ordinary ranges
                    // and content, so the range is where the text stood before and after (#1446).
                    "HyperlinkTextSource" => {
                        let start = b.text.len();
                        self.walk_story(c, b, pf, pchars, cf, brk);
                        if b.text.len() > start {
                            b.link_srcs.push((c.get("Self").unwrap_or("").to_string(), start, b.text.len()));
                        }
                    }
                    "HyperlinkTextDestination" | "ParagraphDestination" => {
                        b.anchors.push((c.get("Self").unwrap_or("").to_string(), c.get("Name").unwrap_or("Anchor").to_string()));
                        b.push(&designcraft_doc::ANCHOR_MARK.to_string(), cf);
                    }
                    "PageReference" => {
                        use designcraft_doc::index::IndexRange as R;
                        let topic = c.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).cloned().unwrap_or_default();
                        let range = match c.get("PageReferenceType") {
                            Some("ToEndOfStory") | Some("ToEndOfDocument") | Some("ToEndOfSection") => R::ToEndOfStory,
                            Some("ForNextNParagraphs") => R::NextParagraphs(c.num("PageReferenceLimit").unwrap_or(1.0).max(1.0) as u32),
                            Some("SuppressPageNumbers") => R::SuppressPageRange,
                            _ => R::CurrentPage,
                        };
                        b.index_refs.push(designcraft_doc::IndexRef { topics: topic.path, sort: topic.sort, range });
                        b.push(&designcraft_doc::INDEX_MARK.to_string(), cf);
                    }
                    "CrossReferenceSource" => {
                        // The generated text is regenerated from the format.
                        b.xrefs.push((c.get("Self").unwrap_or("").to_string(), c.get("AppliedFormat").unwrap_or("").to_string()));
                        // A source at paragraph level wraps a range; inside a range it holds the
                        // text directly and takes that range's format.
                        let f = c.find("CharacterStyleRange").map_or_else(|| cf.clone(), |r| self.range_format(r, pchars, cf));
                        b.push(&designcraft_doc::XREF_MARK.to_string(), &f);
                    }
                    "Footnote" => {
                        // The text starts with the number marker (ACE 4, dropped) and the separator.
                        let mut note = self.story(StoryId(0), c);
                        let sep = self.footnote_options.separator.clone();
                        if !sep.is_empty() && note.text.starts_with(&sep) {
                            note.delete(0..sep.len());
                        }
                        b.notes.push(note);
                        // The reference's position comes from the footnote options.
                        let mut rcf = cf.clone();
                        rcf.over.position = None;
                        b.push(&designcraft_doc::FOOTNOTE_REF.to_string(), &rcf);
                    }
                    // Anchored page items, including groups containing text frames.
                    "Rectangle" | "Oval" | "Polygon" | "GraphicLine" | "Group" | "TextFrame" => {
                        if let Some(item) = self.item(c, designcraft_geom::Affine::IDENTITY) {
                            use designcraft_doc::anchored::AnchorAlign as A;
                            // Each setting the item leaves unset comes from its object style chain,
                            // then from the IDML defaults.
                            let levels: Vec<&El> = std::iter::once(c).chain(self.object_style_chain(c)).collect();
                            let s = |k: &str| {
                                let set = levels.iter().filter_map(|l| l.find("AnchoredObjectSetting")).find_map(|x| x.get(k));
                                set.unwrap_or(match k {
                                    "AnchoredPosition" => "InlinePosition",
                                    "AnchorPoint" => "BottomRightAnchor",
                                    "HorizontalAlignment" => "LeftAlign",
                                    "HorizontalReferencePoint" => "TextFrame",
                                    "VerticalAlignment" => "BottomAlign",
                                    "VerticalReferencePoint" => "LineBaseline",
                                    "PinPosition" => "true",
                                    _ => "",
                                })
                            };
                            let get = |k: &str| s(k).trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0);
                            let position = match s("AnchoredPosition") {
                                "AboveLine" => designcraft_doc::AnchorPosition::AboveLine {
                                    align: match s("HorizontalAlignment") {
                                        "CenterAlign" => A::Center,
                                        "RightAlign" => A::Right,
                                        _ => A::Left,
                                    },
                                    space_before: get("AnchorSpaceAbove"),
                                    space_after: get("AnchorYoffset"),
                                },
                                "Anchored" => {
                                    let col = match s("HorizontalAlignment") {
                                        "CenterAlign" => 1,
                                        "RightAlign" => 2,
                                        _ => 0,
                                    };
                                    let row = match s("VerticalAlignment") {
                                        "CenterAlign" => 1,
                                        "BottomAlign" => 2,
                                        _ => 0,
                                    };
                                    designcraft_doc::AnchorPosition::Custom {
                                        x_relative: names::anchor_rel_in(s("HorizontalReferencePoint")),
                                        y_relative: names::anchor_rel_in(s("VerticalReferencePoint")),
                                        // IDML measures a left-aligned object's X offset leftwards.
                                        x_offset: if col == 0 { 0.0 - get("AnchorXoffset") } else { get("AnchorXoffset") },
                                        y_offset: get("AnchorYoffset"),
                                        object_point: names::anchor_in(s("AnchorPoint")),
                                        ref_point: row * 3 + col,
                                        keep_within_column: s("PinPosition") == "true",
                                        spine_relative: s("SpineRelative") == "true",
                                    }
                                }
                                _ => designcraft_doc::AnchorPosition::Inline { y_offset: get("AnchorYoffset") },
                            };
                            b.objects.push(designcraft_doc::AnchoredObject::new(item, position));
                            b.push(&designcraft_doc::OBJECT_MARK.to_string(), cf);
                        }
                    }
                    "TextVariableInstance" => {
                        let t = self.text_variable_instance(c);
                        b.push(&t, cf);
                    }
                    "Change" => {
                        // Tracked changes: deleted text stays in the story, marked, and takes no
                        // room in the layout.
                        let mut ncf = cf.clone();
                        match c.get("ChangeType") {
                            Some("DeletedText") => ncf.over.change = Some(designcraft_doc::ChangeMark::Deleted),
                            Some("InsertedText" | "MovedText") => ncf.over.change = Some(designcraft_doc::ChangeMark::Inserted),
                            _ => {}
                        }
                        self.walk_story(c, b, pf, pchars, &ncf, brk);
                    }
                    "Properties" | "Note" | "StoryPreference" | "InCopyExportOption" => {}
                    _ => self.walk_story(c, b, pf, pchars, cf, brk),
                },
                Node::Pi(t, v) if t == "ACE" => {
                    if let Some(ch) = ace_char(v) {
                        b.push(&ch.to_string(), cf);
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn link_threads(&mut self) {
        let threads = std::mem::take(&mut self.ctx.threads);
        let by_self: HashMap<String, ItemId> = self.item_ids.clone();
        let mut per_story: BTreeMap<StoryId, Vec<usize>> = BTreeMap::new();
        for (i, (id, story, _, _)) in threads.iter().enumerate() {
            let sid = self.story_ids.get(story).copied().or_else(|| self.frame_story(*id));
            if let Some(sid) = sid {
                per_story.entry(sid).or_default().push(i);
            }
        }
        for (sid, idx) in per_story {
            let members: HashSet<ItemId> = idx.iter().map(|i| threads[*i].0).collect();
            let next_of: HashMap<ItemId, ItemId> =
                idx.iter().filter_map(|i| by_self.get(&threads[*i].3).filter(|n| members.contains(n)).map(|n| (threads[*i].0, *n))).collect();
            let has_prev: HashSet<ItemId> = next_of.values().copied().collect();
            let mut order = Vec::new();
            let mut seen = HashSet::new();
            for i in &idx {
                let head = threads[*i].0;
                if has_prev.contains(&head) || seen.contains(&head) {
                    continue;
                }
                let mut cur = Some(head);
                while let Some(c) = cur {
                    if !seen.insert(c) {
                        break;
                    }
                    order.push(c);
                    cur = next_of.get(&c).copied();
                }
            }
            for i in &idx {
                let f = threads[*i].0;
                if seen.insert(f) {
                    order.push(f);
                }
            }
            if let Some(s) = self.stories.get_mut(&sid) {
                s.frames = order;
            }
        }
    }

    fn frame_story(&self, id: ItemId) -> Option<StoryId> {
        fn find(items: &[Arc<Item>], id: ItemId) -> Option<StoryId> {
            for it in items {
                let mut r = None;
                it.walk(&mut |i| {
                    if i.id == id {
                        r = i.text_frame().map(|t| t.story);
                    }
                });
                if r.is_some() {
                    return r;
                }
            }
            None
        }
        self.spreads.iter().chain(self.parents.iter()).find_map(|s| find(&s.items, id))
    }

    /// A nested style list replaces the one a style or paragraph inherits, and with it the drop
    /// cap style (the list's leading "through 1 Dropcap" entry, read into the drop cap style). A
    /// list of its own without that entry clears an inherited drop cap style.
    pub(super) fn clear_inherited_drop_cap_styles(&mut self) {
        let clears = |a: &designcraft_doc::ParaAttrs| a.drop_cap_style.is_none() && a.nested_styles.is_some();
        let cleared: Vec<String> = self
            .styles
            .paragraph
            .iter()
            .filter(|s| clears(&s.para) && inherits_drop_cap_style(&self.styles, s.based_on.as_deref()))
            .map(|s| s.name.clone())
            .collect();
        for name in cleared {
            if let Some(s) = self.styles.para_mut(&name) {
                s.para.drop_cap_style = Some(st::NO_CHAR_STYLE.into());
            }
        }
        for story in self.stories.values_mut() {
            clear_story_drop_cap_styles(&self.styles, story, 0);
        }
    }
}

struct StoryBuilder {
    text: String,
    paras: Vec<ParaFormat>,
    runs: Vec<CharRun>,
    /// The current paragraph has no content yet (its format can still be set by a range).
    fresh: bool,
    last: CharFormat,
    /// Tables by anchor paragraph index.
    tables: Vec<(usize, Table)>,
    /// Content after a table anchor starts a new paragraph (with this format).
    after_table: Option<ParaFormat>,
    /// Footnote texts in reference order.
    notes: Vec<Story>,
    /// Text destinations (Self, name) and cross-reference sources (Self, format Self) in order.
    anchors: Vec<(String, String)>,
    index_refs: Vec<designcraft_doc::IndexRef>,
    xrefs: Vec<(String, String)>,
    /// `HyperlinkTextSource` Self and the byte range of the text it wrapped (#1446).
    link_srcs: Vec<(String, usize, usize)>,
    objects: Vec<designcraft_doc::AnchoredObject>,
}

impl StoryBuilder {
    /// A table gets an anchor paragraph of its own.
    fn push_table(&mut self, t: Table, pf: &ParaFormat, cf: &CharFormat) {
        let para_empty = self.text.rsplit('\n').next().is_none_or(str::is_empty);
        if !para_empty {
            self.after_table = None;
            self.push("\n", cf);
            self.paras.push(pf.clone());
        }
        self.after_table = None;
        self.push(&st::TABLE_ANCHOR.to_string(), cf);
        self.tables.push((self.paras.len() - 1, t));
        self.after_table = Some(pf.clone());
        self.fresh = false;
    }

    fn push(&mut self, s: &str, f: &CharFormat) {
        if s.is_empty() {
            return;
        }
        if let Some(pf) = self.after_table.take()
            && !s.starts_with('\n')
        {
            self.push("\n", f);
            self.paras.push(pf);
        }
        self.text.push_str(s);
        if !s.contains('\n') || s.len() > 1 {
            self.fresh = false;
        }
        match self.runs.last_mut() {
            Some(r) if r.format == *f => r.len += s.len(),
            _ => self.runs.push(CharRun { len: s.len(), format: f.clone() }),
        }
    }
}

/// Deepest table-in-cell or note nesting the drop cap pass walks into.
pub(super) const MAX_STORY_NESTING: usize = 32;
