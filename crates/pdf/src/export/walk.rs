use super::common::{TagEntry, blend, norm, rects_overlap, solid_fill, tf, to_path};
use super::graphics::draw_coverage;
use super::pipeline::{item_exported, layer_exported};
use super::*;

impl Exporter<'_> {
    /// Story `sid` composed for `page` ([`Cache::get`]), asked of the cache once per export.
    fn composed(&mut self, sid: designcraft_doc::StoryId, page: Option<usize>) -> Arc<designcraft_compose::ComposedStory> {
        let (doc, cache) = (self.doc, self.cache);
        self.composed.entry((sid, page)).or_insert_with(|| cache.get(doc, sid, page)).clone()
    }

    /// The parent pages' items on `layer` shown on page `pi` of spread `si`: the parent's bases
    /// first, then the parent.
    fn parent_items(&mut self, s: &mut Surface, si: usize, pi: usize, page: &designcraft_doc::Page, layer: &designcraft_doc::Layer, first: usize) {
        let doc = self.doc;
        for (ppi, ppage) in doc.shown_parent_pages(SpreadRef::Doc(si), pi) {
            let Some(parent) = doc.parents.get(ppi) else { continue };
            let Some(ppg) = parent.pages.get(ppage) else { continue };
            let dx = page.x - ppg.x;
            for it in &parent.items {
                if it.layer != layer.id || page.overridden.contains(&it.id) {
                    continue;
                }
                // Parent items belong to the parent page they sit on.
                if parent.page_at_x(it.bounds().center().x) != Some(ppage) && parent.pages.len() > 1 {
                    continue;
                }
                self.top_item(s, it, Affine::translate((dx, 0.0)), Some(first + pi), true);
            }
        }
    }

    pub(super) fn spread(&mut self, s: &mut Surface, si: usize) {
        let doc = self.doc;
        let Some(sp) = doc.spreads.get(si) else { return };
        let first = doc.first_page_of_spread(si);
        for layer in doc.layers.iter().rev() {
            if !layer_exported(layer) {
                continue;
            }
            for (pi, page) in sp.pages.iter().enumerate() {
                if page.show_parent_items {
                    self.parent_items(s, si, pi, page, layer, first);
                }
            }
            for it in &sp.items {
                if it.layer == layer.id {
                    self.top_item(s, it, Affine::IDENTITY, None, false);
                }
            }
        }
    }

    /// Push blend/opacity for an item drawn as a group. Returns the number of pushes.
    fn push_group(s: &mut Surface, opacity: f32, mode: DcBlend) -> usize {
        let mut n = 0;
        if mode != DcBlend::Normal {
            s.push_blend_mode(blend(mode));
            n += 1;
        }
        if opacity < 0.999 {
            s.push_opacity(norm(opacity));
            n += 1;
        } else if mode != DcBlend::Normal {
            s.push_isolated();
            n += 1;
        }
        n
    }

    /// Isolate Blending / Knockout Group: a transparency group with `/I` and `/K` as set.
    fn push_isolation(s: &mut Surface, it: &Item) -> usize {
        if it.isolate || it.knockout {
            s.push_group(krilla::blend::GroupOptions { isolated: it.isolate, knockout: it.knockout });
            1
        } else {
            0
        }
    }

    /// A spread or parent item, inside its structure tag when tagging.
    fn top_item(&mut self, s: &mut Surface, it: &Item, parent: Affine, page: Option<usize>, on_parent: bool) {
        if !self.opts.tagged || !item_exported(it) {
            return self.item(s, it, parent, page);
        }
        let story = it.text_frame().map(|t| t.story);
        let figure = !it.alt_text.is_empty() || matches!(it.content, Content::Graphic(_)) || it.has_nested_items();
        if on_parent || it.export_options.artifact || (story.is_none() && !figure) {
            // Page furniture and decoration.
            let kind = if on_parent { ArtifactType::Page } else { ArtifactType::Other };
            s.start_tagged(ContentTag::Artifact(Artifact::new(kind, None)));
            self.item(s, it, parent, page);
            s.end_tagged();
            return;
        }
        if let Some(sid) = story {
            // Text runs tag themselves by paragraph; the rest of the frame is an artifact.
            if let std::collections::hash_map::Entry::Vacant(e) = self.story_tags.entry(sid) {
                e.insert(Vec::new());
                self.tags.push(TagEntry::Story(sid));
            }
            self.tag_story = Some(sid);
            self.item(s, it, parent, page);
            self.tag_story = None;
            return;
        }
        let id = s.start_tagged(ContentTag::Other);
        self.item(s, it, parent, page);
        s.end_tagged();
        let alt = (!it.alt_text.is_empty()).then(|| it.alt_text.clone());
        let mut g = TagGroup::new(Tag::Figure(alt));
        g.push(id);
        self.tags.push(TagEntry::Figure(g));
    }

    /// The structure tree: stories (one paragraph each, its frames in thread order of appearance)
    /// and figures, in the order they first appear.
    pub(super) fn tag_tree(&mut self) -> TagTree {
        let mut tree = TagTree::new();
        for e in std::mem::take(&mut self.tags) {
            match e {
                TagEntry::Story(sid) => {
                    // One structure element per paragraph: headings and quotes from the
                    // paragraph style's Export Tagging, else P.
                    let mut paras: Vec<usize> = self.para_tags.keys().filter(|(s, _)| *s == sid).map(|(_, p)| *p).collect();
                    paras.sort_unstable();
                    let story = self.doc.story(sid);
                    for pi in paras {
                        let ids = self.para_tags.remove(&(sid, pi)).unwrap_or_default();
                        let style = story.and_then(|st| st.paras.get(pi)).map(|p| p.style.clone()).unwrap_or_default();
                        let tag = self.doc.styles.export_tag(&style, false).map(|e| e.tag.clone()).unwrap_or_default();
                        let text = || story.and_then(|st| st.para_ranges().get(pi).map(|r| st.text[r.clone()].trim().to_string()));
                        let kind: krilla::tagging::TagKind = match tag.as_str() {
                            h if h.len() == 2 && h.starts_with('h') => {
                                let n = h[1..].parse::<u16>().unwrap_or(1).clamp(1, 6);
                                Tag::Hn(std::num::NonZeroU16::new(n).unwrap_or(std::num::NonZeroU16::MIN), text()).into()
                            }
                            "blockquote" => Tag::BlockQuote.into(),
                            _ => Tag::P.into(),
                        };
                        let mut g = TagGroup::new(kind);
                        for id in ids {
                            g.push(id);
                        }
                        tree.push(g);
                    }
                }
                TagEntry::Figure(g) => tree.push(g),
            }
        }
        tree
    }

    fn item(&mut self, s: &mut Surface, it: &Item, parent: Affine, page: Option<usize>) {
        if self.drawing_items.len() >= 64 || self.drawing_items.contains(&it.id) {
            self.warnings.push(format!("Cyclic or excessively nested item {} was not rendered", it.id));
            return;
        }
        self.drawing_items.push(it.id);
        self.item_guarded(s, it, parent, page);
        self.drawing_items.pop();
    }

    fn item_guarded(&mut self, s: &mut Surface, it: &Item, parent: Affine, page: Option<usize>) {
        let gf = &it.effects.gradient_feather;
        if !gf.on || !item_exported(it) {
            return self.item_inner(s, it, parent, page);
        }
        // Gradient feather: a luminosity mask (white = opaque) over the object.
        let xf = parent * it.xf;
        let (p0, p1) = gf.points(it.feather_bounds());
        let gray = |a: f32| -> krilla::color::Color { luma::Color::new((a.clamp(0.0, 1.0) * 255.0).round() as u8).into() };
        let stops = vec![
            Stop { offset: NormalizedF32::ZERO, color: gray(gf.start), opacity: NormalizedF32::ONE },
            Stop { offset: NormalizedF32::ONE, color: gray(gf.end), opacity: NormalizedF32::ONE },
        ];
        let paint: krilla::paint::Paint = if gf.radial {
            let r = (p1 - p0).hypot().max(1e-3) as f32;
            RadialGradient {
                fx: p0.x as f32,
                fy: p0.y as f32,
                fr: 0.0,
                cx: p0.x as f32,
                cy: p0.y as f32,
                cr: r,
                transform: tf(xf),
                spread_method: SpreadMethod::Pad,
                stops,
                anti_alias: false,
            }
            .into()
        } else {
            LinearGradient {
                x1: p0.x as f32,
                y1: p0.y as f32,
                x2: p1.x as f32,
                y2: p1.y as f32,
                transform: tf(xf),
                spread_method: SpreadMethod::Pad,
                stops,
                anti_alias: false,
            }
            .into()
        };
        // The gradient keeps its item-space geometry, but its mask covers the complete exported
        // area (including bleed), so distant child shadows and thick strokes are never cut off.
        let mut sb = s.stream_builder();
        let mut ms = sb.surface();
        if let Some(p) = to_path(&self.clip.to_path(0.1)) {
            ms.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            ms.draw_path(&p);
        }
        ms.finish();
        let stream = sb.finish();
        s.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Luminosity));
        self.item_inner(s, it, parent, page);
        s.pop();
    }

    fn item_inner(&mut self, s: &mut Surface, it: &Item, parent: Affine, page: Option<usize>) {
        if !item_exported(it) {
            return;
        }
        let ds = &it.effects.drop_shadow;
        let grow = it.stroke.extent() + 2.0 + if ds.on { ds.distance.abs() + ds.size.abs() } else { 0.0 };
        let vb = parent.transform_rect_bbox(it.bounds()).inflate(grow, grow);
        if !rects_overlap(vb, self.clip) {
            return;
        }
        let xf = parent * it.xf;
        if it.has_nested_items() {
            self.draw_nested_item(s, it, xf, page);
            return;
        }
        if !it.children().is_empty() {
            self.draw_group_children(s, it, xf, page);
            return;
        }
        let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
        let path = to_path(&bp);
        self.draw_leaf_item(s, it, parent, page, &bp, path.as_ref());
    }

    fn draw_nested_item(&mut self, s: &mut Surface, it: &Item, xf: Affine, page: Option<usize>) {
        // A frame with items pasted into it: fill, the items clipped to the frame, stroke.
        let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
        let path = to_path(&bp);
        let pushes = Self::push_group(s, it.opacity, it.blend);
        s.push_transform(&tf(xf));
        if !it.fill.is_none()
            && let Some(p) = &path
            && let Some(paint) = self.fill_paint(&it.fill, bp.bounding_box())
        {
            self.fill_item_path(s, &it.fill, paint, p);
        }
        s.pop();
        // The clip in page space, so the children keep their absolute transforms.
        let mut page_bp = bp.clone();
        page_bp.apply_affine(xf);
        let clip = to_path(&page_bp);
        if let Some(c) = &clip {
            s.push_clip_path(c, &FillRule::NonZero);
        }
        for child in it.children() {
            self.item(s, child, xf, page);
        }
        if clip.is_some() {
            s.pop();
        }
        if !it.stroke.is_none()
            && let Some(p) = &path
        {
            s.push_transform(&tf(xf));
            self.stroke(s, it, &bp, p);
            s.pop();
        }
        for _ in 0..pushes {
            s.pop();
        }
    }

    fn draw_group_children(&mut self, s: &mut Surface, it: &Item, xf: Affine, page: Option<usize>) {
        let pushes = Self::push_group(s, it.opacity, it.blend) + Self::push_isolation(s, it);
        for child in it.shown_children() {
            self.item(s, child, xf, page);
        }
        for _ in 0..pushes {
            s.pop();
        }
    }

    fn draw_leaf_item(&mut self, s: &mut Surface, it: &Item, parent: Affine, page: Option<usize>, bp: &BezPath, path: Option<&Path>) {
        let xf = parent * it.xf;
        let pushes = Self::push_group(s, it.opacity, it.blend);
        let frame_art = self.tag_story.is_some();
        if frame_art {
            s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Other, None)));
        }
        self.draw_drop_shadow(s, it, parent, xf, path);
        s.push_transform(&tf(xf));
        if !it.fill.is_none()
            && let Some(p) = path
            && let Some(paint) = self.fill_paint(&it.fill, bp.bounding_box())
        {
            self.fill_item_path(s, &it.fill, paint, p);
        }
        if frame_art {
            s.end_tagged();
        }
        // A text frame's anchored objects are drawn under its text when they can be.
        let drawn = self.draw_leaf_content(s, it, xf, bp, path, page);
        self.draw_leaf_stroke(s, it, bp, path, frame_art);
        s.pop();
        for _ in 0..pushes {
            s.pop();
        }
        if let Content::Text(tfr) = &it.content
            && !drawn
        {
            let cs = self.composed(tfr.story, page);
            if let Some(ft) = cs.frame(it.id) {
                self.frame_objects(s, ft, xf * self.doc.text_local(it), page);
            }
        }
    }

    /// Draw a cached coverage mask or the offset item silhouette as the drop shadow.
    fn draw_drop_shadow(&mut self, s: &mut Surface, it: &Item, parent: Affine, xf: Affine, path: Option<&Path>) {
        let ds = &it.effects.drop_shadow;
        if ds.on
            && let Some(mask) = self.opts.shadows.get(&it.id)
            && let Some(c) = self.swatch_color(&ds.color, 1.0)
        {
            s.push_transform(&tf(parent));
            draw_coverage(s, mask, c);
            s.pop();
        } else if ds.on
            && let Some(p) = path
            && let Some(c) = self.swatch_color(&ds.color, 1.0)
        {
            let a = self.doc.light_angle(ds.angle, ds.global_light).to_radians();
            let off = Vec2::new(-a.cos() * ds.distance, a.sin() * ds.distance);
            s.push_transform(&tf(Affine::translate(off) * xf));
            s.set_stroke(None);
            s.set_fill(Some(solid_fill(c, ds.opacity)));
            s.draw_path(p);
            s.set_fill(None);
            s.pop();
        }
    }

    /// The item's content; returns whether a text frame's anchored objects were drawn with it.
    fn draw_leaf_content(&mut self, s: &mut Surface, it: &Item, xf: Affine, bp: &BezPath, path: Option<&Path>, page: Option<usize>) -> bool {
        let doc = self.doc;
        let mut drawn = false;
        match &it.content {
            Content::Graphic(g) => {
                let g = &if it.media.is_some() { it.drawn_graphic(&self.doc.assets).unwrap_or_else(|| g.clone()) } else { g.clone() };
                if let Some(p) = path {
                    s.push_clip_path(p, &FillRule::NonZero);
                    // An explicit all-on choice is `Some(&[])`, not the file's defaults: passing
                    // `None` would embed the source's default-hidden layers and skip the rewrite
                    // error that a stale choice must raise.
                    self.graphic(s, g, it.has_pdf_layer_override().then_some(it.pdf_hidden_layers.as_slice()));
                    s.pop();
                }
            }
            Content::Text(tfr) => {
                let cs = self.composed(tfr.story, page);
                if let (Some(ft), Some(pt)) = (cs.frame(it.id), &tfr.options.path) {
                    // Type on a path: the glyphs as outlines along the path.
                    for (si, gp) in designcraft_compose::path_glyphs(&cs, ft, bp, pt) {
                        let st = &cs.styles[si as usize];
                        // A gradient spans the path's box.
                        let fill = designcraft_doc::Fill {
                            swatch: st.fill.clone(),
                            tint: st.fill_tint,
                            overprint: st.overprint_fill,
                            ..designcraft_doc::Fill::none()
                        };
                        if let (Some(paint), Some(p)) = (self.fill_paint(&fill, bp.bounding_box()), to_path(&gp)) {
                            let op = self.overprints(&st.fill, st.fill_tint, st.overprint_fill);
                            let op = self.push_overprint(s, op, false);
                            s.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                            s.draw_path(&p);
                            s.set_fill(None);
                            if op {
                                s.pop();
                            }
                        }
                    }
                } else if let Some(ft) = cs.frame(it.id) {
                    let text = doc.story(tfr.story).map(|st| st.text.as_str()).unwrap_or("");
                    let local = doc.text_local(it);
                    if local != Affine::IDENTITY {
                        s.push_transform(&tf(local));
                    }
                    // Anchored text frames now draw inside `frame_text`: restore the host's area
                    // after, so an object drawn there doesn't clear it for the host's glyphs.
                    let area = (local.determinant().abs() > 1e-12).then(|| local.inverse().transform_rect_bbox(it.inner_bounds()));
                    let outer = std::mem::replace(&mut self.text_area, area);
                    drawn = self.frame_text(s, &cs, ft, text, Some((xf * local, page)));
                    self.text_area = outer;
                    if local != Affine::IDENTITY {
                        s.pop();
                    }
                }
            }
            _ => {}
        }
        drawn
    }

    fn draw_leaf_stroke(&mut self, s: &mut Surface, it: &Item, bp: &BezPath, path: Option<&Path>, frame_art: bool) {
        if !it.stroke.is_none()
            && let Some(p) = path
        {
            if frame_art {
                s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Other, None)));
            }
            self.stroke(s, it, bp, p);
            if frame_art {
                s.end_tagged();
            }
        }
    }

    /// Draw embedded page items after restoring the surface, retaining spread-space
    /// transforms for page culling (also for anonymous note and cell stories).
    pub(crate) fn frame_objects(&mut self, s: &mut Surface, ft: &designcraft_compose::FrameText, xf: Affine, page: Option<usize>) {
        let mut pending = vec![(ft, xf)];
        while let Some((frame, transform)) = pending.pop() {
            for object in &frame.objects {
                self.item(s, &object.object.item, transform * Affine::translate(object.origin.to_vec2()), page);
            }
            for note in &frame.notes {
                if let Some(frame) = note.text.frames.first() {
                    pending.push((frame, transform * Affine::translate(note.origin.to_vec2())));
                }
            }
            for cell in frame.tables.iter().flat_map(|t| &t.cells) {
                if let Some(frame) = cell.text.frames.first() {
                    pending.push((frame, transform * Affine::translate(cell.origin.to_vec2())));
                }
            }
        }
    }
}
