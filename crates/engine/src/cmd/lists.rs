//! Type › Bulleted & Numbered Lists › Define Lists: named lists whose numbers can continue from
//! story to story. Paragraphs join one with `listName` (and restart with `startAt`).

use designcraft_doc::NumberedList;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "list.define",
            "Define Lists…",
            ["Type", "Bulleted & Numbered Lists"],
            None,
            "{name, continueAcrossStories?: true, rename?} — paragraphs join it with type.para {listType: numbers, listName}",
            has_doc,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("list.define", "missing name"))?.to_string();
                let cont = p.get("continueAcrossStories").and_then(Value::as_bool);
                let rename = str_param(p, "rename").map(str::to_string);
                s.edit(|d, _| {
                    let lists = &mut d.settings.lists;
                    match lists.iter_mut().find(|l| l.name == name) {
                        Some(l) => {
                            if let Some(c) = cont {
                                l.continue_across_stories = c;
                            }
                            if let Some(r) = &rename {
                                l.name = r.clone();
                            }
                        }
                        None => {
                            lists.push(NumberedList { name: rename.clone().unwrap_or(name.clone()), continue_across_stories: cont.unwrap_or(true) })
                        }
                    }
                    Ok(json!({"lists": d.settings.lists}))
                })
            }
        ),
        cmd!("list.delete", "Delete List", [], None, "{name}", has_doc, |s, p| {
            let name = str_param(p, "name").ok_or_else(|| bad("list.delete", "missing name"))?.to_string();
            s.edit(|d, _| {
                d.settings.lists.retain(|l| l.name != name);
                Ok(json!({"lists": d.settings.lists}))
            })
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    /// The digit the story's first line starts with (its list number).
    fn digit(g: &designcraft_compose::PlacedGlyph) -> char {
        let face = g.face.get();
        ('0'..='9').find(|c| face.glyph_for(*c) == g.gid).unwrap_or('?')
    }

    fn first_char(s: &Session, sid: u64) -> char {
        let d = &s.doc().unwrap().doc;
        let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
        digit(&cs.frames[0].lines[0].glyphs[0])
    }

    #[test]
    fn named_lists_continue_across_stories() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        s.execute("list.define", &json!({"name": "Steps"})).unwrap();
        let mut stories = vec![];
        for (spread, text) in [(0, "Mix\nStir"), (1, "Bake\nServe")] {
            let r = s.execute("frame.create", &json!({"spread": spread, "rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
            let sid = r["story"].as_u64().unwrap();
            s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": text.len()})).unwrap();
            s.execute("type.para", &json!({"attrs": {"listType": "numbers", "listName": "Steps"}})).unwrap();
            stories.push(sid);
        }
        assert_eq!(first_char(&s, stories[0]), '1');
        assert_eq!(first_char(&s, stories[1]), '3', "carries on from the first story");
        // Not continuing: each story starts at 1.
        s.execute("list.define", &json!({"name": "Steps", "continueAcrossStories": false})).unwrap();
        assert_eq!(first_char(&s, stories[1]), '1');
        s.execute("list.define", &json!({"name": "Steps", "continueAcrossStories": true})).unwrap();
        // The session's cache follows earlier stories (no stale numbers).
        s.execute("text.select", &json!({"story": stories[0], "anchor": 0, "focus": 0})).unwrap();
        s.execute("text.insert", &json!({"text": "Prep\n"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, designcraft_doc::StoryId(stories[1]), None);
        assert_eq!(digit(&cs.frames[0].lines[0].glyphs[0]), '4');
        // Start At restarts.
        s.execute("text.select", &json!({"story": stories[1], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"attrs": {"startAt": 7}})).unwrap();
        assert_eq!(first_char(&s, stories[1]), '7');
        // null removes the override again.
        s.execute("type.para", &json!({"attrs": {"startAt": null}})).unwrap();
        assert_eq!(first_char(&s, stories[1]), '4');
    }
}
