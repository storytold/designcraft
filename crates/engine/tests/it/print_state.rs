//! Synthetic print-state diagnostics; no private files or proprietary fonts.
use super::pdf_backend::{evidence, rendered};
use designcraft_engine::Session;
use designcraft_engine::cmd::base64_decode;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width":100,"height":100,"pages":1})).unwrap();
    s.execute("edit.transparencyBlendSpace", &json!({"space":"rgb"})).unwrap();
    for (name, color) in
        [("Cyan", json!([0, 255, 255])), ("Magenta", json!([255, 0, 255])), ("Red", json!([255, 0, 0])), ("Blue", json!([0, 0, 255]))]
    {
        s.execute("swatch.create", &json!({"name":name,"color":color})).unwrap();
    }
    s
}

fn rectangle(s: &mut Session, swatch: &str) {
    s.execute("frame.create", &json!({"rect":[10,10,90,90]})).unwrap();
    s.execute("object.fill", &json!({"swatch":swatch})).unwrap();
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn rgb_multiply_and_alpha_agree_between_screen_and_export() {
    for (name, back, front, opacity, blend, expected) in
        [("multiply", "Cyan", "Magenta", 1.0, "multiply", [0u8, 0, 255]), ("alpha", "Red", "Blue", 0.5, "normal", [128u8, 0, 128])]
    {
        let mut s = session();
        rectangle(&mut s, back);
        rectangle(&mut s, front);
        s.execute("object.opacity", &json!({"opacity":opacity,"blend":blend})).unwrap();
        let mut renderer = designcraft_render::Renderer::new();
        renderer.threads = 0;
        let screen = renderer.render_page(&s.doc().unwrap().doc, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let result = s.execute("file.exportPdf", &json!({})).unwrap();
        let pdf = base64_decode(result["base64"].as_str().unwrap());
        let export = rendered(&pdf);
        for (label, pixel) in [("screen", screen.pixel(50, 50)), ("PDF", export.pixel(50, 50))] {
            for channel in 0..3 {
                assert!(pixel[channel].abs_diff(expected[channel]) <= 3, "{name} {label}: {pixel:?} expected {expected:?}");
            }
        }
        evidence(&format!("print-{name}.pdf"), &pdf);
        evidence(&format!("print-{name}-screen.png"), &screen.to_png());
        evidence(&format!("print-{name}-export.png"), &export.to_png());
    }
}

fn overprint_warnings(kind: &str, enabled: bool, visible: bool, grouped: bool) -> Vec<String> {
    let mut s = session();
    s.execute("swatch.create", &json!({"name":"Process Cyan","color":{"c":100,"m":0,"y":0,"k":0}})).unwrap();
    s.execute("swatch.create", &json!({"name":"Process Magenta","color":{"c":0,"m":100,"y":0,"k":0}})).unwrap();
    rectangle(&mut s, if kind == "fill" { "Process Cyan" } else { "[None]" });
    if kind != "fill" {
        let stroke_kind = match kind {
            "gap" => json!({"kind":"dashed","pattern":[8,8]}),
            "outlineGap" => json!({"kind":"thickThin"}),
            _ => json!({"kind":"solid"}),
        };
        s.execute("object.stroke", &json!({"swatch":"Process Cyan","weight":8,"type":stroke_kind,"gapSwatch":"Process Magenta"})).unwrap();
    }
    let flag = match kind {
        "fill" => "overprintFill",
        "stroke" => "overprintStroke",
        _ => "overprintGap",
    };
    let mut attrs = json!({"nonprinting":!visible});
    attrs[flag] = json!(enabled);
    s.execute("object.attributes", &attrs).unwrap();
    if grouped {
        let painted = s.doc().unwrap().selection.items[0];
        let other = s.execute("frame.create", &json!({"rect":[0,0,2,2]})).unwrap()["id"].clone();
        let g = s.execute("object.group", &json!({"ids":[painted.0,other]})).unwrap()["id"].clone();
        s.execute("object.transparencyGroup", &json!({"ids":[g],"knockout":true})).unwrap();
        let other = s.execute("frame.create", &json!({"rect":[95,95,99,99]})).unwrap()["id"].clone();
        s.execute("object.group", &json!({"ids":[g,other]})).unwrap();
    }
    let result = s.execute("file.exportPdf", &json!({})).unwrap();
    result["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(str::to_string)).collect()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn exported_stroke_and_gap_overprint_are_never_silently_dropped() {
    let mut missing = Vec::new();
    for (kind, grouped) in ["fill", "stroke", "gap", "outlineGap"].into_iter().flat_map(|kind| [(kind, false), (kind, true)]) {
        let warnings = overprint_warnings(kind, true, true, grouped);
        if !warnings.iter().any(|w| w.contains("overprint is not exported yet")) {
            missing.push((kind, grouped));
        }
        for (on, visible) in [(false, true), (true, false)] {
            let warnings = overprint_warnings(kind, on, visible, grouped);
            assert!(!warnings.iter().any(|w| w.contains("overprint")), "inactive {kind}: {warnings:?}");
        }
    }
    assert!(missing.is_empty(), "silent overprint loss: {missing:?}");
    for grouped in [false, true] {
        assert!(!overprint_warnings("inactiveGap", true, true, grouped).iter().any(|w| w.contains("overprint")), "solid strokes have no painted gap");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn nested_knockout_group_preserves_screen_compositing_on_pdf_export() {
    for (depth, transformed, own_blend) in [(0, false, false), (1, false, false), (2, true, false), (0, false, true), (1, false, true)] {
        let mut s = session();
        s.execute("swatch.create", &json!({"name":"Yellow","color":[255,255,0]})).unwrap();
        if transformed || own_blend {
            rectangle(&mut s, "Cyan");
        }
        let mut ids = Vec::new();
        for (rect, swatch) in [(json!([10, 10, 70, 90]), "Yellow"), (json!([40, 10, 90, 90]), "Magenta")] {
            let id = s.execute("frame.create", &json!({"rect":rect})).unwrap()["id"].clone();
            s.execute("object.fill", &json!({"swatch":swatch})).unwrap();
            s.execute("object.opacity", &json!({"opacity":0.5})).unwrap();
            ids.push(id);
        }
        let group = s.execute("object.group", &json!({"ids":ids})).unwrap();
        s.execute("object.transparencyGroup", &json!({"ids":[group["id"]],"knockout":true})).unwrap();
        if own_blend {
            s.execute("object.opacity", &json!({"ids":[group["id"]],"opacity":0.7,"blend":"multiply"})).unwrap();
        }
        let mut outer = group["id"].clone();
        for _ in 0..depth {
            let sibling = s.execute("frame.create", &json!({"rect":[0,0,5,5]})).unwrap();
            outer = s.execute("object.group", &json!({"ids":[outer,sibling["id"]]})).unwrap()["id"].clone();
        }
        if transformed {
            s.execute("transform.scale", &json!({"ids":[outer],"sx":0.8,"sy":0.8})).unwrap();
            s.execute("transform.move", &json!({"ids":[outer],"dx":4,"dy":-2})).unwrap();
            s.execute("object.opacity", &json!({"ids":[outer],"opacity":0.7,"blend":"multiply"})).unwrap();
        }
        s.execute("frame.create", &json!({"rect":[75,75,85,85]})).unwrap();
        s.execute("object.fill", &json!({"swatch":"Blue"})).unwrap();
        let mut renderer = designcraft_render::Renderer::new();
        renderer.threads = 0;
        let screen = renderer.render_page(&s.doc().unwrap().doc, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
        let result = s.execute("file.exportPdf", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before, "export leaves the live document unchanged");
        let pdf = base64_decode(result["base64"].as_str().unwrap());
        let export = rendered(&pdf);
        evidence(&format!("knockout-depth-{depth}-blend-{own_blend}.pdf"), &pdf);
        evidence(&format!("knockout-depth-{depth}-blend-{own_blend}-screen.png"), &screen.to_png());
        evidence(&format!("knockout-depth-{depth}-blend-{own_blend}-export.png"), &export.to_png());
        for (x, y) in [(50, 50), (30, 50), (80, 80)] {
            let a = screen.pixel(x, y);
            let raw = export.pixel(x, y);
            let b = raw.map(|v| ((u32::from(v) * u32::from(raw[3]) + 255 * (255 - u32::from(raw[3])) + 127) / 255) as u8);
            for channel in 0..3 {
                assert!(a[channel].abs_diff(b[channel]) <= 3, "depth={depth} at {x},{y} screen={a:?} PDF={b:?}");
            }
        }
        assert!(
            result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("knockout")),
            "raster fallback is disclosed: {result}"
        );
    }
}
