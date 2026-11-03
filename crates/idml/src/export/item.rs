use super::common::{affine_s, bool_s, p, pct, uid, with_props};
use super::{
    Affine, ColumnsKind, Content, El, Ex, HashSet, Item, Node, OBJECT_BUILTINS, PathData, Shape, TextFrameOptions, base64_encode, escape_id, names,
    num, pt,
};

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
            el.push(
                El::new("TextPath")
                    .attr("Self", format!("{}tp", uid(it.id.0)))
                    .attr("ParentStory", uid(t.story.0))
                    .attr("PreviousTextFrame", "n")
                    .attr("NextTextFrame", "n")
                    .attr("PathEffect", "RainbowPathEffect")
                    .attr("PathAlignment", "CenterPathAlignment")
                    .attr("TextAlignment", names::path_align_out(pt.align))
                    .attr("FlipPathEffect", if pt.flip { "Flipped" } else { "NotFlipped" })
                    .attr("StartBracket", num(pt.start))
                    .attr("EndBracket", num(pt.end.map_or(len, |e| e.min(len)))),
            );
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
        // Corner options: map path-order corners to the named corners by geometry.
        if !it.corners.is_none() {
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

    fn append_transparency(&self, el: &mut El, it: &Item, os: &str) {
        // Transparency.
        // An item whose style sets effects writes the effects it has turned off too, so that on
        // import it doesn't take the style's.
        let styled = self.d.styles.object_effects(os).is_some();
        let ds = &it.effects.drop_shadow;
        let (gf, df) = (&it.effects.gradient_feather, &it.effects.directional_feather);
        if it.opacity < 1.0 || it.blend != designcraft_color::BlendMode::Normal || ds.on || it.effects.feather > 0.0 || gf.on || df.on || styled {
            let mut t = El::new("TransparencySetting");
            t.push(El::new("BlendingSetting").attr("BlendMode", names::blend_out(it.blend)).attr("Opacity", pct(it.opacity as f64)));
            for c in self.effect_els(&it.effects, styled) {
                t.push(c);
            }
            if df.on {
                let [top, left, bottom, right] = df.widths;
                t.push(
                    El::new("DirectionalFeatherSetting")
                        .attr("Applied", "true")
                        .attr("LeftWidth", num(left))
                        .attr("RightWidth", num(right))
                        .attr("TopWidth", num(top))
                        .attr("BottomWidth", num(bottom)),
                );
            }
            if gf.on {
                let mut g = El::new("GradientFeatherSetting").attr("Applied", "true").attr("Type", if gf.radial { "Radial" } else { "Linear" });
                if let Some([x0, y0, x1, y1]) = gf.vector {
                    // Gradient Feather tool vector: start, length and angle in the item's space.
                    g.set("Angle", num((-(y1 - y0)).atan2(x1 - x0).to_degrees()));
                    g.set("Length", num((x1 - x0).hypot(y1 - y0)));
                    g.set("GradientStart", format!("{} {}", num(x0), num(y0)));
                } else {
                    g.set("Angle", num(gf.angle));
                }
                let id = uid(it.id.0);
                for (i, (opacity, location)) in [(gf.start, 0.0), (gf.end, 100.0)].into_iter().enumerate() {
                    let mut s = El::new("OpacityGradientStop")
                        .attr("Self", format!("{id}TransparencySetting1GradientFeatherSetting1OpacityGradientStop{i}"))
                        .attr("Opacity", pct(opacity as f64))
                        .attr("Location", num(location));
                    if i > 0 {
                        s.set("Midpoint", "50");
                    }
                    g.push(s);
                }
                t.push(g);
            }
            el.push(t);
        }
    }

    /// The `TransparencySetting` children for `e`'s drop shadow and feather; `all` also writes the
    /// effects that are off (`Mode="None"`), as an object style that sets effects does.
    pub(super) fn effect_els(&self, e: &designcraft_doc::Effects, all: bool) -> Vec<El> {
        let mut out = Vec::new();
        let ds = &e.drop_shadow;
        if ds.on || all {
            let a = self.d.light_angle(ds.angle, ds.global_light).to_radians();
            out.push(
                El::new("DropShadowSetting")
                    .attr("Mode", if ds.on { "Drop" } else { "None" })
                    .attr("XOffset", num(-ds.distance * a.cos()))
                    .attr("YOffset", num(ds.distance * a.sin()))
                    .attr("Size", num(ds.size))
                    .attr("Spread", num(ds.spread))
                    .attr("Opacity", pct(ds.opacity as f64))
                    .attr("BlendMode", names::blend_out(ds.mode))
                    .attr("EffectColor", self.sw(&ds.color)),
            );
        }
        if e.feather > 0.0 || all {
            let corners = match e.feather_options.corners {
                designcraft_doc::FeatherCorners::Sharp => "Sharp",
                designcraft_doc::FeatherCorners::Rounded => "Rounded",
                designcraft_doc::FeatherCorners::Diffused => "Diffusion",
            };
            out.push(
                El::new("FeatherSetting")
                    .attr("Mode", if e.feather > 0.0 { "Standard" } else { "None" })
                    .attr("Width", num(e.feather))
                    .attr("ChokeAmount", num(e.feather_options.choke))
                    .attr("CornerType", corners),
            );
        }
        out
    }

    fn image_el(&mut self, asset: &designcraft_doc::Asset, g: &designcraft_doc::Graphic) -> El {
        let is_pdf = asset.mime == "application/pdf";
        let is_eps = asset.mime == "application/postscript" || designcraft_images::is_eps(&asset.data);
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
        let embed = !asset.data.is_empty() && (self.opts.embed_images || asset.link.is_none());
        let mut props = Vec::new();
        if embed {
            let mut c = El::new("Contents");
            c.children.push(Node::CData(base64_encode(&asset.data)));
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

/// Style folder tree (names `A/B/C` → groups A, B).
#[derive(Default)]
pub(super) struct Group {
    styles: Vec<El>,
    groups: Vec<(String, Group)>,
}

impl Group {
    pub(super) fn insert(&mut self, name: &str, el: El) {
        let builtin = name.starts_with('[');
        let mut segs: Vec<&str> = if builtin { vec![name] } else { name.split('/').collect() };
        let _leaf = segs.pop();
        let mut g = self;
        for s in segs {
            let i = match g.groups.iter().position(|(n, _)| n == s) {
                Some(i) => i,
                None => {
                    g.groups.push((s.to_string(), Group::default()));
                    g.groups.len() - 1
                }
            };
            g = &mut g.groups[i].1;
        }
        g.styles.push(el);
    }

    pub(super) fn emit(self, parent: &mut El, kind: &str, path: &str) {
        for s in self.styles {
            parent.push(s);
        }
        for (name, g) in self.groups {
            let full = if path.is_empty() { name.clone() } else { format!("{path}:{name}") };
            let mut ge = El::new(kind).attr("Self", format!("{kind}/{}", escape_id(&full))).attr("Name", &name);
            g.emit(&mut ge, kind, &full);
            parent.push(ge);
        }
    }
}

/// `rule_color` is the IDML reference of the column rule's swatch.
/// `TextWrapPreference` of a page item or a placed graphic.
fn text_wrap_pref(w: &designcraft_doc::TextWrap) -> El {
    with_props(
        El::new("TextWrapPreference")
            .attr("Inverse", bool_s(w.invert))
            .attr("ApplyToMasterPageOnly", "false")
            .attr("TextWrapSide", names::wrap_side_out(w.side))
            .attr("TextWrapMode", names::wrap_mode_out(w.mode)),
        w.path
            .iter()
            .filter(|p| !p.is_empty())
            .map(path_geometry)
            .chain([El::new("TextWrapOffset")
                .attr("Top", num(w.offsets[0]))
                .attr("Left", num(w.offsets[1]))
                .attr("Bottom", num(w.offsets[2]))
                .attr("Right", num(w.offsets[3]))])
            .collect(),
    )
    .child(El::new("ContourOption").attr("ContourType", names::contour_type_out(w.contour)).attr("IncludeInsideEdges", "false"))
}

pub(super) fn text_frame_pref(o: &TextFrameOptions, rule_color: &str) -> El {
    let mut inset = El::new("InsetSpacing").attr("type", "list");
    for v in o.inset {
        inset.push(p("ListItem", "unit", num(v)));
    }
    let el = El::new("TextFramePreference")
        .attr("TextColumnCount", o.columns.max(1))
        .attr("TextColumnGutter", num(o.gutter))
        .attr("UseFixedColumnWidth", bool_s(o.columns_kind == ColumnsKind::FixedWidth))
        .attr("UseFlexibleColumnWidth", bool_s(o.columns_kind == ColumnsKind::FlexibleWidth))
        .attr("VerticalBalanceColumns", bool_s(o.balance_columns))
        .attr("FirstBaselineOffset", names::first_baseline_out(o.first_baseline))
        .attr("MinimumFirstBaselineOffset", num(o.first_baseline_min))
        .attr("VerticalJustification", names::vj_out(o.vertical_justification))
        .attr("VerticalThreshold", num(o.vj_paragraph_spacing_limit))
        .attr("IgnoreWrap", bool_s(o.ignore_wrap))
        .attr("AutoSizingType", names::auto_size_out(o.auto_size))
        .attr("AutoSizingReferencePoint", names::REF_POINTS[(o.auto_size_ref as usize).min(8)])
        .attr("UseMinimumHeightForAutoSizing", bool_s(o.auto_size_use_min_height))
        .attr("MinimumHeightForAutoSizing", num(o.auto_size_min_height))
        .attr("UseMinimumWidthForAutoSizing", bool_s(o.auto_size_use_min_width))
        .attr("MinimumWidthForAutoSizing", num(o.auto_size_min_width))
        .attr("UseNoLineBreaksForAutoSizing", bool_s(o.auto_size_no_line_breaks));
    let el = if o.column_width > 0.0 { el.attr("TextColumnFixedWidth", num(o.column_width)) } else { el };
    // Written on every frame: on import a frame takes any value it lacks from its object style.
    let el = el
        .attr("ColumnRuleOverride", bool_s(o.column_rule))
        .attr("ColumnRuleStrokeWidth", num(o.column_rule_weight))
        .attr("ColumnRuleStrokeColor", rule_color)
        .attr("ColumnRuleStrokeTint", num(f64::from(o.column_rule_tint) * 100.0))
        .attr("ColumnRuleOffset", num(o.column_rule_offset))
        .attr("ColumnRuleTopInset", num(o.column_rule_top_inset))
        .attr("ColumnRuleBottomInset", num(o.column_rule_bottom_inset))
        .attr("ColumnRuleInsetChainOverride", bool_s(o.column_rule_top_inset == o.column_rule_bottom_inset));
    with_props(el, vec![inset])
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
    let mut pg = El::new("PathGeometry");
    for sp in &path.subpaths {
        let mut arr = El::new("PathPointArray");
        for a in &sp.anchors {
            arr.push(
                El::new("PathPointType")
                    .attr("Anchor", pt(a.p.x, a.p.y))
                    .attr("LeftDirection", pt(a.h_in.x, a.h_in.y))
                    .attr("RightDirection", pt(a.h_out.x, a.h_out.y)),
            );
        }
        pg.push(El::new("GeometryPathType").attr("PathOpen", bool_s(!sp.closed)).child(arr));
    }
    pg
}

/// IDML corner names for anchors 0..4 of a path (by position on its bounding box).
pub(crate) fn corner_names(path: &PathData) -> [&'static str; 4] {
    let default = ["TopLeft", "TopRight", "BottomRight", "BottomLeft"];
    let Some(sp) = path.subpaths.first() else { return default };
    if sp.anchors.len() != 4 {
        return default;
    }
    let Some(b) = path.bounds() else { return default };
    let c = b.center();
    let mut out = default;
    let mut seen = HashSet::new();
    for (i, a) in sp.anchors.iter().enumerate() {
        let n = match (a.p.y < c.y, a.p.x < c.x) {
            (true, true) => "TopLeft",
            (true, false) => "TopRight",
            (false, false) => "BottomRight",
            (false, true) => "BottomLeft",
        };
        if !seen.insert(n) {
            return default;
        }
        out[i] = n;
    }
    out
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
