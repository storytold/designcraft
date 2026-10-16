//! Check the shared writer source, including transitive users such as krilla-svg.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const REV: &str = "c3487320b2d1de657b860b3c8abb44e1f2b32bd8";
const PACKAGES: &[(&str, &str)] = &[("craft-pdf", "0.1.0"), ("krilla", "0.8.2")];

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
    validate(&metadata)?;
    println!("PDF writer: facade and patched Krilla resolve once to {REV}");
    Ok(())
}

fn validate(metadata: &Value) -> Result<(), String> {
    let packages = metadata.get("packages").and_then(Value::as_array).ok_or("cargo metadata has no packages")?;
    let source = format!("git+https://github.com/storytold/craft-libs?rev={REV}#{REV}");
    for (name, version) in PACKAGES {
        let matching: Vec<_> = packages.iter().filter(|p| p.get("name").and_then(Value::as_str) == Some(name)).collect();
        let [package] = matching.as_slice() else {
            return Err(format!("{name}: expected one resolved package, found {}", matching.len()));
        };
        if package.get("version").and_then(Value::as_str) != Some(version) || package.get("source").and_then(Value::as_str) != Some(source.as_str()) {
            return Err(format!("{name}: expected {version} from {source}; got {package}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata() -> Value {
        json!({"packages": PACKAGES.iter().map(|(name, version)| json!({"name":name,"version":version,
            "source":format!("git+https://github.com/storytold/craft-libs?rev={REV}#{REV}")})).collect::<Vec<_>>()})
    }

    #[test]
    fn accepts_one_pinned_facade_and_writer() {
        assert!(validate(&metadata()).is_ok());
    }

    #[test]
    fn rejects_unpatched_and_local_writer_sources() {
        for source in [Value::Null, json!("registry+https://github.com/rust-lang/crates.io-index")] {
            let mut m = metadata();
            m["packages"][1]["source"] = source;
            assert!(validate(&m).is_err());
        }
    }

    #[test]
    fn rejects_missing_duplicate_and_wrong_version_packages() {
        for index in [0, 1] {
            let mut m = metadata();
            m["packages"][index]["version"] = json!("99.0.0");
            assert!(validate(&m).is_err());
            let mut m = metadata();
            let duplicate = m["packages"][index].clone();
            m["packages"].as_array_mut().unwrap().push(duplicate);
            assert!(validate(&m).is_err());
            let mut m = metadata();
            m["packages"].as_array_mut().unwrap().remove(index);
            assert!(validate(&m).is_err());
        }
    }

    #[test]
    fn rejects_facade_revision_drift_and_malformed_metadata() {
        let mut m = metadata();
        m["packages"][0]["source"] = json!("git+https://github.com/storytold/craft-libs?branch=main#other");
        assert!(validate(&m).is_err());
        assert!(validate(&json!({})).is_err());
    }
}
