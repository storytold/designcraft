//! Disposable raster previews. No source paths or original artwork are written here.
//! Versioned keys include SHA-256 of the original bytes, page and mip; values include a
//! checksum of the premultiplied pixels. Cache errors always fall back to original decoding.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
}
static DISK: Mutex<Option<Disk>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(0);

/// A native host supplies its cache directory. None disables persistence (CLI and web default).
pub fn set_directory(directory: Option<PathBuf>) {
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

fn path(bytes: &Arc<Vec<u8>>, page: u32, level: u8) -> Option<PathBuf> {
    let id = Arc::as_ptr(bytes) as usize;
    let (directory, hash) = {
        let disk = DISK.lock().unwrap_or_else(|e| e.into_inner());
        let disk = disk.as_ref()?;
        (disk.directory.clone()?, disk.hashes.get(&id).filter(|(w, _)| w.strong_count() > 0).map(|(_, h)| h.clone()))
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
    let colour = format!("{:x}", Sha256::digest(format!("{:?}", designcraft_color::cms::active_settings()).as_bytes()));
    Some(directory.join(format!("v3-{}-{colour}-{hash}-{page}-{level}.dcpreview", renderer_tag())))
}

pub(super) fn load(bytes: &Arc<Vec<u8>>, page: u32, level: u8, size: (u32, u32)) -> Option<Pixmap> {
    if size.0 == 0 || size.1 == 0 || size.0 > MAX_SIDE || size.1 > MAX_SIDE {
        return None;
    }
    let file = std::fs::File::open(path(bytes, page, level)?).ok()?;
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

pub(super) fn store(bytes: &Arc<Vec<u8>>, page: u32, level: u8, pm: &Pixmap) {
    if pm.width() as u32 > MAX_SIDE || pm.height() as u32 > MAX_SIDE {
        return;
    }
    let Some(path) = path(bytes, page, level) else { return };
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
        std::fs::rename(&temp, &path)
    };
    if write().is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    prune(dir);
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files = Vec::new();
    let mut total = 0u64;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("v1-") || name.starts_with("v2-")) || !name.ends_with(".dcpreview") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        total = total.saturating_add(meta.len());
        files.push((meta.modified().ok(), meta.len(), entry.path()));
    }
    files.sort_unstable();
    for (_, size, path) in files {
        if total <= DISK_BUDGET {
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
        assert!(load(&bytes, 0, 2, (2, 2)).is_none());
        assert!(load(&Arc::new(vec![1, 2, 4]), 1, 2, (2, 2)).is_none());
        assert!(load(&bytes, 1, 2, (3, 2)).is_none());
        let p = path(&bytes, 1, 2).unwrap();
        let mut raw = std::fs::read(&p).unwrap();
        *raw.last_mut().unwrap() ^= 1;
        std::fs::write(&p, raw).unwrap();
        assert!(load(&bytes, 1, 2, (2, 2)).is_none());
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
