//! Document → IDML package.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;

use designcraft_color::{Color, ColorType, GradientKind, SwatchValue, swatch};
use designcraft_doc::{
    CharAttrs, CharFormat, ColumnsKind, Content, Document, Item, ListType, PageSide, ParaAttrs, ParaFormat, Rule, Shape, SpanColumns, Spread, Story,
    TextFrameOptions, story as st,
};
use designcraft_geom::{Affine, PathData, Rect};

use crate::names::{self, CHAR_BUILTINS, OBJECT_BUILTINS, PARA_BUILTINS, escape_id, num, pt};
use crate::xml::{El, Node, document, document_with_pi};
use crate::{DOM_VERSION, MIMETYPE, base64_encode};

const PKG_NS: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
const NO_PARA_ID: &str = "ParagraphStyle/$ID/[No paragraph style]";
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
pub fn export_idml(doc: &Document) -> Vec<u8> {
    export_idml_with(doc, &ExportOptions::default())
}

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

pub(crate) fn uid(n: u64) -> String {
    format!("u{n:x}")
}

fn p(name: &str, ty: &str, val: impl Into<String>) -> El {
    El::new(name).attr("type", ty).text(val)
}

/// Insert a `<Properties>` element as the first child when there are any.
fn with_props(mut el: El, props: Vec<El>) -> El {
    if !props.is_empty() {
        let mut pe = El::new("Properties");
        pe.children = props.into_iter().map(Node::El).collect();
        el.children.insert(0, Node::El(pe));
    }
    el
}

fn pkg_root(kind: &str) -> El {
    El::new(&format!("idPkg:{kind}")).attr("xmlns:idPkg", PKG_NS).attr("DOMVersion", DOM_VERSION)
}

fn bool_s(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn pct(v: f64) -> String {
    num(v * 100.0)
}

/// (model, space, value) for a colour.
fn color_value(c: &Color) -> (&'static str, String) {
    match *c {
        Color::Cmyk { c, m, y, k } => ("CMYK", format!("{} {} {} {}", pct(c as f64), pct(m as f64), pct(y as f64), pct(k as f64))),
        Color::Rgb { r, g, b } => ("RGB", format!("{} {} {}", num(r as f64 * 255.0), num(g as f64 * 255.0), num(b as f64 * 255.0))),
        Color::Gray { k } => ("CMYK", format!("0 0 0 {}", pct(k as f64))),
    }
}

/// The IDML spread origin in our spread coordinates.
pub(crate) fn spread_origin(sp: &Spread, facing: bool) -> (f64, f64) {
    let h = sp.pages.iter().map(|p| p.height).fold(0.0, f64::max);
    let b = sp.bounds();
    let ox = if facing {
        if let Some(r) = sp.pages.iter().find(|p| p.side == PageSide::Right) {
            r.x
        } else if let Some(l) = sp.pages.iter().rev().find(|p| p.side == PageSide::Left) {
            l.x + l.width
        } else {
            b.center().x
        }
    } else {
        b.center().x
    };
    (ox, h / 2.0)
}

fn affine_s(a: Affine) -> String {
    a.as_coeffs().iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")
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
    /// Index topics (paths) and their See / See also cross-references.
    index_topics: BTreeSet<Vec<String>>,
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
            index_topics: BTreeSet::new(),
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
        let mut used: HashSet<String> = ["Swatch/None", "Color/Paper", "Color/Black", "Color/Registration"].iter().map(|s| s.to_string()).collect();
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

    // ---------- designmap ----------

    fn designmap(&mut self, masters: &[(String, Vec<u8>)], spreads: &[(String, Vec<u8>)], stories: &[(String, Vec<u8>)]) -> Vec<u8> {
        let d = self.d;
        let story_list: Vec<String> = d.stories.keys().map(|k| uid(k.0)).collect();
        let mut root = El::new("Document")
            .attr("xmlns:idPkg", PKG_NS)
            .attr("DOMVersion", DOM_VERSION)
            .attr("Self", "d")
            .attr("StoryList", story_list.join(" "))
            .attr("Name", format!("{}.indd", d.title))
            .attr("ZeroPoint", "0 0")
            .attr("ActiveLayer", d.layers.first().map(|l| uid(l.id.0)).unwrap_or_else(|| "n".into()));
        root.push(
            El::new("Language")
                .attr("Self", "Language/$ID/[No Language]")
                .attr("Name", "$ID/[No Language]")
                .attr("SingleQuotes", "''")
                .attr("DoubleQuotes", "\"\"")
                .attr("PrimaryLanguageName", "$ID/[No Language]")
                .attr("SublanguageName", "$ID/[No Language]")
                .attr("Id", "0")
                .attr("HyphenationVendor", "$ID/")
                .attr("SpellingVendor", "$ID/"),
        );
        root.push(
            El::new("Language")
                .attr("Self", "Language/$ID/English%3a USA")
                .attr("Name", "$ID/English: USA")
                .attr("SingleQuotes", "\u{2018}\u{2019}")
                .attr("DoubleQuotes", "\u{201C}\u{201D}")
                .attr("PrimaryLanguageName", "$ID/English")
                .attr("SublanguageName", "$ID/USA")
                .attr("Id", "269")
                .attr("HyphenationVendor", "Hunspell")
                .attr("SpellingVendor", "Hunspell"),
        );
        let mut sets = std::collections::BTreeMap::new();
        for a in d.styles.paragraph.iter().map(|p| &p.para).chain(d.stories.values().flat_map(|s| s.paras.iter().map(|p| &p.para))) {
            if let Some(Some(k)) = &a.kinsoku
                && !k.name.is_empty()
            {
                sets.insert(&k.name, k);
            }
        }
        for k in sets.values() {
            root.push(
                El::new("KinsokuTable")
                    .attr("Self", format!("KinsokuTable/{}", escape_id(&k.name)))
                    .attr("Name", &k.name)
                    .attr("CantBeginLineChars", &k.no_start)
                    .attr("CantEndLineChars", &k.no_end)
                    .attr("CantBeSeparatedChars", &k.inseparable)
                    .attr("HangingPunctuationChars", &k.hanging),
            );
        }
        for t in &d.styles.mojikumi_tables {
            let mut e = El::new("MojikumiTable").attr("Self", format!("MojikumiTable/{}", escape_id(&t.name))).attr("Name", &t.name);
            if !t.based_on.is_empty() {
                e.set("BasedOnMojikumiSet", &t.based_on);
            }
            if !t.overrides.is_empty() {
                let mut list = El::new("OverrideMojikumiAkiList");
                for r in &t.overrides {
                    list.push(
                        El::new("OverrideMojikumiAkiType")
                            .attr("TargetMojikumiClass", r.target_class)
                            .attr("SideMojikumiClass", r.side_class)
                            .attr("SideIsAfterTarget", bool_s(r.after))
                            .attr("Minimum", num(r.minimum))
                            .attr("Desired", num(r.desired))
                            .attr("Maximum", num(r.maximum))
                            .attr("CompressionPriority", r.priority)
                            .attr("AkiDoesNotFloat", bool_s(r.does_not_float)),
                    );
                }
                e.push(El::new("Properties").child(list));
            }
            root.push(e);
        }
        root.push(El::new("idPkg:Graphic").attr("src", "Resources/Graphic.xml"));
        root.push(El::new("idPkg:Fonts").attr("src", "Resources/Fonts.xml"));
        root.push(El::new("idPkg:Styles").attr("src", "Resources/Styles.xml"));
        root.push(
            El::new("NumberingList")
                .attr("Self", "NumberingList/$ID/[Default]")
                .attr("Name", "$ID/[Default]")
                .attr("ContinueNumbersAcrossStories", "false")
                .attr("ContinueNumbersAcrossDocuments", "false"),
        );
        root.push(El::new("idPkg:Preferences").attr("src", "Resources/Preferences.xml"));
        for c in &d.conditions {
            root.push(
                El::new("Condition")
                    .attr("Self", format!("Condition/{}", escape_id(&c.name)))
                    .attr("Name", &c.name)
                    .attr("IndicatorColor", format!("{} {} {}", c.color[0], c.color[1], c.color[2]))
                    .attr("IndicatorMethod", "UseUnderline")
                    .attr("UnderlineIndicatorAppearance", "Solid")
                    .attr("Visible", bool_s(c.visible)),
            );
        }
        // Variables InDesign's cross-reference page/chapter blocks rely on.
        for (name, ty) in [("XRefChapterNumber", "XrefChapterNumberType"), ("XRefPageNumber", "XrefPageNumberType")] {
            let n = format!("<?AID 001b?>TV {name}");
            root.push(El::new("TextVariable").attr("Self", format!("dTextVariablen{n}")).attr("Name", &n).attr("VariableType", ty));
        }
        // IDML lists layers back to front.
        for l in d.layers.iter().rev() {
            let color = match names::layer_color_out(l.color) {
                Some(n) => p("LayerColor", "enumeration", n),
                None => {
                    let mut e = El::new("LayerColor").attr("type", "list");
                    for c in l.color {
                        e.push(p("ListItem", "double", num(c as f64)));
                    }
                    e
                }
            };
            let el = El::new("Layer")
                .attr("Self", uid(l.id.0))
                .attr("Name", &l.name)
                .attr("Visible", bool_s(l.visible))
                .attr("Locked", bool_s(l.locked))
                .attr("IgnoreWrap", bool_s(l.suppress_wrap_when_hidden))
                .attr("ShowGuides", bool_s(l.show_guides))
                .attr("LockGuides", "false")
                .attr("UI", "true")
                .attr("Expendable", "true")
                .attr("Printable", bool_s(l.printable));
            root.push(with_props(el, vec![color]));
        }
        for (name, _) in masters {
            root.push(El::new("idPkg:MasterSpread").attr("src", name));
        }
        for (name, _) in spreads {
            root.push(El::new("idPkg:Spread").attr("src", name));
        }
        // Sections.
        let mut secs = d.sections.clone();
        secs.sort_by_key(|s| s.start);
        if secs.is_empty() || secs[0].start != 0 {
            secs.insert(
                0,
                designcraft_doc::Section {
                    start: 0,
                    start_number: Some(1),
                    style: Default::default(),
                    prefix: String::new(),
                    marker: String::new(),
                    include_prefix: false,
                },
            );
        }
        let total = d.page_count();
        for (i, s) in secs.iter().enumerate() {
            if s.start >= total {
                continue;
            }
            let end = secs.get(i + 1).map(|n| n.start).unwrap_or(total).min(total);
            let Some(page) = d.page(s.start) else { continue };
            let mut el = El::new("Section")
                .attr("Self", self.fresh())
                .attr("Length", end.saturating_sub(s.start).max(1))
                .attr("Name", &s.prefix)
                .attr("ContinueNumbering", bool_s(s.start_number.is_none()))
                .attr("IncludeSectionPrefix", bool_s(s.include_prefix));
            if let Some(n) = s.start_number {
                el.set("PageNumberStart", n);
            }
            el = el.attr("Marker", &s.marker).attr("PageStart", uid(page.id.0)).attr("SectionPrefix", &s.prefix);
            root.push(with_props(el, vec![p("PageNumberStyle", "enumeration", names::number_style_out(s.style))]));
        }
        if !self.index_topics.is_empty() || !self.index_xrefs.is_empty() {
            root.push(self.index_el());
        }
        // Formats before the stories whose sources apply them.
        for (i, f) in d.xref_formats.iter().enumerate() {
            root.push(xref_format_el(i, f));
        }
        for (name, _) in stories {
            root.push(El::new("idPkg:Story").attr("src", name));
        }
        for (src, target) in self.xref_sources.clone() {
            let mut h = El::new("Hyperlink")
                .attr("Self", self.fresh())
                .attr("Name", "Cross-Reference")
                .attr("Source", &src)
                .attr("Visible", "false")
                .attr("Highlight", "None")
                .attr("Width", "Thin")
                .attr("BorderStyle", "Solid")
                .attr("Hidden", "false")
                .attr("DestinationUniqueKey", target & 0xFFFF_FFFF);
            let mut props = El::new("Properties");
            props.push(p("Destination", "object", anchor_self(target)));
            h.push(props);
            root.push(h);
        }
        // Root colour group: the Swatches panel order.
        let mut cg =
            El::new("ColorGroup").attr("Self", "ColorGroup/[Root Color Group]").attr("Name", "[Root Color Group]").attr("IsRootColorGroup", "true");
        let mut seen = HashSet::new();
        for s in &d.swatches {
            let id = self.sw(&s.name);
            if seen.insert(id.clone()) {
                let n = seen.len() - 1;
                cg.push(El::new("ColorGroupSwatch").attr("Self", format!("u{:x}ColorGroupSwatch{n:x}", self.next + 1)).attr("SwatchItemRef", id));
            }
        }
        self.next += 1;
        root.push(cg);
        document_with_pi(
            &root,
            &format!("<?aid style=\"50\" type=\"document\" readerVersion=\"6.0\" featureSet=\"257\" product=\"{DOM_VERSION}(1)\" ?>"),
        )
    }

    // ---------- Graphic.xml ----------

    fn graphic_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let mut root = pkg_root("Graphic");
        let mut colors: Vec<El> = Vec::new();
        let mut tints: Vec<El> = Vec::new();
        let mut gradients: Vec<El> = Vec::new();
        let mut inks: Vec<El> = Vec::new();
        let color_el = |self_id: &str, name: &str, model: &str, c: &Color, over: &str, editable: bool, removable: bool, visible: bool| {
            let (space, val) = color_value(c);
            El::new("Color")
                .attr("Self", self_id)
                .attr("Model", model)
                .attr("Space", space)
                .attr("ColorValue", val)
                .attr("ColorOverride", over)
                .attr("AlternateSpace", "NoAlternateColor")
                .attr("AlternateColorValue", "")
                .attr("Name", name)
                .attr("ColorEditable", bool_s(editable))
                .attr("ColorRemovable", bool_s(removable))
                .attr("Visible", bool_s(visible))
                .attr("SwatchCreatorID", "7937")
        };
        // Specials (always present).
        colors.push(color_el("Color/Black", "Black", "Process", &Color::cmyk(0.0, 0.0, 0.0, 1.0), "Specialblack", false, false, true));
        let paper = match d.swatch(swatch::PAPER).map(|s| &s.value) {
            Some(SwatchValue::Paper { color }) => *color,
            _ => Color::cmyk(0.0, 0.0, 0.0, 0.0),
        };
        let paper = if paper == Color::WHITE { Color::cmyk(0.0, 0.0, 0.0, 0.0) } else { paper };
        colors.push(color_el("Color/Paper", "Paper", "Process", &paper, "Specialpaper", true, false, true));
        colors.push(color_el(
            "Color/Registration",
            "Registration",
            "Registration",
            &Color::cmyk(1.0, 1.0, 1.0, 1.0),
            "Specialregistration",
            false,
            false,
            true,
        ));
        for (i, ink) in
            [("Process Cyan", 75, 0.61), ("Process Magenta", 15, 0.76), ("Process Yellow", 0, 0.16), ("Process Black", 45, 1.7)].iter().enumerate()
        {
            inks.push(
                El::new("Ink")
                    .attr("Self", format!("Ink/$ID/{}", ink.0))
                    .attr("Name", format!("$ID/{}", ink.0))
                    .attr("Angle", ink.1)
                    .attr("ConvertToProcess", "false")
                    .attr("Frequency", "70")
                    .attr("NeutralDensity", num(ink.2))
                    .attr("PrintInk", "true")
                    .attr("TrapOrder", i + 1)
                    .attr("InkType", "Normal"),
            );
        }
        // Colour swatches, by value, for gradient stop lookup.
        let mut by_value: Vec<(Color, String)> = Vec::new();
        for s in &d.swatches {
            if s.is_special() {
                continue;
            }
            let id = self.sw(&s.name);
            match &s.value {
                SwatchValue::Color { color, color_type } => {
                    let model = if *color_type == ColorType::Spot { "Spot" } else { "Process" };
                    colors.push(color_el(&id, &s.name, model, color, "Normal", true, true, true));
                    by_value.push((*color, id.clone()));
                    if *color_type == ColorType::Spot {
                        let mut ink = El::new("Ink");
                        if let Some((_, to)) = d.inks.aliases.iter().find(|(a, _)| *a == s.name) {
                            ink.set("AliasInkName", to);
                        }
                        let convert = d.inks.all_to_process || d.inks.to_process.contains(&s.name);
                        inks.push(
                            ink.attr("Self", format!("Ink/{}", escape_id(&s.name)))
                                .attr("Name", &s.name)
                                .attr("Angle", "45")
                                .attr("ConvertToProcess", bool_s(convert))
                                .attr("Frequency", "70")
                                .attr("NeutralDensity", "0.5")
                                .attr("PrintInk", "true")
                                .attr("TrapOrder", inks.len() + 1)
                                .attr("InkType", "Normal"),
                        );
                    }
                }
                SwatchValue::Tint { base, tint } => {
                    tints.push(
                        El::new("Tint")
                            .attr("Self", &id)
                            .attr("TintValue", pct(*tint as f64))
                            .attr("BaseColor", self.sw(base))
                            .attr("Name", &s.name)
                            .attr("ColorOverride", "Normal")
                            .attr("AlternateSpace", "NoAlternateColor")
                            .attr("AlternateColorValue", "")
                            .attr("ColorEditable", "true")
                            .attr("ColorRemovable", "true")
                            .attr("Visible", "true")
                            .attr("SwatchCreatorID", "7937"),
                    );
                }
                _ => {}
            }
        }
        for s in &d.swatches {
            let SwatchValue::Gradient { gradient } = &s.value else { continue };
            let id = self.sw(&s.name);
            let mut g = El::new("Gradient")
                .attr("Self", &id)
                .attr("Type", if gradient.kind == GradientKind::Radial { "Radial" } else { "Linear" })
                .attr("Name", &s.name)
                .attr("ColorEditable", "true")
                .attr("ColorRemovable", "true")
                .attr("Visible", "true")
                .attr("SwatchCreatorID", "7937");
            for (i, stop) in gradient.stops.iter().enumerate() {
                let cid = match by_value.iter().find(|(c, _)| *c == stop.color) {
                    Some((_, id)) => id.clone(),
                    None => {
                        let cid = format!("Color/{}", self.fresh());
                        colors.push(color_el(&cid, "$ID/", "Process", &stop.color, "Normal", true, true, false));
                        by_value.push((stop.color, cid.clone()));
                        cid
                    }
                };
                let mut e = El::new("GradientStop")
                    .attr("Self", format!("{}GradientStop{i}", id.replace(['/', ' '], "_")))
                    .attr("StopColor", cid)
                    .attr("Location", pct(stop.offset as f64));
                if i > 0 {
                    e.set("Midpoint", pct(gradient.stops[i - 1].midpoint as f64));
                }
                g.push(e);
            }
            gradients.push(g);
        }
        for c in colors {
            root.push(c);
        }
        for i in inks {
            root.push(i);
        }
        for t in tints {
            root.push(t);
        }
        root.push(
            El::new("Swatch")
                .attr("Self", "Swatch/None")
                .attr("Name", "None")
                .attr("ColorEditable", "false")
                .attr("ColorRemovable", "false")
                .attr("Visible", "true")
                .attr("SwatchCreatorID", "7937"),
        );
        for g in gradients {
            root.push(g);
        }
        for (gi, g) in d.color_groups.iter().enumerate() {
            let mut el = El::new("ColorGroup")
                .attr("Self", format!("ColorGroup/{}", escape_id(&g.name)))
                .attr("Name", &g.name)
                .attr("IsRootColorGroup", "false");
            for (k, n) in g.swatches.iter().enumerate() {
                if let Some(id) = self.swatch_ids.get(n) {
                    el.push(El::new("ColorGroupSwatch").attr("Self", format!("u{gi}ColorGroupSwatch{k}")).attr("SwatchItemRef", id));
                }
            }
            root.push(el);
        }
        for st in &d.stroke_styles {
            let id = format!("CustomStrokeStyle/{}", st.name);
            let pcts = |v: &[f64]| v.iter().map(|x| num(x * 100.0)).collect::<Vec<_>>().join(" ");
            let el = match &st.kind {
                designcraft_doc::StrokeType::Stripes { bands } => {
                    let flat: Vec<f64> = bands.iter().flat_map(|(a, w)| [*a, a + w]).collect();
                    Some(El::new("StripedStrokeStyle").attr("StripeArray", pcts(&flat)))
                }
                designcraft_doc::StrokeType::Dashed { pattern } => {
                    Some(El::new("DashedStrokeStyle").attr("DashArray", pattern.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" ")))
                }
                designcraft_doc::StrokeType::Dotted => Some(El::new("DottedStrokeStyle").attr("DotArray", "0 200")),
                _ => None,
            };
            if let Some(el) = el {
                root.push(el.attr("Self", id).attr("Name", &st.name));
            }
        }
        for s in STROKE_STYLES {
            root.push(El::new("StrokeStyle").attr("Self", format!("StrokeStyle/$ID/{s}")).attr("Name", format!("$ID/{s}")));
        }
        document(&root)
    }

    // ---------- Fonts.xml ----------

    fn note_font(&mut self, family: &str, style: &str) {
        self.fonts.entry(family.to_string()).or_default().insert(style.to_string());
    }

    fn fonts_part(&mut self) -> Vec<u8> {
        let mut root = pkg_root("Fonts");
        for f in &self.d.styles.composite_fonts {
            let mut el = El::new("CompositeFont").attr("Self", format!("CompositeFont/{}", escape_id(&f.name))).attr("Name", &f.name);
            for (i, e) in f.entries.iter().enumerate() {
                self.note_font(&e.family, &e.style);
                el.push(
                    El::new("CompositeFontEntry")
                        .attr("Self", format!("dcComposite{}_{i}", escape_id(&f.name)))
                        .attr("Name", &e.name)
                        .attr("CustomCharacters", &e.characters)
                        .attr("FontStyle", &e.style)
                        .attr("RelativeSize", num(e.relative_size * 100.0))
                        .attr("HorizontalScale", num(e.horizontal_scale * 100.0))
                        .attr("VerticalScale", num(e.vertical_scale * 100.0))
                        .attr("BaselineShift", num(e.baseline_shift * 100.0))
                        .attr("ScaleOption", bool_s(e.scale_option))
                        .child(El::new("Properties").child(p("AppliedFont", "string", &e.family))),
                );
            }
            root.push(el);
        }
        let fonts = std::mem::take(&mut self.fonts);
        for (fam, styles) in &fonts {
            let fid = format!("di{:x}", {
                self.next += 1;
                self.next
            });
            let mut fe = El::new("FontFamily").attr("Self", &fid).attr("Name", fam);
            for s in styles {
                fe.push(
                    El::new("Font")
                        .attr("Self", format!("{fid}Fontn{fam} {s}"))
                        .attr("FontFamily", fam)
                        .attr("Name", format!("{fam} {s}"))
                        .attr("PostScriptName", format!("$ID/{}-{}", fam.replace(' ', ""), s.replace(' ', "")))
                        .attr("Status", "NotAvailable")
                        .attr("FontStyleName", s)
                        .attr("FontType", "OpenTypeTT")
                        .attr("WritingScript", "0")
                        .attr("FullName", format!("{fam} {s}"))
                        .attr("FullNameNative", format!("{fam} {s}"))
                        .attr("FontStyleNameNative", s)
                        .attr("PlatformName", "$ID/")
                        .attr("Version", "$ID/")
                        .attr("TypekitID", "$ID/"),
                );
            }
            root.push(fe);
        }
        self.fonts = fonts;
        document(&root)
    }

    // ---------- Preferences.xml ----------

    fn prefs_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let s = &d.settings;
        let mut root = pkg_root("Preferences");
        root.push(
            El::new("DocumentPreference")
                .attr("PageHeight", num(s.page_height))
                .attr("PageWidth", num(s.page_width))
                // InDesign creates this many pages before reading the spreads and replaces only the first
                // spread, so (as InDesign itself writes) the value must be 1.
                .attr("PagesPerDocument", 1)
                .attr("FacingPages", bool_s(s.facing_pages))
                .attr("DocumentBleedTopOffset", num(s.bleed[0]))
                .attr("DocumentBleedBottomOffset", num(s.bleed[1]))
                .attr("DocumentBleedInsideOrLeftOffset", num(s.bleed[2]))
                .attr("DocumentBleedOutsideOrRightOffset", num(s.bleed[3]))
                .attr("DocumentBleedUniformSize", bool_s(s.bleed.iter().all(|b| *b == s.bleed[0])))
                .attr("SlugTopOffset", num(s.slug[0]))
                .attr("SlugBottomOffset", num(s.slug[1]))
                .attr("SlugInsideOrLeftOffset", num(s.slug[2]))
                .attr("SlugRightOrOutsideOffset", num(s.slug[3]))
                .attr("DocumentSlugUniformSize", bool_s(s.slug.iter().all(|b| *b == s.slug[0])))
                .attr("PreserveLayoutWhenShuffling", "true")
                .attr("AllowPageShuffle", "true")
                .attr("OverprintBlack", bool_s(s.overprint_black))
                .attr("PageBinding", if s.right_to_left_binding { "RightToLeft" } else { "LeftToRight" })
                .attr("ColumnDirection", "Horizontal")
                .attr(
                    "Intent",
                    match s.intent {
                        designcraft_doc::Intent::Print => "PrintIntent",
                        designcraft_doc::Intent::Web => "WebIntent",
                        designcraft_doc::Intent::Mobile => "MobileIntent",
                    },
                )
                .attr("CreatePrimaryTextFrame", bool_s(s.primary_text_frame))
                .attr("ColumnGuideLocked", "true")
                .attr("MasterTextFrame", bool_s(s.primary_text_frame))
                .attr("SnippetImportUsesOriginalLocation", "false"),
        );
        let page = d.parents.first().and_then(|p| p.pages.first()).or_else(|| d.page(0));
        if let Some(pg) = page {
            root.push(margin_el(pg));
        }
        root.push(
            El::new("ViewPreference")
                .attr("HorizontalMeasurementUnits", names::unit_out(s.horizontal_units))
                .attr("VerticalMeasurementUnits", names::unit_out(s.vertical_units))
                .attr("RulerOrigin", "SpreadOrigin")
                .attr("CursorKeyIncrement", num(s.keyboard_increment))
                .attr("PointsPerInch", "72"),
        );
        let bg = &s.baseline_grid;
        root.push(
            El::new("GridPreference")
                .attr("HorizontalGridlineDivision", num(s.grid.horizontal))
                .attr("VerticalGridlineDivision", num(s.grid.vertical))
                .attr("HorizontalGridSubdivision", s.grid.subdivisions)
                .attr("VerticalGridSubdivision", s.grid.subdivisions)
                .attr("GridsInBack", bool_s(s.grid.in_back))
                .attr("BaselineStart", num(bg.start))
                .attr("BaselineDivision", num(bg.increment))
                .attr("BaselineViewThreshold", pct(bg.view_threshold))
                .attr(
                    "BaselineGridRelativeOption",
                    match bg.relative_to {
                        designcraft_doc::GridRelative::TopOfPage => "TopOfPageOfBaselineGridRelativeOption",
                        designcraft_doc::GridRelative::TopMargin => "TopOfMarginOfBaselineGridRelativeOption",
                    },
                ),
        );
        root.push(El::new("PasteboardPreference").attr("PasteboardMargins", pt(s.pasteboard.0, s.pasteboard.1)));
        root.push(self.footnote_option_el());
        root.push(El::new("TransparencyPreference").attr(
            "BlendingSpace",
            match s.blend_space {
                designcraft_doc::BlendSpace::Cmyk => "CMYK",
                designcraft_doc::BlendSpace::Rgb => "RGB",
            },
        ));
        let a = &s.advanced_type;
        root.push(
            El::new("TextPreference")
                .attr("SuperscriptSize", num(a.superscript_size))
                .attr("SuperscriptPosition", num(a.superscript_position))
                .attr("SubscriptSize", num(a.subscript_size))
                .attr("SubscriptPosition", num(a.subscript_position)),
        );
        document(&root)
    }

    // ---------- Styles.xml ----------

    fn styles_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let styles = d.styles.clone();
        let mut root = pkg_root("Styles");

        // Character styles.
        let mut cg = Group::default();
        for cs in &styles.character {
            let id = names::style_self("CharacterStyle", CHAR_BUILTINS, &cs.name);
            let mut el =
                El::new("CharacterStyle").attr("Self", &id).attr("Imported", "false").attr("Name", names::style_name_out(CHAR_BUILTINS, &cs.name));
            let mut props = Vec::new();
            if cs.name != st::NO_CHAR_STYLE {
                let based = cs.based_on.as_deref().filter(|b| *b != st::NO_CHAR_STYLE);
                match based {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[No character style]")),
                }
                self.char_attrs(&mut el, &mut props, &cs.chars);
            }
            cg.insert(&cs.name, with_props(el, props));
        }
        let mut rc = El::new("RootCharacterStyleGroup").attr("Self", self.fresh());
        cg.emit(&mut rc, "CharacterStyleGroup", "");
        root.push(rc);

        // Paragraph styles.
        let mut pg = Group::default();
        for ps in &styles.paragraph {
            let id = names::style_self("ParagraphStyle", PARA_BUILTINS, &ps.name);
            let mut el =
                El::new("ParagraphStyle").attr("Self", &id).attr("Name", names::style_name_out(PARA_BUILTINS, &ps.name)).attr("Imported", "false");
            let mut props = Vec::new();
            let (pa, ca) = if ps.name == designcraft_doc::NO_PARA_STYLE {
                // The root style carries the complete attribute set.
                let mut pa = designcraft_doc::ParaProps::common([&designcraft_doc::ParaProps::default()]);
                pa.merge(&ps.para);
                let mut ca = designcraft_doc::CharProps::common([&designcraft_doc::CharProps::default()]);
                ca.merge(&ps.chars);
                (pa, ca)
            } else {
                let next = ps.next_style.as_deref().unwrap_or(&ps.name);
                el.set("NextStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, next));
                match ps.based_on.as_deref().filter(|b| *b != designcraft_doc::NO_PARA_STYLE) {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("ParagraphStyle", PARA_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[No paragraph style]")),
                }
                (ps.para.clone(), ps.chars.clone())
            };
            self.para_attrs(&mut el, &mut props, &pa);
            self.char_attrs(&mut el, &mut props, &ca);
            pg.insert(&ps.name, with_props(el, props));
        }
        let mut rp = El::new("RootParagraphStyleGroup").attr("Self", self.fresh());
        pg.emit(&mut rp, "ParagraphStyleGroup", "");
        root.push(rp);

        // Cell and table styles.
        let mut cells = El::new("RootCellStyleGroup")
            .attr("Self", self.fresh())
            .child(El::new("CellStyle").attr("Self", "CellStyle/$ID/[None]").attr("AppliedParagraphStyle", NO_PARA_ID).attr("Name", "$ID/[None]"));
        for cs in styles.cell.iter().filter(|c| c.name != designcraft_doc::NO_CELL_STYLE) {
            let mut el = El::new("CellStyle")
                .attr("Self", names::style_self("CellStyle", names::CELL_BUILTINS, &cs.name))
                .attr("Name", names::style_name_out(names::CELL_BUILTINS, &cs.name));
            if let Some(f) = &cs.fill {
                el.set("FillColor", self.sw(f));
            }
            if let Some(t) = cs.fill_tint {
                el.set("FillTint", pct(t as f64));
            }
            if let Some(i) = cs.insets {
                for (k, v) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().zip(i) {
                    el.set(k, num(v));
                }
            }
            for (key, value) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().zip(cs.inset_overrides) {
                if let Some(value) = value {
                    el.set(key, num(value));
                }
            }
            if let Some(vj) = cs.vj {
                el.set("VerticalJustification", names::vj_out(vj));
            }
            if let Some(st) = &cs.stroke {
                for side in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"] {
                    self.cell_stroke_attrs(&mut el, side, st);
                }
            }
            for (side, attrs) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().zip(&cs.strokes) {
                self.partial_cell_stroke_attrs(&mut el, side, attrs);
            }
            if let Some(ps) = &cs.paragraph_style {
                el.set("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, ps));
            }
            let base = cs.based_on.as_deref().unwrap_or(designcraft_doc::NO_CELL_STYLE);
            let based_on = if base == designcraft_doc::NO_CELL_STYLE {
                p("BasedOn", "string", "$ID/[None]")
            } else {
                p("BasedOn", "object", names::style_self("CellStyle", names::CELL_BUILTINS, base))
            };
            cells = cells.child(with_props(el, vec![based_on]));
        }
        root.push(cells);
        let mut tables = El::new("RootTableStyleGroup").attr("Self", self.fresh());
        if !styles.table.iter().any(|s| s.name == designcraft_doc::NO_TABLE_STYLE) {
            tables.push(El::new("TableStyle").attr("Self", "TableStyle/$ID/[No table style]").attr("Name", "$ID/[No table style]"));
        }
        if !styles.table.iter().any(|s| s.name == designcraft_doc::BASIC_TABLE) {
            tables.push(with_props(
                El::new("TableStyle").attr("Self", "TableStyle/$ID/[Basic Table]").attr("Name", "$ID/[Basic Table]"),
                vec![p("BasedOn", "string", "$ID/[No table style]")],
            ));
        }
        for ts in &styles.table {
            let mut el = El::new("TableStyle")
                .attr("Self", names::style_self("TableStyle", names::TABLE_BUILTINS, &ts.name))
                .attr("Name", names::style_name_out(names::TABLE_BUILTINS, &ts.name));
            for (k, v) in [
                ("HeaderRegionCellStyle", &ts.header),
                ("BodyRegionCellStyle", &ts.body),
                ("FooterRegionCellStyle", &ts.footer),
                ("LeftColumnRegionCellStyle", &ts.left_column),
                ("RightColumnRegionCellStyle", &ts.right_column),
            ] {
                if let Some(n) = v {
                    el.set(k, names::style_self("CellStyle", names::CELL_BUILTINS, n));
                }
            }
            for (key, value) in [
                ("HeaderRegionSameAsBodyRegion", ts.header_same_as_body),
                ("FooterRegionSameAsBodyRegion", ts.footer_same_as_body),
                ("LeftColumnRegionSameAsBodyRegion", ts.left_column_same_as_body),
                ("RightColumnRegionSameAsBodyRegion", ts.right_column_same_as_body),
            ] {
                if let Some(value) = value {
                    el.set(key, value);
                }
            }
            if let Some(b) = &ts.border {
                for side in ["Top", "Left", "Bottom", "Right"] {
                    self.cell_stroke_attrs(&mut el, &format!("{side}Border"), b);
                }
            }
            for (side, attrs) in ["TopBorder", "LeftBorder", "BottomBorder", "RightBorder"].iter().zip(&ts.borders) {
                self.partial_cell_stroke_attrs(&mut el, side, attrs);
            }
            if let Some(a) = &ts.alt_rows {
                el.set("StartRowFillColor", self.sw(&a.first_color));
                el.set("StartRowFillCount", a.first);
                el.set("StartRowFillTint", pct(a.first_tint as f64));
                el.set("EndRowFillColor", self.sw(&a.next_color));
                el.set("EndRowFillCount", a.next);
                el.set("EndRowFillTint", pct(a.next_tint as f64));
            }
            self.partial_alt_fill_attrs(&mut el, "Row", &ts.row_fills);
            self.partial_alt_fill_attrs(&mut el, "Column", &ts.column_fills);
            self.partial_alt_stroke_attrs(&mut el, "Row", &ts.row_strokes);
            self.partial_alt_stroke_attrs(&mut el, "Column", &ts.column_strokes);
            if let Some(v) = ts.space_before {
                el.set("SpaceBefore", num(v));
            }
            if let Some(v) = ts.space_after {
                el.set("SpaceAfter", num(v));
            }
            let mut props = Vec::new();
            if ts.name != designcraft_doc::NO_TABLE_STYLE {
                let base = ts.based_on.as_deref().unwrap_or(designcraft_doc::NO_TABLE_STYLE);
                props.push(if base == designcraft_doc::NO_TABLE_STYLE {
                    p("BasedOn", "string", "$ID/[No table style]")
                } else {
                    p("BasedOn", "object", names::style_self("TableStyle", names::TABLE_BUILTINS, base))
                });
            }
            tables = tables.child(with_props(el, props));
        }
        root.push(tables);
        // Named numbered lists.
        for l in &self.d.settings.lists {
            root.push(
                El::new("NumberingList")
                    .attr("Self", format!("NumberingList/{}", escape_id(&l.name)))
                    .attr("Name", &l.name)
                    .attr("ContinueNumbersAcrossStories", bool_s(l.continue_across_stories))
                    .attr("ContinueNumbersAcrossDocuments", "false"),
            );
        }

        // Object styles.
        let mut og = Group::default();
        let mut have: Vec<&str> = styles.object.iter().map(|o| o.name.as_str()).collect();
        let defaults = designcraft_doc::Styles::default();
        let mut all: Vec<designcraft_doc::ObjectStyle> = styles.object.clone();
        for b in &defaults.object {
            if !have.contains(&b.name.as_str()) {
                all.push(b.clone());
                have.push(&b.name);
            }
        }
        for os in &all {
            let id = names::style_self("ObjectStyle", OBJECT_BUILTINS, &os.name);
            let mut el = El::new("ObjectStyle").attr("Self", &id).attr("Name", names::style_name_out(OBJECT_BUILTINS, &os.name));
            let mut props = Vec::new();
            if os.name != designcraft_doc::NO_OBJECT_STYLE {
                match os.based_on.as_deref().filter(|b| *b != designcraft_doc::NO_OBJECT_STYLE) {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("ObjectStyle", OBJECT_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[None]")),
                }
            }
            el.set("EnableFill", bool_s(os.fill.is_some()));
            el.set("EnableStroke", bool_s(os.stroke.is_some()));
            el.set("EnableParagraphStyle", bool_s(os.paragraph_style.is_some()));
            el.set("EnableTextFrameGeneralOptions", bool_s(os.text_frame.is_some()));
            if let Some(f) = &os.fill {
                el.set("FillColor", self.sw(&f.swatch));
                el.set("FillTint", pct(f.tint as f64));
            }
            if let Some(s) = &os.stroke {
                self.stroke_attrs(&mut el, s);
            }
            if let Some(ps) = &os.paragraph_style {
                el.set("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, ps));
            }
            let mut el = with_props(el, props);
            if let Some(tf) = &os.text_frame {
                el.push(text_frame_pref(tf, &self.sw(&tf.column_rule_color)));
            }
            og.insert(&os.name, el);
        }
        let mut ro = El::new("RootObjectStyleGroup").attr("Self", self.fresh());
        og.emit(&mut ro, "ObjectStyleGroup", "");
        root.push(ro);
        document(&root)
    }

    fn char_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &CharAttrs) {
        if let Some(v) = a.leading_aki {
            el.set("LeadingAki", num(v.unwrap_or(-1.0)));
        }
        if let Some(v) = a.trailing_aki {
            el.set("TrailingAki", num(v.unwrap_or(-1.0)));
        }
        if let Some(v) = a.tsume {
            el.set("Tsume", num(v * 100.0));
        }
        if let Some(v) = a.jidori {
            el.set("Jidori", v);
        }
        if let Some(v) = a.leading_model {
            el.set("LeadingModel", crate::cjk::leading_out(v));
        }
        if let Some(v) = a.character_alignment {
            el.set("CharacterAlignment", crate::cjk::alignment_out(v));
        }
        if let Some(f) = &a.font_family {
            props.push(p("AppliedFont", "string", f.clone()));
            let style = a.font_style.clone().unwrap_or_else(|| "Regular".into());
            self.note_font(f, &style);
        }
        if let Some(v) = &a.font_style {
            el.set("FontStyle", v);
        }
        if let Some(v) = a.size {
            el.set("PointSize", num(v));
        }
        if let Some(l) = a.leading {
            let (v, ty) = names::leading_out(l);
            props.push(p("Leading", ty, v));
        }
        if let Some(k) = a.kerning {
            match k {
                designcraft_doc::Kerning::Metrics => el.set("KerningMethod", "$ID/Metrics"),
                designcraft_doc::Kerning::Optical => el.set("KerningMethod", "$ID/Optical"),
                designcraft_doc::Kerning::None => el.set("KerningMethod", "None"),
                designcraft_doc::Kerning::Manual(v) => el.set("KerningValue", num(v)),
            }
        }
        if let Some(v) = a.tracking {
            el.set("Tracking", num(v));
        }
        if let Some(v) = a.h_scale {
            el.set("HorizontalScale", pct(v));
        }
        if let Some(v) = a.v_scale {
            el.set("VerticalScale", pct(v));
        }
        if let Some(v) = a.baseline_shift {
            el.set("BaselineShift", num(v));
        }
        if let Some(v) = a.skew {
            el.set("Skew", num(v));
        }
        if let Some(v) = &a.fill {
            el.set("FillColor", self.sw(v));
        }
        if let Some(v) = a.fill_tint {
            el.set("FillTint", pct(v as f64));
        }
        if let Some(v) = &a.stroke {
            el.set("StrokeColor", self.sw(v));
        }
        if let Some(v) = a.stroke_tint {
            el.set("StrokeTint", pct(v as f64));
        }
        if let Some(v) = a.stroke_weight {
            el.set("StrokeWeight", num(v));
        }
        if let Some(v) = a.capitalization {
            el.set("Capitalization", names::caps_out(v));
        }
        if let Some(v) = a.position {
            el.set("Position", names::position_out(v));
        }
        if let Some(v) = a.underline {
            el.set("Underline", bool_s(v));
        }
        if let Some(v) = a.strikethrough {
            el.set("StrikeThru", bool_s(v));
        }
        if let Some(v) = a.ligatures {
            el.set("Ligatures", bool_s(v));
        }
        for (k, w, o, c, t) in [
            ("Underline", a.underline_weight, a.underline_offset, &a.underline_color, a.underline_tint),
            ("StrikeThru", a.strikethrough_weight, a.strikethrough_offset, &a.strikethrough_color, a.strikethrough_tint),
        ] {
            if let Some(w) = w {
                el.set(&format!("{k}Weight"), num(w.unwrap_or(-9999.0)));
            }
            if let Some(o) = o {
                el.set(&format!("{k}Offset"), num(o.unwrap_or(-9999.0)));
            }
            if let Some(c) = c {
                el.set(&format!("{k}Color"), if c.is_empty() { "Text Color".into() } else { self.sw(c) });
            }
            if let Some(t) = t {
                el.set(&format!("{k}Tint"), num(t as f64 * 100.0));
            }
        }
        if let Some(list) = &a.otf_features {
            for (tag, _, attr) in designcraft_doc::otf::TOGGLES {
                el.set(attr, bool_s(designcraft_doc::otf::is_on(list, tag)));
            }
            let fig = designcraft_doc::otf::figures(list);
            if let Some(f) = designcraft_doc::otf::FIGURES.iter().find(|f| f.0 == fig) {
                el.set("OTFFigureStyle", f.3);
            }
            el.set("OTFStylisticSets", designcraft_doc::otf::stylistic_sets(list).to_string());
        }
        if let Some(v) = &a.glyph_form {
            el.set("GlyphForm", v);
        }
        if let Some(v) = a.no_break {
            el.set("NoBreak", bool_s(v));
        }
        if let Some(v) = a.digits {
            el.set("DigitsType", names::digits_out(v));
        }
        if let Some(v) = a.character_direction {
            el.set("CharacterDirection", crate::arabic::direction_out(v));
        }
        if let Some(v) = a.allow_kashidas {
            el.set("Kashidas", if v { "DefaultKashidas" } else { "KashidasOff" });
        }
        if let Some(v) = a.diacritic_position {
            el.set("DiacriticPosition", crate::arabic::diacritic_out(v));
        }
        if let Some(v) = a.diacritic_x_offset {
            el.set("XOffsetDiacritic", num(v));
        }
        if let Some(v) = a.diacritic_y_offset {
            el.set("YOffsetDiacritic", num(v));
        }
        if let Some(v) = &a.positional_form {
            el.set("PositionalForm", v);
        }
        if let Some(r) = a.ruby.as_ref() {
            el.set("RubyFlag", bool_s(!r.is_empty()));
            el.set("RubyString", r.as_str());
        }
        if let Some(k) = a.kenten {
            el.set("KentenKind", if k { "KentenSesameDot" } else { "None" });
        }
        if let Some(c) = &a.kenten_character
            && !c.is_empty()
        {
            el.set("KentenKind", "Custom");
            el.set("KentenCustomCharacter", c);
        }
        if let Some(v) = a.tate_chu_yoko_x_offset {
            el.set("TatechuyokoXOffset", num(v));
        }
        if let Some(v) = a.tate_chu_yoko_y_offset {
            el.set("TatechuyokoYOffset", num(v));
        }
        if let Some(v) = a.tate_chu_yoko {
            el.set("Tatechuyoko", bool_s(v));
        }
        if let Some(v) = a.warichu {
            el.set("Warichu", bool_s(v));
        }
        if let Some(v) = a.warichu_lines {
            el.set("WarichuLines", v);
        }
        if let Some(v) = a.warichu_size {
            el.set("WarichuSize", num(v));
        }
        if let Some(v) = a.warichu_line_spacing {
            el.set("WarichuLineSpacing", num(v));
        }
        if let Some(v) = a.warichu_alignment {
            el.set("WarichuAlignment", crate::cjk::warichu_align_out(v));
        }
        if let Some(v) = a.warichu_chars_before_break {
            el.set("WarichuCharsBeforeBreak", v);
        }
        if let Some(v) = a.warichu_chars_after_break {
            el.set("WarichuCharsAfterBreak", v);
        }
        if let Some(list) = &a.conditions {
            el.set("AppliedConditions", list.iter().map(|c| format!("Condition/{}", escape_id(c))).collect::<Vec<_>>().join(" "));
        }
        if let Some(v) = &a.language {
            el.set("AppliedLanguage", format!("$ID/{v}"));
        }
    }

    fn para_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs) {
        if let Some(v) = &a.mojikumi {
            props.push(p("Mojikumi", if v.starts_with("MojikumiTable/") { "object" } else { "enumeration" }, v));
        }
        if let Some(v) = &a.kinsoku_type {
            el.set("KinsokuType", v);
        }
        if let Some(v) = a.kinsoku_hang {
            el.set(
                "KinsokuHangType",
                match v {
                    designcraft_doc::cjk::KinsokuHang::None => "None",
                    designcraft_doc::cjk::KinsokuHang::Regular => "KinsokuHangRegular",
                    designcraft_doc::cjk::KinsokuHang::Force => "KinsokuHangForce",
                },
            );
        }
        if let Some(v) = a.bunri_kinshi {
            el.set("BunriKinshi", bool_s(v));
        }
        if let Some(v) = a.rensuuji {
            el.set("Rensuuji", bool_s(v));
        }
        if let Some(v) = a.treat_ideographic_space_as_space {
            el.set("TreatIdeographicSpaceAsSpace", bool_s(v));
        }
        if let Some(Some(k)) = &a.kinsoku {
            props.push(p(
                "KinsokuSet",
                if k.name.is_empty() { "enumeration" } else { "object" },
                if k.name.is_empty() { "Nothing".into() } else { format!("KinsokuTable/{}", escape_id(&k.name)) },
            ));
        }
        if let Some(n) = &a.list_name {
            el.set(
                "AppliedNumberingList",
                if n.is_empty() { "NumberingList/$ID/[Default]".to_string() } else { format!("NumberingList/{}", escape_id(n)) },
            );
        }
        if let Some(v) = a.start_at {
            match v {
                Some(n) => {
                    el.set("NumberingContinue", "false");
                    el.set("NumberingStartAt", n);
                }
                None => el.set("NumberingContinue", "true"),
            }
        }
        // Nested and GREP styles: property lists of records.
        if let Some(list) = &a.nested_styles {
            let mut l = El::new("AllNestedStyles").attr("type", "list");
            for ns in list {
                let (ty, delim) = names::nested_until_out(&ns.until);
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &ns.style)))
                        .child(p("Delimiter", ty, delim))
                        .child(p("Repetition", "long", ns.count.to_string()))
                        .child(p("Inclusive", "boolean", bool_s(ns.through))),
                );
            }
            props.push(l);
        }
        if let Some(list) = &a.nested_line_styles {
            let mut l = El::new("AllNestedLineStyles").attr("type", "list");
            for ns in list {
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &ns.style)))
                        .child(p("LineCount", "long", ns.lines.to_string())),
                );
            }
            props.push(l);
        }
        if let Some(list) = &a.grep_styles {
            let mut l = El::new("AllGREPStyles").attr("type", "list");
            for g in list {
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &g.style)))
                        .child(p("GrepExpression", "string", g.pattern.clone())),
                );
            }
            props.push(l);
        }
        macro_rules! n {
            ($f:ident, $k:literal) => {
                if let Some(v) = a.$f {
                    el.set($k, num(v as f64));
                }
            };
            ($f:ident, $k:literal, pct) => {
                if let Some(v) = a.$f {
                    el.set($k, pct(v as f64));
                }
            };
        }
        macro_rules! b {
            ($f:ident, $k:literal) => {
                if let Some(v) = a.$f {
                    el.set($k, bool_s(v));
                }
            };
        }
        if let Some(v) = a.align {
            el.set("Justification", names::align_out(v));
        }
        if let Some(v) = a.direction {
            el.set(
                "ParagraphDirection",
                if v == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
            );
        }
        if let Some(v) = a.kashidas {
            el.set("Kashidas", if v { "DefaultKashidas" } else { "KashidasOff" });
        }
        if let Some(v) = &a.arabic_justification {
            el.set("ParagraphJustification", v);
        }
        if let Some(Some(v)) = a.paragraph_kashida_width {
            el.set("ParagraphKashidaWidth", num(v));
        }
        n!(left_indent, "LeftIndent");
        n!(right_indent, "RightIndent");
        n!(first_line_indent, "FirstLineIndent");
        n!(last_line_indent, "LastLineIndent");
        n!(space_before, "SpaceBefore");
        n!(space_after, "SpaceAfter");
        n!(drop_cap_lines, "DropCapLines");
        n!(drop_cap_chars, "DropCapCharacters");
        if let Some(g) = a.grid_align {
            match g {
                designcraft_doc::GridAlign::None => el.set("GridAlignment", "None"),
                designcraft_doc::GridAlign::AllLines => {
                    el.set("GridAlignment", "AlignToBaseline");
                    el.set("GridAlignFirstLineOnly", "false");
                }
                designcraft_doc::GridAlign::FirstLineOnly => {
                    el.set("GridAlignment", "AlignToBaseline");
                    el.set("GridAlignFirstLineOnly", "true");
                }
            }
        }
        if let Some(c) = a.composer {
            el.set(
                "Composer",
                match c {
                    designcraft_doc::Composer::Paragraph => "HL Composer",
                    designcraft_doc::Composer::SingleLine => "HL Single",
                },
            );
        }
        b!(hyphenate, "Hyphenation");
        n!(hyph_min_word, "HyphenateWordsLongerThan");
        n!(hyph_after_first, "HyphenateAfterFirst");
        n!(hyph_before_last, "HyphenateBeforeLast");
        n!(hyph_limit, "HyphenateLadderLimit");
        n!(hyph_zone, "HyphenationZone");
        b!(hyph_capitalized, "HyphenateCapitalizedWords");
        b!(hyph_last_word, "HyphenateLastWord");
        b!(hyph_across_column, "HyphenateAcrossColumns");
        if let Some(v) = a.hyph_weight {
            el.set("HyphenWeight", num((v * 10.0).round()));
        }
        n!(word_space_min, "MinimumWordSpacing", pct);
        n!(word_space_desired, "DesiredWordSpacing", pct);
        n!(word_space_max, "MaximumWordSpacing", pct);
        n!(letter_space_min, "MinimumLetterSpacing", pct);
        n!(letter_space_desired, "DesiredLetterSpacing", pct);
        n!(letter_space_max, "MaximumLetterSpacing", pct);
        n!(glyph_scale_min, "MinimumGlyphScaling", pct);
        n!(glyph_scale_desired, "DesiredGlyphScaling", pct);
        n!(glyph_scale_max, "MaximumGlyphScaling", pct);
        n!(auto_leading, "AutoLeading", pct);
        if let Some(v) = a.single_word_justify {
            el.set("SingleWordJustification", names::align_out(v));
        }
        n!(keep_with_next, "KeepWithNext");
        b!(keep_lines_together, "KeepLinesTogether");
        b!(keep_all_lines, "KeepAllLinesTogether");
        n!(keep_first, "KeepFirstLines");
        n!(keep_last, "KeepLastLines");
        if let Some(v) = a.start_paragraph {
            el.set("StartParagraph", names::start_para_out(v));
        }
        if let Some(sc) = a.span_columns {
            match sc {
                SpanColumns::Single => el.set("SpanColumnType", "SingleColumn"),
                SpanColumns::Span(n) | SpanColumns::Split(n) => {
                    el.set("SpanColumnType", if matches!(sc, SpanColumns::Span(_)) { "SpanColumns" } else { "SplitColumns" });
                    props.push(if n == 0 {
                        p("SpanSplitColumnCount", "enumeration", "All")
                    } else {
                        p("SpanSplitColumnCount", "long", n.to_string())
                    });
                }
            }
        }
        n!(split_inside_gutter, "SplitColumnInsideGutter");
        n!(split_outside_gutter, "SplitColumnOutsideGutter");
        if let Some(r) = &a.rule_above {
            self.rule(el, props, "RuleAbove", r);
        }
        if let Some(r) = &a.rule_below {
            self.rule(el, props, "RuleBelow", r);
        }
        if let Some(tabs) = &a.tabs {
            let mut tl = El::new("TabList").attr("type", "list");
            for t in tabs {
                tl.push(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("Alignment", "enumeration", names::tab_align_out(t.align)))
                        .child(p("AlignmentCharacter", "string", if t.align_on.is_empty() { ".".to_string() } else { t.align_on.clone() }))
                        .child(p("Leader", "string", t.leader.clone()))
                        .child(p("Position", "unit", num(t.position))),
                );
            }
            props.push(tl);
        }
        if let Some(l) = a.list_type {
            el.set(
                "BulletsAndNumberingListType",
                match l {
                    ListType::None => "NoList",
                    ListType::Bullets => "BulletList",
                    ListType::Numbers => "NumberedList",
                },
            );
        }
        if let Some(n) = a.number_style {
            props.push(p("NumberingFormat", "string", names::numbering_format_out(n)));
        }
        if let Some(x) = &a.number_expression {
            el.set("NumberingExpression", names::list_text_out(x));
        }
        if let Some(x) = &a.list_separator {
            el.set("BulletsTextAfter", names::list_text_out(x));
        }
        if let Some(c) = a.bullet_char.as_deref().and_then(|s| s.chars().next()) {
            props.push(El::new("BulletChar").attr("BulletCharacterType", "UnicodeOnly").attr("BulletCharacterValue", u32::from(c).to_string()));
        }
        if let Some(b) = a.balance_ragged {
            props.push(p("BalanceRaggedLines", "enumeration", if b { "FullyBalanced" } else { "NoBalancing" }));
        }
        b!(shading_on, "ParagraphShadingOn");
        if let Some(c) = &a.shading_color {
            props.push(p("ParagraphShadingColor", "object", self.sw(c)));
        }
        n!(shading_tint, "ParagraphShadingTint", pct);
        const SIDES: [&str; 4] = ["Top", "Left", "Bottom", "Right"];
        if let Some(o) = a.shading_offsets {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphShading{side}Offset"), num(o[i]));
            }
        }
        b!(border_on, "ParagraphBorderOn");
        if let Some(c) = &a.border_color {
            props.push(p("ParagraphBorderColor", "object", self.sw(c)));
        }
        n!(border_tint, "ParagraphBorderTint", pct);
        if let Some(w) = a.border_weights {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphBorder{side}LineWeight"), num(w[i]));
            }
        }
        if let Some(o) = a.border_offsets {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphBorder{side}Offset"), num(o[i]));
            }
        }
    }

    fn rule(&mut self, el: &mut El, props: &mut Vec<El>, k: &str, r: &Rule) {
        el.set(k, bool_s(r.on));
        el.set(&format!("{k}LineWeight"), num(r.weight));
        el.set(&format!("{k}Tint"), pct(r.tint as f64));
        el.set(&format!("{k}Width"), if r.column_width { "ColumnWidth" } else { "TextWidth" });
        el.set(&format!("{k}Offset"), num(r.offset));
        el.set(&format!("{k}LeftIndent"), num(r.left_indent));
        el.set(&format!("{k}RightIndent"), num(r.right_indent));
        props.push(p(&format!("{k}Color"), "object", self.sw(&r.color)));
    }

    fn stroke_attrs(&self, el: &mut El, s: &designcraft_doc::Stroke) {
        el.set("StrokeColor", self.sw(&s.swatch));
        el.set("StrokeWeight", num(if s.is_none() { 0.0 } else { s.weight }));
        el.set("StrokeTint", pct(s.tint as f64));
        el.set("StrokeType", names::stroke_type_out(&s.kind));
        el.set("StrokeAlignment", names::stroke_align_out(s.align));
        el.set("EndCap", names::cap_out(s.cap));
        el.set("EndJoin", names::join_out(s.join));
        el.set("MiterLimit", num(s.miter_limit));
        el.set("LeftLineEnd", names::arrow_out(s.start));
        el.set("RightLineEnd", names::arrow_out(s.end));
        el.set("GapColor", self.sw(&s.gap_swatch));
        el.set("GapTint", pct(s.gap_tint as f64));
        el.set("OverprintStroke", bool_s(s.overprint));
        el.set("OverprintGap", bool_s(s.gap_overprint));
    }

    // ---------- spreads ----------

    fn spread_el_doc(&mut self, sp: &Spread, si: usize) -> El {
        let first = self.d.first_page_of_spread(si);
        let mut el = self.spread_common(sp, false, first);
        el.name = "Spread".into();
        el
    }

    fn spread_el(&mut self, sp: &Spread, _master: bool) -> El {
        let mut el = self.spread_common(sp, true, 0);
        el.name = "MasterSpread".into();
        el
    }

    fn spread_common(&mut self, sp: &Spread, master: bool, first_page: usize) -> El {
        let d = self.d;
        let facing = d.settings.facing_pages;
        let (ox, oy) = spread_origin(sp, facing);
        let mut el = El::new("Spread").attr("Self", uid(sp.id.0));
        if let (true, Some(info)) = (master, &sp.parent) {
            el.set("Name", info.label());
            el.set("NamePrefix", &info.prefix);
            el.set("BaseName", &info.name);
        }
        el.set("ShowMasterItems", bool_s(sp.pages.iter().all(|p| p.show_parent_items)));
        el.set("PageCount", sp.pages.len());
        if !master {
            el.set("BindingLocation", sp.pages.iter().filter(|p| p.side == PageSide::Left).count());
            el.set("AllowPageShuffle", bool_s(sp.allow_shuffle));
        }
        el.set("ItemTransform", "1 0 0 1 0 0");
        let based_on = sp.parent.as_ref().and_then(|i| i.based_on);
        for (i, pg) in sp.pages.iter().enumerate() {
            let name = if master { sp.parent.as_ref().map(|i| i.prefix.clone()).unwrap_or_default() } else { d.page_name(first_page + i) };
            let applied = if master { based_on } else { pg.parent };
            let pe = El::new("Page")
                .attr("Self", uid(pg.id.0))
                .attr("Name", name)
                .attr("AppliedMaster", applied.map(|s| uid(s.0)).unwrap_or_else(|| "n".into()))
                .attr("OverrideList", "")
                .attr("GeometricBounds", format!("0 0 {} {}", num(pg.height), num(pg.width)))
                .attr("ItemTransform", format!("1 0 0 1 {} {}", num(pg.x - ox), num(-oy)))
                .child(margin_el(pg));
            el.push(pe);
        }
        let origin = Affine::translate((-ox, -oy));
        for it in &sp.items {
            let e = self.item_el(it, origin * it.xf);
            el.push(e);
        }
        el
    }

    fn item_el(&mut self, it: &Item, xf: Affine) -> El {
        let d = self.d;
        let path_text = matches!(&it.content, Content::Text(t) if t.options.path.is_some());
        let tag = match (&it.content, it.shape) {
            (Content::Text(_), _) if path_text => match it.shape {
                Shape::GraphicLine => "GraphicLine",
                Shape::Oval => "Oval",
                Shape::Rectangle => "Rectangle",
                _ => "Polygon",
            },
            (Content::Text(_), _) => "TextFrame",
            (Content::Group { .. }, Shape::Group) => "Group",
            (_, Shape::Oval) => "Oval",
            (_, Shape::Polygon) | (_, Shape::Path) => "Polygon",
            (_, Shape::GraphicLine) => "GraphicLine",
            _ => "Rectangle",
        };
        let mut el = El::new(tag).attr("Self", uid(it.id.0));
        if let Content::Text(t) = &it.content
            && let Some(pt) = &t.options.path
        {
            // Type on a path: a TextPath child holds the story.
            let len = designcraft_geom::warp::PathWarp::new(&it.path.to_bezpath(), false).length();
            el.push(
                El::new("TextPath")
                    .attr("Self", format!("{}tp", uid(it.id.0)))
                    .attr("ParentStory", uid(t.story.0))
                    .attr("PreviousTextFrame", "n")
                    .attr("NextTextFrame", "n")
                    .attr("PathEffect", "RainbowPathEffect")
                    .attr("PathAlignment", "CenterPathAlignment")
                    .attr("TextAlignment", names::path_align_out(pt.align))
                    .attr("FlipPathEffect", if pt.flip { "Flipped" } else { "NotFlipped" })
                    .attr("StartBracket", num(pt.start))
                    .attr("EndBracket", num(len)),
            );
        } else if let Content::Text(t) = &it.content {
            let (prev, next) = self.threads.get(&it.id.0).copied().unwrap_or((None, None));
            el.set("ParentStory", uid(t.story.0));
            el.set("PreviousTextFrame", prev.map(uid).unwrap_or_else(|| "n".into()));
            el.set("NextTextFrame", next.map(uid).unwrap_or_else(|| "n".into()));
        }
        if tag != "Group" {
            el.set(
                "ContentType",
                match it.content {
                    Content::Text(_) => "TextType",
                    Content::Graphic(_) => "GraphicType",
                    _ => "Unassigned",
                },
            );
        }
        el.set("Name", if it.name.is_empty() { "$ID/".to_string() } else { it.name.clone() });
        el.set("ItemLayer", uid(it.layer.0));
        el.set("Visible", bool_s(!it.hidden));
        el.set("Locked", bool_s(it.locked));
        el.set("Nonprinting", bool_s(it.nonprinting));
        let os = if it.object_style.is_empty() { designcraft_doc::NO_OBJECT_STYLE } else { &it.object_style };
        el.set("AppliedObjectStyle", names::style_self("ObjectStyle", OBJECT_BUILTINS, os));
        el.set("ItemTransform", affine_s(xf));
        el.set("FillColor", self.sw(&it.fill.swatch));
        el.set("FillTint", pct(it.fill.tint as f64));
        el.set("OverprintFill", bool_s(it.fill.overprint));
        if let Some([x0, y0, x1, y1]) = it.fill.gradient_vector {
            // Gradient Swatch tool vector: start, length and angle in the item's space.
            el.set("GradientFillStart", format!("{} {}", num(x0), num(y0)));
            el.set("GradientFillLength", num((x1 - x0).hypot(y1 - y0)));
            el.set("GradientFillAngle", num((-(y1 - y0)).atan2(x1 - x0).to_degrees()));
        } else if let Some(a) = it.fill.gradient_angle {
            el.set("GradientFillAngle", num(a));
        }
        self.stroke_attrs(&mut el, &it.stroke);
        // Corner options: map path-order corners to the named corners by geometry.
        if !it.corners.is_none() {
            let names_by_index = corner_names(&it.path);
            for (i, c) in it.corners.corners.iter().enumerate() {
                let n = names_by_index[i];
                el.set(&format!("{n}CornerOption"), names::corner_out(c.shape));
                el.set(&format!("{n}CornerRadius"), num(c.size));
            }
        }
        let mut props = Vec::new();
        if tag != "Group" && !it.path.is_empty() {
            props.push(path_geometry(&it.path));
        }
        let mut el = with_props(el, props);
        // Transparency.
        let ds = &it.effects.drop_shadow;
        if it.opacity < 1.0 || it.blend != designcraft_color::BlendMode::Normal || ds.on || it.effects.feather > 0.0 {
            let mut t = El::new("TransparencySetting");
            t.push(El::new("BlendingSetting").attr("BlendMode", names::blend_out(it.blend)).attr("Opacity", pct(it.opacity as f64)));
            if ds.on {
                let a = self.d.light_angle(ds.angle, ds.global_light).to_radians();
                t.push(
                    El::new("DropShadowSetting")
                        .attr("Mode", "Drop")
                        .attr("XOffset", num(-ds.distance * a.cos()))
                        .attr("YOffset", num(ds.distance * a.sin()))
                        .attr("Size", num(ds.size))
                        .attr("Spread", num(ds.spread))
                        .attr("Opacity", pct(ds.opacity as f64))
                        .attr("EffectColor", self.sw(&ds.color)),
                );
            }
            if it.effects.feather > 0.0 {
                t.push(El::new("FeatherSetting").attr("Mode", "Standard").attr("Width", num(it.effects.feather)));
            }
            el.push(t);
        }
        if let Content::Text(t) = &it.content {
            el.push(text_frame_pref(&t.options, &self.sw(&t.options.column_rule_color)));
            if let Some((start, inc)) = t.options.baseline_grid {
                el.push(
                    El::new("BaselineFrameGridOption")
                        .attr("UseCustomBaselineFrameGrid", "true")
                        .attr("StartingOffsetForBaselineFrameGrid", num(start))
                        .attr("BaselineFrameGridIncrement", num(inc)),
                );
            }
        }
        el.push(text_wrap_pref(&it.wrap));
        match &it.content {
            Content::Group { items } => {
                for c in items {
                    let e = self.item_el(c, c.xf);
                    el.push(e);
                }
            }
            Content::Graphic(g) => {
                if g.auto_fit != designcraft_doc::Fitting::None || g.fit_align != 4 || g.crop != [0.0; 4] {
                    el.push(
                        El::new("FrameFittingOption")
                            .attr("AutoFit", bool_s(g.auto_fit != designcraft_doc::Fitting::None))
                            .attr("TopCrop", num(g.crop[0]))
                            .attr("LeftCrop", num(g.crop[1]))
                            .attr("BottomCrop", num(g.crop[2]))
                            .attr("RightCrop", num(g.crop[3]))
                            .attr("FittingOnEmptyFrame", names::fitting_out(g.auto_fit))
                            .attr("FittingAlignment", names::anchor_out(g.fit_align)),
                    );
                }
                if let Some(asset) = d.assets.get(&g.asset) {
                    let e = self.image_el(asset, g);
                    el.push(e);
                }
            }
            _ => {}
        }
        el
    }

    fn image_el(&mut self, asset: &designcraft_doc::Asset, g: &designcraft_doc::Graphic) -> El {
        let is_pdf = asset.mime == "application/pdf";
        let mut el = El::new(if is_pdf { "PDF" } else { "Image" }).attr("Self", self.fresh());
        if let Some((pw, ph)) = asset.pixels
            && g.size.0 > 0.0
            && g.size.1 > 0.0
        {
            el.set("ActualPpi", pt(pw as f64 * 72.0 / g.size.0, ph as f64 * 72.0 / g.size.1));
        }
        el.set("ItemTransform", affine_s(g.xf));
        let embed = !asset.data.is_empty() && (self.opts.embed_images || asset.link.is_none());
        let mut props = Vec::new();
        if embed {
            let mut c = El::new("Contents");
            c.children.push(Node::CData(base64_encode(&asset.data)));
            props.push(c);
        }
        props.push(El::new("GraphicBounds").attr("Left", "0").attr("Top", "0").attr("Right", num(g.size.0)).attr("Bottom", num(g.size.1)));
        let mut el = with_props(el, props);
        if !g.wrap.is_default() {
            el.push(text_wrap_pref(&g.wrap));
        }
        let uri = match &asset.link {
            Some(l) => path_to_uri(l),
            None => format!("file:{}", asset.name),
        };
        el.push(
            El::new("Link")
                .attr("Self", self.fresh())
                .attr("AssetURL", "$ID/")
                .attr("AssetID", "$ID/")
                .attr("LinkResourceURI", uri)
                .attr("LinkResourceFormat", format!("$ID/{}", format_name(&asset.mime)))
                .attr("StoredState", if embed { "Embedded" } else { "Normal" })
                .attr("LinkClassID", "35906")
                .attr("LinkClientID", "257")
                .attr("LinkResourceModified", "false")
                .attr("LinkObjectModified", "false")
                .attr("ShowInUI", "true")
                .attr("CanEmbed", "true")
                .attr("CanUnembed", "true")
                .attr("CanPackage", "true")
                .attr("ImportPolicy", "NoAutoImport")
                .attr("ExportPolicy", "NoAutoExport"),
        );
        el
    }

    /// `<Index>`: nested topics with their See / See also cross-references.
    fn index_el(&mut self) -> El {
        let mut topics = self.index_topics.clone();
        for (_, _, target) in &self.index_xrefs {
            topics.insert(vec![target.clone()]);
        }
        fn build(ex: &mut Ex, topics: &BTreeSet<Vec<String>>, prefix: &[String]) -> Vec<El> {
            let mut out = Vec::new();
            for t in topics.iter().filter(|t| t.len() == prefix.len() + 1 && t.starts_with(prefix)) {
                let mut el = El::new("Topic").attr("Self", topic_self(t)).attr("SortOrder", "").attr("Name", t.last().map_or("", String::as_str));
                for (from, kind, target) in ex.index_xrefs.clone() {
                    if from == *t {
                        el.push(
                            El::new("CrossReference")
                                .attr("Self", ex.fresh())
                                .attr("ReferencedTopic", topic_self(std::slice::from_ref(&target)))
                                .attr("CrossReferenceType", kind)
                                .attr("CustomTypeString", ""),
                        );
                    }
                }
                for c in build(ex, topics, t) {
                    el.push(c);
                }
                out.push(el);
            }
            out
        }
        let mut el = El::new("Index").attr("Self", "ix");
        for t in build(self, &topics, &[]) {
            el.push(t);
        }
        el
    }

    fn footnote_option_el(&self) -> El {
        let o = &self.d.footnote_options;
        let marker = if o.ref_char_style == st::NO_CHAR_STYLE {
            NO_CHAR_ID.to_string()
        } else {
            names::style_self("CharacterStyle", CHAR_BUILTINS, &o.ref_char_style)
        };
        let enumv = |name: &str, v: &str| {
            let mut e = El::new(name).attr("type", "enumeration");
            e.children.push(Node::Text(v.into()));
            e
        };
        let mut color = El::new("RuleColor").attr("type", "object");
        color.children.push(Node::Text(self.sw(&o.rule.color)));
        let mut el = El::new("FootnoteOption")
            .attr("StartAt", o.start_at)
            .attr("Prefix", &o.prefix)
            .attr("Suffix", &o.suffix)
            .attr("FootnoteTextStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, &o.para_style))
            .attr("FootnoteMarkerStyle", marker)
            .attr("SeparatorText", &o.separator)
            .attr("SpaceBetween", num(o.space_between))
            .attr("Spacer", num(o.space_before))
            .attr("FootnoteFirstBaselineOffset", names::first_baseline_out(o.first_baseline))
            .attr("FootnoteMinimumFirstBaselineOffset", num(o.first_baseline_min))
            .attr("EnableStraddling", bool_s(o.span_columns))
            .attr("RuleOn", bool_s(o.rule.on))
            .attr("RuleLineWeight", num(o.rule.weight))
            .attr("RuleTint", num(o.rule.tint as f64 * 100.0))
            .attr("RuleLeftIndent", num(o.rule.left_indent))
            .attr("RuleWidth", num(o.rule.width))
            .attr("RuleOffset", num(o.rule.offset));
        let restart = names::NOTE_RESTART.iter().find(|r| r.0 == o.restart).map_or("DontRestart", |r| r.1);
        let affix = names::NOTE_AFFIX.iter().find(|r| r.0 == o.affix_in).map_or("NoPrefixSuffix", |r| r.1);
        let mut props = El::new("Properties");
        props.push(enumv("FootnoteNumberingStyle", names::note_style_out(o.style)));
        props.push(enumv("RestartNumbering", restart));
        props.push(enumv("ShowPrefixSuffix", affix));
        props.push(enumv("MarkerPositioning", names::note_marker_out(o.ref_position)));
        props.push(color);
        el.push(props);
        el
    }

    /// `<Footnote>`: the footnote's paragraphs, the first starting with the footnote number marker
    /// and the separator (as InDesign stores them).
    fn footnote_el(&mut self, note: &Story) -> El {
        let mut el = El::new("Footnote");
        let mut paras = self.story_paras(note);
        let lead = vec![Node::Pi("ACE".into(), "4".into()), Node::Text(self.d.footnote_options.separator.clone())];
        if let Some(Node::El(csr)) =
            paras.first_mut().and_then(|p| p.children.iter_mut().find(|n| matches!(n, Node::El(e) if e.name == "CharacterStyleRange")))
        {
            match csr.children.iter_mut().find_map(|n| match n {
                Node::El(c) if c.name == "Content" => Some(c),
                _ => None,
            }) {
                Some(c) => {
                    c.children.splice(0..0, lead);
                }
                None => {
                    let mut c = El::new("Content");
                    c.children = lead;
                    csr.children.insert(0, Node::El(c));
                }
            }
        }
        for p in paras {
            el.push(p);
        }
        el
    }

    // ---------- stories ----------

    fn story_el(&mut self, s: &Story) -> El {
        let mut el = El::new("Story").attr("Self", uid(s.id.0)).attr("AppliedTOCStyle", "n").attr("TrackChanges", "false").attr("StoryTitle", "$ID/");
        el.push(
            El::new("StoryPreference")
                .attr("OpticalMarginAlignment", "false")
                .attr("OpticalMarginSize", "12")
                .attr("FrameType", "TextFrameType")
                .attr("StoryOrientation", if s.vertical { "Vertical" } else { "Horizontal" })
                .attr(
                    "StoryDirection",
                    if s.direction == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
                ),
        );
        for psr in self.story_paras(s) {
            el.push(psr);
        }
        el
    }

    /// `ParagraphStyleRange`s of a story (or a table cell's story).
    fn story_paras(&mut self, s: &Story) -> Vec<El> {
        let mut paras = Vec::new();
        let ranges = s.para_ranges();
        let n = ranges.len();
        for (pi, r) in ranges.into_iter().enumerate() {
            let pf = s.paras.get(pi).cloned().unwrap_or_default();
            let mut psr = self.psr_el(&pf);
            let last = pi + 1 == n;
            let ends_with_break = s.text.get(r.clone()).and_then(|t| t.chars().next_back()).is_some_and(st::is_break_char);
            // Character runs intersecting the paragraph.
            let mut segs: Vec<(std::ops::Range<usize>, CharFormat)> = Vec::new();
            for (rr, f) in s.runs() {
                let a = rr.start.max(r.start);
                let b = rr.end.min(r.end);
                if a < b {
                    segs.push((a..b, f.clone()));
                }
            }
            if segs.is_empty() {
                let f = s.char_format_at(r.start).clone();
                segs.push((r.start..r.start, f));
            }
            let nseg = segs.len();
            for (k, (rr, f)) in segs.into_iter().enumerate() {
                let seg_start = rr.start;
                let text = &s.text[rr];
                // Split at break characters.
                let mut cur = String::new();
                let mut pending: Vec<Node> = Vec::new();
                let mut csrs: Vec<El> = Vec::new();
                let flush_text = |cur: &mut String, pending: &mut Vec<Node>| {
                    if !cur.is_empty() {
                        pending.push(Node::Text(std::mem::take(cur)));
                    }
                };
                let flush_content = |pending: &mut Vec<Node>, out: &mut Vec<Node>| {
                    if !pending.is_empty() {
                        let mut c = El::new("Content");
                        c.children = std::mem::take(pending);
                        out.push(Node::El(c));
                    }
                };
                let mut out: Vec<Node> = Vec::new();
                for (ci, ch) in text.char_indices() {
                    let ace = match ch {
                        st::PAGE_NUMBER | st::NEXT_PAGE_NUMBER | st::PREV_PAGE_NUMBER => Some("18"),
                        st::SECTION_MARKER => Some("19"),
                        st::INDENT_HERE => Some("7"),
                        st::RIGHT_INDENT_TAB => Some("8"),
                        _ => None,
                    };
                    let brk = match ch {
                        st::COLUMN_BREAK => Some("NextColumn"),
                        st::FRAME_BREAK => Some("NextFrame"),
                        st::PAGE_BREAK => Some("NextPage"),
                        st::ODD_PAGE_BREAK => Some("NextOddPage"),
                        st::EVEN_PAGE_BREAK => Some("NextEvenPage"),
                        _ => None,
                    };
                    if let Some(code) = ace {
                        flush_text(&mut cur, &mut pending);
                        pending.push(Node::Pi("ACE".into(), code.into()));
                    } else if ch == st::TABLE_ANCHOR {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if let Some(t) = s.para_table(pi) {
                            out.push(Node::El(self.table_el(t)));
                        }
                    } else if ch == designcraft_doc::ANCHOR_MARK {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::ANCHOR_MARK).count();
                        if let Some(a) = s.anchors.get(k) {
                            out.push(Node::El(
                                El::new("HyperlinkTextDestination")
                                    .attr("Self", anchor_self(a.id))
                                    .attr("Name", &a.name)
                                    .attr("Hidden", "false")
                                    .attr("DestinationUniqueKey", a.id & 0xFFFF_FFFF),
                            ));
                        }
                    } else if ch == designcraft_doc::OBJECT_MARK {
                        // An anchored object: the item inside the range with its anchoring settings.
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::OBJECT_MARK).count();
                        if let Some(o) = s.objects.get(k) {
                            let mut e = self.item_el(&o.item, o.item.xf);
                            use designcraft_doc::anchored::AnchorAlign as A;
                            let setting = match &o.position {
                                designcraft_doc::AnchorPosition::Inline { y_offset } => {
                                    El::new("AnchoredObjectSetting").attr("AnchoredPosition", "InlinePosition").attr("AnchorYoffset", num(*y_offset))
                                }
                                designcraft_doc::AnchorPosition::AboveLine { align, space_before, space_after } => El::new("AnchoredObjectSetting")
                                    .attr("AnchoredPosition", "AboveLine")
                                    .attr(
                                        "HorizontalAlignment",
                                        match align {
                                            A::Left => "LeftAlign",
                                            A::Center => "CenterAlign",
                                            A::Right => "RightAlign",
                                        },
                                    )
                                    .attr("AnchorSpaceAbove", num(*space_before))
                                    .attr("AnchorYoffset", num(*space_after)),
                                designcraft_doc::AnchorPosition::Custom {
                                    x_relative,
                                    y_relative,
                                    x_offset,
                                    y_offset,
                                    object_point,
                                    ref_point,
                                    keep_within_column,
                                } => El::new("AnchoredObjectSetting")
                                    .attr("AnchoredPosition", "Anchored")
                                    .attr("AnchorPoint", names::anchor_out(*object_point))
                                    .attr("HorizontalReferencePoint", names::anchor_rel_out(*x_relative, false))
                                    .attr("VerticalReferencePoint", names::anchor_rel_out(*y_relative, true))
                                    .attr("HorizontalAlignment", ["LeftAlign", "CenterAlign", "RightAlign"][(*ref_point % 3) as usize])
                                    .attr("VerticalAlignment", ["TopAlign", "CenterAlign", "BottomAlign"][(*ref_point / 3).min(2) as usize])
                                    .attr("AnchorXoffset", num(*x_offset))
                                    .attr("AnchorYoffset", num(*y_offset))
                                    .attr("PinPosition", bool_s(*keep_within_column)),
                            };
                            e.push(setting);
                            out.push(Node::El(e));
                        }
                    } else if ch == designcraft_doc::INDEX_MARK {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::INDEX_MARK).count();
                        if let Some(r) = s.index_refs.get(k).filter(|r| !r.topics.is_empty()) {
                            use designcraft_doc::index::IndexRange as R;
                            for n in 1..=r.topics.len() {
                                self.index_topics.insert(r.topics[..n].to_vec());
                            }
                            let (kind, limit) = match &r.range {
                                R::CurrentPage => (Some("CurrentPage"), None),
                                R::ToEndOfStory => (Some("ToEndOfStory"), None),
                                R::NextParagraphs(n) => (Some("ForNextNParagraphs"), Some(*n)),
                                R::SuppressPageRange => (Some("SuppressPageNumbers"), None),
                                R::See(t) => {
                                    self.index_xrefs.push((r.topics.clone(), "See", t.clone()));
                                    (None, None)
                                }
                                R::SeeAlso(t) => {
                                    self.index_xrefs.push((r.topics.clone(), "SeeAlso", t.clone()));
                                    (Some("CurrentPage"), None)
                                }
                            };
                            if let Some(kind) = kind {
                                let mut el = El::new("PageReference")
                                    .attr("Self", self.fresh())
                                    .attr("PageReferenceType", kind)
                                    .attr("ReferencedTopic", topic_self(&r.topics))
                                    .attr("Id", k + 1);
                                if let Some(n) = limit {
                                    el.set("PageReferenceLimit", n);
                                }
                                out.push(Node::El(el));
                            }
                        }
                    } else if ch == designcraft_doc::XREF_MARK {
                        // A source wrapping its current text (InDesign regenerates it on update).
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if !out.is_empty() {
                            let mut c = self.csr_el(&f);
                            c.children.append(&mut out);
                            csrs.push(c);
                        }
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::XREF_MARK).count();
                        if let Some(x) = s.xrefs.get(k) {
                            let me = self.fresh();
                            let fmt = self.d.xref_formats.iter().position(|ff| ff.name == x.format).map_or("n".to_string(), xref_format_self);
                            let text = self.d.xref_values(x.target).map_or("??".into(), |mut v| {
                                v.page = "?".into();
                                designcraft_doc::xref::expand(self.d.xref_format(&x.format).map_or("<fullPara />", |ff| ff.definition.as_str()), &v)
                            });
                            let mut c = self.csr_el(&f);
                            let mut content = El::new("Content");
                            content.children.push(Node::Text(text));
                            c.push(content);
                            let mut src = El::new("CrossReferenceSource")
                                .attr("Self", &me)
                                .attr("AppliedFormat", fmt)
                                .attr("Name", format!("Cross-Reference {}", self.xref_sources.len() + 1))
                                .attr("Hidden", "false");
                            src.push(c);
                            csrs.push(src);
                            self.xref_sources.push((me, x.target));
                        }
                    } else if ch == designcraft_doc::NOTE_MARK {
                        // Editorial notes aren't written to IDML yet.
                    } else if ch == designcraft_doc::ENDNOTE_REF {
                        // Endnotes aren't written to IDML yet: the endnote frame keeps their text
                        // (numbers included) and the references are left out.
                    } else if ch == designcraft_doc::FOOTNOTE_REF {
                        // The reference: its own range carrying the reference position.
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if !out.is_empty() {
                            let mut c = self.csr_el(&f);
                            c.children.append(&mut out);
                            csrs.push(c);
                        }
                        if let Some(n) = s.notes.get(s.notes_before(seg_start + ci)) {
                            let mut c = self.csr_el(&f);
                            let pos = self.d.footnote_options.ref_position;
                            if pos != designcraft_doc::Position::Normal {
                                c.set("Position", names::position_out(pos));
                            }
                            c.push(self.footnote_el(&n.text));
                            csrs.push(c);
                        }
                    } else if let Some(bt) = brk {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if !out.is_empty() {
                            let mut c = self.csr_el(&f);
                            c.children.append(&mut out);
                            csrs.push(c);
                        }
                        let mut c = self.csr_el(&f);
                        c.set("ParagraphBreakType", bt);
                        c.push(El::new("Br"));
                        csrs.push(c);
                    } else {
                        cur.push(ch);
                    }
                }
                flush_text(&mut cur, &mut pending);
                flush_content(&mut pending, &mut out);
                let is_last_seg = k + 1 == nseg;
                // A break character that ends the paragraph was written as its `Br`.
                if is_last_seg && !last && !ends_with_break {
                    out.push(Node::El(El::new("Br")));
                }
                if !out.is_empty() || csrs.is_empty() {
                    let mut c = self.csr_el(&f);
                    c.children.extend(out);
                    csrs.push(c);
                }
                for c in csrs {
                    psr.push(c);
                }
            }
            paras.push(psr);
        }
        paras
    }

    fn cell_stroke_attrs(&self, el: &mut El, prefix: &str, s: &designcraft_doc::CellStroke) {
        el.set(&format!("{prefix}StrokeWeight"), num(s.weight));
        el.set(&format!("{prefix}StrokeColor"), self.sw(&s.color));
        el.set(&format!("{prefix}StrokeTint"), pct(s.tint as f64));
        el.set(&format!("{prefix}StrokeType"), names::stroke_type_out(&s.kind));
    }

    fn partial_cell_stroke_attrs(&self, el: &mut El, prefix: &str, attrs: &designcraft_doc::CellStrokeAttrs) {
        if let Some(value) = attrs.weight {
            el.set(&format!("{prefix}StrokeWeight"), num(value));
        }
        if let Some(value) = &attrs.color {
            el.set(&format!("{prefix}StrokeColor"), self.sw(value));
        }
        if let Some(value) = attrs.tint {
            el.set(&format!("{prefix}StrokeTint"), pct(value as f64));
        }
        if let Some(value) = &attrs.kind {
            el.set(&format!("{prefix}StrokeType"), names::stroke_type_out(value));
        }
    }

    fn partial_alt_fill_attrs(&self, el: &mut El, kind: &str, attrs: &designcraft_doc::AltFillsAttrs) {
        if let Some(value) = attrs.first {
            el.set(&format!("Start{kind}FillCount"), value);
        }
        if let Some(value) = &attrs.first_color {
            el.set(&format!("Start{kind}FillColor"), self.sw(value));
        }
        if let Some(value) = attrs.first_tint {
            el.set(&format!("Start{kind}FillTint"), pct(value as f64));
        }
        if let Some(value) = attrs.next {
            el.set(&format!("End{kind}FillCount"), value);
        }
        if let Some(value) = &attrs.next_color {
            el.set(&format!("End{kind}FillColor"), self.sw(value));
        }
        if let Some(value) = attrs.next_tint {
            el.set(&format!("End{kind}FillTint"), pct(value as f64));
        }
        if let Some(value) = attrs.skip_first {
            el.set(&format!("SkipFirstAlternatingFill{kind}s"), value);
        }
        if let Some(value) = attrs.skip_last {
            el.set(&format!("SkipLastAlternatingFill{kind}s"), value);
        }
    }

    fn partial_alt_stroke_attrs(&self, el: &mut El, kind: &str, attrs: &designcraft_doc::AltStrokesAttrs) {
        if let Some(value) = attrs.first {
            el.set(&format!("Start{kind}StrokeCount"), value);
        }
        if let Some(value) = attrs.next {
            el.set(&format!("End{kind}StrokeCount"), value);
        }
        self.partial_cell_stroke_attrs(el, &format!("Start{kind}"), &attrs.first_stroke);
        self.partial_cell_stroke_attrs(el, &format!("End{kind}"), &attrs.next_stroke);
        if kind == "Column"
            && let Some(value) = &attrs.next_stroke.kind
        {
            el.attrs.retain(|(key, _)| key != "EndColumnStrokeType");
            el.set("EndColumnLineStyle", names::stroke_type_out(value));
        }
        if let Some(value) = attrs.skip_first {
            el.set(&format!("SkipFirstAlternatingStroke{kind}s"), value);
        }
        if let Some(value) = attrs.skip_last {
            el.set(&format!("SkipLastAlternatingStroke{kind}s"), value);
        }
    }

    /// `<Table>` with rows, columns and cells (cell names are `column:row`).
    fn table_el(&mut self, t: &designcraft_doc::Table) -> El {
        let id = uid(t.id);
        let (h, f) = (t.header_rows(), t.footer_rows());
        let mut el = El::new("Table")
            .attr("Self", &id)
            .attr("HeaderRowCount", h)
            .attr("FooterRowCount", f)
            .attr("BodyRowCount", t.nrows() - h - f)
            .attr("ColumnCount", t.ncols())
            .attr(
                "AppliedTableStyle",
                names::style_self("TableStyle", names::TABLE_BUILTINS, if t.style.is_empty() { designcraft_doc::BASIC_TABLE } else { &t.style }),
            )
            .attr(
                "TableDirection",
                if t.options.direction == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
            )
            .attr("SpaceBefore", num(t.options.space_before))
            .attr("SpaceAfter", num(t.options.space_after))
            .attr("HeaderBehavior", if t.options.repeat_header { "RepeatOnEachTextColumn" } else { "RepeatOnce" })
            .attr("FooterBehavior", if t.options.repeat_footer { "RepeatOnEachTextColumn" } else { "RepeatOnce" });
        for (i, side) in ["Top", "Left", "Bottom", "Right"].iter().enumerate() {
            self.cell_stroke_attrs(&mut el, &format!("{side}Border"), t.options.border_for(i));
        }
        for (kind, alt) in [("Row", &t.options.alt_rows), ("Column", &t.options.alt_cols)] {
            if let Some(a) = alt {
                el.set(&format!("Start{kind}FillColor"), self.sw(&a.first_color));
                el.set(&format!("Start{kind}FillCount"), a.first);
                el.set(&format!("Start{kind}FillTint"), pct(a.first_tint as f64));
                el.set(&format!("End{kind}FillColor"), self.sw(&a.next_color));
                el.set(&format!("End{kind}FillCount"), a.next);
                el.set(&format!("End{kind}FillTint"), pct(a.next_tint as f64));
                el.set(&format!("SkipFirstAlternatingFill{kind}s"), a.skip_first);
                el.set(&format!("SkipLastAlternatingFill{kind}s"), a.skip_last);
            } else {
                // A local off value must override any enabled pattern in the named style.
                el.set(&format!("Start{kind}FillCount"), 0);
                el.set(&format!("End{kind}FillCount"), 0);
            }
        }
        for (kind, strokes) in [("Row", &t.options.row_strokes), ("Column", &t.options.column_strokes)] {
            if let Some(strokes) = strokes {
                el.set(&format!("Start{kind}StrokeCount"), strokes.first);
                el.set(&format!("End{kind}StrokeCount"), strokes.next);
                self.cell_stroke_attrs(&mut el, &format!("Start{kind}"), &strokes.first_stroke);
                self.cell_stroke_attrs(&mut el, &format!("End{kind}"), &strokes.next_stroke);
                if kind == "Column" {
                    el.attrs.retain(|(key, _)| key != "EndColumnStrokeType");
                    el.set("EndColumnLineStyle", names::stroke_type_out(&strokes.next_stroke.kind));
                }
                el.set(&format!("SkipFirstAlternatingStroke{kind}s"), strokes.skip_first);
                el.set(&format!("SkipLastAlternatingStroke{kind}s"), strokes.skip_last);
            } else {
                el.set(&format!("Start{kind}StrokeCount"), 0);
                el.set(&format!("End{kind}StrokeCount"), 0);
            }
        }
        for (r, row) in t.rows.iter().enumerate() {
            let exact = row.mode == designcraft_doc::RowHeightMode::Exactly;
            el.push(
                El::new("Row")
                    .attr("Self", format!("{id}Row{r}"))
                    .attr("Name", r)
                    .attr("SingleRowHeight", num(row.height.max(3.0)))
                    .attr("MinimumHeight", num(row.height.max(0.0)))
                    .attr("AutoGrow", bool_s(!exact)),
            );
        }
        for (c, col) in t.columns.iter().enumerate() {
            el.push(El::new("Column").attr("Self", format!("{id}Column{c}")).attr("Name", c).attr("SingleColumnWidth", num(col.width)));
        }
        let owners = t.owners();
        for r in 0..t.nrows() {
            for c in 0..t.ncols() {
                if owners[r * t.ncols() + c] != (r, c) {
                    continue;
                }
                let Some(cell) = t.cell(r, c) else { continue };
                let vj = match cell.vj {
                    designcraft_doc::VerticalJustification::Top => "TopAlign",
                    designcraft_doc::VerticalJustification::Center => "CenterAlign",
                    designcraft_doc::VerticalJustification::Bottom => "BottomAlign",
                    designcraft_doc::VerticalJustification::Justify => "JustifyAlign",
                };
                let mut ce = El::new("Cell")
                    .attr("Self", format!("{id}i{}", r * t.ncols() + c))
                    .attr("Name", format!("{c}:{r}"))
                    .attr("RowSpan", cell.row_span)
                    .attr("ColumnSpan", cell.col_span)
                    .attr(
                        "AppliedCellStyle",
                        names::style_self(
                            "CellStyle",
                            names::CELL_BUILTINS,
                            if cell.style.is_empty() { designcraft_doc::NO_CELL_STYLE } else { &cell.style },
                        ),
                    )
                    .attr("FillColor", self.sw(&cell.fill))
                    .attr("FillTint", pct(cell.fill_tint as f64))
                    .attr("TopInset", num(cell.insets[0]))
                    .attr("LeftInset", num(cell.insets[1]))
                    .attr("BottomInset", num(cell.insets[2]))
                    .attr("RightInset", num(cell.insets[3]))
                    .attr("VerticalJustification", vj)
                    .attr("RotationAngle", num(cell.rotation));
                for (i, side) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().enumerate() {
                    // Do not turn absent native defaults into explicit shared-edge candidates.
                    if cell.stroke_defined[i]
                        || cell.border_overrides[i]
                        || cell.stroke_priorities[i] != 0
                        || cell.strokes[i] != designcraft_doc::CellStroke::default()
                    {
                        self.cell_stroke_attrs(&mut ce, side, &cell.strokes[i]);
                    }
                    let priority = if cell.border_overrides[i] && !cell.stroke_defined[i] {
                        cell.stroke_priorities[i].max(1)
                    } else {
                        cell.stroke_priorities[i]
                    };
                    if priority != 0 {
                        ce.set(&format!("{side}StrokePriority"), priority);
                    }
                }
                for psr in self.story_paras(&cell.text) {
                    ce.push(psr);
                }
                el.push(ce);
            }
        }
        el
    }

    fn psr_el(&mut self, pf: &ParaFormat) -> El {
        let mut el = El::new("ParagraphStyleRange").attr("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, &pf.style));
        let mut props = Vec::new();
        self.para_attrs(&mut el, &mut props, &pf.para);
        self.char_attrs(&mut el, &mut props, &pf.chars);
        with_props(el, props)
    }

    fn csr_el(&mut self, f: &CharFormat) -> El {
        let id = if f.style.is_empty() || f.style == st::NO_CHAR_STYLE {
            NO_CHAR_ID.to_string()
        } else {
            names::style_self("CharacterStyle", CHAR_BUILTINS, &f.style)
        };
        let mut el = El::new("CharacterStyleRange").attr("AppliedCharacterStyle", id);
        let mut props = Vec::new();
        self.char_attrs(&mut el, &mut props, &f.over);
        with_props(el, props)
    }
}

/// Style folder tree (names `A/B/C` → groups A, B).
#[derive(Default)]
struct Group {
    styles: Vec<El>,
    groups: Vec<(String, Group)>,
}

impl Group {
    fn insert(&mut self, name: &str, el: El) {
        let builtin = name.starts_with('[');
        let mut segs: Vec<&str> = if builtin { vec![name] } else { name.split('/').collect() };
        let _leaf = segs.pop();
        let mut g = self;
        for s in segs {
            let i = match g.groups.iter().position(|(n, _)| n == s) {
                Some(i) => i,
                None => {
                    g.groups.push((s.to_string(), Group::default()));
                    g.groups.len() - 1
                }
            };
            g = &mut g.groups[i].1;
        }
        g.styles.push(el);
    }

    fn emit(self, parent: &mut El, kind: &str, path: &str) {
        for s in self.styles {
            parent.push(s);
        }
        for (name, g) in self.groups {
            let full = if path.is_empty() { name.clone() } else { format!("{path}:{name}") };
            let mut ge = El::new(kind).attr("Self", format!("{kind}/{}", escape_id(&full))).attr("Name", &name);
            g.emit(&mut ge, kind, &full);
            parent.push(ge);
        }
    }
}

fn margin_el(pg: &designcraft_doc::Page) -> El {
    let m = pg.margin_rect();
    let cols = designcraft_doc::column_rects(Rect::new(0.0, 0.0, m.width(), m.height()), pg.columns.count.max(1), pg.columns.gutter);
    let pos: Vec<String> = cols.iter().flat_map(|c| [num(c.x0), num(c.x1)]).collect();
    El::new("MarginPreference")
        .attr("ColumnCount", pg.columns.count.max(1))
        .attr("ColumnGutter", num(pg.columns.gutter))
        .attr("Top", num(pg.margins.top))
        .attr("Bottom", num(pg.margins.bottom))
        .attr("Left", num(pg.margins.inside))
        .attr("Right", num(pg.margins.outside))
        .attr("ColumnDirection", "Horizontal")
        .attr("ColumnsPositions", pos.join(" "))
}

/// `rule_color` is the IDML reference of the column rule's swatch.
/// `TextWrapPreference` of a page item or a placed graphic.
fn text_wrap_pref(w: &designcraft_doc::TextWrap) -> El {
    with_props(
        El::new("TextWrapPreference")
            .attr("Inverse", bool_s(w.invert))
            .attr("ApplyToMasterPageOnly", "false")
            .attr("TextWrapSide", names::wrap_side_out(w.side))
            .attr("TextWrapMode", names::wrap_mode_out(w.mode)),
        vec![
            El::new("TextWrapOffset")
                .attr("Top", num(w.offsets[0]))
                .attr("Left", num(w.offsets[1]))
                .attr("Bottom", num(w.offsets[2]))
                .attr("Right", num(w.offsets[3])),
        ],
    )
}

fn text_frame_pref(o: &TextFrameOptions, rule_color: &str) -> El {
    let mut inset = El::new("InsetSpacing").attr("type", "list");
    for v in o.inset {
        inset.push(p("ListItem", "unit", num(v)));
    }
    let el = El::new("TextFramePreference")
        .attr("TextColumnCount", o.columns.max(1))
        .attr("TextColumnGutter", num(o.gutter))
        .attr("UseFixedColumnWidth", bool_s(o.columns_kind == ColumnsKind::FixedWidth))
        .attr("UseFlexibleColumnWidth", bool_s(o.columns_kind == ColumnsKind::FlexibleWidth))
        .attr("VerticalBalanceColumns", bool_s(o.balance_columns))
        .attr("FirstBaselineOffset", names::first_baseline_out(o.first_baseline))
        .attr("MinimumFirstBaselineOffset", num(o.first_baseline_min))
        .attr("VerticalJustification", names::vj_out(o.vertical_justification))
        .attr("VerticalThreshold", num(o.vj_paragraph_spacing_limit))
        .attr("IgnoreWrap", bool_s(o.ignore_wrap))
        .attr("AutoSizingType", names::auto_size_out(o.auto_size))
        .attr("AutoSizingReferencePoint", names::REF_POINTS[(o.auto_size_ref as usize).min(8)]);
    let el = if o.column_width > 0.0 { el.attr("TextColumnFixedWidth", num(o.column_width)) } else { el };
    let d = TextFrameOptions::default();
    let rule_set = o.column_rule
        || o.column_rule_weight != d.column_rule_weight
        || o.column_rule_color != d.column_rule_color
        || o.column_rule_tint != d.column_rule_tint
        || o.column_rule_offset != d.column_rule_offset
        || o.column_rule_top_inset != d.column_rule_top_inset
        || o.column_rule_bottom_inset != d.column_rule_bottom_inset;
    let el = if rule_set {
        el.attr("ColumnRuleOverride", bool_s(o.column_rule))
            .attr("ColumnRuleStrokeWidth", num(o.column_rule_weight))
            .attr("ColumnRuleStrokeColor", rule_color)
            .attr("ColumnRuleStrokeTint", num(f64::from(o.column_rule_tint) * 100.0))
            .attr("ColumnRuleOffset", num(o.column_rule_offset))
            .attr("ColumnRuleTopInset", num(o.column_rule_top_inset))
            .attr("ColumnRuleBottomInset", num(o.column_rule_bottom_inset))
            .attr("ColumnRuleInsetChainOverride", bool_s(o.column_rule_top_inset == o.column_rule_bottom_inset))
    } else {
        el
    };
    with_props(el, vec![inset])
}

fn path_geometry(path: &PathData) -> El {
    let mut pg = El::new("PathGeometry");
    for sp in &path.subpaths {
        let mut arr = El::new("PathPointArray");
        for a in &sp.anchors {
            arr.push(
                El::new("PathPointType")
                    .attr("Anchor", pt(a.p.x, a.p.y))
                    .attr("LeftDirection", pt(a.h_in.x, a.h_in.y))
                    .attr("RightDirection", pt(a.h_out.x, a.h_out.y)),
            );
        }
        pg.push(El::new("GeometryPathType").attr("PathOpen", bool_s(!sp.closed)).child(arr));
    }
    pg
}

/// IDML corner names for anchors 0..4 of a path (by position on its bounding box).
pub(crate) fn corner_names(path: &PathData) -> [&'static str; 4] {
    let default = ["TopLeft", "TopRight", "BottomRight", "BottomLeft"];
    let Some(sp) = path.subpaths.first() else { return default };
    if sp.anchors.len() != 4 {
        return default;
    }
    let Some(b) = path.bounds() else { return default };
    let c = b.center();
    let mut out = default;
    let mut seen = HashSet::new();
    for (i, a) in sp.anchors.iter().enumerate() {
        let n = match (a.p.y < c.y, a.p.x < c.x) {
            (true, true) => "TopLeft",
            (true, false) => "TopRight",
            (false, false) => "BottomRight",
            (false, true) => "BottomLeft",
        };
        if !seen.insert(n) {
            return default;
        }
        out[i] = n;
    }
    out
}

fn format_name(mime: &str) -> &'static str {
    match mime {
        "image/png" => "Portable Network Graphics (PNG)",
        "image/jpeg" => "JPEG",
        "image/gif" => "GIF",
        "image/tiff" => "TIFF",
        "application/pdf" => "Adobe Portable Document Format (PDF)",
        _ => "Portable Network Graphics (PNG)",
    }
}

/// File path → IDML link URI (`file:/abs/path` with spaces percent-encoded).
pub(crate) fn path_to_uri(path: &str) -> String {
    if path.contains(':') && !path.starts_with('/') && path.chars().nth(1) != Some(':') {
        return path.to_string(); // already a URI
    }
    let mut out = String::from("file:");
    let p = path.replace('\\', "/");
    for c in p.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            c => out.push(c),
        }
    }
    out
}

const CONTAINER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\t<rootfiles>\n\t\t<rootfile full-path=\"designmap.xml\" media-type=\"text/xml\"/>\n\t</rootfiles>\n</container>\n";

const STROKE_STYLES: &[&str] = &[
    "Triple_Stroke",
    "ThickThinThick",
    "ThinThickThin",
    "ThickThick",
    "ThickThin",
    "ThinThick",
    "ThinThin",
    "Japanese Dots",
    "White Diamond",
    "Left Slant Hash",
    "Right Slant Hash",
    "Straight Hash",
    "Wavy",
    "Canned Dotted",
    "Canned Dashed 3x2",
    "Canned Dashed 4x4",
    "Dashed",
    "Solid",
];

fn anchor_self(id: u64) -> String {
    format!("HyperlinkTextDestination/a{id}")
}

fn xref_format_self(i: usize) -> String {
    format!("CrossReferenceFormat/f{i}")
}

/// A cross-reference format as InDesign building blocks.
fn xref_format_el(i: usize, f: &designcraft_doc::XrefFormat) -> El {
    let me = xref_format_self(i);
    let mut el = El::new("CrossReferenceFormat").attr("Self", &me).attr("Name", &f.name).attr("AppliedCharacterStyle", "n");
    let mut k = 0;
    let mut block = |el: &mut El, kind: &str, text: &str, delim: &str, include: bool| {
        el.push(
            El::new("BuildingBlock")
                .attr("Self", format!("{me}BuildingBlock{k}"))
                .attr("BlockType", kind)
                .attr("AppliedCharacterStyle", "n")
                .attr("CustomText", if text.is_empty() { "$ID/" } else { text })
                .attr("AppliedDelimiter", if delim.is_empty() { "$ID/" } else { delim })
                .attr("IncludeDelimiter", bool_s(include)),
        );
        k += 1;
    };
    let mut rest = f.definition.as_str();
    while let Some(a) = rest.find('<') {
        if a > 0 {
            block(&mut el, "CustomStringBuildingBlock", &rest[..a], "", false);
        }
        let Some(b) = rest[a..].find('>') else { break };
        let tag = rest[a + 1..a + b].trim().trim_end_matches('/').trim();
        let attr = |k: &str| tag.find(&format!("{k}=\"")).and_then(|i| tag[i + k.len() + 2..].split('"').next()).unwrap_or("").to_string();
        match tag.split_whitespace().next().unwrap_or("") {
            "fullPara" => block(&mut el, "FullParagraphBuildingBlock", "", "", false),
            "paraText" => block(&mut el, "ParagraphTextBuildingBlock", "", "", false),
            "paraNum" => block(&mut el, "ParagraphNumberBuildingBlock", "", "", false),
            "pageNum" => block(&mut el, "PageNumberBuildingBlock", "", "", false),
            "txtAnchrName" => block(&mut el, "BookmarkNameBuildingBlock", "", "", false),
            "chapNum" => block(&mut el, "ChapterNumberBuildingBlock", "", "", false),
            "fileName" => block(&mut el, "FileNameBuildingBlock", "", "", false),
            "partialPara" => block(&mut el, "PartialParagraphBuildingBlock", "", &attr("delim"), attr("includeDelim") == "true"),
            _ => block(&mut el, "CustomStringBuildingBlock", &rest[a..a + b + 1], "", false),
        }
        rest = &rest[a + b + 1..];
    }
    if !rest.is_empty() {
        block(&mut el, "CustomStringBuildingBlock", rest, "", false);
    }
    el
}

fn topic_self(path: &[String]) -> String {
    let mut s = String::from("ix");
    for n in path {
        s.push_str("Topicn");
        s.push_str(&escape_id(n));
    }
    s
}
