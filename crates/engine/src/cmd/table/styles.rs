use super::cell::{StrokeEdit, first_baseline_params};
use super::target::{color_param, edit_table, target};
use super::{CellStroke, Document, Result, Session, Value, bad, json, ok, str_param};

// ---------- cell and table styles ----------

/// A style's uniform edge stroke from `stroke`/`border`: the given values over the default
/// edge, validated like `table.setCell`'s stroke.
fn stroke_param(s: &Session, v: Option<&Value>, cmd: &str) -> Result<Option<CellStroke>> {
    match v.filter(|v| !v.is_null()) {
        Some(v) => Ok(Some(StrokeEdit::parse(&s.doc()?.doc, v, cmd)?.apply(CellStroke::default()))),
        None => Ok(None),
    }
}

/// Fill in the cell style fields given in `p`.
fn cell_style_fields(cs: &mut designcraft_doc::CellStyle, p: &Value, stroke: Option<CellStroke>, cmd: &str) -> Result<()> {
    if let Some(v) = p.get("basedOn") {
        cs.based_on = v.as_str().map(str::to_string);
    }
    if let Some(f) = color_param(p, "fill") {
        cs.fill = Some(f);
    }
    if let Some(t) = p.get("tint").and_then(Value::as_f64) {
        cs.fill_tint = Some(t.clamp(0.0, 1.0) as f32);
    }
    match p.get("insets") {
        Some(Value::Number(n)) => {
            cs.insets = n.as_f64().map(|v| [v.max(0.0); 4]);
            cs.inset_overrides = Default::default();
        }
        Some(Value::Array(a)) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0).max(0.0)).collect();
            cs.insets = Some([v[0], v[1], v[2], v[3]]);
            cs.inset_overrides = Default::default();
        }
        _ => {}
    }
    if let Some(v) = p.get("vj") {
        cs.vj = Some(serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("vj: {e}")))?);
    }
    let (first_baseline, first_baseline_min) = first_baseline_params(p, cmd)?;
    if first_baseline.is_some() {
        cs.first_baseline = first_baseline;
    }
    if first_baseline_min.is_some() {
        cs.first_baseline_min = first_baseline_min;
    }
    if let Some(st) = stroke {
        cs.stroke = Some(st);
        cs.strokes = Default::default();
    }
    if let Some(ps) = str_param(p, "paragraphStyle") {
        cs.paragraph_style = Some(ps.to_string());
    }
    Ok(())
}

pub(super) fn cell_style_create(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Cell Style 1").to_string();
    let mut cs = designcraft_doc::CellStyle::default();
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false) {
        let g = target(s, p, "style.cell.create")?;
        let d = &s.doc()?.doc;
        let cell = d
            .story(g.story)
            .and_then(|st| st.tables.get(&g.table))
            .and_then(|t| {
                let (r, c) = t.owner(g.range.r0, g.range.c0);
                t.cell(r, c)
            })
            .ok_or_else(|| bad("style.cell.create", "no cell"))?;
        cs.fill = Some(cell.fill.clone());
        cs.fill_tint = Some(cell.fill_tint);
        cs.insets = Some(cell.insets);
        cs.vj = Some(cell.vj);
        cs.first_baseline = Some(cell.first_baseline);
        cs.first_baseline_min = Some(cell.first_baseline_min);
        cs.stroke = Some(cell.strokes[0].clone());
        cs.paragraph_style = cell.text.paras.first().map(|p| p.style.clone());
    }
    let stroke = stroke_param(s, p.get("stroke"), "style.cell.create")?;
    cell_style_fields(&mut cs, p, stroke, "style.cell.create")?;
    s.edit(|d, _| {
        let name = designcraft_doc::Styles::unique_name(|n| d.styles.cell.iter().any(|c| c.name == n), &base);
        cs.name = name.clone();
        d.styles_mut().cell.push(cs);
        Ok(json!({"name": name}))
    })
}

pub(super) fn cell_style_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.cell.apply", "missing name"))?.to_string();
    let styles = &s.doc()?.doc.styles.cell;
    if !styles.iter().any(|c| c.name == name) {
        return Err(bad("style.cell.apply", format!("no cell style `{name}`")));
    }
    let cs = designcraft_doc::resolve_cell_style(styles, &name);
    let g = target(s, p, "style.cell.apply")?;
    edit_table(s, &g, "style.cell.apply", |t| {
        let mut n = 0;
        for (r, c) in t.selected_owners(g.range) {
            if let Some(cell) = t.cell_mut(r, c) {
                cs.apply_to(cell);
                n += 1;
            }
        }
        Ok(json!({"cells": n}))
    })
}

/// Re-apply cell styles named in `names` to every cell using them (after a style edit).
fn reapply_cell_styles(d: &mut Document, names: &[String]) {
    let styles: Vec<_> = names.iter().map(|name| designcraft_doc::resolve_cell_style(&d.styles.cell, name)).collect();
    for sid in d.stories.keys().copied().collect::<Vec<_>>() {
        let Some(st) = d.story_mut(sid) else { continue };
        for t in st.tables.values_mut() {
            if !t.cells.iter().any(|c| names.contains(&c.style)) {
                continue;
            }
            for cell in &mut std::sync::Arc::make_mut(t).cells {
                if names.contains(&cell.style)
                    && let Some(cs) = styles.iter().find(|x| x.name == cell.style)
                {
                    cs.apply_to(cell);
                }
            }
        }
        st.rev = st.rev.wrapping_add(1);
    }
}

/// Include descendants when editing an inherited style, with the resolver's depth limit.
fn dependent_styles<'a>(name: &str, styles: impl Iterator<Item = (&'a str, Option<&'a str>)>) -> Vec<String> {
    let styles: Vec<_> = styles.collect();
    styles
        .iter()
        .filter(|(candidate, _)| {
            let mut current = Some(*candidate);
            for _ in 0..32 {
                let Some(current_name) = current else { return false };
                if current_name == name {
                    return true;
                }
                current = styles.iter().find(|(n, _)| *n == current_name).and_then(|(_, base)| *base);
            }
            false
        })
        .map(|(name, _)| (*name).to_string())
        .collect()
}

pub(super) fn cell_style_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.cell.edit", "missing name"))?.to_string();
    let stroke = stroke_param(s, p.get("stroke"), "style.cell.edit")?;
    let p = p.clone();
    s.edit(|d, _| {
        let cs = d.styles_mut().cell.iter_mut().find(|c| c.name == name).ok_or_else(|| bad("style.cell.edit", format!("no cell style `{name}`")))?;
        cell_style_fields(cs, &p, stroke, "style.cell.edit")?;
        let names = dependent_styles(&name, d.styles.cell.iter().map(|cs| (cs.name.as_str(), cs.based_on.as_deref())));
        reapply_cell_styles(d, &names);
        ok()
    })
}

fn table_style_fields(ts: &mut designcraft_doc::TableStyle, p: &Value, border: Option<CellStroke>) {
    if let Some(v) = p.get("basedOn") {
        ts.based_on = v.as_str().map(str::to_string);
    }
    for (k, f) in [
        ("header", &mut ts.header),
        ("body", &mut ts.body),
        ("footer", &mut ts.footer),
        ("leftColumn", &mut ts.left_column),
        ("rightColumn", &mut ts.right_column),
    ] {
        if let Some(v) = p.get(k) {
            *f = v.as_str().map(str::to_string);
        }
    }
    if let Some(b) = border {
        ts.border = Some(b);
        ts.borders = Default::default();
    }
    if let Some(a) = p.get("altRows").and_then(Value::as_object) {
        let n = |k: &str, d: u32| a.get(k).and_then(Value::as_u64).map_or(d, |v| u32::try_from(v).unwrap_or(u32::MAX));
        let c = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or(designcraft_color::swatch::NONE).to_string();
        let t = |k: &str| a.get(k).and_then(Value::as_f64).map_or(1.0, |v| v.clamp(0.0, 1.0) as f32);
        ts.alt_rows = Some(designcraft_doc::AltFills {
            first: n("first", 1),
            first_color: c("firstColor"),
            first_tint: t("firstTint"),
            next: n("next", 1),
            next_color: c("nextColor"),
            next_tint: t("nextTint"),
            skip_first: n("skipFirst", 0),
            skip_last: n("skipLast", 0),
        });
    }
    if let Some(v) = p.get("spaceBefore").and_then(Value::as_f64) {
        ts.space_before = Some(v);
    }
    if let Some(v) = p.get("spaceAfter").and_then(Value::as_f64) {
        ts.space_after = Some(v);
    }
}

pub(super) fn table_style_create(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Table Style 1").to_string();
    let mut ts = designcraft_doc::TableStyle::default();
    let border = stroke_param(s, p.get("border"), "style.table.create")?;
    table_style_fields(&mut ts, p, border);
    s.edit(|d, _| {
        let name = designcraft_doc::Styles::unique_name(|n| d.styles.table.iter().any(|c| c.name == n), &base);
        ts.name = name.clone();
        d.styles_mut().table.push(ts);
        Ok(json!({"name": name}))
    })
}

pub(super) fn table_style_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.table.apply", "missing name"))?.to_string();
    let doc = &s.doc()?.doc;
    if !doc.styles.table.iter().any(|c| c.name == name) {
        return Err(bad("style.table.apply", format!("no table style `{name}`")));
    }
    let ts = doc.styles.resolve_table_style(&name);
    let cells = doc.styles.cell.clone();
    let g = target(s, p, "style.table.apply")?;
    edit_table(s, &g, "style.table.apply", |t| {
        ts.apply_to(t, &cells);
        ok()
    })
}

pub(super) fn table_style_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.table.edit", "missing name"))?.to_string();
    let border = stroke_param(s, p.get("border"), "style.table.edit")?;
    let p = p.clone();
    s.edit(|d, _| {
        let ts =
            d.styles_mut().table.iter_mut().find(|c| c.name == name).ok_or_else(|| bad("style.table.edit", format!("no table style `{name}`")))?;
        table_style_fields(ts, &p, border);
        let names = dependent_styles(&name, d.styles.table.iter().map(|ts| (ts.name.as_str(), ts.based_on.as_deref())));
        let styles: Vec<_> = names.iter().map(|name| d.styles.resolve_table_style(name)).collect();
        let cells = d.styles.cell.clone();
        for sid in d.stories.keys().copied().collect::<Vec<_>>() {
            let Some(st) = d.story_mut(sid) else { continue };
            for t in st.tables.values_mut() {
                if let Some(ts) = styles.iter().find(|ts| ts.name == t.style) {
                    ts.apply_to(std::sync::Arc::make_mut(t), &cells);
                }
            }
            st.rev = st.rev.wrapping_add(1);
        }
        ok()
    })
}

#[cfg(test)]
mod style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn cell_first_baseline_is_a_cell_option_and_a_cell_style_attribute() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        s.execute("table.setCell", &json!({"row": 0, "col": 0, "firstBaseline": "capHeight", "firstBaselineMin": 3})).unwrap();
        let got = s.execute("table.get", &json!({})).unwrap();
        let cell = got["cells"].as_array().unwrap().iter().find(|c| c["row"] == 0 && c["col"] == 0).unwrap().clone();
        assert_eq!((cell["firstBaseline"].as_str(), cell["firstBaselineMin"].as_f64()), (Some("capHeight"), Some(3.0)));
        assert!(s.execute("table.setCell", &json!({"row": 0, "col": 0, "firstBaseline": "middle"})).is_err());
        // Cell Options › Text › Rotation: carried and written back, composed horizontally so far,
        // which the IDML import reports (#1468).
        for (given, want) in [(90, 90.0), (270, 270.0), (0, 0.0), (-90, 270.0), (450, 90.0)] {
            s.execute("table.setCell", &json!({"row": 0, "col": 0, "rotation": given})).unwrap_or_else(|e| panic!("{given}: {e}"));
            let cell = s.execute("table.get", &json!({})).unwrap()["cells"][0].clone();
            assert_eq!(cell["rotation"].as_f64(), Some(want), "{given}");
        }
        for bad in [45, 1, 100] {
            assert!(s.execute("table.setCell", &json!({"row": 0, "col": 0, "rotation": bad})).is_err(), "{bad}");
        }
        assert!(s.execute("table.setCell", &json!({"row": 0, "col": 0, "firstBaselineMin": -1})).is_err());
        s.execute("style.cell.create", &json!({"name": "Leading", "firstBaseline": "leading"})).unwrap();
        s.execute("style.cell.apply", &json!({"name": "Leading", "row": 1, "col": 1})).unwrap();
        let d = &s.doc().unwrap().doc;
        let t = d.story(designcraft_doc::StoryId(sid)).unwrap().tables.values().next().unwrap();
        assert_eq!(t.cell(1, 1).unwrap().first_baseline, designcraft_doc::FirstBaseline::Leading);
        assert_eq!(t.cell(1, 0).unwrap().first_baseline, designcraft_doc::FirstBaseline::Ascent);
    }

    #[test]
    fn cell_and_table_styles_apply_and_follow_edits() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Cell Head"})).unwrap();
        s.execute("style.cell.create", &json!({"name": "Head", "fill": "[Black]", "paragraphStyle": "Cell Head", "insets": 6})).unwrap();
        s.execute("style.cell.create", &json!({"name": "Body", "fill": "[Paper]"})).unwrap();
        s.execute("style.table.create", &json!({"name": "Data", "header": "Head", "body": "Body", "border": {"weight": 2, "color": "[Black]"}}))
            .unwrap();
        s.execute("style.table.apply", &json!({"name": "Data"})).unwrap();
        let t = |s: &Session| -> designcraft_doc::Table {
            let d = &s.doc().unwrap().doc;
            (**d.story(designcraft_doc::StoryId(sid)).unwrap().tables.values().next().unwrap()).clone()
        };
        let tb = t(&s);
        assert_eq!(tb.style, "Data");
        assert_eq!(tb.options.border.weight, 2.0);
        let head = tb.cell(0, 0).unwrap();
        assert_eq!((head.fill.as_str(), head.style.as_str(), head.insets), ("[Black]", "Head", [6.0; 4]));
        assert_eq!(head.text.paras[0].style, "Cell Head");
        assert_eq!(tb.cell(1, 1).unwrap().style, "Body");
        // Editing the cell style updates its cells.
        s.execute("style.cell.edit", &json!({"name": "Head", "fill": "C=100 M=0 Y=0 K=0"})).unwrap();
        assert_eq!(t(&s).cell(0, 1).unwrap().fill, "C=100 M=0 Y=0 K=0");
        // Editing the table style re-applies it.
        s.execute("style.table.edit", &json!({"name": "Data", "border": {"weight": 4}})).unwrap();
        assert_eq!(t(&s).options.border.weight, 4.0);
        assert!(s.execute("style.cell.apply", &json!({"name": "Nope"})).is_err());
    }
}

#[cfg(test)]
mod style_alt_rows_tests {
    use serde_json::json;

    use super::table_style_fields;

    #[test]
    fn style_alt_rows_counts_saturate() {
        let mut ts = designcraft_doc::TableStyle::default();
        let p = json!({"altRows": {"first": 4294967297u64, "next": 4294967296u64, "skipFirst": 4294967295u64, "skipLast": 3}});
        table_style_fields(&mut ts, &p, None);
        let a = ts.alt_rows.unwrap();
        assert_eq!(a.first, u32::MAX, "first");
        assert_eq!(a.next, u32::MAX, "next");
        assert_eq!(a.skip_first, u32::MAX, "skip_first");
        assert_eq!(a.skip_last, 3, "skip_last");
    }
}
