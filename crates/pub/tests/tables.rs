// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Optional checks of the three allowed binary table fixtures; synthetic cases run in tables.rs.
#[test]
#[ignore = "needs PUB_CORPUS containing the allowed pubsmith binary fixtures"]
fn observed_table_fixtures_import_as_native_tables() {
    let dirs = std::env::var("PUB_CORPUS").expect("set PUB_CORPUS");
    // The amber trellis 8359 grid check covers ordinary cells and a merged row.
    for (name, rows, merged) in [("table__2x2", 2, false), ("table__3x2", 3, false), ("table__merged", 2, true)] {
        let path = dirs.split(':').map(|d| std::path::Path::new(d).join(format!("{name}.pub"))).find(|p| p.is_file()).unwrap();
        let imp = designcraft_pub::import_pub(&std::fs::read(path).unwrap()).unwrap();
        let tables: Vec<_> = imp
            .document
            .stories
            .values()
            .flat_map(|s| {
                s.check().unwrap();
                s.tables.values()
            })
            .collect();
        assert_eq!(tables.len(), 1, "{name}");
        let t = tables[0];
        assert_eq!((t.nrows(), t.ncols()), (rows, 2), "{name}");
        assert!(t.rows.iter().all(|r| r.height == 36.0));
        assert!(t.columns.iter().all(|c| c.width == 144.0));
        assert_eq!(t.cell(0, 0).unwrap().col_span, if merged { 2 } else { 1 });
        if name == "table__2x2" {
            assert_eq!(t.cells.iter().map(|c| c.text.text.as_str()).collect::<Vec<_>>(), ["A1", "B1", "A2", "B2"]);
        } else {
            assert!(t.cells.iter().all(|c| c.text.text.is_empty()));
        }
        assert!(imp.warnings.iter().any(|w| w.contains("cell fills, borders")));
        assert!(imp.warnings.iter().all(|w| !w.contains("stored endings for row positions")), "{name}");
        assert!((t.rows.iter().map(|r| r.height).sum::<f64>() - rows as f64 * 36.0).abs() < 1e-9);
        for c in &t.cells {
            c.text.check().unwrap();
            assert!(!c.has_fill());
            assert!(c.strokes.iter().all(|s| !s.is_visible()));
        }
    }
}

#[test]
#[ignore = "needs PUB_POI_TABLES containing the eight task-scoped Apache POI table samples"]
fn observed_apache_tables_use_endings_and_match_measured_drawn_height() {
    use std::io::Read;
    let root = std::env::var("PUB_POI_TABLES").expect("set PUB_POI_TABLES");
    for name in
        ["Sample.pub", "Sample2.pub", "Sample3.pub", "Sample4.pub", "Sample2_2010.pub", "Sample3_2010.pub", "Sample4_2010.pub", "Sample_2010.pub"]
    {
        let mut bytes = Vec::new();
        std::fs::File::open(std::path::Path::new(&root).join(name)).unwrap().take(512 * 1024 * 1024 + 1).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 512 * 1024 * 1024);
        let imp = designcraft_pub::import_pub(&bytes).unwrap();
        let tables: Vec<_> = imp.document.stories.values().flat_map(|s| s.tables.values()).collect();
        assert_eq!(tables.len(), 1, "{name}");
        let t = tables[0];
        assert_eq!((t.nrows(), t.ncols()), (3, 2));
        assert!(t.rows.iter().all(|r| (r.height - 219_188.0 / 12_700.0).abs() < 1e-9));
        assert!((t.rows.iter().map(|r| r.height).sum::<f64>() - 657_564.0 / 12_700.0).abs() < 1e-9, "{name}");
        let warnings: Vec<_> = imp.warnings.iter().filter(|w| w.contains("stored endings for row positions")).collect();
        assert_eq!(warnings.len(), 1, "{name}");
        assert!(warnings[0].contains("9564 EMU; cause unknown"));
    }
}
