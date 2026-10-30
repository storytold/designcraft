//! The document's XML structure: `XMLElement`s in the stories (story, table, cell and inline text
//! elements) and the root in `XML/BackingStory.xml`, read into `XmlSettings::elements`.

use std::collections::{BTreeMap, HashMap};

use designcraft_doc::{XmlContent, XmlElement, XmlTag};

use super::{El, Importer};
use crate::xml::Node;

/// What the import gathers about XML elements while reading the stories.
#[derive(Default)]
pub(super) struct XmlImport {
    pub elements: BTreeMap<u64, XmlElement>,
    /// Element → its `XMLContent` (a story, table or cell Self), resolved once all are read.
    content: HashMap<u64, String>,
    /// Element Self → id: the root's children in `BackingStory.xml` are the stories' elements.
    by_self: HashMap<String, u64>,
    /// The elements being read, innermost last.
    stack: Vec<u64>,
    /// Table Self → table id, cell Self → (table id, row-major index).
    pub tables: HashMap<String, u64>,
    pub cells: HashMap<String, (u64, usize)>,
    /// Table id → whether the import added a paragraph break before and after its anchor.
    pub breaks: HashMap<u64, (bool, bool)>,
    pub tags: Vec<XmlTag>,
    pub root: String,
    pub root_children: Vec<u64>,
}

/// A tag name from `MarkupTag="XMLTag/…"` (percent-escaped like other IDML names).
fn tag_name(markup: &str) -> String {
    crate::import::package::percent_decode(markup.strip_prefix("XMLTag/").unwrap_or(markup))
}

impl<'r> Importer<'r> {
    /// Starts reading `<XMLElement>` `e`: registers it under the element being read and returns its
    /// id, and whether it is inline (marks text rather than a story, table or cell).
    pub(super) fn xml_element_begin(&mut self, e: &El) -> (u64, bool) {
        let id = self.alloc();
        let attrs = e.find_all("XMLAttribute").filter_map(|a| Some((a.get("Name")?.to_string(), a.get("Value").unwrap_or("").to_string()))).collect();
        let parent = self.xml.stack.last().copied().unwrap_or(0);
        let raw = e.get("XMLContent").filter(|c| !c.is_empty() && *c != "n");
        let inline = raw.is_none();
        if let Some(c) = raw {
            self.xml.content.insert(id, c.to_string());
        }
        if let Some(me) = e.get("Self") {
            self.xml.by_self.insert(me.to_string(), id);
        }
        // An element without content of its own lists its children; one with content finds them
        // in it on export.
        if let Some(p) = self.xml.elements.get_mut(&parent)
            && p.content == XmlContent::None
            && !self.xml.content.contains_key(&parent)
        {
            p.children.push(id);
        }
        let tag = tag_name(e.get("MarkupTag").unwrap_or("XMLTag/Element"));
        let content = if inline { XmlContent::Text } else { XmlContent::None };
        self.xml.elements.insert(id, XmlElement { tag, attrs, parent, content, children: Vec::new() });
        self.xml.stack.push(id);
        (id, inline)
    }

    pub(super) fn xml_element_end(&mut self) {
        self.xml.stack.pop();
    }

    /// The tags (`XML/Tags.xml`) and the root's children (`XML/BackingStory.xml`), then each
    /// element's content resolved to the story, table or cell it names. Run after the stories.
    pub(super) fn xml_structure(&mut self, top: &[El]) {
        for t in top.iter().filter(|e| e.local() == "XMLTag") {
            let name = t.get("Name").map(str::to_string).unwrap_or_else(|| tag_name(t.get("Self").unwrap_or("")));
            let color = t
                .find("Properties")
                .and_then(|p| p.find("TagColor"))
                .and_then(|c| crate::import::names::layer_color_in(c.text_content().trim()))
                .unwrap_or([60, 140, 220]);
            if !name.is_empty() && !self.xml.tags.iter().any(|x| x.name == name) {
                self.xml.tags.push(XmlTag { name, color });
            }
        }
        let root = top.iter().filter(|e| e.local() == "XmlStory").find_map(|s| find_root(s));
        if let Some(root) = root {
            self.xml.root = tag_name(root.get("MarkupTag").unwrap_or("XMLTag/Root"));
            for c in root.elements().filter(|c| c.local() == "XMLElement") {
                // The stories' elements were read with their content; the root only names them.
                let id = match c.get("Self").and_then(|s| self.xml.by_self.get(s)).copied() {
                    Some(id) => id,
                    None => self.xml_detached(c, 0),
                };
                if let Some(el) = self.xml.elements.get_mut(&id) {
                    el.parent = 0;
                }
                self.xml.root_children.push(id);
            }
        }
        let content = std::mem::take(&mut self.xml.content);
        for (id, c) in content {
            let target = if let Some(sid) = self.story_ids.get(&c) {
                XmlContent::Story { story: *sid }
            } else if let Some(t) = self.xml.tables.get(&c) {
                let (before, after) = self.xml.breaks.get(t).copied().unwrap_or_default();
                XmlContent::Table { table: *t, added_break_before: before, added_break_after: after }
            } else if let Some((t, i)) = self.xml.cells.get(&c) {
                XmlContent::Cell { table: *t, cell: *i }
            } else {
                // A page item or something not kept: the element stays, with what it contains.
                XmlContent::None
            };
            if let Some(el) = self.xml.elements.get_mut(&id) {
                el.content = target;
            }
        }
    }

    /// An element of the root that no story holds (its content wasn't read): kept with its own
    /// children.
    fn xml_detached(&mut self, e: &El, parent: u64) -> u64 {
        let id = self.alloc();
        let attrs = e.find_all("XMLAttribute").filter_map(|a| Some((a.get("Name")?.to_string(), a.get("Value").unwrap_or("").to_string()))).collect();
        let tag = tag_name(e.get("MarkupTag").unwrap_or("XMLTag/Element"));
        let children: Vec<u64> = e.elements().filter(|c| c.local() == "XMLElement").map(|c| self.xml_detached(c, id)).collect();
        self.xml.elements.insert(id, XmlElement { tag, attrs, parent, content: XmlContent::None, children });
        id
    }
}

/// The root `XMLElement` in the backing story.
fn find_root(e: &El) -> Option<&El> {
    for n in &e.children {
        if let Node::El(c) = n {
            if c.local() == "XMLElement" {
                return Some(c);
            }
            if let Some(r) = find_root(c) {
                return Some(r);
            }
        }
    }
    None
}
