//! IDML package → Document.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use designcraft_color::cms::lab;
use designcraft_color::{Color, ColorType, Gradient, GradientKind, GradientStop, Swatch, SwatchValue, swatch};
use designcraft_doc::{
    AltFills, Asset, AssetId, BaselineGrid, Cell, CellRange, CellStroke, CharAttrs, CharFormat, CharRun, CharacterStyle, Columns, ColumnsKind,
    Content, DocSettings, Document, DropShadow, Effects, Fill, GridAlign, GridRelative, Guide, Item, ItemId, Kerning, LAYER_COLORS, Layer, LayerId,
    ListType, Margins, ObjectStyle, Page, PageId, PageSide, ParaAttrs, ParaFormat, ParagraphStyle, ParentInfo, RowHeightMode, Rule, Section, Shape,
    SpanColumns, Spread, SpreadId, Story, StoryId, Stroke, Styles, TabStop, Table, TextFrame, TextFrameOptions, TextWrap, VerticalJustification,
    story as st,
};
use designcraft_geom::corners::{Corner, CornerOptions};
use designcraft_geom::{Affine, Anchor, PathData, Point, Rect, SubPath};

use crate::names::{self, CHAR_BUILTINS, OBJECT_BUILTINS, PARA_BUILTINS, unescape_id};
use crate::xml::{El, Node, parse};
use crate::{IdmlError, MIMETYPE, Result, base64_decode, sniff_image};

/// Import an IDML package. Linked images are read from disk when available (not on wasm).
pub fn import_idml(bytes: &[u8]) -> Result<Document> {
    import_idml_with(bytes, &default_reader)
}

fn default_reader(path: &str) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read(path).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

/// Import with a custom reader for linked files (`path` → bytes).
pub fn import_idml_with(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Document> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        // The size is what the archive claims: don't reserve more than a sane part up front.
        let mut buf = Vec::with_capacity((f.size() as usize).min(1 << 24));
        f.read_to_end(&mut buf).map_err(|e| IdmlError::Part { part: name.clone(), msg: e.to_string() })?;
        files.insert(name, buf);
    }
    if let Some(m) = files.get("mimetype")
        && String::from_utf8_lossy(m).trim() != MIMETYPE
    {
        return Err(IdmlError::NotIdml(format!("unexpected mimetype `{}`", String::from_utf8_lossy(m).trim())));
    }
    let dm = files.get("designmap.xml").ok_or_else(|| IdmlError::NotIdml("missing designmap.xml".into()))?;
    let root = parse(dm).map_err(|msg| IdmlError::Part { part: "designmap.xml".into(), msg })?;
    if root.local() != "Document" {
        return Err(IdmlError::NotIdml(format!("designmap root is <{}>", root.name)));
    }
    // Flatten includes: every `idPkg:*` child is replaced by the children of its part's root.
    let mut top: Vec<El> = Vec::new();
    for c in root.elements() {
        if c.name.starts_with("idPkg:") {
            let Some(src) = c.get("src") else { continue };
            let Some(data) = files.get(src) else { continue };
            let part = parse(data).map_err(|msg| IdmlError::Part { part: src.into(), msg })?;
            top.extend(part.elements().cloned());
        } else {
            top.push(c.clone());
        }
    }
    let mut im = Importer::new(read_link);
    im.run(&root, &top)?;
    let d = im.finish(&root)?;
    d.check().map_err(|e| IdmlError::Invalid(e.to_string()))?;
    Ok(d)
}

struct ItemCtx {
    /// Self → (story self, prev self, next self)
    threads: Vec<(ItemId, String, String, String)>,
}

struct Importer<'r> {
    read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>,
    next_id: u64,
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
    para_names: HashMap<String, String>,
    char_names: HashMap<String, String>,
    lists: Vec<designcraft_doc::NumberedList>,
    object_names: HashMap<String, String>,
    /// Object style Self → element (attribute fallback for page items).
    object_els: HashMap<String, El>,
    layers: Vec<Layer>,
    layer_ids: HashMap<String, LayerId>,
    stories: BTreeMap<StoryId, Story>,
    story_ids: HashMap<String, StoryId>,
    /// Stories with a vertical StoryOrientation (their frames set text vertically).
    vertical_stories: std::collections::HashSet<StoryId>,
    parents: Vec<Spread>,
    parent_ids: HashMap<String, SpreadId>,
    spreads: Vec<Spread>,
    page_index: HashMap<String, usize>,
    item_ids: HashMap<String, ItemId>,
    assets: BTreeMap<AssetId, Arc<Asset>>,
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
    /// Index topic Self → topic path; topic See / See also cross-references.
    index_topics: HashMap<String, Vec<String>>,
    index_xrefs: Vec<designcraft_doc::IndexRef>,
}

fn lab_to_color(l: f32, a: f32, b: f32) -> Color {
    let rgb = lab::xyz_to_srgb(lab::lab_to_xyz(lab::Lab::new(l, a, b)));
    Color::rgb(rgb[0], rgb[1], rgb[2])
}

fn nums(s: &str) -> Vec<f64> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

fn parse_affine(s: Option<&str>) -> Affine {
    match s.map(nums) {
        Some(v) if v.len() == 6 => Affine::new([v[0], v[1], v[2], v[3], v[4], v[5]]),
        _ => Affine::IDENTITY,
    }
}

fn parse_point(s: Option<&str>) -> Option<Point> {
    let v = nums(s?);
    (v.len() >= 2).then(|| Point::new(v[0], v[1]))
}

/// IDML tint (percent, `-1` = default) → 0..1.
fn tint(v: Option<f64>) -> Option<f32> {
    v.map(|t| if t < 0.0 { 1.0 } else { (t / 100.0) as f32 })
}

impl<'r> Importer<'r> {
    fn new(read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>) -> Self {
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
            next_id: 0,
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
            para_names: HashMap::new(),
            char_names: HashMap::new(),
            lists: Vec::new(),
            object_names: HashMap::new(),
            object_els: HashMap::new(),
            layers: Vec::new(),
            layer_ids: HashMap::new(),
            stories: BTreeMap::new(),
            vertical_stories: Default::default(),
            story_ids: HashMap::new(),
            parents: Vec::new(),
            parent_ids: HashMap::new(),
            spreads: Vec::new(),
            page_index: HashMap::new(),
            item_ids: HashMap::new(),
            assets: BTreeMap::new(),
            ctx: ItemCtx { threads: Vec::new() },
            sections: Vec::new(),
            footnote_options: Default::default(),
            anchor_ids: HashMap::new(),
            xref_srcs: HashMap::new(),
            xref_format_names: HashMap::new(),
            xref_formats: designcraft_doc::xref::default_formats(),
            index_topics: HashMap::new(),
            index_xrefs: Vec::new(),
        }
    }

    fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn run(&mut self, _root: &El, top: &[El]) -> Result<()> {
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
        self.graphics(top);
        self.styles(top);
        if let Some(e) = top.iter().find(|e| e.local() == "FootnoteOption") {
            self.footnote_option(e);
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
            self.index_topics_of(ix, &[]);
        }
        // Stories (ids first so frames can reference them).
        for e in top.iter().filter(|e| e.local() == "Story") {
            let id = StoryId(self.alloc());
            if let Some(s) = e.get("Self") {
                self.story_ids.insert(s.to_string(), id);
            }
            let story = self.story(id, e);
            if e.find("StoryPreference").and_then(|p| p.get("StoryOrientation")) == Some("Vertical") {
                self.vertical_stories.insert(id);
            }
            self.stories.insert(id, story);
        }
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

    fn doc_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        if let Some(v) = e.get("OverprintBlack") {
            s.overprint_black = v != "false";
        }
        if let Some(v) = e.num("PageWidth") {
            s.page_width = v;
        }
        if let Some(v) = e.num("PageHeight") {
            s.page_height = v;
        }
        if let Some(v) = e.boolean("FacingPages") {
            s.facing_pages = v;
        }
        if let Some(v) = e.get("PageBinding") {
            s.right_to_left_binding = v == "RightToLeft";
        }
        for (i, k) in ["DocumentBleedTopOffset", "DocumentBleedBottomOffset", "DocumentBleedInsideOrLeftOffset", "DocumentBleedOutsideOrRightOffset"]
            .iter()
            .enumerate()
        {
            if let Some(v) = e.num(k) {
                s.bleed[i] = v;
            }
        }
        for (i, k) in ["SlugTopOffset", "SlugBottomOffset", "SlugInsideOrLeftOffset", "SlugRightOrOutsideOffset"].iter().enumerate() {
            if let Some(v) = e.num(k) {
                s.slug[i] = v;
            }
        }
        if let Some(i) = e.get("Intent") {
            s.intent = match i {
                "WebIntent" => designcraft_doc::Intent::Web,
                "MobileIntent" => designcraft_doc::Intent::Mobile,
                _ => designcraft_doc::Intent::Print,
            };
        }
        if let Some(v) = e.boolean("CreatePrimaryTextFrame") {
            s.primary_text_frame = v;
        }
    }

    fn grid_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        let bg: &mut BaselineGrid = &mut s.baseline_grid;
        if let Some(v) = e.num("BaselineStart") {
            bg.start = v;
        }
        if let Some(v) = e.num("BaselineDivision") {
            bg.increment = v;
        }
        if let Some(v) = e.num("BaselineViewThreshold") {
            bg.view_threshold = v / 100.0;
        }
        if let Some(v) = e.get("BaselineGridRelativeOption") {
            bg.relative_to = if v.contains("Margin") { GridRelative::TopMargin } else { GridRelative::TopOfPage };
        }
        if let Some(v) = e.num("HorizontalGridlineDivision") {
            s.grid.horizontal = v;
        }
        if let Some(v) = e.num("VerticalGridlineDivision") {
            s.grid.vertical = v;
        }
        if let Some(v) = e.num("HorizontalGridSubdivision") {
            s.grid.subdivisions = v.max(1.0) as u32;
        }
        if let Some(v) = e.boolean("GridsInBack") {
            s.grid.in_back = v;
        }
    }

    // ---------- swatches ----------

    fn color_of(e: &El) -> Option<(Color, ColorType)> {
        let v = nums(e.get("ColorValue").unwrap_or(""));
        let space = e.get("Space").unwrap_or("CMYK");
        let c = match space {
            "RGB" if v.len() >= 3 => Color::rgb((v[0] / 255.0) as f32, (v[1] / 255.0) as f32, (v[2] / 255.0) as f32),
            "LAB" if v.len() >= 3 => lab_to_color(v[0] as f32, v[1] as f32, v[2] as f32),
            _ if v.len() >= 4 => Color::cmyk((v[0] / 100.0) as f32, (v[1] / 100.0) as f32, (v[2] / 100.0) as f32, (v[3] / 100.0) as f32),
            _ => return None,
        };
        let ty = if e.get("Model") == Some("Spot") { ColorType::Spot } else { ColorType::Process };
        Some((c, ty))
    }

    fn graphics(&mut self, top: &[El]) {
        let mut order: Vec<(String, Swatch)> = Vec::new();
        // Colours.
        for e in top.iter().filter(|e| e.local() == "Color") {
            let Some(id) = e.get("Self") else { continue };
            let Some((c, ty)) = Self::color_of(e) else { continue };
            match id {
                "Color/Paper" => {
                    if let Some(s) = self.swatches.iter_mut().find(|s| s.name == swatch::PAPER) {
                        s.value = SwatchValue::Paper { color: if c == Color::cmyk(0.0, 0.0, 0.0, 0.0) { Color::WHITE } else { c } };
                    }
                    continue;
                }
                "Color/Black" | "Color/Registration" => continue,
                _ => {}
            }
            let name = e.get("Name").unwrap_or("");
            if e.get("Visible") == Some("false") || name.is_empty() || name == "$ID/" || e.get("Model") == Some("Registration") {
                self.hidden_colors.insert(id.to_string(), c);
                continue;
            }
            let named = !name.starts_with("C=") && !name.starts_with("R=");
            self.swatch_names.insert(id.to_string(), name.to_string());
            order.push((
                id.to_string(),
                Swatch { name: name.to_string(), value: SwatchValue::Color { color: c, color_type: ty }, locked: false, named, hidden: false },
            ));
        }
        // Tints (after colours: they reference them).
        for e in top.iter().filter(|e| e.local() == "Tint") {
            let Some(id) = e.get("Self") else { continue };
            let base = e.get("BaseColor").map(|b| self.swatch_ref(b)).unwrap_or_else(|| swatch::BLACK.to_string());
            let t = e.num("TintValue").unwrap_or(100.0);
            let name = match e.get("Name") {
                Some(n) if !n.is_empty() && n != "$ID/" => n.to_string(),
                _ => format!("{base} {}%", names::num(t)),
            };
            self.swatch_names.insert(id.to_string(), name.clone());
            order.push((
                id.to_string(),
                Swatch { name, value: SwatchValue::Tint { base, tint: (t / 100.0) as f32 }, locked: false, named: true, hidden: false },
            ));
        }
        // Custom stroke styles.
        for e in top.iter().filter(|e| matches!(e.local(), "StripedStrokeStyle" | "DashedStrokeStyle" | "DottedStrokeStyle")) {
            let (Some(id), Some(name)) = (e.get("Self"), e.get("Name")) else { continue };
            let nums = |k: &str| e.get(k).unwrap_or("").split_whitespace().filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<_>>();
            let kind = match e.local() {
                "StripedStrokeStyle" => designcraft_doc::StrokeType::Stripes {
                    bands: nums("StripeArray").as_chunks::<2>().0.iter().map(|c| (c[0] / 100.0, (c[1] - c[0]) / 100.0)).collect(),
                },
                "DashedStrokeStyle" => designcraft_doc::StrokeType::Dashed { pattern: nums("DashArray") },
                _ => designcraft_doc::StrokeType::Dotted,
            };
            let _ = id;
            self.stroke_styles.push(designcraft_doc::StrokeStyleDef { name: name.to_string(), kind });
        }
        // Ink Manager.
        for e in top.iter().filter(|e| e.local() == "Ink") {
            let Some(name) = e
                .get("Name")
                .filter(|n| !n.starts_with("$ID/") && !["Process Cyan", "Process Magenta", "Process Yellow", "Process Black"].contains(n))
            else {
                continue;
            };
            if e.get("ConvertToProcess") == Some("true") {
                self.inks.to_process.push(name.to_string());
            }
            if let Some(a) = e.get("AliasInkName").filter(|a| !a.is_empty() && *a != name && !a.starts_with("$ID/")) {
                self.inks.aliases.push((name.to_string(), a.to_string()));
            }
        }
        for e in top.iter().filter(|e| e.local() == "Gradient") {
            let Some(id) = e.get("Self") else { continue };
            let mut stops: Vec<GradientStop> = Vec::new();
            for s in e.find_all("GradientStop") {
                let c = s.get("StopColor").map(|r| self.color_for_ref(r, &order)).unwrap_or(Color::BLACK);
                let loc = s.num("Location").unwrap_or(0.0) / 100.0;
                if let (Some(prev), Some(m)) = (stops.last_mut(), s.num("Midpoint")) {
                    prev.midpoint = (m / 100.0) as f32;
                }
                stops.push(GradientStop { offset: loc as f32, color: c, opacity: 1.0, midpoint: 0.5 });
            }
            let kind = if e.get("Type") == Some("Radial") { GradientKind::Radial } else { GradientKind::Linear };
            let unnamed = e.get("Visible") == Some("false") || e.get("Name").is_none_or(|n| n.is_empty() || n == "$ID/");
            let name = if unnamed { "Gradient".to_string() } else { e.get("Name").unwrap_or_default().to_string() };
            let sw =
                Swatch { name, value: SwatchValue::Gradient { gradient: Gradient { kind, stops } }, locked: false, named: !unnamed, hidden: false };
            if unnamed {
                // Unnamed gradients become swatches only when something uses them.
                self.hidden_swatches.insert(id.to_string(), sw);
            } else {
                self.swatch_names.insert(id.to_string(), sw.name.clone());
                order.push((id.to_string(), sw));
            }
        }
        // Panel order from the root colour group.
        let group_order: Vec<String> = top
            .iter()
            .filter(|e| e.local() == "ColorGroup")
            .flat_map(|g| g.find_all("ColorGroupSwatch").filter_map(|s| s.get("SwatchItemRef").map(str::to_string)).collect::<Vec<_>>())
            .collect();
        if !group_order.is_empty() {
            order.sort_by_key(|(id, _)| group_order.iter().position(|g| g == id).unwrap_or(usize::MAX));
        }
        let mut names: HashSet<String> = self.swatches.iter().map(|s| s.name.clone()).collect();
        for (id, mut s) in order {
            if names.contains(&s.name) {
                let base = s.name.clone();
                s.name = Styles::unique_name(|n| names.contains(n), &base);
                self.swatch_names.insert(id, s.name.clone());
            }
            names.insert(s.name.clone());
            self.swatches.push(s);
        }
        // Colour groups other than the root one become Swatches panel folders.
        for g in top.iter().filter(|e| e.local() == "ColorGroup" && e.get("IsRootColorGroup") != Some("true")) {
            let swatches: Vec<String> =
                g.find_all("ColorGroupSwatch").filter_map(|s| s.get("SwatchItemRef")).filter_map(|r| self.swatch_names.get(r).cloned()).collect();
            self.color_groups.push(designcraft_doc::ColorGroup { name: g.get("Name").unwrap_or("Color Group").to_string(), swatches });
        }
    }

    fn color_for_ref(&self, r: &str, pending: &[(String, Swatch)]) -> Color {
        if let Some(c) = self.hidden_colors.get(r) {
            return *c;
        }
        match r {
            "Color/Black" => return Color::cmyk(0.0, 0.0, 0.0, 1.0),
            "Color/Paper" => return Color::WHITE,
            "Color/Registration" => return Color::cmyk(1.0, 1.0, 1.0, 1.0),
            _ => {}
        }
        if let Some((_, s)) = pending.iter().find(|(id, _)| id == r) {
            match &s.value {
                SwatchValue::Color { color, .. } => return *color,
                SwatchValue::Tint { base, tint } => {
                    let mut all: Vec<Swatch> = self.swatches.clone();
                    all.extend(pending.iter().map(|(_, s)| s.clone()));
                    return designcraft_color::swatch::resolve(&all, base, *tint).unwrap_or(Color::BLACK);
                }
                _ => {}
            }
        }
        Color::BLACK
    }

    /// A swatch reference (Self id) → our swatch name; unnamed colours become value-named swatches.
    fn swatch_ref(&mut self, r: &str) -> String {
        if let Some(n) = self.swatch_names.get(r) {
            return n.clone();
        }
        if r == "n" || r.is_empty() {
            return swatch::NONE.into();
        }
        if let Some(mut sw) = self.hidden_swatches.remove(r) {
            sw.name = Styles::unique_name(|n| self.swatches.iter().any(|s| s.name == n), &sw.name);
            self.swatch_names.insert(r.to_string(), sw.name.clone());
            let name = sw.name.clone();
            self.swatches.push(sw);
            return name;
        }
        if let Some(c) = self.hidden_colors.get(r).copied() {
            let name = match c {
                Color::Cmyk { c, m, y, k } => designcraft_color::swatch::cmyk_name(c, m, y, k),
                Color::Rgb { r, g, b } => format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round()),
                Color::Gray { k } => format!("K={}", (k * 100.0).round()),
            };
            if !self.swatches.iter().any(|s| s.name == name) {
                self.swatches.push(Swatch {
                    name: name.clone(),
                    value: SwatchValue::Color { color: c, color_type: ColorType::Process },
                    locked: false,
                    named: false,
                    hidden: false,
                });
            }
            self.swatch_names.insert(r.to_string(), name.clone());
            return name;
        }
        swatch::NONE.into()
    }

    // ---------- styles ----------

    fn styles(&mut self, top: &[El]) {
        for e in top.iter().filter(|e| e.local() == "Condition") {
            let (Some(id), Some(name)) = (e.get("Self"), e.get("Name")) else { continue };
            // Indicator colours: "r g b" (0–255) or a UI colour name (kept as the default blue).
            let nums: Vec<u8> = e
                .get("IndicatorColor")
                .unwrap_or("")
                .split_whitespace()
                .filter_map(|x| x.parse::<f64>().ok())
                .map(|x| x.clamp(0.0, 255.0) as u8)
                .collect();
            let color = if nums.len() == 3 { [nums[0], nums[1], nums[2]] } else { [79, 153, 255] };
            self.condition_names.insert(id.to_string(), name.to_string());
            self.conditions.push(designcraft_doc::Condition { name: name.to_string(), color, visible: e.get("Visible") != Some("false") });
        }
        for e in top.iter().filter(|e| e.local() == "NumberingList") {
            let name = e.get("Name").unwrap_or("").to_string();
            if !name.is_empty() && !name.starts_with("$ID/") && !self.lists.iter().any(|l| l.name == name) {
                self.lists
                    .push(designcraft_doc::NumberedList { name, continue_across_stories: e.get("ContinueNumbersAcrossStories") != Some("false") });
            }
        }
        let mut paras: Vec<(String, El)> = Vec::new();
        let mut chars: Vec<(String, El)> = Vec::new();
        let mut objects: Vec<(String, El)> = Vec::new();
        let mut cell_styles: Vec<(String, El)> = Vec::new();
        let mut table_styles: Vec<(String, El)> = Vec::new();
        fn walk(e: &El, style: &str, group: &str, out: &mut Vec<(String, El)>) {
            for c in e.elements() {
                if c.local() == style {
                    out.push((c.get("Self").unwrap_or("").to_string(), c.clone()));
                } else if c.local() == group {
                    walk(c, style, group, out);
                }
            }
        }
        for e in top {
            match e.local() {
                "RootParagraphStyleGroup" => walk(e, "ParagraphStyle", "ParagraphStyleGroup", &mut paras),
                "RootCharacterStyleGroup" => walk(e, "CharacterStyle", "CharacterStyleGroup", &mut chars),
                "RootObjectStyleGroup" => walk(e, "ObjectStyle", "ObjectStyleGroup", &mut objects),
                "RootCellStyleGroup" => walk(e, "CellStyle", "CellStyleGroup", &mut cell_styles),
                "RootTableStyleGroup" => walk(e, "TableStyle", "TableStyleGroup", &mut table_styles),
                _ => {}
            }
        }
        for (id, e) in &paras {
            let n = names::style_name_in(PARA_BUILTINS, e.get("Name").unwrap_or(""));
            self.para_names.insert(id.clone(), n);
        }
        for (id, e) in &chars {
            let n = names::style_name_in(CHAR_BUILTINS, e.get("Name").unwrap_or(""));
            self.char_names.insert(id.clone(), n);
        }
        for (id, e) in &objects {
            let n = names::style_name_in(OBJECT_BUILTINS, e.get("Name").unwrap_or(""));
            self.object_names.insert(id.clone(), n);
            self.object_els.insert(id.clone(), e.clone());
        }
        for (_, e) in &paras {
            let name = names::style_name_in(PARA_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = if name == designcraft_doc::NO_PARA_STYLE { None } else { self.based_on(e, true) };
            let next_style = e.get("NextStyle").map(|n| self.para_style_ref(n)).filter(|n| *n != name);
            let para = self.para_attrs(e);
            let chars = self.char_attrs(e);
            let s = ParagraphStyle { name: name.clone(), based_on, next_style, para, chars, shortcut: String::new() };
            match self.styles.para_mut(&name) {
                Some(slot) => *slot = s,
                None => self.styles.paragraph.push(s),
            }
        }
        for (_, e) in &chars {
            let name = names::style_name_in(CHAR_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = if name == st::NO_CHAR_STYLE { None } else { self.based_on(e, false) };
            let chars = self.char_attrs(e);
            let s = CharacterStyle { name: name.clone(), based_on, chars, shortcut: String::new() };
            match self.styles.char_style_mut(&name) {
                Some(slot) => *slot = s,
                None => self.styles.character.push(s),
            }
        }
        // Cell and table styles (the built-ins stay as they are).
        let cell_ref = |r: &str| names::style_name_in(names::CELL_BUILTINS, &unescape_id(r.trim_start_matches("CellStyle/")));
        for (_, e) in &cell_styles {
            let name = names::style_name_in(names::CELL_BUILTINS, e.get("Name").unwrap_or(""));
            if name == designcraft_doc::NO_CELL_STYLE {
                continue;
            }
            let mut cs = designcraft_doc::CellStyle { name: name.clone(), ..Default::default() };
            cs.fill = e.get("FillColor").map(|c| self.swatch_ref(c));
            cs.fill_tint = e.num("FillTint").and_then(|v| tint(Some(v)));
            let ins: Vec<Option<f64>> = ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().map(|k| e.num(k)).collect();
            if ins.iter().all(Option::is_some) {
                cs.insets = Some([ins[0].unwrap_or(0.0), ins[1].unwrap_or(0.0), ins[2].unwrap_or(0.0), ins[3].unwrap_or(0.0)]);
            }
            cs.vj = e.get("VerticalJustification").map(names::vj_in);
            if e.get("TopEdgeStrokeWeight").is_some() || e.get("TopEdgeStrokeColor").is_some() {
                let mut st = CellStroke::default();
                if let Some(w) = e.num("TopEdgeStrokeWeight") {
                    st.weight = w.max(0.0);
                }
                if let Some(c) = e.get("TopEdgeStrokeColor") {
                    st.color = self.swatch_ref(c);
                }
                cs.stroke = Some(st);
            }
            cs.paragraph_style = e.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).filter(|n| n != designcraft_doc::NO_PARA_STYLE);
            match self.styles.cell.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = cs,
                None => self.styles.cell.push(cs),
            }
        }
        for (_, e) in &table_styles {
            let name = names::style_name_in(names::TABLE_BUILTINS, e.get("Name").unwrap_or(""));
            if name == designcraft_doc::BASIC_TABLE || name == "[No table style]" {
                continue;
            }
            let mut ts = designcraft_doc::TableStyle { name: name.clone(), ..Default::default() };
            ts.header = e.get("HeaderRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.body = e.get("BodyRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.footer = e.get("FooterRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.left_column = e.get("LeftColumnRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.right_column = e.get("RightColumnRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            if e.get("TopBorderStrokeWeight").is_some() {
                let mut b = CellStroke::default();
                if let Some(w) = e.num("TopBorderStrokeWeight") {
                    b.weight = w.max(0.0);
                }
                if let Some(c) = e.get("TopBorderStrokeColor") {
                    b.color = self.swatch_ref(c);
                }
                ts.border = Some(b);
            }
            if let Some(first) = e.num("StartRowFillCount").filter(|n| *n > 0.0) {
                ts.alt_rows = Some(AltFills {
                    first: first as u32,
                    first_color: e.get("StartRowFillColor").map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                    first_tint: tint(e.num("StartRowFillTint")).unwrap_or(1.0),
                    next: e.num("EndRowFillCount").unwrap_or(1.0) as u32,
                    next_color: e.get("EndRowFillColor").map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                    next_tint: tint(e.num("EndRowFillTint")).unwrap_or(1.0),
                    skip_first: 0,
                    skip_last: 0,
                });
            }
            ts.space_before = e.num("SpaceBefore");
            ts.space_after = e.num("SpaceAfter");
            match self.styles.table.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = ts,
                None => self.styles.table.push(ts),
            }
        }
        for (_, e) in &objects {
            let name = names::style_name_in(OBJECT_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = e.prop("BasedOn").map(|b| {
                self.object_names
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| names::style_name_in(OBJECT_BUILTINS, &unescape_id(b.trim_start_matches("ObjectStyle/"))))
            });
            let based_on = based_on.filter(|b| *b != designcraft_doc::NO_OBJECT_STYLE && *b != name);
            let enabled = |k: &str, attr: &str| match e.get(k) {
                Some(v) => v == "true",
                None => e.get(attr).is_some(),
            };
            let fill = enabled("EnableFill", "FillColor").then(|| {
                let sw = e.get("FillColor").map(|r| self.swatch_ref(r)).unwrap_or_else(|| swatch::NONE.into());
                Fill { swatch: sw, tint: tint(e.num("FillTint")).unwrap_or(1.0), ..Fill::none() }
            });
            let stroke = enabled("EnableStroke", "StrokeColor").then(|| self.stroke_from(e, None));
            let paragraph_style = enabled("EnableParagraphStyle", "AppliedParagraphStyle")
                .then(|| e.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)))
                .flatten();
            let text_frame =
                if e.get("EnableTextFrameGeneralOptions") == Some("true") { e.find("TextFramePreference").map(text_frame_options) } else { None };
            let s = ObjectStyle { name: name.clone(), based_on, fill, stroke, paragraph_style, text_frame };
            match self.styles.object.iter_mut().find(|o| o.name == name) {
                Some(slot) => *slot = s,
                None => self.styles.object.push(s),
            }
        }
    }

    fn based_on(&self, e: &El, para: bool) -> Option<String> {
        let b = e.prop("BasedOn")?;
        let b = b.trim();
        if para {
            if let Some(n) = self.para_names.get(b) {
                return Some(n.clone());
            }
            let n = names::style_name_in(PARA_BUILTINS, &unescape_id(b.trim_start_matches("ParagraphStyle/")));
            Some(n)
        } else {
            if let Some(n) = self.char_names.get(b) {
                return Some(n.clone()).filter(|n| n != st::NO_CHAR_STYLE);
            }
            let n = names::style_name_in(CHAR_BUILTINS, &unescape_id(b.trim_start_matches("CharacterStyle/")));
            Some(n).filter(|n| n != st::NO_CHAR_STYLE)
        }
    }

    /// Index topics (recursively); at the root, their See / See also cross-references too.
    fn index_topics_of(&mut self, e: &El, prefix: &[String]) {
        for t in e.find_all("Topic") {
            let mut path = prefix.to_vec();
            path.push(t.get("Name").unwrap_or("").to_string());
            if let Some(me) = t.get("Self") {
                self.index_topics.insert(me.to_string(), path.clone());
            }
            self.index_topics_of(t, &path);
        }
        if !prefix.is_empty() {
            return;
        }
        let mut stack = vec![(e, Vec::<String>::new())];
        while let Some((el, path)) = stack.pop() {
            for t in el.find_all("Topic") {
                let mut p = path.clone();
                p.push(t.get("Name").unwrap_or("").to_string());
                for x in t.find_all("CrossReference") {
                    let target = x.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).map(|p| p.join(": ")).unwrap_or_default();
                    use designcraft_doc::index::IndexRange as R;
                    let range = match x.get("CrossReferenceType") {
                        Some("SeeAlso") | Some("SeeAlsoHerein") => R::SeeAlso(target),
                        _ => R::See(target),
                    };
                    self.index_xrefs.push(designcraft_doc::IndexRef { topics: p.clone(), sort: vec![], range });
                }
                stack.push((t, p));
            }
        }
    }

    /// `CrossReferenceFormat`: building blocks back to a format definition.
    fn xref_format(&mut self, e: &El) {
        let name = e.get("Name").unwrap_or("Format").to_string();
        let mut def = String::new();
        for b in e.find_all("BuildingBlock") {
            let text = |k: &str| b.get(k).filter(|v| *v != "$ID/").unwrap_or("").to_string();
            match b.get("BlockType").unwrap_or("") {
                "CustomStringBuildingBlock" => def.push_str(&text("CustomText")),
                "FullParagraphBuildingBlock" => def.push_str("<fullPara />"),
                "ParagraphTextBuildingBlock" => def.push_str("<paraText />"),
                "ParagraphNumberBuildingBlock" => def.push_str("<paraNum />"),
                "PageNumberBuildingBlock" => def.push_str("<pageNum />"),
                "BookmarkNameBuildingBlock" => def.push_str("<txtAnchrName />"),
                "ChapterNumberBuildingBlock" => def.push_str("<chapNum />"),
                "FileNameBuildingBlock" => def.push_str("<fileName />"),
                "PartialParagraphBuildingBlock" => def.push_str(&format!(
                    "<partialPara delim=\"{}\" includeDelim=\"{}\" />",
                    text("AppliedDelimiter"),
                    b.get("IncludeDelimiter") == Some("true")
                )),
                _ => {}
            }
        }
        if let Some(me) = e.get("Self") {
            self.xref_format_names.insert(me.to_string(), name.clone());
        }
        match self.xref_formats.iter_mut().find(|f| f.name == name) {
            Some(f) => f.definition = def,
            None => self.xref_formats.push(designcraft_doc::XrefFormat { name, definition: def }),
        }
    }

    /// Preferences › `FootnoteOption` (after styles: it names the footnote text and marker styles).
    fn footnote_option(&mut self, e: &El) {
        let mut o = designcraft_doc::FootnoteOptions::default();
        if let Some(v) = e.num("StartAt") {
            o.start_at = v.max(0.0) as u32;
        }
        o.prefix = e.get("Prefix").unwrap_or("").into();
        o.suffix = e.get("Suffix").unwrap_or("").into();
        if let Some(r) = e.get("FootnoteTextStyle") {
            o.para_style = self.para_style_ref(r);
        }
        if let Some(r) = e.get("FootnoteMarkerStyle") {
            o.ref_char_style = self.char_style_ref(r);
        }
        if let Some(v) = e.get("SeparatorText") {
            o.separator = v.into();
        }
        o.space_between = e.num("SpaceBetween").unwrap_or(0.0);
        o.space_before = e.num("Spacer").unwrap_or(0.0);
        if let Some(v) = e.prop("FootnoteFirstBaselineOffset") {
            o.first_baseline = names::first_baseline_in(v.trim());
        }
        o.first_baseline_min = e.num("FootnoteMinimumFirstBaselineOffset").unwrap_or(0.0);
        o.span_columns = e.boolean("EnableStraddling").unwrap_or(false);
        o.rule.on = e.boolean("RuleOn").unwrap_or(true);
        o.rule.weight = e.num("RuleLineWeight").unwrap_or(1.0);
        o.rule.tint = (e.num("RuleTint").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0) as f32;
        o.rule.left_indent = e.num("RuleLeftIndent").unwrap_or(0.0);
        o.rule.width = e.num("RuleWidth").unwrap_or(72.0);
        o.rule.offset = e.num("RuleOffset").unwrap_or(0.0);
        if let Some(c) = e.prop("RuleColor") {
            o.rule.color = self.swatch_ref(c.trim());
        }
        if let Some(v) = e.prop("FootnoteNumberingStyle") {
            o.style = names::note_style_in(v.trim());
        }
        if let Some(v) = e.prop("RestartNumbering") {
            o.restart = names::NOTE_RESTART.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = e.prop("ShowPrefixSuffix") {
            o.affix_in = names::NOTE_AFFIX.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = e.prop("MarkerPositioning") {
            o.ref_position = names::note_marker_in(v.trim());
        }
        self.footnote_options = o;
    }

    fn para_style_ref(&self, r: &str) -> String {
        self.para_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(PARA_BUILTINS, &unescape_id(r.trim_start_matches("ParagraphStyle/"))))
    }
    fn char_style_ref(&self, r: &str) -> String {
        self.char_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(CHAR_BUILTINS, &unescape_id(r.trim_start_matches("CharacterStyle/"))))
    }

    fn char_attrs(&mut self, e: &El) -> CharAttrs {
        let mut a = CharAttrs::default();
        if let Some(f) = e.prop("AppliedFont") {
            let f = f.trim();
            // Some writers append the style after a tab.
            let fam = f.split('\t').next().unwrap_or(f);
            if !fam.is_empty() && fam != "$ID/" {
                a.font_family = Some(fam.to_string());
            }
        }
        a.font_style = e.prop("FontStyle");
        a.size = e.num("PointSize");
        a.leading = e.prop("Leading").and_then(|l| names::leading_in(l.trim()));
        if let Some(v) = e.num("KerningValue") {
            a.kerning = Some(Kerning::Manual(v));
        } else if let Some(k) = e.prop("KerningMethod") {
            a.kerning = Some(match k.trim().trim_start_matches("$ID/") {
                "Optical" => Kerning::Optical,
                "None" => Kerning::None,
                _ => Kerning::Metrics,
            });
        }
        a.tracking = e.num("Tracking");
        a.h_scale = e.num("HorizontalScale").map(|v| v / 100.0);
        a.v_scale = e.num("VerticalScale").map(|v| v / 100.0);
        a.baseline_shift = e.num("BaselineShift");
        a.skew = e.num("Skew");
        if let Some(r) = e.prop("FillColor") {
            a.fill = Some(self.swatch_ref(r.trim()));
        }
        a.fill_tint = tint(e.num("FillTint"));
        if let Some(r) = e.prop("StrokeColor") {
            a.stroke = Some(self.swatch_ref(r.trim()));
        }
        a.stroke_tint = tint(e.num("StrokeTint"));
        a.stroke_weight = e.num("StrokeWeight");
        a.capitalization = e.prop("Capitalization").and_then(|v| names::caps_in(v.trim()));
        a.position = e.prop("Position").and_then(|v| names::position_in(v.trim()));
        a.underline = e.boolean("Underline");
        a.strikethrough = e.boolean("StrikeThru");
        a.ligatures = e.boolean("Ligatures");
        // Underline / strikethrough options (-9999 = automatic).
        for k in ["Underline", "StrikeThru"] {
            let num = |n: &str| e.num(&format!("{k}{n}")).filter(|v| *v > -9000.0);
            let color = e.prop(&format!("{k}Color")).map(|r| self.swatch_ref(r.trim())).filter(|r| r != "Text Color" && !r.is_empty());
            let tint = e.num(&format!("{k}Tint")).filter(|v| *v >= 0.0).map(|v| (v / 100.0) as f32);
            if k == "Underline" {
                a.underline_weight = num("Weight").map(Some);
                a.underline_offset = num("Offset").map(Some);
                a.underline_color = color;
                a.underline_tint = tint;
            } else {
                a.strikethrough_weight = num("Weight").map(Some);
                a.strikethrough_offset = num("Offset").map(Some);
                a.strikethrough_color = color;
                a.strikethrough_tint = tint;
            }
        }
        // OpenType features (only when the element sets any).
        let mut otf_list: Vec<String> = Vec::new();
        let mut any = false;
        for (tag, _, attr) in designcraft_doc::otf::TOGGLES {
            if let Some(on) = e.boolean(attr) {
                designcraft_doc::otf::set(&mut otf_list, tag, on);
                any = true;
            }
        }
        if let Some(fig) = e.prop("OTFFigureStyle")
            && let Some((id, ..)) = designcraft_doc::otf::FIGURES.iter().find(|f| f.3 == fig.trim())
        {
            designcraft_doc::otf::set_figures(&mut otf_list, id);
            any = true;
        }
        if let Some(m) = e.num("OTFStylisticSets") {
            designcraft_doc::otf::set_stylistic_sets(&mut otf_list, m.max(0.0) as u32);
            any = true;
        }
        if any {
            a.otf_features = Some(otf_list);
        }
        a.no_break = e.boolean("NoBreak");
        a.tate_chu_yoko = e.boolean("Tatechuyoko");
        if e.boolean("RubyFlag") == Some(true) {
            a.ruby = e.get("RubyString").map(str::to_string);
        }
        if let Some(k) = e.get("KentenKind") {
            a.kenten = Some(k != "None");
        }
        a.digits = e.get("DigitsType").and_then(names::digits_in);
        if let Some(v) = e.get("AppliedConditions").filter(|v| !v.trim().is_empty()) {
            let names: Vec<String> = v.split_whitespace().filter_map(|r| self.condition_names.get(r).cloned()).collect();
            if !names.is_empty() {
                a.conditions = Some(names);
            }
        }
        a.language = e.prop("AppliedLanguage").map(|l| l.trim().trim_start_matches("$ID/").to_string());
        a
    }

    fn para_attrs(&mut self, e: &El) -> ParaAttrs {
        let mut a = ParaAttrs::default();
        let u = |k: &str| e.num(k).map(|v| v.max(0.0) as u32);
        let frac = |k: &str| e.num(k).map(|v| v / 100.0);
        a.align = e.prop("Justification").and_then(|v| names::align_in(v.trim()));
        a.direction = e.prop("ParagraphDirection").map(|v| {
            if v.trim() == "RightToLeftDirection" { designcraft_doc::TextDirection::RightToLeft } else { designcraft_doc::TextDirection::LeftToRight }
        });
        a.left_indent = e.num("LeftIndent");
        a.right_indent = e.num("RightIndent");
        a.first_line_indent = e.num("FirstLineIndent");
        a.last_line_indent = e.num("LastLineIndent");
        a.space_before = e.num("SpaceBefore");
        if let Some(r) = e.prop("AppliedNumberingList") {
            let r = r.trim().trim_start_matches("NumberingList/");
            a.list_name = Some(if r.starts_with("$ID/") { String::new() } else { unescape_id(r) });
        }
        match e.prop("NumberingContinue").as_deref().map(str::trim) {
            Some("false") => a.start_at = Some(e.num("NumberingStartAt").map(|n| n.max(1.0) as u32)),
            Some("true") => a.start_at = Some(None),
            _ => {}
        }
        // Nested and GREP styles.
        if let Some(l) = e.prop_el("AllNestedStyles") {
            let v: Vec<designcraft_doc::NestedStyle> = l
                .find_all("ListItem")
                .map(|it| {
                    let t = |k: &str| it.find(k).map(|x| x.text_content()).unwrap_or_default();
                    let d = it.find("Delimiter");
                    designcraft_doc::NestedStyle {
                        style: self.char_style_ref(t("AppliedCharacterStyle").trim()),
                        through: t("Inclusive").trim() != "false",
                        count: t("Repetition").trim().parse().unwrap_or(1),
                        until: names::nested_until_in(d.and_then(|d| d.get("type")).unwrap_or("enumeration"), &t("Delimiter")),
                    }
                })
                .collect();
            a.nested_styles = Some(v);
        }
        if let Some(l) = e.prop_el("AllNestedLineStyles") {
            let v: Vec<designcraft_doc::NestedLineStyle> = l
                .find_all("ListItem")
                .map(|it| {
                    let t = |k: &str| it.find(k).map(|x| x.text_content()).unwrap_or_default();
                    designcraft_doc::NestedLineStyle {
                        style: self.char_style_ref(t("AppliedCharacterStyle").trim()),
                        lines: t("LineCount").trim().parse().unwrap_or(1),
                    }
                })
                .collect();
            a.nested_line_styles = Some(v);
        }
        if let Some(l) = e.prop_el("AllGREPStyles") {
            let v: Vec<designcraft_doc::GrepStyle> = l
                .find_all("ListItem")
                .map(|it| {
                    let t = |k: &str| it.find(k).map(|x| x.text_content()).unwrap_or_default();
                    designcraft_doc::GrepStyle { style: self.char_style_ref(t("AppliedCharacterStyle").trim()), pattern: t("GrepExpression") }
                })
                .collect();
            a.grep_styles = Some(v);
        }
        a.space_after = e.num("SpaceAfter");
        a.drop_cap_lines = u("DropCapLines");
        a.drop_cap_chars = u("DropCapCharacters");
        if let Some(g) = e.prop("GridAlignment") {
            a.grid_align = Some(if g == "None" {
                GridAlign::None
            } else if e.boolean("GridAlignFirstLineOnly") == Some(true) {
                GridAlign::FirstLineOnly
            } else {
                GridAlign::AllLines
            });
        }
        a.composer = e
            .prop("Composer")
            .map(|c| if c.contains("Single") { designcraft_doc::Composer::SingleLine } else { designcraft_doc::Composer::Paragraph });
        a.hyphenate = e.boolean("Hyphenation");
        a.hyph_min_word = u("HyphenateWordsLongerThan");
        a.hyph_after_first = u("HyphenateAfterFirst");
        a.hyph_before_last = u("HyphenateBeforeLast");
        a.hyph_limit = u("HyphenateLadderLimit");
        a.hyph_zone = e.num("HyphenationZone");
        a.hyph_capitalized = e.boolean("HyphenateCapitalizedWords");
        a.hyph_last_word = e.boolean("HyphenateLastWord");
        a.hyph_across_column = e.boolean("HyphenateAcrossColumns");
        a.hyph_weight = e.num("HyphenWeight").map(|v| v / 10.0);
        a.word_space_min = frac("MinimumWordSpacing");
        a.word_space_desired = frac("DesiredWordSpacing");
        a.word_space_max = frac("MaximumWordSpacing");
        a.letter_space_min = frac("MinimumLetterSpacing");
        a.letter_space_desired = frac("DesiredLetterSpacing");
        a.letter_space_max = frac("MaximumLetterSpacing");
        a.glyph_scale_min = frac("MinimumGlyphScaling");
        a.glyph_scale_desired = frac("DesiredGlyphScaling");
        a.glyph_scale_max = frac("MaximumGlyphScaling");
        a.auto_leading = frac("AutoLeading");
        a.single_word_justify = e.prop("SingleWordJustification").and_then(|v| names::align_in(v.trim()));
        a.keep_with_next = u("KeepWithNext");
        a.keep_lines_together = e.boolean("KeepLinesTogether");
        a.keep_all_lines = e.boolean("KeepAllLinesTogether");
        a.keep_first = u("KeepFirstLines");
        a.keep_last = u("KeepLastLines");
        a.start_paragraph = e.prop("StartParagraph").and_then(|v| names::start_para_in(v.trim()));
        if let Some(t) = e.prop("SpanColumnType") {
            let n = e.prop("SpanSplitColumnCount").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
            a.span_columns = Some(match t.trim() {
                "SpanColumns" => SpanColumns::Span(n),
                "SplitColumns" => SpanColumns::Split(n.max(2)),
                _ => SpanColumns::Single,
            });
        }
        a.rule_above = self.rule(e, "RuleAbove");
        a.rule_below = self.rule(e, "RuleBelow");
        if let Some(tl) = e.prop_el("TabList") {
            let mut tabs = Vec::new();
            for li in tl.find_all("ListItem") {
                let get = |k: &str| li.find(k).map(|x| x.text_content());
                tabs.push(TabStop {
                    position: get("Position").and_then(|v| v.trim().parse().ok()).unwrap_or(0.0),
                    align: names::tab_align_in(get("Alignment").unwrap_or_default().trim()),
                    leader: get("Leader").unwrap_or_default(),
                    align_on: get("AlignmentCharacter").filter(|c| c != ".").unwrap_or_default(),
                });
            }
            a.tabs = Some(tabs);
        }
        a.list_type = e.prop("BulletsAndNumberingListType").map(|v| match v.trim() {
            "BulletList" => ListType::Bullets,
            "NumberedList" => ListType::Numbers,
            _ => ListType::None,
        });
        a.balance_ragged = e.prop("BalanceRaggedLines").map(|v| v.trim() != "NoBalancing" && v.trim() != "false");
        a.shading_on = e.boolean("ParagraphShadingOn");
        if let Some(c) = e.prop("ParagraphShadingColor") {
            a.shading_color = Some(self.swatch_ref(c.trim()));
        }
        a.shading_tint = tint(e.num("ParagraphShadingTint"));
        let sides = |f: &dyn Fn(&str) -> String| -> Option<[f64; 4]> {
            let v = ["Top", "Left", "Bottom", "Right"].map(|s| e.num(&f(s)));
            v.iter().any(Option::is_some).then(|| v.map(|x| x.unwrap_or(0.0)))
        };
        a.shading_offsets = sides(&|s| format!("ParagraphShading{s}Offset"));
        a.border_on = e.boolean("ParagraphBorderOn");
        if let Some(c) = e.prop("ParagraphBorderColor") {
            a.border_color = Some(self.swatch_ref(c.trim()));
        }
        a.border_tint = tint(e.num("ParagraphBorderTint"));
        a.border_weights = sides(&|s| format!("ParagraphBorder{s}LineWeight"));
        a.border_offsets = sides(&|s| format!("ParagraphBorder{s}Offset"));
        a
    }

    fn rule(&mut self, e: &El, k: &str) -> Option<Rule> {
        let on = e.boolean(k);
        let weight = e.num(&format!("{k}LineWeight"));
        on.or(weight.map(|_| false))?;
        let mut r = Rule { on: on.unwrap_or(false), ..Rule::default() };
        if let Some(w) = weight {
            r.weight = w;
        }
        if let Some(c) = e.prop(&format!("{k}Color")) {
            let c = c.trim();
            if c.contains('/') {
                r.color = self.swatch_ref(c);
            }
        }
        if let Some(t) = tint(e.num(&format!("{k}Tint"))) {
            r.tint = t;
        }
        r.column_width = e.prop(&format!("{k}Width")).is_none_or(|w| w.trim() != "TextWidth");
        r.offset = e.num(&format!("{k}Offset")).unwrap_or(0.0);
        r.left_indent = e.num(&format!("{k}LeftIndent")).unwrap_or(0.0);
        r.right_indent = e.num(&format!("{k}RightIndent")).unwrap_or(0.0);
        Some(r)
    }

    // ---------- stories ----------

    fn story(&mut self, id: StoryId, e: &El) -> Story {
        let mut b = StoryBuilder {
            text: String::new(),
            paras: vec![ParaFormat::default()],
            runs: Vec::new(),
            fresh: true,
            last: CharFormat::default(),
            tables: Vec::new(),
            after_table: None,
            notes: Vec::new(),
            anchors: Vec::new(),
            xrefs: Vec::new(),
            index_refs: Vec::new(),
            objects: Vec::new(),
        };
        self.walk_story(e, &mut b, &ParaFormat::default(), &CharAttrs::default(), &CharFormat::default(), None);
        let StoryBuilder { text, mut paras, runs, last, tables: tbls, notes, anchors, xrefs, index_refs, objects, .. } = b;
        let mut chars: Vec<CharRun> = runs.into_iter().filter(|r| r.len > 0).collect();
        if chars.is_empty() {
            chars.push(CharRun { len: 0, format: last });
        }
        let mut tables = BTreeMap::new();
        for (pi, t) in tbls {
            if let Some(p) = paras.get_mut(pi) {
                p.table = Some(t.id);
                tables.insert(t.id, Arc::new(t));
            }
        }
        let notes = notes.into_iter().enumerate().map(|(i, text)| Arc::new(designcraft_doc::Footnote { id: i as u64 + 1, text })).collect();
        let mut anchor_list = Vec::new();
        for (k, (me, name)) in anchors.into_iter().enumerate() {
            let aid = (id.0 << 24) | (k as u64 + 1);
            self.anchor_ids.insert(me, aid);
            anchor_list.push(Arc::new(designcraft_doc::TextAnchor { id: aid, name }));
        }
        let mut xref_list = Vec::new();
        for (k, (me, fmt)) in xrefs.into_iter().enumerate() {
            self.xref_srcs.insert(me, (id, k));
            let format = self.xref_format_names.get(&fmt).cloned().unwrap_or_else(|| "Full Paragraph & Page Number".into());
            xref_list.push(Arc::new(designcraft_doc::CrossRef { target: 0, format }));
        }
        let mut st = Story {
            id,
            text,
            paras,
            chars,
            frames: vec![],
            rev: 0,
            tables,
            notes,
            endnotes: Vec::new(),
            anchors: anchor_list,
            xrefs: xref_list,
            index_refs: index_refs.into_iter().map(Arc::new).collect(),
            objects: objects.into_iter().map(Arc::new).collect(),
            editorial: Vec::new(),
            link: None,
        };
        st.fix_notes();
        st.fix_marks();
        // Drop anything inconsistent (e.g. a table sharing a paragraph with another).
        if st.check().is_err() {
            st.tables.retain(|_, _| false);
            for p in &mut st.paras {
                p.table = None;
            }
        }
        st
    }

    /// `<Table>`: rows, columns and cells (cell `Name` is `column:row`).
    fn table(&mut self, e: &El) -> Table {
        let id = self.alloc();
        let rows: Vec<&El> = e.find_all("Row").collect();
        let cols: Vec<&El> = e.find_all("Column").collect();
        let nr = rows.len().max(1);
        let nc = cols.len().max(1);
        let mut t = Table::new(id, nr, nc, 0, 0, 0.0);
        for (r, re) in rows.iter().enumerate() {
            let auto = re.boolean("AutoGrow").unwrap_or(true);
            t.rows[r].mode = if auto { RowHeightMode::AtLeast } else { RowHeightMode::Exactly };
            t.rows[r].height = if auto { re.num("MinimumHeight").unwrap_or(3.0) } else { re.num("SingleRowHeight").unwrap_or(12.0) };
        }
        for (c, ce) in cols.iter().enumerate() {
            t.columns[c].width = ce.num("SingleColumnWidth").unwrap_or(72.0).max(3.0);
        }
        let h = e.num("HeaderRowCount").unwrap_or(0.0) as usize;
        let f = e.num("FooterRowCount").unwrap_or(0.0) as usize;
        if h.saturating_add(f) < nr {
            t.set_header_footer(h, f);
        }
        let stroke = |me: &mut Self, el: &El, prefix: &str, base: &CellStroke| -> CellStroke {
            let mut s = base.clone();
            if let Some(w) = el.num(&format!("{prefix}StrokeWeight")) {
                s.weight = w.max(0.0);
            }
            if let Some(c) = el.get(&format!("{prefix}StrokeColor")) {
                s.color = me.swatch_ref(c);
            }
            if let Some(v) = el.num(&format!("{prefix}StrokeTint")) {
                s.tint = tint(Some(v)).unwrap_or(1.0);
            }
            if let Some(v) = el.get(&format!("{prefix}StrokeType")) {
                s.kind = names::stroke_type_in(v);
            }
            s
        };
        t.options.border = stroke(self, e, "TopBorder", &t.options.border);
        t.options.space_before = e.num("SpaceBefore").unwrap_or(t.options.space_before);
        t.options.space_after = e.num("SpaceAfter").unwrap_or(t.options.space_after);
        t.options.repeat_header = e.get("HeaderBehavior") != Some("RepeatOnce");
        t.options.repeat_footer = e.get("FooterBehavior") != Some("RepeatOnce");
        for kind in ["Row", "Column"] {
            let Some(first) = e.num(&format!("Start{kind}FillCount")).filter(|n| *n > 0.0) else { continue };
            let alt = AltFills {
                first: first as u32,
                first_color: e.get(&format!("Start{kind}FillColor")).map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                first_tint: tint(e.num(&format!("Start{kind}FillTint"))).unwrap_or(1.0),
                next: e.num(&format!("End{kind}FillCount")).unwrap_or(1.0) as u32,
                next_color: e.get(&format!("End{kind}FillColor")).map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                next_tint: tint(e.num(&format!("End{kind}FillTint"))).unwrap_or(1.0),
                skip_first: e.num(&format!("SkipFirstAlternatingFill{kind}s")).unwrap_or(0.0) as u32,
                skip_last: e.num(&format!("SkipLastAlternatingFill{kind}s")).unwrap_or(0.0) as u32,
            };
            if kind == "Row" {
                t.options.alt_rows = Some(alt);
            } else {
                t.options.alt_cols = Some(alt);
            }
        }
        t.style = e
            .get("AppliedTableStyle")
            .map(|r| names::style_name_in(names::TABLE_BUILTINS, &unescape_id(r.trim_start_matches("TableStyle/"))))
            .filter(|n| n != designcraft_doc::BASIC_TABLE && n != "[No table style]")
            .unwrap_or_default();
        let mut regions = Vec::new();
        for ce in e.find_all("Cell") {
            let Some((c, r)) =
                ce.get("Name").and_then(|n| n.split_once(':')).and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
            else {
                continue;
            };
            if r >= nr || c >= nc {
                continue;
            }
            let rs = ce.num("RowSpan").unwrap_or(1.0).max(1.0) as usize;
            let cs = ce.num("ColumnSpan").unwrap_or(1.0).max(1.0) as usize;
            let text = self.story(StoryId(0), ce);
            let mut cell = Cell { text, ..Default::default() };
            cell.style = ce
                .get("AppliedCellStyle")
                .map(|r| names::style_name_in(names::CELL_BUILTINS, &unescape_id(r.trim_start_matches("CellStyle/"))))
                .filter(|n| n != designcraft_doc::NO_CELL_STYLE)
                .unwrap_or_default();
            if let Some(fc) = ce.get("FillColor") {
                cell.fill = self.swatch_ref(fc);
            }
            cell.fill_tint = tint(ce.num("FillTint")).unwrap_or(1.0);
            for (i, k) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().enumerate() {
                if let Some(v) = ce.num(k) {
                    cell.insets[i] = v.max(0.0);
                }
            }
            cell.vj = match ce.get("VerticalJustification") {
                Some("CenterAlign") => VerticalJustification::Center,
                Some("BottomAlign") => VerticalJustification::Bottom,
                Some("JustifyAlign") => VerticalJustification::Justify,
                _ => VerticalJustification::Top,
            };
            for (i, side) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().enumerate() {
                let base = cell.strokes[i].clone();
                cell.strokes[i] = stroke(self, ce, side, &base);
            }
            if let Some(slot) = t.cell_mut(r, c) {
                *slot = cell;
            }
            if rs > 1 || cs > 1 {
                regions.push(CellRange { r0: r, c0: c, r1: (r + rs - 1).min(nr - 1), c1: (c + cs - 1).min(nc - 1) });
            }
        }
        for rg in regions {
            let _ = t.merge(rg);
        }
        t
    }

    fn walk_story(&mut self, e: &El, b: &mut StoryBuilder, pf: &ParaFormat, pchars: &CharAttrs, cf: &CharFormat, brk: Option<&str>) {
        for n in &e.children {
            match n {
                Node::El(c) => match c.local() {
                    "ParagraphStyleRange" => {
                        let style = c.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).unwrap_or_else(|| st::BASIC_PARAGRAPH.into());
                        let para = self.para_attrs(c);
                        let chars = self.char_attrs(c);
                        let npf = ParaFormat { style, para, chars: CharAttrs::default(), table: None };
                        if b.fresh
                            && let Some(last) = b.paras.last_mut()
                        {
                            *last = npf.clone();
                        }
                        self.walk_story(c, b, &npf, &chars, cf, None);
                    }
                    "CharacterStyleRange" => {
                        let style = c.get("AppliedCharacterStyle").map(|r| self.char_style_ref(r)).unwrap_or_else(|| st::NO_CHAR_STYLE.into());
                        let mut over = pchars.clone();
                        over.merge(&self.char_attrs(c));
                        let ncf = CharFormat { style, over };
                        let bt = c.get("ParagraphBreakType").filter(|t| *t != "Anywhere");
                        b.last = ncf.clone();
                        self.walk_story(c, b, pf, pchars, &ncf, bt);
                    }
                    "Content" => {
                        for t in &c.children {
                            match t {
                                Node::Text(s) | Node::CData(s) => {
                                    let s: String = s
                                        .chars()
                                        .filter(|c| *c != '\u{FEFF}')
                                        .map(|c| if c == '\n' || c == '\r' || c == '\u{2029}' { st::FORCED_LINE_BREAK } else { c })
                                        .collect();
                                    b.push(&s, cf);
                                }
                                Node::Pi(t, v) if t == "ACE" => {
                                    if let Some(ch) = ace_char(v) {
                                        b.push(&ch.to_string(), cf);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "Br" => match brk {
                        Some(t) => {
                            let ch = match t {
                                "NextColumn" => st::COLUMN_BREAK,
                                "NextFrame" => st::FRAME_BREAK,
                                _ => st::PAGE_BREAK,
                            };
                            b.push(&ch.to_string(), cf);
                        }
                        None => {
                            b.push("\n", cf);
                            b.paras.push(pf.clone());
                            b.fresh = true;
                        }
                    },
                    "Table" => {
                        let t = self.table(c);
                        b.push_table(t, pf, cf);
                    }
                    "HyperlinkTextDestination" | "ParagraphDestination" => {
                        b.anchors.push((c.get("Self").unwrap_or("").to_string(), c.get("Name").unwrap_or("Anchor").to_string()));
                        b.push(&designcraft_doc::ANCHOR_MARK.to_string(), cf);
                    }
                    "PageReference" => {
                        use designcraft_doc::index::IndexRange as R;
                        let topics = c.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).cloned().unwrap_or_default();
                        let range = match c.get("PageReferenceType") {
                            Some("ToEndOfStory") | Some("ToEndOfDocument") | Some("ToEndOfSection") => R::ToEndOfStory,
                            Some("ForNextNParagraphs") => R::NextParagraphs(c.num("PageReferenceLimit").unwrap_or(1.0).max(1.0) as u32),
                            Some("SuppressPageNumbers") => R::SuppressPageRange,
                            _ => R::CurrentPage,
                        };
                        b.index_refs.push(designcraft_doc::IndexRef { topics, sort: vec![], range });
                        b.push(&designcraft_doc::INDEX_MARK.to_string(), cf);
                    }
                    "CrossReferenceSource" => {
                        // The generated text is regenerated from the format.
                        b.xrefs.push((c.get("Self").unwrap_or("").to_string(), c.get("AppliedFormat").unwrap_or("").to_string()));
                        let inner = c.find("CharacterStyleRange");
                        let f = match inner.and_then(|r| r.get("AppliedCharacterStyle")) {
                            Some(r) => CharFormat { style: self.char_style_ref(r), over: pchars.clone() },
                            None => cf.clone(),
                        };
                        b.push(&designcraft_doc::XREF_MARK.to_string(), &f);
                    }
                    "Footnote" => {
                        // The text starts with the number marker (ACE 4, dropped) and the separator.
                        let mut note = self.story(StoryId(0), c);
                        let sep = self.footnote_options.separator.clone();
                        if !sep.is_empty() && note.text.starts_with(&sep) {
                            note.delete(0..sep.len());
                        }
                        b.notes.push(note);
                        // The reference's position comes from the footnote options.
                        let mut rcf = cf.clone();
                        rcf.over.position = None;
                        b.push(&designcraft_doc::FOOTNOTE_REF.to_string(), &rcf);
                    }
                    // Not supported yet: skip their content entirely.
                    "Rectangle" | "Oval" | "Polygon" | "GraphicLine" | "Group" => {
                        // An anchored object (text frames inside stories aren't supported yet).
                        let has_text = c.find_all("TextFrame").next().is_some();
                        if !has_text && let Some(item) = self.item(c, designcraft_geom::Affine::IDENTITY) {
                            use designcraft_doc::anchored::AnchorAlign as A;
                            let set = c.find("AnchoredObjectSetting");
                            let get = |k: &str| set.and_then(|s| s.num(k)).unwrap_or(0.0);
                            let position = match set.and_then(|s| s.get("AnchoredPosition")) {
                                Some("AboveLine") => designcraft_doc::AnchorPosition::AboveLine {
                                    align: match set.and_then(|s| s.get("HorizontalAlignment")) {
                                        Some("CenterAlign") => A::Center,
                                        Some("RightAlign") => A::Right,
                                        _ => A::Left,
                                    },
                                    space_before: get("AnchorSpaceAbove"),
                                    space_after: get("AnchorYoffset"),
                                },
                                Some("Anchored") => {
                                    let s = |k: &str| set.and_then(|x| x.get(k)).unwrap_or("");
                                    let col = match s("HorizontalAlignment") {
                                        "CenterAlign" => 1,
                                        "RightAlign" => 2,
                                        _ => 0,
                                    };
                                    let row = match s("VerticalAlignment") {
                                        "CenterAlign" => 1,
                                        "BottomAlign" => 2,
                                        _ => 0,
                                    };
                                    designcraft_doc::AnchorPosition::Custom {
                                        x_relative: names::anchor_rel_in(s("HorizontalReferencePoint")),
                                        y_relative: names::anchor_rel_in(s("VerticalReferencePoint")),
                                        x_offset: get("AnchorXoffset"),
                                        y_offset: get("AnchorYoffset"),
                                        object_point: names::anchor_in(s("AnchorPoint")),
                                        ref_point: row * 3 + col,
                                        keep_within_column: s("PinPosition") == "true",
                                    }
                                }
                                _ => designcraft_doc::AnchorPosition::Inline { y_offset: get("AnchorYoffset") },
                            };
                            b.objects.push(designcraft_doc::AnchoredObject::new(item, position));
                            b.push(&designcraft_doc::OBJECT_MARK.to_string(), cf);
                        }
                    }
                    "Properties" | "Note" | "TextFrame" | "StoryPreference" | "InCopyExportOption" | "TextVariableInstance" => {}
                    _ => self.walk_story(c, b, pf, pchars, cf, brk),
                },
                Node::Pi(t, v) if t == "ACE" => {
                    if let Some(ch) = ace_char(v) {
                        b.push(&ch.to_string(), cf);
                    }
                }
                _ => {}
            }
        }
    }

    // ---------- spreads and items ----------

    fn spread(&mut self, e: &El, id: SpreadId, master: bool) -> Spread {
        let facing = self.settings.facing_pages;
        // Page rectangles in IDML spread coordinates.
        let mut pages: Vec<(Rect, &El)> = Vec::new();
        for p in e.find_all("Page") {
            let gb = nums(p.get("GeometricBounds").unwrap_or(""));
            let (t, l, bt, r) =
                if gb.len() == 4 { (gb[0], gb[1], gb[2], gb[3]) } else { (0.0, 0.0, self.settings.page_height, self.settings.page_width) };
            let xf = parse_affine(p.get("ItemTransform"));
            let a = xf * Point::new(l, t);
            let c = xf * Point::new(r, bt);
            pages.push((Rect::from_points(a, c), p));
        }
        let min_x = pages.iter().map(|(r, _)| r.x0).fold(f64::INFINITY, f64::min);
        let min_y = pages.iter().map(|(r, _)| r.y0).fold(f64::INFINITY, f64::min);
        let (min_x, min_y) = if min_x.is_finite() { (min_x, min_y) } else { (0.0, 0.0) };
        let shift = Affine::translate((-min_x, -min_y));
        let show = e.get("ShowMasterItems") != Some("false");
        let first_abs: usize = self.spreads.iter().map(|s| s.pages.len()).sum();
        let mut out_pages = Vec::new();
        for (i, (r, p)) in pages.iter().enumerate() {
            let pid = PageId(self.alloc());
            if let Some(s) = p.get("Self")
                && !master
            {
                self.page_index.insert(s.to_string(), first_abs + i);
            }
            let (margins, columns) = match p.find("MarginPreference") {
                Some(m) => margins_of(m),
                None => self.default_margins.clone().unwrap_or((Margins::uniform(36.0), Columns::default())),
            };
            let side = if !facing {
                PageSide::Single
            } else if r.x1 <= 1e-6 {
                PageSide::Left
            } else {
                PageSide::Right
            };
            let parent = p.get("AppliedMaster").and_then(|m| self.parent_ids.get(m)).copied();
            out_pages.push(Page {
                id: pid,
                width: r.width(),
                height: r.height(),
                x: r.x0 - min_x,
                margins,
                columns,
                parent,
                side,
                overridden: vec![],
                guides: Vec::<Guide>::new(),
                show_parent_items: show,
                liquid: Default::default(),
                transition: None,
                view_rotation: 0,
            });
        }
        let overrides: Vec<Vec<String>> =
            pages.iter().map(|(_, p)| p.get("OverrideList").unwrap_or("").split_whitespace().map(str::to_string).collect()).collect();
        let mut items = Vec::new();
        for c in e.elements() {
            if let Some(it) = self.item(c, shift) {
                items.push(Arc::new(it));
            }
        }
        for (pg, ov) in out_pages.iter_mut().zip(overrides) {
            pg.overridden = ov.iter().filter_map(|s| self.item_ids.get(s)).copied().filter(|i| self.is_parent_item(*i)).collect();
        }
        let parent = if master {
            let prefix =
                e.get("NamePrefix").map(str::to_string).unwrap_or_else(|| e.get("Name").unwrap_or("A").split('-').next().unwrap_or("A").to_string());
            let name = e.get("BaseName").map(str::to_string).unwrap_or_else(|| {
                let n = e.get("Name").unwrap_or("Parent");
                n.split_once('-').map(|(_, b)| b.to_string()).unwrap_or_else(|| n.to_string())
            });
            let based_on = pages.first().and_then(|(_, p)| p.get("AppliedMaster")).and_then(|m| self.parent_ids.get(m)).copied().filter(|b| *b != id);
            Some(ParentInfo { prefix, name, based_on })
        } else {
            None
        };
        for p in &mut out_pages {
            if master {
                p.parent = None;
            }
        }
        Spread { id, pages: out_pages, items, parent, allow_shuffle: e.get("AllowPageShuffle") != Some("false") }
    }

    fn is_parent_item(&self, id: ItemId) -> bool {
        self.parents.iter().any(|p| {
            p.items.iter().any(|it| {
                let mut found = false;
                it.walk(&mut |i| found |= i.id == id);
                found
            })
        })
    }

    /// Resolve an item attribute, falling back to its object style chain.
    fn attr_or_style(&self, e: &El, k: &str) -> Option<String> {
        if let Some(v) = e.get(k) {
            return Some(v.to_string());
        }
        let mut os = e.get("AppliedObjectStyle").map(str::to_string);
        let mut depth = 0;
        while let Some(id) = os {
            depth += 1;
            if depth > 16 {
                break;
            }
            let Some(s) = self.object_els.get(&id) else { break };
            if let Some(v) = s.get(k) {
                return Some(v.to_string());
            }
            os = s.prop("BasedOn").map(|b| if b.starts_with("ObjectStyle/") { b } else { format!("ObjectStyle/{}", names::escape_id(&b)) });
        }
        None
    }

    fn stroke_from(&mut self, e: &El, item: Option<&El>) -> Stroke {
        let g = |s: &Self, k: &str| -> Option<String> { if item.is_some() { s.attr_or_style(e, k) } else { e.get(k).map(str::to_string) } };
        let color = g(self, "StrokeColor");
        let mut s = Stroke::none();
        if let Some(c) = color {
            s.swatch = self.swatch_ref(&c);
        }
        if let Some(w) = g(self, "StrokeWeight").and_then(|v| v.parse().ok()) {
            s.weight = w;
        }
        if let Some(t) = g(self, "StrokeTint").and_then(|v| v.parse().ok()) {
            s.tint = tint(Some(t)).unwrap_or(1.0);
        }
        if let Some(t) = g(self, "StrokeType") {
            s.kind = names::stroke_type_in(&t);
        }
        if let Some(v) = g(self, "StrokeAlignment") {
            s.align = names::stroke_align_in(&v);
        }
        if let Some(v) = g(self, "EndCap") {
            s.cap = names::cap_in(&v);
        }
        if let Some(v) = g(self, "EndJoin") {
            s.join = names::join_in(&v);
        }
        if let Some(v) = g(self, "MiterLimit").and_then(|v| v.parse().ok()) {
            s.miter_limit = v;
        }
        if let Some(v) = g(self, "LeftLineEnd") {
            s.start = names::arrow_in(&v);
        }
        if let Some(v) = g(self, "RightLineEnd") {
            s.end = names::arrow_in(&v);
        }
        if let Some(v) = g(self, "GapColor") {
            s.gap_swatch = self.swatch_ref(&v);
        }
        if let Some(t) = g(self, "GapTint").and_then(|v| v.parse().ok()) {
            s.gap_tint = tint(Some(t)).unwrap_or(1.0);
        }
        s.overprint = g(self, "OverprintStroke").as_deref() == Some("true");
        s.gap_overprint = g(self, "OverprintGap").as_deref() == Some("true");
        s
    }

    fn item(&mut self, e: &El, parent: Affine) -> Option<Item> {
        let tag = e.local();
        let shape = match tag {
            "Rectangle" | "TextFrame" => Shape::Rectangle,
            "Oval" => Shape::Oval,
            "Polygon" => Shape::Polygon,
            "GraphicLine" => Shape::GraphicLine,
            "Group" => Shape::Group,
            _ => return None,
        };
        let id = ItemId(self.alloc());
        if let Some(s) = e.get("Self") {
            self.item_ids.insert(s.to_string(), id);
        }
        let layer = e.get("ItemLayer").and_then(|l| self.layer_ids.get(l)).copied().unwrap_or_else(|| self.layers[0].id);
        let path = e.prop_el("PathGeometry").map(path_of).unwrap_or_default();
        let mut it = Item::new(id, layer, shape, path);
        it.xf = parent * parse_affine(e.get("ItemTransform"));
        it.name = e.get("Name").filter(|n| *n != "$ID/").unwrap_or("").to_string();
        it.locked = e.get("Locked") == Some("true");
        it.hidden = e.get("Visible") == Some("false");
        it.nonprinting = e.get("Nonprinting") == Some("true");
        it.object_style = e
            .get("AppliedObjectStyle")
            .map(|r| {
                self.object_names
                    .get(r)
                    .cloned()
                    .unwrap_or_else(|| names::style_name_in(OBJECT_BUILTINS, &unescape_id(r.trim_start_matches("ObjectStyle/"))))
            })
            .unwrap_or_else(|| designcraft_doc::NO_OBJECT_STYLE.into());
        // Fill and stroke.
        if tag != "Group" {
            if let Some(f) = self.attr_or_style(e, "FillColor") {
                it.fill.swatch = self.swatch_ref(&f);
            }
            if let Some(t) = self.attr_or_style(e, "FillTint").and_then(|v| v.parse().ok()) {
                it.fill.tint = tint(Some(t)).unwrap_or(1.0);
            }
            it.fill.overprint = self.attr_or_style(e, "OverprintFill").as_deref() == Some("true");
            if let Some(a) = e.num("GradientFillAngle")
                && a != 0.0
            {
                it.fill.gradient_angle = Some(a);
            }
            let start: Option<Vec<f64>> = e.prop("GradientFillStart").map(|s| s.split_whitespace().filter_map(|v| v.parse().ok()).collect());
            if let (Some([x, y]), Some(len)) = (start.as_deref().and_then(|v| <[f64; 2]>::try_from(v).ok()), e.num("GradientFillLength"))
                && len > 0.0
            {
                let a = e.num("GradientFillAngle").unwrap_or(0.0).to_radians();
                it.fill.gradient_vector = Some([x, y, x + len * a.cos(), y - len * a.sin()]);
            }
            it.stroke = self.stroke_from(e, Some(e));
            if it.stroke.weight <= 0.0 {
                it.stroke.swatch = swatch::NONE.into();
                it.stroke.weight = 1.0;
            }
        }
        // Corners.
        let cn = crate::export::corner_names(&it.path);
        // The legacy all-corners attributes apply only when no per-corner attribute is present.
        let per_corner = cn.iter().any(|n| e.get(&format!("{n}CornerOption")).is_some());
        let legacy = if per_corner { (None, None) } else { (e.get("CornerOption"), e.num("CornerRadius")) };
        let mut corners = CornerOptions::default();
        for (i, n) in cn.iter().enumerate() {
            let shape = e.get(&format!("{n}CornerOption")).or(legacy.0).map(names::corner_in).unwrap_or_default();
            let size = e.num(&format!("{n}CornerRadius")).or(legacy.1).unwrap_or(0.0);
            corners.corners[i] = Corner { shape, size };
        }
        if !corners.is_none() {
            it.corners = corners;
        }
        // Transparency.
        if let Some(t) = e.find("TransparencySetting") {
            if let Some(bs) = t.find("BlendingSetting") {
                if let Some(o) = bs.num("Opacity") {
                    it.opacity = (o / 100.0).clamp(0.0, 1.0) as f32;
                }
                if let Some(m) = bs.get("BlendMode") {
                    it.blend = names::blend_in(m);
                }
            }
            if let Some(ds) = t.find("DropShadowSetting")
                && ds.get("Mode") == Some("Drop")
            {
                let x = ds.num("XOffset").unwrap_or(4.95);
                let y = ds.num("YOffset").unwrap_or(4.95);
                let def = DropShadow::default();
                it.effects.drop_shadow = DropShadow {
                    on: true,
                    color: ds.get("EffectColor").map(|c| self.swatch_ref(c)).unwrap_or(def.color),
                    opacity: ds.num("Opacity").map(|o| (o / 100.0) as f32).unwrap_or(def.opacity),
                    angle: names_angle(x, y),
                    distance: x.hypot(y),
                    size: ds.num("Size").unwrap_or(def.size),
                    spread: ds.num("Spread").unwrap_or(def.spread),
                    global_light: false,
                };
            }
            if let Some(f) = t.find("FeatherSetting")
                && f.get("Mode").is_some_and(|m| m != "None")
            {
                it.effects = Effects { feather: f.num("Width").unwrap_or(9.0), ..it.effects.clone() };
            }
        }
        // Text wrap.
        if let Some(w) = e.find("TextWrapPreference") {
            let mut tw = TextWrap {
                mode: names::wrap_mode_in(w.get("TextWrapMode").unwrap_or("None")),
                invert: w.get("Inverse") == Some("true"),
                side: names::wrap_side_in(w.get("TextWrapSide").unwrap_or("BothSides")),
                ..TextWrap::default()
            };
            if let Some(o) = w.prop_el("TextWrapOffset") {
                tw.offsets =
                    [o.num("Top").unwrap_or(0.0), o.num("Left").unwrap_or(0.0), o.num("Bottom").unwrap_or(0.0), o.num("Right").unwrap_or(0.0)];
            }
            it.wrap = tw;
        }
        // Type on a path.
        if tag != "TextFrame"
            && let Some(tp) = e.find("TextPath")
        {
            let story_self = tp.get("ParentStory").unwrap_or("n").to_string();
            if let Some(story) = self.story_ids.get(&story_self).copied() {
                let options = TextFrameOptions {
                    path: Some(designcraft_doc::PathType {
                        start: tp.num("StartBracket").unwrap_or(0.0).max(0.0),
                        flip: tp.get("FlipPathEffect") == Some("Flipped"),
                        align: names::path_align_in(tp.get("TextAlignment").unwrap_or("")),
                    }),
                    ..Default::default()
                };
                it.content = Content::Text(TextFrame { story, options });
                self.ctx.threads.push((id, story_self, "n".into(), "n".into()));
            }
        }
        // Content.
        match tag {
            "TextFrame" => {
                let story_self = e.get("ParentStory").unwrap_or("n").to_string();
                let story = match self.story_ids.get(&story_self) {
                    Some(s) => *s,
                    None => {
                        let sid = StoryId(self.alloc());
                        self.stories.insert(sid, Story::new(sid));
                        self.story_ids.insert(format!("#orphan{}", sid.0), sid);
                        sid
                    }
                };
                let mut options = e.find("TextFramePreference").map(text_frame_options).unwrap_or_default();
                options.vertical = self.vertical_stories.contains(&story);
                if let Some(g) = e.find("BaselineFrameGridOption")
                    && g.get("UseCustomBaselineFrameGrid") == Some("true")
                {
                    options.baseline_grid =
                        Some((g.num("StartingOffsetForBaselineFrameGrid").unwrap_or(0.0), g.num("BaselineFrameGridIncrement").unwrap_or(12.0)));
                }
                it.content = Content::Text(TextFrame { story, options });
                self.ctx.threads.push((
                    id,
                    story_self,
                    e.get("PreviousTextFrame").unwrap_or("n").to_string(),
                    e.get("NextTextFrame").unwrap_or("n").to_string(),
                ));
            }
            "Group" => {
                let mut kids = Vec::new();
                for c in e.elements() {
                    if let Some(k) = self.item(c, Affine::IDENTITY) {
                        kids.push(Arc::new(k));
                    }
                }
                it.content = Content::Group { items: kids };
                it.fill = Fill::none();
                it.stroke = Stroke::none();
            }
            _ => {
                if let Some(g) = e.elements().find(|c| matches!(c.local(), "Image" | "PDF" | "EPS" | "ImportedPage" | "WMF" | "PICT" | "SVG")) {
                    it.content = self.graphic(g);
                    if let (Some(ff), Content::Graphic(gr)) = (e.elements().find(|c| c.local() == "FrameFittingOption"), &mut it.content) {
                        gr.crop = [ff.num("TopCrop"), ff.num("LeftCrop"), ff.num("BottomCrop"), ff.num("RightCrop")].map(|v| v.unwrap_or(0.0));
                        gr.fit_align = ff.get("FittingAlignment").map_or(4, names::anchor_in);
                        if ff.get("AutoFit") == Some("true") {
                            gr.auto_fit = ff.get("FittingOnEmptyFrame").map_or(designcraft_doc::Fitting::FillProportionally, names::fitting_in);
                            if gr.auto_fit == designcraft_doc::Fitting::None {
                                gr.auto_fit = designcraft_doc::Fitting::FillProportionally;
                            }
                        }
                    }
                } else {
                    // Page items pasted into this frame (Paste Into).
                    let kids: Vec<Arc<Item>> = e
                        .elements()
                        .filter(|c| matches!(c.local(), "Rectangle" | "Oval" | "Polygon" | "GraphicLine" | "Group" | "TextFrame"))
                        .filter_map(|c| self.item(c, Affine::IDENTITY))
                        .map(Arc::new)
                        .collect();
                    if !kids.is_empty() {
                        it.content = Content::Group { items: kids };
                    }
                }
            }
        }
        Some(it)
    }

    fn graphic(&mut self, g: &El) -> Content {
        let gxf = parse_affine(g.get("ItemTransform"));
        let (l, t, r, b) = match g.prop_el("GraphicBounds") {
            Some(gb) => {
                (gb.num("Left").unwrap_or(0.0), gb.num("Top").unwrap_or(0.0), gb.num("Right").unwrap_or(0.0), gb.num("Bottom").unwrap_or(0.0))
            }
            None => (0.0, 0.0, 0.0, 0.0),
        };
        let size = ((r - l).abs(), (b - t).abs());
        let link = g.find("Link");
        let uri = link.and_then(|k| k.get("LinkResourceURI")).map(uri_to_path);
        let mut data = g.prop_el("Contents").map(|c| base64_decode(&c.text_content())).unwrap_or_default();
        if data.is_empty()
            && let Some(p) = &uri
            && let Some(d) = (self.read_link)(p)
        {
            data = d;
        }
        let (mime, px) = sniff_image(&data);
        let mime = mime.map(str::to_string).unwrap_or_else(|| match link.and_then(|k| k.get("LinkResourceFormat")).unwrap_or("") {
            f if f.contains("JPEG") => "image/jpeg".into(),
            f if f.contains("TIFF") => "image/tiff".into(),
            f if f.contains("PDF") || g.local() == "PDF" => "application/pdf".into(),
            f if f.contains("GIF") => "image/gif".into(),
            _ => "image/png".into(),
        });
        let pixels = px.or_else(|| {
            let ppi = nums(g.get("ActualPpi").unwrap_or(""));
            (ppi.len() == 2 && size.0 > 0.0).then(|| ((size.0 * ppi[0] / 72.0).round() as u32, (size.1 * ppi[1] / 72.0).round() as u32))
        });
        let name =
            uri.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| "image".into());
        let id = AssetId(self.alloc());
        let link_path = uri.filter(|p| p.contains('/') || p.contains('\\'));
        self.assets.insert(id, Arc::new(Asset { page: 0, id, name, mime, link: link_path, data: Arc::new(data), pixels }));
        Content::Graphic(designcraft_doc::Graphic {
            asset: id,
            size,
            xf: gxf * Affine::translate((l, t)),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
        })
    }

    fn link_threads(&mut self) {
        let threads = std::mem::take(&mut self.ctx.threads);
        let by_self: HashMap<String, ItemId> = self.item_ids.clone();
        let mut per_story: BTreeMap<StoryId, Vec<usize>> = BTreeMap::new();
        for (i, (id, _, _, _)) in threads.iter().enumerate() {
            let sid = self.frame_story(*id);
            if let Some(sid) = sid {
                per_story.entry(sid).or_default().push(i);
            }
        }
        for (sid, idx) in per_story {
            let members: HashSet<ItemId> = idx.iter().map(|i| threads[*i].0).collect();
            let next_of: HashMap<ItemId, ItemId> =
                idx.iter().filter_map(|i| by_self.get(&threads[*i].3).filter(|n| members.contains(n)).map(|n| (threads[*i].0, *n))).collect();
            let has_prev: HashSet<ItemId> = next_of.values().copied().collect();
            let mut order = Vec::new();
            let mut seen = HashSet::new();
            for i in &idx {
                let head = threads[*i].0;
                if has_prev.contains(&head) || seen.contains(&head) {
                    continue;
                }
                let mut cur = Some(head);
                while let Some(c) = cur {
                    if !seen.insert(c) {
                        break;
                    }
                    order.push(c);
                    cur = next_of.get(&c).copied();
                }
            }
            for i in &idx {
                let f = threads[*i].0;
                if seen.insert(f) {
                    order.push(f);
                }
            }
            if let Some(s) = self.stories.get_mut(&sid) {
                s.frames = order;
            }
        }
    }

    fn frame_story(&self, id: ItemId) -> Option<StoryId> {
        fn find(items: &[Arc<Item>], id: ItemId) -> Option<StoryId> {
            for it in items {
                let mut r = None;
                it.walk(&mut |i| {
                    if i.id == id {
                        r = i.text_frame().map(|t| t.story);
                    }
                });
                if r.is_some() {
                    return r;
                }
            }
            None
        }
        self.spreads.iter().chain(self.parents.iter()).find_map(|s| find(&s.items, id))
    }

    fn finish(mut self, root: &El) -> Result<Document> {
        let title = root.get("Name").map(|n| n.trim_end_matches(".indd").to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Untitled".into());
        if self.spreads.is_empty() {
            return Err(IdmlError::Invalid("document has no spreads".into()));
        }
        self.settings.lists = std::mem::take(&mut self.lists);
        // Make sure built-in paragraph styles exist and are first.
        let d = Document {
            title,
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
            toc: None,
            text_variables: designcraft_doc::vars::defaults(),
            footnote_options: std::mem::take(&mut self.footnote_options),
            xref_formats: std::mem::take(&mut self.xref_formats),
            index: None,
            created: designcraft_doc::vars::now(),
            modified: 0,
            next_id: self.next_id,
        };
        Ok(d)
    }
}

/// Drop-shadow angle (degrees) from IDML offsets (inverse of export's mapping).
fn names_angle(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 135.0;
    }
    let a = y.atan2(-x).to_degrees();
    if a < 0.0 { a + 360.0 } else { a }
}

struct StoryBuilder {
    text: String,
    paras: Vec<ParaFormat>,
    runs: Vec<CharRun>,
    /// The current paragraph has no content yet (its format can still be set by a range).
    fresh: bool,
    last: CharFormat,
    /// Tables by anchor paragraph index.
    tables: Vec<(usize, Table)>,
    /// Content after a table anchor starts a new paragraph (with this format).
    after_table: Option<ParaFormat>,
    /// Footnote texts in reference order.
    notes: Vec<Story>,
    /// Text destinations (Self, name) and cross-reference sources (Self, format Self) in order.
    anchors: Vec<(String, String)>,
    index_refs: Vec<designcraft_doc::IndexRef>,
    xrefs: Vec<(String, String)>,
    objects: Vec<designcraft_doc::AnchoredObject>,
}

impl StoryBuilder {
    /// A table gets an anchor paragraph of its own.
    fn push_table(&mut self, t: Table, pf: &ParaFormat, cf: &CharFormat) {
        let para_empty = self.text.rsplit('\n').next().is_none_or(str::is_empty);
        if !para_empty {
            self.after_table = None;
            self.push("\n", cf);
            self.paras.push(pf.clone());
        }
        self.after_table = None;
        self.push(&st::TABLE_ANCHOR.to_string(), cf);
        self.tables.push((self.paras.len() - 1, t));
        self.after_table = Some(pf.clone());
        self.fresh = false;
    }

    fn push(&mut self, s: &str, f: &CharFormat) {
        if s.is_empty() {
            return;
        }
        if let Some(pf) = self.after_table.take()
            && !s.starts_with('\n')
        {
            self.push("\n", f);
            self.paras.push(pf);
        }
        self.text.push_str(s);
        if !s.contains('\n') || s.len() > 1 {
            self.fresh = false;
        }
        match self.runs.last_mut() {
            Some(r) if r.format == *f => r.len += s.len(),
            _ => self.runs.push(CharRun { len: s.len(), format: f.clone() }),
        }
    }
}

fn ace_char(v: &str) -> Option<char> {
    match v.trim() {
        "18" => Some(st::PAGE_NUMBER),
        "19" => Some(st::SECTION_MARKER),
        "7" => Some(st::INDENT_HERE),
        "8" => Some(st::RIGHT_INDENT_TAB),
        _ => None,
    }
}

fn margins_of(m: &El) -> (Margins, Columns) {
    let g = |k: &str, d: f64| m.num(k).unwrap_or(d);
    (
        Margins { top: g("Top", 36.0), bottom: g("Bottom", 36.0), inside: g("Left", 36.0), outside: g("Right", 36.0) },
        Columns { count: g("ColumnCount", 1.0).max(1.0) as u32, gutter: g("ColumnGutter", 12.0), positions: None },
    )
}

fn text_frame_options(e: &El) -> TextFrameOptions {
    let mut o = TextFrameOptions::default();
    if let Some(v) = e.num("TextColumnCount") {
        o.columns = v.max(1.0) as u32;
    }
    if let Some(v) = e.num("TextColumnGutter") {
        o.gutter = v;
    }
    o.columns_kind = if e.get("UseFixedColumnWidth") == Some("true") {
        ColumnsKind::FixedWidth
    } else if e.get("UseFlexibleColumnWidth") == Some("true") {
        ColumnsKind::FlexibleWidth
    } else {
        ColumnsKind::FixedNumber
    };
    if o.columns_kind != ColumnsKind::FixedNumber
        && let Some(v) = e.num("TextColumnFixedWidth")
    {
        o.column_width = v;
    }
    o.balance_columns = e.get("VerticalBalanceColumns") == Some("true");
    if let Some(l) = e.prop_el("InsetSpacing") {
        let v: Vec<f64> = l.find_all("ListItem").filter_map(|i| i.text_content().trim().parse().ok()).collect();
        if v.len() == 4 {
            o.inset = [v[0], v[1], v[2], v[3]];
        }
    } else if let Some(v) = e.num("InsetSpacing") {
        o.inset = [v; 4];
    }
    if let Some(v) = e.get("VerticalJustification") {
        o.vertical_justification = names::vj_in(v);
    }
    if let Some(v) = e.num("VerticalThreshold") {
        o.vj_paragraph_spacing_limit = v;
    }
    if let Some(v) = e.get("FirstBaselineOffset") {
        o.first_baseline = names::first_baseline_in(v);
    }
    if let Some(v) = e.num("MinimumFirstBaselineOffset") {
        o.first_baseline_min = v;
    }
    o.ignore_wrap = e.get("IgnoreWrap") == Some("true");
    if let Some(v) = e.get("AutoSizingType") {
        o.auto_size = names::auto_size_in(v);
    }
    if let Some(v) = e.get("AutoSizingReferencePoint") {
        o.auto_size_ref = names::REF_POINTS.iter().position(|p| *p == v).unwrap_or(1) as u8;
    }
    o
}

fn path_of(pg: &El) -> PathData {
    let mut subs = Vec::new();
    for g in pg.find_all("GeometryPathType") {
        let closed = g.get("PathOpen") != Some("true");
        let mut anchors = Vec::new();
        if let Some(arr) = g.find("PathPointArray") {
            for p in arr.find_all("PathPointType") {
                let Some(a) = parse_point(p.get("Anchor")) else { continue };
                let hin = parse_point(p.get("LeftDirection")).unwrap_or(a);
                let hout = parse_point(p.get("RightDirection")).unwrap_or(a);
                anchors.push(Anchor::with_handles(a, hin, hout));
            }
        }
        if !anchors.is_empty() {
            subs.push(SubPath::new(anchors, closed));
        }
    }
    PathData::new(subs)
}

/// IDML link URI → file system path (`file:/a%20b` → `/a b`, `file:///C:/x` → `C:/x`).
pub(crate) fn uri_to_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file://").or_else(|| uri.strip_prefix("file:")).unwrap_or(uri);
    // `file:///C:/…` → `/C:/…` → `C:/…`
    let rest = if rest.len() > 3 && rest.starts_with('/') && rest.as_bytes()[2] == b':' { &rest[1..] } else { rest };
    let mut out = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
