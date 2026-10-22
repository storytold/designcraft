//! Edit › Color Settings: the RGB and CMYK working spaces, conversion intent and black-point
//! compensation (process-wide), and loading ICC profiles.

use designcraft_color::cms::{self, ColorSettings, Intent};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, str_param};
use crate::Result;

fn state() -> Value {
    let st = cms::active_settings();
    json!({"rgb": st.rgb, "cmyk": st.cmyk, "intent": st.intent.id(), "bpc": st.bpc, "profiles": cms::profiles()})
}

fn set(p: &Value) -> Result<Value> {
    let mut st: ColorSettings = cms::active_settings();
    if let Some(v) = str_param(p, "rgb") {
        st.rgb = v.to_string();
    }
    if let Some(v) = str_param(p, "cmyk") {
        st.cmyk = v.to_string();
    }
    if let Some(v) = str_param(p, "intent") {
        st.intent = Intent::parse(v).ok_or_else(|| bad("color.settings", format!("unknown intent `{v}`")))?;
    }
    if let Some(v) = p.get("bpc").and_then(Value::as_bool) {
        st.bpc = v;
    }
    cms::set_active(&st).map_err(|e| bad("color.settings", e.to_string()))?;
    Ok(state())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "color.settings",
            "Color Settings",
            [],
            None,
            "{rgb?: working RGB profile, cmyk?: working CMYK profile, intent?: perceptual|relative|saturation|absolute, bpc?: bool} → {rgb, cmyk, intent, bpc, profiles}",
            always,
            |_s, p| set(p)
        ),
        cmd!(noundo "color.loadProfile", "Load Profile…", [], None, "{path} — an ICC profile (.icc/.icm) to use as a working space or proof target", always, |_s, p| {
            let path = str_param(p, "path").ok_or_else(|| bad("color.loadProfile", "`path` required"))?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let bytes = crate::io::read_file(path).map_err(|e| bad("color.loadProfile", e))?;
                let info = cms::register_icc(&bytes, None).map_err(|e| bad("color.loadProfile", e.to_string()))?;
                Ok(json!(info))
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = path;
                Err(bad("color.loadProfile", "no file system on the web"))
            }
        }),
        cmd!(query "color.convert", "Convert Color", [], None, "{color: {model: rgb, r, g, b} | {model: cmyk, c, m, y, k} | {model: gray, k}, to: rgb|cmyk|gray|lab} — through the working spaces", always, |_s, p| {
            let c: designcraft_color::Color = serde_json::from_value(p.get("color").cloned().unwrap_or_default()).map_err(|e| bad("color.convert", e.to_string()))?;
            let a = cms::active();
            let intent = a.settings().intent;
            Ok(match str_param(p, "to").unwrap_or("rgb") {
                "lab" => {
                    let l = a.lab(&c);
                    json!({"l": l.l, "a": l.a, "b": l.b, "outOfGamut": a.out_of_gamut(&c)})
                }
                "cmyk" => json!(a.convert(&c, cms::Model::Cmyk, intent)),
                "gray" => json!(a.convert(&c, cms::Model::Gray, intent)),
                _ => json!(a.convert(&c, cms::Model::Rgb, intent)),
            })
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn color_settings_report_convert_and_validate() {
        // The settings are process-wide: this test only re-applies the current ones, so tests
        // rendering in parallel aren't affected.
        let mut s = Session::new();
        let before = s.execute("color.settings", &json!({})).unwrap();
        assert!(before["profiles"].as_array().unwrap().len() >= 6);
        let cyan = json!({"color": {"model": "cmyk", "c": 1.0, "m": 0.0, "y": 0.0, "k": 0.0}, "to": "rgb"});
        let rgb = s.execute("color.convert", &cyan).unwrap();
        assert_eq!(rgb["model"], "rgb");
        assert!(rgb["r"].as_f64().unwrap() < 0.3 && rgb["b"].as_f64().unwrap() > 0.7, "{rgb}");
        let lab = s.execute("color.convert", &json!({"color": {"model": "rgb", "r": 0.0, "g": 1.0, "b": 0.0}, "to": "lab"})).unwrap();
        assert_eq!(lab["outOfGamut"], true, "pure RGB green is outside the press gamut");
        let again = s
            .execute("color.settings", &json!({"rgb": before["rgb"], "cmyk": before["cmyk"], "intent": before["intent"], "bpc": before["bpc"]}))
            .unwrap();
        assert_eq!(again["cmyk"], before["cmyk"]);
        assert!(s.execute("color.settings", &json!({"cmyk": "Nope"})).is_err());
        assert!(s.execute("color.settings", &json!({"intent": "sideways"})).is_err());
        assert_eq!(s.execute("color.settings", &json!({})).unwrap()["cmyk"], before["cmyk"], "a failed change leaves the settings");
    }
}
