//! End-to-end: EPS programs in, PDF content out.

use super::*;

/// The page content stream of a converted EPS, inflated.
fn content(pdf: &[u8]) -> String {
    // The content stream is object 4, written right after the page.
    let at = pdf.windows(7).position(|w| w == b"4 0 obj").expect("content object");
    let rest = &pdf[at..];
    let s = rest.windows(6).position(|w| w == b"stream").unwrap() + 6;
    let s = s + if rest[s] == b'\r' { 2 } else { 1 };
    let e = rest.windows(9).position(|w| w == b"endstream").unwrap();
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&rest[s..e])
        .unwrap_or_else(|_| miniz_oxide::inflate::decompress_to_vec_zlib(&rest[s..e - 1]).unwrap());
    String::from_utf8_lossy(&raw).into_owned()
}

fn eps(body: &str) -> Vec<u8> {
    format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 10 20 110 70\n%%EndComments\n{body}\n%%EOF\n").into_bytes()
}

fn run(body: &str) -> (String, Converted) {
    let c = eps_to_pdf(&eps(body), Limits::default()).unwrap();
    (content(&c.pdf), c)
}

#[test]
fn paths_fill_and_stroke_in_their_colours() {
    let (c, conv) = run("0 0 1 0 setcmykcolor 10 20 50 30 rectfill  1 0 0 setrgbcolor 2 setlinewidth newpath 10 20 moveto 100 60 lineto stroke");
    assert_eq!(conv.bbox, [10.0, 20.0, 110.0, 70.0]);
    // The bounding box's corner moves to the page origin.
    assert!(c.starts_with("1 0 0 1 -10 -20 cm"), "{c}");
    assert!(c.contains("0 0 1 0 k"), "CMYK stays CMYK: {c}");
    assert!(c.contains("10 20 m") && c.contains("f"), "{c}");
    assert!(c.contains("1 0 0 RG") && c.contains("2 w") && c.contains("S"), "{c}");
}

#[test]
fn procedures_loops_and_dictionaries_run() {
    let (c, _) = run("/sq { newpath moveto 10 0 rlineto 0 10 rlineto -10 0 rlineto closepath fill } bind def
         0 1 2 { 15 mul 10 add 30 sq } for
         /d 3 dict def d /n 7 put d /n get 7 eq { 0.5 setgray 60 20 5 5 rectfill } if
         { 1 0 div } stopped { 0.25 setgray 70 20 5 5 rectfill } if
         [ 1 2 3 ] { pop } forall  3 { } repeat  0 { 1 add dup 4 ge { exit } if } loop pop");
    assert_eq!(c.lines().filter(|l| *l == "f").count(), 5, "{c}");
    assert!(c.contains("0.5 g") && c.contains("0.25 g"), "{c}");
}

#[test]
fn transforms_curves_and_clips() {
    let (c, _) = run("gsave 60 45 translate 2 2 scale newpath 0 0 10 0 360 arc clip newpath -20 -20 40 40 rectfill grestore 10 20 5 5 rectfill");
    assert!(c.contains(" c"), "arcs become curves: {c}");
    assert!(c.contains("W") && c.contains("q") && c.contains("Q"), "clip in its own q: {c}");
    // The circle is centred at (60, 45) with radius 20 in device space.
    assert!(c.contains("80 45 m"), "{c}");
}

#[test]
fn separation_colours_use_their_tint_transform() {
    let (c, _) = run("[/Separation (Spot) /DeviceCMYK { dup 0 exch 0 } ] setcolorspace 0.5 setcolor 10 20 10 10 rectfill");
    assert!(c.contains("0.5 0 0.5 0 k") || c.contains(".5 0 .5 0 k"), "{c}");
}

#[test]
fn images_and_masks_become_xobjects() {
    let (c, conv) = run("gsave 10 20 translate 20 10 scale 2 1 8 [2 0 0 -1 0 1] <00ff> image grestore
                         gsave 40 20 translate 10 10 scale 0 0 1 setrgbcolor 8 1 true [8 0 0 -1 0 1] <f0> imagemask grestore
                         gsave 60 20 translate 10 10 scale 1 1 8 [1 0 0 -1 0 1] currentfile /ASCIIHexDecode filter false 3 colorimage
ff8000>
grestore");
    assert_eq!(c.matches("Do").count(), 3, "{c}");
    let pdf = String::from_utf8_lossy(&conv.pdf);
    assert!(pdf.contains("/ImageMask true") && pdf.contains("/DeviceRGB") && pdf.contains("/DeviceGray"), "{pdf}");
}

#[test]
fn type3_fonts_draw_their_glyphs() {
    let (c, _) = run("/T3 8 dict dup begin /FontType 3 def /FontMatrix [0.001 0 0 0.001 0 0] def /FontBBox [0 0 1000 1000] def
         /Encoding 256 array def 0 1 255 { Encoding exch /.notdef put } for Encoding 65 /A put
         /BuildChar { pop pop 600 0 0 0 500 500 setcachedevice 0 0 500 500 rectfill } def end def
         /T3 T3 definefont pop /T3 findfont 20 scalefont setfont 10 20 moveto (AA) show");
    // Two glyphs, the second 12 points (600 units at 20 pt) to the right.
    assert_eq!(c.lines().filter(|l| *l == "f").count(), 2, "{c}");
    assert!(c.contains("22 20 m"), "{c}");
}

#[test]
fn a_string_made_a_file_runs_as_a_program() {
    // Adobe's idiom: `(code) 0 () /SubFileDecode filter cvx exec`.
    let (c, _) = run("(0.5 setgray 10 20 5 5 rectfill) 0 () /SubFileDecode filter cvx exec");
    assert!(c.contains("0.5 g"), "{c}");
}

#[test]
fn eexec_sections_run() {
    // `(1 0 0 setrgbcolor 10 20 5 5 rectfill) ` encrypted with the eexec key, as hex.
    let plain = b"1 0 0 setrgbcolor 10 20 5 5 rectfill currentfile closefile\n";
    let mut r: u16 = 55665;
    let mut hex = String::new();
    for &p in [1u8, 2, 3, 4].iter().chain(plain.iter()) {
        let c = p ^ (r >> 8) as u8;
        r = (u16::from(c).wrapping_add(r)).wrapping_mul(52845).wrapping_add(22719);
        hex.push_str(&format!("{c:02x}"));
    }
    let (c, _) = run(&format!("currentfile eexec\n{hex}\n0 1 0 setrgbcolor 20 20 5 5 rectfill"));
    assert!(c.contains("1 0 0 rg") && c.contains("0 1 0 rg"), "the program carries on after the section: {c}");
}

#[test]
fn hostile_programs_end_with_an_error_not_a_hang_or_panic() {
    let limits = Limits { steps: 200_000, memory: 64 << 20 };
    for body in [
        "{ } loop",
        "/f { f } def f",
        "/f { 1 f } def f",
        "{ 1 } loop",
        "1000000000 array",
        "0 1 1e9 { pop } for",
        "[ 1 2",
        "/x 10 dict def x begin x begin",
        "100000 { gsave } repeat",
        "{ { { { { { stop } exec } exec } exec } exec } exec } loop",
        "(unterminated",
        "1 0 idiv",
        "mark { 0 0 moveto } 1000000 { dup exec } repeat",
        "currentfile 100000000 string readstring",
        "/A85 currentfile /ASCII85Decode filter def ~>",
        "0 0 moveto 1 1 1e300 1e300 0 0 curveto stroke",
        "[1e300 0 0 1e300 0 0] concat 0 0 1 1 rectfill",
    ] {
        let _ = eps_to_pdf(&eps(body), limits);
    }
    // Nothing painted and an error: no PDF, so the caller keeps the preview.
    assert!(eps_to_pdf(&eps("1 0 div"), limits).is_err());
    assert!(eps_to_pdf(b"%!PS-Adobe-3.0 EPSF-3.0\n0 0 1 1 rectfill\n", limits).is_err(), "no bounding box");
    assert!(eps_to_pdf(b"not postscript", limits).is_err());
}

#[test]
fn a_file_in_a_name_is_data_until_executed() {
    // cairo keeps its image filter in a name and calls `status` and `flushfile` on it.
    let (c, _) = run("/f (0.5 setgray 10 20 5 5 rectfill) 0 () /SubFileDecode filter def f status pop f cvx exec");
    assert!(c.contains("0.5 g"), "{c}");
}

#[test]
fn smooth_shadings_become_pdf_shadings() {
    let (c, conv) = run("<< /ShadingType 2 /ColorSpace /DeviceCMYK /Coords [10 20 110 20] /Extend [true true]
                           /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0 0] /C1 [0 0 1 0] /N 1 >> >> shfill
                         << /ShadingType 3 /ColorSpace [/Separation (Gold) /DeviceCMYK { dup 0.2 mul exch 0 exch 0 } ]
                           /Coords [60 45 0 60 45 20] /Function { } >> shfill");
    assert_eq!(c.matches(" sh").count(), 2, "{c}");
    let pdf = String::from_utf8_lossy(&conv.pdf);
    assert!(pdf.contains("/ShadingType 2") && pdf.contains("/ShadingType 3"), "{pdf}");
    // CMYK and a spot made of CMYK stay CMYK.
    assert_eq!(pdf.matches("/ColorSpace /DeviceCMYK").count(), 2, "{pdf}");
}

#[test]
fn shading_patterns_fill_their_path() {
    let (c, _) = run("<< /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 100 0]
                         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> matrix makepattern
                       setpattern 10 20 50 30 rectfill");
    assert!(c.contains("W") && c.contains(" sh"), "clipped to the rectangle, then shaded: {c}");
}

#[test]
fn mesh_shadings_keep_their_vertices() {
    // Two free-form triangles from an array, the second sharing an edge (flag 1).
    let (_, conv) = run("<< /ShadingType 4 /ColorSpace /DeviceRGB
                           /DataSource [0 10 20 1 0 0  0 110 20 0 1 0  0 60 70 0 0 1  1 110 70 1 1 0] >> shfill
                         << /ShadingType 6 /ColorSpace /DeviceGray /DataSource [0 10 20 10 30 10 40 10 50 20 50 30 50 40 50 50 50 50 40 50 30 50 20 40 20 30 20 20 20  0 0.3 0.6 1] >> shfill");
    let pdf = String::from_utf8_lossy(&conv.pdf);
    assert!(pdf.contains("/ShadingType 4") && pdf.contains("/ShadingType 6"), "{pdf}");
    // Malformed meshes draw what they hold, or nothing.
    for bad in [
        "<< /ShadingType 4 /ColorSpace /DeviceRGB /DataSource [0 1] >> shfill",
        "<< /ShadingType 7 /ColorSpace /DeviceGray /DataSource (abc) /BitsPerCoordinate 32 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 1 0 1 0 1] >> shfill 0 0 1 1 rectfill",
    ] {
        let _ = eps_to_pdf(&eps(bad), Limits::default());
    }
}

#[test]
fn type42_fonts_draw_their_truetype_glyphs() {
    use skrifa::MetadataProvider;
    let ttf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/JetBrainsMono-Regular.ttf")).unwrap();
    let gid = skrifa::FontRef::new(&ttf).unwrap().charmap().map('A').unwrap().to_u32();
    // sfnts strings split at an even length below the string limit.
    let sfnts: String = ttf.chunks(30_000).map(|c| format!("<{}>\n", c.iter().map(|b| format!("{b:02x}")).collect::<String>())).collect();
    let (c, _) = run(&format!(
        "/T42 11 dict dup begin /FontType 42 def /FontMatrix [1 0 0 1 0 0] def /PaintType 0 def /FontBBox [0 0 1 1] def
         /Encoding 256 array def 0 1 255 {{ Encoding exch /.notdef put }} for Encoding 65 /A put
         /CharStrings 2 dict dup /.notdef 0 put dup /A {gid} put def /sfnts [ {sfnts} ] def end definefont
         20 scalefont setfont 10 20 moveto (AA) show"
    ));
    // Two outlined glyphs, filled; the second 12 points on (JetBrains Mono advances 600/1000).
    assert_eq!(c.lines().filter(|l| *l == "f").count(), 2, "{}", &c[..c.len().min(400)]);
}

/// A CFF INDEX of `items` (4-byte offsets).
fn cff_index(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    if items.is_empty() {
        return out;
    }
    out.push(4);
    let mut off = 1u32;
    out.extend(off.to_be_bytes());
    for it in items {
        off += it.len() as u32;
        out.extend(off.to_be_bytes());
    }
    for it in items {
        out.extend(it);
    }
    out
}

/// A DICT integer operand in its five-byte form (so offsets don't change the length).
fn cff_int(v: i32) -> Vec<u8> {
    [vec![29], v.to_be_bytes().to_vec()].concat()
}

/// A one-glyph CFF font set: glyph 1 is a 500 × 700 box, 600 wide. CID-keyed fonts give it CID 5.
fn tiny_cff(cid: bool) -> Vec<u8> {
    let n = |v: i32| vec![(v + 139) as u8];
    let box_glyph =
        [n(-100), n(50), n(0), vec![21], vec![247, 136], n(0), vec![5], n(0), vec![248, 80], vec![5], vec![251, 136], n(0), vec![5], vec![14]]
            .concat();
    // 600 = width as the first (odd) operand: nominalWidthX 700 → −100.
    let charstrings = cff_index(&[vec![14], box_glyph]);
    let charset = if cid { vec![0, 0, 5] } else { vec![0, 0, 34] };
    let private = [cff_int(700), vec![21]].concat();
    let strings = cff_index(&[b"Adobe".to_vec(), b"Identity".to_vec()]);
    let head = vec![1, 0, 4, 4];
    let names = cff_index(&[b"T".to_vec()]);
    // Lay out: header, names, top dict, strings, gsubrs, charset, charstrings, private, [fdarray, fdselect].
    let top_len = |extra: usize| 1 + 2 + 4 + 4 + (5 + 1) * 2 + (5 + 5 + 1) + extra;
    let extra = if cid { 5 + 5 + 5 + 2 + 5 + 2 + 5 + 2 + 5 + 2 } else { 0 };
    let top_index_len = 2 + 1 + 8 + (top_len(extra) - 11);
    let mut at = head.len() + names.len() + top_index_len + strings.len() + 2;
    let charset_at = at;
    at += charset.len();
    let cs_at = at;
    at += charstrings.len();
    let priv_at = at;
    at += private.len();
    let fd_private = private.clone();
    let fd_dict = [cff_int(fd_private.len() as i32), cff_int(priv_at as i32), vec![18]].concat();
    let fdarray = cff_index(&[fd_dict]);
    let fdarray_at = at;
    at += fdarray.len();
    let fdselect_at = at;
    let fdselect = vec![0, 0, 0];
    let mut top =
        [cff_int(charset_at as i32), vec![15], cff_int(cs_at as i32), vec![17], cff_int(private.len() as i32), cff_int(priv_at as i32), vec![18]]
            .concat();
    if cid {
        top = [
            cff_int(391),
            cff_int(392),
            cff_int(0),
            vec![12, 30],
            cff_int(6),
            vec![12, 34],
            cff_int(fdarray_at as i32),
            vec![12, 36],
            cff_int(fdselect_at as i32),
            vec![12, 37],
            top,
        ]
        .concat();
    }
    let tops = cff_index(&[top]);
    let mut out = [head, names, tops, strings, vec![0, 0]].concat();
    assert_eq!(out.len(), charset_at, "layout");
    out.extend(charset);
    out.extend(charstrings);
    out.extend(private);
    if cid {
        out.extend(fdarray);
        out.extend(fdselect);
    }
    out
}

fn startdata_eps(cff: &[u8], body: &str) -> Vec<u8> {
    let mut v =
        format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 10 20 110 70\n/FontSetInit /ProcSet findresource begin\n/T {} StartData ", cff.len())
            .into_bytes();
    v.extend_from_slice(cff);
    v.extend_from_slice(format!("\n{body}\n%%EOF\n").as_bytes());
    v
}

#[test]
fn cff_fonts_from_startdata_draw_their_glyphs() {
    let c = content(
        &eps_to_pdf(&startdata_eps(&tiny_cff(false), "/T findfont 10 scalefont setfont 10 20 moveto (AA) show"), Limits::default()).unwrap().pdf,
    );
    assert_eq!(c.lines().filter(|l| *l == "f").count(), 2, "{c}");
    // The box starts 0.5 points (50 units) in; the second glyph 6 points (600 units) on.
    assert!(c.contains("10.5 20 m") && c.contains("16.5 20 m"), "{c}");
}

#[test]
fn cid_fonts_draw_through_cmaps() {
    let body = "/T-Identity-H findfont 10 scalefont setfont 10 20 moveto <00050005> show
                /CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName /One-H def
                1 begincodespacerange <00> <ff> endcodespacerange 1 begincidchar <41> 5 endcidchar
                endcmap CMapName currentdict /CMap defineresource pop end end
                /U /One-H [/T] composefont pop /U findfont 10 scalefont setfont 10 40 moveto (AA) show";
    let c = content(&eps_to_pdf(&startdata_eps(&tiny_cff(true), body), Limits::default()).unwrap().pdf);
    assert_eq!(c.lines().filter(|l| *l == "f").count(), 4, "{c}");
    assert!(c.contains("16.5 20 m") && c.contains("16.5 40 m"), "{c}");
    // Broken font data is a warning, not a failure.
    let r = eps_to_pdf(&startdata_eps(b"\x01\x00\x04\x04\xff\xff\xff", "0 0 1 1 rectfill"), Limits::default()).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("CFF")), "{:?}", r.warnings);
}

#[test]
fn tiling_patterns_become_pdf_patterns() {
    let (c, conv) = run("<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10
                            /PaintProc { pop 1 0 0 setrgbcolor 0 0 5 5 rectfill } >> matrix makepattern setpattern
                          10 20 50 30 rectfill
                          << /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 4 4] /XStep 4 /YStep 4
                            /PaintProc { pop 1 0 0 setrgbcolor 0 0 2 4 rectfill } >> matrix makepattern /S exch def
                          [/Pattern /DeviceCMYK] setcolorspace 0 1 0 0 S setcolor 60 20 40 30 rectfill");
    assert!(c.contains("/Pattern cs") && c.contains("/P0 scn") && c.contains("/P1 scn"), "{c}");
    let pdf = String::from_utf8_lossy(&conv.pdf);
    assert!(pdf.contains("/PatternType 1") && pdf.contains("/XStep 10"), "{pdf}");
    // A cell that fails or nests too deeply ends cleanly.
    for bad in [
        "<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 0 /YStep 1 /PaintProc { pop } >> matrix makepattern setpattern 0 0 1 1 rectfill",
        "/P << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 1 /YStep 1 /PaintProc { pop P setpattern 0 0 1 1 rectfill } >> matrix makepattern def P setpattern 0 0 1 1 rectfill",
        "<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 1 /YStep 1 /PaintProc { 1 0 div } >> matrix makepattern setpattern 0 0 1 1 rectfill",
    ] {
        let _ = eps_to_pdf(&eps(bad), Limits::default());
    }
}

#[test]
fn strokepath_outlines_the_stroke() {
    // A 4-point butt-capped line from (10,20) to (60,20): its outline is the band y 18–22.
    let (c, _) = run("newpath 10 30 moveto 60 30 lineto 4 setlinewidth strokepath 0 setgray fill");
    assert!(c.lines().any(|l| l == "f"), "{c}");
    assert!(!c.contains(" S\n") && !c.lines().any(|l| l == "S"), "filled, not stroked: {c}");
    for y in ["28", "32"] {
        assert!(c.contains(&format!(" {y} l")) || c.contains(&format!(" {y} m")), "edge at y={y}: {c}");
    }
    // Dashes split it; a path through a degenerate matrix ends empty; a clip to a stroke works.
    let (c, _) = run("newpath 10 30 moveto 110 30 lineto 2 setlinewidth [10 10] 0 setdash strokepath fill");
    assert!(c.matches(" m").count() >= 5, "five dashes: {c}");
    let _ = run("[0 0 0 0 0 0] concat newpath 0 0 moveto 1 1 lineto strokepath fill");
    let (c, _) = run("newpath 10 30 moveto 100 30 lineto 10 setlinewidth strokepath clip newpath 10 20 100 50 rectfill");
    assert!(c.contains("W"), "{c}");
}

#[test]
fn vertical_cid_text_runs_down_from_its_vertical_origin() {
    let body = "/T-Identity-V findfont 10 scalefont setfont 10 40 moveto <00050005> show";
    let c = content(&eps_to_pdf(&startdata_eps(&tiny_cff(true), body), Limits::default()).unwrap().pdf);
    // v = (300, 880) units: the glyph's origin is 3 left and 8.8 below the point; the second
    // glyph is one em (10) lower. The box starts 0.5 in from its origin.
    assert!(c.contains("7.5 31.2 m") && c.contains("7.5 21.2 m"), "{c}");
    // A CMap program can make a font vertical too.
    let body = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName /V1 def /WMode 1 def
                1 begincodespacerange <00> <ff> endcodespacerange 1 begincidchar <41> 5 endcidchar
                endcmap CMapName currentdict /CMap defineresource pop end end
                /U /V1 [/T] composefont pop /U findfont 10 scalefont setfont 10 40 moveto (AA) show";
    let c = content(&eps_to_pdf(&startdata_eps(&tiny_cff(true), body), Limits::default()).unwrap().pdf);
    assert!(c.contains("7.5 31.2 m") && c.contains("7.5 21.2 m"), "{c}");
}

#[test]
fn a_tiny_dash_on_a_long_path_draws_solid() {
    let body = "newpath 0 30 moveto 100000 30 lineto [0.0001] 0 setdash";
    let (c, _) = run(&format!("{body} stroke"));
    assert!(!c.contains(" d\n") && !c.contains("] 0 d"), "{c}");
    let (c, _) = run(&format!("{body} strokepath fill"));
    assert!(c.matches(" m").count() < 10, "{}", &c[..c.len().min(300)]);
}

#[test]
fn metrics_without_two_numbers_dont_underflow() {
    // `[/a /b]` has two elements but no numbers: the width index used to go below zero.
    let (c, _) = run("/Helvetica findfont dup length 1 add dict copy dup /Metrics << /A [/a /b] /B [7] >> put
                      /H exch definefont 10 scalefont setfont 10 20 moveto (AB) show 10 20 1 1 rectfill");
    assert!(c.contains("re") || c.contains(" m"), "{c}");
}

#[test]
fn work_in_proportion_to_operands_counts_against_the_limits() {
    let limits = Limits { steps: 200_000, memory: 8 << 20 };
    let ends = |body: &str| {
        let r = eps_to_pdf(&eps(&format!("10 20 1 1 rectfill {body}")), limits).expect("what was painted is kept");
        let w = r.warnings.join(" ");
        assert!(w.contains("too long") || w.contains("memory") || w.contains("too much data"), "{body}: {w}");
        // It stops at the charge that crosses the limit: overshoot is one operand's worth at most.
        assert!(r.steps <= limits.steps + 1_000_000 || r.memory > limits.memory, "{body}: {} steps", r.steps);
    };
    // Each would take seconds to minutes in a few steps without its charge.
    ends("/h 1000000 string def /n 500 string def { h n search pop pop } loop");
    ends("100000 { 0 } repeat { 100000 copy 100000 { pop } repeat } loop");
    ends("/a 4000000 string def /b 4000000 string def { a b eq pop } loop");
    ends("newpath 0 0 moveto 100000 { 1 0 rlineto } repeat { gsave grestore } loop");
    ends("{ 4000000 string pop } loop");
    ends("/d 100000 dict def 0 1 99999 { d exch 0 put } for { d { pop pop } forall } loop");
    ends("/Helvetica findfont 10 scalefont setfont /s 1000000 string def { 0 0 moveto s show } loop");
    ends("mark 100000 { 0 } repeat { counttomark pop } loop");
    // Reading counts: a stream decoded over and over.
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&vec![0u8; 1 << 20], 9);
    let hex: String = z.iter().map(|b| format!("{b:02x}")).collect();
    ends(&format!("/z <{hex}> def {{ z /FlateDecode filter dup 2000000 string readstring pop pop closefile }} loop"));
    // Tiny programs that only loop still end at the step limit.
    ends("{ } loop");
}

#[test]
fn the_web_gets_smaller_default_limits() {
    let l = Limits::default();
    if cfg!(target_arch = "wasm32") {
        assert!(l.memory <= 256 << 20 && l.steps <= 20_000_000);
    } else {
        assert!(l.memory <= 1 << 30 && l.steps <= 50_000_000);
    }
}
