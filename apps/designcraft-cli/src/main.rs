//! Headless DesignCraft.
//!
//! ```text
//! designcraft-cli run [--in FILE.designcraft|FILE.idml | --sample] [--cmd ID[=JSON]]... [--page N] [--scale S] [--pdf-options JSON] [--export OUT.png|.jpg|.pdf|.designcraft|.idml|.epub] [--all-pages DIR]
//!                                      # --page, --scale and --pdf-options apply to the exports that follow them
//! designcraft-cli commands [FILTER]   # list commands (JSON), optionally only ids/labels/menus containing FILTER
//! designcraft-cli describe ID          # one command: label, menu, shortcut, parameters
//! designcraft-cli script [FILE|-] [--in FILE | --sample] [--connect PORT] [--save OUT] [--export OUT] [--keep-going]
//!                                      # run a command script (crates/engine/src/script.rs): `$N.path` references
//! designcraft-cli app [--port PORT] COMMAND [JSON]   # run a command in the running app (designcraft --control PORT)
//! designcraft-cli app [--port PORT] --method METHOD [JSON]   # any control-channel method (ui.screenshot, ui.render, …)
//! designcraft-cli mcp [--connect PORT] [--sample]  # MCP server over stdio (docs/mcp.md)
//! designcraft-cli perf [--pages N] [--frames N] [--chars N] [--images N] [--runs N] [--strict]  # budgets on a synthetic stress document
//! designcraft-cli bench FILE [--runs N]  # the same measurements on one document
//! designcraft-cli links                   # Discord, website, app page and GitHub links
//! designcraft-cli --version               # print the version
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
use std::process::ExitCode;

/// `println!` that ends the program quietly when stdout is closed (`designcraft-cli commands |
/// head`) instead of panicking with "failed printing to stdout: Broken pipe (os error 32)".
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            $crate::stdout_failed(e);
        }
    }};
}

mod perf;

use designcraft_engine::Session;
use serde_json::{Value, json};

/// The usage block. Shared by `--help` (stdout, success) and an unknown command (stderr, failure).
const USAGE: &str = "usage: designcraft-cli run [--in FILE | --sample] [--cmd ID[=JSON]]... [--page N] [--scale S] [--pdf-options JSON] [--export OUT] [--all-pages DIR]\n         (--page, --scale and --pdf-options apply to the exports that follow them)\n       designcraft-cli commands [FILTER]\n       designcraft-cli describe COMMAND\n       designcraft-cli script [FILE|-] [--in FILE | --sample] [--connect PORT] [--save OUT] [--export OUT] [--keep-going]\n       designcraft-cli app [--port PORT] COMMAND [JSON] | --method METHOD [JSON]\n       designcraft-cli mcp [--connect PORT] [--sample]\n       designcraft-cli perf [--pages N] [--runs N] [--strict]\n       designcraft-cli bench FILE [--runs N]\n       designcraft-cli links\n       designcraft-cli --version";

/// The links line under the usage block.
fn usage_footer() -> String {
    format!(
        "\nCommunity: {}  ·  {}  ·  {}",
        designcraft_engine::links::DISCORD,
        designcraft_engine::links::APP_PAGE,
        designcraft_engine::links::GITHUB
    )
}

/// stdout went away. A reader that stopped early (a closed pipe) ends the program quietly, as
/// ripgrep does; any other write error is reported.
fn stdout_failed(e: std::io::Error) -> ! {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
    eprintln!("designcraft-cli: can't write to stdout: {e}");
    std::process::exit(1);
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V" | "version") => {
            outln!("designcraft-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("run") => report(run(&args[1..])),
        Some("commands") => {
            let s = Session::new();
            let filter = args.get(1).map(|f| f.to_lowercase());
            let list: Vec<Value> = serde_json::to_value(s.commands())
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .filter(|c| filter.as_ref().is_none_or(|f| c.to_string().to_lowercase().contains(f.as_str())))
                .collect();
            outln!("{}", serde_json::to_string_pretty(&list).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Some("describe") => report(describe(args.get(1).map(String::as_str))),
        Some("script") => report(script(&args[1..])),
        Some("app") => report(app(&args[1..])),
        Some("mcp") => report(mcp(&args[1..])),
        Some("perf") => report(perf::perf(&args[1..])),
        Some("bench") => report(perf::bench(&args[1..])),
        Some("links") => {
            use designcraft_engine::links::*;
            outln!("Discord   {DISCORD}\nWebsite   {WEBSITE}\nApp page  {APP_PAGE}\nGitHub    {GITHUB}\nIssues    {ISSUES}");
            ExitCode::SUCCESS
        }
        // Asking for help is not an error: it goes to stdout and succeeds. An unknown command
        // still prints the same usage to stderr and fails.
        Some("--help" | "-h" | "help") => {
            outln!("{USAGE}");
            outln!("{}", usage_footer());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            eprintln!("{}", usage_footer());
            ExitCode::FAILURE
        }
    }
}

fn report(r: Result<(), String>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("designcraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `mcp` (headless, in-process engine) or `mcp --connect PORT|HOST:PORT` (drive a running app
/// started with `designcraft --control PORT`). JSON-RPC on stdin/stdout; logs on stderr.
fn mcp(args: &[String]) -> Result<(), String> {
    use designcraft_mcp::{Backend, Headless, Remote, Server, control_addr};
    let mut connect: Option<String> = None;
    let mut sample = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--connect" => connect = Some(it.next().cloned().ok_or("--connect needs a port or host:port")?),
            "--sample" => sample = true,
            other => return Err(format!("unknown mcp option `{other}` (usage: designcraft-cli mcp [--connect PORT] [--sample])")),
        }
    }
    let backend: Box<dyn Backend> = match connect {
        Some(c) => {
            let addr = control_addr(&c);
            Box::new(
                Remote::connect(&addr)
                    .map_err(|e| format!("cannot connect to the DesignCraft app at {addr}: {e} (start it with `designcraft --control PORT`)"))?,
            )
        }
        None => {
            let mut h = Headless::with_document();
            if sample {
                h.session.execute("file.newSample", &json!({})).map_err(|e| e.to_string())?;
            }
            Box::new(h)
        }
    };
    eprintln!("designcraft-cli: MCP server on stdio ({})", backend.describe());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

/// The first `--page`, `--scale` or `--pdf-options` with no `--export` or `--all-pages` after it.
/// They apply to the exports that follow them, so one after the last export would do nothing.
fn option_after_last_export(args: &[String]) -> Option<&str> {
    let mut unused = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--page" | "--scale" | "--pdf-options" => {
                unused = unused.or(Some(a.as_str()));
                it.next();
            }
            "--export" | "--all-pages" => {
                unused = None;
                it.next();
            }
            "--in" | "--cmd" => {
                it.next();
            }
            _ => {}
        }
    }
    unused
}

fn run(args: &[String]) -> Result<(), String> {
    if let Some(opt) = option_after_last_export(args) {
        return Err(format!(
            "{opt} has no --export or --all-pages after it; options apply to the exports that follow them, so put it before its --export"
        ));
    }
    let mut s = Session::new();
    let mut page = 0usize;
    let mut scale = 1.0f64;
    let mut it = args.iter();
    let mut opened = false;
    let mut pdf_opts = json!({});
    // Earlier --cmd results, for `$N.path` references.
    let mut results: Vec<Value> = Vec::new();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--in" => {
                let p = val()?;
                s.execute("file.open", &json!({"path": p})).map_err(|e| e.to_string())?;
                opened = true;
            }
            "--sample" => {
                s.execute("file.newSample", &json!({})).map_err(|e| e.to_string())?;
                opened = true;
            }
            "--cmd" => {
                if !opened {
                    s.execute("file.new", &json!({})).map_err(|e| e.to_string())?;
                    opened = true;
                }
                let c = val()?;
                let (id, p) = c.split_once('=').unwrap_or((&c, "{}"));
                let p: Value = serde_json::from_str(p).map_err(|e| format!("--cmd {id}: {e}"))?;
                let p = designcraft_engine::script::resolve(&p, &results).map_err(|e| format!("--cmd {id}: {e}"))?;
                let r = s.execute(id, &p).map_err(|e| e.to_string())?;
                results.push(r.clone());
                if !r.is_null() {
                    outln!("{}", serde_json::to_string(&r).unwrap_or_default());
                }
            }
            "--page" => page = val()?.parse().map_err(|_| "bad --page")?,
            "--scale" => scale = val()?.parse().map_err(|_| "bad --scale")?,
            "--pdf-options" => {
                pdf_opts = serde_json::from_str(&val()?).map_err(|e| format!("--pdf-options: {e}"))?;
            }
            "--export" => {
                let out = val()?;
                if out.ends_with(".epub") {
                    let r = s.execute("file.exportEpub", &json!({"path": out})).map_err(|e| e.to_string())?;
                    eprintln!("wrote {out} ({} bytes)", r["bytes"]);
                } else if out.ends_with(".pdf") {
                    let mut p = pdf_opts.clone();
                    p["path"] = json!(out);
                    let r = s.execute("file.exportPdf", &p).map_err(|e| e.to_string())?;
                    eprintln!("wrote {out} ({} pages, {} bytes)", r["pages"], r["bytes"]);
                    for w in r["warnings"].as_array().into_iter().flatten() {
                        eprintln!("warning: {}", w.as_str().unwrap_or_default());
                    }
                } else {
                    export(&mut s, &out, page, scale)?;
                }
            }
            "--all-pages" => {
                let dir = val()?;
                let n = s.doc().map_err(|e| e.to_string())?.doc.page_count();
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                for p in 0..n {
                    export(&mut s, &format!("{dir}/page-{:03}.png", p + 1), p, scale)?;
                }
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(())
}

fn export(s: &mut Session, out: &str, page: usize, scale: f64) -> Result<(), String> {
    if out.to_ascii_lowercase().ends_with(".idml") {
        let r = s.execute("file.exportIdml", &json!({"path": out})).map_err(|e| e.to_string())?;
        eprintln!("wrote {out} ({} bytes)", r["bytes"]);
        return Ok(());
    }
    if out.ends_with(".designcraft") {
        s.execute("file.saveAs", &json!({"path": out})).map_err(|e| e.to_string())?;
        eprintln!("saved {out}");
        return Ok(());
    }
    let st = s.doc().map_err(|e| e.to_string())?;
    let mut r = designcraft_render::Renderer::new();
    let t = std::time::Instant::now();
    let img = r
        .render_page(&st.doc, &s.cache, page, scale, true, &designcraft_render::RenderOptions { printing_only: true, ..Default::default() })
        .ok_or("no such page")?;
    let bytes = if out.ends_with(".jpg") || out.ends_with(".jpeg") { img.to_jpeg(90) } else { img.to_png() };
    std::fs::write(out, bytes).map_err(|e| format!("{out}: {e}"))?;
    eprintln!("wrote {out} ({}×{}, {:.1} ms, {} glyphs)", img.width, img.height, t.elapsed().as_secs_f64() * 1000.0, r.stats.glyphs);
    Ok(())
}

/// `describe ID`: one command's documentation.
fn describe(id: Option<&str>) -> Result<(), String> {
    let id = id.ok_or("usage: designcraft-cli describe COMMAND")?;
    let s = Session::new();
    let all = serde_json::to_value(s.commands()).map_err(|e| e.to_string())?;
    let c = all.as_array().and_then(|a| a.iter().find(|c| c["id"] == id)).ok_or_else(|| {
        let near: Vec<String> = all
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["id"].as_str())
            .filter(|c| c.split('.').next() == id.split('.').next())
            .map(str::to_string)
            .collect();
        format!("no command `{id}`{}", if near.is_empty() { String::new() } else { format!(" (similar: {})", near.join(", ")) })
    })?;
    // Enablement depends on a live document and selection: not meaningful here.
    let mut c = c.clone();
    if let Some(o) = c.as_object_mut() {
        o.remove("enabled");
        o.remove("disabled_reason");
    }
    outln!("{}", serde_json::to_string_pretty(&c).unwrap_or_default());
    Ok(())
}

/// `script`: run a command script headless (or in the running app with `--connect`).
fn script(args: &[String]) -> Result<(), String> {
    use designcraft_mcp::{Backend, Headless, Remote, control_addr};
    let mut file: Option<String> = None;
    let mut setup: Vec<(String, Value)> = Vec::new();
    let mut connect: Option<String> = None;
    let mut save: Option<String> = None;
    let mut export: Vec<String> = Vec::new();
    let mut keep_going = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--in" => {
                let p = val()?;
                let cmd = if p.to_lowercase().ends_with(".idml") { "file.openIdml" } else { "file.open" };
                setup.push((cmd.into(), json!({"path": p})));
            }
            "--sample" => setup.push(("file.newSample".into(), json!({}))),
            "--connect" => connect = Some(val()?),
            "--save" => save = Some(val()?),
            "--export" => export.push(val()?),
            "--keep-going" => keep_going = true,
            f if !f.starts_with("--") && file.is_none() => file = Some(f.to_string()),
            other => return Err(format!("unknown script option `{other}`")),
        }
    }
    let text = match file.as_deref() {
        None | Some("-") => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).map_err(|e| e.to_string())?;
            t
        }
        Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
    };
    let steps = designcraft_engine::script::parse(&text)?;
    let mut backend: Box<dyn Backend> = match &connect {
        Some(c) => {
            let addr = control_addr(c);
            Box::new(
                Remote::connect(&addr)
                    .map_err(|e| format!("cannot connect to the DesignCraft app at {addr}: {e} (start it with `designcraft --control PORT`)"))?,
            )
        }
        None => Box::new(Headless::with_document()),
    };
    let b = &mut *backend;
    let exec = |b: &mut dyn Backend, id: &str, p: Value| b.call("engine.execute", json!({"command": id, "params": p}));
    for (c, p) in &setup {
        exec(b, c, p.clone())?;
    }
    let report = if keep_going {
        // Run every step; failures become {"error": …} results.
        let mut results: Vec<Value> = Vec::new();
        for st in &steps {
            let r = designcraft_engine::script::resolve(&st.params, &results).and_then(|p| exec(b, &st.command, p));
            results.push(r.unwrap_or_else(|e| json!({"error": e, "command": st.command})));
        }
        designcraft_engine::script::Report { results, failed: None }
    } else {
        designcraft_engine::script::run(&steps, |id, p| exec(b, id, p))
    };
    if report.failed.is_none() {
        if let Some(out) = &save {
            exec(b, "file.saveAs", json!({"path": out}))?;
        }
        for out in &export {
            let lower = out.to_lowercase();
            let p = json!({"path": out});
            if lower.ends_with(".pdf") {
                exec(b, "file.exportPdf", p)?;
            } else if lower.ends_with(".idml") {
                exec(b, "file.exportIdml", p)?;
            } else if lower.ends_with(".epub") {
                exec(b, "file.exportEpub", p)?;
            } else {
                // PNG / JPEG of the first page.
                b.call("app.export", p)?;
            }
        }
    }
    outln!("{}", serde_json::to_string_pretty(&report.to_json()).unwrap_or_default());
    match report.failed {
        Some((i, c, e)) => Err(format!("step {i} ({c}) failed: {e}")),
        None => Ok(()),
    }
}

/// `app`: one command or control-channel method in the running app.
fn app(args: &[String]) -> Result<(), String> {
    use designcraft_mcp::{Backend, Remote, control_addr};
    let mut port = "7979".to_string();
    let mut method: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" | "--connect" => port = it.next().cloned().ok_or("--port needs a value")?,
            "--method" => method = Some(it.next().cloned().ok_or("--method needs a value")?),
            _ => rest.push(a.clone()),
        }
    }
    let addr = control_addr(&port);
    let mut r = Remote::connect(&addr)
        .map_err(|e| format!("cannot connect to the DesignCraft app at {addr}: {e} (start it with `designcraft --control {port}`)"))?;
    let json_arg =
        |s: Option<&String>| -> Result<Value, String> { s.map_or(Ok(json!({})), |t| serde_json::from_str(t).map_err(|e| format!("bad JSON: {e}"))) };
    let out = match method {
        Some(m) => r.call(&m, json_arg(rest.first())?)?,
        None => {
            let cmd = rest.first().ok_or("usage: designcraft-cli app [--port PORT] COMMAND [JSON]")?;
            r.call("engine.execute", json!({"command": cmd, "params": json_arg(rest.get(1))?}))?
        }
    };
    outln!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(())
}
