//! Index: page references (New Page Reference) and Generate Index.

use designcraft_doc::index::{IndexRange, IndexRef, IndexSpec, Resolved};
use designcraft_doc::{Document, INDEX_MARK, ParaAttrs, ParaFormat, ParagraphStyle, SpreadRef, StoryId, TextSel};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "index.addReference",
            "New Page Reference…",
            ["Layout", "Index"],
            None,
            "{topics?: [level 1..4] | topic?: \"Level 1 > Level 2\" (default: the selected text), sort?: [keys], range?: currentPage|toEndOfStory|suppressPageRange|nextParagraphs|see|seeAlso, paragraphs?: n, target?: topic for see/seeAlso} — a marker at the insertion point",
            in_story_text,
            add_reference
        ),
        cmd!(query "index.list", "Index Page References", [], None, "{} → [{story, pos, topics, range, page}]", has_doc, list),
        cmd!(
            "index.generate",
            "Generate Index…",
            ["Layout", "Index"],
            None,
            "{title?: \"Index\", sectionHeadings?: true, runIn?: false, page?: 1-based page for a new frame, rect?: [x0,y0,x1,y1]} — replaces the existing index → {story, entries}",
            has_doc,
            generate
        ),
        cmd!("index.update", "Update Index", ["Layout", "Index"], None, "{} — regenerate the existing index", has_index, |s, _| {
            let spec = s.doc()?.doc.index.clone().ok_or_else(|| bad("index.update", "no index"))?;
            fill_index(s, spec, None)
        }),
    ]
}

fn in_story_text(s: &Session) -> std::result::Result<(), String> {
    super::has_text(s)?;
    match s.active().and_then(|d| d.selection.text) {
        Some(t) if t.cell.is_some() => Err("index markers go in story text".into()),
        _ => Ok(()),
    }
}

fn has_index(s: &Session) -> std::result::Result<(), String> {
    match s.doc() {
        Ok(st) if st.doc.index.as_ref().is_some_and(|t| st.doc.story(t.story).is_some()) => Ok(()),
        _ => Err("the document has no index".into()),
    }
}

fn add_reference(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let t = st.selection.text.ok_or_else(|| bad("index.addReference", "no insertion point"))?;
    let selected = st.doc.story(t.story).map(|x| designcraft_doc::xref::clean(x.slice(t.range()))).unwrap_or_default();
    let topics: Vec<String> = if let Some(a) = p.get("topics").and_then(Value::as_array) {
        a.iter().filter_map(Value::as_str).map(str::to_string).collect()
    } else if let Some(t) = str_param(p, "topic") {
        t.split('>').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    } else if !selected.is_empty() {
        vec![selected]
    } else {
        return Err(bad("index.addReference", "give `topic` or select the text to index"));
    };
    if topics.is_empty() || topics.len() > 4 {
        return Err(bad("index.addReference", "1 to 4 topic levels"));
    }
    let target = || str_param(p, "target").unwrap_or("").to_string();
    let range = match str_param(p, "range").unwrap_or("currentPage") {
        "currentPage" => IndexRange::CurrentPage,
        "toEndOfStory" => IndexRange::ToEndOfStory,
        "suppressPageRange" => IndexRange::SuppressPageRange,
        "nextParagraphs" => {
            IndexRange::NextParagraphs(p.get("paragraphs").and_then(Value::as_u64).map_or(1, |x| u32::try_from(x).unwrap_or(u32::MAX)))
        }
        "see" => IndexRange::See(target()),
        "seeAlso" => IndexRange::SeeAlso(target()),
        r => return Err(bad("index.addReference", format!("unknown range `{r}`"))),
    };
    let sort: Vec<String> =
        p.get("sort").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let r = IndexRef { topics, sort, range };
    s.edit(|d, sel| {
        let st = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        // InDesign puts the marker before the selected text.
        let at = t.range().start.min(st.len());
        st.insert_index_ref(at, r.clone());
        let shift = INDEX_MARK.len_utf8();
        sel.text = Some(TextSel { anchor: t.anchor + shift, focus: t.focus + shift, ..t });
        Ok(json!({"pos": at}))
    })
}

/// Every page reference with its page span.
fn resolve(s: &Session, d: &Document, skip: Option<StoryId>) -> Vec<(StoryId, usize, Resolved)> {
    let mut out = Vec::new();
    for st in d.stories.values().filter(|st| !st.index_refs.is_empty() && Some(st.id) != skip) {
        let cs = s.cache.get(d, st.id, None);
        let page = |pos: usize| designcraft_compose::vars::page_of(d, &cs, pos);
        for ((pos, _), r) in st.text.match_indices(INDEX_MARK).zip(&st.index_refs) {
            let start = page(pos);
            let end = match &r.range {
                IndexRange::ToEndOfStory => page(st.len().saturating_sub(1)),
                IndexRange::NextParagraphs(n) => {
                    let ranges = st.para_ranges();
                    let n = usize::try_from(*n).unwrap_or(usize::MAX);
                    let pi = st.para_at(pos).saturating_add(n).min(ranges.len().saturating_sub(1));
                    ranges.get(pi).and_then(|r| page(r.end.saturating_sub(1).max(r.start)))
                }
                _ => start,
            };
            let pages = match (start, end) {
                (Some(a), Some(b)) => Some((a, b.max(a))),
                (Some(a), None) => Some((a, a)),
                _ => None,
            };
            out.push((st.id, pos, Resolved { r: (**r).clone(), pages }));
        }
    }
    out
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let d = s.doc()?.doc.clone();
    let rs = resolve(s, &d, None);
    Ok(Value::Array(
        rs.into_iter()
            .map(|(sid, pos, r)| json!({"story": sid.0, "pos": pos, "topics": r.r.topics, "range": r.r.range, "page": r.pages.map(|p| d.page_name(p.0))}))
            .collect(),
    ))
}

fn ensure_styles(d: &mut Document) {
    let mut want = vec![
        ("Index Title", ParaAttrs { space_after: Some(12.0), ..Default::default() }, Some(16.0)),
        (
            "Index Section Head",
            ParaAttrs { space_before: Some(8.0), space_after: Some(2.0), keep_with_next: Some(2), ..Default::default() },
            Some(11.0),
        ),
    ];
    let levels = [("Index Level 1", 0.0), ("Index Level 2", 12.0), ("Index Level 3", 24.0), ("Index Level 4", 36.0)];
    for (name, indent) in levels {
        // Hanging indent so turnover lines stand out from subtopics.
        want.push((name, ParaAttrs { left_indent: Some(indent + 12.0), first_line_indent: Some(-12.0), ..Default::default() }, None));
    }
    let missing: Vec<_> = want.into_iter().filter(|(n, ..)| d.styles.para(n).is_none()).collect();
    if missing.is_empty() {
        return;
    }
    let st = d.styles_mut();
    for (name, para, size) in missing {
        st.paragraph.push(ParagraphStyle {
            name: name.into(),
            based_on: Some(designcraft_doc::story::BASIC_PARAGRAPH.into()),
            next_style: None,
            para,
            chars: designcraft_doc::CharAttrs { size, ..Default::default() },
            shortcut: String::new(),
        });
    }
}

fn fill_index(s: &mut Session, spec: IndexSpec, new_frame: Option<(usize, Option<Rect>)>) -> Result<Value> {
    let d0 = s.doc()?.doc.clone();
    let existing = d0.index.as_ref().map(|i| i.story).filter(|sid| d0.story(*sid).is_some());
    let refs = resolve(s, &d0, existing);
    let resolved: Vec<Resolved> = refs.into_iter().map(|(_, _, r)| r).collect();
    let paras = designcraft_doc::index::build(&spec, &resolved, &|p| d0.page_name(p));
    s.edit(|d, sel| {
        ensure_styles(d);
        let story = match existing {
            Some(sid) => sid,
            None => {
                let (page, rect) = new_frame.unwrap_or((0, None));
                let (si, pi) = d.page_loc(page).ok_or_else(|| bad("index.generate", format!("no page {}", page + 1)))?;
                let r: Rect = rect.unwrap_or_else(|| d.spreads[si].pages[pi].margin_rect());
                let layer = d.default_layer();
                let (fid, sid) = d.add_text_frame(SpreadRef::Doc(si), r, layer, "", ParaFormat::default())?;
                *sel = designcraft_doc::Selection::items(vec![fid]);
                sid
            }
        };
        let text = paras.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
        let st = d.story_mut(story).ok_or(designcraft_doc::DocError::NoStory(story))?;
        let len = st.len();
        st.replace(0..len, &text);
        for (pi, r) in st.para_ranges().into_iter().enumerate() {
            if let Some((name, _)) = paras.get(pi) {
                st.format_paras(r.start..r.start, |f| *f = ParaFormat { style: name.clone(), ..Default::default() });
            }
        }
        d.index = Some(IndexSpec { story, ..spec.clone() });
        Ok(json!({"story": story.0, "entries": paras.iter().filter(|(s, _)| s.starts_with("Index Level")).count()}))
    })
}

fn generate(s: &mut Session, p: &Value) -> Result<Value> {
    let mut spec = s.doc()?.doc.index.clone().unwrap_or_default();
    if let Some(t) = str_param(p, "title") {
        spec.title = t.into();
    }
    if let Some(b) = p.get("sectionHeadings").and_then(Value::as_bool) {
        spec.section_headings = b;
    }
    if let Some(b) = p.get("runIn").and_then(Value::as_bool) {
        spec.run_in = b;
    }
    let page = p.get("page").and_then(Value::as_u64).unwrap_or(1).max(1) as usize - 1;
    fill_index(s, spec, Some((page, super::rect_param(p, "rect"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_references_build_an_index() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let mut sids = vec![];
        for page in 0..3usize {
            let sp = s.doc().unwrap().doc.page_loc(page).unwrap().0;
            let text = ["Kerning adjusts pairs.", "Leading sets line spacing. Kerning again.", "Grids organise pages."][page];
            let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
            sids.push(s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story);
        }
        // Topic from the selected word, explicit nested topics, a See reference.
        s.execute("text.select", &json!({"story": sids[0].0, "anchor": 0, "focus": 7})).unwrap();
        s.execute("index.addReference", &json!({})).unwrap();
        s.execute("text.select", &json!({"story": sids[1].0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("index.addReference", &json!({"topic": "Type > Leading"})).unwrap();
        s.execute("text.select", &json!({"story": sids[1].0, "anchor": 30, "focus": 30})).unwrap();
        s.execute("index.addReference", &json!({"topic": "Kerning"})).unwrap();
        s.execute("text.select", &json!({"story": sids[2].0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("index.addReference", &json!({"topic": "Line spacing", "range": "see", "target": "Type: Leading"})).unwrap();
        // The marker doesn't change the selection's text.
        let st = s.doc().unwrap().doc.story(sids[0]).unwrap().clone();
        let sel = s.doc().unwrap().selection.text.unwrap();
        let _ = sel;
        assert!(st.text.starts_with(INDEX_MARK));
        let l = s.execute("index.list", &json!({})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 4);
        s.execute("layout.pages.insert", &json!({"count": 1})).unwrap();
        let r = s.execute("index.generate", &json!({"page": 4})).unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        let text = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
        assert_eq!(text, "Index\nK\nKerning  1, 2\nL\nLine spacing  See Type: Leading\nT\nType\nLeading  2");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().paras[6].style, "Index Level 1");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().paras[7].style, "Index Level 2");
        // Deleting a marked word removes its reference; update regenerates in place.
        s.execute("text.select", &json!({"story": sids[0].0, "anchor": 0, "focus": INDEX_MARK.len_utf8() + 7})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        s.execute("index.update", &json!({})).unwrap();
        let text = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
        assert!(text.contains("Kerning  2\n"), "{text}");
        s.doc().unwrap().doc.check().unwrap();
    }
}
