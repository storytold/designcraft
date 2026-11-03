// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! External preset fixtures are data only; no scoring exports are read here.
use designcraft_doc::Shape;
use designcraft_geom::PathEl;

#[test]
#[ignore = "needs PUB_CORPUS containing the pinned heart and six existing preset fixtures"]
fn confirmed_preset_fixtures_keep_their_outlines() {
    let folders = std::env::var("PUB_CORPUS").unwrap();
    for (name, shape) in [
        ("heart", Shape::Path),
        ("star5", Shape::Polygon),
        ("diamond", Shape::Polygon),
        ("hexagon", Shape::Polygon),
        ("rightarrow", Shape::Polygon),
        ("triangle", Shape::Polygon),
        ("roundrect", Shape::Rectangle),
    ] {
        let file = folders.split(':').map(|p| std::path::Path::new(p).join(format!("shapes__{name}.pub"))).find(|p| p.is_file()).unwrap();
        let imported = designcraft_pub::import_pub(&std::fs::read(file).unwrap()).unwrap();
        let items: Vec<_> = imported.document.spreads.iter().flat_map(|s| s.items.iter()).collect();
        assert_eq!(items.len(), 1, "{name}");
        assert_eq!(items[0].shape, shape, "{name}");
        assert!(!imported.warnings.iter().any(|w| w.contains("not supported yet") || w.contains("heart outline")), "{name}: {:?}", imported.warnings);
        let path = items[0].path.to_bezpath();
        assert!(matches!(path.elements().last(), Some(PathEl::ClosePath)), "{name}");
        if name == "heart" {
            let [PathEl::MoveTo(top), PathEl::CurveTo(a, b, tip), PathEl::CurveTo(c, d, end), PathEl::ClosePath] = path.elements() else {
                panic!("heart must contain two closed cubic curves");
            };
            assert_eq!(top, end);
            assert_eq!(top.x, tip.x);
            assert!(tip.y > top.y && a.y < top.y);
            assert!((a.y - d.y).abs() < 1e-8);
            assert!((b.y - top.y).abs() < 1e-8 && (c.y - top.y).abs() < 1e-8);
            assert!((a.x + d.x - 2.0 * top.x).abs() < 1e-8);
            assert!((b.x + c.x - 2.0 * top.x).abs() < 1e-8);
            // The two horizontal control offsets have the standard's 49:10 ratio.
            assert!(((b.x - top.x) / (a.x - top.x) - 4.9).abs() < 1e-8);
        }
    }
}
