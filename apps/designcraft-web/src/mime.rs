//! Blob MIME types for browser downloads (`Services::write` / `download`).

/// The Blob type for a download named `name`, chosen from its extension.
pub(crate) fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("pdf") => "application/pdf",
        // The native format is a ZIP package: use the same type the writer stores in the
        // archive's first entry, so the Blob and the bytes can't disagree.
        Some("designcraft") => designcraft_format::MIME,
        Some("idml") => "application/vnd.adobe.indesign-idml-package",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A native `.designcraft` download is a ZIP package whose first entry is the stored
    /// `mimetype` file: [`designcraft_format::MIME`]. The Blob type must say the same (#94).
    #[test]
    fn native_document_blob_matches_format_mime() {
        assert_eq!(mime_for("doc.designcraft"), designcraft_format::MIME);
        assert_eq!(mime_for("DOC.DESIGNCRAFT"), designcraft_format::MIME);
        assert_ne!(mime_for("doc.designcraft"), "application/json");
    }

    /// The other mappings stay as they are, including the fallback for unknown extensions.
    #[test]
    fn other_extensions_unchanged() {
        assert_eq!(mime_for("a.png"), "image/png");
        assert_eq!(mime_for("a.jpeg"), "image/jpeg");
        assert_eq!(mime_for("a.jpg"), "image/jpeg");
        assert_eq!(mime_for("a.pdf"), "application/pdf");
        assert_eq!(mime_for("a.idml"), "application/vnd.adobe.indesign-idml-package");
        assert_eq!(mime_for("a.xyz"), "application/octet-stream");
        assert_eq!(mime_for("no-extension"), "application/octet-stream");
    }
}
