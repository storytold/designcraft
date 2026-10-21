//! What a PageMaker document holds, independent of the file version: the importer reads into
//! this, and `build` turns it into a DesignCraft document.

use std::collections::BTreeMap;

/// A rectangle in points, in spread space: x from the spine (facing pages) or the page's centre,
/// y from the top of the page: (left, top, right, bottom).
pub type Box4 = [f64; 4];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PmDoc {
    /// Page size in points.
    pub width: f64,
    pub height: f64,
    pub facing: bool,
    /// Margins in points: top, bottom, inside, outside.
    pub margins: Option<[f64; 4]>,
    /// The master spread's objects.
    pub masters: Vec<Obj>,
    /// The objects listed with each page (an object may lie on the other page of its spread:
    /// its position says where).
    pub pages: Vec<Vec<Obj>>,
    /// Stories by id.
    pub stories: BTreeMap<u32, PmStory>,
    /// Images embedded in the file.
    pub images: Vec<crate::images::Embedded>,
    /// The colour table, by index.
    pub colors: Vec<PmColor>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Obj {
    pub kind: ObjKind,
    /// Bounds on the page (of the rotated shape, when it is rotated).
    pub rect: Box4,
    /// Rotation, clockwise in degrees, about the bounds' centre; with it, the unrotated size.
    pub rotation: Option<(f64, f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjKind {
    /// A text block: part of story `story`, `block` orders the blocks of one story.
    Text {
        story: u32,
        block: u32,
    },
    /// A line from `a` to `b` (spread space, as `Obj::rect`), `weight` points wide when known.
    Line {
        a: (f64, f64),
        b: (f64, f64),
        weight: Option<f64>,
    },
    Box,
    Oval,
    /// A placed graphic (image, EPS, metafile), with the pixel size of its image when it is one.
    Graphic {
        pixels: Option<(u32, u32)>,
    },
    /// A polygon or another drawn shape, kept as its bounds.
    Shape,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PmStory {
    pub text: String,
    pub chars: Vec<CharRun>,
    pub paras: Vec<ParaRun>,
}

/// A colour of the document's colour table.
#[derive(Clone, Debug, PartialEq)]
pub enum PmColor {
    None,
    Registration,
    Paper,
    Black,
    /// A named process colour: CMYK, 0–1.
    Cmyk(String, [f32; 4]),
    /// A named RGB colour, 0–1.
    Rgb(String, [f32; 3]),
}

/// Formatting of `len` characters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharRun {
    pub len: usize,
    pub font: Option<String>,
    /// Points.
    pub size: Option<f64>,
    /// Points; `None` is auto leading.
    pub leading: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Index into the colour table.
    pub color: Option<usize>,
}

/// Formatting of `len` characters' paragraphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParaRun {
    pub len: usize,
    pub align: Align,
    /// Points: left, first-line and right indents, space before and after.
    pub left: f64,
    pub first: f64,
    pub right: f64,
    pub before: f64,
    pub after: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
