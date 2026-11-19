use super::common::{uid, xref_format_self};
use super::{CHAR_BUILTINS, Document, El, Ex, HashSet, PARA_BUILTINS, names, num};

impl<'a> Ex<'a> {
    // ---------- designmap ----------

    pub(super) fn designmap(&mut self, masters: &[(String, Vec<u8>)], spreads: &[(String, Vec<u8>)], stories: &[(String, Vec<u8>)]) -> Vec<u8> {
        let d = self.d;
        let story_list: Vec<String> = d.stories.keys().map(|k| uid(k.0)).collect();
        let [x, y] = d.zero_point();
        let mut root = crate::xml::from_craft(craft_idml::designmap_root(
            "d",
            &story_list.join(" "),
            &format!("{}.indd", d.title),
            &format!("{} {}", num(x), num(y)),
            &d.layers.first().map(|l| uid(l.id.0)).unwrap_or_else(|| "n".into()),
        ));
        for lang in craft_idml::model::default_languages() {
            root.push(crate::xml::from_craft(lang));
        }
        let mut sets = std::collections::BTreeMap::new();
        for a in d.styles.paragraph.iter().map(|p| &p.para).chain(d.stories.values().flat_map(|s| s.paras.iter().map(|p| &p.para))) {
            if let Some(Some(k)) = &a.kinsoku
                && !k.name.is_empty()
            {
                sets.insert(&k.name, k);
            }
        }
        for k in sets.values() {
            root.push(crate::xml::from_craft(
                craft_idml::model::KinsokuTableValues {
                    name: k.name.clone(),
                    cant_begin_line_chars: k.no_start.clone(),
                    cant_end_line_chars: k.no_end.clone(),
                    cant_be_separated_chars: k.inseparable.clone(),
                    hanging_punctuation_chars: k.hanging.clone(),
                }
                .to_el(),
            ));
        }
        for t in &d.styles.mojikumi_tables {
            root.push(mojikumi_table_el(t));
        }
        root.push(crate::xml::from_craft(craft_idml::designmap_include("Graphic", "Resources/Graphic.xml")));
        root.push(crate::xml::from_craft(craft_idml::designmap_include("Fonts", "Resources/Fonts.xml")));
        root.push(crate::xml::from_craft(craft_idml::designmap_include("Styles", "Resources/Styles.xml")));
        root.push(crate::xml::from_craft(craft_idml::model::NumberingListValues::default_list().to_el()));
        root.push(crate::xml::from_craft(craft_idml::designmap_include("Preferences", "Resources/Preferences.xml")));
        for c in &d.conditions {
            root.push(crate::xml::from_craft(
                craft_idml::model::ConditionValues { self_id: None, name: Some(c.name.clone()), indicator_rgb: Some(c.color), visible: c.visible }
                    .to_el(),
            ));
        }
        // Variables InDesign's cross-reference page/chapter blocks rely on.
        for tv in craft_idml::model::default_xref_text_variables() {
            root.push(crate::xml::from_craft(tv));
        }
        for v in &d.text_variables {
            root.push(text_variable_el(v));
        }
        // IDML lists layers back to front.
        for l in d.layers.iter().rev() {
            root.push(layer_el(l));
        }
        for (name, _) in masters {
            root.push(crate::xml::from_craft(craft_idml::designmap_include("MasterSpread", name)));
        }
        for (name, _) in spreads {
            root.push(crate::xml::from_craft(craft_idml::designmap_include("Spread", name)));
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
                    layout: None,
                },
            );
        }
        let total = d.page_count();
        // Alternate layouts: every section names the layout it is in (InDesign's AlternateLayout).
        let layouts = d.layouts();
        let alternates = layouts.len() > 1 || secs.iter().any(|s| s.layout.is_some());
        for (i, s) in secs.iter().enumerate() {
            if s.start >= total {
                continue;
            }
            let end = secs.get(i + 1).map(|n| n.start).unwrap_or(total).min(total);
            let Some(page) = d.page(s.start) else { continue };
            let alternate_layout =
                alternates.then(|| layouts.iter().rfind(|l| l.start <= s.start).map(|l| (l.name.clone(), l.end.saturating_sub(l.start)))).flatten();
            root.push(crate::xml::from_craft(
                craft_idml::model::SectionValues {
                    self_id: self.fresh(),
                    length: end.saturating_sub(s.start).max(1),
                    name: s.prefix.clone(),
                    continue_numbering: s.start_number.is_none(),
                    include_section_prefix: s.include_prefix,
                    page_number_start: s.start_number,
                    marker: s.marker.clone(),
                    page_start: uid(page.id.0),
                    section_prefix: s.prefix.clone(),
                    alternate_layout,
                    page_number_style: names::number_style_out(s.style).into(),
                }
                .to_el(),
            ));
        }
        if !self.index_topics.is_empty() || !self.index_xrefs.is_empty() {
            root.push(self.index_el());
        }
        // Formats before the stories whose sources apply them.
        for (i, f) in d.xref_formats.iter().enumerate() {
            root.push(xref_format_el(i, f));
        }
        for (name, _) in stories {
            root.push(crate::xml::from_craft(craft_idml::designmap_include("Story", name)));
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
                De::Anchor(a) => d.find_anchor(*a).map(|_| craft_idml::model::StoryAnchor::destination_self(*a)),
                De::Url(u) => Some(self.url_dest_el(&mut root, u, &h.name)),
                De::Email(e) => Some(self.url_dest_el(&mut root, &format!("mailto:{e}"), &h.name)),
                De::Page(p) => d.page(*p).map(|pg| {
                    // One element per page, as `url_dest_el` writes one per URL: every link to
                    // the same page pushed another `HyperlinkPageDestination` with the same
                    // `Self`, and IDML's references rely on a `Self` naming one element (noted
                    // in review).
                    let me = format!("HyperlinkPageDestination/p{}", pg.id.0);
                    if !has_self(&root, "HyperlinkPageDestination", &me) {
                        root.push(crate::xml::from_craft(craft_idml::model::hyperlink_page_destination_el(&me, &h.name, &uid(pg.id.0))));
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
                    root.push(crate::xml::from_craft(craft_idml::model::hyperlink_page_item_source_el(&me, &h.name, &uid(id.0))));
                    vec![me]
                }
                So::Item { .. } => vec![],
                So::Text { .. } => text_sources.iter().filter(|(_, k)| *k == i).map(|(me, _)| me.clone()).collect(),
            };
            for src in sources {
                root.push(crate::xml::from_craft(craft_idml::model::hyperlink_el(&self.fresh(), &h.name, &src, &dest, true, h.id)));
            }
        }
        for (src, target) in self.xref_sources.clone() {
            root.push(crate::xml::from_craft(craft_idml::model::hyperlink_el(
                &self.fresh(),
                "Cross-Reference",
                &src,
                &craft_idml::model::StoryAnchor::destination_self(target),
                false,
                target,
            )));
        }
        // Root colour group: the Swatches panel order.
        let mut seen = HashSet::new();
        let mut swatch_ids = Vec::new();
        for s in &d.swatches {
            let id = self.sw(&s.name);
            if seen.insert(id.clone()) {
                swatch_ids.push(id);
            }
        }
        let prefix = format!("u{:x}", self.next + 1);
        self.next += 1;
        root.push(crate::xml::from_craft(craft_idml::model::root_color_group_el(&swatch_ids, &prefix)));
        craft_idml::designmap_xml(&crate::xml::to_craft(&root))
    }
}

pub(super) const CONTAINER: &str = craft_idml::CONTAINER_XML;

/// A cross-reference format as InDesign building blocks.
fn xref_format_el(i: usize, f: &designcraft_doc::XrefFormat) -> El {
    let me = xref_format_self(i);
    crate::xml::from_craft(craft_idml::model::CrossReferenceFormatValues::to_el(&me, &f.name, &f.definition))
}
/// Package metadata: the creation and modification times that date variables show.
pub(super) fn metadata_xmp(d: &Document) -> String {
    use designcraft_doc::vars::format_date;
    let iso = |t: i64| format!("{}:{:02}Z", format_date(t, "yyyy-MM-ddTHH:mm"), t.rem_euclid(60));
    let modified = if d.modified > 0 { d.modified } else { d.created };
    craft_idml::xmp_metadata(&iso(d.created), &iso(modified))
}

/// A `MojikumiTable` element with its spacing overrides.
fn mojikumi_table_el(t: &designcraft_doc::cjk::MojikumiTable) -> El {
    let overrides = t
        .overrides
        .iter()
        .map(|r| craft_idml::model::MojikumiRow {
            target_class: r.target_class,
            side_class: r.side_class,
            after: r.after,
            minimum: r.minimum,
            desired: r.desired,
            maximum: r.maximum,
            priority: r.priority,
            does_not_float: r.does_not_float,
        })
        .collect();
    crate::xml::from_craft(craft_idml::model::MojikumiTableValues { name: t.name.clone(), based_on: t.based_on.clone(), overrides }.to_el())
}

/// A `Layer` element with its colour property.
fn layer_el(l: &designcraft_doc::Layer) -> El {
    let color = match names::layer_color_out(l.color) {
        Some(n) => craft_idml::model::LayerColorValue::Named(n.into()),
        None => craft_idml::model::LayerColorValue::Rgb(l.color.map(f64::from)),
    };
    crate::xml::from_craft(
        craft_idml::model::LayerValues {
            self_id: uid(l.id.0),
            name: l.name.clone(),
            visible: l.visible,
            locked: l.locked,
            ignore_wrap: l.suppress_wrap_when_hidden,
            show_guides: l.show_guides,
            lock_guides: l.lock_guides,
            printable: l.printable,
            color,
        }
        .to_el(),
    )
}

/// A text variable definition with its type's settings element.
fn text_variable_el(v: &designcraft_doc::vars::TextVariable) -> El {
    use craft_idml::model::{TextDateKind, TextVariableKind, TextVariableValues};
    use designcraft_doc::vars::{Use, VarKind};
    let kind = match &v.kind {
        VarKind::Custom { text } => TextVariableKind::CustomText { text: text.clone() },
        VarKind::LastPageNumber { section } => TextVariableKind::LastPageNumber { section: *section },
        VarKind::ChapterNumber => TextVariableKind::ChapterNumber,
        VarKind::FileName { extension } => TextVariableKind::FileName { extension: *extension },
        VarKind::CreationDate { format } => TextVariableKind::Date { kind: TextDateKind::Creation, format: format.clone() },
        VarKind::ModificationDate { format } => TextVariableKind::Date { kind: TextDateKind::Modification, format: format.clone() },
        VarKind::OutputDate { format } => TextVariableKind::Date { kind: TextDateKind::Output, format: format.clone() },
        VarKind::RunningHeader { style, use_, character } => {
            let style_ref = if *character {
                names::style_self("CharacterStyle", CHAR_BUILTINS, style)
            } else {
                names::style_self("ParagraphStyle", PARA_BUILTINS, style)
            };
            TextVariableKind::RunningHeader { style: style_ref, character: *character, last_on_page: *use_ == Use::LastOnPage }
        }
        VarKind::Imported { variable_type, settings_element, settings, .. } => TextVariableKind::Imported {
            variable_type: variable_type.clone(),
            settings_element: settings_element.clone(),
            settings: settings.clone(),
        },
    };
    crate::xml::from_craft(TextVariableValues { self_id: None, name: v.name.clone(), before: v.before.clone(), after: v.after.clone(), kind }.to_el())
}

impl Ex<'_> {
    /// A `HyperlinkURLDestination` for `url`, written once however many links lead to it.
    fn url_dest_el(&mut self, root: &mut El, url: &str, name: &str) -> String {
        let me = format!("HyperlinkURLDestination/u{:x}", fnv(url));
        if !has_self(root, "HyperlinkURLDestination", &me) {
            root.push(crate::xml::from_craft(craft_idml::model::hyperlink_url_destination_el(&me, if name.is_empty() { url } else { name }, url)));
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
    root.children.iter().any(|n| matches!(n, crate::xml::Node::El(e) if e.name == name && e.get("Self") == Some(me)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::Node;

    fn custom_text(definition: &str) -> String {
        let f = designcraft_doc::XrefFormat { name: "x".into(), definition: definition.into() };
        xref_format_el(0, &f)
            .children
            .iter()
            .filter_map(|n| match n {
                Node::El(e) if e.get("BlockType") == Some("CustomStringBuildingBlock") => e.get("CustomText").map(str::to_string),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn xref_format_unclosed_angle_bracket_keeps_text_once() {
        assert_eq!(custom_text("a < b"), "a < b");
    }

    #[test]
    fn xref_format_well_formed_keeps_custom_text() {
        assert_eq!(custom_text("p <pageNum /> of <chapNum />"), "p  of ");
    }
}
