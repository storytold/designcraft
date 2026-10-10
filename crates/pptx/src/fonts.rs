//! Font names as PowerPoint looks them up, and nominal advances to measure its line widths.

use std::collections::HashMap;

use designcraft_fonts::FontFace;
use skrifa::MetadataProvider;
use skrifa::string::StringId;

/// How a run names its face: the legacy (style-linked) family plus bold and italic flags, which
/// is how Office finds a face (`Source Serif 4 Semibold` is its own family; `Bold` is a flag).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunFont {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Default)]
pub struct Fonts {
    names: HashMap<u32, RunFont>,
    cmaps: HashMap<u32, HashMap<char, u32>>,
    /// Glyph → the lowest character mapping to it.
    reverse: HashMap<u32, HashMap<u32, char>>,
    /// Faces the slides use, by (family, bold, italic), in first-use order.
    pub used: Vec<(RunFont, designcraft_fonts::FaceRef)>,
}

fn english(face: &FontFace, id: StringId) -> Option<String> {
    let f = face.skrifa()?;
    let mut first = None;
    for s in f.localized_strings(id) {
        let text: String = s.chars().collect();
        if text.trim().is_empty() {
            continue;
        }
        if s.language().is_none_or(|l| l.starts_with("en")) {
            return Some(text);
        }
        first.get_or_insert(text);
    }
    first
}

impl Fonts {
    /// The run font for a face a slide uses (recorded for embedding).
    pub fn use_face(&mut self, face: designcraft_fonts::FaceRef) -> RunFont {
        let rf = self.run_font(face.get());
        if !self.used.iter().any(|(u, _)| *u == rf) {
            self.used.push((rf.clone(), face));
        }
        rf
    }

    /// Embedded OpenType for every family used, and the faces that couldn't be embedded.
    pub fn embedded(&self) -> (Vec<EmbeddedFont>, Vec<String>) {
        let mut out: Vec<EmbeddedFont> = Vec::new();
        let mut skipped = Vec::new();
        for (rf, face) in &self.used {
            let face = face.get();
            match eot(face) {
                Ok(bytes) => {
                    let slot = usize::from(rf.bold) + 2 * usize::from(rf.italic);
                    let at = match out.iter().position(|e| e.family == rf.family) {
                        Some(i) => i,
                        None => {
                            out.push(EmbeddedFont { family: rf.family.clone(), ..Default::default() });
                            out.len() - 1
                        }
                    };
                    if let Some(e) = out.get_mut(at)
                        && let Some(s) = e.faces.get_mut(slot)
                    {
                        *s = Some(bytes);
                    }
                }
                Err(why) => skipped.push(format!(
                    "{} {} ({})",
                    face.family,
                    face.style,
                    if why == NoEmbed::Restricted { "its licence forbids embedding" } else { "not a TrueType font" }
                )),
            }
        }
        (out, skipped)
    }

    pub fn run_font(&mut self, face: &FontFace) -> RunFont {
        self.names
            .entry(face.id())
            .or_insert_with(|| {
                let family = english(face, StringId::FAMILY_NAME).unwrap_or_else(|| face.family.clone());
                let sub = english(face, StringId::SUBFAMILY_NAME).unwrap_or_else(|| face.style.clone()).to_ascii_lowercase();
                // A named instance of a variable font has no legacy names of its own.
                if face.is_variable() {
                    let style = face.style.to_ascii_lowercase();
                    let bold = face.weight >= 650.0;
                    let regular = matches!(style.as_str(), "regular" | "italic" | "bold" | "bold italic");
                    let family = if regular { face.family.clone() } else { format!("{} {}", face.family, face.style.replace(" Italic", "")) };
                    return RunFont { family, bold: bold && regular, italic: face.italic };
                }
                RunFont { family, bold: sub.contains("bold"), italic: sub.contains("italic") || sub.contains("oblique") }
            })
            .clone()
    }

    /// Nominal advance of `c` in font units (no kerning or features), as an application without
    /// the composer's spacing would set it.
    pub fn advance(&mut self, face: &FontFace, c: char) -> Option<f64> {
        self.cmap(face).get(&c).map(|g| face.advance(*g))
    }

    pub fn glyph(&mut self, face: &FontFace, c: char) -> Option<u32> {
        self.cmap(face).get(&c).copied()
    }

    /// The character a glyph shows (the lowest code point mapping to it).
    pub fn char_of(&mut self, face: &FontFace, gid: u32) -> Option<char> {
        let map = self.reverse.entry(face.id()).or_insert_with(|| {
            let mut m = HashMap::new();
            for (c, g) in face.chars() {
                m.entry(g).or_insert(c);
            }
            m
        });
        map.get(&gid).copied()
    }

    fn cmap(&mut self, face: &FontFace) -> &HashMap<char, u32> {
        self.cmaps.entry(face.id()).or_insert_with(|| face.chars().into_iter().collect())
    }
}

/// A family embedded in the presentation: its regular, bold, italic and bold italic faces.
#[derive(Clone, Debug, Default)]
pub struct EmbeddedFont {
    pub family: String,
    pub faces: [Option<Vec<u8>>; 4],
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*d.get(at)?, *d.get(at + 1)?, *d.get(at + 2)?, *d.get(at + 3)?]))
}

fn utf16_name(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    let bytes = (units.len() * 2).min(u16::MAX as usize & !1);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(bytes as u16).to_le_bytes());
    for u in units.iter().take(bytes / 2) {
        out.extend_from_slice(&u.to_le_bytes());
    }
}

/// Why a face can't be embedded.
#[derive(Debug, PartialEq, Eq)]
pub enum NoEmbed {
    /// The font's licence (OS/2 `fsType`) forbids embedding.
    Restricted,
    /// PowerPoint embeds TrueType outlines only (not CFF, collections or variable instances).
    Format,
}

/// The face as Embedded OpenType (uncompressed, version 2.1), the form PowerPoint stores
/// embedded fonts in.
pub fn eot(face: &FontFace) -> Result<Vec<u8>, NoEmbed> {
    use skrifa::raw::TableProvider as _;
    use skrifa::raw::types::Tag;
    let data = face.data();
    if face.index() != 0 || face.is_variable() || data.get(..4) == Some(b"OTTO") || data.get(..4) == Some(b"ttcf") {
        return Err(NoEmbed::Format);
    }
    let f = face.skrifa().ok_or(NoEmbed::Format)?;
    if f.glyf().is_err() {
        return Err(NoEmbed::Format);
    }
    let os2 = f.table_data(Tag::new(b"OS/2")).map(|t| t.as_bytes().to_vec()).unwrap_or_default();
    let head = f.table_data(Tag::new(b"head")).map(|t| t.as_bytes().to_vec()).unwrap_or_default();
    let fs_type = be16(&os2, 8).unwrap_or(0);
    // Restricted License embedding (bit 1 alone): may not be embedded.
    if fs_type & 0x000F == 0x0002 {
        return Err(NoEmbed::Restricted);
    }
    let mut h: Vec<u8> = Vec::with_capacity(256);
    h.extend_from_slice(&[0; 8]); // EOTSize, FontDataSize (filled in below)
    h.extend_from_slice(&0x0002_0001u32.to_le_bytes());
    h.extend_from_slice(&0u32.to_le_bytes()); // Flags: no compression, no obfuscation
    h.extend_from_slice(os2.get(32..42).unwrap_or(&[0; 10]));
    h.push(1); // DEFAULT_CHARSET
    h.push(u8::from(face.italic));
    h.extend_from_slice(&u32::from(be16(&os2, 4).unwrap_or(400)).to_le_bytes());
    h.extend_from_slice(&fs_type.to_le_bytes());
    h.extend_from_slice(&0x504Cu16.to_le_bytes());
    for i in 0..4 {
        h.extend_from_slice(&be32(&os2, 42 + 4 * i).unwrap_or(0).to_le_bytes());
    }
    let has_codepages = be16(&os2, 0).unwrap_or(0) >= 1;
    for i in 0..2 {
        h.extend_from_slice(&(if has_codepages { be32(&os2, 78 + 4 * i).unwrap_or(0) } else { 0 }).to_le_bytes());
    }
    h.extend_from_slice(&be32(&head, 8).unwrap_or(0).to_le_bytes());
    h.extend_from_slice(&[0; 16]); // Reserved1–4
    let family = english(face, StringId::FAMILY_NAME).unwrap_or_else(|| face.family.clone());
    let style = english(face, StringId::SUBFAMILY_NAME).unwrap_or_else(|| face.style.clone());
    let version = english(face, StringId::VERSION_STRING).unwrap_or_default();
    let full = english(face, StringId::FULL_NAME).unwrap_or_else(|| format!("{family} {style}"));
    utf16_name(&mut h, &family);
    utf16_name(&mut h, &style);
    utf16_name(&mut h, &version);
    utf16_name(&mut h, &full);
    utf16_name(&mut h, ""); // RootString
    let total = h.len().checked_add(data.len()).and_then(|n| u32::try_from(n).ok()).ok_or(NoEmbed::Format)?;
    h[0..4].copy_from_slice(&total.to_le_bytes());
    h[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
    h.extend_from_slice(data);
    Ok(h)
}
