//! Installed system fonts: found by family name by every lookup, whatever ran before it (#22).

use std::path::{Path, PathBuf};

use super::*;

const FAMILY: &str = "Sysfont Sans3";

#[cfg(not(any(target_os = "macos", windows)))]
mod user_font_dirs {
    use super::*;

    #[test]
    fn user_font_dirs_when_xdg_is_absolute() {
        let home = Path::new("/home/font-user");
        let xdg = Path::new("/custom/data");
        let dirs = unix_font_dirs(Some(home), Some(xdg));
        assert_eq!(dirs, ["/usr/share/fonts", "/usr/local/share/fonts", "/home/font-user/.fonts", "/custom/data/fonts"].map(PathBuf::from));
    }

    #[test]
    fn user_font_dirs_when_xdg_is_unset() {
        let home = Path::new("/home/font-user");
        let dirs = unix_font_dirs(Some(home), None);
        assert_eq!(
            dirs,
            ["/usr/share/fonts", "/usr/local/share/fonts", "/home/font-user/.fonts", "/home/font-user/.local/share/fonts"].map(PathBuf::from)
        );
    }

    #[test]
    fn user_font_dirs_when_xdg_is_relative() {
        let home = Path::new("/home/font-user");
        let xdg = Path::new("relative/data");
        let dirs = unix_font_dirs(Some(home), Some(xdg));
        assert_eq!(
            dirs,
            ["/usr/share/fonts", "/usr/local/share/fonts", "/home/font-user/.fonts", "/home/font-user/.local/share/fonts"].map(PathBuf::from)
        );
    }

    #[test]
    fn user_font_dirs_when_xdg_is_empty() {
        let home = Path::new("/home/font-user");
        let xdg = Path::new("");
        let dirs = unix_font_dirs(Some(home), Some(xdg));
        assert_eq!(
            dirs,
            ["/usr/share/fonts", "/usr/local/share/fonts", "/home/font-user/.fonts", "/home/font-user/.local/share/fonts"].map(PathBuf::from)
        );
    }

    #[test]
    fn user_font_dirs_when_home_is_unset_and_xdg_is_absolute() {
        let xdg = Path::new("/custom/data");
        let dirs = unix_font_dirs(None, Some(xdg));
        assert_eq!(dirs, ["/usr/share/fonts", "/usr/local/share/fonts", "/custom/data/fonts"].map(PathBuf::from));
    }

    #[test]
    fn user_font_dirs_when_home_and_xdg_are_unset() {
        let dirs = unix_font_dirs(None, None);
        assert_eq!(dirs, ["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
    }
}

/// A bundled Source Sans 3 file renamed [`FAMILY`] (as long as the original name).
fn renamed(file: &str) -> Vec<u8> {
    let mut data = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts").join(file)).unwrap();
    let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
    for (from, to) in [(utf16("Source Sans 3"), utf16(FAMILY)), (b"Source Sans 3".to_vec(), FAMILY.as_bytes().to_vec())] {
        let mut i = 0;
        while let Some(at) = data[i..].windows(from.len()).position(|w| w == from) {
            data[i + at..i + at + to.len()].copy_from_slice(&to);
            i += at + to.len();
        }
    }
    data
}

/// The fonts in `faces` as one collection (TTC) file.
fn collection(faces: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"ttcf".to_vec();
    out.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    out.extend_from_slice(&(faces.len() as u32).to_be_bytes());
    let mut base = 12 + 4 * faces.len();
    for f in faces {
        out.extend_from_slice(&(base as u32).to_be_bytes());
        base += f.len();
    }
    for f in faces {
        // Table offsets count from the start of the collection.
        let start = out.len() as u32;
        let mut f = f.clone();
        let tables = u16::from_be_bytes([f[4], f[5]]) as usize;
        for r in 0..tables {
            let at = 12 + r * 16 + 8;
            let off = u32::from_be_bytes(f[at..at + 4].try_into().unwrap()) + start;
            f[at..at + 4].copy_from_slice(&off.to_be_bytes());
        }
        out.extend_from_slice(&f);
    }
    out
}

/// A font folder (with a subfolder, a damaged font and a file that isn't a font) holding
/// [`FAMILY`] Regular and Bold.
fn font_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-sysfonts-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Sub")).unwrap();
    std::fs::write(dir.join("Sysfont-Regular.ttf"), renamed("SourceSans3-Regular.ttf")).unwrap();
    std::fs::write(dir.join("Sub/Sysfont-Bold.TTF"), renamed("SourceSans3-Bold.ttf")).unwrap();
    std::fs::write(dir.join("damaged.ttf"), b"ttcf\0\x01\0\0\xff\xff\xff\xff").unwrap();
    std::fs::write(dir.join("readme.txt"), FAMILY).unwrap();
    dir
}

fn has(list: &[String], family: &str) -> bool {
    list.iter().any(|f| f == family)
}

#[test]
fn every_lookup_by_name_finds_installed_fonts_in_a_fresh_database() {
    let dir = font_dir("fresh");
    // Each lookup is the first thing a new database is asked. A folder listed twice is read once.
    let db = || FontDb::with_font_dirs(vec![dir.clone(), dir.join("Sub/..")]);
    let f = db().face("sysfont sans3", "Bold");
    assert_eq!((f.family.as_str(), f.style.as_str()), (FAMILY, "Bold"));
    assert!(db().has_family(FAMILY));
    assert!(has(&db().families(), FAMILY));
    assert_eq!(db().styles(FAMILY), ["Regular", "Bold"]);
    assert_eq!(db().load_system_fonts(), 2);
    // Missing fonts stay missing.
    let db = db();
    assert!(!db.has_family("No Such Font"));
    assert_eq!(db.face("No Such Font", "Regular").family, FALLBACK_FAMILY);
}

#[test]
fn the_scan_reads_collections_and_loads_fonts_only_when_used() {
    let dir = font_dir("collection");
    std::fs::remove_file(dir.join("Sysfont-Regular.ttf")).unwrap();
    std::fs::remove_file(dir.join("Sub/Sysfont-Bold.TTF")).unwrap();
    std::fs::write(dir.join("Sysfont.ttc"), collection(&[renamed("SourceSans3-Regular.ttf"), renamed("SourceSans3-Bold.ttf")])).unwrap();
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    assert_eq!(db.styles(FAMILY), ["Regular", "Bold"]);
    // Cataloged, not loaded.
    assert!(!db.is_loaded(FAMILY));
    let bold = db.face(FAMILY, "Bold");
    assert_eq!((bold.style.as_str(), bold.index()), ("Bold", 1));
    assert_eq!(db.face(FAMILY, "Regular").index(), 0, "the collection's faces load together");
}

#[test]
fn rescanning_finds_fonts_installed_since() {
    let dir = font_dir("rescan");
    let later = dir.join("Later");
    let db = FontDb::with_font_dirs(vec![later.clone()]);
    assert!(!db.has_family(FAMILY));
    std::fs::create_dir_all(&later).unwrap();
    std::fs::copy(dir.join("Sysfont-Regular.ttf"), later.join("Sysfont-Regular.ttf")).unwrap();
    assert!(!db.has_family(FAMILY), "the folders are scanned once, not on every miss");
    assert_eq!(db.load_system_fonts(), 1);
    assert!(db.has_family(FAMILY) && has(&db.families(), FAMILY));
}

#[test]
fn a_background_scan_serves_the_first_lookup() {
    let dir = font_dir("background");
    let db: &'static FontDb = Box::leak(Box::new(FontDb::with_font_dirs(vec![dir])));
    db.scan_in_background();
    // Waits for the scan when it is still running.
    assert!(has(&db.families(), FAMILY));
    assert_eq!(db.load_system_fonts(), 2, "a rescan catalogs the two faces again");
}

#[cfg(unix)]
#[test]
fn a_link_back_to_a_parent_folder_ends_the_scan() {
    let dir = font_dir("loop");
    std::os::unix::fs::symlink(&dir, dir.join("Sub/loop")).unwrap();
    let db = FontDb::with_font_dirs(vec![dir]);
    assert_eq!(db.load_system_fonts(), 2);
}

/// The folder a font service keeps its fonts in, outside the font folders, and the font folder
/// of [`fonts_the_platform_lists_outside_the_font_folders_are_cataloged_once`].
fn service_dirs() -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir();
    let id = std::process::id();
    (tmp.join(format!("dc-sysfonts-{id}-service")), tmp.join(format!("dc-sysfonts-{id}-platform")))
}

/// The platform's list: a font service's file without an extension (twice, as for each face of a
/// collection), a font folder's file by another path, a file gone since and a folder.
fn service_files() -> Vec<PathBuf> {
    let (service, folder) = service_dirs();
    vec![service.join(".r/.12345"), folder.join("Sub/../Sysfont-Regular.ttf"), service.join(".r/.12345"), service.join("gone.otf"), service.clone()]
}

#[test]
fn fonts_the_platform_lists_outside_the_font_folders_are_cataloged_once() {
    let (service, folder) = service_dirs();
    let dir = font_dir("platform");
    assert_eq!(dir, folder);
    let _ = std::fs::remove_dir_all(&service);
    std::fs::create_dir_all(service.join(".r")).unwrap();
    std::fs::write(service.join(".r/.12345"), renamed("SourceSans3-It.ttf")).unwrap();
    let db = FontDb { font_files: service_files, ..FontDb::with_font_dirs(vec![dir]) };
    assert_eq!(db.styles(FAMILY), ["Regular", "Bold", "Italic"]);
    assert_eq!(db.load_system_fonts(), 3, "the font folder's Regular listed again isn't cataloged twice");
    let italic = db.face(FAMILY, "Italic");
    assert_eq!(italic.style, "Italic");
    assert_eq!(italic.source.path(), Some(service.join(".r/.12345").as_path()));
    let _ = std::fs::remove_dir_all(&service);
}

#[test]
fn a_familys_kind_is_read_without_keeping_it_loaded() {
    let dir = font_dir("kind");
    let db = FontDb::with_font_dirs(vec![dir]);
    db.load_system_fonts();
    let fonts = db.scoped(0);
    let (kind, added) = fonts.family_kind(FAMILY).expect("an installed family");
    assert_eq!(kind.class, crate::kind::FontClass::SansSerif);
    assert!(added.is_some(), "the file's date");
    assert!(!db.is_loaded(FAMILY), "the file was read and let go");
    // A loaded family is asked directly, with the same answer.
    let _ = fonts.face(FAMILY, "Regular");
    assert!(db.is_loaded(FAMILY));
    assert_eq!(fonts.family_kind(FAMILY).map(|(k, _)| k.class), Some(crate::kind::FontClass::SansSerif));
    assert!(fonts.family_kind("No Such Family").is_none());
}

#[test]
fn the_scan_reads_names_without_loading_the_font() {
    let dir = font_dir("names");
    let names: Vec<(String, String)> = file_face_names(&dir.join("Sysfont-Regular.ttf")).into_iter().map(|f| (f.family, f.style)).collect();
    assert_eq!(names, [(FAMILY.to_string(), "Regular".to_string())]);
    assert!(file_face_names(&dir.join("damaged.ttf")).is_empty());
    assert!(file_face_names(&dir.join("readme.txt")).is_empty());
    assert!(file_face_names(&dir.join("missing.ttf")).is_empty());
}

#[test]
fn faces_without_outlines_are_not_cataloged() {
    // A face whose glyphs live in a table nothing draws from (`hvgl`, say) lists no face; in a
    // collection, its drawable sibling is still listed.
    let regular = renamed("SourceSans3-Regular.ttf");
    let mut hvgl = renamed("SourceSans3-It.ttf");
    let tables = u16::from_be_bytes([hvgl[4], hvgl[5]]) as usize;
    let glyf = (0..tables).map(|r| 12 + r * 16).find(|&at| &hvgl[at..at + 4] == b"glyf").unwrap();
    hvgl[glyf..glyf + 4].copy_from_slice(b"hvgl");
    let dir = std::env::temp_dir().join(format!("dc-sysfonts-outlines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hvgl.ttf"), &hvgl).unwrap();
    std::fs::write(dir.join("mixed.ttc"), collection(&[hvgl, regular])).unwrap();
    assert!(file_face_names(&dir.join("hvgl.ttf")).is_empty());
    let names: Vec<String> = file_face_names(&dir.join("mixed.ttc")).into_iter().map(|f| f.style).collect();
    assert_eq!(names, ["Regular"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn font_menus_group_installed_and_loaded_fonts_without_loading_them() {
    use crate::testing::{font_with, with_code_pages, with_names};
    use crate::{FamilyInfo, FontGroup};
    let dir = std::env::temp_dir().join(format!("dc-sysfonts-groups-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let names = |family: &str, lang: u16, native: &str| {
        let font = font_with(family, &['a']).unwrap();
        with_names(&font, &[(3, 1, 0x409, 1, family), (3, 1, 0x409, 2, "Regular"), (3, 1, lang, 1, native)]).unwrap()
    };
    std::fs::write(dir.join("jp.ttf"), names("Groups Mincho", 0x0411, "グループ明朝")).unwrap();
    // Declares nothing (no code pages): its kana make it Japanese, read from the cmap.
    std::fs::write(dir.join("kana.otf"), with_code_pages(font_with("Groups Kana", &['a', 'あ']).unwrap(), 0, 0).unwrap()).unwrap();
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    db.set_system_fallback(false);
    db.add_font(with_code_pages(font_with("Groups Ming", &['a']).unwrap(), 1 << 20, 0).unwrap());
    let infos = db.scoped(0).family_infos();
    let find = |f: &str| infos.iter().find(|i| i.family == f).cloned().unwrap_or_else(|| panic!("{f} in {infos:?}"));
    assert_eq!(find("Groups Mincho"), FamilyInfo { family: "Groups Mincho".into(), group: FontGroup::Japanese, native: Some("グループ明朝".into()) });
    assert_eq!(find("Groups Kana").group, FontGroup::Japanese);
    assert_eq!(find("Groups Ming").group, FontGroup::ChineseTraditional);
    assert_eq!(find(crate::DEFAULT_FAMILY).group, FontGroup::Western);
    assert!(!db.is_loaded("Groups Mincho"), "the scan read its tables, not the font");
    // Families are listed once.
    assert_eq!(infos.iter().filter(|i| i.family == "Groups Mincho").count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rescanning_retries_missing_glyphs_and_absent_fallback_families() {
    use crate::testing::font_with;
    let dir = font_dir("rescan-fallback-ttf");
    // Only synthetic TTF files in this folder are scanned. Keep the loaded Latin fallback
    // alone so optional craft-fonts cannot provide the characters used by this fixture.
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    db.faces.write().unwrap().retain(|f| f.family == FALLBACK_FAMILY);
    let c = '\u{1e900}';
    assert!(db.fallback_for(c, 0, None).is_none());
    // A loaded non-chain face supplies Hangul while all installed Korean chain faces are absent.
    db.add_font(font_with("Earlier Korean Fallback", &['가']).unwrap());
    let chain_family = "Noto Serif CJK KR";
    assert!(!db.has_family(chain_family));
    assert_eq!(db.fallback_for('가', 0, Some("ko")).unwrap().family, "Earlier Korean Fallback");
    let family = "Installed After Glyph Miss";
    std::fs::write(dir.join("Later.ttf"), font_with(family, &[c]).unwrap()).unwrap();
    std::fs::write(dir.join("LaterKorean.ttf"), font_with(chain_family, &['가']).unwrap()).unwrap();
    // Before an explicit rescan, both fallbacks still use the previous catalog.
    assert!(db.fallback_for(c, 0, None).is_none());
    assert_eq!(db.fallback_for('가', 0, Some("ko")).unwrap().family, "Earlier Korean Fallback");
    assert_eq!(db.load_system_fonts(), 4);
    assert!(db.has_family(family) && db.has_family(chain_family));
    assert_eq!(db.fallback_for(c, 0, None).unwrap().family, family);
    assert_eq!(db.fallback_for('가', 0, Some("ko")).unwrap().family, chain_family);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn a_rescan_does_not_cache_an_older_in_flight_glyph_miss() {
    use crate::testing::font_with;
    use std::sync::{Barrier, mpsc};
    use std::time::Duration;

    let dir = font_dir("rescan-in-flight-ttf");
    let db = Arc::new(FontDb::with_font_dirs(vec![dir.clone()]));
    // Optional craft-fonts must not supply the test glyph from an already loaded face.
    db.faces.write().unwrap().retain(|f| f.family == FALLBACK_FAMILY);
    let c = '\u{1e900}';
    let family = "Installed During Glyph Lookup";
    let font = font_with(family, &[c]).unwrap();
    assert!(db.read_faces().iter().all(|f| !f.covers(c)));
    assert_eq!(db.load_system_fonts(), 2);

    let deadline = Duration::from_secs(10);
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let resume = Arc::new(Barrier::new(2));
    *db.fallback_snapshot_hook.lock().unwrap() = Some(FallbackSnapshotHook { reached: reached_tx, resume: resume.clone() });
    let (lookup_tx, lookup_rx) = mpsc::sync_channel(1);
    let lookup_db = db.clone();
    let lookup = std::thread::spawn(move || {
        let face = lookup_db.fallback_for(c, 0, None).map(|f| f.family.clone());
        let _ = lookup_tx.send(face);
    });
    reached_rx.recv_timeout(deadline).expect("the lookup must reach its old catalog snapshot");

    // Finish the rescan while the old lookup is paused. A timeout also detects accidentally
    // holding sys/catalog locks at the hook; release the lookup before reporting that failure.
    let (scan_tx, scan_rx) = mpsc::sync_channel(1);
    let scan_db = db.clone();
    let scan = std::thread::spawn(move || {
        let result = std::fs::write(dir.join("Later.ttf"), font).map(|()| scan_db.load_system_fonts());
        let _ = scan_tx.send(result);
    });
    let scanned = scan_rx.recv_timeout(deadline);
    resume.wait();
    let old_face = lookup_rx.recv_timeout(deadline).expect("the old lookup must finish after release");
    lookup.join().unwrap();
    assert_eq!(scanned.expect("the rescan must finish before releasing the old lookup").unwrap(), 3);
    scan.join().unwrap();
    assert!(old_face.is_none(), "the paused lookup only searched the old paths");
    assert!(db.has_family(family));
    assert!(!db.is_loaded(family), "a rescan catalogs the font without loading it");
    // The old miss must not hide the new catalog entry; no second rescan is performed.
    assert_eq!(db.fallback_for(c, 0, None).unwrap().family, family);
    std::fs::remove_dir_all(db.font_dirs.first().unwrap()).unwrap();
}

#[test]
fn installed_and_loaded_fonts_are_found_by_postscript_name() {
    let dir = std::env::temp_dir().join(format!("dc-sysfonts-postscript-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ps.ttf"), crate::testing::font_with("DC Test PS Installed", &['a']).unwrap()).unwrap();
    let db = FontDb::with_font_dirs(vec![dir]);
    let found = Some(("DC Test PS Installed".to_string(), "Regular".to_string()));
    // From the catalog, without loading the font; case doesn't matter.
    assert_eq!(db.postscript_face("dctestpsinstalled-regular"), found);
    assert!(!db.is_loaded("DC Test PS Installed"));
    // Bundled and added (loaded) faces.
    assert_eq!(db.postscript_face("SourceSerif4-Semibold"), Some(("Source Serif 4".to_string(), "Semibold".to_string())));
    db.add_font(crate::testing::font_with("DC Test PS Added", &['a']).unwrap());
    assert_eq!(db.postscript_face("DCTestPSAdded-Regular"), Some(("DC Test PS Added".to_string(), "Regular".to_string())));
    // A family name isn't a PostScript name.
    assert_eq!(db.postscript_face("Source Serif 4"), None);
    assert_eq!(db.postscript_face("NoSuchFont-Regular"), None);
}

/// Write `font` to `path` with a modification time `secs` from now, so a rewrite of the same
/// length still reads as changed however coarse the file system's clock.
fn write_font(path: &Path, font: &[u8], secs: u64) {
    std::fs::write(path, font).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(secs)).unwrap();
}

#[test]
fn reloading_reads_changed_font_files_again_and_keeps_the_others() {
    use crate::testing::font_with_glyph;
    let dir = font_dir("reload");
    let family = "DC Test Reloaded";
    let path = dir.join("Reloaded.ttf");
    write_font(&path, &font_with_glyph(family, &['a'], 'W').unwrap(), 10);
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    let before = db.face(family, "Regular");
    let wide = before.advance(before.glyph_for('a'));
    let kept = db.face(FAMILY, "Regular");
    // The font is edited and installed again under the same name (#328).
    write_font(&path, &font_with_glyph(family, &['a'], 'i').unwrap(), 20);
    assert_eq!(db.face(family, "Regular").id(), before.id(), "a lookup alone doesn't read the file again");
    let reloaded = db.reload_fonts();
    assert_eq!(reloaded.changed, std::slice::from_ref(&path));
    assert_eq!(reloaded.cataloged, 3);
    let after = db.face(family, "Regular");
    assert_ne!(after.id(), before.id());
    let narrow = after.advance(after.glyph_for('a'));
    assert!(narrow < wide, "the edited font sets 'a' as its i: {narrow} vs {wide}");
    assert_eq!(db.face(FAMILY, "Regular").id(), kept.id(), "unchanged files aren't read again");
    assert_eq!(db.reload_fonts().changed, Vec::<PathBuf>::new(), "nothing changed since");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reloading_lets_go_of_removed_fonts_and_finds_new_ones() {
    let dir = font_dir("reload-removed");
    let removed = dir.join("Removed.ttf");
    write_font(&removed, &crate::testing::font_with("DC Test Removed", &['a']).unwrap(), 10);
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    assert_eq!(db.face("DC Test Removed", "Regular").family, "DC Test Removed");
    std::fs::remove_file(&removed).unwrap();
    write_font(&dir.join("Added.ttf"), &crate::testing::font_with("DC Test Added Later", &['a']).unwrap(), 10);
    assert_eq!(db.reload_fonts().changed, [removed]);
    assert!(!db.has_family("DC Test Removed"));
    assert_eq!(db.face("DC Test Removed", "Regular").family, FALLBACK_FAMILY);
    assert!(db.has_family("DC Test Added Later"));
    let _ = std::fs::remove_dir_all(&dir);
}
