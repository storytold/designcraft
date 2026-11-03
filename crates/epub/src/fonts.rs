use super::{BTreeMap, Document, Styles};

/// A font file packaged with an EPUB: the face of `family` in `style` (`Bold Italic`…).
#[derive(Clone, Debug)]
pub struct EmbeddedFont {
    pub family: String,
    pub style: String,
    pub data: Vec<u8>,
}

/// The `(family, style)` pairs the stylesheet names, one each, in style order: the fonts an
/// export can embed.
pub fn stylesheet_fonts(doc: &Document) -> Vec<(String, String)> {
    let st: &Styles = &doc.styles;
    let mut out: Vec<(String, String)> = Vec::new();
    for ps in &st.paragraph {
        if ps.name == designcraft_doc::NO_PARA_STYLE {
            continue;
        }
        let (_, cp) = st.resolve_para_style(&ps.name);
        let f = (cp.font_family.clone(), cp.font_style.clone());
        if !f.0.is_empty() && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// The CSS generic family for `family`: the known one, else a guess from the name
/// (`… Mono`/`Courier`/`… Code` → monospace, `… Sans`/`Gothic`/`Grotesk`/`Helvetica`/`Arial`/
/// `Verdana` → sans-serif), else serif.
pub(crate) fn generic_family<'a>(family: &str, known: &'a BTreeMap<String, String>) -> &'a str {
    const GENERIC: [&str; 6] = ["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui"];
    if let Some(g) = known.get(family).and_then(|g| GENERIC.iter().find(|x| **x == g.as_str())) {
        return g;
    }
    let f = family.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| f.contains(w));
    if has(&["mono", "courier", " code", "consol"]) {
        "monospace"
    } else if has(&["sans", "gothic", "grotesk", "grotesque", "helvetica", "arial", "verdana"]) {
        "sans-serif"
    } else {
        "serif"
    }
}

/// CSS weight and style of a style name (`Semibold Italic` → 600, italic).
pub(crate) fn css_weight_style(style: &str) -> (u16, &'static str) {
    let s = style.to_lowercase().replace([' ', '-'], "");
    let weight = [
        ("thin", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("light", 300),
        ("medium", 500),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
    ]
    .iter()
    .find(|(n, _)| s.contains(n))
    .map_or(400, |(_, w)| *w);
    (weight, if s.contains("italic") || s.contains("oblique") { "italic" } else { "normal" })
}

/// Package path and media type of an embedded font (`fonts/{k}-{family}-{style}.{ext}`).
pub(crate) fn font_file(k: usize, f: &EmbeddedFont) -> (String, &'static str) {
    let (ext, mime) = match f.data.get(..4) {
        Some(b"OTTO") => ("otf", "font/otf"),
        Some(b"wOFF") => ("woff", "font/woff"),
        Some(b"wOF2") => ("woff2", "font/woff2"),
        _ => ("ttf", "font/ttf"),
    };
    let name: String =
        format!("{}-{}", f.family, f.style).chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let name = name.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    (format!("fonts/{k}-{name}.{ext}"), mime)
}
