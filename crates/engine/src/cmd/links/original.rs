//! Edit › Edit Original and Edit With: the files behind the selected placed graphics.
//!
//! Opening a file in another application is the host's job (the UI's `links.editOriginal` and
//! `links.editWith`); the engine only says which files those are, so the control channel and MCP
//! can ask too. A graphic qualifies when it is linked, its link is followed (see
//! [`crate::io::document_path`]) and the file is on disk: an embedded graphic has no file to edit,
//! and a missing one can't be opened.

use std::path::{Path, PathBuf};

use designcraft_doc::{AssetId, Content, Document};
use serde_json::{Value, json};

use super::{CommandSpec, asset_param, bad, cmd, has_doc};
use crate::{Result, Session};

/// The command that lists the files; the UI commands that open them read it.
pub const COMMAND: &str = "links.originalFiles";

/// The file of a placed graphic that Edit Original would open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Original {
    pub asset: AssetId,
    /// The graphic's name (its file name when it was placed).
    pub name: String,
    /// Where the stored link resolves, absolute.
    pub path: PathBuf,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        query "links.originalFiles",
        "Original Files",
        [],
        None,
        "{asset? (a Links panel row) | ids?: [frame ids]} (default: the selected graphic frames) → {files: [{asset, name, path}]} — the linked files Edit Original opens; error when none is linked to a file on disk (embedded, missing, nothing placed selected) of a type DesignCraft places (graphic, video, sound: never a program, script or shortcut)",
        has_doc,
        |s, p| {
            let files = originals(s, p)?;
            let files: Vec<Value> =
                files.iter().map(|o| json!({"asset": o.asset.0, "name": o.name, "path": o.path.to_string_lossy()})).collect();
            Ok(json!({ "files": files }))
        }
    )]
}

/// The graphics a call names: `asset`, else the graphic frames among `ids`, else among the
/// selection. Each asset once, in order.
fn named_assets(d: &Document, selection: &[designcraft_doc::ItemId], p: &Value) -> Result<Vec<AssetId>> {
    if p.get("asset").is_some() {
        let aid = asset_param(p, COMMAND)?;
        if !d.assets.contains_key(&aid) {
            return Err(bad(COMMAND, "no such graphic"));
        }
        return Ok(vec![aid]);
    }
    let ids: Vec<designcraft_doc::ItemId> = match p.get("ids") {
        Some(v) => {
            let list = v.as_array().ok_or_else(|| bad(COMMAND, "ids must be a list of frame ids"))?;
            // A hostile list is cut short: a selection is never this long.
            list.iter()
                .take(10_000)
                .map(|i| i.as_u64().map(designcraft_doc::ItemId).ok_or_else(|| bad(COMMAND, "ids must be numbers")))
                .collect::<Result<_>>()?
        }
        None => selection.to_vec(),
    };
    let mut out = Vec::new();
    for id in ids {
        if let Some(Content::Graphic(g)) = d.item(id).map(|it| &it.content)
            && !out.contains(&g.asset)
        {
            out.push(g.asset);
        }
    }
    Ok(out)
}

/// The linked files of the graphics a call names (see [`named_assets`]) that are on disk. An
/// error says why there are none: nothing placed is named, or each is embedded or missing.
pub fn originals(s: &Session, p: &Value) -> Result<Vec<Original>> {
    let st = s.doc()?;
    let assets = named_assets(&st.doc, &st.selection.items, p)?;
    if assets.is_empty() {
        return Err(bad(COMMAND, "select a placed graphic"));
    }
    let document = st.path.as_deref().map(Path::new);
    let mut out = Vec::new();
    let mut why = None;
    for aid in assets {
        let Some(a) = st.doc.assets.get(&aid) else { continue };
        match on_disk(a, document).and_then(|path| openable(a, path)) {
            Ok(path) => out.push(Original { asset: aid, name: a.name.clone(), path }),
            Err(e) => {
                why.get_or_insert(e);
            }
        }
    }
    match (out.is_empty(), why) {
        (true, Some(e)) => Err(bad(COMMAND, e)),
        _ => Ok(out),
    }
}

/// Whether Edit Original can run on the selection: some selected graphic has its file on disk.
/// Cheap enough for menu greying: a look at each selected graphic's file, nothing read.
pub fn can_edit_original(s: &Session) -> bool {
    s.active().is_some_and(|st| !st.selection.items.is_empty()) && originals(s, &Value::Null).is_ok_and(|o| !o.is_empty())
}

/// What Links › Reveal in Finder (Explorer, the file manager) shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reveal {
    /// The linked file, selected in its folder.
    File(PathBuf),
    /// The folder a missing file was in, opened (the file can't be selected).
    Folder(PathBuf),
}

/// What revealing the graphic a call names shows (see [`named_assets`]: `asset`, else the
/// graphic frames among `ids`, else the selection's): the first one linked to a file on disk, or
/// else the folder of the first missing one when that folder is there. An error says why there is
/// none (embedded, a link that isn't followed, no folder, nothing placed named).
pub fn reveal_target(s: &Session, p: &Value) -> Result<Reveal> {
    let st = s.doc()?;
    let assets = named_assets(&st.doc, &st.selection.items, p)?;
    if assets.is_empty() {
        return Err(bad(REVEAL, "select a placed graphic"));
    }
    let document = st.path.as_deref().map(Path::new);
    let mut folder = None;
    let mut why = None;
    for aid in assets {
        let Some(a) = st.doc.assets.get(&aid) else { continue };
        match on_disk(a, document) {
            Ok(path) => return Ok(Reveal::File(path)),
            Err(e) => {
                why.get_or_insert(e);
            }
        }
        if folder.is_none() {
            folder = missing_folder(a, document);
        }
    }
    match (folder, why) {
        (Some(dir), _) => Ok(Reveal::Folder(dir)),
        (None, Some(e)) => Err(bad(REVEAL, e)),
        (None, None) => Err(bad(REVEAL, "select a placed graphic")),
    }
}

/// The name [`reveal_target`]'s errors carry (the UI command that reveals).
const REVEAL: &str = "links.reveal";

/// The folder a missing linked file was in, when the link is followed and the folder is there.
fn missing_folder(a: &designcraft_doc::Asset, document: Option<&Path>) -> Option<PathBuf> {
    let link = a.link.as_deref()?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = crate::io::document_path(link, document).ok()?;
        let dir = path.parent()?;
        crate::io::is_dir(dir).then(|| dir.to_path_buf())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (link, document);
        None
    }
}

/// `path` when it is a kind of file DesignCraft places (by its extension): the host opens it with
/// the system's opener, which runs a program, a script or a shortcut, and the link comes from the
/// document. Revealing it in its folder runs nothing, so [`reveal_target`] doesn't ask.
fn openable(a: &designcraft_doc::Asset, path: PathBuf) -> std::result::Result<PathBuf, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if designcraft_doc::placeable_graphic_name(&name) {
        Ok(path)
    } else {
        Err(format!("{}: {name} isn't a graphic, video or sound file, so it isn't opened", a.name))
    }
}

/// Where an asset's linked file is, when it is a file on disk.
fn on_disk(a: &designcraft_doc::Asset, document: Option<&Path>) -> std::result::Result<PathBuf, String> {
    let Some(link) = a.link.as_deref() else { return Err(format!("{} is embedded: it has no file to edit", a.name)) };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = crate::io::document_path(link, document).map_err(|r| format!("{}: {r}", a.name))?;
        if !crate::io::is_file(&path) {
            return Err(format!("{}: the linked file is missing", a.name));
        }
        Ok(path)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (link, document);
        Err(format!("{}: files can't be opened in other applications on the web", a.name))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn png() -> Vec<u8> {
        designcraft_render::Rendered { width: 4, height: 4, pixels: vec![200u8; 64] }.to_png()
    }

    /// A document with a linked, an embedded and a (then) missing graphic, each in its own frame.
    fn three_graphics(case: &str) -> (Session, tempfile::TempDir, [u64; 3], PathBuf) {
        let dir = tempfile::Builder::new().prefix(&format!("dc-edit-original-{case}")).tempdir().unwrap();
        let linked = dir.path().join("linked.png");
        let gone = dir.path().join("gone.png");
        std::fs::write(&linked, png()).unwrap();
        std::fs::write(&gone, png()).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let mut place = |p: Value| {
            s.doc_mut().unwrap().selection = Default::default();
            s.execute("file.place", &p).unwrap()["id"].as_u64().unwrap()
        };
        let a = place(json!({"path": linked.to_string_lossy(), "x": 10, "y": 10}));
        let b = place(json!({"base64": crate::cmd::file::base64_encode(&png()), "name": "inside.png", "x": 100, "y": 10}));
        let c = place(json!({"path": gone.to_string_lossy(), "x": 200, "y": 10}));
        std::fs::remove_file(&gone).unwrap();
        (s, dir, [a, b, c], linked)
    }

    fn select(s: &mut Session, ids: &[u64]) {
        s.execute("selection.set", &json!({ "ids": ids })).unwrap();
    }

    #[test]
    fn a_linked_graphic_on_disk_is_found_and_enables_edit_original() {
        let (mut s, _dir, [linked, ..], path) = three_graphics("linked");
        select(&mut s, &[linked]);
        assert!(can_edit_original(&s));
        let r = s.execute(COMMAND, &json!({})).unwrap();
        let files = r["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["name"], "linked.png");
        assert_eq!(std::fs::canonicalize(files[0]["path"].as_str().unwrap()).unwrap(), std::fs::canonicalize(&path).unwrap());
        // The Links panel names its row's asset instead of the selection.
        let aid = files[0]["asset"].clone();
        select(&mut s, &[]);
        assert!(!can_edit_original(&s));
        assert_eq!(s.execute(COMMAND, &json!({"asset": aid})).unwrap()["files"][0]["name"], "linked.png");
    }

    #[test]
    fn embedded_missing_and_nothing_selected_disable_it_with_a_reason() {
        let (mut s, _dir, [linked, embedded, missing], _) = three_graphics("disabled");
        // Nothing selected.
        select(&mut s, &[]);
        assert!(!can_edit_original(&s));
        assert!(s.execute(COMMAND, &json!({})).unwrap_err().to_string().contains("select a placed graphic"));
        // An embedded graphic.
        select(&mut s, &[embedded]);
        assert!(!can_edit_original(&s));
        assert!(s.execute(COMMAND, &json!({})).unwrap_err().to_string().contains("embedded"));
        // A missing file.
        select(&mut s, &[missing]);
        assert!(!can_edit_original(&s));
        assert!(s.execute(COMMAND, &json!({})).unwrap_err().to_string().contains("missing"));
        // A text frame is no graphic.
        let text = s.execute("frame.create", &json!({"rect": [0, 300, 100, 400], "content": "text"})).unwrap()["id"].as_u64().unwrap();
        select(&mut s, &[text]);
        assert!(!can_edit_original(&s));
        // With all three selected, only the one on disk opens.
        select(&mut s, &[embedded, linked, missing]);
        assert!(can_edit_original(&s));
        let r = s.execute(COMMAND, &json!({})).unwrap();
        assert_eq!(r["files"].as_array().unwrap().len(), 1);
        assert_eq!(r["files"][0]["name"], "linked.png");
    }

    /// The opener runs programs and scripts, so only a file named as a graphic, video or sound
    /// is offered, whatever its contents; revealing it is fine.
    #[test]
    fn only_graphic_video_and_sound_files_are_opened() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for name in ["tool.command", "setup.exe", "photo.png.", "photo.png ", "noextension"] {
            let file = dir.path().join(name);
            std::fs::write(&file, png()).unwrap();
            s.doc_mut().unwrap().selection = Default::default();
            let id = s.execute("file.place", &json!({"path": file.to_string_lossy()})).unwrap()["id"].as_u64().unwrap();
            select(&mut s, &[id]);
            assert!(!can_edit_original(&s), "{name}");
            assert!(s.execute(COMMAND, &json!({})).unwrap_err().to_string().contains("isn't a graphic"), "{name}");
            assert!(matches!(reveal_target(&s, &Value::Null), Ok(Reveal::File(_))), "{name}");
        }
        for name in ["a.png", "B.JPG", "c.tiff", "d.psd", "e.ai", "f.eps", "g.pdf", "h.svg", "clip.mp4", "song.MP3"] {
            assert!(designcraft_doc::placeable_graphic_name(name), "{name}");
        }
        for name in ["a.exe", "a.bat", "a.lnk", "a.sh", "a.command", "a.app", "a.png.exe", "a.exe.", "a.png.", "a"] {
            assert!(!designcraft_doc::placeable_graphic_name(name), "{name}");
        }
    }

    #[test]
    fn reveal_shows_the_file_or_a_missing_file_s_folder() {
        let (mut s, dir, [linked, embedded, missing], path) = three_graphics("reveal");
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap();
        select(&mut s, &[linked]);
        match reveal_target(&s, &Value::Null).unwrap() {
            Reveal::File(p) => assert_eq!(canon(&p), canon(&path)),
            other => panic!("{other:?}"),
        }
        // A missing file: its folder, while the folder is there.
        select(&mut s, &[missing]);
        match reveal_target(&s, &Value::Null).unwrap() {
            Reveal::Folder(p) => assert_eq!(canon(&p), canon(dir.path())),
            other => panic!("{other:?}"),
        }
        // The file on disk wins over the missing one, whatever the order.
        select(&mut s, &[missing, linked]);
        assert!(matches!(reveal_target(&s, &Value::Null).unwrap(), Reveal::File(_)));
        // Embedded: nothing to reveal; nothing selected; hostile params.
        select(&mut s, &[embedded]);
        assert!(reveal_target(&s, &Value::Null).unwrap_err().to_string().contains("embedded"));
        select(&mut s, &[]);
        assert!(reveal_target(&s, &Value::Null).is_err());
        for p in [json!({"asset": "x"}), json!({"asset": u64::MAX}), json!({"ids": "all"}), json!({"ids": [u64::MAX]})] {
            assert!(reveal_target(&s, &p).is_err(), "{p}");
        }
        // The folder goes too: nothing to reveal.
        select(&mut s, &[missing]);
        drop(dir);
        assert!(reveal_target(&s, &Value::Null).unwrap_err().to_string().contains("missing"));
        assert!(reveal_target(&Session::new(), &Value::Null).is_err());
    }

    #[test]
    fn hostile_params_are_errors() {
        let (mut s, _dir, [linked, ..], _) = three_graphics("hostile");
        for p in [
            json!({"asset": "x"}),
            json!({"asset": -1}),
            json!({"asset": u64::MAX}),
            json!({"ids": "all"}),
            json!({"ids": [1.5]}),
            json!({"ids": [u64::MAX]}),
            json!({"ids": []}),
        ] {
            assert!(s.execute(COMMAND, &p).is_err(), "{p}");
        }
        // Named frames win over the selection.
        select(&mut s, &[]);
        assert_eq!(s.execute(COMMAND, &json!({ "ids": [linked] })).unwrap()["files"].as_array().unwrap().len(), 1);
        // No document.
        let mut empty = Session::new();
        assert!(empty.execute(COMMAND, &json!({})).is_err());
        assert!(!can_edit_original(&empty));
    }
}
