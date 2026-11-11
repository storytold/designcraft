//! The document's XML structure (`XmlSettings::elements`) written as InDesign writes it: the root
//! and its children in `XML/BackingStory.xml`, the tags in `XML/Tags.xml`, and in the stories an
//! `XMLElement` around each tagged story's text, table and cell, and inline elements around their
//! text – inside the character range when they fit one, around the character ranges of their
//! paragraph when they span several, around paragraphs when they span those.

use std::collections::HashMap;

use designcraft_doc::{XmlContent, XmlElement};

use super::common::pkg_root;
use super::{El, Ex, NO_CHAR_ID, NO_PARA_ID, Node, document, names, uid};

/// The attribute that carries a range's XML element from `story_para` to the wrapping (never written).
const ELEMENT_MARK: &str = "\u{1}xml";

/// Where each element is written and under which `Self`.
#[derive(Default)]
pub(super) struct XmlIndex {
    /// Element → its `Self`. A story's elements share one: InDesign writes an anchored frame's story
    /// element twice, around the frame in the parent story and around the text in its own.
    selves: HashMap<u64, String>,
    /// Story id → the element written around its text.
    stories: HashMap<u64, u64>,
    tables: HashMap<u64, u64>,
    cells: HashMap<(u64, usize), u64>,
    root_self: String,
}

impl XmlIndex {
    pub(super) fn new(d: &designcraft_doc::Document) -> Self {
        let xml = &d.xml;
        let mut ix = XmlIndex { root_self: "di2".into(), ..Default::default() };
        if xml.elements.is_empty() {
            return ix;
        }
        // A story's element: the one with elements under it (the one read with the story's text).
        for (&id, e) in &xml.elements {
            let has_children = || xml.elements.values().any(|c| c.parent == id);
            match e.content {
                XmlContent::Story { story } if d.stories.contains_key(&story) => {
                    let keep = ix.stories.get(&story.0).is_some_and(|k| xml.elements.values().any(|c| c.parent == *k));
                    if !keep && (has_children() || !ix.stories.contains_key(&story.0)) {
                        ix.stories.insert(story.0, id);
                    }
                }
                XmlContent::Table { table, .. } => {
                    ix.tables.insert(table, id);
                }
                XmlContent::Cell { table, cell } => {
                    ix.cells.insert((table, cell), id);
                }
                _ => {}
            }
        }
        // Selves in InDesign's form: the parent's, then "i" and a running hex number.
        let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
        for (&id, e) in &xml.elements {
            children.entry(e.parent).or_default().push(id);
        }
        for e in xml.elements.values() {
            for &c in &e.children {
                children.entry(c).or_default();
            }
        }
        let mut next = 0u64;
        let mut order: Vec<(u64, String)> = xml.root_children.iter().map(|&c| (c, ix.root_self.clone())).collect();
        // Elements whose parent isn't written (a duplicate story element): named under the root.
        for (&id, e) in &xml.elements {
            if e.parent == 0 && !xml.root_children.contains(&id) {
                order.push((id, ix.root_self.clone()));
            }
        }
        order.reverse();
        while let Some((id, parent_self)) = order.pop() {
            if ix.selves.contains_key(&id) {
                continue;
            }
            let me = match xml.elements.get(&id).map(|e| e.content) {
                Some(XmlContent::Story { story }) => match ix.stories.get(&story.0).and_then(|k| ix.selves.get(k)) {
                    Some(shared) => shared.clone(),
                    None => {
                        next += 1;
                        format!("{parent_self}i{next:x}")
                    }
                },
                _ => {
                    next += 1;
                    format!("{parent_self}i{next:x}")
                }
            };
            if let Some(XmlContent::Story { story }) = xml.elements.get(&id).map(|e| e.content) {
                for (&other, e) in &xml.elements {
                    if e.content == (XmlContent::Story { story }) {
                        ix.selves.entry(other).or_insert_with(|| me.clone());
                    }
                }
            }
            ix.selves.insert(id, me.clone());
            let mut kids: Vec<u64> = children.get(&id).cloned().unwrap_or_default();
            if let Some(e) = xml.elements.get(&id) {
                let more: Vec<u64> = e.children.iter().copied().filter(|c| !kids.contains(c)).collect();
                kids.extend(more);
            }
            for &k in kids.iter().rev() {
                order.push((k, me.clone()));
            }
        }
        ix
    }
}

impl<'a> Ex<'a> {
    fn xml_el(&self, id: u64, children: Vec<Node>) -> El {
        let e = &self.d.xml.elements[&id];
        let me = self.xml.selves.get(&id).cloned().unwrap_or_else(|| format!("{}i{id:x}", self.xml.root_self));
        let mut el = El::new("XMLElement").attr("Self", &me).attr("MarkupTag", markup_tag(&e.tag));
        match e.content {
            XmlContent::Story { story } => el.set("XMLContent", uid(story.0)),
            XmlContent::Table { table, .. } => el.set("XMLContent", uid(table)),
            XmlContent::Cell { table, cell } => el.set("XMLContent", format!("{}i{cell}", uid(table))),
            XmlContent::None | XmlContent::Text => {}
        }
        el.children = children;
        attributes(&mut el, &me, e);
        el
    }

    /// The element around a whole story's text, if the story has one.
    pub(super) fn story_xml(&self, story: u64, paras: Vec<El>) -> Vec<El> {
        match self.xml.stories.get(&story) {
            Some(&id) => vec![self.xml_el(id, paras.into_iter().map(Node::El).collect())],
            None => paras,
        }
    }

    /// The element around an anchored text frame whose story is tagged inside the parent story (one
    /// the root lists is written there only, or InDesign would find it twice).
    pub(super) fn anchored_xml(&self, item: &designcraft_doc::Item, el: El) -> El {
        let designcraft_doc::Content::Text(t) = &item.content else { return el };
        let xml = &self.d.xml;
        let inside =
            |id: &u64| xml.elements.iter().any(|(e, x)| x.content == xml.elements[id].content && x.parent != 0 && !xml.root_children.contains(e));
        match self.xml.stories.get(&t.story.0).filter(|id| inside(id) && !xml.root_children.contains(id)) {
            Some(&id) => {
                let mut x = self.xml_el(id, vec![Node::El(el)]);
                x.set("NoTextMarker", "true");
                strip_attributes(&mut x);
                x
            }
            None => el,
        }
    }

    pub(super) fn table_xml(&self, table: u64, el: El) -> El {
        match self.xml.tables.get(&table) {
            Some(&id) => {
                let mut x = self.xml_el(id, vec![Node::El(el)]);
                x.set("NoTextMarker", "true");
                x
            }
            None => el,
        }
    }

    pub(super) fn cell_xml(&self, table: u64, cell: usize, paras: Vec<El>) -> Vec<El> {
        match self.xml.cells.get(&(table, cell)) {
            Some(&id) => vec![self.xml_el(id, paras.into_iter().map(Node::El).collect())],
            None => paras,
        }
    }

    /// Paragraph ranges whose character ranges carry their inline element (from `story_para`), with
    /// those elements put around their text and the marks removed.
    pub(super) fn wrap_inline_xml(&self, paras: Vec<El>) -> Vec<El> {
        let mut leaves = Vec::new();
        let mut templates = Vec::new();
        for (p, mut psr) in paras.into_iter().enumerate() {
            let mut keep = Vec::new();
            for n in std::mem::take(&mut psr.children) {
                match n {
                    Node::El(c) if c.name == "Properties" => keep.push(Node::El(c)),
                    Node::El(mut c) => {
                        // A range carries its element; ranges wrapped in something of their own (a
                        // hyperlink source) give the wrapper the element of the first one.
                        let id = match c.get(ELEMENT_MARK).and_then(|v| v.parse::<u64>().ok()) {
                            Some(id) => Some(id),
                            None => take_marks(&mut c),
                        };
                        c.attrs.retain(|(k, _)| k != ELEMENT_MARK);
                        let chain = id.map(|id| self.inline_chain(id)).unwrap_or_default();
                        leaves.push(Leaf { para: p, chain, node: c });
                    }
                    // Anything else of the paragraph stays at its place: before its text.
                    other => leaves.push(Leaf { para: p, chain: Vec::new(), node: wrap_node(other) }),
                }
            }
            psr.children = keep;
            templates.push(psr);
        }
        if leaves.iter().all(|l| l.chain.is_empty()) {
            return self.plain_paras(templates, leaves);
        }
        self.emit_paras(&templates, leaves, 0)
            .into_iter()
            .filter_map(|n| match n {
                Node::El(e) => Some(e),
                _ => None,
            })
            .collect()
    }

    /// Without inline elements: each paragraph's ranges back into it.
    fn plain_paras(&self, mut templates: Vec<El>, leaves: Vec<Leaf>) -> Vec<El> {
        for l in leaves {
            templates[l.para].children.push(unwrap_node(l.node));
        }
        templates
    }

    /// The inline elements around a range's text, outermost first (up to the story's or cell's).
    fn inline_chain(&self, id: u64) -> Vec<u64> {
        let mut chain = Vec::new();
        let mut cur = id;
        while let Some(e) = self.d.xml.elements.get(&cur) {
            if e.content != XmlContent::Text || chain.len() > 64 {
                break;
            }
            chain.push(cur);
            cur = e.parent;
        }
        chain.reverse();
        chain
    }

    /// Story level: an element spanning paragraphs goes around them; the rest into its paragraph's range.
    fn emit_paras(&self, templates: &[El], leaves: Vec<Leaf>, depth: usize) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        let mut open: Option<(usize, El)> = None;
        let flush = |open: &mut Option<(usize, El)>, out: &mut Vec<Node>| {
            if let Some((_, psr)) = open.take() {
                out.push(Node::El(psr));
            }
        };
        for group in groups(leaves, depth) {
            let first_para = group[0].para;
            let one_para = group.iter().all(|l| l.para == first_para);
            match group[0].chain.get(depth).copied() {
                Some(e) if !one_para => {
                    flush(&mut open, &mut out);
                    out.push(Node::El(self.xml_el(e, self.emit_paras(templates, group, depth + 1))));
                }
                _ => {
                    // By paragraph, each into its range (continued while the paragraph goes on).
                    let mut rest = group;
                    while !rest.is_empty() {
                        let p = rest[0].para;
                        let n = rest.iter().take_while(|l| l.para == p).count();
                        let tail = rest.split_off(n);
                        let nodes = self.emit_ranges(rest, depth);
                        rest = tail;
                        if open.as_ref().is_none_or(|(op, _)| *op != p) {
                            flush(&mut open, &mut out);
                            open = Some((p, templates[p].clone()));
                        }
                        if let Some((_, psr)) = open.as_mut() {
                            psr.children.extend(nodes);
                        }
                    }
                }
            }
        }
        flush(&mut open, &mut out);
        out
    }

    /// Paragraph level (all of one paragraph): an element in one character range goes inside it,
    /// one spanning several around them.
    fn emit_ranges(&self, leaves: Vec<Leaf>, depth: usize) -> Vec<Node> {
        let mut out = Vec::new();
        for group in groups(leaves, depth) {
            match group[0].chain.get(depth).copied() {
                None => out.extend(group.into_iter().map(|l| unwrap_node(l.node))),
                Some(e) if group.len() == 1 && group[0].chain.len() == depth + 1 && group[0].node.name == "CharacterStyleRange" => {
                    let mut csr = group.into_iter().next().map(|l| l.node).unwrap_or_else(|| El::new("CharacterStyleRange"));
                    let (props, text): (Vec<Node>, Vec<Node>) =
                        std::mem::take(&mut csr.children).into_iter().partition(|n| matches!(n, Node::El(p) if p.name == "Properties"));
                    csr.children = props;
                    csr.children.push(Node::El(self.xml_el(e, text)));
                    out.push(Node::El(csr));
                }
                Some(e) => out.push(Node::El(self.xml_el(e, self.emit_ranges(group, depth + 1)))),
            }
        }
        out
    }

    /// `XML/Tags.xml`, or none for a document without XML.
    pub(super) fn tags_part(&self) -> Option<Vec<u8>> {
        let xml = &self.d.xml;
        if xml.tags.is_empty() && xml.elements.is_empty() {
            return None;
        }
        let mut root = pkg_root("Tags");
        let mut seen = Vec::new();
        let root_tag = if xml.root.is_empty() { "Root" } else { xml.root.as_str() };
        let named = std::iter::once((root_tag.to_string(), [60, 140, 220]))
            .chain(xml.tags.iter().map(|t| (t.name.clone(), t.color)))
            .chain(xml.elements.values().map(|e| (e.tag.clone(), [60, 140, 220])));
        for (name, color) in named {
            if seen.contains(&name) {
                continue;
            }
            // The tag list's color wins over the default of a tag known only from an element.
            let color = xml.tags.iter().find(|t| t.name == name).map_or(color, |t| t.color);
            let mut tag = El::new("XMLTag").attr("Self", markup_tag(&name)).attr("Name", &name);
            let mut props = El::new("Properties");
            props.push(tag_color(color));
            tag.push(props);
            root.push(tag);
            seen.push(name);
        }
        Some(document(&root))
    }

    /// `XML/BackingStory.xml`: the root element with its children, as InDesign writes it (the root
    /// between its invisible U+FEFF marks); none for a document without XML.
    pub(super) fn backing_story_part(&mut self) -> Option<Vec<u8>> {
        let xml = &self.d.xml;
        if xml.elements.is_empty() && xml.root.is_empty() {
            return None;
        }
        let mut root = El::new("XMLElement")
            .attr("Self", &self.xml.root_self)
            .attr("MarkupTag", markup_tag(if xml.root.is_empty() { "Root" } else { &xml.root }));
        for &c in &xml.root_children {
            root.push(self.root_child(c));
        }
        let mut csr = El::new("CharacterStyleRange").attr("AppliedCharacterStyle", NO_CHAR_ID);
        csr.push(El::new("Content").text("\u{feff}\u{feff}"));
        csr.push(root);
        csr.push(El::new("Content").text("\u{feff}\u{feff}\u{feff}"));
        let mut psr = El::new("ParagraphStyleRange").attr("AppliedParagraphStyle", NO_PARA_ID);
        psr.push(csr);
        let story = El::new("XmlStory")
            .attr("Self", self.fresh())
            .attr("AppliedTOCStyle", "n")
            .attr("UserText", "true")
            .attr("IsEndnoteStory", "false")
            .attr("TrackChanges", "false")
            .attr("StoryTitle", "$ID/")
            .attr("AppliedNamedGrid", "n")
            .child(psr);
        Some(document(&pkg_root("BackingStory").child(story)))
    }

    /// A child of the root: a reference to the story it marks up (written in full in the story), or
    /// an element kept without content, with its children.
    fn root_child(&self, id: u64) -> El {
        let e = &self.d.xml.elements[&id];
        if matches!(e.content, XmlContent::Story { .. } | XmlContent::Table { .. } | XmlContent::Cell { .. }) {
            let mut el = self.xml_el(id, Vec::new());
            strip_attributes(&mut el);
            return el;
        }
        let kids = e.children.iter().filter(|c| self.d.xml.elements.contains_key(c)).map(|&c| Node::El(self.root_child(c))).collect();
        self.xml_el(id, kids)
    }
}

/// A character range (or another node of a paragraph range) with the inline elements around its text.
struct Leaf {
    para: usize,
    chain: Vec<u64>,
    node: El,
}

/// Removes the marks of the ranges inside `el`; the first one's element, if any.
fn take_marks(el: &mut El) -> Option<u64> {
    let mut first = None;
    fn walk(e: &mut El, first: &mut Option<u64>) {
        for n in &mut e.children {
            if let Node::El(c) = n {
                if let Some(v) = c.get(ELEMENT_MARK).and_then(|v| v.parse::<u64>().ok()) {
                    first.get_or_insert(v);
                }
                c.attrs.retain(|(k, _)| k != ELEMENT_MARK);
                walk(c, first);
            }
        }
    }
    walk(el, &mut first);
    first
}

/// Consecutive leaves with the same element at `depth` (or none).
fn groups(leaves: Vec<Leaf>, depth: usize) -> Vec<Vec<Leaf>> {
    let mut out: Vec<Vec<Leaf>> = Vec::new();
    for l in leaves {
        let key = l.chain.get(depth).copied();
        match out.last_mut() {
            Some(g) if g[0].chain.get(depth).copied() == key => g.push(l),
            _ => out.push(vec![l]),
        }
    }
    out
}

/// Nodes of a paragraph range that aren't elements (text, instructions) travel as a pseudo element.
fn wrap_node(n: Node) -> El {
    match n {
        Node::El(e) => e,
        other => {
            let mut e = El::new("\u{1}node");
            e.children.push(other);
            e
        }
    }
}

fn unwrap_node(e: El) -> Node {
    if e.name == "\u{1}node" { e.children.into_iter().next().unwrap_or(Node::Text(String::new())) } else { Node::El(e) }
}

/// `XMLAttribute`s after the content, as InDesign writes them.
fn attributes(el: &mut El, me: &str, e: &XmlElement) {
    for (k, v) in &e.attrs {
        el.push(El::new("XMLAttribute").attr("Self", format!("{me}XMLAttributen{k}")).attr("Name", k).attr("Value", v));
    }
}

/// The reference copy of an element (in the root, around an anchored frame) carries no attributes:
/// InDesign keeps them with the element's text.
fn strip_attributes(el: &mut El) {
    el.children.retain(|n| !matches!(n, Node::El(c) if c.name == "XMLAttribute"));
}

fn markup_tag(name: &str) -> String {
    format!("XMLTag/{}", names::escape_id(name))
}

fn tag_color(c: [u8; 3]) -> El {
    match names::layer_color_out(c) {
        Some(name) => El::new("TagColor").attr("type", "enumeration").text(name),
        None => {
            let mut el = El::new("TagColor").attr("type", "list");
            for v in c {
                el.push(El::new("ListItem").attr("type", "double").text(v.to_string()));
            }
            el
        }
    }
}

/// Marks a range with the inline element its text belongs to (read back by `wrap_inline_xml`).
pub(super) fn mark_element(el: &mut El, element: Option<u64>) {
    if let Some(id) = element {
        el.set(ELEMENT_MARK, id);
    }
}
