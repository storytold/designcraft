use super::frame::{Prefs, frame_grid_relative, text_frame_options, text_wrap};
use super::object_style::INDESIGN_CORNER_SIZE;
use super::package::uri_to_path;
use super::values::{nums, parse_affine, tint};
use super::{
    Affine, Anchor, Arc, Asset, AssetId, Content, Corner, CornerOptions, El, Fill, Importer, Item, ItemId, OBJECT_BUILTINS, PathData, Point, Shape,
    Story, StoryId, Stroke, SubPath, TextFrame, TextFrameOptions, base64_decode, names, sniff_image, swatch, unescape_id,
};

impl<'r> Importer<'r> {
    pub(super) fn item(&mut self, e: &El, parent: Affine) -> Option<Item> {
        let model_item = craft_idml::model::PageItem::from_el(&crate::xml::to_craft(e))?;
        let tag = model_item.kind.name();
        let shape = match &model_item.kind {
            craft_idml::model::ItemKind::Rectangle | craft_idml::model::ItemKind::TextFrame => Shape::Rectangle,
            craft_idml::model::ItemKind::Oval => Shape::Oval,
            craft_idml::model::ItemKind::Polygon => Shape::Polygon,
            craft_idml::model::ItemKind::GraphicLine => Shape::GraphicLine,
            craft_idml::model::ItemKind::Group => Shape::Group,
            _ => return None,
        };
        let id = ItemId(self.alloc());
        if let Some(s) = model_item.self_id() {
            self.item_ids.insert(s.to_string(), id);
        }
        let layer = model_item.item_layer().and_then(|l| self.layer_ids.get(l)).copied().unwrap_or_else(|| self.layers[0].id);
        let path = path_from_model(model_item.geometry());
        let mut it = Item::new(id, layer, shape, path);
        it.xf = parent * Affine::new(model_item.item_transform());
        it.name = model_item.name().filter(|n| *n != "$ID/").unwrap_or("").to_string();
        it.locked = model_item.locked();
        it.hidden = model_item.hidden();
        it.overrides = model_item.overridden_page_item().and_then(|s| self.item_ids.get(s)).copied().filter(|i| self.is_parent_item(*i));
        it.nonprinting = model_item.nonprinting();
        it.object_style = model_item
            .applied_object_style()
            .map(|r| {
                self.object_names
                    .get(r)
                    .cloned()
                    .unwrap_or_else(|| names::style_name_in(OBJECT_BUILTINS, &unescape_id(r.trim_start_matches("ObjectStyle/"))))
            })
            .unwrap_or_else(|| designcraft_doc::NO_OBJECT_STYLE.into());
        // Fill and stroke.
        if tag != "Group" {
            if let Some(f) = self.attr_or_style(e, "FillColor") {
                it.fill.swatch = self.swatch_ref(&f);
            }
            if let Some(t) = self.attr_or_style(e, "FillTint").and_then(|v| v.parse().ok()) {
                it.fill.tint = tint(Some(t)).unwrap_or(1.0);
            }
            it.fill.overprint = self.attr_or_style(e, "OverprintFill").as_deref() == Some("true");
            if let Some(a) = model_item.gradient_fill_angle()
                && a != 0.0
            {
                it.fill.gradient_angle = Some(a);
            }
            if let (Some([x, y]), Some(len)) = (model_item.gradient_fill_start(), model_item.gradient_fill_length())
                && len > 0.0
            {
                let a = model_item.gradient_fill_angle().unwrap_or(0.0).to_radians();
                it.fill.gradient_vector = Some([x, y, x + len * a.cos(), y - len * a.sin()]);
            }
            it.stroke = self.stroke_from(e, Some(e));
            if it.stroke.weight <= 0.0 {
                it.stroke.swatch = swatch::NONE.into();
                it.stroke.weight = 1.0;
            }
            // A gradient on the stroke: its angle, or the Gradient Swatch tool's vector.
            if let Some(a) = model_item.gradient_stroke_angle()
                && a != 0.0
            {
                it.stroke.gradient_angle = Some(a);
            }
            if let (Some([x, y]), Some(len)) = (model_item.gradient_stroke_start(), model_item.gradient_stroke_length())
                && len > 0.0
            {
                let a = model_item.gradient_stroke_angle().unwrap_or(0.0).to_radians();
                it.stroke.gradient_vector = Some([x, y, x + len * a.cos(), y - len * a.sin()]);
            }
        }
        // Corners: each corner's shape and radius come from the item, then its object style chain;
        // at each level a per-corner attribute wins over the legacy all-corners one.
        if tag != "Group" {
            let levels: Vec<&El> = std::iter::once(e).chain(self.object_style_chain(e)).collect();
            let corner =
                |n: &str, what: &str| levels.iter().find_map(|s| s.get(&format!("{n}Corner{what}")).or_else(|| s.get(&format!("Corner{what}"))));
            let mut corners = CornerOptions::default();
            for (i, n) in crate::export::corner_names(&it.path).iter().enumerate() {
                let shape = corner(n, "Option").map(names::corner_in).unwrap_or_default();
                let size = corner(n, "Radius").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(INDESIGN_CORNER_SIZE);
                corners.corners[i] = Corner { shape, size };
            }
            if !corners.is_none() {
                it.corners = corners;
            }
        }
        // Transparency.
        if let Some(t) = self.with_style(e, "TransparencySetting") {
            if let Some(bs) = t.find("BlendingSetting") {
                if let Some(o) = bs.num("Opacity") {
                    it.opacity = (o / 100.0).clamp(0.0, 1.0) as f32;
                }
                if let Some(m) = bs.get("BlendMode") {
                    it.blend = names::blend_in(m);
                }
            }
            self.effects_in(&t, &mut it.effects);
        }
        // Text wrap.
        if let Some(w) = self.with_style(e, "TextWrapPreference") {
            let mut tw = text_wrap(&w);
            if let Some(pg) = w.prop_el("PathGeometry") {
                match wrap_path_of(pg) {
                    Ok(p) => tw.path = Some(p),
                    Err(why) => log::warn!("IDML: custom text wrap boundary of {} dropped: {why}", model_item.self_id().unwrap_or("an item")),
                }
            }
            it.wrap = tw;
        }
        // Type on a path.
        if !matches!(&model_item.kind, craft_idml::model::ItemKind::TextFrame)
            && let Some(tp) = model_item.text_path()
        {
            let story_self = tp.parent_story().unwrap_or("n").to_string();
            if let Some(story) = self.story_ids.get(&story_self).copied() {
                let options = TextFrameOptions {
                    path: Some(designcraft_doc::PathType {
                        start: tp.start_bracket().unwrap_or(0.0).max(0.0),
                        // At (or past) the path's length it is the path's end: no end bracket.
                        end: tp
                            .end_bracket()
                            .filter(|e| e.is_finite() && *e >= 0.0)
                            .filter(|e| *e < designcraft_geom::warp::PathWarp::new(&it.path.to_bezpath(), false).length() - 0.01),
                        flip: tp.flipped(),
                        align: names::path_align_in(tp.alignment().unwrap_or("")),
                    }),
                    ..Default::default()
                };
                it.content = Content::Text(TextFrame { story, options });
                self.ctx.threads.push((id, story_self, "n".into(), "n".into()));
            }
        }
        self.read_content(e, &model_item, tag, id, &mut it);
        Some(it)
    }

    fn read_content(&mut self, e: &El, model_item: &craft_idml::model::PageItem, tag: &str, id: ItemId, item: &mut Item) {
        match tag {
            "TextFrame" => {
                let story_self = model_item.parent_story().unwrap_or("n").to_string();
                let story = match self.story_ids.get(&story_self) {
                    Some(s) => *s,
                    None => {
                        let sid = StoryId(self.alloc());
                        self.stories.insert(sid, Story::new(sid));
                        self.story_ids.insert(format!("#orphan{}", sid.0), sid);
                        sid
                    }
                };
                // Options the frame doesn't set come from its object style chain.
                let prefs: Vec<&El> = std::iter::once(e).chain(self.object_style_chain(e)).filter_map(|s| s.find("TextFramePreference")).collect();
                let mut options = text_frame_options(&prefs);
                // `prefs` borrows the object styles, so the rule colour's swatch is resolved after it.
                if let Some(r) = Prefs(&prefs).prop("ColumnRuleStrokeColor") {
                    options.column_rule_color = self.swatch_ref(&r);
                }
                if let Some(g) = self.with_style(e, "BaselineFrameGridOption")
                    && g.get("UseCustomBaselineFrameGrid") == Some("true")
                {
                    options.baseline_grid = Some(designcraft_doc::FrameBaselineGrid {
                        start: g.num("StartingOffsetForBaselineFrameGrid").unwrap_or(0.0),
                        increment: g.num("BaselineFrameGridIncrement").unwrap_or(12.0),
                        relative_to: g.get("BaselineFrameGridRelativeOption").map(frame_grid_relative).unwrap_or_default(),
                    });
                }
                item.content = Content::Text(TextFrame { story, options });
                self.ctx.threads.push((
                    id,
                    story_self,
                    model_item.previous_text_frame().unwrap_or("n").to_string(),
                    model_item.next_text_frame().unwrap_or("n").to_string(),
                ));
            }
            "Group" => {
                let mut kids = Vec::new();
                for child in model_item.items() {
                    let child = crate::xml::from_craft(child.to_el());
                    if let Some(k) = self.item(&child, Affine::IDENTITY) {
                        kids.push(Arc::new(k));
                    }
                }
                item.content = Content::Group { items: kids };
                item.fill = Fill::none();
                item.stroke = Stroke::none();
            }
            _ => {
                if let Some(graphic) = model_item.graphics().next() {
                    let graphic_xml = crate::xml::from_craft(graphic.to_el());
                    item.content = self.graphic(&graphic_xml);
                    let fill = self.image_fill(&graphic_xml);
                    if let Content::Graphic(gr) = &mut item.content {
                        gr.fill = fill;
                    }
                    if let (Some(ff), Content::Graphic(gr)) = (self.with_style(e, "FrameFittingOption"), &mut item.content) {
                        gr.crop = [ff.num("TopCrop"), ff.num("LeftCrop"), ff.num("BottomCrop"), ff.num("RightCrop")].map(|v| v.unwrap_or(0.0));
                        gr.fit_align = ff.get("FittingAlignment").map_or(4, names::anchor_in);
                        if ff.get("AutoFit") == Some("true") {
                            gr.auto_fit = ff.get("FittingOnEmptyFrame").map_or(designcraft_doc::Fitting::FillProportionally, names::fitting_in);
                            if gr.auto_fit == designcraft_doc::Fitting::None {
                                gr.auto_fit = designcraft_doc::Fitting::FillProportionally;
                            }
                        }
                    }
                } else {
                    // Page items pasted into this frame (Paste Into).
                    let kids: Vec<Arc<Item>> = e
                        .elements()
                        .filter(|c| matches!(c.local(), "Rectangle" | "Oval" | "Polygon" | "GraphicLine" | "Group" | "TextFrame"))
                        .filter_map(|c| self.item(c, Affine::IDENTITY))
                        .map(Arc::new)
                        .collect();
                    if !kids.is_empty() {
                        item.content = Content::Group { items: kids };
                    }
                }
            }
        }
    }

    /// The swatch applied to a placed image (`FillColor`, `FillTint`): it colours a greyscale or
    /// bitmap image. Black at 100% is how InDesign writes an image nobody coloured: no swatch.
    fn image_fill(&mut self, g: &El) -> Fill {
        let Some(r) = g.get("FillColor") else { return Fill::none() };
        let swatch = self.swatch_ref(r);
        let tint = tint(g.num("FillTint")).unwrap_or(1.0);
        if swatch == swatch::BLACK && tint >= 1.0 {
            return Fill::none();
        }
        Fill { swatch, tint, ..Fill::none() }
    }

    fn graphic(&mut self, g: &El) -> Content {
        let layer_options = g.find("GraphicLayerOption").map(|options| {
            let attrs = |e: &El| e.attrs.iter().filter(|(key, _)| key != "Self").cloned().collect();
            let mut out = designcraft_doc::GraphicLayerOptions { attributes: attrs(options), ..Default::default() };
            let mut pending: Vec<_> = options.find_all("GraphicLayer").map(|e| (e, None)).collect();
            pending.reverse();
            while let Some((layer, parent)) = pending.pop() {
                let index = out.layers.len();
                out.layers.push(attrs(layer));
                out.parents.push(parent);
                let children: Vec<_> = layer.find_all("GraphicLayer").collect();
                pending.extend(children.into_iter().rev().map(|e| (e, Some(index))));
            }
            if out.parents.iter().all(Option::is_none) {
                out.parents.clear();
            }
            out
        });
        if layer_options.as_ref().is_some_and(designcraft_doc::GraphicLayerOptions::has_visibility_overrides) {
            self.warnings.push(format!("graphic {}: {}", g.get("Self").unwrap_or("(unnamed)"), designcraft_doc::GRAPHIC_LAYER_RENDERING_WARNING));
        }
        let gxf = parse_affine(g.get("ItemTransform"));
        let (l, t, r, b) = match g.prop_el("GraphicBounds") {
            Some(gb) => {
                (gb.num("Left").unwrap_or(0.0), gb.num("Top").unwrap_or(0.0), gb.num("Right").unwrap_or(0.0), gb.num("Bottom").unwrap_or(0.0))
            }
            None => (0.0, 0.0, 0.0, 0.0),
        };
        let size = ((r - l).abs(), (b - t).abs());
        let wrap =
            if g.get("AppliedObjectStyle").is_some() { self.with_style(g, "TextWrapPreference") } else { g.find("TextWrapPreference").cloned() };
        let wrap = wrap.map(|w| text_wrap(&w)).unwrap_or_default();
        let clip = clipping_path(g, Affine::translate((-l, -t)));
        let link = g.find("Link");
        let uri = link.and_then(|k| k.get("LinkResourceURI")).map(uri_to_path);
        let graphic = |asset, size| {
            Content::Graphic(designcraft_doc::Graphic {
                layer_options: layer_options.clone(),
                asset,
                size,
                xf: gxf * Affine::translate((l, t)),
                clip: clip.clone(),
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                // A wrap set on the placed graphic (Direct Selection) is stored on the graphic element,
                // over the graphic's own object style when it names one.
                wrap,
                fill: Default::default(),
            })
        };
        let mut data = g.prop_el("Contents").map(|c| base64_decode(&c.text_content())).unwrap_or_default();
        let page = pdf_page_in(g);
        // Where the box sits on the page needs the PDF parsed: the engine finds it after import.
        let pdf_crop = pdf_attribute(g).and_then(|a| a.get("PDFCrop")).map_or(designcraft_doc::PdfCrop::Crop, names::pdf_crop_in);
        let mut read_from = None;
        if data.is_empty()
            && let Some(p) = &uri
        {
            // Every placement of a linked file with one page and crop shares one asset; read the
            // file once.
            let key = (p.clone(), page, pdf_crop);
            if let Some(id) = self.linked_assets.get(&key) {
                let size = self.assets.get(id).and_then(|a| craft_svg::sniff::viewbox_size(&a.data)).unwrap_or(size);
                return graphic(*id, size);
            }
            if let Some(d) = (self.read_link)(p) {
                data = d;
                read_from = Some(key);
            }
        }
        // A linked file that can't be read shows the document's preview of it, if it has one. The
        // preview only stands in for the file: format and pixel size still describe the link.
        let preview = data.is_empty().then(|| g.get("Self").and_then(|s| self.previews.get(s))).flatten();
        // InDesign draws a viewBox-only SVG at one unit to the point: its GraphicBounds are the
        // art's bounds, which can overhang the viewBox, so they must not size the viewBox.
        let size = craft_svg::sniff::viewbox_size(&data).unwrap_or(size);
        let (mime, px) = if preview.is_some() { (None, None) } else { sniff_image(&data) };
        let mime = mime.map(str::to_string).unwrap_or_else(|| match link.and_then(|k| k.get("LinkResourceFormat")).unwrap_or("") {
            f if f.contains("EPS") || g.local() == "EPS" => "application/postscript".into(),
            f if f.contains("JPEG") => "image/jpeg".into(),
            f if f.contains("TIFF") => "image/tiff".into(),
            f if f.contains("PDF") || g.local() == "PDF" => "application/pdf".into(),
            f if f.contains("GIF") => "image/gif".into(),
            _ => "image/png".into(),
        });
        let pixels = px.or_else(|| {
            let ppi = nums(g.get("ActualPpi").unwrap_or(""));
            (ppi.len() == 2 && size.0 > 0.0).then(|| ((size.0 * ppi[0] / 72.0).round() as u32, (size.1 * ppi[1] / 72.0).round() as u32))
        });
        let name =
            uri.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| "image".into());
        let fresh = AssetId(self.alloc());
        let link_path = uri.filter(|p| p.contains('/') || p.contains('\\'));
        let id = self.asset_index.insert_asset(
            &mut self.assets,
            Asset {
                page,
                id: fresh,
                name,
                mime,
                link: link_path,
                data: preview.cloned().unwrap_or_else(|| Arc::new(data)),
                pixels,
                pdf_crop,
                ..Default::default()
            },
        );
        if let Some(key) = read_from {
            self.linked_assets.insert(key, id);
        }
        graphic(id, size)
    }
}

/// A graphic's `ClippingPathSettings` with its saved geometry, moved into graphic space by
/// `to_graphic`. `None` for type None, an unknown type, or no geometry (the outline is never
/// traced here).
fn clipping_path(g: &El, to_graphic: Affine) -> Option<Arc<designcraft_doc::ClippingPath>> {
    let cs = g.find("ClippingPathSettings")?;
    let kind = names::clipping_kind_in(cs.get("ClippingType").unwrap_or("None"))?;
    let geometry = cs.prop_el("ClippingPathGeometry").or_else(|| cs.find("ClippingPathGeometry")).or_else(|| g.prop_el("ClippingPathGeometry"))?;
    let mut path = path_capped(geometry, designcraft_doc::MAX_CLIP_ANCHORS);
    path.subpaths.retain(|sp| sp.anchors.len() > 1 && sp.anchors.iter().all(|a| a.p.is_finite() && a.h_in.is_finite() && a.h_out.is_finite()));
    if path.is_empty() {
        return None;
    }
    path.transform(to_graphic);
    let finite = |v: Option<f64>, def: f64| v.filter(|v| v.is_finite()).unwrap_or(def);
    let mut c = designcraft_doc::ClippingPath::new(kind, path);
    c.invert = cs.boolean("InvertPath").unwrap_or(false);
    c.include_inside_edges = cs.boolean("IncludeInsideEdges").unwrap_or(false);
    c.restrict_to_frame = cs.boolean("RestrictToFrame").unwrap_or(false);
    c.use_high_resolution = cs.boolean("UseHighResolutionImage").unwrap_or(true);
    c.threshold = finite(cs.num("Threshold"), c.threshold).clamp(0.0, 255.0);
    c.tolerance = finite(cs.num("Tolerance"), c.tolerance).clamp(0.0, 10.0);
    c.inset = finite(cs.num("InsetFrame"), 0.0);
    c.source_name = cs.get("AppliedPathName").filter(|n| *n != "$ID/").unwrap_or("").to_string();
    c.source_index = cs.num("Index").filter(|v| v.is_finite()).map_or(-1, |v| v.clamp(-1.0, i32::MAX as f64) as i32);
    Some(Arc::new(c))
}

/// The path of a `…Geometry` element, keeping at most `max` anchors.
fn path_capped(pg: &El, max: usize) -> PathData {
    let geometry = craft_idml::model::PathGeometry::from_el_capped(&crate::xml::to_craft(pg), max);
    path_from_model(geometry)
}

fn path_from_model(geometry: craft_idml::model::PathGeometry) -> PathData {
    let subpaths = geometry
        .paths
        .into_iter()
        .filter_map(|path| {
            let anchors: Vec<_> = path
                .points
                .into_iter()
                .map(|point| {
                    Anchor::with_handles(
                        Point::new(point.anchor[0], point.anchor[1]),
                        Point::new(point.left[0], point.left[1]),
                        Point::new(point.right[0], point.right[1]),
                    )
                })
                .collect();
            (!anchors.is_empty()).then(|| SubPath::new(anchors, !path.open))
        })
        .collect();
    PathData::new(subpaths)
}

/// A placed PDF's `PDFAttribute` (Image Import Options), as a child or a property.
fn pdf_attribute(g: &El) -> Option<&El> {
    g.find("PDFAttribute").or_else(|| g.prop_el("PDFAttribute"))
}

/// The page a placed PDF shows (0-based), from `PDFAttribute/@PageNumber` (1-based). A missing or
/// malformed value means page 1; a page the PDF lacks draws nothing and is left out of PDF export.
fn pdf_page_in(g: &El) -> u32 {
    pdf_attribute(g)
        .and_then(|a| a.get("PageNumber"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .and_then(|n| u32::try_from(n.saturating_sub(1)).ok())
        .unwrap_or(0)
}

/// Anchors a custom text wrap boundary may have.
const MAX_WRAP_PATH_ANCHORS: usize = 4096;

/// A custom text wrap boundary (`TextWrapPreference/Properties/PathGeometry`), in the item's inner
/// space like its own path. Hostile input is an error: too many anchors, a malformed or non-finite
/// point, or no points at all.
fn wrap_path_of(pg: &El) -> std::result::Result<PathData, &'static str> {
    let geometry =
        craft_idml::model::PathGeometry::try_from_el_capped(&crate::xml::to_craft(pg), MAX_WRAP_PATH_ANCHORS).map_err(|error| match error {
            craft_idml::model::PathGeometryError::TooManyPoints => "too many points",
            craft_idml::model::PathGeometryError::MissingAnchor => "point without an anchor",
            craft_idml::model::PathGeometryError::MalformedPoint => "malformed or non-finite point",
            craft_idml::model::PathGeometryError::NonFinitePoint => "malformed or non-finite point",
        })?;
    let path = path_from_model(geometry);
    if path.subpaths.is_empty() {
        return Err("no points");
    }
    Ok(path)
}
