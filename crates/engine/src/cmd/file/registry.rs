#[cfg(not(target_arch = "wasm32"))]
use super::EngineError;
use super::bytes::{base64_encode, to_bytes, to_bytes_with_preview, with_document_extension};
use super::new::{file_new, file_sample, save_preset};
use super::open::{file_open, file_open_bytes, file_open_recent, has_recent, recent_entry};
use super::place::{file_place, place_drop, place_load, read_source};
use super::save::{FolderAccess, can_revert, file_revert, file_save, recovery_dir};
use super::snippets::{snippet_export, snippet_place};
use super::{CommandSpec, PRESETS, Value, always, bad, cmd, has_doc, json, ok, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.new", "Document…", ["File", "New"], Some("Cmd+N"),
            "{preset?: \"Letter\"|\"A4\"|… or a saved preset (file.savePreset), intent?: print|web|mobile, units?: points|picas|millimeters|…, width?, height?, pages?, startPage?, facingPages?, columns?, gutter?, margins?: number|{top,bottom,inside,outside}, bleed?, slug?: n | [top, bottom, inside, outside], title?, primaryTextFrame?: bool}",
            always, file_new),
        cmd!(noundo "file.newSample", "Sample Document", ["Help"], None, "{} — a multi-page magazine sample", always, file_sample),
        cmd!(query "file.presets", "Document Presets", [], None, "{} → the built-in presets [{name, intent, width, height, units}], then the saved ones (with `saved: true`)", always, |s, _| {
            let mut presets: Vec<Value> = PRESETS.iter().map(|p| serde_json::to_value(p).unwrap_or_default()).collect();
            presets.extend(
                s.prefs
                    .document_presets
                    .iter()
                    .map(|p| json!({"name": p.title, "intent": p.intent, "width": p.width, "height": p.height, "units": p.units, "saved": true})),
            );
            Ok(Value::Array(presets))
        }),
        cmd!(noundo "file.savePreset", "Save Document Preset", [], None,
            "{name, …the settings as file.new} — kept with the preferences; `file.new {preset: name}` uses it. A saved preset of that name is replaced; built-in names are refused",
            always, save_preset),
        cmd!(noundo "file.deletePreset", "Delete Document Preset", [], None, "{name} — a saved document preset", always, |s, p| {
            let name = str_param(p, "name").unwrap_or_default().trim().to_string();
            let before = s.prefs.document_presets.len();
            s.prefs.document_presets.retain(|x| !x.title.eq_ignore_ascii_case(&name));
            if s.prefs.document_presets.len() == before {
                return Err(bad("file.deletePreset", format!("no saved preset `{name}`")));
            }
            ok()
        }),
        cmd!(noundo "file.open", "Open…", ["File"], Some("Cmd+O"),
            "{path} — a DesignCraft or IDML file (recognised by its content, so the extension may be missing), or an InDesign .indd/.indt (recognised by its signature or extension, converted to IDML; its conversion warnings join `warnings`); the fonts in a `Document Fonts` folder beside it load first → {index, documentFonts: faces loaded, warnings: font files skipped, GREP styles whose pattern can't compile, Based on Parent links cleared and, for IDML and InDesign files, package parts missing; warningItems (IDML and InDesign files): per warning, the id of the frame it is about, or null}",
            always, file_open),
        cmd!(noundo "file.openBytes", "Open Bytes", [], None, "{name, base64} — DesignCraft JSON, an IDML package or an InDesign .indd/.indt → {index, warnings}", always, file_open_bytes),
        cmd!(noundo "file.openRecent", "Open Recent File", [], None,
            "{index} — reopen entry `index` of File › Open Recent (as file.open); a file that is gone is an error and stays listed",
            has_recent, file_open_recent),
        cmd!(query "file.recentFiles", "Recent Files", [], None, "{} → [{path, name, exists}] File › Open Recent, most recent first", always, |s, _| {
            Ok(Value::Array(s.prefs.recent_files.iter().map(|p| recent_entry(p)).collect()))
        }),
        cmd!(noundo "file.clearRecent", "Clear Recent Files", ["File", "Open Recent"], None, "{} — empty File › Open Recent", has_recent, |s, _| {
            s.prefs.recent_files.clear();
            ok()
        }),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?} — a `path` without the .designcraft extension gets it → {path: the file written, bytes}", has_doc, file_save),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path} — a path without the .designcraft extension gets it → {path: the file written, bytes}", has_doc, file_save),
        cmd!(noundo "file.saveACopy", "Save a Copy…", ["File"], None, "{path} — writes the document without changing which file it is or its unsaved state; a path without the .designcraft extension gets it → {path: the file written, bytes}", has_doc, |s, p| {
            let path = str_param(p, "path").ok_or_else(|| bad("file.saveACopy", "missing `path`"))?;
            let path = with_document_extension(path);
            let mut copy = (*s.doc()?.doc).clone();
            crate::cmd::datamerge::refresh_relative_paths(&mut copy, std::path::Path::new(&path));
            let bytes = to_bytes_with_preview(&copy, &s.cache)?;
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(&path, &bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }),
        cmd!(noundo "file.revert", "Revert", ["File"], None, "{} — back to the last saved version (not undoable)", can_revert, file_revert),
        cmd!(query "file.recovery.list", "Recovery Data", [], None, "{dir?} → [{uid, path, title, saved}] unsaved documents left by a crash (default: the session's recovery folder)", always, |s, p| {
            let dir = recovery_dir(s, p, FolderAccess::Read)?;
            Ok(Value::Array(crate::recovery::list(&dir).into_iter().map(|(uid, mut m)| { m["uid"] = json!(uid); m }).collect()))
        }),
        cmd!(noundo "file.recovery.save", "Save Recovery Data", [], None, "{dir?} — write every unsaved document to the recovery folder (the app does this on a timer)", always, |s, p| {
            let dir = recovery_dir(s, p, FolderAccess::Write)?;
            Ok(json!({"saved": crate::recovery::save_tracked(s, &dir)?}))
        }),
        cmd!(noundo "file.recovery.open", "Recover Documents", [], None, "{dir?} — reopen documents left in the recovery folder (unsaved)", always, |s, p| {
            // Reopening removes the entries it read.
            let dir = recovery_dir(s, p, FolderAccess::ReadWrite)?;
            Ok(json!({"opened": crate::recovery::open(s, &dir)?}))
        }),
        cmd!(query "file.serialize", "Serialize", [], None, "{} → {base64, bytes} the .designcraft file", has_doc, |s, _| {
            let st = s.doc()?;
            // Preview fill is session state. Serialize the unfilled template.
            let doc = st.preview_stash.as_deref().unwrap_or(st.doc.as_ref());
            let bytes = to_bytes(doc)?;
            Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()}))
        }),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{index?}", has_doc, |s, p| {
            let i = p.get("index").and_then(Value::as_u64).map(|v| v as usize).or(s.active_index()).unwrap_or(0);
            s.close_document(i);
            ok()
        }),
        cmd!(noundo "file.activate", "Activate Document", [], None, "{index}", has_doc, |s, p| {
            s.set_active(p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize);
            ok()
        }),
        cmd!(query "snippet.export", "Export Selection as Snippet", ["File", "Export"], None,
            "{path?} — the selected items (with their stories, styles and images) as a .designcraft snippet; no path: {base64}", crate::cmd::has_selection, snippet_export),
        cmd!(query "place.styles", "Import Options: Styles", [], None,
        "{path | base64, name} → {paragraph: [name], character: [name], conflicts: [name]} — the styles a Word/RTF file brings (for styleMap / styleConflicts)",
        has_doc, |s, p| {
            let (bytes, name, _) = read_source(p)?;
            let imp = designcraft_textimport::import(&name, &bytes).map_err(|e| bad("place.styles", e.to_string()))?;
            let st = &s.doc()?.doc.styles;
            let conflicts: Vec<&str> = imp
                .para_styles
                .iter()
                .filter(|x| st.para(&x.name).is_some())
                .chain(imp.char_styles.iter().filter(|x| st.char_style(&x.name).is_some()))
                .map(|x| x.name.as_str())
                .collect();
            Ok(json!({"paragraph": imp.para_styles.iter().map(|x| &x.name).collect::<Vec<_>>(), "character": imp.char_styles.iter().map(|x| &x.name).collect::<Vec<_>>(), "conflicts": conflicts}))
        }),
        cmd!(
            "snippet.place",
            "Place Snippet",
            [],
            None,
            "{path | base64, spread?, x?, y?} — items keep their positions unless x/y given (top-left)",
            has_doc,
            snippet_place
        ),
        cmd!(
            "place.load",
            "Load Place Cursor",
            [],
            None,
            "{path | base64, name?} — load a graphic into the place cursor (then click/drag with the placeGun tool)",
            has_doc,
            place_load
        ),
        cmd!("place.drop", "Place Loaded Graphic", [], None, "{spread?, x, y, rect?: [x0,y0,x1,y1], frame?: id}", has_doc, place_drop),
        cmd!(
            "file.place",
            "Place…",
            ["File"],
            Some("Cmd+D"),
            "{path?|base64?, name?, frame?: id (place into), spread?, x?, y?, width?: finite number > 0 (null: absent), height? (video/audio), pdfPage?: n (1-based, Image Import Options), pdfCrop?: crop|art|trim|bleed|media|contentVisible|contentAll (the box the graphic shows; `width` is the whole page's), loadCursor? (with no frame to fill and no x/y, an image loads into the place cursor, as File › Place), layoutPage?: n (IDML / .designcraft: that page's objects as a group), page?, rect?: [x0,y0,x1,y1] (text: a new frame there), autoflow? (threads one frame per page column from the first frame on, adding pages until the text fits), removeStyles?, styleMap?: {imported name: document style}, styleConflicts?: useExisting|redefine|autoRename, link? (keep the file linked in the Links panel; default: the linkTextFiles preference)} — places an image into `frame` when given; otherwise any explicit x/y/spread/width creates a new frame; otherwise uses the selected empty or graphic frame, or creates a new frame; text files (.txt, .docx, .rtf, .md) and Excel workbooks (.xlsx, as a table) go into the insertion point, the selected frame or a new frame on `page`/`rect`",
            has_doc,
            file_place
        ),
    ]
}
