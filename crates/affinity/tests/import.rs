//! Affinity → DesignCraft mapping on original synthetic documents (built with the `synth`
//! feature; no Affinity software or files were used).
#![cfg(feature = "synth")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/text_fixture.rs"]
mod text_fixture;

use designcraft_affinity::synth::{self, F, Method, tag};
use designcraft_color::Color;
use designcraft_doc::{Content, Item, Leading, PageSide, Shape};
use text_fixture::{array_f64, array_i32, attrs, block, run, shared_objects, utf8};

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

/// A blue 2 px line on a table edge.
fn table_edge() -> F {
    F::Obj(
        tag(b"TEdg"),
        vec![
            (tag(b"Line"), F::Obj(tag(b"LDsc"), vec![(tag(b"LDeL"), F::Obj(tag(b"LSty"), vec![(tag(b"Wght"), F::F64(2.0))]))])),
            (tag(b"Fill"), F::Obj(tag(b"FDsc"), vec![(tag(b"FDeF"), cmyk(1.0, 0.5, 0.0, 0.0))])),
        ],
    )
}

/// A 3 × 2 table whose cell (1, 1) is merged into (1, 0); cells "A".."E" in reading order, each
/// ended by a cell break. Cells are 100 × 50 px from (0, 0); `cells` cell records (6 for the grid).
fn table_node(id: u32, cells: usize, transform: [f64; 6]) -> F {
    let texts = ["A", "B", "C", "D", "E"];
    let brk = || F::Obj(tag(b"BrGl"), vec![(tag(b"HdBk"), F::Enum(4, 1))]);
    let segments: Vec<F> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut fields = vec![(tag(b"Utf8"), F::Str((*t).into()))];
            if i > 0 {
                fields.insert(0, (tag(b"Glys"), F::Shared(vec![brk()])));
            }
            F::Obj(tag(b"GSeg"), fields)
        })
        .collect();
    let len = (texts.len() * 2 - 1) as i32;
    let glyphs = F::Obj(tag(b"GStr"), vec![(tag(b"Mixd"), F::Shared(segments))]);
    let story = block(glyphs, vec![run(len, Some(attrs("Inter", "Inter-Regular", 400, false, 40.0)))], &[]);
    let cell = |merged: bool| {
        let mut f = vec![(tag(b"AliY"), F::Enum(1, 0)), (tag(b"Inse"), F::F64s(vec![5.0, 6.0, 7.0, 8.0]))];
        if merged {
            f.push((tag(b"BrLf"), F::Bool(true)));
        }
        F::Obj(tag(b"TCel"), f)
    };
    let pos = |values: &[f64]| F::Obj(tag(b"TPos"), vec![(tag(b"Posn"), array_f64(b"Posn", values))]);
    let edges = |n: usize| F::Obj(tag(b"TEds"), vec![(tag(b"Edge"), F::Shared((0..n).map(|_| table_edge()).collect()))]);
    let table = F::Obj(
        tag(b"Tabl"),
        vec![
            (tag(b"CPos"), pos(&[0.0, 100.0, 200.0, 300.0])),
            (tag(b"RPos"), pos(&[0.0, 50.0, 100.0])),
            (tag(b"CEdg"), edges(8)),
            (tag(b"REdg"), edges(9)),
            (tag(b"Cell"), F::Obj(tag(b"TCls"), vec![(tag(b"Cell"), F::Shared((0..cells).map(|i| cell(i == 4)).collect()))])),
        ],
    );
    F::Def(
        id,
        vec![tag(b"TxtT")],
        vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![story]))])),
            (tag(b"TxtH"), F::Obj(tag(b"TbFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 300.0, 100.0])), (tag(b"Tabl"), table)])),
            (tag(b"Xfrm"), F::F64s(transform.to_vec())),
        ],
    )
}

/// One A4 page at 300 dpi holding `node`.
fn page_with(node: F) -> Vec<u8> {
    let page = F::Obj(tag(b"PagR"), vec![(tag(b"rctp"), F::F64s(vec![0.0, 0.0, 2480.0, 3508.0]))]);
    let spread = F::Def(
        2,
        vec![tag(b"Sprd")],
        vec![(tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared(vec![page]))])), (tag(b"Chld"), F::Shared(vec![node]))],
    );
    let data = stream(vec![
        (tag(b"UVCn"), F::Obj(tag(b"UVCn"), vec![(tag(b"UPPI"), F::F64(DPI))])),
        (tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread]))])),
    ]);
    synth::container(&[("doc.dat", &data, Method::Zstd)], None)
}

fn only_table(d: &designcraft_doc::Document) -> (&Item, designcraft_doc::Table) {
    let frame = d.spreads[0].items.iter().find(|i| i.is_text_frame()).expect("a table frame");
    let Content::Text(tf) = &frame.content else { unreachable!() };
    let story = d.story(tf.story).unwrap();
    let table = story.tables.values().next().expect("the frame's story holds the table");
    (frame, (**table).clone())
}

/// Character attributes without their own fixed leading.
fn auto_leading(mut attrs: F) -> F {
    if let F::Obj(_, fields) = &mut attrs {
        for (t, v) in fields.iter_mut() {
            if *t == tag(b"Ints") {
                *v = array_i32(b"Ints", &[0, 0, 0, 0, 0]);
            }
        }
    }
    attrs
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
    // Artistic text spaces its lines by the font size (DesignCraft's default is 120 %).
    assert!(story.paras.iter().all(|p| p.para.auto_leading == Some(1.0)), "{:?}", story.paras);
}

#[test]
fn tables_keep_their_grid_text_merged_cells_and_lines() {
    // Stretched to twice its width by its transform: the grid follows, the type doesn't.
    let imported = designcraft_affinity::import(&page_with(table_node(30, 6, [2.0, 0.0, 400.0, 0.0, 1.0, 600.0]))).unwrap();
    let d = &imported.document;
    let (frame, t) = only_table(d);
    assert_eq!((t.nrows(), t.ncols()), (2, 3));
    assert!(t.columns.iter().all(|c| near(c.width, 200.0 * PT)), "{:?}", t.columns);
    assert!(t.rows.iter().all(|r| near(r.height, 50.0 * PT)), "{:?}", t.rows);
    let text = |r: usize, c: usize| t.cell(r, c).unwrap().text.text.clone();
    assert_eq!([text(0, 0), text(0, 1), text(0, 2), text(1, 0), text(1, 2)], ["A", "B", "C", "D", "E"]);
    assert_eq!(t.cell(1, 0).unwrap().col_span, 2, "the merged cell spans two columns");
    let a = t.cell(0, 0).unwrap();
    assert_eq!(a.text.chars[0].format.over.size, Some(40.0 * PT), "type keeps its size");
    assert_eq!(a.text.chars[0].format.over.h_scale, None, "type isn't stretched with the grid");
    assert_eq!(a.vj, designcraft_doc::VerticalJustification::Center);
    assert!(near(a.insets[1], 5.0 * 2.0 * PT), "left inset scales with the grid: {:?}", a.insets);
    let line = &a.strokes[0];
    assert!(line.is_visible() && line.weight > 0.0);
    assert_eq!(d.resolve_color(&line.color, 1.0), Some(Color::cmyk(1.0, 0.5, 0.0, 0.0)));
    let b = frame.bounds();
    assert!(near(b.x0, 400.0 * PT) && near(b.y0, 600.0 * PT) && near(b.width(), 600.0 * PT), "{b:?}");
    d.check().unwrap();
}

#[test]
fn a_table_whose_cells_do_not_match_its_grid_still_opens() {
    let imported = designcraft_affinity::import(&page_with(table_node(30, 2, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]))).unwrap();
    assert!(imported.warnings.iter().any(|w| w.contains("cells don't match their grid")), "{:?}", imported.warnings);
    let (_, t) = only_table(&imported.document);
    assert_eq!((t.nrows(), t.ncols()), (2, 3));
    // Without cell records nothing is merged: the five texts fill the first five cells.
    assert_eq!(t.cell(1, 1).unwrap().text.text, "E");
    imported.document.check().unwrap();
}

#[test]
fn each_paragraph_keeps_its_own_alignment() {
    // "Left" then "Right": paragraph runs end at characters 5 and 11.
    let text = "Left\u{2029}Right\u{2029}";
    let para = |end: i32, align: i32| {
        vec![(tag(b"Indx"), F::I32(end)), (tag(b"Item"), F::Obj(tag(b"PAtt"), vec![(tag(b"Ints"), array_i32(b"Ints", &[align]))]))]
    };
    let blk = F::Obj(
        tag(b"StBl"),
        vec![
            (tag(b"Glyp"), utf8(text)),
            (
                tag(b"GAtt"),
                F::Obj(
                    tag(b"GlAS"),
                    vec![(tag(b"Runs"), F::Objs(tag(b"GlAR"), vec![run(11, Some(attrs("Inter", "Inter-Regular", 400, false, 30.0)))]))],
                ),
            ),
            (tag(b"PAtt"), F::Obj(tag(b"PaAS"), vec![(tag(b"Runs"), F::Objs(tag(b"PaAR"), vec![para(5, 0), para(11, 2)]))])),
        ],
    );
    let node = F::Def(
        40,
        vec![tag(b"TxtF")],
        vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![blk]))])),
            (tag(b"TxtH"), F::Obj(tag(b"FrFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 500.0, 300.0]))])),
        ],
    );
    let imported = designcraft_affinity::import(&page_with(node)).unwrap();
    assert!(!imported.warnings.iter().any(|w| w.contains("mixed paragraph alignments")), "{:?}", imported.warnings);
    let d = &imported.document;
    let frame = d.spreads[0].items.iter().find(|i| i.is_text_frame()).unwrap();
    let Content::Text(tf) = &frame.content else { unreachable!() };
    let story = d.story(tf.story).unwrap();
    assert_eq!(story.text, "Left\nRight");
    let aligns: Vec<_> = story.paras.iter().map(|p| p.para.align).collect();
    assert_eq!(aligns, [None, Some(designcraft_doc::Align::Right)]);
}

#[test]
fn paragraph_indents_and_spacing_follow_affinity() {
    // Doub slots: 1 leading (Ints slot 1 = 2), 2 left, 3 right, 4 first line, 5 space before, 6 after.
    let text = "One\u{2029}Two\u{2029}";
    let para = |end: i32| {
        vec![
            (tag(b"Indx"), F::I32(end)),
            (
                tag(b"Item"),
                F::Obj(
                    tag(b"PAtt"),
                    vec![(tag(b"Doub"), array_f64(b"Doub", &[1.0, 5.0, 40.0, 3.0, 25.0, 9.0, 17.0])), (tag(b"Ints"), array_i32(b"Ints", &[0, 2]))],
                ),
            ),
        ]
    };
    let blk = F::Obj(
        tag(b"StBl"),
        vec![
            (tag(b"Glyp"), utf8(text)),
            (
                tag(b"GAtt"),
                F::Obj(
                    tag(b"GlAS"),
                    vec![(tag(b"Runs"), F::Objs(tag(b"GlAR"), vec![run(8, Some(auto_leading(attrs("Inter", "Inter-Regular", 400, false, 30.0))))]))],
                ),
            ),
            (tag(b"PAtt"), F::Obj(tag(b"PaAS"), vec![(tag(b"Runs"), F::Objs(tag(b"PaAR"), vec![para(4), para(8)]))])),
        ],
    );
    let node = F::Def(
        40,
        vec![tag(b"TxtF")],
        vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![blk]))])),
            (tag(b"TxtH"), F::Obj(tag(b"FrFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 500.0, 300.0]))])),
        ],
    );
    let imported = designcraft_affinity::import(&page_with(node)).unwrap();
    assert!(!imported.warnings.iter().any(|w| w.contains("indents or spacing")), "{:?}", imported.warnings);
    let d = &imported.document;
    let frame = d.spreads[0].items.iter().find(|i| i.is_text_frame()).unwrap();
    let Content::Text(tf) = &frame.content else { unreachable!() };
    let story = d.story(tf.story).unwrap();
    let p = |i: usize| story.paras[i].para.clone();
    let pt = |v: f64| Some(v * PT);
    assert_eq!((p(0).left_indent, p(0).first_line_indent, p(0).right_indent), (pt(40.0), pt(-15.0), pt(3.0)));
    assert_eq!(story.chars[0].format.over.leading, Some(designcraft_doc::Leading::Points(5.0 * PT)), "the paragraph's fixed leading");
    assert_eq!((p(0).space_before, p(0).space_after), (pt(9.0), pt(17.0)));
    // The larger of 17 after "One" and 9 before "Two": nothing more before "Two".
    assert_eq!((p(1).space_before, p(1).space_after), (None, pt(17.0)));
    assert_eq!(p(0).hyphenate, Some(false));
}

#[test]
fn frame_type_follows_the_frame_text_scale_not_the_frame_transform() {
    // The same frame scaled 3× by its transform, without and with a text scale (FTxS) of 3.
    let frame = |id: u32, ftxs: bool| {
        let story = block(utf8("Hi"), vec![run(2, Some(attrs("Inter", "Inter-Regular", 400, false, 10.0)))], &[]);
        let mut fields = vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![story]))])),
            (tag(b"TxtH"), F::Obj(tag(b"FrFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 100.0, 50.0]))])),
            (tag(b"Xfrm"), F::F64s(vec![3.0, 0.0, 0.0, 0.0, 3.0, 0.0])),
        ];
        if ftxs {
            fields.push((tag(b"FTxS"), F::F64s(vec![3.0, 0.0, 0.0, 0.0, 3.0, 0.0])));
        }
        F::Def(id, vec![tag(b"TxtF")], fields)
    };
    for (ftxs, size) in [(false, 10.0), (true, 30.0)] {
        let imported = designcraft_affinity::import(&page_with(frame(40, ftxs))).unwrap();
        let d = &imported.document;
        let item = d.spreads[0].items.iter().find(|i| i.is_text_frame()).unwrap();
        assert!(near(item.bounds().width(), 300.0 * PT), "the frame scales either way");
        let Content::Text(tf) = &item.content else { unreachable!() };
        assert_eq!(d.story(tf.story).unwrap().chars[0].format.over.size, Some(size * PT), "FTxS {ftxs}");
    }
}

/// A document at 72 dpi holding `nodes` on a canvas without pages.
fn canvas_with(nodes: Vec<F>, embedded: Option<&[u8]>) -> Vec<u8> {
    let spread = F::Def(2, vec![tag(b"Sprd")], vec![(tag(b"SprB"), F::F64s(vec![0.0, 0.0, 1000.0, 1000.0])), (tag(b"Chld"), F::Shared(nodes))]);
    let data = stream(vec![(tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread]))]))]);
    match embedded {
        Some(inner) => {
            let mut entry = b"EmDc\0\0\0\0".to_vec();
            entry.extend_from_slice(inner);
            synth::container(&[("doc.dat", &data, Method::Zstd), ("edc/1", &entry, Method::Zstd)], None)
        }
        None => synth::container(&[("doc.dat", &data, Method::Zstd)], None),
    }
}

/// An instance of the embedded document `edc/1`, scaled 2× with its content's centre at (500, 300).
fn embedded_node(id: u32) -> F {
    let content = F::Obj(tag(b"EmbC"), vec![(tag(b"EmbC"), F::Entry("edc/1".into()))]);
    F::Def(
        id,
        vec![tag(b"EmbN")],
        vec![
            (tag(b"Bitm"), F::Obj(tag(b"EmbR"), vec![(tag(b"EmCn"), content), (tag(b"PBBx"), F::Enum(2, 0))])),
            (tag(b"Xfrm"), F::F64s(vec![2.0, 0.0, 500.0, 0.0, 2.0, 300.0])),
        ],
    )
}

#[test]
fn embedded_affinity_documents_come_in_as_their_content() {
    let inner = canvas_with(vec![rectangle(10, (0.0, 0.0), (100.0, 50.0))], None);
    let outer = canvas_with(vec![embedded_node(20)], Some(&inner));
    let imported = designcraft_affinity::import(&outer).unwrap();
    let d = &imported.document;
    let group = d.spreads[0].items.first().expect("the embedded document's group");
    let Content::Group { items } = &group.content else { panic!("a group: {:?}", group.content) };
    let rect = items.iter().find(|i| i.name == "Magenta box").expect("the embedded rectangle");
    // Content bounds (0,0)-(100,50), centre (50,25) → origin, then 2× at (500,300).
    let b = rect.bounds();
    assert!(near(b.x0, 400.0) && near(b.y0, 250.0) && near(b.width(), 200.0) && near(b.height(), 100.0), "{b:?}");
    assert_eq!(d.resolve_color(&rect.fill.swatch, 1.0), Some(Color::cmyk(0.0, 1.0, 0.0, 0.0)));
    assert!(!imported.warnings.iter().any(|w| w.contains("without a cached picture")), "{:?}", imported.warnings);
}

#[test]
fn embedded_documents_nest_only_so_deep() {
    let mut doc = canvas_with(vec![rectangle(10, (0.0, 0.0), (100.0, 50.0))], None);
    for _ in 0..6 {
        doc = canvas_with(vec![embedded_node(20)], Some(&doc));
    }
    let imported = designcraft_affinity::import(&doc).unwrap();
    assert!(imported.warnings.iter().any(|w| w.contains("nested more than four deep")), "{:?}", imported.warnings);
    imported.document.check().unwrap();
}

#[test]
fn master_pages_become_parents_showing_the_right_master_page() {
    // A two-page master spread (pages 0–100 and 100–200 wide) with a box on each page; the
    // document's one page shows its second page, as Affinity's instance says (MPOf 1).
    let master_page = |x0: f64| F::Obj(tag(b"PagR"), vec![(tag(b"rctp"), F::F64s(vec![x0, 0.0, x0 + 100.0, 150.0]))]);
    let master = F::Def(
        7,
        vec![tag(b"Sprd")],
        vec![
            (tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared(vec![master_page(0.0), master_page(100.0)]))])),
            (tag(b"Chld"), F::Shared(vec![rectangle(30, (10.0, 10.0), (20.0, 20.0)), rectangle(31, (110.0, 10.0), (30.0, 40.0))])),
        ],
    );
    let instance = F::Def(
        5,
        vec![tag(b"MPIN")],
        vec![
            (tag(b"SLnk"), F::Obj(tag(b"ILSN"), vec![(tag(b"ILOb"), F::Shared(vec![master, F::Ref(5)]))])),
            (tag(b"PgOf"), F::U32(0)),
            (tag(b"MPOf"), F::U32(1)),
            (tag(b"PgCt"), F::U32(1)),
        ],
    );
    let page = F::Obj(tag(b"PagR"), vec![(tag(b"rctp"), F::F64s(vec![0.0, 0.0, 100.0, 150.0]))]);
    let spread = F::Def(
        2,
        vec![tag(b"Sprd")],
        vec![(tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared(vec![page]))])), (tag(b"Chld"), F::Shared(vec![instance]))],
    );
    let data = stream(vec![(
        tag(b"DocR"),
        F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread])), (tag(b"MpCh"), F::Shared(vec![F::Ref(7)]))]),
    )]);
    let imported = designcraft_affinity::import(&synth::container(&[("doc.dat", &data, Method::Zstd)], None)).unwrap();
    let d = &imported.document;
    assert!(!imported.warnings.iter().any(|w| w.contains("master")), "{:?}", imported.warnings);
    let parent_id = d.spreads[0].pages[0].parent.expect("the page has a parent");
    let parent = d.parents.iter().find(|p| p.id == parent_id).unwrap();
    assert_eq!(parent.pages.len(), 1);
    assert_eq!(parent.parent.as_ref().map(|p| p.prefix.as_str()), Some("A"));
    // Only the second master page's box, at its place on that page.
    assert_eq!(parent.items.len(), 1, "{:?}", parent.items.iter().map(|i| i.bounds()).collect::<Vec<_>>());
    let b = parent.items[0].bounds();
    assert!(near(b.x0, 10.0) && near(b.y0, 10.0) && near(b.width(), 30.0) && near(b.height(), 40.0), "{b:?}");
    assert_eq!(d.parent_page_for(0), Some((0, 0)));
    d.check().unwrap();
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
                (tag(b"Chld"), F::Shared(vec![rectangle(10, (1.0, 2.0), (3.0, 4.0)), artistic_text(20, "Hi", 40.0), table_node(30, 6, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0])])),
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
