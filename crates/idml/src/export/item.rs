use super::common::{affine_s, bool_s, pct, uid, with_props};
use super::{Affine, ColumnsKind, Content, El, Ex, Item, Node, OBJECT_BUILTINS, PathData, Shape, TextFrameOptions, base64_encode, names, num, pt};

impl<'a> Ex<'a> {
    pub(super) fn item_el(&mut self, it: &Item, xf: Affine) -> El {
        let d = self.d;
        let path_text = matches!(&it.content, Content::Text(t) if t.options.path.is_some());
        let tag = match (&it.content, it.shape) {
            (Content::Text(_), _) if path_text => match it.shape {
                Shape::GraphicLine => "GraphicLine",
                Shape::Oval => "Oval",
                Shape::Rectangle => "Rectangle",
                _ => "Polygon",
            },
            (Content::Text(_), _) => "TextFrame",
            (Content::Group { .. }, Shape::Group) => "Group",
            (_, Shape::Oval) => "Oval",
            (_, Shape::Polygon) | (_, Shape::Path) => "Polygon",
            (_, Shape::GraphicLine) => "GraphicLine",
            _ => "Rectangle",
        };
        let mut el = El::new(tag).attr("Self", uid(it.id.0));
        if let Some(o) = it.overrides {
            el.set("OverriddenPageItem", uid(o.0));
        }
        if let Content::Text(t) = &it.content
            && let Some(pt) = &t.options.path
        {
            // Type on a path: a TextPath child holds the story.
            let len = designcraft_geom::warp::PathWarp::new(&it.path.to_bezpath(), false).length();
            el.push(crate::xml::from_craft(craft_idml::model::TextPath::to_el(
                &uid(it.id.0),
                &uid(t.story.0),
                names::path_align_out(pt.align),
                pt.flip,
                pt.start,
                pt.end.map_or(len, |end| end.min(len)),
            )));
        } else if let Content::Text(t) = &it.content {
            let (prev, next) = self.threads.get(&it.id.0).copied().unwrap_or((None, None));
            el.set("ParentStory", uid(t.story.0));
            el.set("PreviousTextFrame", prev.map(uid).unwrap_or_else(|| "n".into()));
            el.set("NextTextFrame", next.map(uid).unwrap_or_else(|| "n".into()));
        }
        if tag != "Group" {
            el.set(
                "ContentType",
                match it.content {
                    Content::Text(_) => "TextType",
                    Content::Graphic(_) => "GraphicType",
                    _ => "Unassigned",
                },
            );
        }
        el.set("Name", if it.name.is_empty() { "$ID/".to_string() } else { it.name.clone() });
        el.set("ItemLayer", uid(it.layer.0));
        el.set("Visible", bool_s(!it.hidden));
        el.set("Locked", bool_s(it.locked));
        el.set("Nonprinting", bool_s(it.nonprinting));
        let os = if it.object_style.is_empty() { designcraft_doc::NO_OBJECT_STYLE } else { &it.object_style };
        el.set("AppliedObjectStyle", names::style_self("ObjectStyle", OBJECT_BUILTINS, os));
        el.set("ItemTransform", affine_s(xf));
        el.set("FillColor", self.sw(&it.fill.swatch));
        el.set("FillTint", pct(it.fill.tint as f64));
        el.set("OverprintFill", bool_s(it.fill.overprint));
        if let Some([x0, y0, x1, y1]) = it.fill.gradient_vector {
            // Gradient Swatch tool vector: start, length and angle in the item's space.
            el.set("GradientFillStart", format!("{} {}", num(x0), num(y0)));
            el.set("GradientFillLength", num((x1 - x0).hypot(y1 - y0)));
            el.set("GradientFillAngle", num((-(y1 - y0)).atan2(x1 - x0).to_degrees()));
        } else if let Some(a) = it.fill.gradient_angle {
            el.set("GradientFillAngle", num(a));
        }
        // The same on the stroke.
        if let Some([x0, y0, x1, y1]) = it.stroke.gradient_vector {
            el.set("GradientStrokeStart", format!("{} {}", num(x0), num(y0)));
            el.set("GradientStrokeLength", num((x1 - x0).hypot(y1 - y0)));
            el.set("GradientStrokeAngle", num((-(y1 - y0)).atan2(x1 - x0).to_degrees()));
        } else if let Some(a) = it.stroke.gradient_angle {
            el.set("GradientStrokeAngle", num(a));
        }
        self.stroke_attrs(&mut el, &it.stroke);
        // Corner options: map path-order corners to the named corners by geometry. An item without
        // corners whose object style chain sets some writes None, so an import does not give it
        // the style's corners.
        if !it.corners.is_none() || self.style_sets_corners(os) {
            let names_by_index = corner_names(&it.path);
            for (i, c) in it.corners.corners.iter().enumerate() {
                let n = names_by_index[i];
                el.set(&format!("{n}CornerOption"), names::corner_out(c.shape));
                el.set(&format!("{n}CornerRadius"), num(c.size));
            }
        }
        let mut props = Vec::new();
        if tag != "Group" && !it.path.is_empty() {
            props.push(path_geometry(&it.path));
        }
        let mut el = with_props(el, props);
        self.append_transparency(&mut el, it, os);
        if let Content::Text(t) = &it.content {
            el.push(text_frame_pref(&t.options, &self.sw(&t.options.column_rule_color)));
            if let Some(g) = t.options.baseline_grid {
                el.push(
                    El::new("BaselineFrameGridOption")
                        .attr("UseCustomBaselineFrameGrid", "true")
                        .attr("StartingOffsetForBaselineFrameGrid", num(g.start))
                        .attr(
                            "BaselineFrameGridRelativeOption",
                            match g.relative_to {
                                designcraft_doc::FrameGridRelative::TopOfPage => "TopOfPageOfBaselineFrameGridRelativeOption",
                                designcraft_doc::FrameGridRelative::TopMargin => "TopOfMarginOfBaselineFrameGridRelativeOption",
                                designcraft_doc::FrameGridRelative::TopOfFrame => "TopOfFrameOfBaselineFrameGridRelativeOption",
                                designcraft_doc::FrameGridRelative::TopInset => "TopOfInsetOfBaselineFrameGridRelativeOption",
                            },
                        )
                        .attr("BaselineFrameGridIncrement", num(g.increment)),
                );
            }
        }
        el.push(text_wrap_pref(&it.wrap));
        match &it.content {
            Content::Group { items } => {
                for c in items {
                    let e = self.item_el(c, c.xf);
                    el.push(e);
                }
            }
            Content::Graphic(g) => {
                if g.auto_fit != designcraft_doc::Fitting::None || g.fit_align != 4 || g.crop != [0.0; 4] {
                    el.push(
                        El::new("FrameFittingOption")
                            .attr("AutoFit", bool_s(g.auto_fit != designcraft_doc::Fitting::None))
                            .attr("TopCrop", num(g.crop[0]))
                            .attr("LeftCrop", num(g.crop[1]))
                            .attr("BottomCrop", num(g.crop[2]))
                            .attr("RightCrop", num(g.crop[3]))
                            .attr("FittingOnEmptyFrame", names::fitting_out(g.auto_fit))
                            .attr("FittingAlignment", names::anchor_out(g.fit_align)),
                    );
                }
                if let Some(asset) = d.assets.get(&g.asset) {
                    let e = self.image_el(asset, g);
                    el.push(e);
                }
            }
            _ => {}
        }
        el
    }

    /// Whether object style `name`, a style it is based on, or `[None]` (which an import reads
    /// after every item's style chain) sets corners other than None. A built-in style the
    /// document lacks counts as its default.
    fn style_sets_corners(&mut self, name: &str) -> bool {
        if let Some(&known) = self.style_corners.get(name) {
            return known;
        }
        let defaults = designcraft_doc::Styles::default();
        let style = |n: &str| self.d.styles.object_style(n).or_else(|| defaults.object_style(n));
        let rounds = |os: &designcraft_doc::ObjectStyle| os.corners.is_some_and(|c| !c.is_none());
        let mut seen: Vec<&str> = Vec::new();
        let mut cur = Some(name);
        let mut found = false;
        while let Some(n) = cur.take() {
            if seen.contains(&n) {
                break;
            }
            let Some(os) = style(n) else { break };
            if rounds(os) {
                found = true;
                break;
            }
            seen.push(n);
            cur = os.based_on.as_deref();
        }
        let found = found || style(designcraft_doc::NO_OBJECT_STYLE).is_some_and(rounds);
        self.style_corners.insert(name.to_string(), found);
        found
    }

    fn append_transparency(&self, el: &mut El, it: &Item, os: &str) {
        // Transparency.
        // An item whose style sets effects writes the effects it has turned off too, so that on
        // import it doesn't take the style's.
        let styled = self.d.styles.object_effects(os).is_some();
        let ds = &it.effects.drop_shadow;
        let (gf, df) = (&it.effects.gradient_feather, &it.effects.directional_feather);
        if it.opacity < 1.0 || it.blend != designcraft_color::BlendMode::Normal || ds.on || it.effects.feather > 0.0 || gf.on || df.on || styled {
            let gradient_feather = if gf.on {
                let (angle, length, start) = gf.vector.map_or((Some(gf.angle), None, None), |[x0, y0, x1, y1]| {
                    (Some((-(y1 - y0)).atan2(x1 - x0).to_degrees()), Some((x1 - x0).hypot(y1 - y0)), Some([x0, y0]))
                });
                Some(craft_idml::model::GradientFeatherValues {
                    applied: true,
                    radial: gf.radial,
                    angle,
                    length,
                    start,
                    stops: vec![
                        craft_idml::model::OpacityStopValues { location: Some(0.0), opacity: Some(f64::from(gf.start) * 100.0) },
                        craft_idml::model::OpacityStopValues { location: Some(100.0), opacity: Some(f64::from(gf.end) * 100.0) },
                    ],
                })
            } else {
                None
            };
            let directional_feather = df.on.then(|| craft_idml::model::DirectionalFeatherValues { applied: true, widths: df.widths.map(Some) });
            let transparency = craft_idml::model::TransparencySetting {
                blend_mode: Some(names::blend_out(it.blend).into()),
                opacity: Some(f64::from(it.opacity) * 100.0),
                drop_shadow: (ds.on || styled).then(|| {
                    let a = self.d.light_angle(ds.angle, ds.global_light).to_radians();
                    craft_idml::model::DropShadowValues {
                        mode: Some(if ds.on { "Drop" } else { "None" }.into()),
                        x_offset: Some(-ds.distance * a.cos()),
                        y_offset: Some(ds.distance * a.sin()),
                        color: Some(self.sw(&ds.color)),
                        opacity: Some(f64::from(ds.opacity) * 100.0),
                        size: Some(ds.size),
                        spread: Some(ds.spread),
                        blend_mode: Some(names::blend_out(ds.mode).into()),
                    }
                }),
                feather: (it.effects.feather > 0.0 || styled).then(|| {
                    let corners = match it.effects.feather_options.corners {
                        designcraft_doc::FeatherCorners::Sharp => "Sharp",
                        designcraft_doc::FeatherCorners::Rounded => "Rounded",
                        designcraft_doc::FeatherCorners::Diffused => "Diffusion",
                    };
                    craft_idml::model::FeatherValues {
                        mode: Some(if it.effects.feather > 0.0 { "Standard" } else { "None" }.into()),
                        width: Some(it.effects.feather),
                        choke_amount: Some(it.effects.feather_options.choke),
                        corner_type: Some(corners.into()),
                    }
                }),
                gradient_feather,
                directional_feather,
            };
            el.push(crate::xml::from_craft(transparency.to_el(&uid(it.id.0))));
        }
    }

    /// The `TransparencySetting` children for `e`'s drop shadow and feather; `all` also writes the
    /// effects that are off (`Mode="None"`), as an object style that sets effects does.
    pub(super) fn effect_els(&self, e: &designcraft_doc::Effects, all: bool) -> Vec<El> {
        let mut out = Vec::new();
        let ds = &e.drop_shadow;
        if ds.on || all {
            let a = self.d.light_angle(ds.angle, ds.global_light).to_radians();
            out.push(crate::xml::from_craft(
                craft_idml::model::DropShadowValues {
                    mode: Some(if ds.on { "Drop" } else { "None" }.into()),
                    x_offset: Some(-ds.distance * a.cos()),
                    y_offset: Some(ds.distance * a.sin()),
                    color: Some(self.sw(&ds.color)),
                    opacity: Some(f64::from(ds.opacity) * 100.0),
                    size: Some(ds.size),
                    spread: Some(ds.spread),
                    blend_mode: Some(names::blend_out(ds.mode).into()),
                }
                .to_el(),
            ));
        }
        if e.feather > 0.0 || all {
            let corners = match e.feather_options.corners {
                designcraft_doc::FeatherCorners::Sharp => "Sharp",
                designcraft_doc::FeatherCorners::Rounded => "Rounded",
                designcraft_doc::FeatherCorners::Diffused => "Diffusion",
            };
            out.push(crate::xml::from_craft(
                craft_idml::model::FeatherValues {
                    mode: Some(if e.feather > 0.0 { "Standard" } else { "None" }.into()),
                    width: Some(e.feather),
                    choke_amount: Some(e.feather_options.choke),
                    corner_type: Some(corners.into()),
                }
                .to_el(),
            ));
        }
        out
    }

    fn image_el(&mut self, asset: &designcraft_doc::Asset, g: &designcraft_doc::Graphic) -> El {
        let is_pdf = asset.mime == "application/pdf";
        // Bytes that can't be had are written like a link without data.
        let data = asset.data.load().unwrap_or_default();
        let is_eps = asset.mime == "application/postscript" || designcraft_images::is_eps(&data);
        let mut el = El::new(if is_eps {
            "EPS"
        } else if is_pdf {
            "PDF"
        } else {
            "Image"
        })
        .attr("Self", self.fresh());
        // A PDF is vector: its page size in `pixels` isn't a resolution, and a cropped graphic
        // spans less than the page. An EPS's `pixels` is its preview raster, so it still counts.
        if let Some((pw, ph)) = asset.pixels.filter(|_| !is_pdf)
            && g.size.0 > 0.0
            && g.size.1 > 0.0
        {
            el.set("ActualPpi", pt(pw as f64 * 72.0 / g.size.0, ph as f64 * 72.0 / g.size.1));
        }
        el.set("ItemTransform", affine_s(g.xf));
        // The swatch that colours a greyscale or bitmap image; written only when it exists.
        if !g.fill.is_none()
            && let Some(id) = self.swatch_ids.get(&g.fill.swatch)
        {
            el.set("FillColor", id.clone());
            el.set("FillTint", pct(g.fill.tint as f64));
        }
        let wanted = !data.is_empty() && (self.opts.embed_images || asset.link.is_none());
        // An image with no link of its own has nowhere else to live, so it is always embedded
        // (dropping its data would lose it); only an image with a real link can fall back to it.
        let embed = wanted && (asset.link.is_none() || self.embedded.saturating_add(data.len()) <= self.embed_limit);
        if wanted && !embed {
            self.warnings.push(format!(
                "image {} is written as a link, not embedded: the package already holds {} bytes of images (limit {})",
                asset.name, self.embedded, self.embed_limit
            ));
        }
        let mut props = Vec::new();
        if embed {
            self.embedded = self.embedded.saturating_add(data.len());
            let mut c = El::new("Contents");
            c.children.push(Node::CData(base64_encode(&data)));
            props.push(c);
        }
        props.push(El::new("GraphicBounds").attr("Left", "0").attr("Top", "0").attr("Right", num(g.size.0)).attr("Bottom", num(g.size.1)));
        let mut el = with_props(el, props);
        if !g.wrap.is_default() {
            el.push(text_wrap_pref(&g.wrap));
        }
        if let Some(c) = &g.clip {
            el.push(clipping_settings(c));
        }
        if let Some(options) = &g.layer_options {
            el.push(self.layer_option_el(options));
        }
        let uri = match &asset.link {
            Some(l) => path_to_uri(l),
            None => format!("file:{}", asset.name),
        };
        el.push(
            El::new("Link")
                .attr("Self", self.fresh())
                .attr("AssetURL", "$ID/")
                .attr("AssetID", "$ID/")
                .attr("LinkResourceURI", uri)
                .attr("LinkResourceFormat", format!("$ID/{}", format_name(if is_eps { "application/postscript" } else { &asset.mime })))
                .attr("StoredState", if embed { "Embedded" } else { "Normal" })
                .attr("LinkClassID", "35906")
                .attr("LinkClientID", "257")
                .attr("LinkResourceModified", "false")
                .attr("LinkObjectModified", "false")
                .attr("ShowInUI", "true")
                .attr("CanEmbed", "true")
                .attr("CanUnembed", "true")
                .attr("CanPackage", "true")
                .attr("ImportPolicy", "NoAutoImport")
                .attr("ExportPolicy", "NoAutoExport"),
        );
        if is_pdf {
            // `GraphicBounds` span the chosen box, as graphic space does. Without `PDFAttribute`
            // the page would be cropped to its visible content, which changes the graphic's size
            // and so its scale and position.
            el.push(
                El::new("PDFAttribute")
                    .attr("PageNumber", asset.page.saturating_add(1))
                    .attr("PDFCrop", names::pdf_crop_out(asset.pdf_crop))
                    .attr("TransparentBackground", "true"),
            );
        }
        el
    }
}

/// `rule_color` is the IDML reference of the column rule's swatch.
/// `TextWrapPreference` of a page item or a placed graphic.
fn text_wrap_pref(w: &designcraft_doc::TextWrap) -> El {
    let paths: Vec<_> = w.path.iter().filter(|p| !p.is_empty()).map(path_geometry_value).collect();
    let mut el = crate::xml::from_craft(craft_idml::model::TextWrapPreference::to_el(
        names::wrap_mode_out(w.mode),
        w.invert,
        names::wrap_side_out(w.side),
        names::contour_type_out(w.contour),
        w.offsets,
        &paths,
    ));
    if let Some(c) = el.find_mut("ContourOption") {
        c.set("IncludeInsideEdges", bool_s(w.inside_edges));
    }
    el
}

pub(super) fn text_frame_pref(o: &TextFrameOptions, rule_color: &str) -> El {
    crate::xml::from_craft(
        craft_idml::model::TextFramePreferences {
            column_count: Some(f64::from(o.columns.max(1))),
            column_gutter: Some(o.gutter),
            fixed_column_width: (o.column_width > 0.0).then_some(o.column_width),
            fixed_width: o.columns_kind == ColumnsKind::FixedWidth,
            flexible_width: o.columns_kind == ColumnsKind::FlexibleWidth,
            balance_columns: o.balance_columns,
            inset: Some(o.inset),
            vertical_justification: Some(names::vj_out(o.vertical_justification).into()),
            vertical_threshold: Some(o.vj_paragraph_spacing_limit),
            first_baseline: Some(names::first_baseline_out(o.first_baseline).into()),
            minimum_first_baseline: Some(o.first_baseline_min),
            ignore_wrap: o.ignore_wrap,
            auto_sizing_type: Some(names::auto_size_out(o.auto_size).into()),
            auto_sizing_reference: Some(names::REF_POINTS[(o.auto_size_ref as usize).min(8)].into()),
            use_minimum_height: o.auto_size_use_min_height,
            minimum_height: Some(o.auto_size_min_height),
            use_minimum_width: o.auto_size_use_min_width,
            minimum_width: Some(o.auto_size_min_width),
            no_line_breaks: o.auto_size_no_line_breaks,
            column_rule_override: o.column_rule,
            column_rule_width: Some(o.column_rule_weight),
            column_rule_tint: Some(f64::from(o.column_rule_tint) * 100.0),
            column_rule_offset: Some(o.column_rule_offset),
            column_rule_top_inset: Some(o.column_rule_top_inset),
            column_rule_bottom_inset: Some(o.column_rule_bottom_inset),
        }
        .to_el(rule_color),
    )
}

/// `<ClippingPathSettings>` with the outline (graphic space is the image's inner space, whose
/// `GraphicBounds` start at 0,0).
fn clipping_settings(c: &designcraft_doc::ClippingPath) -> El {
    let mut geometry = path_geometry(&c.path);
    geometry.name = "ClippingPathGeometry".into();
    let el = El::new("ClippingPathSettings")
        .attr("ClippingType", names::clipping_kind_out(c.kind))
        .attr("InvertPath", bool_s(c.invert))
        .attr("IncludeInsideEdges", bool_s(c.include_inside_edges))
        .attr("RestrictToFrame", bool_s(c.restrict_to_frame))
        .attr("UseHighResolutionImage", bool_s(c.use_high_resolution))
        .attr("Threshold", num(c.threshold))
        .attr("Tolerance", num(c.tolerance))
        .attr("InsetFrame", num(c.inset))
        .attr("AppliedPathName", if c.source_name.is_empty() { "$ID/" } else { c.source_name.as_str() })
        .attr("Index", c.source_index.to_string());
    with_props(el, vec![geometry])
}

fn path_geometry(path: &PathData) -> El {
    crate::xml::from_craft(path_geometry_value(path).to_el())
}

fn path_geometry_value(path: &PathData) -> craft_idml::model::PathGeometry {
    craft_idml::model::PathGeometry {
        paths: path
            .subpaths
            .iter()
            .map(|sp| craft_idml::model::SubPath {
                open: !sp.closed,
                points: sp
                    .anchors
                    .iter()
                    .map(|a| craft_idml::model::PathPoint { anchor: [a.p.x, a.p.y], left: [a.h_in.x, a.h_in.y], right: [a.h_out.x, a.h_out.y] })
                    .collect(),
            })
            .collect(),
    }
}

/// IDML corner names for anchors 0..4 of a path (by position on its bounding box).
pub(crate) fn corner_names(path: &PathData) -> [&'static str; 4] {
    designcraft_geom::corners::named_slots(path).map(|k| ["TopLeft", "TopRight", "BottomRight", "BottomLeft"][k])
}

fn format_name(mime: &str) -> &'static str {
    match mime {
        "image/png" => "Portable Network Graphics (PNG)",
        "image/jpeg" => "JPEG",
        "image/gif" => "GIF",
        "image/tiff" => "TIFF",
        "application/pdf" => "Adobe Portable Document Format (PDF)",
        "application/postscript" => "Encapsulated PostScript (EPS)",
        _ => "Portable Network Graphics (PNG)",
    }
}

/// File path → IDML link URI (`file:/abs/path` with spaces percent-encoded).
pub(crate) fn path_to_uri(path: &str) -> String {
    if path.contains(':') && !path.starts_with('/') && path.chars().nth(1) != Some(':') {
        return path.to_string(); // already a URI
    }
    let mut out = String::from("file:");
    let p = path.replace('\\', "/");
    for c in p.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod group_depth_tests {
    use craft_idml::model::{StyleGroupWriter, StyleKind};

    #[test]
    fn a_style_name_with_many_separators_makes_bounded_folders() {
        let mut group = StyleGroupWriter::new(StyleKind::Character, "root");
        let style = craft_idml::El::new("CharacterStyle");
        group.insert(&"a/".repeat(10_000), style);
        let root = group.to_el();
        let mut depth = 0;
        let mut current = root.elements().next();
        while let Some(el) = current.filter(|el| el.local() == "CharacterStyleGroup") {
            depth += 1;
            current = el.elements().next();
        }
        assert_eq!(depth, 32);
    }
}
