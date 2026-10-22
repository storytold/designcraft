//! macOS: the font files CoreText's font manager has available, which the system font scan reads
//! with the font folders.
//!
//! Font services and font managers (Adobe Fonts through Creative Cloud, RightFont, …) register
//! fonts with CoreText from files in their own folders, outside the font folders
//! (<https://developer.apple.com/documentation/coretext/ctfontmanagerregisterfontsforurl(_:_:_:)>).
//! CoreText's collection of available fonts holds them
//! (<https://developer.apple.com/documentation/coretext/ctfontcollectioncreatefromavailablefonts(_:)>),
//! so the scan reads their files in place (#261, #327).

use std::path::PathBuf;

/// The most font descriptors read from the collection (a system has a few thousand faces).
const MAX_FONTS: usize = 1 << 16;

/// The files of the fonts available to the process as CoreText lists them now, fonts registered
/// since the last call included. A file holding several faces is listed once per face.
pub(crate) fn available_font_files() -> Vec<PathBuf> {
    // `core-text` asserts that each attribute has the type CoreText documents; should one not, the
    // scan goes on with the font folders alone.
    std::panic::catch_unwind(|| {
        let Some(descriptors) = core_text::font_collection::create_for_all_families().get_descriptors() else {
            return Vec::new();
        };
        descriptors.iter().take(MAX_FONTS).filter_map(|d| d.font_path()).collect()
    })
    .unwrap_or_default()
}
