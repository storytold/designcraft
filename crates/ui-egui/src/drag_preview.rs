//! Retained image-frame dragging: the moving frame sits between cached lower/upper layers.
//! Source pixels, clipping and z-order are rendered once; pointer updates only move a texture.
// Web uses the lower-resolution synchronous fallback; retained-layer preparation is native.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]
use crate::canvas::Xf;
use designcraft_doc::{Document, Item, ItemId, SpreadRef, WrapMode};
use designcraft_engine::DocState;
use designcraft_geom::{Affine, Rect, Vec2};
use designcraft_render::{Cancellation, DrawSlice, Placed, RenderOptions, Rendered, SlicePart};
use egui::{Color32, TextureHandle};
use std::sync::Arc;

#[derive(Clone)]
struct Key {
    doc: Arc<Document>,
    id: ItemId,
    viewport: Rect,
    scale: f64,
    opts: RenderOptions,
}
impl Key {
    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.doc, &other.doc)
            && self.id == other.id
            && self.viewport == other.viewport
            && self.scale == other.scale
            && self.opts == other.opts
    }
}
struct Job {
    generation: u64,
    key: Key,
    placed: Vec<Placed>,
    object: Rect,
    cache: Arc<designcraft_compose::Cache>,
}
struct Done {
    generation: u64,
    images: Vec<(Rect, Rendered)>,
}
struct Layer {
    rect: Rect,
    texture: TextureHandle,
}
#[derive(Default)]
pub(crate) struct Preview {
    key: Option<Key>,
    layers: Vec<Layer>,
    delta: Vec2,
    generation: u64,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<Worker>,
    pub prepared: u64,
    pub moved_frames: u64,
    pub preparing_frames: u64,
}

fn ordinary(it: &Item, depth: usize) -> bool {
    depth <= 64 && it.blend == Default::default() && !it.knockout && !it.effects.any() && it.children().iter().all(|c| ordinary(c, depth + 1))
}

/// Retained layers cannot approximate backdrop-dependent blend modes or live text-wrap reflow.
fn eligible(doc: &Document, id: ItemId, opts: &RenderOptions) -> Option<SpreadRef> {
    let loc = doc.find(id)?;
    let it = doc.item(id)?;
    if !matches!(loc.spread, SpreadRef::Doc(_))
        || loc.path.len() != 1
        || it.graphic().is_none()
        || it.hidden
        || it.locked
        || it.wrap.mode != WrapMode::None
        || opts.overprint_preview
        || opts.plate.is_some()
        || doc.layer(it.layer).is_none_or(|l| !l.visible || opts.printing_only && !l.printable)
        || opts.printing_only && it.nonprinting
    {
        return None;
    }
    // A non-normal blend above the frame needs the changing backdrop, not a flattened overlay.
    for sp in std::iter::once(doc.spread(loc.spread)?).chain(doc.parents.iter().map(AsRef::as_ref)) {
        if sp.items.iter().any(|i| !ordinary(i, 0)) {
            return None;
        }
        if opts.blend_space_view
            && doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk
            && sp.items.iter().any(|i| i.involves_transparency())
        {
            return None;
        }
    }
    Some(loc.spread)
}

impl Preview {
    pub fn ready(&self) -> bool {
        self.layers.len() == 3
    }
    pub fn clear(&mut self) {
        if self.key.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(w) = &self.worker {
                w.cancel();
            }
        }
        self.layers.clear();
    }
    /// `hold` keeps the last correctly stacked preview until the final canvas job arrives.
    pub fn update(
        &mut self,
        st: &DocState,
        ctx: &egui::Context,
        placed: Vec<Placed>,
        viewport: Rect,
        scale: f64,
        opts: RenderOptions,
        cache: Arc<designcraft_compose::Cache>,
        warm: bool,
        hold: bool,
        enabled: bool,
    ) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (st, ctx, placed, viewport, scale, opts, cache, warm, hold, enabled);
            false
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if hold && self.ready() {
                if let Some(key) = &self.key {
                    self.delta = delta(&key.doc, &st.doc, key.id, &placed).unwrap_or(Vec2::ZERO);
                }
                return false;
            }
            let interaction = st.interaction.as_ref();
            let active = interaction.is_some();
            let moving = interaction.and_then(|i| i.preview.as_ref()).is_some_and(|(cmd, p)| {
                cmd == "transform.move" && p.get("copy").and_then(|v| v.as_bool()) != Some(true) && p.get("toSpread").is_none()
            });
            if !enabled || st.editing_parents || active && !moving {
                self.clear();
                return false;
            }
            let (doc, selected) = match interaction {
                Some(i) => (i.doc.clone(), &i.selection.items),
                None => (st.doc.clone(), &st.selection.items),
            };
            let [id] = selected.as_slice() else {
                self.clear();
                return false;
            };
            if eligible(&doc, *id, &opts).is_none() {
                self.clear();
                return false;
            }
            let Some(delta) = delta(&doc, &st.doc, *id, &placed) else {
                self.clear();
                return false;
            };
            self.delta = delta;
            let key = Key { doc, id: *id, viewport, scale: scale.min(1.0), opts };
            if self.key.as_ref().is_none_or(|old| !old.same(&key)) {
                if !active && !warm {
                    self.clear();
                    return false;
                }
                self.clear();
                if self.worker.is_none() {
                    self.worker = Worker::spawn(ctx.clone());
                }
                let Some(worker) = &self.worker else { return false };
                let Some(loc) = key.doc.find(key.id) else { return false };
                let Some(item) = key.doc.item(key.id) else { return false };
                let Some(pl) = placed.iter().find(|p| p.spread == loc.spread) else { return false };
                // Keep wide borders, joins and arrowheads inside the moving texture as well.
                let linear = item.xf.as_coeffs().iter().take(4).map(|v| v.abs()).sum::<f64>().max(1.0);
                let stroke = if item.stroke.is_none() { 0.0 } else { item.stroke.extent() * item.stroke.miter_limit.max(1.0) * linear };
                let margin = stroke + 2.0 / key.scale.max(0.01);
                let object = pl.xf.transform_rect_bbox(item.bounds().inflate(margin, margin));
                if !object.width().is_finite() || !object.height().is_finite() || object.width() <= 0.0 || object.height() <= 0.0 {
                    return false;
                }
                self.generation = self.generation.wrapping_add(1);
                worker.submit(Job { generation: self.generation, key: key.clone(), placed, object, cache });
                self.key = Some(key);
            }
            if let Some(done) = self.worker.as_ref().and_then(Worker::poll)
                && done.generation == self.generation
            {
                self.layers = done
                    .images
                    .into_iter()
                    .enumerate()
                    .map(|(i, (rect, img))| Layer {
                        rect,
                        texture: ctx.load_texture(
                            format!("drag-{}-{i}", self.generation),
                            egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels),
                            egui::TextureOptions::LINEAR,
                        ),
                    })
                    .collect();
                self.prepared = self.prepared.saturating_add(1);
            }
            if active {
                if self.ready() {
                    self.moved_frames = self.moved_frames.saturating_add(1);
                } else {
                    self.preparing_frames = self.preparing_frames.saturating_add(1);
                }
            }
            active
        }
    }
    pub fn paint(&self, painter: &egui::Painter, xf: &Xf) {
        for (i, layer) in self.layers.iter().enumerate() {
            let r = if i == 1 { layer.rect + self.delta } else { layer.rect };
            let mut mesh = egui::Mesh::with_texture(layer.texture.id());
            for (p, uv) in [
                (r.origin(), egui::pos2(0.0, 0.0)),
                (designcraft_geom::Point::new(r.x1, r.y0), egui::pos2(1.0, 0.0)),
                (designcraft_geom::Point::new(r.x1, r.y1), egui::pos2(1.0, 1.0)),
                (designcraft_geom::Point::new(r.x0, r.y1), egui::pos2(0.0, 1.0)),
            ] {
                mesh.vertices.push(egui::epaint::Vertex { pos: xf.to_screen(p), uv, color: Color32::WHITE });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3]);
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}
fn delta(old: &Document, new: &Document, id: ItemId, placed: &[Placed]) -> Option<Vec2> {
    let loc = old.find(id)?;
    if new.find(id)?.spread != loc.spread {
        return None;
    }
    let a = old.item(id)?.xf.as_coeffs();
    let b = new.item(id)?.xf.as_coeffs();
    if a[..4] != b[..4] {
        return None;
    }
    let xf = placed.iter().find(|p| p.spread == loc.spread)?.xf;
    let origin = xf * designcraft_geom::Point::ZERO;
    Some(xf * designcraft_geom::Point::new(b[4] - a[4], b[5] - a[5]) - origin)
}

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    tx: std::sync::mpsc::Sender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
    latest: Arc<std::sync::atomic::AtomicU64>,
}
#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    fn poll(&self) -> Option<Done> {
        let mut latest = None;
        while let Ok(done) = self.rx.try_recv() {
            latest = Some(done);
        }
        latest
    }
    fn cancel(&self) {
        self.latest.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn submit(&self, job: Job) {
        self.latest.store(job.generation, std::sync::atomic::Ordering::Relaxed);
        let _ = self.tx.send(job);
    }
    fn spawn(ctx: egui::Context) -> Option<Self> {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done, rx) = std::sync::mpsc::channel();
        let latest = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let generation = latest.clone();
        std::thread::Builder::new()
            .name("designcraft-drag".into())
            .spawn(move || {
                let mut renderer = designcraft_render::Renderer::new();
                renderer.threads = 0;
                while let Ok(mut job) = jobs.recv() {
                    while let Ok(next) = jobs.try_recv() {
                        job = next;
                    }
                    let cancellation = Cancellation { generation: generation.clone(), expected: job.generation };
                    let mut images = Vec::new();
                    for part in [SlicePart::Before, SlicePart::Item, SlicePart::After] {
                        if cancellation.cancelled() {
                            break;
                        }
                        let rect = if part == SlicePart::Item { job.object } else { job.key.viewport };
                        let scale = job.key.scale.min(2048.0 / rect.width().max(rect.height()).max(1.0));
                        let (w, h) =
                            ((rect.width() * scale).ceil().clamp(1.0, 2048.0) as u32, (rect.height() * scale).ceil().clamp(1.0, 2048.0) as u32);
                        let view =
                            Affine::scale_non_uniform(w as f64 / rect.width(), h as f64 / rect.height()) * Affine::translate((-rect.x0, -rect.y0));
                        let mut opts = job.key.opts.clone();
                        opts.slice = Some(DrawSlice { item: job.key.id, part });
                        opts.cancellation = Some(cancellation.clone());
                        if part != SlicePart::Before {
                            opts.background = None;
                        }
                        let image = renderer.render(&job.key.doc, &job.cache, &job.placed, w, h, view, &opts);
                        if image.width == 0 || cancellation.cancelled() {
                            break;
                        }
                        images.push((rect, image));
                    }
                    designcraft_render::images::trim_originals();
                    if images.len() == 3 && !cancellation.cancelled() {
                        if done.send(Done { generation: job.generation, images }).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
            })
            .ok()?;
        Some(Self { tx, rx, latest })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use designcraft_doc::{Asset, AssetId, Content, Graphic, ItemId, Shape};
    use designcraft_geom::shapes;
    use serde_json::json;
    fn fixture() -> Result<(Document, ItemId), designcraft_doc::DocError> {
        let mut doc = Document::new(&designcraft_doc::build::NewDocument { width: 300.0, height: 200.0, facing_pages: false, ..Default::default() });
        doc.settings.blend_space = designcraft_doc::BlendSpace::Rgb;
        let aid = AssetId(doc.alloc());
        doc.assets.insert(aid,Arc::new(Asset {id:aid,name:"synthetic drag graphic".into(),mime:"image/svg+xml".into(),link:None,
            data:Arc::new(br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="red" fill-opacity="0.5"/></svg>"#.to_vec()),pixels:Some((100,100)),page:0,..Default::default()}));
        let id = ItemId(doc.alloc());
        let mut it = Item::new(id, doc.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(30.0, 30.0, 230.0, 170.0)));
        it.content = Content::Graphic(Graphic {
            asset: aid,
            size: (300.0, 300.0),
            xf: Affine::translate((-20.0, -30.0)),
            clip: None,
            wrap: Default::default(),
            fill: Default::default(),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [10.0, 20.0, 30.0, 40.0],
            layer_options: None,
        });
        doc.insert_item(SpreadRef::Doc(0), it, None)?;
        Ok((doc, id))
    }
    fn frame(app: &mut crate::DesignApp, ctx: &egui::Context) {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                max_texture_side: Some(8192),
                ..Default::default()
            },
            |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
        );
        out.textures_delta.clear();
    }
    fn until(app: &mut crate::DesignApp, ctx: &egui::Context, p: impl Fn(&crate::DesignApp) -> bool) {
        let start = std::time::Instant::now();
        while !p(app) {
            assert!(start.elapsed().as_secs() < 10, "asynchronous render timeout");
            frame(app, ctx);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    #[test]
    fn retained_drag_accepts_crop_alpha_and_rejects_backdrop_dependent_cases() {
        let (mut doc, id) = fixture().unwrap();
        let opts = RenderOptions::default();
        doc.item_mut(id).unwrap().opacity = 0.6;
        assert_eq!(eligible(&doc, id, &opts), Some(SpreadRef::Doc(0)));
        doc.item_mut(id).unwrap().wrap.mode = WrapMode::BoundingBox;
        assert!(eligible(&doc, id, &opts).is_none());
        doc.item_mut(id).unwrap().wrap.mode = WrapMode::None;
        doc.item_mut(id).unwrap().blend = designcraft_doc::color::BlendMode::Multiply;
        assert!(eligible(&doc, id, &opts).is_none());
        doc.item_mut(id).unwrap().blend = Default::default();
        doc.settings.blend_space = designcraft_doc::BlendSpace::Cmyk;
        assert!(eligible(&doc, id, &RenderOptions { blend_space_view: true, ..Default::default() }).is_none());
    }
    #[test]
    fn retained_image_drag_reuses_pixels_then_restores_commit_cancel_and_undo() {
        let (doc, id) = fixture().unwrap();
        let mut s = designcraft_engine::Session::new();
        s.add_document(DocState::new(doc, None));
        s.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
        let mut app = crate::DesignApp::new(s, crate::Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| a.canvas.drag.ready());
        let source = app.session.active().unwrap().doc.clone();
        let prepared = app.canvas.drag.prepared;
        app.session.begin_interaction("Move").unwrap();
        for n in 1..=8 {
            app.session.preview("transform.move", json!({"dx":n as f64,"dy":4.0})).unwrap();
            frame(&mut app, &ctx);
        }
        assert_eq!(app.canvas.drag.prepared, prepared, "a translation must not re-rasterize the image");
        assert!(app.canvas.drag.moved_frames >= 8);
        assert!(Arc::ptr_eq(app.canvas.shown_doc.as_ref().unwrap(), &source), "normal canvas must stay retained during the drag");
        app.session.commit_interaction().unwrap();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| !a.canvas.defer_full && a.canvas.pending.is_none());
        assert!(!app.canvas.low_quality);
        assert!(Arc::ptr_eq(app.canvas.shown_doc.as_ref().unwrap(), &app.session.active().unwrap().doc));
        app.session.execute("edit.undo", &json!({})).unwrap();
        frame(&mut app, &ctx);
        assert_eq!(app.session.active().unwrap().doc.item(id).unwrap().xf, source.item(id).unwrap().xf);
        app.session.begin_interaction("Cancel").unwrap();
        app.session.preview("transform.move", json!({"dx":40.0})).unwrap();
        frame(&mut app, &ctx);
        app.session.cancel_interaction();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| !a.canvas.defer_full && a.canvas.pending.is_none());
        assert_eq!(app.session.active().unwrap().doc.item(id).unwrap().xf, source.item(id).unwrap().xf);
        let placed = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
        let old = source.item(id).unwrap().xf;
        let mut moved = (*source).clone();
        moved.item_mut(id).unwrap().xf = Affine::translate((25.0, -7.0)) * old;
        assert_eq!(delta(&source, &moved, id, &placed), Some(Vec2::new(25.0, -7.0)));
        moved.item_mut(id).unwrap().xf = Affine::scale(2.0) * old;
        assert!(delta(&source, &moved, id, &placed).is_none());
        app.session.close_document(0);
        app.logic(&ctx);
        assert!(app.canvas.drag.key.is_none());
    }
    #[test]
    fn immediate_drag_keeps_moving_while_retained_layers_prepare() {
        let (doc, id) = fixture().unwrap();
        let mut session = designcraft_engine::Session::new();
        session.add_document(DocState::new(doc, None));
        session.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
        let mut app = crate::DesignApp::new(session, crate::Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx);
        app.session.begin_interaction("Immediate move").unwrap();
        app.session.preview("transform.move", json!({"dx":20.0,"dy":10.0})).unwrap();
        frame(&mut app, &ctx);
        assert!(
            app.canvas.drag.ready() || app.canvas.shown_doc.as_ref().is_some_and(|d| Arc::ptr_eq(d, &app.session.active().unwrap().doc)),
            "first drag must draw a moving preview, not freeze the old canvas"
        );
        if !app.canvas.drag.ready() {
            assert!(Arc::ptr_eq(app.canvas.shown_doc.as_ref().unwrap(), &app.session.active().unwrap().doc));
        }
        app.session.cancel_interaction();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| !a.canvas.defer_full && a.canvas.pending.is_none());
        assert!(!app.canvas.low_quality);
    }

    #[test]
    fn small_scene_keeps_partial_redraw_and_pan_empty_commit_restore() {
        let (mut doc, id) = fixture().unwrap();
        let marker = ItemId(doc.alloc());
        let mut item = Item::new(marker, doc.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(250.0, 30.0, 280.0, 60.0)));
        item.fill = designcraft_doc::Fill::swatch(designcraft_color::swatch::BLACK);
        item.blend = designcraft_doc::color::BlendMode::Multiply;
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        let mut session = designcraft_engine::Session::new();
        session.add_document(DocState::new(doc, None));
        session.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
        let mut app = crate::DesignApp::new(session, crate::Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| a.canvas.texture.is_some() && a.canvas.pending.is_none());
        let patches = app.canvas.patches;
        app.session.begin_interaction("Small move").unwrap();
        for n in 1..=3 {
            app.session.preview("transform.move", json!({"dx":n as f64,"dy":1.0})).unwrap();
            frame(&mut app, &ctx);
        }
        assert!(app.canvas.patches >= patches + 3);
        assert!(!app.canvas.low_quality);
        assert!(Arc::ptr_eq(app.canvas.shown_doc.as_ref().unwrap(), &app.session.active().unwrap().doc));
        app.session.commit_interaction().unwrap();
        frame(&mut app, &ctx);
        assert!(!app.canvas.defer_full && app.canvas.pending.is_none());
        app.session.begin_interaction("No change").unwrap();
        frame(&mut app, &ctx);
        app.session.commit_interaction().unwrap();
        frame(&mut app, &ctx);
        assert!(!app.canvas.defer_full && !app.canvas.low_quality && app.canvas.pending.is_none());
        let patches = app.canvas.patches;
        let origin = app.canvas.shown.unwrap().origin;
        app.view_mut().unwrap().origin.y += 2000.0;
        app.session.begin_interaction("Move while panning").unwrap();
        app.session.preview("transform.move", json!({"dx":5.0})).unwrap();
        frame(&mut app, &ctx);
        assert_eq!(app.canvas.patches, patches, "an old texture cannot patch pixels outside its viewport");
        assert!(app.canvas.low_quality);
        assert!(app.canvas.shown.unwrap().origin.y > origin.y + 1000.0);
        app.session.cancel_interaction();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| !a.canvas.defer_full && a.canvas.pending.is_none());
        assert!(!app.canvas.low_quality);
    }

    #[test]
    fn escape_cancels_real_pointer_drag_and_later_mouse_up_cannot_commit() {
        let (doc, id) = fixture().unwrap();
        let mut session = designcraft_engine::Session::new();
        session.add_document(DocState::new(doc, None));
        session.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
        let mut app = crate::DesignApp::new(session, crate::Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| a.canvas.drag.ready());
        let original = app.session.active().unwrap().doc.item(id).unwrap().xf;
        let layout = designcraft_tools::CanvasLayout::new(&app.session.active().unwrap().doc, false);
        let start = layout.to_canvas(SpreadRef::Doc(0), designcraft_geom::Point::new(100.0, 80.0));
        let end = start + Vec2::new(20.0, 10.0);
        let view = app.view_info();
        for (kind, pos) in [(designcraft_tools::PointerKind::Down, start), (designcraft_tools::PointerKind::Drag, end)] {
            app.session.pointer(&designcraft_tools::PointerEvent { kind, pos, mods: Default::default(), clicks: 1 }, view).unwrap();
        }
        frame(&mut app, &ctx);
        assert!(app.session.active().unwrap().interaction.is_some());
        assert_ne!(app.session.active().unwrap().doc.item(id).unwrap().xf, original);
        assert!(app.session.tool_key(designcraft_tools::ToolKey::Escape, Default::default(), view).unwrap());
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| !a.canvas.defer_full && a.canvas.pending.is_none());
        assert!(app.session.active().unwrap().interaction.is_none());
        assert_eq!(app.session.active().unwrap().doc.item(id).unwrap().xf, original);
        assert!(!app.session.tool_busy());
        app.session
            .pointer(
                &designcraft_tools::PointerEvent { kind: designcraft_tools::PointerKind::Up, pos: end, mods: Default::default(), clicks: 1 },
                view,
            )
            .unwrap();
        frame(&mut app, &ctx);
        assert!(app.session.active().unwrap().interaction.is_none());
        assert_eq!(app.session.active().unwrap().doc.item(id).unwrap().xf, original);
        assert!(!app.canvas.low_quality);
    }

    #[test]
    fn complex_drag_uses_a_draft_and_release_restores_full_resolution() {
        let (mut doc, id) = fixture().unwrap();
        doc.item_mut(id).unwrap().effects.drop_shadow.on = true;
        let mut s = designcraft_engine::Session::new();
        s.add_document(DocState::new(doc, None));
        s.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
        let mut app = crate::DesignApp::new(s, crate::Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| a.canvas.texture.is_some() && a.canvas.pending.is_none());
        let full_density = app.canvas.texture.as_ref().unwrap().size()[1] as f64 / app.canvas.shown.unwrap().size.1 as f64;
        app.session.begin_interaction("Move").unwrap();
        app.session.preview("transform.move", json!({"dx":14.0,"dy":3.0})).unwrap();
        frame(&mut app, &ctx);
        assert!(app.canvas.low_quality);
        let draft_density = app.canvas.texture.as_ref().unwrap().size()[1] as f64 / app.canvas.shown.unwrap().size.1 as f64;
        assert!(draft_density < full_density * 0.6);
        app.session.commit_interaction().unwrap();
        frame(&mut app, &ctx);
        until(&mut app, &ctx, |a| !a.canvas.defer_full && a.canvas.pending.is_none());
        assert!(!app.canvas.low_quality);
        let restored_density = app.canvas.texture.as_ref().unwrap().size()[1] as f64 / app.canvas.shown.unwrap().size.1 as f64;
        assert!((restored_density - ctx.pixels_per_point() as f64).abs() < 0.01);
        assert!((restored_density - full_density).abs() < 0.01);
    }
}
