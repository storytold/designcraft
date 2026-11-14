//! End reaches the end of the line the caret is on, also when the caret sits at the first letter of a wrapped line.
use designcraft_engine::Session;
use serde_json::json;

#[test]
fn line_end_from_the_start_of_a_wrapped_line_reaches_its_end() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let text = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo";
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 120, 400], "content": "text", "text": text})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    let mv = |s: &mut Session, dir: &str| s.execute("text.move", &json!({"dir": dir})).unwrap()["pos"].as_u64().unwrap() as usize;

    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    let first_end = mv(&mut s, "lineEnd");
    assert!(first_end > 0 && first_end < text.len(), "the text wraps: {first_end}");

    // The caret at the first letter of the second line.
    s.execute("text.select", &json!({"story": sid, "anchor": first_end, "focus": first_end})).unwrap();
    assert_eq!(mv(&mut s, "lineStart"), first_end, "Home stays at the start of its line");
    let second_end = mv(&mut s, "lineEnd");
    assert!(second_end > first_end, "End moves to the end of the second line: {first_end} -> {second_end}");
}
