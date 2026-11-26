use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::json;

use super::*;
use crate::menus::{self, menu_enabled};

/// What the injected file manager was asked to show: (path, selected).
type Shown = Rc<RefCell<Vec<(PathBuf, bool)>>>;

fn png() -> Vec<u8> {
    designcraft_render::Rendered { width: 4, height: 4, pixels: vec![200u8; 64] }.to_png()
}

/// An app whose file manager records instead of starting anything.
fn app() -> (DesignApp, Shown) {
    let shown: Shown = Rc::default();
    let seen = shown.clone();
    let services = crate::Services {
        reveal: Some(Box::new(move |path: &Path, select: bool| {
            seen.borrow_mut().push((path.to_path_buf(), select));
            Ok(())
        })),
        ..Default::default()
    };
    let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
    app.run("file.new", json!({})).unwrap();
    (app, shown)
}

fn os(s: &[OsString]) -> Vec<String> {
    s.iter().map(|a| a.to_string_lossy().to_string()).collect()
}

#[test]
fn the_mac_reveals_with_open_r_and_opens_a_folder_with_open() {
    let c = reveal_commands(Os::Mac, Path::new("/tmp/a b&c,'.png"), true).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].program, "/usr/bin/open");
    assert_eq!(os(&c[0].args), ["-R", "/tmp/a b&c,'.png"]);
    let c = reveal_commands(Os::Mac, Path::new("/tmp/photos"), false).unwrap();
    assert_eq!(os(&c[0].args), ["/tmp/photos"]);
    // A relative path could read as an option.
    for bad in ["-R", "photo.png", ""] {
        assert!(reveal_commands(Os::Mac, Path::new(bad), true).is_err(), "{bad:?}");
    }
}

#[test]
fn windows_selects_with_explorer_and_the_path_as_its_own_argument() {
    let c = reveal_commands(Os::Windows, Path::new(r"C:\Users\me\My Photos\a&b.png"), true).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].program, "explorer.exe");
    assert_eq!(os(&c[0].args), ["/select,", r"C:\Users\me\My Photos\a&b.png"]);
    let c = reveal_commands(Os::Windows, Path::new(r"\\server\share\folder"), false).unwrap();
    assert_eq!(os(&c[0].args), [r"\\server\share\folder"]);
    for bad in ["/select,C:\\x", "C:x.png", "x.png", "/tmp/x.png", "C:"] {
        assert!(reveal_commands(Os::Windows, Path::new(bad), true).is_err(), "{bad:?}");
    }
}

#[test]
fn linux_asks_the_file_manager_over_d_bus_then_opens_the_folder() {
    let c = reveal_commands(Os::Unix, Path::new("/home/me/a b,c'd.png"), true).unwrap();
    let programs: Vec<_> = c.iter().map(|p| p.program.to_string_lossy().to_string()).collect();
    assert_eq!(programs, ["dbus-send", "gdbus", "xdg-open"]);
    let uri = "file:///home/me/a%20b%2Cc%27d.png";
    assert!(os(&c[0].args).contains(&format!("array:string:{uri}")), "{:?}", c[0].args);
    assert!(os(&c[1].args).contains(&format!("['{uri}']")), "{:?}", c[1].args);
    assert_eq!(os(&c[2].args), ["/home/me"]);
    // A folder: opened, no D-Bus call.
    let c = reveal_commands(Os::Unix, Path::new("/home/me"), false).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(os(&c[0].args), ["/home/me"]);
    assert!(reveal_commands(Os::Unix, Path::new("relative"), true).is_err());
    assert_eq!(file_uri(Path::new("/a/ü~-._")), "file:///a/%C3%BC~-._");
}

#[test]
fn reveal_shows_the_selected_graphic_s_file_or_a_missing_one_s_folder() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png()).unwrap();
    let (mut app, shown) = app();
    let linked = app.run("file.place", json!({"path": file.to_string_lossy(), "x": 10, "y": 10})).unwrap();
    app.run("selection.set", json!({"ids": []})).unwrap();
    let embedded =
        app.run("file.place", json!({"base64": designcraft_engine::cmd::base64_encode(&png()), "name": "in.png", "x": 200, "y": 10})).unwrap();
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap();
    // Nothing selected, or an embedded graphic: disabled, an error, nothing shown.
    app.run("selection.set", json!({"ids": []})).unwrap();
    assert!(!menu_enabled(&app, REVEAL));
    assert!(app.run(REVEAL, json!({})).is_err());
    app.run("selection.set", json!({"ids": [embedded["id"]]})).unwrap();
    assert!(!menu_enabled(&app, REVEAL));
    assert!(app.run(REVEAL, json!({})).unwrap_err().contains("embedded"));
    assert!(shown.borrow().is_empty());
    // The linked graphic: its file, selected.
    app.run("selection.set", json!({"ids": [linked["id"]]})).unwrap();
    assert!(menu_enabled(&app, REVEAL));
    app.run(REVEAL, json!({})).unwrap();
    assert_eq!(shown.borrow().len(), 1);
    assert_eq!((canon(&shown.borrow()[0].0), shown.borrow()[0].1), (canon(&file), true));
    // The Links panel's row, without a selection.
    app.run("selection.set", json!({"ids": []})).unwrap();
    assert!(available_for(&app, &json!({"asset": linked["asset"]})));
    app.run(REVEAL, json!({"asset": linked["asset"]})).unwrap();
    assert_eq!(shown.borrow().len(), 2);
    // Missing: its folder, opened.
    std::fs::remove_file(&file).unwrap();
    app.run(REVEAL, json!({"asset": linked["asset"]})).unwrap();
    assert_eq!((canon(&shown.borrow()[2].0), shown.borrow()[2].1), (canon(dir.path()), false));
    // Hostile params.
    for bad in [json!({"asset": "x"}), json!({"asset": u64::MAX}), json!({"ids": "all"}), json!({"ids": [-1]})] {
        assert!(app.run(REVEAL, bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(shown.borrow().len(), 3);
}

#[test]
fn without_a_file_manager_reveal_is_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png()).unwrap();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("file.place", json!({"path": file.to_string_lossy()})).unwrap();
    app.run("selection.set", json!({"ids": [r["id"]]})).unwrap();
    assert!(!menu_enabled(&app, REVEAL));
    assert!(app.run(REVEAL, json!({})).unwrap_err().contains("isn't available"));
}

#[test]
fn reveal_is_in_the_graphic_context_menu_and_has_no_default_shortcut() {
    assert_eq!(menus::default_shortcut(REVEAL), None);
    assert_eq!(menus::ui_label(REVEAL).map(|(l, _)| l), Some(LABEL));
    #[cfg(target_os = "macos")]
    assert_eq!(LABEL, "Reveal in Finder", "the label InDesign's set files name it by");
    assert!(LABELS.contains(&LABEL));
}
