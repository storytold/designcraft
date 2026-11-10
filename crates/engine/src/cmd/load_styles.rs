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
mod tests;
