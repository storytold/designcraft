use std::collections::HashSet;

use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier};
use hayro_syntax::page::Resources;

use super::MAX_DEPTH;

pub(super) fn unembedded_fonts(pdf: &[u8]) -> Vec<String> {
    let Ok(file) = hayro_syntax::Pdf::new(pdf.to_vec()) else { return Vec::new() };
    let mut found: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, page) in file.pages().iter().enumerate() {
        let mut scan = FontScan::default();
        if let Some(content) = page.page_stream() {
            scan.content(content, page.resources(), 0);
        }
        for name in scan.found {
            match found.iter_mut().find(|(n, _)| *n == name) {
                Some((_, pages)) => pages.push(i + 1),
                None => found.push((name, vec![i + 1])),
            }
        }
    }
    found
        .into_iter()
        .map(|(name, pages)| {
            let list: Vec<String> = pages.iter().take(10).map(|p| p.to_string()).collect();
            let more = if pages.len() > 10 { ", …" } else { "" };
            let s = if pages.len() == 1 { "" } else { "s" };
            format!("the font {name} isn't embedded (page{s} {}{more}): PDF/X-4 needs every font used for visible text embedded", list.join(", "))
        })
        .collect()
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
    found: Vec<String>,
    active: HashSet<ObjectIdentifier>,
    font: Option<Dict<'a>>,
    mode: i32,
}

impl<'a> FontScan<'a> {
    fn content(&mut self, data: &[u8], res: &Resources<'a>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let mut saved = Vec::new();
        let mut ops = UntypedIter::new(data);
        while let Some(op) = ops.next() {
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
                        if !self.found.contains(&name) {
                            self.found.push(name);
                        }
                    }
                }
                b"Do" => {
                    let xo = op.operands().last().cloned().and_then(Object::into_name).and_then(|n| res.get_x_object(&n));
                    if let Some(form) = xo.filter(|x| x.dict().get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Form"))
                        && depth < MAX_DEPTH
                        && self.active.insert(form.obj_id())
                    {
                        let inner = Resources::from_parent(form.dict().get::<Dict<'_>>(b"Resources").unwrap_or_default(), res.clone());
                        let state = (self.font.clone(), self.mode);
                        if let Ok(data) = form.decoded() {
                            self.content(&data, &inner, depth + 1);
                        }
                        (self.font, self.mode) = state;
                        self.active.remove(&form.obj_id());
                    }
                }
                _ => {}
            }
        }
    }
}
