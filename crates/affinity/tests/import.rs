//! Affinity → DesignCraft mapping on original synthetic documents (built with the `synth`
//! feature; no Affinity software or files were used).
#![cfg(feature = "synth")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/text_fixture.rs"]
mod text_fixture;

use designcraft_affinity::synth::{self, F, Method, tag};
use designcraft_color::Color;
use designcraft_doc::{Content, Item, Leading, PageSide, Shape};
use text_fixture::{attrs, block, run, shared_objects, utf8};

/// Encode a stream whose shared arrays may list inline objects.
fn stream(mut root: Vec<(designcraft_affinity::stream::Tag, F)>) -> Vec<u8> {
    let mut next_id = 1000;
    for (_, value) in &mut root {
        shared_objects(value, &mut next_id);
    }
    synth::stream(&root)
}

/// 300 dpi: a document pixel is 0.24 pt.
const DPI: f64 = 300.0;
const PT: f64 = 72.0 / DPI;

fn cmyk(c: f32, m: f32, y: f32, k: f32) -> F {
    F::Obj(
        tag(b"FilS"),
        vec![(tag(b"Colr"), F::Obj(tag(b"CMYK"), vec![(tag(b"_col"), F::Struct([c, m, y, k, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect()))]))],
    )
}

fn rectangle(id: u32, at: (f64, f64), size: (f64, f64)) -> F {
    F::Def(
        id,
        vec![tag(b"ShpN")],
        vec![
            (tag(b"ShpB"), F::F64s(vec![0.0, 0.0, size.0, size.1])),
            (tag(b"Shpe"), F::Def(id + 10_000, vec![tag(b"ShNR")], vec![])),
            (tag(b"Xfrm"), F::F64s(vec![1.0, 0.0, at.0, 0.0, 1.0, at.1])),
            (tag(b"BFil"), cmyk(0.0, 1.0, 0.0, 0.0)),
            (tag(b"Desc"), F::Str("Magenta box".into())),
        ],
    )
}

fn artistic_text(id: u32, text: &str, size: f64) -> F {
    F::Def(
        id,
        vec![tag(b"TxtA")],
        vec![
            (
                tag(b"StSt"),
                F::Obj(
                    tag(b"Stry"),
                    vec![(
                        tag(b"Blok"),
                        F::Shared(vec![block(
                            utf8(text),
                            vec![run(text.chars().count() as i32, Some(attrs("Inter", "Inter-Bold", 700, false, size)))],
                            &[1],
                        )]),
                    )],
                ),
            ),
            (tag(b"TxtH"), F::Obj(tag(b"ArFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 400.0, 100.0])), (tag(b"ArtV"), F::F64(80.0))])),
            (tag(b"Xfrm"), F::F64s(vec![1.0, 0.0, 1000.0, 0.0, 1.0, 500.0])),
        ],
    )
}

/// A two-page Publisher spread at 300 dpi: a layer with a CMYK rectangle, and centred artistic text.
fn publisher() -> Vec<u8> {
    let page = |x0: f64| F::Obj(tag(b"PagR"), vec![(tag(b"rctp"), F::F64s(vec![x0, 0.0, x0 + 2480.0, 3508.0]))]);
    let layer = F::Def(
        3,
        vec![tag(b"Scop")],
        vec![(tag(b"Desc"), F::Str("Background".into())), (tag(b"Chld"), F::Shared(vec![rectangle(10, (100.0, 100.0), (100.0, 200.0))]))],
    );
    let spread = F::Def(
        2,
        vec![tag(b"Sprd")],
        vec![
            (tag(b"SprB"), F::F64s(vec![0.0, 0.0, 4960.0, 3508.0])),
            (tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared(vec![page(0.0), page(2480.0)]))])),
            (tag(b"Chld"), F::Shared(vec![layer, artistic_text(20, "Hello\u{2029}World\u{2029}", 100.0)])),
        ],
    );
    let data = stream(vec![
        (tag(b"UVCn"), F::Obj(tag(b"UVCn"), vec![(tag(b"UPPI"), F::F64(DPI))])),
        (tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread]))])),
    ]);
    synth::container(&[("doc.dat", &data, Method::Zstd)], None)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn find<'a>(items: &'a [std::sync::Arc<Item>], name: &str) -> &'a Item {
    items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("no item {name}"))
}

#[test]
fn publisher_spread_becomes_facing_pages_in_points() {
    let imported = designcraft_affinity::import(&publisher()).unwrap();
    let d = &imported.document;
    assert!(d.settings.facing_pages);
    assert_eq!(d.spreads.len(), 1);
    let pages = &d.spreads[0].pages;
    assert_eq!(pages.iter().map(|p| p.side).collect::<Vec<_>>(), [PageSide::Left, PageSide::Right]);
    assert!(near(pages[0].width, 2480.0 * PT) && near(pages[0].height, 3508.0 * PT), "{:?}", (pages[0].width, pages[0].height));
    assert!(near(pages[1].x, 2480.0 * PT));
    assert!(near(d.settings.page_width, 2480.0 * PT));
    d.check().unwrap();
}

#[test]
fn layers_shapes_and_cmyk_colours_are_kept() {
    let imported = designcraft_affinity::import(&publisher()).unwrap();
    let d = &imported.document;
    let layer = d.layers.iter().find(|l| l.name == "Background").expect("the Affinity layer becomes a document layer");
    let rect = find(&d.spreads[0].items, "Magenta box");
    assert_eq!(rect.layer, layer.id);
    assert_eq!(rect.shape, Shape::Rectangle);
    let b = rect.bounds();
    assert!(near(b.x0, 100.0 * PT) && near(b.y0, 100.0 * PT) && near(b.width(), 100.0 * PT) && near(b.height(), 200.0 * PT), "{b:?}");
    assert_eq!(d.resolve_color(&rect.fill.swatch, 1.0), Some(Color::cmyk(0.0, 1.0, 0.0, 0.0)), "CMYK stays CMYK");
    // The text sat on no layer: it keeps the document's own layer, which therefore stays.
    assert_eq!(d.layers.len(), 2);
}

#[test]
fn artistic_text_becomes_a_frame_with_its_baseline_at_the_anchor() {
    let imported = designcraft_affinity::import(&publisher()).unwrap();
    let d = &imported.document;
    let frame = d.spreads[0].items.iter().find(|i| i.is_text_frame()).unwrap();
    let Content::Text(tf) = &frame.content else { unreachable!() };
    let story = d.story(tf.story).unwrap();
    assert_eq!(story.text, "Hello\nWorld", "paragraph marks become paragraphs; the final one goes");
    assert_eq!(story.paras.len(), 2);
    assert_eq!(story.paras[0].para.align, Some(designcraft_doc::Align::Center));
    let f = &story.chars[0].format.over;
    assert_eq!(f.font_family.as_deref(), Some("Inter"));
    assert_eq!(f.font_style.as_deref(), Some("Bold"));
    assert!(near(f.size.unwrap(), 100.0 * PT));
    assert!(f.leading.is_none() || matches!(f.leading, Some(Leading::Points(_))));
    // Centred on the anchor (x = 1000 + 200), first baseline 80 px below the frame top (y = 500).
    let b = frame.bounds();
    assert!(near((b.x0 + b.x1) / 2.0, 1200.0 * PT), "{b:?}");
    assert!(near(b.y0 + tf.options.first_baseline_min, 580.0 * PT), "{b:?} {:?}", tf.options);
    assert_eq!(tf.options.first_baseline, designcraft_doc::FirstBaseline::Fixed);
}

#[test]
fn not_an_affinity_file_is_an_error_not_a_crash() {
    assert!(designcraft_affinity::import(b"not affinity").is_err());
    let mut file = publisher();
    file.truncate(file.len() / 2);
    assert!(designcraft_affinity::import(&file).is_err());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(256))]
    #[test]
    fn mutated_documents_never_panic(edits in proptest::collection::vec((0usize..4096, proptest::prelude::any::<u8>()), 1..16)) {
        // The stream is stored uncompressed so edits reach the object graph, not just the codec.
        let mut file = {
            let page = F::Obj(tag(b"PagR"), vec![(tag(b"rctp"), F::F64s(vec![0.0, 0.0, 2480.0, 3508.0]))]);
            let spread = F::Def(2, vec![tag(b"Sprd")], vec![
                (tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared(vec![page]))])),
                (tag(b"Chld"), F::Shared(vec![rectangle(10, (1.0, 2.0), (3.0, 4.0)), artistic_text(20, "Hi", 40.0)])),
            ]);
            let data = stream(vec![(tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread]))]))]);
            synth::container(&[("doc.dat", &data, Method::Stored)], None)
        };
        let n = file.len();
        for (at, v) in edits {
            if let Some(b) = file.get_mut(at % n) {
                *b = v;
            }
        }
        if let Ok(imported) = designcraft_affinity::import(&file) {
            proptest::prop_assert!(imported.document.check().is_ok());
        }
    }
}
