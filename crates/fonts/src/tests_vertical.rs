//! Vertical metrics (`vmtx`, `vhea`, `VORG`), the ideographic em box and top-to-bottom shaping.
//!
//! The synthetic font (Source Sans 3 with `vhea`/`vmtx`, and `VORG` where asked) runs everywhere;
//! the checks against Shippori Mincho's own tables run when built with craft-fonts.

use skrifa::raw::TableProvider;

use super::*;
use crate::testing::{font_mapping, font_with, with_base, with_table_len, with_table_u16, with_vmtx, with_vorg};
use crate::{ShapeContext, feature, shape_vertical, shape_with_context};

/// Text in Japanese.
fn japanese() -> ShapeContext<'static> {
    ShapeContext { language: Some("ja"), ..Default::default() }
}

/// The synthetic font's characters and the Source Sans 3 glyphs that draw them.
const GLYPHS: &[(char, char)] = &[('一', 'X'), ('ヸ', 'O'), ('、', ',')];

/// A font with vertical metrics: every glyph one em down the line with a top side bearing of 120,
/// except ヸ (drawn as O): 1.024 em, top side bearing 100.
fn vertical_font() -> Vec<u8> {
    let base = font_mapping("Vertical", GLYPHS).unwrap();
    let wi = gid_in(&base, 'ヸ');
    with_vmtx(&base, (1000, 120), &[(wi, 1024, 100)]).unwrap()
}

fn gid_in(font: &[u8], c: char) -> u16 {
    let gid = skrifa::FontRef::new(font).unwrap().charmap().map(c).unwrap().to_u32();
    u16::try_from(gid).unwrap()
}

/// Shippori Mincho from craft-fonts, or `None` (with a note) when built without it.
fn shippori() -> Option<Arc<FontFace>> {
    if !crate::japanese_fonts().any(|f| f.family == "Shippori Mincho") {
        eprintln!("skipped: built without craft-fonts (set CRAFT_FONTS_DIR to a checkout)");
        return None;
    }
    let face = FontDb::global().face("Shippori Mincho", "Regular");
    assert_eq!(face.family, "Shippori Mincho");
    Some(face)
}

fn face_of(bytes: Vec<u8>) -> FontFace {
    make_face(FontBytes::Owned(Arc::new(bytes)), FontSource::Memory, 0, "Test".into(), "Regular".into(), Vec::new()).unwrap()
}

#[test]
fn reads_vmtx() {
    let face = face_of(vertical_font());
    let font = face.skrifa().unwrap();
    let gm = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    for (c, advance, tsb) in [('一', 1000.0, 120.0), ('、', 1000.0, 120.0), ('ヸ', 1024.0, 100.0)] {
        let gid = face.glyph_for(c);
        assert_ne!(gid, 0, "{c}");
        assert_eq!(face.v_advance(gid), advance, "{c}");
        // No VORG (TrueType outlines): the origin is the glyph's top plus its top side bearing.
        let top = f64::from(gm.bounds(GlyphId::new(gid)).unwrap().y_max);
        assert_eq!(face.v_origin(gid), top + tsb, "{c}");
    }
    // The em box doesn't come from vmtx: Source Sans 3's BASE table.
    assert_eq!(face.em_box(), (830.0, -170.0));
    // Glyphs past the long metrics advance as the last of them.
    let face = face_of(with_table_u16(vertical_font(), b"vhea", 34, 1).unwrap());
    assert_eq!(face.v_advance(face.glyph_for('ヸ')), 1000.0);
}

#[test]
fn vorg_gives_the_vertical_origin() {
    let wi = gid_in(&vertical_font(), 'ヸ');
    let face = face_of(with_vorg(&vertical_font(), 880, &[(wi, 900)]).unwrap());
    assert_eq!(face.v_origin(face.glyph_for('一')), 880.0);
    assert_eq!(face.v_origin(u32::from(wi)), 900.0);
    assert_eq!(face.v_advance(u32::from(wi)), 1024.0, "advances still come from vmtx");
    // VORG without vmtx: origins from VORG, one em down the line.
    let base = font_mapping("Vorg Only", GLYPHS).unwrap();
    let face = face_of(with_vorg(&base, 870, &[]).unwrap());
    let ichi = face.glyph_for('一');
    assert_eq!((face.v_advance(ichi), face.v_origin(ichi)), (1000.0, 870.0));
}

#[test]
fn shippori_reads_vmtx() {
    let Some(face) = shippori() else { return };
    let font = face.skrifa().unwrap();
    let vmtx = font.vmtx().unwrap();
    let gm = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    for c in ['「', '、', '一', 'ヸ'] {
        let gid = face.glyph_for(c);
        let g = GlyphId::new(gid);
        assert_eq!(face.v_advance(gid), f64::from(vmtx.advance(g).unwrap()), "{c}");
        // No VORG (TrueType outlines): the origin is the glyph's top plus its top side bearing.
        let top = f64::from(gm.bounds(g).unwrap().y_max) + f64::from(vmtx.side_bearing(g).unwrap());
        assert_eq!(face.v_origin(gid), top, "{c}");
    }
    // No BASE table: the em box is the OS/2 typo ascender and descender (exactly one em here).
    assert_eq!(face.em_box(), (880.0, -120.0));
    // Ideographs fill the em box; ヸ is taller than it is wide.
    let ichi = face.glyph_for('一');
    assert_eq!((face.v_advance(ichi), face.v_origin(ichi)), (1000.0, 880.0));
    let wi = face.glyph_for('ヸ');
    assert_eq!((face.advance(wi), face.v_advance(wi)), (1000.0, 1024.0));
}

#[test]
fn fonts_without_vmtx_use_the_em_box() {
    // Source Sans 3 has no vmtx; its BASE table gives the ideographic em box bottom (-170).
    let face = face_of(font_with("No Vmtx", &['一']).unwrap());
    let gid = face.glyph_for('一');
    assert_ne!(gid, 0);
    assert_eq!(face.em_box(), (830.0, -170.0));
    assert_eq!(face.v_advance(gid), face.upem);
    assert_eq!(face.v_origin(gid), 830.0);
    // Without BASE either: the OS/2 typo ascender and descender (1000, -326), centred on one em.
    let face = face_of(with_table_len(font_with("No Base", &['一']).unwrap(), b"BASE", 0).unwrap());
    assert_eq!(face.em_box(), (837.0, -163.0));
    // Without OS/2: the ascender and descender, the same way.
    let face = face_of(with_table_len(with_table_len(font_with("No OS2", &['一']).unwrap(), b"BASE", 0).unwrap(), b"OS/2", 0).unwrap());
    let (top, bottom) = face.em_box();
    assert_eq!(top - bottom, face.upem);
    assert!((top + bottom - (face.ascent - face.descent)).abs() < 1e-9, "{top} {bottom}");
}

#[test]
fn icf_box_comes_from_base_else_an_inset_em_box() {
    let font = font_with("ICF", &['一']).unwrap();
    let face = face_of(with_base(&font, &[(*b"ideo", -120), (*b"idtp", 880), (*b"icfb", -80), (*b"icft", 840)]).unwrap());
    assert_eq!((face.em_box(), face.icf_box()), ((880.0, -120.0), (840.0, -80.0)));
    // One of the two: mirrored inside the em box.
    let face = face_of(with_base(&font, &[(*b"ideo", -120), (*b"icfb", -90)]).unwrap());
    assert_eq!(face.icf_box(), (850.0, -90.0));
    // No ICF baselines (Source Sans 3's BASE has only ideo and romn): the em box inset by 5% of the em.
    let face = face_of(font);
    assert_eq!(face.em_box(), (830.0, -170.0));
    assert_eq!(face.icf_box(), (780.0, -120.0));
}

#[test]
fn broken_vertical_tables_do_not_panic() {
    let mut fonts = vec![("synthetic", vertical_font())];
    if let Some(face) = shippori() {
        fonts.push(("Shippori Mincho", face.data().to_vec()));
    }
    let vorg = with_vorg(&vertical_font(), 880, &[(1, 900), (2, 910)]).unwrap();
    let mut cases = vec![
        ("synthetic: VORG cut short".to_string(), with_table_len(vorg.clone(), b"VORG", 6).unwrap()),
        ("synthetic: more origins than VORG holds".to_string(), with_table_u16(vorg, b"VORG", 6, u16::MAX).unwrap()),
        ("BASE cut short".to_string(), with_table_len(crate::bundled()[0].to_vec(), b"BASE", 6).unwrap()),
    ];
    for (name, font) in fonts {
        cases.extend([
            (format!("{name}: vmtx cut short"), with_table_len(font.clone(), b"vmtx", 2).unwrap()),
            (format!("{name}: vmtx empty"), with_table_len(font.clone(), b"vmtx", 0).unwrap()),
            (format!("{name}: more metrics than vmtx holds"), with_table_u16(font.clone(), b"vhea", 34, u16::MAX).unwrap()),
            (format!("{name}: no long metrics"), with_table_u16(font.clone(), b"vhea", 34, 0).unwrap()),
            (format!("{name}: vhea cut short"), with_table_len(font, b"vhea", 10).unwrap()),
        ]);
    }
    for (what, bytes) in cases {
        let face = face_of(bytes);
        let (top, bottom) = face.em_box();
        assert!(top > bottom && (top - bottom - face.upem).abs() < 1e-9, "{what}: {top} {bottom}");
        let (icf_top, icf_bottom) = face.icf_box();
        assert!(icf_top > icf_bottom && icf_top.is_finite() && icf_bottom.is_finite(), "{what}: ICF {icf_top} {icf_bottom}");
        for gid in [0, face.glyph_for('一'), face.glyph_for('、'), u32::from(u16::MAX), u32::MAX] {
            let (adv, origin) = (face.v_advance(gid), face.v_origin(gid));
            assert!(adv.is_finite() && adv >= 0.0 && origin.is_finite(), "{what}: {gid} {adv} {origin}");
        }
        let g = shape_vertical(&face, "一、ま", &[feature("vpal").unwrap()], |c| c, japanese());
        assert_eq!(g.len(), 3, "{what}");
    }
}

#[test]
fn upright_text_shapes_top_to_bottom() {
    let face = face_of(vertical_font());
    let h = shape_with_context(&face, "一ヸ、", &[], |c| c, japanese());
    let v = shape_vertical(&face, "一ヸ、", &[], |c| c, japanese());
    // This font has no vertical forms (`vert`): the same glyphs.
    assert_eq!(v.iter().map(|g| g.gid).collect::<Vec<_>>(), h.iter().map(|g| g.gid).collect::<Vec<_>>());
    // Advances run down the line, by vmtx; offsets are what GPOS moves a glyph from its vertical
    // origin (nothing here).
    let placed: Vec<_> = v.iter().map(|g| (g.x_advance, g.y_advance, g.x_offset, g.y_offset)).collect();
    assert_eq!(placed, [(0, -1000, 0, 0), (0, -1024, 0, 0), (0, -1000, 0, 0)]);
    // Origins from VORG leave the offsets alone too.
    let face = face_of(with_vorg(&vertical_font(), 880, &[(gid_in(&vertical_font(), 'ヸ'), 900)]).unwrap());
    let v = shape_vertical(&face, "一ヸ", &[], |c| c, ShapeContext::default());
    assert_eq!(v.iter().map(|g| (g.y_advance, g.x_offset, g.y_offset)).collect::<Vec<_>>(), [(-1000, 0, 0), (-1024, 0, 0)]);
}

#[test]
fn shippori_takes_vertical_forms_and_proportional_metrics() {
    let Some(face) = shippori() else { return };
    let h = shape_with_context(&face, "一、", &[], |c| c, japanese());
    let v = shape_vertical(&face, "一、", &[], |c| c, japanese());
    assert_eq!(v[0].gid, h[0].gid);
    // `vert` without asking: the comma's vertical form.
    assert_ne!(v[1].gid, h[1].gid);
    assert_eq!(face.v_origin(v[1].gid), 880.0);
    // Advances run down the line; offsets are what GPOS moves a glyph from its vertical origin.
    for g in &v {
        assert_eq!((g.x_advance, g.y_advance, g.x_offset, g.y_offset), (0, -1000, 0, 0), "{g:?}");
    }
    assert_eq!(shape_vertical(&face, "ヸ", &[], |c| c, ShapeContext::default())[0].y_advance, -1024);
    // `vpal` (proportional vertical metrics) shortens kana and moves them along the line.
    let ma = shape_vertical(&face, "ま", &[feature("vpal").unwrap()], |c| c, ShapeContext::default())[0];
    assert_eq!((ma.y_advance, ma.x_offset, ma.y_offset), (-974, 0, 4));
}
