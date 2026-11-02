use super::face::FontFace;
#[cfg(not(target_arch = "wasm32"))]
use super::system::CatalogEntry;
use super::{Arc, MetadataProvider, StringId};

pub(super) fn aliases(font: &skrifa::FontRef<'_>, ids: &[StringId]) -> Vec<String> {
    let mut names: Vec<String> =
        ids.iter().flat_map(|id| font.localized_strings(*id)).take(256).map(|name| name.to_string()).filter(|name| !name.trim().is_empty()).collect();
    names.sort();
    names.dedup();
    names
}

pub(super) fn name(font: &skrifa::FontRef<'_>, ids: &[StringId]) -> Option<String> {
    ids.iter().find_map(|id| font.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
}

/// A face's family and style names (the default instance's style for a variable font).
/// Is a face of `family` and `style` among `faces`?
pub(super) fn has_face(faces: &[Arc<FontFace>], family: &str, style: &str) -> bool {
    faces.iter().any(|f| f.family.eq_ignore_ascii_case(family) && f.style.eq_ignore_ascii_case(style))
}

/// A face's WWS family and style names (name IDs 21 and 22), if it has a WWS family and it differs
/// from `family`/`style`.
pub(super) fn wws_names(f: &skrifa::FontRef<'_>, family: &str, style: &str) -> Option<(String, String)> {
    let wf = name(f, &[StringId::WWS_FAMILY_NAME])?;
    let ws = name(f, &[StringId::WWS_SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
    (!wf.eq_ignore_ascii_case(family) || !ws.eq_ignore_ascii_case(style)).then_some((wf, ws))
}

/// The family and style a face is known by: its typographic names, or its WWS names (if it has
/// them) when `by_wws`.
pub(super) fn known_as<'a>(family: &'a str, style: &'a str, wws: Option<&'a (String, String)>, by_wws: bool) -> Option<(&'a str, &'a str)> {
    if by_wws { wws.map(|w| (w.0.as_str(), w.1.as_str())) } else { Some((family, style)) }
}

/// Is `family` the WWS family name of `f`?
pub(super) fn has_wws_family(f: &FontFace, family: &str) -> bool {
    f.wws.as_ref().is_some_and(|w| w.0.eq_ignore_ascii_case(family))
}

/// Is `family` the typographic or the WWS family name of `c`?
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn in_catalog_family(c: &CatalogEntry, family: &str) -> bool {
    c.family.eq_ignore_ascii_case(family) || c.wws.as_ref().is_some_and(|w| w.0.eq_ignore_ascii_case(family))
}

pub(super) fn face_names(f: &skrifa::FontRef<'_>) -> Option<(String, String)> {
    let family = name(f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME])?;
    let style = name(f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
    Some((family, style))
}

/// `family` without what layout apps add to a family name, if it has any: InDesign's `$ID/`
/// prefix (`$ID/Arial` → `Arial`) and the font-format suffix appended when a family is installed
/// in several formats (`Minion Pro (OTF)` → `Minion Pro`).
fn plain_family(family: &str) -> Option<&str> {
    let unprefixed = family.trim_start().strip_prefix("$ID/").map(str::trim).filter(|f| !f.is_empty());
    without_format_suffix(unprefixed.unwrap_or(family)).or(unprefixed)
}

/// `family` without the font-format suffix layout apps append when a family is installed in
/// several formats (`Minion Pro (OTF)` → `Minion Pro`), if it has one.
fn without_format_suffix(family: &str) -> Option<&str> {
    const FORMATS: [&str; 5] = ["OTF", "TT", "TTF", "TTC", "T1"];
    let (base, suffix) = family.trim_end().strip_suffix(')')?.rsplit_once('(')?;
    let base = base.trim_end();
    (!base.is_empty() && FORMATS.iter().any(|f| f.eq_ignore_ascii_case(suffix.trim()))).then_some(base)
}

/// The names `family` is looked up under when no font has it as written: without surrounding
/// spaces, then without an `$ID/` prefix or a format suffix (see [`plain_family`]).
pub(super) fn alternate_names(family: &str) -> impl Iterator<Item = &str> {
    let trimmed = family.trim();
    (trimmed != family && !trimmed.is_empty()).then_some(trimmed).into_iter().chain(plain_family(trimmed))
}

/// The named style part of a style with axis settings (`Bold {wght:650}` → `Bold`).
pub fn base_style(style: &str) -> &str {
    style.split('{').next().unwrap_or(style).trim()
}

/// Do two style names name the same face? Spacing, punctuation and case don't count, as when a face
/// is looked up (InDesign documents can carry `Regular ` with a trailing space).
pub fn same_style(a: &str, b: &str) -> bool {
    norm(a) == norm(b)
}

pub(super) fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Weight implied by a style name.
pub(super) fn style_weight(style: &str) -> f32 {
    let s = norm(style);
    const TABLE: &[(&str, f32)] = &[
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("semibold", 600.0),
        ("demibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("hairline", 100.0),
        ("thin", 100.0),
        ("light", 300.0),
        ("medium", 500.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, w)| *w).unwrap_or(400.0)
}

pub(super) fn style_italic(style: &str) -> bool {
    let s = norm(style);
    s.contains("italic") || s.contains("oblique") || s == "it"
}
