//! Check the shared reader source, including the parser reached through Krilla.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const REPO: &str = "https://github.com/storytold/craft-libs";
const PACKAGES: &[(&str, &str)] = &[
    ("craft-pdf", "0.1.0"),
    ("krilla", "0.8.2"),
    ("hayro", "0.7.1"),
    ("hayro-interpret", "0.7.0"),
    ("hayro-syntax", "0.7.2"),
    ("hayro-jbig2", "0.3.0"),
    ("hayro-cmap", "0.1.0"),
    ("hayro-postscript", "0.1.0"),
];

/// The craft-libs commit the workspace's framework resolves from: the source of `craft-ui`. The PDF backend must resolve
/// from that same commit, so the pinned Krilla and Hayro parsers are the ones the framework ships. A port branch pins an
/// integration branch of craft-libs while the framework pull requests it needs are open, and re-pinning is one edit, so the
/// commit is read from the resolution rather than written here.
fn framework_source(metadata: &Value) -> Result<String, String> {
    let packages = metadata.get("packages").and_then(Value::as_array).ok_or("cargo metadata has no packages")?;
    let mut sources: Vec<&str> = packages
        .iter()
        .filter(|p| p.get("name").and_then(Value::as_str) == Some("craft-ui"))
        .filter_map(|p| p.get("source").and_then(Value::as_str))
        .collect();
    sources.dedup();
    match sources.as_slice() {
        [source] if source.starts_with("git+") && source.contains("craft-libs") => Ok((*source).to_owned()),
        [] => Err("craft-ui: no resolved package from craft-libs".into()),
        found => Err(format!("craft-ui: expected one craft-libs source, found {found:?}")),
    }
}

pub fn run(root: &Path) -> Result<(), String> {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root)
        .args(["metadata", "--locked", "--format-version", "1"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let source = framework_source(&metadata)?;
    validate(&metadata, &source)?;
    println!("PDF backend: facade, Krilla and Hayro resolve once to the framework's craft-libs {source}");
    Ok(())
}

fn validate(metadata: &Value, source: &str) -> Result<(), String> {
    let packages = metadata.get("packages").and_then(Value::as_array).ok_or("cargo metadata has no packages")?;
    debug_assert!(source.contains(REPO));
    for (name, version) in PACKAGES {
        let matching: Vec<_> = packages.iter().filter(|p| p.get("name").and_then(Value::as_str) == Some(name)).collect();
        let [package] = matching.as_slice() else {
            return Err(format!("{name}: expected one resolved package, found {}", matching.len()));
        };
        if package.get("version").and_then(Value::as_str) != Some(version) || package.get("source").and_then(Value::as_str) != Some(source) {
            return Err(format!("{name}: expected {version} from {source}; got {package}"));
        }
    }
    let nodes = metadata.pointer("/resolve/nodes").and_then(Value::as_array).ok_or("cargo metadata has no resolved nodes")?;
    let facade = packages.iter().find(|p| p["name"] == "craft-pdf").ok_or("craft-pdf: missing package")?;
    let node = nodes.iter().find(|n| n["id"] == facade["id"]).ok_or("craft-pdf: missing resolved node")?;
    let features = node["features"].as_array().ok_or("craft-pdf: missing features")?;
    for required in ["read", "interpret", "raster", "write", "write-pdf"] {
        if !features.iter().any(|f| f.as_str() == Some(required)) {
            return Err(format!("craft-pdf: missing required {required} feature"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The synthetic resolution: every reader package, and the framework, from one craft-libs commit.
    const SOURCE: &str =
        "git+https://github.com/storytold/craft-libs?rev=0123456789abcdef0123456789abcdef01234567#0123456789abcdef0123456789abcdef01234567";

    fn metadata() -> Value {
        let mut packages: Vec<Value> = PACKAGES
            .iter()
            .map(|(name, version)| {
                json!({"id":name,"name":name,"version":version,
            "source": SOURCE})
            })
            .collect();
        packages.push(json!({"id": "craft-ui", "name": "craft-ui", "version": "0.1.0", "source": SOURCE}));
        json!({"packages": packages, "resolve": {"nodes": [{"id": "craft-pdf", "features": ["read", "interpret", "raster", "write", "write-pdf"]}]}})
    }

    #[test]
    fn accepts_one_pinned_facade_and_reader() {
        assert!(validate(&metadata(), SOURCE).is_ok());
    }

    #[test]
    fn rejects_unpatched_and_local_reader_sources() {
        for source in [Value::Null, json!("registry+https://github.com/rust-lang/crates.io-index")] {
            let mut m = metadata();
            m["packages"][1]["source"] = source;
            assert!(validate(&m, SOURCE).is_err());
        }
    }

    #[test]
    fn rejects_missing_duplicate_and_wrong_version_packages() {
        for index in 0..PACKAGES.len() {
            let mut m = metadata();
            m["packages"][index]["version"] = json!("99.0.0");
            assert!(validate(&m, SOURCE).is_err());
            let mut m = metadata();
            let duplicate = m["packages"][index].clone();
            m["packages"].as_array_mut().unwrap().push(duplicate);
            assert!(validate(&m, SOURCE).is_err());
            let mut m = metadata();
            m["packages"].as_array_mut().unwrap().remove(index);
            assert!(validate(&m, SOURCE).is_err());
        }
    }

    #[test]
    fn rejects_missing_reader_or_writer_features() {
        for index in 0..5 {
            let mut m = metadata();
            m["resolve"]["nodes"][0]["features"].as_array_mut().unwrap().remove(index);
            assert!(validate(&m, SOURCE).is_err());
        }
        let mut m = metadata();
        m["resolve"] = Value::Null;
        assert!(validate(&m, SOURCE).is_err());
    }

    #[test]
    fn the_anchor_is_the_frameworks_single_craft_libs_commit() {
        assert_eq!(framework_source(&metadata()).unwrap(), SOURCE);
        let mut m = metadata();
        m["packages"].as_array_mut().unwrap().retain(|p| p["name"] != "craft-ui");
        assert!(framework_source(&m).is_err(), "no framework package is an error");
        let mut m = metadata();
        let mut other = m["packages"].as_array().unwrap()[0].clone();
        other["id"] = json!("craft-ui-2");
        other["source"] = json!(
            "git+https://github.com/storytold/craft-libs?rev=ffffffffffffffffffffffffffffffffffffffff#ffffffffffffffffffffffffffffffffffffffff"
        );
        // A second craft-ui from another commit is not a single anchor.
        other["name"] = json!("craft-ui");
        m["packages"].as_array_mut().unwrap().push(other);
        assert!(framework_source(&m).is_err(), "two framework sources are an error");
        // The readers must come from the framework's commit, not another branch.
        let mut m = metadata();
        m["packages"][1]["source"] = json!("git+https://github.com/storytold/craft-libs?branch=main#other");
        assert!(validate(&m, SOURCE).is_err());
    }

    #[test]
    fn rejects_facade_revision_drift_and_malformed_metadata() {
        let mut m = metadata();
        m["packages"][0]["source"] = json!("git+https://github.com/storytold/craft-libs?branch=main#other");
        assert!(validate(&m, SOURCE).is_err());
        assert!(validate(&json!({}), SOURCE).is_err());
    }
}
