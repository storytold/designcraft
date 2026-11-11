//! macOS: an object copy's placeholder text and its PNG picture, on the system clipboard together
//! (#2204). `NSPasteboard` holds several representations at once; writing them through `arboard`
//! (or egui's own clipboard call) one after another would each clear what the previous call wrote,
//! losing either the picture (so another app's Paste gets nothing to show) or the placeholder text
//! (so this app's own Paste could no longer tell its own copy from foreign text). One
//! `clearContents` then both writes, as `vectorcraft`'s clipboard module already does for its own
//! shape copies, avoids that.

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSData, NSString};

/// Writes `text` (the placeholder) and `png` (the picture) together; `Ok(())` only when both
/// writes succeeded, so a partial write (one representation missing) is reported rather than
/// silently left to confuse a later Paste.
pub fn write(text: &str, png: &[u8]) -> Result<(), String> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let wrote_text = pasteboard.setString_forType(&NSString::from_str(text), &NSString::from_str("public.utf8-plain-text"));
        let wrote_png = pasteboard.setData_forType(Some(&NSData::with_bytes(png)), &NSString::from_str("public.png"));
        if wrote_text && wrote_png { Ok(()) } else { Err("the system clipboard refused the write".to_string()) }
    })
}
