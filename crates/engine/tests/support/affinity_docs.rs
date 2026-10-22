//! Original synthetic Affinity documents for the engine's Affinity import fuzzer,
//! built with the reader's `synth` feature in the layout the reader accepts (no Affinity
//! software or files were used).
#![allow(dead_code)]

use crate::text_fixture::{array_f64, array_i32, attrs, block, run, shared_objects, utf8};
use designcraft_affinity::synth::{self, F, Method, tag};

pub type Field = (designcraft_affinity::stream::Tag, F);

/// Encode a stream whose shared arrays may list inline objects.
pub fn stream(mut root: Vec<Field>) -> Vec<u8> {
    let mut next_id = 1000;
    for (_, value) in &mut root {
        shared_objects(value, &mut next_id);
    }
    synth::stream(&root)
}

/// 300 dpi: a document pixel is 0.24 pt.
pub const DPI: f64 = 300.0;

pub const PT: f64 = 72.0 / DPI;

pub fn rgba(r: f32, g: f32, b: f32) -> F {
    F::Obj(tag(b"RGBA"), vec![(tag(b"_col"), F::Struct([r, g, b, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect()))])
}

pub fn cmyk(c: f32, m: f32, y: f32, k: f32) -> F {
    F::Obj(
        tag(b"FilS"),
        vec![(tag(b"Colr"), F::Obj(tag(b"CMYK"), vec![(tag(b"_col"), F::Struct([c, m, y, k, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect()))]))],
    )
}

/// A magenta rectangle named `name`.
pub fn named_rect(id: u32, name: &str, at: (f64, f64), size: (f64, f64)) -> F {
    F::Def(
        id,
        vec![tag(b"ShpN")],
        vec![
            (tag(b"ShpB"), F::F64s(vec![0.0, 0.0, size.0, size.1])),
            (tag(b"Shpe"), F::Def(id + 10_000, vec![tag(b"ShNR")], vec![])),
            (tag(b"Xfrm"), F::F64s(vec![1.0, 0.0, at.0, 0.0, 1.0, at.1])),
            (tag(b"BFil"), cmyk(0.0, 1.0, 0.0, 0.0)),
            (tag(b"Desc"), F::Str(name.into())),
        ],
    )
}

pub fn rectangle(id: u32, at: (f64, f64), size: (f64, f64)) -> F {
    named_rect(id, "Magenta box", at, size)
}

/// An artboard (`ABEn`) named `name` over `bounds` at `at`, holding `children`.
pub fn board(id: u32, name: &str, at: (f64, f64), size: (f64, f64), children: Vec<F>) -> F {
    F::Def(
        id,
        vec![tag(b"ShpN")],
        vec![
            (tag(b"Desc"), F::Str(name.into())),
            (tag(b"ABEn"), F::Bool(true)),
            (tag(b"Shpe"), F::Def(id + 10_000, vec![tag(b"ShNR")], vec![])),
            (tag(b"ShpB"), F::F64s(vec![0.0, 0.0, size.0, size.1])),
            (tag(b"Xfrm"), F::F64s(vec![1.0, 0.0, at.0, 0.0, 1.0, at.1])),
            (tag(b"Chld"), F::Shared(children)),
        ],
    )
}

pub fn artistic_text(id: u32, text: &str, size: f64) -> F {
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

pub fn page(x0: f64, w: f64, h: f64) -> F {
    F::Obj(tag(b"PagR"), vec![(tag(b"rctp"), F::F64s(vec![x0, 0.0, x0 + w, h]))])
}

/// A Publisher spread of A4 pages at 300 dpi holding `nodes`.
pub fn spread(id: u32, pages: usize, nodes: Vec<F>) -> F {
    let w = 2480.0 * pages as f64;
    F::Def(
        id,
        vec![tag(b"Sprd")],
        vec![
            (tag(b"SprB"), F::F64s(vec![0.0, 0.0, w, 3508.0])),
            (
                tag(b"SpMd"),
                F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared((0..pages).map(|i| page(2480.0 * i as f64, 2480.0, 3508.0)).collect()))]),
            ),
            (tag(b"Chld"), F::Shared(nodes)),
        ],
    )
}

pub fn publisher_with(spreads: Vec<F>) -> Vec<u8> {
    let data = stream(vec![
        (tag(b"UVCn"), F::Obj(tag(b"UVCn"), vec![(tag(b"UPPI"), F::F64(DPI))])),
        (tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(spreads))])),
    ]);
    synth::container(&[("doc.dat", &data, Method::Zstd)], None)
}

/// A two-page spread: a layer with a CMYK rectangle, and centred artistic text.
pub fn publisher() -> Vec<u8> {
    let layer = F::Def(
        3,
        vec![tag(b"Scop")],
        vec![(tag(b"Desc"), F::Str("Background".into())), (tag(b"Chld"), F::Shared(vec![rectangle(10, (100.0, 100.0), (100.0, 200.0))]))],
    );
    publisher_with(vec![spread(2, 2, vec![layer, artistic_text(20, "Hello\u{2029}World\u{2029}", 100.0)])])
}

/// One A4 page at 300 dpi holding `node`.
pub fn page_with(node: F) -> Vec<u8> {
    publisher_with(vec![spread(2, 1, vec![node])])
}

/// A document at 72 dpi holding `nodes` on a canvas without pages; `embedded` is `edc/1`.
pub fn canvas_with(nodes: Vec<F>, embedded: Option<&[u8]>) -> Vec<u8> {
    packed(&canvas_stream(nodes), embedded, Method::Zstd)
}

/// The object stream of [`canvas_with`].
pub fn canvas_stream(nodes: Vec<F>) -> Vec<u8> {
    let spread = F::Def(2, vec![tag(b"Sprd")], vec![(tag(b"SprB"), F::F64s(vec![0.0, 0.0, 1000.0, 1000.0])), (tag(b"Chld"), F::Shared(nodes))]);
    stream(vec![(tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread]))]))])
}

/// A document of object stream `data`, packed with `method`; `embedded` is entry `edc/1`.
pub fn packed(data: &[u8], embedded: Option<&[u8]>, method: Method) -> Vec<u8> {
    match embedded {
        Some(inner) => {
            let mut entry = b"EmDc\0\0\0\0".to_vec();
            entry.extend_from_slice(inner);
            synth::container(&[("doc.dat", data, method), ("edc/1", &entry, method)], None)
        }
        None => synth::container(&[("doc.dat", data, method)], None),
    }
}

/// An instance of the embedded document `edc/1`, scaled 2× with its content's centre at (500, 300).
pub fn embedded_node(id: u32) -> F {
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

/// A blue 2 px line on a table edge.
pub fn table_edge() -> F {
    F::Obj(
        tag(b"TEdg"),
        vec![
            (tag(b"Line"), F::Obj(tag(b"LDsc"), vec![(tag(b"LDeL"), F::Obj(tag(b"LSty"), vec![(tag(b"Wght"), F::F64(2.0))]))])),
            (tag(b"Fill"), F::Obj(tag(b"FDsc"), vec![(tag(b"FDeF"), cmyk(1.0, 0.5, 0.0, 0.0))])),
        ],
    )
}

/// A 3 × 2 table whose cell (1, 1) is merged into (1, 0); cells "A".."E" in reading order, each
/// ended by a cell break. Cells are 100 × 50 px from (0, 0).
pub fn table_node(id: u32, transform: [f64; 6]) -> F {
    table_with(id, transform, ["A", "B", "C", "D", "E"])
}

/// [`table_node`] with these cell texts.
pub fn table_with(id: u32, transform: [f64; 6], texts: [&str; 5]) -> F {
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
    let len = (texts.iter().map(|t| t.chars().count()).sum::<usize>() + texts.len() - 1) as i32;
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
            (tag(b"Cell"), F::Obj(tag(b"TCls"), vec![(tag(b"Cell"), F::Shared((0..6).map(|i| cell(i == 4)).collect()))])),
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

/// Character attributes without their own fixed leading.
pub fn auto_leading(mut attrs: F) -> F {
    if let F::Obj(_, fields) = &mut attrs {
        for (t, v) in fields.iter_mut() {
            if *t == tag(b"Ints") {
                *v = array_i32(b"Ints", &[0, 0, 0, 0, 0]);
            }
        }
    }
    attrs
}

/// A two-page master spread (pages 0–100 and 100–200 wide) with a box on each page; the
/// document's page shows its second page, as Affinity's instance says (MPOf 1).
pub fn with_master(pages: usize) -> Vec<u8> {
    packed(&master_stream(pages), None, Method::Zstd)
}

/// The object stream of [`with_master`].
pub fn master_stream(pages: usize) -> Vec<u8> {
    let master = F::Def(
        7,
        vec![tag(b"Sprd")],
        vec![
            (tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared(vec![page(0.0, 100.0, 150.0), page(100.0, 100.0, 150.0)]))])),
            (
                tag(b"Chld"),
                F::Shared(vec![
                    named_rect(30, "Left master box", (10.0, 10.0), (20.0, 20.0)),
                    named_rect(31, "Right master box", (110.0, 10.0), (30.0, 40.0)),
                ]),
            ),
        ],
    );
    let instance = F::Def(
        5,
        vec![tag(b"MPIN")],
        vec![
            (tag(b"SLnk"), F::Obj(tag(b"ILSN"), vec![(tag(b"ILOb"), F::Shared(vec![master, F::Ref(5)]))])),
            (tag(b"PgOf"), F::U32(0)),
            (tag(b"MPOf"), F::U32(1)),
            (tag(b"PgCt"), F::U32(pages as u32)),
        ],
    );
    let spread = F::Def(
        2,
        vec![tag(b"Sprd")],
        vec![
            (tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared((0..pages).map(|i| page(100.0 * i as f64, 100.0, 150.0)).collect()))])),
            (tag(b"Chld"), F::Shared(vec![instance, named_rect(40, "Page box", (50.0, 100.0), (10.0, 10.0))])),
        ],
    );
    stream(vec![(
        tag(b"DocR"),
        F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread])), (tag(b"MpCh"), F::Shared(vec![F::Ref(7)]))]),
    )])
}

pub fn with_effects(mut node: F, effects: Vec<F>) -> F {
    if let F::Def(_, _, fields) = &mut node {
        fields.push((tag(b"FiEf"), F::Shared(effects)));
    }
    node
}

/// Artistic text "Wide line" over "Mid", the paragraphs aligned `first` and `second`
/// (0 left, 1 centre, 2 right).
pub fn mixed_alignment(first: i32, second: i32) -> F {
    let para = |end: i32, align: i32| {
        vec![(tag(b"Indx"), F::I32(end)), (tag(b"Item"), F::Obj(tag(b"PAtt"), vec![(tag(b"Ints"), array_i32(b"Ints", &[align]))]))]
    };
    let blk = F::Obj(
        tag(b"StBl"),
        vec![
            (tag(b"Glyp"), utf8("Wide line\u{2029}Mid\u{2029}")),
            (
                tag(b"GAtt"),
                F::Obj(
                    tag(b"GlAS"),
                    vec![(tag(b"Runs"), F::Objs(tag(b"GlAR"), vec![run(14, Some(attrs("Inter", "Inter-Regular", 400, false, 40.0)))]))],
                ),
            ),
            (tag(b"PAtt"), F::Obj(tag(b"PaAS"), vec![(tag(b"Runs"), F::Objs(tag(b"PaAR"), vec![para(10, first), para(14, second)]))])),
        ],
    );
    F::Def(
        50,
        vec![tag(b"TxtA")],
        vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![blk]))])),
            (tag(b"TxtH"), F::Obj(tag(b"ArFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 400.0, 100.0])), (tag(b"ArtV"), F::F64(80.0))])),
            (tag(b"Xfrm"), F::F64s(vec![1.0, 0.0, 100.0, 0.0, 1.0, 100.0])),
        ],
    )
}

/// Frame text of two 100 px lines in a frame `height` tall.
pub fn two_line_frame(height: f64) -> F {
    let blk = block(utf8("One\u{2029}Two\u{2029}"), vec![run(8, Some(auto_leading(attrs("Inter", "Inter-Regular", 400, false, 100.0))))], &[0]);
    F::Def(
        60,
        vec![tag(b"TxtF")],
        vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![blk]))])),
            (tag(b"TxtH"), F::Obj(tag(b"FrFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 500.0, height]))])),
        ],
    )
}

/// A 16 × 16 CMYK pixel layer of pure cyan, opaque or (`transparent`) with no alpha, in a
/// document that embeds `profile` as its CMYK profile.
pub fn cmyk_layer(transparent: bool, profile: Option<&[u8]>) -> Vec<u8> {
    packed(&cmyk_stream(transparent, profile), None, Method::Zstd)
}

/// The object stream of [`cmyk_layer`].
pub fn cmyk_stream(transparent: bool, profile: Option<&[u8]>) -> Vec<u8> {
    let mut bitmap = vec![(tag(b"Frmt"), F::Enum(4, 0)), (tag(b"BmpW"), F::I32(16)), (tag(b"BmpH"), F::I32(16))];
    for c in 1..=5 {
        // Tile state 2: every sample 255; 0: every sample 0.
        let full = c == 1 || (c == 5 && !transparent);
        bitmap.push((tag(&[b'T', b'W', b'i', b'0' + c]), F::I32(1)));
        bitmap.push((tag(&[b'T', b'H', b'i', b'0' + c]), F::I32(1)));
        bitmap.push((tag(&[b'S', b't', b'a', b'0' + c]), F::U8s(vec![if full { 2 } else { 0 }])));
    }
    let layer = F::Def(10, vec![tag(b"Rstr")], vec![(tag(b"Bitm"), F::Obj(tag(b"DyBm"), bitmap)), (tag(b"Desc"), F::Str("Photo".into()))]);
    let spread = F::Def(2, vec![tag(b"Sprd")], vec![(tag(b"SprB"), F::F64s(vec![0.0, 0.0, 16.0, 16.0])), (tag(b"Chld"), F::Shared(vec![layer]))]);
    let mut root = vec![(tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread]))]))];
    if let Some(icc) = profile {
        root.push((tag(b"CMYP"), F::Obj(tag(b"ICCP"), vec![(tag(b"Data"), blob(b"Data", icc))])));
    }
    stream(root)
}

/// A byte blob field, as `LSty` `Data` and `ICCP` `Data` are stored.
pub fn blob(field: &[u8; 4], bytes: &[u8]) -> F {
    let mut b = vec![0x2d];
    b.extend(tag(field).0.to_le_bytes());
    b.extend((bytes.len() as u32).to_le_bytes());
    b.extend_from_slice(bytes);
    F::Raw(b)
}

/// A drop shadow 3 px below, blurred 2 px.
pub fn shadow() -> F {
    F::Obj(
        tag(b"Shad"),
        vec![
            (tag(b"Enab"), F::Bool(true)),
            (tag(b"Opac"), F::F64(0.5)),
            (tag(b"Radi"), F::F64(2.0)),
            (tag(b"Offs"), F::F64(3.0)),
            (tag(b"Angl"), F::F64(std::f64::consts::FRAC_PI_2)),
            (tag(b"Colr"), rgba(0.0, 0.0, 1.0)),
        ],
    )
}

/// An effect of `class` (`OutG`, `InnS`, `BevE` …) with this radius, yellow at 80 %.
pub fn effect(class: &[u8; 4], radius: f64) -> F {
    F::Obj(
        tag(class),
        vec![(tag(b"Enab"), F::Bool(true)), (tag(b"Opac"), F::F64(0.8)), (tag(b"Radi"), F::F64(radius)), (tag(b"Colr"), rgba(1.0, 1.0, 0.0))],
    )
}

/// A red outline effect `width` wide, aligned `align` (0 outside, 1 inside, 2 centred).
pub fn outline(align: u16, width: f64) -> F {
    F::Obj(
        tag(b"Strk"),
        vec![(tag(b"Enab"), F::Bool(true)), (tag(b"Radi"), F::F64(width)), (tag(b"Alig"), F::Enum(align, 0)), (tag(b"Colr"), rgba(1.0, 0.0, 0.0))],
    )
}

/// A green colour overlay at `opacity`.
pub fn colour_overlay(opacity: f64) -> F {
    F::Obj(tag(b"ColO"), vec![(tag(b"Enab"), F::Bool(true)), (tag(b"Opac"), F::F64(opacity)), (tag(b"Colr"), rgba(0.0, 1.0, 0.0))])
}

/// A rectangle with a black stroke 2 px wide in line style `style` (`LSty` `Data` byte 10:
/// 0 none, 1 solid, 2 dashed, 3 brush), dashed when `dash` has lengths.
pub fn stroked(id: u32, style: u8, dash: &[f64]) -> F {
    let mut node = rectangle(id, (0.0, 0.0), (50.0, 50.0));
    let mut data = 10.0f64.to_le_bytes().to_vec();
    data.extend([0, 0, style]);
    let mut lsty = vec![(tag(b"Wght"), F::F64(2.0)), (tag(b"Data"), blob(b"Data", &data))];
    if !dash.is_empty() {
        lsty.push((tag(b"Patn"), array_f64(b"Patn", dash)));
    }
    if let F::Def(_, _, fields) = &mut node {
        fields.push((tag(b"PFil"), cmyk(0.0, 0.0, 0.0, 1.0)));
        fields.push((tag(b"LSty"), F::Obj(tag(b"LSty"), lsty)));
    }
    node
}

/// Character attributes of 40 px Inter with these `Ints` (slot 0 underline, 1 strikethrough).
pub fn decorated(ints: &[i32]) -> F {
    let mut a = attrs("Inter", "Inter-Regular", 400, false, 40.0);
    if let F::Obj(_, fields) = &mut a {
        for (t, v) in fields.iter_mut() {
            if *t == tag(b"Ints") {
                *v = array_i32(b"Ints", ints);
            }
        }
    }
    a
}

/// Artistic text with an underlined run, a struck-through one and a line break inside a
/// paragraph.
pub fn decorated_text(id: u32) -> F {
    let blk = block(
        utf8("Plain under\u{2028}struck\u{2029}"),
        vec![
            run(6, Some(attrs("Inter", "Inter-Regular", 400, false, 40.0))),
            run(12, Some(decorated(&[1, 0, 0, 0, 1]))),
            run(19, Some(decorated(&[0, 1, 0, 0, 1]))),
        ],
        &[0],
    );
    F::Def(
        id,
        vec![tag(b"TxtA")],
        vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![blk]))])),
            (tag(b"TxtH"), F::Obj(tag(b"ArFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 400.0, 100.0])), (tag(b"ArtV"), F::F64(80.0))])),
        ],
    )
}
