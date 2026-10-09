//! Original synthetic graph tests for bounded `.af` outline expansion. These model the reader's
//! accepted schema, without using Affinity software, third-party parser code or external assets.
#![cfg(feature = "synth")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use designcraft_affinity::{
    Document, Limits,
    model::{Kind, Path, Point},
    synth::{self, F, Method, tag},
};

fn compound(id: u32, children: Vec<F>) -> F {
    F::Def(id, vec![tag(b"Comp")], vec![(tag(b"Chld"), F::Shared(children))])
}

fn shape(id: u32, class: &[u8; 4], parameters: Vec<(designcraft_affinity::stream::Tag, F)>) -> F {
    F::Def(
        id,
        vec![tag(b"ShpN")],
        vec![(tag(b"ShpB"), F::F64s(vec![0.0, 0.0, 10.0, 20.0])), (tag(b"Shpe"), F::Def(id + 10_000, vec![tag(class)], parameters))],
    )
}

fn read(definitions: Vec<F>, top: u32) -> Document {
    let data = synth::stream(&[
        (tag(b"Defs"), F::Shared(definitions)),
        (
            tag(b"DocR"),
            F::Def(
                1,
                vec![tag(b"DocN")],
                vec![(
                    tag(b"Chld"),
                    F::Shared(vec![F::Def(
                        2,
                        vec![tag(b"Sprd")],
                        vec![(tag(b"SprB"), F::F64s(vec![0.0, 0.0, 400.0, 300.0])), (tag(b"Chld"), F::Shared(vec![F::Ref(top)]))],
                    )]),
                )],
            ),
        ),
    ]);
    let bytes = synth::container(&[("doc.dat", &data, Method::Zlib)], None);
    designcraft_affinity::read(&bytes, Limits::default()).unwrap()
}

fn path(document: &Document) -> &Path {
    match &document.spreads[0].nodes[0].kind {
        Kind::Shape { path, .. } => path,
        other => panic!("expected shape, got {other:?}"),
    }
}

fn rejected(document: &Document, warning: &str) {
    assert!(matches!(document.spreads[0].nodes[0].kind, Kind::Unsupported));
    assert!(document.warnings.iter().any(|value| value.contains(warning)), "{:?}", document.warnings);
}

#[test]
fn self_cycle_and_mutual_cycle_discard_the_entire_compound() {
    // A valid first operand ensures a malformed later operand cannot leave a plausible partial path.
    let rectangle = shape(100, b"ShNR", vec![]);
    let self_cycle = compound(101, vec![F::Ref(100), F::Ref(101)]);
    rejected(&read(vec![rectangle.clone(), self_cycle], 101), "compound outline that contains itself");
    let mutual_cycle = compound(101, vec![F::Ref(100), compound(102, vec![F::Ref(101)])]);
    rejected(&read(vec![rectangle, mutual_cycle], 101), "compound outline that contains itself");
}

#[test]
fn deep_reference_chain_is_bounded_independently_of_stream_nesting() {
    let mut definitions = vec![shape(100, b"ShNR", vec![])];
    for id in 101..=250 {
        definitions.push(compound(id, vec![F::Ref(id - 1)]));
    }
    rejected(&read(definitions, 250), "compound outlines nested deeper than 128 levels");
}

#[test]
fn outline_at_the_depth_limit_is_still_imported() {
    let mut definitions = vec![shape(100, b"ShNR", vec![])];
    for id in 101..=227 {
        definitions.push(compound(id, vec![F::Ref(id - 1)]));
    }
    let document = read(definitions, 227);
    assert!(document.warnings.is_empty(), "{:?}", document.warnings);
    assert_eq!(path(&document).subpaths.len(), 1);
    assert_eq!(path(&document).subpaths[0].segments.len(), 3);
}

#[test]
fn excessive_declared_curve_count_is_rejected_before_expansion() {
    let curve = F::Def(
        100,
        vec![tag(b"PCrv")],
        vec![(tag(b"Crvs"), F::Def(101, vec![tag(b"PCvD")], vec![(tag(b"Data"), F::Pos(vec![F::U8(0), F::U32(u32::MAX)]))]))],
    );
    rejected(&read(vec![curve], 100), "compound outlines exceeding the geometry work limit");
}

#[test]
fn empty_shared_fanout_cannot_bypass_the_operand_visit_limit() {
    // Only 21 on-disk objects, but an unbounded traversal would visit over two million operands.
    let mut definitions = vec![compound(100, vec![])];
    for id in 101..=120 {
        definitions.push(compound(id, vec![F::Ref(id - 1), F::Ref(id - 1)]));
    }
    rejected(&read(definitions, 120), "compound outlines exceeding the operand visit limit");
}

#[test]
fn shared_large_operands_are_charged_for_cumulative_geometry_work() {
    // A small reference graph expands a 20,000-segment star. Count work across every reuse and
    // transform, rather than permitting each individual operand's allocation limit repeatedly.
    let mut definitions = vec![shape(100, b"ShSt", vec![(tag(b"Pnts"), F::I32(10_000))])];
    for id in 101..=112 {
        definitions.push(compound(id, vec![F::Ref(id - 1), F::Ref(id - 1)]));
    }
    rejected(&read(definitions, 112), "compound outlines exceeding the geometry work limit");
}

#[test]
fn ordinary_nested_compound_preserves_repeated_operands_and_transforms() {
    let mut rectangle = shape(100, b"ShNR", vec![]);
    if let F::Def(_, _, fields) = &mut rectangle {
        fields.push((tag(b"Xfrm"), F::F64s(vec![1.0, 0.0, 3.0, 0.0, 1.0, 7.0])));
    }
    let inner = compound(101, vec![F::Ref(100), F::Ref(100)]);
    let mut outer = compound(102, vec![F::Ref(101)]);
    if let F::Def(_, _, fields) = &mut outer {
        fields.push((tag(b"Xfrm"), F::F64s(vec![1.0, 0.0, 100.0, 0.0, 1.0, 200.0])));
    }
    let document = read(vec![rectangle, inner, outer], 102);
    assert!(document.warnings.is_empty(), "{:?}", document.warnings);
    let path = path(&document);
    assert_eq!(path.subpaths.len(), 2);
    assert_eq!(path.subpaths[0], path.subpaths[1]);
    assert_eq!(path.subpaths[0].start, Point { x: 103.0, y: 207.0 });
    assert_eq!(path.subpaths[0].segments.last().unwrap()[2], Point { x: 103.0, y: 227.0 });
}

#[test]
fn overflowing_compound_transform_is_reported_before_processing_geometry() {
    let mut child = shape(100, b"ShNR", vec![]);
    if let F::Def(_, _, fields) = &mut child {
        fields.push((tag(b"Xfrm"), F::F64s(vec![1.0e308, 0.0, 0.0, 0.0, 1.0, 0.0])));
    }
    let mut outer = compound(101, vec![F::Ref(100)]);
    if let F::Def(_, _, fields) = &mut outer {
        fields.push((tag(b"Xfrm"), F::F64s(vec![1.0e308, 0.0, 0.0, 0.0, 1.0, 0.0])));
    }
    rejected(&read(vec![child, outer], 101), "compound operand with an invalid transform");
}

#[test]
fn pie_angles_with_huge_finite_values_do_not_loop_or_emit_nonfinite_points() {
    let document = read(vec![shape(100, b"ShPi", vec![(tag(b"AngS"), F::F64(1.0e308)), (tag(b"AngE"), F::F64(-1.0e308))])], 100);
    for subpath in &path(&document).subpaths {
        assert!(subpath.start.x.is_finite() && subpath.start.y.is_finite());
        assert!(subpath.segments.len() <= 20);
        assert!(subpath.segments.iter().flatten().all(|point| point.x.is_finite() && point.y.is_finite()));
    }
}

#[test]
fn wrapped_pie_angles_keep_the_same_geometry_as_the_positive_span() {
    let make = |end| read(vec![shape(100, b"ShPi", vec![(tag(b"AngS"), F::F64(0.0)), (tag(b"AngE"), F::F64(end))])], 100);
    assert_eq!(path(&make(-std::f64::consts::FRAC_PI_2)), path(&make(3.0 * std::f64::consts::FRAC_PI_2)));
}
