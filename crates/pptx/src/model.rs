//! Data returned by the DesignCraft-to-Presentation model adapter.

#[derive(Clone, Debug, Default)]
pub struct Slide {
    /// Page size in points.
    pub size: (f64, f64),
    pub name: String,
    /// Model shapes in stacking order.
    pub shapes: Vec<craft_pptx::Shape>,
    /// Media names keyed by picture shape id.
    pub media: Vec<(u32, String, Option<String>)>,
}

#[derive(Clone, Debug)]
pub struct Media {
    pub name: String,
    pub bytes: Vec<u8>,
}
