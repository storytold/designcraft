//! Plug-ins: WebAssembly modules that act on the document through the same commands as the menus,
//! the CLI, the control channel and MCP, inside the sandbox of `craft-plugin` (RFC-0046). This
//! module is DesignCraft's side of it: which commands a plug-in may call ([`policy`]), one undo step
//! per run (rolled back when the run fails), read-only access checked after every call, the
//! plug-ins' settings (kept with the preferences) and the installed plug-ins (the user's
//! `Plug-ins` folder, or this session only). The `plugin.*` commands are in `cmd::plugins`. See
//! `docs/plugins.md`.

use std::sync::{Arc, OnceLock};

pub use craft_plugin;
use craft_plugin::fetch::{self, Fetch};
use craft_plugin::{API_VERSION, AppInfo, Instance, Limits, Manifest, Policy, RunOutput, UiChange, bundle, catalog, github};
use serde_json::{Map, Value, json};

use crate::{EngineError, HistoryEntry, Result, Session, UiRequest};

/// DesignCraft as manifests name it: the command plug-ins of every shared ABI version, and only
/// manifests that name `designcraft` in `apps`.
pub const APP: AppInfo<'static> =
    AppInfo { id: "designcraft", version: env!("CARGO_PKG_VERSION"), api: 1..=API_VERSION, kinds: &["command"], accept_legacy: false, kind_api: &[] };

/// What one plug-in may keep in its settings (keys and JSON values, in bytes), and its longest key.
pub const MAX_SETTINGS_BYTES: usize = 64 * 1024;
pub const MAX_SETTING_KEY: usize = 128;
/// Catalogs a person may add.
pub const MAX_CATALOGS: usize = 32;
/// Messages a run may show (`notify`), and their length in characters.
const MAX_NOTES: usize = 20;
const MAX_NOTE_CHARS: usize = 1000;

/// DesignCraft's commands no plug-in may call, on top of [`craft_plugin::ALWAYS_DENIED`] (plug-ins,
/// Undo and Redo, `file.*`, `app.*`, `pref.*`, `window.*`). A plug-in gets no path, no file outside
/// the document, no other document, no preferences and no undo history, and a run is already one
/// undo step, so nothing that nests commands either.
const DENIED: &[&str] = &[
    // Other open documents, and the document's undo history.
    "document.list",
    "document.history",
    // Preferences (DesignCraft's are `prefs.*`), the tool in hand and the colour settings.
    "prefs.*",
    "tool.*",
    "color.settings",
    "color.loadProfile",
    // Files beside the document: books, object libraries, linked files and text, data-merge
    // sources, the font folders.
    "book.*",
    "library.*",
    "links.*",
    "story.links",
    "story.placeAndLink",
    "story.updateLink",
    "data.*",
    "font.reload",
    // The clipboard and the Content Collector belong to the session, and may hold other documents.
    "edit.copy",
    "edit.cut",
    "edit.paste",
    "edit.pasteInPlace",
    "edit.pasteInto",
    "edit.pasteWithoutFormatting",
    "conveyor.*",
    // Run other commands inside, past this policy (a script, a live preview).
    "script.*",
    "edit.preview",
    "edit.commitPreview",
    "edit.cancelPreview",
];

/// The rules every plug-in run follows: [`craft_plugin::ALWAYS_DENIED`] and [`DENIED`].
pub fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| Policy::new(DENIED, &[]))
}

/// Why plug-in `m` may not call `command` with `params`, before it runs: a refused command, one
/// outside its manifest's `permissions.commands`, or parameters that name a file.
pub fn refusal(m: &Manifest, command: &str, params: &Value) -> Option<String> {
    if let Err(why) = craft_plugin::check_call(m, policy(), command) {
        return Some(why);
    }
    if names_a_file(params, 0) {
        return Some(format!("plug-ins can't name files (`{command}` was given a `path` or `folder`): pass the content itself, as `base64` or text"));
    }
    None
}

/// Whether `params` hold a `path`, `paths` or `folder` key at any depth: DesignCraft's names for
/// files and folders on disk (`place.load`, `style.load`, `xml.loadDtd`, `snippet.export`…).
fn names_a_file(params: &Value, depth: usize) -> bool {
    // serde_json refuses nesting past 128, so nothing deeper reaches here.
    depth < 128
        && match params {
            Value::Object(m) => m.iter().any(|(k, v)| matches!(k.as_str(), "path" | "paths" | "folder") || names_a_file(v, depth + 1)),
            Value::Array(a) => a.iter().any(|v| names_a_file(v, depth + 1)),
            _ => false,
        }
}

/// An installed plug-in.
#[derive(Clone, Debug)]
pub struct Plugin {
    pub manifest: Manifest,
    /// The SHA-256 of its bundle (lower-case hex).
    pub sha256: String,
    /// Where it came from: `{type: file, path}`, `{type: catalog, catalog, package}` or
    /// `{type: github, repo, tag, asset}` (craft-plugin's `store::Source`).
    pub source: Value,
    /// Kept in the user's `Plug-ins` folder; otherwise for this session only (the CLI, MCP).
    pub stored: bool,
    wasm: Arc<Vec<u8>>,
}

impl Plugin {
    /// What `plugin.list` says about it.
    pub fn describe(&self) -> Value {
        let mut v = serde_json::to_value(&self.manifest).unwrap_or_else(|_| json!({"id": self.manifest.id}));
        v["kind"] = json!(self.manifest.kind);
        v["sha256"] = json!(self.sha256);
        v["source"] = self.source.clone();
        v["stored"] = json!(self.stored);
        v
    }
}

/// A bundle checked and shown for review (`plugin.install {review: true}`), kept so the install
/// that follows doesn't download it again.
struct Pending {
    sha256: String,
    /// The `plugin.install` parameters that named it.
    request: Value,
    source: Value,
    bytes: Vec<u8>,
    in_catalog: Option<bool>,
}

/// The session's plug-ins.
#[derive(Default)]
pub struct Runtime {
    /// The user's `Plug-ins` folder (the app sets it; the CLI and MCP install for the session).
    #[cfg(not(target_arch = "wasm32"))]
    store: Option<craft_plugin::store::Store>,
    plugins: Vec<Plugin>,
    /// Plug-in folders that couldn't be read, or plug-ins this version can't run.
    problems: Vec<String>,
    /// HTTPS for catalogs and GitHub releases (the app sets its own client; without one, only
    /// files and `file://` catalogs install).
    pub fetch: Option<Box<dyn Fetch + Send>>,
    pending: Option<Pending>,
    /// The last run (plug-in id, command id, params).
    pub last: Option<(String, String, Value)>,
}

impl Runtime {
    /// Keep plug-ins in `<settings_dir>/Plug-ins` and read the ones installed there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_store(&mut self, settings_dir: &std::path::Path) {
        self.store = Some(craft_plugin::store::Store::new(craft_plugin::store::plugins_dir(settings_dir)));
        self.reload();
    }

    /// Read the store again (the session's own plug-ins stay).
    #[cfg(not(target_arch = "wasm32"))]
    fn reload(&mut self) {
        let Some(store) = &self.store else { return };
        let listing = store.list_for(&APP);
        self.plugins.retain(|p| !p.stored);
        self.problems = listing.problems.into_iter().map(|p| format!("{}: {}", p.dir.display(), p.error)).collect();
        self.problems.extend(listing.not_runnable.into_iter().map(|(i, why)| format!("{}: {why}", i.manifest.name)));
        for (i, _) in listing.runnable {
            let source = serde_json::to_value(&i.record.source).unwrap_or(Value::Null);
            let plugin = Plugin { manifest: i.manifest, sha256: i.record.sha256, source, stored: true, wasm: Arc::new(i.bundle.wasm) };
            self.plugins.retain(|p| !p.manifest.id.eq_ignore_ascii_case(&plugin.manifest.id));
            self.plugins.push(plugin);
        }
    }

    /// The installed plug-ins.
    pub fn list(&self) -> &[Plugin] {
        &self.plugins
    }

    /// Why plug-in folders couldn't be used.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Plug-in `id` (ignoring case, as the store does).
    pub fn get(&self, id: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.manifest.id.eq_ignore_ascii_case(id))
    }

    /// Whether plug-ins are kept in the user's folder (else for this session only).
    pub fn has_store(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.store.is_some();
        #[cfg(target_arch = "wasm32")]
        false
    }
}

/// A fetcher for sessions without network access: `file://` catalogs still read.
struct NoNetwork;

impl Fetch for NoNetwork {
    fn get(&mut self, url: &str, _max: usize) -> std::result::Result<Option<Vec<u8>>, String> {
        let shown: String = url.chars().take(120).collect();
        Err(format!("there is no network access here to read {shown}: install the plug-in from a file, or in the DesignCraft app"))
    }
}

/// Run `f` with the session's HTTPS client (or [`NoNetwork`]).
pub(crate) fn with_fetch<T>(s: &mut Session, f: impl FnOnce(&mut dyn Fetch) -> T) -> T {
    match s.plugins.fetch.as_deref_mut() {
        Some(http) => f(http),
        None => f(&mut NoNetwork),
    }
}

/// A plug-in error as the engine reports it.
fn failed(name: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("{name}: {e}"))
}

// ---------- running ----------

/// The sandbox's view of the session: every call is checked by [`refusal`], and one that opens a
/// dialog or, from a read-only plug-in, changes the document is put back and refused.
struct EngineHost<'s> {
    s: &'s mut Session,
    manifest: &'s Manifest,
    notes: Vec<String>,
}

impl craft_plugin::Host for EngineHost<'_> {
    fn call(&mut self, command: &str, params: &str) -> std::result::Result<String, String> {
        let params: Value = serde_json::from_str(params).map_err(|e| format!("the params for `{command}` aren't JSON: {e}"))?;
        if let Some(why) = refusal(self.manifest, command, &params) {
            return Err(why);
        }
        let s = &mut *self.s;
        let before = s.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone()));
        let (ui_before, journal) = (s.ui_requests.len(), s.journal.len());
        let r = s.execute(command, &params);
        // A file picker or a dialog would wait for the person; a view change is harmless.
        let asks = s.ui_requests.get(ui_before..).is_some_and(|new| new.iter().any(|r| !matches!(r, UiRequest::View { .. })));
        let changed = match (&before, s.active()) {
            (Some((uid, doc, _)), Some(d)) => d.uid != *uid || !Arc::ptr_eq(doc, &d.doc),
            (None, None) => false,
            _ => true,
        };
        let refused = if asks {
            Some(format!("`{command}` opens a dialog or a file picker, which plug-ins can't"))
        } else if changed {
            craft_plugin::check_read_only(self.manifest, command, true).err()
        } else {
            None
        };
        if let Some(why) = refused {
            // Put the call back: the document and selection as they were (the run's undo step is
            // settled when it ends), and nothing of it journaled or shown.
            if let (Some((uid, doc, selection)), Some(d)) = (before, s.active_mut())
                && d.uid == uid
            {
                d.doc = doc;
                d.selection = selection;
                d.bump_revision();
            }
            s.ui_requests.truncate(ui_before);
            s.journal.truncate(journal);
            return Err(why);
        }
        r.map(|v| v.to_string()).map_err(|e| e.to_string())
    }

    fn selection(&mut self) -> String {
        let ids: Vec<u64> = self.s.active().map(|d| d.selection.items.iter().map(|i| i.0).collect()).unwrap_or_default();
        json!(ids).to_string()
    }

    fn notify(&mut self, message: &str) {
        if self.notes.len() < MAX_NOTES {
            self.notes.push(message.chars().take(MAX_NOTE_CHARS).collect());
        }
    }

    fn log(&mut self, line: &str) {
        log::info!("plug-in {}: {line}", self.manifest.id);
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        Some(self.s.prefs.plugin_settings.get(&self.manifest.id)?.get(key)?.to_string())
    }

    fn set_setting(&mut self, key: &str, value: Option<&str>) -> std::result::Result<(), String> {
        set_setting(&mut self.s.prefs.plugin_settings, &self.manifest.id, key, value)
    }
}

/// Store (`Some`) or forget (`None`) plug-in `id`'s setting `key` in `all`, within
/// [`MAX_SETTINGS_BYTES`] and [`MAX_SETTING_KEY`].
fn set_setting(
    all: &mut std::collections::BTreeMap<String, Map<String, Value>>,
    id: &str,
    key: &str,
    value: Option<&str>,
) -> std::result::Result<(), String> {
    let Some(text) = value else {
        if let Some(mine) = all.get_mut(id) {
            mine.remove(key);
            if mine.is_empty() {
                all.remove(id);
            }
        }
        return Ok(());
    };
    if key.is_empty() || key.chars().count() > MAX_SETTING_KEY || key.chars().any(char::is_control) {
        return Err(format!("a setting's key must be 1–{MAX_SETTING_KEY} characters without control characters"));
    }
    let too_big = || format!("a plug-in's settings are limited to {} KB", MAX_SETTINGS_BYTES / 1024);
    if key.len() + text.len() > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("a setting must be JSON: {e}"))?;
    let others: usize =
        all.get(id).map_or(0, |mine| mine.iter().filter(|(k, _)| k.as_str() != key).map(|(k, v)| k.len() + v.to_string().len()).sum());
    if others + key.len() + v.to_string().len() > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    all.entry(id.to_string()).or_default().insert(key.to_string(), v);
    Ok(())
}

/// Run plug-in `id`'s `command` (default: its first) with `params` as one undo step named after
/// the plug-in. A run that fails (a trap, an exhausted budget, a refusal it passes on, a non-zero
/// status) leaves the document, selection and undo history as they were.
pub fn run(s: &mut Session, id: &str, command: Option<&str>, params: &Value, fuel: Option<u64>) -> Result<Value> {
    let cmd = "plugin.run";
    let p = s.plugins.get(id).cloned().ok_or_else(|| crate::cmd::bad(cmd, format!("no plug-in `{id}` is installed (see plugin.list)")))?;
    let m = &p.manifest;
    let api = m.target(&APP).map_err(|e| crate::cmd::bad(cmd, format!("{} can't run here: {e}", m.name)))?.api();
    let c = m.command(command).ok_or_else(|| crate::cmd::bad(cmd, format!("{} has no command `{}`", m.name, command.unwrap_or_default())))?;
    let params = c.resolve_params(params).map_err(|e| crate::cmd::bad(cmd, e))?;
    let mut limits = Limits::default();
    if let Some(f) = fuel {
        limits.fuel = f.clamp(1000, limits.fuel);
    }
    let mut instance = Instance::new(&p.wasm, &limits).map_err(|e| failed(&m.name, e))?;
    let selection: Vec<u64> = s.active().map(|d| d.selection.items.iter().map(|i| i.0).collect()).unwrap_or_default();
    let input = json!({"api": api, "command": c.id, "params": params, "selection": selection}).to_string();
    s.plugins.last = Some((m.id.clone(), c.id.clone(), params));
    let label = if m.commands.len() > 1 { format!("{}: {}", m.name, c.label.trim_end_matches(['…', '.'])) } else { m.name.clone() };

    // The run's commands record undo steps of their own; they collapse into one below, as
    // `script.run`'s do.
    let before = s.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone(), d.history.clone()));
    let (journal, ui) = (s.journal.len(), s.ui_requests.len());
    let outer = std::mem::replace(&mut s.in_undo_step, false);
    let mut host = EngineHost { s: &mut *s, manifest: m, notes: Vec::new() };
    let ran: std::result::Result<RunOutput, craft_plugin::PluginError> = instance.call(&input, &mut host);
    let notes = std::mem::take(&mut host.notes);
    s.in_undo_step = outer;
    if let (Some((uid, doc, selection, history)), Some(st)) = (before, s.active_mut())
        && st.uid == uid
    {
        let changed = !Arc::ptr_eq(&doc, &st.doc);
        st.history = history;
        if ran.is_err() {
            st.doc = doc;
            st.selection = selection;
            st.bump_revision();
        } else if changed && !outer {
            crate::record_undo(st, HistoryEntry { label, doc, selection });
        }
    }
    let out = match ran {
        Ok(out) => out,
        Err(e) => {
            s.journal.truncate(journal);
            s.ui_requests.truncate(ui);
            return Err(failed(&m.name, e));
        }
    };
    let output = serde_json::from_str::<Value>(&out.output).unwrap_or(Value::String(out.output));
    // Panels (`ui_show`) aren't shown in DesignCraft yet: say so rather than drop it silently.
    let panel = matches!(out.ui, Some(UiChange::Show(_))).then_some("DesignCraft doesn't show plug-in panels yet");
    Ok(
        json!({"plugin": m.id, "command": c.id, "output": output, "notes": notes, "logs": out.logs, "calls": out.calls, "fuel": out.fuel, "panel": panel}),
    )
}

// ---------- installing ----------

/// A bundle obtained for installing: its bytes, where it came from and, for a repository,
/// whether a catalog in use lists it.
pub(crate) struct Candidate {
    pub bytes: Vec<u8>,
    pub source: Value,
    /// `Some(true)` from a catalog; for a repository, whether a catalog in use lists it.
    pub in_catalog: Option<bool>,
}

/// Read the bundle `p` names: `path`, `data` (base64), `repo` (`owner/repo[@tag]`) or `catalog` +
/// `id` (and optionally `version`).
pub(crate) fn obtain(s: &mut Session, cmd: &str, p: &Value) -> Result<Candidate> {
    let bad = |msg: String| crate::cmd::bad(cmd, msg);
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
    if let Some(path) = text("path") {
        let bytes = crate::io::read_capped(path, bundle::MAX_BUNDLE_BYTES as u64).map_err(|e| bad(format!("{path}: {e}")))?;
        if bytes.len() > bundle::MAX_BUNDLE_BYTES {
            return Err(bad(format!("{path} is larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20)));
        }
        return Ok(Candidate { bytes, source: json!({"type": "file", "path": path}), in_catalog: None });
    }
    if let Some(data) = text("data") {
        if data.len() > bundle::MAX_BUNDLE_BYTES / 3 * 4 + 4 {
            return Err(bad(format!("the bundle is larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20)));
        }
        let name = text("name").unwrap_or("data");
        return Ok(Candidate { bytes: crate::cmd::base64_decode(data), source: json!({"type": "file", "path": name}), in_catalog: None });
    }
    if let Some(spec) = text("repo") {
        let repo = github::parse_repo_spec(spec).map_err(bad)?;
        let catalogs = s.prefs.plugin_catalogs.clone();
        let (plan, bytes) = with_fetch(s, |f| {
            let plan = fetch::resolve_repo(f, &repo)?;
            let bytes = fetch::download_repo(f, &plan)?;
            Ok::<_, String>((plan, bytes))
        })
        .map_err(bad)?;
        let source = json!({"type": "github", "repo": plan.repo.slug(), "tag": plan.repo.tag.clone().unwrap_or_default(), "asset": plan.asset});
        let in_catalog = with_fetch(s, |f| {
            catalogs.iter().filter(|url| local_refused(url).is_ok()).any(|url| {
                fetch::fetch_catalog(f, url)
                    .is_ok_and(|(cat, _)| cat.packages.iter().any(|pkg| pkg.repo_ref().is_some_and(|r| r.same_repo(&repo.owner, &repo.name))))
            })
        });
        return Ok(Candidate { bytes, source, in_catalog: Some(in_catalog) });
    }
    if let Some(url) = text("catalog") {
        let id = text("id").ok_or_else(|| bad("`id` required with `catalog` (a package of the catalog)".into()))?;
        let version = text("version");
        local_refused(url).map_err(bad)?;
        let bytes = with_fetch(s, |f| {
            let (cat, _) = fetch::fetch_catalog(f, url)?;
            let package = cat.package(id).ok_or_else(|| format!("{url} has no package `{id}`"))?;
            let release = match version {
                Some(v) => package.release(v).ok_or_else(|| format!("{id} has no version {v} in {url}"))?,
                None => package.latest_for(&APP).ok_or_else(|| format!("{id} has no release for DesignCraft {}", APP.version))?,
            };
            release.target(&package.kind, &APP)?;
            let bytes = fetch::download_release(f, url, package, release)?;
            let (_, manifest) = bundle::open(&bytes)?;
            package.check_manifest(release, &manifest)?;
            Ok::<_, String>(bytes)
        })
        .map_err(bad)?;
        return Ok(Candidate { bytes, source: json!({"type": "catalog", "catalog": url, "package": id}), in_catalog: Some(true) });
    }
    Err(bad("pass `path`, `data` (base64), `repo` (owner/repo or owner/repo@tag) or `catalog` with `id`".into()))
}

/// What an install confirmation shows about `c`: the manifest's permissions, kind and commands,
/// the source, the SHA-256, whether a catalog lists it and what it replaces. An error when the
/// bundle isn't a plug-in DesignCraft can run.
pub(crate) fn review(s: &Session, cmd: &str, c: &Candidate) -> Result<(Manifest, Value)> {
    let (_, manifest) = bundle::open(&c.bytes).map_err(|e| crate::cmd::bad(cmd, e))?;
    manifest.target(&APP).map_err(|e| crate::cmd::bad(cmd, format!("{} can't be installed: {e}", manifest.name)))?;
    let old = s.plugins.get(&manifest.id);
    let mut v = serde_json::to_value(&manifest).unwrap_or_else(|_| json!({"id": manifest.id}));
    v["kind"] = json!(manifest.kind);
    v["sha256"] = json!(bundle::sha256_hex(&c.bytes));
    v["source"] = c.source.clone();
    v["inCatalog"] = json!(c.in_catalog);
    v["replaces"] = json!(old.map(|o| &o.manifest.version));
    v["permissionsChanged"] = json!(old.is_some_and(|o| craft_plugin::permissions::changed(&o.manifest, &manifest)));
    v["stored"] = json!(s.plugins.has_store());
    Ok((manifest, v))
}

/// Install `c` (already reviewed): into the user's folder when the session has one, else for the
/// session. Replaces an installed plug-in with the same id; its settings stay.
pub(crate) fn install(s: &mut Session, cmd: &str, c: Candidate) -> Result<Plugin> {
    let (manifest, _) = review(s, cmd, &c)?;
    let sha256 = bundle::sha256_hex(&c.bytes);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(store) = &s.plugins.store {
        let source: craft_plugin::store::Source = serde_json::from_value(c.source.clone()).map_err(|e| crate::cmd::bad(cmd, e.to_string()))?;
        let (i, _) = store.install_for(&APP, &c.bytes, source).map_err(|e| crate::cmd::bad(cmd, e))?;
        let plugin = Plugin { manifest: i.manifest, sha256: i.record.sha256, source: c.source, stored: true, wasm: Arc::new(i.bundle.wasm) };
        return Ok(keep(s, plugin));
    }
    let (b, _) = bundle::open(&c.bytes).map_err(|e| crate::cmd::bad(cmd, e))?;
    Ok(keep(s, Plugin { manifest, sha256, source: c.source, stored: false, wasm: Arc::new(b.wasm) }))
}

/// Put `plugin` in the session's list, in place of one with the same id.
fn keep(s: &mut Session, plugin: Plugin) -> Plugin {
    let list = &mut s.plugins.plugins;
    match list.iter_mut().find(|p| p.manifest.id.eq_ignore_ascii_case(&plugin.manifest.id)) {
        Some(slot) => *slot = plugin.clone(),
        None => list.push(plugin.clone()),
    }
    plugin
}

/// Keep `c`, named by the install parameters `request`, for the install that follows its review
/// (one at a time).
pub(crate) fn hold(s: &mut Session, sha256: &str, request: Value, c: Candidate) {
    s.plugins.pending = Some(Pending { sha256: sha256.into(), request, bytes: c.bytes, source: c.source, in_catalog: c.in_catalog });
}

/// The bundle reviewed last, when it is `sha256` and `request` named it.
pub(crate) fn take_held(s: &mut Session, sha256: &str, request: &Value) -> Option<Candidate> {
    let held = s.plugins.pending.take_if(|h| h.sha256.eq_ignore_ascii_case(sha256) && h.request == *request)?;
    Some(Candidate { bytes: held.bytes, source: held.source, in_catalog: held.in_catalog })
}

/// Remove plug-in `id` and its settings; whether it was installed.
pub(crate) fn remove(s: &mut Session, cmd: &str, id: &str) -> Result<bool> {
    let Some(p) = s.plugins.get(id).cloned() else { return Ok(false) };
    #[cfg(not(target_arch = "wasm32"))]
    if p.stored
        && let Some(store) = &s.plugins.store
    {
        store.remove(&p.manifest.id).map_err(|e| crate::cmd::bad(cmd, e))?;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = cmd;
    s.plugins.plugins.retain(|x| !x.manifest.id.eq_ignore_ascii_case(id));
    s.prefs.plugin_settings.remove(&p.manifest.id);
    if s.plugins.last.as_ref().is_some_and(|(last, ..)| last.eq_ignore_ascii_case(id)) {
        s.plugins.last = None;
    }
    Ok(true)
}

/// The catalog `url`'s packages DesignCraft can run, with their newest release for it.
pub(crate) fn catalog_packages(s: &mut Session, url: &str) -> std::result::Result<(Vec<Value>, Vec<String>), String> {
    local_refused(url)?;
    let (cat, warnings) = with_fetch(s, |f| fetch::fetch_catalog(f, url))?;
    let packages = cat
        .packages
        .iter()
        .filter_map(|pkg| {
            let release = pkg.latest_for(&APP)?;
            let installed = s.plugins.get(&pkg.id).map(|p| p.manifest.version.clone());
            Some(json!({
                "catalog": url, "id": pkg.id, "name": pkg.name, "version": release.version, "author": pkg.author,
                "description": pkg.description, "license": pkg.license, "repo": pkg.repo, "kind": pkg.kind,
                "sha256": release.sha256, "installed": installed,
            }))
        })
        .collect();
    Ok((packages, warnings))
}

/// Why the catalog `url` can't be read here: a `file://` catalog (and the files it names) is read
/// straight from the disk, past the automation roots, so it is refused while they are in force.
fn local_refused(url: &str) -> std::result::Result<(), String> {
    if url.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("file://")) && crate::file_access::current().is_some() {
        return Err("file:// catalogs can't be read while automation is confined to folders".into());
    }
    Ok(())
}

/// Whether `url` is a catalog address DesignCraft reads (`https://` on GitHub's hosts, or
/// `file://`), or why not.
pub(crate) fn check_catalog_url(url: &str) -> std::result::Result<(), String> {
    local_refused(url)?;
    catalog::check_url(url, None).map(|_| ())
}

/// Where plug-in `p` has a newer release: the install parameters that get it and its version (a
/// tag, for a repository). `None` when it is up to date or came from a file.
pub(crate) fn update_for(s: &mut Session, p: &Plugin) -> std::result::Result<Option<(Value, String)>, String> {
    let text = |k: &str| p.source.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match p.source.get("type").and_then(Value::as_str) {
        Some("catalog") => {
            let (url, id) = (text("catalog"), text("package"));
            local_refused(&url)?;
            let (cat, _) = with_fetch(s, |f| fetch::fetch_catalog(f, &url))?;
            let release = cat.package(&id).and_then(|pkg| pkg.latest_for(&APP));
            Ok(release
                .filter(|r| bundle::newer(&r.version, &p.manifest.version))
                .map(|r| (json!({"catalog": url, "id": id, "version": r.version}), r.version.clone())))
        }
        Some("github") => {
            let mut repo = github::parse_repo_spec(&text("repo"))?;
            repo.tag = None;
            let plan = with_fetch(s, |f| fetch::resolve_repo(f, &repo))?;
            let tag = plan.repo.tag.clone().unwrap_or_default();
            Ok((tag != text("tag")).then(|| (json!({"repo": format!("{}@{tag}", plan.repo.slug())}), tag)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;
