// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Pinned, external curve/polyline data; scoring exports are not used here.
use designcraft_doc::Shape;
use designcraft_geom::PathEl;

#[test]
#[ignore = "needs PUB_CORPUS containing the pinned curve and polyline .pub samples"]
fn imports_real_freeform_paths() {
    let folders = std::env::var("PUB_CORPUS").unwrap();
    for (name, code, count) in [("shapes__curve.pub", "0x0D", 2), ("shapes__polyline.pub", "0x0C", 5)] {
        let file = folders.split(':').map(|p| std::path::Path::new(p).join(name)).find(|p| p.is_file()).unwrap();
        let imported = designcraft_pub::import_pub(&std::fs::read(file).unwrap()).unwrap();
        let items: Vec<_> = imported.document.spreads.iter().flat_map(|s| s.items.iter()).collect();
        let paths: Vec<_> = items.iter().filter(|i| i.shape == Shape::Path).collect();
        assert_eq!(paths.len(), 1, "{name}: freeform must not become a rectangle");
        let path = paths[0].path.to_bezpath();
        assert_eq!(path.elements().len(), count, "{name}");
        if name == "shapes__curve.pub" {
            let [PathEl::MoveTo(start), PathEl::CurveTo(a, b, end)] = path.elements() else { panic!("expected one cubic") };
            assert!((start.y - end.y).abs() < 1e-8);
            assert!(start.x < a.x && a.x < b.x && b.x < end.x);
            assert!(a.y < start.y && b.y > start.y);
        } else {
            assert!(matches!(path.elements()[0], PathEl::MoveTo(_)));
            assert!(path.elements()[1..4].iter().all(|e| matches!(e, PathEl::LineTo(_))));
            assert!(matches!(path.elements()[4], PathEl::ClosePath));
        }
        let warnings: Vec<_> = imported.warnings.iter().filter(|w| w.contains("Freeform")).collect();
        assert_eq!(warnings.len(), 1, "{name}: {:?}", imported.warnings);
        assert!(warnings[0].contains(code));
        assert!(!imported.warnings.iter().any(|w| w.contains("rectangle")));
    }
}
