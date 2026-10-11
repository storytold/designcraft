//! Installed system fonts: found by family name by every lookup, whatever ran before it (#22).

use std::path::{Path, PathBuf};

use super::*;

const FAMILY: &str = "Sysfont Sans3";

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
