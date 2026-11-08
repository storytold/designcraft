//! Pure browser startup and download metadata policies.

/// Query flags are case-sensitive decoded keys. Values are ignored: `?sample=false`
/// still enables sample startup, and repeated keys have the same presence semantics.
pub(crate) fn has_query_flag(query: &str, flag: &str) -> bool {
    let query = query.strip_prefix('?').unwrap_or(query);
    form_urlencoded::parse(query.as_bytes()).any(|(key, _)| key == flag)
}

/// The extensions a package's document may have.
pub(crate) const PACKAGE_DOC_EXTS: &[&str] = &["designcraft", "idml", "indd", "indt"];

/// Whether `path` names a package's document (not a `._` resource fork).
fn is_package_document_path(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    !name.starts_with("._") && name.rsplit_once('.').is_some_and(|(_, ext)| PACKAGE_DOC_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

/// Whether the files read from a dropped folder (`paths`) go to the app. A folder without a
/// document gives its fonts to the open document, so a drop cut short by the folder limits
/// (`partial`), which may have left its document unread, goes only when it holds one.
pub(crate) fn deliver_dropped_folder<'a>(mut paths: impl Iterator<Item = &'a str>, partial: bool) -> bool {
    !partial || paths.any(is_package_document_path)
}

pub(crate) fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("pdf") => "application/pdf",
        Some("designcraft") => designcraft_format::MIME,
        Some("idml") => "application/vnd.adobe.indesign-idml-package",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{deliver_dropped_folder, has_query_flag, mime_for};

    #[test]
    fn a_dropped_folder_cut_short_goes_only_with_its_document() {
        let fonts = ["Issue/Document Fonts/a.ttf", "Issue/Document Fonts/b.otf"];
        assert!(deliver_dropped_folder(fonts.into_iter(), false), "a whole folder of fonts goes");
        assert!(!deliver_dropped_folder(fonts.into_iter(), true), "a cut-short one may have left its document");
        assert!(!deliver_dropped_folder(["Issue/._Issue.idml", "Issue/Document Fonts/a.ttf"].into_iter(), true), "a resource fork isn't a document");
        assert!(deliver_dropped_folder(["Issue/Issue.IDML", "Issue/Document Fonts/a.ttf"].into_iter(), true));
    }

    #[test]
    fn startup_flags_match_complete_decoded_keys() {
        for flag in ["sample", "webgl"] {
            for query in [format!("?{flag}"), format!("?{flag}=false"), format!("?{flag}=1&{flag}=0"), format!("?other=value&{flag}")] {
                assert!(has_query_flag(&query, flag), "{query}");
            }
            for query in [
                String::new(),
                format!("?note={flag}"),
                format!("?{flag}d=1"),
                format!("?note=not_a_{flag}"),
                "?note=%73ample%77ebgl".to_string(),
                format!("?{flag}+extra=1"),
                format!("?{flag}%26extra=1"),
                format!("?{flag}%3Dextra=1"),
            ] {
                assert!(!has_query_flag(&query, flag), "{query}");
            }
        }
        assert!(has_query_flag("?webgl&sample", "sample"));
        assert!(has_query_flag("?webgl&sample", "webgl"));
        assert!(has_query_flag("?%73ample", "sample"));
        assert!(has_query_flag("?%77ebgl", "webgl"));
        assert!(!has_query_flag("?Sample&WEBGL", "sample"));
        assert!(!has_query_flag("?Sample&WEBGL", "webgl"));
    }

    #[test]
    fn download_mime_matches_native_package_and_existing_exports() {
        for (name, expected) in [
            ("Quarterly.designcraft", designcraft_format::MIME),
            ("日本語.multi.DEsignCraft", designcraft_format::MIME),
            ("page.pdf", "application/pdf"),
            ("page.png", "image/png"),
            ("page.jpg", "image/jpeg"),
            ("page.JPEG", "image/jpeg"),
            ("document.idml", "application/vnd.adobe.indesign-idml-package"),
            ("unknown.bin", "application/octet-stream"),
        ] {
            assert_eq!(mime_for(name), expected, "{name}");
        }
    }
}
