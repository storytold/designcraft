//! Reading linked files' bytes for the document model ([`designcraft_doc::asset_data`]): the
//! [`Loader`] every [`crate::Session`] installs. Files are read through [`crate::io`] (the
//! automation roots of the loading thread apply) and only ever yield the bytes the link record
//! names: other bytes at the path are [`Unavailable::Modified`] and are never handed out.
//!
//! The web build installs no loader: its linked assets are [`Unavailable::NoLoader`].

use std::path::{Path, PathBuf};

use designcraft_doc::asset_data::{Loaded, Loader, MAX_LINKED_BYTES};
use designcraft_doc::{Fingerprint, LinkedSource, Unavailable};

/// Read linked files with [`FileLoader`] in this process (on the web: nothing to install).
pub fn install() {
    #[cfg(not(target_arch = "wasm32"))]
    designcraft_doc::asset_data::install_loader(std::sync::Arc::new(FileLoader));
}

/// The engine's [`Loader`]: [`read_linked`].
pub struct FileLoader;

impl Loader for FileLoader {
    fn load(&self, source: &LinkedSource, trusted: Option<Fingerprint>) -> Result<Loaded, Unavailable> {
        read_linked(source, trusted)
    }

    fn load_current(&self, source: &LinkedSource) -> Result<Vec<u8>, Unavailable> {
        read_current(source)
    }
}

/// The bytes of the first candidate file that exists, whatever they are (no fingerprint or hash
/// check): what an export uses for a link whose file has changed.
pub fn read_current(source: &LinkedSource) -> Result<Vec<u8>, Unavailable> {
    for path in candidates(source) {
        let Some(meta) = crate::io::file_metadata(&path) else { continue };
        if meta.len() > MAX_LINKED_BYTES {
            return Err(Unavailable::TooLarge);
        }
        if let Ok(bytes) = crate::io::read_capped(&path, MAX_LINKED_BYTES) {
            return Ok(bytes);
        }
    }
    Err(Unavailable::Missing)
}

/// `sha256:` and the hex digits of the SHA-256 of `bytes` ([`designcraft_doc::Asset::link_hash`]).
pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(7 + 64);
    out.push_str("sha256:");
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Size and modification time (whole seconds) from a file's metadata, as link records store them.
pub fn fingerprint_from(meta: &crate::io::Metadata) -> Fingerprint {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Fingerprint { size: meta.len(), mtime }
}

/// Whether `f` has a modification time to tell the file by: [`fingerprint_from`] gives 0 where
/// the file system reports none, and two such files of one size would look the same.
pub fn has_time(f: &Fingerprint) -> bool {
    f.mtime != 0
}

/// The files that may hold `source`'s bytes, in the order they are tried: the absolute path,
/// then the relative one resolved against the document. Paths [`crate::io::document_path`]
/// refuses are left out.
pub fn candidates(source: &LinkedSource) -> Vec<PathBuf> {
    let document = source.document.as_deref();
    let mut out: Vec<PathBuf> = crate::io::document_path(&source.path, document).into_iter().collect();
    if let (Some(relative), Some(document)) = (source.relative.as_deref(), document)
        && let Ok(p) = crate::io::document_path(relative, Some(document))
        && !out.contains(&p)
    {
        out.push(p);
    }
    out
}

/// Why one candidate file doesn't serve.
enum Miss {
    Absent,
    Modified,
    TooLarge,
}

/// The bytes of the first candidate file ([`candidates`]) that holds `source`'s bytes: one of
/// `source.len` bytes whose fingerprint is `source.fingerprint` or `trusted`, or whose content
/// hash is `source.hash`. A source that records neither fingerprint nor hash is taken by its
/// size. The fingerprint of a file accepted by its hash is returned with the bytes.
pub fn read_linked(source: &LinkedSource, trusted: Option<Fingerprint>) -> Result<Loaded, Unavailable> {
    if source.len > MAX_LINKED_BYTES {
        return Err(Unavailable::TooLarge);
    }
    let mut worst = Miss::Absent;
    for path in candidates(source) {
        match read_candidate(&path, source, trusted) {
            Ok(loaded) => return Ok(loaded),
            Err(Miss::Absent) => {}
            Err(miss @ Miss::TooLarge) => worst = miss,
            Err(Miss::Modified) => {
                if !matches!(worst, Miss::TooLarge) {
                    worst = Miss::Modified;
                }
            }
        }
    }
    Err(match worst {
        Miss::Absent => Unavailable::Missing,
        Miss::Modified => Unavailable::Modified,
        Miss::TooLarge => Unavailable::TooLarge,
    })
}

fn read_candidate(path: &Path, source: &LinkedSource, trusted: Option<Fingerprint>) -> Result<Loaded, Miss> {
    let meta = crate::io::file_metadata(path).ok_or(Miss::Absent)?;
    let found = fingerprint_from(&meta);
    if found.size > MAX_LINKED_BYTES {
        return Err(Miss::TooLarge);
    }
    if found.size != source.len {
        return Err(Miss::Modified);
    }
    let known = has_time(&found) && (Some(found) == source.fingerprint || Some(found) == trusted);
    if !known && source.hash.is_none() && source.fingerprint.is_some() {
        // Nothing to tell the bytes by.
        return Err(Miss::Modified);
    }
    // At most one byte past the size: a file that grew since is caught without reading it all.
    let bytes = crate::io::read_capped(path, source.len).map_err(|_| Miss::Absent)?;
    if bytes.len() as u64 != source.len {
        return Err(Miss::Modified);
    }
    if known {
        return Ok(Loaded { bytes, fingerprint: None });
    }
    match &source.hash {
        Some(hash) if *hash != content_hash(&bytes) => Err(Miss::Modified),
        Some(_) => Ok(Loaded { bytes, fingerprint: Some(found) }),
        None => Ok(Loaded { bytes, fingerprint: None }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::AssetData;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("dc-asset-io-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The link record of the file at `path` as it is now.
    fn source_of(path: &Path) -> LinkedSource {
        let bytes = std::fs::read(path).unwrap();
        LinkedSource {
            path: path.to_string_lossy().into_owned(),
            relative: None,
            document: None,
            len: bytes.len() as u64,
            fingerprint: Some(fingerprint_from(&std::fs::metadata(path).unwrap())),
            hash: Some(content_hash(&bytes)),
        }
    }

    /// Set the file's modification time `secs` seconds into the past.
    fn age(path: &Path, secs: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
    }

    #[test]
    fn content_hashes_are_sha256_hex() {
        assert_eq!(content_hash(b"abc"), "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_linked_file_is_read_when_it_holds_the_recorded_bytes() {
        let dir = Dir::new("read");
        let path = dir.0.join("a.png");
        std::fs::write(&path, b"original").unwrap();
        let source = source_of(&path);
        assert_eq!(read_linked(&source, None).ok().map(|l| (l.bytes, l.fingerprint)), Some((b"original".to_vec(), None)));
        // Same bytes, another time: accepted by the hash, and the new fingerprint returned.
        age(&path, 1000);
        let now = fingerprint_from(&std::fs::metadata(&path).unwrap());
        let loaded = read_linked(&source, None).ok().unwrap();
        assert_eq!((loaded.bytes.as_slice(), loaded.fingerprint), (b"original".as_slice(), Some(now)));
        assert!(read_linked(&source, Some(now)).is_ok_and(|l| l.fingerprint.is_none()), "a trusted fingerprint needs no hash");
        let unverifiable = LinkedSource { hash: None, ..source.clone() };
        assert_eq!(read_linked(&unverifiable, None).err(), Some(Unavailable::Modified), "nothing to tell the bytes by");
    }

    #[test]
    fn other_bytes_at_the_path_are_modified_and_a_missing_file_is_missing() {
        let dir = Dir::new("modified");
        let path = dir.0.join("a.png");
        std::fs::write(&path, b"original").unwrap();
        let source = source_of(&path);
        std::fs::write(&path, b"changed!").unwrap();
        age(&path, 1000);
        assert_eq!(read_linked(&source, None).err(), Some(Unavailable::Modified), "same size, other bytes");
        std::fs::write(&path, b"longer than before").unwrap();
        assert_eq!(read_linked(&source, None).err(), Some(Unavailable::Modified), "another size");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_linked(&source, None).err(), Some(Unavailable::Missing));
        let too_large = LinkedSource { len: MAX_LINKED_BYTES + 1, ..source };
        assert_eq!(read_linked(&too_large, None).err(), Some(Unavailable::TooLarge));
    }

    #[test]
    fn the_relative_path_serves_when_the_absolute_one_does_not() {
        let dir = Dir::new("relative");
        std::fs::create_dir_all(dir.0.join("moved/links")).unwrap();
        let away = dir.0.join("moved/links/a.png");
        std::fs::write(&away, b"original").unwrap();
        let mut source = source_of(&away);
        source.path = dir.0.join("gone/links/a.png").to_string_lossy().into_owned();
        assert_eq!(read_linked(&source, None).err(), Some(Unavailable::Missing));
        source.relative = Some("links/a.png".into());
        source.document = Some(dir.0.join("moved/doc.dcx"));
        assert_eq!(read_linked(&source, None).ok().map(|l| l.bytes), Some(b"original".to_vec()));
        // A changed file at the absolute path doesn't hide the one that holds the bytes.
        std::fs::create_dir_all(dir.0.join("gone/links")).unwrap();
        std::fs::write(dir.0.join("gone/links/a.png"), b"other!!!").unwrap();
        age(&dir.0.join("gone/links/a.png"), 1000);
        assert_eq!(read_linked(&source, None).ok().map(|l| l.bytes), Some(b"original".to_vec()));
    }

    /// A time of 0 is what a file system without times gives: it can't tell two files apart.
    #[test]
    fn a_file_without_a_time_is_told_by_its_hash() {
        let dir = Dir::new("no-time");
        let path = dir.0.join("a.png");
        let at_epoch = |p: &Path| std::fs::OpenOptions::new().write(true).open(p).unwrap().set_modified(std::time::UNIX_EPOCH).unwrap();
        std::fs::write(&path, b"original").unwrap();
        at_epoch(&path);
        let source = source_of(&path);
        assert_eq!(source.fingerprint.map(|f| f.mtime), Some(0));
        std::fs::write(&path, b"changed!").unwrap();
        at_epoch(&path);
        assert_eq!(read_linked(&source, None).err(), Some(Unavailable::Modified));
    }

    /// Through the installed loader and the store: a file changed on disk is never adopted.
    #[test]
    fn asset_data_loads_through_the_installed_loader() {
        install();
        let dir = Dir::new("installed");
        let path = dir.0.join("a.png");
        std::fs::write(&path, b"original").unwrap();
        let data = AssetData::linked(source_of(&path));
        let changed = AssetData::linked(source_of(&path));
        assert_eq!(data.load().ok().map(|b| b.to_vec()), Some(b"original".to_vec()));
        assert!(data.resident().is_some());
        std::fs::write(&path, b"changed!").unwrap();
        age(&path, 1000);
        assert_eq!(data.load().ok().map(|b| b.to_vec()), Some(b"original".to_vec()), "kept bytes are the recorded ones");
        assert_eq!(changed.load().err(), Some(Unavailable::Modified));
        assert!(changed.resident().is_none(), "nothing adopted");
        std::fs::remove_file(&path).unwrap();
        data.evict();
        assert_eq!(data.load().err(), Some(Unavailable::Missing));
    }
}
