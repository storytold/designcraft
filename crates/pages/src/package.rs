//! The package: a ZIP holding `Index/*.iwa` archives (the objects) and `Data/` (placed files).
//! Each archive is a run of objects: a varint length, a `TSP.ArchiveInfo` (identifier in field 1,
//! message infos in field 2: type in 1, payload length in 3) and the payloads that follow.

use std::collections::HashMap;
use std::io::Read;

use crate::proto::{Msg, varint};
use crate::{PagesError, Result, snappy};

/// The most any one ZIP entry is read to, and the most all archives may expand to.
const MAX_ENTRY: u64 = 1 << 30;
/// The most all the files read from a package may hold together.
const MAX_PACKAGE: u64 = 2 << 30;
const MAX_OBJECTS_BYTES: usize = 1 << 30;

/// `TSP.PackageMetadata`, which lists the files in `Data/`.
const PACKAGE_METADATA: u32 = 11006;

/// Whether the file at `name` in a package is one the import reads.
pub(crate) fn wanted(name: &str) -> bool {
    (name.starts_with("Index/") && name.ends_with(".iwa")) || name.starts_with("Data/") || name == "Index.zip"
}

pub(crate) struct Package {
    archives: Vec<Vec<u8>>,
    /// Object identifier → (type, archive, payload range).
    objects: HashMap<u64, (u32, usize, std::ops::Range<usize>)>,
    /// Files in `Data/` by name.
    data: HashMap<String, Vec<u8>>,
    /// Data identifier → (file name in `Data/`, name to show).
    data_names: HashMap<u64, (String, String)>,
}

impl Package {
    /// A single-file document (a ZIP).
    pub fn read(bytes: &[u8]) -> Result<Package> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| PagesError::NotPages(e.to_string()))?;
        let mut files = Vec::new();
        let mut left = MAX_PACKAGE;
        for i in 0..zip.len() {
            let Ok(mut entry) = zip.by_index(i) else { continue };
            let name = entry.name().to_string();
            if !wanted(&name) {
                continue;
            }
            // One byte over the allowance tells a file that is too large from one that fits.
            let allowance = left.min(MAX_ENTRY);
            let mut buf = Vec::new();
            (&mut entry).take(allowance.saturating_add(1)).read_to_end(&mut buf).map_err(|e| PagesError::Invalid(format!("{name}: {e}")))?;
            let read = buf.len() as u64;
            if read > allowance {
                return Err(PagesError::Invalid(format!("{name}: the package is too large to open")));
            }
            left -= read;
            files.push((name, buf));
        }
        Package::from_files(files)
    }

    /// A document from its files, named by their path in the package (`Index/Document.iwa`).
    pub fn from_files(files: Vec<(String, Vec<u8>)>) -> Result<Package> {
        let mut pkg = Package { archives: Vec::new(), objects: HashMap::new(), data: HashMap::new(), data_names: HashMap::new() };
        let mut expanded = 0usize;
        for (name, buf) in files {
            if name == "Index.zip" {
                // Pages 5.0 kept the archives in a ZIP of their own.
                return Err(PagesError::Unsupported(
                    "documents saved by Pages 5.0 (Index.zip); open and save the document in a newer Pages first".into(),
                ));
            }
            if name.starts_with("Index/") && name.ends_with(".iwa") {
                let raw =
                    snappy::unframe(&buf, MAX_OBJECTS_BYTES.saturating_sub(expanded)).map_err(|e| PagesError::Invalid(format!("{name}: {e}")))?;
                expanded += raw.len();
                if !pkg.add_archive(raw) {
                    return Err(PagesError::Invalid(format!("{name}: a damaged object record")));
                }
            } else if let Some(file) = name.strip_prefix("Data/") {
                pkg.data.insert(file.to_string(), buf);
            }
        }
        if pkg.objects.is_empty() {
            return Err(PagesError::NotPages("no Index archives (an iWork '09 or older document?)".into()));
        }
        pkg.read_data_names();
        Ok(pkg)
    }

    /// Adds the objects of an unpacked archive; false when a record is damaged (the package is
    /// then rejected rather than opened with part of its objects).
    fn add_archive(&mut self, raw: Vec<u8>) -> bool {
        let a = self.archives.len();
        let mut p = 0usize;
        while p < raw.len() {
            let Some((len, start)) = varint(&raw, p) else { return false };
            let Some(end) = usize::try_from(len).ok().and_then(|l| start.checked_add(l)) else { return false };
            let Some(info) = raw.get(start..end).map(Msg) else { return false };
            p = end;
            let Some(id) = info.uint(1) else { return false };
            // An object may carry several messages (patches over the first); the first is the object.
            let mut first = true;
            for mi in info.msgs(2) {
                let Some(len) = mi.uint(3).and_then(|l| usize::try_from(l).ok()) else { return false };
                let Some(range_end) = p.checked_add(len).filter(|&e| e <= raw.len()) else { return false };
                if first && let Some(ty) = mi.uint(1).and_then(|t| u32::try_from(t).ok()) {
                    self.objects.entry(id).or_insert((ty, a, p..range_end));
                }
                first = false;
                p = range_end;
            }
        }
        self.archives.push(raw);
        true
    }

    fn read_data_names(&mut self) {
        let metas: Vec<u64> = self.objects.iter().filter(|(_, o)| o.0 == PACKAGE_METADATA).map(|(id, _)| *id).collect();
        let mut names = HashMap::new();
        for id in metas {
            let Some(meta) = self.object(id) else { continue };
            for d in meta.msgs(4) {
                let Some(did) = d.uint(1) else { continue };
                let shown = d.str(3).unwrap_or_default().to_string();
                let file = d.str(4).filter(|s| !s.is_empty()).map_or_else(|| shown.clone(), str::to_string);
                names.insert(did, (file, shown));
            }
        }
        self.data_names = names;
    }

    /// The type of object `id`.
    pub fn type_of(&self, id: u64) -> Option<u32> {
        self.objects.get(&id).map(|o| o.0)
    }

    /// The message of object `id`.
    pub fn object(&self, id: u64) -> Option<Msg<'_>> {
        let (_, a, r) = self.objects.get(&id)?;
        self.archives.get(*a)?.get(r.clone()).map(Msg)
    }

    /// The message of object `id` when it has type `ty`.
    pub fn typed(&self, id: u64, ty: u32) -> Option<Msg<'_>> {
        if self.type_of(id)? == ty { self.object(id) } else { None }
    }

    /// The bytes and display name of data file `id`.
    pub fn data(&self, id: u64) -> Option<(&[u8], &str)> {
        let (file, shown) = self.data_names.get(&id)?;
        let bytes = self.data.get(file).or_else(|| self.data.get(shown))?;
        Some((bytes, if shown.is_empty() { file } else { shown }))
    }
}
