//! What each subcommand does once its arguments are parsed.

use std::path::Path;

use designcraft_engine::Session;
use serde_json::{Value, json};

use crate::args::{AppArgs, Command, CommandsArgs, DescribeArgs, Export, McpArgs, PdfStandard, RunArgs, RunStep, ScriptArgs, Setup};
use crate::args::{ThumbnailArgs, ValidatePdfArgs};
use crate::error::{CliError, unicode};
use crate::{Io, errln, outln, perf};

/// Run one subcommand.
pub(crate) fn dispatch(command: Command, io: &mut Io) -> Result<(), CliError> {
    match command {
        Command::Run(a) => run(&a, io),
        Command::Commands(a) => commands(&a, io),
        Command::Describe(a) => describe(&a, io),
        Command::Script(a) => script(&a, io),
        Command::App(a) => app(&a, io),
        Command::Mcp(a) => mcp(&a, io),
        Command::Perf(a) => perf::perf(&a, io),
        Command::Bench(a) => perf::bench(&a, io),
        Command::ValidatePdf(a) => validate_pdf(&a, io),
        Command::Thumbnail(a) => thumbnail(&a),
        Command::Links => {
            use designcraft_engine::links::{APP_PAGE, DISCORD, GITHUB, ISSUES, WEBSITE};
            outln!(io, "Discord   {DISCORD}\nWebsite   {WEBSITE}\nApp page  {APP_PAGE}\nGitHub    {GITHUB}\nIssues    {ISSUES}");
            Ok(())
        }
    }
}

fn commands(a: &CommandsArgs, io: &mut Io) -> Result<(), CliError> {
    let s = Session::new();
    let filter = a.filter.as_ref().map(|f| f.to_lowercase());
    let list: Vec<Value> = serde_json::to_value(s.commands())
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| filter.as_ref().is_none_or(|f| c.to_string().to_lowercase().contains(f.as_str())))
        .collect();
    outln!(io, "{}", serde_json::to_string_pretty(&list).unwrap_or_default());
    Ok(())
}

fn validate_pdf(a: &ValidatePdfArgs, io: &mut Io) -> Result<(), CliError> {
    let bytes = std::fs::read(&a.file).map_err(|source| CliError::File { path: a.file.clone(), source })?;
    let standard = match a.standard {
        Some(PdfStandard::X4) => designcraft_pdf::Standard::PdfX4,
        Some(PdfStandard::X1a) => designcraft_pdf::Standard::PdfX1a,
        None if designcraft_pdf::pdfx_version(&bytes).is_some_and(|v| v.starts_with("PDF/X-1:")) => designcraft_pdf::Standard::PdfX1a,
        None => designcraft_pdf::Standard::PdfX4,
    };
    let (name, issues) = match standard {
        designcraft_pdf::Standard::PdfX1a => ("PDF/X-1a:2003", designcraft_pdf::check_pdfx1a(&bytes)),
        _ => ("PDF/X-4", designcraft_pdf::check_pdfx4(&bytes)),
    };
    let (warnings, issues): (Vec<String>, Vec<String>) = issues.into_iter().partition(|i| i.starts_with("warning: "));
    let warnings: Vec<&str> = warnings.iter().map(|w| w.trim_start_matches("warning: ")).collect();
    let valid = issues.is_empty();
    let report = json!({"path": unicode(&a.file)?, "standard": name, "valid": valid, "issues": issues, "warnings": warnings});
    outln!(io, "{}", serde_json::to_string_pretty(&report).map_err(CliError::engine)?);
    if valid { Ok(()) } else { Err(CliError::Invalid { path: a.file.clone(), standard: name }) }
}

/// `thumbnail FILE OUT [SIZE]`: the first-page preview saved in a `.designcraft` file, as a PNG
/// whose longer side is at most SIZE pixels (never enlarged). It is the freedesktop thumbnailer
/// (`packaging/linux/ai.storyteller.designcraft.thumbnailer`), so it only reads the preview: a file
/// without one fails, and the file manager keeps the file type's icon.
fn thumbnail(a: &ThumbnailArgs) -> Result<(), CliError> {
    let ThumbnailArgs { input, out, size } = a;
    if names_the_same_file(input, out) {
        return Err(CliError::WouldReplace { out: out.clone(), input: input.clone() });
    }
    let file = std::fs::File::open(input).map_err(|source| CliError::File { path: input.clone(), source })?;
    let preview = designcraft_format::read_preview(std::io::BufReader::new(file)).ok_or_else(|| CliError::NoPreview(input.clone()))?;
    let png = fit_png(preview, *size).map_err(|why| CliError::Preview { path: input.clone(), why })?;
    designcraft_engine::atomic::write_atomic(out, &png).map_err(|source| CliError::File { path: out.clone(), source })
}

/// Whether `out` is the file `input` names: the same path, a symlink to it or, on Unix, a hard
/// link (the same device and inode). An `out` that doesn't exist yet can't be the input.
fn names_the_same_file(input: &std::path::Path, out: &std::path::Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::canonicalize(input), std::fs::canonicalize(out)) else { return false };
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(&a), std::fs::metadata(&b)) {
            return (a.dev(), a.ino()) == (b.dev(), b.ino());
        }
    }
    false
}

/// `png` with its longer side brought down to `size`. A picture already that small is returned
/// as it is; it is decoded anyway, so a broken one is an error rather than an unreadable thumbnail.
fn fit_png(png: Vec<u8>, size: Option<u32>) -> Result<Vec<u8>, String> {
    // The preview comes from the file: it may claim any size.
    let info = craft_image_codecs::inspect_raster(&png).map_err(|e| e.to_string())?.ok_or("invalid PNG preview")?;
    if info.width > 4096 || info.height > 4096 {
        return Err("preview is larger than 4096 pixels".into());
    }
    let decoded = craft_image_codecs::decode(&png).map_err(|e| e.to_string())?;
    let Some(size) = size.filter(|s| decoded.width().max(decoded.height()) > *s) else { return Ok(png) };
    let rgba = image::RgbaImage::from_raw(decoded.width(), decoded.height(), decoded.to_rgba8()).ok_or("invalid PNG pixel data")?;
    let resized = image::DynamicImage::ImageRgba8(rgba).resize(size, size, image::imageops::FilterType::Triangle).to_rgba8();
    let encoded = craft_image_codecs::Image::from_u8(resized.width(), resized.height(), craft_image_codecs::ChannelLayout::Rgba, resized.into_raw())
        .map_err(|e| e.to_string())?;
    craft_image_codecs::encode(&encoded, craft_image_codecs::Format::Png, &Default::default()).map_err(|e| e.to_string())
}

/// `mcp` (headless, in-process engine) or `mcp --connect PORT|HOST:PORT` (drive a running app
/// started with `designcraft --control PORT`). JSON-RPC on stdin/stdout; logs on stderr.
fn mcp(a: &McpArgs, io: &mut Io) -> Result<(), CliError> {
    use designcraft_engine::file_access::{self, AutomationRoots};
    use designcraft_mcp::{Backend, Headless, Server};
    let read: Vec<&Path> = a.read_roots.iter().map(std::path::PathBuf::as_path).collect();
    let write: Vec<&Path> = a.write_roots.iter().map(std::path::PathBuf::as_path).collect();
    let roots = AutomationRoots::new(&read, &write).map_err(CliError::Engine)?;
    let backend: Box<dyn Backend> = match &a.connect {
        Some(c) => Box::new(connect(c)?),
        None => {
            let mut h = Headless::with_document();
            // `--sample` is the operator's own choice, made before the roots come into force.
            if a.sample {
                h.session.execute("file.newSample", &json!({})).map_err(CliError::engine)?;
            }
            if let Some(roots) = roots {
                // Every thread of this process, not only the server's: nothing reaches past the roots.
                file_access::confine_process(roots.clone());
                errln!(io, "designcraft-cli: files confined: {}", roots.describe());
                h = h.with_automation_roots(Some(roots));
            }
            Box::new(h)
        }
    };
    errln!(io, "designcraft-cli: MCP server on stdio ({})", backend.describe());
    let stdin = std::io::stdin();
    Server::new(backend).serve(stdin.lock(), &mut *io.out).map_err(CliError::engine)
}

/// The running app at PORT or HOST:PORT.
fn connect(port: &str) -> Result<designcraft_mcp::Remote, CliError> {
    let addr = designcraft_mcp::control_addr(port);
    designcraft_mcp::Remote::connect(&addr).map_err(|message| CliError::Connect { addr, port: port.to_owned(), message: message.to_string() })
}

fn run(a: &RunArgs, io: &mut Io) -> Result<(), CliError> {
    let mut s = Session::new();
    let mut page = 0usize;
    let mut scale = 1.0f64;
    let mut opened = false;
    let mut pdf_opts = json!({});
    // Earlier --cmd results, for `$N.path` references.
    let mut results: Vec<Value> = Vec::new();
    for step in &a.steps {
        match step {
            RunStep::Open(p) => {
                s.execute("file.open", &json!({"path": unicode(p)?})).map_err(CliError::engine)?;
                opened = true;
            }
            RunStep::Sample => {
                s.execute("file.newSample", &json!({})).map_err(CliError::engine)?;
                opened = true;
            }
            RunStep::Cmd(c) => {
                if !opened {
                    s.execute("file.new", &json!({})).map_err(CliError::engine)?;
                    opened = true;
                }
                let id = c.id.as_str();
                let p = designcraft_engine::script::resolve(&c.params, &results).map_err(|e| CliError::Engine(format!("--cmd {id}: {e}")))?;
                // A key the command's description doesn't name is an error, not a default (#776).
                designcraft_engine::check_params(id, &p).map_err(CliError::engine)?;
                let r = s.execute(id, &p).map_err(CliError::engine)?;
                results.push(r.clone());
                if !r.is_null() {
                    outln!(io, "{}", serde_json::to_string(&r).unwrap_or_default());
                }
            }
            RunStep::Page(n) => page = *n,
            RunStep::Scale(x) => scale = *x,
            // The export adds its path to them.
            RunStep::PdfOptions(o) => pdf_opts = o.clone(),
            RunStep::Export(t) => handle_export(&mut s, unicode(&t.path)?, t.kind, page, scale, &pdf_opts, io)?,
            RunStep::AllPages(dir) => {
                let dir = unicode(dir)?;
                let n = s.doc().map_err(CliError::engine)?.doc.page_count();
                std::fs::create_dir_all(dir).map_err(|source| CliError::File { path: dir.into(), source })?;
                for p in 0..n {
                    export(&mut s, &format!("{dir}/page-{:03}.png", p + 1), p, scale, io)?;
                }
            }
        }
    }
    Ok(())
}

fn handle_export(s: &mut Session, out: &str, kind: Export, page: usize, scale: f64, pdf_opts: &Value, io: &mut Io) -> Result<(), CliError> {
    match kind {
        Export::Image => export(s, out, page, scale, io)?,
        Export::Pdf => {
            let mut p = pdf_opts.clone();
            p["path"] = json!(out);
            let r = s.execute("file.exportPdf", &p).map_err(CliError::engine)?;
            errln!(io, "wrote {out} ({} pages, {} bytes)", r["pages"], r["bytes"]);
            for w in r["warnings"].as_array().into_iter().flatten() {
                errln!(io, "warning: {}", w.as_str().unwrap_or_default());
            }
        }
        Export::Document => {
            s.execute("file.saveAs", &json!({"path": out})).map_err(CliError::engine)?;
            errln!(io, "saved {out}");
        }
        Export::Pptx => {
            let r = s.execute("file.exportPptx", &json!({"path": out})).map_err(CliError::engine)?;
            errln!(io, "wrote {out} ({} slides, {} bytes)", r["slides"], r["bytes"]);
            for w in r["warnings"].as_array().into_iter().flatten() {
                errln!(io, "warning: {}", w.as_str().unwrap_or_default());
            }
        }
        Export::Command(cmd) => {
            let r = s.execute(cmd, &json!({"path": out})).map_err(CliError::engine)?;
            errln!(io, "wrote {out} ({} bytes)", r["bytes"]);
        }
    }
    Ok(())
}

/// A page as PNG, or as JPEG for `.jpg` and `.jpeg`.
fn export(s: &mut Session, out: &str, page: usize, scale: f64, io: &mut Io) -> Result<(), CliError> {
    let st = s.doc().map_err(CliError::engine)?;
    let mut r = designcraft_render::Renderer::new();
    let t = std::time::Instant::now();
    let img = r
        .render_page(&st.doc, &s.cache, page, scale, true, &designcraft_render::RenderOptions { printing_only: true, ..Default::default() })
        .ok_or(CliError::Usage("no such page"))?;
    let jpeg = craft_file_formats::detect(out, &[]) == Some(craft_file_formats::Format::Jpeg);
    let bytes = if jpeg { img.to_jpeg(90) } else { img.to_png() };
    designcraft_engine::atomic::write_atomic(Path::new(out), &bytes).map_err(|source| CliError::File { path: out.into(), source })?;
    errln!(io, "wrote {out} ({}×{}, {:.1} ms, {} glyphs)", img.width, img.height, t.elapsed().as_secs_f64() * 1000.0, r.stats.glyphs);
    Ok(())
}

/// `describe ID`: one command's documentation.
fn describe(a: &DescribeArgs, io: &mut Io) -> Result<(), CliError> {
    let id = a.id.as_str();
    let s = Session::new();
    let all = serde_json::to_value(s.commands()).map_err(CliError::engine)?;
    let c = all.as_array().and_then(|a| a.iter().find(|c| c["id"] == id)).ok_or_else(|| {
        let near: Vec<String> = all
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["id"].as_str())
            .filter(|c| c.split('.').next() == id.split('.').next())
            .map(str::to_string)
            .collect();
        CliError::Engine(format!("no command `{id}`{}", if near.is_empty() { String::new() } else { format!(" (similar: {})", near.join(", ")) }))
    })?;
    // Enablement depends on a live document and selection: not meaningful here.
    let mut c = c.clone();
    if let Some(o) = c.as_object_mut() {
        o.remove("enabled");
        o.remove("disabled_reason");
    }
    outln!(io, "{}", serde_json::to_string_pretty(&c).unwrap_or_default());
    Ok(())
}

/// `script`: run a command script headless (or in the running app with `--connect`).
fn script(a: &ScriptArgs, io: &mut Io) -> Result<(), CliError> {
    use designcraft_mcp::{Backend, Headless};
    let mut setup: Vec<(&str, Value)> = Vec::new();
    for step in &a.setup {
        match step {
            Setup::Open(p) => {
                let p = unicode(p)?;
                let cmd = if craft_file_formats::detect(p, &[]) == Some(craft_file_formats::Format::Idml) { "file.openIdml" } else { "file.open" };
                setup.push((cmd, json!({"path": p})));
            }
            Setup::Sample => setup.push(("file.newSample", json!({}))),
        }
    }
    let text = match a.file.as_deref() {
        None | Some("-") => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).map_err(CliError::engine)?;
            t
        }
        Some(f) => std::fs::read_to_string(f).map_err(|source| CliError::File { path: f.into(), source })?,
    };
    let steps = designcraft_engine::script::parse(&text).map_err(CliError::Engine)?;
    let mut backend: Box<dyn Backend> = match &a.connect {
        Some(c) => Box::new(connect(c)?),
        None => Box::new(Headless::with_document()),
    };
    let b = &mut *backend;
    let exec = |b: &mut dyn Backend, id: &str, p: Value| b.call("engine.execute", json!({"command": id, "params": p}));
    for (c, p) in &setup {
        exec(b, c, p.clone()).map_err(CliError::Engine)?;
    }
    let report = if a.keep_going {
        // Run every step; failures become {"error": …} results.
        let mut results: Vec<Value> = Vec::new();
        let mut failed = None;
        for (i, st) in steps.iter().enumerate() {
            let r = designcraft_engine::script::resolve(&st.params, &results).and_then(|p| exec(b, &st.command, p));
            results.push(r.unwrap_or_else(|e| {
                failed.get_or_insert_with(|| (i, st.command.clone(), e.clone()));
                json!({"error": e, "command": st.command})
            }));
        }
        designcraft_engine::script::Report { results, failed }
    } else {
        designcraft_engine::script::run(&steps, |id, p| exec(b, id, p))
    };
    if report.failed.is_none() {
        if let Some(out) = &a.save {
            exec(b, "file.saveAs", json!({"path": unicode(out)?})).map_err(CliError::Engine)?;
        }
        for target in &a.exports {
            let p = json!({"path": unicode(&target.path)?});
            let r = match target.kind {
                // PNG / JPEG of the first page.
                Export::Image => b.call("app.export", p),
                Export::Pdf => exec(b, "file.exportPdf", p),
                // A copy: the document in the app stays the file it is (`--save` is Save As).
                Export::Document => exec(b, "file.saveACopy", p),
                Export::Pptx => exec(b, "file.exportPptx", p),
                Export::Command(cmd) => exec(b, cmd, p),
            }
            .map_err(CliError::Engine)?;
            // What the export itself reports (effects rasterised, transparency flattened…) goes
            // to stderr, as `run --export` prints it: the report on stdout covers only the steps.
            for w in r["warnings"].as_array().into_iter().flatten() {
                errln!(io, "warning: {}", w.as_str().unwrap_or_default());
            }
        }
    }
    outln!(io, "{}", serde_json::to_string_pretty(&report.to_json()).unwrap_or_default());
    match report.failed {
        Some((index, command, message)) => Err(CliError::Step { index, command, message }),
        None => Ok(()),
    }
}

/// `app`: one command or control-channel method in the running app.
fn app(a: &AppArgs, io: &mut Io) -> Result<(), CliError> {
    use designcraft_mcp::Backend;
    let mut r = connect(&a.port)?;
    let out = match (&a.method, &a.command) {
        (Some(m), _) => r.call(m, a.params.clone()),
        (None, Some(cmd)) => r.call("engine.execute", json!({"command": cmd, "params": a.params})),
        (None, None) => return Err(CliError::Usage("usage: designcraft-cli app [--port PORT] COMMAND [JSON]")),
    }
    .map_err(CliError::Engine)?;
    outln!(io, "{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(())
}
