//! View › Rotate Spread lives in the `Spread` element's `ItemTransform`.

use std::sync::Arc;

use super::*;

fn fixture_with_spread_transform(xf: &str) -> Document {
    let spread = SPREAD.replacen(
        r#"<Spread Self="sp1" PageCount="2" ItemTransform="1 0 0 1 0 0">"#,
        &format!(r#"<Spread Self="sp1" PageCount="2" ItemTransform="{xf}">"#),
        1,
    );
    assert!(spread.contains(&format!(r#"ItemTransform="{xf}">"#)));
    import_idml(&zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", STORY),
    ]))
    .unwrap()
}

fn rotations(d: &Document) -> Vec<Vec<u8>> {
    d.spreads.iter().map(|s| s.pages.iter().map(|p| p.view_rotation).collect()).collect()
}

fn assert_same_geometry(a: &Document, b: &Document) {
    assert_eq!(a.spreads.len(), b.spreads.len());
    for (sa, sb) in a.spreads.iter().zip(&b.spreads) {
        assert_eq!(sa.pages.len(), sb.pages.len());
        for (pa, pb) in sa.pages.iter().zip(&sb.pages) {
            assert_eq!((pa.x, pa.width, pa.height, pa.side), (pb.x, pb.width, pb.height, pb.side));
        }
        assert_eq!(sa.items.len(), sb.items.len());
        for (ia, ib) in sa.items.iter().zip(&sb.items) {
            assert_eq!(ia.xf, ib.xf);
            assert_eq!(ia.bounds(), ib.bounds());
        }
    }
}

#[test]
fn spread_transform_rotation_imports_as_view_rotation_without_moving_geometry() {
    let plain = fixture_with_spread_transform("1 0 0 1 0 0");
    assert_eq!(rotations(&plain), vec![vec![0, 0]]);
    for (xf, turns) in [("0 1 -1 0 0 350", 1), ("-1 0 -0 -1 0 0", 2), ("0 -1 1 0 0 0", 3), ("0.0000001 1 -1 0 0 0", 1)] {
        let d = fixture_with_spread_transform(xf);
        assert_eq!(rotations(&d), vec![vec![turns, 0]], "{xf}");
        assert_same_geometry(&plain, &d);
    }
    for xf in ["2 0 0 2 0 0", "0.7071 0.7071 -0.7071 0.7071 0 0", "0 1 -1 0 0", "0 1 x -1 0 0 0", "NaN 1 -1 0 0 0", "inf 0 0 -inf 0 0", ""] {
        let d = fixture_with_spread_transform(xf);
        assert_eq!(rotations(&d), vec![vec![0, 0]], "{xf}");
        assert_same_geometry(&plain, &d);
    }
}

#[test]
fn view_rotation_round_trips() {
    for r in 0..4u8 {
        let mut d = small_doc();
        let n = d.spreads.len() as u8;
        for (i, s) in d.spreads.iter_mut().enumerate() {
            Arc::make_mut(s).pages[0].view_rotation = (r + i as u8) % 4;
        }
        let back = import_idml(&export_idml(&d)).unwrap();
        assert_eq!(rotations(&back), rotations(&d), "rotation {r} over {n} spreads");
        assert!(back.parents.iter().all(|s| s.pages.iter().all(|p| p.view_rotation == 0)));
        let mut unrotated = d.clone();
        unrotated.spreads.iter_mut().for_each(|s| Arc::make_mut(s).pages[0].view_rotation = 0);
        assert_same_geometry(&import_idml(&export_idml(&unrotated)).unwrap(), &back);
    }
}
