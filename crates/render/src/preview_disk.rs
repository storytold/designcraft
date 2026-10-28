//! Disposable raster previews. No source paths or original artwork are written here.
//! Versioned keys include SHA-256 of the original bytes, page and mip; values include a
//! checksum of the premultiplied pixels. Cache errors always fall back to original decoding.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use sha2::{Digest, Sha256};
use vello_cpu::Pixmap;

const MAGIC: &[u8; 8] = b"DCPRV001";
const HEADER: usize = 48;
const MAX_SIDE: u32 = 2048;
const DISK_BUDGET: u64 = 512 << 20;
const MAX_BYTES: u64 = (MAX_SIDE as u64 * MAX_SIDE as u64 * 4) + HEADER as u64;
type Fingerprint = (Weak<Vec<u8>>, String);

#[derive(Default)]
struct Disk {
    directory: Option<PathBuf>,
    hashes: HashMap<usize, Fingerprint>,
    colour: Option<(Weak<designcraft_color::cms::Cms>, String)>,
}
static DISK: Mutex<Option<Disk>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(0);
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Disabling persistence leaves the in-memory previews and original artwork unchanged.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// A native host supplies its cache directory. None disables persistence (CLI and web default).
pub fn set_directory(directory: Option<PathBuf>) {
    if ENABLED.load(Ordering::Relaxed)
        && let Some(dir) = &directory
    {
        prune(dir);
    }
    let mut disk = DISK.lock().unwrap_or_else(|e| e.into_inner());
    *disk = Some(Disk { directory, ..Default::default() });
}

// Automatically invalidate when decoding/rasterization code or pinned dependencies change,
// including development builds that retain the same application version.
fn renderer_tag() -> &'static str {
    static TAG: OnceLock<String> = OnceLock::new();
    TAG.get_or_init(|| {
        let mut hash = Sha256::new();
        for source in [
            include_bytes!("lib.rs").as_slice(),
            include_bytes!("color.rs").as_slice(),
            include_bytes!("decode.rs").as_slice(),
            include_bytes!("draw.rs").as_slice(),
            include_bytes!("driver.rs").as_slice(),
            include_bytes!("types.rs").as_slice(),
            include_bytes!("pdf_raster.rs").as_slice(),
            include_bytes!("blendspace.rs").as_slice(),
            include_bytes!("images.rs").as_slice(),
            include_bytes!("preview_disk.rs").as_slice(),
            include_bytes!("../../../Cargo.lock").as_slice(),
            include_bytes!("../../images/src/lib.rs").as_slice(),
            include_bytes!("../../images/src/psd.rs").as_slice(),
            include_bytes!("../../images/src/eps.rs").as_slice(),
        ] {
            hash.update(source);
        }
        format!("{:x}", hash.finalize())
    })
}

fn colour_hash(settings: &designcraft_color::cms::ColorSettings, cmyk: Option<&[u8]>) -> String {
    let mut hash = Sha256::new();
    hash.update(format!("{settings:?}").as_bytes());
    hash.update([u8::from(cmyk.is_some())]);
    if let Some(profile) = cmyk {
        hash.update(profile);
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn path_for(bytes: &Arc<Vec<u8>>, page: u32, level: u8, cms: &Arc<designcraft_color::cms::Cms>) -> Option<PathBuf> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    let id = Arc::as_ptr(bytes) as usize;
    let (directory, hash, colour) = {
        let disk = DISK.lock().unwrap_or_else(|e| e.into_inner());
        let disk = disk.as_ref()?;
        (
            disk.directory.clone()?,
            disk.hashes.get(&id).filter(|(w, _)| w.strong_count() > 0).map(|(_, h)| h.clone()),
            disk.colour.as_ref().filter(|(w, _)| w.strong_count() > 0 && std::ptr::eq(w.as_ptr(), Arc::as_ptr(cms))).map(|(_, h)| h.clone()),
        )
    };
    let hash = hash.unwrap_or_else(|| {
        let hash = format!("{:x}", Sha256::digest(bytes.as_slice()));
        let mut disk = DISK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(disk) = disk.as_mut() {
            disk.hashes.retain(|_, (w, _)| w.strong_count() > 0);
            disk.hashes.insert(id, (Arc::downgrade(bytes), hash.clone()));
        }
        hash
    });
    let colour = colour.unwrap_or_else(|| {
        // A replacement ICC profile can keep its display name. Hash its effective
        // CMYK bytes too, once per immutable context, rather than reusing old pixels.
        let profile = cms.cmyk_icc_profile().map(|p| p.to_bytes());
        let hash = colour_hash(cms.settings(), profile.as_deref().map(Vec::as_slice));
        if let Some(disk) = DISK.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            disk.colour = Some((Arc::downgrade(cms), hash.clone()));
        }
        hash
    });
    Some(directory.join(format!("v3-{}-{colour}-{hash}-{page}-{level}.dcpreview", renderer_tag())))
}

pub(super) fn load_path(path: &Path, size: (u32, u32)) -> Option<Pixmap> {
    if !ENABLED.load(Ordering::Relaxed) || size.0 == 0 || size.1 == 0 || size.0 > MAX_SIDE || size.1 > MAX_SIDE {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let expected = HEADER as u64 + size.0 as u64 * size.1 as u64 * 4;
    if file.metadata().ok()?.len() != expected {
        return None;
    }
    let mut data = Vec::new();
    data.try_reserve_exact(usize::try_from(expected).ok()?).ok()?;
    file.take(MAX_BYTES + 1).read_to_end(&mut data).ok()?;
    let header = data.get(..HEADER)?;
    if data.len() as u64 != expected || header.get(..8)? != MAGIC {
        return None;
    }
    let number = |r: std::ops::Range<usize>| -> Option<u32> { Some(u32::from_le_bytes(header.get(r)?.try_into().ok()?)) };
    if number(8..12)? != size.0 || number(12..16)? != size.1 {
        return None;
    }
    let pixels = data.get(HEADER..)?;
    if &Sha256::digest(pixels)[..] != header.get(16..48)? {
        return None;
    }
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(pixels.len() / 4).ok()?;
    for p in pixels.as_chunks::<4>().0 {
        // Reject invalid premultiplied values even if a cache file was deliberately rewritten.
        if p[0] > p[3] || p[1] > p[3] || p[2] > p[3] {
            return None;
        }
        rgba.push(vello_cpu::color::PremulRgba8 { r: p[0], g: p[1], b: p[2], a: p[3] });
    }
    Some(Pixmap::from_parts(rgba, size.0 as u16, size.1 as u16))
}

pub(super) fn store_path(path: &Path, pm: &Pixmap) {
    if !ENABLED.load(Ordering::Relaxed) || pm.width() as u32 > MAX_SIDE || pm.height() as u32 > MAX_SIDE {
        return;
    }
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let temp = path.with_extension(format!("{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let Ok(mut file) = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp) else { return };
    let write = || -> std::io::Result<()> {
        let pixels: Vec<u8> = pm.data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
        file.write_all(MAGIC)?;
        file.write_all(&(pm.width() as u32).to_le_bytes())?;
        file.write_all(&(pm.height() as u32).to_le_bytes())?;
        file.write_all(&Sha256::digest(&pixels))?;
        file.write_all(&pixels)?;
        drop(file);
        std::fs::rename(&temp, path)
    };
    if write().is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    prune(dir);
}

fn prune(dir: &Path) {
    prune_with_budget(dir, DISK_BUDGET, std::time::SystemTime::now());
}

// Recognize past and future cache versions too: renderer changes must not orphan files.
fn cache_name(name: &str) -> bool {
    let Some((version, _)) = name.strip_prefix('v').and_then(|s| s.split_once('-')) else { return false };
    !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit())
}

fn prune_with_budget(dir: &Path, budget: u64, now: std::time::SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files = Vec::new();
    let mut total = 0u64;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !cache_name(&name) {
            continue;
        }
        // Do not follow symlinks or touch directories, including similarly named ones.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if name.ends_with(".tmp") {
            // Another process may still be writing a fresh temporary. Only reap abandoned ones.
            if meta.modified().ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age.as_secs() >= 24 * 60 * 60) {
                let _ = std::fs::remove_file(entry.path());
            }
            continue;
        }
        if !name.ends_with(".dcpreview") {
            continue;
        }
        total = total.saturating_add(meta.len());
        files.push((meta.modified().ok(), meta.len(), entry.path()));
    }
    files.sort_unstable();
    for (_, size, path) in files {
        if total <= budget {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(bytes: &Arc<Vec<u8>>, page: u32, level: u8) -> Option<PathBuf> {
        path_for(bytes, page, level, &designcraft_color::cms::active())
    }

    #[test]
    fn preview_colour_keys_include_profile_bytes_not_just_the_name() {
        let settings = designcraft_color::cms::ColorSettings::default();
        let first = colour_hash(&settings, Some(b"first profile bytes"));
        assert_ne!(first, colour_hash(&settings, Some(b"replacement profile bytes")));
        assert_eq!(first, colour_hash(&settings, Some(b"first profile bytes")));
        assert_ne!(first, colour_hash(&settings, None));
    }

    fn load(bytes: &Arc<Vec<u8>>, page: u32, level: u8, size: (u32, u32)) -> Option<Pixmap> {
        load_path(&path(bytes, page, level)?, size)
    }

    fn store(bytes: &Arc<Vec<u8>>, page: u32, level: u8, pm: &Pixmap) {
        if let Some(path) = path(bytes, page, level) {
            store_path(&path, pm);
        }
    }

    fn dated_file(dir: &Path, name: &str, size: usize, time: std::time::SystemTime) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![0; size]).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_times(std::fs::FileTimes::new().set_modified(time)).unwrap();
        path
    }

    #[test]
    fn preview_budget_counts_current_and_other_versions_and_removes_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now();
        let files: Vec<_> =
            ["v1-old.dcpreview", "v2-old.dcpreview", "v3-old-renderer.dcpreview", "v3-current-renderer.dcpreview", "v12-future.dcpreview"]
                .into_iter()
                .enumerate()
                .map(|(i, name)| dated_file(dir.path(), name, 4, now - std::time::Duration::from_secs(100 - i as u64)))
                .collect();
        let original = dated_file(dir.path(), "original.png", 100, now);
        let unrelated = dated_file(dir.path(), "v-not-a-cache.dcpreview", 100, now);
        prune_with_budget(dir.path(), 8, now);
        assert!(files[..3].iter().all(|p| !p.exists()));
        assert!(files[3..].iter().all(|p| p.exists()));
        assert_eq!(files.iter().filter_map(|p| p.metadata().ok()).map(|m| m.len()).sum::<u64>(), 8);
        assert!(original.exists() && unrelated.exists());
    }

    #[test]
    fn preview_cleanup_reaps_abandoned_temps_without_touching_active_writes_or_directories() {
        let dir = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now();
        let old = dated_file(dir.path(), "v3-renderer-1-0.123-0.tmp", 20, now - std::time::Duration::from_secs(24 * 60 * 60));
        let active = dated_file(dir.path(), "v3-renderer-1-0.123-1.tmp", 20, now);
        let unrelated = dated_file(dir.path(), "notes.tmp", 20, now - std::time::Duration::from_secs(48 * 60 * 60));
        let folder = dir.path().join("v3-directory.dcpreview");
        std::fs::create_dir(&folder).unwrap();
        #[cfg(unix)]
        let symlink = {
            let target = dir.path().join("v3-link.dcpreview");
            std::os::unix::fs::symlink(&unrelated, &target).unwrap();
            target
        };
        prune_with_budget(dir.path(), 0, now);
        assert!(!old.exists());
        assert!(active.exists() && unrelated.exists() && folder.is_dir());
        #[cfg(unix)]
        assert!(symlink.symlink_metadata().unwrap().file_type().is_symlink());
    }

    fn two_page_pdf() -> Arc<Vec<u8>> {
        let red = "1 0 0 rg 0 0 100 100 re f";
        let blue = "0 0 1 rg 0 0 100 100 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 5 0 R >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 6 0 R >>".into(),
            format!("<< /Length {} >>\nstream\n{red}\nendstream", red.len()),
            format!("<< /Length {} >>\nstream\n{blue}\nendstream", blue.len()),
        ];
        let mut pdf = "%PDF-1.4\n".to_string();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
        }
        let xref = pdf.len();
        pdf.push_str("xref\n0 7\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"));
        Arc::new(pdf.into_bytes())
    }

    #[test]
    fn disk_previews_are_lossless_content_addressed_and_corruption_is_ignored() {
        // Only this test enables the process-wide disk cache in render's unit-test process.
        let dir = tempfile::tempdir().unwrap();
        set_directory(Some(dir.path().into()));
        let bytes = Arc::new(vec![1, 2, 3]);
        let pm = Pixmap::from_parts(vec![vello_cpu::color::PremulRgba8 { r: 10, g: 20, b: 30, a: 40 }; 4], 2, 2);
        store(&bytes, 1, 2, &pm);
        assert_eq!(load(&bytes, 1, 2, (2, 2)).unwrap().data(), pm.data());
        set_enabled(false);
        assert!(load(&bytes, 1, 2, (2, 2)).is_none());
        store(&bytes, 9, 2, &pm);
        let abandoned =
            dated_file(dir.path(), "v3-disabled.1-0.tmp", 20, std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 60 * 60));
        set_directory(Some(dir.path().into()));
        assert!(abandoned.exists(), "installing a directory while disabled must not clean up files");
        set_enabled(true);
        set_directory(Some(dir.path().into()));
        assert!(!abandoned.exists(), "enabled startup still cleans abandoned cache writes");
        assert!(!path(&bytes, 9, 2).unwrap().exists());
        assert_eq!(load(&bytes, 1, 2, (2, 2)).unwrap().data(), pm.data());
        assert!(load(&bytes, 0, 2, (2, 2)).is_none());
        assert!(load(&Arc::new(vec![1, 2, 4]), 1, 2, (2, 2)).is_none());
        assert!(load(&bytes, 1, 2, (3, 2)).is_none());
        let p = path(&bytes, 1, 2).unwrap();
        let mut raw = std::fs::read(&p).unwrap();
        *raw.last_mut().unwrap() ^= 1;
        std::fs::write(&p, raw).unwrap();
        assert!(load(&bytes, 1, 2, (2, 2)).is_none());
        // A pending write owns its captured key, even if host configuration changes
        // while pixels are prepared. It must never recompute a newer settings/path key.
        let pending = path(&bytes, 8, 2).unwrap();
        let other = tempfile::tempdir().unwrap();
        set_directory(Some(other.path().into()));
        let changed = path(&bytes, 8, 2).unwrap();
        store_path(&pending, &pm);
        assert!(pending.exists());
        assert!(!changed.exists());
        set_directory(Some(dir.path().into()));
        // A fresh document can reuse the right PDF page without rasterizing it first.
        let pdf = two_page_pdf();
        let red = crate::images::mip(&pdf, 0, 100.0, 1.0).unwrap();
        let blue = crate::images::mip(&pdf, 1, 100.0, 1.0).unwrap();
        assert!(red.data()[0].r > 0 && red.data()[0].b == 0);
        assert!(blue.data()[0].b > 0 && blue.data()[0].r == 0);
        let reopened = Arc::new(pdf.as_slice().to_vec());
        let again = crate::images::mip(&reopened, 0, 100.0, 1.0).unwrap();
        assert_eq!(red.data(), again.data());
        assert!(
            crate::images::lookup((Arc::as_ptr(&reopened) as usize, 0, 0)).is_none(),
            "loading a PDF preview must not rasterize the full page just to discover its size"
        );
        set_directory(None);
    }
}
