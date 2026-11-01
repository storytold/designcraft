use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Resources;

use super::MAX_DEPTH;

#[cfg(test)]
mod tests;

const MAX_OPERATIONS: usize = 100_000;
const LIMIT_WARNING: &str = "font scan operation limit reached: PDF/X-4 font embedding checks are incomplete";
const DEPTH_WARNING: &str = "font scan nesting depth limit reached: PDF/X-4 font embedding checks are incomplete";

pub(super) fn unembedded_fonts(pdf: &[u8]) -> Vec<String> {
    let Ok(file) = hayro_syntax::Pdf::new(pdf.to_vec()) else { return Vec::new() };
    let mut found: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut scan = FontScan::default();
    for (i, page) in file.pages().iter().enumerate() {
        scan.font = None;
        scan.mode = 0;
        if let Some(content) = page.page_stream() {
            scan.content(content, page.resources(), 0);
        }
        for name in std::mem::take(&mut scan.found) {
            found.entry(name).or_default().push(i + 1);
        }
        if scan.limited {
            break;
        }
    }
    let mut warnings: Vec<String> = found
        .into_iter()
        .map(|(name, pages)| {
            let list: Vec<String> = pages.iter().take(10).map(ToString::to_string).collect();
            let more = if pages.len() > 10 { ", …" } else { "" };
            let s = if pages.len() == 1 { "" } else { "s" };
            format!("the font {name} isn't embedded (page{s} {}{more}): PDF/X-4 needs every font used for visible text embedded", list.join(", "))
        })
        .collect();
    if scan.limited {
        warnings.push(LIMIT_WARNING.to_string());
    }
    if scan.depth_limited {
        warnings.push(DEPTH_WARNING.to_string());
    }
    warnings
}

fn font_embedded(font: &Dict<'_>) -> bool {
    let descriptor = match font.get::<Name<'_>>(b"Subtype").as_deref() {
        Some(b"Type3") => return true,
        Some(b"Type0") => {
            font.get::<Array<'_>>(b"DescendantFonts").and_then(|a| a.iter::<Dict<'_>>().next()).and_then(|d| d.get::<Dict<'_>>(b"FontDescriptor"))
        }
        _ => font.get::<Dict<'_>>(b"FontDescriptor"),
    };
    descriptor.is_some_and(|d| d.contains_key(b"FontFile") || d.contains_key(b"FontFile2") || d.contains_key(b"FontFile3"))
}

#[derive(Default)]
struct FontScan<'a> {
    found: BTreeSet<String>,
    active: HashSet<ObjectIdentifier>,
    path: Vec<ObjectIdentifier>,
    completed: HashMap<FormContext<'a>, BTreeSet<String>>,
    font: Option<Dict<'a>>,
    mode: i32,
    operations: usize,
    limited: bool,
    // A form nested too deeply to follow. The scan goes on (unlike `limited`), and the flag is
    // never cleared, so a memoized subtree that was cut short can't hide it.
    depth_limited: bool,
}

// Forms inherit text state and missing resource names. Depth and ancestors also affect
// which descendants the depth/cycle guards permit; only identical contexts may share results.
#[derive(Hash, PartialEq, Eq)]
struct FormContext<'a> {
    form: ObjectIdentifier,
    font: Option<&'a [u8]>,
    mode: i32,
    resources: Vec<(&'a [u8], &'a [u8])>,
    ancestors: Vec<ObjectIdentifier>,
}

impl<'a> FontScan<'a> {
    fn content(&mut self, data: &[u8], res: &Resources<'a>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let mut saved = Vec::new();
        let mut ops = UntypedIter::new(data);
        while let Some(op) = ops.next() {
            if self.operations == MAX_OPERATIONS {
                self.limited = true;
                return;
            }
            self.operations += 1;
            let operator: &[u8] = op.operator;
            match operator {
                b"q" => saved.push((self.font.clone(), self.mode)),
                b"Q" => {
                    if let Some(state) = saved.pop() {
                        (self.font, self.mode) = state;
                    }
                }
                b"Tf" => self.font = op.operands().next().cloned().and_then(Object::into_name).and_then(|n| res.get_font(&n)),
                b"Tr" => {
                    if let Some(mode) = op.operands().next().cloned().and_then(|n| i32::try_from(n).ok()) {
                        self.mode = mode;
                    }
                }
                b"Tj" | b"TJ" | b"'" | b"\"" if self.mode != 3 => {
                    if let Some(font) = self.font.as_ref().filter(|f| !font_embedded(f)) {
                        let name = font.get::<Name<'_>>(b"BaseFont").map_or_else(|| "(unnamed)".to_string(), |n| n.as_str().to_string());
                        self.found.insert(name);
                    }
                }
                b"Do" => {
                    let xo = op.operands().last().cloned().and_then(Object::into_name).and_then(|n| res.get_x_object(&n));
                    if let Some(form) = xo.filter(|x| x.dict().get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Form"))
                        && (depth < MAX_DEPTH || {
                            // A form already being scanned is a cycle: it can't hide new content.
                            self.depth_limited |= !self.active.contains(&form.obj_id());
                            false
                        })
                        && self.active.insert(form.obj_id())
                        && self.scan_form(form, res, depth)
                    {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// Scans a form XObject the caller has marked active. Returns `true` when the operation
    /// limit stopped the scan, so the caller must stop too.
    fn scan_form(&mut self, form: Stream<'a>, res: &Resources<'a>, depth: usize) -> bool {
        let mut resources = Vec::new();
        let mut parent = Some(res);
        while let Some(r) = parent {
            resources.push((r.fonts.data(), r.x_objects.data()));
            parent = r.parent();
        }
        let key =
            FormContext { form: form.obj_id(), font: self.font.as_ref().map(Dict::data), mode: self.mode, resources, ancestors: self.path.clone() };
        if let Some(found) = self.completed.get(&key) {
            self.found.extend(found.iter().cloned());
            self.active.remove(&form.obj_id());
            return false;
        }
        let inner = Resources::from_parent(form.dict().get::<Dict<'_>>(b"Resources").unwrap_or_default(), res.clone());
        let state = (self.font.clone(), self.mode);
        let outer_found = std::mem::take(&mut self.found);
        self.path.push(form.obj_id());
        if let Ok(data) = form.decoded() {
            self.content(&data, &inner, depth + 1);
        }
        self.path.pop();
        if !self.limited {
            self.completed.insert(key, self.found.clone());
        }
        self.found.extend(outer_found);
        (self.font, self.mode) = state;
        self.active.remove(&form.obj_id());
        self.limited
    }
}
