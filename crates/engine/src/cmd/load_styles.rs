//! Load Styles: the paragraph and character styles of another document (`.designcraft` or IDML)
//! come into this one, with the styles, swatches and lists they name.

use std::collections::{BTreeMap, HashMap, HashSet};

use designcraft_color::{Swatch, SwatchValue};
use designcraft_doc::{CharAttrs, Document, NO_CHAR_STYLE, NO_PARA_STYLE, NumberedList, ParaAttrs, ParagraphStyle, Styles};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

const ID: &str = "style.load";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "style.load",
            "Load Styles…",
            [],
            None,
            "{path | base64, name?: the file's name (`.idml` reads IDML), paragraph?: [name], character?: [name], conflicts?: redefine|useExisting|autoRename} — paragraph and character styles of another document (.designcraft or .idml): all of a kind, or the named ones (`[]`: none). The styles, swatches and lists they name come along where this document has none. A style this document defines differently under the same name takes the loaded definition (redefine, the default), stays as it is (useExisting), or the loaded one gets a new name (autoRename) → {paragraph: {added, replaced, renamed: {name: new name}}, character: {…}, swatches: [name]}",
            has_doc,
            load
        ),
        cmd!(query "style.load.list", "List Styles in File", [], None,
            "{path | base64, name?} → {paragraph: [{name, conflict}], character: [{name, conflict}]} — the styles `style.load` would bring; `conflict`: this document defines a style of that name differently",
            has_doc, list),
    ]
}

/// What happens to a loaded style when the document defines its name differently.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conflict {
    /// The loaded definition replaces the document's.
    Redefine,
    /// The document's style stays.
    UseExisting,
    /// The loaded style comes in under a free name (`Name 2`).
    AutoRename,
}

/// What loading does with one style of the file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    /// The document keeps what it has.
    Skip,
    Add,
    Replace,
    /// Added under a new name.
    Rename,
}

/// The loaded styles of one kind (paragraph or character), by their name in the file.
#[derive(Default)]
struct Kind {
    actions: HashMap<String, Action>,
    renames: HashMap<String, String>,
    added: Vec<String>,
    replaced: Vec<String>,
}

impl Kind {
    /// The name a style of the file has in this document.
    fn follow(&self, name: &mut String) {
        if let Some(new) = self.renames.get(name.as_str()) {
            *name = new.clone();
        }
    }

    fn changes(&self) -> bool {
        !(self.added.is_empty() && self.replaced.is_empty() && self.renames.is_empty())
    }

    fn report(&self) -> Value {
        let renamed: BTreeMap<&String, &String> = self.renames.iter().collect();
        json!({"added": self.added, "replaced": self.replaced, "renamed": renamed})
    }
}

/// The document a command reads styles from.
fn source(p: &Value, id: &str) -> Result<Document> {
    let (bytes, name) = match (str_param(p, "path"), str_param(p, "base64")) {
        (_, Some(b)) => (super::file::base64_decode(b), str_param(p, "name").unwrap_or("").to_string()),
        (Some(path), None) => (crate::io::read_file(path).map_err(|e| bad(id, e))?, path.to_string()),
        _ => return Err(bad(id, "give `path` or `base64`")),
    };
    if name.to_lowercase().ends_with(".idml") {
        designcraft_idml::import_idml(&bytes).map_err(|e| bad(id, e.to_string()))
    } else {
        super::file::from_bytes(&bytes).map_err(|e| bad(id, e.to_string()))
    }
}

/// The styles of one kind to load: every one (`None`) or the named ones.
fn names(p: &Value, key: &str) -> Result<Option<Vec<String>>> {
    let list = match p.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v.as_array().and_then(|a| a.iter().map(|n| n.as_str().map(str::to_string)).collect::<Option<Vec<_>>>()),
    };
    list.map(Some).ok_or_else(|| bad(ID, format!("`{key}` must be a list of style names")))
}

/// The styles a paragraph style names: its base, its next style and its nested, line and GREP
/// styles. `true` marks a paragraph style.
fn para_refs(s: &ParagraphStyle) -> Vec<(bool, String)> {
    let p = &s.para;
    let chars = p
        .nested_styles
        .iter()
        .flatten()
        .map(|n| &n.style)
        .chain(p.grep_styles.iter().flatten().map(|g| &g.style))
        .chain(p.nested_line_styles.iter().flatten().map(|l| &l.style));
    s.based_on.iter().chain(&s.next_style).map(|n| (true, n.clone())).chain(chars.map(|n| (false, n.clone()))).collect()
}

fn char_swatches(c: &CharAttrs) -> impl Iterator<Item = &String> {
    [&c.fill, &c.stroke, &c.underline_color, &c.strikethrough_color].into_iter().flatten()
}

fn para_swatches(p: &ParaAttrs) -> impl Iterator<Item = &String> {
    p.rule_above.iter().chain(&p.rule_below).map(|r| &r.color).chain(&p.shading_color).chain(&p.border_color)
}

/// Decide what happens to every asked-for style and to the styles those name. A style the
/// document defines the same way is left alone; one that is only named by another is added when
/// the document has none, and never replaces one.
fn plan(doc: &Document, src: &Document, paragraph: Option<&[String]>, character: Option<&[String]>, conflict: Conflict) -> Result<(Kind, Kind)> {
    let (theirs, mine) = (&src.styles, &doc.styles);
    let mut todo: Vec<(bool, String)> = Vec::new();
    match paragraph {
        Some(names) => {
            for n in names {
                if n == NO_PARA_STYLE || theirs.para(n).is_none() {
                    return Err(bad(ID, format!("the file has no paragraph style `{n}`")));
                }
                todo.push((true, n.clone()));
            }
        }
        None => todo.extend(theirs.paragraph.iter().filter(|s| s.name != NO_PARA_STYLE).map(|s| (true, s.name.clone()))),
    }
    match character {
        Some(names) => {
            for n in names {
                if n == NO_CHAR_STYLE || theirs.char_style(n).is_none() {
                    return Err(bad(ID, format!("the file has no character style `{n}`")));
                }
                todo.push((false, n.clone()));
            }
        }
        None => todo.extend(theirs.character.iter().filter(|s| s.name != NO_CHAR_STYLE).map(|s| (false, s.name.clone()))),
    }
    let asked: HashSet<(bool, String)> = todo.iter().cloned().collect();
    let (mut para, mut chars) = (Kind::default(), Kind::default());
    while let Some(key) = todo.pop() {
        let (is_para, name) = (key.0, key.1.as_str());
        let kind = if is_para { &mut para } else { &mut chars };
        if kind.actions.contains_key(name) {
            continue;
        }
        // Is the document's style of that name the same, and which styles does the file's name?
        let (same, refs) = if is_para {
            let Some(s) = theirs.para(name).filter(|s| s.name != NO_PARA_STYLE) else { continue };
            (mine.para(name).map(|m| m == s), para_refs(s))
        } else {
            let Some(s) = theirs.char_style(name).filter(|s| s.name != NO_CHAR_STYLE) else { continue };
            (mine.char_style(name).map(|m| m == s), s.based_on.iter().map(|b| (false, b.clone())).collect())
        };
        let action = match same {
            None => Action::Add,
            Some(false) if asked.contains(&key) => match conflict {
                Conflict::Redefine => Action::Replace,
                Conflict::UseExisting => Action::Skip,
                Conflict::AutoRename => Action::Rename,
            },
            Some(_) => Action::Skip,
        };
        if action != Action::Skip {
            todo.extend(refs);
        }
        kind.actions.insert(key.1, action);
    }
    // Free names for the renamed styles, in the file's order.
    let order = theirs.paragraph.iter().map(|s| (true, &s.name)).chain(theirs.character.iter().map(|s| (false, &s.name)));
    for (is_para, name) in order {
        let kind = if is_para { &mut para } else { &mut chars };
        if kind.actions.get(name) != Some(&Action::Rename) || kind.renames.contains_key(name) {
            continue;
        }
        // A built-in style comes in as an ordinary one, without its brackets.
        let base = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')).filter(|n| !n.is_empty()).unwrap_or(name);
        let taken = |n: &str| {
            (if is_para { mine.para(n).is_some() } else { mine.char_style(n).is_some() })
                || kind.actions.get(n).is_some_and(|a| *a != Action::Skip)
                || kind.renames.values().any(|r| r == n)
        };
        let new = Styles::unique_name(taken, base);
        kind.renames.insert(name.clone(), new);
    }
    Ok((para, chars))
}

/// The `conflicts` parameter: how a style that differs under the same name is handled.
fn conflict_param(p: &Value) -> Result<Conflict> {
    Ok(match p.get("conflicts") {
        None | Some(Value::Null) => Conflict::Redefine,
        Some(v) => match v.as_str() {
            Some("redefine") => Conflict::Redefine,
            Some("useExisting") => Conflict::UseExisting,
            Some("autoRename") => Conflict::AutoRename,
            _ => return Err(bad(ID, "`conflicts` must be redefine, useExisting or autoRename")),
        },
    })
}

/// Give the loaded style `ours` the export tag the source had for `theirs` (`kind` is `p` or `c`).
fn carry_export_tag(src: &Styles, dst: &mut Styles, kind: char, theirs: &str, ours: &str) {
    let tag = format!("{kind}:{ours}");
    match src.export_tags.get(&format!("{kind}:{theirs}")) {
        Some(t) => dst.export_tags.insert(tag, t.clone()),
        None => dst.export_tags.remove(&tag),
    };
}

/// A loaded style and one the document keeps can name each other as their base.
fn check_based_on_cycles(styles: &Styles, loaded: &[(bool, String)]) -> Result<()> {
    for (is_para, name) in loaded {
        let looped = if *is_para {
            styles.para(name).and_then(|s| s.based_on.as_deref()).is_some_and(|b| styles.para_based_on_cycles(name, b))
        } else {
            styles.char_style(name).and_then(|s| s.based_on.as_deref()).is_some_and(|b| styles.char_based_on_cycles(name, b))
        };
        if looped {
            let kind = if *is_para { "paragraph" } else { "character" };
            return Err(bad(ID, format!("{kind} style `{name}` would be based on itself; load the style it is based on as well")));
        }
    }
    Ok(())
}

fn load(s: &mut Session, p: &Value) -> Result<Value> {
    let src = source(p, ID)?;
    let conflict = conflict_param(p)?;
    let (paragraph, character) = (names(p, "paragraph")?, names(p, "character")?);
    let doc = s.doc()?.doc.clone();
    let (mut para, mut chars) = plan(&doc, &src, paragraph.as_deref(), character.as_deref(), conflict)?;

    let mut styles = (*doc.styles).clone();
    let (mut swatches, mut lists): (HashSet<String>, HashSet<String>) = Default::default();
    // (kind, name here) of every style that came in, for the based-on check.
    let mut loaded: Vec<(bool, String)> = Vec::new();
    for theirs in &src.styles.paragraph {
        let Some(action) = para.actions.remove(&theirs.name).filter(|a| *a != Action::Skip) else { continue };
        let mut style = theirs.clone();
        para.follow(&mut style.name);
        style.based_on.iter_mut().chain(&mut style.next_style).for_each(|n| para.follow(n));
        let p = &mut style.para;
        p.nested_styles.iter_mut().flatten().for_each(|n| chars.follow(&mut n.style));
        p.grep_styles.iter_mut().flatten().for_each(|g| chars.follow(&mut g.style));
        p.nested_line_styles.iter_mut().flatten().for_each(|l| chars.follow(&mut l.style));
        swatches.extend(char_swatches(&style.chars).chain(para_swatches(&style.para)).cloned());
        lists.extend(style.para.list_name.iter().cloned());
        carry_export_tag(&src.styles, &mut styles, 'p', &theirs.name, &style.name);
        loaded.push((true, style.name.clone()));
        match styles.para_mut(&style.name) {
            Some(m) if action == Action::Replace => {
                *m = style;
                para.replaced.push(theirs.name.clone());
            }
            _ => {
                if action == Action::Add {
                    para.added.push(theirs.name.clone());
                }
                styles.paragraph.push(style);
            }
        }
    }
    for theirs in &src.styles.character {
        let Some(action) = chars.actions.remove(&theirs.name).filter(|a| *a != Action::Skip) else { continue };
        let mut style = theirs.clone();
        chars.follow(&mut style.name);
        style.based_on.iter_mut().for_each(|n| chars.follow(n));
        swatches.extend(char_swatches(&style.chars).cloned());
        carry_export_tag(&src.styles, &mut styles, 'c', &theirs.name, &style.name);
        loaded.push((false, style.name.clone()));
        match styles.char_style_mut(&style.name) {
            Some(m) if action == Action::Replace => {
                *m = style;
                chars.replaced.push(theirs.name.clone());
            }
            _ => {
                if action == Action::Add {
                    chars.added.push(theirs.name.clone());
                }
                styles.character.push(style);
            }
        }
    }
    check_based_on_cycles(&styles, &loaded)?;

    let swatches = missing_swatches(&doc, &src, swatches);
    let lists: Vec<NumberedList> = {
        let mut seen = HashSet::new();
        let has = |n: &str| doc.settings.lists.iter().any(|l| l.name == n);
        src.settings.lists.iter().filter(|l| lists.contains(&l.name) && !has(&l.name) && seen.insert(l.name.as_str())).cloned().collect()
    };
    let report = json!({"paragraph": para.report(), "character": chars.report(), "swatches": swatches.iter().map(|w| &w.name).collect::<Vec<_>>()});
    // Nothing to load leaves no undo step.
    if para.changes() || chars.changes() {
        s.edit(|d, _| {
            *d.styles_mut() = styles;
            d.swatches.extend(swatches);
            d.settings.lists.extend(lists);
            Ok(())
        })?;
    }
    Ok(report)
}

/// The swatches of the file that the loaded styles name and the document lacks, in the file's
/// order; a tint brings its base colour.
fn missing_swatches(doc: &Document, src: &Document, mut used: HashSet<String>) -> Vec<Swatch> {
    let mut todo: Vec<String> = used.iter().cloned().collect();
    while let Some(name) = todo.pop() {
        if let Some(Swatch { value: SwatchValue::Tint { base, .. }, .. }) = src.swatch(&name)
            && used.insert(base.clone())
        {
            todo.push(base.clone());
        }
    }
    let mut seen = HashSet::new();
    src.swatches.iter().filter(|w| used.contains(&w.name) && doc.swatch(&w.name).is_none() && seen.insert(w.name.as_str())).cloned().collect()
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let src = source(p, "style.load.list")?;
    let mine = &s.doc()?.doc.styles;
    let row = |name: &str, conflict: bool| json!({"name": name, "conflict": conflict});
    let paragraph: Vec<Value> =
        src.styles.paragraph.iter().filter(|x| x.name != NO_PARA_STYLE).map(|x| row(&x.name, mine.para(&x.name).is_some_and(|m| m != x))).collect();
    let character: Vec<Value> = src
        .styles
        .character
        .iter()
        .filter(|x| x.name != NO_CHAR_STYLE)
        .map(|x| row(&x.name, mine.char_style(&x.name).is_some_and(|m| m != x)))
        .collect();
    Ok(json!({"paragraph": paragraph, "character": character}))
}

#[cfg(test)]
mod tests {
    use designcraft_color::{Color, Swatch, SwatchValue};
    use designcraft_doc::{CharAttrs, CharacterStyle, ExportTag, GrepStyle, NumberedList, ParaAttrs, ParagraphStyle, Rule, Styles};
    use serde_json::{Value, json};

    use crate::Session;

    const BASIC: &str = "[Basic Paragraph]";

    fn para(name: &str, based_on: Option<&str>, size: f64) -> ParagraphStyle {
        ParagraphStyle {
            name: name.into(),
            based_on: based_on.map(str::to_string),
            next_style: None,
            para: ParaAttrs::default(),
            chars: CharAttrs { size: Some(size), ..Default::default() },
            shortcut: String::new(),
        }
    }

    fn chars(name: &str, based_on: Option<&str>, size: f64) -> CharacterStyle {
        CharacterStyle {
            name: name.into(),
            based_on: based_on.map(str::to_string),
            chars: CharAttrs { size: Some(size), ..Default::default() },
            shortcut: String::new(),
        }
    }

    fn new_doc() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s
    }

    fn file_of(s: &Session) -> String {
        super::super::file::base64_encode(&super::super::file::to_bytes(&s.doc().unwrap().doc).unwrap())
    }

    /// A client's document: Body in the brand colour with a GREP style, Head and Quote based on
    /// it, Small characters based on Lead, an 11 pt [Basic Paragraph], and a swatch no style uses.
    fn client() -> Session {
        let mut s = new_doc();
        s.edit(|d, _| {
            d.swatches.push(Swatch::color("Brand Teal", Color::cmyk(0.9, 0.0, 0.4, 0.2)));
            d.swatches.push(Swatch {
                name: "Brand Teal 40%".into(),
                value: SwatchValue::Tint { base: "Brand Teal".into(), tint: 0.4 },
                locked: false,
                named: true,
                hidden: false,
            });
            d.swatches.push(Swatch::color("Unused Red", Color::cmyk(0.0, 1.0, 1.0, 0.0)));
            let st = d.styles_mut();
            st.para_mut(BASIC).unwrap().chars.size = Some(11.0);
            st.character.push(chars("Lead", None, 9.0));
            st.character.push(chars("Small", Some("Lead"), 7.0));
            let mut body = para("Body", Some(BASIC), 10.0);
            body.chars.fill = Some("Brand Teal".into());
            body.para.grep_styles = Some(vec![GrepStyle { style: "Small".into(), pattern: r"\d+".into() }]);
            st.paragraph.push(body);
            let mut head = para("Head", Some("Body"), 20.0);
            head.next_style = Some("Body".into());
            st.paragraph.push(head);
            let mut quote = para("Quote", Some("Body"), 12.0);
            quote.para.rule_above = Some(Rule { on: true, color: "Brand Teal 40%".into(), ..Default::default() });
            st.paragraph.push(quote);
            Ok(())
        })
        .unwrap();
        s
    }

    fn styles(s: &Session) -> Styles {
        (*s.doc().unwrap().doc.styles).clone()
    }

    #[test]
    fn every_style_of_the_file_loads_with_the_swatches_it_uses() {
        let mut s = new_doc();
        let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
        let d = &s.doc().unwrap().doc;
        let head = d.styles.para("Head").unwrap();
        assert_eq!((head.based_on.as_deref(), head.next_style.as_deref(), head.chars.size), (Some("Body"), Some("Body"), Some(20.0)));
        assert_eq!(d.styles.para("Body").unwrap().chars.fill.as_deref(), Some("Brand Teal"));
        assert_eq!(d.styles.char_style("Small").unwrap().based_on.as_deref(), Some("Lead"));
        assert_eq!(d.swatch("Brand Teal"), client().doc().unwrap().doc.swatch("Brand Teal"));
        assert!(d.swatch("Brand Teal 40%").is_some());
        assert!(d.swatch("Unused Red").is_none(), "a swatch no loaded style uses stays behind");
        assert_eq!(r["paragraph"]["added"], json!(["Body", "Head", "Quote"]));
        assert_eq!(r["character"]["added"], json!(["Lead", "Small"]));
        assert_eq!(r["swatches"], json!(["Brand Teal", "Brand Teal 40%"]));
    }

    #[test]
    fn basic_paragraph_takes_the_definition_of_the_file() {
        let mut s = new_doc();
        let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para(BASIC).unwrap().chars.size, Some(11.0));
        assert_eq!(r["paragraph"]["replaced"], json!([BASIC]));
    }

    #[test]
    fn a_named_style_brings_the_styles_it_names() {
        let mut s = new_doc();
        let r = s.execute("style.load", &json!({"base64": file_of(&client()), "paragraph": ["Head"], "character": []})).unwrap();
        let st = styles(&s);
        assert_eq!(r["paragraph"]["added"], json!(["Body", "Head"]), "Head is based on Body");
        assert_eq!(r["character"]["added"], json!(["Lead", "Small"]), "Body's GREP style names Small, which is based on Lead");
        assert!(st.para("Quote").is_none());
        assert_eq!(st.para(BASIC).unwrap().chars.size, None, "a style the document has is only replaced when it was asked for");
        assert_eq!(r["paragraph"]["replaced"], json!([]));
        assert_eq!(r["swatches"], json!(["Brand Teal"]));
    }

    /// The document already has a Body and a Lead of its own.
    fn doc_with_body_and_lead() -> Session {
        let mut s = new_doc();
        s.edit(|d, _| {
            let st = d.styles_mut();
            st.paragraph.push(para("Body", None, 8.0));
            st.character.push(chars("Lead", None, 5.0));
            Ok(())
        })
        .unwrap();
        s
    }

    #[test]
    fn a_style_of_the_same_name_takes_the_loaded_definition() {
        let mut s = doc_with_body_and_lead();
        let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
        let st = styles(&s);
        assert_eq!((st.para("Body").unwrap().chars.size, st.char_style("Lead").unwrap().chars.size), (Some(10.0), Some(9.0)));
        assert_eq!(r["paragraph"]["replaced"], json!([BASIC, "Body"]));
        assert_eq!(r["character"]["replaced"], json!(["Lead"]));
        assert_eq!(st.paragraph.iter().filter(|p| p.name == "Body").count(), 1);
    }

    #[test]
    fn use_existing_keeps_the_styles_of_the_document() {
        let mut s = doc_with_body_and_lead();
        let r = s.execute("style.load", &json!({"base64": file_of(&client()), "conflicts": "useExisting"})).unwrap();
        let st = styles(&s);
        assert_eq!((st.para("Body").unwrap().chars.size, st.char_style("Lead").unwrap().chars.size), (Some(8.0), Some(5.0)));
        assert_eq!(st.para(BASIC).unwrap().chars.size, None);
        assert_eq!(st.para("Head").unwrap().based_on.as_deref(), Some("Body"));
        assert_eq!((&r["paragraph"]["added"], &r["paragraph"]["replaced"]), (&json!(["Head", "Quote"]), &json!([])));
    }

    #[test]
    fn auto_rename_loads_a_clashing_style_under_a_new_name() {
        let mut s = doc_with_body_and_lead();
        let r = s.execute("style.load", &json!({"base64": file_of(&client()), "conflicts": "autoRename"})).unwrap();
        let st = styles(&s);
        assert_eq!((st.para("Body").unwrap().chars.size, st.para("Body 2").unwrap().chars.size), (Some(8.0), Some(10.0)));
        assert_eq!((st.char_style("Lead").unwrap().chars.size, st.char_style("Lead 2").unwrap().chars.size), (Some(5.0), Some(9.0)));
        let head = st.para("Head").unwrap();
        assert_eq!(
            (head.based_on.as_deref(), head.next_style.as_deref()),
            (Some("Body 2"), Some("Body 2")),
            "the loaded Head follows the loaded Body"
        );
        assert_eq!(st.char_style("Small").unwrap().based_on.as_deref(), Some("Lead 2"));
        assert_eq!(st.para("Body 2").unwrap().based_on.as_deref(), Some("Basic Paragraph"));
        assert_eq!(st.para("Basic Paragraph").unwrap().chars.size, Some(11.0), "a built-in style comes in without its brackets");
        assert_eq!(st.para(BASIC).unwrap().chars.size, None);
        assert_eq!(r["paragraph"]["renamed"], json!({"[Basic Paragraph]": "Basic Paragraph", "Body": "Body 2"}));
        assert_eq!(r["character"]["renamed"], json!({"Lead": "Lead 2"}));
        assert_eq!(r["paragraph"]["replaced"], json!([]));
    }

    #[test]
    fn a_style_that_is_the_same_in_both_is_no_conflict() {
        let mut s = new_doc();
        let file = file_of(&client());
        s.execute("style.load", &json!({"base64": file})).unwrap();
        let loaded = styles(&s);
        for conflicts in ["redefine", "autoRename"] {
            let r = s.execute("style.load", &json!({"base64": file, "conflicts": conflicts})).unwrap();
            let none = json!({"added": [], "replaced": [], "renamed": {}});
            assert_eq!(r, json!({"paragraph": none, "character": none, "swatches": []}), "{conflicts}");
        }
        assert_eq!(styles(&s), loaded);
        // Loading nothing leaves no undo step: one Undo takes back the first load.
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(styles(&s).para("Head").is_none());
    }

    #[test]
    fn loading_is_one_undo_step() {
        let mut s = doc_with_body_and_lead();
        let (before, swatches) = (styles(&s), s.doc().unwrap().doc.swatches.clone());
        s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
        assert_ne!(styles(&s), before);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(styles(&s), before);
        assert_eq!(s.doc().unwrap().doc.swatches, swatches);
    }

    #[test]
    fn a_swatch_of_the_document_wins_over_the_one_in_the_file() {
        let mut s = new_doc();
        s.execute("swatch.create", &json!({"name": "Brand Teal", "color": "#ff0000"})).unwrap();
        let mine = s.doc().unwrap().doc.swatch("Brand Teal").cloned();
        let r = s.execute("style.load", &json!({"base64": file_of(&client())})).unwrap();
        assert_eq!(s.doc().unwrap().doc.swatch("Brand Teal").cloned(), mine);
        assert_eq!(r["swatches"], json!(["Brand Teal 40%"]));
    }

    #[test]
    fn a_tint_swatch_brings_its_base_colour() {
        let mut src = client();
        src.edit(|d, _| {
            let mut ruled = para("Ruled", None, 9.0);
            ruled.para.rule_below = Some(Rule { on: true, color: "Brand Teal 40%".into(), ..Default::default() });
            d.styles_mut().paragraph.push(ruled);
            Ok(())
        })
        .unwrap();
        let mut s = new_doc();
        let r = s.execute("style.load", &json!({"base64": file_of(&src), "paragraph": ["Ruled"], "character": []})).unwrap();
        assert_eq!(r["paragraph"]["added"], json!(["Ruled"]));
        assert_eq!(r["swatches"], json!(["Brand Teal", "Brand Teal 40%"]));
    }

    #[test]
    fn export_tags_and_named_lists_come_with_their_styles() {
        let mut src = client();
        src.edit(|d, _| {
            d.settings.lists.push(NumberedList { name: "Steps".into(), continue_across_stories: false });
            d.settings.lists.push(NumberedList { name: "Unused".into(), ..Default::default() });
            let st = d.styles_mut();
            st.export_tags.insert("p:Head".into(), ExportTag { tag: "h1".into(), class: "title".into(), split: true });
            st.export_tags.insert("c:Lead".into(), ExportTag { tag: "em".into(), class: String::new(), ..Default::default() });
            st.para_mut("Quote").unwrap().para.list_name = Some("Steps".into());
            Ok(())
        })
        .unwrap();
        let mut s = doc_with_body_and_lead();
        s.execute("style.load", &json!({"base64": file_of(&src), "conflicts": "autoRename"})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.styles.export_tag("Head", false).map(|t| (t.tag.as_str(), t.split)), Some(("h1", true)), "Split Document comes along");
        assert_eq!(d.styles.export_tag("Lead 2", true).map(|t| t.tag.as_str()), Some("em"), "the tag follows the new name");
        assert_eq!(d.styles.export_tag("Lead", true), None);
        assert_eq!(d.settings.lists.iter().map(|l| (l.name.as_str(), l.continue_across_stories)).collect::<Vec<_>>(), [("Steps", false)]);
    }

    #[test]
    fn styles_load_from_an_idml_file() {
        let idml = client().execute("file.exportIdml", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
        let mut s = new_doc();
        let r = s.execute("style.load", &json!({"base64": idml, "name": "Client.IDML", "paragraph": ["Head"]})).unwrap();
        let d = &s.doc().unwrap().doc;
        let head = d.styles.para("Head").unwrap();
        assert_eq!((head.based_on.as_deref(), head.chars.size), (Some("Body"), Some(20.0)));
        assert_eq!(d.styles.para("Body").unwrap().chars.fill.as_deref(), Some("Brand Teal"));
        assert!(d.swatch("Brand Teal").is_some());
        assert_eq!(r["paragraph"]["added"], json!(["Body", "Head"]));
    }

    #[test]
    fn styles_load_from_a_path() {
        let path = std::env::temp_dir().join(format!("dc-load-styles-{}.designcraft", std::process::id()));
        std::fs::write(&path, super::super::file::to_bytes(&client().doc().unwrap().doc).unwrap()).unwrap();
        let mut s = new_doc();
        let r = s.execute("style.load", &json!({"path": path.to_str().unwrap(), "paragraph": [], "character": ["Small"]}));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(r.unwrap()["character"]["added"], json!(["Lead", "Small"]));
        assert!(styles(&s).para("Head").is_none());
    }

    #[test]
    fn the_file_lists_its_styles_and_which_ones_clash() {
        let mut s = doc_with_body_and_lead();
        let file = file_of(&client());
        let name = |n: &str, conflict: bool| json!({"name": n, "conflict": conflict});
        let r = s.execute("style.load.list", &json!({"base64": file})).unwrap();
        assert_eq!(r["paragraph"], json!([name(BASIC, true), name("Body", true), name("Head", false), name("Quote", false)]));
        assert_eq!(r["character"], json!([name("Lead", true), name("Small", false)]));
        s.execute("style.load", &json!({"base64": file})).unwrap();
        let r = s.execute("style.load.list", &json!({"base64": file})).unwrap();
        assert!(r["paragraph"].as_array().unwrap().iter().all(|p| p["conflict"] == false), "the same definition is no clash: {r}");
    }

    fn unchanged_after(s: &mut Session, params: Value, why: &str) {
        let before = s.doc().unwrap().doc.clone();
        let e = s.execute("style.load", &params).unwrap_err().to_string();
        assert!(e.contains(why), "{e}");
        assert_eq!(s.doc().unwrap().doc, before);
    }

    #[test]
    fn what_cannot_be_loaded_is_an_error_and_changes_nothing() {
        let mut s = doc_with_body_and_lead();
        let file = file_of(&client());
        unchanged_after(&mut s, json!({}), "`path` or `base64`");
        unchanged_after(&mut s, json!({"base64": "AAAA"}), "");
        unchanged_after(&mut s, json!({"base64": "AAAA", "name": "a.idml"}), "");
        unchanged_after(&mut s, json!({"base64": file, "paragraph": ["Head", "Nope"]}), "no paragraph style `Nope`");
        unchanged_after(&mut s, json!({"base64": file, "character": ["[None]"]}), "no character style `[None]`");
        unchanged_after(&mut s, json!({"base64": file, "paragraph": ["[No Paragraph Style]"]}), "no paragraph style");
        unchanged_after(&mut s, json!({"base64": file, "paragraph": "Head"}), "`paragraph` must be a list of style names");
        unchanged_after(&mut s, json!({"base64": file, "character": [1]}), "`character` must be a list of style names");
        unchanged_after(&mut s, json!({"base64": file, "conflicts": "merge"}), "`conflicts`");
    }

    #[test]
    fn a_style_that_would_be_based_on_itself_is_refused() {
        // In the file B is based on A; in the document A is based on B.
        let mut src = new_doc();
        src.edit(|d, _| {
            let st = d.styles_mut();
            st.paragraph.push(para("A", None, 9.0));
            st.paragraph.push(para("B", Some("A"), 9.0));
            st.character.push(chars("X", None, 9.0));
            st.character.push(chars("Y", Some("X"), 9.0));
            Ok(())
        })
        .unwrap();
        let mut s = new_doc();
        s.edit(|d, _| {
            let st = d.styles_mut();
            st.paragraph.push(para("B", None, 9.0));
            st.paragraph.push(para("A", Some("B"), 9.0));
            st.character.push(chars("Y", None, 9.0));
            st.character.push(chars("X", Some("Y"), 9.0));
            Ok(())
        })
        .unwrap();
        unchanged_after(&mut s, json!({"base64": file_of(&src), "paragraph": ["B"], "character": []}), "`B` would be based on itself");
        unchanged_after(&mut s, json!({"base64": file_of(&src), "paragraph": [], "character": ["Y"]}), "`Y` would be based on itself");
        // With A loaded as well both take the file's order.
        s.execute("style.load", &json!({"base64": file_of(&src), "character": []})).unwrap();
        assert_eq!((styles(&s).para("A").unwrap().based_on.clone(), styles(&s).para("B").unwrap().based_on.clone()), (None, Some("A".to_string())));
    }

    #[test]
    fn a_file_with_a_based_on_loop_of_its_own_does_not_hang() {
        let mut src = new_doc();
        src.edit(|d, _| {
            let st = d.styles_mut();
            st.paragraph.push(para("A", Some("B"), 9.0));
            st.paragraph.push(para("B", Some("A"), 9.0));
            st.paragraph.push(para("Next", None, 9.0));
            st.para_mut("Next").unwrap().next_style = Some("Next".into());
            Ok(())
        })
        .unwrap();
        let mut s = new_doc();
        unchanged_after(&mut s, json!({"base64": file_of(&src), "paragraph": ["A"]}), "would be based on itself");
        let r = s.execute("style.load", &json!({"base64": file_of(&src), "paragraph": ["Next"]})).unwrap();
        assert_eq!(r["paragraph"]["added"], json!(["Next"]));
    }
}
