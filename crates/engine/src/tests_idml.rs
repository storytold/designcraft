//! IDML round-trip tests (the sample magazine) and the interchange commands.

use designcraft_doc::{Document, ItemId};
use serde_json::json;

use crate::Session;

/// Every item of a spread list in order, flattened (pre-order).
fn items(spreads: &[std::sync::Arc<designcraft_doc::Spread>]) -> Vec<(ItemId, designcraft_geom::Rect)> {
    let mut v = Vec::new();
    for sp in spreads {
        for it in &sp.items {
            it.walk(&mut |i| v.push((i.id, i.bounds())));
        }
    }
    v
}

/// Thread order of every story as positions in the flattened item list.
fn positions(d: &Document) -> Vec<Vec<usize>> {
    let all: Vec<ItemId> = items(&d.spreads).into_iter().chain(items(&d.parents)).map(|(id, _)| id).collect();
    d.stories.values().map(|s| s.frames.iter().map(|f| all.iter().position(|i| i == f).expect("frame")).collect()).collect()
}

#[test]
fn magazine_round_trips_through_idml() {
    let d = crate::sample::magazine();
    let bytes = designcraft_idml::export_idml(&d);
    let back = designcraft_idml::import_idml(&bytes).unwrap();
    assert_eq!(back.page_count(), d.page_count());
    assert_eq!(back.spreads.len(), d.spreads.len());
    assert_eq!(back.parents.len(), d.parents.len());
    // Stories: same text in the same order, same styles applied.
    let ta: Vec<&str> = d.stories.values().map(|s| s.text.as_str()).collect();
    let tb: Vec<&str> = back.stories.values().map(|s| s.text.as_str()).collect();
    assert_eq!(ta, tb);
    for (a, b) in d.stories.values().zip(back.stories.values()) {
        let sa: Vec<&str> = a.paras.iter().map(|p| p.style.as_str()).collect();
        let sb: Vec<&str> = b.paras.iter().map(|p| p.style.as_str()).collect();
        assert_eq!(sa, sb);
        let ca: Vec<(usize, &str)> = a.chars.iter().map(|r| (r.len, r.format.style.as_str())).collect();
        let cb: Vec<(usize, &str)> = b.chars.iter().map(|r| (r.len, r.format.style.as_str())).collect();
        assert_eq!(ca, cb);
        for (ra, rb) in a.chars.iter().zip(&b.chars) {
            assert_eq!(ra.format.over, rb.format.over);
        }
        for (pa, pb) in a.paras.iter().zip(&b.paras) {
            assert_eq!(pa.para, pb.para);
        }
    }
    // Styles and swatches by name.
    for s in &d.styles.paragraph {
        let b = back.styles.para(&s.name).unwrap_or_else(|| panic!("paragraph style {}", s.name));
        if s.name != designcraft_doc::NO_PARA_STYLE {
            assert_eq!(b.chars, s.chars, "{}", s.name);
            assert_eq!(b.para, s.para, "{}", s.name);
        }
    }
    for s in &d.styles.character {
        let b = back.styles.char_style(&s.name).unwrap_or_else(|| panic!("character style {}", s.name));
        assert_eq!(b.chars, s.chars);
    }
    for s in &d.swatches {
        let b = back.swatch(&s.name).unwrap_or_else(|| panic!("swatch {}", s.name));
        assert_eq!(b.value, s.value, "{}", s.name);
    }
    // Frame geometry (document and parent spreads), pages.
    for (sa, sb) in [(&d.spreads, &back.spreads), (&d.parents, &back.parents)] {
        let (ia, ib) = (items(sa), items(sb));
        assert_eq!(ia.len(), ib.len());
        for ((_, a), (_, b)) in ia.iter().zip(&ib) {
            let close = (a.x0 - b.x0).abs() < 0.01 && (a.y0 - b.y0).abs() < 0.01 && (a.x1 - b.x1).abs() < 0.01 && (a.y1 - b.y1).abs() < 0.01;
            assert!(close, "{a:?} vs {b:?}");
        }
        for (pa, pb) in sa.iter().flat_map(|s| s.pages.iter()).zip(sb.iter().flat_map(|s| s.pages.iter())) {
            assert!((pa.x - pb.x).abs() < 0.01 && pa.side == pb.side && pa.margins == pb.margins && pa.columns == pb.columns);
            assert_eq!(pa.parent.is_some(), pb.parent.is_some());
        }
    }
    // Threads: the same frames in the same order.
    assert_eq!(positions(&d), positions(&back));
    assert!(back.stories.values().any(|s| s.frames.len() == 2));
    // Images are embedded.
    assert_eq!(back.assets.len(), d.assets.len());
    for (a, b) in d.assets.values().zip(back.assets.values()) {
        assert_eq!(a.data, b.data);
        assert_eq!(a.pixels, b.pixels);
    }
    // A second round trip is stable.
    let again = designcraft_idml::import_idml(&designcraft_idml::export_idml(&back)).unwrap();
    assert_eq!(items(&again.spreads).len(), items(&back.spreads).len());
}

#[test]
fn idml_commands() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    let r = s.execute("file.exportIdml", &json!({})).unwrap();
    let b64 = r["base64"].as_str().unwrap().to_string();
    let r = s.execute("file.openIdml", &json!({"base64": b64, "name": "copy.idml"})).unwrap();
    let i = r["index"].as_u64().unwrap() as usize;
    assert_eq!(s.doc().unwrap().doc.title, "copy");
    assert_eq!(s.doc().unwrap().doc.page_count(), 4);
    assert!(s.doc().unwrap().path.is_none());
    // openBytes recognises the package too.
    let r = s.execute("file.openBytes", &json!({"base64": b64, "name": "again.idml"})).unwrap();
    assert_eq!(r["index"].as_u64().unwrap() as usize, i + 1);
    assert!(s.execute("file.openIdml", &json!({})).is_err());
    let dir = std::env::temp_dir().join(format!("dc-idml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("mag.idml").to_string_lossy().to_string();
    let r = s.execute("file.exportIdml", &json!({"path": p})).unwrap();
    assert!(r["bytes"].as_u64().unwrap() > 1000);
    s.execute("file.open", &json!({"path": p})).unwrap();
    assert_eq!(s.doc().unwrap().doc.title, "mag");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gradient_vector_round_trips_through_idml() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200]})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.gradient", &json!({"from": [120, 180], "to": [280, 120]})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let v = d.item(ItemId(id)).unwrap().fill.gradient_vector.unwrap();
    let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
    let it = back.spreads.iter().flat_map(|sp| sp.items.iter()).find(|i| i.fill.gradient_vector.is_some()).expect("a gradient item");
    let w = it.fill.gradient_vector.unwrap();
    for k in 0..4 {
        assert!((v[k] - w[k]).abs() < 1e-3, "{v:?} vs {w:?}");
    }
}

#[test]
fn open_type_features_round_trip_through_idml() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "1/2 Office"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
    s.execute("type.openType", &json!({"feature": "frac"})).unwrap();
    s.execute("type.openType", &json!({"figures": "tabularOldstyle", "stylisticSets": [3]})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
    let st = back.stories.values().find(|st| st.text.starts_with("1/2")).unwrap();
    let f = st.format_after(0).over.otf_features.clone().unwrap_or_default();
    assert!(designcraft_doc::otf::is_on(&f, "frac"), "{f:?}");
    assert_eq!(designcraft_doc::otf::figures(&f), "tabularOldstyle");
    assert_eq!(designcraft_doc::otf::stylistic_sets(&f), 0b100);
    // Text after the selection has no features of its own.
    let after = st.format_after(5).over.otf_features.clone().unwrap_or_default();
    assert!(!designcraft_doc::otf::is_on(&after, "frac") && designcraft_doc::otf::stylistic_sets(&after) == 0, "{after:?}");
}

#[test]
fn underline_options_render_and_round_trip() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Underlined words"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 10})).unwrap();
    s.execute("type.char", &json!({"attrs": {"underline": true, "underlineWeight": 3, "underlineOffset": 4, "underlineColor": "C=100 M=0 Y=0 K=0"}}))
        .unwrap();
    let d = s.doc().unwrap().doc.clone();
    let cs = designcraft_compose::compose_story(&d, designcraft_doc::StoryId(sid), &Default::default());
    let st = cs.styles.iter().find(|st| st.underline).unwrap();
    assert_eq!((st.underline_rule.weight, st.underline_rule.offset), (3.0, 4.0));
    assert_eq!(st.underline_rule.color, "C=100 M=0 Y=0 K=0");
    // Drawn in cyan, 4–7 pt under the baseline.
    let l = &cs.frames[0].lines[0];
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = 0;
    let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
    let x = l.glyphs[2].x as u32;
    let p = img.pixel(x, (l.baseline + 5.5) as u32);
    assert!(p[0] < 80 && p[2] > 150, "cyan underline: {p:?}");
    let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
    let story = back.stories.values().find(|x| x.text.starts_with("Underlined")).unwrap();
    let f = story.format_after(0);
    assert_eq!(f.over.underline_weight, Some(Some(3.0)));
    assert_eq!(f.over.underline_offset, Some(Some(4.0)));
    assert_eq!(f.over.underline_color.as_deref(), Some("C=100 M=0 Y=0 K=0"));
}

#[test]
fn newer_features_round_trip_through_idml() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    // Nested and GREP styles on a paragraph style.
    s.execute("style.character.create", &json!({"name": "Lead", "chars": {"fontStyle": "Bold"}})).unwrap();
    s.execute(
        "style.paragraph.create",
        &json!({"name": "Intro", "para": {
            "nestedStyles": [{"style": "Lead", "through": false, "count": 2, "until": {"kind": "chars", "chars": ":"}}],
            "grepStyles": [{"style": "Lead", "pattern": "\\d+"}]
        }}),
    )
    .unwrap();
    // Cell and table styles on a table.
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 300], "content": "text", "text": ""})).unwrap();
    s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
    s.execute("table.insert", &json!({"rows": 2, "cols": 2, "headerRows": 1})).unwrap();
    s.execute("style.cell.create", &json!({"name": "Head", "fill": "[Black]", "insets": 5, "paragraphStyle": "Intro"})).unwrap();
    s.execute("style.table.create", &json!({"name": "Grid", "header": "Head", "border": {"weight": 3, "color": "[Black]"}})).unwrap();
    s.execute("style.table.apply", &json!({"name": "Grid"})).unwrap();
    // Type on a path.
    let line = s.execute("line.create", &json!({"a": [100, 500], "b": [400, 500]})).unwrap()["id"].as_u64().unwrap();
    s.execute("type.onPath", &json!({"id": line, "text": "On the line", "start": 12, "flip": true, "align": "center"})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
    let intro = back.styles.para("Intro").unwrap();
    let ns = intro.para.nested_styles.clone().unwrap();
    assert_eq!(ns[0].style, "Lead");
    assert_eq!((ns[0].through, ns[0].count), (false, 2));
    assert_eq!(ns[0].until, designcraft_doc::NestedUntil::Chars(":".into()));
    assert_eq!(intro.para.grep_styles.clone().unwrap()[0].pattern, "\\d+");
    let head = back.styles.cell.iter().find(|c| c.name == "Head").unwrap();
    assert_eq!((head.fill.as_deref(), head.insets, head.paragraph_style.as_deref()), (Some("[Black]"), Some([5.0; 4]), Some("Intro")));
    let grid = back.styles.table.iter().find(|t| t.name == "Grid").unwrap();
    assert_eq!(grid.header.as_deref(), Some("Head"));
    assert_eq!(grid.border.as_ref().unwrap().weight, 3.0);
    let t = back.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
    assert_eq!(t.style, "Grid");
    assert_eq!(t.cell(0, 0).unwrap().style, "Head");
    let pt = back
        .spreads
        .iter()
        .flat_map(|sp| sp.items.iter())
        .find_map(|it| match &it.content {
            designcraft_doc::Content::Text(tf) => tf.options.path.clone().map(|p| (p, tf.story)),
            _ => None,
        })
        .expect("type on a path");
    assert_eq!((pt.0.start, pt.0.flip, pt.0.align), (12.0, true, designcraft_doc::PathAlign::Center));
    assert_eq!(back.story(pt.1).unwrap().text, "On the line");
}

#[test]
fn named_lists_round_trip_through_idml() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("list.define", &json!({"name": "Steps", "continueAcrossStories": false})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One\nTwo"})).unwrap();
    s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 3})).unwrap();
    s.execute("type.para", &json!({"attrs": {"listType": "numbers", "listName": "Steps", "startAt": 5}})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
    assert_eq!(back.settings.lists, vec![designcraft_doc::NumberedList { name: "Steps".into(), continue_across_stories: false }]);
    let st = back.stories.values().find(|st| st.text.starts_with("One")).unwrap();
    assert_eq!(st.paras[0].para.list_name.as_deref(), Some("Steps"));
    assert_eq!(st.paras[0].para.start_at, Some(Some(5)));
}

#[test]
fn opens_indesign_documents() {
    let indd = designcraft_indd::synthetic::sample().build();
    let mut s = Session::new();
    let b64 = crate::cmd::base64_encode(&indd);
    s.execute("file.openBytes", &json!({"base64": b64, "name": "card.indd"})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.title, "card");
    assert_eq!(d.page_count(), 1);
    let texts: Vec<&str> = d.stories.values().map(|st| st.text.as_str()).collect();
    assert!(texts.iter().any(|t| t.contains("Hello") && t.contains("World")), "{texts:?}");
    // file.open routes the extension too.
    let dir = std::env::temp_dir().join(format!("dc-indd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("card.indd");
    std::fs::write(&p, &indd).unwrap();
    s.execute("file.open", &json!({"path": p.to_string_lossy()})).unwrap();
    assert_eq!(s.doc().unwrap().doc.title, "card");
    let _ = std::fs::remove_dir_all(&dir);
    // Damaged documents are an error, not a crash.
    let mut broken = indd.clone();
    broken.truncate(8192);
    let b64 = crate::cmd::base64_encode(&broken);
    assert!(s.execute("file.openBytes", &json!({"base64": b64, "name": "broken.indd"})).is_err());
}
