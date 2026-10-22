//! Untrusted Affinity documents never crash DesignCraft: documents that use what the reader
//! maps (layer effects and colour overlays, also over tables, line styles, paragraph alignment,
//! frame text, underline, strikethrough and line breaks inside a paragraph, tables with two-line
//! cells, master pages, CMYK pixel layers with and without the document's CMYK profile, embedded
//! documents) are mutated and must open as an error or as a document that then renders every
//! page and exports PDF, with no panic (the command guard's internal error counts as one).
//! Original synthetic documents built with the reader's `synth` feature, not an Affinity writer.
//!
//! `PROPTEST_CASES=20000 cargo test -p designcraft-engine --test affinity_fuzz` searches deeper.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/affinity_docs.rs"]
mod affinity_docs;
#[path = "../../affinity/tests/support/text_fixture.rs"]
mod text_fixture;

use designcraft_affinity::synth::Method;
use designcraft_engine::{EngineError, Session, cmd::base64_encode};
use proptest::prelude::*;
use serde_json::json;

/// One sample document: its object stream and, for embedded content, the embedded document's.
struct Sample {
    name: &'static str,
    stream: Vec<u8>,
    embedded: Option<Vec<u8>>,
}

fn samples() -> Vec<Sample> {
    use affinity_docs::*;
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let fx = vec![shadow(), effect(b"OutG", 6.0), effect(b"InnS", 3.0), effect(b"BevE", 4.0), outline(0, 3.0), outline(2, 4.0), colour_overlay(0.5)];
    let canvas = canvas_stream(vec![
        with_effects(stroked(10, 1, &[]), fx),
        stroked(11, 0, &[]),
        stroked(12, 2, &[4.0, 2.0]),
        stroked(13, 3, &[]),
        mixed_alignment(1, 2),
        two_line_frame(150.0),
        decorated_text(70),
        with_effects(table_with(30, identity, ["A", "B\u{2028}two", "C", "D", "E"]), vec![colour_overlay(1.0)]),
    ]);
    let icc = designcraft_color::cms::icc_out::generic_cmyk_icc().unwrap().to_vec();
    let inner = canvas_stream(vec![with_effects(rectangle(10, (0.0, 0.0), (50.0, 50.0)), vec![colour_overlay(1.0)]), artistic_text(20, "Hi", 40.0)]);
    vec![
        Sample { name: "effects, strokes, text and tables", stream: canvas, embedded: None },
        Sample { name: "master pages", stream: master_stream(3), embedded: None },
        Sample { name: "CMYK pixels with a profile", stream: cmyk_stream(false, Some(&icc)), embedded: None },
        Sample { name: "CMYK pixels with transparency", stream: cmyk_stream(true, None), embedded: None },
        Sample { name: "embedded document", stream: canvas_stream(vec![embedded_node(20)]), embedded: Some(inner) },
    ]
}

/// A sample's file, its streams stored uncompressed so mutations reach the object graph.
fn file(stream: &[u8], embedded: Option<&[u8]>) -> Vec<u8> {
    let inner = embedded.map(|e| affinity_docs::packed(e, None, Method::Stored));
    affinity_docs::packed(stream, inner.as_deref(), Method::Stored)
}

/// Open `bytes`; what opens must render every page and export PDF. `Err` names a panic.
fn survive(what: &str, bytes: &[u8]) -> Result<(), String> {
    let internal = |r: Result<serde_json::Value, EngineError>| match r {
        Err(e @ EngineError::Internal(..)) => Err(format!("{what}: {e}")),
        _ => Ok(()),
    };
    let mut s = Session::new();
    let opened = s.execute("file.openBytes", &json!({"name": "fuzz.afpub", "base64": base64_encode(bytes)}));
    let ok = opened.is_ok();
    internal(opened)?;
    if !ok {
        return Ok(());
    }
    let st = s.doc().map_err(|e| e.to_string())?;
    let mut r = designcraft_render::Renderer::new();
    for page in 0..st.doc.page_count().min(8) {
        let _ = r.render_page(&st.doc, &s.cache, page, 0.1, true, &designcraft_render::RenderOptions::default());
    }
    internal(s.execute("file.exportPdf", &json!({})))
}

#[test]
fn the_samples_open_and_reach_what_they_are_for() {
    for s in samples() {
        let bytes = file(&s.stream, s.embedded.as_deref());
        let mut session = Session::new();
        let r = session.execute("file.openBytes", &json!({"name": "x.afpub", "base64": base64_encode(&bytes)})).unwrap();
        assert!(r["warnings"].is_array(), "{}: {r}", s.name);
        survive(s.name, &bytes).unwrap();
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: if std::env::var_os("PROPTEST_CASES").is_some() { ProptestConfig::default().cases } else { 64 }, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn mutated_affinity_documents_open_render_and_export(
        sample in 0usize..5,
        inner in any::<bool>(),
        cut in 0usize..8192,
        edits in prop::collection::vec((0usize..8192, any::<u8>()), 0..24),
    ) {
        let samples = samples();
        let s = &samples[sample % samples.len()];
        let (mut stream, mut embedded) = (s.stream.clone(), s.embedded.clone());
        // Mutate the embedded document's stream instead, where there is one.
        let target = match embedded.as_mut() {
            Some(e) if inner => e,
            _ => &mut stream,
        };
        let n = target.len();
        for (at, b) in edits { target[at % n] = b; }
        target.truncate(cut.max(16));
        let bytes = file(&stream, embedded.as_deref());
        prop_assert!(survive(s.name, &bytes).is_ok(), "{:?}", survive(s.name, &bytes));
    }
}
