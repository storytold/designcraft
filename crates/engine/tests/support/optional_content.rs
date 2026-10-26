use std::sync::Arc;

use designcraft_doc::{Asset, AssetId, Content, Document, Graphic, Item, ItemId, Shape, SpreadRef, build::NewDocument};
use designcraft_geom::{Affine, Rect, shapes};
use hayro_syntax::{
    Pdf,
    object::{Array, Dict, Name, ObjectIdentifier},
};

pub fn fixture(config: &str, second_name: &str) -> Vec<u8> {
    layered_fixture(Some(config), second_name)
}

pub fn orphaned_fixture() -> Vec<u8> {
    layered_fixture(None, "Hidden")
}

fn layered_fixture(config: Option<&str>, second_name: &str) -> Vec<u8> {
    let properties = config.map(|config| format!("/OCProperties<</OCGs[5 0 R 6 3 R]/D<<{config}>>>>")).unwrap_or_default();
    properties_fixture(&properties, second_name)
}

pub fn properties_fixture(properties: &str, second_name: &str) -> Vec<u8> {
    grouped_fixture(properties, Some(second_name))
}

pub fn nameless_fixture(config: &str) -> Vec<u8> {
    grouped_fixture(&format!("/OCProperties<</OCGs[5 0 R 6 3 R]/D<<{config}>>>>"), None)
}

fn grouped_fixture(properties: &str, second_name: Option<&str>) -> Vec<u8> {
    let content = "/OC /a BDC 1 0 0 0 k 0 0 50 100 re f EMC /OC /b BDC 0 1 0 0 k 50 0 50 100 re f EMC";
    let name = second_name.map(|name| format!("/Name({name})")).unwrap_or_default();
    let objects = [
        format!("<</Type/Catalog/Pages 2 0 R{properties}>>"),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</Properties<</a 5 0 R/b 6 3 R>>>>/Contents 4 0 R>>".into(),
        format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
        "<</Type/OCG/Name(Shown)/Intent[/View/Design]/Usage<</Print<</PrintState/ON>>>>/Custom(keep \\(this\\) >>)>>".into(),
        format!("<</Type/OCG{name}/Intent[/View/Design]/Usage<</Print<</PrintState/ON>>>>/Custom<00ff80>>>"),
    ];
    assemble(&objects, &[2, 0, 0, 0, 0, 3])
}

pub fn unlayered_fixture() -> Vec<u8> {
    let content = "1 0 0 0 k 0 0 50 100 re f";
    assemble(
        &[
            "<</Type/Catalog/Pages 2 0 R>>".into(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<<>>/Contents 4 0 R>>".into(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
        ],
        &[2, 0, 0, 0],
    )
}

pub fn stream_fixture(config: &str) -> Vec<u8> {
    let mut bytes = fixture(config, "Hidden");
    let tail = std::str::from_utf8(&bytes).unwrap();
    let previous: usize = tail.rsplit("startxref").next().unwrap().split_whitespace().next().unwrap().parse().unwrap();
    let offset = bytes.len();
    let mut entry = vec![1];
    entry.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
    entry.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(format!("7 0 obj\n<</Type/XRef/Size 8/Root 1 2 R/Prev {previous}/W[1 4 2]/Index[7 1]/Length 7>>\nstream\n").as_bytes());
    bytes.extend_from_slice(&entry);
    bytes.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{offset}\n%%EOF\n").as_bytes());
    bytes
}

fn assemble(objects: &[String], generations: &[i32]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} {} obj\n{body}\nendobj\n", index + 1, generations[index]).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", objects.len() + 1).as_bytes());
    for (offset, generation) in offsets.iter().zip(generations) {
        bytes.extend_from_slice(format!("{offset:010} {generation:05} n\r\n").as_bytes());
    }
    bytes.extend_from_slice(format!("trailer\n<</Size {}/Root 1 2 R>>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    bytes
}

pub fn document(bytes: Vec<u8>, hidden_sets: &[&[&str]]) -> Document {
    let width = 100.0 * f64::from(u32::try_from(hidden_sets.len()).unwrap());
    let mut doc = Document::new(&NewDocument { width, height: 100.0, facing_pages: false, ..Default::default() });
    let asset = AssetId(doc.alloc());
    doc.add_asset(Asset {
        id: asset,
        name: "layers.pdf".into(),
        mime: "application/pdf".into(),
        data: Arc::new(bytes),
        link: None,
        pixels: None,
        page: 0,
        pdf_crop: Default::default(),
        pdf_box: None,
    });
    for (index, hidden) in hidden_sets.iter().enumerate() {
        let x = 100.0 * f64::from(u32::try_from(index).unwrap());
        let mut item = Item::new(ItemId(doc.alloc()), doc.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0)));
        item.xf = Affine::translate((x, 0.0));
        item.pdf_hidden_layers = hidden.iter().map(|name| (*name).to_owned()).collect();
        item.content = Content::Graphic(Graphic {
            asset,
            size: (100.0, 100.0),
            xf: Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
            layer_options: None,
        });
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    }
    doc
}

pub fn refs(dict: &Dict<'_>, key: &[u8]) -> Vec<ObjectIdentifier> {
    dict.get::<Array<'_>>(key).unwrap().raw_iter().map(|value| value.as_obj_ref().unwrap().into()).collect()
}

pub fn catalog(bytes: &[u8]) -> Vec<(String, bool)> {
    let pdf = Pdf::new(bytes.to_vec()).unwrap();
    let root = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
    let properties = root.get::<Dict<'_>>(b"OCProperties").expect("export must register its embedded OCGs");
    let groups = refs(&properties, b"OCGs");
    let defaults = properties.get::<Dict<'_>>(b"D").unwrap();
    assert!(defaults.get::<hayro_syntax::object::String<'_>>(b"Name").is_some());
    assert_eq!(defaults.get::<Name<'_>>(b"BaseState").unwrap().as_ref(), b"ON");
    assert!(!defaults.contains_key(b"AS"));
    assert_eq!(refs(&defaults, b"Order"), groups);
    let off = refs(&defaults, b"OFF");
    assert!(off.iter().all(|id| groups.contains(id)));
    let mut unique = groups.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), groups.len());
    let mut result = Vec::new();
    for id in groups {
        let group = pdf.xref().get::<Dict<'_>>(id).unwrap();
        assert!(group.contains_key(b"Usage"));
        assert!(group.contains_key(b"Intent"));
        assert!(group.contains_key(b"Custom"));
        let custom = group.get::<hayro_syntax::object::String<'_>>(b"Custom").unwrap();
        assert!([b"keep (this) >>".as_slice(), &[0, 255, 128]].contains(&custom.as_bytes()));
        let intent = group.get::<Array<'_>>(b"Intent").unwrap();
        assert_eq!(intent.iter::<Name<'_>>().map(|name| name.as_ref().to_vec()).collect::<Vec<_>>(), [b"View".to_vec(), b"Design".to_vec()]);
        let usage = group.get::<Dict<'_>>(b"Usage").unwrap().get::<Dict<'_>>(b"Print").unwrap();
        assert_eq!(usage.get::<Name<'_>>(b"PrintState").unwrap().as_ref(), b"ON");
        let name = group.get::<hayro_syntax::object::String<'_>>(b"Name").unwrap();
        result.push((String::from_utf8(name.as_bytes().to_vec()).unwrap(), off.contains(&id)));
    }
    result.sort();
    result
}

pub fn painted(bytes: &[u8], expected: &[bool]) {
    let width = u32::try_from(expected.len()).unwrap() * 50;
    let image = designcraft_render::render_pdf_page(bytes, 0, width.max(100)).unwrap();
    for (index, painted) in expected.iter().enumerate() {
        let image_width = usize::from(image.width());
        let x = (2 * index + 1) * image_width / (2 * expected.len());
        let y = usize::from(image.height()) / 2;
        let pixel = image.data()[y * image_width + x];
        assert_eq!(pixel.a > 200, *painted, "half {index}: {pixel:?}");
    }
}
