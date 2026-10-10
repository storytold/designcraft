//! Which wgpu backend draws the window, and the fallback when one of them crashes at start-up.
//!
//! The canvas is rendered on the CPU (`designcraft-render`); the GPU only composites the
//! interface, so every backend shows the same pixels and the one that never crashes is the right
//! one. wgpu's default backend set (`PRIMARY | GL`) enumerates Vulkan first, and on an AMD
//! hybrid-graphics laptop (a Radeon RX 6800M beside an integrated Radeon, two versions of
//! `amdvlk64.dll` loaded into the process) AMD's Vulkan driver faulted with an access violation
//! at the first swapchain present. A fault inside a driver can't be caught (the crash guard never
//! sees it), so the backend is chosen before the window exists: on Windows DirectX 12 first (the
//! backend browsers' WebGPU uses), then Vulkan, then OpenGL; on Linux Vulkan, then OpenGL; on
//! macOS Metal. `WGPU_BACKEND` — wgpu's own variable: `dx12`, `vulkan`, `gl`, `metal`, or a
//! comma-separated list — overrides the choice and turns the fallback off.
//!
//! The fallback: before the window is created, `gpu.json` in the settings directory records the
//! backend being tried; once a frame has been presented, that entry is cleared. An entry still
//! there at the next start means the previous start never got that far — the driver crashed, or
//! the app was killed meanwhile — so that backend joins `failed`, the next candidate is tried and
//! the status bar says so. When every candidate has failed the list starts over, and so does a
//! new DesignCraft version (drivers and wgpu move on). `DESIGNCRAFT_NO_PREFS` records nothing.

use std::path::{Path, PathBuf};

use eframe::wgpu;
use serde::{Deserialize, Serialize};

/// The record's file name inside the settings directory.
pub const FILE: &str = "gpu.json";

/// A graphics backend wgpu can draw the window with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Dx12,
    Vulkan,
    Metal,
    Gl,
}

impl Backend {
    /// The name the status bar and the log use.
    pub fn name(self) -> &'static str {
        match self {
            Backend::Dx12 => "DirectX 12",
            Backend::Vulkan => "Vulkan",
            Backend::Metal => "Metal",
            Backend::Gl => "OpenGL",
        }
    }

    /// wgpu's bit for this backend.
    pub fn bit(self) -> wgpu::Backends {
        match self {
            Backend::Dx12 => wgpu::Backends::DX12,
            Backend::Vulkan => wgpu::Backends::VULKAN,
            Backend::Metal => wgpu::Backends::METAL,
            Backend::Gl => wgpu::Backends::GL,
        }
    }

    /// The backend of the adapter wgpu picked; `None` for the web and no-op backends.
    pub fn of(backend: wgpu::Backend) -> Option<Backend> {
        match backend {
            wgpu::Backend::Dx12 => Some(Backend::Dx12),
            wgpu::Backend::Vulkan => Some(Backend::Vulkan),
            wgpu::Backend::Metal => Some(Backend::Metal),
            wgpu::Backend::Gl => Some(Backend::Gl),
            _ => None,
        }
    }
}

/// The backends to try on this platform, best first.
pub fn candidates() -> &'static [Backend] {
    if cfg!(windows) {
        &[Backend::Dx12, Backend::Vulkan, Backend::Gl]
    } else if cfg!(target_os = "macos") {
        &[Backend::Metal]
    } else {
        &[Backend::Vulkan, Backend::Gl]
    }
}

/// What `gpu.json` holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The DesignCraft version that wrote the record; another version starts afresh.
    #[serde(default)]
    pub version: String,
    /// The backend of a start that has not presented a frame yet.
    #[serde(default)]
    pub trying: Option<Backend>,
    /// Backends whose start never presented a frame.
    #[serde(default)]
    pub failed: Vec<Backend>,
}

/// The backend for a start, from [`choose`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub backend: Backend,
    /// The record to write before the window is created.
    pub record: Record,
    /// The backend the previous start never finished with, when this start moves away from it.
    pub switched_from: Option<Backend>,
}

/// Pick the backend for this start from the candidates (best first) and the previous start's
/// record; `None` without candidates.
pub fn choose(candidates: &[Backend], previous: &Record, version: &str) -> Option<Choice> {
    let first = *candidates.first()?;
    let same_version = previous.version == version;
    let mut failed: Vec<Backend> = if same_version { previous.failed.clone() } else { Vec::new() };
    failed.retain(|b| candidates.contains(b));
    let unfinished = previous.trying.filter(|_| same_version);
    if let Some(b) = unfinished
        && !failed.contains(&b)
    {
        failed.push(b);
    }
    let backend = match candidates.iter().copied().find(|b| !failed.contains(b)) {
        Some(b) => b,
        // Every candidate has failed: start over rather than never starting.
        None => {
            failed.clear();
            first
        }
    };
    let switched_from = unfinished.filter(|b| *b != backend);
    Some(Choice { backend, record: Record { version: version.to_owned(), trying: Some(backend), failed }, switched_from })
}

/// The record in `path`; a missing or unreadable file is an empty record.
pub fn read(path: &Path) -> Record {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Write `record` to `path` (creating the directory).
pub fn write(path: &Path, record: &Record) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

/// A start in progress: the backend chosen for it and the record that says so until a frame has
/// been presented.
#[derive(Debug)]
pub struct Startup {
    pub backend: Backend,
    pub switched_from: Option<Backend>,
    /// `gpu.json`, when the record is persisted.
    path: Option<PathBuf>,
    record: Record,
}

impl Startup {
    /// Choose the backend for this start and record it in `path` (nothing is recorded without
    /// one). `None` when `WGPU_BACKEND` is set — wgpu follows it and the fallback is off — or the
    /// platform has no candidates.
    pub fn begin(path: Option<PathBuf>, version: &str) -> Option<Startup> {
        if wgpu::Backends::from_env().is_some_and(|b| !b.is_empty()) {
            log::info!("graphics: WGPU_BACKEND is set, wgpu picks the backend and the start-up fallback is off");
            return None;
        }
        Self::begin_unforced(path, version)
    }

    fn begin_unforced(path: Option<PathBuf>, version: &str) -> Option<Startup> {
        let previous = path.as_deref().map(read).unwrap_or_default();
        let choice = choose(candidates(), &previous, version)?;
        if let Some(p) = &path
            && let Err(e) = write(p, &choice.record)
        {
            log::warn!("graphics: {}: {e}", p.display());
        }
        match choice.switched_from {
            Some(from) => log::warn!(
                "graphics: the last start with {} never showed a frame; trying {} (failed so far: {:?})",
                from.name(),
                choice.backend.name(),
                choice.record.failed
            ),
            None => log::info!("graphics: {} (WGPU_BACKEND overrides)", choice.backend.name()),
        }
        Some(Startup { backend: choice.backend, switched_from: choice.switched_from, path, record: choice.record })
    }

    /// The backends wgpu may use: the chosen one, with OpenGL as the in-process fallback when it
    /// has no adapter (unless OpenGL itself has failed). Not on Windows: some OpenGL drivers
    /// (AMD's) crash while the instance is created (#167), before any fallback could help, so
    /// there OpenGL is only ever tried as its own candidate.
    pub fn backends(&self) -> wgpu::Backends {
        let mut backends = self.backend.bit();
        if !cfg!(windows) && !self.record.failed.contains(&Backend::Gl) {
            backends |= wgpu::Backends::GL;
        }
        backends
    }

    /// wgpu picked an adapter of `backend`: record that one, so a crash is blamed on the backend
    /// that is actually drawing.
    pub fn started_with(&mut self, backend: Backend) {
        if self.record.trying == Some(backend) {
            return;
        }
        self.backend = backend;
        self.record.trying = Some(backend);
        self.save();
    }

    /// A frame has been presented (or the app exited normally): this start did not crash.
    pub fn presented(&mut self) {
        if self.record.trying.take().is_some() {
            self.save();
        }
    }

    /// The status-bar line for a start that moved away from a backend that crashed.
    pub fn status_line(&self) -> Option<String> {
        let from = self.switched_from?;
        Some(format!(
            "Graphics: switched from {} to {} — the last start with {} never showed a frame (set WGPU_BACKEND to choose).",
            from.name(),
            self.backend.name(),
            from.name()
        ))
    }

    fn save(&self) {
        if let Some(p) = &self.path
            && let Err(e) = write(p, &self.record)
        {
            log::warn!("graphics: {}: {e}", p.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const V: &str = "0.5.0";
    const WIN: &[Backend] = &[Backend::Dx12, Backend::Vulkan, Backend::Gl];

    fn temp_path(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("designcraft-gpu-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        d.join(FILE)
    }

    fn record(trying: Option<Backend>, failed: &[Backend]) -> Record {
        Record { version: V.into(), trying, failed: failed.to_vec() }
    }

    #[test]
    fn every_platform_has_candidates_without_duplicates() {
        let c = candidates();
        assert!(!c.is_empty());
        for (i, b) in c.iter().enumerate() {
            assert!(!c[..i].contains(b), "{b:?} twice in {c:?}");
        }
        if cfg!(windows) {
            assert_eq!(c[0], Backend::Dx12, "DirectX 12 is the Windows default (AMD's Vulkan driver faulted at the first present)");
            assert!(!c.contains(&Backend::Metal));
        }
        if cfg!(target_os = "macos") {
            assert_eq!(c, &[Backend::Metal]);
        }
    }

    #[test]
    fn a_fresh_start_takes_the_first_candidate() {
        let c = choose(WIN, &Record::default(), V).unwrap();
        assert_eq!(c.backend, Backend::Dx12);
        assert_eq!(c.record, record(Some(Backend::Dx12), &[]));
        assert_eq!(c.switched_from, None);
        assert!(choose(&[], &Record::default(), V).is_none());
    }

    #[test]
    fn a_start_that_never_presented_moves_to_the_next_candidate() {
        let c = choose(WIN, &record(Some(Backend::Dx12), &[]), V).unwrap();
        assert_eq!(c.backend, Backend::Vulkan);
        assert_eq!(c.record, record(Some(Backend::Vulkan), &[Backend::Dx12]));
        assert_eq!(c.switched_from, Some(Backend::Dx12));

        let c = choose(WIN, &record(Some(Backend::Vulkan), &[Backend::Dx12]), V).unwrap();
        assert_eq!(c.backend, Backend::Gl);
        assert_eq!(c.record.failed, &[Backend::Dx12, Backend::Vulkan]);
        assert_eq!(c.switched_from, Some(Backend::Vulkan));
    }

    #[test]
    fn a_finished_start_keeps_the_failed_list_and_switches_silently() {
        // `trying` was cleared by a presented frame; the next start still avoids what failed.
        let c = choose(WIN, &record(None, &[Backend::Dx12]), V).unwrap();
        assert_eq!(c.backend, Backend::Vulkan);
        assert_eq!(c.record.failed, &[Backend::Dx12]);
        assert_eq!(c.switched_from, None);
        // The same backend failing again is listed once.
        let c = choose(WIN, &record(Some(Backend::Dx12), &[Backend::Dx12]), V).unwrap();
        assert_eq!(c.record.failed, &[Backend::Dx12]);
    }

    #[test]
    fn when_every_candidate_failed_the_list_starts_over() {
        let c = choose(WIN, &record(Some(Backend::Gl), &[Backend::Dx12, Backend::Vulkan]), V).unwrap();
        assert_eq!(c.backend, Backend::Dx12);
        assert_eq!(c.record.failed, &[]);
        assert_eq!(c.switched_from, Some(Backend::Gl));
    }

    #[test]
    fn another_version_or_platform_forgets_what_failed() {
        let old = Record { version: "0.4.0".into(), trying: Some(Backend::Dx12), failed: vec![Backend::Vulkan] };
        let c = choose(WIN, &old, V).unwrap();
        assert_eq!(c.backend, Backend::Dx12);
        assert_eq!(c.record, record(Some(Backend::Dx12), &[]));
        assert_eq!(c.switched_from, None);
        // A backend that is no candidate here (a record copied from another machine) is dropped.
        let c = choose(&[Backend::Vulkan, Backend::Gl], &record(None, &[Backend::Metal, Backend::Vulkan]), V).unwrap();
        assert_eq!(c.backend, Backend::Gl);
        assert_eq!(c.record.failed, &[Backend::Vulkan]);
    }

    #[test]
    fn the_record_round_trips_and_garbage_reads_as_empty() {
        let path = temp_path("roundtrip");
        assert_eq!(read(&path), Record::default());
        let r = record(Some(Backend::Vulkan), &[Backend::Dx12]);
        write(&path, &r).expect("write");
        assert_eq!(read(&path), r);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("\"trying\": \"vulkan\"") && text.contains("\"dx12\""), "{text}");
        std::fs::write(&path, b"{not json").expect("garbage");
        assert_eq!(read(&path), Record::default());
        // Unknown fields and missing ones are tolerated.
        std::fs::write(&path, br#"{"failed":["gl"],"later":1}"#).expect("partial");
        assert_eq!(read(&path), Record { version: String::new(), trying: None, failed: vec![Backend::Gl] });
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn a_start_records_its_backend_until_a_frame_is_presented() {
        let path = temp_path("startup");
        let mut s = Startup::begin_unforced(Some(path.clone()), V).expect("candidates");
        let first = candidates()[0];
        assert_eq!(s.backend, first);
        assert_eq!(read(&path).trying, Some(first));
        assert!(s.backends().contains(first.bit()));
        // OpenGL is the in-process fallback, except on Windows where it is only its own candidate.
        assert_eq!(s.backends().contains(wgpu::Backends::GL), !cfg!(windows) || first == Backend::Gl);
        assert_eq!(s.status_line(), None);

        // wgpu actually picked OpenGL (the chosen backend had no adapter): blame that one.
        s.started_with(Backend::Gl);
        assert_eq!(read(&path).trying, Some(Backend::Gl));
        s.presented();
        assert_eq!(read(&path), record(None, &[]));
        s.presented();
        assert_eq!(read(&path), record(None, &[]));

        // The next start after one that never presented: switches, says so, and leaves OpenGL
        // out of the set once it failed.
        write(&path, &record(Some(Backend::Gl), &[])).expect("write");
        let s = Startup::begin_unforced(Some(path.clone()), V).expect("candidates");
        if candidates().len() > 1 {
            assert_eq!(s.switched_from, Some(Backend::Gl));
            let line = s.status_line().expect("status");
            assert!(line.contains("OpenGL") && line.contains(s.backend.name()) && line.contains("WGPU_BACKEND"), "{line}");
            assert!(!s.backends().contains(wgpu::Backends::GL));
        } else {
            assert_eq!(s.switched_from, None);
        }
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn without_a_path_nothing_is_written_and_a_bad_path_is_not_fatal() {
        let mut s = Startup::begin_unforced(None, V).expect("candidates");
        s.started_with(Backend::Gl);
        s.presented();
        // A file where the directory should be: every write fails, nothing panics.
        let blocked = temp_path("blocked");
        let dir = blocked.parent().expect("dir");
        std::fs::create_dir_all(dir.parent().expect("tmp")).expect("tmp");
        std::fs::write(dir, "a file where the directory should be").expect("block");
        let mut s = Startup::begin_unforced(Some(blocked.clone()), V).expect("candidates");
        s.started_with(Backend::Gl);
        s.presented();
        let _ = std::fs::remove_file(dir);
    }
}
