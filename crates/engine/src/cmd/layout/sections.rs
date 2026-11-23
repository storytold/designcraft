use super::{Result, Session, Value, bad, json};

/// A start page number parameter: a whole number in 1..=[`designcraft_doc::MAX_PAGE_NUMBER`].
pub(super) fn start_number(cmd: &str, key: &str, v: &Value) -> Result<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| (1..=designcraft_doc::MAX_PAGE_NUMBER).contains(n))
        .ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number from 1 to {}", designcraft_doc::MAX_PAGE_NUMBER)))
}

/// The largest chapter number (as for page numbers).
const MAX_CHAPTER: u64 = designcraft_doc::MAX_PAGE_NUMBER as u64;

/// A `layout.section` edit, checked before the document changes.
pub(super) struct SectionEdit {
    /// Absolute index of the section's first page.
    start: usize,
    remove: bool,
    /// Some(None): continue from the previous section.
    start_number: Option<Option<u32>>,
    style: Option<designcraft_doc::NumberStyle>,
    prefix: Option<String>,
    include_prefix: Option<bool>,
    marker: Option<String>,
}

impl SectionEdit {
    pub(super) fn parse(cmd: &str, s: &Session, p: &Value) -> Result<Self> {
        let page = p.get("page").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "missing page"))?;
        let n = s.doc()?.doc.page_count() as u64;
        if page == 0 || page > n {
            return Err(bad(cmd, format!("no page {page}")));
        }
        let prefix = p.get("prefix").and_then(Value::as_str).map(str::to_string);
        if prefix.as_ref().is_some_and(|x| x.chars().count() > 8 || x.contains(['+', ','])) {
            return Err(bad(cmd, "section prefix: up to 8 characters, without + or ,"));
        }
        let start_number = match p.get("startNumber") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => Some(Some(start_number(cmd, "startNumber", v)?)),
        };
        let style = match p.get("style") {
            None => None,
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(designcraft_doc::NumberStyle::Symbols) => return Err(bad(cmd, "symbols don't number pages")),
                Ok(style) => Some(style),
                Err(e) => return Err(bad(cmd, e.to_string())),
            },
        };
        Ok(SectionEdit {
            start: (page - 1) as usize,
            remove: p.get("remove").and_then(Value::as_bool).unwrap_or(false),
            start_number,
            style,
            prefix,
            include_prefix: p.get("includePrefix").and_then(Value::as_bool),
            marker: p.get("marker").and_then(Value::as_str).map(str::to_string),
        })
    }

    pub(super) fn planned_sections(&self, current: &[designcraft_doc::Section]) -> Vec<designcraft_doc::Section> {
        let mut sections = current.to_vec();
        let start = self.start;
        // Page 1 always starts a section.
        if self.remove && start != 0 {
            // A section that starts an alternate layout stays (the layout needs it); it just
            // continues the previous section's numbers.
            if let Some(sec) = sections.iter_mut().find(|x| x.start == start && x.layout.is_some()) {
                sec.start_number = None;
            } else {
                sections.retain(|x| x.start != start);
            }
            // The pages after it follow the previous section's numbers again.
            return sections;
        }
        let mut sec = sections.iter().find(|x| x.start == start).cloned().unwrap_or(designcraft_doc::Section {
            start,
            start_number: None,
            style: Default::default(),
            prefix: String::new(),
            marker: String::new(),
            layout: None,
            include_prefix: false,
        });
        if let Some(n) = self.start_number {
            sec.start_number = n;
        }
        if let Some(style) = self.style {
            sec.style = style;
        }
        if let Some(prefix) = &self.prefix {
            sec.prefix = prefix.clone();
        }
        if let Some(on) = self.include_prefix {
            sec.include_prefix = on;
        }
        if let Some(marker) = &self.marker {
            sec.marker = marker.clone();
        }
        sections.retain(|x| x.start != start);
        sections.push(sec);
        sections.sort_by_key(|x| x.start);
        sections
    }

    pub(super) fn apply(&self, d: &mut designcraft_doc::Document) {
        d.sections = self.planned_sections(&d.sections);
        // Real edits retain the existing reflow, including chapter-only combined edits.
        d.repaginate();
    }
}

/// A `layout.chapterNumbering` edit, checked before the document changes.
pub(super) struct ChapterEdit {
    style: Option<designcraft_doc::NumberStyle>,
    source: Option<designcraft_doc::ChapterSource>,
    start: Option<u32>,
}

impl ChapterEdit {
    pub(super) fn parse(cmd: &str, p: &Value) -> Result<Self> {
        let style = match p.get("style") {
            Some(v) => match serde_json::from_value::<designcraft_doc::NumberStyle>(v.clone()) {
                Ok(designcraft_doc::NumberStyle::Symbols) | Err(_) => return Err(bad(cmd, format!("unknown chapter style {v}"))),
                Ok(style) => Some(style),
            },
            None => None,
        };
        let mut source = match p.get("source") {
            Some(v) => Some(
                serde_json::from_value::<designcraft_doc::ChapterSource>(v.clone())
                    .map_err(|_| bad(cmd, format!("unknown chapter number source {v}")))?,
            ),
            None => None,
        };
        let start = match p.get("start") {
            Some(v) => {
                Some(v.as_u64().filter(|n| (1..=MAX_CHAPTER).contains(n)).ok_or_else(|| bad(cmd, format!("chapter start: 1 to {MAX_CHAPTER}")))?
                    as u32)
            }
            None => None,
        };
        // A start number alone means Start Chapter Numbering at.
        if start.is_some() && source.is_none() {
            source = Some(designcraft_doc::ChapterSource::UserDefined);
        }
        Ok(ChapterEdit { style, source, start })
    }

    pub(super) fn changes(&self, d: &designcraft_doc::Document) -> bool {
        // Stored values matter even when the current mode/labels do not expose them.
        self.style.is_some_and(|style| style != d.settings.chapter_style)
            || self.source.is_some_and(|source| source != d.settings.chapter_source)
            || self.start.is_some_and(|start| start != d.settings.chapter_number)
    }

    pub(super) fn apply(&self, d: &mut designcraft_doc::Document) {
        if let Some(style) = self.style {
            d.settings.chapter_style = style;
        }
        if let Some(source) = self.source {
            d.settings.chapter_source = source;
        }
        if let Some(n) = self.start {
            d.settings.chapter_number = n;
        }
    }
}

/// The active document's chapter numbering, and the open book that holds it (what
/// `layout.chapterNumbering {}` reports, read without running a command).
pub fn chapter_numbering(s: &Session) -> Result<Value> {
    let st = s.doc()?;
    let set = &st.doc.settings;
    let book = s
        .book
        .as_ref()
        .filter(|b| st.path.as_deref().is_some_and(|p| b.documents.iter().any(|d| std::path::Path::new(d) == std::path::Path::new(p))))
        .map(|b| std::path::Path::new(&b.path).file_stem().map_or_else(|| b.path.clone(), |n| n.to_string_lossy().to_string()));
    Ok(
        json!({"chapterNumber": set.chapter_number.max(1), "label": st.doc.chapter_label(), "style": set.chapter_style, "source": set.chapter_source, "book": book}),
    )
}

#[cfg(test)]
mod section_side_tests {
    use serde_json::json;

    use crate::Session;

    /// Each spread's pages as (name, side), side `L`, `R` or `S`.
    fn spreads(s: &Session) -> Vec<Vec<(String, char)>> {
        let d = &s.doc().unwrap().doc;
        let mut abs = 0;
        d.spreads
            .iter()
            .map(|sp| {
                sp.pages
                    .iter()
                    .map(|p| {
                        let side = match p.side {
                            designcraft_doc::PageSide::Left => 'L',
                            designcraft_doc::PageSide::Right => 'R',
                            designcraft_doc::PageSide::Single => 'S',
                        };
                        abs += 1;
                        (d.page_name(abs - 1), side)
                    })
                    .collect()
            })
            .collect()
    }

    fn pages(v: &[&[(&str, char)]]) -> Vec<Vec<(String, char)>> {
        v.iter().map(|sp| sp.iter().map(|(n, c)| (n.to_string(), *c)).collect()).collect()
    }

    #[test]
    fn a_section_starting_at_one_on_a_left_page_moves_it_to_the_right() {
        // Chris Nielsen's "Numbering Page Sections" (7:03): with the spread 8–9 selected, the
        // section starts on page 8, which becomes a lone right-hand page 1; the pages after it
        // shuffle and their objects go with them. Undo puts them back.
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 612, "height": 792, "facingPages": true, "pages": 10})).unwrap();
        let id = s.execute("frame.create", &json!({"spread": 4, "rect": [700, 100, 800, 200], "content": "text", "text": "TOPIC"})).unwrap()["id"]
            .as_u64()
            .unwrap();
        let page_of = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            d.page_of_item(designcraft_doc::ItemId(id)).unwrap()
        };
        assert_eq!(page_of(&s), 8, "page 9, the right page of the spread 8–9");
        let before = spreads(&s);
        s.execute("layout.section", &json!({"page": 8, "startNumber": 1})).unwrap();
        assert_eq!(
            spreads(&s),
            pages(&[
                &[("1", 'R')],
                &[("2", 'L'), ("3", 'R')],
                &[("4", 'L'), ("5", 'R')],
                &[("6", 'L'), ("7", 'R')],
                &[("1", 'R')],
                &[("2", 'L'), ("3", 'R')],
            ])
        );
        assert_eq!(page_of(&s), 8, "the object stays on its page, now the left page of a spread");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(spreads(&s), before);
        // On a right page (the video's page 9) nothing moves; an even start on a right page moves
        // it to the left.
        s.execute("layout.section", &json!({"page": 9, "startNumber": 1})).unwrap();
        assert_eq!(spreads(&s)[4], pages(&[&[("8", 'L'), ("1", 'R')]])[0]);
        s.execute("layout.section", &json!({"page": 9, "startNumber": 2})).unwrap();
        assert_eq!(spreads(&s)[4..], pages(&[&[("8", 'L')], &[("2", 'L'), ("3", 'R')]]));
        // Removing the section numbers the pages on from 9 again.
        s.execute("layout.section", &json!({"page": 9, "remove": true})).unwrap();
        assert_eq!(spreads(&s), before);
    }

    #[test]
    fn single_sided_documents_keep_their_pages() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"facingPages": false, "pages": 4})).unwrap();
        s.execute("layout.section", &json!({"page": 2, "startNumber": 1})).unwrap();
        let sp = spreads(&s);
        assert_eq!(sp.len(), 4);
        assert!(sp.iter().all(|p| p.len() == 1 && p[0].1 == 'S'), "{sp:?}");
    }
}


#[cfg(test)]
mod numbering_noop_guard_tests {
    use super::{Value, json};
    use crate::{DocState, HistoryEntry, Session};
    use std::sync::Arc;

    type Entries = Vec<(String, Arc<designcraft_doc::Document>, designcraft_doc::Selection)>;
    fn entries(items: &[HistoryEntry]) -> Entries {
        items.iter().map(|item| (item.label.clone(), item.doc.clone(), item.selection.clone())).collect()
    }
    fn fresh() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "width": 612, "height": 792, "facingPages": true})).unwrap();
        assert!(!s.doc().unwrap().is_dirty());
        s
    }
    fn snapshot(s: &Session) -> (DocState, Vec<(String, Value)>) {
        (s.doc().unwrap().clone(), s.journal.clone())
    }
    fn preserved(s: &Session, before: &(DocState, Vec<(String, Value)>), attempt: Option<&Value>) {
        let now = s.doc().unwrap();
        let old = &before.0;
        assert_eq!(now.doc.as_ref(), old.doc.as_ref(), "ALL document fields, IDs, next_id and geometry remain exact");
        assert!(Arc::ptr_eq(&now.doc, &old.doc), "no-op/refusal cannot replace the live Arc");
        assert!(Arc::ptr_eq(&now.saved_doc, &old.saved_doc));
        assert_eq!(now.saved_doc.as_ref(), old.saved_doc.as_ref());
        assert_eq!((now.uid, now.identity_epoch, now.revision, now.saved_revision, now.is_dirty()), (old.uid, old.identity_epoch, old.revision, old.saved_revision, old.is_dirty()));
        assert_eq!(now.selection, old.selection);
        assert_eq!(entries(&now.history.undo), entries(&old.history.undo));
        assert_eq!(entries(&now.history.redo), entries(&old.history.redo));
        assert_eq!(now.history.limit, old.history.limit);
        assert_eq!(now.path, old.path);
        assert_eq!(now.preview_record, old.preview_record);
        assert_eq!(now.preview_stash.as_deref(), old.preview_stash.as_deref());
        if let (Some(now), Some(old)) = (&now.preview_stash, &old.preview_stash) { assert!(Arc::ptr_eq(now, old)); }
        assert_eq!(now.active_layer, old.active_layer);
        assert_eq!(now.editing_parents, old.editing_parents);
        let mut journal = before.1.clone();
        if let Some(params) = attempt { journal.push(("layout.numberingOptions".into(), params.clone())); }
        assert_eq!(s.journal, journal, "successful no-ops journal attempts; refused commands do not");
    }
    fn one_edit(s: &Session, before: &(DocState, Vec<(String, Value)>), params: &Value) {
        let now = s.doc().unwrap();
        let old = &before.0;
        assert_ne!(now.doc.as_ref(), old.doc.as_ref());
        assert!(!Arc::ptr_eq(&now.doc, &old.doc));
        assert_ne!(now.doc.spreads.iter().map(|spread| spread.id).collect::<Vec<_>>(), old.doc.spreads.iter().map(|spread| spread.id).collect::<Vec<_>>(), "real combined edits still run the original repagination, even for chapter-only changes");
        assert!(now.revision > old.revision, "a fresh process-global revision, not a guessed +1");
        assert_eq!((now.uid, now.identity_epoch, now.saved_revision), (old.uid, old.identity_epoch, old.saved_revision));
        assert!(Arc::ptr_eq(&now.saved_doc, &old.saved_doc));
        assert!(now.is_dirty());
        assert_eq!(now.history.undo.len(), old.history.undo.len() + 1);
        assert!(now.history.redo.is_empty());
        assert_eq!(entries(&now.history.undo[..old.history.undo.len()]), entries(&old.history.undo));
        let undo = now.history.undo.last().unwrap();
        assert_eq!(undo.label, "Apply Numbering & Section Options");
        assert_eq!(undo.doc.as_ref(), old.doc.as_ref());
        assert_eq!(undo.selection, old.selection);
        assert_eq!(now.selection, old.selection);
        let mut journal = before.1.clone();
        journal.push(("layout.numberingOptions".into(), params.clone()));
        assert_eq!(s.journal, journal);
    }

    #[test]
    fn clean_noops_preserve_full_identity_saved_state_and_normal_attempt_journal() {
        let mut s = fresh();
        for params in [
            json!({"page": 1}),
            json!({"page": 1, "startNumber": 1, "style": "arabic", "prefix": "", "marker": "", "includePrefix": false,
                "chapter": {"source": "automatic", "style": "arabic", "start": 1}}),
            json!({"page": 1, "remove": true}),
            json!({"page": 3, "remove": true}),
            json!({"page": 1, "chapter": {}}),
            json!({"page": 1, "chapter": null}),
        ] {
            let before = snapshot(&s);
            let result = s.execute("layout.numberingOptions", &params).unwrap();
            assert_eq!(result, json!({"names": ["1", "2", "3", "4"], "chapter": "1"}));
            preserved(&s, &before, Some(&params));
            assert!(!s.doc().unwrap().is_dirty());
        }
    }

    #[test]
    fn dirty_noops_preserve_nonempty_selection_and_complete_undo_and_redo_entries() {
        let mut s = fresh();
        let id = s.execute("frame.create", &json!({"rect": [10, 10, 40, 40]})).unwrap()["id"].as_u64().unwrap();
        s.execute("frame.create", &json!({"rect": [50, 50, 80, 80]})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        assert!(s.doc().unwrap().is_dirty());
        assert!(!s.doc().unwrap().history.undo.is_empty());
        assert!(!s.doc().unwrap().history.redo.is_empty());
        assert_eq!(s.doc().unwrap().selection.items, [designcraft_doc::ItemId(id)]);
        let params = json!({"page": 1, "startNumber": 1, "chapter": {"source": "automatic"}});
        let before = snapshot(&s);
        s.execute("layout.numberingOptions", &params).unwrap();
        preserved(&s, &before, Some(&params));
        // Redo is still the real previously undone frame edit, not a length-only placeholder.
        let expected = s.doc().unwrap().history.redo.last().unwrap().clone();
        let revision = s.doc().unwrap().revision;
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.as_ref(), expected.doc.as_ref());
        assert_eq!(s.doc().unwrap().selection, expected.selection);
        assert!(s.doc().unwrap().revision > revision);
    }

    #[test]
    fn stored_values_modes_and_new_section_targets_are_real_edits_even_with_equal_labels() {
        for params in [
            json!({"page": 1, "prefix": "A"}), // hidden while includePrefix is false
            json!({"page": 1, "marker": "bookmark"}),
            json!({"page": 1, "startNumber": null}), // explicit one -> Automatic
            json!({"page": 1, "chapter": {"source": "sameAsPrevious"}}),
            json!({"page": 1, "chapter": {"source": "automatic", "start": 7}}),
            json!({"page": 1, "chapter": {"style": "upperRoman"}}),
            json!({"page": 3}), // create a continuing section, though its page labels match
            json!({"page": 3, "startNumber": 1}),
        ] {
            let mut s = fresh();
            let before = snapshot(&s);
            s.execute("layout.numberingOptions", &params).unwrap();
            one_edit(&s, &before, &params);
            let changed = snapshot(&s);
            // Repeating the exact typed plan must now be a no-op, including latent fields.
            s.execute("layout.numberingOptions", &params).unwrap();
            preserved(&s, &changed, Some(&params));
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.as_ref(), before.0.doc.as_ref());
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.as_ref(), changed.0.doc.as_ref());
        }
    }

    #[test]
    fn refusal_validates_both_parts_before_noop_or_atomic_mutation() {
        let mut s = fresh();
        for params in [
            json!({"page": 1, "chapter": {"start": 100000}}), // otherwise unchanged section
            json!({"page": 3, "startNumber": 5, "chapter": {"start": 100000}}),
            json!({"page": 1, "chapter": {"source": "unknown"}}),
            json!({"page": 1, "chapter": {"style": "symbols"}}),
            json!({"page": 1, "startNumber": 0, "chapter": {"start": 4}}),
            json!({"page": 1, "prefix": "A+B", "chapter": {"start": 4}}),
            json!({"page": 0, "chapter": {"start": 4}}),
            json!({"page": 5, "chapter": {"start": 4}}),
            json!({"page": 1, "style": "symbols", "chapter": {"start": 4}}),
            json!({"page": 3, "remove": true, "chapter": {"start": 0}}),
        ] {
            let before = snapshot(&s);
            assert!(s.execute("layout.numberingOptions", &params).is_err(), "{params}");
            preserved(&s, &before, None);
        }
    }

    #[test]
    fn real_atomic_edits_keep_repagination_object_page_and_full_undo_redo_checkpoints() {
        let mut s = fresh();
        let id = s.execute("frame.create", &json!({"spread": 1, "rect": [700, 100, 800, 200]})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(s.doc().unwrap().doc.page_of_item(designcraft_doc::ItemId(id)), Some(2));
        assert_eq!(s.doc().unwrap().doc.page(1).unwrap().side, designcraft_doc::PageSide::Left);
        let before = snapshot(&s);
        let params = json!({"page": 2, "startNumber": 1, "style": "lowerRoman", "prefix": "A-", "includePrefix": true,
            "chapter": {"source": "userDefined", "start": 4, "style": "upperRoman"}});
        let result = s.execute("layout.numberingOptions", &params).unwrap();
        assert_eq!(result, json!({"names": ["1", "A-i", "A-ii", "A-iii"], "chapter": "IV"}));
        one_edit(&s, &before, &params);
        assert_eq!(s.doc().unwrap().doc.page(1).unwrap().side, designcraft_doc::PageSide::Right);
        assert_eq!(s.doc().unwrap().doc.page_of_item(designcraft_doc::ItemId(id)), Some(2));
        let changed = snapshot(&s);
        s.execute("layout.numberingOptions", &params).unwrap();
        preserved(&s, &changed, Some(&params));
        let revision = s.doc().unwrap().revision;
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.as_ref(), before.0.doc.as_ref());
        assert_eq!(s.doc().unwrap().selection, before.0.selection);
        assert!(s.doc().unwrap().revision > revision);
        assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &before.0.saved_doc));
        assert_eq!((s.doc().unwrap().history.undo.len(), s.doc().unwrap().history.redo.len()), (before.0.history.undo.len(), 1));
        let revision = s.doc().unwrap().revision;
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.as_ref(), changed.0.doc.as_ref());
        assert_eq!(s.doc().unwrap().selection, changed.0.selection);
        assert!(s.doc().unwrap().revision > revision);
        assert_eq!(entries(&s.doc().unwrap().history.undo), entries(&changed.0.history.undo));
        assert_eq!(entries(&s.doc().unwrap().history.redo), entries(&changed.0.history.redo));
    }

    #[test]
    fn removal_plans_keep_alternate_layouts_and_distinguish_missing_and_existing_sections() {
        let mut s = fresh();
        s.execute("layout.section", &json!({"page": 3, "startNumber": 9})).unwrap();
        let params = json!({"page": 3, "remove": true});
        let before = snapshot(&s);
        s.execute("layout.numberingOptions", &params).unwrap();
        one_edit(&s, &before, &params);
        assert!(!s.doc().unwrap().doc.sections.iter().any(|section| section.start == 2));
        let missing = snapshot(&s);
        s.execute("layout.numberingOptions", &params).unwrap();
        preserved(&s, &missing, Some(&params));
        let result = s.execute("layout.createAlternate", &json!({"name": "Wide", "width": 900, "height": 600})).unwrap();
        let page = result["firstPage"].as_u64().unwrap();
        s.execute("layout.section", &json!({"page": page, "startNumber": 7})).unwrap();
        let params = json!({"page": page, "remove": true});
        let before = snapshot(&s);
        s.execute("layout.numberingOptions", &params).unwrap();
        one_edit(&s, &before, &params);
        let section = s.doc().unwrap().doc.sections.iter().find(|section| section.start == (page - 1) as usize).unwrap();
        assert_eq!(section.layout.as_deref(), Some("Wide"));
        assert_eq!(section.start_number, None);
        assert_eq!(s.doc().unwrap().doc.layouts().len(), 2);
        let kept = snapshot(&s);
        // These validated but ignored section fields must not fool the remove-layout plan.
        let params = json!({"page": page, "remove": true, "startNumber": 9, "style": "upperRoman", "prefix": "UNUSED", "marker": "ignored", "includePrefix": true});
        s.execute("layout.numberingOptions", &params).unwrap();
        preserved(&s, &kept, Some(&params));
    }

    fn pending_primary_story() -> Session {
        // Same public native document/story fixture seam used by place_text's autoflow tests.
        // Explicit page breaks make pending flow independent of an incidental line-wrap count.
        let mut doc = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument {
            pages: 1, facing_pages: false, primary_text_frame: true, ..Default::default()
        });
        let story = doc.settings.primary_story.unwrap();
        doc.story_mut(story).unwrap().insert(0, &format!("First{}Second{}Third", designcraft_doc::PAGE_BREAK, designcraft_doc::PAGE_BREAK));
        let mut s = Session::new();
        s.add_document(DocState::new(doc, None));
        s.execute("prefs.set", &json!({"smartTextReflow": true})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
        assert!(!s.doc().unwrap().is_dirty());
        assert!(s.cache.get(&s.doc().unwrap().doc, story, None).overset_at.is_some(), "a legal loaded/pending primary story really needs reflow");
        s
    }

    #[test]
    fn pending_primary_story_noop_preserves_identity_but_real_numbering_change_keeps_postpasses() {
        let mut s = pending_primary_story();
        let before = snapshot(&s);
        let params = json!({"page": 1});
        s.execute("layout.numberingOptions", &params).unwrap();
        preserved(&s, &before, Some(&params));
        let story = s.doc().unwrap().doc.settings.primary_story.unwrap();
        assert!(s.cache.get(&s.doc().unwrap().doc, story, None).overset_at.is_some(), "a no-op does not secretly settle the pending story");
        let real_before = snapshot(&s);
        let params = json!({"page": 1, "startNumber": 2});
        s.execute("layout.numberingOptions", &params).unwrap();
        one_edit(&s, &real_before, &params);
        assert!(s.doc().unwrap().doc.page_count() > 1, "genuine numbering changes still run ordinary smart reflow");
        assert!(s.cache.get(&s.doc().unwrap().doc, story, None).overset_at.is_none());
        let changed = snapshot(&s);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.as_ref(), before.0.doc.as_ref());
        assert_eq!(s.doc().unwrap().selection, before.0.selection);
        assert!(!s.doc().unwrap().is_dirty());
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.as_ref(), changed.0.doc.as_ref());
        assert_eq!(s.doc().unwrap().selection, changed.0.selection);
    }

    #[test]
    fn other_unchanged_commands_retain_their_existing_edit_and_postpass_policy() {
        for (id, params) in [
            ("layout.chapterNumbering", json!({"source": "automatic"})),
            // Its empty-object handler does NOT edit or replace the document Arc.
            // Default success passes must still run, unlike the numbering opt-in.
            ("document.preferences", json!({})),
        ] {
            let mut s = pending_primary_story();
            let before = snapshot(&s);
            s.execute(id, &params).unwrap();
            assert!(s.doc().unwrap().doc.page_count() > 1);
            assert!(!Arc::ptr_eq(&s.doc().unwrap().doc, &before.0.doc));
            assert!(s.doc().unwrap().revision > before.0.revision);
            assert_eq!(s.doc().unwrap().history.undo.len(), before.0.history.undo.len() + 1);
            assert_eq!(s.journal.len(), before.1.len() + 1);
            assert_eq!(s.journal.last().unwrap(), &(id.to_owned(), params));
            assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &before.0.saved_doc));
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.as_ref(), before.0.doc.as_ref());
            assert!(!s.doc().unwrap().is_dirty());
        }
    }

    fn pending_caption() -> (Session, designcraft_doc::ItemId, designcraft_doc::StoryId) {
        let mut source = Session::new();
        source.execute("file.new", &json!({"pages": 1})).unwrap();
        let item = source.execute("frame.create", &json!({"rect": [72, 72, 272, 172]})).unwrap()["id"].as_u64().unwrap();
        source.execute("object.altText", &json!({"text": "First", "ids": [item]})).unwrap();
        let caption = source.execute("object.caption", &json!({"ids": [item], "text": "Photo: {altText}", "live": true})).unwrap()["ids"][0].as_u64().unwrap();
        let caption = designcraft_doc::ItemId(caption);
        let mut loaded = (*source.doc().unwrap().doc).clone();
        // A legal persisted/native model with stale dependent text, not a synthetic engine flag.
        // Native opening/add_document leaves the stored geometry/text intact until an edit.
        loaded.item_mut(designcraft_doc::ItemId(item)).unwrap().alt_text = "Second\nThird\nFourth\nFifth\nSixth".into();
        let frame = loaded.item_mut(caption).unwrap().text_frame_mut().unwrap();
        frame.options.auto_size = designcraft_doc::AutoSize::HeightOnly;
        let story = frame.story;
        loaded.check().unwrap();
        let mut s = Session::new();
        s.add_document(DocState::new(loaded, None));
        assert!(!s.doc().unwrap().is_dirty());
        assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Photo: First");
        (s, caption, story)
    }

    #[test]
    fn legal_pending_caption_and_dependent_autosize_are_quiet_only_for_numbering_noops() {
        for (id, params) in [
            ("layout.numberingOptions", json!({"page": 1, "startNumber": 2})),
            ("layout.chapterNumbering", json!({"source": "automatic"})),
            ("document.preferences", json!({})),
        ] {
            let genuine = id == "layout.numberingOptions";
            let (mut s, caption, story) = pending_caption();
            let before = snapshot(&s);
            let noop = json!({"page": 1});
            s.execute("layout.numberingOptions", &noop).unwrap();
            preserved(&s, &before, Some(&noop));
            assert_eq!(s.doc().unwrap().doc.item(caption).unwrap().bounds(), before.0.doc.item(caption).unwrap().bounds());
            let before_real = snapshot(&s);
            s.execute(id, &params).unwrap();
            if genuine { one_edit(&s, &before_real, &params); }
            assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Photo: Second\nThird\nFourth\nFifth\nSixth");
            assert!(s.doc().unwrap().doc.item(caption).unwrap().bounds().height() > before_real.0.doc.item(caption).unwrap().bounds().height(), "the original caption->autosize ordering remains effective");
            assert_eq!(s.doc().unwrap().history.undo.len(), before_real.0.history.undo.len() + 1);
            assert_eq!(s.journal.last().unwrap(), &(id.to_owned(), params));
            assert_eq!(s.journal.len(), before_real.1.len() + 1);
            assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &before_real.0.saved_doc));
            assert!(s.doc().unwrap().is_dirty());
            let accepted = snapshot(&s);
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.as_ref(), before_real.0.doc.as_ref());
            assert!(!s.doc().unwrap().is_dirty());
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.as_ref(), accepted.0.doc.as_ref());
        }
    }

    fn preview_fixture() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        s.execute("data.source.select", &json!({"bytes": crate::cmd::base64_encode(b"Name\nAda\nGrace\nKatherine\n"), "name": "people.csv"})).unwrap();
        s.execute("data.preview", &json!({"record": 2})).unwrap();
        assert_eq!(s.doc().unwrap().preview_record, Some(2));
        assert!(s.doc().unwrap().preview_stash.is_some());
        s
    }

    #[test]
    fn legal_preview_noop_is_quiet_while_real_default_and_invalid_attempts_keep_legacy_prelude() {
        let mut s = preview_fixture();
        let before = snapshot(&s);
        let template = before.0.preview_stash.as_ref().unwrap().clone();
        let params = json!({"page": 1});
        s.execute("layout.numberingOptions", &params).unwrap();
        preserved(&s, &before, Some(&params));
        assert_eq!(s.doc().unwrap().preview_record, before.0.preview_record);
        assert!(Arc::ptr_eq(s.doc().unwrap().preview_stash.as_ref().unwrap(), &template));
        for (id, params, refused) in [
            ("layout.numberingOptions", json!({"page": 1, "startNumber": 2}), false),
            ("layout.chapterNumbering", json!({"source": "automatic"}), false),
            ("layout.numberingOptions", json!({"page": 1, "chapter": {"source": "invalid"}}), true),
        ] {
            let mut s = preview_fixture();
            let before = snapshot(&s);
            let template = before.0.preview_stash.as_ref().unwrap().clone();
            let result = s.execute(id, &params);
            assert_eq!(result.is_err(), refused);
            assert!(s.doc().unwrap().preview_stash.is_none());
            assert_eq!(s.doc().unwrap().preview_record, None);
            assert!(s.doc().unwrap().revision > before.0.revision);
            assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &before.0.saved_doc));
            if refused {
                assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &template));
                assert_eq!(s.doc().unwrap().selection, before.0.selection);
                assert_eq!(entries(&s.doc().unwrap().history.undo), entries(&before.0.history.undo));
                assert_eq!(entries(&s.doc().unwrap().history.redo), entries(&before.0.history.redo));
                assert_eq!(s.doc().unwrap().history.limit, before.0.history.limit);
                assert_eq!(s.journal, before.1);
            } else {
                assert_eq!(s.doc().unwrap().history.undo.len(), before.0.history.undo.len() + 1);
                assert_eq!(s.journal.len(), before.1.len() + 1);
                assert_eq!(s.journal.last().unwrap(), &(id.to_owned(), params));
                s.execute("edit.undo", &json!({})).unwrap();
                assert_eq!(s.doc().unwrap().doc.as_ref(), template.as_ref(), "the undo target is the template, never a preview");
            }
        }
    }

    fn composing_fixture() -> (Session, designcraft_doc::StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let story = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap()["story"].as_u64().unwrap();
        s.set_tool("type");
        s.tool_preedit("Marked", Some(0..6), crate::ViewInfo::default()).unwrap();
        assert!(s.tool_composing());
        (s, designcraft_doc::StoryId(story))
    }

    #[test]
    fn legal_composition_noop_is_quiet_while_real_default_and_invalid_attempts_keep_legacy_prelude() {
        let (mut s, story) = composing_fixture();
        let before = snapshot(&s);
        let params = json!({"page": 1});
        s.execute("layout.numberingOptions", &params).unwrap();
        preserved(&s, &before, Some(&params));
        assert!(s.tool_composing());
        assert!(s.doc().unwrap().interaction.is_some());
        assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Marked");
        s.tool_ime_commit("Committed", crate::ViewInfo::default()).unwrap();
        assert!(!s.tool_composing());
        assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Committed");
        assert_eq!(s.doc().unwrap().history.undo.len(), before.0.history.undo.len() + 1);
        assert_eq!(s.journal.len(), before.1.len() + 2, "one numbering attempt plus the later independent IME commit");
        for (id, params, refused) in [
            ("layout.numberingOptions", json!({"page": 1, "startNumber": 2}), false),
            ("layout.chapterNumbering", json!({"source": "automatic"}), false),
            ("layout.numberingOptions", json!({"page": 1, "chapter": {"source": "invalid"}}), true),
        ] {
            let (mut s, story) = composing_fixture();
            let before = snapshot(&s);
            let result = s.execute(id, &params);
            assert_eq!(result.is_err(), refused);
            assert!(!s.tool_composing(), "non-noop and invalid attempts retain the original composition commit prelude");
            assert!(s.doc().unwrap().interaction.is_none());
            assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Marked");
            assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &before.0.saved_doc));
            assert_eq!(s.doc().unwrap().history.undo.len(), before.0.history.undo.len() + if refused { 1 } else { 2 });
            assert_eq!(s.journal.len(), before.1.len() + if refused { 1 } else { 2 });
            assert_eq!(s.journal[before.1.len()], ("text.insert".to_owned(), json!({"text": "Marked"})));
            if !refused {
                assert_eq!(s.journal.last().unwrap(), &(id.to_owned(), params));
                s.execute("edit.undo", &json!({})).unwrap();
                assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Marked", "numbering is one atomic step after the separate prelude commit");
            }
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "");
        }
    }
}
