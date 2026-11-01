use super::fonts::{EmbeddedFont, css_weight_style, font_file, generic_family};
use super::markup::{css_string, slug, valid_xml_char};
use super::*;

/// Class names for `names`, in order: [`slug`]s made unique with a `-2`, `-3`… suffix, so styles
/// whose names slug alike ("Body Text" / "Body-Text", or non-ASCII names) keep their own rules.
fn class_names<'a>(names: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, String> {
    let mut out = BTreeMap::new();
    let mut used = std::collections::BTreeSet::new();
    for name in names {
        let base = slug(name);
        let mut class = base.clone();
        let mut n = 1;
        while !used.insert(class.clone()) {
            n += 1;
            class = format!("{base}-{n}");
        }
        out.insert(name, class);
    }
    out
}

pub(crate) fn para_classes(st: &Styles) -> BTreeMap<&str, String> {
    class_names(st.paragraph.iter().map(|s| s.name.as_str()))
}

pub(crate) fn char_classes(st: &Styles) -> BTreeMap<&str, String> {
    class_names(st.character.iter().map(|s| s.name.as_str()))
}

fn hex(doc: &Document, swatch: &str, tint: f32) -> Option<String> {
    let c = doc.resolve_color(swatch, tint)?;
    let [r, g, b, _] = c.to_rgba8(1.0);
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

pub(crate) fn char_css(doc: &Document, c: &CharProps, base_size: f64, generics: &BTreeMap<String, String>, out: &mut String) {
    let _ = write!(out, "font-family: \"{}\", {}; ", css_string(&c.font_family), generic_family(&c.font_family, generics));
    // The face's own weight, so an embedded Semibold or Light face is the one chosen.
    let (weight, style) = css_weight_style(&c.font_style);
    let _ = write!(out, "font-weight: {weight}; font-style: {style}; ");
    let _ = write!(out, "font-size: {:.3}em; ", c.size / base_size);
    decor_css(c.tracking, c.capitalization, c.underline, c.strikethrough, out);
    if let Some(h) = hex(doc, &c.fill, c.fill_tint) {
        let _ = write!(out, "color: {h}; ");
    }
}

fn para_css(p: &ParaProps, base_size: f64, line_px: f64, out: &mut String) {
    let align = match p.align {
        Align::Left | Align::TowardsSpine => "left",
        Align::Center => "center",
        Align::Right | Align::AwayFromSpine => "right",
        _ => "justify",
    };
    let _ = write!(out, "text-align: {align}; ");
    let em = |v: f64| v / base_size;
    let _ = write!(
        out,
        "margin: {:.3}em {:.3}em {:.3}em {:.3}em; text-indent: {:.3}em; line-height: {:.3}; ",
        em(p.space_before),
        em(p.right_indent),
        em(p.space_after),
        em(p.left_indent),
        em(p.first_line_indent),
        line_px
    );
    out.push_str(if p.hyphenate { "hyphens: auto; -webkit-hyphens: auto; " } else { "hyphens: manual; " });
}

/// The stylesheet for all paragraph and character styles.
pub fn stylesheet(doc: &Document) -> String {
    stylesheet_with(doc, &BTreeMap::new(), &[])
}

/// [`stylesheet`] with the generic families to fall back on and `@font-face` rules for the
/// embedded fonts (packaged as [`font_file`] names them).
pub(crate) fn stylesheet_with(doc: &Document, generics: &BTreeMap<String, String>, fonts: &[EmbeddedFont]) -> String {
    let st: &Styles = &doc.styles;
    let mut faces = String::new();
    for (k, f) in fonts.iter().enumerate() {
        let (path, _) = font_file(k, f);
        let (weight, style) = css_weight_style(&f.style);
        let _ = writeln!(
            faces,
            "@font-face {{ font-family: \"{}\"; src: url(\"{path}\"); font-weight: {weight}; font-style: {style}; }}",
            css_string(&f.family)
        );
    }
    let (_, basic) = st.resolve_para_style(story::BASIC_PARAGRAPH);
    let base = basic.size.max(1.0);
    let mut css = faces;
    css.push_str(
        "body { margin: 0 5%; font-size: 1em; }\nfigure { margin: 1em 0; text-align: center; }\nfigure img { max-width: 100%; }\nh1, h2, h3 { font-size: inherit; margin: 0; }\n",
    );
    let (pc, cc) = (para_classes(st), char_classes(st));
    for ps in &st.paragraph {
        if ps.name == designcraft_doc::NO_PARA_STYLE {
            continue;
        }
        let (pp, cp) = st.resolve_para_style(&ps.name);
        let lead = match cp.leading {
            designcraft_doc::Leading::Auto => pp.auto_leading,
            designcraft_doc::Leading::Points(v) => v / cp.size.max(1.0),
        };
        let mut rule = String::new();
        para_css(&pp, base, lead, &mut rule);
        char_css(doc, &cp, base, generics, &mut rule);
        let (tag, class) = export_mapping(st, &ps.name, false, &pc);
        if let Some(selector) = class_selector(tag, &class) {
            let _ = writeln!(css, "{selector} {{ {rule}}}");
        }
    }
    for cs in &st.character {
        if cs.name == story::NO_CHAR_STYLE {
            continue;
        }
        let mut rule = String::new();
        let c = &cs.chars;
        if let Some(f) = &c.font_style {
            let f = f.to_lowercase();
            if f.contains("bold") {
                rule.push_str("font-weight: bold; ");
            }
            if f.contains("italic") {
                rule.push_str("font-style: italic; ");
            }
        }
        if let Some(fill) = &c.fill
            && let Some(h) = hex(doc, fill, c.fill_tint.unwrap_or(1.0))
        {
            let _ = write!(rule, "color: {h}; ");
        }
        if let Some(sz) = c.size {
            let _ = write!(rule, "font-size: {:.3}em; ", sz / base);
        }
        attrs_decor_css(&st.char_style_attrs(&cs.name), &mut rule);
        let (tag, class) = export_mapping(st, &cs.name, true, &cc);
        if let Some(selector) = class_selector(tag, &class) {
            let _ = writeln!(css, "{selector} {{ {rule}}}");
        }
    }
    css
}

/// Tracking, capitalization and line decorations of a style or override, as CSS declarations.
fn attrs_decor_css(c: &CharAttrs, out: &mut String) {
    decor_css(c.tracking.unwrap_or(0.0), c.capitalization.unwrap_or_default(), c.underline == Some(true), c.strikethrough == Some(true), out);
}

/// Tracking, capitalization and line decorations as CSS declarations; defaults write nothing.
fn decor_css(tracking: f64, caps: designcraft_doc::Capitalization, underline: bool, strikethrough: bool, out: &mut String) {
    if tracking != 0.0 {
        let _ = write!(out, "letter-spacing: {:.3}em; ", tracking / 1000.0);
    }
    match caps {
        designcraft_doc::Capitalization::AllCaps => out.push_str("text-transform: uppercase; "),
        designcraft_doc::Capitalization::SmallCaps | designcraft_doc::Capitalization::OpenTypeAllSmallCaps => {
            out.push_str("font-variant: small-caps; ")
        }
        _ => {}
    }
    // One declaration: a later `text-decoration` would replace the earlier one.
    match (underline, strikethrough) {
        (true, true) => out.push_str("text-decoration: underline line-through; "),
        (true, false) => out.push_str("text-decoration: underline; "),
        (false, true) => out.push_str("text-decoration: line-through; "),
        (false, false) => {}
    }
}

/// Inline CSS for the local override `o` of a run in character style `style`. Tracking, caps and
/// line decorations are resolved against the style (and its Based On chain), so an override that
/// sets only one of underline/strikethrough keeps the other, and one that clears a value the
/// style sets writes the reset.
pub(crate) fn override_css(doc: &Document, style: &str, o: &CharAttrs) -> String {
    let mut s = String::new();
    if let Some(f) = &o.font_style {
        let f = f.to_lowercase();
        if f.contains("bold") {
            s.push_str("font-weight: bold; ");
        }
        if f.contains("italic") {
            s.push_str("font-style: italic; ");
        }
    }
    if let Some(fill) = &o.fill
        && let Some(h) = hex(doc, fill, o.fill_tint.unwrap_or(1.0))
    {
        let _ = write!(s, "color: {h}; ");
    }
    let base = doc.styles.char_style_attrs(style);
    let mut merged = base.clone();
    merged.merge(o);
    let tracking = if o.tracking.is_some() { merged.tracking.unwrap_or(0.0) } else { 0.0 };
    let caps = if o.capitalization.is_some() { merged.capitalization.unwrap_or_default() } else { Default::default() };
    let lines = o.underline.is_some() || o.strikethrough.is_some();
    let (underline, strikethrough) = (lines && merged.underline == Some(true), lines && merged.strikethrough == Some(true));
    if o.tracking.is_some() && tracking == 0.0 && base.tracking.is_some_and(|t| t != 0.0) {
        s.push_str("letter-spacing: 0em; ");
    }
    // The class rule's other caps property would still apply, so clear both before the new mode.
    if o.capitalization.is_some() && caps != base.capitalization.unwrap_or_default() && base.capitalization.is_some_and(|c| c != Default::default()) {
        s.push_str("text-transform: none; font-variant: normal; ");
    }
    decor_css(tracking, caps, underline, strikethrough, &mut s);
    if lines && !underline && !strikethrough && (base.underline == Some(true) || base.strikethrough == Some(true)) {
        s.push_str("text-decoration: none; ");
    }
    s.trim_end().to_string()
}

/// Elements Export Tagging may choose (paragraph or character level).
fn is_tag(t: &str, character: bool) -> bool {
    if character {
        matches!(t, "span" | "em" | "strong" | "i" | "b" | "code" | "sup" | "sub" | "small" | "mark" | "cite" | "q")
    } else {
        matches!(t, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "li" | "figcaption" | "aside" | "div")
    }
}

/// Resolve Export Tagging once for both markup and its stylesheet. Keep the generated,
/// collision-free class when no custom class was supplied, and the existing tag fallback.
pub(crate) fn export_mapping<'a>(st: &'a Styles, name: &str, character: bool, classes: &BTreeMap<&str, String>) -> (&'a str, String) {
    let et = st.export_tag(name, character);
    let tag = et.map(|e| e.tag.as_str()).filter(|t| is_tag(t, character)).unwrap_or(if character { "span" } else { "p" });
    let class = et.map(|e| e.class.clone()).filter(|c| !c.is_empty()).or_else(|| classes.get(name).cloned()).unwrap_or_else(|| slug(name));
    // Match the characters xml_attr actually emits, before escaping either output context.
    let class = class.chars().filter(|c| valid_xml_char(*c) && !('\u{E000}'..='\u{E1FF}').contains(c)).collect();
    (tag, class)
}

/// Each class token must match on the same element. Escape CSS identifier punctuation and
/// leading digits without changing the class written into XHTML.
fn class_selector(tag: &str, class: &str) -> Option<String> {
    let mut tokens = class.split_ascii_whitespace().peekable();
    // A nonempty but whitespace-only custom class has no token to select. Never emit a bare
    // element selector, which would apply this style to unrelated paragraphs or character runs.
    tokens.peek()?;
    let mut selector = tag.to_string();
    for token in tokens {
        selector.push('.');
        for (i, c) in token.chars().enumerate() {
            let leading_digit = c.is_ascii_digit() && (i == 0 || i == 1 && token.starts_with('-'));
            if !leading_digit && token != "-" && (c.is_ascii_alphanumeric() || matches!(c, '-' | '_') || !c.is_ascii()) {
                selector.push(c);
            } else {
                let _ = write!(selector, "\\{:x} ", c as u32);
            }
        }
    }
    Some(selector)
}
