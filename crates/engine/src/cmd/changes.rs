//! Type › Track Changes: record edits as inserted / deleted text; accept or reject them.

use designcraft_doc::{ChangeMark, Document, Story, StoryId};
use serde_json::{Value, json};

use super::{CommandSpec, cmd, has_doc};
use crate::{Result, Session};

/// Returns the new start of text after `r` and the list of physically removed byte ranges (in original offsets, last first)
/// so callers can reconcile document-level text ranges.
pub(crate) fn mark_deleted_reporting(st: &mut Story, r: std::ops::Range<usize>) -> (usize, Vec<std::ops::Range<usize>>) {
    if r.is_empty() {
        return (r.start, Vec::new());
    }
    // Inserted pieces inside the range go away; the rest is marked deleted.
    let mut ins: Vec<std::ops::Range<usize>> = st
        .runs()
        .filter(|(rr, f)| f.over.change == Some(ChangeMark::Inserted) && rr.start < r.end && rr.end > r.start)
        .map(|(rr, _)| rr.start.max(r.start)..rr.end.min(r.end))
        .collect();
    let mut end = r.end;
    ins.sort_by_key(|x| std::cmp::Reverse(x.start));
    let removed = ins.clone();
    for x in ins {
        st.delete(x.clone());
        end -= x.len();
    }
    if end > r.start {
        st.format_chars(r.start..end, |f| f.over.change = Some(ChangeMark::Deleted));
    }
    (end, removed)
}

/// Mark `r` deleted (text this session inserted while tracking is simply removed). Returns where
/// the text after the range now starts.
pub(crate) fn mark_deleted(st: &mut Story, r: std::ops::Range<usize>) -> usize {
    mark_deleted_reporting(st, r).0
}

/// Ranges of `mark` in `st`, last first.
fn ranges(st: &Story, mark: ChangeMark) -> Vec<std::ops::Range<usize>> {
    let mut v: Vec<std::ops::Range<usize>> = st.runs().filter(|(_, f)| f.over.change == Some(mark)).map(|(r, _)| r).collect();
    v.sort_by_key(|r| std::cmp::Reverse(r.start));
    v
}

/// Accept (or reject) every change in `sid`.
fn resolve(d: &mut Document, sid: StoryId, accept: bool) -> usize {
    let (remove, keep) = if accept { (ChangeMark::Deleted, ChangeMark::Inserted) } else { (ChangeMark::Inserted, ChangeMark::Deleted) };
    let (removed, keep_count) = {
        let st = match d.story(sid) {
            Some(st) => st,
            None => return 0,
        };
        (ranges(st, remove), ranges(st, keep).len())
    };
    let n = removed.len() + keep_count;
    if let Some(st) = d.story_mut(sid) {
        for r in &removed {
            st.delete(r.clone());
        }
        let len = st.len();
        st.format_chars(0..len, |f| {
            if f.over.change.is_some() {
                f.over.change = None;
            }
        });
    }
    for r in removed {
        d.adjust_text_ranges(sid, r, 0);
    }
    n
}

/// Accept or reject the one change at `start` in story `story`.
fn one(s: &mut Session, p: &Value, accept: bool) -> Result<Value> {
    let id = if accept { "changes.accept" } else { "changes.reject" };
    let sid = p.get("story").and_then(Value::as_u64).map(StoryId).ok_or_else(|| super::bad(id, "`story` required"))?;
    let start = p.get("start").and_then(Value::as_u64).ok_or_else(|| super::bad(id, "`start` required"))? as usize;
    s.edit(|d, sel| {
        let (r, mark) = {
            let st = d.story(sid).ok_or_else(|| super::bad(id, "no such story"))?;
            st.runs()
                .find(|(r, f)| matches!(f.over.change, Some(ChangeMark::Inserted | ChangeMark::Deleted)) && r.start <= start && start < r.end)
                .map(|(r, f)| (r, f.over.change.unwrap_or_default()))
                .ok_or_else(|| super::bad(id, "no change there"))?
        };
        // Accepting a deletion or rejecting an insertion removes the text; otherwise it stays, unmarked.
        let delete = (mark == ChangeMark::Deleted) == accept;
        if let Some(st) = d.story_mut(sid) {
            if delete {
                st.delete(r.clone());
            } else {
                st.format_chars(r.clone(), |f| f.over.change = None);
            }
        }
        if delete {
            d.adjust_text_ranges(sid, r.clone(), 0);
        }
        if let Some(t) = &mut sel.text
            && t.story == sid
        {
            let len = d.story(sid).map_or(0, |x| x.len());
            t.anchor = t.anchor.min(len);
            t.focus = t.focus.min(len);
        }
        Ok(json!({"start": r.start, "end": r.end}))
    })
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("changes.track", "Track Changes", ["Type", "Track Changes"], None, "{on?: bool (default: toggle)}", has_doc, |s, p| {
            let cur = s.doc()?.doc.settings.track_changes;
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(!cur);
            s.edit(|d, _| {
                d.settings.track_changes = on;
                Ok(json!({"on": on}))
            })
        }),
        cmd!(query "changes.list", "Changes", [], None, "{} → [{story, start, end, kind: inserted|deleted, text}]", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let mut out = Vec::new();
            for st in d.stories.values() {
                for (r, f) in st.runs() {
                    let kind = match f.over.change {
                        Some(ChangeMark::Inserted) => "inserted",
                        Some(ChangeMark::Deleted) => "deleted",
                        _ => continue,
                    };
                    out.push(json!({"story": st.id.0, "start": r.start, "end": r.end, "kind": kind, "text": st.text[r].to_string()}));
                }
            }
            Ok(Value::Array(out))
        }),
        cmd!("changes.accept", "Accept Change", [], None, "{story, start} — the change at that position", has_doc, |s, p| one(s, p, true)),
        cmd!("changes.reject", "Reject Change", [], None, "{story, start} — the change at that position", has_doc, |s, p| one(s, p, false)),
        cmd!(
            "changes.acceptAll",
            "Accept All Changes",
            ["Type", "Track Changes"],
            None,
            "{story?} — deleted text goes, added text stays",
            has_doc,
            |s, p| all(s, p, true)
        ),
        cmd!(
            "changes.rejectAll",
            "Reject All Changes",
            ["Type", "Track Changes"],
            None,
            "{story?} — added text goes, deleted text comes back",
            has_doc,
            |s, p| all(s, p, false)
        ),
    ]
}

fn all(s: &mut Session, p: &Value, accept: bool) -> Result<Value> {
    let only = p.get("story").and_then(Value::as_u64).map(StoryId);
    s.edit(|d, sel| {
        let ids: Vec<StoryId> = d.stories.keys().copied().filter(|id| only.is_none_or(|o| o == *id)).collect();
        let mut n = 0;
        for id in ids {
            n += resolve(d, id, accept);
        }
        // The caret may now be past the end.
        if let Some(t) = &mut sel.text
            && let Some(st) = d.story(t.story)
        {
            t.anchor = t.anchor.min(st.len());
            t.focus = t.focus.min(st.len());
        }
        Ok(json!({"changes": n}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn tracked_edits_accept_and_reject() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "The cat sat"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("changes.track", &json!({"on": true})).unwrap();
        // Replace "cat" with "dog", then type and erase a typo (gone without a trace).
        s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 7})).unwrap();
        s.execute("text.insert", &json!({"text": "dog"})).unwrap();
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        let text = |s: &Session| s.doc().unwrap().doc.stories[&sid].text.clone();
        assert_eq!(text(&s), "The catdog sat", "deleted text stays (marked)");
        let l = s.execute("changes.list", &json!({})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 2, "{l}");
        // Deleted text takes no room in the layout.
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, sid, None);
        assert!(cs.frames[0].lines[0].glyphs.iter().any(|g| !g.visible && g.byte >= 4 && g.byte < 7));
        // One at a time: accept the deletion of "cat", then reject the insertion of "dog".
        s.execute("changes.accept", &json!({"story": sid.0, "start": 4})).unwrap();
        assert_eq!(text(&s), "The dog sat");
        s.execute("changes.reject", &json!({"story": sid.0, "start": 4})).unwrap();
        assert_eq!(text(&s), "The  sat");
        assert!(s.execute("changes.accept", &json!({"story": sid.0, "start": 0})).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s), "The catdog sat");
        s.execute("changes.rejectAll", &json!({})).unwrap();
        assert_eq!(text(&s), "The cat sat");
        s.execute("edit.undo", &json!({})).unwrap();
        let exported = s.execute("file.exportText", &json!({"story": sid.0})).unwrap()["text"].as_str().unwrap().to_string();
        assert_eq!(exported, "The dog sat", "deleted text isn't exported");
        s.execute("changes.acceptAll", &json!({})).unwrap();
        assert_eq!(text(&s), "The dog sat");
        assert!(s.execute("changes.list", &json!({})).unwrap().as_array().unwrap().is_empty());
    }
}
