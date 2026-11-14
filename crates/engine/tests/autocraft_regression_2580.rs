//! A QR placeholder on an existing QR code is refused, not accepted and then silently never merged.
use designcraft_engine::Session;
use designcraft_engine::cmd::base64_encode;
use serde_json::json;

fn session_with_source() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let csv = "Name,Link\nA,https://a.example\nB,https://b.example\nC,https://c.example\n";
    s.execute("data.source.select", &json!({"bytes": base64_encode(csv.as_bytes()), "name": "links.csv"})).unwrap();
    s
}

#[test]
fn qr_placeholder_on_an_existing_qr_code_is_refused() {
    let mut s = session_with_source();
    let qr = s.execute("object.qrCode", &json!({"content": "https://old.example", "rect": [72, 72, 216, 216]})).unwrap()["id"].as_u64().unwrap();

    let res = s.execute("data.placeholder.add", &json!({"field": "Link", "role": "qr", "item": qr}));
    let msg = res.expect_err("a QR placeholder on an existing QR code must be refused").to_string();
    assert!(msg.contains("empty graphic frame"), "{msg}");
    assert!(s.doc().unwrap().doc.data_merge.placeholders.is_empty(), "no placeholder was recorded");
}

#[test]
fn qr_placeholder_on_an_empty_frame_still_merges_each_record() {
    let mut s = session_with_source();
    let f = s.execute("frame.create", &json!({"rect": [72, 72, 216, 216], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("data.placeholder.add", &json!({"field": "Link", "role": "qr", "item": f})).unwrap();
    s.execute("data.merge", &json!({})).unwrap();

    let doc = &s.doc().unwrap().doc;
    let mut codes = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            it.walk(&mut |i| {
                if i.alt_text.starts_with("QR code: ") {
                    codes.push(i.alt_text.clone());
                }
            });
        }
    }
    codes.sort();
    assert_eq!(codes.len(), 3, "{codes:?}");
    assert!(codes[0].ends_with("https://a.example") && codes[2].ends_with("https://c.example"), "{codes:?}");
}
