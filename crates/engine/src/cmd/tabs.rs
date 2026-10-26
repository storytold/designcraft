//! Tab stops of the selected paragraphs or of a paragraph style (Type › Tabs).
//!
//! Positions are points from the left edge of the text column, as composition measures them.
//! The list operations are public so the style dialog can edit a list before it applies it.

use designcraft_doc::{ItemId, TabAlign, TabStop};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::text::{format_paras, format_targets};
use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

/// Most stops a paragraph holds.
pub const MAX_TABS: usize = designcraft_doc::MAX_TAB_STOPS;
/// Furthest stop position: 216 in, the largest page.
pub const MAX_POSITION: f64 = designcraft_doc::MAX_TAB_POSITION;
/// Longest leader, in characters.
pub const MAX_LEADER: usize = designcraft_doc::MAX_TAB_LEADER;
/// Stops closer than this share a position.
const SAME: f64 = designcraft_doc::SAME_TAB_POSITION;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "type.tabs.get",
            "Get Tabs",
            [],
            None,
            "{style?} → {tabs, leftIndent, firstLineIndent, rightIndent, width, frame?, column?: [x0,y0,x1,y1] (frame inner space)} — the first selected paragraph's (or the style's) stops and indents, and the width of its text column",
            has_doc,
            get
        ),
        cmd!(
            "type.tabs.add",
            "Add Tab",
            [],
            None,
            "{position (pt from the column's left edge), align?: left|center|right|char, leader? (≤ 8 characters), alignOn? (one character), style?} → {index, tabs} — a stop at the same position is replaced",
            has_doc,
            |s, p| {
                let c = "type.tabs.add";
                let stop = stop_param(p, c)?;
                edit(s, p, c, |tabs, _| add(tabs, stop))
            }
        ),
        cmd!("type.tabs.move", "Move Tab", [], None, "{index, position, style?} → {index, tabs}", has_doc, |s, p| {
            let c = "type.tabs.move";
            let (i, pos) = (index_param(p, c)?, position_param(p, c)?);
            edit(s, p, c, |tabs, _| move_to(tabs, i, pos))
        }),
        cmd!(
            "type.tabs.change",
            "Change Tab",
            [],
            None,
            "{index, align?: left|center|right|char, leader?, alignOn?, style?} → {index, tabs}",
            has_doc,
            |s, p| {
                let c = "type.tabs.change";
                let i = index_param(p, c)?;
                let align = p.get("align").map(|v| align_value(v, c)).transpose()?;
                let leader = str_param(p, "leader").map(|v| leader_value(v, c)).transpose()?;
                let align_on = str_param(p, "alignOn").map(|v| align_on_value(v, c)).transpose()?;
                edit(s, p, c, |tabs, _| change(tabs, i, align, leader, align_on))
            }
        ),
        cmd!("type.tabs.remove", "Delete Tab", [], None, "{index, style?} → {tabs}", has_doc, |s, p| {
            let i = index_param(p, "type.tabs.remove")?;
            edit(s, p, "type.tabs.remove", |tabs, _| remove(tabs, i))
        }),
        cmd!("type.tabs.clear", "Clear All Tabs", [], None, "{style?} → {tabs}", has_doc, |s, p| {
            edit(s, p, "type.tabs.clear", |tabs, _| {
                tabs.clear();
                Ok(None)
            })
        }),
        cmd!(
            "type.tabs.repeat",
            "Repeat Tab",
            [],
            None,
            "{index, width?, style?} → {index, tabs} — repeats the distance from the previous stop (or the left indent) to this one up to the column width less the right indent",
            has_doc,
            |s, p| {
                let c = "type.tabs.repeat";
                let i = index_param(p, c)?;
                let width = p.get("width").and_then(Value::as_f64).filter(|w| w.is_finite());
                edit(s, p, c, |tabs, m| repeat(tabs, i, m.left_indent, width.unwrap_or(m.width) - m.right_indent))
            }
        ),
    ]
}

// ---------- list operations ----------

/// Sort by position (stable for equal ones).
pub fn sort(tabs: &mut [TabStop]) {
    tabs.sort_by(|a, b| a.position.total_cmp(&b.position));
}

/// A finite position clamped to `0..=MAX_POSITION`.
pub fn clamp_position(v: f64) -> std::result::Result<f64, String> {
    TabStop::clamp_position(v)
}

/// Insert `stop` (replacing one at the same position); returns its index.
pub fn add(tabs: &mut Vec<TabStop>, mut stop: TabStop) -> std::result::Result<Option<usize>, String> {
    stop.position = clamp_position(stop.position)?;
    if let Some(i) = tabs.iter().position(|t| (t.position - stop.position).abs() < SAME) {
        if let Some(t) = tabs.get_mut(i) {
            *t = stop;
        }
        return Ok(Some(i));
    }
    if tabs.len() >= MAX_TABS {
        return Err(format!("a paragraph holds at most {MAX_TABS} tab stops"));
    }
    let pos = stop.position;
    tabs.push(stop);
    sort(tabs);
    Ok(tabs.iter().position(|t| t.position == pos))
}

/// Move stop `index` to `position` (a stop already there is replaced); returns its new index.
pub fn move_to(tabs: &mut Vec<TabStop>, index: usize, position: f64) -> std::result::Result<Option<usize>, String> {
    let position = clamp_position(position)?;
    if index >= tabs.len() {
        return Err(format!("no tab stop {index}"));
    }
    let mut stop = tabs.remove(index);
    stop.position = position;
    tabs.retain(|t| (t.position - position).abs() >= SAME);
    tabs.push(stop);
    sort(tabs);
    Ok(tabs.iter().position(|t| t.position == position))
}

/// Change the alignment, leader or align-on character of stop `index`.
pub fn change(
    tabs: &mut [TabStop],
    index: usize,
    align: Option<TabAlign>,
    leader: Option<String>,
    align_on: Option<String>,
) -> std::result::Result<Option<usize>, String> {
    let t = tabs.get_mut(index).ok_or_else(|| format!("no tab stop {index}"))?;
    if let Some(a) = align {
        t.align = a;
        if a == TabAlign::Char && t.align_on.is_empty() {
            t.align_on = ".".into();
        }
    }
    if let Some(l) = leader {
        t.leader = l;
    }
    if let Some(c) = align_on {
        t.align_on = c;
    }
    Ok(Some(index))
}

/// Remove stop `index`.
pub fn remove(tabs: &mut Vec<TabStop>, index: usize) -> std::result::Result<Option<usize>, String> {
    if index >= tabs.len() {
        return Err(format!("no tab stop {index}"));
    }
    tabs.remove(index);
    Ok(None)
}

/// Repeat Tab: copies of stop `index` at the distance between it and the previous stop (or the
/// left indent), up to `limit`. Returns the index of the stop repeated.
pub fn repeat(tabs: &mut Vec<TabStop>, index: usize, left_indent: f64, limit: f64) -> std::result::Result<Option<usize>, String> {
    let stop = tabs.get(index).cloned().ok_or_else(|| format!("no tab stop {index}"))?;
    let from = match index.checked_sub(1).and_then(|i| tabs.get(i)) {
        Some(prev) => prev.position,
        None if left_indent.is_finite() && left_indent < stop.position => left_indent.max(0.0),
        None => 0.0,
    };
    let step = stop.position - from;
    if !step.is_finite() || step < 1.0 {
        return Err("the stop needs a distance of at least 1 pt from the previous stop or the left indent".into());
    }
    let limit = if limit.is_finite() { limit.min(MAX_POSITION) } else { MAX_POSITION };
    let mut x = stop.position + step;
    while x <= limit + SAME && tabs.len() < MAX_TABS {
        tabs.retain(|t| (t.position - x).abs() >= SAME);
        tabs.push(TabStop { position: x, ..stop.clone() });
        x += step;
    }
    sort(tabs);
    Ok(tabs.iter().position(|t| t.position == stop.position))
}

// ---------- parameters ----------

fn index_param(p: &Value, c: &str) -> Result<usize> {
    p.get("index").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok()).ok_or_else(|| bad(c, "`index`: a tab stop number from 0"))
}

fn position_param(p: &Value, c: &str) -> Result<f64> {
    let v = p.get("position").and_then(Value::as_f64).ok_or_else(|| bad(c, "`position`: a number of points"))?;
    clamp_position(v).map_err(|e| bad(c, e))
}

fn align_value(v: &Value, c: &str) -> Result<TabAlign> {
    serde_json::from_value(v.clone()).map_err(|_| bad(c, "`align`: left, center, right or char"))
}

/// A leader: up to [`MAX_LEADER`] characters, no control characters.
pub fn leader_value(v: &str, c: &str) -> Result<String> {
    TabStop::check_leader(v).map_err(|e| bad(c, e))?;
    Ok(v.to_string())
}

/// An align-on character: one printable character or none (a decimal point).
pub fn align_on_value(v: &str, c: &str) -> Result<String> {
    TabStop::check_align_on(v).map_err(|e| bad(c, e))?;
    Ok(v.to_string())
}

fn stop_param(p: &Value, c: &str) -> Result<TabStop> {
    let align = p.get("align").map(|v| align_value(v, c)).transpose()?.unwrap_or_default();
    let mut stop = TabStop {
        position: position_param(p, c)?,
        align,
        leader: str_param(p, "leader").map(|v| leader_value(v, c)).transpose()?.unwrap_or_default(),
        align_on: str_param(p, "alignOn").map(|v| align_on_value(v, c)).transpose()?.unwrap_or_default(),
    };
    if align == TabAlign::Char && stop.align_on.is_empty() {
        stop.align_on = ".".into();
    }
    Ok(stop)
}

// ---------- reading and writing ----------

/// The indents and measure that tab editing works against.
#[derive(Clone, Copy, Debug)]
struct Measure {
    left_indent: f64,
    first_line_indent: f64,
    right_indent: f64,
    width: f64,
}

/// The stops ([`TabStop::sanitized_list`]) and indents of the first selected paragraph, or of
/// style `style`.
fn current(s: &Session, style: Option<&str>, c: &str) -> Result<(Vec<TabStop>, Measure)> {
    let st = s.doc()?;
    let props = match style {
        Some(name) => {
            if st.doc.styles.para(name).is_none() {
                return Err(bad(c, format!("no paragraph style `{name}`")));
            }
            st.doc.styles.resolve_para_style(name).0
        }
        None => {
            let t = format_targets(s).into_iter().next().ok_or_else(|| bad(c, "select text or a text frame, or name a `style`"))?;
            let story = st.doc.text_story(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
            let pf = story.paras.get(story.para_at(t.range.start)).ok_or_else(|| bad(c, "no paragraph at the selection"))?;
            st.doc.styles.resolve_para(pf).0
        }
    };
    let width = if style.is_none() { column_of(s).map(|(_, r)| r.width()) } else { None }.unwrap_or_else(|| page_column_width(s));
    // A list that never went through the checks (one set by code) reads as a file's would.
    let tabs = TabStop::sanitized_list(props.tabs);
    Ok((tabs, Measure { left_indent: props.left_indent, first_line_indent: props.first_line_indent, right_indent: props.right_indent, width }))
}

/// The text column of the first selected paragraph: (frame, column rect in the frame's inner space).
pub(crate) fn column_of(s: &Session) -> Option<(ItemId, Rect)> {
    let st = s.active()?;
    let t = format_targets(s).into_iter().next()?;
    let cs = s.cache.get(&st.doc, t.story, None);
    if t.cell.is_none() {
        let pi = st.doc.story(t.story)?.para_at(t.range.start);
        let hit = cs
            .frames
            .iter()
            .find_map(|ft| ft.lines.iter().find(|l| l.para == pi).and_then(|l| ft.columns.get(l.column as usize)).map(|r| (ft.frame, *r)));
        if hit.is_some() {
            return hit;
        }
    }
    let ft = cs.frames.first()?;
    Some((ft.frame, *ft.columns.first()?))
}

/// The first page's column width (a style's tabs have no frame to measure).
fn page_column_width(s: &Session) -> f64 {
    s.active().and_then(|st| st.doc.page(0).and_then(|p| p.column_rects().first().map(|r| r.width()))).filter(|w| *w > 1.0).unwrap_or(468.0)
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let style = str_param(p, "style");
    let (tabs, m) = current(s, style, "type.tabs.get")?;
    let mut out = json!({
        "tabs": tabs,
        "leftIndent": m.left_indent,
        "firstLineIndent": m.first_line_indent,
        "rightIndent": m.right_indent,
        "width": m.width,
    });
    if style.is_none()
        && let Some((frame, r)) = column_of(s)
    {
        out["frame"] = json!(frame.0);
        out["column"] = json!([r.x0, r.y0, r.x1, r.y1]);
    }
    Ok(out)
}

/// Apply `op` to the current stops and write the list to the selected paragraphs or the style.
/// Returns the list as stored ([`TabStop::checked_list`]) and the index of `op`'s stop in it.
fn edit(
    s: &mut Session,
    p: &Value,
    c: &str,
    op: impl FnOnce(&mut Vec<TabStop>, Measure) -> std::result::Result<Option<usize>, String>,
) -> Result<Value> {
    let style = str_param(p, "style").map(str::to_string);
    let (mut tabs, m) = current(s, style.as_deref(), c)?;
    let index = op(&mut tabs, m).map_err(|e| bad(c, e))?;
    let at = index.and_then(|i| tabs.get(i)).map(|t| t.position);
    let tabs = TabStop::checked_list(tabs).map_err(|e| bad(c, e))?;
    // Checking can drop stops that share a position, which moves the indices after them.
    let index = at.and_then(|x| tabs.iter().position(|t| (t.position - x).abs() < SAME));
    let list = serde_json::to_value(&tabs).map_err(|e| bad(c, e.to_string()))?;
    match style {
        Some(name) => s.edit(|d, _| {
            let st = d.styles_mut().para_mut(&name).ok_or_else(|| bad(c, format!("no paragraph style `{name}`")))?;
            st.para.set_json_over("tabs", &list, &designcraft_doc::ParaProps::default()).map_err(|e| bad(c, e))
        })?,
        None => {
            format_paras(s, &json!({ "tabs": list }))?;
        }
    }
    Ok(json!({"index": index, "tabs": list}))
}

#[cfg(test)]
mod tests {
    use designcraft_doc::{TabAlign, TabStop};
    use serde_json::{Value, json};

    use crate::Session;

    /// A 300 pt wide frame (no inset) holding `text`, with the caret in it.
    fn session(text: &str) -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 372, 300], "content": "text", "text": text})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        (s, sid)
    }

    fn positions(s: &mut Session) -> Vec<f64> {
        let v = s.execute("type.tabs.get", &json!({})).unwrap();
        v["tabs"].as_array().unwrap().iter().map(|t| t["position"].as_f64().unwrap()).collect()
    }

    #[test]
    fn edit_stops_of_the_selected_paragraph_with_one_undo_step_each() {
        let (mut s, _) = session("Name\tPrice");
        let r = s.execute("type.tabs.add", &json!({"position": 144})).unwrap();
        assert_eq!(r["index"], 0);
        let r = s.execute("type.tabs.add", &json!({"position": 72, "align": "center", "leader": ". "})).unwrap();
        assert_eq!(r["index"], 0, "stops stay sorted");
        assert_eq!(positions(&mut s), [72.0, 144.0]);
        let r = s.execute("type.tabs.move", &json!({"index": 0, "position": 200})).unwrap();
        assert_eq!(r["index"], 1);
        assert_eq!(positions(&mut s), [144.0, 200.0]);
        s.execute("type.tabs.change", &json!({"index": 1, "align": "char", "leader": "-"})).unwrap();
        let t = &s.execute("type.tabs.get", &json!({})).unwrap()["tabs"][1];
        assert_eq!((t["align"].as_str(), t["leader"].as_str(), t["alignOn"].as_str()), (Some("char"), Some("-"), Some(".")));
        s.execute("type.tabs.remove", &json!({"index": 0})).unwrap();
        assert_eq!(positions(&mut s), [200.0]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(positions(&mut s), [144.0, 200.0], "undo brings the stop back");
        s.execute("type.tabs.clear", &json!({})).unwrap();
        assert!(positions(&mut s).is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(positions(&mut s).len(), 2);
    }

    #[test]
    fn repeat_fills_the_column_at_the_stop_distance() {
        let (mut s, _) = session("a\tb");
        let width = s.execute("type.tabs.get", &json!({})).unwrap()["width"].as_f64().unwrap();
        s.execute("type.para", &json!({"attrs": {"leftIndent": 12}})).unwrap();
        s.execute("type.tabs.add", &json!({"position": 36, "align": "right", "leader": "."})).unwrap();
        s.execute("type.tabs.add", &json!({"position": 96})).unwrap();
        s.execute("type.tabs.repeat", &json!({"index": 1})).unwrap();
        let got = positions(&mut s);
        let want: Vec<f64> = std::iter::successors(Some(36.0), |x| Some(x + 60.0)).take_while(|x| *x <= width).collect();
        assert_eq!(got.first(), Some(&36.0));
        assert_eq!(&got[1..], &want[1..], "60 pt apart from 96 up to the column width {width}");
        // The first stop repeats its distance from the left indent (24 pt).
        s.execute("type.tabs.clear", &json!({})).unwrap();
        s.execute("type.tabs.add", &json!({"position": 36})).unwrap();
        s.execute("type.tabs.repeat", &json!({"index": 0, "width": 100})).unwrap();
        assert_eq!(positions(&mut s), [36.0, 60.0, 84.0]);
    }

    #[test]
    fn a_right_tab_ends_the_text_at_its_position() {
        let (mut s, sid) = session("Item\tEnd");
        s.execute("type.tabs.add", &json!({"position": 200, "align": "right"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = designcraft_compose::compose_story(&d, designcraft_doc::StoryId(sid), &Default::default());
        let ft = &cs.frames[0];
        let line = &ft.lines[0];
        let last = line.glyphs.iter().rfind(|g| g.visible).unwrap();
        let end = last.x + last.adv - ft.columns[0].x0;
        assert!((end - 200.0).abs() < 0.01, "text ends at {end}");
    }

    #[test]
    fn style_tabs_reach_paragraphs_that_use_the_style() {
        let (mut s, sid) = session("Left\tRight");
        s.execute("style.paragraph.create", &json!({"name": "Price List"})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Price List"})).unwrap();
        s.execute("type.tabs.add", &json!({"style": "Price List", "position": 120, "align": "right"})).unwrap();
        assert_eq!(positions(&mut s), [120.0], "the paragraph follows its style");
        let get = s.execute("type.tabs.get", &json!({"style": "Price List"})).unwrap();
        assert_eq!(get["tabs"][0]["align"], "right");
        let d = s.doc().unwrap().doc.clone();
        let story = d.story(designcraft_doc::StoryId(sid)).unwrap();
        assert_eq!(story.paras[0].para.tabs, None, "no paragraph override");
        assert!(s.execute("type.tabs.add", &json!({"style": "Nope", "position": 1})).is_err());
    }

    /// A style's stored list that was never checked (an older file) is checked when a Tabs
    /// command rewrites it, as a paragraph's is.
    #[test]
    fn style_tab_lists_are_checked_when_edited() {
        let (mut s, _) = session("x");
        s.execute("style.paragraph.create", &json!({"name": "Old"})).unwrap();
        let stop = |position, align| TabStop { position, align, leader: String::new(), align_on: String::new() };
        s.edit(|d, _| {
            let st = d.styles_mut().para_mut("Old").unwrap();
            st.para.tabs = Some(vec![stop(50.0, TabAlign::Char), stop(72.0, TabAlign::Left), stop(72.001, TabAlign::Right)]);
            Ok(())
        })
        .unwrap();
        s.execute("type.tabs.add", &json!({"style": "Old", "position": 120})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let tabs = d.styles.para("Old").unwrap().para.tabs.clone().unwrap();
        let at: Vec<(f64, TabAlign, &str)> = tabs.iter().map(|t| (t.position, t.align, t.align_on.as_str())).collect();
        assert_eq!(at, [(50.0, TabAlign::Char, "."), (72.001, TabAlign::Right, ""), (120.0, TabAlign::Left, "")]);
    }

    /// A command returns the list as it stores it, though the list it started from was never
    /// checked.
    #[test]
    fn tab_commands_return_the_stored_list() {
        let (mut s, _) = session("x");
        s.execute("style.paragraph.create", &json!({"name": "Old"})).unwrap();
        let stop = |position, align| TabStop { position, align, leader: String::new(), align_on: String::new() };
        s.edit(|d, _| {
            let st = d.styles_mut().para_mut("Old").unwrap();
            st.para.tabs = Some(vec![stop(50.0, TabAlign::Char), stop(72.0, TabAlign::Left), stop(72.001, TabAlign::Right)]);
            Ok(())
        })
        .unwrap();
        let out = s.execute("type.tabs.add", &json!({"style": "Old", "position": 120})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let stored = d.styles.para("Old").unwrap().para.tabs.clone().unwrap();
        assert_eq!(out["tabs"], serde_json::to_value(&stored).unwrap());
        assert_eq!(out["index"], 2, "the added stop, in the stored list");
    }

    /// `type.para` and the paragraph style commands take a whole `tabs` list; it gets the checks
    /// the Tabs commands apply.
    #[test]
    fn tab_lists_set_as_paragraph_attributes_are_checked() {
        let (mut s, sid) = session("x");
        let many: Vec<Value> = (0..=super::MAX_TABS).map(|i| json!({"position": i})).collect();
        for tabs in [
            json!(many),
            json!([{"position": f64::NAN}]),
            json!([{"position": "NaN"}]),
            json!([{"position": 10, "leader": "123456789"}]),
            json!([{"position": 10, "leader": "\u{7}"}]),
            json!([{"position": 10, "align": "char", "alignOn": ".,"}]),
        ] {
            assert!(s.execute("type.para", &json!({"attrs": {"tabs": tabs}})).is_err(), "{tabs}");
            assert!(s.execute("style.paragraph.create", &json!({"name": "Bad", "para": {"tabs": tabs}})).is_err(), "{tabs}");
        }
        s.execute("style.paragraph.create", &json!({"name": "Ok"})).unwrap();
        assert!(s.execute("style.paragraph.edit", &json!({"name": "Ok", "para": {"tabs": [{"position": 1, "leader": "123456789"}]}})).is_err());

        let tabs = json!([{"position": 200, "align": "char"}, {"position": 1e300}, {"position": -5, "leader": ". "}, {"position": 72}]);
        s.execute("type.para", &json!({"attrs": {"tabs": tabs}})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let stored = d.story(designcraft_doc::StoryId(sid)).unwrap().paras[0].para.tabs.clone().unwrap();
        let at: Vec<f64> = stored.iter().map(|t| t.position).collect();
        assert_eq!(at, [0.0, 72.0, 200.0, super::MAX_POSITION], "clamped and sorted");
        assert_eq!((stored[0].leader.as_str(), stored[2].align_on.as_str()), (". ", "."), "a char stop aligns on a point");
    }

    #[test]
    fn hostile_parameters_are_errors() {
        let (mut s, _) = session("x");
        for p in [
            json!({}),
            json!({"position": "NaN"}),
            json!({"position": null}),
            json!({"position": 10, "leader": "123456789"}),
            json!({"position": 10, "leader": "\n"}),
            json!({"position": 10, "alignOn": ".,"}),
            json!({"position": 10, "align": "diagonal"}),
            json!([1, 2]),
        ] {
            assert!(s.execute("type.tabs.add", &p).is_err(), "{p}");
        }
        for (c, p) in [
            ("type.tabs.move", json!({"index": 5, "position": 1})),
            ("type.tabs.move", json!({"index": -1, "position": 1})),
            ("type.tabs.change", json!({"index": 0})),
            ("type.tabs.remove", json!({"index": 18446744073709551615u64})),
            ("type.tabs.repeat", json!({"index": 0})),
        ] {
            assert!(s.execute(c, &p).is_err(), "{c} {p}");
        }
        // Huge positions clamp; the stop count is capped.
        s.execute("type.tabs.add", &json!({"position": 1e300})).unwrap();
        assert_eq!(positions(&mut s), [super::MAX_POSITION]);
        s.execute("type.tabs.add", &json!({"position": 1})).unwrap();
        s.execute("type.tabs.repeat", &json!({"index": 0, "width": 1e9})).unwrap();
        assert_eq!(positions(&mut s).len(), super::MAX_TABS);
        assert!(s.execute("type.tabs.add", &json!({"position": 0.5})).is_err());
        let many: Vec<Value> = (0..10_000).map(|i| json!({"position": i})).collect();
        let r = s.execute("type.tabs.add", &json!({"position": many}));
        assert!(r.is_err());
    }
}
