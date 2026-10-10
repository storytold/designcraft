//! The tool catalogue: ids, labels, shortcuts and Tools-panel groups (InDesign's order).

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ToolInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub shortcut: Option<&'static str>,
    pub icon: &'static str,
}

const fn t(id: &'static str, label: &'static str, shortcut: Option<&'static str>, icon: &'static str) -> ToolInfo {
    ToolInfo { id, label, shortcut, icon }
}

/// Tools-panel groups in order (the first tool of each group shows by default). A `None` group
/// marks a separator.
pub const TOOL_GROUPS: &[&[ToolInfo]] = &[
    &[t("selection", "Selection Tool", Some("V"), "tool-selection")],
    &[t("directSelection", "Direct Selection Tool", Some("A"), "tool-direct")],
    &[t("page", "Page Tool", Some("Shift+P"), "tool-page")],
    &[t("gap", "Gap Tool", Some("U"), "tool-gap")],
    &[
        t("contentCollector", "Content Collector Tool", Some("B"), "tool-collector"),
        t("contentPlacer", "Content Placer Tool", Some("B"), "tool-placer"),
    ],
    &[],
    &[
        t("type", "Type Tool", Some("T"), "tool-type"),
        t("typeOnPath", "Type on a Path Tool", Some("Shift+T"), "tool-type-path"),
        t("verticalType", "Vertical Type Tool", None, "tool-type-vertical"),
        t("verticalTypeOnPath", "Vertical Type on a Path Tool", None, "tool-type-path"),
    ],
    &[t("line", "Line Tool", Some("\\"), "tool-line")],
    &[
        t("pen", "Pen Tool", Some("P"), "tool-pen"),
        t("addAnchor", "Add Anchor Point Tool", Some("="), "tool-pen-add"),
        t("deleteAnchor", "Delete Anchor Point Tool", Some("-"), "tool-pen-delete"),
        t("convertDirection", "Convert Direction Point Tool", Some("Shift+C"), "tool-anchor"),
    ],
    &[
        t("pencil", "Pencil Tool", Some("N"), "tool-pencil"),
        t("smooth", "Smooth Tool", None, "tool-smooth"),
        t("erase", "Erase Tool", None, "tool-erase"),
    ],
    &[
        t("rectangleFrame", "Rectangle Frame Tool", Some("F"), "tool-rect-frame"),
        t("ellipseFrame", "Ellipse Frame Tool", None, "tool-ellipse-frame"),
        t("polygonFrame", "Polygon Frame Tool", None, "tool-polygon-frame"),
    ],
    &[
        t("rectangle", "Rectangle Tool", Some("M"), "tool-rect"),
        t("ellipse", "Ellipse Tool", Some("L"), "tool-ellipse"),
        t("polygon", "Polygon Tool", None, "tool-polygon"),
    ],
    &[t("dataGrid", "Data Merge Grid Tool", None, "tool-data-grid")],
    &[],
    &[t("scissors", "Scissors Tool", Some("C"), "tool-scissors")],
    &[
        t("freeTransform", "Free Transform Tool", Some("E"), "tool-free-transform"),
        t("rotate", "Rotate Tool", Some("R"), "tool-rotate"),
        t("scale", "Scale Tool", Some("S"), "tool-scale"),
        t("shear", "Shear Tool", Some("O"), "tool-shear"),
    ],
    &[
        t("gradientSwatch", "Gradient Swatch Tool", Some("G"), "tool-gradient"),
        t("gradientFeather", "Gradient Feather Tool", Some("Shift+G"), "tool-gradient-feather"),
    ],
    &[],
    &[t("note", "Note Tool", None, "tool-note")],
    &[
        t("colorTheme", "Color Theme Tool", Some("Shift+I"), "tool-color-theme"),
        t("eyedropper", "Eyedropper Tool", Some("I"), "tool-eyedropper"),
        t("measure", "Measure Tool", Some("K"), "tool-measure"),
    ],
    &[t("hand", "Hand Tool", Some("H"), "tool-hand")],
    &[t("zoom", "Zoom Tool", Some("Z"), "tool-zoom")],
];

pub fn tool_info(id: &str) -> Option<ToolInfo> {
    TOOL_GROUPS.iter().flat_map(|g| g.iter()).find(|t| t.id == id).copied()
}

/// Tool for a single-key shortcut (`"T"`, `"Shift+P"`).
pub fn tool_for_shortcut(key: &str) -> Option<&'static str> {
    TOOL_GROUPS.iter().flat_map(|g| g.iter()).find(|t| t.shortcut == Some(key)).map(|t| t.id)
}
