//! Data returned by the DesignCraft-to-PresentationML model adapter.

#[derive(Clone, Debug, Default)]
pub struct Slide {
    /// Page size in points.
    pub size: (f64, f64),
    pub name: String,
    /// Model shapes in stacking order.
    pub shapes: Vec<craft_pptx::Shape>,
    /// Media part names keyed by the id of a picture shape.
    pub media: Vec<(u32, String, Option<String>)>,
}

#[derive(Clone, Debug)]
pub struct Media {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Rel {
    pub id: String,
    pub kind: &'static str,
    pub target: String,
}
