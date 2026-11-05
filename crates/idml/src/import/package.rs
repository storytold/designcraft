use super::{HashMap, IdmlError, Read, Result};

/// Most entries an IDML package may hold.
const MAX_ENTRIES: usize = 100_000;

/// Most bytes one package part may expand to.
const MAX_PART_BYTES: u64 = 512 << 20;

/// Most bytes all the parts read from one package may expand to together.
const MAX_TOTAL_BYTES: u64 = 2 << 30;

/// An IDML package whose parts are decompressed only when the import asks for them, within
/// byte budgets that don't trust the sizes the archive claims.
pub(super) struct Package<'a> {
    zip: zip::ZipArchive<std::io::Cursor<&'a [u8]>>,
    /// Normalised part name → entry name, first entry wins (zip order, so the choice is stable).
    lenient: HashMap<String, String>,
    /// Bytes the parts still to be read may expand to.
    left: u64,
}

impl<'a> Package<'a> {
    pub(super) fn open(bytes: &'a [u8]) -> Result<Self> {
        let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
        if zip.len() > MAX_ENTRIES {
            return Err(IdmlError::NotIdml(format!("{} entries (at most {MAX_ENTRIES})", zip.len())));
        }
        let mut lenient: HashMap<String, String> = HashMap::new();
        for i in 0..zip.len() {
            let Some(name) = zip.name_for_index(i) else { continue };
            if name.ends_with('/') {
                continue;
            }
            lenient.entry(normalize_part_name(name)).or_insert_with(|| name.to_string());
        }
        Ok(Package { zip, lenient, left: MAX_TOTAL_BYTES })
    }

    /// The bytes of part `name` (matched exactly, then leniently), or `None` if the package has
    /// no such part.
    pub(super) fn part(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let exact = self.zip.index_for_name(name).filter(|_| !name.ends_with('/'));
        let Some(i) = exact.or_else(|| self.lenient.get(&normalize_part_name(name)).and_then(|n| self.zip.index_for_name(n))) else {
            return Ok(None);
        };
        let f = self.zip.by_index(i).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
        let part = f.name().to_string();
        let limit = MAX_PART_BYTES.min(self.left);
        let too_big = || IdmlError::Part {
            part: part.clone(),
            msg: format!("larger than the {MAX_PART_BYTES}-byte part limit (or the package is larger than {MAX_TOTAL_BYTES} bytes)"),
        };
        if f.size() > limit {
            return Err(too_big());
        }
        let mut buf = Vec::new();
        f.take(limit.saturating_add(1)).read_to_end(&mut buf).map_err(|e| IdmlError::Part { part: part.clone(), msg: e.to_string() })?;
        let n = buf.len() as u64;
        if n > limit {
            return Err(too_big());
        }
        self.left = self.left.saturating_sub(n);
        Ok(Some(buf))
    }
}

/// A package part name in the form the lookup compares: InDesign and other IDML writers differ
/// in percent-encoding, case, path separators and a leading `./` or `/`.
fn normalize_part_name(name: &str) -> String {
    let decoded = percent_decode(&name.replace('\\', "/"));
    let mut rest = decoded.as_str();
    loop {
        if let Some(r) = rest.strip_prefix("./") {
            rest = r;
        } else if let Some(r) = rest.strip_prefix('/') {
            rest = r;
        } else {
            break;
        }
    }
    rest.to_lowercase()
}

/// Decode `%XX` escapes; anything else (including a malformed escape) is kept as written.
pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%'
            && let (Some(h), Some(l)) = (b.get(i + 1).and_then(|h| (*h as char).to_digit(16)), b.get(i + 2).and_then(|l| (*l as char).to_digit(16)))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// IDML link URI → file system path (`file:/a%20b` → `/a b`, `file:///C:/x` → `C:/x`). A URI
/// naming another computer keeps it as a network path (`file://host/s/x` → `//host/s/x`).
pub(crate) fn uri_to_path(uri: &str) -> String {
    let rest = match uri.strip_prefix("file://") {
        Some(rest) => {
            let host_end = rest.find(['/', '\\']).unwrap_or(rest.len());
            let host = rest.get(..host_end).unwrap_or_default();
            if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
                rest.get(host_end..).unwrap_or(rest)
            } else if matches!(host.as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic()) {
                rest
            } else {
                return format!("//{}", percent_decode(rest));
            }
        }
        None => uri.strip_prefix("file:").unwrap_or(uri),
    };
    // `file:///C:/…` → `/C:/…` → `C:/…`
    let rest = match rest.as_bytes() {
        [b'/', _, b':', _, ..] => rest.get(1..).unwrap_or(rest),
        _ => rest,
    };
    percent_decode(rest)
}
