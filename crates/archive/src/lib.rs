//! Allocation-free ZIP end-record and central-directory preflight.
//!
//! `zip::ZipArchive` necessarily trusts enough end-record metadata to find and index the central
//! directory. Importers call [`preflight`] first so hostile entry counts and directory sizes are
//! rejected before that indexing work begins.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const ZIP64_EOCD_SIGNATURE: &[u8; 4] = b"PK\x06\x06";
const ZIP64_LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
const CENTRAL_FILE_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
const CENTRAL_DIGITAL_SIGNATURE: &[u8; 4] = b"PK\x05\x05";
const EOCD_LEN: usize = 22;
const ZIP64_LOCATOR_LEN: usize = 20;
const CENTRAL_FILE_LEN: usize = 46;
const MAX_COMMENT_LEN: usize = u16::MAX as usize;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_entries: u64,
    pub max_central_directory_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Directory {
    pub entries: u64,
    pub central_directory_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingEndRecord,
    Invalid(&'static str),
    TooManyEntries { actual: u64, limit: u64 },
    CentralDirectoryTooLarge { actual: u64, limit: u64 },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEndRecord => f.write_str("ZIP end record is missing"),
            Self::Invalid(message) => write!(f, "invalid ZIP structure: {message}"),
            Self::TooManyEntries { actual, limit } => write!(f, "ZIP contains {actual} entries; the limit is {limit}"),
            Self::CentralDirectoryTooLarge { actual, limit } => {
                write!(f, "ZIP central directory is {actual} bytes; the limit is {limit}")
            }
        }
    }
}

impl std::error::Error for Error {}

fn bytes_at(bytes: &[u8], at: usize, len: usize) -> Option<&[u8]> {
    bytes.get(at..at.checked_add(len)?)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes_at(bytes, at, 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes_at(bytes, at, 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes_at(bytes, at, 8)?.try_into().ok()?))
}

fn signature_at(bytes: &[u8], at: usize, signature: &[u8; 4]) -> bool {
    bytes_at(bytes, at, signature.len()) == Some(signature)
}

/// Validate the ZIP end records and walk the declared central directory without allocating.
///
/// Multi-disk archives and prefixed/self-extracting archives are intentionally rejected. Neither
/// is a document format supported by DesignCraft, and accepting their relative offsets would make
/// the preflight disagree with the bytes later indexed by the ZIP reader.
pub fn preflight(bytes: &[u8], limits: Limits) -> Result<Directory, Error> {
    if bytes.len() < EOCD_LEN {
        return Err(Error::MissingEndRecord);
    }
    let first = bytes.len().saturating_sub(EOCD_LEN.saturating_add(MAX_COMMENT_LEN));
    let last = bytes.len() - EOCD_LEN;
    let mut candidate_error = None;
    for eocd_at in (first..=last).rev() {
        if !signature_at(bytes, eocd_at, EOCD_SIGNATURE) {
            continue;
        }
        let Some(comment_len) = u16_at(bytes, eocd_at + 20).map(usize::from) else { continue };
        let Some(end) = eocd_at.checked_add(EOCD_LEN).and_then(|v| v.checked_add(comment_len)) else { continue };
        if end != bytes.len() {
            continue;
        }
        match preflight_at(bytes, eocd_at, limits) {
            Ok(directory) => return Ok(directory),
            Err(e @ Error::TooManyEntries { .. } | e @ Error::CentralDirectoryTooLarge { .. }) => return Err(e),
            Err(e) => candidate_error = Some(e),
        }
    }
    Err(candidate_error.unwrap_or(Error::MissingEndRecord))
}

fn preflight_at(bytes: &[u8], eocd_at: usize, limits: Limits) -> Result<Directory, Error> {
    let disk = u16_at(bytes, eocd_at + 4).ok_or(Error::Invalid("truncated end record"))?;
    let directory_disk = u16_at(bytes, eocd_at + 6).ok_or(Error::Invalid("truncated end record"))?;
    let disk_entries = u16_at(bytes, eocd_at + 8).ok_or(Error::Invalid("truncated end record"))?;
    let entries = u16_at(bytes, eocd_at + 10).ok_or(Error::Invalid("truncated end record"))?;
    let directory_size = u32_at(bytes, eocd_at + 12).ok_or(Error::Invalid("truncated end record"))?;
    let directory_offset = u32_at(bytes, eocd_at + 16).ok_or(Error::Invalid("truncated end record"))?;
    let locator_at = eocd_at.checked_sub(ZIP64_LOCATOR_LEN).filter(|at| signature_at(bytes, *at, ZIP64_LOCATOR_SIGNATURE));
    let needs_zip64 = disk == u16::MAX
        || directory_disk == u16::MAX
        || disk_entries == u16::MAX
        || entries == u16::MAX
        || directory_size == u32::MAX
        || directory_offset == u32::MAX;

    let (entries, directory_size, directory_offset, structure_at) = match locator_at {
        Some(locator_at) => {
            let locator_disk = u32_at(bytes, locator_at + 4).ok_or(Error::Invalid("truncated ZIP64 locator"))?;
            let zip64_offset = u64_at(bytes, locator_at + 8).ok_or(Error::Invalid("truncated ZIP64 locator"))?;
            let disks = u32_at(bytes, locator_at + 16).ok_or(Error::Invalid("truncated ZIP64 locator"))?;
            if locator_disk != 0 || disks != 1 {
                return Err(Error::Invalid("multi-disk ZIP64 archive"));
            }
            let zip64_at = usize::try_from(zip64_offset).map_err(|_| Error::Invalid("ZIP64 end-record offset does not fit this platform"))?;
            if !signature_at(bytes, zip64_at, ZIP64_EOCD_SIGNATURE) {
                return Err(Error::Invalid("ZIP64 end record is missing at its declared offset"));
            }
            let record_size = u64_at(bytes, zip64_at + 4).ok_or(Error::Invalid("truncated ZIP64 end record"))?;
            if record_size < 44 {
                return Err(Error::Invalid("ZIP64 end record is too short"));
            }
            let record_end = u64::try_from(zip64_at)
                .ok()
                .and_then(|at| at.checked_add(12))
                .and_then(|at| at.checked_add(record_size))
                .ok_or(Error::Invalid("ZIP64 end-record length overflow"))?;
            if record_end != u64::try_from(locator_at).map_err(|_| Error::Invalid("ZIP64 locator offset does not fit"))? {
                return Err(Error::Invalid("ZIP64 end record does not end at its locator"));
            }
            let zip64_disk = u32_at(bytes, zip64_at + 16).ok_or(Error::Invalid("truncated ZIP64 end record"))?;
            let zip64_directory_disk = u32_at(bytes, zip64_at + 20).ok_or(Error::Invalid("truncated ZIP64 end record"))?;
            let zip64_disk_entries = u64_at(bytes, zip64_at + 24).ok_or(Error::Invalid("truncated ZIP64 end record"))?;
            let zip64_entries = u64_at(bytes, zip64_at + 32).ok_or(Error::Invalid("truncated ZIP64 end record"))?;
            let zip64_directory_size = u64_at(bytes, zip64_at + 40).ok_or(Error::Invalid("truncated ZIP64 end record"))?;
            let zip64_directory_offset = u64_at(bytes, zip64_at + 48).ok_or(Error::Invalid("truncated ZIP64 end record"))?;
            if zip64_disk != 0 || zip64_directory_disk != 0 || zip64_disk_entries != zip64_entries {
                return Err(Error::Invalid("multi-disk ZIP64 archive"));
            }
            if (disk != u16::MAX && u32::from(disk) != zip64_disk)
                || (directory_disk != u16::MAX && u32::from(directory_disk) != zip64_directory_disk)
                || (disk_entries != u16::MAX && u64::from(disk_entries) != zip64_disk_entries)
                || (entries != u16::MAX && u64::from(entries) != zip64_entries)
                || (directory_size != u32::MAX && u64::from(directory_size) != zip64_directory_size)
                || (directory_offset != u32::MAX && u64::from(directory_offset) != zip64_directory_offset)
            {
                return Err(Error::Invalid("classic and ZIP64 end records disagree"));
            }
            (zip64_entries, zip64_directory_size, zip64_directory_offset, zip64_at)
        }
        None if needs_zip64 => return Err(Error::Invalid("ZIP64 sentinel without a ZIP64 locator")),
        None => {
            if disk != 0 || directory_disk != 0 || disk_entries != entries {
                return Err(Error::Invalid("multi-disk ZIP archive"));
            }
            (u64::from(entries), u64::from(directory_size), u64::from(directory_offset), eocd_at)
        }
    };

    if entries > limits.max_entries {
        return Err(Error::TooManyEntries { actual: entries, limit: limits.max_entries });
    }
    if directory_size > limits.max_central_directory_bytes {
        return Err(Error::CentralDirectoryTooLarge { actual: directory_size, limit: limits.max_central_directory_bytes });
    }
    let directory_end = directory_offset.checked_add(directory_size).ok_or(Error::Invalid("central-directory offset overflow"))?;
    if directory_end != u64::try_from(structure_at).map_err(|_| Error::Invalid("end-record offset does not fit"))? {
        return Err(Error::Invalid("central-directory offset and size do not reach the end records"));
    }
    let start = usize::try_from(directory_offset).map_err(|_| Error::Invalid("central-directory offset does not fit this platform"))?;
    let end = usize::try_from(directory_end).map_err(|_| Error::Invalid("central-directory end does not fit this platform"))?;
    validate_central_directory(bytes, start, end, entries)?;
    Ok(Directory { entries, central_directory_bytes: directory_size })
}

fn validate_central_directory(bytes: &[u8], start: usize, end: usize, entries: u64) -> Result<(), Error> {
    if end > bytes.len() || start > end {
        return Err(Error::Invalid("central directory is outside the archive"));
    }
    let mut at = start;
    for _ in 0..entries {
        if !signature_at(bytes, at, CENTRAL_FILE_SIGNATURE) {
            return Err(Error::Invalid("central-directory entry signature is missing"));
        }
        let name = usize::from(u16_at(bytes, at + 28).ok_or(Error::Invalid("truncated central-directory entry"))?);
        let extra = usize::from(u16_at(bytes, at + 30).ok_or(Error::Invalid("truncated central-directory entry"))?);
        let comment = usize::from(u16_at(bytes, at + 32).ok_or(Error::Invalid("truncated central-directory entry"))?);
        let disk = u16_at(bytes, at + 34).ok_or(Error::Invalid("truncated central-directory entry"))?;
        if disk != 0 {
            return Err(Error::Invalid("central-directory entry refers to another disk"));
        }
        at = at
            .checked_add(CENTRAL_FILE_LEN)
            .and_then(|v| v.checked_add(name))
            .and_then(|v| v.checked_add(extra))
            .and_then(|v| v.checked_add(comment))
            .ok_or(Error::Invalid("central-directory entry length overflow"))?;
        if at > end {
            return Err(Error::Invalid("central-directory entry exceeds the declared directory size"));
        }
    }
    if at < end && signature_at(bytes, at, CENTRAL_DIGITAL_SIGNATURE) {
        let signature_len = usize::from(u16_at(bytes, at + 4).ok_or(Error::Invalid("truncated central-directory signature"))?);
        at = at.checked_add(6).and_then(|v| v.checked_add(signature_len)).ok_or(Error::Invalid("central-directory signature length overflow"))?;
    }
    if at != end {
        return Err(Error::Invalid("central-directory entry count and size disagree"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_LIMITS: Limits = Limits { max_entries: 8, max_central_directory_bytes: 1024 };

    fn one_entry(comment: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&[0; 22]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.push(b'a');
        let directory_at = out.len() as u32;
        out.extend_from_slice(CENTRAL_FILE_SIGNATURE);
        out.extend_from_slice(&[0; 24]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.push(b'a');
        out.extend_from_slice(EOCD_SIGNATURE);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&47u32.to_le_bytes());
        out.extend_from_slice(&directory_at.to_le_bytes());
        out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        out.extend_from_slice(comment);
        out
    }

    fn empty_zip64(entries: u64, directory_size: u64, directory_offset: u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(ZIP64_EOCD_SIGNATURE);
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&45u16.to_le_bytes());
        out.extend_from_slice(&45u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&entries.to_le_bytes());
        out.extend_from_slice(&entries.to_le_bytes());
        out.extend_from_slice(&directory_size.to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(ZIP64_LOCATOR_SIGNATURE);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(EOCD_SIGNATURE);
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn classic_directory_and_comment_are_validated() {
        let bytes = one_entry(b"comment with PK\x05\x06 inside");
        assert_eq!(preflight(&bytes, TEST_LIMITS).unwrap(), Directory { entries: 1, central_directory_bytes: 47 });
        assert!(matches!(preflight(&bytes, Limits { max_entries: 0, ..TEST_LIMITS }), Err(Error::TooManyEntries { actual: 1, limit: 0 })));
        assert!(matches!(
            preflight(&bytes, Limits { max_central_directory_bytes: 46, ..TEST_LIMITS }),
            Err(Error::CentralDirectoryTooLarge { actual: 47, limit: 46 })
        ));

        let mut multiple_disks = bytes.clone();
        let eocd = bytes.len() - EOCD_LEN - b"comment with PK\x05\x06 inside".len();
        multiple_disks[eocd + 4..eocd + 6].copy_from_slice(&1u16.to_le_bytes());
        assert!(matches!(preflight(&multiple_disks, TEST_LIMITS), Err(Error::Invalid("multi-disk ZIP archive"))));

        let mut inconsistent_offset = bytes.clone();
        inconsistent_offset[eocd + 16..eocd + 20].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            preflight(&inconsistent_offset, TEST_LIMITS),
            Err(Error::Invalid("central-directory offset and size do not reach the end records"))
        ));
    }

    #[test]
    fn zip64_sentinels_locator_and_multidisk_fields_are_checked() {
        let bytes = empty_zip64(0, 0, 0);
        assert_eq!(preflight(&bytes, TEST_LIMITS).unwrap(), Directory { entries: 0, central_directory_bytes: 0 });

        let mut multiple_disks = bytes.clone();
        multiple_disks[72..76].copy_from_slice(&2u32.to_le_bytes());
        assert!(matches!(preflight(&multiple_disks, TEST_LIMITS), Err(Error::Invalid("multi-disk ZIP64 archive"))));

        let overflowing = empty_zip64(0, 1, u64::MAX);
        assert!(matches!(preflight(&overflowing, TEST_LIMITS), Err(Error::Invalid("central-directory offset overflow"))));
    }
}
