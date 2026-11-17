//! The Plug-ins menu and its dialogs: Manage Plug-ins (the installed plug-ins and their commands,
//! installing from a file, a GitHub repository or a catalog, updates), the install confirmation
//! (permissions, kind, source, SHA-256) and the parameter dialog generated from a plug-in command's
//! `params`. Everything goes through the engine's `plugin.*` commands (`designcraft_engine::plugin`).

use designcraft_engine::plugin::craft_plugin::{Param, ParamKind};
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::dialogs::Dialog;
use crate::menus::Item;
use crate::theme::semibold;

/// Manage Plug-ins.
pub const MANAGE: &str = "plugins";
/// The install confirmation.
pub const CONFIRM: &str = "pluginInstall";
/// A plug-in command's parameters.
pub const PARAMS: &str = "pluginParams";
/// The menu bar's Plug-ins menu.
pub const MENU: &str = "Plug-ins";
/// Runs a plug-in command from the menu: `{id, command}`, asking for its parameters first.
const RUN_FROM_MENU: &str = "app.pluginCommand";

/// HTTPS for catalogs and GitHub releases, over the client DesignCraft checks for updates with.
#[cfg(not(target_arch = "wasm32"))]
struct UpdateHttp(craft_update::Http);

#[cfg(not(target_arch = "wasm32"))]
impl designcraft_engine::plugin::craft_plugin::Fetch for UpdateHttp {
    fn get(&mut self, url: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        use craft_update::http::Fetched;
        if !url.starts_with("https://api.github.com/") {
            return self.0.get_bytes(url, max);
        }
        match self.0.get_api(url, None)? {
            Fetched::Body { text, .. } if text.len() > max => Err(format!("GitHub's answer is larger than {max} bytes")),
            Fetched::Body { text, .. } => Ok(Some(text.into_bytes())),
            Fetched::NotFound => Ok(None),
            Fetched::RateLimited { .. } => Err("GitHub's API limit for this hour is used up: try again later".into()),
            Fetched::NotModified => Err("GitHub answered 304 Not Modified to a request without a cache".into()),
        }
    }
}

/// The HTTPS client for `Session::plugins.fetch`, when one can be made.
#[cfg(not(target_arch = "wasm32"))]
pub fn http_fetch() -> Option<Box<dyn designcraft_engine::plugin::craft_plugin::Fetch + Send>> {
    let agent = format!("DesignCraft/{}", env!("CARGO_PKG_VERSION"));
    craft_update::Http::new(&agent).map_err(|e| log::warn!("plug-in downloads: {e}")).ok().map(|h| Box::new(UpdateHttp(h)) as _)
}

/// The Plug-ins menu: each installed plug-in's commands, then Manage Plug-ins….
pub fn menu_items(app: &DesignApp) -> Vec<Item> {
    let mut out = Vec::new();
    for p in app.session.plugins.list() {
        let m = &p.manifest;
        let item = |label: String, command: &str| Item::Cmd {
            label,
            id: RUN_FROM_MENU.into(),
            params: json!({"id": m.id, "command": command}),
            shortcut: None,
        };
        match m.commands.as_slice() {
            [one] => out.push(item(m.name.clone(), &one.id)),
            many => out.push(Item::Sub(m.name.clone(), many.iter().map(|c| item(c.label.clone(), &c.id)).collect())),
        }
    }
    if !out.is_empty() {
        out.push(Item::Sep);
    }
    out.push(Item::Cmd { label: "Manage Plug-ins…".into(), id: "app.pluginsDialog".into(), params: Value::Null, shortcut: None });
    out
}

/// The menu bar's Plug-ins menu (built from the installed plug-ins, so it isn't in `MENUS`).
pub fn menu(app: &mut DesignApp, ui: &mut egui::Ui) {
    let lang = app.ui.language.clone();
    crate::menus::menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&lang, MENU)), |ui| {
        ui.set_min_width(240.0);
        draw_items(app, ui, &menu_items(app));
    });
}

fn draw_items(app: &mut DesignApp, ui: &mut egui::Ui, items: &[Item]) {
    for it in items {
        match it {
            Item::Sep => {
                ui.separator();
            }
            // Plug-in names and command labels are the plug-in's, not translated.
            Item::Sub(name, children) => {
                crate::menus::menu_button(ui, crate::rtl::widget(ui, name.as_str()), |ui| draw_items(app, ui, children));
            }
            Item::Cmd { label, id, params, .. } => {
                let label = if id == "app.pluginsDialog" { crate::i18n::tr(&app.ui.language, label).to_string() } else { label.clone() };
                if ui.button(crate::rtl::widget(ui, label)).clicked() {
                    crate::menus::activate(app, id, params);
                    ui.close();
                }
            }
        }
    }
}

/// The UI commands of this module (`app.pluginsDialog`, the menu's runs), or `None` for others.
pub fn command(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    match id {
        "app.pluginsDialog" => {
            app.ui.dialog = Some(Dialog::new(MANAGE, json!({"repo": "", "catalogUrl": ""})));
            Some(Ok(Value::Null))
        }
        RUN_FROM_MENU => Some(run_from_menu(app, p)),
        _ => None,
    }
}

/// Run a plug-in command, after its parameter dialog when it declares parameters.
fn run_from_menu(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let id = p.get("id").and_then(Value::as_str).ok_or("`id` required")?;
    let command = p.get("command").and_then(Value::as_str).unwrap_or_default();
    let plugin = app.session.plugins.get(id).ok_or_else(|| format!("no plug-in `{id}` is installed"))?;
    let c = plugin.manifest.command(Some(command)).ok_or_else(|| format!("{} has no command `{command}`", plugin.manifest.name))?;
    if c.params.is_empty() {
        let (id, command) = (plugin.manifest.id.clone(), c.id.clone());
        return run_plugin(app, json!({"id": id, "command": command}));
    }
    let values: Map<String, Value> = c.params.iter().map(|p| (p.name.clone(), p.default_value())).collect();
    let title = format!("{}: {}", plugin.manifest.name, c.label.trim_end_matches('…'));
    app.ui.dialog =
        Some(Dialog::new(PARAMS, json!({"id": plugin.manifest.id, "command": c.id, "title": title, "params": c.params, "values": values})));
    Ok(Value::Null)
}

/// `plugin.run`, its notes or failure in the status line.
fn run_plugin(app: &mut DesignApp, params: Value) -> Result<Value, String> {
    let name = params["id"].as_str().and_then(|id| app.session.plugins.get(id)).map(|p| p.manifest.name.clone()).unwrap_or_default();
    match app.run("plugin.run", params) {
        Ok(r) => {
            let notes: Vec<&str> = r["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let mut line = if notes.is_empty() { format!("{name}: {}", crate::i18n::tr(&app.ui.language, "Done")) } else { notes.join(" · ") };
            if let Some(panel) = r["panel"].as_str() {
                line = format!("{line} ({panel})");
            }
            app.status(line);
            Ok(r)
        }
        Err(e) => {
            app.status(e.clone());
            Err(e)
        }
    }
}

/// The dialog titles.
pub fn title(app: &DesignApp, d: &Dialog) -> String {
    let tr = |s| crate::i18n::tr(&app.ui.language, s).to_string();
    match d.id.as_str() {
        CONFIRM => tr("Install Plug-in"),
        PARAMS => d.s("title"),
        _ => tr("Plug-ins"),
    }
}

/// The OK button's label.
pub fn ok_label(app: &DesignApp, d: &Dialog) -> String {
    let tr = |s| crate::i18n::tr(&app.ui.language, s).to_string();
    match d.id.as_str() {
        CONFIRM => tr("Install"),
        PARAMS => tr("Run"),
        _ => tr("Done"),
    }
}

/// Check a bundle (`plugin.install {…, review: true}`) and turn `d` into its confirmation.
fn review(app: &mut DesignApp, d: &mut Dialog, request: Value) {
    let mut asked = request.clone();
    asked["review"] = json!(true);
    match app.run("plugin.install", asked) {
        Ok(info) => *d = Dialog::new(CONFIRM, json!({"request": request, "info": info})),
        Err(e) => {
            d.fields.insert("status".into(), json!(e));
        }
    }
}

/// The body of whichever of the three dialogs `d` is.
pub fn body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    match d.id.as_str() {
        CONFIRM => confirm_body(app, ui, d),
        PARAMS => params_body(app, ui, d),
        _ => manage_body(app, ui, d),
    }
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(text).font(semibold(13.0)));
}

/// A text field bound to `d.fields[key]`.
fn text_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str, width: f32, hint: &str) {
    let mut s = d.s(key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(width).hint_text(hint)).changed() {
        d.fields.insert(key.into(), json!(s));
    }
}

fn manage_body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s| crate::i18n::tr(&lang, s);
    let dim = crate::theme::Tokens::get(ui.ctx()).text_dim;
    heading(ui, tr("Installed"));
    let plugins: Vec<_> = app.session.plugins.list().to_vec();
    if plugins.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(tr("No plug-ins installed.")).color(dim));
    }
    for p in &plugins {
        let m = &p.manifest;
        ui.horizontal(|ui| {
            crate::rtl::label(ui, egui::RichText::new(format!("{} {}", m.name, m.version)).strong());
            let access = if m.permissions.access == designcraft_engine::plugin::craft_plugin::Access::Write {
                tr("edits documents")
            } else {
                tr("reads documents")
            };
            crate::rtl::label(ui, egui::RichText::new(access).color(dim));
            if !p.stored {
                crate::rtl::label(ui, egui::RichText::new(tr("this session only")).color(dim));
            }
        });
        ui.horizontal_wrapped(|ui| {
            for c in &m.commands {
                if ui.button(c.label.as_str()).clicked() {
                    match run_from_menu(app, &json!({"id": m.id, "command": c.id})) {
                        // The parameter dialog replaced this one.
                        Ok(_) if app.ui.dialog.as_ref().is_some_and(|x| x.id == PARAMS) => {
                            if let Some(x) = app.ui.dialog.take() {
                                *d = x;
                            }
                            return;
                        }
                        Ok(_) => {
                            d.fields.insert("status".into(), json!(app.ui.status));
                        }
                        Err(e) => {
                            d.fields.insert("status".into(), json!(e));
                        }
                    }
                }
            }
            if ui.button(tr("Remove")).clicked() {
                let r = app.run("plugin.remove", json!({"id": m.id}));
                d.fields.insert("status".into(), json!(r.err().unwrap_or_default()));
            }
        });
    }
    for problem in app.session.plugins.problems() {
        crate::rtl::label(ui, egui::RichText::new(problem).color(dim));
    }

    heading(ui, tr("Install"));
    ui.horizontal(|ui| {
        if ui.button(tr("From File…")).clicked() {
            let asked = app.ask_open("plugin", Bound::App, |app, path| {
                let mut d = Dialog::new(MANAGE, json!({}));
                review(app, &mut d, json!({"path": path}));
                app.ui.dialog = Some(d);
                Ok(Value::Null)
            });
            if let Err(e) = asked {
                d.fields.insert("status".into(), json!(e));
            }
        }
    });
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("GitHub repository"));
        text_field(ui, d, "repo", 220.0, "owner/repo");
        if ui.add_enabled(!d.s("repo").trim().is_empty(), egui::Button::new(tr("Review…"))).clicked() {
            let repo = d.s("repo").trim().to_string();
            review(app, d, json!({"repo": repo}));
        }
    });

    heading(ui, tr("Catalogs"));
    for url in app.session.prefs.plugin_catalogs.clone() {
        ui.horizontal(|ui| {
            crate::rtl::label(ui, url.as_str());
            if ui.small_button(tr("Remove")).clicked() {
                let _ = app.run("plugin.catalog.add", json!({"url": url, "remove": true}));
            }
        });
    }
    ui.horizontal(|ui| {
        text_field(ui, d, "catalogUrl", 300.0, "https://raw.githubusercontent.com/…/index.json");
        if ui.add_enabled(!d.s("catalogUrl").trim().is_empty(), egui::Button::new(tr("Add"))).clicked() {
            match app.run("plugin.catalog.add", json!({"url": d.s("catalogUrl").trim()})) {
                Ok(_) => d.fields.insert("catalogUrl".into(), json!("")),
                Err(e) => d.fields.insert("status".into(), json!(e)),
            };
        }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!app.session.prefs.plugin_catalogs.is_empty(), egui::Button::new(tr("Browse Catalogs"))).clicked() {
            match app.run("plugin.catalog.list", json!({})) {
                Ok(r) => {
                    d.fields.insert("packages".into(), r["packages"].clone());
                    let warnings: Vec<&str> = r["warnings"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                    d.fields.insert("status".into(), json!(warnings.join("\n")));
                }
                Err(e) => {
                    d.fields.insert("status".into(), json!(e));
                }
            }
        }
        if ui.add_enabled(!plugins.is_empty(), egui::Button::new(tr("Check for Updates"))).clicked() {
            match app.run("plugin.update", json!({})) {
                Ok(r) => {
                    let none = r["updates"].as_array().is_none_or(Vec::is_empty);
                    d.fields.insert("updates".into(), r["updates"].clone());
                    d.fields.insert("status".into(), json!(if none { tr("Every plug-in is up to date.") } else { "" }));
                }
                Err(e) => {
                    d.fields.insert("status".into(), json!(e));
                }
            }
        }
    });
    let packages = d.fields.get("packages").and_then(Value::as_array).cloned().unwrap_or_default();
    for pkg in packages {
        ui.horizontal(|ui| {
            let installed = pkg["installed"].as_str().map(|v| format!(" ({} {v})", tr("installed"))).unwrap_or_default();
            crate::rtl::label(ui, format!("{} {}{installed}", pkg["name"].as_str().unwrap_or_default(), pkg["version"].as_str().unwrap_or_default()));
            if ui.small_button(tr("Install…")).clicked() {
                review(app, d, json!({"catalog": pkg["catalog"], "id": pkg["id"]}));
            }
        });
        if let Some(text) = pkg["description"].as_str().filter(|t| !t.is_empty()) {
            crate::rtl::label(ui, egui::RichText::new(text).color(dim));
        }
    }
    let updates = d.fields.get("updates").and_then(Value::as_array).cloned().unwrap_or_default();
    for u in updates {
        ui.horizontal(|ui| {
            crate::rtl::label(
                ui,
                format!(
                    "{} {} → {}",
                    u["id"].as_str().unwrap_or_default(),
                    u["installed"].as_str().unwrap_or_default(),
                    u["available"].as_str().unwrap_or_default()
                ),
            );
            if ui.small_button(tr("Update…")).clicked() {
                review(app, d, u["install"].clone());
            }
        });
    }
}

fn confirm_body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s| crate::i18n::tr(&lang, s);
    let info = d.fields.get("info").cloned().unwrap_or_default();
    let text = |k: &str| info[k].as_str().unwrap_or_default().to_string();
    crate::rtl::label(ui, egui::RichText::new(format!("{} {}", text("name"), text("version"))).font(semibold(14.0)));
    for k in ["author", "description", "license", "repo"] {
        if !text(k).is_empty() {
            crate::rtl::label(ui, text(k));
        }
    }
    egui::Grid::new("plugin_confirm").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Kind"));
        crate::rtl::label(ui, text("kind"));
        ui.end_row();
        crate::rtl::label(ui, tr("Access"));
        let write = info["permissions"]["access"] == "write";
        crate::rtl::label(ui, if write { tr("Reads and edits the open document") } else { tr("Reads the open document") });
        ui.end_row();
        crate::rtl::label(ui, tr("Commands"));
        let allowed: Vec<&str> = info["permissions"]["commands"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        crate::rtl::label(ui, if allowed.is_empty() { tr("Any command its access allows").to_string() } else { allowed.join(", ") });
        ui.end_row();
        if info["permissions"]["saveFile"] == true {
            crate::rtl::label(ui, tr("Files"));
            crate::rtl::label(ui, tr("Asks to save files (DesignCraft doesn't offer this yet)"));
            ui.end_row();
        }
        crate::rtl::label(ui, tr("Source"));
        crate::rtl::label(ui, source_text(&info["source"]));
        ui.end_row();
        crate::rtl::label(ui, "SHA-256");
        ui.add(egui::Label::new(egui::RichText::new(text("sha256")).monospace()).wrap());
        ui.end_row();
    });
    let warn = crate::theme::Tokens::get(ui.ctx()).text_dim;
    if info["inCatalog"] == false {
        crate::rtl::label(
            ui,
            egui::RichText::new(tr("This plug-in isn't in any catalog you use: install it only if you trust its author.")).strong(),
        );
    }
    if let Some(old) = info["replaces"].as_str() {
        crate::rtl::label(ui, egui::RichText::new(tr("Replaces version {version}.").replace("{version}", old)).color(warn));
    }
    if info["permissionsChanged"] == true {
        crate::rtl::label(ui, egui::RichText::new(tr("This version asks for different permissions.")).strong());
    }
    if info["stored"] == false {
        crate::rtl::label(ui, egui::RichText::new(tr("It is kept for this session only.")).color(warn));
    }
}

/// Where a plug-in comes from, in a line.
fn source_text(source: &Value) -> String {
    let t = |k: &str| source[k].as_str().unwrap_or_default();
    match t("type") {
        "github" => format!("github.com/{} {} ({})", t("repo"), t("tag"), t("asset")),
        "catalog" => format!("{} · {}", t("package"), t("catalog")),
        _ => t("path").to_string(),
    }
}

fn params_body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let params: Vec<Param> = d.fields.get("params").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut values = d.fields.get("values").and_then(Value::as_object).cloned().unwrap_or_default();
    egui::Grid::new("plugin_params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        for p in &params {
            crate::rtl::label(ui, p.title());
            let v = values.entry(p.name.clone()).or_insert_with(|| p.default_value());
            field(ui, p, v);
            ui.end_row();
        }
    });
    let _ = app;
    d.fields.insert("values".into(), Value::Object(values));
}

/// One parameter's control, editing `v`.
fn field(ui: &mut egui::Ui, p: &Param, v: &mut Value) {
    match p.kind {
        ParamKind::Number | ParamKind::Slider => {
            let mut n = v.as_f64().unwrap_or(0.0);
            let changed = match (p.kind, p.min, p.max) {
                (ParamKind::Slider, Some(lo), Some(hi)) if lo < hi => ui.add(egui::Slider::new(&mut n, lo..=hi)).changed(),
                _ => {
                    let mut drag = egui::DragValue::new(&mut n);
                    if let (Some(lo), Some(hi)) = (p.min, p.max)
                        && lo <= hi
                    {
                        drag = drag.range(lo..=hi);
                    }
                    ui.add(drag).changed()
                }
            };
            if changed {
                *v = json!(n);
            }
        }
        ParamKind::Boolean => {
            let mut b = v.as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                *v = json!(b);
            }
        }
        ParamKind::Select | ParamKind::List => {
            let current = v.as_str().unwrap_or_default().to_string();
            craft_ui::ComboBox::from_id_salt(("plugin_param", &p.name)).selected_text(current.clone()).show_ui(ui, |ui| {
                for o in &p.options {
                    if ui.selectable_label(*o == current, o.as_str()).clicked() {
                        *v = json!(o);
                    }
                }
            });
        }
        // Text, and a colour as `#RRGGBB`.
        _ => {
            let mut s = v.as_str().unwrap_or_default().to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(240.0)).changed() {
                *v = json!(s);
            }
        }
    }
}

/// OK in one of the dialogs: Done closes Manage Plug-ins, Install installs what was reviewed, Run
/// runs the command with the parameters entered.
pub fn confirm(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        CONFIRM => {
            let mut request = d.fields.get("request").cloned().unwrap_or_default();
            let info = d.fields.get("info").cloned().unwrap_or_default();
            request["sha256"] = info["sha256"].clone();
            let r = app.run("plugin.install", request);
            let status = match &r {
                Ok(_) => crate::i18n::tr(&app.ui.language, "Installed {name}.").replace("{name}", info["name"].as_str().unwrap_or_default()),
                Err(e) => e.clone(),
            };
            app.ui.dialog = Some(Dialog::new(MANAGE, json!({"status": status})));
            r
        }
        PARAMS => {
            let params = json!({"id": d.fields.get("id"), "command": d.fields.get("command"), "params": d.fields.get("values")});
            run_plugin(app, params)
        }
        _ => Ok(Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use designcraft_engine::plugin::craft_plugin::bundle::Bundle;
    use serde_json::json;

    use super::*;

    /// A plug-in whose `frame` command makes a frame `size` points wide.
    fn install(app: &mut DesignApp) {
        let wasm = wat::parse_str(
            r#"(module
              (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "frame.create")
              (data (i32.const 32) "{\"rect\":[36,36,136,136]}")
              (func (export "craft_run") (param i32) (result i32)
                (drop (call $call (i32.const 0) (i32.const 12) (i32.const 32) (i32.const 24))) (i32.const 0)))"#,
        )
        .unwrap();
        let manifest = json!({"id": "io.example.ui", "name": "UI Test", "version": "1.0.0", "main": "p.wasm",
            "apps": {"designcraft": {"api": 6}}, "permissions": {"access": "write"},
            "commands": [{"id": "plain", "label": "Plain"},
                         {"id": "asks", "label": "Asks…", "params": [{"name": "count", "type": "number", "default": 2, "min": 1, "max": 9}]}]});
        let bytes = Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode();
        let data = designcraft_engine::cmd::base64_encode(&bytes);
        // Review, then install what was reviewed, as the confirmation does.
        let mut d = Dialog::new(MANAGE, json!({}));
        review(app, &mut d, json!({"data": data}));
        assert_eq!(d.id, CONFIRM, "{:?}", d.fields.get("status"));
        assert_eq!(d.fields["info"]["permissions"]["access"], "write");
        confirm(app, d).unwrap();
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(MANAGE));
    }

    #[test]
    fn the_plugins_menu_lists_commands_and_asks_for_parameters() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let items = menu_items(&app);
        assert!(matches!(items.as_slice(), [Item::Cmd { id, .. }] if id == "app.pluginsDialog"));
        install(&mut app);
        let items = menu_items(&app);
        let Some(Item::Sub(name, commands)) = items.first() else { panic!("{items:?}") };
        assert_eq!((name.as_str(), commands.len()), ("UI Test", 2));
        // A command without parameters runs at once, as one undo step.
        app.ui.dialog = None;
        app.run(RUN_FROM_MENU, json!({"id": "io.example.ui", "command": "plain"})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 1);
        // One with parameters asks first.
        app.run(RUN_FROM_MENU, json!({"id": "io.example.ui", "command": "asks"})).unwrap();
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!(d.id, PARAMS);
        assert_eq!(d.fields["values"]["count"], 2.0);
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 2);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 1);
    }

    #[test]
    fn the_install_confirmation_says_when_a_repository_is_in_no_catalog() {
        let info = json!({"name": "X", "version": "1.0.0", "kind": "command", "permissions": {"access": "read"},
            "source": {"type": "github", "repo": "example/x", "tag": "v1", "asset": "x.craftplugin"}, "sha256": "ab", "inCatalog": false});
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.dialog = Some(Dialog::new(CONFIRM, json!({"request": {"repo": "example/x"}, "info": info})));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| crate::dialogs::show(&mut app, ui.ctx()));
        out.textures_delta.clear();
        assert_eq!(source_text(&info["source"]), "github.com/example/x v1 (x.craftplugin)");
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(CONFIRM), "the confirmation stays until a choice");
    }
}
