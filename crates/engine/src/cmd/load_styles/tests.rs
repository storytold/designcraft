use designcraft_color::{Color, Swatch, SwatchValue};
use designcraft_doc::{CharAttrs, CharacterStyle, ExportTag, GrepStyle, NumberedList, ParaAttrs, ParagraphStyle, Rule, Styles};
use serde_json::{Value, json};

use crate::Session;

const BASIC: &str = "[Basic Paragraph]";

fn para(name: &str, based_on: Option<&str>, size: f64) -> ParagraphStyle {
    ParagraphStyle {
        name: name.into(),
        based_on: based_on.map(str::to_string),
        next_style: None,
        para: ParaAttrs::default(),
        chars: CharAttrs { size: Some(size), ..Default::default() },
        shortcut: String::new(),
    }
}

fn chars(name: &str, based_on: Option<&str>, size: f64) -> CharacterStyle {
    CharacterStyle {
        name: name.into(),
        based_on: based_on.map(str::to_string),
        chars: CharAttrs { size: Some(size), ..Default::default() },
        shortcut: String::new(),
    }
}

fn new_doc() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn file_of(s: &Session) -> String {
    super::super::file::base64_encode(&super::super::file::to_bytes(&s.doc().unwrap().doc).unwrap())
}

/// A client's document: Body in the brand colour with a GREP style, Head and Quote based on
/// it, Small characters based on Lead, an 11 pt [Basic Paragraph], and a swatch no style uses.
fn client() -> Session {
    let mut s = new_doc();
    s.edit(|d, _| {
        d.swatches.push(Swatch::color("Brand Teal", Color::cmyk(0.9, 0.0, 0.4, 0.2)));
        d.swatches.push(Swatch {
            name: "Brand Teal 40%".into(),
            value: SwatchValue::Tint { base: "Brand Teal".into(), tint: 0.4 },
            locked: false,
            named: true,
            hidden: false,
        });
        d.swatches.push(Swatch::color("Unused Red", Color::cmyk(0.0, 1.0, 1.0, 0.0)));
        let st = d.styles_mut();
        st.para_mut(BASIC).unwrap().chars.size = Some(11.0);
        st.character.push(chars("Lead", None, 9.0));
        st.character.push(chars("Small", Some("Lead"), 7.0));
        let mut body = para("Body", Some(BASIC), 10.0);
        body.chars.fill = Some("Brand Teal".into());
        body.para.grep_styles = Some(vec![GrepStyle { style: "Small".into(), pattern: r"\d+".into() }]);
        st.paragraph.push(body);
        let mut head = para("Head", Some("Body"), 20.0);
        head.next_style = Some("Body".into());
        st.paragraph.push(head);
        let mut quote = para("Quote", Some("Body"), 12.0);
        quote.para.rule_above = Some(Rule { on: true, color: "Brand Teal 40%".into(), ..Default::default() });
        st.paragraph.push(quote);
        Ok(())
    })
    .unwrap();
    s
}

fn styles(s: &Session) -> Styles {
    (*s.doc().unwrap().doc.styles).clone()
}

#[test]
fn every_style_of_the_file_loads_with_the_swatches_it_uses() {
    let mut s = new_doc();
    let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
    let d = &s.doc().unwrap().doc;
    let head = d.styles.para("Head").unwrap();
    assert_eq!((head.based_on.as_deref(), head.next_style.as_deref(), head.chars.size), (Some("Body"), Some("Body"), Some(20.0)));
    assert_eq!(d.styles.para("Body").unwrap().chars.fill.as_deref(), Some("Brand Teal"));
    assert_eq!(d.styles.char_style("Small").unwrap().based_on.as_deref(), Some("Lead"));
    assert_eq!(d.swatch("Brand Teal"), client().doc().unwrap().doc.swatch("Brand Teal"));
    assert!(d.swatch("Brand Teal 40%").is_some());
    assert!(d.swatch("Unused Red").is_none(), "a swatch no loaded style uses stays behind");
    assert_eq!(r["paragraph"]["added"], json!(["Body", "Head", "Quote"]));
    assert_eq!(r["character"]["added"], json!(["Lead", "Small"]));
    assert_eq!(r["swatches"], json!(["Brand Teal", "Brand Teal 40%"]));
}

#[test]
fn basic_paragraph_takes_the_definition_of_the_file() {
    let mut s = new_doc();
    let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.para(BASIC).unwrap().chars.size, Some(11.0));
    assert_eq!(r["paragraph"]["replaced"], json!([BASIC]));
}

#[test]
fn a_named_style_brings_the_styles_it_names() {
    let mut s = new_doc();
    let r = s.execute("style.load", &json!({"base64": file_of(&client()), "paragraph": ["Head"], "character": []})).unwrap();
    let st = styles(&s);
    assert_eq!(r["paragraph"]["added"], json!(["Body", "Head"]), "Head is based on Body");
    assert_eq!(r["character"]["added"], json!(["Lead", "Small"]), "Body's GREP style names Small, which is based on Lead");
    assert!(st.para("Quote").is_none());
    assert_eq!(st.para(BASIC).unwrap().chars.size, None, "a style the document has is only replaced when it was asked for");
    assert_eq!(r["paragraph"]["replaced"], json!([]));
    assert_eq!(r["swatches"], json!(["Brand Teal"]));
}

/// The document already has a Body and a Lead of its own.
fn doc_with_body_and_lead() -> Session {
    let mut s = new_doc();
    s.edit(|d, _| {
        let st = d.styles_mut();
        st.paragraph.push(para("Body", None, 8.0));
        st.character.push(chars("Lead", None, 5.0));
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn a_style_of_the_same_name_takes_the_loaded_definition() {
    let mut s = doc_with_body_and_lead();
    let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
    let st = styles(&s);
    assert_eq!((st.para("Body").unwrap().chars.size, st.char_style("Lead").unwrap().chars.size), (Some(10.0), Some(9.0)));
    assert_eq!(r["paragraph"]["replaced"], json!([BASIC, "Body"]));
    assert_eq!(r["character"]["replaced"], json!(["Lead"]));
    assert_eq!(st.paragraph.iter().filter(|p| p.name == "Body").count(), 1);
}

#[test]
fn use_existing_keeps_the_styles_of_the_document() {
    let mut s = doc_with_body_and_lead();
    let r = s.execute("style.load", &json!({"base64": file_of(&client()), "conflicts": "useExisting"})).unwrap();
    let st = styles(&s);
    assert_eq!((st.para("Body").unwrap().chars.size, st.char_style("Lead").unwrap().chars.size), (Some(8.0), Some(5.0)));
    assert_eq!(st.para(BASIC).unwrap().chars.size, None);
    assert_eq!(st.para("Head").unwrap().based_on.as_deref(), Some("Body"));
    assert_eq!((&r["paragraph"]["added"], &r["paragraph"]["replaced"]), (&json!(["Head", "Quote"]), &json!([])));
}

#[test]
fn auto_rename_loads_a_clashing_style_under_a_new_name() {
    let mut s = doc_with_body_and_lead();
    let r = s.execute("style.load", &json!({"base64": file_of(&client()), "conflicts": "autoRename"})).unwrap();
    let st = styles(&s);
    assert_eq!((st.para("Body").unwrap().chars.size, st.para("Body 2").unwrap().chars.size), (Some(8.0), Some(10.0)));
    assert_eq!((st.char_style("Lead").unwrap().chars.size, st.char_style("Lead 2").unwrap().chars.size), (Some(5.0), Some(9.0)));
    let head = st.para("Head").unwrap();
    assert_eq!((head.based_on.as_deref(), head.next_style.as_deref()), (Some("Body 2"), Some("Body 2")), "the loaded Head follows the loaded Body");
    assert_eq!(st.char_style("Small").unwrap().based_on.as_deref(), Some("Lead 2"));
    assert_eq!(st.para("Body 2").unwrap().based_on.as_deref(), Some("Basic Paragraph"));
    assert_eq!(st.para("Basic Paragraph").unwrap().chars.size, Some(11.0), "a built-in style comes in without its brackets");
    assert_eq!(st.para(BASIC).unwrap().chars.size, None);
    assert_eq!(r["paragraph"]["renamed"], json!({"[Basic Paragraph]": "Basic Paragraph", "Body": "Body 2"}));
    assert_eq!(r["character"]["renamed"], json!({"Lead": "Lead 2"}));
    assert_eq!(r["paragraph"]["replaced"], json!([]));
}

#[test]
fn a_style_that_is_the_same_in_both_is_no_conflict() {
    let mut s = new_doc();
    let file = file_of(&client());
    s.execute("style.load", &json!({"base64": file})).unwrap();
    let loaded = styles(&s);
    for conflicts in ["redefine", "autoRename"] {
        let r = s.execute("style.load", &json!({"base64": file, "conflicts": conflicts})).unwrap();
        let none = json!({"added": [], "replaced": [], "renamed": {}});
        assert_eq!(r, json!({"paragraph": none, "character": none, "swatches": []}), "{conflicts}");
    }
    assert_eq!(styles(&s), loaded);
    // Loading nothing leaves no undo step: one Undo takes back the first load.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(styles(&s).para("Head").is_none());
}

#[test]
fn loading_is_one_undo_step() {
    let mut s = doc_with_body_and_lead();
    let (before, swatches) = (styles(&s), s.doc().unwrap().doc.swatches.clone());
    s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
    assert_ne!(styles(&s), before);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(styles(&s), before);
    assert_eq!(s.doc().unwrap().doc.swatches, swatches);
}

#[test]
fn a_swatch_of_the_document_wins_over_the_one_in_the_file() {
    let mut s = new_doc();
    s.execute("swatch.create", &json!({"name": "Brand Teal", "color": "#ff0000"})).unwrap();
    let mine = s.doc().unwrap().doc.swatch("Brand Teal").cloned();
    let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
    assert_eq!(s.doc().unwrap().doc.swatch("Brand Teal").cloned(), mine);
    assert_eq!(r["swatches"], json!(["Brand Teal 40%"]));
}

#[test]
fn a_tint_swatch_brings_its_base_colour() {
    let mut src = client();
    src.edit(|d, _| {
        let mut ruled = para("Ruled", None, 9.0);
        ruled.para.rule_below = Some(Rule { on: true, color: "Brand Teal 40%".into(), ..Default::default() });
        d.styles_mut().paragraph.push(ruled);
        Ok(())
    })
    .unwrap();
    let mut s = new_doc();
    let r = s.execute("style.load", &json!({"base64": file_of(&src), "paragraph": ["Ruled"], "character": []})).unwrap();
    assert_eq!(r["paragraph"]["added"], json!(["Ruled"]));
    assert_eq!(r["swatches"], json!(["Brand Teal", "Brand Teal 40%"]));
}

#[test]
fn export_tags_and_named_lists_come_with_their_styles() {
    let mut src = client();
    src.edit(|d, _| {
        d.settings.lists.push(NumberedList { name: "Steps".into(), continue_across_stories: false });
        d.settings.lists.push(NumberedList { name: "Unused".into(), ..Default::default() });
        let st = d.styles_mut();
        st.export_tags.insert("p:Head".into(), ExportTag { tag: "h1".into(), class: "title".into(), split: true });
        st.export_tags.insert("c:Lead".into(), ExportTag { tag: "em".into(), class: String::new(), ..Default::default() });
        st.para_mut("Quote").unwrap().para.list_name = Some("Steps".into());
        Ok(())
    })
    .unwrap();
    let mut s = doc_with_body_and_lead();
    s.execute("style.load", &json!({"base64": file_of(&src), "conflicts": "autoRename"})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.styles.export_tag("Head", false).map(|t| (t.tag.as_str(), t.split)), Some(("h1", true)), "Split Document comes along");
    assert_eq!(d.styles.export_tag("Lead 2", true).map(|t| t.tag.as_str()), Some("em"), "the tag follows the new name");
    assert_eq!(d.styles.export_tag("Lead", true), None);
    assert_eq!(d.settings.lists.iter().map(|l| (l.name.as_str(), l.continue_across_stories)).collect::<Vec<_>>(), [("Steps", false)]);
}

#[test]
fn styles_load_from_an_idml_file() {
    let idml = client().execute("file.exportIdml", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
    let mut s = new_doc();
    let r = s.execute("style.load", &json!({"base64": idml, "name": "Client.IDML", "paragraph": ["Head"]})).unwrap();
    let d = &s.doc().unwrap().doc;
    let head = d.styles.para("Head").unwrap();
    assert_eq!((head.based_on.as_deref(), head.chars.size), (Some("Body"), Some(20.0)));
    assert_eq!(d.styles.para("Body").unwrap().chars.fill.as_deref(), Some("Brand Teal"));
    assert!(d.swatch("Brand Teal").is_some());
    assert_eq!(r["paragraph"]["added"], json!(["Body", "Head"]));
}

#[test]
fn styles_load_from_a_path() {
    let path = std::env::temp_dir().join(format!("dc-load-styles-{}.designcraft", std::process::id()));
    std::fs::write(&path, super::super::file::to_bytes(&client().doc().unwrap().doc).unwrap()).unwrap();
    let mut s = new_doc();
    let r = s.execute("style.load", &json!({"path": path.to_str().unwrap(), "paragraph": [], "character": ["Small"]}));
    std::fs::remove_file(&path).unwrap();
    assert_eq!(r.unwrap()["character"]["added"], json!(["Lead", "Small"]));
    assert!(styles(&s).para("Head").is_none());
}

#[test]
fn the_file_lists_its_styles_and_which_ones_clash() {
    let mut s = doc_with_body_and_lead();
    let file = file_of(&client());
    let name = |n: &str, conflict: bool| json!({"name": n, "conflict": conflict});
    let r = s.execute("style.load.list", &json!({"base64": file})).unwrap();
    assert_eq!(r["paragraph"], json!([name(BASIC, true), name("Body", true), name("Head", false), name("Quote", false)]));
    assert_eq!(r["character"], json!([name("Lead", true), name("Small", false)]));
    s.execute("style.load", &json!({"base64": file})).unwrap();
    let r = s.execute("style.load.list", &json!({"base64": file})).unwrap();
    assert!(r["paragraph"].as_array().unwrap().iter().all(|p| p["conflict"] == false), "the same definition is no clash: {r}");
}

fn unchanged_after(s: &mut Session, params: Value, why: &str) {
    let before = s.doc().unwrap().doc.clone();
    let e = s.execute("style.load", &params).unwrap_err().to_string();
    assert!(e.contains(why), "{e}");
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn what_cannot_be_loaded_is_an_error_and_changes_nothing() {
    let mut s = doc_with_body_and_lead();
    let file = file_of(&client());
    unchanged_after(&mut s, json!({}), "`path` or `base64`");
    unchanged_after(&mut s, json!({"base64": "AAAA"}), "");
    unchanged_after(&mut s, json!({"base64": "AAAA", "name": "a.idml"}), "");
    unchanged_after(&mut s, json!({"base64": file, "paragraph": ["Head", "Nope"]}), "no paragraph style `Nope`");
    unchanged_after(&mut s, json!({"base64": file, "character": ["[None]"]}), "no character style `[None]`");
    unchanged_after(&mut s, json!({"base64": file, "paragraph": ["[No Paragraph Style]"]}), "no paragraph style");
    unchanged_after(&mut s, json!({"base64": file, "paragraph": "Head"}), "`paragraph` must be a list of style names");
    unchanged_after(&mut s, json!({"base64": file, "character": [1]}), "`character` must be a list of style names");
    unchanged_after(&mut s, json!({"base64": file, "conflicts": "merge"}), "`conflicts`");
}

#[test]
fn a_style_that_would_be_based_on_itself_is_refused() {
    // In the file B is based on A; in the document A is based on B.
    let mut src = new_doc();
    src.edit(|d, _| {
        let st = d.styles_mut();
        st.paragraph.push(para("A", None, 9.0));
        st.paragraph.push(para("B", Some("A"), 9.0));
        st.character.push(chars("X", None, 9.0));
        st.character.push(chars("Y", Some("X"), 9.0));
        Ok(())
    })
    .unwrap();
    let mut s = new_doc();
    s.edit(|d, _| {
        let st = d.styles_mut();
        st.paragraph.push(para("B", None, 9.0));
        st.paragraph.push(para("A", Some("B"), 9.0));
        st.character.push(chars("Y", None, 9.0));
        st.character.push(chars("X", Some("Y"), 9.0));
        Ok(())
    })
    .unwrap();
    unchanged_after(&mut s, json!({"base64": file_of(&src), "paragraph": ["B"], "character": []}), "`B` would be based on itself");
    unchanged_after(&mut s, json!({"base64": file_of(&src), "paragraph": [], "character": ["Y"]}), "`Y` would be based on itself");
    // With A loaded as well both take the file's order.
    s.execute("style.load", &json!({"base64": file_of(&src), "character": []})).unwrap();
    assert_eq!((styles(&s).para("A").unwrap().based_on.clone(), styles(&s).para("B").unwrap().based_on.clone()), (None, Some("A".to_string())));
}

#[test]
fn a_file_with_a_based_on_loop_of_its_own_does_not_hang() {
    let mut src = new_doc();
    src.edit(|d, _| {
        let st = d.styles_mut();
        st.paragraph.push(para("A", Some("B"), 9.0));
        st.paragraph.push(para("B", Some("A"), 9.0));
        st.paragraph.push(para("Next", None, 9.0));
        st.para_mut("Next").unwrap().next_style = Some("Next".into());
        Ok(())
    })
    .unwrap();
    let mut s = new_doc();
    unchanged_after(&mut s, json!({"base64": file_of(&src), "paragraph": ["A"]}), "would be based on itself");
    let r = s.execute("style.load", &json!({"base64": file_of(&src), "paragraph": ["Next"]})).unwrap();
    assert_eq!(r["paragraph"]["added"], json!(["Next"]));
}
