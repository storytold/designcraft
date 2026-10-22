//! Crafted documents can't make the reader allocate or work without bound: tables, shared
//! stories, paragraphs and embedded documents stay within the whole read's budget, however often
//! a file links or embeds the same content. Original synthetic documents in the reader's
//! accepted schema; no Affinity software or files were used.
#![cfg(feature = "synth")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support {
    pub mod text_fixture;
}

use designcraft_affinity::{
    Document, Error, Limits,
    model::{Kind, Node},
    read,
    synth::{self, F, Method, tag},
};
use support::text_fixture::*;

type Field = (designcraft_affinity::stream::Tag, F);

/// A document of one spread holding `nodes`, plus these other archive entries.
fn file(nodes: Vec<F>, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let spread = F::Obj(tag(b"Sprd"), vec![(tag(b"SprB"), F::F64s(vec![0.0, 0.0, 400.0, 300.0])), (tag(b"Chld"), F::Shared(nodes))]);
    let mut root = vec![(tag(b"DocR"), F::Obj(tag(b"DocN"), vec![(tag(b"Chld"), F::Shared(vec![spread]))]))];
    let mut next_id = 1000;
    for (_, value) in &mut root {
        shared_objects(value, &mut next_id);
    }
    let data = synth::stream(&root);
    let mut all = vec![("doc.dat", data.as_slice(), Method::Zlib)];
    all.extend(entries.iter().map(|(name, bytes)| (*name, *bytes, Method::Stored)));
    synth::container(&all, None)
}

fn open(nodes: Vec<F>) -> Document {
    read(&file(nodes, &[]), Limits::default()).unwrap()
}

fn all(nodes: &[Node]) -> Vec<&Node> {
    let mut out = Vec::new();
    for n in nodes {
        out.push(n);
        out.extend(all(&n.children));
    }
    out
}

fn warned(d: &Document, needle: &str) -> bool {
    d.warnings.iter().any(|w| w.contains(needle))
}

fn story(text: &str) -> F {
    F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![block(utf8(text), vec![run(text.chars().count() as i32, None)], &[])]))])
}

fn artistic(story: F) -> F {
    F::Obj(
        tag(b"TxtA"),
        vec![
            (tag(b"StSt"), story),
            (tag(b"TxtH"), F::Obj(tag(b"ArFr"), vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, 100.0, 20.0])), (tag(b"ArtV"), F::F64(16.0))])),
        ],
    )
}

/// A table frame (`TbFr`) with a grid of `cols` × `rows` cells and no cell objects or lines.
fn grid(cols: usize, rows: usize) -> Vec<Field> {
    let pos = |n: usize| {
        let v: Vec<f64> = (0..=n).map(|i| i as f64).collect();
        F::Obj(tag(b"TPos"), vec![(tag(b"Posn"), array_f64(b"Posn", &v))])
    };
    let table = F::Obj(tag(b"Tabl"), vec![(tag(b"CPos"), pos(cols)), (tag(b"RPos"), pos(rows))]);
    vec![(tag(b"FrmB"), F::F64s(vec![0.0, 0.0, cols as f64, rows as f64])), (tag(b"Tabl"), table)]
}

fn table(frame: F) -> F {
    F::Obj(tag(b"TxtT"), vec![(tag(b"StSt"), story("A")), (tag(b"TxtH"), frame)])
}

fn tables(d: &Document) -> Vec<&designcraft_affinity::model::Table> {
    all(&d.spreads[0].nodes).into_iter().filter_map(|n| if let Kind::Table(t) = &n.kind { Some(t) } else { None }).collect()
}

#[test]
fn a_table_of_more_than_65536_cells_is_left_out_before_its_cells_are_made() {
    // 4096 × 4096 is within each dimension's limit but 16.7 million cells.
    let d = open(vec![table(F::Obj(tag(b"TbFr"), grid(4096, 4096))), table(F::Obj(tag(b"TbFr"), grid(256, 256)))]);
    let t = tables(&d);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].cells.len(), t[0].vertical.len(), t[0].horizontal.len()), (65536, 0, 0), "no lines when they don't match");
    assert!(warned(&d, "tables with more than 65536 cells"), "{:?}", d.warnings);
}

#[test]
fn tables_sharing_one_grid_stay_within_the_cells_of_one_document() {
    // One 65536-cell grid linked from five tables: four fill the document's 262144 cells.
    let shared = F::Def(2_000_000, vec![tag(b"TbFr")], grid(256, 256));
    let d = open((0..5).map(|i| table(if i == 0 { shared.clone() } else { F::Ref(2_000_000) })).collect());
    assert_eq!(tables(&d).len(), 4);
    assert!(warned(&d, "tables beyond the cells one document may hold"), "{:?}", d.warnings);
}

#[test]
fn a_shared_story_is_read_within_the_characters_of_one_document() {
    // A story of a million characters linked from twenty text layers: 16 million in all.
    let text = "x".repeat(1 << 20);
    let shared = F::Def(2_000_000, vec![tag(b"Stry")], vec![(tag(b"Blok"), F::Shared(vec![block(utf8(&text), vec![run(1 << 20, None)], &[])]))]);
    let d = open((0..20).map(|i| artistic(if i == 0 { shared.clone() } else { F::Ref(2_000_000) })).collect());
    let chars: usize = all(&d.spreads[0].nodes)
        .into_iter()
        .filter_map(|n| if let Kind::Text(t) = &n.kind { Some(t.runs.iter().map(|r| r.text.len()).sum::<usize>()) } else { None })
        .sum();
    assert_eq!(chars, 16 << 20);
    assert!(warned(&d, "text beyond the characters one document may hold"), "{:?}", d.warnings);
}

#[test]
fn many_paragraphs_and_runs_are_matched_in_about_linear_time() {
    // 200 000 paragraphs, each with its own run and format.
    let n = 200_000;
    let text = "a\u{2029}".repeat(n);
    let fmt =
        |end: usize| vec![(tag(b"Indx"), F::I32(end as i32)), (tag(b"Item"), F::Obj(tag(b"PAtt"), vec![(tag(b"Ints"), array_i32(b"Ints", &[1]))]))];
    let blk = F::Obj(
        tag(b"StBl"),
        vec![
            (tag(b"Glyp"), utf8(&text)),
            (tag(b"GAtt"), F::Obj(tag(b"GlAS"), vec![(tag(b"Runs"), F::Objs(tag(b"GlAR"), (1..=n).map(|i| run(2 * i as i32, None)).collect()))])),
            (tag(b"PAtt"), F::Obj(tag(b"PaAS"), vec![(tag(b"Runs"), F::Objs(tag(b"PaAR"), (1..=n).map(|i| fmt(2 * i)).collect()))])),
        ],
    );
    let started = std::time::Instant::now();
    let d = open(vec![artistic(F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(vec![blk]))]))]);
    let Kind::Text(t) = &d.spreads[0].nodes[0].kind else { panic!("text") };
    assert_eq!(t.runs.len(), n);
    // (About half a second; quadratic matching took over half a minute on the same machine.)
    assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
}

/// A shape node of `size` px.
fn rect(size: f64) -> F {
    F::Obj(tag(b"ShpN"), vec![(tag(b"ShpB"), F::F64s(vec![0.0, 0.0, size, size])), (tag(b"Shpe"), F::Obj(tag(b"ShNR"), vec![]))])
}

/// An instance of the embedded document in archive entry `edc/1`, with `children` of its own.
fn embedded(children: Vec<F>) -> F {
    let content = F::Obj(tag(b"EmbC"), vec![(tag(b"EmbC"), F::Entry("edc/1".into()))]);
    let mut fields = vec![(tag(b"Bitm"), F::Obj(tag(b"EmbR"), vec![(tag(b"EmCn"), content)]))];
    if !children.is_empty() {
        fields.push((tag(b"Chld"), F::Shared(children)));
    }
    F::Obj(tag(b"EmbN"), fields)
}

fn embedding(inner: &[u8], nodes: Vec<F>) -> Vec<u8> {
    let mut entry = b"EmDc\0\0\0\0".to_vec();
    entry.extend_from_slice(inner);
    file(nodes, &[("edc/1", &entry)])
}

#[test]
fn an_embedded_documents_content_stays_with_its_node() {
    // Read before the node's own children, which must not take it.
    let inner = file(vec![rect(10.0), rect(20.0)], &[]);
    let d = read(&embedding(&inner, vec![embedded(vec![rect(5.0)])]), Limits::default()).unwrap();
    let node = &d.spreads[0].nodes[0];
    assert_eq!(node.children.len(), 3, "its own child and the embedded document's two shapes");
    assert!(node.children.iter().all(|c| c.children.is_empty()));
}

#[test]
fn embedding_one_document_many_times_shares_one_budget() {
    // 2000 shapes, embedded 300 times and read twice each: 1.2 million nodes, past the 500 000
    // a whole read may take (each embedding used to have an allowance of its own).
    let inner = file((0..2000).map(|_| rect(10.0)).collect(), &[]);
    let outer = embedding(&inner, (0..300).map(|_| embedded(Vec::new())).collect());
    assert!(matches!(read(&outer, Limits::default()), Err(Error::Limit(_))));
    // Embedded fewer times, it opens.
    let d = read(&embedding(&inner, (0..50).map(|_| embedded(Vec::new())).collect()), Limits::default()).unwrap();
    assert_eq!(all(&d.spreads[0].nodes).len(), 50 + 50 * 2000);
}
