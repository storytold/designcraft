//! Synthetic fonts for tests (here and in dependent crates, through the `testing` feature).

use skrifa::MetadataProvider;

/// A font named `family` (style Regular) that maps exactly `chars`, all to one glyph (`X`): the bundled
/// Source Sans 3 Regular with its `name` and `cmap` tables replaced. Stands in for a font that
/// isn't installed (a system CJK font on a machine without it). `None` only if the bundled font
/// can't be read.
pub fn font_with(family: &str, chars: &[char]) -> Option<Vec<u8>> {
    font_with_glyph(family, chars, 'X')
}

/// [`font_with`], drawing every character as Source Sans 3's `glyph` (two fonts of one name that
/// set the same text differently).
pub fn font_with_glyph(family: &str, chars: &[char], glyph: char) -> Option<Vec<u8>> {
    let glyphs: Vec<(char, char)> = chars.iter().map(|c| (*c, glyph)).collect();
    font_mapping(family, &glyphs)
}

/// [`font_with`], mapping each character of `glyphs` to Source Sans 3's glyph for the character
/// paired with it (`('一', 'X')` draws 一 as X), so characters can have glyphs (and metrics) of
/// their own. `None` if the bundled font can't be read or lacks a glyph asked for.
pub fn font_mapping(family: &str, glyphs: &[(char, char)]) -> Option<Vec<u8>> {
    let base = *crate::bundled().first()?;
    let base_cmap = skrifa::FontRef::new(base).ok()?.charmap();
    let mut map: Vec<(u32, u32)> = glyphs.iter().map(|(c, g)| Some((*c as u32, base_cmap.map(*g)?.to_u32()))).collect::<Option<_>>()?;
    map.sort_unstable();
    map.dedup_by_key(|m| m.0);
    let mut tables = tables_of(base)?;
    tables.retain(|(tag, _)| tag != b"name" && tag != b"cmap");

    // `name`: format 0, family (1), subfamily (2) and PostScript name (6), Windows Unicode English.
    let ps: String = family.chars().filter(char::is_ascii_alphanumeric).chain("-Regular".chars()).collect();
    let strings: Vec<Vec<u8>> = [family, "Regular", &ps].iter().map(|s| s.encode_utf16().flat_map(u16::to_be_bytes).collect()).collect();
    let mut name = Vec::new();
    for v in [0u16, 3, 6 + 3 * 12] {
        name.extend_from_slice(&v.to_be_bytes());
    }
    let mut at = 0usize;
    for (id, s) in [1u16, 2, 6].iter().zip(&strings) {
        for v in [3u16, 1, 0x409, *id, u16::try_from(s.len()).ok()?, u16::try_from(at).ok()?] {
            name.extend_from_slice(&v.to_be_bytes());
        }
        at += s.len();
    }
    strings.iter().for_each(|s| name.extend_from_slice(s));
    tables.push((*b"name", name));

    // `cmap`: one format 12 subtable (Windows, full Unicode), one group per character.
    let sub_len = u32::try_from(16 + 12 * map.len()).ok()?;
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 10] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&12u16.to_be_bytes());
    cmap.extend_from_slice(&0u16.to_be_bytes());
    for v in [sub_len, 0, u32::try_from(map.len()).ok()?] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }
    for (cp, gid) in map {
        for v in [cp, cp, gid] {
            cmap.extend_from_slice(&v.to_be_bytes());
        }
    }
    tables.push((*b"cmap", cmap));
    assemble(base, tables)
}

/// Every table of the font file `font`: (tag, bytes).
fn tables_of(font: &[u8]) -> Option<Vec<([u8; 4], Vec<u8>)>> {
    let be16 = |b: &[u8], at: usize| b.get(at..at + 2).and_then(|s| s.try_into().ok()).map(u16::from_be_bytes);
    let be32 = |b: &[u8], at: usize| b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes);
    let mut tables = Vec::new();
    for r in 0..usize::from(be16(font, 4)?) {
        let rec = 12 + r * 16;
        let tag: [u8; 4] = font.get(rec..rec + 4)?.try_into().ok()?;
        let (offset, len) = (be32(font, rec + 8)? as usize, be32(font, rec + 12)? as usize);
        tables.push((tag, font.get(offset..offset.checked_add(len)?)?.to_vec()));
    }
    Some(tables)
}

/// A font file of `tables`, with the sfnt version of `font`: the table directory (sorted by tag),
/// then the tables, each 4-byte aligned.
fn assemble(font: &[u8], mut tables: Vec<([u8; 4], Vec<u8>)>) -> Option<Vec<u8>> {
    tables.sort_by_key(|t| t.0);
    let n = u16::try_from(tables.len()).ok()?;
    let pow = 1u16 << (15 - n.leading_zeros().min(15));
    let mut out = font.get(0..4)?.to_vec();
    for v in [n, pow * 16, pow.trailing_zeros() as u16, n * 16 - pow * 16] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).ok()?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}

/// `font` with `tables` added, each replacing the table of its tag. `None` if `font` can't be read.
fn with_tables(font: &[u8], tables: Vec<([u8; 4], Vec<u8>)>) -> Option<Vec<u8>> {
    let mut all = tables_of(font)?;
    all.retain(|(tag, _)| !tables.iter().any(|(t, _)| t == tag));
    all.extend(tables);
    assemble(font, all)
}

/// `font` with vertical metrics: a `vhea` table and a `vmtx` table that give every glyph `default`
/// (advance down the line, top side bearing), except the glyphs `overrides` lists (glyph id,
/// advance, top side bearing). Every glyph has a long metric. `None` if `font` can't be read.
pub fn with_vmtx(font: &[u8], default: (u16, i16), overrides: &[(u16, u16, i16)]) -> Option<Vec<u8>> {
    use skrifa::raw::TableProvider;
    let glyphs = skrifa::FontRef::new(font).ok()?.maxp().ok()?.num_glyphs();
    let mut vmtx = Vec::with_capacity(usize::from(glyphs) * 4);
    let mut max_advance = 0;
    for gid in 0..glyphs {
        let (advance, tsb) = overrides.iter().find(|o| o.0 == gid).map_or(default, |o| (o.1, o.2));
        max_advance = max_advance.max(advance);
        vmtx.extend_from_slice(&advance.to_be_bytes());
        vmtx.extend_from_slice(&tsb.to_be_bytes());
    }
    // `vhea` 1.1: typo ascender, descender and line gap, advanceHeightMax, minTop/BottomSideBearing,
    // yMaxExtent, caret slope rise and run, caret offset, four reserved, metricDataFormat and
    // numOfLongVerMetrics (at byte 34).
    let mut vhea = 0x0001_1000_u32.to_be_bytes().to_vec();
    for v in [500i16, -500, 0] {
        vhea.extend_from_slice(&v.to_be_bytes());
    }
    vhea.extend_from_slice(&max_advance.to_be_bytes());
    for v in [0i16, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0] {
        vhea.extend_from_slice(&v.to_be_bytes());
    }
    vhea.extend_from_slice(&glyphs.to_be_bytes());
    with_tables(font, vec![(*b"vhea", vhea), (*b"vmtx", vmtx)])
}

/// `font` with a `VORG` table: every glyph's vertical origin is `default` above its baseline,
/// except the glyphs `overrides` lists (glyph id, origin). `None` if `font` can't be read.
pub fn with_vorg(font: &[u8], default: i16, overrides: &[(u16, i16)]) -> Option<Vec<u8>> {
    let mut entries = overrides.to_vec();
    entries.sort_unstable_by_key(|e| e.0);
    entries.dedup_by_key(|e| e.0);
    let mut vorg = Vec::new();
    for v in [1u16, 0] {
        vorg.extend_from_slice(&v.to_be_bytes());
    }
    vorg.extend_from_slice(&default.to_be_bytes());
    vorg.extend_from_slice(&u16::try_from(entries.len()).ok()?.to_be_bytes());
    for (gid, y) in entries {
        vorg.extend_from_slice(&gid.to_be_bytes());
        vorg.extend_from_slice(&y.to_be_bytes());
    }
    with_tables(font, vec![(*b"VORG", vorg)])
}

/// `font` with its OS/2 `fsType` (embedding licence bits) set to `fs_type`. `None` if the font has
/// no OS/2 table.
pub fn with_fs_type(mut font: Vec<u8>, fs_type: u16) -> Option<Vec<u8>> {
    let tables = usize::from(u16::from_be_bytes(font.get(4..6)?.try_into().ok()?));
    let rec = (0..tables).map(|r| 12 + r * 16).find(|rec| font.get(*rec..rec + 4) == Some(b"OS/2"))?;
    let offset = u32::from_be_bytes(font.get(rec + 8..rec + 12)?.try_into().ok()?) as usize;
    font.get_mut(offset + 8..offset + 10)?.copy_from_slice(&fs_type.to_be_bytes());
    Some(font)
}

/// Offset and length of table `tag` in `font`'s table directory, and where its record is.
fn table(font: &[u8], tag: &[u8; 4]) -> Option<(usize, usize, usize)> {
    let tables = usize::from(u16::from_be_bytes(font.get(4..6)?.try_into().ok()?));
    let rec = (0..tables).map(|r| 12 + r * 16).find(|rec| font.get(*rec..rec + 4) == Some(tag))?;
    let offset = u32::from_be_bytes(font.get(rec + 8..rec + 12)?.try_into().ok()?) as usize;
    let len = u32::from_be_bytes(font.get(rec + 12..rec + 16)?.try_into().ok()?) as usize;
    Some((rec, offset, len))
}

/// `font` with table `tag` cut to `len` bytes (its directory length; the bytes stay). `None` if
/// the font has no such table.
pub fn with_table_len(mut font: Vec<u8>, tag: &[u8; 4], len: u32) -> Option<Vec<u8>> {
    let (rec, _, _) = table(&font, tag)?;
    font.get_mut(rec + 12..rec + 16)?.copy_from_slice(&len.to_be_bytes());
    Some(font)
}

/// `font` with the big-endian `u16` at byte `at` of table `tag` set to `value`. `None` if the font
/// has no such table or it is too short.
pub fn with_table_u16(mut font: Vec<u8>, tag: &[u8; 4], at: usize, value: u16) -> Option<Vec<u8>> {
    let (_, offset, len) = table(&font, tag)?;
    if at + 2 > len {
        return None;
    }
    font.get_mut(offset + at..offset + at + 2)?.copy_from_slice(&value.to_be_bytes());
    Some(font)
}

/// `font` (a TrueType font) with an empty `.notdef`: glyph 0's `loca` entry starts where it ends,
/// so the glyph has no outline. `None` if the font has no `head`/`loca` tables.
pub fn with_empty_notdef(mut font: Vec<u8>) -> Option<Vec<u8>> {
    let (_, head, _) = table(&font, b"head")?;
    let long = font.get(head + 50..head + 52)? != [0, 0];
    let (_, loca, _) = table(&font, b"loca")?;
    let width = if long { 4 } else { 2 };
    let end = font.get(loca + width..loca + 2 * width)?.to_vec();
    font.get_mut(loca..loca + width)?.copy_from_slice(&end);
    Some(font)
}

/// `font` with its `name` table replaced by `records`: (platform, encoding, language, name id,
/// text). Text is written as UTF-16BE on the Unicode and Windows platforms, as its bytes on the
/// Macintosh platform. `None` if `font` can't be read.
pub fn with_names(font: &[u8], records: &[(u16, u16, u16, u16, &str)]) -> Option<Vec<u8>> {
    let strings: Vec<Vec<u8>> =
        records.iter().map(|r| if r.0 == 1 { r.4.as_bytes().to_vec() } else { r.4.encode_utf16().flat_map(u16::to_be_bytes).collect() }).collect();
    let count = u16::try_from(records.len()).ok()?;
    let mut name = Vec::new();
    for v in [0u16, count, 6 + 12 * count] {
        name.extend_from_slice(&v.to_be_bytes());
    }
    let mut at = 0usize;
    for (r, s) in records.iter().zip(&strings) {
        for v in [r.0, r.1, r.2, r.3, u16::try_from(s.len()).ok()?, u16::try_from(at).ok()?] {
            name.extend_from_slice(&v.to_be_bytes());
        }
        at += s.len();
    }
    strings.iter().for_each(|s| name.extend_from_slice(s));
    with_tables(font, vec![(*b"name", name)])
}

/// `font` with its OS/2 `ulCodePageRange1` and `ulCodePageRange2` set (the font's OS/2 table must
/// be version 1 or later). `None` if it has none.
pub fn with_code_pages(font: Vec<u8>, range1: u32, range2: u32) -> Option<Vec<u8>> {
    let font = with_table_u16(font, b"OS/2", 78, (range1 >> 16) as u16)?;
    let font = with_table_u16(font, b"OS/2", 80, range1 as u16)?;
    let font = with_table_u16(font, b"OS/2", 82, (range2 >> 16) as u16)?;
    with_table_u16(font, b"OS/2", 84, range2 as u16)
}

/// `font` with a `meta` table holding `dlng` and `slng` script and language tag lists (as written,
/// comma-separated), each when given. `None` if `font` can't be read.
pub fn with_meta(font: &[u8], dlng: Option<&str>, slng: Option<&str>) -> Option<Vec<u8>> {
    let maps: Vec<(&[u8; 4], &str)> = [(b"dlng", dlng), (b"slng", slng)].into_iter().filter_map(|(t, v)| Some((t, v?))).collect();
    let count = u32::try_from(maps.len()).ok()?;
    let mut meta = Vec::new();
    for v in [1u32, 0, 0, count] {
        meta.extend_from_slice(&v.to_be_bytes());
    }
    let mut at = 16 + 12 * maps.len();
    for (tag, v) in &maps {
        meta.extend_from_slice(*tag);
        meta.extend_from_slice(&u32::try_from(at).ok()?.to_be_bytes());
        meta.extend_from_slice(&u32::try_from(v.len()).ok()?.to_be_bytes());
        at += v.len();
    }
    maps.iter().for_each(|(_, v)| meta.extend_from_slice(v.as_bytes()));
    with_tables(font, vec![(*b"meta", meta)])
}
