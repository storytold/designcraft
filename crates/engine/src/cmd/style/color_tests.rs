use super::*;

fn fill_session() -> Session {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let frame = session.execute("frame.create", &json!({"rect": [0, 0, 200, 100], "content": "text", "text": "Hi"})).unwrap();
    session.execute("text.select", &json!({"story": frame["story"], "anchor": 0, "focus": 2})).unwrap();
    session.execute("style.paragraph.create", &json!({"name": "Existing"})).unwrap();
    session.execute("style.character.create", &json!({"name": "Existing"})).unwrap();
    session.execute("type.char", &json!({"size": 18})).unwrap();
    session.execute("edit.undo", &json!({})).unwrap();
    session
}

fn assert_fill_rejected(session: &mut Session, command: &str, params: Value) {
    let before = session.doc().unwrap().clone();
    let journal = session.journal.clone();
    let fill = params
        .pointer("/chars/fill")
        .or_else(|| params.pointer("/attrs/fill"))
        .or_else(|| params.get("fill"))
        .and_then(Value::as_str)
        .unwrap_or("Missing Ink");

    let result = session.execute(command, &params);

    match result {
        Err(crate::EngineError::BadParams { cmd, msg }) => {
            assert_eq!(cmd, command);
            assert!(msg.contains(fill), "{msg}");
            assert!(msg.contains("swatch.create") && msg.contains("returned name"), "{msg}");
        }
        other => panic!("expected BadParams for {command}, got {other:?}"),
    }
    let after = session.doc().unwrap();
    assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.selection, after.selection);
    assert_eq!(before.revision, after.revision);
    assert_eq!(format!("{:?}", before.history), format!("{:?}", after.history));
    assert_eq!(journal, session.journal);
}

#[test]
fn char_fill_rejects_unknown_flat_and_nested_names_when_formatting() {
    for fill in ["Missing Ink", "#ff0000", "[black]", " [Black]"] {
        for params in [json!({"fill": fill, "size": 24}), json!({"attrs": {"fill": fill, "size": 24}})] {
            let mut session = fill_session();
            assert_fill_rejected(&mut session, "type.char", params);
        }
    }
}

#[test]
fn char_fill_rejects_unknown_names_when_creating_or_editing_styles() {
    for command in ["style.paragraph.create", "style.paragraph.edit", "style.character.create", "style.character.edit"] {
        for fill in ["Missing Ink", "#ff0000"] {
            let mut session = fill_session();
            session.execute("style.paragraph.apply", &json!({"name": "Existing"})).unwrap();
            session.execute("style.character.apply", &json!({"name": "Existing"})).unwrap();
            let mut params = json!({"name": "Existing", "chars": {"fill": fill, "size": 24}});
            // Only the edit commands take `rename`.
            if command.ends_with(".edit") {
                params["rename"] = json!("Renamed");
            }
            assert_fill_rejected(&mut session, command, params);
        }
    }
}

#[test]
fn char_fill_preserves_deleted_swatch_when_creating_from_selection() {
    for params in [json!({"name": "Inherited", "fromSelection": true}), json!({"name": "Inherited", "fromSelection": true, "chars": {"size": 24}})] {
        let mut session = fill_session();
        let swatch = session.execute("swatch.create", &json!({"name": "Deleted Ink", "color": "#108080"})).unwrap();
        let fill = swatch["name"].as_str().unwrap();
        session.execute("type.char", &json!({"fill": fill})).unwrap();
        session.execute("swatch.delete", &json!({"name": fill})).unwrap();
        assert_eq!(session.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], fill);

        session.execute("style.paragraph.create", &params).unwrap();

        let chars = &session.doc().unwrap().doc.styles.para("Inherited").unwrap().chars;
        assert_eq!(chars.fill.as_deref(), Some(fill));
        if params.get("chars").is_some() {
            assert_eq!(chars.size, Some(24.0));
        }
    }
}

#[test]
fn char_fill_rejects_explicit_unknown_name_when_creating_from_selection() {
    for fill in ["Missing Ink", "#ff0000"] {
        let mut session = fill_session();

        assert_fill_rejected(
            &mut session,
            "style.paragraph.create",
            json!({"name": "Explicit", "fromSelection": true, "chars": {"fill": fill, "size": 24}}),
        );
    }
}

#[test]
fn char_fill_accepts_valid_override_when_selection_has_unknown_name() {
    let mut session = fill_session();
    session
        .edit(|document, selection| {
            let selected = selection.text.unwrap();
            document.story_mut(selected.story).unwrap().format_chars(selected.range(), |format| format.over.fill = Some("Missing Ink".into()));
            Ok(())
        })
        .unwrap();

    session.execute("style.paragraph.create", &json!({"name": "Override", "fromSelection": true, "chars": {"fill": "[Paper]"}})).unwrap();

    assert_eq!(session.doc().unwrap().doc.styles.para("Override").unwrap().chars.fill.as_deref(), Some("[Paper]"));
}

#[test]
fn char_fill_rejects_unknown_name_when_changing_text_even_without_output() {
    for (find, change) in [("Hi", "Bye"), ("Hi", ""), ("Absent", "Bye")] {
        for fill in ["Missing Ink", "#ff0000"] {
            let mut session = fill_session();
            assert_fill_rejected(&mut session, "find.change", json!({"find": find, "change": change, "attrs": {"fill": fill}}));
        }
    }
}

#[test]
fn char_fill_preserves_exact_registered_names_when_used_at_all_boundaries() {
    for fill in ["Brand Ink", "#ff0000", "[None]", "[Black]"] {
        let mut session = fill_session();
        if !fill.starts_with('[') {
            let created = session.execute("swatch.create", &json!({"name": fill, "color": "#108080"})).unwrap();
            assert_eq!(created["name"], fill);
        }
        for params in [json!({"fill": fill}), json!({"attrs": {"fill": fill}})] {
            session.execute("type.char", &params).unwrap();
            assert_eq!(session.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], fill);
        }
        for kind in ["paragraph", "character"] {
            session.execute(&format!("style.{kind}.create"), &json!({"name": "Colored", "chars": {"fill": fill}})).unwrap();
            session.execute(&format!("style.{kind}.edit"), &json!({"name": "Existing", "chars": {"fill": fill}})).unwrap();
            let document = &session.doc().unwrap().doc;
            for name in ["Colored", "Existing"] {
                let attrs =
                    if kind == "paragraph" { &document.styles.para(name).unwrap().chars } else { &document.styles.char_style(name).unwrap().chars };
                assert_eq!(attrs.fill.as_deref(), Some(fill));
            }
        }
        assert_eq!(session.execute("find.change", &json!({"find": "Hi", "change": "Bye", "attrs": {"fill": fill}})).unwrap()["count"], 1);
        let selected = session.doc().unwrap().selection.text.unwrap();
        let story = session.doc().unwrap().doc.story(selected.story).unwrap();
        assert_eq!(story.text, "Bye");
        assert_eq!(story.format_after(0).over.fill.as_deref(), Some(fill));
    }
}

#[test]
fn char_fill_accepts_omitted_and_null_attributes_when_used_at_all_boundaries() {
    for chars in [json!({"size": 18}), json!({"fill": null})] {
        let mut session = fill_session();
        session.execute("type.char", &json!({"attrs": chars})).unwrap();
        for kind in ["paragraph", "character"] {
            session.execute(&format!("style.{kind}.create"), &json!({"name": "Nullable", "chars": chars})).unwrap();
            session.execute(&format!("style.{kind}.edit"), &json!({"name": "Existing", "chars": chars})).unwrap();
        }
        assert_eq!(session.execute("find.change", &json!({"find": "Hi", "change": "Bye", "attrs": chars})).unwrap()["count"], 1);
    }
}

#[test]
fn percent_components_at_most_one_stay_percent() {
    use designcraft_color::Color;
    // {"c": 0.7, ..., "y": 12.2} is a pale cream (C 0.7 %, Y 12.2 %), not C 70 %.
    let Some(Color::Cmyk { c, m, y, k }) = parse_color(&json!({"c": 0.7, "m": 0, "y": 12.2, "k": 0})) else { panic!("not CMYK") };
    assert!((c - 0.007).abs() < 1e-6 && m == 0.0 && (y - 0.122).abs() < 1e-6 && k == 0.0, "got {c} {m} {y} {k}");
    // Fractions take the colour's own form, with its model.
    let Some(Color::Cmyk { c, k, .. }) = parse_color(&json!({"model": "cmyk", "c": 0.5, "m": 0, "y": 0, "k": 1})) else { panic!("not CMYK") };
    assert!((c - 0.5).abs() < 1e-6 && (k - 1.0).abs() < 1e-6);
    // RGB arrays follow the same rule: [1, 128, 255] is 0..255.
    let Some(Color::Rgb { r, g, b }) = parse_color(&json!([1, 128, 255])) else { panic!("not RGB") };
    assert!((r - 1.0 / 255.0).abs() < 1e-6 && (g - 128.0 / 255.0).abs() < 1e-6 && (b - 1.0).abs() < 1e-6);
}

/// #910: `{c: 1, m: 0, y: 0, k: 0}` is a 1 % cyan tint, not solid cyan, though every component is within 0..1.
#[test]
fn cmyk_percentages_of_one_or_less_stay_percent() {
    use designcraft_color::Color;
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
    s.execute("swatch.create", &json!({"name": "Tint1", "color": {"c": 1, "m": 0, "y": 0, "k": 0}})).unwrap();
    s.execute("object.fill", &json!({"ids": [id], "swatch": "Tint1"})).unwrap();
    let d = &s.doc().unwrap().doc;
    let Some(Color::Cmyk { c, m, y, k }) = d.resolve_color("Tint1", 1.0) else { panic!("not CMYK") };
    assert!((c - 0.01).abs() < 1e-6 && m == 0.0 && y == 0.0 && k == 0.0, "got {c} {m} {y} {k}");
    let pdf = designcraft_pdf::export_pdf(d, &s.cache, &designcraft_pdf::PdfOptions { compress: false, ..Default::default() }).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    // Operands of every `k` (DeviceCMYK fill), last first: [k, y, m, c]. Inks are written in 1/255 steps (C 1 % is 3/255).
    let fills: Vec<Vec<f32>> =
        text.split(" k ").filter_map(|before| before.rsplit(' ').take(4).map(|x| x.parse().ok()).collect::<Option<Vec<f32>>>()).collect();
    let cyan = |want: f32| fills.iter().any(|f| (f[3] - want).abs() < 0.5 / 255.0 + 1e-6 && f[..3] == [0.0; 3]);
    assert!(cyan(0.01), "C 1 % in the PDF: {fills:?}");
    assert!(!cyan(1.0), "no solid cyan: {fills:?}");
    // A light tint whose components are all 1 or less, through object.color.
    s.execute("selection.set", &json!({"ids": [id]})).unwrap();
    s.execute("object.color", &json!({"color": {"c": 0.5, "m": 0.5, "y": 0, "k": 0}})).unwrap();
    let d = &s.doc().unwrap().doc;
    let fill = &d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch;
    let Some(Color::Cmyk { c, m, .. }) = d.resolve_color(fill, 1.0) else { panic!("not CMYK") };
    assert!((c - 0.005).abs() < 1e-6 && (m - 0.005).abs() < 1e-6, "got {c} {m}");
}

#[test]
fn a_colour_with_a_model_is_read_as_fractions_without_guessing() {
    use designcraft_color::Color;
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": [id]})).unwrap();
    let fill = |s: &Session| {
        let d = &s.doc().unwrap().doc;
        d.resolve_color(&d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, 1.0).unwrap()
    };
    // M 1 % from the Color panel: not M 100 % (magenta), though every component is within 0..1.
    s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0, "m": 0.01, "y": 0, "k": 0}})).unwrap();
    let Color::Cmyk { c, m, y, k } = fill(&s) else { panic!("not CMYK") };
    assert!(c == 0.0 && (m - 0.01).abs() < 1e-6 && y == 0.0 && k == 0.0, "got {c} {m} {y} {k}");
    // R 1 of 255 is a near black, not pure red.
    s.execute("object.color", &json!({"color": {"model": "rgb", "r": 1.0 / 255.0, "g": 0, "b": 0}})).unwrap();
    let Color::Rgb { r, g, b } = fill(&s) else { panic!("not RGB") };
    assert!((r - 1.0 / 255.0).abs() < 1e-6 && g == 0.0 && b == 0.0, "got {r} {g} {b}");
    // Out of range or not a model: refused rather than guessed.
    assert!(s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0, "m": 50, "y": 0, "k": 0}})).is_err());
    assert!(s.execute("object.color", &json!({"color": {"model": "hsb", "h": 0, "s": 0, "b": 0}})).is_err());
}

#[test]
fn colours_that_round_to_the_same_name_share_one_swatch() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": [id]})).unwrap();
    let n = |s: &Session| s.doc().unwrap().doc.swatches.len();
    let first = s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0.1, "m": 0.5, "y": 1, "k": 0}})).unwrap();
    let before = n(&s);
    for m in [0.501, 0.499, 0.5041] {
        let r = s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0.1, "m": m, "y": 1, "k": 0}})).unwrap();
        assert_eq!(r["swatch"], first["swatch"], "M {m}");
    }
    s.execute("object.color", &json!({"color": {"model": "rgb", "r": 0.5, "g": 0.25, "b": 0}})).unwrap();
    s.execute("object.color", &json!({"color": {"model": "rgb", "r": 0.5005, "g": 0.25, "b": 0}})).unwrap();
    assert_eq!(n(&s), before + 1, "no hidden twins for values a fraction of a step apart");
    // A whole percent apart is a different colour.
    let r = s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0.1, "m": 0.51, "y": 1, "k": 0}})).unwrap();
    assert_eq!(r["swatch"], "C=10 M=51 Y=100 K=0");
}

#[test]
fn style_groups_rename_every_use() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Sub", "basedOn": "Head"})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 2})).unwrap();
    s.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
    let r = s.execute("style.group", &json!({"kind": "paragraph", "names": ["Head", "Sub"], "group": "Titles"})).unwrap();
    assert_eq!(r["renamed"]["Head"], "Titles/Head");
    let d = s.doc().unwrap().doc.clone();
    assert_eq!(d.story(designcraft_doc::StoryId(sid)).unwrap().paras[0].style, "Titles/Head");
    assert_eq!(d.styles.para("Titles/Sub").unwrap().based_on.as_deref(), Some("Titles/Head"));
    // Out of the group again.
    s.execute("style.group", &json!({"kind": "paragraph", "names": ["Titles/Head"], "group": ""})).unwrap();
    assert!(s.doc().unwrap().doc.styles.para("Head").is_some());
    assert!(s.execute("style.group", &json!({"names": ["[Basic Paragraph]"], "group": "X"})).is_err());
}

#[test]
fn character_style_edit_sets_unsets_rebases_and_renames_every_use() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("style.character.create", &json!({"name": "Base", "chars": {"fontStyle": "Bold"}})).unwrap();
    s.execute("style.character.create", &json!({"name": "Emphasis", "chars": {"fontStyle": "Italic", "tracking": 20}})).unwrap();
    s.execute(
        "style.paragraph.create",
        &json!({"name": "Lead", "para": {
            "nestedStyles": [{"style": "Emphasis", "through": true, "count": 1, "until": {"kind": "words"}}],
            "grepStyles": [{"style": "Emphasis", "pattern": "\\d+"}],
            "nestedLineStyles": [{"style": "Emphasis", "lines": 1}]}}),
    )
    .unwrap();
    s.execute("footnote.options", &json!({"refCharStyle": "Emphasis"})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi there"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 2})).unwrap();
    s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();

    s.execute(
        "style.character.edit",
        &json!({"name": "Emphasis", "rename": "Strong", "basedOn": "Base", "chars": {"size": 14, "fill": "[Paper]", "fontStyle": null}}),
    )
    .unwrap();
    let d = s.doc().unwrap().doc.clone();
    assert!(d.styles.char_style("Emphasis").is_none());
    let st = d.styles.char_style("Strong").unwrap();
    assert_eq!(st.based_on.as_deref(), Some("Base"));
    // Exactly the attributes set: `null` unset the font style, the rest stayed unset.
    assert_eq!(st.chars, CharAttrs { size: Some(14.0), fill: Some("[Paper]".into()), tracking: Some(20.0), ..Default::default() });
    let story = d.story(designcraft_doc::StoryId(sid)).unwrap();
    assert!(story.chars.iter().any(|r| r.format.style == "Strong"));
    assert!(story.chars.iter().all(|r| r.format.style != "Emphasis"));
    let lead = d.styles.para("Lead").unwrap();
    assert_eq!(lead.para.nested_styles.as_ref().unwrap()[0].style, "Strong");
    assert_eq!(lead.para.grep_styles.as_ref().unwrap()[0].style, "Strong");
    assert_eq!(lead.para.nested_line_styles.as_ref().unwrap()[0].style, "Strong");
    assert_eq!(d.footnote_options.ref_char_style, "Strong");

    // Based On [None] (null or the name) clears it.
    s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": null})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.char_style("Strong").unwrap().based_on, None);
    s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "Base"})).unwrap();
    s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "[None]"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.char_style("Strong").unwrap().based_on, None);

    // Refused: a based-on cycle, an unknown parent, a taken or empty name, editing [None].
    s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "Base"})).unwrap();
    assert!(s.execute("style.character.edit", &json!({"name": "Base", "basedOn": "Strong"})).is_err());
    assert!(s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "Nope"})).is_err());
    assert!(s.execute("style.character.edit", &json!({"name": "Strong", "rename": "Base"})).is_err());
    assert!(s.execute("style.character.edit", &json!({"name": "Strong", "rename": "  "})).is_err());
    assert!(s.execute("style.character.edit", &json!({"name": "[None]", "chars": {"size": 9}})).is_err());
    assert!(s.doc().unwrap().doc.styles.char_style("[None]").unwrap().chars.is_empty());
}

#[test]
fn continue_numbering_in_a_style_overrides_an_inherited_start_at() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Steps", "para": {"listType": "numbers", "startAt": 5}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "More Steps", "basedOn": "Steps"})).unwrap();
    let start = |s: &Session, name: &str| s.doc().unwrap().doc.styles.resolve_para_style(name).0.start_at;
    assert_eq!(start(&s, "More Steps"), Some(5), "inherited");
    s.execute("style.paragraph.edit", &json!({"name": "More Steps", "para": {"startAt": null}})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.para("More Steps").unwrap().para.start_at, Some(None));
    assert_eq!(start(&s, "More Steps"), None, "continues");
    assert_eq!(start(&s, "Steps"), Some(5), "the parent keeps its Start At");
    s.execute("style.paragraph.edit", &json!({"name": "Steps", "para": {"startAt": null}})).unwrap();
    assert_eq!(start(&s, "Steps"), None);
    s.execute("style.paragraph.create", &json!({"name": "Fresh", "basedOn": "More Steps", "para": {"start_at": null}})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.para("Fresh").unwrap().para.start_at, Some(None));
}

#[test]
fn null_kinsoku_sets_the_default_rules_over_a_based_on_set() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let hard = serde_json::to_value(designcraft_doc::cjk::Kinsoku::named("HardKinsoku").unwrap()).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Base", "para": {"kinsoku": hard}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Body", "basedOn": "Base", "para": {"kinsoku": hard}})).unwrap();
    s.execute("style.paragraph.edit", &json!({"name": "Body", "para": {"kinsoku": null}})).unwrap();
    let styles = s.doc().unwrap().doc.styles.clone();
    assert_eq!(styles.para("Body").unwrap().para.kinsoku, Some(None));
    assert_eq!(styles.resolve_para_style("Body").0.kinsoku, None, "Base's set no longer applies");
    // An edit without the attribute keeps it.
    s.execute("style.paragraph.edit", &json!({"name": "Body", "para": {"rensuuji": false}})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().para.kinsoku, Some(None));
}

#[test]
fn new_character_style_checks_its_based_on() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    assert!(s.execute("style.character.create", &json!({"name": "Orphan", "basedOn": "Nope"})).is_err());
    s.execute("style.character.create", &json!({"name": "Plain", "basedOn": "[None]", "chars": {"size": 9}})).unwrap();
    let st = s.doc().unwrap().doc.styles.char_style("Plain").cloned().unwrap();
    assert_eq!((st.based_on, st.chars), (None, CharAttrs { size: Some(9.0), ..Default::default() }));
}

#[test]
fn swatches_save_and_load_ase() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("swatch.create", &json!({"name": "Brand Teal", "color": "#108080"})).unwrap();
    let b64 = s.execute("swatch.save", &json!({"names": ["Brand Teal"]})).unwrap()["base64"].as_str().unwrap().to_string();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
    assert_eq!(r["added"], 1);
    assert!(s.doc().unwrap().doc.swatch("Brand Teal").is_some());
    let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
    assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(0), Some(0)));
    assert!(s.execute("swatch.load", &json!({"base64": "AAAA"})).is_err());
}

#[test]
fn loading_swatches_matches_the_first_swatch_of_each_name() {
    use designcraft_color::{Color, ColorType};
    let ink = |k: f32| SwatchValue::Color { color: Color::cmyk(0.0, 0.0, 0.0, k), color_type: ColorType::Process };
    let sw = |name: &str, k: f32| Swatch { name: name.into(), value: ink(k), locked: false, named: true, hidden: false };
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    // Two swatches share a name, and an unnamed color is hidden.
    s.edit(|d, _| {
        d.swatches.extend([sw("Twin", 0.1), sw("Twin", 0.2), Swatch { named: false, hidden: true, ..sw("Unnamed", 0.3) }]);
        Ok(())
    })
    .unwrap();
    let file = designcraft_color::ase::write(&[sw("New", 0.4), sw("New", 0.5), sw("Twin", 0.6), sw("Unnamed", 0.7), sw("[Black]", 0.8)]);
    let b64 = crate::cmd::base64_encode(&file);
    let named = |s: &Session, name: &str| -> Vec<(SwatchValue, bool)> {
        s.doc().unwrap().doc.swatches.iter().filter(|w| w.name == name).map(|w| (w.value.clone(), w.hidden)).collect()
    };
    // With `replace`, the file's second "New" is added as "New 2", only the first "Twin" changes, the hidden
    // swatch becomes visible, and the locked [Black] stays.
    let r = s.execute("swatch.load", &json!({"base64": b64, "replace": true})).unwrap();
    assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(2), Some(2)));
    assert_eq!(named(&s, "New"), [(ink(0.4), false)]);
    assert_eq!(named(&s, "New 2"), [(ink(0.5), false)]);
    assert_eq!(named(&s, "Twin"), [(ink(0.6), false), (ink(0.2), false)]);
    assert_eq!(named(&s, "Unnamed"), [(ink(0.7), false)]);
    assert_eq!(named(&s, "[Black]"), [(ink(1.0), false)]);
    // Without it, every name is already there and nothing changes.
    let before = s.doc().unwrap().doc.swatches.clone();
    let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
    assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(0), Some(0)));
    assert_eq!(s.doc().unwrap().doc.swatches, before);
}

#[test]
fn loading_swatches_numbers_repeated_names_and_names_unnamed_colors() {
    use designcraft_color::{Color, ColorType};
    let ink = |k: f32| SwatchValue::Color { color: Color::cmyk(0.0, 0.0, 0.0, k), color_type: ColorType::Process };
    let sw = |name: &str, k: f32| Swatch { name: name.into(), value: ink(k), locked: false, named: true, hidden: false };
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit(|d, _| {
        d.swatches.push(sw("Kept", 0.1));
        Ok(())
    })
    .unwrap();
    // "Red 2" comes before the second "Red", which takes "Red 3". Two unnamed colors share a value.
    let file =
        designcraft_color::ase::write(&[sw("Red", 0.2), sw("Red 2", 0.3), sw("Red", 0.4), sw("", 0.55), sw("", 0.55), sw("", 0.65), sw("Kept", 0.7)]);
    let b64 = crate::cmd::base64_encode(&file);
    let named = |s: &Session, name: &str| -> Vec<(SwatchValue, bool)> {
        s.doc().unwrap().doc.swatches.iter().filter(|w| w.name == name).map(|w| (w.value.clone(), w.named)).collect()
    };
    // Names already in the document are kept.
    let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
    assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(6), Some(0)));
    assert_eq!(named(&s, "Red"), [(ink(0.2), true)]);
    assert_eq!(named(&s, "Red 2"), [(ink(0.3), true)]);
    assert_eq!(named(&s, "Red 3"), [(ink(0.4), true)]);
    assert_eq!(named(&s, "C=0 M=0 Y=0 K=55"), [(ink(0.55), false)]);
    assert_eq!(named(&s, "C=0 M=0 Y=0 K=55 2"), [(ink(0.55), false)]);
    assert_eq!(named(&s, "C=0 M=0 Y=0 K=65"), [(ink(0.65), false)]);
    assert_eq!(named(&s, "Kept"), [(ink(0.1), true)]);
    assert!(named(&s, "").is_empty());
    // With `replace`, the same file again replaces each of its colors once.
    let r = s.execute("swatch.load", &json!({"base64": b64, "replace": true})).unwrap();
    assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(0), Some(7)));
    assert_eq!(named(&s, "Red"), [(ink(0.2), true)]);
    assert_eq!(named(&s, "Red 3"), [(ink(0.4), true)]);
    assert_eq!(named(&s, "Kept"), [(ink(0.7), true)]);
}

#[test]
fn unnamed_colors_then_add_to_swatches() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": [id]})).unwrap();
    let listed = |s: &Session| s.doc().unwrap().doc.swatches.iter().filter(|w| !w.hidden).count();
    let before = listed(&s);
    let r = s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}})).unwrap();
    assert_eq!(r["swatch"], "C=10 M=20 Y=30 K=0");
    assert_eq!(listed(&s), before, "unnamed: not in the Swatches panel");
    // The same colour again reuses it; the stroke target works too.
    let n = s.doc().unwrap().doc.swatches.len();
    s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}, "target": "stroke"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.swatches.len(), n);
    let d = &s.doc().unwrap().doc;
    let it = d.item(designcraft_doc::ItemId(id)).unwrap();
    assert_eq!(it.fill.swatch, it.stroke.swatch);
    // Add to Swatches with a name: listed, and the object follows the rename.
    s.execute("swatch.addToSwatches", &json!({"name": "Sand"})).unwrap();
    assert_eq!(listed(&s), before + 1);
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, "Sand");
    // A colour equal to an existing swatch applies that swatch.
    s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}})).unwrap();
    assert_eq!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, "Sand");
}

#[test]
fn add_to_swatches_renames_the_colour_in_text_cells_and_notes() {
    use std::sync::Arc;
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Hello world"})).unwrap();
    let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
    s.execute("text.select", &json!({"story": sid.0, "anchor": 11})).unwrap();
    s.execute("endnote.insert", &json!({"text": "end"})).unwrap();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 11})).unwrap();
    s.execute("footnote.insert", &json!({"text": "note"})).unwrap();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0})).unwrap();
    s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 1, "focus": 4})).unwrap();
    let old = s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}})).unwrap()["swatch"].as_str().unwrap().to_string();
    // The same unnamed colour on a cell, the cell's text and the footnote's text.
    s.edit(|d, _| {
        let st = d.story_mut(sid).unwrap();
        let t = Arc::make_mut(st.tables.values_mut().next().unwrap());
        t.cells[0].fill = old.clone();
        t.cells[0].text.chars[0].format.over.fill = Some(old.clone());
        Arc::make_mut(&mut st.notes[0]).text.chars[0].format.over.stroke = Some(old.clone());
        Arc::make_mut(&mut st.endnotes[0]).text.chars[0].format.over.underline_color = Some(old.clone());
        Ok(())
    })
    .unwrap();
    s.execute("swatch.addToSwatches", &json!({"swatch": old, "name": "Sand"})).unwrap();
    let d = &s.doc().unwrap().doc;
    let st = d.story(sid).unwrap();
    assert_eq!(st.char_format_at(2).over.fill.as_deref(), Some("Sand"));
    let cell = &st.tables.values().next().unwrap().cells[0];
    assert_eq!(cell.fill, "Sand");
    assert_eq!(cell.text.chars[0].format.over.fill.as_deref(), Some("Sand"));
    assert_eq!(st.notes[0].text.chars[0].format.over.stroke.as_deref(), Some("Sand"));
    assert_eq!(st.endnotes[0].text.chars[0].format.over.underline_color.as_deref(), Some("Sand"));
    d.check().unwrap();
}
