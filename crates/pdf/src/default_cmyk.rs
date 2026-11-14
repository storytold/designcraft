//! Default CMYK colour profile attachment for PDF export.
//!
//! Writes `/DefaultCMYK [/ICCBased <icc> <gen> R]` into every resource scope that does not already
//! define one (Pages, inherited `/Pages` ancestors, Form XObjects, SMask forms, TilingPatterns,
//! and Type3 fonts). PDF/X-4 reuses the catalog output intent's `/DestOutputProfile` stream; plain
//! print exports embed one shared Flate-compressed ICC profile stream.

use std::collections::{BTreeMap, HashSet};
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use craft_pdf::syntax::Pdf;
use craft_pdf::syntax::content::UntypedIter;
use craft_pdf::syntax::object::{Dict, Name, Object, ObjectIdentifier, Stream};
use craft_pdf::syntax::page::{Page, Resources};
use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::optional_content::{append_with, edited_with, has_entry, trailer_size};
use crate::pdfx::pdfx_intents;
use crate::{PdfError, Result};

/// Maximum nesting depth when walking `/Parent`, Form XObjects, patterns, and soft masks.
const MAX_DEPTH: usize = 32;

/// Maximum object-number probes beyond trailer `/Size` before treating the xref as damaged.
const MAX_ID_PROBES: usize = 65_536;

/// Maximum `scan_stream` invocations across a document before aborting `CmykScan`.
const MAX_SCAN_STREAMS: usize = 65_536;

fn damaged() -> PdfError {
    PdfError::Write("the exported PDF could not be updated with a DefaultCMYK profile cleanly".into())
}

fn encrypted() -> PdfError {
    PdfError::Write("an encrypted PDF cannot be updated with a DefaultCMYK profile".into())
}

fn depth_limit() -> PdfError {
    PdfError::Write("the exported PDF exceeds the DefaultCMYK nesting depth limit".into())
}

fn scan_limit() -> PdfError {
    PdfError::Write("the exported PDF exceeds the DefaultCMYK stream scan budget".into())
}

/// Ensures every page and nested resource scope in `bytes` carries `/DefaultCMYK` mapped to an
/// `/ICCBased` 4-channel ICC profile stream, unless that scope already defines `/DefaultCMYK`.
///
/// When `reuse_output_intent` is true and the PDF has a 4-channel `GTS_PDFX` `/DestOutputProfile`,
/// `/DefaultCMYK` references that stream directly without embedding a duplicate profile.
pub(crate) fn apply(bytes: &[u8], icc: &[u8], reuse_output_intent: bool) -> Result<Vec<u8>> {
    let old_size = trailer_size(bytes, damaged, encrypted)?;
    let pdf = Pdf::new(bytes.to_vec()).map_err(|_| damaged())?;
    if pdf.pages().is_empty() {
        return Ok(bytes.to_vec());
    }

    let existing_icc = if reuse_output_intent { output_intent_profile(&pdf) } else { None };
    let (icc_id, new_icc_body) = match existing_icc {
        Some(id) => (id, None),
        None => {
            if icc.is_empty() {
                return Err(damaged());
            }
            (next_object_id(&pdf, old_size)?, Some(encode_icc_stream(icc)?))
        }
    };

    let mut updater = Updater::new(&pdf, icc_id);
    for page in pdf.pages().iter() {
        updater.visit_page(page)?;
    }

    if updater.changes.is_empty() {
        return Ok(bytes.to_vec());
    }
    if let Some(body) = new_icc_body {
        updater.changes.insert(icc_id, body);
    }
    let ordered: Vec<(ObjectIdentifier, Vec<u8>)> = updater.changes.into_iter().collect();
    append_with(bytes, &ordered, None, damaged, encrypted)
}

/// Attaches `/DefaultCMYK` to an exported PDF when appropriate for `standard`: plain (`None`) and
/// `PdfX4` exports attach the profile whenever `cmyk_icc` was prepared for CMYK print output or
/// the PDF actually draws in `DeviceCMYK`.
pub(crate) fn attach(bytes: Vec<u8>, cmyk_icc: Option<&[u8]>, standard: crate::Standard, warnings: &mut Vec<String>) -> Result<Vec<u8>> {
    if !matches!(standard, crate::Standard::None | crate::Standard::PdfX4) {
        return Ok(bytes);
    }
    let fallback;
    let icc = match cmyk_icc {
        Some(icc) => icc,
        None if uses_device_cmyk(&bytes)? => {
            let (_, profile, warning) = crate::pdfx::print_settings_and_profile(false)?;
            warnings.extend(warning);
            fallback = profile;
            &fallback
        }
        None => return Ok(bytes),
    };
    apply(&bytes, icc, standard == crate::Standard::PdfX4)
}

/// Returns whether any page or nested graphic in `bytes` uses `DeviceCMYK` (directly, as a
/// transparency group, image, shading, tiling pattern, special color space alternate/base, or
/// Type3 font glyph).
pub(crate) fn uses_device_cmyk(bytes: &[u8]) -> Result<bool> {
    let pdf = Pdf::new(bytes.to_vec()).map_err(|_| damaged())?;
    let mut scan = CmykScan::default();
    for page in pdf.pages().iter() {
        if scan.page(page)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Returns the first 4-channel `/DestOutputProfile` stream ID from `/OutputIntents`.
fn output_intent_profile(pdf: &Pdf) -> Option<ObjectIdentifier> {
    pdfx_intents(pdf).into_iter().find_map(|oi| {
        let profile = oi.get::<Stream<'_>>(b"DestOutputProfile")?;
        (profile.dict().get::<i32>(b"N") == Some(4)).then(|| profile.obj_id())
    })
}

/// Returns the first object number `>= start` not already present in the cross-reference table.
fn next_object_id(pdf: &Pdf, start: i32) -> Result<ObjectIdentifier> {
    let mut id = start.max(1);
    for _ in 0..MAX_ID_PROBES {
        let candidate = ObjectIdentifier::new(id, 0);
        if pdf.xref().get::<Object<'_>>(candidate).is_none() {
            return Ok(candidate);
        }
        id = id.checked_add(1).ok_or_else(damaged)?;
    }
    Err(damaged())
}

fn encode_icc_stream(icc: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(icc).map_err(|_| damaged())?;
    let compressed = encoder.finish().map_err(|_| damaged())?;
    let mut out = format!("<</N 4/Filter/FlateDecode/Length {}>>\nstream\n", compressed.len()).into_bytes();
    out.extend_from_slice(&compressed);
    out.extend_from_slice(b"\nendstream");
    Ok(out)
}

/// Target object to update when a scope's `/Resources` dict is inline (or omitted on a page).
#[derive(Clone)]
enum ScopeOwner<'a> {
    Dict(ObjectIdentifier, Dict<'a>),
    Stream(Stream<'a>),
}

struct Updater<'a> {
    pdf: &'a Pdf,
    default_cmyk: String,
    changes: BTreeMap<ObjectIdentifier, Vec<u8>>,
    seen_res: HashSet<ObjectIdentifier>,
    seen_cs: HashSet<ObjectIdentifier>,
    seen_owners: HashSet<ObjectIdentifier>,
    seen_streams: HashSet<ObjectIdentifier>,
    seen_fonts: HashSet<ObjectIdentifier>,
}

impl<'a> Updater<'a> {
    fn new(pdf: &'a Pdf, icc_id: ObjectIdentifier) -> Self {
        Self {
            pdf,
            default_cmyk: format!("[/ICCBased {} {} R]", icc_id.obj_number, icc_id.gen_number),
            changes: BTreeMap::new(),
            seen_res: HashSet::new(),
            seen_cs: HashSet::new(),
            seen_owners: HashSet::new(),
            seen_streams: HashSet::new(),
            seen_fonts: HashSet::new(),
        }
    }

    fn visit_page(&mut self, page: &'a Page<'a>) -> Result<()> {
        let raw = page.raw();
        if has_entry(raw, b"Resources") {
            let page_id = raw.obj_id().ok_or_else(damaged)?;
            let owner = ScopeOwner::Dict(page_id, raw.clone());
            return self.visit_resource_owner(raw, Some(owner), 0);
        }

        // Pages inherit `/Resources` from their nearest `/Parent` ancestor that defines one.
        let mut parent_ref = raw.get_ref(b"Parent");
        let mut seen_parents = HashSet::new();
        for _ in 0..MAX_DEPTH {
            let Some(r) = parent_ref else { break };
            let parent_id: ObjectIdentifier = r.into();
            if !seen_parents.insert(parent_id) {
                return Err(damaged());
            }
            let parent = self.pdf.xref().get::<Dict<'_>>(parent_id).ok_or_else(damaged)?;
            if has_entry(&parent, b"Resources") {
                let owner = ScopeOwner::Dict(parent_id, parent.clone());
                return self.visit_resource_owner(&parent, Some(owner), 0);
            }
            parent_ref = parent.get_ref(b"Parent");
        }
        if parent_ref.is_some() {
            return Err(damaged());
        }

        // Neither the page nor any ancestor had `/Resources`: attach one directly to the page.
        let page_id = raw.obj_id().ok_or_else(damaged)?;
        if !self.seen_owners.insert(page_id) {
            return Ok(());
        }
        let res_bytes = format!("<</ColorSpace<</DefaultCMYK {}>>>>", self.default_cmyk);
        let updated = edited_with(raw, &[(b"Resources", res_bytes.as_bytes())], damaged)?;
        self.changes.insert(page_id, updated);
        Ok(())
    }

    fn visit_resource_owner(&mut self, owner_dict: &Dict<'a>, owner: Option<ScopeOwner<'a>>, depth: usize) -> Result<()> {
        if depth >= MAX_DEPTH {
            return Err(depth_limit());
        }
        if !has_entry(owner_dict, b"Resources") {
            return Ok(());
        }
        let res_ref = owner_dict.get_ref(b"Resources").map(ObjectIdentifier::from);
        if let Some(id) = res_ref
            && !self.seen_res.insert(id)
        {
            return Ok(());
        }
        let res_dict = owner_dict.get::<Dict<'a>>(b"Resources").ok_or_else(damaged)?;
        self.ensure_default_cmyk(&res_dict, res_ref, owner)?;
        self.walk_children(&res_dict, depth)
    }

    fn ensure_default_cmyk(&mut self, res_dict: &Dict<'a>, res_ref: Option<ObjectIdentifier>, owner: Option<ScopeOwner<'a>>) -> Result<()> {
        let cs_opt = if has_entry(res_dict, b"ColorSpace") { Some(res_dict.get::<Dict<'a>>(b"ColorSpace").ok_or_else(damaged)?) } else { None };
        if cs_opt.as_ref().is_some_and(|cs| has_entry(cs, b"DefaultCMYK")) {
            return Ok(());
        }

        if let Some(cs_ref) = res_dict.get_ref(b"ColorSpace").map(ObjectIdentifier::from) {
            if !self.seen_cs.insert(cs_ref) {
                return Ok(());
            }
            let cs_dict = cs_opt.as_ref().ok_or_else(damaged)?;
            let updated_cs = edited_with(cs_dict, &[(b"DefaultCMYK", self.default_cmyk.as_bytes())], damaged)?;
            self.changes.insert(cs_ref, updated_cs);
            return Ok(());
        }

        let new_cs = match cs_opt.as_ref() {
            Some(cs_dict) => edited_with(cs_dict, &[(b"DefaultCMYK", self.default_cmyk.as_bytes())], damaged)?,
            None => format!("<</DefaultCMYK {}>>", self.default_cmyk).into_bytes(),
        };

        if let Some(res_id) = res_ref {
            let updated_res = edited_with(res_dict, &[(b"ColorSpace", new_cs.as_slice())], damaged)?;
            self.changes.insert(res_id, updated_res);
            return Ok(());
        }

        if let Some(owner) = owner {
            let new_res = edited_with(res_dict, &[(b"ColorSpace", new_cs.as_slice())], damaged)?;
            self.write_inline_resources(owner, &new_res)?;
        }
        Ok(())
    }

    fn write_inline_resources(&mut self, owner: ScopeOwner<'a>, new_res: &[u8]) -> Result<()> {
        match owner {
            ScopeOwner::Dict(id, dict) => {
                if !self.seen_owners.insert(id) {
                    return Ok(());
                }
                let updated = edited_with(&dict, &[(b"Resources", new_res)], damaged)?;
                self.changes.insert(id, updated);
            }
            ScopeOwner::Stream(stream) => {
                let id = stream.obj_id();
                if !self.seen_owners.insert(id) {
                    return Ok(());
                }
                let raw = stream.raw_data();
                let len_str = raw.len().to_string();
                let dict = edited_with(stream.dict(), &[(b"Resources", new_res), (b"Length", len_str.as_bytes())], damaged)?;
                let mut body = dict;
                body.extend_from_slice(b"\nstream\n");
                body.extend_from_slice(&raw);
                body.extend_from_slice(b"\nendstream");
                self.changes.insert(id, body);
            }
        }
        Ok(())
    }

    fn walk_children(&mut self, res: &Dict<'a>, depth: usize) -> Result<()> {
        let next = depth + 1;

        if let Some(xobjects) = res.get::<Dict<'a>>(b"XObject") {
            for key in xobjects.keys() {
                if let Some(stream) = xobjects.get::<Stream<'a>>(key.as_ref())
                    && stream.dict().get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Form")
                {
                    self.visit_stream(&stream, next)?;
                }
            }
        }

        if let Some(extgstate) = res.get::<Dict<'a>>(b"ExtGState") {
            for key in extgstate.keys() {
                if let Some(gs) = extgstate.get::<Dict<'a>>(key.as_ref())
                    && let Some(smask) = gs.get::<Dict<'a>>(b"SMask")
                    && let Some(form) = smask.get::<Stream<'a>>(b"G")
                {
                    self.visit_stream(&form, next)?;
                }
            }
        }

        if let Some(patterns) = res.get::<Dict<'a>>(b"Pattern") {
            for key in patterns.keys() {
                if let Some(stream) = patterns.get::<Stream<'a>>(key.as_ref()) {
                    self.visit_stream(&stream, next)?;
                }
            }
        }

        if let Some(fonts) = res.get::<Dict<'a>>(b"Font") {
            for key in fonts.keys() {
                if let Some(font) = fonts.get::<Dict<'a>>(key.as_ref())
                    && font.get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Type3")
                {
                    // Never fall back to `font.obj_id()` for inline Type3 dicts: it returns the enclosing object.
                    let font_id = fonts.get_ref(key.as_ref()).map(ObjectIdentifier::from);
                    if let Some(id) = font_id
                        && !self.seen_fonts.insert(id)
                    {
                        continue;
                    }
                    if next >= MAX_DEPTH {
                        return Err(depth_limit());
                    }
                    let owner = font_id.map(|id| ScopeOwner::Dict(id, font.clone()));
                    self.visit_resource_owner(&font, owner, next)?;
                }
            }
        }
        Ok(())
    }

    fn visit_stream(&mut self, stream: &Stream<'a>, depth: usize) -> Result<()> {
        if !self.seen_streams.insert(stream.obj_id()) {
            return Ok(());
        }
        if depth >= MAX_DEPTH {
            return Err(depth_limit());
        }
        let owner = ScopeOwner::Stream(stream.clone());
        self.visit_resource_owner(stream.dict(), Some(owner), depth)
    }
}

fn is_device_cmyk(cs: &Object<'_>, res: &Resources<'_>, depth: usize) -> Result<bool> {
    if depth >= MAX_DEPTH {
        return Err(depth_limit());
    }
    match cs {
        Object::Name(n) => match n.as_ref() {
            b"DeviceCMYK" | b"CMYK" => Ok(true),
            b"DeviceRGB" | b"RGB" | b"DeviceGray" | b"G" | b"CalGray" | b"CalRGB" | b"Lab" | b"Pattern" | b"DefaultCMYK" => Ok(false),
            other => match res.get_color_space(&Name::new_unescaped(other)) {
                Some(named) if !matches!(named, Object::Name(ref m) if m.as_ref() == other) => is_device_cmyk(&named, res, depth + 1),
                _ => Ok(false),
            },
        },
        Object::Array(a) => {
            let mut items = a.iter::<Object<'_>>();
            let Some(Object::Name(family)) = items.next() else {
                return Ok(false);
            };
            match family.as_ref() {
                b"DeviceCMYK" | b"CMYK" => Ok(true),
                b"Indexed" | b"I" | b"Pattern" => items.next().map_or(Ok(false), |base| is_device_cmyk(&base, res, depth + 1)),
                b"Separation" | b"DeviceN" => {
                    let _names = items.next();
                    items.next().map_or(Ok(false), |alt| is_device_cmyk(&alt, res, depth + 1))
                }
                _ => Ok(false),
            }
        }
        _ => Ok(false),
    }
}

type ResourceLayer<'a> = [&'a [u8]; 6];

#[derive(Hash, PartialEq, Eq)]
struct StreamKey<'a> {
    id: ObjectIdentifier,
    resources: Vec<ResourceLayer<'a>>,
}

impl<'a> StreamKey<'a> {
    fn new(id: ObjectIdentifier, res: &Resources<'a>) -> Self {
        let mut resources = Vec::new();
        let mut cur = Some(res);
        while let Some(r) = cur {
            resources.push([r.color_spaces.data(), r.shadings.data(), r.patterns.data(), r.ext_g_states.data(), r.x_objects.data(), r.fonts.data()]);
            cur = r.parent();
        }
        Self { id, resources }
    }
}

fn find_font_with_id<'a>(res: &Resources<'a>, name: &Name<'_>) -> Option<(Dict<'a>, Option<ObjectIdentifier>)> {
    let mut cur = Some(res);
    while let Some(r) = cur {
        if let Some(font) = r.fonts.get::<Dict<'a>>(name.as_ref()) {
            let id = r.fonts.get_ref(name.as_ref()).map(ObjectIdentifier::from);
            return Some((font, id));
        }
        cur = r.parent();
    }
    None
}

#[derive(Default)]
struct CmykScan<'a> {
    active_streams: HashSet<ObjectIdentifier>,
    active_fonts: HashSet<ObjectIdentifier>,
    completed_false_streams: HashSet<StreamKey<'a>>,
    completed_false_fonts: HashSet<StreamKey<'a>>,
    scan_calls: usize,
}

impl<'a> CmykScan<'a> {
    fn page(&mut self, page: &'a Page<'a>) -> Result<bool> {
        let res = page.resources();
        if let Some(g) = page.raw().get::<Dict<'_>>(b"Group")
            && Self::group(&g, res)?
        {
            return Ok(true);
        }
        let Some(data) = page.page_stream() else {
            return Ok(false);
        };
        let mut hit_cycle = false;
        self.content(data, res, 0, &mut hit_cycle)
    }

    fn group(group: &Dict<'_>, res: &Resources<'_>) -> Result<bool> {
        match group.get::<Object<'_>>(b"CS") {
            Some(cs) => is_device_cmyk(&cs, res, 0),
            None => Ok(false),
        }
    }

    fn content(&mut self, data: &[u8], res: &Resources<'a>, depth: usize, hit_cycle: &mut bool) -> Result<bool> {
        if depth >= MAX_DEPTH {
            return Err(depth_limit());
        }
        let mut ops = UntypedIter::new(data);
        while let Some(op) = ops.next() {
            let operator: &[u8] = op.operator;
            let last = op.operands().last().cloned();
            let name = || last.clone().and_then(Object::into_name);
            match operator {
                b"k" | b"K" => return Ok(true),
                b"cs" | b"CS" => {
                    if let Some(n) = name()
                        && is_device_cmyk(&Object::Name(n), res, 0)?
                    {
                        return Ok(true);
                    }
                }
                b"sh" => {
                    if let Some(sh) = name().and_then(|n| res.get_shading(&n))
                        && Self::shading(sh, res)?
                    {
                        return Ok(true);
                    }
                }
                b"scn" | b"SCN" => {
                    if let Some(p) = name().and_then(|n| res.get_pattern(&n))
                        && self.pattern(p, res, depth, hit_cycle)?
                    {
                        return Ok(true);
                    }
                }
                b"gs" => {
                    let form = name()
                        .and_then(|n| res.get_ext_g_state(&n))
                        .and_then(|gs| gs.get::<Dict<'_>>(b"SMask"))
                        .and_then(|m| m.get::<Stream<'a>>(b"G"));
                    if let Some(form) = form
                        && self.form(&form, res, depth, hit_cycle)?
                    {
                        return Ok(true);
                    }
                }
                b"Do" => {
                    if let Some(xo) = name().and_then(|n| res.get_x_object(&n)) {
                        let hit = match xo.dict().get::<Name<'_>>(b"Subtype").as_deref() {
                            Some(b"Image") => Self::image(&xo, res)?,
                            Some(b"Form") => self.form(&xo, res, depth, hit_cycle)?,
                            _ => false,
                        };
                        if hit {
                            return Ok(true);
                        }
                    }
                }
                b"BI" | b"EI" => {
                    if let Some(Object::Stream(img)) = last
                        && Self::inline_image(&img, res)?
                    {
                        return Ok(true);
                    }
                }
                b"Tf" => {
                    if let Some(font_name) = op.operands().next().cloned().and_then(Object::into_name)
                        && let Some((font, font_id)) = find_font_with_id(res, &font_name)
                        && font.get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Type3")
                        && self.type3(&font, font_id, res, depth, hit_cycle)?
                    {
                        return Ok(true);
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn image(img: &Stream<'_>, res: &Resources<'_>) -> Result<bool> {
        if img.dict().get::<bool>(b"ImageMask") == Some(true) {
            return Ok(false);
        }
        match img.dict().get::<Object<'_>>(b"ColorSpace") {
            Some(cs) => is_device_cmyk(&cs, res, 0),
            None => Ok(false),
        }
    }

    fn inline_image(img: &Stream<'_>, res: &Resources<'_>) -> Result<bool> {
        if img.dict().get::<bool>(b"IM") == Some(true) || img.dict().get::<bool>(b"ImageMask") == Some(true) {
            return Ok(false);
        }
        let cs = img.dict().get::<Object<'_>>(b"CS").or_else(|| img.dict().get::<Object<'_>>(b"ColorSpace"));
        match cs {
            Some(cs) => is_device_cmyk(&cs, res, 0),
            None => Ok(false),
        }
    }

    fn shading(sh: Object<'_>, res: &Resources<'_>) -> Result<bool> {
        let dict = match sh {
            Object::Dict(d) => d,
            Object::Stream(s) => s.dict().clone(),
            _ => return Ok(false),
        };
        match dict.get::<Object<'_>>(b"ColorSpace") {
            Some(cs) => is_device_cmyk(&cs, res, 0),
            None => Ok(false),
        }
    }

    fn pattern(&mut self, pat: Object<'a>, res: &Resources<'a>, depth: usize, hit_cycle: &mut bool) -> Result<bool> {
        match pat {
            Object::Dict(d) => match d.get::<Object<'_>>(b"Shading") {
                Some(sh) => Self::shading(sh, res),
                None => Ok(false),
            },
            Object::Stream(s) => self.form(&s, res, depth, hit_cycle),
            _ => Ok(false),
        }
    }

    fn scan_stream(&mut self, stream: &Stream<'a>, res: Resources<'a>, depth: usize, hit_cycle: &mut bool, check_group: bool) -> Result<bool> {
        if self.scan_calls >= MAX_SCAN_STREAMS {
            return Err(scan_limit());
        }
        self.scan_calls += 1;
        let id = stream.obj_id();
        if self.active_streams.contains(&id) {
            *hit_cycle = true;
            return Ok(false);
        }
        if depth >= MAX_DEPTH {
            return Err(depth_limit());
        }
        let key = StreamKey::new(id, &res);
        if self.completed_false_streams.contains(&key) {
            return Ok(false);
        }
        if check_group
            && let Some(g) = stream.dict().get::<Dict<'_>>(b"Group")
            && Self::group(&g, &res)?
        {
            return Ok(true);
        }
        let data = stream.decoded().map_err(|_| damaged())?;
        self.active_streams.insert(id);
        let mut inner_cycle = false;
        let res_found = self.content(&data, &res, depth, &mut inner_cycle);
        self.active_streams.remove(&id);
        if inner_cycle {
            *hit_cycle = true;
        }
        let found = res_found?;
        if !found && !inner_cycle {
            self.completed_false_streams.insert(key);
        }
        Ok(found)
    }

    fn form(&mut self, form: &Stream<'a>, parent: &Resources<'a>, depth: usize, hit_cycle: &mut bool) -> Result<bool> {
        let res = Resources::from_scope(form.dict().get::<Dict<'a>>(b"Resources"), parent.clone());
        self.scan_stream(form, res, depth + 1, hit_cycle, true)
    }

    fn type3(
        &mut self,
        font: &Dict<'a>,
        font_id: Option<ObjectIdentifier>,
        parent: &Resources<'a>,
        depth: usize,
        hit_cycle: &mut bool,
    ) -> Result<bool> {
        if let Some(id) = font_id
            && self.active_fonts.contains(&id)
        {
            *hit_cycle = true;
            return Ok(false);
        }
        if depth + 1 >= MAX_DEPTH {
            return Err(depth_limit());
        }
        let res = Resources::from_scope(font.get::<Dict<'a>>(b"Resources"), parent.clone());
        let key = font_id.map(|id| StreamKey::new(id, &res));
        if let Some(ref k) = key
            && self.completed_false_fonts.contains(k)
        {
            return Ok(false);
        }
        let Some(procs) = font.get::<Dict<'a>>(b"CharProcs") else {
            return Ok(false);
        };
        if let Some(id) = font_id {
            self.active_fonts.insert(id);
        }
        let mut inner_cycle = false;
        let mut found = Ok(false);
        for proc_key in procs.keys() {
            if let Some(glyph) = procs.get::<Stream<'a>>(proc_key.as_ref()) {
                match self.scan_stream(&glyph, res.clone(), depth + 1, &mut inner_cycle, false) {
                    Ok(true) => {
                        found = Ok(true);
                        break;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        found = Err(e);
                        break;
                    }
                }
            }
        }
        if let Some(id) = font_id {
            self.active_fonts.remove(&id);
        }
        if inner_cycle {
            *hit_cycle = true;
        }
        let hit = found?;
        if !hit
            && !inner_cycle
            && let Some(k) = key
        {
            self.completed_false_fonts.insert(k);
        }
        Ok(hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_cms::builtin::COATED_FOGRA39_ICC;

    fn build_pdf(objects: &[Vec<u8>], trailer_extra: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize];
        for (idx, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", idx + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        let size = objects.len() + 1;
        out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
        for offset in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<</Size {size}/Root 1 0 R{trailer_extra}>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }

    fn read_default_cmyk<'a>(res: &'a Resources<'a>) -> Option<(i32, Vec<u8>)> {
        let cs = res.get_color_space(&Name::new_unescaped(b"DefaultCMYK"))?.into_array()?;
        let mut items = cs.iter::<Object<'_>>();
        let tag = items.next()?.into_name()?;
        assert_eq!(tag.as_ref(), b"ICCBased");
        let stream = items.next()?.into_stream()?;
        assert_eq!(stream.dict().get::<i32>(b"N"), Some(4));
        Some((stream.obj_id().obj_number, stream.decoded().ok()?.into_owned()))
    }

    #[test]
    fn inline_resources_inherited_pages_preserved_existing_and_cyclic_forms() {
        let generic = designcraft_color::cms::Cms::new(&designcraft_color::cms::ColorSettings {
            cmyk: designcraft_color::cms::GENERIC_CMYK.into(),
            ..Default::default()
        })
        .unwrap();
        let custom_icc = generic.output_intent().unwrap().icc.into_owned();
        assert_ne!(custom_icc, COATED_FOGRA39_ICC);
        // 1: Catalog
        // 2: Pages (carries inherited /Resources with existing /ColorSpace << /CS0 ... >>)
        // 3: Page 1 (inherits /Resources from Pages 2)
        // 4: Page 2 (inline /Resources with inline Form XObject 6, preserves content)
        // 5: Page 3 (already defines /DefaultCMYK pointing to 7)
        // 6: Cyclic Form XObject (inline /Resources with XObject /Self -> 6)
        // 7: Pre-existing custom ICC stream (tagged /N 4)
        let form =
            b"<</Type/XObject/Subtype/Form/BBox[0 0 10 10]/Resources<</XObject<</Self 6 0 R>>>>/Length 8>>\nstream\n0 0 0 1 k\nendstream".to_vec();
        let existing_icc = b"<</N 4/Length 4>>\nstream\ntest\nendstream".to_vec();
        let pdf_bytes = build_pdf(
            &[
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R 4 0 R 5 0 R]/Count 3/Resources<</ColorSpace<</CS0/DeviceGray>>>>>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</XObject<</Fm0 6 0 R>>>>>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</ColorSpace<</DefaultCMYK[/ICCBased 7 0 R]>>>>>>".to_vec(),
                form,
                existing_icc,
            ],
            "",
        );

        let updated = apply(&pdf_bytes, &custom_icc, false).unwrap();
        let parsed = Pdf::new(updated.clone()).unwrap();

        // Inherited `/Pages` dict got DefaultCMYK while keeping CS0.
        let (p1_id, p1_prof) = read_default_cmyk(parsed.pages()[0].resources()).expect("page 1 inherits DefaultCMYK");
        assert_eq!(p1_prof, custom_icc);
        assert!(parsed.pages()[0].resources().get_color_space(&Name::new_unescaped(b"CS0")).is_some());

        // Page 2 inline Resources + Form 6 inline Resources share the exact same embedded ICC stream id.
        let (p2_id, _) = read_default_cmyk(parsed.pages()[1].resources()).expect("page 2 DefaultCMYK");
        assert_eq!(p1_id, p2_id);
        let form_obj = parsed.pages()[1].resources().get_x_object(&Name::new_unescaped(b"Fm0")).unwrap();
        let form_res = Resources::from_parent(form_obj.dict().get::<Dict<'_>>(b"Resources").unwrap(), parsed.pages()[1].resources().clone());
        let (f_id, f_prof) = read_default_cmyk(&form_res).expect("cyclic form DefaultCMYK");
        assert_eq!(f_id, p1_id);
        assert_eq!(f_prof, custom_icc);

        // Page 3's pre-existing /DefaultCMYK was preserved (kept pointing to 7).
        let (p3_id, p3_prof) = read_default_cmyk(parsed.pages()[2].resources()).expect("page 3 kept existing DefaultCMYK");
        assert_eq!(p3_id, 7);
        assert_eq!(p3_prof, b"test");

        // Idempotent when applied again after all scopes already have /DefaultCMYK.
        let second = apply(&updated, &custom_icc, false).unwrap();
        assert_eq!(second, updated);
    }

    #[test]
    fn excessive_form_nesting_is_bounded_and_never_overflows() {
        let make_nested = |levels: usize| {
            let mut objects = vec![
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 4 0 R/Resources<</XObject<</F0 5 0 R>>>>>>".to_vec(),
                b"<</Length 6>>\nstream\n/F0 Do\nendstream".to_vec(),
            ];
            for i in 0..levels {
                let next_obj = objects.len() + 2;
                let body = if i + 1 < levels {
                    format!(
                        "<</Type/XObject/Subtype/Form/BBox[0 0 1 1]/Resources<</XObject<</F {next_obj} 0 R>>>>/Length 5>>\nstream\n/F Do\nendstream"
                    )
                } else {
                    "<</Type/XObject/Subtype/Form/BBox[0 0 1 1]/Resources<<>>/Length 0>>\nstream\n\nendstream".to_string()
                };
                objects.push(body.into_bytes());
            }
            build_pdf(&objects, "")
        };

        // Just inside MAX_DEPTH (31 Form levels beneath Page at depth 0) succeeds.
        let valid_bytes = make_nested(MAX_DEPTH - 1);
        let updated = apply(&valid_bytes, COATED_FOGRA39_ICC, false).unwrap();
        let parsed = Pdf::new(updated).unwrap();
        assert!(read_default_cmyk(parsed.pages()[0].resources()).is_some());
        assert!(!uses_device_cmyk(&valid_bytes).unwrap());

        // Past MAX_DEPTH (40 non-cyclic Form levels) fails explicitly instead of silently truncating.
        let deep_bytes = make_nested(40);
        assert!(apply(&deep_bytes, COATED_FOGRA39_ICC, false).is_err());
        assert!(uses_device_cmyk(&deep_bytes).is_err());
    }

    #[test]
    fn encrypted_and_damaged_pdfs_fail_gracefully() {
        let enc = build_pdf(
            &[
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 10 10]>>".to_vec(),
            ],
            "/Encrypt 99 0 R",
        );
        assert!(apply(&enc, COATED_FOGRA39_ICC, false).is_err());
        assert!(apply(b"not a pdf", COATED_FOGRA39_ICC, false).is_err());
        assert!(
            apply(
                &build_pdf(
                    &[
                        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 10 10]>>".to_vec()
                    ],
                    ""
                ),
                &[],
                false
            )
            .is_err()
        );
        assert!(uses_device_cmyk(b"not a pdf").is_err());
    }

    #[test]
    fn uses_device_cmyk_distinguishes_pure_rgb_from_cmyk() {
        let make = |cs_dict: &str, content: &[u8]| {
            build_pdf(
                &[
                    b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                    b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                    format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 4 0 R/Resources<</ColorSpace<<{cs_dict}>>>>>>").into_bytes(),
                    format!("<</Length {}>>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)).into_bytes(),
                ],
                "",
            )
        };
        assert!(!uses_device_cmyk(&make("", b"1 0 0 rg 0 0 10 10 re f")).unwrap());
        assert!(uses_device_cmyk(&make("", b"0 0 0 1 k 0 0 10 10 re f")).unwrap());
        assert!(uses_device_cmyk(&make("", b"/DeviceCMYK cs 1 0 0 0 scn 0 0 10 10 re f")).unwrap());

        let sep_rgb = "/CS0[/Separation/Gold/DeviceRGB<</FunctionType 2/Domain[0 1]/C0[0 0 0]/C1[0 0 1]/N 1>>]";
        let sep_cmyk = "/CS0[/Separation/Gold/DeviceCMYK<</FunctionType 2/Domain[0 1]/C0[0 0 0 0]/C1[0 0.3 1 0]/N 1>>]";
        let devn_cmyk = "/CS0[/DeviceN[/Gold]/DeviceCMYK<</FunctionType 2/Domain[0 1]/C0[0 0 0 0]/C1[0 0.3 1 0]/N 1>>]";
        let idx_sep_cmyk = "/CS0[/Indexed[/Separation/Gold/DeviceCMYK<</FunctionType 2/Domain[0 1]/C0[0 0 0 0]/C1[0 0.3 1 0]/N 1>>]1 <00FF>]";
        assert!(!uses_device_cmyk(&make(sep_rgb, b"/CS0 cs 1 scn 0 0 10 10 re f")).unwrap());
        assert!(uses_device_cmyk(&make(sep_cmyk, b"/CS0 cs 1 scn 0 0 10 10 re f")).unwrap());
        assert!(uses_device_cmyk(&make(devn_cmyk, b"/CS0 cs 1 scn 0 0 10 10 re f")).unwrap());
        assert!(uses_device_cmyk(&make(idx_sep_cmyk, b"/CS0 cs 1 scn 0 0 10 10 re f")).unwrap());
        assert!(uses_device_cmyk(&make("/CS0[/Pattern/DeviceCMYK]", b"/CS0 cs 0 0 10 10 re f")).unwrap());
    }

    #[test]
    fn smask_tiling_pattern_type3_and_indirect_colorspace_receive_default_cmyk() {
        // 1: Catalog, 2: Pages, 3: Page (Resources -> 4)
        // 4: Page Resources (indirect ColorSpace -> 5, ExtGState /GS with SMask /G -> 6, Pattern /P1 -> 7, Font /F3 -> 8)
        // 5: Indirect ColorSpace (preserves /Sep while adding /DefaultCMYK)
        // 6: SMask Form (indirect Resources -> 9)
        // 7: TilingPattern (inline Resources with /ColorSpace <<>>)
        // 8: Type3 Font (inline Resources with /ColorSpace <<>>, CharProcs /a -> 11)
        // 9: SMask Resources
        // 10: Page content stream drawing Type3 glyph `(a)`
        // 11: Type3 CharProc stream drawing in DeviceCMYK (`0 0 0 1 k`)
        let pdf_bytes = build_pdf(
            &[
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 10 0 R/Resources 4 0 R>>".to_vec(),
                b"<</ColorSpace 5 0 R/ExtGState<</GS<</SMask<</S/Luminosity/G 6 0 R>>>>>>/Pattern<</P1 7 0 R>>/Font<</F3 8 0 R>>>>".to_vec(),
                b"<</Sep/DeviceGray>>".to_vec(),
                b"<</Type/XObject/Subtype/Form/BBox[0 0 10 10]/Resources 9 0 R/Length 0>>\nstream\n\nendstream".to_vec(),
                b"<</Type/Pattern/PatternType 1/PaintType 1/TilingType 1/BBox[0 0 5 5]/XStep 5/YStep 5/Resources<</ColorSpace<<>>>>/Length 0>>\nstream\n\nendstream".to_vec(),
                b"<</Type/Font/Subtype/Type3/FontBBox[0 0 10 10]/FontMatrix[0.1 0 0 0.1 0 0]/CharProcs<</a 11 0 R>>/Encoding<</Differences[97/a]>>/FirstChar 97/LastChar 97/Widths[10]/Resources<</ColorSpace<<>>>>>>".to_vec(),
                b"<</ColorSpace<<>>>>".to_vec(),
                b"<</Length 15>>\nstream\n/F3 10 Tf (a) Tj\nendstream".to_vec(),
                b"<</Length 19>>\nstream\n0 0 0 1 k 0 0 5 5 re f\nendstream".to_vec(),
            ],
            "",
        );
        assert!(uses_device_cmyk(&pdf_bytes).unwrap(), "Type3 glyph CMYK usage should be detected");
        let updated = apply(&pdf_bytes, COATED_FOGRA39_ICC, false).unwrap();
        let parsed = Pdf::new(updated).unwrap();
        let page_res = parsed.pages()[0].resources();
        let (p_id, _) = read_default_cmyk(page_res).expect("page indirect ColorSpace DefaultCMYK");
        assert!(page_res.get_color_space(&Name::new_unescaped(b"Sep")).is_some(), "existing indirect ColorSpace entry kept");

        let smask =
            page_res.get_ext_g_state(&Name::new_unescaped(b"GS")).unwrap().get::<Dict<'_>>(b"SMask").unwrap().get::<Stream<'_>>(b"G").unwrap();
        let smask_res = Resources::from_parent(smask.dict().get::<Dict<'_>>(b"Resources").unwrap(), page_res.clone());
        assert_eq!(read_default_cmyk(&smask_res).expect("SMask form DefaultCMYK").0, p_id);

        let pat = match page_res.get_pattern(&Name::new_unescaped(b"P1")).unwrap() {
            Object::Stream(s) => s,
            other => panic!("expected pattern stream, got {other:?}"),
        };
        let pat_res = Resources::from_parent(pat.dict().get::<Dict<'_>>(b"Resources").unwrap(), page_res.clone());
        assert_eq!(read_default_cmyk(&pat_res).expect("TilingPattern DefaultCMYK").0, p_id);

        let font = page_res.get_font(&Name::new_unescaped(b"F3")).unwrap();
        let font_res = Resources::from_parent(font.get::<Dict<'_>>(b"Resources").unwrap(), page_res.clone());
        assert_eq!(read_default_cmyk(&font_res).expect("Type3 font DefaultCMYK").0, p_id);
    }

    #[test]
    fn inline_type3_font_does_not_overwrite_container_resources() {
        // Object 4 is an indirect /Resources dictionary containing two inline Type3 fonts:
        // /F3 with inline /Resources (must not corrupt object 4) and /F4 with indirect /Resources 5 0 R.
        let type3 = |extra: &str| {
            format!(
                "<</Type/Font/Subtype/Type3/FontBBox[0 0 10 10]/FontMatrix[0.1 0 0 0.1 0 0]/CharProcs<</a 8 0 R>>/Encoding<</Differences[97/a]>>/FirstChar 97/LastChar 97/Widths[10]/Resources {extra}>>"
            )
        };
        let res4 = format!("<</Font<</F3 {}/F4 {}>>/XObject<</Fm 6 0 R>>>>", type3("<</ColorSpace<<>>>>"), type3("5 0 R"));
        let pdf_bytes = build_pdf(
            &[
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 7 0 R/Resources 4 0 R>>".to_vec(),
                res4.into_bytes(),
                b"<</ColorSpace<<>>>>".to_vec(),
                b"<</Type/XObject/Subtype/Form/BBox[0 0 10 10]/Resources<</ColorSpace<<>>>>/Length 0>>\nstream\n\nendstream".to_vec(),
                b"<</Length 15>>\nstream\n/F3 10 Tf (a) Tj\nendstream".to_vec(),
                b"<</Length 19>>\nstream\n0 0 0 1 k 0 0 5 5 re f\nendstream".to_vec(),
            ],
            "",
        );
        assert!(uses_device_cmyk(&pdf_bytes).unwrap());
        let updated = apply(&pdf_bytes, COATED_FOGRA39_ICC, false).unwrap();
        let parsed = Pdf::new(updated).unwrap();
        let page_res = parsed.pages()[0].resources();
        let (p_id, _) = read_default_cmyk(page_res).expect("page resources retain ColorSpace and get DefaultCMYK");
        assert!(page_res.get_font(&Name::new_unescaped(b"F3")).is_some(), "container /Font /F3 preserved");
        assert!(page_res.get_x_object(&Name::new_unescaped(b"Fm")).is_some(), "container /XObject preserved");
        let f4 = page_res.get_font(&Name::new_unescaped(b"F4")).expect("container /Font /F4 preserved");
        let f4_res = Resources::from_scope(f4.get::<Dict<'_>>(b"Resources"), page_res.clone());
        assert_eq!(read_default_cmyk(&f4_res).expect("inline Type3 indirect Resources DefaultCMYK").0, p_id);
    }

    #[test]
    fn shared_form_without_resources_uses_each_pages_resources() {
        let two_page_shared_form = |page2_cs: &str| {
            // 1: Catalog, 2: Pages (2 kids), 3: Page 1 (CS0 = DeviceRGB, invokes Form 7),
            // 4: Page 2 (CS0 = `page2_cs`, invokes Form 7), 5: Page 1 content, 6: Page 2 content,
            // 7: Shared Form without /Resources using `/CS0 cs`.
            let page_stream = "/Fm Do";
            let form_stream = "/CS0 cs 1 0 0 0 scn 0 0 1 1 re f";
            build_pdf(
                &[
                    b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                    b"<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>".to_vec(),
                    b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 5 0 R/Resources<</ColorSpace<</CS0/DeviceRGB>>/XObject<</Fm 7 0 R>>>>>>".to_vec(),
                    format!(
                        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 6 0 R/Resources<</ColorSpace<</CS0{page2_cs}>>/XObject<</Fm 7 0 R>>>>>>"
                    )
                    .into_bytes(),
                    format!("<</Length {}>>\nstream\n{page_stream}\nendstream", page_stream.len()).into_bytes(),
                    format!("<</Length {}>>\nstream\n{page_stream}\nendstream", page_stream.len()).into_bytes(),
                    format!("<</Type/XObject/Subtype/Form/BBox[0 0 10 10]/Length {}>>\nstream\n{form_stream}\nendstream", form_stream.len())
                        .into_bytes(),
                ],
                "",
            )
        };
        assert!(!uses_device_cmyk(&two_page_shared_form("/DeviceRGB")).unwrap());
        assert!(
            uses_device_cmyk(&two_page_shared_form("/DeviceCMYK")).unwrap(),
            "Page 1 negative cache for resource-less Form must not hide Page 2 DeviceCMYK resolution"
        );
    }

    #[test]
    fn branching_self_referencing_form_chain_hits_scan_budget() {
        let make_chain = |levels: usize| {
            let xobj_entries: String = (0..levels).map(|i| format!("/F{i} {} 0 R", i + 5)).collect();
            let mut objects = vec![
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 4 0 R/Resources<</XObject<<{xobj_entries}>>>>>>").into_bytes(),
                b"<</Length 6>>\nstream\n/F0 Do\nendstream".to_vec(),
            ];
            for i in 0..levels {
                // Omit `/Resources` on each Form so it inherits the page scope rather than holding a
                // self-reference inside its own indirect object context.
                let ops = if i + 1 < levels {
                    let next = i + 1;
                    format!("/F{i} Do /F{next} Do /F{next} Do")
                } else {
                    format!("/F{i} Do")
                };
                objects.push(format!("<</Type/XObject/Subtype/Form/BBox[0 0 1 1]/Length {}>>\nstream\n{ops}\nendstream", ops.len()).into_bytes());
            }
            build_pdf(&objects, "")
        };

        let small = make_chain(4);
        assert!(!uses_device_cmyk(&small).unwrap());

        let deep_branching = make_chain(20);
        let err = uses_device_cmyk(&deep_branching).unwrap_err();
        assert_eq!(err.to_string(), scan_limit().to_string());
        let attach_err = attach(deep_branching, None, crate::Standard::None, &mut Vec::new()).unwrap_err();
        assert_eq!(attach_err.to_string(), scan_limit().to_string());
    }
}
