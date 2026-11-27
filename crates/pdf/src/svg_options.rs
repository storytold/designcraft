//! DesignCraft font selection for the PDF writer's SVG parser.

use std::sync::{Arc, OnceLock};

use craft_pdf::write::svg::usvg;

fn svg_fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for f in designcraft_fonts::bundled() {
            db.load_font_data(f.to_vec());
        }
        // Japanese text in SVGs: the craft-fonts faces (none without CRAFT_FONTS_DIR).
        for f in designcraft_fonts::japanese_document_fonts() {
            db.load_font_data(f.bytes.to_vec());
        }
        db.set_serif_family("Source Serif 4");
        db.set_sans_serif_family("Source Sans 3");
        db.set_monospace_family("JetBrains Mono");
        Arc::new(db)
    })
    .clone()
}

/// SVG options with DesignCraft's font database and family/PostScript fallback policy.
/// The PDF adapter uses these same font choices while owning its separate parser.
pub(crate) fn svg_options_for_pdf() -> usvg::Options<'static> {
    usvg::Options {
        fontdb: svg_fontdb(),
        font_family: "Source Sans 3".into(),
        font_resolver: usvg::FontResolver { select_font: select_font(), select_fallback: select_fallback() },
        ..Default::default()
    }
}

/// A family the SVG names that isn't bundled comes from the shared fonts (installed ones too),
/// as it would in a text frame, and matches in any case, as it does there. A name that is no
/// family's but a face's PostScript name (as Illustrator's SVG export writes them:
/// `font-family="'MyriadPro-Regular'"`) sets the text in that face, whatever font-weight and
/// font-style say: the name already picks the weight and style, and a family's other faces would
/// contradict it. A name that is a face's legacy family name (name ID 1) or full name (ID 4), as
/// Windows names faces outside the regular/bold/italic quartet (`'Segoe UI Semibold'`, whose
/// typographic family is `Segoe UI`), sets the text in that face (the closest to font-weight and
/// font-style when the legacy family has several). Families listed before it come first.
fn select_font() -> usvg::FontSelectionFn<'static> {
    use usvg::fontdb::{Family, Query, Stretch, Style, Weight};
    Box::new(move |font, db| {
        let fonts = designcraft_fonts::FontDb::global();
        let spelled = |db: &usvg::fontdb::Database, name: &str| {
            db.faces().flat_map(|f| &f.families).find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(n, _)| n.clone())
        };
        // (index in the font-family list, face) of the faces a name names by its legacy names.
        let mut legacy: Vec<(usize, usvg::fontdb::ID)> = Vec::new();
        for (i, family) in font.families().iter().enumerate() {
            if let usvg::FontFamily::Named(name) = family
                && spelled(db, name).is_none()
            {
                if fonts.has_family(name) {
                    for style in fonts.styles(name) {
                        let face = fonts.face(name, &style);
                        let named = has_legacy_name(&face, name);
                        if let Some(id) = add_face(db, face)
                            && named
                        {
                            legacy.push((i, id));
                        }
                    }
                } else if postscript_face(db, name).is_none() {
                    if let Some((family, style)) = fonts.postscript_face(name) {
                        add_face(db, fonts.face(&family, &style));
                    } else {
                        // A full name: the family is the name without its last words ("Bold").
                        let mut base = name.as_str();
                        for _ in 0..3 {
                            let Some((head, _)) = base.trim_end().rsplit_once(' ') else { break };
                            base = head;
                            if fonts.has_family(base) {
                                for style in fonts.styles(base) {
                                    let face = fonts.face(base, &style);
                                    if has_legacy_name(&face, name)
                                        && let Some(id) = add_face(db, face)
                                    {
                                        legacy.push((i, id));
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
        let want_style = match font.style() {
            usvg::FontStyle::Normal => Style::Normal,
            usvg::FontStyle::Italic => Style::Italic,
            usvg::FontStyle::Oblique => Style::Oblique,
        };
        for (i, family) in font.families().iter().enumerate() {
            match family {
                usvg::FontFamily::Named(name) if spelled(db, name).is_none() => {
                    if let Some(id) = postscript_face(db, name) {
                        return Some(id);
                    }
                    let closest = legacy
                        .iter()
                        .filter(|(at, _)| *at == i)
                        .filter_map(|(_, id)| db.face(*id))
                        .min_by_key(|f| (f.style != want_style, f.weight.0.abs_diff(font.weight())))
                        .map(|f| f.id);
                    if closest.is_some() {
                        return closest;
                    }
                }
                // A family (or generic family) that matches: the query below.
                _ => break,
            }
        }
        // usvg's default query, but with each named family spelled as the database spells it:
        // fontdb compares names exactly, so "DeJaVu SaNs" would quietly fall to the serif.
        let names: Vec<Option<String>> = font
            .families()
            .iter()
            .map(|f| match f {
                usvg::FontFamily::Named(name) => Some(spelled(db, name).unwrap_or_else(|| name.clone())),
                _ => None,
            })
            .collect();
        let mut families: Vec<Family> = font
            .families()
            .iter()
            .zip(&names)
            .map(|(f, name)| match f {
                usvg::FontFamily::Serif => Family::Serif,
                usvg::FontFamily::SansSerif => Family::SansSerif,
                usvg::FontFamily::Cursive => Family::Cursive,
                usvg::FontFamily::Fantasy => Family::Fantasy,
                usvg::FontFamily::Monospace => Family::Monospace,
                usvg::FontFamily::Named(_) => Family::Name(name.as_deref().unwrap_or_default()),
            })
            .collect();
        families.push(Family::Serif);
        let stretch = match font.stretch() {
            usvg::FontStretch::UltraCondensed => Stretch::UltraCondensed,
            usvg::FontStretch::ExtraCondensed => Stretch::ExtraCondensed,
            usvg::FontStretch::Condensed => Stretch::Condensed,
            usvg::FontStretch::SemiCondensed => Stretch::SemiCondensed,
            usvg::FontStretch::Normal => Stretch::Normal,
            usvg::FontStretch::SemiExpanded => Stretch::SemiExpanded,
            usvg::FontStretch::Expanded => Stretch::Expanded,
            usvg::FontStretch::ExtraExpanded => Stretch::ExtraExpanded,
            usvg::FontStretch::UltraExpanded => Stretch::UltraExpanded,
        };
        db.query(&Query { families: &families, weight: Weight(font.weight()), stretch, style: want_style })
    })
}

/// Is `name` (ignoring case) a legacy family name (name ID 1) or full name (ID 4) of `face`?
fn has_legacy_name(face: &designcraft_fonts::FontFace, name: &str) -> bool {
    use skrifa::MetadataProvider;
    use skrifa::string::StringId;
    let Some(f) = face.skrifa() else { return false };
    [StringId::FAMILY_NAME, StringId::FULL_NAME]
        .into_iter()
        .flat_map(|id| f.localized_strings(id))
        .take(256)
        .any(|s| s.chars().map(|c| c.to_ascii_lowercase()).eq(name.chars().map(|c| c.to_ascii_lowercase())))
}

/// The face in the SVG's font database with PostScript name `name` (ignoring case).
fn postscript_face(db: &usvg::fontdb::Database, name: &str) -> Option<usvg::fontdb::ID> {
    db.faces().find(|f| f.post_script_name.eq_ignore_ascii_case(name)).map(|f| f.id)
}

/// Characters the chosen fonts lack (Korean, Arabic…) fall back to a shared face that has them.
fn select_fallback() -> usvg::FallbackSelectionFn<'static> {
    let loaded = usvg::FontResolver::default_fallback_selector();
    Box::new(move |c, exclude, db| {
        loaded(c, exclude, db).or_else(|| {
            let id = add_face(db, designcraft_fonts::FontDb::global().fallback_for(c, 0, None)?)?;
            // usvg asks again until we run out: a face it already tried ends the search.
            (!exclude.contains(&id)).then_some(id)
        })
    })
}

/// A shared face's bytes, handed to usvg without a copy.
struct FaceBytes(Arc<designcraft_fonts::FontFace>);

impl AsRef<[u8]> for FaceBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.data()
    }
}

/// Add `face` to the SVG's font database (once) and return its id there.
fn add_face(db: &mut Arc<usvg::fontdb::Database>, face: Arc<designcraft_fonts::FontFace>) -> Option<usvg::fontdb::ID> {
    let same = |f: &usvg::fontdb::FaceInfo| {
        f.index == face.index()
            && matches!(&f.source, usvg::fontdb::Source::Binary(b) if std::ptr::eq(b.as_ref().as_ref().as_ptr(), face.data().as_ptr()))
    };
    if let Some(f) = db.faces().find(|f| same(f)) {
        return Some(f.id);
    }
    let index = face.index();
    let ids = Arc::make_mut(db).load_font_source(usvg::fontdb::Source::Binary(Arc::new(FaceBytes(face))));
    // A collection loads all its faces (skipping any it can't read): pick ours by its index.
    ids.into_iter().find(|id| db.face(*id).is_some_and(|f| f.index == index))
}
