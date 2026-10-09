use std::io::{Cursor, Read};

use crate::ImportError;

const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4_096;
const MAX_CENTRAL_DIRECTORY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_XML_PART_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ArchiveLimits {
    archive_bytes: usize,
    entries: usize,
    central_directory_bytes: u64,
    entry_bytes: u64,
    expanded_bytes: u64,
}

const ARCHIVE_LIMITS: ArchiveLimits = ArchiveLimits {
    archive_bytes: MAX_ARCHIVE_BYTES,
    entries: MAX_ARCHIVE_ENTRIES,
    central_directory_bytes: MAX_CENTRAL_DIRECTORY_BYTES,
    entry_bytes: MAX_ENTRY_BYTES,
    expanded_bytes: MAX_EXPANDED_BYTES,
};

pub(crate) fn open(bytes: &[u8]) -> Result<zip::ZipArchive<Cursor<&[u8]>>, ImportError> {
    open_with_limits(bytes, ARCHIVE_LIMITS)
}

fn open_with_limits(bytes: &[u8], limits: ArchiveLimits) -> Result<zip::ZipArchive<Cursor<&[u8]>>, ImportError> {
    if bytes.len() > limits.archive_bytes {
        return Err(ImportError::Corrupt(format!("file exceeds the {}-byte size limit", limits.archive_bytes)));
    }
    designcraft_archive::preflight(
        bytes,
        designcraft_archive::Limits { max_entries: limits.entries as u64, max_central_directory_bytes: limits.central_directory_bytes },
    )
    .map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    validate(&mut zip, limits)?;
    Ok(zip)
}

fn validate(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, limits: ArchiveLimits) -> Result<(), ImportError> {
    if zip.len() > limits.entries {
        return Err(ImportError::Corrupt(format!("archive contains more than {} entries", limits.entries)));
    }
    let mut expanded = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| ImportError::Corrupt(e.to_string()))?;
        let size = f.size();
        if size > limits.entry_bytes {
            return Err(ImportError::Corrupt(format!("archive entry exceeds the {}-byte limit", limits.entry_bytes)));
        }
        expanded = expanded.checked_add(size).ok_or_else(|| ImportError::Corrupt("archive expanded size is too large".into()))?;
        if expanded > limits.expanded_bytes {
            return Err(ImportError::Corrupt(format!("archive expands beyond the {}-byte limit", limits.expanded_bytes)));
        }
    }
    Ok(())
}

pub(crate) fn part(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<String>, ImportError> {
    part_capped(zip, name, MAX_XML_PART_BYTES)
}

fn part_capped(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, max_bytes: usize) -> Result<Option<String>, ImportError> {
    let mut f = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(ImportError::Corrupt(e.to_string())),
    };
    if f.size() > max_bytes as u64 {
        return Err(ImportError::Corrupt(format!("{name} exceeds the {max_bytes}-byte XML part limit")));
    }
    let capacity = usize::try_from(f.size()).unwrap_or(max_bytes).min(max_bytes);
    let mut s = String::with_capacity(capacity);
    (&mut f).take(max_bytes.saturating_add(1) as u64).read_to_string(&mut s).map_err(|e| ImportError::Corrupt(format!("can't read {name}: {e}")))?;
    if s.len() > max_bytes {
        return Err(ImportError::Corrupt(format!("{name} exceeds the {max_bytes}-byte XML part limit")));
    }
    Ok(Some(s))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (name, body) in entries {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(body).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn archive_resource_limits_cover_count_entry_and_total_sizes() {
        let bytes = archive(&[("one", b"123"), ("two", b"456")]);
        let limits = |entries, entry_bytes, expanded_bytes| ArchiveLimits { entries, entry_bytes, expanded_bytes, ..ARCHIVE_LIMITS };

        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate(&mut z, limits(1, 10, 10)).is_err());
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate(&mut z, limits(2, 2, 10)).is_err());
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate(&mut z, limits(2, 10, 5)).is_err());
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate(&mut z, limits(2, 3, 6)).is_ok());
    }

    #[test]
    fn xml_parts_are_read_only_to_the_configured_boundary() {
        let body = vec![b' '; 64 * 1024];
        let bytes = archive(&[("word/document.xml", &body)]);
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(part_capped(&mut z, "word/document.xml", 4 * 1024).is_err());
        assert_eq!(part_capped(&mut z, "word/document.xml", 64 * 1024).unwrap().map(|s| s.len()), Some(64 * 1024));
        assert_eq!(part_capped(&mut z, "word/missing.xml", 4 * 1024).unwrap(), None);
    }

    #[test]
    fn open_rejects_entry_count_during_preflight() {
        let bytes = archive(&[("one", b""), ("two", b"")]);
        let limits = ArchiveLimits { entries: 1, ..ARCHIVE_LIMITS };
        assert!(matches!(open_with_limits(&bytes, limits), Err(ImportError::Corrupt(message)) if message.contains("entries")));
    }
}
