//! Original procedural CMYK TIFFs shared by conversion and public-export contract tests.

pub const INKS: [u8; 8] = [0, 0, 0, 255, 0, 255, 255, 0];
pub const ALPHA: [u8; 2] = [255, 128];

pub fn cmyk_tiff(alpha: Option<u16>, other_inks: bool) -> Vec<u8> {
    use tiff::encoder::colortype::{CMYK8, CMYKA8};
    use tiff::tags::Tag;
    let mut bytes = Vec::new();
    let mut encoder = tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut bytes)).unwrap();
    if let Some(kind) = alpha {
        let mut image = encoder.new_image::<CMYKA8>(2, 1).unwrap();
        image.encoder().write_tag(Tag::ExtraSamples, &[kind][..]).unwrap();
        image.write_data(&[0, 0, 0, 255, ALPHA[0], 0, 255, 255, 0, ALPHA[1]]).unwrap();
    } else {
        let mut image = encoder.new_image::<CMYK8>(2, 1).unwrap();
        if other_inks {
            image.encoder().write_tag(Tag::Unknown(332), 2u16).unwrap();
        }
        image.write_data(&INKS).unwrap();
    }
    bytes
}
