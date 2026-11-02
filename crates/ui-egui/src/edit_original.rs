//! Edit › Edit Original and Edit › Edit With: open a placed graphic's linked file in another
//! application, then pick up the edit when the window is back in front.
//!
//! The engine says which files those are (`links.originalFiles`); starting another application is
//! the host's job, through [`crate::Services::launch`] (the desktop app installs
//! [`system_launch`]; the browser build has none, so the commands are disabled there). The
//! applications Edit With lists come from [`crate::Services::applications`] (macOS Launch
//! Services in the desktop app); without it the submenu holds Other… only, which asks for an
//! application.
//!
//! An edited file is remembered with its size and time. When the window gains focus again, each
//! remembered file of the active document is looked at (metadata only) and a changed one is
//! updated (`links.update {asset}`), as the reference app updates a link edited this way. A save
//! that lands after the window is back in front (an editor that saves when it is switched away
//! from, or in the background) is picked up by the link watch's next check
//! ([`crate::link_watch`]), which calls [`refresh_edited`] whenever it finds a modified link
//! while the window is in front.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::{Bound, with_path};
use crate::menus::Item;

/// Edit › Edit Original.
pub const EDIT_ORIGINAL: &str = "links.editOriginal";
/// Edit › Edit With › an application, or Other….
pub const EDIT_WITH: &str = "links.editWith";
/// The Edit › Edit With submenu, whose applications come before Other….
pub const EDIT_WITH_MENU: &str = "Edit/Edit With";

/// Why the commands can't run in this host.
const UNAVAILABLE: &str = "opening files in other applications isn't available here";

/// An application that can open a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppChoice {
    /// As the menu shows it ("Preview").
    pub name: String,
    /// What [`crate::Services::launch`] is given (an `.app` bundle on macOS, a program elsewhere).
    pub path: PathBuf,
}

/// Opens `file` in `app`, or in the system's default application for its type.
pub type LaunchFn = Box<dyn FnMut(&Path, Option<&Path>) -> Result<(), String>>;

/// The applications that can open a file, the default first.
pub type ApplicationsFn = Box<dyn Fn(&Path) -> Vec<AppChoice>>;

/// A file opened for editing, and how it was then.
struct Edited {
    doc: u64,
    asset: u64,
    path: PathBuf,
    stamp: Option<Stamp>,
}

/// A file's size and modification time.
type Stamp = (u64, Option<std::time::SystemTime>);

/// What Edit Original remembers between frames.
#[derive(Default)]
pub(crate) struct State {
    edited: Vec<Edited>,
    /// Whether the window had focus on the last frame (`None` before the host said).
    focused: Option<bool>,
    /// The applications for the file last asked about (Launch Services is asked once per file).
    apps: Option<(PathBuf, Vec<AppChoice>)>,
}

/// Run `links.editOriginal` or `links.editWith`; `None` if `id` is neither.
pub(crate) fn run(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        EDIT_ORIGINAL => edit_original(app, p),
        EDIT_WITH => edit_with(app, p),
        _ => return None,
    })
}

/// Whether the commands can run: the host can launch applications and some selected graphic is
/// linked to a file on disk.
pub fn available(app: &DesignApp) -> bool {
    app.services.launch.is_some() && designcraft_engine::cmd::can_edit_original(&app.session)
}

/// The params the engine reads: the Links panel's `asset`, or `ids`.
fn target(p: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for k in ["asset", "ids"] {
        if let Some(v) = p.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    Value::Object(out)
}

fn originals(app: &DesignApp, p: &Value) -> Result<Vec<designcraft_engine::cmd::LinkOriginal>, String> {
    designcraft_engine::cmd::link_originals(&app.session, &target(p)).map_err(|e| e.to_string())
}

fn edit_original(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let files = originals(app, p)?;
    open_all(app, &files, None)
}

/// Opens each file (in `with`, or its default application) and remembers it for the update.
fn open_all(app: &mut DesignApp, files: &[designcraft_engine::cmd::LinkOriginal], with: Option<&Path>) -> Result<Value, String> {
    let launch = app.services.launch.as_mut().ok_or(UNAVAILABLE)?;
    let mut opened = Vec::new();
    for f in files {
        launch(&f.path, with)?;
        opened.push(f.path.to_string_lossy().to_string());
    }
    remember(app, files);
    Ok(json!({ "opened": opened }))
}

fn edit_with(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let files = originals(app, p)?;
    if p.get("list").and_then(Value::as_bool) == Some(true) {
        let Some(first) = files.first() else { return Ok(json!([])) };
        let apps = applications_for(app, &first.path);
        return Ok(Value::Array(apps.iter().map(|a| json!({"name": a.name, "path": a.path.to_string_lossy()})).collect()));
    }
    if app.services.launch.is_none() {
        return Err(UNAVAILABLE.into());
    }
    match p.get("app") {
        Some(v) => {
            let with = v.as_str().filter(|s| !s.trim().is_empty()).ok_or("links.editWith: `app` must be the path of an application")?;
            let with = Path::new(with);
            #[cfg(not(target_arch = "wasm32"))]
            if !designcraft_engine::io::exists(with) {
                return Err(format!("links.editWith: no application at {}", with.display()));
            }
            open_all(app, &files, Some(with))
        }
        // Other…: choose the application, then open the files in it.
        None => {
            let p = p.clone();
            app.ask_open("application", Bound::Document, move |app, path| edit_with(app, &with_path(&p, "app", path)))
        }
    }
}

/// The applications that can open `file`, asked of the host once per file.
fn applications_for(app: &mut DesignApp, file: &Path) -> Vec<AppChoice> {
    if let Some((at, apps)) = &app.edit_original.apps
        && at == file
    {
        return apps.clone();
    }
    let apps = app.services.applications.as_ref().map(|f| f(file)).unwrap_or_default();
    app.edit_original.apps = Some((file.to_path_buf(), apps.clone()));
    apps
}

/// Edit › Edit With's applications for the selection (`links.editWith {app}`), then a separator;
/// nothing when there are none.
pub fn edit_with_items(app: &mut DesignApp) -> Vec<Item> {
    edit_with_items_for(app, &Value::Null)
}

/// [`edit_with_items`] for the graphics `target` names (the Links panel's `{asset}`).
pub(crate) fn edit_with_items_for(app: &mut DesignApp, target_params: &Value) -> Vec<Item> {
    if app.services.launch.is_none() {
        return Vec::new();
    }
    let Some(first) = originals(app, target_params).ok().and_then(|f| f.into_iter().next()) else { return Vec::new() };
    let mut out: Vec<Item> = applications_for(app, &first.path)
        .into_iter()
        .map(|a| {
            let mut params = target(target_params);
            params["app"] = json!(a.path.to_string_lossy());
            Item::Cmd { label: a.name, id: EDIT_WITH.into(), params, shortcut: None }
        })
        .collect();
    if !out.is_empty() {
        out.push(Item::Sep);
    }
    out
}

/// Draws [`edit_with_items_for`] (application names are not translated).
pub(crate) fn edit_with_menu_items(app: &mut DesignApp, ui: &mut egui::Ui, target_params: &Value) {
    for it in edit_with_items_for(app, target_params) {
        match it {
            Item::Cmd { label, id, params, .. } => {
                let tip = params["app"].as_str().unwrap_or_default().to_string();
                if ui.button(crate::rtl::widget(ui, label)).on_hover_text(tip).clicked() {
                    if let Err(e) = app.run(&id, params) {
                        app.status(e);
                    }
                    ui.close();
                }
            }
            Item::Sep => {
                ui.separator();
            }
            Item::Sub(..) => {}
        }
    }
}

/// A file's size and time, without reading it.
fn stamp(path: &Path) -> Option<Stamp> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let m = designcraft_engine::io::metadata(path).ok()?;
        Some((m.len(), m.modified().ok()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

fn remember(app: &mut DesignApp, files: &[designcraft_engine::cmd::LinkOriginal]) {
    let Some(doc) = app.session.active().map(|d| d.uid) else { return };
    for f in files {
        let stamp = stamp(&f.path);
        let e = Edited { doc, asset: f.asset.0, path: f.path.clone(), stamp };
        match app.edit_original.edited.iter_mut().find(|x| x.doc == doc && x.asset == f.asset.0) {
            Some(x) => *x = e,
            None => app.edit_original.edited.push(e),
        }
    }
}

/// Once per frame: when the window gains focus again, update the edited links that changed.
pub(crate) fn on_frame(app: &mut DesignApp, ctx: &egui::Context) {
    focus(app, ctx.input(|i| i.viewport().focused));
}

/// The window's focus this frame, as the host reports it.
fn focus(app: &mut DesignApp, focused: Option<bool>) {
    let was = std::mem::replace(&mut app.edit_original.focused, focused);
    if focused == Some(true) && was == Some(false) && !app.edit_original.edited.is_empty() {
        refresh_edited(app);
    }
}

/// Updates the edited links of the active document whose files changed since they were opened
/// (or last updated) → how many links were updated. Only their metadata is read unless they
/// changed; a file that is gone is left for the Links panel to show as missing.
pub(crate) fn refresh_edited(app: &mut DesignApp) -> u64 {
    let open: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    app.edit_original.edited.retain(|e| open.contains(&e.doc));
    let Some(active) = app.session.active().map(|d| d.uid) else { return 0 };
    let mut edited = std::mem::take(&mut app.edit_original.edited);
    let mut updated = 0;
    for e in edited.iter_mut().filter(|e| e.doc == active) {
        let now = stamp(&e.path);
        if now.is_none() || now == e.stamp {
            continue;
        }
        e.stamp = now;
        match app.run("links.update", json!({ "asset": e.asset })) {
            Ok(r) => updated += r["updated"].as_u64().unwrap_or(0),
            Err(err) => app.status(err),
        }
    }
    // Links opened meanwhile (from the update's status, never) come after the ones checked.
    edited.append(&mut app.edit_original.edited);
    app.edit_original.edited = edited;
    updated
}

/// The process that opens `file` in `app` (or its default application) on this platform.
#[cfg(not(target_arch = "wasm32"))]
pub fn launch_command(file: &Path, app: Option<&Path>) -> Result<std::process::Command, String> {
    // A resolved link is absolute, so it can't be read as an option of the opener.
    if !file.is_absolute() {
        return Err(format!("{}: not an absolute path", file.display()));
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("/usr/bin/open");
        if let Some(app) = app {
            c.arg("-a").arg(app);
        }
        c.arg(file);
        Ok(c)
    }
    #[cfg(target_os = "windows")]
    {
        // Explorer opens a file with its default application without a command shell, which
        // would read `&` or `%` in the file name as its own syntax.
        let mut c = std::process::Command::new(app.map_or_else(|| PathBuf::from("explorer.exe"), Path::to_path_buf));
        c.arg(file);
        Ok(c)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut c = std::process::Command::new(app.map_or_else(|| PathBuf::from("xdg-open"), Path::to_path_buf));
        c.arg(file);
        Ok(c)
    }
}

/// Opens `file` in `app`, or in its default application: the desktop host's
/// [`crate::Services::launch`]. The opener is waited for off the UI thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn system_launch(file: &Path, app: Option<&Path>) -> Result<(), String> {
    use std::process::Stdio;
    let mut child = launch_command(file, app)?
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("couldn't open {}: {e}", file.display()))?;
    // Reaped on its own thread, so neither a slow opener nor its exit blocks the frame.
    let _ = std::thread::Builder::new().name("edit-original".into()).spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests;
