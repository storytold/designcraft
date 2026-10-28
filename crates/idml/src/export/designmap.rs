use super::common::{anchor_self, bool_s, p, uid, with_props, xref_format_self};
use super::*;

impl<'a> Ex<'a> {
    // ---------- designmap ----------

    pub(super) fn designmap(&mut self, masters: &[(String, Vec<u8>)], spreads: &[(String, Vec<u8>)], stories: &[(String, Vec<u8>)]) -> Vec<u8> {
        let d = self.d;
        let story_list: Vec<String> = d.stories.keys().map(|k| uid(k.0)).collect();
        let mut root = El::new("Document")
            .attr("xmlns:idPkg", PKG_NS)
            .attr("DOMVersion", DOM_VERSION)
            .attr("Self", "d")
            .attr("StoryList", story_list.join(" "))
            .attr("Name", format!("{}.indd", d.title))
            .attr("ZeroPoint", {
                let [x, y] = d.zero_point();
                format!("{} {}", num(x), num(y))
            })
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
            root.push(mojikumi_table_el(t));
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
        for v in &d.text_variables {
            root.push(text_variable_el(v));
        }
        // IDML lists layers back to front.
        for l in d.layers.iter().rev() {
            root.push(layer_el(l));
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
        // The document's own hyperlinks (#1446): a destination element, a source element for a
        // page item (a text source is already in its story) and a `Hyperlink` joining them.
        // Stories are written before this, so `link_text_sources` is complete.
        let text_sources: Vec<(String, usize)> = std::mem::take(&mut self.link_text_sources);
        for (i, h) in d.hyperlinks.iter().enumerate() {
            use designcraft_doc::{HyperlinkDest as De, HyperlinkSource as So};
            // A text destination is already written where its anchor sits.
            let dest = match &h.dest {
                // Resolved first, as the page branch resolves its page: an anchor whose text was
                // deleted names no element, so the file would hold a `Hyperlink` whose
                // `Destination` points at nothing -- import only warns and InDesign may refuse
                // the file (noted in review).
                De::Anchor(a) => d.find_anchor(*a).map(|_| anchor_self(*a)),
                De::Url(u) => Some(self.url_dest_el(&mut root, u, &h.name)),
                De::Email(e) => Some(self.url_dest_el(&mut root, &format!("mailto:{e}"), &h.name)),
                De::Page(p) => d.page(*p).map(|pg| {
                    // One element per page, as `url_dest_el` writes one per URL: every link to
                    // the same page pushed another `HyperlinkPageDestination` with the same
                    // `Self`, and IDML's references rely on a `Self` naming one element (noted
                    // in review).
                    let me = format!("HyperlinkPageDestination/p{}", pg.id.0);
                    if !has_self(&root, "HyperlinkPageDestination", &me) {
                        root.push(
                            El::new("HyperlinkPageDestination")
                                .attr("Self", &me)
                                .attr("Name", &h.name)
                                .attr("DestinationPage", uid(pg.id.0))
                                .attr("ViewSetting", "FitPage")
                                .attr("Hidden", "false"),
                        );
                    }
                    me
                }),
            };
            let Some(dest) = dest else { continue };
            // One `Hyperlink` per source element: a link whose text crosses a paragraph has one
            // source per paragraph, so each piece stays clickable.
            let sources: Vec<String> = match &h.source {
                So::Item { id } if d.item(*id).is_some() => {
                    // The link's index as well as the item's: InDesign joins one source to one
                    // destination, so three links on one frame are three source elements, and
                    // naming them all `i{item}` gave three elements one `Self` (noted in review).
                    let me = format!("HyperlinkPageItemSource/i{}k{i}", id.0);
                    root.push(
                        El::new("HyperlinkPageItemSource")
                            .attr("Self", &me)
                            .attr("Name", &h.name)
                            .attr("SourcePageItem", uid(id.0))
                            .attr("Hidden", "false"),
                    );
                    vec![me]
                }
                So::Item { .. } => vec![],
                So::Text { .. } => text_sources.iter().filter(|(_, k)| *k == i).map(|(me, _)| me.clone()).collect(),
            };
            for src in sources {
                let mut el = El::new("Hyperlink")
                    .attr("Self", self.fresh())
                    .attr("Name", &h.name)
                    .attr("Source", src)
                    .attr("Visible", "true")
                    .attr("Highlight", "None")
                    .attr("Width", "Thin")
                    .attr("BorderStyle", "Solid")
                    .attr("Hidden", "false")
                    .attr("DestinationUniqueKey", h.id & 0xFFFF_FFFF);
                let mut props = El::new("Properties");
                props.push(p("Destination", "object", &dest));
                el.push(props);
                root.push(el);
            }
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
}

pub(super) const CONTAINER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\t<rootfiles>\n\t\t<rootfile full-path=\"designmap.xml\" media-type=\"text/xml\"/>\n\t</rootfiles>\n</container>\n";

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

/// Package metadata: the creation and modification times that date variables show.
pub(super) fn metadata_xmp(d: &Document) -> String {
    use designcraft_doc::vars::format_date;
    let iso = |t: i64| format!("{}:{:02}Z", format_date(t, "yyyy-MM-ddTHH:mm"), t.rem_euclid(60));
    let modified = if d.modified > 0 { d.modified } else { d.created };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n   <xmp:CreateDate>{}</xmp:CreateDate>\n   <xmp:ModifyDate>{}</xmp:ModifyDate>\n  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n",
        iso(d.created),
        iso(modified)
    )
}

/// A `MojikumiTable` element with its spacing overrides.
fn mojikumi_table_el(t: &designcraft_doc::cjk::MojikumiTable) -> El {
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
    e
}

/// A `Layer` element with its colour property.
fn layer_el(l: &designcraft_doc::Layer) -> El {
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
    with_props(el, vec![color])
}

/// A text variable definition with its type's settings element.
fn text_variable_el(v: &designcraft_doc::vars::TextVariable) -> El {
    use designcraft_doc::vars::{Use, VarKind};
    let pref = |name: &str| El::new(name).attr("TextBefore", &v.before);
    let (ty, settings) = match &v.kind {
        VarKind::Custom { text } => ("CustomTextType", Some(pref("CustomTextVariablePreference").attr("Contents", text))),
        VarKind::LastPageNumber { section } => (
            "LastPageNumberType",
            Some(
                pref("PageNumberVariablePreference").attr("Format", "Current").attr("Scope", if *section { "SectionScope" } else { "DocumentScope" }),
            ),
        ),
        VarKind::ChapterNumber => ("ChapterNumberType", Some(pref("ChapterNumberVariablePreference").attr("Format", "Current"))),
        VarKind::FileName { extension } => {
            ("FileNameType", Some(pref("FileNameVariablePreference").attr("IncludePath", "false").attr("IncludeExtension", bool_s(*extension))))
        }
        VarKind::CreationDate { format } => ("CreationDateType", Some(pref("DateVariablePreference").attr("Format", format))),
        VarKind::ModificationDate { format } => ("ModificationDateType", Some(pref("DateVariablePreference").attr("Format", format))),
        VarKind::OutputDate { format } => ("OutputDateType", Some(pref("DateVariablePreference").attr("Format", format))),
        VarKind::RunningHeader { style, use_, character } => {
            let (ty, el, key, style_ref) = if *character {
                (
                    "MatchCharacterStyleType",
                    "MatchCharacterStylePreference",
                    "AppliedCharacterStyle",
                    names::style_self("CharacterStyle", CHAR_BUILTINS, style),
                )
            } else {
                (
                    "MatchParagraphStyleType",
                    "MatchParagraphStylePreference",
                    "AppliedParagraphStyle",
                    names::style_self("ParagraphStyle", PARA_BUILTINS, style),
                )
            };
            let strategy = if *use_ == Use::LastOnPage { "LastOnPage" } else { "FirstOnPage" };
            (
                ty,
                Some(pref(el).attr(key, style_ref).attr("SearchStrategy", strategy).attr("ChangeCase", "None").attr("DeleteEndPunctuation", "false")),
            )
        }
        VarKind::Imported { variable_type, settings_element, settings, .. } => {
            let el = (!settings_element.is_empty()).then(|| El { name: settings_element.clone(), attrs: settings.clone(), children: vec![] });
            (variable_type.as_str(), el)
        }
    };
    let mut e = El::new("TextVariable").attr("Self", format!("dTextVariablen{}", v.name)).attr("Name", &v.name).attr("VariableType", ty);
    if let Some(mut s) = settings {
        if !matches!(v.kind, VarKind::Imported { .. }) {
            s.set("TextAfter", &v.after);
        }
        e.push(s);
    }
    e
}

impl Ex<'_> {
    /// A `HyperlinkURLDestination` for `url`, written once however many links lead to it.
    fn url_dest_el(&mut self, root: &mut El, url: &str, name: &str) -> String {
        let me = format!("HyperlinkURLDestination/u{:x}", fnv(url));
        if !has_self(root, "HyperlinkURLDestination", &me) {
            root.push(
                El::new("HyperlinkURLDestination")
                    .attr("Self", &me)
                    .attr("Name", if name.is_empty() { url } else { name })
                    .attr("DestinationURL", url)
                    .attr("Hidden", "false"),
            );
        }
        me
    }
}

/// A short stable name for a string, so one URL gives one destination element.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// Whether `root` already holds an element of this name with this `Self`. IDML references name
/// one element, so a `Self` written twice is a broken file.
fn has_self(root: &El, name: &str, me: &str) -> bool {
    root.children.iter().any(|n| matches!(n, Node::El(e) if e.name == name && e.get("Self") == Some(me)))
}
