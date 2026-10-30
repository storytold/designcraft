//! IDML package → Document.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use designcraft_color::cms::lab;
use designcraft_color::{Color, ColorType, Gradient, GradientKind, GradientStop, Swatch, SwatchValue, swatch};
use designcraft_doc::{
    Asset, AssetId, BaselineGrid, CellRange, CharAttrs, CharFormat, CharRun, CharacterStyle, Columns, ColumnsKind, Content, DirectionalFeather,
    DocSettings, Document, DropShadow, Effects, Fill, GradientFeather, GridAlign, GridRelative, Guide, Item, ItemId, Kerning, LAYER_COLORS, Layer,
    LayerId, ListType, Margins, ObjectStyle, Orientation, Page, PageId, PageSide, ParaAttrs, ParaFormat, ParagraphStyle, ParentInfo, RowHeightMode,
    Rule, Section, Shape, SpanColumns, Spread, SpreadId, Story, StoryId, Stroke, Styles, TabStop, Table, TextFrame, TextFrameOptions, TextWrap,
    story as st,
};
use designcraft_geom::corners::{Corner, CornerOptions};
use designcraft_geom::{Affine, Anchor, PathData, Point, Rect, SubPath};

use crate::names::{self, CHAR_BUILTINS, OBJECT_BUILTINS, PARA_BUILTINS, unescape_id};
use crate::xml::{El, Node, parse};
use crate::{IdmlError, MIMETYPE, Result, base64_decode, sniff_image};

mod document;
mod frame;
mod item;
mod object_style;
mod package;
mod spread;
mod story;
mod styles;
mod swatches;
mod table;
mod text_attrs;
mod values;

use document::IndexTopic;
use frame::margins_of;
use package::Package;
#[cfg(test)]
pub(crate) use package::{percent_decode, uri_to_path};
use text_attrs::mojikumi_row;
use values::{nums, xmp_date};

/// Import an IDML package without reading its linked images from disk: this crate never touches
/// the file system, so a caller that wants the links brings its own reader
/// ([`import_idml_with`]). The engine passes one that checks the automation roots.
pub fn import_idml(bytes: &[u8]) -> Result<Document> {
    import_idml_with(bytes, &|_| None)
}

/// Import with a custom reader for linked files (`path` → bytes).
pub fn import_idml_with(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Document> {
    import_idml_report(bytes, read_link).map(|r| r.document)
}

/// An imported IDML document and what the import could not bring in.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    /// One line per problem that did not stop the import (a part listed in `designmap.xml` but
    /// absent from the package, …).
    pub warnings: Vec<String>,
}

/// Import with a custom reader for linked files, reporting the problems that did not stop it.
pub fn import_idml_report(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Imported> {
    import_idml_report_with_previews(bytes, read_link, &HashMap::new())
}

/// [`import_idml_report`], with stand-in images for graphics whose file neither the package nor
/// `read_link` provides: `previews` maps a graphic's `Self` to an image (an InDesign document's
/// screen preview of a linked file). The graphic keeps its link, so relinking replaces it.
pub fn import_idml_report_with_previews(
    bytes: &[u8],
    read_link: &dyn Fn(&str) -> Option<Vec<u8>>,
    previews: &HashMap<String, Arc<Vec<u8>>>,
) -> Result<Imported> {
    let mut pkg = Package::open(bytes)?;
    if let Some(m) = pkg.part("mimetype")?
        && String::from_utf8_lossy(&m).trim() != MIMETYPE
    {
        return Err(IdmlError::NotIdml(format!("unexpected mimetype `{}`", String::from_utf8_lossy(&m).trim())));
    }
    let dm = pkg.part("designmap.xml")?.ok_or_else(|| IdmlError::NotIdml("missing designmap.xml".into()))?;
    let root = parse(&dm).map_err(|msg| IdmlError::Part { part: "designmap.xml".into(), msg })?;
    if root.local() != "Document" {
        return Err(IdmlError::NotIdml(format!("designmap root is <{}>", root.name)));
    }
    // Flatten includes: every `idPkg:*` child is replaced by the children of its part's root.
    let mut top: Vec<El> = Vec::new();
    let mut warnings = Vec::new();
    for c in root.elements() {
        if c.name.starts_with("idPkg:") {
            let Some(src) = c.get("src") else { continue };
            let Some(data) = pkg.part(src)? else {
                warnings.push(format!("{src}: listed in designmap.xml but missing from the package; its contents were not imported"));
                continue;
            };
            let el = parse(&data).map_err(|msg| IdmlError::Part { part: src.into(), msg })?;
            top.extend(el.elements().cloned());
        } else {
            top.push(c.clone());
        }
    }
    let mut im = Importer::new(read_link, previews);
    if let Some(m) = pkg.part("META-INF/metadata.xml")? {
        let m = String::from_utf8_lossy(&m);
        im.created = xmp_date(&m, "xmp:CreateDate");
        im.modified = xmp_date(&m, "xmp:ModifyDate");
    }
    im.run(&root, &top)?;
    if im.repaired_tab_lists > 0 {
        warnings.push(format!(
            "tab lists repaired to fit the limits (at most {} stops, finite positions up to {} in, leaders of at most {} characters, one stop per position): {}",
            designcraft_doc::MAX_TAB_STOPS,
            designcraft_doc::MAX_TAB_POSITION / 72.0,
            designcraft_doc::MAX_TAB_LEADER,
            im.repaired_tab_lists,
        ));
    }
    warnings.append(&mut im.warnings);
    let mut document = im.finish(&root).map_err(|e| match e {
        // Without its spread parts there is nothing to open: say which parts were missing.
        IdmlError::Invalid(msg) if !warnings.is_empty() => IdmlError::Invalid(format!("{msg} ({})", warnings.join("; "))),
        e => e,
    })?;
    warnings.extend(document.fix_parent_bases());
    document.check().map_err(|e| IdmlError::Invalid(e.to_string()))?;
    Ok(Imported { document, warnings })
}

struct ItemCtx {
    /// Self → (story self, prev self, next self)
    threads: Vec<(ItemId, String, String, String)>,
}

struct Importer<'r> {
    read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>,
    /// Graphic `Self` → stand-in image for a linked file that can't be read.
    previews: &'r HashMap<String, Arc<Vec<u8>>>,
    next_id: u64,
    /// Cells every table in this import has reserved. One table is capped, but a file chooses
    /// how many tables it has, so the cap has to be document-wide as well (noted in review).
    table_cells: usize,
    /// Tables whose size the file claimed and did not get, warned about so far. A file chooses
    /// how many tables it has, so a line each is its own flood (noted in review).
    table_warnings: usize,
    /// Whether the file has already been told its tables ran out of cells between them.
    table_budget_spent: bool,
    settings: DocSettings,
    default_margins: Option<(Margins, Columns)>,
    swatches: Vec<Swatch>,
    /// Swatch Self → our swatch name.
    swatch_names: HashMap<String, String>,
    /// Hidden (unnamed) colours by Self.
    hidden_colors: HashMap<String, Color>,
    /// Unnamed swatches (gradients) added on first use.
    hidden_swatches: HashMap<String, Swatch>,
    color_groups: Vec<designcraft_doc::ColorGroup>,
    conditions: Vec<designcraft_doc::Condition>,
    condition_names: HashMap<String, String>,
    inks: designcraft_doc::InkManager,
    stroke_styles: Vec<designcraft_doc::StrokeStyleDef>,
    styles: Styles,
    composite_names: HashMap<String, String>,
    mojikumi_names: HashMap<String, String>,
    kinsoku: HashMap<String, designcraft_doc::cjk::Kinsoku>,
    para_names: HashMap<String, String>,
    char_names: HashMap<String, String>,
    lists: Vec<designcraft_doc::NumberedList>,
    object_names: HashMap<String, String>,
    cell_names: HashMap<String, String>,
    table_names: HashMap<String, String>,
    /// Object style Self → element (attribute fallback for page items).
    object_els: HashMap<String, El>,
    /// Paragraph style Self → element (attribute fallback for partly overridden settings).
    para_els: HashMap<String, El>,
    layers: Vec<Layer>,
    layer_ids: HashMap<String, LayerId>,
    stories: BTreeMap<StoryId, Story>,
    story_ids: HashMap<String, StoryId>,
    parents: Vec<Spread>,
    /// Things read but not kept as they were, for the import report.
    warnings: Vec<String>,
    /// Cells whose text is rotated (Cell Options › Text › Rotation): the angle is kept and
    /// written back, but only 0 is composed, so a narrow header column of vertical labels comes
    /// in horizontal and wraps mid-word (#1468).
    rotated_cells: usize,
    parent_ids: HashMap<String, SpreadId>,
    spreads: Vec<Spread>,
    page_index: HashMap<String, usize>,
    item_ids: HashMap<String, ItemId>,
    assets: BTreeMap<AssetId, Arc<Asset>>,
    /// Assets read from a link path at a page and crop, so later placements of that path, page
    /// and crop reuse them unread.
    linked_assets: HashMap<(String, u32, designcraft_doc::PdfCrop), AssetId>,
    ctx: ItemCtx,
    sections: Vec<Section>,
    footnote_options: designcraft_doc::FootnoteOptions,
    /// Text destination Self → anchor id.
    anchor_ids: HashMap<String, u64>,
    /// Cross-reference source Self → (story, index in its cross-references).
    xref_srcs: HashMap<String, (StoryId, usize)>,
    /// Cross-reference format Self → name.
    xref_format_names: HashMap<String, String>,
    xref_formats: Vec<designcraft_doc::XrefFormat>,
    /// Index topic Self → topic path and per-level sort keys; topic cross-references.
    index_topics: HashMap<String, IndexTopic>,
    index_xrefs: Vec<designcraft_doc::IndexRef>,
    /// The file's text variables (`None`: it defines none) and their Self → index.
    text_vars: Option<Vec<designcraft_doc::vars::TextVariable>>,
    text_var_ids: HashMap<String, usize>,
    /// Creation and modification times (Unix seconds) from the package metadata.
    created: Option<i64>,
    modified: Option<i64>,
    /// Tab lists [`TabStop::sanitized_list`] had to repair (an import warning).
    repaired_tab_lists: usize,
    toc: Option<designcraft_doc::Toc>,
}

impl<'r> Importer<'r> {
    fn new(read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>, previews: &'r HashMap<String, Arc<Vec<u8>>>) -> Self {
        let specials: Vec<Swatch> = designcraft_color::default_swatches().into_iter().filter(|s| s.locked).collect();
        let mut swatch_names = HashMap::new();
        swatch_names.insert("Swatch/None".to_string(), swatch::NONE.to_string());
        swatch_names.insert("Color/Paper".to_string(), swatch::PAPER.to_string());
        swatch_names.insert("Color/Black".to_string(), swatch::BLACK.to_string());
        swatch_names.insert("Color/Registration".to_string(), swatch::REGISTRATION.to_string());
        let mut styles = Styles::default();
        // Built-ins are replaced by the file's definitions when present.
        styles.paragraph.iter_mut().for_each(|s| s.based_on = None);
        Importer {
            read_link,
            previews,
            next_id: 0,
            table_cells: 0,
            table_warnings: 0,
            table_budget_spent: false,
            settings: DocSettings::default(),
            default_margins: None,
            swatches: specials,
            swatch_names,
            hidden_colors: HashMap::new(),
            hidden_swatches: HashMap::new(),
            color_groups: Vec::new(),
            conditions: Vec::new(),
            condition_names: HashMap::new(),
            inks: Default::default(),
            stroke_styles: Vec::new(),
            styles,
            composite_names: HashMap::new(),
            mojikumi_names: HashMap::new(),
            kinsoku: HashMap::new(),
            para_names: HashMap::new(),
            char_names: HashMap::new(),
            lists: Vec::new(),
            object_names: HashMap::new(),
            cell_names: HashMap::new(),
            table_names: HashMap::new(),
            object_els: HashMap::new(),
            para_els: HashMap::new(),
            layers: Vec::new(),
            layer_ids: HashMap::new(),
            stories: BTreeMap::new(),
            story_ids: HashMap::new(),
            parents: Vec::new(),
            warnings: Vec::new(),
            rotated_cells: 0,
            parent_ids: HashMap::new(),
            spreads: Vec::new(),
            page_index: HashMap::new(),
            item_ids: HashMap::new(),
            assets: BTreeMap::new(),
            linked_assets: HashMap::new(),
            ctx: ItemCtx { threads: Vec::new() },
            sections: Vec::new(),
            footnote_options: Default::default(),
            anchor_ids: HashMap::new(),
            xref_srcs: HashMap::new(),
            xref_format_names: HashMap::new(),
            xref_formats: designcraft_doc::xref::default_formats(),
            index_topics: HashMap::new(),
            index_xrefs: Vec::new(),
            text_vars: None,
            text_var_ids: HashMap::new(),
            created: None,
            modified: None,
            repaired_tab_lists: 0,
            toc: None,
        }
    }

    fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn read_preferences(&mut self, top: &[El]) {
        // Preferences first (page size, facing pages).
        for e in top {
            match e.local() {
                "DocumentPreference" => self.doc_prefs(e),
                "TransparencyPreference" => {
                    if let Some(v) = e.get("BlendingSpace") {
                        self.settings.blend_space = if v == "RGB" { designcraft_doc::BlendSpace::Rgb } else { designcraft_doc::BlendSpace::Cmyk };
                    }
                }
                "TextPreference" => {
                    let a = &mut self.settings.advanced_type;
                    for (k, v) in [
                        ("SuperscriptSize", &mut a.superscript_size),
                        ("SuperscriptPosition", &mut a.superscript_position),
                        ("SubscriptSize", &mut a.subscript_size),
                        ("SubscriptPosition", &mut a.subscript_position),
                    ] {
                        if let Some(x) = e.num(k) {
                            *v = x;
                        }
                    }
                }
                "MarginPreference" => {
                    let (m, c) = margins_of(e);
                    self.default_margins = Some((m, c));
                }
                "ViewPreference" => {
                    if let Some(u) = e.get("HorizontalMeasurementUnits") {
                        self.settings.horizontal_units = names::unit_in(u);
                    }
                    if let Some(u) = e.get("VerticalMeasurementUnits") {
                        self.settings.vertical_units = names::unit_in(u);
                    }
                    if let Some(v) = e.num("CursorKeyIncrement") {
                        self.settings.keyboard_increment = v;
                    }
                    match e.get("RulerOrigin") {
                        Some("PageOrigin") => self.settings.ruler_origin = designcraft_doc::RulerOrigin::Page,
                        Some("SpineOrigin") => self.settings.ruler_origin = designcraft_doc::RulerOrigin::Spine,
                        Some("SpreadOrigin") => self.settings.ruler_origin = designcraft_doc::RulerOrigin::Spread,
                        _ => {}
                    }
                }
                "GridPreference" => self.grid_prefs(e),
                "PasteboardPreference" => {
                    if let Some(v) = e.get("PasteboardMargins").map(nums)
                        && v.len() == 2
                        && v[0] >= 0.0
                        && v[1] >= 0.0
                    {
                        self.settings.pasteboard = (v[0], v[1]);
                    }
                }
                _ => {}
            }
        }
    }

    fn read_composite_fonts(&mut self, top: &[El]) {
        for e in top.iter().filter(|e| e.local() == "CompositeFont") {
            let name =
                e.get("Name").map(str::to_string).unwrap_or_else(|| unescape_id(e.get("Self").unwrap_or("").trim_start_matches("CompositeFont/")));
            if let Some(id) = e.get("Self") {
                self.composite_names.insert(id.to_string(), format!("CompositeFont/{name}"));
            }
            let entries = e
                .find_all("CompositeFontEntry")
                .map(|c| designcraft_doc::cjk::CompositeFontEntry {
                    name: c.get("Name").unwrap_or("").into(),
                    characters: c.get("CustomCharacters").unwrap_or("").into(),
                    family: c.prop("AppliedFont").as_deref().and_then(names::font_family_in).unwrap_or_default(),
                    style: c.get("FontStyle").unwrap_or("Regular").trim_start_matches("$ID/").into(),
                    // A metric `Document::check` refuses would make the whole file fail to
                    // open, so one that is unusable here falls back to full size / no shift.
                    relative_size: c.num("RelativeSize").map(|v| v / 100.0).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0),
                    horizontal_scale: c.num("HorizontalScale").map(|v| v / 100.0).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0),
                    vertical_scale: c.num("VerticalScale").map(|v| v / 100.0).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0),
                    baseline_shift: c.num("BaselineShift").filter(|v| v.is_finite()).unwrap_or(0.0) / 100.0,
                    scale_option: c.boolean("ScaleOption").unwrap_or(true),
                })
                .collect();
            self.styles.composite_fonts.push(designcraft_doc::cjk::CompositeFont { name, entries });
        }
    }

    fn read_cjk_tables(&mut self, top: &[El]) -> Result<()> {
        for e in top.iter().filter(|e| e.local() == "KinsokuTable") {
            if e.get("CantBeginLineChars").is_some() || e.get("CantEndLineChars").is_some() {
                self.kinsoku.insert(
                    e.get("Self").unwrap_or("").into(),
                    designcraft_doc::cjk::Kinsoku {
                        name: e.get("Name").unwrap_or("").into(),
                        no_start: e.get("CantBeginLineChars").unwrap_or("").into(),
                        no_end: e.get("CantEndLineChars").unwrap_or("").into(),
                        inseparable: e.get("CantBeSeparatedChars").unwrap_or("").into(),
                        hanging: e.get("HangingPunctuationChars").unwrap_or("").into(),
                    },
                );
            }
        }
        for e in top.iter().filter(|e| e.local() == "MojikumiTable") {
            if let (Some(id), Some(name)) = (e.get("Self"), e.get("Name")) {
                self.mojikumi_names.insert(id.into(), format!("MojikumiTable/{}", unescape_id(name)));
            }
            let overrides = if let Some(list) = e.prop_el("OverrideMojikumiAkiList") {
                list.find_all("OverrideMojikumiAkiType").map(mojikumi_row).collect::<Result<Vec<_>>>()?
            } else {
                Vec::new()
            };
            self.styles.mojikumi_tables.push(designcraft_doc::cjk::MojikumiTable {
                name: unescape_id(e.get("Name").unwrap_or("")),
                based_on: e.get("BasedOnMojikumiSet").unwrap_or("").into(),
                overrides,
            });
        }
        Ok(())
    }

    fn run(&mut self, _root: &El, top: &[El]) -> Result<()> {
        self.read_preferences(top);
        self.read_composite_fonts(top);
        self.read_cjk_tables(top)?;
        self.graphics(top);
        self.styles(top);
        if let Some(e) = top.iter().find(|e| e.local() == "FootnoteOption") {
            self.footnote_option(e);
        }
        if let Some(e) = top.iter().find(|e| e.local() == "EPubExportPreference") {
            self.epub_split(e);
        }
        for e in top.iter().filter(|e| e.local() == "Layer") {
            let id = LayerId(self.alloc());
            let color = match e.prop_el("LayerColor") {
                Some(c) if c.get("type") == Some("list") => {
                    let v: Vec<f64> = c.find_all("ListItem").filter_map(|i| i.text_content().trim().parse().ok()).collect();
                    if v.len() == 3 { [v[0] as u8, v[1] as u8, v[2] as u8] } else { LAYER_COLORS[0].1 }
                }
                Some(c) => names::layer_color_in(c.text_content().trim()).unwrap_or(LAYER_COLORS[self.layers.len() % LAYER_COLORS.len()].1),
                None => LAYER_COLORS[self.layers.len() % LAYER_COLORS.len()].1,
            };
            self.layers.push(Layer {
                id,
                name: e.get("Name").unwrap_or("Layer").to_string(),
                color,
                visible: e.get("Visible") != Some("false"),
                locked: e.get("Locked") == Some("true"),
                printable: e.get("Printable") != Some("false"),
                show_guides: e.get("ShowGuides") != Some("false"),
                suppress_wrap_when_hidden: e.get("IgnoreWrap") == Some("true"),
            });
            if let Some(s) = e.get("Self") {
                self.layer_ids.insert(s.to_string(), id);
            }
        }
        // IDML lists layers back to front; `Document::layers` is front to back.
        self.layers.reverse();
        if self.layers.is_empty() {
            let id = LayerId(self.alloc());
            self.layers.push(Layer {
                id,
                name: "Layer 1".into(),
                color: LAYER_COLORS[0].1,
                visible: true,
                locked: false,
                printable: true,
                show_guides: true,
                suppress_wrap_when_hidden: false,
            });
        }
        for e in top.iter().filter(|e| e.local() == "CrossReferenceFormat") {
            self.xref_format(e);
        }
        if let Some(ix) = top.iter().find(|e| e.local() == "Index") {
            self.index_topics_of(ix, &IndexTopic::default());
        }
        for e in top.iter().filter(|e| e.local() == "TextVariable") {
            self.text_variable(e);
        }
        // Stories (ids first so frames can reference them).
        let mut pending_stories = Vec::new();
        for e in top.iter().filter(|e| e.local() == "Story") {
            let id = StoryId(self.alloc());
            if let Some(s) = e.get("Self") {
                self.story_ids.insert(s.to_string(), id);
            }
            pending_stories.push((id, e));
        }
        for (id, e) in pending_stories {
            let story = self.story(id, e);
            self.stories.insert(id, story);
        }
        self.toc = self.toc_of(top);
        // Topic cross-references (See / See also) become markers at the start of the first story
        // with text.
        if !self.index_xrefs.is_empty()
            && let Some(st) = self.stories.values_mut().find(|s| !s.text.is_empty())
        {
            for r in std::mem::take(&mut self.index_xrefs).into_iter().rev() {
                st.insert_index_ref(0, r);
            }
        }
        // Cross-references point at their hyperlink's destination.
        for h in top.iter().filter(|e| e.local() == "Hyperlink") {
            let (Some(src), Some(dest)) = (h.get("Source"), h.prop("Destination")) else { continue };
            let (Some(&(sid, k)), Some(&target)) = (self.xref_srcs.get(src), self.anchor_ids.get(dest.trim())) else { continue };
            if let Some(x) = self.stories.get_mut(&sid).and_then(|st| st.xrefs.get_mut(k)) {
                Arc::make_mut(x).target = target;
            }
        }
        // Parent spreads: ids first (pages reference other parents).
        let masters: Vec<&El> = top.iter().filter(|e| e.local() == "MasterSpread").collect();
        let mut master_ids = Vec::new();
        for m in &masters {
            let id = SpreadId(self.alloc());
            if let Some(s) = m.get("Self") {
                self.parent_ids.insert(s.to_string(), id);
            }
            master_ids.push(id);
        }
        for (m, id) in masters.iter().zip(master_ids) {
            let sp = self.spread(m, id, true);
            self.parents.push(sp);
        }
        for e in top.iter().filter(|e| e.local() == "Spread") {
            let id = SpreadId(self.alloc());
            let sp = self.spread(e, id, false);
            self.spreads.push(sp);
        }
        // Sections.
        for e in top.iter().filter(|e| e.local() == "Section") {
            let Some(start) = e.get("PageStart").and_then(|p| self.page_index.get(p)).copied() else { continue };
            let cont = e.get("ContinueNumbering") != Some("false");
            self.sections.push(Section {
                start,
                start_number: if cont && start > 0 { None } else { Some(e.num("PageNumberStart").unwrap_or(1.0).max(1.0) as u32) },
                style: e.prop("PageNumberStyle").map(|s| names::number_style_in(s.trim())).unwrap_or_default(),
                prefix: e.get("SectionPrefix").unwrap_or("").to_string(),
                marker: e.get("Marker").unwrap_or("").to_string(),
                include_prefix: e.get("IncludeSectionPrefix") == Some("true"),
            });
        }
        if self.rotated_cells > 0 {
            let n = self.rotated_cells;
            self.warnings.push(format!(
                "{n} table cell(s) have rotated text (Cell Options › Text › Rotation): the angle is kept and written back, but the text is composed horizontally"
            ));
        }
        self.sections.sort_by_key(|s| s.start);
        self.sections.dedup_by_key(|s| s.start);
        if self.sections.first().is_none_or(|s| s.start != 0) {
            self.sections.insert(
                0,
                Section {
                    start: 0,
                    start_number: Some(1),
                    style: Default::default(),
                    prefix: String::new(),
                    marker: String::new(),
                    include_prefix: false,
                },
            );
        }
        self.link_threads();
        Ok(())
    }

    fn finish(mut self, root: &El) -> Result<Document> {
        let title = root.get("Name").map(|n| n.trim_end_matches(".indd").to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Untitled".into());
        if self.spreads.is_empty() {
            return Err(IdmlError::Invalid("document has no spreads".into()));
        }
        self.settings.lists = std::mem::take(&mut self.lists);
        self.clear_inherited_drop_cap_styles();
        // Make sure built-in paragraph styles exist and are first.
        let mut d = Document {
            title,
            file_info: Default::default(),
            settings: self.settings.clone(),
            spreads: std::mem::take(&mut self.spreads).into_iter().map(Arc::new).collect(),
            parents: std::mem::take(&mut self.parents).into_iter().map(Arc::new).collect(),
            layers: std::mem::take(&mut self.layers),
            stories: std::mem::take(&mut self.stories).into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            styles: Arc::new(std::mem::take(&mut self.styles)),
            swatches: std::mem::take(&mut self.swatches),
            color_groups: std::mem::take(&mut self.color_groups),
            conditions: std::mem::take(&mut self.conditions),
            inks: std::mem::take(&mut self.inks),
            stroke_styles: std::mem::take(&mut self.stroke_styles),
            articles: vec![],
            xml: Default::default(),
            endnote_options: Default::default(),
            endnote_story: None,
            sections: std::mem::take(&mut self.sections),
            assets: std::mem::take(&mut self.assets),
            hyperlinks: vec![],
            data_merge: Default::default(),
            bookmarks: vec![],
            user_words: vec![],
            hyphenation_exceptions: vec![],
            toc: self.toc.take(),
            text_variables: self.text_vars.take().unwrap_or_else(designcraft_doc::vars::defaults),
            footnote_options: std::mem::take(&mut self.footnote_options),
            xref_formats: std::mem::take(&mut self.xref_formats),
            index: None,
            created: self.created.unwrap_or_else(designcraft_doc::vars::now),
            modified: self.modified.unwrap_or(0),
            next_id: self.next_id,
            font_scope: 0,
        };
        if let [x, y] = root.get("ZeroPoint").map(nums).unwrap_or_default()[..]
            && let Some(zp) = d.clamp_zero_point([x, y])
        {
            d.settings.zero_point = zp;
        }
        Ok(d)
    }
}
