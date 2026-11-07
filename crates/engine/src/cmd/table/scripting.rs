//! Table commands for scripts, which name a table instead of putting the caret in it:
//! `table.list`, `table.setWidth` and `table.paragraphStyle`.

use super::sizing::{MAX_TRACK, MIN_TRACK};
use super::target::{edit_table, target};
use super::{Result, Session, StoryId, Value, bad, json, ok, str_param};

pub(super) fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let only_story = p.get("story").and_then(Value::as_u64).map(StoryId);
    let only_frame = p.get("frame").and_then(Value::as_u64);
    let d = s.doc()?.doc.clone();
    let mut out = Vec::new();
    for st in d.stories.values().filter(|st| !st.tables.is_empty() && only_story.is_none_or(|o| o == st.id)) {
        // Where each table is set: the frame and column of its first fragment.
        let cs = s.cache.get(&d, st.id, None);
        let placed = |tid: u64| cs.frames.iter().find_map(|ft| ft.tables.iter().find(|f| f.table == tid).map(|f| (ft.frame.0, f.column)));
        for (tid, t) in &st.tables {
            let at = placed(*tid);
            if only_frame.is_some_and(|f| at.map(|a| a.0) != Some(f)) {
                continue;
            }
            out.push(json!({
                "story": st.id.0,
                "table": tid,
                "rows": t.nrows(),
                "columns": t.ncols(),
                "width": t.columns.iter().map(|c| c.width).sum::<f64>(),
                "style": t.style,
                "anchor": st.table_anchor(*tid),
                "frame": at.map(|a| a.0),
                "column": at.map(|a| a.1),
            }));
        }
    }
    Ok(Value::Array(out))
}

pub(super) fn set_width(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.setWidth";
    let w = p.get("width").and_then(Value::as_f64).ok_or_else(|| bad(ID, "missing width"))?;
    let g = target(s, p, ID)?;
    edit_table(s, &g, ID, |t| {
        let n = t.ncols().max(1) as f64;
        let total: f64 = t.columns.iter().map(|c| c.width).sum();
        let (narrowest, widest) = t.columns.iter().fold((f64::INFINITY, 0.0_f64), |(lo, hi), c| (lo.min(c.width), hi.max(c.width)));
        // Each column keeps its share, so the narrowest and the widest one bound the width: no
        // column goes below or above what table.setColumnWidth allows. Columns without a width
        // share it evenly.
        let shares = total > 0.0 && narrowest > 0.0;
        let (lo, hi) = if shares { (MIN_TRACK * total / narrowest, MAX_TRACK * total / widest) } else { (MIN_TRACK * n, MAX_TRACK * n) };
        if !w.is_finite() || w < lo - 1e-9 || w > hi + 1e-9 {
            return Err(bad(ID, format!("width must be from {lo:.3} to {hi:.3} points for these columns")));
        }
        for c in &mut t.columns {
            c.width = if shares { c.width / total * w } else { w / n };
        }
        ok()
    })
}

pub(super) fn paragraph_style(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.paragraphStyle";
    let name = str_param(p, "name").ok_or_else(|| bad(ID, "missing name"))?.to_string();
    if s.doc()?.doc.styles.para(&name).is_none() {
        return Err(bad(ID, format!("no paragraph style `{name}`")));
    }
    let clear = p.get("clearOverrides").and_then(Value::as_bool).unwrap_or(false);
    let g = target(s, p, ID)?;
    edit_table(s, &g, ID, |t| {
        for cell in &mut t.cells {
            let len = cell.text.text.len();
            cell.text.format_paras(0..len, |f| {
                f.style = name.clone();
                if clear {
                    f.para = Default::default();
                    f.chars = Default::default();
                }
            });
            if clear {
                cell.text.format_chars(0..len, |f| f.over = Default::default());
            }
        }
        ok()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn tables_are_listed_sized_and_styled_without_a_caret_in_them() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        let (frame, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 3})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Grundtext"})).unwrap();
        s.execute("style.table.create", &json!({"name": "Grundtabelle"})).unwrap();
        // The caret goes elsewhere: scripts name the table.
        let other = s.execute("frame.create", &json!({"rect": [72, 450, 300, 500], "content": "text", "text": "elsewhere"})).unwrap();
        s.execute("text.select", &json!({"story": other["story"], "anchor": 0, "focus": 0})).unwrap();

        let list = s.execute("table.list", &json!({"frame": frame})).unwrap();
        let t = &list[0];
        assert_eq!(
            (t["story"].as_u64(), t["rows"].as_u64(), t["columns"].as_u64(), t["frame"].as_u64()),
            (Some(sid), Some(2), Some(3), Some(frame)),
            "{list}"
        );
        assert_eq!(t["column"], 0);
        let tid = t["table"].as_u64().unwrap();
        assert_eq!(s.execute("table.list", &json!({"frame": other["id"]})).unwrap(), json!([]), "another frame's tables");

        // Width: every column scaled, shares kept.
        s.execute("table.setColumnWidth", &json!({"story": sid, "table": tid, "cols": [0, 0], "width": 100})).unwrap();
        s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": 300})).unwrap();
        let got = s.execute("table.get", &json!({"story": sid, "table": tid})).unwrap();
        let widths: Vec<f64> = got["columns"].as_array().unwrap().iter().filter_map(|c| c["width"].as_f64().or_else(|| c.as_f64())).collect();
        assert!((widths.iter().sum::<f64>() - 300.0).abs() < 1e-6, "{widths:?}");
        // The 100 pt column keeps its share against the others (100 : 428/3 before).
        assert!((widths[0] / widths[1] - 100.0 / (428.0 / 3.0)).abs() < 1e-3, "{widths:?}");
        assert_eq!(s.execute("table.list", &json!({})).unwrap()[0]["width"], 300.0);
        assert!(s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": 2})).is_err(), "narrower than its columns can be");

        // Styles by name, on the named table.
        s.execute("style.table.apply", &json!({"name": "Grundtabelle", "story": sid, "table": tid})).unwrap();
        s.execute("table.paragraphStyle", &json!({"name": "Grundtext", "story": sid, "table": tid})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let table = d.story(designcraft_doc::StoryId(sid)).unwrap().tables.get(&tid).unwrap().clone();
        assert_eq!(table.style, "Grundtabelle");
        assert!(table.cells.iter().all(|c| c.text.paras.iter().all(|p| p.style == "Grundtext")));
        assert!(s.execute("table.paragraphStyle", &json!({"name": "Nope", "story": sid, "table": tid})).is_err());
        assert!(s.execute("table.setWidth", &json!({"table": 999_999, "width": 300})).is_err());
    }

    #[test]
    fn a_table_width_keeps_every_column_within_what_a_column_may_be() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
        let tid = s.execute("table.list", &json!({})).unwrap()[0]["table"].as_u64().unwrap();
        let widths = |s: &mut Session| -> Vec<f64> {
            let got = s.execute("table.get", &json!({"story": sid, "table": tid})).unwrap();
            got["columns"].as_array().unwrap().iter().filter_map(|c| c["width"].as_f64().or_else(|| c.as_f64())).collect()
        };
        // Columns of 3 and 997 pt (the review's case): 6 pt wide would make the first 0.018 pt.
        s.execute("table.setColumnWidth", &json!({"story": sid, "table": tid, "cols": [0, 0], "width": 3})).unwrap();
        s.execute("table.setColumnWidth", &json!({"story": sid, "table": tid, "cols": [1, 1], "width": 997})).unwrap();
        assert!(s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": 6})).is_err());
        assert!(s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": 999})).is_err());
        s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": 1000})).unwrap();
        // Nor above table.setColumnWidth's 10000 pt: 997 pt may grow to 10000, the table to 10030.
        assert!(s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": 10_100})).is_err());
        s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": 10_030})).unwrap();
        let w = widths(&mut s);
        assert!(w.iter().all(|c| (3.0 - 1e-9..=10_000.0 + 1e-9).contains(c)), "{w:?}");
        assert!(s.execute("table.setWidth", &json!({"story": sid, "table": tid, "width": f64::NAN})).is_err());
    }
}
