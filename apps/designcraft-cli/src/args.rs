//! The command line as clap derive types (RFC-0018). Values arrive typed, so the commands never validate a
//! string; what clap cannot say alone is a [`UsageError`], printed and exited like clap's own.
//!
//! `run` and `script` are ordered: an option applies to the exports that follow it, so the repeatable options are
//! kept with their positions and read back in the order they were given ([`RunArgs::steps`], [`ScriptArgs::setup`]).

use std::ffi::OsString;
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{ArgAction, ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use serde_json::Value;

/// The end of every `--help`.
const EXIT_STATUS: &str = "Exit status:\n  0  success\n  1  the command failed\n  2  usage error\n\nA process ended by a signal exits 128 + the signal number (130 for Ctrl-C), as in the shell.";

/// The top-level `--help` ends with the links (the engine's `links`; a test keeps them equal), the environment and the exit status.
const TOP_NOTES: &str = "Community: https://discord.gg/artcraft  ·  https://getartcraft.com/apps/designcraft  ·  https://github.com/storytold/designcraft\n\nEnvironment:\n  DESIGNCRAFT_PERF_DUMP  where `perf --dump` writes its pictures when the flag is not given\n\nExit status:\n  0  success\n  1  the command failed\n  2  usage error\n\nA process ended by a signal exits 128 + the signal number (130 for Ctrl-C), as in the shell.";

/// Headless DesignCraft: run commands, export, script, serve MCP, benchmark and validate
#[derive(Parser, Debug)]
#[command(name = "designcraft-cli", version, propagate_version = true, arg_required_else_help = true, after_help = TOP_NOTES)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Run commands on a document and export pages or files
    ///
    /// Options are read in order: `--page`, `--scale` and `--pdf-options` apply to the exports that follow them (each
    /// needs an `--export` or `--all-pages` after it), `.rtf` and `.txt` hold the story being edited or of the selected
    /// frame, and `--cmd ID=JSON` runs one command (its parameters may refer to earlier results with `$N.path`).
    #[command(after_help = EXIT_STATUS)]
    Run(RunArgs),
    /// List the commands as JSON, optionally only those whose id, label or menu contains FILTER
    #[command(after_help = EXIT_STATUS)]
    Commands(CommandsArgs),
    /// One command: label, menu, shortcut and parameters
    #[command(after_help = EXIT_STATUS)]
    Describe(DescribeArgs),
    /// Run a command script, headless or in the running app
    ///
    /// Runs the script in FILE (stdin when FILE is `-` or missing), headless or, with --connect PORT, in the running app
    /// (designcraft --control PORT). Prints {"completed": n, "results": [...]}, plus failedIndex / failedCommand / error
    /// and a non-zero exit when a step fails.
    #[command(after_long_help = SCRIPT_SYNTAX)]
    Script(ScriptArgs),
    /// Run one command, or one control-channel method, in the running app
    ///
    /// `app [--port PORT] COMMAND [JSON]`, or `app [--port PORT] --method METHOD [JSON]` for any control-channel method
    /// (ui.screenshot, ui.render, …).
    #[command(after_help = EXIT_STATUS)]
    App(AppArgs),
    /// Serve the Model Context Protocol on stdio (docs/mcp.md)
    ///
    /// The automation roots confine every file the agent's commands read (open, placed images, data sources, libraries)
    /// and write (save, export, package) to those folders, links followed. Each may be given more than once; a kind left
    /// out grants none of its access. They confine the in-process server, so they conflict with --connect (a running app
    /// is not confined by them). Nothing but JSON-RPC goes to stdout.
    #[command(after_help = EXIT_STATUS)]
    Mcp(McpArgs),
    /// Budgets on a synthetic stress document
    #[command(after_help = EXIT_STATUS)]
    Perf(PerfArgs),
    /// The same measurements on one document
    #[command(after_help = EXIT_STATUS)]
    Bench(BenchArgs),
    /// The built-in PDF/X-4 or PDF/X-1a checks, including visible unembedded fonts
    #[command(after_help = EXIT_STATUS)]
    ValidatePdf(ValidatePdfArgs),
    /// The first-page preview saved in a `.designcraft` file, as a PNG
    ///
    /// The freedesktop thumbnailer for Linux file managers: it only reads the saved preview, so a file without one fails.
    #[command(after_help = EXIT_STATUS)]
    Thumbnail(ThumbnailArgs),
    /// Discord, website, app page and GitHub links
    #[command(after_help = EXIT_STATUS)]
    Links,
}

/// `script --help`: the script syntax, otherwise only in `describe script.run` and docs/agents.md.
const SCRIPT_SYNTAX: &str = r#"Script text, one step per line:
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

Exit status: 0 success, 1 a step or an output failed, 2 usage error."#;

/// What `--export OUT` writes. The extension of `OUT` picks it, in any case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Export {
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
    fn of(out: &str) -> Option<Self> {
        let ext = std::path::Path::new(out).extension().and_then(std::ffi::OsStr::to_str).map(str::to_lowercase);
        Some(match ext.as_deref() {
            None | Some("png" | "jpg" | "jpeg") => Export::Image,
            Some("pdf") => Export::Pdf,
            Some("designcraft") => Export::Document,
            Some("idml") => Export::Command("file.exportIdml"),
            Some("epub") => Export::Command("file.exportEpub"),
            Some("pptx") => Export::Pptx,
            Some("html" | "htm") => Export::Command("file.exportHtml"),
            // The story being edited or of the selected frame.
            Some("rtf" | "txt") => Export::Command("file.exportText"),
            Some(_) => return None,
        })
    }
}

/// A file `--export` writes and what it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExportTarget {
    pub(crate) path: PathBuf,
    pub(crate) kind: Export,
}

fn export_target(text: &str) -> Result<ExportTarget, UsageError> {
    Export::of(text).map(|kind| ExportTarget { path: PathBuf::from(text), kind }).ok_or_else(|| UsageError::ExportType(text.to_owned()))
}

/// One `--cmd ID[=JSON]`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CmdArg {
    pub(crate) id: String,
    pub(crate) params: Value,
}

fn cmd_arg(text: &str) -> Result<CmdArg, UsageError> {
    let (id, params) = text.split_once('=').unwrap_or((text, "{}"));
    let params = serde_json::from_str(params).map_err(|e| UsageError::CmdJson { id: id.to_owned(), why: e.to_string() })?;
    Ok(CmdArg { id: id.to_owned(), params })
}

/// A JSON object, or null for none.
fn json_object(text: &str) -> Result<Value, UsageError> {
    match serde_json::from_str(text).map_err(|e| UsageError::PdfOptions(e.to_string()))? {
        Value::Null => Ok(serde_json::json!({})),
        object @ Value::Object(_) => Ok(object),
        _ => Err(UsageError::PdfOptions("expected a JSON object".to_owned())),
    }
}

/// Where `--sample` stands on the command line. A counted flag that is absent still reports its default, so an absent
/// one has to be told apart by where its value came from.
fn sample_at(sub: &ArgMatches) -> impl Iterator<Item = usize> {
    let given = sub.value_source("sample") == Some(clap::parser::ValueSource::CommandLine);
    sub.indices_of("sample").into_iter().flatten().filter(move |_| given)
}

fn positive_scale(text: &str) -> Result<f64, UsageError> {
    text.parse().ok().filter(|s: &f64| s.is_finite() && *s > 0.0).ok_or(UsageError::Scale)
}

fn non_empty_folder(text: &str) -> Result<PathBuf, UsageError> {
    if text.is_empty() { Err(UsageError::EmptyFolder) } else { Ok(PathBuf::from(text)) }
}

/// One step of `run`, in the order given.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RunStep {
    Open(PathBuf),
    Sample,
    Cmd(CmdArg),
    Page(usize),
    Scale(f64),
    PdfOptions(Value),
    Export(ExportTarget),
    AllPages(PathBuf),
}

#[derive(Args, Debug)]
pub(crate) struct RunArgs {
    /// Open this document (.designcraft or .idml) first; repeatable
    #[arg(long = "in", value_name = "FILE")]
    ins: Vec<PathBuf>,
    /// Start from the sample document; repeatable
    #[arg(long, action = ArgAction::Count)]
    sample: u8,
    /// Run a command, `ID` or `ID=JSON`; repeatable, in order
    #[arg(long = "cmd", value_name = "ID[=JSON]", value_parser = cmd_arg)]
    cmds: Vec<CmdArg>,
    /// The page the exports that follow write, counting from 0; repeatable
    #[arg(long = "page", value_name = "N", allow_negative_numbers = true)]
    pages: Vec<usize>,
    /// The scale of the raster exports that follow (a positive number); repeatable
    #[arg(long = "scale", value_name = "S", allow_negative_numbers = true, value_parser = positive_scale)]
    scales: Vec<f64>,
    /// PDF options for the exports that follow, as a JSON object; repeatable
    #[arg(long = "pdf-options", value_name = "JSON", value_parser = json_object)]
    pdf_options: Vec<Value>,
    /// Write a file: .png, .jpg, .pdf, .designcraft, .idml, .epub, .pptx, .html, .rtf or .txt; repeatable
    #[arg(long = "export", value_name = "OUT", value_parser = export_target)]
    exports: Vec<ExportTarget>,
    /// Write every page as PNG into this folder; repeatable
    #[arg(long = "all-pages", value_name = "DIR")]
    all_pages: Vec<PathBuf>,
    /// The options in the order they were given.
    #[arg(skip)]
    pub(crate) steps: Vec<RunStep>,
}

/// The values of a repeatable option with the position each was given at.
fn at<T: Clone>(sub: &ArgMatches, id: &str, values: &[T]) -> Vec<(usize, T)> {
    sub.indices_of(id).into_iter().flatten().zip(values.iter().cloned()).collect()
}

impl RunArgs {
    /// Fill `steps` and check what only the whole line can say.
    fn check(&mut self, sub: &ArgMatches) -> Result<(), UsageError> {
        let mut steps: Vec<(usize, RunStep)> = Vec::new();
        steps.extend(at(sub, "ins", &self.ins).into_iter().map(|(i, v)| (i, RunStep::Open(v))));
        steps.extend(sample_at(sub).map(|i| (i, RunStep::Sample)));
        steps.extend(at(sub, "cmds", &self.cmds).into_iter().map(|(i, v)| (i, RunStep::Cmd(v))));
        steps.extend(at(sub, "pages", &self.pages).into_iter().map(|(i, v)| (i, RunStep::Page(v))));
        steps.extend(at(sub, "scales", &self.scales).into_iter().map(|(i, v)| (i, RunStep::Scale(v))));
        steps.extend(at(sub, "pdf_options", &self.pdf_options).into_iter().map(|(i, v)| (i, RunStep::PdfOptions(v))));
        steps.extend(at(sub, "exports", &self.exports).into_iter().map(|(i, v)| (i, RunStep::Export(v))));
        steps.extend(at(sub, "all_pages", &self.all_pages).into_iter().map(|(i, v)| (i, RunStep::AllPages(v))));
        steps.sort_by_key(|(i, _)| *i);
        self.steps = steps.into_iter().map(|(_, s)| s).collect();
        // The options apply to the exports that follow them: one after the last export would do nothing.
        let mut unused = None;
        for step in &self.steps {
            match step {
                RunStep::Page(_) => unused = unused.or(Some("--page")),
                RunStep::Scale(_) => unused = unused.or(Some("--scale")),
                RunStep::PdfOptions(_) => unused = unused.or(Some("--pdf-options")),
                RunStep::Export(_) | RunStep::AllPages(_) => unused = None,
                _ => {}
            }
        }
        match unused {
            Some(option) => Err(UsageError::OptionAfterLastExport(option)),
            None => Ok(()),
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct CommandsArgs {
    /// Only commands whose id, label or menu contains this
    #[arg(value_name = "FILTER")]
    pub(crate) filter: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct DescribeArgs {
    /// The command id
    #[arg(value_name = "COMMAND")]
    pub(crate) id: String,
}

/// One setup step of `script`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Setup {
    Open(PathBuf),
    Sample,
}

#[derive(Args, Debug)]
pub(crate) struct ScriptArgs {
    /// The script (stdin when `-` or missing)
    #[arg(value_name = "FILE")]
    pub(crate) file: Option<String>,
    /// Open this document (.designcraft or .idml) first; repeatable
    #[arg(long = "in", value_name = "FILE")]
    ins: Vec<PathBuf>,
    /// Start from the sample document; repeatable
    #[arg(long, action = ArgAction::Count)]
    sample: u8,
    /// Run in the running app (designcraft --control PORT) instead of headless
    #[arg(long, value_name = "PORT")]
    pub(crate) connect: Option<String>,
    /// Save as .designcraft after the script succeeds
    #[arg(long, value_name = "OUT")]
    pub(crate) save: Option<PathBuf>,
    /// Export after the script succeeds: .pdf, .idml, .epub, .pptx, .png, .jpg…; repeatable
    #[arg(long = "export", value_name = "OUT", value_parser = export_target)]
    pub(crate) exports: Vec<ExportTarget>,
    /// Run every step, recording failures as {"error": ...} results
    #[arg(long)]
    pub(crate) keep_going: bool,
    /// `--in` and `--sample` in the order given.
    #[arg(skip)]
    pub(crate) setup: Vec<Setup>,
}

impl ScriptArgs {
    fn check(&mut self, sub: &ArgMatches) {
        let mut setup: Vec<(usize, Setup)> = at(sub, "ins", &self.ins).into_iter().map(|(i, v)| (i, Setup::Open(v))).collect();
        setup.extend(sample_at(sub).map(|i| (i, Setup::Sample)));
        setup.sort_by_key(|(i, _)| *i);
        self.setup = setup.into_iter().map(|(_, s)| s).collect();
    }
}

#[derive(Args, Debug)]
pub(crate) struct AppArgs {
    /// The running app's control port (designcraft --control PORT)
    #[arg(long, visible_alias = "connect", value_name = "PORT", default_value = "7979")]
    pub(crate) port: String,
    /// Call this control-channel method instead of a command
    #[arg(long, value_name = "METHOD")]
    pub(crate) method: Option<String>,
    /// COMMAND [JSON], or with --method just [JSON]
    #[arg(value_name = "COMMAND [JSON]", num_args = 0..=2)]
    positional: Vec<String>,
    /// The command to run (none with `--method`).
    #[arg(skip)]
    pub(crate) command: Option<String>,
    /// The parameters: a JSON value, `{}` when none.
    #[arg(skip)]
    pub(crate) params: Value,
}

impl AppArgs {
    fn check(&mut self) -> Result<(), UsageError> {
        let mut words = std::mem::take(&mut self.positional).into_iter();
        if self.method.is_none() {
            self.command = Some(words.next().ok_or(UsageError::AppNeedsCommand)?);
        }
        self.params = match words.next() {
            Some(text) => serde_json::from_str(&text).map_err(|e| UsageError::BadJson(e.to_string()))?,
            None => serde_json::json!({}),
        };
        match words.next() {
            None => Ok(()),
            Some(_) => Err(UsageError::AppExtra),
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct McpArgs {
    /// Drive the running app at PORT or HOST:PORT (designcraft --control PORT) instead of the in-process engine
    #[arg(long, value_name = "PORT", conflicts_with_all = ["read_roots", "write_roots"])]
    pub(crate) connect: Option<String>,
    /// Start from the sample document
    #[arg(long)]
    pub(crate) sample: bool,
    /// A folder the agent may read from; repeatable (`--automation-read-root=DIR` for a name that starts with `-`)
    #[arg(long = "automation-read-root", value_name = "DIR", value_parser = non_empty_folder)]
    pub(crate) read_roots: Vec<PathBuf>,
    /// A folder the agent may write to; repeatable
    #[arg(long = "automation-write-root", value_name = "DIR", value_parser = non_empty_folder)]
    pub(crate) write_roots: Vec<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct PerfArgs {
    /// Pages in the synthetic document
    #[arg(long, value_name = "N")]
    pub(crate) pages: Option<usize>,
    /// Text frames per page
    #[arg(long, value_name = "N")]
    pub(crate) frames: Option<usize>,
    /// Characters of text
    #[arg(long, value_name = "N")]
    pub(crate) chars: Option<usize>,
    /// Images
    #[arg(long, value_name = "N")]
    pub(crate) images: Option<usize>,
    /// Repetitions per measurement (the median is reported)
    #[arg(long, value_name = "N")]
    pub(crate) runs: Option<usize>,
    /// Fail when a budget is exceeded on an idle machine
    #[arg(long)]
    pub(crate) strict: bool,
    /// Write the pictures it renders into this folder (a blank value counts as unset)
    #[arg(long, value_name = "DIR", env = "DESIGNCRAFT_PERF_DUMP")]
    pub(crate) dump: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct BenchArgs {
    /// The document
    #[arg(value_name = "FILE")]
    pub(crate) file: PathBuf,
    /// Repetitions per measurement (the median is reported)
    #[arg(long, value_name = "N")]
    pub(crate) runs: Option<usize>,
}

/// The PDF/X standard `validate-pdf --standard` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PdfStandard {
    /// PDF/X-4
    X4,
    /// PDF/X-1a:2003
    X1a,
}

#[derive(Args, Debug)]
pub(crate) struct ValidatePdfArgs {
    /// The PDF
    #[arg(value_name = "FILE.pdf")]
    pub(crate) file: PathBuf,
    /// The standard to check (default: the file's own GTS_PDFXVersion)
    #[arg(long, value_enum)]
    pub(crate) standard: Option<PdfStandard>,
}

#[derive(Args, Debug)]
pub(crate) struct ThumbnailArgs {
    /// The document
    #[arg(value_name = "FILE.designcraft")]
    pub(crate) input: PathBuf,
    /// The PNG to write
    #[arg(value_name = "OUT.png")]
    pub(crate) out: PathBuf,
    /// The longer side in pixels at most (never enlarged)
    #[arg(value_name = "SIZE", value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) size: Option<u32>,
}

/// A mistake in the command line that clap can't see by itself.
#[derive(Debug, PartialEq, thiserror::Error)]
pub(crate) enum UsageError {
    #[error("--export {0}: unknown file type (.png, .jpg, .pdf, .designcraft, .idml, .epub, .pptx, .html, .rtf, .txt)")]
    ExportType(String),
    #[error("--cmd {id}: {why}")]
    CmdJson { id: String, why: String },
    #[error("--pdf-options: {0}")]
    PdfOptions(String),
    #[error("bad JSON: {0}")]
    BadJson(String),
    #[error("bad --scale: expected a positive number")]
    Scale,
    #[error("a folder is needed, not an empty name")]
    EmptyFolder,
    #[error("{0} has no --export or --all-pages after it; options apply to the exports that follow them, so put it before its --export")]
    OptionAfterLastExport(&'static str),
    #[error("app needs a COMMAND (or --method METHOD)")]
    AppNeedsCommand,
    #[error("app takes a COMMAND and its JSON, or --method METHOD and its JSON, and nothing else")]
    AppExtra,
}

impl UsageError {
    /// The clap error, printed with the usage and exited with 2 like clap's own.
    fn into_clap(self, subcommand: &str) -> clap::Error {
        let mut command = Cli::command();
        // Built, so a subcommand's usage names the program: `designcraft-cli run`.
        command.build();
        let mut command = command.find_subcommand_mut(subcommand).map_or_else(Cli::command, std::mem::take);
        command.error(ErrorKind::ArgumentConflict, self.to_string())
    }
}

/// The clap definition of the whole command line (completions and man pages are generated from it).
pub fn command() -> clap::Command {
    Cli::command()
}

/// Parse the arguments (without the program name). Help and the version come back as the `Err` clap uses for them.
pub(crate) fn parse(args: &[OsString]) -> Result<Cli, clap::Error> {
    let matches = Cli::command().try_get_matches_from(std::iter::once(OsString::from("designcraft-cli")).chain(args.iter().cloned()))?;
    let mut cli = Cli::from_arg_matches(&matches)?;
    let sub = matches.subcommand();
    match (&mut cli.command, sub) {
        (Command::Run(run), Some(("run", sub))) => run.check(sub).map_err(|e| e.into_clap("run"))?,
        (Command::Script(script), Some(("script", sub))) => script.check(sub),
        (Command::App(app), _) => app.check().map_err(|e| e.into_clap("app"))?,
        _ => {}
    }
    Ok(cli)
}
