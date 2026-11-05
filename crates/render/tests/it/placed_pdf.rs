//! Placed PDFs are drawn through craft-pdf's patched Hayro crates. Calculator functions whose `idiv`
//! divides by zero or overflows, stitching functions nested a hundred thousand deep, and mesh
//! shadings with points far off the page, ten thousand overlapping triangles or forty thousand
//! patches do not panic, overflow the stack or take more than a minute, and the rest of the page is
//! drawn. A page with a `/UserUnit` is sized in points, and so are its boxes. The function and mesh
//! cases follow pdfcraft's tests in `crates/render/src/raster.rs`.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use craft_pdf::interpret::Function;
use craft_pdf::syntax::Pdf;
use craft_pdf::syntax::object::{Object, ObjectIdentifier};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Graphic, Item, ItemId, PdfCrop, Shape, SpreadRef};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_render::{Rendered, Renderer, pdf_crop_box, pdf_page_size};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// An exponential function of one input with one output.
const LEAF: &str = "<</FunctionType 2/Domain[0 1]/C0[0]/C1[1]/N 1>>";

/// Mesh entries common to the shadings below: RGB colors, 8-bit coordinates and components.
const MESH: &str = "/ColorSpace /DeviceRGB /BitsPerCoordinate 8 /BitsPerComponent 8";

/// One triangle of a free-form triangle mesh: three vertices of a flag, x, y and a color.
const TRIANGLE: [u8; 18] = [0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 255];

/// A stream object with the `dict` entries, its `/Length` and `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<<{dict} /Length {}>>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

/// A one-page PDF whose page dictionary holds the `page` entries (boxes, resources) and whose page
/// draws `content`. The `objects` are numbered from 5.
fn pdf(page: &str, content: &str, objects: &[Vec<u8>]) -> Vec<u8> {
    let mut bodies = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        format!("<</Type/Page/Parent 2 0 R{page}/Contents 4 0 R>>").into_bytes(),
        stream("", content.as_bytes()),
    ];
    bodies.extend_from_slice(objects);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", bodies.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", bodies.len() + 1).as_bytes());
    out
}

/// The page of a document `size` points square with `pdf` placed to fill it, rendered by
/// `Renderer::render_page` at one pixel per point. Fails when the render panics or takes more
/// than a minute.
fn placed(size: f64, pdf: Vec<u8>) -> Rendered {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut d = Document::new(&NewDocument { width: size, height: size, facing_pages: false, ..Default::default() });
        let id = AssetId(d.alloc());
        let asset = d.add_asset(Asset { id, name: "art.pdf".into(), mime: "application/pdf".into(), data: Arc::new(pdf), ..Default::default() });
        let id = ItemId(d.alloc());
        let mut item = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, size, size)));
        item.content = Content::Graphic(Graphic {
            layer_options: None,
            asset,
            size: (size, size),
            xf: Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        d.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let _ = tx.send(renderer.render_page(&d, &Cache::new(), 0, 1.0, false, &Default::default()));
    });
    match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(page) => page.expect("the page renders"),
        Err(RecvTimeoutError::Timeout) => panic!("the placed PDF did not render within a minute"),
        Err(RecvTimeoutError::Disconnected) => panic!("the render of the placed PDF panicked"),
    }
}

/// A 40-point page that paints an axial shading through the first of `functions` (objects 6
/// onwards), then a blue square over the whole page.
fn shaded(functions: &[Vec<u8>]) -> Vec<u8> {
    let mut objects = vec![b"<</ShadingType 2/ColorSpace/DeviceRGB/Coords[0 0 40 0]/Function 6 0 R>>".to_vec()];
    objects.extend_from_slice(functions);
    pdf("/MediaBox[0 0 40 40]/Resources<</Shading<</S 5 0 R>>>>", "/S sh 0 0 1 rg 0 0 40 40 re f", &objects)
}

/// A calculator (Type 4) function of one input that runs `program`.
fn calculator(program: &str) -> Vec<u8> {
    stream("/FunctionType 4/Domain[0 1]/Range[0 1 0 1 0 1]", program.as_bytes())
}

/// The function that is object 5 of a PDF holding `objects`.
fn function(objects: &[&str]) -> Option<Function> {
    let objects: Vec<Vec<u8>> = objects.iter().map(|o| o.as_bytes().to_vec()).collect();
    let parsed = Pdf::new(pdf("/MediaBox[0 0 20 20]", "", &objects)).unwrap();
    let object = parsed.xref().get::<Object<'_>>(ObjectIdentifier::new(5, 0)).unwrap();
    Function::new(&object)
}

#[test]
fn placed_pdf_function_dividing_by_zero_leaves_the_page_drawn() {
    assert_eq!(placed(40.0, shaded(&[calculator("{ pop 1 0 idiv }")])).pixel(20, 20), BLUE);
}

#[test]
fn placed_pdf_function_dividing_the_smallest_integer_by_minus_one_leaves_the_page_drawn() {
    assert_eq!(placed(40.0, shaded(&[calculator("{ pop -2147483648 -1 idiv }")])).pixel(20, 20), BLUE);
}

/// A chain of 100,000 functions: each stitching function names the next by reference, and the last
/// is an exponential function.
#[test]
fn placed_pdf_chain_of_stitching_functions_leaves_the_page_drawn() {
    let mut functions: Vec<Vec<u8>> =
        (7..100_006).map(|next| format!("<</FunctionType 3/Domain[0 1]/Functions[{next} 0 R]/Bounds[]/Encode[0 1]>>").into_bytes()).collect();
    functions.push(b"<</FunctionType 2/Domain[0 1]/C0[0 0 0]/C1[1 1 1]/N 1>>".to_vec());
    assert_eq!(placed(40.0, shaded(&functions)).pixel(20, 20), BLUE);
}

/// Stitching functions that each name the next through `N 0 R`: 64 evaluate, 65 are refused.
#[test]
fn function_limits_depth_guard_refuses_a_chain_of_references() {
    for functions in [64, 65] {
        let objects: Vec<String> = (5..5 + functions)
            .map(|number| {
                if number < 4 + functions {
                    format!("<</FunctionType 3/Domain[0 1]/Functions[{} 0 R]/Bounds[]/Encode[0 1]>>", number + 1)
                } else {
                    LEAF.to_owned()
                }
            })
            .collect();
        let objects: Vec<&str> = objects.iter().map(String::as_str).collect();
        let chain = function(&objects);
        if functions == 64 {
            assert_eq!(chain.unwrap().eval([0.5].into_iter().collect()).unwrap().as_slice(), &[0.5]);
        } else {
            assert!(chain.is_none(), "65 functions are one past the function-depth guard");
        }
    }
}

/// Stitching functions with a cycle, no children or a missing child are refused; one child named
/// twice still evaluates.
#[test]
fn function_limits_stitching_cycles_are_refused_and_shared_children_work() {
    for objects in [
        &["<</FunctionType 3/Domain[0 1]/Functions[5 0 R]/Bounds[]/Encode[0 1]>>"][..],
        &[
            "<</FunctionType 3/Domain[0 1]/Functions[6 0 R]/Bounds[]/Encode[0 1]>>",
            "<</FunctionType 3/Domain[0 1]/Functions[5 0 R]/Bounds[]/Encode[0 1]>>",
        ],
        &["<</FunctionType 3/Domain[0 1]/Functions[]/Bounds[]/Encode[]>>"],
        &["<</FunctionType 3/Domain[0 1]/Functions[99 0 R]/Bounds[]/Encode[0 1]>>"],
        &["<</FunctionType 3/Domain[0 1]/Functions[6 0 R 99 0 R]/Bounds[0.5]/Encode[0 1 0 1]>>", LEAF],
    ] {
        assert!(function(objects).is_none(), "accepted invalid children: {objects:?}");
    }
    let shared = function(&["<</FunctionType 3/Domain[0 1]/Functions[6 0 R 6 0 R]/Bounds[0.5]/Encode[0 1 0 1]>>", LEAF]).unwrap();
    let output = shared.eval([0.75].into_iter().collect()).unwrap();
    assert!((output[0] - 0.5).abs() < 0.001);
}

/// Ten thousand triangles that each cover half of the placed page, which is drawn 1000 pixels
/// square.
#[test]
fn many_overlapping_mesh_triangles_finish() {
    let shading = stream(&format!("/ShadingType 4 {MESH} /BitsPerFlag 8 /Decode [0 125 0 125 0 1 0 1 0 1]"), &TRIANGLE.repeat(10_000));
    let page = placed(125.0, pdf("/MediaBox[0 0 125 125]/Resources<</Shading<</Sh0 5 0 R>>>>", "/Sh0 sh", &[shading]));
    let p = page.pixel(12, 112);
    assert!(p != WHITE && p[3] == 255, "the triangles are drawn: {p:?}");
}

/// One Coons patch: its flag, twelve points around a square of 8-bit coordinates, four colors.
fn coons() -> Vec<u8> {
    let mut patch = vec![0u8];
    for p in [(0, 0), (0, 85), (0, 170), (0, 255), (85, 255), (170, 255), (255, 255), (255, 170), (255, 85), (255, 0), (170, 0), (85, 0)] {
        patch.extend_from_slice(&[p.0, p.1]);
    }
    patch.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]);
    patch
}

/// A 40-point page that paints `shading` (object 5), then a red square at its bottom left.
fn meshed(shading: Vec<u8>) -> Vec<u8> {
    pdf("/MediaBox[0 0 40 40]/Resources<</Shading<</Sh0 5 0 R>>>>", "/Sh0 sh 1 0 0 rg 0 0 4 4 re f", &[shading])
}

/// Meshes whose points lie far off the page are sampled only where the page shows them.
#[test]
fn mesh_shadings_far_off_the_page_render() {
    let far = "/Decode [-40000 40000 -40000 40000 0 1 0 1 0 1]";
    for (what, shading) in [
        ("a Coons patch", stream(&format!("/ShadingType 6 {MESH} /BitsPerFlag 8 {far}"), &coons())),
        ("a triangle mesh", stream(&format!("/ShadingType 4 {MESH} /BitsPerFlag 8 {far}"), &TRIANGLE)),
    ] {
        assert_eq!(placed(40.0, meshed(shading)).pixel(1, 38), RED, "{what} far off the page: the rest of the page draws");
    }
    for (what, decode) in [("on the page", "[0 40 0 40 0 1 0 1 0 1]"), ("covering the page", "[-40000 40000 -40000 40000 0 1 0 1 0 1]")] {
        let page = placed(40.0, meshed(stream(&format!("/ShadingType 6 {MESH} /BitsPerFlag 8 /Decode {decode}"), &coons())));
        let center = page.pixel(20, 20);
        assert!(center != WHITE && center[3] == 255, "a patch {what} is shaded: {center:?}");
    }
    // A patch filling a 4 × 4 pt rectangle as a pattern is still shaded.
    let shading = stream(&format!("/ShadingType 6 {MESH} /BitsPerFlag 8 /Decode [10 14 10 14 0 1 0 1 0 1]"), &coons());
    let page = placed(
        40.0,
        pdf("/MediaBox[0 0 40 40]/Resources<</Pattern<</P0<</PatternType 2/Shading 5 0 R>>>>>>", "/Pattern cs /P0 scn 10 10 4 4 re f", &[shading]),
    );
    let middle = page.pixel(12, 28);
    assert!(middle != WHITE && middle[3] == 255, "a small patch is shaded: {middle:?}");
}

/// Forty thousand Coons patches beside the page, and a lattice with no vertices per row.
#[test]
fn mesh_shadings_with_many_patches_or_no_rows_finish() {
    for shading in [
        stream(&format!("/ShadingType 6 {MESH} /BitsPerFlag 8 /Decode [100 140 100 140 0 1 0 1 0 1]"), &coons().repeat(40_000)),
        stream(&format!("/ShadingType 5 {MESH} /VerticesPerRow 0 /Decode [0 40 0 40 0 1 0 1 0 1]"), &[0; 64]),
    ] {
        assert_eq!(placed(40.0, meshed(shading)).pixel(1, 38), RED, "the rest of the page draws");
    }
}

/// A page with `/UserUnit 2` is twice its media box in points, and the crop boxes import offers
/// are measured in points as well.
#[test]
fn a_page_with_a_user_unit_and_its_boxes_are_sized_in_points() {
    let page = pdf("/MediaBox[0 0 100 50]/TrimBox[10 10 90 40]/UserUnit 2", "", &[]);
    assert_eq!(pdf_page_size(&page, 0), Some((200.0, 100.0)));
    let near = |crop: PdfCrop, frac: [f64; 4], size: (f64, f64)| {
        let (f, s) = pdf_crop_box(&page, 0, crop).unwrap();
        assert!(
            f.iter().zip(frac).all(|(a, b)| (a - b).abs() < 1e-6) && (s.0 - size.0).abs() < 1e-6 && (s.1 - size.1).abs() < 1e-6,
            "{crop:?}: {f:?} {s:?}"
        );
    };
    near(PdfCrop::Trim, [0.1, 0.2, 0.9, 0.8], (160.0, 60.0));
    near(PdfCrop::Media, [0.0, 0.0, 1.0, 1.0], (200.0, 100.0));
}
