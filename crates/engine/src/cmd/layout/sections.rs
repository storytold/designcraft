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

    pub(super) fn apply(&self, d: &mut designcraft_doc::Document) {
        let start = self.start;
        // Page 1 always starts a section.
        if self.remove && start != 0 {
            d.sections.retain(|x| x.start != start);
            // The pages after it follow the previous section's numbers again.
            d.repaginate();
            return;
        }
        let mut sec = d.sections.iter().find(|x| x.start == start).cloned().unwrap_or(designcraft_doc::Section {
            start,
            start_number: None,
            style: Default::default(),
            prefix: String::new(),
            marker: String::new(),
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
        d.sections.retain(|x| x.start != start);
        d.sections.push(sec);
        d.sections.sort_by_key(|x| x.start);
        // Facing pages take their sides from their numbers: odd on the right.
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
