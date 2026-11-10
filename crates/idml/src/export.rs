//! Document → IDML package.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;

use designcraft_color::{Color, ColorType, GradientKind, SwatchValue, swatch};
use designcraft_doc::{
    CharAttrs, CharFormat, ColumnsKind, Content, Document, Item, ListType, PageSide, ParaAttrs, ParaFormat, Rule, Shape, SpanColumns, Spread, Story,
    TextFrameOptions, story as st,
};
use designcraft_geom::{Affine, PathData};

use crate::names::{self, CHAR_BUILTINS, OBJECT_BUILTINS, PARA_BUILTINS, escape_id, num, pt};
use crate::xml::{El, Node, document, document_with_pi};
use crate::{DOM_VERSION, MIMETYPE, base64_encode};

mod common;
mod designmap;
mod fonts_prefs;
mod graphics;
mod item;
mod spread;
mod story;
mod styles;
mod table;
mod text_attrs;

use common::pkg_root;
pub(crate) use common::uid;
use designmap::{CONTAINER, metadata_xmp};
pub(crate) use item::corner_names;
#[cfg(test)]
pub(crate) use item::path_to_uri;

const PKG_NS: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
const NO_PARA_ID: &str = "ParagraphStyle/$ID/[No paragraph style]";
const TOC_STYLE_ID: &str = "TOCStyle/Table of Contents";
const TOC_STYLE_NAME: &str = "Table of Contents";
const NO_CHAR_ID: &str = "CharacterStyle/$ID/[No character style]";

/// Export options.
#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// Embed image data in the package (`<Contents>`). When false, images with a known link path
    /// are written as links only.
    pub embed_images: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions { embed_images: true }
    }
}

/// Export a document as an IDML package (ZIP bytes).
///
/// IDML has one sort key per index topic path. If native references disagree, the first
/// reference serialized for that path supplies its key, including an empty/default key.
/// Later references cannot overwrite it; no synthetic topic names or sort keys are created.
/// Consistent references retain their keys. Conflicting per-reference keys cannot all survive
/// this topic-level representation. This policy also applies to [`export_idml_with`].
pub fn export_idml(doc: &Document) -> Vec<u8> {
    export_idml_with(doc, &ExportOptions::default())
}

/// Export with options, using the index sort-key conflict policy of [`export_idml`].
pub fn export_idml_with(doc: &Document, opts: &ExportOptions) -> Vec<u8> {
    let mut ex = Ex::new(doc, opts);
    let parts = ex.build();
    zip_parts(&parts)
}

fn zip_parts(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut write = || -> zip::result::ZipResult<()> {
        w.start_file("mimetype", stored)?;
        w.write_all(MIMETYPE.as_bytes())?;
        for (name, data) in parts {
            w.start_file(name.as_str(), deflated)?;
            w.write_all(data)?;
        }
        Ok(())
    };
    // Writing into memory cannot fail except on internal errors; return what we have then.
    let _ = write();
    w.finish().map(|c| c.into_inner()).unwrap_or_default()
}

struct Ex<'a> {
    d: &'a Document,
    opts: &'a ExportOptions,
    next: u64,
    swatch_ids: HashMap<String, String>,
    fonts: BTreeMap<String, BTreeSet<String>>,
    /// Item id → (prev frame, next frame) for threaded text frames.
    threads: HashMap<u64, (Option<u64>, Option<u64>)>,
    /// Cross-reference sources written so far: (Self, target anchor).
    xref_sources: Vec<(String, u64)>,
    /// `HyperlinkTextSource`s written into the stories: (Self, index in `Document::hyperlinks`).
    /// A link whose text crosses a paragraph gets one source per paragraph, since the element
    /// nests inside a `ParagraphStyleRange` (#1446).
    link_text_sources: Vec<(String, usize)>,
    /// Index topic path → first serialized sort key; See / See also cross-references.
    index_topics: BTreeMap<Vec<String>, String>,
    index_xrefs: Vec<(Vec<String>, &'static str, String)>,
}

impl<'a> Ex<'a> {
    fn new(d: &'a Document, opts: &'a ExportOptions) -> Self {
        let mut ex = Ex {
            d,
            opts,
            next: d.next_id.saturating_add(0x100),
            swatch_ids: HashMap::new(),
            fonts: BTreeMap::new(),
            threads: HashMap::new(),
            xref_sources: Vec::new(),
            link_text_sources: Vec::new(),
            index_topics: BTreeMap::new(),
            index_xrefs: Vec::new(),
        };
        ex.assign_swatch_ids();
        for s in d.stories.values() {
            for (i, f) in s.frames.iter().enumerate() {
                let prev = if i > 0 { Some(s.frames[i - 1].0) } else { None };
                let next = s.frames.get(i + 1).map(|f| f.0);
                ex.threads.insert(f.0, (prev, next));
            }
        }
        ex
    }

    fn fresh(&mut self) -> String {
        self.next += 1;
        uid(self.next)
    }

    fn assign_swatch_ids(&mut self) {
        let mut used: HashSet<String> = ["Swatch/None", "Color/Paper", "Color/Black", "Color/Registration"].iter().map(ToString::to_string).collect();
        self.swatch_ids.insert(swatch::NONE.into(), "Swatch/None".into());
        self.swatch_ids.insert(swatch::PAPER.into(), "Color/Paper".into());
        self.swatch_ids.insert(swatch::BLACK.into(), "Color/Black".into());
        self.swatch_ids.insert(swatch::REGISTRATION.into(), "Color/Registration".into());
        for s in &self.d.swatches {
            if self.swatch_ids.contains_key(&s.name) {
                continue;
            }
            let kind = match s.value {
                SwatchValue::Tint { .. } => "Tint",
                SwatchValue::Gradient { .. } => "Gradient",
                SwatchValue::None => "Swatch",
                _ => "Color",
            };
            let base = format!("{kind}/{}", escape_id(&s.name));
            let mut id = base.clone();
            let mut n = 2;
            while used.contains(&id) {
                id = format!("{base} {n}");
                n += 1;
            }
            used.insert(id.clone());
            self.swatch_ids.insert(s.name.clone(), id);
        }
    }

    fn sw(&self, name: &str) -> String {
        self.swatch_ids.get(name).cloned().unwrap_or_else(|| "Swatch/None".into())
    }

    fn build(&mut self) -> Vec<(String, Vec<u8>)> {
        let d = self.d;
        let mut parts = Vec::new();
        // Stories first (they register fonts).
        let mut story_parts = Vec::new();
        for s in d.stories.values() {
            let name = format!("Stories/Story_{}.xml", uid(s.id.0));
            let el = self.story_el(s);
            story_parts.push((name, document(&pkg_root("Story").child(el))));
        }
        let styles = self.styles_part();
        let mut masters = Vec::new();
        for sp in &d.parents {
            let name = format!("MasterSpreads/MasterSpread_{}.xml", uid(sp.id.0));
            let el = self.spread_el(sp, true);
            masters.push((name, document(&pkg_root("MasterSpread").child(el))));
        }
        let mut spreads = Vec::new();
        for (si, sp) in d.spreads.iter().enumerate() {
            let name = format!("Spreads/Spread_{}.xml", uid(sp.id.0));
            let el = self.spread_el_doc(sp, si);
            spreads.push((name, document(&pkg_root("Spread").child(el))));
        }
        let graphic = self.graphic_part();
        let fonts = self.fonts_part();
        let prefs = self.prefs_part();
        let designmap = self.designmap(&masters, &spreads, &story_parts);

        parts.push(("META-INF/container.xml".to_string(), CONTAINER.as_bytes().to_vec()));
        parts.push(("META-INF/metadata.xml".to_string(), metadata_xmp(d).into_bytes()));
        parts.push(("designmap.xml".to_string(), designmap));
        parts.push(("Resources/Graphic.xml".to_string(), graphic));
        parts.push(("Resources/Fonts.xml".to_string(), fonts));
        parts.push(("Resources/Styles.xml".to_string(), styles));
        parts.push(("Resources/Preferences.xml".to_string(), prefs));
        parts.extend(masters);
        parts.extend(spreads);
        parts.extend(story_parts);
        parts
    }
}
