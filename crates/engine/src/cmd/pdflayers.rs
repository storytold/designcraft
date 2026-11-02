//! Object › Object Layer Options: show or hide the layers (optional content) of a placed PDF.

use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, targets};
use crate::Session;

fn placed_pdf(s: &Session, id: designcraft_doc::ItemId) -> Option<std::sync::Arc<Vec<u8>>> {
    let d = &s.active()?.doc;
    let g = d.item(id)?.graphic()?;
    let a = d.assets.get(&g.asset)?;
    designcraft_render::is_pdf(&a.data).then(|| a.data.clone())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "object.pdfLayers", "Object Layers", [], None, "{id?} → [{name, visible, default}] — the layers of the selected placed PDF", has_selection, |s, p| {
            let id = super::id_param(p, "id").or_else(|| s.active().and_then(|d| d.selection.items.iter().copied().find(|id| placed_pdf(s, *id).is_some()))).ok_or_else(|| bad("object.pdfLayers", "no selection"))?;
            let data = placed_pdf(s, id).ok_or_else(|| bad("object.pdfLayers", "not a placed PDF"))?;
            let item = s.doc()?.doc.item(id);
            let hidden = item.map(|i| i.pdf_hidden_layers.clone()).unwrap_or_default();
            let overridden = item.is_some_and(|i| i.has_pdf_layer_override());
            Ok(Value::Array(
                designcraft_render::pdf_layers(&data)
                    .into_iter()
                    .map(|(name, on)| json!({"visible": if overridden { !hidden.contains(&name) } else { on }, "name": name, "default": on, "overridden": overridden}))
                    .collect(),
            ))
        }),
        cmd!(
            "object.layerOptions",
            "Object Layer Options…",
            ["Object"],
            None,
            "{ids?, hidden: [layer name], useDefaults?: bool} — hide layers; useDefaults:false makes [] explicitly all on; legacy [] restores file defaults",
            has_selection,
            |s, p| {
                let ids: Vec<_> = targets(s, p)?.into_iter().filter(|id| placed_pdf(s, *id).is_some()).collect();
                let hidden: Vec<String> = p
                    .get("hidden")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                let use_defaults = match p.get("useDefaults") {
                    None => hidden.is_empty(),
                    Some(v) => v.as_bool().ok_or_else(|| bad("object.layerOptions", "useDefaults must be a boolean"))?,
                };
                if use_defaults && !hidden.is_empty() {
                    return Err(bad("object.layerOptions", "useDefaults cannot be combined with hidden layers"));
                }
                if ids.is_empty() {
                    return Err(bad("object.layerOptions", "select a placed PDF"));
                }
                // Validate every selected PDF before editing any item or recording undo, so a
                // mixed request stays atomic. Any explicit choice, including all-on
                // (useDefaults:false with no hidden names), needs one actual rewrite per distinct
                // source: a PDF can list readable layers and still fail the rewrite, and a
                // layerless PDF cannot be rewritten at all. Resetting to the file defaults needs no
                // variant and is always allowed.
                if !use_defaults {
                    let mut checked = std::collections::HashSet::new();
                    for id in &ids {
                        let Some(data) = placed_pdf(s, *id) else { continue };
                        if checked.insert(std::sync::Arc::as_ptr(&data)) && designcraft_render::pdf_hide_layers(&data, &hidden).is_none() {
                            return Err(bad(
                                "object.layerOptions",
                                "the selected PDF layers could not be changed (no readable optional-content layers, or an unsupported file); use the file defaults or replace the PDF",
                            ));
                        }
                    }
                }
                s.edit(|d, _| {
                    for id in &ids {
                        if let Some(it) = d.item_mut(*id) {
                            it.pdf_hidden_layers = hidden.clone();
                            it.pdf_layer_override = !use_defaults;
                        }
                    }
                    Ok(json!({"hidden": hidden, "useDefaults": use_defaults}))
                })
            }
        ),
    ]
}

#[cfg(test)]
pub(crate) mod tests;
