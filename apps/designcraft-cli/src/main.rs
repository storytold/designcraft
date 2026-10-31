//! Headless DesignCraft.
//!
//! ```text
//! designcraft-cli run [--in FILE.designcraft|FILE.idml | --sample] [--cmd ID[=JSON]]... [--page N] [--scale S] [--pdf-options JSON] [--export OUT.png|.jpg|.pdf|.designcraft|.idml|.epub|.pptx|.html|.rtf|.txt] [--all-pages DIR]
//!                                      # --page, --scale and --pdf-options apply to the exports that follow them
//!                                      # .rtf and .txt hold the story being edited or of the selected frame
//! designcraft-cli commands [FILTER]   # list commands (JSON), optionally only ids/labels/menus containing FILTER
//! designcraft-cli describe ID          # one command: label, menu, shortcut, parameters
//! designcraft-cli script [FILE|-] [--in FILE | --sample] [--connect PORT] [--save OUT] [--export OUT] [--keep-going]
//!                                      # run a command script (crates/engine/src/script.rs): `$N.path` references
//! designcraft-cli app [--port PORT] COMMAND [JSON]   # run a command in the running app (designcraft --control PORT)
//! designcraft-cli app [--port PORT] --method METHOD [JSON]   # any control-channel method (ui.screenshot, ui.render, …)
//! designcraft-cli mcp [--connect PORT] [--sample] [--automation-read-root DIR]... [--automation-write-root DIR]...
//!                                      # MCP server over stdio (docs/mcp.md); the roots confine the files it reads and writes
//! designcraft-cli perf [--pages N] [--frames N] [--chars N] [--images N] [--runs N] [--strict]  # budgets on a synthetic stress document
//! designcraft-cli bench FILE [--runs N]  # the same measurements on one document
//! designcraft-cli validate-pdf FILE.pdf [--standard x4|x1a]   # the built-in PDF/X-4 or PDF/X-1a checks (default: the
//!                                      # file's own GTS_PDFXVersion)
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

mod automation_args;
mod perf;

use designcraft_engine::Session;
use serde_json::{Value, json};

/// The usage block. Shared by `--help` (stdout, success) and an unknown command (stderr, failure).
const USAGE: &str = "usage: designcraft-cli run [--in FILE | --sample] [--cmd ID[=JSON]]... [--page N] [--scale S] [--pdf-options JSON] [--export OUT] [--all-pages DIR]\n         (--page, --scale and --pdf-options apply to the exports that follow them)\n       designcraft-cli commands [FILTER]\n       designcraft-cli describe COMMAND\n       designcraft-cli script [FILE|-] [--in FILE | --sample] [--connect PORT] [--save OUT] [--export OUT] [--keep-going]\n       designcraft-cli app [--port PORT] COMMAND [JSON] | --method METHOD [JSON]\n       designcraft-cli mcp [--connect PORT] [--sample] [--automation-read-root DIR]... [--automation-write-root DIR]...\n         the roots confine every file the agent's commands read (open, placed images, data sources, libraries) and\n         write (save, export, package) to those folders, links followed. Each may be given more than once; a kind\n         left out grants none of its access.\n         They run the in-process server, so they conflict with --connect (a running app is not confined by them).\n       designcraft-cli perf [--pages N] [--runs N] [--strict]\n       designcraft-cli bench FILE [--runs N]\n       designcraft-cli validate-pdf FILE.pdf  # built-in PDF/X-4 checks (including visible unembedded fonts)\n       designcraft-cli links\n       designcraft-cli --version";

/// `script --help`: the script syntax, otherwise only in `describe script.run` and docs/agents.md.
const SCRIPT_HELP: &str = r#"usage: designcraft-cli script [FILE|-] [--in FILE | --sample] [--connect PORT] [--save OUT] [--export OUT] [--keep-going]

Runs a command script from FILE (stdin when FILE is `-` or missing), headless or, with
--connect PORT, in the running app (designcraft --control PORT).

Script text, one step per line:
  command.id {"param": 1}     also `command.id={json}`, or `command.id` alone
  {"command": "command.id", "params": {...}}   JSON lines, or a JSON array of them
  # comment, // comment       skipped, like blank lines
  UTF-8 with or without a byte order mark; LF or CRLF line endings.

References to earlier results (in string parameters):
  "$N.path"     the value at `path` in the result of step N. N is the 0-based step index:
                comments and blank lines are not steps. `$last` is the previous step.
                Paths go through result fields and array indices: "$1.story", "$3.ids.0".
  "${N.path}"   inside a longer string, replaced by the value's text: "Frame ${0.id}".
  Any other string starting with `$` stays as written ("$1-$2", "$5 off").
  "$$"          a literal `$` anywhere: "$$1" passes `$1` (a GREP group), "$${0.id}" passes `${0.id}`.

Text parameters: "\n" starts a new paragraph, "\u2028" is a forced line break, "\t" a tab.
"\u000b" (Word's line break, Chr(11)) is read as a forced line break too.
Most commands ignore parameters they do not know instead of rejecting them: check names
with `designcraft-cli describe COMMAND`.

Options:
  --in FILE / --sample   open a document (.designcraft or .idml) or the sample first
  --save OUT             save as .designcraft after the script succeeds
  --export OUT           export after the script succeeds (.pdf, .idml, .epub, .pptx, .png, .jpg); repeatable
  --keep-going           run every step, recording failures as {"error": ...} results

Prints {"completed": n, "results": [...]}, plus failedIndex / failedCommand / error and a
non-zero exit when a step fails."#;

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

/// Options whose value is a path, and so can be written `./-name` when it starts with `-`, the
/// shell's own way out. None of them has an `--opt=value` form (the automation roots, parsed in
/// [`automation_args`], do and say so themselves).
const PATH_OPTIONS: [&str; 4] = ["--in", "--export", "--all-pages", "--save"];

/// An option's value, refusing the next token when it is another option: `--export --scale` used
/// to write a file called `--scale`.
fn value(a: &str, next: Option<&String>) -> Result<String, String> {
    match next {
        None => Err(format!("{a} needs a value")),
        Some(v) if !can_be_value(v) => {
            // The way out is worth offering only where the value could be such a name.
            let out = if PATH_OPTIONS.contains(&a) { format!("; if you did mean that name, write `./{v}`") } else { String::new() };
            Err(format!("{a} needs a value and `{v}` is another option{out}"))
        }
        Some(v) => Ok(v.clone()),
    }
}

/// Whether a token can be an option's value. A negative number can; anything else that starts
/// with `-` is another option. `-inf` and `-nan` parse as numbers too, so a bad `--scale` still
/// reaches the check that reports it rather than being called an option.
fn can_be_value(v: &str) -> bool {
    !(v.starts_with('-') && v.len() > 1 && v.parse::<f64>().is_err())
}

/// The usage lines for one subcommand, for `<sub> --help`. Each line of [`USAGE`] names its
/// subcommand, and the indented lines after it belong to it.
fn usage_for(sub: &str) -> String {
    let mut out = Vec::new();
    let mut keep = false;
    for line in USAGE.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("usage: designcraft-cli ").or_else(|| trimmed.strip_prefix("designcraft-cli ")) {
            keep = rest.split_whitespace().next() == Some(sub);
        }
        if keep {
            out.push(line);
        }
    }
    if out.is_empty() { USAGE.to_string() } else { out.join("\n") }
}

fn main() -> ExitCode {
    // `std::env::args` panics on an argument that is not UTF-8 ("called `Result::unwrap()` on an
    // `Err` value"), which a Latin-1 file name on Linux or macOS is. Such a name cannot be passed
    // through to the engine, whose paths are `str`, so it is a usage error here rather than a
    // crash (photocraft#1108).
    let mut args: Vec<String> = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(a) => args.push(a),
            Err(bad) => {
                eprintln!("designcraft-cli: `{}` is not valid UTF-8, so it cannot be used as an argument", bad.to_string_lossy());
                return ExitCode::FAILURE;
            }
        }
    }
    // `<sub> --help` asks about that subcommand, which every parser but `script`'s read as an
    // unknown option (photocraft#423). It goes to stdout and succeeds, as `--help` does.
    if let Some(sub) = args.first().map(String::as_str)
        && !matches!(sub, "--help" | "-h" | "help" | "script")
        && args[1..].iter().any(|a| a == "--help" || a == "-h")
    {
        outln!("{}", usage_for(sub));
        return ExitCode::SUCCESS;
    }
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
        Some("validate-pdf") => report(validate_pdf(args.get(1..).unwrap_or_default())),
        Some("links") => {
            use designcraft_engine::links::{APP_PAGE, DISCORD, GITHUB, ISSUES, WEBSITE};
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

fn validate_pdf(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: designcraft-cli validate-pdf FILE.pdf [--standard x4|x1a]";
    const X1A: &[u8] = b"/GTS_PDFXVersion(PDF/X-1:";
    let mut path = None;
    let mut standard = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--standard" => standard = Some(it.next().ok_or(USAGE)?.as_str()),
            p if path.is_none() => path = Some(p),
            _ => return Err(USAGE.into()),
        }
    }
    let path = path.ok_or(USAGE)?;
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let standard = match standard.map(designcraft_pdf::Standard::parse) {
        Some(Some(s)) => s,
        Some(None) => return Err(USAGE.into()),
        None if bytes.windows(X1A.len()).any(|w| w == X1A) => designcraft_pdf::Standard::PdfX1a,
        None => designcraft_pdf::Standard::PdfX4,
    };
    let (name, issues) = match standard {
        designcraft_pdf::Standard::PdfX1a => ("PDF/X-1a:2003", designcraft_pdf::check_pdfx1a(&bytes)),
        designcraft_pdf::Standard::PdfX4 => ("PDF/X-4", designcraft_pdf::check_pdfx4(&bytes)),
        _ => return Err(USAGE.into()),
    };
    let (warnings, issues): (Vec<String>, Vec<String>) = issues.into_iter().partition(|i| i.starts_with("warning: "));
    let warnings: Vec<&str> = warnings.iter().map(|w| w.trim_start_matches("warning: ")).collect();
    let valid = issues.is_empty();
    let report = json!({"path": path, "standard": name, "valid": valid, "issues": issues, "warnings": warnings});
    outln!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    if valid { Ok(()) } else { Err(format!("{path} failed {name} validation")) }
}

/// `mcp` (headless, in-process engine) or `mcp --connect PORT|HOST:PORT` (drive a running app
/// started with `designcraft --control PORT`). JSON-RPC on stdin/stdout; logs on stderr.
fn mcp(args: &[String]) -> Result<(), String> {
    use designcraft_engine::file_access::{self, AutomationRoots};
    use designcraft_mcp::{Backend, Headless, Remote, Server, control_addr};
    let mut connect: Option<String> = None;
    let mut sample = false;
    let mut roots_args = automation_args::AutomationArgs::default();
    let mut it = args.iter().cloned();
    while let Some(a) = it.next() {
        if roots_args.consume(&a, &mut it)? {
            continue;
        }
        match a.as_str() {
            "--connect" => connect = Some(it.next().ok_or("--connect needs a port or host:port")?),
            "--sample" => sample = true,
            other => {
                return Err(format!(
                    "unknown mcp option `{other}` (usage: designcraft-cli mcp [--connect PORT] [--sample] [--automation-read-root DIR]... [--automation-write-root DIR]...)"
                ));
            }
        }
    }
    let read: Vec<&std::path::Path> = roots_args.read_roots.iter().map(std::path::PathBuf::as_path).collect();
    let write: Vec<&std::path::Path> = roots_args.write_roots.iter().map(std::path::PathBuf::as_path).collect();
    let roots = AutomationRoots::new(&read, &write)?;
    if roots.is_some() && connect.is_some() {
        return Err("--automation-read-root and --automation-write-root confine the in-process server; a running app is not confined by them, \
so drop --connect (or use --connect without the roots)"
            .into());
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
            // `--sample` is the operator's own choice, made before the roots come into force.
            if sample {
                h.session.execute("file.newSample", &json!({})).map_err(|e| e.to_string())?;
            }
            if let Some(roots) = roots {
                // Every thread of this process, not only the server's: nothing reaches past the roots.
                file_access::confine_process(roots.clone());
                eprintln!("designcraft-cli: files confined: {}", roots.describe());
                h = h.with_automation_roots(Some(roots));
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
    let mut it = args.iter().peekable();
    // A value is skipped only where the next token can be one. Taking it whatever it is read
    // `--scale --export out.png` as --scale applied to no export, so a missing value was
    // reported as the order of the options -- and "put it before its --export" is advice for a
    // list that already does that (photocraft#7071).
    while let Some(a) = it.next() {
        let is_opt = matches!(a.as_str(), "--page" | "--scale" | "--pdf-options" | "--export" | "--all-pages" | "--in" | "--cmd");
        match a.as_str() {
            "--page" | "--scale" | "--pdf-options" => unused = unused.or(Some(a.as_str())),
            "--export" | "--all-pages" => unused = None,
            _ => {}
        }
        if is_opt {
            match it.peek() {
                Some(v) if can_be_value(v.as_str()) => {
                    it.next();
                }
                // The option has no value. That is what is wrong with the line, so this check
                // stands aside and lets `value()` say so: reporting the order instead advised
                // moving an option that is already where it belongs (photocraft#7071).
                _ => return None,
            }
        }
    }
    unused
}

/// What `--export OUT` writes. The extension of `OUT` picks it, in any case.
enum Export {
    /// A page as PNG or JPEG.
    Image,
    Pdf,
    /// The document itself.
    Document,
    /// Editable PowerPoint slides.
    Pptx,
    /// What this command writes to a `path`.
    Command(&'static str),
}

impl Export {
    /// A name with another extension is an error: it used to get a PNG of the page. One with no
    /// extension still does (`--export /dev/stdout`).
    fn of(out: &str) -> Result<Self, String> {
        let ext = std::path::Path::new(out).extension().and_then(std::ffi::OsStr::to_str).map(str::to_lowercase);
        Ok(match ext.as_deref() {
            None | Some("png" | "jpg" | "jpeg") => Export::Image,
            Some("pdf") => Export::Pdf,
            Some("designcraft") => Export::Document,
            Some("idml") => Export::Command("file.exportIdml"),
            Some("epub") => Export::Command("file.exportEpub"),
            Some("pptx") => Export::Pptx,
            Some("html" | "htm") => Export::Command("file.exportHtml"),
            // The story being edited or of the selected frame.
            Some("rtf" | "txt") => Export::Command("file.exportText"),
            Some(_) => {
                return Err(format!("--export {out}: unknown file type (.png, .jpg, .pdf, .designcraft, .idml, .epub, .pptx, .html, .rtf, .txt)"));
            }
        })
    }
}

/// Every option that takes a value has one, checked before anything runs. Found later, a missing
/// value is reported only after the exports before it have been written -- and `run --export
/// first.png --scale --export second.png` would then have overwritten an existing `first.png` on
/// an invocation that fails (noted in review).
fn values_present(args: &[String]) -> Result<(), String> {
    let mut i = 0;
    while let Some(a) = args.get(i) {
        i += 1;
        if !matches!(a.as_str(), "--in" | "--cmd" | "--export" | "--all-pages" | "--page" | "--scale" | "--pdf-options") {
            continue;
        }
        value(a, args.get(i))?;
        // Step over the value, so one that looks like an option is not checked as one.
        i += 1;
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    // Before the order check and before any export: a value error is about the line as written,
    // and reporting it must not cost a file that was already there.
    values_present(args)?;
    if let Some(opt) = option_after_last_export(args) {
        return Err(format!(
            "{opt} has no --export or --all-pages after it; options apply to the exports that follow them, so put it before its --export"
        ));
    }
    // Nothing runs when one of the exports cannot be written.
    for a in args.windows(2) {
        if a[0] == "--export" {
            Export::of(&a[1])?;
        }
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
        let mut val = || value(a, it.next());
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
                // A key the command's description doesn't name is an error, not a default (#776).
                designcraft_engine::check_params(id, &p).map_err(|e| e.to_string())?;
                let r = s.execute(id, &p).map_err(|e| e.to_string())?;
                results.push(r.clone());
                if !r.is_null() {
                    outln!("{}", serde_json::to_string(&r).unwrap_or_default());
                }
            }
            "--page" => page = val()?.parse().map_err(|_| "bad --page")?,
            "--scale" => {
                // 0, a negative value and NaN made a 1×1 image; infinity, "no such page".
                scale = val()?.parse().ok().filter(|s: &f64| s.is_finite() && *s > 0.0).ok_or("bad --scale: expected a positive number")?;
            }
            "--pdf-options" => {
                // The export adds its path to them.
                pdf_opts = match serde_json::from_str(&val()?).map_err(|e| format!("--pdf-options: {e}"))? {
                    Value::Null => json!({}),
                    o @ Value::Object(_) => o,
                    _ => return Err("--pdf-options: expected a JSON object".to_string()),
                };
            }
            "--export" => {
                let out = val()?;
                handle_export(&mut s, &out, page, scale, &pdf_opts)?;
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

fn handle_export(s: &mut Session, out: &str, page: usize, scale: f64, pdf_opts: &Value) -> Result<(), String> {
    match Export::of(out)? {
        Export::Image => export(s, out, page, scale)?,
        Export::Pdf => {
            let mut p = pdf_opts.clone();
            p["path"] = json!(out);
            let r = s.execute("file.exportPdf", &p).map_err(|e| e.to_string())?;
            eprintln!("wrote {out} ({} pages, {} bytes)", r["pages"], r["bytes"]);
            for w in r["warnings"].as_array().into_iter().flatten() {
                eprintln!("warning: {}", w.as_str().unwrap_or_default());
            }
        }
        Export::Document => {
            s.execute("file.saveAs", &json!({"path": out})).map_err(|e| e.to_string())?;
            eprintln!("saved {out}");
        }
        Export::Pptx => {
            let r = s.execute("file.exportPptx", &json!({"path": out})).map_err(|e| e.to_string())?;
            eprintln!("wrote {out} ({} slides, {} bytes)", r["slides"], r["bytes"]);
            for w in r["warnings"].as_array().into_iter().flatten() {
                eprintln!("warning: {}", w.as_str().unwrap_or_default());
            }
        }
        Export::Command(cmd) => {
            let r = s.execute(cmd, &json!({"path": out})).map_err(|e| e.to_string())?;
            eprintln!("wrote {out} ({} bytes)", r["bytes"]);
        }
    }
    Ok(())
}

/// A page as PNG, or as JPEG for `.jpg` and `.jpeg`.
fn export(s: &mut Session, out: &str, page: usize, scale: f64) -> Result<(), String> {
    let st = s.doc().map_err(|e| e.to_string())?;
    let mut r = designcraft_render::Renderer::new();
    let t = std::time::Instant::now();
    let img = r
        .render_page(&st.doc, &s.cache, page, scale, true, &designcraft_render::RenderOptions { printing_only: true, ..Default::default() })
        .ok_or("no such page")?;
    let lower = out.to_lowercase();
    let bytes = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { img.to_jpeg(90) } else { img.to_png() };
    designcraft_engine::atomic::write_atomic(std::path::Path::new(out), &bytes).map_err(|e| format!("{out}: {e}"))?;
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
    let mut export: Vec<(Export, String)> = Vec::new();
    let mut keep_going = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || value(a, it.next());
        match a.as_str() {
            "--in" => {
                let p = val()?;
                let cmd = if p.to_lowercase().ends_with(".idml") { "file.openIdml" } else { "file.open" };
                setup.push((cmd.into(), json!({"path": p})));
            }
            "--sample" => setup.push(("file.newSample".into(), json!({}))),
            "--connect" => connect = Some(val()?),
            "--save" => save = Some(val()?),
            "--export" => {
                let out = val()?;
                export.push((Export::of(&out)?, out));
            }
            "--keep-going" => keep_going = true,
            "--help" | "-h" => {
                outln!("{SCRIPT_HELP}");
                return Ok(());
            }
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
        for (kind, out) in &export {
            let p = json!({"path": out});
            match kind {
                // PNG / JPEG of the first page.
                Export::Image => b.call("app.export", p)?,
                Export::Pdf => exec(b, "file.exportPdf", p)?,
                // A copy: the document in the app stays the file it is (`--save` is Save As).
                Export::Document => exec(b, "file.saveACopy", p)?,
                Export::Pptx => exec(b, "file.exportPptx", p)?,
                Export::Command(cmd) => exec(b, cmd, p)?,
            };
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
