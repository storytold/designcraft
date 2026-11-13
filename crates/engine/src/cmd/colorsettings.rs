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

/// An ICC profile installed on this machine, not registered until it is loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct InstalledProfile {
    pub name: String,
    pub kind: cms::ProfileKind,
    pub path: std::path::PathBuf,
}

/// The installed profiles as last scanned: the scan reads every file in the profile folders, so
/// it runs once per session (or when asked again with `refresh`).
static INSTALLED: std::sync::Mutex<Option<std::sync::Arc<Vec<InstalledProfile>>>> = std::sync::Mutex::new(None);

/// The RGB and CMYK ICC profiles installed on this machine (`cms::installed_profile_dirs`, two
/// folder levels deep), by name: scanned on first use and kept, rescanned with `refresh`. Read
/// through the checked reader; nothing is registered; a damaged or oversized file is skipped.
/// Empty on the web.
pub fn installed_profiles(refresh: bool) -> std::sync::Arc<Vec<InstalledProfile>> {
    let mut cached = INSTALLED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !refresh && let Some(list) = cached.as_ref() {
        return list.clone();
    }
    let list = std::sync::Arc::new(scan_installed());
    *cached = Some(list.clone());
    list
}

#[cfg(target_arch = "wasm32")]
fn scan_installed() -> Vec<InstalledProfile> {
    Vec::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn scan_installed() -> Vec<InstalledProfile> {
    const MAX_BYTES: u64 = 32 << 20;
    fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<InstalledProfile>) {
        let Ok(entries) = crate::io::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            if !matches!(ext.as_deref(), Some("icc" | "icm")) {
                // A folder is walked one level down; anything else is skipped.
                if depth > 0 {
                    walk(&path, depth - 1, out);
                }
                continue;
            }
            let Ok(bytes) = crate::io::read_capped(&path, MAX_BYTES) else { continue };
            if let Ok(info) = cms::inspect_icc(&bytes)
                && info.kind != cms::ProfileKind::Gray
            {
                out.push(InstalledProfile { name: info.name, kind: info.kind, path });
            }
        }
    }
    let mut out = Vec::new();
    for dir in cms::installed_profile_dirs() {
        walk(&dir, 1, &mut out);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.path.cmp(&b.path)));
    out.dedup_by(|a, b| a.name == b.name);
    out
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
        cmd!(query "color.installedProfiles", "Installed Profiles", [], None,
        "{refresh?: bool (scan the folders again; the first call scans, later ones reuse it)} → {profiles: [{name, kind: rgb|cmyk, path, loaded}]} — the RGB and CMYK ICC profiles in the platform's profile folders (system, user, Adobe); none is registered until color.loadProfile loads it",
        always, |_s, p| {
            let refresh = p.get("refresh").and_then(Value::as_bool).unwrap_or(false);
            let known: Vec<String> = cms::profiles().into_iter().map(|i| i.name).collect();
            let list = installed_profiles(refresh);
            let profiles: Vec<Value> = list
                .iter()
                .map(|i| json!({"name": i.name, "kind": i.kind, "path": i.path.to_string_lossy(), "loaded": known.contains(&i.name)}))
                .collect();
            Ok(json!({"profiles": profiles}))
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

#[cfg(test)]
mod installed_tests {
    use serde_json::json;

    use crate::Session;

    /// The scan runs once and is reused; it never registers anything and never fails on this
    /// machine's folders.
    #[test]
    fn installed_profiles_are_scanned_once_and_not_registered() {
        let before = designcraft_color::cms::profiles().len();
        let first = super::installed_profiles(false);
        let again = super::installed_profiles(false);
        assert!(std::sync::Arc::ptr_eq(&first, &again), "cached");
        assert!(first.iter().all(|p| p.kind != designcraft_color::cms::ProfileKind::Gray));
        assert_eq!(designcraft_color::cms::profiles().len(), before);
        let mut s = Session::new();
        let r = s.execute("color.installedProfiles", &json!({})).unwrap();
        assert_eq!(r["profiles"].as_array().map(Vec::len), Some(first.len()));
        let fresh = super::installed_profiles(true);
        assert!(!std::sync::Arc::ptr_eq(&first, &fresh), "rescanned");
    }
}
