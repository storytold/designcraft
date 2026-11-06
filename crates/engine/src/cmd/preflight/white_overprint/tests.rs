use std::sync::Arc;

use serde_json::json;

use crate::Session;

/// The `whiteOverprint` issues of the active document: (message, item).
fn white_overprints(s: &mut Session) -> Vec<(String, Option<u64>)> {
    let r = s.execute("preflight.run", &json!({})).unwrap();
    r["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "whiteOverprint")
        .inspect(|i| assert_eq!(i["severity"], "warning"))
        .map(|i| (i["message"].as_str().unwrap().to_string(), i["item"].as_u64()))
        .collect()
}

fn rect(s: &mut Session, r: [f64; 4]) -> u64 {
    s.execute("frame.create", &json!({"rect": r})).unwrap()["id"].as_u64().unwrap()
}

/// A [Paper] fill set to overprint is gone on press: preflight said nothing about it.
#[test]
fn a_paper_fill_set_to_overprint_is_reported() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = rect(&mut s, [72.0, 72.0, 200.0, 200.0]);
    s.execute("object.fill", &json!({"ids": [id], "swatch": "[Paper]"})).unwrap();
    assert!(white_overprints(&mut s).is_empty(), "a white fill that knocks out prints");
    s.execute("object.attributes", &json!({"ids": [id], "overprintFill": true})).unwrap();
    assert_eq!(white_overprints(&mut s), [("White or [Paper] fill set to overprint will not print".to_string(), Some(id))]);
    // A coloured fill set to overprint is ordinary trapping.
    s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
    assert!(white_overprints(&mut s).is_empty());
    // Nonprinting: nothing of it prints anyway.
    s.execute("object.fill", &json!({"ids": [id], "swatch": "[Paper]"})).unwrap();
    s.execute("object.attributes", &json!({"ids": [id], "nonprinting": true})).unwrap();
    assert!(white_overprints(&mut s).is_empty());
}

/// White is any paint that lays down no ink: C=0 M=0 Y=0 K=0, or a 0% tint.
#[test]
fn white_colours_and_strokes_are_reported() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    Arc::make_mut(&mut s.doc_mut().unwrap().doc).swatches.push(designcraft_color::swatch::Swatch::cmyk(0.0, 0.0, 0.0, 0.0));
    let a = rect(&mut s, [72.0, 72.0, 200.0, 200.0]);
    s.execute("object.fill", &json!({"ids": [a], "swatch": "[Black]", "tint": 0.0})).unwrap();
    s.execute("object.stroke", &json!({"ids": [a], "swatch": "C=0 M=0 Y=0 K=0", "weight": 4})).unwrap();
    s.execute("object.attributes", &json!({"ids": [a], "overprintFill": true, "overprintStroke": true})).unwrap();
    // A gap colour shows only between dashes: on a solid stroke it is not drawn.
    let b = rect(&mut s, [300.0, 72.0, 400.0, 200.0]);
    s.execute("object.stroke", &json!({"ids": [b], "swatch": "[Black]", "weight": 4, "gapSwatch": "[Paper]", "gapOverprint": true})).unwrap();
    assert_eq!(white_overprints(&mut s), [("White or [Paper] fill and stroke set to overprint will not print".to_string(), Some(a))]);
    s.execute("object.stroke", &json!({"ids": [b], "type": {"kind": "dotted"}})).unwrap();
    let found = white_overprints(&mut s);
    assert!(found.contains(&("White or [Paper] stroke gap set to overprint will not print".to_string(), Some(b))), "{found:?}");
}

/// A spot ink prints on its own plate whatever colour stands in for it: a white ink or a varnish
/// set to overprint, as they usually are, is not white. A 0% tint of one lays down no ink.
#[test]
fn a_spot_ink_with_a_white_alternate_prints() {
    use designcraft_color::{Color, ColorType, Swatch, SwatchValue};
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let swatches = &mut Arc::make_mut(&mut s.doc_mut().unwrap().doc).swatches;
    for (name, color) in [("Opaque White", Color::cmyk(0.0, 0.0, 0.0, 0.0)), ("Varnish", Color::rgb(1.0, 1.0, 1.0))] {
        swatches.push(Swatch {
            name: name.into(),
            value: SwatchValue::Color { color, color_type: ColorType::Spot },
            locked: false,
            named: true,
            hidden: false,
        });
    }
    swatches.push(Swatch {
        name: "Varnish 50%".into(),
        value: SwatchValue::Tint { base: "Varnish".into(), tint: 0.5 },
        locked: false,
        named: true,
        hidden: false,
    });
    let a = rect(&mut s, [72.0, 72.0, 200.0, 200.0]);
    s.execute("object.fill", &json!({"ids": [a], "swatch": "Opaque White"})).unwrap();
    s.execute("object.stroke", &json!({"ids": [a], "swatch": "Varnish 50%", "weight": 4})).unwrap();
    s.execute("object.attributes", &json!({"ids": [a], "overprintFill": true, "overprintStroke": true})).unwrap();
    assert!(white_overprints(&mut s).is_empty());
    s.execute("object.fill", &json!({"ids": [a], "swatch": "Opaque White", "tint": 0.0})).unwrap();
    assert_eq!(white_overprints(&mut s), [("White or [Paper] fill set to overprint will not print".to_string(), Some(a))]);
}

/// The issue's steps: text in [Paper], and Overprint Fill applied to its frame. Text has no
/// overprint of its own, and the frame's Overprint Fill is for the frame's fill, which is [None]:
/// the text knocks out and prints, so there is nothing to report. With a [Paper] fill on the
/// frame, the frame is what disappears.
#[test]
fn paper_text_in_a_frame_set_to_overprint() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "White"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 5})).unwrap();
    s.execute("type.char", &json!({"attrs": {"fill": "[Paper]"}})).unwrap();
    s.execute("object.attributes", &json!({"ids": [id], "overprintFill": true})).unwrap();
    assert!(white_overprints(&mut s).is_empty());
    s.execute("object.fill", &json!({"ids": [id], "swatch": "[Paper]"})).unwrap();
    assert_eq!(white_overprints(&mut s), [("White or [Paper] fill set to overprint will not print".to_string(), Some(id))]);
}

/// Ruby and kenten are the text that can overprint; in [Paper] they then disappear.
#[test]
fn white_ruby_and_kenten_set_to_overprint_are_reported() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "漢字"})).unwrap();
    let id = r["id"].as_u64();
    let select = |s: &mut Session| s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": "漢字".len()})).unwrap();
    select(&mut s);
    s.execute("type.char", &json!({"attrs": {"kenten": true, "kentenFill": "[Paper]"}})).unwrap();
    assert!(white_overprints(&mut s).is_empty(), "`Auto` follows the text, which knocks out");
    select(&mut s);
    s.execute("type.char", &json!({"attrs": {"kentenOverprintFill": "on"}})).unwrap();
    assert_eq!(white_overprints(&mut s), [("White or [Paper] kenten set to overprint will not print".to_string(), id)]);
    // Ruby in the text's colour, [Paper], with the ruby's stroke set to overprint and none: fine.
    select(&mut s);
    s.execute("type.char", &json!({"attrs": {"ruby": "かんじ", "fill": "[Paper]", "rubyOverprintStroke": "on"}})).unwrap();
    assert_eq!(white_overprints(&mut s), [("White or [Paper] kenten set to overprint will not print".to_string(), id)]);
    select(&mut s);
    s.execute("type.char", &json!({"attrs": {"rubyOverprintFill": "on"}})).unwrap();
    let found = white_overprints(&mut s);
    assert_eq!(found, [("White or [Paper] kenten and ruby set to overprint will not print".to_string(), id)]);
}
