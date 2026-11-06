//! Find/Replace Font lists, and so can replace, a missing font that only an unused style names.
use designcraft_engine::Session;
use serde_json::{Value, json};

fn missing_fonts(s: &mut Session) -> Vec<(String, String)> {
    let list = s.execute("font.list", &json!({})).unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .filter(|f| f["missing"] == true)
        .map(|f| (f["family"].as_str().unwrap().to_string(), f["style"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn a_missing_font_only_an_unused_style_names_is_listed_and_replaced() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Some text here."})).unwrap();
    s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 4})).unwrap();
    s.execute("type.char", &json!({"fontFamily": "Missing Sans", "fontStyle": "Light"})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Unused", "chars": {"fontFamily": "Missing Sans", "fontStyle": "Bold"}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "FamilyOnly", "chars": {"fontFamily": "Missing Sans"}})).unwrap();
    s.execute("style.character.create", &json!({"name": "UnusedChar", "chars": {"fontFamily": "Missing Sans", "fontStyle": "Italic"}})).unwrap();
    s.execute("style.character.create", &json!({"name": "FamilyOnlyChar", "chars": {"fontFamily": "Missing Sans"}})).unwrap();

    let list = s.execute("font.list", &json!({})).unwrap();
    let row = |style: &str| -> Value {
        list.as_array()
            .unwrap()
            .iter()
            .find(|f| f["family"] == "Missing Sans" && f["style"] == style)
            .cloned()
            .unwrap_or_else(|| panic!("Missing Sans / {style} is not listed: {list}"))
    };
    assert_eq!(row("Light")["inStylesOnly"], false, "{list}");
    for style in ["Bold", "Italic"] {
        let f = row(style);
        assert_eq!((&f["missing"], &f["inStylesOnly"], &f["characters"]), (&json!(true), &json!(true), &json!(0)), "{list}");
    }

    for (family, style) in missing_fonts(&mut s) {
        s.execute("font.replace", &json!({"family": family, "style": style, "toFamily": "Source Sans 3", "toStyle": "Regular"})).unwrap();
    }

    assert!(missing_fonts(&mut s).is_empty());
    let doc = &s.doc().unwrap().doc;
    for name in ["Unused", "FamilyOnly"] {
        let family = doc.styles.para(name).unwrap().chars.font_family.as_deref();
        assert_eq!(family, Some("Source Sans 3"), "{name}");
    }
    for name in ["UnusedChar", "FamilyOnlyChar"] {
        let family = doc.styles.char_style(name).unwrap().chars.font_family.as_deref();
        assert_eq!(family, Some("Source Sans 3"), "{name}");
    }
}
