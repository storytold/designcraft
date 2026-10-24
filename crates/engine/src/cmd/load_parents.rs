//! Load Parent Pages: the parent spreads of another document (`.designcraft` or IDML) come into
//! this one with their pages, objects and guides, and with what their objects use.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use designcraft_doc::{
    Content, Document, Item, ItemId, Layer, LayerId, NO_CHAR_STYLE, NO_OBJECT_STYLE, NO_PARA_STYLE, ObjectStyle, Page, PageId, ParentInfo, Spread,
    SpreadId, SpreadRef, Story, StoryId, Stroke, StrokeStyleDef, StrokeType, Styles, vars,
};
use designcraft_geom::Vec2;
use serde_json::{Value, json};

use super::load_styles::{Conflict, merge, missing_lists, missing_swatches, source};
use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

const ID: &str = "layout.parents.load";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "layout.parents.load",
            "Load Parent Pages…",
            [],
            None,
            "{path | base64, name?: the file's name (`.idml` reads IDML), conflicts?: rename|replace} — every parent of another document (.designcraft or .idml) comes in with its pages, objects, guides and based-on parent, and with the paragraph, character and object styles, swatches, lists, stroke styles, text variables and layers its objects use that this document lacks (this document's own of the same names stay). A parent whose name this document has gets the next free prefix (rename, the default) or takes the place of this document's (replace: the pages that use it show the loaded one, and their overrides of its objects become ordinary objects); one whose prefix alone is taken gets the next free prefix. One undo step → {parents: [label], renamed: {label: new label}, replaced: [label], sizesDiffer: the documents' page sizes differ}",
            has_doc,
            load
        ),
        cmd!(query "layout.parents.load.list", "List Parents in File", [], None,
            "{path | base64, name?} → {parents: [{label, conflict}], sizesDiffer} — the parents `layout.parents.load` would bring; `conflict`: this document has a parent of that name; `sizesDiffer`: the documents' page sizes differ",
            has_doc, list),
    ]
}

fn load(s: &mut Session, p: &Value) -> Result<Value> {
    let replace = match p.get("conflicts") {
        None | Some(Value::Null) => false,
        Some(v) => match v.as_str() {
            Some("rename") => false,
            Some("replace") => true,
            _ => return Err(bad(ID, "`conflicts` must be rename or replace")),
        },
    };
    let mut src = source(p, ID, true)?;
    if src.parents.is_empty() {
        return Err(bad(ID, "the file has no parent pages"));
    }
    s.edit(|d, sel| {
        let report = load_into(d, &mut src, replace)?;
        if replace {
            // The replaced parents' objects are gone.
            *sel = Default::default();
        }
        Ok(report)
    })
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let src = source(p, "layout.parents.load.list", false)?;
    let d = &s.doc()?.doc;
    let mine: HashSet<String> = d.parents.iter().filter_map(|sp| sp.parent.as_ref().map(ParentInfo::label)).collect();
    let parents: Vec<Value> =
        src.parents.iter().filter_map(|sp| sp.parent.as_ref()).map(|i| json!({"label": i.label(), "conflict": mine.contains(&i.label())})).collect();
    Ok(json!({"parents": parents, "sizesDiffer": sizes_differ(d, &src)}))
}

/// The documents' page sizes (Document Setup) differ.
fn sizes_differ(a: &Document, b: &Document) -> bool {
    (a.settings.page_width - b.settings.page_width).abs() > 0.01 || (a.settings.page_height - b.settings.page_height).abs() > 0.01
}

/// Load every parent of `src` into `d`, with the layers, text variables, styles, swatches, lists
/// and stroke styles its objects use that `d` lacks.
fn load_into(d: &mut Document, src: &mut Document, replace: bool) -> Result<Value> {
    let sizes = sizes_differ(d, src);
    let mut stories = BTreeSet::new();
    for sp in &src.parents {
        for it in &sp.items {
            it.walk(&mut |i| {
                if let Content::Text(tf) = &i.content {
                    stories.insert(tf.story);
                }
            });
        }
    }
    remap_layers(d, src);
    let variables = remap_variables(d, src, &stories)?;

    // The styles the text and the objects use, with the styles those name. The document's own
    // definitions of the same names stay, as when pasting.
    let objects = object_styles(d, src);
    let (mut para, mut chars, mut lists) = (BTreeSet::new(), BTreeSet::new(), HashSet::new());
    for st in stories.iter().filter_map(|id| src.story(*id)) {
        texts(st, &mut |t| {
            for f in &t.paras {
                para.insert(f.style.clone());
                lists.extend(f.para.list_name.clone());
            }
            for r in &t.chars {
                chars.insert(r.format.style.clone());
            }
        });
    }
    for v in &variables {
        if let vars::VarKind::RunningHeader { style, character, .. } = &v.kind {
            (if *character { &mut chars } else { &mut para }).insert(style.clone());
        }
    }
    para.extend(objects.iter().filter_map(|o| o.paragraph_style.clone()));
    let para: Vec<String> = para.into_iter().filter(|n| n != NO_PARA_STYLE && src.styles.para(n).is_some()).collect();
    let chars: Vec<String> = chars.into_iter().filter(|n| n != NO_CHAR_STYLE && src.styles.char_style(n).is_some()).collect();
    let merged = merge(d, src, Some(para.as_slice()), Some(chars.as_slice()), Conflict::UseExisting)?;

    // The swatches all of that names.
    let added = Styles {
        paragraph: merged.para.added.iter().filter_map(|n| src.styles.para(n).cloned()).collect(),
        character: merged.chars.added.iter().filter_map(|n| src.styles.char_style(n).cloned()).collect(),
        object: objects.clone(),
        cell: Vec::new(),
        table: Vec::new(),
        ..Styles::default()
    };
    let items = src.parents.iter().flat_map(|sp| sp.items.iter().map(|it| it.as_ref()));
    let mut swatches: HashSet<String> =
        designcraft_doc::swatches_named_by(items, stories.iter().filter_map(|id| src.story(*id)), &added).into_iter().collect();
    swatches.extend(merged.swatches);
    let swatches = missing_swatches(d, src, swatches);
    lists.extend(merged.lists);
    let lists = missing_lists(d, src, &lists);
    let strokes = stroke_styles(d, src, &objects);
    let mut styles = merged.styles;
    styles.object.extend(objects);
    *d.styles_mut() = styles;
    d.swatches.extend(swatches);
    d.settings.lists.extend(lists);
    d.stroke_styles.extend(strokes);

    // Prefixes: a parent keeps its own unless this document's parents or a loaded one before it
    // have it. One of the same name gets a free prefix too, or replaces the document's.
    let mine: Vec<(SpreadId, ParentInfo)> = d.parents.iter().filter_map(|sp| sp.parent.clone().map(|i| (sp.id, i))).collect();
    let theirs: Vec<ParentInfo> = src
        .parents
        .iter()
        .map(|sp| sp.parent.clone().unwrap_or_else(|| ParentInfo { prefix: String::new(), name: "Parent".into(), based_on: None }))
        .collect();
    let mut taken: HashSet<String> = mine.iter().map(|(_, m)| m.prefix.clone()).collect();
    let mut target: Vec<Option<SpreadId>> = vec![None; theirs.len()];
    let mut kept: Vec<Option<String>> = vec![None; theirs.len()];
    for (i, t) in theirs.iter().enumerate() {
        let same = mine.iter().find(|(_, m)| m.prefix == t.prefix && m.name == t.name).map(|(id, _)| *id);
        if let Some(id) = same.filter(|id| replace && !target.contains(&Some(*id))) {
            target[i] = Some(id);
            kept[i] = Some(t.prefix.clone());
        } else if same.is_none() && !t.prefix.is_empty() && taken.insert(t.prefix.clone()) {
            kept[i] = Some(t.prefix.clone());
        }
    }
    let prefixes: Vec<String> = kept
        .into_iter()
        .map(|k| {
            k.unwrap_or_else(|| {
                let p = free_prefix(&taken);
                taken.insert(p.clone());
                p
            })
        })
        .collect();

    // The parents, with new ids for them, their pages and their objects.
    let ids: HashMap<SpreadId, SpreadId> = src.parents.iter().map(|sp| (sp.id, SpreadId(d.alloc()))).collect();
    let first = d.parents.len();
    let mut moved: HashMap<ItemId, ItemId> = HashMap::new();
    let (mut labels, mut renamed) = (Vec::new(), BTreeMap::new());
    for (i, sp) in src.parents.iter().enumerate() {
        let t = &theirs[i];
        let info = ParentInfo { prefix: prefixes[i].clone(), name: t.name.clone(), based_on: t.based_on.and_then(|b| ids.get(&b).copied()) };
        if info.prefix != t.prefix {
            renamed.insert(t.label(), info.label());
        }
        labels.push(info.label());
        let pages: Vec<Page> = sp.pages.iter().map(|pg| Page { id: PageId(d.alloc()), parent: None, ..pg.clone() }).collect();
        d.parents.push(Arc::new(Spread { id: ids[&sp.id], pages, items: Vec::new(), parent: Some(info), allow_shuffle: sp.allow_shuffle }));
        let (roots, index): (Vec<ItemId>, usize) = (sp.items.iter().map(|it| it.id).collect(), d.parents.len() - 1);
        let copies = super::object::duplicate_from(d, src, &roots, SpreadRef::Parent(index), Vec2::ZERO)?;
        if copies.len() == roots.len() {
            for (old, new) in sp.items.iter().zip(&copies) {
                let Some(copy) = d.item(*new) else { continue };
                let (mut from, mut to) = (Vec::new(), Vec::new());
                old.walk(&mut |it| from.push(it.id));
                copy.walk(&mut |it| to.push(it.id));
                moved.extend(from.into_iter().zip(to));
            }
        }
    }
    // A parent overrides objects of the parent it's based on by their ids.
    for sp in &mut d.parents[first..] {
        let sp = Arc::make_mut(sp);
        for pg in &mut sp.pages {
            pg.overridden = pg.overridden.iter().filter_map(|o| moved.get(o).copied()).collect();
        }
        for it in &mut sp.items {
            each_item_mut(Arc::make_mut(it), &mut |it| {
                if let Some(o) = it.overrides {
                    it.overrides = moved.get(&o).copied();
                }
            });
        }
    }

    // A replaced parent: what used it uses the loaded one, which takes its place in the list.
    let mut replaced = Vec::new();
    for (i, old) in target.iter().enumerate() {
        let Some(old) = *old else { continue };
        let new = ids[&src.parents[i].id];
        for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
            let uses = sp.pages.iter().any(|pg| pg.parent == Some(old)) || sp.parent.as_ref().is_some_and(|p| p.based_on == Some(old));
            if !uses {
                continue;
            }
            let sp = Arc::make_mut(sp);
            for pg in &mut sp.pages {
                if pg.parent == Some(old) {
                    pg.parent = Some(new);
                }
            }
            if let Some(p) = &mut sp.parent
                && p.based_on == Some(old)
            {
                p.based_on = Some(new);
            }
        }
        let at = d.parent_index(old);
        d.delete_parent(old)?;
        if let (Some(at), Some(from)) = (at, d.parent_index(new)) {
            let sp = d.parents.remove(from);
            d.parents.insert(at.min(d.parents.len()), sp);
        }
        replaced.push(theirs[i].label());
    }
    d.order_parent_pages();
    Ok(json!({"parents": labels, "renamed": renamed, "replaced": replaced, "sizesDiffer": sizes}))
}

/// The first prefix (A…Z, then P1, P2…) that isn't taken.
fn free_prefix(taken: &HashSet<String>) -> String {
    ('A'..='Z').map(String::from).chain((1..).map(|n| format!("P{n}"))).find(|p| !taken.contains(p)).unwrap_or_default()
}

/// A story's text and every text inside it: table cells, footnotes and endnotes.
fn texts(st: &Story, f: &mut impl FnMut(&Story)) {
    st.for_each_text(f);
    for e in &st.endnotes {
        e.text.for_each_text(f);
    }
}

fn each_item_mut(it: &mut Item, f: &mut impl FnMut(&mut Item)) {
    f(it);
    if let Some(kids) = it.children_mut() {
        for k in kids {
            each_item_mut(Arc::make_mut(k), f);
        }
    }
}

/// Put the objects and guides of `src`'s parents on `d`'s layers of the same names. The layers
/// `d` lacks come in above its own, in the file's order; objects on a layer the file doesn't
/// list go on `d`'s default layer.
fn remap_layers(d: &mut Document, src: &mut Document) {
    let mut used = HashSet::new();
    for sp in &src.parents {
        for it in &sp.items {
            it.walk(&mut |i| {
                used.insert(i.layer);
            });
        }
        used.extend(sp.pages.iter().flat_map(|pg| pg.guides.iter().filter_map(|g| g.layer)));
    }
    let (mut map, mut added): (HashMap<LayerId, LayerId>, Vec<Layer>) = Default::default();
    for l in src.layers.iter().filter(|l| used.contains(&l.id)) {
        let to = match d.layers.iter().find(|x| x.name == l.name) {
            Some(x) => x.id,
            None => {
                let id = LayerId(d.alloc());
                added.push(Layer { id, ..l.clone() });
                id
            }
        };
        map.insert(l.id, to);
    }
    d.layers.splice(0..0, added);
    let fallback = d.default_layer();
    for sp in &mut src.parents {
        let sp = Arc::make_mut(sp);
        for it in &mut sp.items {
            each_item_mut(Arc::make_mut(it), &mut |i| i.layer = map.get(&i.layer).copied().unwrap_or(fallback));
        }
        for g in sp.pages.iter_mut().flat_map(|pg| pg.guides.iter_mut()) {
            if let Some(l) = &mut g.layer {
                *l = map.get(l).copied().unwrap_or(fallback);
            }
        }
    }
}

/// Point the text variables in `stories` of `src` at `d`'s variables of the same names; the
/// variables `d` lacks come in. Returns those. Refused when `d` has no room for one: its
/// instances would name another variable.
fn remap_variables(d: &mut Document, src: &mut Document, stories: &BTreeSet<StoryId>) -> Result<Vec<vars::TextVariable>> {
    let mut used = BTreeSet::new();
    for st in stories.iter().filter_map(|id| src.story(*id)) {
        texts(st, &mut |t| used.extend(t.text.chars().filter_map(vars::var_index)));
    }
    let (mut map, mut added) = (HashMap::new(), Vec::new());
    for i in used {
        let Some(v) = src.text_variables.get(i) else { continue };
        let j = match d.text_variables.iter().position(|x| x.name == v.name) {
            Some(j) => j,
            None if d.text_variables.len() < vars::VAR_MAX => {
                d.text_variables.push(v.clone());
                added.push(v.clone());
                d.text_variables.len() - 1
            }
            None => {
                return Err(bad(
                    ID,
                    format!("this document has {} text variables, the most it can hold: there is no room for `{}`", vars::VAR_MAX, v.name),
                ));
            }
        };
        if let Some(c) = vars::var_char(j).filter(|_| i != j) {
            map.insert(i, c);
        }
    }
    if map.is_empty() {
        return Ok(added);
    }
    // Every instance character is in the same UTF-8 length range, so the formatting runs stay.
    let remap = |t: &mut Story| {
        if t.text.chars().any(|c| vars::var_index(c).is_some_and(|i| map.contains_key(&i))) {
            t.text = t.text.chars().map(|c| vars::var_index(c).and_then(|i| map.get(&i).copied()).unwrap_or(c)).collect();
        }
    };
    for id in stories {
        if let Some(st) = src.story_mut(*id) {
            st.for_each_text_mut(&mut |t| remap(t));
            for e in &mut st.endnotes {
                Arc::make_mut(e).text.for_each_text_mut(&mut |t| remap(t));
            }
        }
    }
    Ok(added)
}

/// The object styles the parents' objects use that `d` lacks, with the ones they're based on,
/// in the file's order.
fn object_styles(d: &Document, src: &Document) -> Vec<ObjectStyle> {
    let mut todo = Vec::new();
    for sp in &src.parents {
        for it in &sp.items {
            it.walk(&mut |i| todo.push(i.object_style.clone()));
        }
    }
    let mut wanted = HashSet::new();
    while let Some(name) = todo.pop() {
        if name.is_empty() || name == NO_OBJECT_STYLE || d.styles.object_style(&name).is_some() || !wanted.insert(name.clone()) {
            continue;
        }
        todo.extend(src.styles.object_style(&name).and_then(|o| o.based_on.clone()));
    }
    src.styles.object.iter().filter(|o| wanted.contains(&o.name)).cloned().collect()
}

/// The stroke styles of the file that the parents' objects and the loaded object styles use and
/// `d` lacks.
fn stroke_styles(d: &Document, src: &Document, objects: &[ObjectStyle]) -> Vec<StrokeStyleDef> {
    let mut used = HashSet::new();
    let mut note = |s: &Stroke| {
        if let StrokeType::Style { name } = &s.kind {
            used.insert(name.clone());
        }
    };
    for sp in &src.parents {
        for it in &sp.items {
            it.walk(&mut |i| note(&i.stroke));
        }
    }
    objects.iter().filter_map(|o| o.stroke.as_ref()).for_each(&mut note);
    src.stroke_styles.iter().filter(|s| used.contains(&s.name) && !d.stroke_styles.iter().any(|x| x.name == s.name)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use designcraft_color::{Color, Swatch};
    use designcraft_doc::{CharAttrs, Content, Document, ItemId, ParaAttrs, ParagraphStyle, SpreadId, vars};
    use serde_json::{Value, json};

    use crate::Session;

    fn file_of(s: &Session) -> String {
        super::super::file::base64_encode(&super::super::file::to_bytes(&s.doc().unwrap().doc).unwrap())
    }

    fn doc(s: &Session) -> &Document {
        &s.doc().unwrap().doc
    }

    fn labels(s: &Session) -> Vec<String> {
        doc(s).parents.iter().map(|p| p.parent.as_ref().unwrap().label()).collect()
    }

    fn style(name: &str, based_on: &str, chars: CharAttrs) -> ParagraphStyle {
        ParagraphStyle {
            name: name.into(),
            based_on: Some(based_on.into()),
            next_style: None,
            para: ParaAttrs::default(),
            chars,
            shortcut: String::new(),
        }
    }

    /// A template: on A-Parent a brand-coloured bar and a folio in its own paragraph style ending
    /// in an Edition variable, both on a Furniture layer; B-Section is based on A-Parent.
    fn template() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"facingPages": false})).unwrap();
        s.edit(|d, _| {
            d.swatches.push(Swatch::color("Brand Teal", Color::cmyk(0.9, 0.0, 0.4, 0.2)));
            let st = d.styles_mut();
            st.paragraph.push(style("Folio Base", "[Basic Paragraph]", CharAttrs { size: Some(8.0), ..Default::default() }));
            st.paragraph.push(style("Folio", "Folio Base", CharAttrs { fill: Some("Brand Teal".into()), ..Default::default() }));
            Ok(())
        })
        .unwrap();
        s.execute("variables.define", &json!({"name": "Edition", "type": "custom", "text": "Spring"})).unwrap();
        let layer = s.execute("layer.new", &json!({"name": "Furniture"})).unwrap()["id"].clone();
        s.execute("layer.activate", &json!({"id": layer})).unwrap();
        let parent = json!({"kind": "parent", "index": 0});
        let bar = s.execute("frame.create", &json!({"rect": [0, 0, 612, 24], "spread": parent})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"ids": [bar], "swatch": "Brand Teal"})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 740, 300, 760], "content": "text", "text": "Folio ", "spread": parent})).unwrap();
        s.execute("variables.insert", &json!({"name": "Edition"})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Folio"})).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "B", "name": "Section"})).unwrap();
        s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": "A"})).unwrap();
        s
    }

    fn new_doc(params: Value) -> Session {
        let mut s = Session::new();
        s.execute("file.new", &params).unwrap();
        s
    }

    fn parent_id(s: &Session, label: &str) -> SpreadId {
        doc(s).parents.iter().find(|p| p.parent.as_ref().unwrap().label() == label).unwrap().id
    }

    /// The course's chapter 36: Load Parent Pages brings a template's parents with their
    /// objects, and the styles, swatch, variable and layer those use, in one undo step. The
    /// parent of a name this document has comes in under the next free prefix.
    #[test]
    fn loaded_parents_bring_their_objects_and_what_those_use() {
        let mut s = new_doc(json!({"facingPages": false}));
        // A variable of this document's own, so the template's Edition gets another index here.
        s.execute("variables.define", &json!({"name": "Volume", "type": "custom", "text": "IV"})).unwrap();
        let before = doc(&s).clone();
        let r = s.execute("layout.parents.load", &json!({"base64": file_of(&template()), "name": "Template.designcraft"})).unwrap();
        assert_eq!(r["parents"], json!(["C-Parent", "B-Section"]));
        assert_eq!(r["renamed"], json!({"A-Parent": "C-Parent"}));
        assert_eq!(r["replaced"], json!([]));
        assert_eq!(r["sizesDiffer"], false);
        assert_eq!(labels(&s), ["A-Parent", "C-Parent", "B-Section"]);
        let d = doc(&s);
        let (c, b) = (&d.parents[1], &d.parents[2]);
        assert_eq!(b.parent.as_ref().unwrap().based_on, Some(c.id), "B-Section stays based on the loaded A-Parent");
        assert!(d.parents[0].items.is_empty(), "this document's own A-Parent is untouched");

        // The objects, on the Furniture layer, with their colour, style and variable.
        assert_eq!(c.items.len(), 2);
        let furniture = d.layers.iter().find(|l| l.name == "Furniture").expect("the layer comes along").id;
        assert!(c.items.iter().all(|it| it.layer == furniture));
        assert_eq!(c.items[0].fill.swatch, "Brand Teal");
        assert!(d.swatch("Brand Teal").is_some());
        let Content::Text(tf) = &c.items[1].content else { panic!("the folio is a text frame") };
        let story = d.story(tf.story).unwrap();
        assert_eq!(story.frames, [c.items[1].id]);
        let edition = d.variable("Edition").expect("the variable comes along");
        assert_ne!(edition, d.variable("Volume").unwrap());
        assert_eq!(story.text.trim_end_matches('\n'), format!("Folio {}", vars::var_char(edition).unwrap()));
        assert_eq!(story.paras[0].style, "Folio");
        let folio = d.styles.para("Folio").unwrap();
        assert_eq!(folio.chars.fill.as_deref(), Some("Brand Teal"));
        assert!(d.styles.para("Folio Base").is_some(), "the style Folio is based on comes along");

        // One undo step takes it all back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*doc(&s), before);
    }

    /// Replace Parent Pages: the loaded A-Parent takes the place of this document's. Its pages
    /// show the loaded one and their override of the old parent's object becomes an ordinary
    /// object. A parent whose prefix alone is taken (B) comes in under the next free one.
    #[test]
    fn replace_puts_the_loaded_parent_in_the_place_of_the_one_of_the_same_name() {
        let mut s = new_doc(json!({"facingPages": false, "pages": 2}));
        s.execute("frame.create", &json!({"rect": [36, 36, 200, 100], "spread": {"kind": "parent", "index": 0}})).unwrap();
        s.execute("layout.pages.applyParent", &json!({"pages": [0, 1], "parent": "A"})).unwrap();
        let local = s.execute("layout.overrideParentItems", &json!({"page": 0})).unwrap()["ids"][0].as_u64().map(ItemId).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "B", "name": "Other"})).unwrap();
        let other = parent_id(&s, "B-Other");
        s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": "A"})).unwrap();

        let r = s
            .execute("layout.parents.load", &json!({"base64": file_of(&template()), "name": "Template.designcraft", "conflicts": "replace"}))
            .unwrap();
        assert_eq!(r["replaced"], json!(["A-Parent"]));
        assert_eq!(r["renamed"], json!({"B-Section": "C-Section"}));
        assert_eq!(labels(&s), ["A-Parent", "B-Other", "C-Section"], "in the old A-Parent's place");
        let d = doc(&s);
        let a = &d.parents[0];
        assert_eq!(a.items.len(), 2, "the template's objects");
        assert_eq!(d.parents[1].id, other);
        for p in [&d.parents[1], &d.parents[2]] {
            assert_eq!(p.parent.as_ref().unwrap().based_on, Some(a.id));
        }
        for i in 0..2 {
            assert_eq!(d.page(i).unwrap().parent, Some(a.id), "page {}", i + 1);
        }
        assert!(d.page(0).unwrap().overridden.is_empty());
        assert_eq!(d.item(local).expect("the override stays").overrides, None, "and is an ordinary object now");
    }

    /// Before loading, the file's parents and what loading them would meet: a parent of the same
    /// name, and another page size.
    #[test]
    fn the_list_shows_conflicts_and_another_page_size() {
        let mut s = new_doc(json!({"preset": "A4", "facingPages": false}));
        let r = s.execute("layout.parents.load.list", &json!({"base64": file_of(&template()), "name": "Template.designcraft"})).unwrap();
        assert_eq!(r["parents"], json!([{"label": "A-Parent", "conflict": true}, {"label": "B-Section", "conflict": false}]));
        assert_eq!(r["sizesDiffer"], true);
        let mut empty = template();
        let ids: Vec<u64> = doc(&empty).parents.iter().map(|p| p.id.0).collect();
        for id in ids {
            empty.execute("layout.parents.delete", &json!({"id": id})).unwrap();
        }
        let err = s.execute("layout.parents.load", &json!({"base64": file_of(&empty), "name": "Empty.designcraft"})).unwrap_err();
        assert!(err.to_string().contains("no parent pages"), "{err}");
        assert!(s.execute("layout.parents.load", &json!({"base64": file_of(&template()), "conflicts": "merge"})).is_err());
    }

    /// A document whose variable table is full can't take the template's Edition variable: the
    /// load is refused and changes nothing, rather than its folio naming another variable.
    #[test]
    fn a_full_variable_table_refuses_the_load() {
        let mut s = new_doc(json!({"facingPages": false}));
        for i in doc(&s).text_variables.len()..vars::VAR_MAX {
            s.execute("variables.define", &json!({"name": format!("Variable {i}"), "type": "custom", "text": "x"})).unwrap();
        }
        let before = doc(&s).clone();
        let err = s.execute("layout.parents.load", &json!({"base64": file_of(&template()), "name": "Template.designcraft"})).unwrap_err();
        assert!(err.to_string().contains("Edition"), "{err}");
        assert_eq!(*doc(&s), before);
    }
}
