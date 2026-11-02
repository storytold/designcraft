use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;

use super::*;
use crate::menus::{self, menu_enabled};

/// What the injected launcher was asked to open: (file, application).
type Launched = Rc<RefCell<Vec<(PathBuf, Option<PathBuf>)>>>;

fn png(w: u32) -> Vec<u8> {
    designcraft_render::Rendered { width: w, height: 4, pixels: vec![200u8; (w * 16) as usize] }.to_png()
}

/// An app whose launcher records instead of starting anything, and whose host knows two
/// applications for every file.
fn app() -> (DesignApp, Launched) {
    let launched: Launched = Rc::default();
    let seen = launched.clone();
    let services = crate::Services {
        launch: Some(Box::new(move |file: &Path, with: Option<&Path>| {
            seen.borrow_mut().push((file.to_path_buf(), with.map(Path::to_path_buf)));
            Ok(())
        })),
        applications: Some(Box::new(|_: &Path| {
            vec![
                AppChoice { name: "Painter".into(), path: PathBuf::from("/Applications/Painter.app") },
                AppChoice { name: "Viewer".into(), path: PathBuf::from("/Applications/Viewer.app") },
            ]
        })),
        ..Default::default()
    };
    let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
    app.run("file.new", json!({})).unwrap();
    (app, launched)
}

/// Places `file` (linked) and an embedded graphic; returns their frame ids.
fn place(app: &mut DesignApp, file: &Path) -> (u64, u64) {
    let linked = app.run("file.place", json!({"path": file.to_string_lossy(), "x": 10, "y": 10})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": []})).unwrap();
    let embedded = app
        .run("file.place", json!({"base64": designcraft_engine::cmd::base64_encode(&png(4)), "name": "inside.png", "x": 200, "y": 10}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    (linked, embedded)
}

fn select(app: &mut DesignApp, ids: &[u64]) {
    app.run("selection.set", json!({ "ids": ids })).unwrap();
}

fn same(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
}

#[test]
fn edit_original_opens_the_selected_graphics_file_and_is_greyed_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let (mut app, launched) = app();
    let (linked, embedded) = place(&mut app, &file);

    select(&mut app, &[]);
    assert!(!menu_enabled(&app, EDIT_ORIGINAL), "nothing selected");
    assert!(!menu_enabled(&app, EDIT_WITH));
    assert!(app.run(EDIT_ORIGINAL, json!({})).is_err());
    select(&mut app, &[embedded]);
    assert!(!menu_enabled(&app, EDIT_ORIGINAL), "an embedded graphic has no file");
    assert!(app.run(EDIT_ORIGINAL, json!({})).unwrap_err().contains("embedded"));
    assert!(launched.borrow().is_empty());

    select(&mut app, &[linked]);
    assert!(menu_enabled(&app, EDIT_ORIGINAL));
    let r = app.run(EDIT_ORIGINAL, json!({})).unwrap();
    assert_eq!(r["opened"].as_array().unwrap().len(), 1);
    let calls = launched.borrow().clone();
    assert_eq!(calls.len(), 1);
    assert!(same(&calls[0].0, &file) && calls[0].1.is_none(), "{calls:?}");

    // A missing file can't be opened.
    std::fs::remove_file(&file).unwrap();
    assert!(!menu_enabled(&app, EDIT_ORIGINAL));
    assert!(app.run(EDIT_ORIGINAL, json!({})).unwrap_err().contains("missing"));
    assert_eq!(launched.borrow().len(), 1);
}

#[test]
fn without_a_launcher_the_commands_are_disabled_and_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    // The browser build's host: no launcher.
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let (linked, _) = place(&mut app, &file);
    select(&mut app, &[linked]);
    assert!(!menu_enabled(&app, EDIT_ORIGINAL));
    assert!(!menu_enabled(&app, EDIT_WITH));
    assert_eq!(app.run(EDIT_ORIGINAL, json!({})).unwrap_err(), UNAVAILABLE);
    assert_eq!(app.run(EDIT_WITH, json!({"app": "/bin/sh"})).unwrap_err(), UNAVAILABLE);
    assert!(edit_with_items(&mut app).is_empty());
}

#[test]
fn edit_with_lists_the_hosts_applications_and_opens_the_file_in_the_one_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let painter = dir.path().join("Painter.app");
    std::fs::create_dir(&painter).unwrap();
    let (mut app, launched) = app();
    let (linked, _) = place(&mut app, &file);
    select(&mut app, &[linked]);

    // The submenu's applications, then a separator before Other….
    let items = edit_with_items(&mut app);
    let labels: Vec<String> = items.iter().filter_map(|i| if let Item::Cmd { label, .. } = i { Some(label.clone()) } else { None }).collect();
    assert_eq!(labels, ["Painter", "Viewer"]);
    assert!(matches!(items.last(), Some(Item::Sep)));
    let listed = app.run(EDIT_WITH, json!({"list": true})).unwrap();
    assert_eq!(listed[1], json!({"name": "Viewer", "path": "/Applications/Viewer.app"}));

    // A chosen application.
    app.run(EDIT_WITH, json!({"app": painter.to_string_lossy()})).unwrap();
    assert_eq!(launched.borrow().last().unwrap().1.as_deref(), Some(painter.as_path()));
    // One that isn't there, and hostile values.
    for bad in [json!({"app": "/no/such/Thing.app"}), json!({"app": ""}), json!({"app": 7}), json!({"asset": "x"})] {
        assert!(app.run(EDIT_WITH, bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(launched.borrow().len(), 1);

    // Other…: the application comes from an open dialog.
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(painter.to_string_lossy().to_string())));
    app.run(EDIT_WITH, json!({"other": true})).unwrap();
    app.settle_dialogs();
    assert_eq!(launched.borrow().len(), 2);
    assert!(same(&launched.borrow()[1].0, &file));
}

/// After the other application saves, coming back to the window updates the link.
#[test]
fn an_edited_file_updates_its_link_when_the_window_is_back_in_front() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let (mut app, _) = app();
    let (linked, _) = place(&mut app, &file);
    select(&mut app, &[linked]);
    app.run(EDIT_ORIGINAL, json!({})).unwrap();

    // Nothing changed yet: nothing is updated.
    assert_eq!(refresh_edited(&mut app), 0);
    // The other application saves a wider image: the Links panel shows it modified, and the
    // focus check updates it.
    std::fs::write(&file, png(9)).unwrap();
    let status = |app: &mut DesignApp| app.session.execute("links.list", &json!({})).unwrap()[0]["status"].clone();
    assert_eq!(status(&mut app), "modified");
    assert_eq!(refresh_edited(&mut app), 1);
    assert_eq!(status(&mut app), "ok");
    let pixels = app.session.execute("links.list", &json!({})).unwrap()[0]["pixels"].clone();
    assert_eq!(pixels, json!([9, 4]));
    // Undo puts the old copy back, like Update Link.
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(status(&mut app), "modified");

    // The focus change drives it: losing focus does nothing, regaining it checks again.
    std::fs::write(&file, png(12)).unwrap();
    focus(&mut app, Some(true));
    focus(&mut app, Some(false));
    assert_eq!(status(&mut app), "modified");
    focus(&mut app, Some(true));
    assert_eq!(status(&mut app), "ok");
}

#[test]
fn the_menus_hold_edit_original_and_edit_with_without_a_default_shortcut() {
    let tree = menus::menu_tree();
    let edit = &tree.iter().find(|(m, _)| *m == "Edit").unwrap().1;
    assert!(edit.iter().any(|i| matches!(i, Item::Cmd { id, label, .. } if id == EDIT_ORIGINAL && label == "Edit Original")));
    let with = edit.iter().find_map(|i| if let Item::Sub(name, children) = i { (name == "Edit With").then_some(children) } else { None }).unwrap();
    assert!(matches!(&with[..], [Item::Cmd { id, label, .. }] if id == EDIT_WITH && label == "Other…"));
    // InDesign has no shortcut for it; Cmd+E stays Export.
    assert_eq!(menus::default_shortcut(EDIT_ORIGINAL), None);
    assert_eq!(menus::default_shortcut(EDIT_WITH), None);
    assert_eq!(menus::default_shortcut("app.exportPng"), Some("Cmd+E"));
}

#[cfg(target_os = "macos")]
#[test]
fn the_mac_opener_is_open_with_the_application_when_one_is_chosen() {
    let c = launch_command(Path::new("/tmp/a b&c.png"), None).unwrap();
    assert_eq!(c.get_program(), "/usr/bin/open");
    assert_eq!(c.get_args().collect::<Vec<_>>(), ["/tmp/a b&c.png"]);
    let c = launch_command(Path::new("/tmp/x.png"), Some(Path::new("/Applications/Preview.app"))).unwrap();
    assert_eq!(c.get_args().collect::<Vec<_>>(), ["-a", "/Applications/Preview.app", "/tmp/x.png"]);
    // A relative path could read as an option.
    assert!(launch_command(Path::new("-a"), None).is_err());
}
