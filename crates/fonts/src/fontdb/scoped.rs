#[cfg(test)]
use super::face::{BUNDLED, FontSource, enumerate_faces};
use super::face::{FALLBACK_FAMILY, FontFace, last_resort_face, make_face};
use super::fallback::{cjk_chain, script_chain};
use super::names::{alternate_names, base_style, has_wws_family, known_as, norm, style_italic, style_weight};
use super::{Arc, BezPath, FamilyInfo, FontDb, MetadataProvider, PoisonError};

/// One document's view of the [`FontDb`] ([`FontDb::scoped`]): the fonts of its `Document Fonts`
/// folder, then the shared fonts (bundled, installed, added). Other documents' fonts aren't in it.
#[derive(Clone)]
pub struct ScopedFonts<'a> {
    pub(super) db: &'a FontDb,
    /// The document's own faces, in the folder's file order (none without a scope).
    pub(super) own: Option<Arc<[Arc<FontFace>]>>,
}

impl ScopedFonts<'_> {
    pub(super) fn own(&self) -> &[Arc<FontFace>] {
        self.own.as_deref().unwrap_or(&[])
    }

    /// Resolve localized and legacy family names without confusing replacement fonts with
    /// installed originals. Document-local names take precedence over shared fonts.
    ///
    /// In each scope (the document's faces, loaded faces, the catalog) an exact typographic name
    /// wins, then a WWS family name (an optical-size face's, see `find`), which names its faces as
    /// it is, then the aliases. An optical-size face's legacy family name (name ID 1) is usually
    /// its WWS name, so the aliases must not claim it first.
    pub fn canonical_family(&self, family: &str) -> Option<String> {
        let is = |name: &str| name.eq_ignore_ascii_case(family);
        let in_faces = |faces: &[Arc<FontFace>]| -> Option<String> {
            if let Some(f) = faces.iter().find(|f| is(&f.family)) {
                return Some(f.family.clone());
            }
            if let Some(w) = faces.iter().find_map(|f| f.wws.as_ref().filter(|w| is(&w.0))) {
                return Some(w.0.clone());
            }
            faces.iter().find(|f| f.family_aliases.iter().any(|alias| is(alias))).map(|f| f.family.clone())
        };
        if let Some(name) = in_faces(self.own()).or_else(|| in_faces(&self.db.read_faces())) {
            return Some(name);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let catalog = self.db.read_catalog();
            if let Some(c) = catalog.iter().find(|c| is(&c.family)) {
                return Some(c.family.clone());
            }
            if let Some(w) = catalog.iter().find_map(|c| c.wws.as_ref().filter(|w| is(&w.0))) {
                return Some(w.0.clone());
            }
            if let Some(c) = catalog.iter().find(|c| c.aliases.iter().any(|alias| is(alias))) {
                return Some(c.family.clone());
            }
        }
        None
    }

    /// Whether the requested style exists, including localized names and axis instances.
    pub fn has_style(&self, family: &str, style: &str) -> bool {
        if !self.has_family(family) {
            return false;
        }
        let face = self.face(family, style);
        norm(&face.style) == norm(style)
            || (face.coords.is_empty() && face.style_aliases.iter().any(|alias| norm(alias) == norm(style)))
            // An optical-size face asked for by its WWS names.
            || face.wws.as_ref().is_some_and(|w| w.0.eq_ignore_ascii_case(family) && norm(&w.1) == norm(style))
    }

    /// Prefer known metric-compatible substitutes, then an appropriate script family. This
    /// never changes `has_family`: a usable substitute is still a missing original font.
    pub fn substitute_family(&self, family: &str) -> String {
        let lower = family.to_lowercase();
        let candidates: &[&str] = match lower.as_str() {
            "calibri" | "calibri light" => &["Carlito", "Liberation Sans"],
            "cambria" | "cambria math" => &["Caladea", "Liberation Serif"],
            "arial" | "helvetica" => &["Liberation Sans", "Arial", "Helvetica"],
            "times new roman" | "times" => &["Liberation Serif", "Times New Roman", "Times"],
            "courier new" | "courier" => &["Liberation Mono", "Courier New", "Courier"],
            _ if lower.contains("arabic") || lower.contains("naskh") => &["Amiri", "Noto Naskh Arabic", "Geeza Pro"],
            _ if family.contains('楷') => &["Kaiti SC", "STKaiti", "KaiTi", "Songti SC"],
            _ if family.contains("仿宋") => &["STFangsong", "FangSong", "Songti SC"],
            _ if family.contains('宋') || family.contains("明朝") => &["Songti SC", "Noto Serif CJK SC", "SimSun"],
            _ if family.contains('黑') || family.contains("雅黑") => &["PingFang SC", "Noto Sans CJK SC", "Microsoft YaHei"],
            _ if family.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)) => &["Songti SC", "Noto Serif CJK SC", "PingFang SC"],
            _ => &[],
        };
        candidates.iter().find_map(|name| self.canonical_family(name)).unwrap_or_else(|| FALLBACK_FAMILY.to_string())
    }

    /// A replacement suggestion must cover the text being replaced, not just its main script.
    /// Ties retain the preferred substitute; this does not install or redefine any font.
    pub fn replacement_face(&self, family: &str, style: &str, characters: &[char]) -> Arc<FontFace> {
        let mut best = self.face(family, style);
        let coverage = |face: &FontFace| characters.iter().filter(|&&c| c.is_whitespace() || c.is_control() || face.covers(c)).count();
        let mut score = coverage(&best);
        let candidates: &[&str] = if characters.iter().any(|c| ('\u{0600}'..='\u{08ff}').contains(c)) {
            &["Amiri", "Noto Naskh Arabic", "Al Bayan", "Damascus", "Geeza Pro", "Arial"]
        } else if characters.iter().any(|c| ('\u{3400}'..='\u{9fff}').contains(c)) {
            &["Songti SC", "Kaiti SC", "Noto Serif CJK SC", "PingFang SC", "Noto Sans CJK SC"]
        } else {
            &["Source Serif 4", "Source Sans 3", "Inter"]
        };
        for candidate in candidates {
            if score == characters.len() {
                break;
            }
            if !self.has_family(candidate) {
                continue;
            }
            let face = self.face(candidate, style);
            let next = coverage(&face);
            if next > score {
                best = face;
                score = next;
            }
        }
        best
    }

    /// Family names available (the document's, loaded and cataloged system fonts), sorted and
    /// deduplicated.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.own().iter().chain(self.db.read_faces().iter()).map(|f| f.family.clone()).collect();
        #[cfg(not(target_arch = "wasm32"))]
        v.extend(self.db.read_catalog().iter().map(|c| c.family.clone()));
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// The available families as the font menus list them (see [`ScopedFonts::families`]), each
    /// with its group and native name, unsorted ([`crate::sort_for_menu`] orders them).
    pub fn family_infos(&self) -> Vec<FamilyInfo> {
        let mut out: Vec<FamilyInfo> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for f in self.own().iter().chain(self.db.read_faces().iter()) {
            if seen.insert(f.family.to_lowercase()) {
                let (group, native) = f.menu_group();
                out.push(FamilyInfo { family: f.family.clone(), group, native: native.map(str::to_string) });
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.db.read_catalog().iter() {
            if seen.insert(c.family.to_lowercase()) {
                out.push(FamilyInfo { family: c.family.clone(), group: c.group, native: c.native.clone() });
            }
        }
        out
    }

    /// Style names available for `family` (Regular first, then by weight).
    pub fn styles(&self, family: &str) -> Vec<String> {
        self.style_traits(family).into_iter().map(|t| t.2).collect()
    }

    /// Weight and italic of a style of `family`: from the font when it is known, else from the
    /// style's name.
    pub fn traits_of(&self, family: &str, style: &str) -> (f32, bool) {
        let base = base_style(style);
        self.style_traits(family)
            .into_iter()
            .find(|t| t.2.eq_ignore_ascii_case(base))
            .map_or((style_weight(base), style_italic(base)), |(italic, weight, _)| (weight, italic))
    }

    /// Style linking: the style of `family` that sets bold (`bold`) and italic (`italic`) as asked
    /// and keeps the other from `current` (Bold from Italic is Bold Italic). `None` when the
    /// family has no such style.
    pub fn linked_style(&self, family: &str, current: &str, bold: Option<bool>, italic: Option<bool>) -> Option<String> {
        let (weight, cur_italic) = self.traits_of(family, current);
        let want_italic = italic.unwrap_or(cur_italic);
        let want_weight = match bold {
            Some(true) => 700.0,
            Some(false) => 400.0,
            None => weight,
        };
        // Width words (Condensed, Extended…) of the current style are kept when the family allows.
        const WIDTHS: &[&str] = &["condensed", "compressed", "narrow", "extended", "expanded", "wide"];
        let widths = |s: &str| {
            let n = norm(s);
            WIDTHS.iter().filter(|w| n.contains(*w)).count()
        };
        let cur_widths = widths(current);
        self.style_traits(family)
            .into_iter()
            .filter(|(i, w, _)| *i == want_italic && bold.is_none_or(|b| b == (*w >= 600.0)))
            .min_by(|a, b| {
                let key = |t: &(bool, f32, String)| ((t.1 - want_weight).abs(), usize::from(widths(&t.2) != cur_widths), t.2.len());
                let (ka, kb) = (key(a), key(b));
                ka.0.total_cmp(&kb.0).then(ka.1.cmp(&kb.1)).then(ka.2.cmp(&kb.2))
            })
            .map(|t| t.2)
    }

    /// `(italic, weight, name)` of each style of `family` (the document's, loaded, or installed),
    /// Regular first, then by weight.
    fn style_traits(&self, family: &str) -> Vec<(bool, f32, String)> {
        let v = self.exact_style_traits(family);
        if !v.is_empty() {
            return v;
        }
        alternate_names(family).map(|f| self.exact_style_traits(f)).find(|v| !v.is_empty()).unwrap_or_default()
    }

    fn exact_style_traits(&self, family: &str) -> Vec<(bool, f32, String)> {
        let canonical = self.canonical_family(family);
        let family = canonical.as_deref().unwrap_or(family);
        let v = self.named_style_traits(family, false);
        // No face of that typographic family: the family may be a WWS name (see `find`).
        if v.is_empty() { self.named_style_traits(family, true) } else { v }
    }

    /// [`ScopedFonts::exact_style_traits`] by the faces' typographic names, or by their WWS names
    /// when `by_wws`.
    fn named_style_traits(&self, family: &str, by_wws: bool) -> Vec<(bool, f32, String)> {
        let mut v: Vec<(bool, f32, String)> = self
            .own()
            .iter()
            .chain(self.db.read_faces().iter())
            .filter(|f| !f.style.contains('{'))
            .filter_map(|f| known_as(&f.family, &f.style, f.wws.as_ref(), by_wws).map(|n| (f, n)))
            .filter(|(_, (fam, _))| fam.eq_ignore_ascii_case(family))
            .map(|(f, (_, style))| (f.italic, f.weight, style.to_string()))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.db.read_catalog().iter() {
            if let Some((fam, style)) = known_as(&c.family, &c.style, c.wws.as_ref(), by_wws)
                && fam.eq_ignore_ascii_case(family)
                && !v.iter().any(|(_, _, s)| s.eq_ignore_ascii_case(style))
            {
                v.push((style_italic(style), style_weight(style), style.to_string()));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        v.dedup_by(|a, b| a.2 == b.2);
        v
    }

    /// Resolve a family + style to a face, falling back to the closest style of the family, then to
    /// Source Sans 3 Regular. Installed system fonts are found by name whatever ran before. A
    /// family that isn't found as written is looked up without surrounding spaces, then without
    /// InDesign's `$ID/` prefix or a format suffix (`Minion Pro (OTF)` → `Minion Pro`).
    pub fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        let canonical = self
            .canonical_family(family)
            .or_else(|| alternate_names(family).find_map(|f| self.canonical_family(f)))
            .unwrap_or_else(|| self.substitute_family(alternate_names(family).last().unwrap_or(family)));
        if let Some(f) = self.find_or_load(&canonical, style) {
            return f;
        }
        self.find(FALLBACK_FAMILY, style)
            .or_else(|| self.find(FALLBACK_FAMILY, "Regular"))
            .or_else(|| self.db.read_faces().first().cloned())
            .unwrap_or_else(last_resort_face)
    }

    fn find_or_load(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        if let Some(f) = self.find(family, style) {
            return Some(f);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.db.load_cataloged(family) {
            return self.find(family, style);
        }
        None
    }

    /// The family and style of the face whose PostScript name is `name`, ignoring case (the
    /// document's, loaded, or installed on the system, not loading it). SVGs exported from
    /// Illustrator name fonts this way (`font-family="'MyriadPro-Regular'"`).
    pub fn postscript_face(&self, name: &str) -> Option<(String, String)> {
        let named = |ps: Option<&str>| ps.is_some_and(|ps| ps.eq_ignore_ascii_case(name));
        if let Some(f) = self.own().iter().chain(self.db.read_faces().iter()).find(|f| named(f.postscript_name())) {
            return Some((f.family.clone(), f.style.clone()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(c) = self.db.read_catalog().iter().find(|c| named(c.postscript.as_deref())) {
            return Some((c.family.clone(), c.style.clone()));
        }
        None
    }

    /// Is `family` available (the document's, loaded, or installed on the system), as named or
    /// under an alternate name (see [`ScopedFonts::face`])?
    pub fn has_family(&self, family: &str) -> bool {
        self.canonical_family(family).is_some() || alternate_names(family).any(|f| self.canonical_family(f).is_some())
    }

    /// A variable font's axes: (tag, name, min, default, max), empty for static fonts.
    pub fn axes(&self, family: &str, style: &str) -> Vec<(String, String, f32, f32, f32)> {
        let face = self.face(family, base_style(style));
        let Some(f) = face.skrifa() else { return vec![] };
        f.axes()
            .iter()
            .map(|a| {
                let tag = String::from_utf8_lossy(&a.tag().to_be_bytes()).to_string();
                let name = f.localized_strings(a.name_id()).english_or_first().map(|s| s.to_string()).unwrap_or_else(|| tag.clone());
                (tag, name, a.min_value(), a.default_value(), a.max_value())
            })
            .collect()
    }

    /// Glyph outline in font units, y-down (flipped), cached per (face, glyph); see
    /// [`FontDb::outline`].
    pub fn outline(&self, face: &FontFace, gid: u32) -> Arc<BezPath> {
        self.db.outline(face, gid)
    }

    /// See [`FontDb::missing_box`].
    pub fn missing_box(&self, face: &FontFace) -> Option<BezPath> {
        self.db.missing_box(face)
    }

    /// `Style {wght:650,wdth:90}`: the named style's font at those axis values, made on first use.
    fn instance(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        let open = style.find('{')?;
        let base = self.find(family, style[..open].trim())?;
        if base.skrifa().is_none_or(|f| f.axes().is_empty()) {
            return Some(base);
        }
        let key = (base.id, style.to_string());
        if let Some(f) = self.db.instances.read().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return Some(f.clone());
        }
        let mut coords = base.coords.clone();
        for kv in style[open + 1..].trim_end_matches('}').split(',') {
            let (k, v) = kv.split_once(':')?;
            let k = k.trim().as_bytes();
            let v: f32 = v.trim().parse().ok()?;
            if k.len() != 4 {
                return None;
            }
            let tag = [k[0], k[1], k[2], k[3]];
            match coords.iter_mut().find(|(t, _)| *t == tag) {
                Some(c) => c.1 = v,
                None => coords.push((tag, v)),
            }
        }
        let f = make_face(base.bytes.clone(), base.source.clone(), base.index, base.family.clone(), style.to_string(), coords)?;
        Some(self.db.instances.write().unwrap_or_else(PoisonError::into_inner).entry(key).or_insert_with(|| Arc::new(f)).clone())
    }

    pub(super) fn find(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        if style.contains('{') {
            return self.instance(family, style);
        }
        let faces = self.db.read_faces();
        // The document's fonts first: they stand in for other fonts of the same name.
        let named = |f: &&Arc<FontFace>| f.family.eq_ignore_ascii_case(family);
        let own: Vec<&Arc<FontFace>> = self.own().iter().filter(named).collect();
        let mut cands: Vec<&Arc<FontFace>> = if own.is_empty() { faces.iter().filter(named).collect() } else { own };
        // No face of that typographic family: the family may be a WWS name, whose faces carry
        // their WWS style names.
        let by_wws = cands.is_empty();
        if by_wws {
            let aliased = |f: &&Arc<FontFace>| has_wws_family(f, family);
            let own: Vec<&Arc<FontFace>> = self.own().iter().filter(aliased).collect();
            cands = if own.is_empty() { faces.iter().filter(aliased).collect() } else { own };
        }
        if cands.is_empty() {
            return None;
        }
        let style_of = |f: &FontFace| -> String { norm(f.wws.as_ref().filter(|_| by_wws).map_or(&f.style, |w| &w.1)) };
        let ns = norm(style);
        // A face's own style name first: another face's legacy subfamily ("Black" faces named
        // "Regular" in their RIBBI family) or localized name must not shadow it.
        if let Some(f) = cands
            .iter()
            .find(|f| style_of(f) == ns)
            .or_else(|| cands.iter().find(|f| f.coords.is_empty() && f.style_aliases.iter().any(|alias| norm(alias) == ns)))
        {
            return Some((*f).clone());
        }
        let (tw, ti) = (style_weight(style), style_italic(style));
        cands
            .iter()
            .min_by(|a, b| {
                let sa = (a.weight - tw).abs() + if a.italic != ti { 1000.0 } else { 0.0 };
                let sb = (b.weight - tw).abs() + if b.italic != ti { 1000.0 } else { 0.0 };
                sa.total_cmp(&sb)
            })
            .map(|f| (*f).clone())
    }

    /// A face other than `exclude` that covers `c` in `language` (a BCP 47 tag, `None` when
    /// unknown). CJK characters try their language's chain first (see [`cjk_chain`]), Malayalam
    /// the fonts made for it (see [`script_chain`]); then the first face (fallback family first,
    /// then load order, the document's fonts last) that covers `c`. On native, system fonts are
    /// cataloged and loaded lazily the first time no loaded face covers a character.
    pub fn fallback_for(&self, c: char, exclude: u32, language: Option<&str>) -> Option<Arc<FontFace>> {
        if let Some(chain) = cjk_chain(c, language).or_else(|| script_chain(c))
            && let Some(f) = chain.iter().find_map(|family| self.chain_face(family, c, exclude))
        {
            return Some(f);
        }
        if let Some(f) = self.loaded_fallback(c, exclude) {
            return Some(f);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.db.system_fallback(c) {
            return self.loaded_fallback(c, exclude);
        }
        None
    }

    /// The face of `family` (the document's first, regular first) that covers `c`, loading the
    /// installed family the first time a chain asks for it (native, while the system fallback is
    /// on).
    fn chain_face(&self, family: &'static str, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.db.is_loaded(family) {
            // Held while loading, so a parallel lookup waits for the font instead of passing it by.
            let mut sys = self.db.sys.lock().unwrap_or_else(PoisonError::into_inner);
            if sys.enabled && sys.chain_tried.insert(family) {
                self.db.load_cataloged(family);
            }
        }
        let faces = self.db.read_faces();
        let mut order: Vec<&Arc<FontFace>> =
            self.own().iter().chain(faces.iter()).filter(|f| f.id != exclude && f.family.eq_ignore_ascii_case(family)).collect();
        order.sort_by_key(|f| (f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }

    fn loaded_fallback(&self, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        let faces = self.db.read_faces();
        let mut order: Vec<&Arc<FontFace>> = faces.iter().chain(self.own().iter()).filter(|f| f.id != exclude).collect();
        order.sort_by_key(|f| (!f.family.eq_ignore_ascii_case(FALLBACK_FAMILY), f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_gui_document_fonts_tests {
    use super::*;

    /// The production font scope contains the canonical bundled and Japanese faces in their existing order, including bundled Hebrew/Devanagari faces, without system or injected fonts.
    #[test]
    fn native_gui_canonical_document_faces_load_without_system_or_added_fonts() {
        if crate::CRAFT_FONTS.is_empty() {
            assert!(std::env::var_os("CRAFT_FONTS_REQUIRED").is_none(), "required canonical fonts were not embedded");
            return; // Optional-input builds retain their supported Latin-only database.
        }
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        let scope = db.scoped(0);
        let japanese = crate::japanese_document_fonts();
        assert_existing_document_faces_keep_order(&db, &japanese);
        assert_primary_bundled_faces(&db);
        assert_japanese_fonts_keep_priority(&japanese);
        let latin = db.face(crate::DEFAULT_FAMILY, "Regular");
        assert_japanese_fallback_priority(&db, latin.id());
        let admitted = assert_canonical_script_fonts(&db, &scope, latin.id());
        let loaded = loaded_faces(&db);
        let previous_len = canonical_bundled_faces(&japanese).len();
        assert_eq!(&loaded[previous_len..], admitted.as_slice(), "only the two deliberate Regular tuples follow the existing document faces");
        assert!(!scope.has_family("Unavailable Native GUI Family"));
        assert_eq!(scope.face("Unavailable Native GUI Family", "Regular").family, FALLBACK_FAMILY);
    }

    fn canonical_bundled_faces(japanese: &[&crate::CraftFont]) -> Vec<(String, String)> {
        BUNDLED
            .iter()
            .copied()
            .chain(japanese.iter().map(|font| font.bytes))
            .flat_map(enumerate_faces)
            .map(|(_, family, style, _)| (family, style))
            .collect()
    }

    fn loaded_faces(db: &FontDb) -> Vec<(String, String)> {
        db.read_faces().iter().map(|face| (face.family.clone(), face.style.clone())).collect()
    }

    fn assert_existing_document_faces_keep_order(db: &FontDb, japanese: &[&crate::CraftFont]) {
        let expected = canonical_bundled_faces(japanese);
        let loaded = loaded_faces(db);
        assert!(loaded.len() >= expected.len(), "existing bundled/Japanese faces disappeared");
        assert_eq!(&loaded[..expected.len()], expected.as_slice(), "preserve all Latin and Japanese face ordering");
    }

    fn assert_primary_bundled_faces(db: &FontDb) {
        for family in ["Source Sans 3", "Source Serif 4", "Inter", "JetBrains Mono"] {
            let face = db.face(family, "Regular");
            assert_eq!((face.family.as_str(), face.style.as_str(), &face.source), (family, "Regular", &FontSource::Bundled));
        }
    }

    fn assert_japanese_fonts_keep_priority(japanese: &[&crate::CraftFont]) {
        assert!(!japanese.is_empty(), "the required central manifest includes the existing Japanese faces");
        assert!(japanese[0].family.contains("Mincho"));
        assert_eq!(crate::japanese_ui_fonts(false)[0].family, "BIZ UDPGothic");
        assert_eq!(crate::japanese_ui_fonts(true)[0].style, "Bold");
    }

    fn assert_japanese_fallback_priority(db: &FontDb, latin_face_id: u32) {
        for language in [Some("ja"), None] {
            assert!(
                db.fallback_for('語', latin_face_id, language).is_some_and(|face| face.family.contains("Mincho")),
                "Japanese fallback changed: {language:?}"
            );
        }
    }

    fn assert_canonical_script_fonts(db: &FontDb, scope: &ScopedFonts<'_>, latin_face_id: u32) -> Vec<(String, String)> {
        let mut admitted = Vec::new();
        for (family, text) in [("Noto Sans Hebrew", "שלום עולם"), ("Noto Sans Devanagari", "नमस्ते दुनिया")] {
            let embedded: Vec<_> = crate::CRAFT_FONTS.iter().filter(|font| font.family == family && font.style == "Regular").collect();
            assert_eq!(embedded.len(), 1, "exactly one canonical {family} Regular input");
            assert_script_face_registered(db, scope, family, embedded[0].bytes);
            let face = scope.face(family, "Regular");
            assert_script_glyphs_have_document_fallback(db, latin_face_id, family, text, &face, embedded[0].bytes);
            admitted.push((family.to_string(), "Regular".to_string()));
        }
        admitted
    }

    fn assert_script_face_registered(db: &FontDb, scope: &ScopedFonts<'_>, family: &str, expected_bytes: &[u8]) {
        assert!(scope.has_family(family), "production database did not register {family}");
        assert!(scope.has_style(family, "Regular"), "{family} Regular");
        assert!(!scope.has_style(family, "Unavailable Native GUI Style"));
        assert!(db.families().iter().any(|name| name == family), "family catalog omits {family}");
        assert!(scope.family_infos().iter().any(|info| info.family == family), "menu catalog omits {family}");
        let face = scope.face(family, "Regular");
        assert_eq!((face.family.as_str(), face.style.as_str(), &face.source), (family, "Regular", &FontSource::Bundled));
        assert_eq!(face.data(), expected_bytes, "neither OS discovery nor an injected face may satisfy this contract");
        let replacement = scope.face(family, "Unavailable Native GUI Style");
        assert_eq!((replacement.family.as_str(), replacement.style.as_str()), (family, "Regular"), "nearest-style policy stays truthful");
    }

    fn assert_script_glyphs_have_document_fallback(
        db: &FontDb,
        latin_face_id: u32,
        family: &str,
        text: &str,
        face: &FontFace,
        expected_bytes: &[u8],
    ) {
        for character in text.chars().filter(|character| !character.is_whitespace()) {
            assert_ne!(face.glyph_for(character), 0, "{family} lacks U+{:04X} ({character})", u32::from(character));
            assert!(
                db.fallback_for(character, latin_face_id, None).is_some_and(|fallback| {
                    fallback.family == family
                        && fallback.style == "Regular"
                        && fallback.source == FontSource::Bundled
                        && fallback.data() == expected_bytes
                        && fallback.glyph_for(character) != 0
                }),
                "document fallback must use the canonical {family} Regular face for {character}"
            );
        }
    }
}
