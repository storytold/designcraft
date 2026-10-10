//! File › Print: the pages go to a printer through the system print spooler (`lpr`), as PDF.

#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;

use serde_json::{Value, json};

#[cfg(not(target_arch = "wasm32"))]
use super::str_param;
use super::{CommandSpec, bad, cmd, has_doc};
#[cfg(not(target_arch = "wasm32"))]
use crate::EngineError;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "file.printers", "Printers", [], None, "{} → {printers: [names], default}", super::always, |_, _| Ok(printers())),
        cmd!(
            noundo "file.print",
            "Print…",
            ["File"],
            Some("Cmd+P"),
            "{printer?, copies?: 1, pages?: \"1-3,5\", spreads?, marks?, bleed?, dryRun?: bool (only return the spool command)} → {printer, copies, pages, command}",
            has_doc,
            print
        ),
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn printers() -> Value {
    let run = |args: &[&str]| {
        std::process::Command::new("lpstat").args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
    };
    let names: Vec<String> = run(&["-e"]).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
    // "system default destination: NAME"
    let default = run(&["-d"]).rsplit(':').next().map(str::trim).filter(|s| !s.is_empty() && !s.contains("no system default")).map(str::to_string);
    json!({"printers": names, "default": default})
}

#[cfg(target_arch = "wasm32")]
fn printers() -> Value {
    json!({"printers": [], "default": null})
}

fn print(s: &mut Session, p: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let opts = super::export::options(p, d.page_count())?;
    #[cfg(target_arch = "wasm32")]
    {
        // Even a dry run cannot construct a native spool command in the browser.
        let _ = opts;
        Err(bad("file.print", "printing isn't available on the web: export a PDF and print it"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let copies = p.get("copies").and_then(Value::as_u64).unwrap_or(1).clamp(1, 999);
        let printer = str_param(p, "printer").map(str::to_string);
        let dry = p.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
        let pdf = designcraft_pdf::export_pdf(d, &s.cache, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
        let mut file = secure_print_file()?;
        let file_path = file.path().to_path_buf();
        let mut cmd: Vec<String> = vec!["lpr".into()];
        if let Some(pr) = &printer {
            cmd.extend(["-P".into(), pr.clone()]);
        }
        if copies > 1 {
            cmd.extend(["-#".into(), copies.to_string()]);
        }
        cmd.extend(["-T".into(), d.title.clone(), file_path.to_string_lossy().to_string()]);
        let pages = opts.pages.as_ref().map_or(d.page_count(), |v| v.len());
        let out = json!({"printer": printer, "copies": copies, "pages": pages, "command": cmd, "bytes": pdf.len()});
        if dry {
            return Ok(out);
        }
        file.as_file_mut().write_all(&pdf).map_err(|e| EngineError::Other(format!("{}: {e}", file.path().display())))?;
        file.as_file_mut().flush().map_err(|e| EngineError::Other(format!("{}: {e}", file.path().display())))?;
        let st = std::process::Command::new(&cmd[0]).args(&cmd[1..]).output().map_err(|e| bad("file.print", format!("lpr: {e}")))?;
        if !st.status.success() {
            return Err(bad("file.print", String::from_utf8_lossy(&st.stderr).trim().to_string()));
        }
        Ok(out)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn secure_print_file() -> Result<tempfile::NamedTempFile> {
    let file = tempfile::Builder::new()
        .prefix("designcraft-print-")
        .suffix(".pdf")
        .tempfile()
        .map_err(|e| EngineError::Other(format!("temporary print file: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| EngineError::Other(format!("{}: {e}", file.path().display())))?;
    }
    Ok(file)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn print_builds_the_spool_command() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        let r = s.execute("file.print", &json!({"printer": "Office", "copies": 2, "pages": "2-3", "dryRun": true})).unwrap();
        assert_eq!(r["pages"], 2);
        let cmd: Vec<&str> = r["command"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(&cmd[..5], ["lpr", "-P", "Office", "-#", "2"]);
        assert!(cmd.last().unwrap().ends_with(".pdf"));
        assert!(!std::path::Path::new(cmd.last().unwrap()).exists());
        assert!(r["bytes"].as_u64().unwrap() > 500);
        assert!(s.execute("file.print", &json!({"pages": "9", "dryRun": true})).is_err());
        assert!(s.execute("file.printers", &json!({})).unwrap()["printers"].is_array());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn print_uses_distinct_ephemeral_files() {
        let first = super::secure_print_file().unwrap();
        let second = super::secure_print_file().unwrap();
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();
        assert_ne!(first_path, second_path);
        drop((first, second));
        assert!(!first_path.exists());
        assert!(!second_path.exists());
    }

    #[cfg(all(not(target_arch = "wasm32"), unix))]
    #[test]
    fn print_file_is_private_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;

        let file = super::secure_print_file().unwrap();
        let path = file.path().to_path_buf();
        assert_eq!(file.as_file().metadata().unwrap().permissions().mode() & 0o777, 0o600);
        drop(file);
        assert!(!path.exists());
    }
}
