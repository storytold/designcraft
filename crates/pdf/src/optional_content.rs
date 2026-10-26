use hayro_syntax::Pdf;
use hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::reader::{Reader, ReaderExt};
use std::sync::Arc;

use crate::{PdfError, Result};

const MARKER: &[u8] = b"DesignCraftHiddenOCG";

fn invalid() -> PdfError {
    PdfError::Write("the placed PDF's optional-content state could not be preserved safely".into())
}

/// A layer hidden in Object Layer Options that the placed file doesn't have (relinked, say):
/// exporting would be a guess at what was meant to stay hidden.
fn missing_layer() -> PdfError {
    PdfError::Write("a layer hidden in Object Layer Options is not in the placed PDF; relink it or reset its layer options".into())
}

fn references(dict: &Dict<'_>, key: &[u8]) -> Result<Vec<ObjectIdentifier>> {
    if !dict.contains_key(key) {
        return Ok(Vec::new());
    }
    dict.get::<Array<'_>>(key).ok_or_else(invalid)?.raw_iter().map(|value| value.as_obj_ref().map(Into::into).ok_or_else(invalid)).collect()
}

fn edited(dict: &Dict<'_>, replacements: &[(&[u8], &[u8])]) -> Result<Vec<u8>> {
    let data = dict.data();
    let body = data.strip_prefix(b"<<").and_then(|d| d.strip_suffix(b">>")).ok_or_else(invalid)?;
    let mut reader = Reader::new(body);
    let mut out = b"<<".to_vec();
    loop {
        reader.skip_white_spaces_and_comments();
        if reader.at_end() {
            break;
        }
        let start = reader.offset();
        let name = reader.read_without_context::<Name<'_>>().ok_or_else(invalid)?;
        reader.skip_white_spaces_and_comments();
        reader.skip::<MaybeRef<Object<'_>>>(false).ok_or_else(invalid)?;
        if !replacements.iter().any(|(key, _)| *key == name.as_ref()) {
            out.push(b'\n');
            out.extend_from_slice(body.get(start..reader.offset()).ok_or_else(invalid)?);
        }
    }
    for (key, value) in replacements {
        if !value.is_empty() {
            out.extend_from_slice(b"\n/");
            out.extend_from_slice(key);
            out.push(b' ');
            out.extend_from_slice(value);
        }
    }
    out.extend_from_slice(b"\n>>");
    Ok(out)
}

/// The trailer of the cross-reference section at `offset`: a table's `trailer` dictionary or an
/// xref stream's own. A table is read entry by entry rather than searched for the keyword, so a
/// `trailer` in a comment can't stand in for the real one; reading is bounded either way.
fn trailer_at(bytes: &[u8], offset: usize) -> Option<Dict<'_>> {
    const MAX_SECTION_BYTES: usize = 4 * 1024 * 1024;
    let data = bytes.get(offset..)?;
    let mut reader = Reader::new(data.get(..data.len().min(MAX_SECTION_BYTES))?);
    reader.skip_white_spaces_and_comments();
    if reader.forward_tag(b"xref").is_none() {
        reader.read_without_context::<ObjectIdentifier>()?;
        reader.skip_white_spaces_and_comments();
        let stream = reader.read_without_context::<Stream<'_>>()?;
        let is_xref = stream.dict().get::<Name<'_>>(b"Type").as_ref().map(AsRef::as_ref) == Some(b"XRef");
        return is_xref.then(|| stream.dict().clone());
    }
    // Every pass consumes input or fails, and the input is bounded, so this ends.
    loop {
        reader.skip_white_spaces();
        if reader.forward_tag(b"trailer").is_some() {
            break;
        }
        reader.read_without_context::<u32>()?;
        reader.skip_white_spaces();
        let count = reader.read_without_context::<usize>()?;
        if count > MAX_SECTION_BYTES / 18 {
            return None;
        }
        for _ in 0..count {
            reader.skip_white_spaces();
            reader.read_without_context::<u32>()?;
            reader.skip_white_spaces();
            if reader.read_without_context::<u32>()? > 65535 {
                return None;
            }
            reader.skip_white_spaces();
            reader.forward_tag(b"n").or_else(|| reader.forward_tag(b"f"))?;
        }
    }
    reader.skip_white_spaces_and_comments();
    reader.read_without_context::<Dict<'_>>()
}

/// Whether `dict` has `key`: a literal `null` value is the same as no entry. A reference counts,
/// since a trailer has no context to resolve it in.
fn has_entry(dict: &Dict<'_>, key: &[u8]) -> bool {
    dict.contains_key(key) && !matches!(dict.get_raw::<Object<'_>>(key), Some(MaybeRef::NotRef(Object::Null(_))))
}

fn append(bytes: &[u8], changes: &[(ObjectIdentifier, Vec<u8>)], root: ObjectIdentifier) -> Result<Vec<u8>> {
    let damaged = || PdfError::Write("the placed PDF's cross-reference table is damaged; layer changes could not be preserved safely".into());
    let at = crate::transitions::rfind(bytes, b"startxref").ok_or_else(damaged)?;
    let tail = std::str::from_utf8(bytes.get(at..).ok_or_else(damaged)?).map_err(|_| damaged())?;
    let prev = crate::transitions::int_after(tail, "startxref").ok_or_else(damaged)?;
    let trailer = trailer_at(bytes, prev).ok_or_else(damaged)?;
    // Never write decrypted objects into an encrypted file. An earlier update's trailer can
    // carry the encryption, so follow the (bounded, acyclic) Prev chain to its end.
    let mut offset = Some(prev);
    let mut visited = std::collections::HashSet::new();
    for _ in 0..32 {
        let Some(at) = offset else { break };
        if !visited.insert(at) {
            return Err(damaged());
        }
        let section = trailer_at(bytes, at).ok_or_else(damaged)?;
        if has_entry(&section, b"Encrypt") {
            return Err(invalid());
        }
        offset = section.get::<usize>(b"Prev");
        if offset.is_none() && has_entry(&section, b"Prev") {
            return Err(damaged());
        }
    }
    if offset.is_some() {
        return Err(damaged());
    }
    trailer.get::<i32>(b"Size").filter(|n| *n > 0).ok_or_else(damaged)?;
    let mut out = bytes.to_vec();
    out.push(b'\n');
    let mut offsets = Vec::new();
    for (id, body) in changes {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{} {} obj\n", id.obj_number, id.gen_number).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n");
    offsets.sort_by_key(|(id, _)| *id);
    for (id, offset) in offsets {
        out.extend_from_slice(format!("{} 1\n{offset:010} {:05} n\r\n", id.obj_number, id.gen_number).as_bytes());
    }
    let root = format!("{} {} R", root.obj_number, root.gen_number);
    let prev = prev.to_string();
    let trailer = edited(
        &trailer,
        &[
            (b"Root", root.as_bytes()),
            (b"Prev", prev.as_bytes()),
            (b"XRefStm", b""),
            (b"Type", b""),
            (b"W", b""),
            (b"Index", b""),
            (b"Length", b""),
            (b"Filter", b""),
            (b"DecodeParms", b""),
        ],
    )?;
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(&trailer);
    out.extend_from_slice(format!("\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    Ok(out)
}

#[derive(Clone)]
pub(crate) struct Variant {
    pub data: Arc<Vec<u8>>,
    pub hidden: Vec<ObjectIdentifier>,
    pub warning: Option<String>,
    pub has_optional_content: bool,
}

/// The placed PDF with its layer choice applied: `None` keeps the file's own default
/// visibility, `Some(names)` shows every layer except `names` (so `Some(&[])` is all on).
pub(crate) fn prepare(bytes: &Arc<Vec<u8>>, choice: Option<&[String]>) -> Result<Variant> {
    let hidden = choice.unwrap_or_default();
    let pdf = Pdf::new(bytes.clone()).map_err(|_| invalid())?;
    let xref = pdf.xref();
    let root = xref.get::<Dict<'_>>(xref.root_id()).ok_or_else(invalid)?;
    // A null value, literal or referenced, is the same as no entry.
    if matches!(root.get::<Object<'_>>(b"OCProperties"), None | Some(Object::Null(_))) {
        let has_optional_content = pdf
            .objects()
            .into_iter()
            .any(|object| matches!(object, Object::Dict(dict) if dict.get::<Name<'_>>(b"Type").as_ref().map(AsRef::as_ref) == Some(b"OCG")));
        if !hidden.is_empty() {
            return Err(missing_layer());
        }
        return Ok(Variant { data: bytes.clone(), hidden: Vec::new(), warning: None, has_optional_content });
    }
    let properties = root.get::<Dict<'_>>(b"OCProperties").ok_or_else(invalid)?;
    let mut groups = references(&properties, b"OCGs")?;
    let config = properties.get::<Dict<'_>>(b"D");
    let mut fallback = config.is_none();
    let config = config.unwrap_or_default();
    let base_off = match config.get::<Name<'_>>(b"BaseState").as_ref().map(AsRef::as_ref) {
        None if !config.contains_key(b"BaseState") => false,
        Some(b"ON") => false,
        Some(b"OFF") => true,
        None | Some(_) => {
            fallback = true;
            false
        }
    };
    let off = references(&config, b"OFF")?;
    let on = match references(&config, b"ON") {
        Ok(on) => on,
        Err(error) if !fallback && !base_off && off.is_empty() => {
            let value = config.get::<Object<'_>>(b"ON").ok_or(error)?;
            fallback = true;
            value.into_array().map(|array| array.raw_iter().filter_map(|value| value.as_obj_ref().map(Into::into)).collect()).unwrap_or_default()
        }
        Err(error) => return Err(error),
    };
    if on.iter().any(|id| off.contains(id)) {
        return Err(invalid());
    }
    let incomplete = on.iter().chain(&off).any(|id| !groups.contains(id));
    groups.extend(on.iter().chain(&off).copied());
    groups.sort();
    groups.dedup();
    // Aggregating applications use the default state, not /AS usage-event adjustments.
    let mut changes = Vec::new();
    let mut effective = Vec::new();
    let mut renamed = false;
    let mut found = vec![false; hidden.len()];
    for id in groups {
        let group = xref.get::<Dict<'_>>(id).ok_or_else(invalid)?;
        if group.get::<Name<'_>>(b"Type").as_ref().map(AsRef::as_ref) != Some(b"OCG") {
            return Err(invalid());
        }
        let name = group.get::<hayro_syntax::object::String<'_>>(b"Name");
        let is_hidden = match (choice, name.as_ref()) {
            (None, _) => !fallback && (off.contains(&id) || (base_off && !on.contains(&id))),
            (Some([]), None) => false,
            (Some(_), Some(name)) => matches_hidden(hidden, &mut found, name.as_bytes()),
            (Some(_), None) => return Err(invalid()),
        };
        if is_hidden {
            effective.push(id);
        }
        // Unmarked groups are visible when registered in the output catalog. An explicit choice
        // writes every group's state, so the catalog rewrite is attempted (and a file whose
        // history cannot take one fails closed) even when the choice happens to change nothing.
        if !is_hidden && name.is_some() && choice.is_none() {
            continue;
        }
        let (body, generated) = layer_edit(&group, id, name.is_some(), is_hidden)?;
        renamed |= generated;
        changes.push((id, body));
    }
    if found.contains(&false) {
        return Err(missing_layer());
    }
    effective.sort();
    effective.dedup();
    let data = if changes.is_empty() { bytes.clone() } else { Arc::new(append(bytes, &changes, xref.root_id())?) };
    let warning = layer_warning(fallback, incomplete, renamed);
    Ok(Variant { data, hidden: effective, warning, has_optional_content: true })
}

/// Whether the layer called `name` is one of the `hidden` choices. Each choice it matches is
/// recorded in `found`, so a choice that names no layer can be reported.
fn matches_hidden(hidden: &[String], found: &mut [bool], name: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name);
    let mut is_hidden = false;
    for (n, found) in hidden.iter().zip(found) {
        if n == name.as_ref() {
            (*found, is_hidden) = (true, true);
        }
    }
    is_hidden
}

/// The rewritten group for a layer in the output: its marker records `is_hidden`, and a layer
/// without a readable name gets a generated one. The flag reports whether a name was generated.
fn layer_edit(group: &Dict<'_>, id: ObjectIdentifier, named: bool, is_hidden: bool) -> Result<(Vec<u8>, bool)> {
    let marker = is_hidden.to_string();
    if named {
        return Ok((edited(group, &[(MARKER, marker.as_bytes())])?, false));
    }
    if group.contains_key(b"Name") && group.get::<Object<'_>>(b"Name").is_none() {
        return Err(invalid());
    }
    let label = format!("(Placed PDF layer {} {})", id.obj_number, id.gen_number);
    Ok((edited(group, &[(MARKER, marker.as_bytes()), (b"Name", label.as_bytes())])?, true))
}

/// The warning a prepared variant carries, or `None` when the layers were read as written.
fn layer_warning(fallback: bool, incomplete: bool, renamed: bool) -> Option<String> {
    let mut warnings = Vec::new();
    if fallback {
        warnings.push("layer state not readable, all layers shown unless hidden by Object Layer Options");
    }
    if incomplete {
        warnings.push("layer list incomplete; layers named in the default configuration were added");
    }
    if renamed {
        warnings.push("layer display name not readable; generated a name, visibility preserved");
    }
    if warnings.is_empty() { None } else { Some(warnings.join("; ")) }
}

pub(crate) fn register(bytes: &[u8]) -> Result<Vec<u8>> {
    #[cfg(test)]
    crate::export::registration_tests::REGISTRATIONS.with(|count| count.set(count.get() + 1));
    let pdf = Pdf::new(bytes.to_vec()).map_err(|_| invalid())?;
    let mut groups = Vec::new();
    let mut changes = Vec::new();
    for object in pdf.objects() {
        let Object::Dict(dict) = object else { continue };
        if dict.get::<Name<'_>>(b"Type").as_ref().map(AsRef::as_ref) != Some(b"OCG") {
            continue;
        }
        // Resource-only OCGs without a catalog configuration are visible by default.
        let hidden = if dict.contains_key(MARKER) { dict.get::<bool>(MARKER).ok_or_else(invalid)? } else { false };
        let id = dict.obj_id().ok_or_else(invalid)?;
        groups.push((id, hidden));
        changes.push((id, edited(&dict, &[(MARKER, b"")])?));
    }
    if groups.is_empty() {
        return Ok(bytes.to_vec());
    }
    groups.sort_by_key(|(id, _)| *id);
    let refs = |hidden_only: bool| {
        groups
            .iter()
            .filter(|(_, hidden)| !hidden_only || *hidden)
            .map(|(id, _)| format!("{} {} R", id.obj_number, id.gen_number))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let all = refs(false);
    let properties = format!("<</OCGs[{all}]/D<</Name(Placed PDF layers)/BaseState/ON/OFF[{}]/Order[{all}]>>>>", refs(true));
    let root = pdf.xref().root_id();
    let catalog = pdf.xref().get::<Dict<'_>>(root).ok_or_else(invalid)?;
    changes.push((root, edited(&catalog, &[(b"OCProperties", properties.as_bytes())])?));
    append(bytes, &changes, root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_named_groups_keep_original_buffer_with_repaired_xref() {
        let bytes = Arc::new(
            b"%PDF-1.5\n1 0 obj\n<</Type/Catalog/Pages 2 0 R/OCProperties<</OCGs[4 0 R]/D<</BaseState/ON>>>>>>\nendobj\n\
              2 0 obj\n<</Type/Pages/Kids[3 0 R]/Count 1>>\nendobj\n\
              3 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<<>>>>\nendobj\n\
              4 0 obj\n<</Type/OCG/Name(Visible)>>\nendobj\n\
              trailer\n<</Size 5/Root 1 0 R>>\nstartxref\n1\n%%EOF\n"
                .to_vec(),
        );
        assert!(Pdf::new(bytes.clone()).is_ok());

        let variant = prepare(&bytes, None).unwrap();

        assert!(Arc::ptr_eq(&bytes, &variant.data), "visible groups must not copy the complete PDF or require incremental writes");
        assert!(variant.hidden.is_empty());
        assert!(variant.warning.is_none());
    }

    /// One page with layer "Off", hidden by the file's default configuration. `trailer` is the
    /// trailer dictionary; `previous` an earlier revision's trailer, chained by /Prev.
    fn default_hidden(trailer: &str, previous: Option<&str>) -> Arc<Vec<u8>> {
        let mut bytes = b"%PDF-1.5\n".to_vec();
        let mut offsets = Vec::new();
        for body in [
            "<</Type/Catalog/Pages 2 0 R/OCProperties<</OCGs[4 0 R]/D<</OFF[4 0 R]>>>>>>",
            "<</Type/Pages/Kids[3 0 R]/Count 1>>",
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<<>>>>",
            "<</Type/OCG/Name(Off)>>",
        ] {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", offsets.len()).as_bytes());
        }
        let table = |bytes: &mut Vec<u8>, trailer: &str| {
            let at = bytes.len();
            bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
            for offset in &offsets {
                bytes.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
            }
            bytes.extend_from_slice(format!("trailer\n{trailer}\n").as_bytes());
            at
        };
        let trailer = match previous {
            Some(previous) => {
                let at = table(&mut bytes, previous);
                trailer.replace(">>", &format!("/Prev {at}>>"))
            }
            None => trailer.to_owned(),
        };
        let at = table(&mut bytes, &trailer);
        bytes.extend_from_slice(format!("startxref\n{at}\n%%EOF\n").as_bytes());
        Arc::new(bytes)
    }

    #[test]
    fn an_explicit_choice_overrides_a_layer_the_file_hides() {
        let bytes = default_hidden("<</Size 5/Root 1 0 R>>", None);

        let defaults = prepare(&bytes, None).unwrap();
        let all_on = prepare(&bytes, Some(&[])).unwrap();

        let group = ObjectIdentifier::new(4, 0);
        assert_eq!(defaults.hidden, vec![group], "file defaults keep the layer hidden");
        assert!(all_on.hidden.is_empty(), "an explicit all-on choice shows it");
        let registered = register(&all_on.data).unwrap();
        let pdf = Pdf::new(registered).unwrap();
        let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
        let config = catalog.get::<Dict<'_>>(b"OCProperties").unwrap().get::<Dict<'_>>(b"D").unwrap();
        assert_eq!(references(&config, b"OFF").unwrap(), Vec::new());
    }

    /// The incremental writer alone, so a reader's own handling of the input can't decide the result.
    fn update(bytes: &[u8]) -> Result<Vec<u8>> {
        append(bytes, &[(ObjectIdentifier::new(4, 0), b"<</Type/OCG/Name(Off)>>".to_vec())], ObjectIdentifier::new(1, 0))
    }

    #[test]
    fn a_null_encrypt_entry_is_no_encryption() {
        for trailer in ["<</Size 5/Root 1 0 R/Encrypt null>>", "<</Size 5/Root 1 0 R>>"] {
            assert!(update(&default_hidden(trailer, None)).is_ok(), "{trailer}");
        }
        for trailer in ["<</Size 5/Root 1 0 R/Encrypt 9 0 R>>", "<</Size 5/Root 1 0 R/Encrypt<</Filter/Standard>>>>"] {
            assert!(update(&default_hidden(trailer, None)).is_err(), "{trailer}");
        }
        // End to end, a null entry still lets a layer choice be written.
        assert!(prepare(&default_hidden("<</Size 5/Root 1 0 R/Encrypt null>>", None), None).is_ok());
    }

    #[test]
    fn encryption_in_an_earlier_revision_is_found() {
        let bytes = default_hidden("<</Size 5/Root 1 0 R>>", Some("<</Size 5/Root 1 0 R/Encrypt 9 0 R>>"));
        assert!(update(&bytes).is_err());
        let bytes = default_hidden("<</Size 5/Root 1 0 R>>", Some("<</Size 5/Root 1 0 R/Encrypt null>>"));
        assert!(update(&bytes).is_ok());
    }

    #[test]
    fn a_trailer_in_a_comment_does_not_hide_the_real_one() {
        let bytes = b"xref\n% trailer <</Size 9/Encrypt null>>\n0 1\n0000000000 65535 f\r\ntrailer\n<</Size 9/Encrypt 12 0 R>>";
        assert!(trailer_at(bytes, 0).is_none(), "a comment inside the table is not table syntax");
        let bytes = b"xref\n0 1\n0000000000 65535 f\r\ntrailer\n<</Size 9/Encrypt 12 0 R>>";
        assert!(has_entry(&trailer_at(bytes, 0).unwrap(), b"Encrypt"));
    }

    #[test]
    fn an_unreadable_prev_is_damage_and_a_null_one_ends_the_chain() {
        for value in ["(12)", "/Twelve", "-1", "[12]", "<<>>", "true", "9 0 R"] {
            let bytes = default_hidden(&format!("<</Size 5/Root 1 0 R/Prev {value}>>"), None);
            let error = update(&bytes).unwrap_err().to_string();
            assert!(error.contains("cross-reference"), "{value}: {error}");
        }
        assert!(update(&default_hidden("<</Size 5/Root 1 0 R/Prev null>>", None)).is_ok());
    }

    #[test]
    fn a_cyclic_prev_chain_is_damage() {
        let mut bytes = default_hidden("<</Size 5/Root 1 0 R>>", None).to_vec();
        let at = crate::transitions::rfind(&bytes, b"xref\n").unwrap();
        let patched = String::from_utf8(bytes.clone()).unwrap().replacen("<</Size 5/Root 1 0 R>>", &format!("<</Size 5/Root 1 0 R/Prev {at}>>"), 1);
        bytes = patched.into_bytes();
        let error = update(&bytes).unwrap_err().to_string();
        assert!(error.contains("cross-reference"), "{error}");
    }
}
