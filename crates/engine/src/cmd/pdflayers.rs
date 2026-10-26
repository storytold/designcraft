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
pub(crate) mod tests {
    use serde_json::json;

    use crate::Session;

    /// A one-page PDF (100×100 pt) with two layers: "Blue" (left half) and "Red" (right half).
    pub(crate) fn two_layer_pdf() -> Vec<u8> {
        two_layer_pdf_generation(0)
    }

    fn two_layer_pdf_generation(generation: i32) -> Vec<u8> {
        two_layer_pdf_options(generation, "", "")
    }

    fn two_layer_pdf_options(generation: i32, config: &str, trailer: &str) -> Vec<u8> {
        let content = "/OC /a BDC 0 0 1 rg 0 0 50 100 re f EMC /OC /b BDC 1 0 0 rg 50 0 50 100 re f EMC";
        let objs = [
            format!(
                "<</Type/Catalog/Lang(en-GB)/Metadata 7 0 R/OutputIntents[8 0 R]/Pages 2 0 R/OCProperties<</OCGs[5 0 R 6 0 R]/D<</Order[5 0 R 6 0 R]/Name(Default)/Locked[5 0 R]{config}>>>>>>"
            ),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</Properties<</a 5 0 R/b 6 0 R>>>>/Contents 4 0 R>>".to_string(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
            "<</Type/OCG/Name(Blue)>>".to_string(),
            "<</Type/OCG/Name(Red)>>".to_string(),
            "<</Type/Metadata/Subtype/XML/Length 4>>\nstream\n<x/>\nendstream".to_string(),
            "<</Type/OutputIntent/S/GTS_PDFX/OutputConditionIdentifier(Test)>>".to_string(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} {generation} obj\n{}\nendobj\n", i + 1, o.replace(" 0 R", &format!(" {generation} R"))).as_bytes());
        }
        let x = out.len();
        let mut t = format!("xref\n0 {}\n0000000000 65535 f\r\n", objs.len() + 1);
        for o in offs {
            t.push_str(&format!("{o:010} {generation:05} n\r\n"));
        }
        t.push_str(&format!("trailer\n<</Size {}/Root 1 {generation} R{trailer}>>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1));
        out.extend_from_slice(t.as_bytes());
        out
    }

    #[test]
    fn pdf_layer_update_preserves_catalog_configuration_and_generations() {
        for generation in [0, 1] {
            let original = two_layer_pdf_generation(generation);
            let updated = designcraft_render::pdf_hide_layers(&original, &["Red".into()]).unwrap();
            assert_eq!(designcraft_render::pdf_layers(&updated), vec![("Blue".into(), true), ("Red".into(), false)]);
            let appended = std::str::from_utf8(&updated[original.len()..]).unwrap();
            for token in [
                "/Lang(en-GB)".to_string(),
                format!("/Metadata 7 {generation} R"),
                format!("/OutputIntents[8 {generation} R]"),
                format!("/Locked[5 {generation} R]"),
                "/Name(Default)".into(),
                format!("/Root 1 {generation} R"),
            ] {
                assert!(appended.contains(&token), "missing {token}: {appended}");
            }
            assert!(!appended.contains("9999000"));
            let all_on = designcraft_render::pdf_hide_layers(&updated, &[]).unwrap();
            assert!(designcraft_render::pdf_layers(&all_on).iter().all(|(_, visible)| *visible));
        }
    }

    #[test]
    fn pdf_layer_update_accepts_xref_streams_and_rejects_unsupported_updates() {
        let original = two_layer_pdf();
        let text = std::str::from_utf8(&original).unwrap();
        let offset: usize = text.rsplit_once("startxref").unwrap().1.split_whitespace().next().unwrap().parse().unwrap();
        let mut stream_pdf = original[..offset].to_vec();
        let mut entries = vec![0, 0, 0, 0, 0, 255, 255];
        for number in 1..=9 {
            let at = if number == 9 { offset } else { text.find(&format!("{number} 0 obj")).unwrap() };
            entries.push(1);
            entries.extend_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
            entries.extend_from_slice(&[0, 0]);
        }
        stream_pdf.extend_from_slice(format!("9 0 obj\n<</Type/XRef/Size 10/Root 1 0 R/W[1 4 2]/Length {}>>\nstream\n", entries.len()).as_bytes());
        stream_pdf.extend_from_slice(&entries);
        stream_pdf.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{offset}\n%%EOF\n").as_bytes());
        let updated = designcraft_render::pdf_hide_layers(&stream_pdf, &["Red".into()]).unwrap();
        assert_eq!(designcraft_render::pdf_layers(&updated), vec![("Blue".into(), true), ("Red".into(), false)]);
        for unsupported in ["/Encrypt 7 0 R", "/Encrypt << /Filter /Standard >>", "/XRefStm 0"] {
            let altered = text.replace("/Size 9", &format!("{unsupported}/Size 9"));
            assert!(designcraft_render::pdf_hide_layers(altered.as_bytes(), &["Red".into()]).is_none());
        }
    }

    #[test]
    fn pdf_layer_update_follows_forward_prev_and_rejects_cycles() {
        let original = two_layer_pdf();
        let text = std::str::from_utf8(&original).unwrap();
        let first = original.len();
        let link = |prev: usize| format!("xref\n0 1\n0000000000 65535 f\ntrailer<</Size 9/Root 1 0 R/Prev {prev:010}>>\n");
        let main = first + link(0).len();
        let mut forward = original.clone();
        forward.extend_from_slice(link(main).as_bytes());
        let old: usize = text.rsplit_once("startxref").unwrap().1.split_whitespace().next().unwrap().parse().unwrap();
        forward.extend_from_slice(text[old..].split("startxref").next().unwrap().as_bytes());
        forward.extend_from_slice(format!("startxref\n{first}\n%%EOF\n").as_bytes());
        let updated = designcraft_render::pdf_hide_layers(&forward, &["Red".into()]).unwrap();
        assert_eq!(designcraft_render::pdf_layers(&updated), vec![("Blue".into(), true), ("Red".into(), false)]);
        let mut cyclic = original;
        cyclic.extend_from_slice(link(first).as_bytes());
        cyclic.extend_from_slice(format!("startxref\n{first}\n%%EOF\n").as_bytes());
        assert!(designcraft_render::pdf_hide_layers(&cyclic, &[]).is_none());
    }

    fn add_pdf_revision(bytes: &mut Vec<u8>) {
        let text = std::str::from_utf8(bytes).unwrap();
        let prev: usize = text.rsplit_once("startxref").unwrap().1.split_whitespace().next().unwrap().parse().unwrap();
        let xref = bytes.len();
        bytes.extend_from_slice(
            format!("xref\n0 1\n0000000000 65535 f\r\ntrailer\n<</Size 9/Root 1 0 R/Prev {prev}>>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
    }

    fn place(s: &mut Session, bytes: &[u8], x: f64) -> serde_json::Value {
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(bytes), "name": "layers.pdf", "x": x, "y": 0, "width": 100}))
            .unwrap()["id"]
            .clone()
    }

    fn pixel(s: &Session, x: u32) -> [u8; 4] {
        let mut renderer = designcraft_render::Renderer::new();
        renderer.threads = 0;
        renderer.render_page(&s.doc().unwrap().doc, &s.cache, 0, 1.0, false, &Default::default()).unwrap().pixel(x, 50)
    }

    #[test]
    fn null_encryption_entry_keeps_layer_editing_rendering_and_export_working() {
        for generation in [0, 1] {
            let bytes = two_layer_pdf_options(generation, "", "/Encrypt null");
            let mut s = Session::new();
            s.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
            let id = place(&mut s, &bytes, 0.0);
            s.execute("object.layerOptions", &json!({"ids": [id], "hidden": ["Red"]})).unwrap();
            assert_eq!(s.execute("object.pdfLayers", &json!({"id": id})).unwrap()[1]["visible"], false);
            assert_eq!(pixel(&s, 25), [0, 0, 255, 255]);
            assert_eq!(pixel(&s, 75), [255, 255, 255, 255]);
            s.execute("file.exportPdf", &json!({})).unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(pixel(&s, 75), [255, 0, 0, 255]);
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(pixel(&s, 75), [255, 255, 255, 255]);
        }
    }

    #[test]
    fn unrewritable_layer_choices_are_rejected_atomically_without_an_undo_step() {
        let mut unsupported = two_layer_pdf();
        for _ in 0..32 {
            add_pdf_revision(&mut unsupported);
        }
        assert_eq!(designcraft_render::pdf_layers(&unsupported).len(), 2);
        assert!(designcraft_render::pdf_hide_layers(&unsupported, &["Red".into()]).is_none());
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 100, "pages": 1})).unwrap();
        let good = place(&mut s, &two_layer_pdf(), 0.0);
        let bad = place(&mut s, &unsupported, 100.0);
        s.execute("object.layerOptions", &json!({"ids": [good], "hidden": ["Blue"]})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.doc().unwrap().history.redo.is_empty());
        let before = s.execute("file.serialize", &json!({})).unwrap();
        let undo = s.doc().unwrap().history.undo.len();
        let redo = s.doc().unwrap().history.redo.len();
        for ids in [json!([good, bad]), json!([bad, good])] {
            let err = s.execute("object.layerOptions", &json!({"ids": ids, "hidden": ["Red"]})).unwrap_err();
            assert!(err.to_string().contains("could not be changed"));
            assert_eq!(s.execute("file.serialize", &json!({})).unwrap(), before);
            assert_eq!(s.doc().unwrap().history.undo.len(), undo);
            assert_eq!(s.doc().unwrap().history.redo.len(), redo);
            for id in [&good, &bad] {
                assert_eq!(s.execute("object.pdfLayers", &json!({"id": id})).unwrap()[1]["visible"], true);
            }
        }
        assert_eq!(pixel(&s, 75), [255, 0, 0, 255]);
        assert_eq!(pixel(&s, 175), [255, 0, 0, 255]);
        // A persisted/relinked choice can be unrewritable. Reset must still work.
        let bad_id = serde_json::from_value(bad.clone()).unwrap();
        std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(bad_id).unwrap().pdf_hidden_layers = vec!["Red".into()];
        assert_eq!(pixel(&s, 175), [255, 255, 255, 255], "stale hidden content must not reappear on canvas");
        assert!(s.execute("file.exportPdf", &json!({})).is_err());
        s.execute("object.layerOptions", &json!({"ids": [good, bad], "hidden": []})).unwrap();
        assert_eq!(s.execute("object.pdfLayers", &json!({"id": bad})).unwrap()[1]["visible"], true);
        s.execute("file.exportPdf", &json!({})).unwrap();
    }

    #[test]
    fn explicit_all_on_validates_an_actual_rewrite_before_editing() {
        // 33 xref sections: the layers stay readable, but the visibility rewrite refuses the history.
        let mut unsupported = two_layer_pdf();
        for _ in 0..32 {
            add_pdf_revision(&mut unsupported);
        }
        assert_eq!(designcraft_render::pdf_layers(&unsupported).len(), 2);
        assert!(designcraft_render::pdf_hide_layers(&unsupported, &[]).is_none());
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 100, "pages": 1})).unwrap();
        let good = place(&mut s, &two_layer_pdf(), 0.0);
        let bad = place(&mut s, &unsupported, 100.0);
        let before = s.execute("file.serialize", &json!({})).unwrap();
        let undo = s.doc().unwrap().history.undo.len();
        for ids in [json!([good, bad]), json!([bad, good]), json!([bad])] {
            let err = s.execute("object.layerOptions", &json!({"ids": ids, "hidden": [], "useDefaults": false})).unwrap_err();
            assert!(err.to_string().contains("could not be changed"), "{err}");
            assert_eq!(s.execute("file.serialize", &json!({})).unwrap(), before, "rejected all-on must not edit any target");
            assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        }
        assert_eq!(pixel(&s, 175), [255, 0, 0, 255], "the unsupported source keeps showing its file defaults");
        s.execute("file.exportPdf", &json!({})).unwrap();
        // The supported source alone still accepts the explicit all-on choice.
        s.execute("object.layerOptions", &json!({"ids": [good], "hidden": [], "useDefaults": false})).unwrap();
        assert_eq!(s.execute("object.pdfLayers", &json!({"id": good})).unwrap()[1]["overridden"], true);
        // Resetting to file defaults needs no variant, so it bypasses the rewrite check.
        for reset in [json!({"ids": [good, bad], "hidden": []}), json!({"ids": [good, bad], "useDefaults": true})] {
            s.execute("object.layerOptions", &reset).unwrap();
            for id in [&good, &bad] {
                assert_eq!(s.execute("object.pdfLayers", &json!({"id": id})).unwrap()[1]["overridden"], false);
            }
        }
        // A stale explicit all-on choice that outlived its rewritable source still fails closed.
        let bad_id = serde_json::from_value(bad.clone()).unwrap();
        std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(bad_id).unwrap().pdf_layer_override = true;
        assert_eq!(pixel(&s, 175), [255, 255, 255, 255], "unrewritable explicit choice must not draw the unmodified source");
        assert!(s.execute("file.exportPdf", &json!({})).is_err());
    }

    #[test]
    fn unreadable_non_null_prev_entries_refuse_layer_rewrites() {
        for prev in ["7 0 R", "-1", "false", "<<>>", "[]", "/Unknown", "(unknown)"] {
            let bytes = two_layer_pdf_options(0, "", &format!("/Prev {prev}"));
            assert_eq!(designcraft_render::pdf_layers(&bytes).len(), 2, "reader control: {prev}");
            assert!(designcraft_render::pdf_hide_layers(&bytes, &["Red".into()]).is_none(), "must not truncate unknown history: {prev}");
        }
        for trailer in ["", "/Prev null"] {
            let bytes = two_layer_pdf_options(0, "", trailer);
            assert!(designcraft_render::pdf_hide_layers(&bytes, &["Red".into()]).is_some(), "end-of-chain control: {trailer}");
        }
        let mut revised = two_layer_pdf();
        let prev =
            std::str::from_utf8(&revised).unwrap().rsplit_once("startxref").unwrap().1.split_whitespace().next().unwrap().parse::<usize>().unwrap();
        add_pdf_revision(&mut revised);
        let real_prev = std::str::from_utf8(&revised).unwrap().replace(&format!("/Prev {prev}>>"), &format!("/Prev {prev}.0>>"));
        assert!(real_prev.contains(&format!("/Prev {prev}.0>>")));
        for bytes in [revised, real_prev.into_bytes()] {
            assert!(designcraft_render::pdf_hide_layers(&bytes, &["Red".into()]).is_some(), "readable integer/real history must still work");
        }
    }

    #[test]
    fn fake_null_trailer_cannot_bypass_hybrid_rejection() {
        for real_guard in ["", "/XRefStm 0"] {
            let bytes = two_layer_pdf_options(0, "", real_guard);
            let text = std::str::from_utf8(&bytes).unwrap();
            for fake in ["/Encrypt null", ""] {
                let comment = format!("% trailer <</Size 9/Root 1 0 R{fake}>>\n");
                let inside = text.replacen("xref\n", &format!("xref\n{comment}"), 1);
                assert!(inside.contains(&format!("xref\n{comment}")));
                // Repair-only comments inside a table are always refused, guard or not.
                assert_eq!(designcraft_render::pdf_layers(inside.as_bytes()).len(), 2);
                assert!(designcraft_render::pdf_hide_layers(inside.as_bytes(), &["Red".into()]).is_none());
                // Before xref or after the dictionary, the real trailer decides.
                let leading = text.replacen("xref\n", &format!("{comment}xref\n"), 1);
                let trailing = text.replacen("startxref\n", &format!("{comment}startxref\n"), 1);
                for source in [leading, trailing] {
                    assert_eq!(designcraft_render::pdf_layers(source.as_bytes()).len(), 2);
                    assert_eq!(designcraft_render::pdf_hide_layers(source.as_bytes(), &["Red".into()]).is_some(), real_guard.is_empty());
                }
            }
        }
    }

    #[test]
    fn content_all_still_measures_layerless_and_stub_properties() {
        use designcraft_doc::PdfCrop;
        for props in ["", "/OCProperties null", "/OCProperties<<>>", "/OCProperties<</OCGs[]/D<<>>>>"] {
            let content = "0 0 1 rg 0 0 50 100 re f";
            let objs = [
                format!("<</Type/Catalog/Pages 2 0 R{props}>>"),
                "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
                "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 4 0 R>>".into(),
                format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
            ];
            let mut bytes = b"%PDF-1.7\n".to_vec();
            let mut offsets = Vec::new();
            for (i, body) in objs.iter().enumerate() {
                offsets.push(bytes.len());
                bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
            }
            let xref = bytes.len();
            bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
            for at in offsets {
                bytes.extend_from_slice(format!("{at:010} 00000 n\r\n").as_bytes());
            }
            bytes.extend_from_slice(format!("trailer\n<</Size 5/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
            assert!(designcraft_render::pdf_layers(&bytes).is_empty());
            assert_eq!(designcraft_render::pdf_crop_box(&bytes, 0, PdfCrop::ContentAll).unwrap().1, (50.0, 100.0), "{props}");
        }
    }

    #[test]
    fn content_all_keeps_the_whole_page_when_layer_rewriting_is_unsupported() {
        use designcraft_doc::PdfCrop;
        let mut bytes = two_layer_pdf_options(0, "/BaseState/OFF/ON[5 0 R]", "");
        assert_eq!(designcraft_render::pdf_crop_box(&bytes, 0, PdfCrop::ContentVisible).unwrap().1, (50.0, 100.0));
        assert_eq!(designcraft_render::pdf_crop_box(&bytes, 0, PdfCrop::ContentAll).unwrap().1, (100.0, 100.0));
        for _ in 0..32 {
            add_pdf_revision(&mut bytes);
        }
        assert!(designcraft_render::pdf_hide_layers(&bytes, &[]).is_none());
        assert_eq!(designcraft_render::pdf_crop_box(&bytes, 0, PdfCrop::ContentVisible).unwrap().1, (50.0, 100.0));
        assert_eq!(designcraft_render::pdf_crop_box(&bytes, 0, PdfCrop::ContentAll).unwrap(), ([0.0, 0.0, 1.0, 1.0], (100.0, 100.0)));
    }

    #[test]
    fn placed_pdf_layers_hide() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let pdf = two_layer_pdf();
        let r = s
            .execute("file.place", &json!({"base64": super::super::file::base64_encode(&pdf), "name": "art.pdf", "x": 100, "y": 100, "width": 100}))
            .unwrap();
        let id = r["id"].clone();
        let l = s.execute("object.pdfLayers", &json!({"id": id})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 2);
        assert_eq!(l[0]["name"], "Blue");
        let px = |s: &Session, x: u32| {
            let d = s.doc().unwrap().doc.clone();
            let mut rr = designcraft_render::Renderer::new();
            rr.threads = 0;
            rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap().pixel(x, 150)
        };
        assert!(px(&s, 125)[2] > 200 && px(&s, 175)[0] > 200, "both layers show");
        s.execute("object.layerOptions", &json!({"ids": [id], "hidden": ["Red"]})).unwrap();
        let right = px(&s, 175);
        assert!(right[0] > 200 && right[1] > 200 && right[2] > 200, "the red layer is hidden: {right:?}");
        assert!(px(&s, 125)[2] > 200, "blue still shows");
        assert_eq!(s.execute("object.pdfLayers", &json!({"id": id})).unwrap()[1]["visible"], false);
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(r["warnings"].as_array().unwrap().is_empty(), "{r}");
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        let image = designcraft_render::render_pdf_page(&bytes, 0, 792).unwrap();
        let width = usize::from(image.width());
        assert!(image.data()[150 * width + 125].a > 200);
        assert_eq!(image.data()[150 * width + 175].a, 0);
    }
    #[test]
    fn placed_pdf_all_on_is_distinct_from_defaults_and_survives_exports() {
        let pdf = designcraft_render::pdf_hide_layers(&two_layer_pdf(), &["Red".into()]).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s
            .execute("file.place", &json!({"base64": super::super::file::base64_encode(&pdf), "name":"off.pdf", "x":100,"y":100,"width":100}))
            .unwrap()["id"]
            .clone();
        let visible = |s: &mut Session| s.execute("object.pdfLayers", &json!({"id": id})).unwrap()[1]["visible"].as_bool().unwrap();
        let pixel = |s: &Session| {
            let mut rr = designcraft_render::Renderer::new();
            rr.threads = 0;
            rr.render_page(&s.doc().unwrap().doc, &s.cache, 0, 1.0, false, &Default::default()).unwrap().pixel(175, 150)
        };
        assert!(!visible(&mut s));
        assert!(pixel(&s)[1] > 200, "red hidden by default");
        s.execute("object.layerOptions", &json!({"ids":[id], "hidden":[], "useDefaults":false})).unwrap();
        assert!(visible(&mut s));
        assert!(pixel(&s)[0] > 200 && pixel(&s)[1] < 40, "all-on renders red");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!visible(&mut s));
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(visible(&mut s));
        let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
        let roundtrip = designcraft_format::load(&bytes).unwrap();
        let item = roundtrip.item(designcraft_doc::ItemId(id.as_u64().unwrap())).unwrap();
        assert!(item.pdf_layer_override && item.pdf_hidden_layers.is_empty());
        let exported = s.execute("file.exportPdf", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(exported["base64"].as_str().unwrap());
        let raster = designcraft_render::render_pdf_page(&bytes, 0, 842).unwrap();
        let px = raster.data().get(150 * usize::from(raster.width()) + 175).unwrap();
        assert!(px.r > 200 && px.g < 40, "PDF export retains red: {px:?}");
        let idml = s.execute("file.exportIdml", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(idml["base64"].as_str().unwrap());
        let imported = designcraft_idml::import_idml(&bytes).unwrap();
        let pdf_asset = imported.assets.values().find(|a| designcraft_render::is_pdf(&a.data)).unwrap();
        assert!(designcraft_render::pdf_layers(&pdf_asset.data).iter().all(|(_, on)| *on));
        assert!(s.execute("file.exportIdml", &json!({"embedImages":false})).is_err());
        assert_eq!(s.doc().unwrap().doc.assets.values().find(|a| a.name == "off.pdf").unwrap().data.as_slice(), pdf);
        s.execute("object.layerOptions", &json!({"ids":[id],"hidden":[]})).unwrap();
        assert!(!visible(&mut s), "legacy empty list still resets defaults");
        s.execute("object.layerOptions", &json!({"ids":[id],"hidden":[],"useDefaults":false})).unwrap();
        s.execute("object.layerOptions", &json!({"ids":[id],"useDefaults":true})).unwrap();
        assert!(!visible(&mut s), "explicit reset restores defaults");
    }
    #[test]
    fn parent_page_choices_reach_idml_variants_and_raster_export() {
        // Red is hidden by the file itself, so "show every layer" is a visible difference.
        let pdf = designcraft_render::pdf_hide_layers(&two_layer_pdf(), &["Red".into()]).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
        let id = s
            .execute(
                "file.place",
                &json!({"base64": super::super::file::base64_encode(&pdf), "name": "on-parent.pdf", "spread": {"kind": "parent", "index": 0}, "x": 0.0, "y": 0.0, "width": 100}),
            )
            .unwrap()["id"]
            .clone();
        let item = designcraft_doc::ItemId(id.as_u64().unwrap());
        assert!(
            matches!(s.doc().unwrap().doc.find(item).unwrap().spread, designcraft_doc::SpreadRef::Parent(0)),
            "the placement sits on a parent page"
        );
        s.execute("object.layerOptions", &json!({"ids": [id], "hidden": [], "useDefaults": false})).unwrap();
        // Embedded IDML: the parent-page placement gets its own variant, like a document one.
        let idml = s.execute("file.exportIdml", &json!({})).unwrap();
        let imported = designcraft_idml::import_idml(&super::super::file::base64_decode(idml["base64"].as_str().unwrap())).unwrap();
        let asset = imported.assets.values().find(|a| designcraft_render::is_pdf(&a.data)).unwrap();
        assert!(designcraft_render::pdf_layers(&asset.data).iter().all(|(_, on)| *on), "the embedded copy shows every layer");
        // An external-only copy would keep the original file's defaults: the choice is rejected.
        let err = s.execute("file.exportIdml", &json!({"embedImages": false})).unwrap_err().to_string();
        assert!(err.contains("embedded images"), "{err}");
        // PDF/A-2b rasterises a parent-page placement with a choice, warnings included.
        let a2b = s.execute("file.exportPdf", &json!({"standard": "a2b"})).unwrap();
        assert!(a2b["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("exported as images"))), "{a2b}");
    }

    /// An object anchored in a story is a placement too: the IDML copy gets a variant that shows
    /// every layer, an external-only copy is refused, and PDF/A-2b rasterises it, rather than each
    /// quietly falling back to the file's defaults.
    #[test]
    fn an_anchored_placement_keeps_its_layer_choice_in_saves_and_exports() {
        use designcraft_doc::{AnchoredObject, Content, Graphic, Item, Shape, SpreadRef};
        use designcraft_geom::{Affine, Rect, shapes};

        let pdf = designcraft_render::pdf_hide_layers(&two_layer_pdf(), &["Red".into()]).unwrap();
        let mut d = designcraft_doc::Document::new(&Default::default());
        let asset = designcraft_doc::AssetId(d.alloc());
        d.add_asset(designcraft_doc::Asset {
            id: asset,
            name: "anchored.pdf".into(),
            mime: "application/pdf".into(),
            data: std::sync::Arc::new(pdf),
            ..Default::default()
        });
        let mut item =
            Item::new(designcraft_doc::ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0)));
        item.pdf_layer_override = true;
        item.content = Content::Graphic(Graphic {
            asset,
            size: (100.0, 100.0),
            xf: Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
            layer_options: None,
        });
        let layer = d.default_layer();
        let (_, story) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 80.0, 80.0), layer, "Body", Default::default()).unwrap();
        d.story_mut(story).unwrap().insert_object(0, AnchoredObject { item, position: Default::default() });
        assert!(d.has_pdf_layer_overrides(), "the anchored placement carries the choice");
        let mut s = Session::new();
        s.add_document(crate::DocState::new(d, None));
        // Embedded IDML carries a variant that shows every layer.
        let idml = s.execute("file.exportIdml", &json!({})).unwrap();
        let imported = designcraft_idml::import_idml(&super::super::file::base64_decode(idml["base64"].as_str().unwrap())).unwrap();
        let copies: Vec<_> = imported.assets.values().filter(|a| designcraft_render::is_pdf(&a.data)).collect();
        assert!(!copies.is_empty(), "the anchored placement is exported");
        assert!(
            copies.iter().all(|a| designcraft_render::pdf_layers(&a.data).iter().all(|(_, on)| *on)),
            "the embedded copy must not fall back to the file's defaults"
        );
        // An external-only copy would keep the original file's defaults.
        assert!(s.execute("file.exportIdml", &json!({"embedImages": false})).is_err());
        // PDF/A-2b rasterises a placement with a choice, warnings included.
        let a2b = s.execute("file.exportPdf", &json!({"standard": "a2b"})).unwrap();
        assert!(a2b["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("exported as images"))), "{a2b}");
    }

    /// Relinking a placed PDF to an image leaves the item's layer fields behind. They are no longer a
    /// placed-PDF choice: the package report asks for a graphic whose asset is a PDF, the same thing
    /// the export walks ask for, so it does not name the stale field, and exports run unaffected.
    #[test]
    fn a_stale_choice_on_a_non_pdf_placement_is_not_a_placed_pdf_choice() {
        let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
        let id = s
            .execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "photo.png", "x": 0, "y": 0, "width": 100}))
            .unwrap()["id"]
            .clone();
        let item = designcraft_doc::ItemId(id.as_u64().unwrap());
        // The state a relink from a PDF leaves behind: fields set, asset no longer a PDF.
        std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(item).unwrap().pdf_hidden_layers = vec!["Red".into()];
        let d = &s.doc().unwrap().doc;
        assert!(d.item(item).unwrap().has_pdf_layer_override(), "the stale field is on the item");
        assert!(d.has_pdf_layer_overrides(), "the field is there");
        assert!(!super::super::interchange::has_applicable_pdf_layer_overrides(d), "but it is not a placed-PDF choice");
        // Nothing is dropped: the field is not a choice any export applies.
        s.execute("file.exportIdml", &json!({})).unwrap();
        s.execute("file.exportPdf", &json!({"standard": "a2b"})).unwrap();
        s.execute("file.exportPdf", &json!({})).unwrap();
    }

    #[test]
    fn layerless_pdf_rejects_overrides_without_breaking_idml_export() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let pdf = s.execute("file.exportPdf", &json!({})).unwrap()["base64"].clone();
        let id = s.execute("file.place", &json!({"base64":pdf,"name":"plain.pdf","x":0,"y":0,"width":100})).unwrap()["id"].clone();
        let layered = s
            .execute(
                "file.place",
                &json!({"base64":super::super::file::base64_encode(&two_layer_pdf()),"name":"layered.pdf","x":120,"y":0,"width":100}),
            )
            .unwrap()["id"]
            .clone();
        let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
        assert!(s.execute("object.layerOptions", &json!({"ids":[layered,id],"hidden":[],"useDefaults":false})).is_err());
        assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
        assert!(s.execute("file.exportIdml", &json!({})).is_ok());
        s.execute("object.layerOptions", &json!({"ids":[id],"hidden":[],"useDefaults":true})).unwrap();
        assert!(!s.doc().unwrap().doc.item(designcraft_doc::ItemId(id.as_u64().unwrap())).unwrap().has_pdf_layer_override());
    }

    #[test]
    fn pdf_layer_options_ignore_non_pdf_targets_in_mixed_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let placed = s
            .execute(
                "file.place",
                &json!({"base64": super::super::file::base64_encode(&two_layer_pdf()), "name":"art.pdf", "x":100,"y":100,"width":100}),
            )
            .unwrap();
        let other = s.execute("frame.create", &json!({"rect":[0,0,20,20]})).unwrap();
        for ids in [json!([other["id"], placed["id"]]), json!([placed["id"], other["id"]])] {
            s.execute("selection.set", &json!({"ids":ids})).unwrap();
            assert_eq!(s.execute("object.pdfLayers", &json!({})).unwrap().as_array().unwrap().len(), 2);
        }
        assert!(s.execute("object.pdfLayers", &json!({"id":other["id"]})).is_err(), "explicit non-PDF id remains an error");
        s.execute("object.layerOptions", &json!({"ids":[placed["id"],other["id"]],"hidden":[],"useDefaults":false})).unwrap();
        assert!(!s.doc().unwrap().doc.item(designcraft_doc::ItemId(other["id"].as_u64().unwrap())).unwrap().has_pdf_layer_override());
        assert!(s.execute("object.layerOptions", &json!({"ids":[placed["id"]],"hidden":["Red"],"useDefaults":true})).is_err());
        assert!(s.execute("object.layerOptions", &json!({"ids":[placed["id"]],"useDefaults":"false"})).is_err());
    }
}
