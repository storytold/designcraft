//! Edit › Place and Link: a story that copies another (the parent) and can be updated when the
//! parent changes (Links panel: out of date).

use designcraft_doc::{Document, ParaFormat, Selection, Story, StoryId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, rect_param, spread_param};
use crate::Result;

/// `child` re-made from `parent`'s content (keeping the child's id and frames).
fn copy_into(d: &mut Document, parent: StoryId, child: StoryId) -> Result<()> {
    let src = d.story(parent).ok_or_else(|| bad("story.link", format!("no story {}", parent.0)))?.clone();
    let st = d.story_mut(child).ok_or_else(|| bad("story.link", format!("no story {}", child.0)))?;
    let (frames, rev, file_link) = (std::mem::take(&mut st.frames), st.rev, st.file_link.take());
    *st = Story { id: child, frames, rev: rev + 1, link: Some((parent, src.rev)), file_link, ..src };
    Ok(())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "story.placeAndLink",
            "Place and Link",
            ["Edit"],
            None,
            "{story (parent), rect, spread?} — a new frame whose story copies the parent and stays linked → {id, story}",
            has_doc,
            |s, p| {
                let parent = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("story.placeAndLink", "missing story"))?);
                let rect = rect_param(p, "rect").ok_or_else(|| bad("story.placeAndLink", "missing rect"))?;
                let sr = spread_param(p, "story.placeAndLink")?;
                let layer = s.doc()?.active_layer;
                s.edit(|d, sel| {
                    if d.story(parent).is_none() {
                        return Err(bad("story.placeAndLink", format!("no story {}", parent.0)));
                    }
                    let (fid, sid) = d.add_text_frame(sr, rect, layer, "", ParaFormat::default())?;
                    copy_into(d, parent, sid)?;
                    *sel = Selection::items(vec![fid]);
                    Ok(json!({"id": fid.0, "story": sid.0}))
                })
            }
        ),
        cmd!(query "story.links", "Linked Stories", [], None, "{} → [{story, parent, outOfDate}]", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let out: Vec<Value> = d
                .stories
                .values()
                .filter_map(|st| st.link.map(|(parent, rev)| json!({"story": st.id.0, "parent": parent.0, "outOfDate": d.story(parent).is_none_or(|ps| ps.rev != rev)})))
                .collect();
            Ok(Value::Array(out))
        }),
        cmd!(
            "story.updateLink",
            "Update Link",
            [],
            None,
            "{story? (default: every out-of-date linked story)} — the child takes the parent's current content (its own edits are replaced)",
            has_doc,
            |s, p| {
                let only = p.get("story").and_then(Value::as_u64).map(StoryId);
                s.edit(|d, _| {
                    let todo: Vec<(StoryId, StoryId)> = d
                        .stories
                        .values()
                        .filter(|st| only.is_none_or(|o| o == st.id))
                        .filter_map(|st| {
                            st.link.and_then(|(parent, rev)| {
                                (only.is_some() || d.story(parent).is_some_and(|ps| ps.rev != rev)).then_some((st.id, parent))
                            })
                        })
                        .collect();
                    for (child, parent) in &todo {
                        copy_into(d, *parent, *child)?;
                    }
                    Ok(json!({"updated": todo.len()}))
                })
            }
        ),
        cmd!("story.unlink", "Unlink", [], None, "{story}", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("story.unlink", "missing story"))?);
            s.edit(|d, _| {
                let st = d.story_mut(sid).ok_or_else(|| bad("story.unlink", "no such story"))?;
                st.link = None;
                Ok(Value::Null)
            })
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn linked_story_goes_out_of_date_and_updates() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let p = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Original"})).unwrap();
        let c = s.execute("story.placeAndLink", &json!({"story": p["story"], "rect": [72, 72, 300, 200], "spread": 1})).unwrap();
        let child = designcraft_doc::StoryId(c["story"].as_u64().unwrap());
        assert_eq!(s.doc().unwrap().doc.stories[&child].text, "Original");
        assert_eq!(s.execute("story.links", &json!({})).unwrap()[0]["outOfDate"], false);
        s.execute("story.setText", &json!({"story": p["story"], "text": "Revised"})).unwrap();
        assert_eq!(s.execute("story.links", &json!({})).unwrap()[0]["outOfDate"], true);
        assert_eq!(s.execute("story.updateLink", &json!({})).unwrap()["updated"], 1);
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.stories[&child].text, "Revised");
        assert!(!d.stories[&child].frames.is_empty(), "keeps its frame");
        assert_eq!(s.execute("story.links", &json!({})).unwrap()[0]["outOfDate"], false);
    }
}
