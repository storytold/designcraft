use std::path::{Path, PathBuf};

/// Why a command failed: printed as `error: {e}`, exit 1. The engine, the MCP backends and the document code report
/// their failures as text; [`CliError::Engine`] carries it.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    /// What the engine, a backend or a library reported.
    #[error("{0}")]
    Engine(String),
    /// The engine's commands take paths as JSON, which holds only text.
    #[error("{}: the path is not valid Unicode", .0.display())]
    NotUnicode(PathBuf),
    #[error("{}: {source}", path.display())]
    File { path: PathBuf, source: std::io::Error },
    #[error("cannot connect to the DesignCraft app at {addr}: {message} (start it with `designcraft --control {port}`)")]
    Connect { addr: String, port: String, message: String },
    #[error("{} failed {standard} validation", path.display())]
    Invalid { path: PathBuf, standard: &'static str },
    #[error("step {index} ({command}) failed: {message}")]
    Step { index: usize, command: String, message: String },
    #[error("{} is the document {}: the thumbnail would replace it", out.display(), input.display())]
    WouldReplace { out: PathBuf, input: PathBuf },
    #[error("{} has no preview (a DesignCraft file saved before they had one, or not a DesignCraft file)", .0.display())]
    NoPreview(PathBuf),
    #[error("{}: the preview can't be read: {why}", path.display())]
    Preview { path: PathBuf, why: String },
    #[error("{0}")]
    Usage(&'static str),
    #[error("{0} budget(s) exceeded")]
    BudgetsExceeded(usize),
    /// Standard output went away.
    #[error("can't write to stdout: {0}")]
    Stdout(std::io::Error),
}

impl CliError {
    pub(crate) fn engine(e: impl ToString) -> Self {
        Self::Engine(e.to_string())
    }
}

/// A path as the text the engine's commands take.
pub(crate) fn unicode(path: &Path) -> Result<&str, CliError> {
    path.to_str().ok_or_else(|| CliError::NotUnicode(path.to_owned()))
}
