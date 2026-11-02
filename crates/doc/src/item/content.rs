use super::graphic::Graphic;
use super::text_frame::TextFrame;
use super::{Arc, Deserialize, Item, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Content {
    #[default]
    Unassigned,
    Text(TextFrame),
    Graphic(Graphic),
    Group {
        items: Vec<Arc<Item>>,
    },
}

/// What kind of spline the item was drawn as (labels in the Layers panel, tool behaviours).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    #[default]
    Rectangle,
    Oval,
    Polygon,
    GraphicLine,
    Path,
    Group,
}
