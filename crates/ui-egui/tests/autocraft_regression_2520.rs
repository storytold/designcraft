//! The left toolbar's tool names are translated in German.

use designcraft_tools::TOOL_GROUPS;
use designcraft_ui_egui::i18n::tr;

#[test]
fn every_tool_name_in_the_toolbar_has_a_german_translation() {
    let mut missing = Vec::new();
    let mut count = 0;
    for tool in TOOL_GROUPS.iter().flat_map(|group| group.iter()) {
        count += 1;
        if tr("de", tool.label) == tool.label {
            missing.push(tool.label);
        }
    }
    assert!(count > 0);
    assert!(missing.is_empty(), "untranslated tool names: {missing:?}");
    // Another language's rows are unaffected, and English stays the source text.
    assert_eq!(tr("de", "Hand Tool"), "Handwerkzeug");
    assert_eq!(tr("", "Hand Tool"), "Hand Tool");
    assert_eq!(tr("fr", "Hand Tool"), "Outil Main");
}
