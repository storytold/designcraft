// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Binary fixtures only: scoring exports are not read by these checks.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use designcraft_doc::{Content, Shape};
use designcraft_geom::{Point, warp::PathWarp};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn check(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}

#[test]
#[ignore = "needs PUB_CORPUS containing the pinned WordArt fixtures"]
fn wordart_paths_keep_original_editable_text_and_straight_cases() -> Result<()> {
    let folders = std::env::var("PUB_CORPUS")?;
    for (suffix, curved) in
        [("arch", true), ("circle", false), ("base", false), ("fill-red", false), ("line-white", false), ("shadow", false), ("font", false)]
    {
        let name = format!("wordart__{suffix}.pub");
        let file = folders.split(':').map(|p| std::path::Path::new(p).join(&name)).find(|p| p.is_file()).ok_or("missing fixture")?;
        let mut imported = designcraft_pub::import_pub(&std::fs::read(file)?)?;
        let items: Vec<_> = imported.document.spreads.iter().flat_map(|s| &s.items).collect();
        check(items.len() == 1, "expected one WordArt item")?;
        let item = items.first().ok_or("no item")?;
        check(item.shape == if curved { Shape::Path } else { Shape::Rectangle }, "wrong shape")?;
        let Content::Text(tf) = &item.content else { return Err("text was not retained".into()) };
        check(tf.options.path.is_some() == curved, "wrong path option")?;
        let story_id = tf.story;
        let story = imported.document.story(story_id).ok_or("no story")?;
        check(story.text == "Sample Text!", "original text changed")?;
        let (_, format) = story.runs().next().ok_or("no text style")?;
        let expected_font = if suffix == "font" { "Times New Roman" } else { "Arial Black" };
        check(
            serde_json::to_value(&format.over)?.get("fontFamily").and_then(serde_json::Value::as_str) == Some(expected_font),
            "original font name changed",
        )?;
        if curved {
            check(item.fill.is_none() && item.stroke.is_none(), "frame paints changed")?;
            check(!item.path.subpaths.iter().any(|p| p.closed), "baseline path must be open")?;
            let warp = PathWarp::new(&item.path.to_bezpath(), false);
            let start = warp.at(0.0).ok_or("no baseline start")?.0;
            check((start - Point::new(72.0, 97.5)).hypot() < 0.002, "wrong baseline start")?;
            let half = warp.at(warp.length() / 2.0).ok_or("no midpoint")?.0;
            let expected = if suffix == "arch" { Point::new(202.125, 72.0) } else { Point::new(332.25, 97.5) };
            check((half - expected).hypot() < 0.002, "wrong baseline midpoint")?;
            check(imported.warnings.iter().any(|w| w.contains("measured font advances")), "missing approximation warning")?;
            check(!imported.warnings.iter().any(|w| w.contains("as straight text")), "unexpected fallback")?;
            let composed = designcraft_compose::compose_story(&imported.document, story_id, &Default::default());
            check(!composed.is_overset(), "fitted text is overset")?;
            let frame = composed.frame(item.id).ok_or("no composed frame")?;
            check(frame.lines.len() == 1, "fitted text broke into lines")?;
            let line = frame.lines.first().ok_or("no composed line")?;
            let advance: f64 = line.glyphs.iter().map(|g| g.adv).sum();
            let faces: std::collections::BTreeSet<_> = line.glyphs.iter().map(|g| (g.face.family.clone(), g.face.style.clone())).collect();
            println!("  resolved faces: {faces:?}");
            check((advance - warp.length()).abs() < 0.01, "actual advances do not fill the path")?;
            check(format.over.h_scale.is_none(), "fixed horizontal scale was added")?;
            println!(
                "  measured advances={advance:.9}, path={:.9}, size={:?}, baseline shift={:?}",
                warp.length(),
                format.over.size,
                format.over.baseline_shift
            );
        }
        if suffix == "circle" {
            check(imported.warnings.iter().any(|w| w.contains("as straight text")), "circle fallback warning missing")?;
        }
        // Editing still targets a normal story rather than a baked graphic.
        let story = imported.document.story_mut(story_id).ok_or("no editable story")?;
        story.insert(story.text.len(), " More");
        check(story.text == "Sample Text! More", "story edit failed")?;
        println!("{name}: editable, path={curved}, font={expected_font}");
    }
    Ok(())
}
