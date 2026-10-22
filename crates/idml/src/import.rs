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

/// Preserve the legacy uniform field only when all four sides explicitly specify every
/// property. Partial records must stay sparse so they can inherit from a parent style.
fn uniform_cell_stroke(edges: &[designcraft_doc::CellStrokeAttrs; 4]) -> Option<designcraft_doc::CellStroke> {
    let first = edges.first()?;
    if edges.iter().any(|edge| edge != first) {
        return None;
    }
    let mut stroke = designcraft_doc::CellStroke {
        weight: first.weight?,
        color: first.color.clone()?,
        tint: first.tint?,
        kind: first.kind.clone()?,
        ..Default::default()
    };
    first.apply_to(&mut stroke);
    Some(stroke)
}

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

/// Whether sanitizing a file's tab list dropped, moved or shortened a stop. Sorting, and the `.`
/// a char stop with no character aligns on, change nothing.
fn tab_list_repaired(read: &[TabStop], sanitized: &[TabStop]) -> bool {
    let kept = |t: &TabStop| {
        sanitized.iter().any(|o| {
            o.position == t.position
                && o.leader == t.leader
                && (o.align_on == t.align_on || (t.align_on.is_empty() && o.align == designcraft_doc::TabAlign::Char))
        })
    };
    read.len() != sanitized.len() || !read.iter().all(kept)
}

/// Most entries an IDML package may hold.
const MAX_ENTRIES: usize = 100_000;
/// Most bytes one package part may expand to.
const MAX_PART_BYTES: u64 = 512 << 20;
/// Most bytes all the parts read from one package may expand to together.
const MAX_TOTAL_BYTES: u64 = 2 << 30;

/// An IDML package whose parts are decompressed only when the import asks for them, within
/// byte budgets that don't trust the sizes the archive claims.
struct Package<'a> {
    zip: zip::ZipArchive<std::io::Cursor<&'a [u8]>>,
    /// Normalised part name → entry name, first entry wins (zip order, so the choice is stable).
    lenient: HashMap<String, String>,
    /// Bytes the parts still to be read may expand to.
    left: u64,
}

impl<'a> Package<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self> {
        let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
        if zip.len() > MAX_ENTRIES {
            return Err(IdmlError::NotIdml(format!("{} entries (at most {MAX_ENTRIES})", zip.len())));
        }
        let mut lenient: HashMap<String, String> = HashMap::new();
        for i in 0..zip.len() {
            let Some(name) = zip.name_for_index(i) else { continue };
            if name.ends_with('/') {
                continue;
            }
            lenient.entry(normalize_part_name(name)).or_insert_with(|| name.to_string());
        }
        Ok(Package { zip, lenient, left: MAX_TOTAL_BYTES })
    }

    /// The bytes of part `name` (matched exactly, then leniently), or `None` if the package has
    /// no such part.
    fn part(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let exact = self.zip.index_for_name(name).filter(|_| !name.ends_with('/'));
        let Some(i) = exact.or_else(|| self.lenient.get(&normalize_part_name(name)).and_then(|n| self.zip.index_for_name(n))) else {
            return Ok(None);
        };
        let f = self.zip.by_index(i).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
        let part = f.name().to_string();
        let limit = MAX_PART_BYTES.min(self.left);
        let too_big = || IdmlError::Part {
            part: part.clone(),
            msg: format!("larger than the {MAX_PART_BYTES}-byte part limit (or the package is larger than {MAX_TOTAL_BYTES} bytes)"),
        };
        if f.size() > limit {
            return Err(too_big());
        }
        let mut buf = Vec::new();
        f.take(limit.saturating_add(1)).read_to_end(&mut buf).map_err(|e| IdmlError::Part { part: part.clone(), msg: e.to_string() })?;
        let n = buf.len() as u64;
        if n > limit {
            return Err(too_big());
        }
        self.left = self.left.saturating_sub(n);
        Ok(Some(buf))
    }
}

/// A package part name in the form the lookup compares: InDesign and other IDML writers differ
/// in percent-encoding, case, path separators and a leading `./` or `/`.
fn normalize_part_name(name: &str) -> String {
    let decoded = percent_decode(&name.replace('\\', "/"));
    let mut rest = decoded.as_str();
    loop {
        if let Some(r) = rest.strip_prefix("./") {
            rest = r;
        } else if let Some(r) = rest.strip_prefix('/') {
            rest = r;
        } else {
            break;
        }
    }
    rest.to_lowercase()
}

/// Decode `%XX` escapes; anything else (including a malformed escape) is kept as written.
pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%'
            && let (Some(h), Some(l)) = (b.get(i + 1).and_then(|h| (*h as char).to_digit(16)), b.get(i + 2).and_then(|l| (*l as char).to_digit(16)))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An XMP date property, written as an element or an attribute.
fn xmp_date(xmp: &str, key: &str) -> Option<i64> {
    let value = if let Some(i) = xmp.find(&format!("<{key}>")) {
        let rest = xmp.get(i + key.len() + 2..)?;
        rest.get(..rest.find('<')?)?
    } else {
        let i = xmp.find(&format!("{key}=\""))?;
        let rest = xmp.get(i + key.len() + 2..)?;
        rest.get(..rest.find('"')?)?
    };
    designcraft_doc::vars::parse_iso_date(value)
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

/// IDML stores a sort key on each topic, rather than on its page references.
#[derive(Clone, Default)]
struct IndexTopic {
    path: Vec<String>,
    sort: Vec<String>,
}

impl IndexTopic {
    fn child(&self, topic: &El) -> Self {
        let mut path = self.path.clone();
        path.push(topic.get("Name").unwrap_or("").to_string());
        let mut sort = self.sort.clone();
        if let Some(key) = topic.get("SortOrder").filter(|key| !key.is_empty()) {
            // Keep empty positions before a child's key, but omit trailing defaults.
            sort.resize(self.path.len(), String::new());
            sort.push(key.to_string());
        }
        Self { path, sort }
    }
}

fn lab_to_color(l: f32, a: f32, b: f32) -> Color {
    let rgb = lab::xyz_to_srgb(lab::lab_to_xyz(lab::Lab::new(l, a, b)));
    Color::rgb(rgb[0], rgb[1], rgb[2])
}

fn mojikumi_row(e: &El) -> Result<designcraft_doc::cjk::MojikumiAki> {
    let number = |key: &str| -> Result<f64> {
        let Some(raw) = e.prop(key) else {
            return Ok(0.0);
        };
        raw.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| IdmlError::Invalid(format!("Mojikumi row has invalid {key}")))
    };
    let integer = |key: &str| -> Result<i16> {
        let v = number(key)?;
        if v.fract() != 0.0 || v < f64::from(i16::MIN) || v > f64::from(i16::MAX) {
            return Err(IdmlError::Invalid(format!("Mojikumi {key} must be an in-range integer")));
        }
        Ok(v as i16)
    };
    let boolean = |key: &str| -> Result<bool> {
        match e.prop(key).as_deref().map(str::trim) {
            None | Some("false") => Ok(false),
            Some("true") => Ok(true),
            _ => Err(IdmlError::Invalid(format!("Mojikumi row has invalid {key}"))),
        }
    };
    let row = designcraft_doc::cjk::MojikumiAki {
        target_class: integer("TargetMojikumiClass")?,
        side_class: integer("SideMojikumiClass")?,
        after: boolean("SideIsAfterTarget")?,
        minimum: number("Minimum")?,
        desired: number("Desired")?,
        maximum: number("Maximum")?,
        priority: integer("CompressionPriority")?,
        does_not_float: boolean("AkiDoesNotFloat")?,
    };
    Ok(row)
}

fn nums(s: &str) -> Vec<f64> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

/// Like [`nums`], but empty when any number is NaN or infinite.
fn finite_nums(s: &str) -> Vec<f64> {
    let v = nums(s);
    if v.iter().all(|x| x.is_finite()) { v } else { Vec::new() }
}

/// The viewBox size of an SVG whose root has a viewBox but no width/height (percentages count as
/// none): InDesign places such a file at one viewBox unit to the point.
fn svg_viewbox_size(data: &[u8]) -> Option<(f64, f64)> {
    use quick_xml::events::Event;
    if data.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'<') {
        return None;
    }
    let mut r = quick_xml::Reader::from_reader(data);
    let mut buf = Vec::new();
    let root = loop {
        match r.read_event_into(&mut buf).ok()? {
            Event::Start(e) | Event::Empty(e) => break e.into_owned(),
            Event::Eof => return None,
            _ => buf.clear(),
        }
    };
    if root.local_name().as_ref() != b"svg" {
        return None;
    }
    let attr = |name: &[u8]| {
        root.attributes()
            .with_checks(false)
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.to_string()))
    };
    let absolute = |name: &[u8]| attr(name).is_some_and(|v| !v.trim().ends_with('%'));
    if absolute(b"width") || absolute(b"height") {
        return None;
    }
    let vb: Vec<f64> =
        attr(b"viewBox")?.split(|c: char| c.is_whitespace() || c == ',').filter(|v| !v.is_empty()).filter_map(|v| v.parse().ok()).collect();
    match vb[..] {
        [_, _, w, h] if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 => Some((w, h)),
        _ => None,
    }
}

fn parse_affine(s: Option<&str>) -> Affine {
    match s.map(nums) {
        Some(v) if v.len() == 6 => Affine::new([v[0], v[1], v[2], v[3], v[4], v[5]]),
        _ => Affine::IDENTITY,
    }
}

/// View › Rotate Spread from a `Spread` element's `ItemTransform`: quarter turns clockwise on
/// screen (IDML y points down, so `0 1 -1 0` is one clockwise turn). The spread's items and
/// pages stay in unrotated spread coordinates, so only the rotation part is read. Anything other
/// than an exact quarter turn (identity, scale, other angles, malformed numbers) is no rotation.
fn spread_view_rotation(s: Option<&str>) -> u8 {
    let parsed: std::result::Result<Vec<f64>, _> = s.unwrap_or("").split_whitespace().take(7).map(str::parse::<f64>).collect();
    let Some([a, b, c, d, _, _]) = parsed.ok().and_then(|v| <[f64; 6]>::try_from(v).ok()) else { return 0 };
    let is = |x: f64, y: f64| (x - y).abs() < 1e-6;
    let quarter = |m: [f64; 4]| is(a, m[0]) && is(b, m[1]) && is(c, m[2]) && is(d, m[3]);
    if quarter([0.0, 1.0, -1.0, 0.0]) {
        1
    } else if quarter([-1.0, 0.0, 0.0, -1.0]) {
        2
    } else if quarter([0.0, -1.0, 1.0, 0.0]) {
        3
    } else {
        0
    }
}

fn parse_point(s: Option<&str>) -> Option<Point> {
    let v = nums(s?);
    (v.len() >= 2).then(|| Point::new(v[0], v[1]))
}

/// InDesign's corner size where a file gives a corner shape without one.
const INDESIGN_CORNER_SIZE: f64 = 12.0;

/// The four corners of a paragraph border or shading (`prefix` `ParagraphBorder` /
/// `ParagraphShading`): top left, top right, bottom right, bottom left. Unset when none is given.
fn corners(e: &El, prefix: &str) -> Option<[Corner; 4]> {
    let keys = ["TopLeft", "TopRight", "BottomRight", "BottomLeft"];
    let shapes = keys.map(|n| e.prop(&format!("{prefix}{n}CornerOption")));
    let sizes = keys.map(|n| e.num(&format!("{prefix}{n}CornerRadius")));
    if shapes.iter().all(Option::is_none) && sizes.iter().all(Option::is_none) {
        return None;
    }
    let mut out = [Corner::default(); 4];
    for (c, (shape, size)) in out.iter_mut().zip(shapes.iter().zip(sizes)) {
        c.shape = shape.as_deref().map_or(designcraft_geom::corners::CornerShape::None, |s| names::corner_in(s.trim()));
        c.size = size.filter(|v| v.is_finite()).map_or(12.0, |v| v.max(0.0));
    }
    Some(out)
}

/// IDML tint (percent; `-1` = the colour's own tint, 100 %) → 0..1.
fn tint(v: Option<f64>) -> Option<f32> {
    v.map(|t| if t < 0.0 || t.is_nan() { 1.0 } else { (t / 100.0).min(1.0) as f32 })
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
        if let Some(v) = e.boolean("ColumnGuideLocked") {
            s.column_guides_locked = v;
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
            let kind = match e.local() {
                "StripedStrokeStyle" => designcraft_doc::StrokeType::Stripes {
                    bands: finite_nums(e.get("StripeArray").unwrap_or(""))
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| (c[0] / 100.0, (c[1] - c[0]) / 100.0))
                        .collect(),
                },
                "DashedStrokeStyle" => designcraft_doc::StrokeType::Dashed { pattern: finite_nums(e.get("DashArray").unwrap_or("")) },
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

    /// A `TextFramePreference`, with its column rule colour resolved to a swatch.
    fn frame_options(&mut self, prefs: &[&El]) -> TextFrameOptions {
        let mut o = text_frame_options(prefs);
        if let Some(r) = Prefs(prefs).prop("ColumnRuleStrokeColor") {
            o.column_rule_color = self.swatch_ref(&r);
        }
        o
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
                    .push(designcraft_doc::NumberedList { name, continue_across_stories: e.get("ContinueNumbersAcrossStories") == Some("true") });
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
        for (id, e) in &paras {
            self.para_els.insert(id.clone(), e.clone());
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
        // Keep built-ins as well: [Basic Table] is editable, and [No table style]
        // supplies the defaults used by named styles in ordinary IDML files.
        for (id, e) in &cell_styles {
            self.cell_names.insert(id.clone(), names::style_name_in(names::CELL_BUILTINS, e.get("Name").unwrap_or("")));
        }
        for (id, e) in &table_styles {
            self.table_names.insert(id.clone(), names::style_name_in(names::TABLE_BUILTINS, e.get("Name").unwrap_or("")));
        }
        for (_, e) in &cell_styles {
            let name = names::style_name_in(names::CELL_BUILTINS, e.get("Name").unwrap_or(""));
            let mut cs = designcraft_doc::CellStyle { name: name.clone(), ..Default::default() };
            cs.based_on =
                e.prop("BasedOn").map(|r| self.cell_style_ref(&r)).or_else(|| Some(designcraft_doc::NO_CELL_STYLE.into())).filter(|n| *n != name);
            cs.fill = e.prop("FillColor").map(|c| self.swatch_ref(&c));
            cs.fill_tint = tint(e.num("FillTint"));
            cs.inset_overrides = ["TopInset", "LeftInset", "BottomInset", "RightInset"].map(|k| e.num(k).map(|v| v.max(0.0)));
            if let [Some(top), Some(left), Some(bottom), Some(right)] = cs.inset_overrides {
                cs.insets = Some([top, left, bottom, right]);
                cs.inset_overrides = [None; 4];
            }
            cs.vj = e.get("VerticalJustification").map(names::vj_in);
            cs.first_baseline = e.get("FirstBaselineOffset").map(names::first_baseline_in);
            cs.first_baseline_min = e.num("MinimumFirstBaselineOffset").filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 8640.0));
            cs.strokes = ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].map(|side| self.cell_stroke_attrs(e, side));
            if let Some(stroke) = uniform_cell_stroke(&cs.strokes) {
                cs.stroke = Some(stroke);
                cs.strokes = Default::default();
            }
            cs.paragraph_style = e.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).filter(|n| n != designcraft_doc::NO_PARA_STYLE);
            match self.styles.cell.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = cs,
                None => self.styles.cell.push(cs),
            }
        }
        for (_, e) in &table_styles {
            let name = names::style_name_in(names::TABLE_BUILTINS, e.get("Name").unwrap_or(""));
            let mut ts = designcraft_doc::TableStyle { name: name.clone(), ..Default::default() };
            ts.based_on =
                e.prop("BasedOn").map(|r| self.table_style_ref(&r)).or_else(|| Some(designcraft_doc::NO_TABLE_STYLE.into())).filter(|n| *n != name);
            ts.header = e.get("HeaderRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.body = e.get("BodyRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.footer = e.get("FooterRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.left_column =
                e.get("LeftColumnRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.right_column =
                e.get("RightColumnRegionCellStyle").map(|r| if r == "n" { designcraft_doc::NO_CELL_STYLE.into() } else { self.cell_style_ref(r) });
            ts.header_same_as_body = e.boolean("HeaderRegionSameAsBodyRegion");
            ts.footer_same_as_body = e.boolean("FooterRegionSameAsBodyRegion");
            ts.left_column_same_as_body = e.boolean("LeftColumnRegionSameAsBodyRegion");
            ts.right_column_same_as_body = e.boolean("RightColumnRegionSameAsBodyRegion");
            ts.borders = ["TopBorder", "LeftBorder", "BottomBorder", "RightBorder"].map(|side| self.cell_stroke_attrs(e, side));
            if let Some(border) = uniform_cell_stroke(&ts.borders) {
                ts.border = Some(border);
                ts.borders = Default::default();
            }
            ts.row_fills = self.alt_fill_attrs(e, "Row");
            ts.column_fills = self.alt_fill_attrs(e, "Column");
            ts.row_strokes = self.alt_stroke_attrs(e, "Row");
            ts.column_strokes = self.alt_stroke_attrs(e, "Column");
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
            let text_frame = if e.get("EnableTextFrameGeneralOptions") == Some("true") {
                e.find("TextFramePreference").map(|t| self.frame_options(&[t]))
            } else {
                None
            };
            let effects = self.style_effects(e);
            let s = ObjectStyle { name: name.clone(), based_on, fill, stroke, paragraph_style, text_frame, effects };
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
    fn index_topics_of(&mut self, e: &El, prefix: &IndexTopic) {
        for t in e.find_all("Topic") {
            let topic = prefix.child(t);
            if let Some(me) = t.get("Self") {
                self.index_topics.insert(me.to_string(), topic.clone());
            }
            self.index_topics_of(t, &topic);
        }
        if !prefix.path.is_empty() {
            return;
        }
        let mut stack = vec![(e, IndexTopic::default())];
        while let Some((el, prefix)) = stack.pop() {
            for t in el.find_all("Topic") {
                let topic = prefix.child(t);
                for x in t.find_all("CrossReference") {
                    let target = x.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).map(|t| t.path.join(": ")).unwrap_or_default();
                    use designcraft_doc::index::IndexRange as R;
                    let range = match x.get("CrossReferenceType") {
                        Some("SeeAlso") | Some("SeeAlsoHerein") => R::SeeAlso(target),
                        _ => R::See(target),
                    };
                    self.index_xrefs.push(designcraft_doc::IndexRef { topics: topic.path.clone(), sort: topic.sort.clone(), range });
                }
                stack.push((t, topic));
            }
        }
    }

    /// `CrossReferenceFormat`: building blocks back to a format definition.
    /// A `TextVariable` definition. Types that aren't computed here keep their settings and show
    /// the result of their first instance.
    fn text_variable(&mut self, e: &El) {
        use designcraft_doc::vars::{TextVariable, Use, VAR_MAX, VarKind};
        let ty = e.get("VariableType").unwrap_or("");
        // Cross-reference internals; export writes its own.
        if ty.is_empty() || ty.starts_with("Xref") {
            return;
        }
        let vars = self.text_vars.get_or_insert_with(Vec::new);
        if vars.len() >= VAR_MAX {
            return;
        }
        let pref = e.elements().find(|c| c.local().ends_with("Preference"));
        let attr = |k: &str| pref.and_then(|p| p.get(k)).unwrap_or("");
        let date = || Some(attr("Format")).filter(|f| !f.is_empty()).unwrap_or("MM/dd/yy").to_string();
        let kind = match ty {
            "CustomTextType" => Some(VarKind::Custom { text: attr("Contents").into() }),
            "LastPageNumberType" => Some(VarKind::LastPageNumber { section: attr("Scope") == "SectionScope" }),
            "ChapterNumberType" => Some(VarKind::ChapterNumber),
            "FileNameType" if attr("IncludePath") != "true" => Some(VarKind::FileName { extension: attr("IncludeExtension") == "true" }),
            "CreationDateType" => Some(VarKind::CreationDate { format: date() }),
            "ModificationDateType" => Some(VarKind::ModificationDate { format: date() }),
            "OutputDateType" => Some(VarKind::OutputDate { format: date() }),
            "MatchParagraphStyleType" | "MatchCharacterStyleType" => {
                let character = ty == "MatchCharacterStyleType";
                let r = attr(if character { "AppliedCharacterStyle" } else { "AppliedParagraphStyle" });
                let use_ = if attr("SearchStrategy") == "LastOnPage" { Use::LastOnPage } else { Use::FirstOnPage };
                // Change Case and Delete End Punctuation aren't modelled: the header shows the text as set.
                (!r.is_empty()).then(|| VarKind::RunningHeader {
                    style: if character { self.char_style_ref(r) } else { self.para_style_ref(r) },
                    use_,
                    character,
                })
            }
            _ => None,
        };
        let name = e.get("Name").unwrap_or("").to_string();
        let v = match kind {
            Some(kind) => {
                let mut v = TextVariable::new(&name, kind);
                v.before = attr("TextBefore").into();
                v.after = attr("TextAfter").into();
                v
            }
            None => TextVariable::new(
                &name,
                VarKind::Imported {
                    variable_type: ty.into(),
                    result: String::new(),
                    settings_element: pref.map(|p| p.name.clone()).unwrap_or_default(),
                    settings: pref.map(|p| p.attrs.clone()).unwrap_or_default(),
                },
            ),
        };
        let vars = self.text_vars.get_or_insert_with(Vec::new);
        if let Some(s) = e.get("Self") {
            self.text_var_ids.insert(s.to_string(), vars.len());
        }
        vars.push(v);
    }

    /// A `TextVariableInstance`: the variable's character, or its recorded text when the file
    /// doesn't define the variable.
    fn text_variable_instance(&mut self, e: &El) -> String {
        use designcraft_doc::vars::{VarKind, var_char};
        let result = e.get("ResultText").unwrap_or("");
        let found = e.get("AssociatedTextVariable").and_then(|r| self.text_var_ids.get(r)).copied();
        let var = found.and_then(|i| Some((i, self.text_vars.as_mut()?.get_mut(i)?)));
        match var.and_then(|(i, v)| Some((var_char(i)?, v))) {
            Some((c, v)) => {
                if let VarKind::Imported { result: r, .. } = &mut v.kind
                    && r.is_empty()
                {
                    *r = result.to_string();
                }
                c.to_string()
            }
            None => result.to_string(),
        }
    }

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
        o.rule.tint = tint(e.num("RuleTint")).unwrap_or(1.0);
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

    /// The table of contents: the TOC style a story is generated from (`AppliedTOCStyle`), with
    /// its entries. Styles the document doesn't have fall back to the generated TOC styles.
    fn toc_of(&self, top: &[El]) -> Option<designcraft_doc::Toc> {
        use designcraft_doc::{TocEntry, TocPageNumber};
        let (ts, story) = top.iter().filter(|e| e.local() == "TOCStyle").find_map(|ts| {
            let id = ts.get("Self")?;
            let story = top.iter().find(|e| e.local() == "Story" && e.get("AppliedTOCStyle") == Some(id))?;
            Some((ts, *self.story_ids.get(story.get("Self")?)?))
        })?;
        let known = |r: Option<&str>| r.map(|r| self.para_style_ref(r)).filter(|n| self.styles.para(n).is_some());
        // Character styles the document has; [None] is stored as no style.
        let known_chars = |r: Option<&str>| {
            r.map(|r| self.char_style_ref(r)).filter(|n| n != designcraft_doc::NO_CHAR_STYLE && self.styles.char_style(n).is_some())
        };
        let entries: Vec<TocEntry> = ts
            .elements()
            .filter(|e| e.local() == "TOCStyleEntry")
            .filter_map(|e| {
                let style = names::style_name_in(PARA_BUILTINS, e.get("Name")?);
                if style.is_empty() || style.chars().count() > 256 {
                    return None;
                }
                let level = e.get("Level").and_then(|l| l.trim().parse::<u8>().ok()).unwrap_or(1).clamp(1, 9);
                let page_number = match e.get("PageNumberPosition") {
                    Some("BeforeEntry") => TocPageNumber::Before,
                    Some("NoPageNumber") => TocPageNumber::None,
                    _ => TocPageNumber::After,
                };
                let separator = e.get("Separator").map(crate::names::toc_separator_in).filter(|s| s != "\t" && s.chars().count() <= 64);
                let entry_style = known(e.get("FormatStyle")).filter(|n| *n != format!("TOC Level {level}"));
                let page_number_style = known_chars(e.get("PageNumberStyle"));
                let separator_style = known_chars(e.get("SeparatorStyle"));
                let sort = e.get("Sort").is_some_and(|v| v.trim() == "true");
                Some(TocEntry { style, level, entry_style, page_number, separator, page_number_style, separator_style, sort })
            })
            .take(64)
            .collect();
        if entries.is_empty() {
            return None;
        }
        let title: String = ts.get("Title").unwrap_or("").chars().filter(|c| !matches!(c, '\n' | '\r' | '\u{2029}')).take(1024).collect();
        let title_style = known(ts.get("TitleStyle")).filter(|n| n != "TOC Title");
        Some(designcraft_doc::Toc { story, title, entries, page_numbers: true, title_style })
    }

    fn para_style_ref(&self, r: &str) -> String {
        self.para_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(PARA_BUILTINS, &unescape_id(r.trim_start_matches("ParagraphStyle/"))))
    }
    /// A character style reference written as an element's text (nested, line and GREP styles).
    /// A style name may end in a space, so the reference is used as written when it names a
    /// style; only otherwise is surrounding whitespace dropped.
    fn listed_char_style_ref(&self, r: &str) -> String {
        if self.char_names.contains_key(r) { self.char_style_ref(r) } else { self.char_style_ref(r.trim()) }
    }

    fn char_style_ref(&self, r: &str) -> String {
        self.char_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(CHAR_BUILTINS, &unescape_id(r.trim_start_matches("CharacterStyle/"))))
    }

    fn char_attrs(&mut self, e: &El) -> CharAttrs {
        let mut a = CharAttrs::default();
        if let Some(f) = e.prop("AppliedFont") {
            // Some writers append the style after a tab. Spaces are part of a family's name
            // ("Minion Pro " is another font than "Minion Pro"); line breaks around it aren't.
            let fam = f.split('\t').next().unwrap_or(&f).trim_matches(['\n', '\r']);
            a.font_family = self.composite_names.get(fam.trim()).cloned().or_else(|| names::font_family_in(fam));
        }
        // "Nothing" leaves the style unset: it comes from the paragraph style.
        a.font_style = e.prop("FontStyle").filter(|s| !matches!(s.trim(), "Nothing" | "$ID/Nothing"));
        a.size = e.num("PointSize");
        a.leading = e.prop("Leading").and_then(|l| names::leading_in(l.trim()));
        // Automatic kerning may carry an inactive numeric value (including the
        // 1e11 IDML sentinel). It must not become a multi-million-em advance.
        a.kerning = e.prop("KerningMethod").and_then(|k| match k.trim().trim_start_matches("$ID/") {
            "Optical" => Some(Kerning::Optical),
            "Metrics" | "Metrics - Roman Only" => Some(Kerning::Metrics),
            "None" => Some(Kerning::None),
            _ => None,
        });
        if a.kerning.is_none() {
            a.kerning = e.num("KerningValue").filter(|v| v.is_finite() && v.abs() < 1e10).map(Kerning::Manual);
        }
        a.tracking = e.num("Tracking");
        a.leading_aki = e.num("LeadingAki").map(|v| (v >= 0.0).then_some(v));
        a.trailing_aki = e.num("TrailingAki").map(|v| (v >= 0.0).then_some(v));
        a.tsume = e.num("Tsume").map(|v| (v / 100.0).clamp(0.0, 1.0));
        a.jidori = e.num("Jidori").map(|v| v.clamp(0.0, 10000.0) as u32);
        a.leading_model = e.prop("LeadingModel").and_then(|v| crate::cjk::leading_in(&v));
        a.character_alignment = e.prop("CharacterAlignment").and_then(|v| crate::cjk::alignment_in(&v));
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
        // Text strokes are centred on the outline or lie outside it, behind the fill.
        a.stroke_behind_fill = e.prop("StrokeAlignment").and_then(|v| match v.trim() {
            "OutsideAlignment" => Some(true),
            "CenterAlignment" => Some(false),
            _ => None,
        });
        a.capitalization = e.prop("Capitalization").and_then(|v| names::caps_in(v.trim()));
        a.position = e.prop("Position").and_then(|v| names::position_in(v.trim()));
        a.underline = e.boolean("Underline");
        a.strikethrough = e.boolean("StrikeThru");
        a.ligatures = e.boolean("Ligatures");
        // Underline / strikethrough options (-9999 = automatic).
        for k in ["Underline", "StrikeThru"] {
            let num = |n: &str| e.num(&format!("{k}{n}")).filter(|v| *v > -9000.0);
            // An explicit Text Color resets an inherited swatch; absence still inherits.
            // Resolve the sentinel before swatch_ref, which maps unknown names to [None].
            let color = e.prop(&format!("{k}Color")).map(|r| if r.trim() == "Text Color" { String::new() } else { self.swatch_ref(r.trim()) });
            let tint = tint(e.num(&format!("{k}Tint")));
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
        a.glyph_form = e.prop("GlyphForm");
        a.no_break = e.boolean("NoBreak");
        a.tate_chu_yoko = e.boolean("Tatechuyoko");
        a.tate_chu_yoko_x_offset = e.num("TatechuyokoXOffset");
        a.tate_chu_yoko_y_offset = e.num("TatechuyokoYOffset");
        a.warichu = e.boolean("Warichu");
        a.warichu_lines = e.num("WarichuLines").map(|v| v.max(2.0) as u32);
        a.warichu_size = e.num("WarichuSize");
        a.warichu_line_spacing = e.num("WarichuLineSpacing");
        a.warichu_alignment = e.prop("WarichuAlignment").as_deref().and_then(crate::cjk::warichu_align_in);
        a.warichu_chars_before_break = e.num("WarichuCharsBeforeBreak").map(|v| v.max(0.0) as u32);
        a.warichu_chars_after_break = e.num("WarichuCharsAfterBreak").map(|v| v.max(0.0) as u32);
        // InDesign writes the flag as a number (1); older writers wrote a boolean.
        if let Some(on) = e.boolean("RubyFlag").or_else(|| e.num("RubyFlag").map(|v| v != 0.0)) {
            a.ruby = Some(if on { e.prop("RubyString").unwrap_or_default() } else { String::new() });
        }
        crate::ruby::read(e, &mut a, |r| self.swatch_ref(r));
        if let Some(k) = e.get("KentenKind") {
            a.kenten = Some(k != "None");
            a.kenten_character = crate::cjk::kenten_character(k).map(str::to_string);
            if k == "Custom" {
                a.kenten_character = e.prop("KentenCustomCharacter");
            }
        }
        a.digits = e.prop("DigitsType").and_then(|s| names::digits_in(s.trim()));
        a.character_direction = e.prop("CharacterDirection").and_then(|s| crate::arabic::direction_in(&s));
        a.allow_kashidas = e.prop("Kashidas").and_then(|s| crate::arabic::kashidas_in(&s));
        a.diacritic_position = e.prop("DiacriticPosition").and_then(|s| crate::arabic::diacritic_in(&s));
        a.diacritic_x_offset = e.num("XOffsetDiacritic").filter(|v| v.is_finite());
        a.diacritic_y_offset = e.num("YOffsetDiacritic").filter(|v| v.is_finite());
        a.positional_form = e.prop("PositionalForm");
        self.cjk_settings_chars(e, &mut a);
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
        a.kinsoku_hang = e.prop("KinsokuHangType").and_then(|v| match v.as_str() {
            "None" => Some(designcraft_doc::cjk::KinsokuHang::None),
            "KinsokuHangRegular" => Some(designcraft_doc::cjk::KinsokuHang::Regular),
            "KinsokuHangForce" => Some(designcraft_doc::cjk::KinsokuHang::Force),
            _ => None,
        });
        a.mojikumi = e.prop("Mojikumi").map(|name| self.mojikumi_names.get(&name).cloned().unwrap_or_else(|| unescape_id(&name)));
        a.kinsoku_type = e.prop("KinsokuType");
        a.bunri_kinshi = e.boolean("BunriKinshi");
        a.rensuuji = e.boolean("Rensuuji");
        a.treat_ideographic_space_as_space = e.boolean("TreatIdeographicSpaceAsSpace");
        cjk_settings_para(e, &mut a);
        if let Some(k) = e.prop("KinsokuSet") {
            if k == "Nothing" || k == "None" {
                a.kinsoku = Some(Some(Default::default()));
            } else if let Some(set) = self.kinsoku.get(&k) {
                a.kinsoku = Some(Some(set.clone()));
            } else if let Some(set) = designcraft_doc::cjk::Kinsoku::named(&k) {
                a.kinsoku = Some(Some(set));
            }
        }
        a.direction = e.prop("ParagraphDirection").map(|v| {
            if v.trim() == "RightToLeftDirection" { designcraft_doc::TextDirection::RightToLeft } else { designcraft_doc::TextDirection::LeftToRight }
        });
        a.kashidas = e.prop("Kashidas").and_then(|s| crate::arabic::kashidas_in(&s));
        a.arabic_justification = e.prop("ParagraphJustification");
        a.paragraph_kashida_width = e.num("ParagraphKashidaWidth").filter(|v| v.is_finite()).map(Some);
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
                        style: self.listed_char_style_ref(&t("AppliedCharacterStyle")),
                        through: t("Inclusive").trim() != "false",
                        count: t("Repetition").trim().parse().unwrap_or(1),
                        until: names::nested_until_in(d.and_then(|d| d.get("type")).unwrap_or("enumeration"), &t("Delimiter")),
                    }
                })
                .collect();
            // IDML has no drop cap style attribute: the drop cap's character style is the list's
            // leading "through 1 Dropcap" nested style.
            a.drop_cap_style = v.first().filter(|ns| ns.is_drop_cap()).map(|ns| ns.style.clone());
            a.nested_styles = Some(v);
        }
        if let Some(l) = e.prop_el("AllNestedLineStyles") {
            let v: Vec<designcraft_doc::NestedLineStyle> = l
                .find_all("ListItem")
                .map(|it| {
                    let t = |k: &str| it.find(k).map(|x| x.text_content()).unwrap_or_default();
                    designcraft_doc::NestedLineStyle {
                        style: self.listed_char_style_ref(&t("AppliedCharacterStyle")),
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
                    designcraft_doc::GrepStyle { style: self.listed_char_style_ref(&t("AppliedCharacterStyle")), pattern: t("GrepExpression") }
                })
                .collect();
            a.grep_styles = Some(v);
        }
        a.space_after = e.num("SpaceAfter");
        // A number, or an enumeration (Ignore) that turns it off even over a based-on style's value.
        a.same_para_style_spacing = e.prop("SameParaStyleSpacing").map(|v| v.trim().parse::<f64>().ok().filter(|v| v.is_finite()));
        a.drop_cap_lines = u("DropCapLines");
        a.drop_cap_chars = u("DropCapCharacters");
        // DropcapDetail bits: 1 = Align Left Edge, 2 = Scale for Descenders.
        if let Some(v) = e.num("DropcapDetail").filter(|v| v.is_finite() && *v >= 0.0) {
            let bits = v as u32;
            a.drop_cap_align_left = Some(bits & 1 != 0);
            a.drop_cap_scale_descenders = Some(bits & 2 != 0);
        }
        if let Some(g) = e.prop("GridAlignment") {
            a.grid_align = Some(if g.trim() == "None" {
                GridAlign::None
            } else if e.boolean("GridAlignFirstLineOnly") == Some(true) {
                GridAlign::FirstLineOnly
            } else {
                GridAlign::AllLines
            });
            if g.trim() != "None" {
                a.grid_reference = Some(crate::cjk::grid_reference_in(&g));
            }
        }
        a.composer = e.prop("Composer").map(|c| composer_in(&c));
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
        a.split_inside_gutter = e.num("SplitColumnInsideGutter").filter(|v| v.is_finite());
        a.split_outside_gutter = e.num("SplitColumnOutsideGutter").filter(|v| v.is_finite());
        a.rule_above = self.rule(e, "RuleAbove");
        a.rule_below = self.rule(e, "RuleBelow");
        if let Some(tl) = e.prop_el("TabList") {
            let mut tabs = Vec::new();
            for li in tl.find_all("ListItem") {
                let get = |k: &str| li.find(k).map(|x| x.text_content());
                tabs.push(TabStop {
                    position: get("Position").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0),
                    align: names::tab_align_in(get("Alignment").unwrap_or_default().trim()),
                    leader: get("Leader").unwrap_or_default(),
                    align_on: get("AlignmentCharacter").filter(|c| c != ".").unwrap_or_default(),
                });
            }
            let sanitized = TabStop::sanitized_list(tabs.clone());
            if tab_list_repaired(&tabs, &sanitized) {
                self.repaired_tab_lists = self.repaired_tab_lists.saturating_add(1);
            }
            a.tabs = Some(sanitized);
        }
        a.list_type = e.prop("BulletsAndNumberingListType").map(|v| match v.trim() {
            "BulletList" => ListType::Bullets,
            "NumberedList" => ListType::Numbers,
            _ => ListType::None,
        });
        // A format with no counterpart (CJK, Arabic, Hebrew numbering) is left unset: decimal.
        a.number_style = e.prop("NumberingFormat").and_then(|v| names::numbering_format_in(&v));
        a.number_expression = e.prop("NumberingExpression");
        a.list_separator = e.prop("BulletsTextAfter");
        if let Some(b) = e.prop_el("BulletChar") {
            // A glyph of a named font (`GlyphWithFont`) has no character to map to.
            let unicode = matches!(b.get("BulletCharacterType"), Some("UnicodeOnly" | "UnicodeWithFont"));
            if let Some(c) = b.get("BulletCharacterValue").and_then(|v| v.trim().parse::<u32>().ok()).and_then(char::from_u32).filter(|_| unicode) {
                a.bullet_char = Some(c.to_string());
            }
        }
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
        a.border_type = e.prop("ParagraphBorderType").map(|v| names::stroke_type_in(v.trim()));
        a.border_cap = e.prop("ParagraphBorderStrokeEndCap").map(|v| names::cap_in(v.trim()));
        a.border_join = e.prop("ParagraphBorderStrokeEndJoin").map(|v| names::join_in(v.trim()));
        a.border_corners = corners(e, "ParagraphBorder");
        a.border_width = e.prop("ParagraphBorderWidth").map(|v| names::box_width_in(v.trim()));
        a.border_top = e.prop("ParagraphBorderTopOrigin").map(|v| names::box_top_in(v.trim()));
        a.border_bottom = e.prop("ParagraphBorderBottomOrigin").map(|v| names::box_bottom_in(v.trim()));
        a.border_display_if_splits = e.boolean("ParagraphBorderDisplayIfSplits");
        a.border_merge = e.boolean("MergeConsecutiveParaBorders");
        if let Some(c) = e.prop("ParagraphBorderGapColor") {
            a.border_gap_color = Some(self.swatch_ref(c.trim()));
        }
        a.border_gap_tint = tint(e.num("ParagraphBorderGapTint"));
        a.shading_corners = corners(e, "ParagraphShading");
        a.shading_width = e.prop("ParagraphShadingWidth").map(|v| names::box_width_in(v.trim()));
        a.shading_top = e.prop("ParagraphShadingTopOrigin").map(|v| names::box_top_in(v.trim()));
        a.shading_bottom = e.prop("ParagraphShadingBottomOrigin").map(|v| names::box_bottom_in(v.trim()));
        a.shading_clip = e.boolean("ParagraphShadingClipToFrame");
        a.shading_nonprinting = e.boolean("ParagraphShadingSuppressPrinting");
        a.span_space_before = e.num("SpanColumnMinSpaceBefore").filter(|v| v.is_finite());
        a.span_space_after = e.num("SpanColumnMinSpaceAfter").filter(|v| v.is_finite());
        a.list_level = e.num("NumberingLevel").filter(|v| v.is_finite()).map(|v| v.round().clamp(1.0, designcraft_doc::LIST_LEVELS as f64) as u32);
        if let Some(c) = e.prop("BulletsCharacterStyle") {
            a.bullet_char_style = Some(self.char_style_ref(c.trim()));
        }
        if let Some(c) = e.prop("NumberingCharacterStyle") {
            a.number_char_style = Some(self.char_style_ref(c.trim()));
        }
        a.bullet_align = e.prop("BulletsAlignment").map(|v| names::list_align_in(v.trim()));
        a.number_align = e.prop("NumberingAlignment").map(|v| names::list_align_in(v.trim()));
        a.restart_numbers = e.boolean("NumberingApplyRestartPolicy");
        if let Some(r) = e.prop_el("NumberingRestartPolicies") {
            // A specific level restarts after that level; any other policy after any higher one.
            let level = r.get("LowerLevel").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
            a.restart_after_level = Some(if r.get("RestartPolicy").map(str::trim) == Some("SpecificPreviousLevel") { level } else { 0 });
        }
        if let Some(f) = e.prop("BulletsFont") {
            let f = f.trim();
            a.bullet_font = Some(if f == "$ID/" { String::new() } else { f.split('\t').next().unwrap_or(f).to_string() });
        }
        if let Some(f) = e.prop("BulletsFontStyle") {
            let f = f.trim();
            a.bullet_font_style = Some(if f == "Nothing" || f == "$ID/" { String::new() } else { f.trim_start_matches("$ID/").to_string() });
        }
        a
    }

    fn rule(&mut self, e: &El, k: &str) -> Option<Rule> {
        // A style or range can set some of a rule's settings (such as only its colour); the others
        // come from its paragraph style chain, so the rule is whole.
        let fields = ["", "LineWeight", "Color", "Tint", "Width", "Offset", "LeftIndent", "RightIndent"];
        if !fields.iter().any(|f| e.prop(&format!("{k}{f}")).is_some()) {
            return None;
        }
        let v: Vec<Option<String>> = fields.iter().map(|f| self.para_setting(e, &format!("{k}{f}"))).collect();
        let get = |i: usize| v.get(i).cloned().flatten();
        let num = |i: usize| get(i).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite());
        let on = get(0).map(|v| v.trim() == "true");
        let weight = num(1);
        on.or(weight.map(|_| false))?;
        let mut r = Rule { on: on.unwrap_or(false), ..Rule::default() };
        if let Some(w) = weight {
            r.weight = w;
        }
        if let Some(t) = tint(num(3)) {
            r.tint = t;
        }
        r.column_width = get(4).is_none_or(|w| w.trim() != "TextWidth");
        r.offset = num(5).unwrap_or(0.0);
        r.left_indent = num(6).unwrap_or(0.0);
        r.right_indent = num(7).unwrap_or(0.0);
        if let Some(c) = get(2) {
            let c = c.trim();
            if c == designcraft_doc::TEXT_COLOR {
                r.color = c.to_string();
            } else if c.contains('/') {
                r.color = self.swatch_ref(c);
            }
        }
        Some(r)
    }

    /// Paragraph setting `k` of `e` (a paragraph style or a range): its own, else from its
    /// paragraph style chain (`BasedOn` of a style, the applied style of a range).
    fn para_setting(&self, e: &El, k: &str) -> Option<String> {
        if let Some(v) = e.prop(k) {
            return Some(v);
        }
        let style_ref = |r: String| if r.starts_with("ParagraphStyle/") { r } else { format!("ParagraphStyle/{}", names::escape_id(&r)) };
        let mut next =
            if e.local() == "ParagraphStyle" { e.prop("BasedOn") } else { e.get("AppliedParagraphStyle").map(str::to_string) }.map(style_ref);
        for _ in 0..16 {
            let Some(id) = next else { break };
            let s = self.para_els.get(&id)?;
            if let Some(v) = s.prop(k) {
                return Some(v);
            }
            next = s.prop("BasedOn").map(style_ref);
        }
        None
    }

    fn cell_style_ref(&self, reference: &str) -> String {
        let reference = reference.trim();
        self.cell_names
            .get(reference)
            .cloned()
            .unwrap_or_else(|| names::style_name_in(names::CELL_BUILTINS, &unescape_id(reference.trim_start_matches("CellStyle/"))))
    }

    fn table_style_ref(&self, reference: &str) -> String {
        let reference = reference.trim();
        self.table_names
            .get(reference)
            .cloned()
            .unwrap_or_else(|| names::style_name_in(names::TABLE_BUILTINS, &unescape_id(reference.trim_start_matches("TableStyle/"))))
    }

    fn cell_stroke_attrs(&mut self, el: &El, prefix: &str) -> designcraft_doc::CellStrokeAttrs {
        designcraft_doc::CellStrokeAttrs {
            weight: el.num(&format!("{prefix}StrokeWeight")).map(|v| v.max(0.0)),
            color: el.prop(&format!("{prefix}StrokeColor")).map(|v| self.swatch_ref(&v)),
            tint: tint(el.num(&format!("{prefix}StrokeTint"))),
            kind: el.prop(&format!("{prefix}StrokeType")).map(|v| names::stroke_type_in(&v)),
            gap_color: el.prop(&format!("{prefix}StrokeGapColor")).map(|v| self.swatch_ref(&v)),
            gap_tint: tint(el.num(&format!("{prefix}StrokeGapTint"))),
        }
    }

    fn alt_fill_attrs(&mut self, el: &El, kind: &str) -> designcraft_doc::AltFillsAttrs {
        designcraft_doc::AltFillsAttrs {
            first: el.num(&format!("Start{kind}FillCount")).map(|v| v.max(0.0) as u32),
            first_color: el.prop(&format!("Start{kind}FillColor")).map(|v| self.swatch_ref(&v)),
            first_tint: tint(el.num(&format!("Start{kind}FillTint"))),
            next: el.num(&format!("End{kind}FillCount")).map(|v| v.max(0.0) as u32),
            next_color: el.prop(&format!("End{kind}FillColor")).map(|v| self.swatch_ref(&v)),
            next_tint: tint(el.num(&format!("End{kind}FillTint"))),
            skip_first: el.num(&format!("SkipFirstAlternatingFill{kind}s")).map(|v| v.max(0.0) as u32),
            skip_last: el.num(&format!("SkipLastAlternatingFill{kind}s")).map(|v| v.max(0.0) as u32),
        }
    }

    fn alt_stroke_attrs(&mut self, el: &El, kind: &str) -> designcraft_doc::AltStrokesAttrs {
        let first_stroke = self.cell_stroke_attrs(el, &format!("Start{kind}"));
        let mut next_stroke = self.cell_stroke_attrs(el, &format!("End{kind}"));
        // IDML uses LineStyle for this one attribute (rather than StrokeType).
        if kind == "Column"
            && let Some(value) = el.prop("EndColumnLineStyle")
        {
            next_stroke.kind = Some(names::stroke_type_in(&value));
        }
        designcraft_doc::AltStrokesAttrs {
            first: el.num(&format!("Start{kind}StrokeCount")).map(|v| v.max(0.0) as u32),
            next: el.num(&format!("End{kind}StrokeCount")).map(|v| v.max(0.0) as u32),
            first_stroke,
            next_stroke,
            skip_first: el.num(&format!("SkipFirstAlternatingStroke{kind}s")).map(|v| v.max(0.0) as u32),
            skip_last: el.num(&format!("SkipLastAlternatingStroke{kind}s")).map(|v| v.max(0.0) as u32),
        }
    }

    // ---------- stories ----------

    fn story(&mut self, id: StoryId, e: &El) -> Story {
        self.story_with_para(id, e, ParaFormat::default())
    }

    fn story_with_para(&mut self, id: StoryId, e: &El, default_para: ParaFormat) -> Story {
        let mut b = StoryBuilder {
            text: String::new(),
            paras: vec![default_para.clone()],
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
        self.walk_story(e, &mut b, &default_para, &CharAttrs::default(), &CharFormat::default(), None);
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
            direction: if e.find("StoryPreference").and_then(|p| p.get("StoryDirection")) == Some("RightToLeftDirection") {
                designcraft_doc::TextDirection::RightToLeft
            } else {
                designcraft_doc::TextDirection::LeftToRight
            },
            vertical: e.find("StoryPreference").and_then(|p| p.get("StoryOrientation")) == Some("Vertical"),
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
            file_link: None,
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
        t.options.direction = if e.get("TableDirection") == Some("RightToLeftDirection") {
            designcraft_doc::TextDirection::RightToLeft
        } else {
            designcraft_doc::TextDirection::LeftToRight
        };
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
        let style_name = e.get("AppliedTableStyle").map(|r| self.table_style_ref(r)).unwrap_or_else(|| designcraft_doc::BASIC_TABLE.into());
        let style = self.styles.resolve_table_style(&style_name);
        style.apply_to(&mut t, &self.styles.cell);
        // IDML distinguishes an explicit AppliedCellStyle from the effective region style.
        for cell in &mut t.cells {
            cell.style.clear();
        }
        // Table-local options override the named table style. Cell styles are already
        // materialized independently, so their edges retain their higher precedence.
        let mut borders = std::array::from_fn::<_, 4, _>(|i| t.options.border_for(i).clone());
        for (side, border) in ["TopBorder", "LeftBorder", "BottomBorder", "RightBorder"].iter().zip(&mut borders) {
            self.cell_stroke_attrs(e, side).apply_to(border);
        }
        t.options.border = borders[0].clone();
        t.options.borders = borders.map(Some);
        t.options.space_before = e.num("SpaceBefore").unwrap_or(t.options.space_before);
        t.options.space_after = e.num("SpaceAfter").unwrap_or(t.options.space_after);
        t.options.repeat_header = e.get("HeaderBehavior") != Some("RepeatOnce");
        t.options.repeat_footer = e.get("FooterBehavior") != Some("RepeatOnce");
        // A local count may enable a pattern disabled by its parent style. Merge before
        // materialization so the parent's colors/tints are still available in that case.
        let mut row_fills = style.row_fills.clone();
        row_fills.merge(&self.alt_fill_attrs(e, "Row"));
        row_fills.apply_to(&mut t.options.alt_rows);
        let mut column_fills = style.column_fills.clone();
        column_fills.merge(&self.alt_fill_attrs(e, "Column"));
        column_fills.apply_to(&mut t.options.alt_cols);
        let mut row_strokes = style.row_strokes.clone();
        row_strokes.merge(&self.alt_stroke_attrs(e, "Row"));
        row_strokes.apply_to(&mut t.options.row_strokes);
        let mut column_strokes = style.column_strokes.clone();
        column_strokes.merge(&self.alt_stroke_attrs(e, "Column"));
        column_strokes.apply_to(&mut t.options.column_strokes);
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
            let rs = (span(ce, "RowSpan").max(1.0) as usize).min(nr - r);
            let cs = (span(ce, "ColumnSpan").max(1.0) as usize).min(nc - c);
            let mut cell = t.cell(r, c).cloned().unwrap_or_default();
            let cell_style = ce.get("AppliedCellStyle").map(|r| self.cell_style_ref(r)).unwrap_or_else(|| designcraft_doc::NO_CELL_STYLE.into());
            if cell_style != designcraft_doc::NO_CELL_STYLE {
                designcraft_doc::resolve_cell_style(&self.styles.cell, &cell_style).apply_to(&mut cell);
            }
            cell.style = if cell_style == designcraft_doc::NO_CELL_STYLE { String::new() } else { cell_style };
            let default_para = cell.text.paras.first().cloned().unwrap_or_default();
            cell.text = self.story_with_para(StoryId(0), ce, default_para);
            if let Some(fc) = ce.get("FillColor") {
                cell.fill = self.swatch_ref(fc);
            }
            if let Some(tint) = tint(ce.num("FillTint")) {
                cell.fill_tint = tint;
            }
            for (i, k) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().enumerate() {
                if let Some(v) = ce.num(k) {
                    cell.insets[i] = v.max(0.0);
                }
            }
            if let Some(vj) = ce.get("VerticalJustification") {
                cell.vj = names::vj_in(vj);
            }
            if let Some(fb) = ce.get("FirstBaselineOffset") {
                cell.first_baseline = names::first_baseline_in(fb);
            }
            if let Some(min) = ce.num("MinimumFirstBaselineOffset").filter(|v| v.is_finite()) {
                cell.first_baseline_min = min.clamp(0.0, 8640.0);
            }
            cell.rotation = ce.num("RotationAngle").unwrap_or(cell.rotation);
            for (i, side) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().enumerate() {
                let attrs = self.cell_stroke_attrs(ce, side);
                attrs.apply_to(&mut cell.strokes[i]);
                cell.stroke_defined[i] |= !attrs.is_empty();
                let priority = ce.prop(&format!("{side}StrokePriority")).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
                cell.stroke_priorities[i] = priority;
                // Preserve style-defined edges and #102's positive-priority local overrides.
                cell.border_overrides[i] |= priority > 0;
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

    /// The format of the text in a `CharacterStyleRange`: its style, and its local attributes over
    /// the enclosing paragraph range's, inside a tracked change if `outer` (the enclosing format) is.
    fn range_format(&mut self, c: &El, pchars: &CharAttrs, outer: &CharFormat) -> CharFormat {
        let style = c.get("AppliedCharacterStyle").map(|r| self.char_style_ref(r)).unwrap_or_else(|| st::NO_CHAR_STYLE.into());
        let mut over = pchars.clone();
        over.merge(&self.char_attrs(c));
        over.change = over.change.or(outer.over.change);
        CharFormat { style, over }
    }

    fn walk_story(&mut self, e: &El, b: &mut StoryBuilder, pf: &ParaFormat, pchars: &CharAttrs, cf: &CharFormat, brk: Option<&str>) {
        for n in &e.children {
            match n {
                Node::El(c) => match c.local() {
                    "ParagraphStyleRange" => {
                        let style = c.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).unwrap_or_else(|| pf.style.clone());
                        let para = self.para_attrs(c);
                        let mut chars = self.char_attrs(c);
                        // A paragraph range inside a tracked change is part of that change.
                        chars.change = chars.change.or(cf.over.change);
                        let npf = ParaFormat { style, para, chars: CharAttrs::default(), table: None };
                        if b.fresh
                            && let Some(last) = b.paras.last_mut()
                        {
                            *last = npf.clone();
                        }
                        // Text and markers outside any character range (a destination or a
                        // cross-reference source at paragraph level) take the paragraph's format.
                        let pcf = CharFormat { style: st::NO_CHAR_STYLE.into(), over: chars.clone() };
                        self.walk_story(c, b, &npf, &chars, &pcf, None);
                    }
                    "CharacterStyleRange" => {
                        let ncf = self.range_format(c, pchars, cf);
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
                    "Br" => {
                        // A column, frame or page break ends its paragraph like a return does.
                        if let Some(t) = brk {
                            let ch = match t {
                                "NextColumn" => st::COLUMN_BREAK,
                                "NextFrame" => st::FRAME_BREAK,
                                "NextOddPage" => st::ODD_PAGE_BREAK,
                                "NextEvenPage" => st::EVEN_PAGE_BREAK,
                                _ => st::PAGE_BREAK,
                            };
                            b.push(&ch.to_string(), cf);
                        }
                        b.push("\n", cf);
                        b.paras.push(pf.clone());
                        b.fresh = true;
                    }
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
                        let topic = c.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).cloned().unwrap_or_default();
                        let range = match c.get("PageReferenceType") {
                            Some("ToEndOfStory") | Some("ToEndOfDocument") | Some("ToEndOfSection") => R::ToEndOfStory,
                            Some("ForNextNParagraphs") => R::NextParagraphs(c.num("PageReferenceLimit").unwrap_or(1.0).max(1.0) as u32),
                            Some("SuppressPageNumbers") => R::SuppressPageRange,
                            _ => R::CurrentPage,
                        };
                        b.index_refs.push(designcraft_doc::IndexRef { topics: topic.path, sort: topic.sort, range });
                        b.push(&designcraft_doc::INDEX_MARK.to_string(), cf);
                    }
                    "CrossReferenceSource" => {
                        // The generated text is regenerated from the format.
                        b.xrefs.push((c.get("Self").unwrap_or("").to_string(), c.get("AppliedFormat").unwrap_or("").to_string()));
                        // A source at paragraph level wraps a range; inside a range it holds the
                        // text directly and takes that range's format.
                        let f = c.find("CharacterStyleRange").map_or_else(|| cf.clone(), |r| self.range_format(r, pchars, cf));
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
                    // Anchored page items, including groups containing text frames.
                    "Rectangle" | "Oval" | "Polygon" | "GraphicLine" | "Group" | "TextFrame" => {
                        if let Some(item) = self.item(c, designcraft_geom::Affine::IDENTITY) {
                            use designcraft_doc::anchored::AnchorAlign as A;
                            // Each setting the item leaves unset comes from its object style chain,
                            // then from the IDML defaults.
                            let levels: Vec<&El> = std::iter::once(c).chain(self.object_style_chain(c)).collect();
                            let s = |k: &str| {
                                let set = levels.iter().filter_map(|l| l.find("AnchoredObjectSetting")).find_map(|x| x.get(k));
                                set.unwrap_or(match k {
                                    "AnchoredPosition" => "InlinePosition",
                                    "AnchorPoint" => "BottomRightAnchor",
                                    "HorizontalAlignment" => "LeftAlign",
                                    "HorizontalReferencePoint" => "TextFrame",
                                    "VerticalAlignment" => "BottomAlign",
                                    "VerticalReferencePoint" => "LineBaseline",
                                    "PinPosition" => "true",
                                    _ => "",
                                })
                            };
                            let get = |k: &str| s(k).trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0);
                            let position = match s("AnchoredPosition") {
                                "AboveLine" => designcraft_doc::AnchorPosition::AboveLine {
                                    align: match s("HorizontalAlignment") {
                                        "CenterAlign" => A::Center,
                                        "RightAlign" => A::Right,
                                        _ => A::Left,
                                    },
                                    space_before: get("AnchorSpaceAbove"),
                                    space_after: get("AnchorYoffset"),
                                },
                                "Anchored" => {
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
                                        // IDML measures a left-aligned object's X offset leftwards.
                                        x_offset: if col == 0 { 0.0 - get("AnchorXoffset") } else { get("AnchorXoffset") },
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
                    "TextVariableInstance" => {
                        let t = self.text_variable_instance(c);
                        b.push(&t, cf);
                    }
                    "Change" => {
                        // Tracked changes: deleted text stays in the story, marked, and takes no
                        // room in the layout.
                        let mut ncf = cf.clone();
                        match c.get("ChangeType") {
                            Some("DeletedText") => ncf.over.change = Some(designcraft_doc::ChangeMark::Deleted),
                            Some("InsertedText" | "MovedText") => ncf.over.change = Some(designcraft_doc::ChangeMark::Inserted),
                            _ => {}
                        }
                        self.walk_story(c, b, pf, pchars, &ncf, brk);
                    }
                    "Properties" | "Note" | "StoryPreference" | "InCopyExportOption" => {}
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

    /// Ruler guides of a `<Page>` (its `<Guide>` children), `page` its rectangle in our spread
    /// coordinates. A guide's Location is measured from the ruler origin: the spread's top left, the
    /// page's, or the spine. Fit to Page off: the guide crosses the spread.
    fn guides_of(&self, p: &El, page: Rect, spine: f64) -> Vec<Guide> {
        p.find_all("Guide")
            .filter_map(|g| {
                let at = g.num("Location").filter(|v| v.is_finite() && v.abs() < 1e7)?;
                let orientation = if g.get("Orientation") == Some("Horizontal") { Orientation::Horizontal } else { Orientation::Vertical };
                let position = match (orientation, self.settings.ruler_origin) {
                    (Orientation::Horizontal, designcraft_doc::RulerOrigin::Page) => page.y0 + at,
                    (Orientation::Horizontal, _) => at,
                    (Orientation::Vertical, designcraft_doc::RulerOrigin::Page) => page.x0 + at,
                    (Orientation::Vertical, designcraft_doc::RulerOrigin::Spine) => spine + at,
                    (Orientation::Vertical, designcraft_doc::RulerOrigin::Spread) => at,
                };
                Some(Guide {
                    orientation,
                    position,
                    spread: g.get("FitToPage") == Some("false"),
                    locked: g.get("Locked") == Some("true"),
                    layer: g.get("ItemLayer").and_then(|l| self.layer_ids.get(l)).copied(),
                    liquid: g.get("GuideType") == Some("Liquid"),
                })
            })
            .collect()
    }

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
        // The spine: between a left and a right page (else the spread's left edge).
        let spine = pages.iter().map(|(r, _)| r.x0 - min_x).filter(|x| *x > 1e-6).fold(f64::INFINITY, f64::min);
        let spine = if spine.is_finite() && facing { spine } else { 0.0 };
        let show = e.get("ShowMasterItems") != Some("false");
        let first_abs: usize = self.spreads.iter().map(|s| s.pages.len()).sum();
        let view_rotation = if master { 0 } else { spread_view_rotation(e.get("ItemTransform")) };
        let mut out_pages = Vec::new();
        for (i, (r, p)) in pages.iter().enumerate() {
            let pid = PageId(self.alloc());
            if let Some(s) = p.get("Self")
                && !master
            {
                self.page_index.insert(s.to_string(), first_abs + i);
            }
            let side = if !facing {
                PageSide::Single
            } else if r.x1 <= 1e-6 {
                PageSide::Left
            } else {
                PageSide::Right
            };
            let parent = p.get("AppliedMaster").and_then(|m| self.parent_ids.get(m)).copied();
            // A document page's margins are local when they differ from its parent page's; a
            // page without its own takes the parent page's.
            let from_parent = if master {
                None
            } else {
                parent
                    .and_then(|id| self.parents.iter().find(|sp| sp.id == id))
                    .and_then(|sp| sp.pages.get(sp.parent_page_for_side(side)?))
                    .map(|q| (q.margins, q.columns.clone()))
            };
            let (margins, columns) = match (p.find("MarginPreference"), &from_parent) {
                (Some(m), _) => {
                    let (margins, columns) = margins_of(m);
                    (margins, fit_columns(columns, &margins, r.width()))
                }
                (None, Some(own)) => own.clone(),
                (None, None) => {
                    let (margins, columns) = self.default_margins.clone().unwrap_or((Margins::uniform(36.0), Columns::default()));
                    (margins, fit_columns(columns, &margins, r.width()))
                }
            };
            let local_margins = from_parent.is_some_and(|(m, c)| m != margins || c != columns);
            out_pages.push(Page {
                id: pid,
                width: r.width(),
                height: r.height(),
                x: r.x0 - min_x,
                margins,
                columns,
                local_margins,
                parent,
                side,
                overridden: vec![],
                guides: self.guides_of(p, Rect::new(r.x0 - min_x, r.y0 - min_y, r.x1 - min_x, r.y1 - min_y), spine),
                show_parent_items: show,
                liquid: names::liquid_rule_in(p.get("LayoutRule").unwrap_or("Off")),
                transition: None,
                view_rotation: if i == 0 { view_rotation } else { 0 },
                show_parent_overlay: false,
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
            // A parent spread has one base: its first page's. Pages that disagree lose theirs.
            let applied: Vec<Option<SpreadId>> =
                pages.iter().map(|(_, p)| p.get("AppliedMaster").and_then(|m| self.parent_ids.get(m)).copied()).collect();
            let label = format!("{prefix}-{name}");
            if applied.windows(2).any(|w| w[0] != w[1]) {
                self.warnings.push(format!("parent {label}: its pages are based on different parents; only the first page's is kept"));
            }
            let based_on = applied.first().copied().flatten();
            if applied.contains(&Some(id)) {
                self.warnings.push(format!("parent {label}: a page names its own parent as its base, which is ignored"));
            }
            let based_on = based_on.filter(|b| *b != id);
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

    /// An item's object style chain: its applied style, the styles that one is based on, then
    /// `[None]` (cycle-safe).
    fn object_style_chain(&self, e: &El) -> Vec<&El> {
        let mut chain = self.object_style_bases(e.get("AppliedObjectStyle").map(str::to_string));
        if let Some(none) = self.object_els.get("ObjectStyle/$ID/[None]")
            && !chain.iter().any(|c| std::ptr::eq(*c, none))
        {
            chain.push(none);
        }
        chain
    }

    /// Object style `id` and the styles it is based on, nearest first (cycle-safe, at most 16).
    fn object_style_bases(&self, id: Option<String>) -> Vec<&El> {
        let mut chain: Vec<&El> = Vec::new();
        let mut os = id;
        while let Some(id) = os {
            let Some(s) = self.object_els.get(&id) else { break };
            if chain.len() >= 16 || chain.iter().any(|c| std::ptr::eq(*c, s)) {
                break;
            }
            chain.push(s);
            os = s.prop("BasedOn").map(|b| {
                let b = b.trim();
                if b.starts_with("ObjectStyle/") { b.to_string() } else { format!("ObjectStyle/{}", names::escape_id(b)) }
            });
        }
        chain
    }

    /// Resolve an item attribute, falling back to its object style chain.
    fn attr_or_style(&self, e: &El, k: &str) -> Option<String> {
        e.get(k).or_else(|| self.object_style_chain(e).into_iter().find_map(|s| s.get(k))).map(str::to_string)
    }

    /// The item's child element `name` laid over the same element of each style in its object style
    /// chain: attributes and nested elements merge one by one, nearer levels winning. IDML writes on an
    /// item only the values that differ from its style. Effects whose category the applied style turns
    /// off (`ObjectStyle*EffectsCategorySettings`) come from the item alone.
    fn with_style(&self, e: &El, name: &str) -> Option<El> {
        let chain = self.object_style_chain(e);
        let category = match name {
            "TransparencySetting" => Some("ObjectStyleObjectEffectsCategorySettings"),
            "StrokeTransparencySetting" => Some("ObjectStyleStrokeEffectsCategorySettings"),
            "FillTransparencySetting" => Some("ObjectStyleFillEffectsCategorySettings"),
            "ContentTransparencySetting" => Some("ObjectStyleContentEffectsCategorySettings"),
            _ => None,
        };
        let enabled = |effect: &str| {
            let Some(flag) = category.and_then(|_| effect_flag(effect)) else { return true };
            chain.iter().find_map(|s| category.and_then(|c| s.find(c)).and_then(|c| c.get(flag))) != Some("false")
        };
        let mut out: Option<El> = None;
        for s in chain.iter().rev() {
            let Some(c) = s.find(name) else { continue };
            let mut c = c.clone();
            c.children.retain(|n| !matches!(n, Node::El(x) if !enabled(x.local())));
            match &mut out {
                Some(o) => overlay(o, &c, 0),
                None => out = Some(c),
            }
        }
        if let Some(c) = e.find(name) {
            match &mut out {
                Some(o) => overlay(o, c, 0),
                None => out = Some(c.clone()),
            }
        }
        out
    }

    /// Reads the drop shadow, feather, gradient feather and directional feather of a
    /// `TransparencySetting` into `fx`.
    fn effects_in(&mut self, t: &El, fx: &mut Effects) {
        if let Some(ds) = t.find("DropShadowSetting")
            && ds.get("Mode") == Some("Drop")
        {
            let x = ds.num("XOffset").unwrap_or(4.95);
            let y = ds.num("YOffset").unwrap_or(4.95);
            let def = DropShadow::default();
            fx.drop_shadow = DropShadow {
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
            fx.feather = f.num("Width").unwrap_or(9.0);
            fx.feather_options.choke = finite(f.num("ChokeAmount")).unwrap_or(0.0).clamp(0.0, 100.0);
            fx.feather_options.corners = match f.get("CornerType") {
                Some("Sharp") => designcraft_doc::FeatherCorners::Sharp,
                Some("Rounded") => designcraft_doc::FeatherCorners::Rounded,
                _ => designcraft_doc::FeatherCorners::Diffused,
            };
        }
        if let Some(g) = t.find("GradientFeatherSetting")
            && g.boolean("Applied") == Some(true)
        {
            fx.gradient_feather = gradient_feather_in(g);
        }
        if let Some(df) = t.find("DirectionalFeatherSetting")
            && df.boolean("Applied") == Some(true)
        {
            let w = |k: &str| finite(df.num(k)).unwrap_or(0.0).max(0.0);
            fx.directional_feather = DirectionalFeather { on: true, widths: [w("TopWidth"), w("LeftWidth"), w("BottomWidth"), w("RightWidth")] };
        }
    }

    /// An object style's drop shadow and feather. Each is read when the effects category turns it
    /// on (without category settings: when its setting is on), from the nearest style in the
    /// BasedOn chain that has the setting. `None` when the style sets neither.
    fn style_effects(&mut self, e: &El) -> Option<Effects> {
        let mut chain = self.object_style_bases(e.get("Self").map(str::to_string));
        if chain.is_empty() {
            chain.push(e);
        }
        let category = chain.iter().find_map(|s| s.find("ObjectStyleObjectEffectsCategorySettings")).cloned();
        let setting = |name: &str| chain.iter().find_map(|s| s.find("TransparencySetting")?.find(name)).cloned();
        let mut t = El::new("TransparencySetting");
        let mut any = false;
        for (name, flag) in [("DropShadowSetting", "EnableDropShadow"), ("FeatherSetting", "EnableFeather")] {
            let el = setting(name);
            let on = match &category {
                Some(c) => c.get(flag) == Some("true"),
                None => el.as_ref().and_then(|x| x.get("Mode")).is_some_and(|m| m != "None"),
            };
            any |= on;
            if let (true, Some(el)) = (on, el) {
                t.push(el);
            }
        }
        if !any {
            return None;
        }
        let mut fx = Effects::default();
        self.effects_in(&t, &mut fx);
        Some(fx)
    }

    fn stroke_from(&mut self, e: &El, item: Option<&El>) -> Stroke {
        let g = |s: &Self, k: &str| -> Option<String> { if item.is_some() { s.attr_or_style(e, k) } else { e.get(k).map(str::to_string) } };
        let color = g(self, "StrokeColor");
        let mut s = Stroke::none();
        if let Some(c) = color {
            s.swatch = self.swatch_ref(&c);
        }
        if let Some(w) = g(self, "StrokeWeight").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()) {
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
        if let Some(v) = g(self, "MiterLimit").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()) {
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
        it.overrides = e.get("OverriddenPageItem").and_then(|s| self.item_ids.get(s)).copied().filter(|i| self.is_parent_item(*i));
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
            let start: Option<Vec<f64>> = e.prop("GradientFillStart").map(|s| finite_nums(&s));
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
            // A gradient on the stroke: its angle, or the Gradient Swatch tool's vector.
            if let Some(a) = e.num("GradientStrokeAngle")
                && a != 0.0
            {
                it.stroke.gradient_angle = Some(a);
            }
            let start: Option<Vec<f64>> = e.prop("GradientStrokeStart").map(|s| finite_nums(&s));
            if let (Some([x, y]), Some(len)) = (start.as_deref().and_then(|v| <[f64; 2]>::try_from(v).ok()), e.num("GradientStrokeLength"))
                && len > 0.0
            {
                let a = e.num("GradientStrokeAngle").unwrap_or(0.0).to_radians();
                it.stroke.gradient_vector = Some([x, y, x + len * a.cos(), y - len * a.sin()]);
            }
        }
        // Corners: each corner's shape and radius come from the item, then its object style chain;
        // at each level a per-corner attribute wins over the legacy all-corners one.
        if tag != "Group" {
            let levels: Vec<&El> = std::iter::once(e).chain(self.object_style_chain(e)).collect();
            let corner =
                |n: &str, what: &str| levels.iter().find_map(|s| s.get(&format!("{n}Corner{what}")).or_else(|| s.get(&format!("Corner{what}"))));
            let mut corners = CornerOptions::default();
            for (i, n) in crate::export::corner_names(&it.path).iter().enumerate() {
                let shape = corner(n, "Option").map(names::corner_in).unwrap_or_default();
                let size = corner(n, "Radius").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(INDESIGN_CORNER_SIZE);
                corners.corners[i] = Corner { shape, size };
            }
            if !corners.is_none() {
                it.corners = corners;
            }
        }
        // Transparency.
        if let Some(t) = self.with_style(e, "TransparencySetting") {
            if let Some(bs) = t.find("BlendingSetting") {
                if let Some(o) = bs.num("Opacity") {
                    it.opacity = (o / 100.0).clamp(0.0, 1.0) as f32;
                }
                if let Some(m) = bs.get("BlendMode") {
                    it.blend = names::blend_in(m);
                }
            }
            self.effects_in(&t, &mut it.effects);
        }
        // Text wrap.
        if let Some(w) = self.with_style(e, "TextWrapPreference") {
            let mut tw = text_wrap(&w);
            if let Some(pg) = w.prop_el("PathGeometry") {
                match wrap_path_of(pg) {
                    Ok(p) => tw.path = Some(p),
                    Err(why) => log::warn!("IDML: custom text wrap boundary of {} dropped: {why}", e.get("Self").unwrap_or("an item")),
                }
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
                        // At (or past) the path's length it is the path's end: no end bracket.
                        end: tp
                            .num("EndBracket")
                            .filter(|e| e.is_finite() && *e >= 0.0)
                            .filter(|e| *e < designcraft_geom::warp::PathWarp::new(&it.path.to_bezpath(), false).length() - 0.01),
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
                // Options the frame doesn't set come from its object style chain.
                let prefs: Vec<&El> = std::iter::once(e).chain(self.object_style_chain(e)).filter_map(|s| s.find("TextFramePreference")).collect();
                let mut options = text_frame_options(&prefs);
                // `prefs` borrows the object styles, so the rule colour's swatch is resolved after it.
                if let Some(r) = Prefs(&prefs).prop("ColumnRuleStrokeColor") {
                    options.column_rule_color = self.swatch_ref(&r);
                }
                if let Some(g) = self.with_style(e, "BaselineFrameGridOption")
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
                    let fill = self.image_fill(g);
                    if let Content::Graphic(gr) = &mut it.content {
                        gr.fill = fill;
                    }
                    if let (Some(ff), Content::Graphic(gr)) = (self.with_style(e, "FrameFittingOption"), &mut it.content) {
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

    /// The swatch applied to a placed image (`FillColor`, `FillTint`): it colours a greyscale or
    /// bitmap image. Black at 100% is how InDesign writes an image nobody coloured: no swatch.
    fn image_fill(&mut self, g: &El) -> Fill {
        let Some(r) = g.get("FillColor") else { return Fill::none() };
        let swatch = self.swatch_ref(r);
        let tint = tint(g.num("FillTint")).unwrap_or(1.0);
        if swatch == swatch::BLACK && tint >= 1.0 {
            return Fill::none();
        }
        Fill { swatch, tint, ..Fill::none() }
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
        let wrap =
            if g.get("AppliedObjectStyle").is_some() { self.with_style(g, "TextWrapPreference") } else { g.find("TextWrapPreference").cloned() };
        let wrap = wrap.map(|w| text_wrap(&w)).unwrap_or_default();
        let clip = clipping_path(g, Affine::translate((-l, -t)));
        let link = g.find("Link");
        let uri = link.and_then(|k| k.get("LinkResourceURI")).map(uri_to_path);
        let graphic = |asset, size| {
            Content::Graphic(designcraft_doc::Graphic {
                asset,
                size,
                xf: gxf * Affine::translate((l, t)),
                clip: clip.clone(),
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                // A wrap set on the placed graphic (Direct Selection) is stored on the graphic element,
                // over the graphic's own object style when it names one.
                wrap,
                fill: Default::default(),
            })
        };
        let mut data = g.prop_el("Contents").map(|c| base64_decode(&c.text_content())).unwrap_or_default();
        let page = pdf_page_in(g);
        // Where the box sits on the page needs the PDF parsed: the engine finds it after import.
        let pdf_crop = pdf_attribute(g).and_then(|a| a.get("PDFCrop")).map_or(designcraft_doc::PdfCrop::Crop, names::pdf_crop_in);
        let mut read_from = None;
        if data.is_empty()
            && let Some(p) = &uri
        {
            // Every placement of a linked file with one page and crop shares one asset; read the
            // file once.
            let key = (p.clone(), page, pdf_crop);
            if let Some(id) = self.linked_assets.get(&key) {
                let size = self.assets.get(id).and_then(|a| svg_viewbox_size(&a.data)).unwrap_or(size);
                return graphic(*id, size);
            }
            if let Some(d) = (self.read_link)(p) {
                data = d;
                read_from = Some(key);
            }
        }
        // A linked file that can't be read shows the document's preview of it, if it has one. The
        // preview only stands in for the file: format and pixel size still describe the link.
        let preview = data.is_empty().then(|| g.get("Self").and_then(|s| self.previews.get(s))).flatten();
        // InDesign draws a viewBox-only SVG at one unit to the point: its GraphicBounds are the
        // art's bounds, which can overhang the viewBox, so they must not size the viewBox.
        let size = svg_viewbox_size(&data).unwrap_or(size);
        let (mime, px) = if preview.is_some() { (None, None) } else { sniff_image(&data) };
        let mime = mime.map(str::to_string).unwrap_or_else(|| match link.and_then(|k| k.get("LinkResourceFormat")).unwrap_or("") {
            f if f.contains("EPS") || g.local() == "EPS" => "application/postscript".into(),
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
        let fresh = AssetId(self.alloc());
        let link_path = uri.filter(|p| p.contains('/') || p.contains('\\'));
        let id = designcraft_doc::insert_asset(
            &mut self.assets,
            Asset {
                page,
                id: fresh,
                name,
                mime,
                link: link_path,
                data: preview.cloned().unwrap_or_else(|| Arc::new(data)),
                pixels,
                pdf_crop,
                ..Default::default()
            },
        );
        if let Some(key) = read_from {
            self.linked_assets.insert(key, id);
        }
        graphic(id, size)
    }

    fn link_threads(&mut self) {
        let threads = std::mem::take(&mut self.ctx.threads);
        let by_self: HashMap<String, ItemId> = self.item_ids.clone();
        let mut per_story: BTreeMap<StoryId, Vec<usize>> = BTreeMap::new();
        for (i, (id, story, _, _)) in threads.iter().enumerate() {
            let sid = self.story_ids.get(story).copied().or_else(|| self.frame_story(*id));
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

    /// A nested style list replaces the one a style or paragraph inherits, and with it the drop
    /// cap style (the list's leading "through 1 Dropcap" entry, read into the drop cap style). A
    /// list of its own without that entry clears an inherited drop cap style.
    fn clear_inherited_drop_cap_styles(&mut self) {
        let clears = |a: &designcraft_doc::ParaAttrs| a.drop_cap_style.is_none() && a.nested_styles.is_some();
        let cleared: Vec<String> = self
            .styles
            .paragraph
            .iter()
            .filter(|s| clears(&s.para) && inherits_drop_cap_style(&self.styles, s.based_on.as_deref()))
            .map(|s| s.name.clone())
            .collect();
        for name in cleared {
            if let Some(s) = self.styles.para_mut(&name) {
                s.para.drop_cap_style = Some(st::NO_CHAR_STYLE.into());
            }
        }
        for story in self.stories.values_mut() {
            clear_story_drop_cap_styles(&self.styles, story, 0);
        }
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

/// The object style category flag that turns an effect element on or off.
fn effect_flag(effect: &str) -> Option<&'static str> {
    Some(match effect {
        "BlendingSetting" => "EnableTransparency",
        "DropShadowSetting" => "EnableDropShadow",
        "FeatherSetting" => "EnableFeather",
        "InnerShadowSetting" => "EnableInnerShadow",
        "OuterGlowSetting" => "EnableOuterGlow",
        "InnerGlowSetting" => "EnableInnerGlow",
        "BevelAndEmbossSetting" => "EnableBevelEmboss",
        "SatinSetting" => "EnableSatin",
        "DirectionalFeatherSetting" => "EnableDirectionalFeather",
        "GradientFeatherSetting" => "EnableGradientFeather",
        _ => return None,
    })
}

/// Lays `top` over `base`: attributes one by one, child elements matched by name and merged the same
/// way, text replaced. A property given in one form (attribute or `Properties` child) replaces the
/// other form in `base`.
fn overlay(base: &mut El, top: &El, depth: usize) {
    if depth > 32 {
        *base = top.clone();
        return;
    }
    for (k, v) in &top.attrs {
        base.set(k, v);
        if let Some(Node::El(p)) = base.children.iter_mut().find(|n| matches!(n, Node::El(p) if p.local() == "Properties")) {
            p.children.retain(|n| !matches!(n, Node::El(x) if x.local() == k));
        }
    }
    if let Some(p) = top.find("Properties") {
        let names: Vec<&str> = p.elements().map(El::local).collect();
        base.attrs.retain(|(k, _)| !names.contains(&k.as_str()));
    }
    let has_text = top.children.iter().any(|n| matches!(n, Node::Text(t) | Node::CData(t) if !t.trim().is_empty()));
    if has_text {
        base.children.retain(|n| !matches!(n, Node::Text(_) | Node::CData(_)));
    }
    // The k-th child of a name merges with the k-th child of that name in `base` (repeated elements
    // such as gradient stops pair up in order).
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for n in &top.children {
        match n {
            Node::El(c) => {
                let k = seen.entry(c.local()).or_insert(0);
                let slot = base
                    .children
                    .iter_mut()
                    .filter_map(|b| match b {
                        Node::El(b) if b.local() == c.local() => Some(b),
                        _ => None,
                    })
                    .nth(*k);
                *k += 1;
                match slot {
                    Some(b) => overlay(b, c, depth + 1),
                    None => base.children.push(n.clone()),
                }
            }
            Node::Text(_) | Node::CData(_) if has_text => base.children.push(n.clone()),
            _ => {}
        }
    }
}

/// A cell span count; an infinite span is kept so it clamps to the table edge.
fn span(e: &El, k: &str) -> f64 {
    e.prop(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(1.0)
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite())
}

/// A `GradientFeatherSetting`. The model holds two opacity stops at the ends of the gradient: the
/// first and last `OpacityGradientStop` give them; with a linear vector, their `Location`s move its
/// ends.
fn gradient_feather_in(g: &El) -> GradientFeather {
    let def = GradientFeather::default();
    let angle = finite(g.num("Angle")).unwrap_or(0.0);
    let mut stops: Vec<(f64, f64)> = g
        .find_all("OpacityGradientStop")
        .filter_map(|s| Some((finite(s.num("Location"))?.clamp(0.0, 100.0) / 100.0, finite(s.num("Opacity"))?.clamp(0.0, 100.0) / 100.0)))
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (first, last) = match (stops.first(), stops.last()) {
        (Some(&a), Some(&b)) => (a, b),
        _ => ((0.0, def.start as f64), (1.0, def.end as f64)),
    };
    let start: Option<[f64; 2]> = g
        .prop("GradientStart")
        .and_then(|s| s.split_whitespace().map(|v| v.parse().ok().filter(|v: &f64| v.is_finite())).collect::<Option<Vec<f64>>>())
        .and_then(|v| v.try_into().ok());
    let radial = g.get("Type") == Some("Radial");
    let vector = match (start, finite(g.num("Length"))) {
        (Some([x, y]), Some(len)) if len > 0.0 => {
            let a = angle.to_radians();
            let (dx, dy) = (len * a.cos(), -len * a.sin());
            let (l0, l1) = if !radial && last.0 > first.0 { (first.0, last.0) } else { (0.0, 1.0) };
            Some([x + dx * l0, y + dy * l0, x + dx * l1, y + dy * l1])
        }
        _ => None,
    };
    GradientFeather { on: true, radial, angle, start: first.1 as f32, end: last.1 as f32, vector }
}

/// Drop-shadow angle (degrees) from IDML offsets (inverse of export's mapping).
/// A `TextWrapPreference` element (of a page item or a placed graphic).
fn text_wrap(w: &El) -> TextWrap {
    let mut tw = TextWrap {
        mode: names::wrap_mode_in(w.get("TextWrapMode").unwrap_or("None")),
        invert: w.get("Inverse") == Some("true"),
        side: names::wrap_side_in(w.get("TextWrapSide").unwrap_or("BothSides")),
        contour: names::contour_type_in(w.find("ContourOption").and_then(|c| c.get("ContourType")).unwrap_or("SameAsClipping")),
        ..TextWrap::default()
    };
    if let Some(o) = w.prop_el("TextWrapOffset") {
        tw.offsets = [o.num("Top").unwrap_or(0.0), o.num("Left").unwrap_or(0.0), o.num("Bottom").unwrap_or(0.0), o.num("Right").unwrap_or(0.0)];
    }
    tw
}

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

/// `HL Composer` and `HL Single`, their World-Ready (`… Optyca`) and Japanese (`… J`) forms.
fn composer_in(name: &str) -> designcraft_doc::Composer {
    use designcraft_doc::Composer as C;
    let name = name.trim().trim_start_matches("$ID/");
    match (name.contains("Single"), name.ends_with(" J") || name.contains("Japanese")) {
        (false, false) => C::Paragraph,
        (true, false) => C::SingleLine,
        (false, true) => C::Japanese,
        (true, true) => C::JapaneseSingleLine,
    }
}

fn margins_of(m: &El) -> (Margins, Columns) {
    let g = |k: &str, d: f64| m.num(k).filter(|v| v.is_finite()).unwrap_or(d);
    let margins = Margins { top: g("Top", 36.0), bottom: g("Bottom", 36.0), inside: g("Left", 36.0), outside: g("Right", 36.0) };
    let count = g("ColumnCount", 1.0).clamp(1.0, f64::from(designcraft_doc::MAX_COLUMNS)) as u32;
    let gutter = g("ColumnGutter", 12.0);
    let positions = m.get("ColumnsPositions").and_then(|s| custom_columns(s, count, gutter));
    (margins, Columns { count, gutter, positions })
}

/// `ColumnsPositions` when the column guides have been dragged off the even split: `2 × count`
/// offsets from the margin box's left edge, valid as [`designcraft_doc::valid_column_positions`]
/// over the box they span. InDesign writes the attribute whether or not they have been moved, so
/// an even set is dropped: `positions` means "custom", and keeping an even copy would freeze the
/// columns against a later change of count or gutter. Anything malformed is the even split; at
/// most one number past `2 × count` is parsed, so a long list costs nothing.
fn custom_columns(s: &str, count: u32, gutter: f64) -> Option<Vec<f64>> {
    let want = 2 * count as usize;
    let pos = s.split_whitespace().take(want + 1).map(|v| v.parse::<f64>().ok()).collect::<Option<Vec<f64>>>()?;
    let width = *pos.last()?;
    if !designcraft_doc::valid_column_positions(&pos, count, width) {
        return None;
    }
    let even = Columns { count, gutter, positions: None }.edges(width);
    // The tolerance only absorbs float noise in InDesign's arithmetic; a dragged guide moves far more.
    pos.iter().zip(&even).any(|(a, b)| (a - b).abs() > 1e-4).then_some(pos)
}

/// `columns` with custom positions only when they fit the margin box of a `page_width`-wide page
/// with these margins (the box's width doesn't depend on which side of the spine it is).
fn fit_columns(mut columns: Columns, margins: &Margins, page_width: f64) -> Columns {
    let width = (page_width - margins.inside - margins.outside).max(0.0);
    if columns.valid_positions(width).is_none() {
        columns.positions = None;
    }
    columns
}

/// Text Frame Options from `TextFramePreference` elements: each attribute from the first that sets it.
fn text_frame_options(prefs: &[&El]) -> TextFrameOptions {
    let e = Prefs(prefs);
    let mut o = TextFrameOptions::default();
    if let Some(v) = e.num("TextColumnCount") {
        o.columns = v.clamp(1.0, 216.0) as u32;
    }
    if let Some(v) = e.num("TextColumnGutter") {
        o.gutter = v;
    }
    o.columns_kind = if e.prop("UseFixedColumnWidth").as_deref().map(str::trim) == Some("true") {
        ColumnsKind::FixedWidth
    } else if e.prop("UseFlexibleColumnWidth").as_deref().map(str::trim) == Some("true") {
        ColumnsKind::FlexibleWidth
    } else {
        ColumnsKind::FixedNumber
    };
    if o.columns_kind != ColumnsKind::FixedNumber
        && let Some(v) = e.num("TextColumnFixedWidth")
    {
        o.column_width = v;
    }
    o.balance_columns = e.prop("VerticalBalanceColumns").as_deref().map(str::trim) == Some("true");
    if let Some(p) = prefs.iter().find(|p| p.prop("InsetSpacing").is_some())
        && let Some(inset) = inset_spacing(p)
    {
        o.inset = inset;
    }
    if let Some(v) = e.prop("VerticalJustification") {
        o.vertical_justification = names::vj_in(v.trim());
    }
    if let Some(v) = e.num("VerticalThreshold") {
        o.vj_paragraph_spacing_limit = v;
    }
    if let Some(v) = e.prop("FirstBaselineOffset") {
        o.first_baseline = names::first_baseline_in(v.trim());
    }
    if let Some(v) = e.num("MinimumFirstBaselineOffset") {
        o.first_baseline_min = v;
    }
    o.ignore_wrap = e.prop("IgnoreWrap").as_deref().map(str::trim) == Some("true");
    if let Some(v) = e.prop("AutoSizingType") {
        o.auto_size = names::auto_size_in(v.trim());
    }
    if let Some(v) = e.prop("AutoSizingReferencePoint") {
        o.auto_size_ref = names::REF_POINTS.iter().position(|p| *p == v.trim()).unwrap_or(1) as u8;
    }
    let size = |k: &str| e.num(k).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 10_000.0));
    o.auto_size_use_min_height = e.prop("UseMinimumHeightForAutoSizing").as_deref().map(str::trim) == Some("true");
    o.auto_size_min_height = size("MinimumHeightForAutoSizing").unwrap_or(0.0);
    o.auto_size_use_min_width = e.prop("UseMinimumWidthForAutoSizing").as_deref().map(str::trim) == Some("true");
    o.auto_size_min_width = size("MinimumWidthForAutoSizing").unwrap_or(0.0);
    o.auto_size_no_line_breaks = e.prop("UseNoLineBreaksForAutoSizing").as_deref().map(str::trim) == Some("true");
    // Column rules (the colour is a swatch reference: see `Importer::frame_options`).
    o.column_rule = e.prop("ColumnRuleOverride").as_deref().map(str::trim) == Some("true");
    let finite = |k: &str| e.num(k).filter(|v| v.is_finite());
    if let Some(v) = finite("ColumnRuleStrokeWidth") {
        o.column_rule_weight = v.clamp(0.0, 1000.0);
    }
    if let Some(t) = tint(finite("ColumnRuleStrokeTint")) {
        o.column_rule_tint = t.clamp(0.0, 1.0);
    }
    for (k, v) in [
        ("ColumnRuleOffset", &mut o.column_rule_offset),
        ("ColumnRuleTopInset", &mut o.column_rule_top_inset),
        ("ColumnRuleBottomInset", &mut o.column_rule_bottom_inset),
    ] {
        if let Some(x) = finite(k) {
            *v = x.clamp(-1440.0, 1440.0);
        }
    }
    o
}

/// Deepest table-in-cell or note nesting the drop cap pass walks into.
const MAX_STORY_NESTING: usize = 32;

/// [`Importer::clear_inherited_drop_cap_styles`] for one story's paragraphs and those of the table
/// cells, footnotes and endnotes in it.
fn clear_story_drop_cap_styles(styles: &Styles, story: &mut Story, depth: usize) {
    for p in &mut story.paras {
        if p.para.drop_cap_style.is_none() && p.para.nested_styles.is_some() && inherits_drop_cap_style(styles, Some(&p.style)) {
            p.para.drop_cap_style = Some(st::NO_CHAR_STYLE.into());
        }
    }
    if depth >= MAX_STORY_NESTING {
        return;
    }
    for table in story.tables.values_mut() {
        for cell in &mut Arc::make_mut(table).cells {
            clear_story_drop_cap_styles(styles, &mut cell.text, depth + 1);
        }
    }
    for note in story.notes.iter_mut().chain(story.endnotes.iter_mut()) {
        clear_story_drop_cap_styles(styles, &mut Arc::make_mut(note).text, depth + 1);
    }
}

/// Does paragraph style `name` (and its based-on chain) set a drop cap character style? A style
/// with a nested style list of its own and no drop cap style has none.
fn inherits_drop_cap_style<'a>(styles: &'a Styles, mut name: Option<&'a str>) -> bool {
    let mut seen = HashSet::new();
    while let Some(n) = name {
        let Some(s) = styles.para(n).filter(|_| seen.insert(n)) else { break };
        if let Some(c) = s.para.drop_cap_style.as_deref() {
            return !c.is_empty() && c != st::NO_CHAR_STYLE;
        }
        if s.para.nested_styles.is_some() {
            return false;
        }
        name = s.based_on.as_deref();
    }
    false
}

/// Property lookup over preference levels: the nearest level wins, attributes before children.
struct Prefs<'a>(&'a [&'a El]);

impl Prefs<'_> {
    fn prop(&self, k: &str) -> Option<String> {
        self.0.iter().find_map(|e| e.prop(k))
    }
    fn num(&self, k: &str) -> Option<f64> {
        // The nearest preference wins; within it, attributes precede property children.
        self.prop(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }
}

/// A scalar applies to every edge; a list is top, left, bottom, right. Keep the
/// existing property-element precedence over the legacy scalar attribute, even
/// when the property is malformed. Finite negative insets remain supported.
fn inset_spacing(e: &El) -> Option<[f64; 4]> {
    let number = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    let Some(property) = e.prop_el("InsetSpacing") else {
        return e.get("InsetSpacing").and_then(number).map(|v| [v; 4]);
    };
    let mut items = property.elements();
    if property.get("type") == Some("list") || property.find("ListItem").is_some() {
        let mut inset = [0.0; 4];
        for edge in &mut inset {
            let item = items.next()?;
            if item.local() != "ListItem" || item.elements().next().is_some() {
                return None;
            }
            *edge = number(&item.text_content())?;
        }
        items.next().is_none().then_some(inset)
    } else if items.next().is_none() {
        number(&property.text_content()).map(|v| [v; 4])
    } else {
        None
    }
}

/// A graphic's `ClippingPathSettings` with its saved geometry, moved into graphic space by
/// `to_graphic`. `None` for type None, an unknown type, or no geometry (the outline is never
/// traced here).
fn clipping_path(g: &El, to_graphic: Affine) -> Option<Arc<designcraft_doc::ClippingPath>> {
    let cs = g.find("ClippingPathSettings")?;
    let kind = names::clipping_kind_in(cs.get("ClippingType").unwrap_or("None"))?;
    let geometry = cs.prop_el("ClippingPathGeometry").or_else(|| cs.find("ClippingPathGeometry")).or_else(|| g.prop_el("ClippingPathGeometry"))?;
    let mut path = path_capped(geometry, designcraft_doc::MAX_CLIP_ANCHORS);
    path.subpaths.retain(|sp| sp.anchors.len() > 1 && sp.anchors.iter().all(|a| a.p.is_finite() && a.h_in.is_finite() && a.h_out.is_finite()));
    if path.is_empty() {
        return None;
    }
    path.transform(to_graphic);
    let finite = |v: Option<f64>, def: f64| v.filter(|v| v.is_finite()).unwrap_or(def);
    let mut c = designcraft_doc::ClippingPath::new(kind, path);
    c.invert = cs.boolean("InvertPath").unwrap_or(false);
    c.include_inside_edges = cs.boolean("IncludeInsideEdges").unwrap_or(false);
    c.restrict_to_frame = cs.boolean("RestrictToFrame").unwrap_or(false);
    c.use_high_resolution = cs.boolean("UseHighResolutionImage").unwrap_or(true);
    c.threshold = finite(cs.num("Threshold"), c.threshold).clamp(0.0, 255.0);
    c.tolerance = finite(cs.num("Tolerance"), c.tolerance).clamp(0.0, 10.0);
    c.inset = finite(cs.num("InsetFrame"), 0.0);
    c.source_name = cs.get("AppliedPathName").filter(|n| *n != "$ID/").unwrap_or("").to_string();
    c.source_index = cs.num("Index").filter(|v| v.is_finite()).map_or(-1, |v| v.clamp(-1.0, i32::MAX as f64) as i32);
    Some(Arc::new(c))
}

fn path_of(pg: &El) -> PathData {
    path_capped(pg, usize::MAX)
}

/// The path of a `…Geometry` element, keeping at most `max` anchors.
fn path_capped(pg: &El, max: usize) -> PathData {
    let mut subs = Vec::new();
    let mut left = max;
    for g in pg.find_all("GeometryPathType") {
        let closed = g.get("PathOpen") != Some("true");
        let mut anchors = Vec::new();
        if let Some(arr) = g.find("PathPointArray") {
            for p in arr.find_all("PathPointType").take(left) {
                let Some(a) = parse_point(p.get("Anchor")) else { continue };
                let hin = parse_point(p.get("LeftDirection")).unwrap_or(a);
                let hout = parse_point(p.get("RightDirection")).unwrap_or(a);
                anchors.push(Anchor::with_handles(a, hin, hout));
            }
        }
        left = left.saturating_sub(anchors.len());
        if !anchors.is_empty() {
            subs.push(SubPath::new(anchors, closed));
        }
    }
    PathData::new(subs)
}

/// A placed PDF's `PDFAttribute` (Image Import Options), as a child or a property.
fn pdf_attribute(g: &El) -> Option<&El> {
    g.find("PDFAttribute").or_else(|| g.prop_el("PDFAttribute"))
}

/// The page a placed PDF shows (0-based), from `PDFAttribute/@PageNumber` (1-based). A missing or
/// malformed value means page 1; a page the PDF lacks draws nothing and is left out of PDF export.
fn pdf_page_in(g: &El) -> u32 {
    pdf_attribute(g)
        .and_then(|a| a.get("PageNumber"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .and_then(|n| u32::try_from(n.saturating_sub(1)).ok())
        .unwrap_or(0)
}

/// Anchors a custom text wrap boundary may have.
const MAX_WRAP_PATH_ANCHORS: usize = 4096;

/// A custom text wrap boundary (`TextWrapPreference/Properties/PathGeometry`), in the item's inner
/// space like its own path. Hostile input is an error: too many anchors, a malformed or non-finite
/// point, or no points at all.
fn wrap_path_of(pg: &El) -> std::result::Result<PathData, &'static str> {
    let finite = |s: Option<&str>| -> std::result::Result<Option<Point>, &'static str> {
        let Some(s) = s else { return Ok(None) };
        let v = nums(s);
        match (v.first(), v.get(1)) {
            (Some(&x), Some(&y)) if v.len() == 2 && x.is_finite() && y.is_finite() => Ok(Some(Point::new(x, y))),
            _ => Err("malformed or non-finite point"),
        }
    };
    let mut subs = Vec::new();
    let mut n = 0usize;
    for g in pg.find_all("GeometryPathType") {
        let closed = g.get("PathOpen") != Some("true");
        let mut anchors = Vec::new();
        for p in g.find("PathPointArray").into_iter().flat_map(|arr| arr.find_all("PathPointType")) {
            n = n.saturating_add(1);
            if n > MAX_WRAP_PATH_ANCHORS {
                return Err("too many points");
            }
            let a = finite(p.get("Anchor"))?.ok_or("point without an anchor")?;
            let hin = finite(p.get("LeftDirection"))?.unwrap_or(a);
            let hout = finite(p.get("RightDirection"))?.unwrap_or(a);
            anchors.push(Anchor::with_handles(a, hin, hout));
        }
        if !anchors.is_empty() {
            subs.push(SubPath::new(anchors, closed));
        }
    }
    if subs.is_empty() {
        return Err("no points");
    }
    Ok(PathData::new(subs))
}

/// IDML link URI → file system path (`file:/a%20b` → `/a b`, `file:///C:/x` → `C:/x`). A URI
/// naming another computer keeps it as a network path (`file://host/s/x` → `//host/s/x`).
pub(crate) fn uri_to_path(uri: &str) -> String {
    let rest = match uri.strip_prefix("file://") {
        Some(rest) => {
            let host_end = rest.find(['/', '\\']).unwrap_or(rest.len());
            let host = rest.get(..host_end).unwrap_or_default();
            if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
                rest.get(host_end..).unwrap_or(rest)
            } else if matches!(host.as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic()) {
                rest
            } else {
                return format!("//{}", percent_decode(rest));
            }
        }
        None => uri.strip_prefix("file:").unwrap_or(uri),
    };
    // `file:///C:/…` → `/C:/…` → `C:/…`
    let rest = match rest.as_bytes() {
        [b'/', _, b':', _, ..] => rest.get(1..).unwrap_or(rest),
        _ => rest,
    };
    percent_decode(rest)
}

/// Auto tate-chu-yoko, Japanese composition and grid settings of a paragraph range or style.
fn cjk_settings_para(e: &El, a: &mut ParaAttrs) {
    let count = |k: &str| e.num(k).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1000.0) as u32);
    a.auto_tcy = count("AutoTcy");
    a.auto_tcy_include_roman = e.boolean("AutoTcyIncludeRoman");
    a.rotate_roman = e.boolean("RotateSingleByteCharacters");
    // 欧文泣き別れ: the name is inferred (it follows TreatIdeographicSpaceAsSpace).
    a.roman_word_break = e.boolean("AllowArbitraryHyphenation");
    a.grid_gyoudori = count("GridGyoudori");
    a.paragraph_gyoudori = e.boolean("ParagraphGyoudori");
}

impl Importer<'_> {
    /// Kenten Settings, Kenten Color and Shatai of a character range or style.
    fn cjk_settings_chars(&mut self, e: &El, a: &mut CharAttrs) {
        let finite = |k: &str| e.num(k).filter(|v| v.is_finite());
        if let Some(kind) = e.get("KentenKind").and_then(crate::cjk::kenten_kind_in) {
            a.kenten_kind = Some(kind);
            if a.kenten_character.as_deref().is_none_or(str::is_empty)
                && let Some(mark) = kind.mark()
            {
                a.kenten_character = Some(mark.to_string());
            }
        }
        if let Some(f) = e.prop("KentenFont") {
            let f = f.trim();
            a.kenten_font = Some(if f == "$ID/" { String::new() } else { f.to_string() });
        }
        if let Some(st) = e.prop("KentenFontStyle") {
            let st = st.trim();
            a.kenten_font_style = Some(if st == "Nothing" { String::new() } else { st.to_string() });
        }
        a.kenten_size = finite("KentenFontSize").map(|v| (v > 0.0).then_some(v));
        a.kenten_x_scale = finite("KentenXScale").map(|v| v / 100.0);
        a.kenten_y_scale = finite("KentenYScale").map(|v| v / 100.0);
        a.kenten_distance = finite("KentenPlacement");
        a.kenten_position = e.prop("KentenPosition").and_then(|v| crate::cjk::kenten_position_in(&v));
        a.kenten_alignment = e.prop("KentenAlignment").and_then(|v| crate::cjk::kenten_alignment_in(&v));
        a.kenten_character_set = e.prop("KentenCharacterSet");
        // "Text Color" is the text's own colour; it resets an inherited swatch.
        if let Some(r) = e.prop("KentenFillColor") {
            a.kenten_fill = Some(if r.trim() == "Text Color" { String::new() } else { self.swatch_ref(r.trim()) });
        }
        if let Some(r) = e.prop("KentenStrokeColor") {
            a.kenten_stroke = Some(if r.trim() == "Text Color" { String::new() } else { self.swatch_ref(r.trim()) });
        }
        // Tints and weight: -1 is the text's.
        a.kenten_fill_tint = finite("KentenTint").map(|v| (v >= 0.0).then(|| (v.min(100.0) / 100.0) as f32));
        a.kenten_stroke_tint = finite("KentenStrokeTint").map(|v| (v >= 0.0).then(|| (v.min(100.0) / 100.0) as f32));
        a.kenten_stroke_weight = finite("KentenWeight").map(|v| (v >= 0.0).then_some(v));
        a.kenten_overprint_fill = e.prop("KentenOverprintFill").and_then(|v| crate::cjk::overprint_in(&v));
        a.kenten_overprint_stroke = e.prop("KentenOverprintStroke").and_then(|v| crate::cjk::overprint_in(&v));
        a.shatai_magnification = finite("ShataiMagnification");
        a.shatai_angle = finite("ShataiDegreeAngle").map(|v| v / crate::cjk::SHATAI_ANGLE_UNITS);
        a.shatai_adjust_rotation = e.boolean("ShataiAdjustRotation");
        a.shatai_adjust_tsume = e.boolean("ShataiAdjustTsume");
    }
}

#[cfg(test)]
mod numeric_property_tests {
    use super::*;

    fn pref(attrs: &[(&str, &str)], props: &[(&str, &str)]) -> El {
        let mut e = El::new("TextFramePreference");
        for (key, value) in attrs {
            e.set(key, *value);
        }
        let mut p = El::new("Properties");
        for (key, value) in props {
            p.push(El::new(key).text(*value));
        }
        e.push(p);
        e
    }

    #[test]
    fn numeric_property_forms_import_every_frame_option() {
        let local = pref(
            &[("UseFixedColumnWidth", "true")],
            &[
                ("TextColumnCount", "3"),
                ("TextColumnGutter", "17"),
                ("TextColumnFixedWidth", "123"),
                ("VerticalThreshold", "19"),
                ("MinimumFirstBaselineOffset", "8"),
                ("ColumnRuleStrokeWidth", "2"),
                ("ColumnRuleStrokeTint", "25"),
                ("ColumnRuleOffset", "3"),
                ("ColumnRuleTopInset", "4"),
                ("ColumnRuleBottomInset", "5"),
            ],
        );
        let o = text_frame_options(&[&local]);
        assert_eq!((o.columns, o.gutter, o.column_width), (3, 17., 123.));
        assert_eq!((o.vj_paragraph_spacing_limit, o.first_baseline_min), (19., 8.));
        assert_eq!(
            (o.column_rule_weight, o.column_rule_tint, o.column_rule_offset, o.column_rule_top_inset, o.column_rule_bottom_inset),
            (2., 0.25, 3., 4., 5.)
        );
    }

    #[test]
    fn numeric_property_precedence_is_nearest_level_then_attribute() {
        let local = pref(&[("TextColumnCount", "4")], &[("TextColumnCount", "5"), ("TextColumnGutter", "17")]);
        let child = pref(
            &[("TextColumnGutter", "90"), ("MinimumFirstBaselineOffset", "8")],
            &[("MinimumFirstBaselineOffset", "9"), ("VerticalThreshold", "19")],
        );
        let base = pref(&[("VerticalThreshold", "99")], &[("ColumnRuleStrokeWidth", "2")]);
        let o = text_frame_options(&[&local, &child, &base]);
        assert_eq!((o.columns, o.gutter, o.first_baseline_min, o.vj_paragraph_spacing_limit, o.column_rule_weight), (4, 17., 8., 19., 2.));
        // An explicit invalid value still masks inherited values, as attribute lookup did.
        let invalid = pref(&[("TextColumnCount", "bad")], &[("TextColumnCount", "6")]);
        assert_eq!(text_frame_options(&[&invalid, &local]).columns, TextFrameOptions::default().columns);
    }
}

#[cfg(test)]
mod frame_preference_safety_tests {
    use super::*;

    fn pref(attrs: &[(&str, &str)], props: &[(&str, &str)]) -> El {
        let mut e = El::new("TextFramePreference");
        for (key, value) in attrs {
            e.set(key, *value);
        }
        let mut p = El::new("Properties");
        for (key, value) in props {
            p.push(El::new(key).text(*value));
        }
        e.push(p);
        e
    }

    #[test]
    fn frame_preference_nonfinite_numbers_keep_finite_defaults() {
        const KEYS: [&str; 10] = [
            "TextColumnCount",
            "TextColumnGutter",
            "TextColumnFixedWidth",
            "VerticalThreshold",
            "MinimumFirstBaselineOffset",
            "ColumnRuleStrokeWidth",
            "ColumnRuleStrokeTint",
            "ColumnRuleOffset",
            "ColumnRuleTopInset",
            "ColumnRuleBottomInset",
        ];
        let fallback = pref(&[("UseFixedColumnWidth", "true")], &[]);
        let expected = text_frame_options(&[&fallback]);
        for value in ["NaN", "inf", "-inf", "1e309"] {
            for attribute in [true, false] {
                let values: Vec<_> = KEYS.iter().map(|key| (*key, value)).collect();
                let mut invalid = if attribute { pref(&values, &[]) } else { pref(&[], &values) };
                invalid.set("UseFixedColumnWidth", "true");
                let inherited = pref(&[], &[("TextColumnCount", "7"), ("TextColumnGutter", "9")]);
                assert_eq!(text_frame_options(&[&invalid, &inherited]), expected, "{value}, attribute={attribute}");
            }
        }
    }

    #[test]
    fn frame_preference_property_booleans_enums_and_precedence() {
        let base = pref(
            &[
                ("UseFixedColumnWidth", "true"),
                ("UseFlexibleColumnWidth", "false"),
                ("VerticalBalanceColumns", "false"),
                ("IgnoreWrap", "false"),
                ("ColumnRuleOverride", "false"),
                ("VerticalJustification", "TopAlign"),
                ("FirstBaselineOffset", "AscentOffset"),
                ("AutoSizingType", "Off"),
                ("AutoSizingReferencePoint", "TopCenterPoint"),
            ],
            &[],
        );
        let local = pref(
            &[],
            &[
                ("UseFixedColumnWidth", "false"),
                ("UseFlexibleColumnWidth", "true"),
                ("VerticalBalanceColumns", "true"),
                ("IgnoreWrap", "true"),
                ("ColumnRuleOverride", "true"),
                ("VerticalJustification", "BottomAlign"),
                ("FirstBaselineOffset", "FixedHeight"),
                ("AutoSizingType", "HeightAndWidth"),
                ("AutoSizingReferencePoint", "BottomRightPoint"),
            ],
        );
        let o = text_frame_options(&[&local, &base]);
        assert_eq!(o.columns_kind, ColumnsKind::FlexibleWidth);
        assert!(o.balance_columns && o.ignore_wrap && o.column_rule);
        assert_eq!(o.vertical_justification, designcraft_doc::VerticalJustification::Bottom);
        assert_eq!(o.first_baseline, designcraft_doc::FirstBaseline::Fixed);
        assert_eq!(o.auto_size, designcraft_doc::AutoSize::HeightAndWidth);
        assert_eq!(o.auto_size_ref, 8);
        let fixed = pref(&[], &[("UseFixedColumnWidth", "true")]);
        assert_eq!(text_frame_options(&[&fixed, &local, &base]).columns_kind, ColumnsKind::FixedWidth);
        // At one level an attribute wins over a conflicting property child, for every key.
        let mut same_level = local.clone();
        for (key, value) in &base.attrs {
            same_level.set(key, value);
        }
        assert_eq!(text_frame_options(&[&same_level, &local]), text_frame_options(&[&base]));
        // Malformed nearer values mask ancestors and use the normal per-field fallback.
        let mut malformed = El::new("TextFramePreference");
        for (key, _) in &base.attrs {
            malformed.set(key, "invalid");
        }
        assert_eq!(text_frame_options(&[&malformed, &local]), TextFrameOptions { auto_size_ref: 1, ..Default::default() });
    }
}

#[cfg(test)]
mod page_column_import_safety_tests {
    use super::*;

    #[test]
    fn frame_preference_page_columns_are_bounded_and_finite() {
        for (raw, expected) in [("4000000000", 216), ("1e300", 216), ("-10", 1), ("NaN", 1), ("inf", 1)] {
            let m = El::new("MarginPreference")
                .attr("ColumnCount", raw)
                .attr("ColumnGutter", "NaN")
                .attr("Top", "inf")
                .attr("Bottom", "-inf")
                .attr("Left", "NaN")
                .attr("Right", "1e309");
            let (margins, columns) = margins_of(&m);
            assert_eq!(columns.count, expected);
            assert_eq!(columns.gutter, 12.);
            assert_eq!(margins, Margins::uniform(36.));
        }
    }

    /// Hostile `ColumnsPositions` are the even split: never half-read, never a panic, and a long
    /// list is not parsed past what `ColumnCount` allows (#1096).
    #[test]
    fn hostile_column_positions_fall_back_to_the_even_split() {
        let positions = |count: &str, raw: &str| {
            let m = El::new("MarginPreference").attr("ColumnCount", count).attr("ColumnGutter", "20").attr("ColumnsPositions", raw);
            margins_of(&m).1.positions
        };
        assert_eq!(positions("2", "0 150 170 450"), Some(vec![0., 150., 170., 450.]));
        let huge = (0..10_000).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
        for (count, raw) in [
            ("2", "0 215 235 450"),         // the even split
            ("2", "0 150 170"),             // odd
            ("2", "0 450"),                 // too few for the count
            ("2", "0 100 120 200 220 450"), // too many for the count
            ("2", "0 150 90 450"),          // descending
            ("2", "0 NaN 170 450"),
            ("2", "0 inf 170 450"),
            ("2", "0 150 170 1e309"),
            ("2", "0 x 170 450"),
            ("2", "10 150 170 450"), // not from the margin
            ("2", ""),
            ("2", huge.as_str()),
            ("4000000000", huge.as_str()), // the count is capped at 216, so is the list
            ("216", huge.as_str()),
            ("NaN", "0 150 170 450"),
        ] {
            assert_eq!(positions(count, raw), None, "{count}: {}", raw.get(..raw.len().min(40)).unwrap_or(""));
        }
        // The cap itself: 216 one-point columns with no gutter are the even split; one edge moved
        // makes them custom.
        let gapless = |raw: &str| {
            let m = El::new("MarginPreference").attr("ColumnCount", "216").attr("ColumnGutter", "0").attr("ColumnsPositions", raw);
            margins_of(&m).1.positions
        };
        let edges = |first_right: &str| {
            (0..432).map(|i| if i == 1 { first_right.to_string() } else { ((i + 1) / 2).to_string() }).collect::<Vec<_>>().join(" ")
        };
        assert_eq!(gapless(&edges("1")), None);
        assert_eq!(gapless(&edges("0.5")).map(|p| p.len()), Some(432));
    }
}

#[cfg(test)]
mod frame_preference_scalar_tests {
    use super::*;

    #[test]
    fn frame_preference_scalar_whitespace() {
        let values = [
            ("UseFixedColumnWidth", "true"),
            ("UseFlexibleColumnWidth", "false"),
            ("VerticalBalanceColumns", "true"),
            ("IgnoreWrap", "true"),
            ("ColumnRuleOverride", "true"),
            ("VerticalJustification", "BottomAlign"),
            ("FirstBaselineOffset", "FixedHeight"),
            ("AutoSizingType", "HeightAndWidth"),
            ("AutoSizingReferencePoint", "BottomRightPoint"),
        ];
        let preference = |properties: bool, padded: bool, fixed: bool| {
            let mut e = El::new("TextFramePreference");
            let mut props = El::new("Properties");
            for (key, raw) in values {
                let raw = match key {
                    "UseFixedColumnWidth" if !fixed => "false",
                    "UseFlexibleColumnWidth" if !fixed => "true",
                    _ => raw,
                };
                let value = if padded { format!("\n  {raw}\t ") } else { raw.to_string() };
                if properties {
                    props.push(El::new(key).text(value));
                } else {
                    e.set(key, value);
                }
            }
            e.push(props);
            e
        };
        for fixed in [false, true] {
            let clean = preference(false, false, fixed);
            let expected = text_frame_options(&[&clean]);
            assert_eq!(expected.columns_kind, if fixed { ColumnsKind::FixedWidth } else { ColumnsKind::FlexibleWidth });
            assert!(expected.balance_columns && expected.ignore_wrap && expected.column_rule);
            assert_eq!(expected.vertical_justification, designcraft_doc::VerticalJustification::Bottom);
            assert_eq!(expected.first_baseline, designcraft_doc::FirstBaseline::Fixed);
            assert_eq!(expected.auto_size, designcraft_doc::AutoSize::HeightAndWidth);
            assert_eq!(expected.auto_size_ref, 8);
            for properties in [false, true] {
                let padded = preference(properties, true, fixed);
                assert_eq!(text_frame_options(&[&padded]), expected);
            }
            // A malformed nearest attribute masks both its own valid child and farther values.
            let mut invalid = preference(true, true, fixed);
            for (key, _) in values {
                invalid.set(key, " invalid ");
            }
            assert_eq!(text_frame_options(&[&invalid, &clean]), TextFrameOptions { auto_size_ref: 1, ..Default::default() });
        }
        // Object identities use the raw property lookup, not scalar whitespace normalization.
        let exact = El::new("TextFramePreference").child(El::new("Properties").child(El::new("ColumnRuleStrokeColor").text(" Color/Named ")));
        assert_eq!(Prefs(&[&exact]).prop("ColumnRuleStrokeColor").as_deref(), Some(" Color/Named "));
    }
}
