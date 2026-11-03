// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! The bounded readers on a publication generated from scratch (`tests/data/hello.pub`, written
//! by `support::hello`), plus hostile input.

mod support;

use std::io::{Cursor, Read, Write};

use designcraft_pub::{PubError, is_pub, read_publication};

const HELLO: &[u8] = include_bytes!("data/hello.pub");

/// Regenerates `tests/data/hello.pub` from `support::hello`.
#[test]
#[ignore = "writes tests/data/hello.pub"]
fn write_hello_sample() {
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/hello.pub"), support::hello()).unwrap();
}

#[test]
fn the_committed_sample_is_exactly_the_generated_one() {
    assert_eq!(support::hello(), support::hello(), "the generator is deterministic");
    assert!(HELLO == support::hello().as_slice(), "run `cargo test -p designcraft-pub --test read -- --ignored write_hello_sample`");
}

/// The sample with its `Contents` stream replaced by `f(contents)`.
fn with_contents(f: impl FnOnce(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(HELLO.to_vec())).unwrap();
    let mut contents = Vec::new();
    cf.open_stream("/Contents").unwrap().read_to_end(&mut contents).unwrap();
    let new = f(contents);
    let mut s = cf.create_stream("/Contents").unwrap();
    s.write_all(&new).unwrap();
    drop(s);
    cf.flush().unwrap();
    cf.into_inner().into_inner()
}

#[test]
fn reads_pages_story_and_drawing_records() {
    assert!(is_pub(HELLO));
    let p = read_publication(HELLO).unwrap();
    assert_eq!(p.warnings(), Vec::<String>::new());
    assert_eq!(p.page_size(), (612.0, 792.0));
    assert_eq!((p.page_count(), p.master_count()), (1, 0));
    assert_eq!(p.story_ids(), [2]);
    assert_eq!(p.story_text(2).as_deref(), Some(support::HELLO_TEXT));
    assert_eq!((p.shape_count(), p.picture_count()), (2, 0));
}

#[test]
fn rejects_files_that_are_not_publications() {
    assert!(matches!(read_publication(b""), Err(PubError::NotPub(_))));
    assert!(matches!(read_publication(b"%PDF-1.7\n"), Err(PubError::NotPub(_))));
    assert!(!is_pub(b"PK\x03\x04"));
    let other = with_contents(|_| b"\xd0\xcf\x11\xe0 not publisher".to_vec());
    assert!(!is_pub(&other));
    assert!(matches!(read_publication(&other), Err(PubError::NotPub(_))));
}

#[test]
fn refuses_publisher_2000_and_older_with_a_message() {
    let old = with_contents(|mut c| {
        c[2] = 0x22;
        c
    });
    let Err(PubError::Unsupported(msg)) = read_publication(&old) else { panic!("expected Unsupported") };
    assert!(msg.contains("Publisher 97/98/2000"), "{msg}");
}

#[test]
fn truncated_or_corrupt_input_is_an_error_not_a_panic() {
    for n in [8, 512, 1024, 2048, HELLO.len() / 2, HELLO.len() - 1] {
        let _ = read_publication(&HELLO[..n]);
    }
    let short = with_contents(|c| c[..30.min(c.len())].to_vec());
    assert!(read_publication(&short).is_err());
    let mut seed = 0x2545_f491_u32;
    for _ in 0..3000 {
        let mut b = HELLO.to_vec();
        for _ in 0..4 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = seed as usize % b.len();
            b[at] = (seed >> 24) as u8;
        }
        let _ = read_publication(&b);
    }
}
