use super::export::{download_document, export_bytes, export_epub, export_idml, export_pdf, export_png, separations_view};
use super::invoke::guide_spread;
use super::state::{is_window_control, menu_enabled, window_control_available};
use super::tables::{CLASSIC_GROUPS, TYPOGRAPHY_GROUPS, workspace_open_panel};
use super::tree::recent_label;
use super::{DesignApp, ScreenMode, Value, json};
use crate::app::file_dialogs::{Bound, with_path};

/// Type › Tabs: float the Tabs panel (placed above the selected text frame) or close it.
fn toggle_tabs_panel(app: &mut DesignApp) {
    if app.ui.floating.iter().any(|(p, _)| p == "tabs") {
        app.ui.floating.retain(|(p, _)| p != "tabs");
    } else {
        let at = app.canvas_rect.map_or(egui::pos2(300.0, 140.0), |r| r.min + egui::vec2(80.0, 60.0));
        app.ui.floating.push(("tabs".into(), [at.x, at.y]));
        app.ui.tabs_panel.snap_request = 2;
    }
}

/// Asks for a file to open for `purpose`, then runs the UI command `id` again with `p` and the
/// chosen file under `key`.
#[track_caller]
fn rerun_with_path(app: &mut DesignApp, purpose: &str, id: &str, p: &Value, key: &'static str) -> Result<Value, String> {
    let (id, p) = (id.to_string(), p.clone());
    app.ask_open(purpose, Bound::Document, move |app, path| run_ui(app, &id, &with_path(&p, key, path)).unwrap_or(Ok(Value::Null)))
}

/// File › Save As: asks where, saves there, then runs `after`.
#[track_caller]
fn save_as_dialog(app: &mut DesignApp, after: impl FnOnce(&mut DesignApp) -> Result<Value, String> + 'static) -> Result<Value, String> {
    let name = app.session.active().map(|d| format!("{}.designcraft", d.doc.title)).unwrap_or_default();
    app.ask_save(&name, Bound::Document, move |app, path| {
        app.run("file.saveAs", json!({"path": path}))?;
        after(app)
    })
}

/// Save the active document (a download in a browser; Save As for one never saved), then run
/// `after` once it is written. A save dialog answers on a later frame; Cancel runs nothing.
#[track_caller]
pub(crate) fn save_then(app: &mut DesignApp, after: impl FnOnce(&mut DesignApp) -> Result<Value, String> + 'static) -> Result<Value, String> {
    if app.services.download.is_some() {
        download_document(app)?;
        return after(app);
    }
    if app.session.active().is_some_and(|d| d.path.is_some()) {
        app.run("file.save", json!({}))?;
        return after(app);
    }
    save_as_dialog(app, after)
}

/// Run a UI command; `None` if `id` isn't one.
pub fn run_ui(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let was_find = app.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange");
    let result = run_ui_inner(app, id, p);
    if was_find && app.ui.dialog.as_ref().is_none_or(|d| d.id != "findChange") {
        app.session.reset_find_scope();
    }
    result
}

fn run_ui_inner(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if is_window_control(id) && !window_control_available(app, id) {
        return Some(Err("custom window controls are not available in this host".into()));
    }
    if let Some(result) = crate::panel_docking::command(app, id, p) {
        return Some(result);
    }
    run_ui_part1(app, id, p)
        .or_else(|| run_ui_part2(app, id, p))
        .or_else(|| run_ui_part3(app, id, p))
        .or_else(|| run_ui_part4(app, id, p))
        .or_else(|| run_ui_part5(app, id, p))
}

/// Toggle `b`, answering its new value.
fn flag(b: &mut bool) -> Result<Value, String> {
    *b = !*b;
    Ok(json!(*b))
}

/// One slice of the UI commands: `None` if `id` isn't one of them.
fn run_ui_part1(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        "window.minimize" => {
            app.pending_window_commands.push(egui::ViewportCommand::Minimized(true));
            Ok(Value::Null)
        }
        "window.maximize" => {
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(!app.window_maximized);
            app.pending_window_commands.push(egui::ViewportCommand::Maximized(on));
            Ok(json!(on))
        }
        "window.close" => {
            app.pending_window_commands.push(egui::ViewportCommand::Close);
            Ok(Value::Null)
        }
        "window.drag" => {
            app.pending_window_commands.push(egui::ViewportCommand::StartDrag);
            Ok(Value::Null)
        }
        "window.resize" => {
            let edge = match p.get("edge").and_then(Value::as_str) {
                Some("north") => egui::ResizeDirection::North,
                Some("south") => egui::ResizeDirection::South,
                Some("east") => egui::ResizeDirection::East,
                Some("west") => egui::ResizeDirection::West,
                Some("northEast") => egui::ResizeDirection::NorthEast,
                Some("southEast") => egui::ResizeDirection::SouthEast,
                Some("northWest") => egui::ResizeDirection::NorthWest,
                Some("southWest") => egui::ResizeDirection::SouthWest,
                _ => return Some(Err("invalid window resize edge".into())),
            };
            app.pending_window_commands.push(egui::ViewportCommand::BeginResize(edge));
            Ok(Value::Null)
        }
        "app.newDocumentDialog" => {
            // In the default units (Preferences with no document open).
            let fields = crate::new_document::defaults_for(&app.session.default_new_document());
            app.ui.dialog = Some(crate::dialogs::Dialog::new("newDocument", fields));
            Ok(Value::Null)
        }
        "app.openDialog" => match p.get("path").and_then(Value::as_str) {
            Some(path) => app.open_picked("open", path.to_string()),
            None => app.pick_and_open("open"),
        },
        "app.openRecent" => {
            let index = p.get("index").cloned().unwrap_or(Value::Null);
            let name = index
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| app.session.prefs.recent_files.get(i))
                .map(|path| recent_label(path))
                .unwrap_or_default();
            app.open_named_file("file.openRecent", json!({"index": index}), name)
        }
        "app.placeDialog" => match p.get("path").and_then(Value::as_str) {
            Some(path) => app.open_picked("place", path.to_string()),
            None => app.pick_and_open("place"),
        },
        "app.placeWithOptions" => {
            // Word/RTF Import Options: style mapping and conflicts before the text lands.
            let Some(path) = p.get("path").and_then(Value::as_str) else { return Some(rerun_with_path(app, "place", id, p, "path")) };
            let l = path.to_lowercase();
            if !(l.ends_with(".docx") || l.ends_with(".rtf")) {
                return Some(app.place_file(json!({"path": path})));
            }
            let info = match app.run("place.styles", json!({"path": path})) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            app.ui.dialog = Some(crate::dialogs::Dialog::new(
                "importOptions",
                json!({"path": path, "styles": info["paragraph"], "charStyles": info["character"], "conflicts": info["conflicts"], "styleConflicts": "useExisting", "removeStyles": false, "map": {}}),
            ));
            Ok(Value::Null)
        }
        "app.save" | "app.saveDialog" if app.services.download.is_some() => download_document(app),
        "app.save" => {
            if app.session.active().is_some_and(|d| d.path.is_some()) {
                return Some(app.run("file.save", json!({})));
            }
            return run_ui(app, "app.saveDialog", p);
        }
        "app.saveDialog" => match p.get("path").and_then(Value::as_str) {
            Some(path) => app.run("file.saveAs", json!({"path": path})),
            None => save_as_dialog(app, |_| Ok(Value::Null)),
        },
        "app.quit" => {
            app.request_quit();
            Ok(Value::Null)
        }
        "app.saveCopyDialog" => match p.get("path").and_then(Value::as_str) {
            Some(path) => app.run("file.saveACopy", json!({"path": path})),
            None => {
                let name = app.session.active().map(|d| format!("{} copy.designcraft", d.doc.title)).unwrap_or_default();
                app.ask_save(&name, Bound::Document, |app, path| app.run("file.saveACopy", json!({"path": path})))
            }
        },
        "app.exportPng" => export_png(app, p),
        "app.exportIdml" => export_idml(app, p),
        // From the menu, EPUB Export Options first; with a path, straight to the file.
        "app.exportEpub" if p.get("path").is_some() => export_epub(app, p),
        "app.exportEpub" => {
            crate::dialogs::open_epub_export(app);
            Ok(Value::Null)
        }
        "app.exportInteractivePdf" => {
            let mut q = if p.is_object() { p.clone() } else { json!({}) };
            for (k, v) in [("pageLayout", json!("single")), ("view", json!("fitPage")), ("bookmarksPanel", json!(true)), ("media", json!(true))] {
                q.as_object_mut().map(|o| o.entry(k).or_insert(v));
            }
            export_pdf(app, &q)
        }
        "app.exportFixedEpub" => export_bytes(app, p, "epub", "file.exportFixedEpub"),
        "app.exportHtml" => export_bytes(app, p, "html", "file.exportHtml"),
        "app.exportPptx" => export_bytes(app, p, "pptx", "file.exportPptx"),
        "app.exportXml" => export_bytes(app, p, "xml", "file.exportXml"),
        "app.printBooklet" => export_bytes(app, p, "pdf", "file.printBooklet"),
        "app.menus" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("menus", json!({"query": ""})));
            Ok(Value::Null)
        }
        "app.cellOptionsDialog" => crate::dialogs::open_cell_options(app),
        "window.hideMenuItem" => {
            let Some(item) = p.get("item").and_then(Value::as_str) else { return Some(Err("missing item".into())) };
            let hide = p.get("hidden").and_then(Value::as_bool).unwrap_or(true);
            app.ui.hidden_menu_items.retain(|x| x != item);
            if hide {
                app.ui.hidden_menu_items.push(item.to_string());
            }
            app.ui.show_full_menus = false;
            Ok(Value::Null)
        }
        "app.placeAndLink" => {
            let target = app.session.active().and_then(|d| {
                let id = *d.selection.items.first()?;
                let it = d.doc.item(id)?;
                let loc = d.doc.find(id)?;
                Some((it.text_frame()?.story, it.bounds(), loc.spread))
            });
            let Some((sid, b, sr)) = target else { return Some(Err("select a text frame".into())) };
            Ok(app
                .run("story.placeAndLink", json!({"story": sid.0, "rect": [b.x0 + 24.0, b.y0 + 24.0, b.x1 + 24.0, b.y1 + 24.0], "spread": sr}))
                .unwrap_or(Value::Null))
        }
        "app.fittingOptions" => {
            let g = app.session.active().and_then(|d| d.selection.items.first().and_then(|id| d.doc.item(*id)).and_then(|it| it.graphic().cloned()));
            let Some(g) = g else { return Some(Err("select a frame with a placed graphic".into())) };
            let fitting = serde_json::to_value(g.auto_fit).unwrap_or(json!("none"));
            let c = |v: f64| json!(designcraft_geom::format_measure(v, designcraft_geom::Unit::Points));
            app.ui.dialog = Some(crate::dialogs::Dialog::new(
                "fittingOptions",
                json!({"autoFit": g.auto_fit != designcraft_doc::Fitting::None, "fitting": fitting, "align": g.fit_align,
                    "cropTop": c(g.crop[0]), "cropLeft": c(g.crop[1]), "cropBottom": c(g.crop[2]), "cropRight": c(g.crop[3])}),
            ));
            Ok(Value::Null)
        }
        "app.userDictionary" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("userDictionary", json!({"word": ""})));
            Ok(Value::Null)
        }
        "app.qrCode" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("qrCode", json!({})));
            Ok(Value::Null)
        }
        "app.exportText" => export_bytes(app, p, "rtf", "file.exportText"),
        "app.packageDialog" => {
            // Without a folder, the Package dialog: what the package carries, then the folder.
            let Some(dir) = p.get("dir").and_then(Value::as_str) else {
                return Some(crate::package_dialog::open(app).map(|()| Value::Null));
            };
            let r = app.run("file.package", json!({"dir": dir}));
            if let Ok(v) = &r {
                app.status(
                    crate::i18n::tr(&app.ui.language, "Packaged {count} files into {dir}")
                        .replace("{count}", &v["files"].as_array().map_or(0, |f| f.len()).to_string())
                        .replace("{dir}", v["dir"].as_str().unwrap_or("")),
                );
            }
            r
        }
        "app.language" => {
            let Some(v) = p.get("lang").and_then(Value::as_str) else { return Some(app.run("app.preferences", json!({}))) };
            if !crate::i18n::LANGUAGES.iter().any(|(c, _)| *c == v) {
                return Some(Err(format!("unknown language `{v}`")));
            }
            app.ui.language = v.to_string();
            Ok(json!(v))
        }
        "app.flattener" => {
            let v = p.get("preset").and_then(Value::as_str).unwrap_or("");
            if !["", "high", "medium", "low"].contains(&v) {
                return Some(Err(format!("unknown preset `{v}`")));
            }
            app.ui.flattener = v.to_string();
            Ok(json!(v))
        }
        "app.graphicCell" => match p.get("path").and_then(Value::as_str) {
            Some(path) => app.run("table.placeGraphic", json!({"path": path})),
            None => rerun_with_path(app, "place", id, p, "path"),
        },
        "effects.dialog" => {
            crate::effects_dialog::open(app, p.get("page").and_then(Value::as_str).unwrap_or("transparency"));
            Ok(Value::Null)
        }
        "app.loadSwatches" => {
            if let Some(path) = p.get("path").and_then(Value::as_str) {
                return Some(app.run("swatch.load", json!({"path": path})));
            }
            let request = app.import_request("swatches");
            if let Some(open) = app.services.open_async.as_mut() {
                open(request);
                return Some(Ok(Value::Null));
            }
            app.ask_open("swatches", Bound::Document, |app, path| app.run("swatch.load", json!({"path": path})))
        }
        _ => return None,
    })
}

/// One slice of the UI commands: `None` if `id` isn't one of them.
fn run_ui_part2(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        "app.loadParagraphStyles" => app.pick_styles("paragraph"),
        "app.loadCharacterStyles" => app.pick_styles("character"),
        "app.loadTextStyles" => app.pick_styles(""),
        "app.saveSwatches" => {
            let Some(path) = p.get("path").and_then(Value::as_str).map(str::to_string) else {
                let p = p.clone();
                return Some(app.ask_save("Swatches.ase", Bound::Document, move |app, path| {
                    run_ui(app, "app.saveSwatches", &with_path(&p, "path", path)).unwrap_or(Ok(Value::Null))
                }));
            };
            let r = match app.run("swatch.save", json!({})) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
            match app.services.write.as_mut() {
                Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
                None => Err("no writer".into()),
            }
        }
        "app.storyEditor" => {
            let target = app.session.active().and_then(|st| {
                let story = p.get("story").and_then(Value::as_u64).map(designcraft_doc::StoryId).or_else(|| {
                    st.selection
                        .text
                        .map(|t| t.story)
                        .or_else(|| st.selection.items.iter().find_map(|i| st.doc.item(*i)?.text_frame().map(|t| t.story)))
                })?;
                Some(crate::story_editor::Target { doc: st.uid, story })
            });
            match target {
                Some(t) => {
                    app.story_editor = if app.story_editor == Some(t) { None } else { Some(t) };
                    Ok(Value::Null)
                }
                None => Err("select a text frame or place the cursor in text".into()),
            }
        }
        "app.findChange" => {
            app.session.reset_find_scope();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("findChange", json!({})));
            Ok(Value::Null)
        }
        "app.convertTextToTableDialog" => {
            if !menu_enabled(app, id) {
                return Some(Err("select the text to convert to a table".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("convertTextToTable", p.clone()));
            Ok(Value::Null)
        }
        "app.convertTableToTextDialog" => {
            if !menu_enabled(app, id) {
                return Some(Err("place the insertion point in a table to convert it to text".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("convertTableToText", p.clone()));
            Ok(Value::Null)
        }
        "app.insertTableDialog" => {
            if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
                return Some(Err("place the insertion point in a text frame to create a table".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("insertTable", p.clone()));
            Ok(Value::Null)
        }
        "app.rubyDialog" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let cur = st.selection.text.and_then(|t| {
                let story = st.doc.text_story(t.story, t.cell)?;
                let at = if t.range().is_empty() { t.range().start } else { t.range().start + 1 };
                story.char_format_at(at).over.ruby.clone()
            });
            let cur = cur.unwrap_or_default();
            // The ruby settings the panes show: the selection's resolved character attributes.
            let base = crate::panels::text_attrs(app).and_then(|a| a.get("chars").cloned()).unwrap_or(Value::Null);
            app.ui.dialog = Some(crate::dialogs::Dialog::new("ruby", json!({"text": cur, "base": base})));
            Ok(Value::Null)
        }
        "app.paragraphRulesDialog" => {
            let Some(a) = crate::panels::text_attrs(app) else { return Some(Err("select text or a text frame".into())) };
            let rule = if p.get("rule").and_then(Value::as_str) == Some("ruleBelow") { "ruleBelow" } else { "ruleAbove" };
            let current = json!({"ruleAbove": a["para"]["ruleAbove"], "ruleBelow": a["para"]["ruleBelow"]});
            app.ui.dialog = Some(crate::dialogs::Dialog::new("paragraphRules", json!({"rule": rule, "current": current})));
            Ok(Value::Null)
        }
        "app.paragraphBordersDialog"
        | "app.spanColumnsDialog"
        | "app.bulletsNumberingDialog"
        | "app.keepOptionsDialog"
        | "app.nestedStylesDialog"
        | "app.grepStylesDialog" => {
            let Some(a) = crate::panels::text_attrs(app) else { return Some(Err("select text or a text frame".into())) };
            let section = match id {
                "app.spanColumnsDialog" => "span",
                "app.bulletsNumberingDialog" => "bullets",
                "app.keepOptionsDialog" => "keep",
                "app.nestedStylesDialog" => "nested",
                "app.grepStylesDialog" => "grep",
                _ if p.get("tab").and_then(Value::as_str) == Some("shading") => "shading",
                _ => "border",
            };
            let fields = json!({
                "section": section,
                "current": a["para"],
                "family": a["chars"]["fontFamily"],
                "style": a["chars"]["fontStyle"],
            });
            app.ui.dialog = Some(crate::dialogs::Dialog::new("paragraphLocal", fields));
            Ok(Value::Null)
        }
        "app.hyphenationDialog" => {
            let Some(a) = app.session.execute("type.selectionAttrs", &json!({"mixed": true})).ok().filter(|a| !a.is_null()) else {
                return Some(Err("select text or a text frame".into()));
            };
            let mixed: Vec<&str> = a["paraMixed"].as_array().map(|m| m.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let current: serde_json::Map<String, Value> = crate::dialogs::HYPH_ATTRS
                .iter()
                .map(|k| (k.to_string(), if mixed.contains(k) { Value::Null } else { a["para"][*k].clone() }))
                .collect();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("hyphenation", json!({"current": current})));
            Ok(Value::Null)
        }
        "app.justificationDialog" => {
            let Some(a) = app.session.execute("type.selectionAttrs", &json!({"mixed": true})).ok().filter(|a| !a.is_null()) else {
                return Some(Err("select text or a text frame".into()));
            };
            let mixed: Vec<&str> = a["paraMixed"].as_array().map(|m| m.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let current: serde_json::Map<String, Value> = crate::dialogs::JUSTIFY_ATTRS
                .iter()
                .map(|k| (k.to_string(), if mixed.contains(k) { Value::Null } else { a["para"][*k].clone() }))
                .collect();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("justification", json!({"current": current})));
            Ok(Value::Null)
        }
        "app.footnoteOptionsDialog" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let o = serde_json::to_value(&st.doc.footnote_options).unwrap_or_default();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("footnoteOptions", o));
            Ok(Value::Null)
        }
        "app.findFontDialog" => {
            if app.session.active().is_none() {
                return Some(Err("no document open".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("findFont", json!({})));
            Ok(Value::Null)
        }
        "app.insertXrefDialog" => {
            if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
                return Some(Err("place the insertion point in text to insert a cross-reference".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("insertXref", json!({})));
            Ok(Value::Null)
        }
        "app.deleteAllGuides" => {
            let Some(spread) = guide_spread(app) else { return Some(Err("no document open".into())) };
            return Some(app.run("guide.deleteAll", json!({"spread": spread})));
        }
        "app.deletePage" | "app.duplicateSpread" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let page = crate::canvas::current_page(app).unwrap_or(0);
            let r = if id == "app.deletePage" {
                crate::dialogs::delete_pages(app, vec![page])
            } else {
                let spread = st.doc.page_loc(page).map_or(0, |l| l.0);
                app.run("layout.pages.duplicateSpread", json!({"spread": spread}))
            };
            return Some(r);
        }
        "app.tabsPanel" => {
            toggle_tabs_panel(app);
            Ok(Value::Null)
        }
        "app.exportPdf" => {
            if p.is_null() || p.as_object().is_none_or(|m| m.is_empty()) {
                crate::dialogs::open_pdf_export(app);
                Ok(Value::Null)
            } else {
                export_pdf(app, p)
            }
        }
        "app.palette" => {
            app.ui.palette = Some(String::new());
            Ok(Value::Null)
        }
        "app.preferences" => {
            let mut f = app.session.execute("prefs.set", &json!({})).unwrap_or_default();
            if app.session.active().is_some() {
                let doc = app.session.execute("document.preferences", &json!({})).unwrap_or_default();
                f["horizontalUnits"] = doc["horizontalUnits"].clone();
                f["rulerOrigin"] = doc["rulerOrigin"].clone();
                f["overprintBlack"] = doc["overprintBlack"].clone();
                f["glyphFallback"] = doc["glyphFallback"].clone();
                for k in ["superscriptSize", "superscriptPosition", "subscriptSize", "subscriptPosition"] {
                    f[format!("adv.{k}")] = json!(format!("{}", doc["advancedType"][k].as_f64().unwrap_or(0.0)));
                }
                f["verticalUnits"] = doc["verticalUnits"].clone();
                let words = app.session.execute("spelling.words", &json!({})).unwrap_or_default();
                f["userWords"] =
                    json!(words.as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_default());
                let inc = doc["keyboardIncrement"].as_f64().unwrap_or(1.0);
                f["keyboardIncrement"] = json!(designcraft_geom::format_measure(inc, designcraft_geom::Unit::Points));
                f["kerningIncrement"] = json!(format!("{}", doc["kerningIncrement"].as_f64().unwrap_or(20.0)));
                let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or_default();
                let m = |v: &Value| json!(designcraft_geom::format_measure(v.as_f64().unwrap_or(0.0), units));
                let (bg, g) = (&doc["baselineGrid"], &doc["grid"]);
                f["bg.start"] = m(&bg["start"]);
                f["bg.increment"] = m(&bg["increment"]);
                f["bg.relativeTo"] = bg["relativeTo"].clone();
                f["bg.viewThreshold"] = json!((bg["viewThreshold"].as_f64().unwrap_or(0.75) * 100.0).round());
                f["bg.color"] = bg["color"].clone();
                f["grid.horizontal"] = m(&g["horizontal"]);
                f["grid.vertical"] = m(&g["vertical"]);
                f["grid.subdivisions"] = g["subdivisions"].clone();
                f["grid.inBack"] = g["inBack"].clone();
                f["grid.color"] = g["color"].clone();
                for k in ["marginColor", "columnColor", "bleedColor", "slugColor"] {
                    f[k] = doc[k].clone();
                }
                f["pasteboard.h"] = m(&doc["pasteboard"][0]);
                f["pasteboard.v"] = m(&doc["pasteboard"][1]);
            } else {
                // No document: Units & Increments are what new documents start with, as InDesign's.
                let du = app.session.prefs.default_units;
                f["defaults"] = json!(true);
                f["horizontalUnits"] = json!(du.horizontal_units);
                f["verticalUnits"] = json!(du.vertical_units);
                f["rulerOrigin"] = json!(du.ruler_origin);
                f["keyboardIncrement"] = json!(designcraft_geom::format_measure(du.keyboard_increment, designcraft_geom::Unit::Points));
                f["kerningIncrement"] = json!(format!("{}", du.kerning_increment));
            }
            f["snap.alignEdges"] = json!(app.ui.align_edges);
            f["snap.alignCenters"] = json!(app.ui.align_centers);
            f["snap.dimensions"] = json!(app.ui.smart_dimensions);
            f["snap.spacing"] = json!(app.ui.smart_spacing);
            f["snap.zone"] = json!(app.ui.snap_zone);
            f["displayQuality"] = json!(match app.ui.display_quality {
                designcraft_render::DisplayQuality::Fast => "fast",
                designcraft_render::DisplayQuality::Typical => "typical",
                designcraft_render::DisplayQuality::High => "high",
            });
            f["uiScale"] = json!((app.ui.ui_scale * 100.0).round());
            f["autoCollapseIconicPanels"] = json!(app.ui.dock_auto_collapse);
            // Interface: Cancel goes back to the theme Preferences opened with.
            f["brightness"] = json!(app.ui.brightness.id());
            f["openedBrightness"] = json!(app.ui.brightness.id());
            f["toolsColumns"] = json!(if app.ui.tools_double_column { "double" } else { "single" });
            f["richBlack"] = json!(app.ui.rich_black);
            f["dynamicSpelling"] = json!(app.ui.dynamic_spelling);
            f["storyEditorSize"] = json!(app.ui.story_editor_size);
            let list = app.session.prefs.autocorrect_list.iter().map(|(a, b)| format!("{a} → {b}")).collect::<Vec<_>>().join("\n");
            f["autocorrectText"] = json!(list);
            // The effective setting; it becomes an explicit preference only when the user changes it,
            // so an untouched one keeps following the interface language.
            let cjk = crate::cjk_features(app);
            f["cjkFeatures"] = json!(cjk);
            f["cjkFeatures.initial"] = json!(cjk);
            f["section"] = json!("general");
            // The language previews as it changes; Cancel goes back to this one.
            f["openedLanguage"] = json!(app.ui.language);
            app.ui.dialog = Some(crate::dialogs::Dialog::new("preferences", f));
            Ok(Value::Null)
        }
        _ => return None,
    })
}

/// One slice of the UI commands: `None` if `id` isn't one of them.
fn run_ui_part3(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let rect = app.canvas_rect;
    Some(match id {
        "view.zoomIn" | "view.zoomOut" => {
            if let Some(r) = rect {
                crate::canvas::zoom_at(app, r.center(), if id == "view.zoomIn" { 1.5 } else { 1.0 / 1.5 });
            }
            Ok(Value::Null)
        }
        "view.fitSelection" => {
            if let Some(r) = rect {
                crate::canvas::fit(app, r, "selection");
            }
            Ok(Value::Null)
        }
        "view.fitPage" | "view.fitSpread" | "view.entirePasteboard" => {
            if let Some(r) = rect {
                crate::canvas::fit(
                    app,
                    r,
                    if id == "view.fitPage" {
                        "page"
                    } else if id == "view.fitSpread" {
                        "spread"
                    } else {
                        "all"
                    },
                );
            }
            Ok(Value::Null)
        }
        "view.actualSize" => {
            crate::canvas::set_zoom(app, 1.0);
            Ok(Value::Null)
        }
        "view.zoom" => {
            crate::canvas::set_zoom(app, p.get("zoom").and_then(Value::as_f64).unwrap_or(1.0));
            Ok(Value::Null)
        }
        "view.screenMode" => {
            let mode = match p.get("mode").and_then(Value::as_str).unwrap_or("normal") {
                "preview" => ScreenMode::Preview,
                "bleed" => ScreenMode::Bleed,
                "slug" => ScreenMode::Slug,
                "presentation" => ScreenMode::Presentation,
                _ => ScreenMode::Normal,
            };
            set_screen_mode(app, mode);
            Ok(Value::Null)
        }
        "view.togglePreview" => {
            let mode = if app.ui.screen_mode == ScreenMode::Normal { ScreenMode::Preview } else { ScreenMode::Normal };
            set_screen_mode(app, mode);
            Ok(Value::Null)
        }
        "view.togglePresentation" => {
            let mode = match (app.ui.screen_mode, app.ui.presentation_return) {
                // A settings file can hold Presentation as the return mode; leaving must still leave.
                (ScreenMode::Presentation, ScreenMode::Presentation) => ScreenMode::Normal,
                (ScreenMode::Presentation, back) => back,
                _ => ScreenMode::Presentation,
            };
            set_screen_mode(app, mode);
            Ok(Value::Null)
        }
        "color.defaultFillStroke" => crate::toolbar::default_fill_stroke(app),
        "color.swapFillStroke" => crate::toolbar::swap_fill_stroke(app),
        "color.toggleFillStroke" => crate::toolbar::toggle_fill_stroke(app),
        "color.applyColor" => crate::toolbar::apply_color(app),
        "color.applyGradient" => crate::toolbar::apply_gradient(app),
        "color.applyNone" => crate::toolbar::apply_none(app),
        "view.frameEdges" => flag(&mut app.ui.frame_edges),
        "view.overprintPreview" => {
            app.canvas.shown = None;
            flag(&mut app.ui.overprint_preview)
        }
        "view.fastDisplay" | "view.typicalDisplay" | "view.highQualityDisplay" => {
            use designcraft_render::DisplayQuality as Q;
            app.ui.display_quality = match id {
                "view.fastDisplay" => Q::Fast,
                "view.typicalDisplay" => Q::Typical,
                _ => Q::High,
            };
            // Redraw everything at the new quality.
            app.canvas.shown = None;
            Ok(Value::Null)
        }
        "view.rulers" => flag(&mut app.ui.rulers),
        "view.guides" => flag(&mut app.ui.guides),
        "view.lockGuides" => {
            app.ui.guides_locked = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.guides_locked);
            Ok(json!(app.ui.guides_locked))
        }
        "view.snapToGuides" => flag(&mut app.ui.snap_to_guides),
        "view.snapToDocumentGrid" => flag(&mut app.ui.snap_to_document_grid),
        "view.smartGuides" => flag(&mut app.ui.smart_guides),
        "view.baselineGrid" => flag(&mut app.ui.baseline_grid),
        "view.documentGrid" => {
            // The grid is drawn into the canvas texture.
            app.canvas.shown = None;
            flag(&mut app.ui.document_grid)
        }
        "view.textThreads" => flag(&mut app.ui.text_threads),
        "view.hiddenCharacters" => flag(&mut app.ui.hidden_characters),
        "view.taggedFrames" => flag(&mut app.ui.tagged_frames),
        "view.storyEditorDepthRuler" => flag(&mut app.ui.story_editor_depth_ruler),
        "window.hidePanels" | "window.hidePanelsExceptTools" => {
            let mode = if id == "window.hidePanels" { 1 } else { 2 };
            app.ui.hidden_panels = if app.ui.hidden_panels == mode { 0 } else { mode };
            Ok(json!(app.ui.hidden_panels))
        }
        "window.nextDocument" | "window.previousDocument" => {
            let n = app.session.documents().len();
            if n > 1 {
                let cur = app.session.active_index().unwrap_or(0);
                let next = if id == "window.nextDocument" { (cur + 1) % n } else { (cur + n - 1) % n };
                app.session.set_active(next);
            }
            Ok(json!(app.session.active_index()))
        }
        "app.layerOptionsDialog" => crate::dialogs::open_layer_options(app, p.get("id").and_then(Value::as_u64)).map(|()| Value::Null),
        "app.swatchOptionsDialog" => {
            crate::dialogs::open_swatch_options(app, p.get("name").and_then(Value::as_str).unwrap_or("")).map(|()| Value::Null)
        }
        "app.colorGroupOptionsDialog" => {
            crate::dialogs::open_color_group_options(app, p.get("name").and_then(Value::as_str).unwrap_or("")).map(|()| Value::Null)
        }
        "app.layerOptions" => {
            let layers = match app.session.execute("object.pdfLayers", &json!({})) {
                Ok(v) => v,
                Err(e) => return Some(Err(e.to_string())),
            };
            app.ui.dialog = Some(crate::dialogs::Dialog::new(
                "layerOptions",
                json!({"useDefaults": !layers.as_array().and_then(|a| a.first()).and_then(|l| l["overridden"].as_bool()).unwrap_or(false), "layers": layers}),
            ));
            Ok(Value::Null)
        }
        "view.rotateSpread" => {
            // Turns the spread in the middle of the view (InDesign's Rotate Spread).
            let angle = p.get("angle").and_then(Value::as_i64).unwrap_or(90);
            let Some(st) = app.session.active() else { return Some(Err("no document".into())) };
            let layout = designcraft_tools::CanvasLayout::new(&st.doc, st.editing_parents);
            let Some(slot) = crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i).copied()) else {
                return Some(Err("no spread".into()));
            };
            let designcraft_doc::SpreadRef::Doc(si) = slot.spread else { return Some(Err("parent spreads don't turn".into())) };
            let r = app.run("layout.rotateSpreadView", json!({"spread": si, "angle": angle}));
            if let Some(rect) = app.canvas_rect {
                crate::canvas::fit(app, rect, "spread");
            }
            app.canvas.shown = None;
            r.map(|v| v["rotation"].clone())
        }
        _ => return None,
    })
}

/// One slice of the UI commands: `None` if `id` isn't one of them.
fn run_ui_part4(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        "view.proofColors" => {
            app.ui.proof_colors = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.proof_colors);
            app.canvas.shown = None;
            Ok(json!(app.ui.proof_colors))
        }
        "view.proofSetup" => {
            if let Some(t) = p.get("target").and_then(Value::as_str) {
                let Some(t) = designcraft_color::cms::ProofTarget::parse(t) else { return Some(Err(format!("unknown proof target `{t}`"))) };
                app.ui.proof_setup.target = t;
                app.ui.proof_colors = true;
            }
            if let Some(i) = p.get("intent").and_then(Value::as_str) {
                let Some(i) = designcraft_color::cms::Intent::parse(i) else { return Some(Err(format!("unknown intent `{i}`"))) };
                app.ui.proof_setup.intent = i;
            }
            match p.get("simulatePaper") {
                Some(Value::Bool(b)) => app.ui.proof_setup.simulate_paper = *b,
                Some(Value::String(s)) if s == "toggle" => app.ui.proof_setup.simulate_paper = !app.ui.proof_setup.simulate_paper,
                _ => {}
            }
            app.canvas.shown = None;
            Ok(json!({"target": app.ui.proof_setup.target.id(), "simulatePaper": app.ui.proof_setup.simulate_paper, "on": app.ui.proof_colors}))
        }
        "app.colorSettings" => {
            let f = app.session.execute("color.settings", &json!({})).unwrap_or_default();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("colorSettings", f));
            Ok(Value::Null)
        }
        "view.separations" => separations_view(app, p),
        "view.firstPage" | "view.previousPage" | "view.nextPage" | "view.lastPage" => {
            let n = app.session.active().map_or(0, |d| d.doc.page_count());
            if n > 0 {
                let cur = crate::canvas::current_page(app).unwrap_or(0);
                let to = match id {
                    "view.firstPage" => 0,
                    "view.previousPage" => cur.saturating_sub(1),
                    "view.nextPage" => cur.saturating_add(1).min(n - 1),
                    _ => n - 1,
                };
                crate::canvas::go_to_page(app, to);
            }
            Ok(Value::Null)
        }
        "view.goBack" | "view.goForward" => {
            crate::canvas::go_through_history(app, if id == "view.goBack" { -1 } else { 1 });
            Ok(Value::Null)
        }
        "view.nextSpread" | "view.previousSpread" => {
            crate::canvas::step_spread(app, if id == "view.nextSpread" { 1 } else { -1 });
            Ok(Value::Null)
        }
        "view.goToPage" => {
            match p.get("page") {
                Some(Value::Number(n)) => crate::canvas::go_to_page(app, (n.as_u64().unwrap_or(1) as usize).saturating_sub(1)),
                Some(Value::String(s)) => match app.session.resolve_page(s) {
                    Some(abs) => crate::canvas::go_to_page(app, abs),
                    None => return Some(Err(format!("no page \"{s}\""))),
                },
                _ => app.ui.dialog = Some(crate::dialogs::Dialog::new("goToPage", json!({}))),
            }
            Ok(Value::Null)
        }
        "app.characterPanel" | "app.paragraphPanel" | "app.glyphsPanel" | "app.characterStylesPanel" | "app.paragraphStylesPanel" => {
            let panel = match id {
                "app.characterPanel" => "character",
                "app.paragraphPanel" => "paragraph",
                "app.glyphsPanel" => "glyphs",
                "app.characterStylesPanel" => "characterStyles",
                _ => "paragraphStyles",
            };
            run_ui(app, "window.panel", &json!({"panel": panel})).unwrap_or(Ok(Value::Null))
        }
        "app.insertCurrentPageNumber" => app.run("text.insert", json!({"text": "\u{e000}", "raw": true})),
        // `panel_docking::command` takes the dock panels (and the Window › Styles group) first.
        "window.panel" => {
            let panel = p.get("panel").and_then(Value::as_str).unwrap_or("properties");
            if panel != "tabs" {
                return crate::panel_docking::command(app, "window.panel.activate", &json!({"panel": panel}));
            }
            toggle_tabs_panel(app);
            Ok(Value::Null)
        }
        "fit.fillFrameProportionally" | "fit.contentProportionally" | "fit.frameToContent" | "fit.contentToFrame" | "fit.centerContent" => {
            let mode = match id {
                "fit.fillFrameProportionally" => "fillProportionally",
                "fit.contentProportionally" => "fitProportionally",
                "fit.frameToContent" => "fitFrameToContent",
                "fit.contentToFrame" => "fitContentToFrame",
                _ => "centerContent",
            };
            app.run("object.fit", json!({"mode": mode}))
        }
        "app.keyboardShortcuts" => {
            let select = p.get("command").and_then(Value::as_str).map(|id| crate::keys::command_ref(id, p.get("params").unwrap_or(&Value::Null)));
            crate::shortcuts_dialog::open(app, select.as_ref());
            Ok(Value::Null)
        }
        "window.shortcutSets" => {
            let builtins = &crate::shortcuts_dialog::BUILTINS;
            if let Some(name) = p.get("select").and_then(Value::as_str) {
                if app.shortcut_dialog.is_some() {
                    return Some(Err("close the Keyboard Shortcuts dialog first".into()));
                }
                if let Err(e) = app.ui.shortcut_sets.select(name, builtins) {
                    return Some(Err(format!("{name}: {e}")));
                }
                crate::shortcuts_dialog::load_active_set(app);
            }
            let sets = &app.ui.shortcut_sets;
            Ok(
                json!({"active": sets.active_name(builtins), "sets": sets.names(builtins), "builtIn": builtins.iter().map(|(n, _)| *n).collect::<Vec<_>>()}),
            )
        }
        // The open Keyboard Shortcuts dialog edits the keys itself, and its Cancel puts back the
        // ones it started with, which would undo a change made here.
        "window.setShortcut" | "window.resetShortcuts" | "window.importShortcuts" if app.shortcut_dialog.is_some() => {
            Err("close the Keyboard Shortcuts dialog first".into())
        }
        "window.setShortcut" => {
            let Some(id) = p.get("id").and_then(Value::as_str) else { return Some(Err("missing id".into())) };
            let mut cmd = crate::keys::command_ref(id, p.get("params").unwrap_or(&Value::Null));
            // A bare tool id ("pen") names its tool.
            if app.keymap.bindable(&cmd).is_none() && designcraft_tools::tool_info(id).is_some() {
                cmd = crate::keys::tool(id);
            }
            let shortcut = match p.get("shortcut") {
                None | Some(Value::Null) => None,
                Some(Value::String(sc)) => Some(sc.as_str()),
                Some(_) => return Some(Err("shortcut: a string or null".into())),
            };
            let steal = p.get("steal").and_then(Value::as_bool).unwrap_or(false);
            let policy = if steal { craft_keymap::Policy::Steal } else { craft_keymap::Policy::Keep };
            let taken = match crate::keys::set(app, &cmd, shortcut, policy) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            let mut conflicts: Vec<String> = taken.iter().filter(|c| !crate::keys::twins(&cmd, c)).map(|c| c.key()).collect();
            for key in crate::keys::conflicts(&app.keymap, &cmd) {
                if !conflicts.contains(&key) {
                    conflicts.push(key);
                }
            }
            Ok(json!({"shortcut": crate::keys::shortcut(&app.keymap, &cmd), "conflicts": conflicts}))
        }
        "window.resetShortcuts" => {
            app.keymap.reset_all();
            crate::keys::store(app);
            Ok(Value::Null)
        }
        "window.importShortcuts" => {
            let text = match (p.get("text").and_then(Value::as_str), p.get("path").and_then(Value::as_str)) {
                (Some(text), _) => text.to_string(),
                (None, Some(path)) => match crate::shortcut_import::read_file(path) {
                    Ok(text) => text,
                    Err(e) => return Some(Err(e)),
                },
                (None, None) => return Some(Err("give the set's path or text".into())),
            };
            crate::shortcut_import::import(app, &text)
        }
        "view.snapPreferences" => {
            let b = |k: &str| p.get(k).and_then(Value::as_bool);
            let ui = &mut app.ui;
            ui.align_edges = b("alignEdges").unwrap_or(ui.align_edges);
            ui.align_centers = b("alignCenters").unwrap_or(ui.align_centers);
            ui.smart_dimensions = b("smartDimensions").unwrap_or(ui.smart_dimensions);
            ui.smart_spacing = b("smartSpacing").unwrap_or(ui.smart_spacing);
            if let Some(z) = p.get("zone").and_then(Value::as_f64).filter(|z| z.is_finite()) {
                ui.snap_zone = z.max(0.0);
            }
            Ok(json!({
                "alignEdges": ui.align_edges,
                "alignCenters": ui.align_centers,
                "smartDimensions": ui.smart_dimensions,
                "smartSpacing": ui.smart_spacing,
                "zone": ui.snap_zone,
            }))
        }
        _ => return None,
    })
}

/// One slice of the UI commands: `None` if `id` isn't one of them.
fn run_ui_part5(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        "app.formattingAffectsText" => {
            app.ui.formatting_affects_text = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.formatting_affects_text);
            Ok(json!({"on": app.ui.formatting_affects_text}))
        }
        "window.richBlack" => {
            app.ui.rich_black = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.rich_black);
            app.canvas.shown = None;
            Ok(json!({"on": app.ui.rich_black}))
        }
        "window.uiScale" => {
            let v = p.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32;
            app.ui.ui_scale = v.clamp(0.5, 3.0);
            Ok(json!({"scale": app.ui.ui_scale}))
        }
        "window.controlBar" => flag(&mut app.ui.control_bar),
        "view.flattenerPreview" => flag(&mut app.ui.flattener_preview),
        "view.tagMarkers" => {
            app.canvas.shown = None;
            flag(&mut app.ui.tag_markers)
        }
        "edit.dynamicSpelling" => flag(&mut app.ui.dynamic_spelling),
        "window.split" => {
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(!app.split);
            app.split = on;
            if on {
                if app.second_window {
                    app.end_second_ime_viewport();
                }
                app.second_window = false;
            } else {
                app.switch_pane(0);
                app.focus_pane = 0;
            }
            Ok(json!(on))
        }
        "window.newWindow" => {
            // The second view moves into its own window (on the web: a floating window).
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
            if app.second_window && !on {
                app.end_second_ime_viewport();
            }
            app.second_window = on;
            app.split = false;
            if !app.second_window {
                app.switch_pane(0);
                app.focus_pane = 0;
            }
            Ok(json!(app.second_window))
        }
        "window.taskBar" => flag(&mut app.ui.task_bar),
        "window.taskBarPin" => {
            let at = match p.get("at") {
                None | Some(Value::Null) => None,
                Some(v) => match v.as_array().map(|a| a.iter().map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
                    Some([Some(x), Some(y)]) if x.is_finite() && y.is_finite() => Some([x.clamp(-1e6, 1e6) as f32, y.clamp(-1e6, 1e6) as f32]),
                    _ => return Some(Err("window.taskBarPin: `at` is [x, y] in points".into())),
                },
            };
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(at.is_some() || app.ui.task_bar_pin.is_none());
            app.ui.task_bar_pin = match (on, at.or(app.ui.task_bar_pin).or(app.ui.task_bar_at)) {
                (false, _) => None,
                (true, Some(at)) => Some(at),
                (true, None) => return Some(Err("window.taskBarPin: the Contextual Task Bar isn't showing; give `at`".into())),
            };
            Ok(json!(app.ui.task_bar_pin))
        }
        "window.taskBarReset" => {
            app.ui.task_bar_pin = None;
            Ok(Value::Null)
        }
        "help.about" => {
            if let Some(tab) = p.get("tab").and_then(Value::as_str) {
                let Some(i) = crate::about::ABOUT_TABS.iter().position(|t| t.eq_ignore_ascii_case(tab)) else {
                    return Some(Err(format!("help.about: unknown tab `{tab}` (about, contributors, models)")));
                };
                app.ui.about_tab = u8::try_from(i).unwrap_or(0);
            }
            app.ui.about = p.get("open").and_then(Value::as_bool).unwrap_or(true);
            Ok(json!(app.ui.about))
        }
        "help.checkForUpdates" => crate::updates_ui::check(app),
        "help.app" => match p.get("app").and_then(Value::as_str) {
            Some(slug) if !slug.is_empty() && slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                let url = format!("{}/apps/{slug}", designcraft_engine::links::WEBSITE);
                app.ui.pending_urls.push(url.clone());
                Ok(json!({"url": url}))
            }
            _ => Err("help.app: give `app` (e.g. \"photocraft\")".into()),
        },
        h if h.starts_with("help.") && designcraft_engine::links::get(&h[5..]).is_some() => {
            let url = designcraft_engine::links::get(&h[5..]).unwrap_or_default().to_string();
            app.ui.pending_urls.push(url.clone());
            Ok(json!({"url": url}))
        }
        "window.toolsDoubleColumn" => {
            app.ui.tools_double_column = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.tools_double_column);
            Ok(json!(app.ui.tools_double_column))
        }
        "window.newWorkspace" => {
            let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else {
                app.ui.dialog =
                    Some(crate::dialogs::Dialog::new("newWorkspace", json!({"name": "", "panelLocations": true, "menuCustomization": true})));
                return Some(Ok(Value::Null));
            };
            let u = &app.ui;
            let w = crate::SavedWorkspace {
                name: name.to_string(),
                control_bar: u.control_bar,
                task_bar: u.task_bar,
                task_bar_pin: u.task_bar_pin,
                tools_double_column: u.tools_double_column,
                dock_tab: u.dock_tab.clone(),
                dock_expanded: u.dock_expanded,
                open_panel: u.open_panel.clone(),
                floating: u.floating.clone(),
                docking: u.docking.clone(),
                docking_hidden: u.docking_hidden.clone(),
                icon_groups: u.icon_groups.clone(),
                keep_panels: !p.get("panelLocations").and_then(Value::as_bool).unwrap_or(true),
                hidden_menu_items: p.get("menuCustomization").and_then(Value::as_bool).unwrap_or(true).then(|| u.hidden_menu_items.clone()),
            };
            app.ui.custom_workspaces.retain(|x| x.name != name);
            app.ui.custom_workspaces.push(w);
            app.ui.workspace = name.to_string();
            Ok(Value::Null)
        }
        "window.deleteWorkspace" => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("");
            let n = app.ui.custom_workspaces.len();
            app.ui.custom_workspaces.retain(|x| x.name != name);
            if app.ui.custom_workspaces.len() == n {
                return Some(Err(format!("no saved workspace `{name}`")));
            }
            if app.ui.workspace == name {
                app.ui.workspace = "Essentials".into();
            }
            Ok(Value::Null)
        }
        "window.resetWorkspace" => {
            let name = app.ui.workspace.clone();
            return run_ui(app, "window.workspace", &json!({"name": name}));
        }
        "window.workspace" => {
            app.ui.docking_generation = app.ui.docking_generation.wrapping_add(1);
            crate::panel_docking::forget_panels(app);
            let name = p.get("name").and_then(Value::as_str).unwrap_or("Essentials");
            if let Some(w) = app.ui.custom_workspaces.iter().find(|w| w.name == name).cloned() {
                if let Some(hidden) = w.hidden_menu_items {
                    app.ui.hidden_menu_items = hidden;
                }
                if w.keep_panels {
                    app.ui.workspace = w.name;
                    return Some(Ok(Value::Null));
                }
                app.ui.control_bar = w.control_bar;
                app.ui.task_bar = w.task_bar;
                app.ui.task_bar_pin = w.task_bar_pin;
                app.ui.tools_double_column = w.tools_double_column;
                app.ui.dock_tab = w.dock_tab;
                app.ui.dock_expanded = w.dock_expanded;
                app.ui.open_panel = w.open_panel;
                app.ui.floating = w.floating;
                app.ui.docking = w.docking;
                app.ui.docking_hidden = w.docking_hidden;
                app.ui.icon_groups = w.icon_groups;
                app.ui.workspace = w.name;
                return Some(Ok(Value::Null));
            }
            // Workspaces choose which bars and panels are visible.
            if app.ui.docking.is_some() {
                app.ui.floating.clear();
            }
            app.ui.docking = None;
            app.ui.docking_hidden.clear();
            app.ui.control_bar = matches!(name, "Advanced" | "Typography" | "Printing and Proofing" | "Book" | "Essentials Classic");
            // Essentials Classic: the Control panel and an iconic column of grouped panels, no
            // Properties stack.
            let groups = match name {
                "Essentials Classic" => Some(CLASSIC_GROUPS),
                "Typography" => Some(TYPOGRAPHY_GROUPS),
                _ => None,
            };
            app.ui.icon_groups = groups.map(|groups| groups.iter().map(|g| g.iter().map(ToString::to_string).collect()).collect());
            app.ui.dock_expanded = groups.is_none();
            let dock_tab = match name {
                "Interactive for PDF" | "Digital Publishing" => Some("pages"),
                _ => None,
            };
            if let Some(tab) = dock_tab {
                app.ui.dock_tab = tab.into();
                app.ui.dock_expanded = true;
            }
            app.ui.open_panel = workspace_open_panel(name).map(str::to_string);
            app.ui.workspace = name.to_string();
            Ok(Value::Null)
        }
        "window.brightness" => {
            let b = p.get("brightness").and_then(Value::as_str).and_then(crate::theme::Brightness::parse).unwrap_or(crate::theme::Brightness::Dark);
            app.ui.brightness = b;
            app.restyle = true;
            Ok(Value::Null)
        }
        _ => return None,
    })
}

/// Save by handing the serialized document to the host as a download (web).
/// Switch screen modes; entering Presentation remembers the mode to return to.
fn set_screen_mode(app: &mut DesignApp, mode: ScreenMode) {
    let presenting = app.ui.screen_mode == ScreenMode::Presentation;
    if mode == ScreenMode::Presentation && !presenting {
        app.ui.presentation_return = app.ui.screen_mode;
        // Presentation fits the spread to the screen; leaving it puts the user's view back.
        app.ui.presentation_view = app.view_key().zip(app.view().copied());
        app.ui.presentation_page = crate::canvas::page_in_view(app);
        app.ui.presentation_history = app.page_history().cloned();
    }
    if mode != ScreenMode::Presentation
        && presenting
        && let Some((k, v)) = app.ui.presentation_view.take()
        && app.views.contains_key(&k)
    {
        // Go Back sees the presentation as one jump to the last spread presented, and putting the
        // view back as a jump from there: the spreads in between would otherwise have replaced the
        // place the view was at with the presentation's.
        let shown = app.ui.presentation_page.take().zip(crate::canvas::page_in_view(app)).filter(|_| app.view_key() == Some(k));
        let h = app.page_history.entry(k).or_default();
        *h = app.ui.presentation_history.take().unwrap_or_default();
        if let Some(((from, at), (to, offset))) = shown {
            h.leave(from, at);
            h.visit(to, offset);
            h.visit(from, at);
        }
        if let Some(view) = app.views.get_mut(&k) {
            *view = v;
        }
    }
    app.ui.screen_mode = mode;
}
