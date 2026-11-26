//! Headless DesignCraft (`designcraft-cli`). [`run`] is the whole program, so it can be tested in-process as well as
//! through the binary; `designcraft-cli --help` lists the subcommands. The conventions are RFC-0018's: exit 0, 1 (a
//! command failed) or 2 (a usage error), results on stdout, `error: <message>` on stderr. The synthetic stress
//! document of `designcraft-cli perf` ([`stress`]) is shared with the PDF export benchmark (`examples/pdf_export.rs`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod stress;

mod args;
mod commands;
mod error;
mod perf;

use std::ffi::OsString;
use std::io::Write;

use error::CliError;

pub use args::command;

/// Where a command writes: results to `out`, diagnostics and progress to `err`.
pub(crate) struct Io<'a> {
    pub(crate) out: &'a mut dyn Write,
    pub(crate) err: &'a mut dyn Write,
}

/// Writes a line of results; fails with [`CliError::Stdout`], which `run` turns into a quiet end for a closed pipe
/// (`designcraft-cli commands | head`) and a message for any other error.
macro_rules! outln {
    ($io:expr, $($arg:tt)*) => {
        writeln!($io.out, $($arg)*).map_err($crate::error::CliError::Stdout)?
    };
}

/// Writes a line of progress or a warning; a closed stderr is not worth an error.
macro_rules! errln {
    ($io:expr, $($arg:tt)*) => {
        drop(writeln!($io.err, $($arg)*))
    };
}

pub(crate) use {errln, outln};

/// Run the CLI on its arguments (without the program name), writing results to `out` and diagnostics to `err`.
/// Returns the process exit code: 0 on success, 1 when a command failed, 2 for a usage error. Help and the version
/// go to `out`, exit 0.
pub fn run<I, T>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    // The Launcher's nightly-v1 profile is a desktop-only contract. Never let its arguments fall
    // through to the CLI's directory and credential discovery (which would read the stable
    // profile), and answer before any subcommand parses them. A file name after `--` is a file.
    if args.iter().take_while(|arg| *arg != "--").any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--launch-profile" || arg == "--profile-root" || arg.starts_with("--launch-profile=") || arg.starts_with("--profile-root=")
    }) {
        let _ = writeln!(err, "error: nightly-v1 is supported only by the DesignCraft desktop launcher protocol");
        return 2;
    }
    // Help and the version (`help`, `-h`, `<subcommand> --help`, `-V`) come back as errors too.
    let cli = match args::parse(&args) {
        Ok(cli) => cli,
        Err(e) => {
            let to: &mut dyn Write = if e.use_stderr() { err } else { out };
            let _ = write!(to, "{}", e.render());
            return e.exit_code();
        }
    };
    let mut io = Io { out, err };
    match commands::dispatch(cli.command, &mut io) {
        Ok(()) => 0,
        // A reader that stopped early (a closed pipe) ends the program quietly, as ripgrep does.
        Err(CliError::Stdout(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            errln!(io, "error: {e}");
            1
        }
    }
}
