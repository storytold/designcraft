//! Which wgpu backend draws the window, and the fallback when one of them takes the process down
//! at start-up.
//!
//! A fault inside a graphics driver can't be caught: on an AMD hybrid-graphics laptop (a Radeon
//! RX 6800M beside an integrated Radeon, two versions of `amdvlk64.dll` loaded into the process)
//! AMD's Vulkan driver faulted with an access violation at the first swapchain present; creating
//! an OpenGL instance crashed inside AMD's `atio6axx.dll` (#167); and creating a Vulkan instance
//! loads every installed Vulkan driver into the process, of which a faulty one (Intel's
//! `igvk64.dll`, VectorCraft #806) crashed before the window appeared. So each start creates the
//! instance with one backend, chosen before the window exists: on Windows DirectX 12 first, then
//! OpenGL, then Vulkan, each on its own; on Linux Vulkan, with OpenGL beside it as the in-process
//! fallback, then OpenGL alone; on macOS Metal. `WGPU_BACKEND` — wgpu's own variable: `dx12`,
//! `vulkan`, `gl`, `metal`, or a comma-separated list — overrides the choice and turns the
//! fallback off.
//!
//! The fallback: before the window is created, `gpu.json` in the settings directory records the
//! backend being tried; once a frame has been presented, or the graphics failed with an error the
//! app caught (see `gpu::finish`), that entry is cleared. An entry still there at the next start
//! means the previous start never got that far — the driver crashed, or the app was killed
//! meanwhile — so that backend joins `failed`, the next candidate is tried and the status bar says
//! so. Backends a restart left out (`--gpu-skip=`, see `gpu`) aren't chosen for that start but
//! aren't recorded: the restart caught their failure, and a later launch may try them again. When
//! every candidate has failed the list starts over, and so does a new DesignCraft version (drivers
//! and wgpu move on). `DESIGNCRAFT_NO_PREFS` records nothing.

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

    /// How a restart's skip list ([`super::skip_arg`]) names the whole backend: wgpu's name for it, which also
    /// starts each of its adapters' keys (`Dx12:10de:2204`).
    pub fn key(self) -> &'static str {
        match self {
            Backend::Dx12 => "Dx12",
            Backend::Vulkan => "Vulkan",
            Backend::Metal => "Metal",
            Backend::Gl => "Gl",
        }
    }

    /// The backend [`Backend::key`] names; `None` for anything else, such as an adapter's key.
    pub fn from_key(key: &str) -> Option<Backend> {
        [Backend::Dx12, Backend::Vulkan, Backend::Metal, Backend::Gl].into_iter().find(|b| b.key() == key)
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

/// The backends to try on this platform, best first. Windows reaches Vulkan last: creating its
/// instance loads every installed Vulkan driver (VectorCraft #806).
pub fn candidates() -> &'static [Backend] {
    if cfg!(windows) {
        &[Backend::Dx12, Backend::Gl, Backend::Vulkan]
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

/// The backends a restart's skip list leaves out ([`super::skip_arg`]): a restart found no adapter
/// of them that could show the window.
pub fn skipped_backends(skip: &[String]) -> Vec<Backend> {
    skip.iter().filter_map(|k| Backend::from_key(k)).collect()
}

/// Pick the backend for this start from the candidates (best first), the previous start's record
/// and the backends a restart left out (`skipped`, not recorded); `None` without candidates.
pub fn choose(candidates: &[Backend], previous: &Record, version: &str, skipped: &[Backend]) -> Option<Choice> {
    let first = *candidates.first()?;
    let same_version = previous.version == version;
    let mut failed: Vec<Backend> = if same_version { previous.failed.clone() } else { Vec::new() };
    let unfinished = previous.trying.filter(|_| same_version);
    if let Some(b) = unfinished
        && !failed.contains(&b)
    {
        failed.push(b);
    }
    failed.retain(|b| candidates.contains(b));
    let pick = |failed: &[Backend]| candidates.iter().copied().find(|b| !failed.contains(b) && !skipped.contains(b));
    let backend = match pick(&failed) {
        Some(b) => b,
        // Every candidate has failed: start over rather than never starting.
        None => {
            failed.clear();
            pick(&failed).unwrap_or(first)
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

/// See [`Fallback::backends`].
fn instance_backends(backend: Backend, failed: &[Backend]) -> wgpu::Backends {
    let mut backends = backend.bit();
    if !cfg!(windows) && !failed.contains(&Backend::Gl) {
        backends |= wgpu::Backends::GL;
    }
    backends
}

/// The first of `candidates` whose bit isn't in `tried` and that hasn't `failed`.
fn next_candidate(candidates: &[Backend], tried: wgpu::Backends, failed: &[Backend]) -> Option<Backend> {
    candidates.iter().copied().find(|b| !tried.contains(b.bit()) && !failed.contains(b))
}

/// A start in progress: the backend chosen for it and the record that says so until a frame has
/// been presented.
#[derive(Debug)]
pub struct Fallback {
    pub backend: Backend,
    pub switched_from: Option<Backend>,
    /// The backends the instance is created with ([`Fallback::backends`]).
    instance: wgpu::Backends,
    /// `gpu.json`, when the record is persisted.
    path: Option<PathBuf>,
    record: Record,
}

impl Fallback {
    /// Choose the backend for this start and record it in `path` (nothing is recorded without
    /// one), leaving out the backends a restart `skipped`. `None` when `WGPU_BACKEND` is set —
    /// wgpu follows it and the fallback is off — or the platform has no candidates.
    pub fn begin(path: Option<PathBuf>, version: &str, skipped: &[Backend]) -> Option<Fallback> {
        if wgpu::Backends::from_env().is_some_and(|b| !b.is_empty()) {
            log::info!("graphics: WGPU_BACKEND is set, wgpu picks the backend and the start-up fallback is off");
            return None;
        }
        Self::begin_unforced(path, version, skipped)
    }

    fn begin_unforced(path: Option<PathBuf>, version: &str, skipped: &[Backend]) -> Option<Fallback> {
        let previous = path.as_deref().map(read).unwrap_or_default();
        let choice = choose(candidates(), &previous, version, skipped)?;
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
        let instance = instance_backends(choice.backend, &choice.record.failed);
        Some(Fallback { backend: choice.backend, switched_from: choice.switched_from, instance, path, record: choice.record })
    }

    /// The backends wgpu may use: the chosen one, with OpenGL as the in-process fallback when it
    /// has no adapter (unless OpenGL itself has failed). Not on Windows: some OpenGL drivers
    /// (AMD's) crash while the instance is created (#167), before any fallback could help, so
    /// there OpenGL is only ever tried as its own candidate.
    pub fn backends(&self) -> wgpu::Backends {
        self.instance
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

    /// The backend to start again with when no adapter of this start's backends could show the
    /// window: the next candidate not among them, not failed and not left out by a restart
    /// (`skipped`). `None` when there is none, so the restarts end.
    pub fn next_after(&self, skipped: &[Backend]) -> Option<Backend> {
        let tried = skipped.iter().fold(self.backends(), |set, b| set | b.bit());
        next_candidate(candidates(), tried, &self.record.failed)
    }

    /// The start got past the point where a driver fault could take the process down: a frame was
    /// presented, the app exited normally, or the graphics failed with an error the app caught.
    /// Returns the backend that was being tried, for [`Fallback::retry`].
    pub fn finished(&mut self) -> Option<Backend> {
        let trying = self.record.trying.take();
        if trying.is_some() {
            self.save();
        }
        trying
    }

    /// Undo [`Fallback::finished`]: the start isn't over after all (starting again failed).
    pub fn retry(&mut self, trying: Option<Backend>) {
        if self.record.trying != trying {
            self.record.trying = trying;
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
    const WIN: &[Backend] = &[Backend::Dx12, Backend::Gl, Backend::Vulkan];

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
            assert_eq!(c, WIN, "DirectX 12 first (AMD's Vulkan driver faulted at the first present), Vulkan last (#806)");
        }
        if cfg!(target_os = "macos") {
            assert_eq!(c, &[Backend::Metal]);
        }
    }

    #[test]
    fn a_fresh_start_takes_the_first_candidate() {
        let c = choose(WIN, &Record::default(), V, &[]).unwrap();
        assert_eq!(c.backend, Backend::Dx12);
        assert_eq!(c.record, record(Some(Backend::Dx12), &[]));
        assert_eq!(c.switched_from, None);
        assert!(choose(&[], &Record::default(), V, &[]).is_none());
    }

    #[test]
    fn a_start_that_never_presented_moves_to_the_next_candidate() {
        let c = choose(WIN, &record(Some(Backend::Dx12), &[]), V, &[]).unwrap();
        assert_eq!(c.backend, Backend::Gl);
        assert_eq!(c.record, record(Some(Backend::Gl), &[Backend::Dx12]));
        assert_eq!(c.switched_from, Some(Backend::Dx12));

        let c = choose(WIN, &record(Some(Backend::Gl), &[Backend::Dx12]), V, &[]).unwrap();
        assert_eq!(c.backend, Backend::Vulkan);
        assert_eq!(c.record.failed, &[Backend::Dx12, Backend::Gl]);
        assert_eq!(c.switched_from, Some(Backend::Gl));
    }

    #[test]
    fn a_finished_start_keeps_the_failed_list_and_switches_silently() {
        // `trying` was cleared by a presented frame; the next start still avoids what failed.
        let c = choose(WIN, &record(None, &[Backend::Dx12]), V, &[]).unwrap();
        assert_eq!(c.backend, Backend::Gl);
        assert_eq!(c.record.failed, &[Backend::Dx12]);
        assert_eq!(c.switched_from, None);
        // The same backend failing again is listed once.
        let c = choose(WIN, &record(Some(Backend::Dx12), &[Backend::Dx12]), V, &[]).unwrap();
        assert_eq!(c.record.failed, &[Backend::Dx12]);
    }

    #[test]
    fn when_every_candidate_failed_the_list_starts_over() {
        let c = choose(WIN, &record(Some(Backend::Vulkan), &[Backend::Dx12, Backend::Gl]), V, &[]).unwrap();
        assert_eq!(c.backend, Backend::Dx12);
        assert_eq!(c.record.failed, &[]);
        assert_eq!(c.switched_from, Some(Backend::Vulkan));
    }

    #[test]
    fn another_version_or_platform_forgets_what_failed() {
        let old = Record { version: "0.4.0".into(), trying: Some(Backend::Dx12), failed: vec![Backend::Vulkan] };
        let c = choose(WIN, &old, V, &[]).unwrap();
        assert_eq!(c.backend, Backend::Dx12);
        assert_eq!(c.record, record(Some(Backend::Dx12), &[]));
        assert_eq!(c.switched_from, None);
        // A backend that is no candidate here (a record copied from another machine) is dropped.
        let c = choose(&[Backend::Vulkan, Backend::Gl], &record(None, &[Backend::Metal, Backend::Vulkan]), V, &[]).unwrap();
        assert_eq!(c.backend, Backend::Gl);
        assert_eq!(c.record.failed, &[Backend::Vulkan]);
    }

    /// A restart that found no adapter of DirectX 12 names the backend in its skip list: that start
    /// takes the next candidate, with or without a record, and the record doesn't keep it as failed
    /// (a later launch tries it again).
    #[test]
    fn backends_a_restart_left_out_are_not_chosen_or_recorded() {
        let skip: Vec<String> = ["Dx12:10de:2204", "Dx12", "Metal:Intel Iris Pro Graphics", "dx12", ""].map(String::from).to_vec();
        assert_eq!(skipped_backends(&skip), [Backend::Dx12]);
        // A backend's key is wgpu's name for it, the prefix of its adapters' keys.
        for w in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan, wgpu::Backend::Metal, wgpu::Backend::Gl] {
            let b = Backend::of(w).expect("native backend");
            assert_eq!(b.key(), format!("{w:?}"));
            assert_eq!(Backend::from_key(b.key()), Some(b));
        }
        let c = choose(WIN, &Record::default(), V, &[Backend::Dx12]).unwrap();
        assert_eq!(c.backend, Backend::Gl);
        assert_eq!(c.record, record(Some(Backend::Gl), &[]));
        let c = choose(WIN, &record(None, &[Backend::Gl]), V, &[Backend::Dx12]).unwrap();
        assert_eq!(c.record, record(Some(Backend::Vulkan), &[Backend::Gl]));
        // Every candidate failed or left out: the record starts over, still without the left-out one.
        let c = choose(WIN, &record(Some(Backend::Vulkan), &[Backend::Gl]), V, &[Backend::Dx12]).unwrap();
        assert_eq!(c.record, record(Some(Backend::Gl), &[]));
        // A backend that is no candidate here is ignored.
        let c = choose(WIN, &Record::default(), V, &[Backend::Metal]).unwrap();
        assert_eq!(c.backend, Backend::Dx12);
    }

    /// When no adapter of a start's backends could show the window, the restart moves on to a
    /// backend not yet tried in this start (an in-process OpenGL fallback counts as tried), and
    /// the restarts end once none is left.
    #[test]
    fn the_next_backend_after_one_without_a_working_adapter() {
        let none = wgpu::Backends::empty();
        assert_eq!(next_candidate(WIN, wgpu::Backends::DX12, &[]), Some(Backend::Gl));
        assert_eq!(next_candidate(WIN, wgpu::Backends::DX12 | wgpu::Backends::GL, &[]), Some(Backend::Vulkan));
        assert_eq!(next_candidate(WIN, wgpu::Backends::DX12, &[Backend::Gl]), Some(Backend::Vulkan));
        assert_eq!(next_candidate(WIN, wgpu::Backends::all(), &[]), None);
        assert_eq!(next_candidate(WIN, none, &[Backend::Dx12, Backend::Gl, Backend::Vulkan]), None);
        // Linux: Vulkan with OpenGL beside it has tried both.
        assert_eq!(next_candidate(&[Backend::Vulkan, Backend::Gl], wgpu::Backends::VULKAN | wgpu::Backends::GL, &[]), None);
        assert_eq!(next_candidate(&[Backend::Metal], wgpu::Backends::METAL | wgpu::Backends::GL, &[]), None);

        let s = Fallback::begin_unforced(None, V, &[]).expect("candidates");
        let next = s.next_after(&[]);
        if cfg!(windows) {
            assert_eq!(next, Some(Backend::Gl));
            assert_eq!(s.next_after(&[Backend::Gl]), Some(Backend::Vulkan));
            assert_eq!(s.next_after(&[Backend::Gl, Backend::Vulkan]), None);
        } else {
            assert_eq!(next, None, "the first start already has OpenGL beside its backend");
        }
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
        let mut s = Fallback::begin_unforced(Some(path.clone()), V, &[]).expect("candidates");
        let first = candidates()[0];
        assert_eq!(s.backend, first);
        assert_eq!(read(&path).trying, Some(first));
        assert!(s.backends().contains(first.bit()));
        // OpenGL is the in-process fallback, except on Windows where it is only its own candidate
        // (#167), and Windows never loads Vulkan drivers on its first try (#806).
        assert_eq!(s.backends().contains(wgpu::Backends::GL), !cfg!(windows) || first == Backend::Gl);
        if cfg!(windows) {
            assert_eq!(s.backends(), wgpu::Backends::DX12);
        }
        assert_eq!(s.status_line(), None);

        // wgpu actually picked OpenGL (the chosen backend had no adapter): blame that one.
        s.started_with(Backend::Gl);
        assert_eq!(read(&path).trying, Some(Backend::Gl));
        // A restart takes the entry and puts it back when starting again failed.
        let trying = s.finished();
        assert_eq!((trying, read(&path)), (Some(Backend::Gl), record(None, &[])));
        s.retry(trying);
        assert_eq!(read(&path).trying, Some(Backend::Gl));
        s.finished();
        assert_eq!(read(&path), record(None, &[]));
        assert_eq!(s.finished(), None);
        assert_eq!(read(&path), record(None, &[]));

        // The next start after one that never presented: switches, says so, and leaves OpenGL
        // out of the set once it failed.
        write(&path, &record(Some(Backend::Gl), &[])).expect("write");
        let s = Fallback::begin_unforced(Some(path.clone()), V, &[]).expect("candidates");
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
        let mut s = Fallback::begin_unforced(None, V, &[]).expect("candidates");
        s.started_with(Backend::Gl);
        s.finished();
        // A file where the directory should be: every write fails, nothing panics.
        let blocked = temp_path("blocked");
        let dir = blocked.parent().expect("dir");
        std::fs::create_dir_all(dir.parent().expect("tmp")).expect("tmp");
        std::fs::write(dir, "a file where the directory should be").expect("block");
        let mut s = Fallback::begin_unforced(Some(blocked.clone()), V, &[]).expect("candidates");
        s.started_with(Backend::Gl);
        s.finished();
        let _ = std::fs::remove_file(dir);
    }
}
