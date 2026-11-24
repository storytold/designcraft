//! Bake the contributor credits into the binary as static Rust tables, and optionally embed
//! shared `craft-i18n` interface catalogs from `CRAFT_I18N_DIR`. Nothing is read at run time.
//!
//! - contributors/contributors.json: commit stats per GitHub username, with a mark for whose people.toml
//!   entry is verified.
//! - contributors/people.toml: the names people chose for themselves. A name is used only when the
//!   JSON says the entry was verified (`people_entry`).
//! - `CRAFT_I18N_DIR`: optional absolute path to a `storytold/craft-i18n` checkout. Unset, the
//!   generated `CRAFT_I18N` table is empty and the app falls back to its built-in tables/English;
//!   with `CRAFT_I18N_REQUIRED=1`, a missing or invalid checkout is a build error.
//!
//! A missing or malformed credits file gives empty tables (or no names) and a warning.

use std::fmt::Write as _;

use serde_json::Value;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contributors");
    let json = read(&dir.join("contributors.json"), "the About window shows no contributors", |t| {
        serde_json::from_str::<Value>(t).map_err(|e| e.to_string())
    })
    .unwrap_or(Value::Null);
    let people = read(&dir.join("people.toml"), "contributors are shown by username only", |t| t.parse::<toml::Table>().map_err(|e| e.to_string()))
        .unwrap_or_default();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("credits.rs");
    if let Err(e) = std::fs::write(&out, generate(&json, &people)) {
        println!("cargo::warning=writing {}: {e}", out.display());
    }
    let i18n_out = out.with_file_name("craft_i18n.rs");
    if let Err(e) = std::fs::write(&i18n_out, craft_i18n()) {
        println!("cargo::error=writing {}: {e}", i18n_out.display());
    }
}

/// The DesignCraft catalog filename under `locales/<locale>/` in `storytold/craft-i18n`.
const APP_CATALOG: &str = "designcraft.tsv";

/// Generates `CRAFT_I18N` from `CRAFT_I18N_DIR` (`storytold/craft-i18n` checkout).
fn craft_i18n() -> String {
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_REQUIRED");
    let required = std::env::var("CRAFT_I18N_REQUIRED").is_ok_and(|v| v.trim() == "1");
    let entries = match std::env::var_os("CRAFT_I18N_DIR").filter(|d| !d.is_empty()) {
        // Not canonicalized: on Windows that yields `\\?\C:\…`, which `include_str!` cannot read.
        Some(dir) => match locales(&std::path::PathBuf::from(dir).join("locales")) {
            Ok(entries) => entries,
            Err(e) => {
                let level = if required { "error" } else { "warning" };
                println!("cargo::{level}=CRAFT_I18N_DIR: {e}; using built-in catalogs only");
                Vec::new()
            }
        },
        None => {
            if required {
                println!("cargo::error=CRAFT_I18N_REQUIRED=1 but CRAFT_I18N_DIR is unset");
            }
            Vec::new()
        }
    };
    let mut o = String::from("/// (locale, shared catalog, DesignCraft catalog), one entry per embedded locale; `\"\"` when absent.\n");
    o.push_str("pub static CRAFT_I18N: &craft_i18n::Embedded = &[\n");
    let mut sv_app = None;
    let mut sv_shared = None;
    for (code, shared, app) in entries {
        if code == "sv" {
            sv_shared = shared.clone();
            sv_app = app.clone();
        }
        let shared = shared.map_or_else(|| "\"\"".to_string(), |p| format!("include_str!({p:?})"));
        let app = app.map_or_else(|| "\"\"".to_string(), |p| format!("include_str!({p:?})"));
        let _ = writeln!(o, "    ({code:?}, {shared}, {app}),");
    }
    o.push_str("];\n");
    match sv_app {
        Some(p) => {
            let _ = writeln!(o, "pub(crate) static CRAFT_I18N_SV_APP: &craft_i18n::Embedded = &[(\"sv\", \"\", include_str!({p:?}))];");
        }
        None => {
            o.push_str("pub(crate) static CRAFT_I18N_SV_APP: &craft_i18n::Embedded = &[];\n");
        }
    }
    match sv_shared {
        Some(p) => {
            let _ = writeln!(o, "pub(crate) static CRAFT_I18N_SV_SHARED: &craft_i18n::Embedded = &[(\"sv\", include_str!({p:?}), \"\")];");
        }
        None => {
            o.push_str("pub(crate) static CRAFT_I18N_SV_SHARED: &craft_i18n::Embedded = &[];\n");
        }
    }
    o
}

type LocaleEntry = (String, Option<std::path::PathBuf>, Option<std::path::PathBuf>);

/// Reads `locales/` under a `craft-i18n` checkout for locales that have `designcraft.tsv` (and
/// `sv` if `common.tsv` is present before `designcraft.tsv` lands).
fn locales(dir: &std::path::Path) -> Result<Vec<LocaleEntry>, String> {
    if !dir.is_absolute() {
        return Err(format!("{} is not an absolute path", dir.display()));
    }
    println!("cargo::rerun-if-changed={}", dir.display());
    let list = std::fs::read_dir(dir).map_err(|e| format!("{} is not a craft-i18n checkout ({e})", dir.display()))?;
    let mut out = Vec::new();
    for entry in list.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(code) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        let shared = path.join("common.tsv");
        let app = path.join(APP_CATALOG);
        let shared = shared.is_file().then_some(shared);
        let app = app.is_file().then_some(app);
        if app.is_none() && !(code == "sv" && shared.is_some()) {
            continue;
        }
        println!("cargo::rerun-if-changed={}", path.display());
        if let Some(p) = &shared {
            println!("cargo::rerun-if-changed={}", p.display());
        }
        if let Some(p) = &app {
            println!("cargo::rerun-if-changed={}", p.display());
        }
        out.push((code, shared, app));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn read<T>(path: &std::path::Path, fallback: &str, parse: impl Fn(&str) -> Result<T, String>) -> Option<T> {
    println!("cargo::rerun-if-changed={}", path.display());
    let r = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| parse(&t));
    if let Err(e) = &r {
        println!("cargo::warning={}: {e}; {fallback}", path.display());
    }
    r.ok()
}

/// A non-empty string field of someone's people.toml entry, as a Rust `Option<&str>` literal.
fn name(v: Option<&str>) -> String {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        Some(x) => format!("Some({x:?})"),
        None => "None".into(),
    }
}

fn s(v: &Value, k: &str) -> String {
    format!("{:?}", v.get(k).and_then(Value::as_str).unwrap_or(""))
}

fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn rows(json: &Value, k: &str) -> Vec<Value> {
    json.get(k).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn generate(json: &Value, people: &toml::Table) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "pub static TOTAL_COMMITS: u64 = {};", n(json, "commits"));
    let _ = writeln!(o, "pub static CONTRIBUTORS: &[Contributor] = &[");
    for c in rows(json, "contributors") {
        let login = c.get("login").and_then(Value::as_str).unwrap_or("");
        if login.is_empty() {
            continue;
        }
        // Names only from a verified, self-chosen people.toml entry.
        let entry = people.get("people").and_then(|p| p.get(login)).filter(|_| c.get("people_entry").and_then(Value::as_bool) == Some(true));
        let field = |k: &str| entry.and_then(|e| e.get(k)).and_then(toml::Value::as_str);
        let synced = entry.and_then(|e| e.get("sync_github_name")).and_then(toml::Value::as_bool) == Some(true);
        let display = field("display_name")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| if synced { c.get("github_name").and_then(Value::as_str) } else { None });
        let _ = writeln!(
            o,
            "    Contributor {{ login: {login:?}, display_name: {}, real_name: {}, prs: {}, commits: {}, lines_added: {}, lines_deleted: {}, binary_added: {}, binary_deleted: {}, first_commit: {}, last_commit: {} }},",
            name(display),
            name(field("real_name")),
            n(&c, "prs"),
            n(&c, "commits"),
            n(&c, "lines_added"),
            n(&c, "lines_deleted"),
            n(&c, "binary_added"),
            n(&c, "binary_deleted"),
            s(&c, "first_commit"),
            s(&c, "last_commit"),
        );
    }
    let _ = writeln!(o, "];");
    let _ = writeln!(o, "pub static MODELS: &[Model] = &[");
    for m in rows(json, "models") {
        let _ = writeln!(
            o,
            "    Model {{ company: {}, model: {}, version: {}, commits: {}, lines_added: {}, lines_deleted: {} }},",
            s(&m, "company"),
            s(&m, "model"),
            s(&m, "version"),
            n(&m, "commits"),
            n(&m, "lines_added"),
            n(&m, "lines_deleted"),
        );
    }
    let _ = writeln!(o, "];");
    o
}
