//! Original synthetic reader fixtures, shared by native and engine regression tests.
//! These describe the reader's existing schema; no Affinity software or third-party parser was
//! used to create them. They prove recovery/mapping behavior, not Affinity's layout or exporter.
#![allow(dead_code)]

use designcraft_affinity::synth::{self, F, Method, tag};

pub fn array_f64(field: &[u8; 4], values: &[f64]) -> F {
    let mut bytes = vec![0x8a];
    bytes.extend(tag(field).0.to_le_bytes());
    bytes.extend((values.len() as u32).to_le_bytes());
    bytes.extend(values.iter().flat_map(|v| v.to_le_bytes()));
    F::Raw(bytes)
}

pub fn array_i32(field: &[u8; 4], values: &[i32]) -> F {
    let mut bytes = vec![0x87];
    bytes.extend(tag(field).0.to_le_bytes());
    bytes.extend((values.len() as u32).to_le_bytes());
    bytes.extend(values.iter().flat_map(|v| v.to_le_bytes()));
    F::Raw(bytes)
}

pub fn solid(alpha: f32) -> F {
    F::Obj(
        tag(b"FilS"),
        vec![(
            tag(b"Colr"),
            F::Obj(tag(b"RGBA"), vec![(tag(b"_col"), F::Struct([0.2f32, 0.4, 0.6, alpha].iter().flat_map(|v| v.to_le_bytes()).collect()))]),
        )],
    )
}

pub fn attrs(family: &str, postscript: &str, weight: i32, italic: bool, size: f64) -> F {
    let mut doubles = vec![0.0; 14];
    doubles[0] = size;
    doubles[4] = 0.025;
    doubles[10] = 1.0;
    doubles[13] = 30.0;
    F::Obj(
        tag(b"GAtt"),
        vec![
            (
                tag(b"DFnt"),
                F::Obj(
                    tag(b"Font"),
                    vec![
                        (tag(b"Famy"), F::Str(family.into())),
                        (tag(b"Post"), F::Str(postscript.into())),
                        (tag(b"Wegt"), F::I32(weight)),
                        (tag(b"Ital"), F::Bool(italic)),
                    ],
                ),
            ),
            (tag(b"Doub"), array_f64(b"Doub", &doubles)),
            (tag(b"Ints"), array_i32(b"Ints", &[0, 0, 0, 0, 1])),
            (tag(b"Objs"), F::Shared(vec![solid(1.0)])),
        ],
    )
}

pub fn run(end: i32, attrs: Option<F>) -> Vec<(designcraft_affinity::stream::Tag, F)> {
    let mut fields = vec![(tag(b"Indx"), F::I32(end))];
    if let Some(attrs) = attrs {
        fields.push((tag(b"Item"), attrs));
    }
    fields
}

pub fn block(glyphs: F, runs: Vec<Vec<(designcraft_affinity::stream::Tag, F)>>, alignments: &[i32]) -> F {
    F::Obj(
        tag(b"StBl"),
        vec![
            (tag(b"Glyp"), glyphs),
            (tag(b"GAtt"), F::Obj(tag(b"GlAS"), vec![(tag(b"Runs"), F::Objs(tag(b"GlAR"), runs))])),
            (
                tag(b"PAtt"),
                F::Obj(
                    tag(b"PaAS"),
                    vec![(
                        tag(b"Runs"),
                        F::Objs(
                            tag(b"PaAR"),
                            alignments
                                .iter()
                                .map(|a| vec![(tag(b"Item"), F::Obj(tag(b"PAtt"), vec![(tag(b"Ints"), array_i32(b"Ints", &[*a]))]))])
                                .collect(),
                        ),
                    )],
                ),
            ),
        ],
    )
}

pub fn utf8(text: &str) -> F {
    F::Obj(tag(b"GStr"), vec![(tag(b"Utf8"), F::Str(text.into()))])
}

pub fn document(blocks: Vec<F>, frame: bool, dpi: f64, transform: [f64; 6]) -> Vec<u8> {
    let text = F::Obj(
        tag(if frame { b"TxtF" } else { b"TxtA" }),
        vec![
            (tag(b"StSt"), F::Obj(tag(b"Stry"), vec![(tag(b"Blok"), F::Shared(blocks))])),
            (
                tag(b"TxtH"),
                F::Obj(
                    tag(if frame { b"FrFr" } else { b"ArFr" }),
                    vec![(tag(b"FrmB"), F::F64s(vec![10.0, 20.0, 210.0, 120.0])), (tag(b"ArtV"), F::F64(18.0))],
                ),
            ),
            (tag(b"Xfrm"), F::F64s(transform.to_vec())),
        ],
    );
    let mut root = vec![
        (tag(b"UVCn"), F::Obj(tag(b"UVCn"), vec![(tag(b"UPPI"), F::F64(dpi))])),
        (
            tag(b"DocR"),
            F::Obj(
                tag(b"DocN"),
                vec![(
                    tag(b"Chld"),
                    F::Shared(vec![F::Obj(
                        tag(b"Sprd"),
                        vec![(tag(b"SprB"), F::F64s(vec![0.0, 0.0, 400.0, 300.0])), (tag(b"Chld"), F::Shared(vec![text]))],
                    )]),
                )],
            ),
        ),
    ];
    let mut next_id = 1000;
    for (_, value) in &mut root {
        shared_objects(value, &mut next_id);
    }
    let stream = synth::stream(&root);
    synth::container(&[("doc.dat", &stream, Method::Zlib)], None)
}

pub const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

// Shared arrays require definitions/references, whereas inline fields use Obj. Give the original
// fixture objects independent shared IDs without making the fixture descriptions noisy.
pub fn shared_objects(value: &mut F, next_id: &mut u32) {
    match value {
        F::Shared(items) => {
            for item in items {
                if let F::Obj(class, fields) = item {
                    let class = *class;
                    let fields = std::mem::take(fields);
                    *next_id += 1;
                    *item = F::Def(*next_id, vec![class], fields);
                }
                shared_objects(item, next_id);
            }
        }
        F::Obj(_, fields) | F::Def(_, _, fields) => {
            for (_, value) in fields {
                shared_objects(value, next_id);
            }
        }
        F::Objs(_, items) => {
            for fields in items {
                for (_, value) in fields {
                    shared_objects(value, next_id);
                }
            }
        }
        _ => {}
    }
}
