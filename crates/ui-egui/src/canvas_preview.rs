//! Tiny, disposable spread previews beneath the sharp viewport texture. Work is demand driven:
//! visible spreads and their immediate neighbours, one native job at a time, at most 32 textures.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use designcraft_doc::{Document, SpreadRef};
use designcraft_geom::Affine;
use designcraft_render::{Placed, RenderOptions, Rendered};
use designcraft_tools::layout::SpreadSlot;

const SIDE: u32 = 128;
const LIMIT: usize = 32;

#[derive(Clone, PartialEq)]
pub(crate) struct Settings {
    pub options: RenderOptions,
    pub parents: bool,
    pub colors: Option<designcraft_color::cms::ColorSettings>,
    pub proof: Option<designcraft_color::cms::ProofSetup>,
    /// The print separations overlay owns ink visualization; composite previews stay hidden.
    pub separations: bool,
}
impl Settings {
    pub fn coarse(mut self) -> Self {
        self.options.greek_below_px = 3.0;
        if self.options.quality == designcraft_render::DisplayQuality::High {
            self.options.quality = designcraft_render::DisplayQuality::Typical;
        }
        self
    }
}

struct Job {
    generation: u64,
    slot: SpreadSlot,
    doc: Arc<Document>,
    cache: Arc<designcraft_compose::Cache>,
    settings: Settings,
}
struct Done {
    generation: u64,
    spread: SpreadRef,
    image: Option<Rendered>,
}

fn dimensions(slot: SpreadSlot) -> Option<(u32, u32, Affine)> {
    let b = slot.bounds;
    let (w, h) = (b.width(), b.height());
    if ![b.x0, b.y0, b.x1, b.y1, w, h].iter().all(|x| x.is_finite()) || w < 1e-6 || h < 1e-6 {
        return None;
    }
    let scale = f64::from(SIDE) / w.max(h);
    let (pw, ph) = ((w * scale).ceil().clamp(1.0, f64::from(SIDE)) as u32, (h * scale).ceil().clamp(1.0, f64::from(SIDE)) as u32);
    let view = Affine::scale_non_uniform(f64::from(pw) / w, f64::from(ph) / h) * Affine::translate((-b.x0, -b.y0));
    Some((pw, ph, view))
}

fn render(job: Job) -> Done {
    let image = dimensions(job.slot).and_then(|(w, h, view)| {
        let mut renderer = designcraft_render::Renderer::new();
        renderer.threads = 0;
        let mut image =
            renderer.render(&job.doc, &job.cache, &[Placed { spread: job.slot.spread, xf: job.slot.xf }], w, h, view, &job.settings.options);
        if image.width == 0 || job.settings.options.cancellation.as_ref().is_some_and(designcraft_render::Cancellation::cancelled) {
            return None;
        }
        if let Some(proof) = &job.settings.proof {
            designcraft_render::proof_view(&mut image, proof);
        }
        Some(image)
    });
    designcraft_render::images::trim_originals();
    Done { generation: job.generation, spread: job.slot.spread, image }
}

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    tx: std::sync::mpsc::SyncSender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
    generation: Arc<std::sync::atomic::AtomicU64>,
}
#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    fn spawn(ctx: egui::Context) -> Option<Self> {
        let (tx, jobs) = std::sync::mpsc::sync_channel::<Job>(1);
        let (finished, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("designcraft-scroll-previews".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    if finished.send(render(job)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Self { tx, rx, generation: Arc::new(std::sync::atomic::AtomicU64::new(0)) })
    }
}

#[derive(Default)]
pub(crate) struct Previews {
    uid: u64,
    revision: u64,
    doc: Option<Arc<Document>>,
    settings: Option<Settings>,
    generation: u64,
    textures: HashMap<SpreadRef, (egui::TextureHandle, u64)>,
    failed: HashSet<SpreadRef>,
    clock: u64,
    pending: bool,
    pub rendered: u64,
    pub reused: u64,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<Worker>,
}
impl Previews {
    pub fn document_uid(&self) -> u64 {
        self.uid
    }

    pub fn retain_open(&mut self, open: bool) {
        if self.doc.is_some() && !open {
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        self.doc = None;
        self.settings = None;
        self.textures.clear();
        self.failed.clear();
        self.generation = self.generation.wrapping_add(1);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(worker) = &self.worker {
            worker.generation.store(self.generation, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn begin(&mut self, uid: u64, revision: u64, doc: Arc<Document>, settings: Settings, ctx: &egui::Context) {
        if self.uid != uid
            || self.revision != revision
            || self.doc.as_ref().is_none_or(|old| !Arc::ptr_eq(old, &doc))
            || self.settings.as_ref() != Some(&settings)
        {
            self.clear();
            self.uid = uid;
            self.revision = revision;
            self.doc = Some(doc);
            self.settings = Some(settings);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(done) = self.worker.as_ref().and_then(|worker| worker.rx.try_recv().ok()) {
            self.accept(done, ctx);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = ctx;
    }

    fn accept(&mut self, done: Done, ctx: &egui::Context) {
        self.pending = false;
        if done.generation != self.generation || self.doc.is_none() {
            return;
        }
        if let Some(image) = done.image.filter(|im| im.width > 0 && im.height > 0 && im.width <= SIDE && im.height <= SIDE) {
            if self.textures.len() >= LIMIT
                && let Some(oldest) = self.textures.iter().min_by_key(|(_, (_, stamp))| *stamp).map(|(key, _)| *key)
            {
                self.textures.remove(&oldest);
            }
            self.clock = self.clock.wrapping_add(1);
            let pixels = egui::ColorImage::from_rgba_premultiplied([image.width as usize, image.height as usize], &image.pixels);
            self.rendered = self.rendered.saturating_add(1);
            self.textures.insert(done.spread, (ctx.load_texture("scroll-preview", pixels, egui::TextureOptions::LINEAR), self.clock));
        } else {
            self.failed.insert(done.spread);
        }
    }

    pub fn get(&mut self, spread: SpreadRef) -> Option<egui::TextureHandle> {
        if self.settings.as_ref().is_some_and(|s| s.separations) {
            return None;
        }
        let (texture, stamp) = self.textures.get_mut(&spread)?;
        self.clock = self.clock.wrapping_add(1);
        *stamp = self.clock;
        self.reused = self.reused.saturating_add(1);
        Some(texture.clone())
    }

    /// Caller orders candidates by current viewport priority each frame; no navigation backlog.
    pub fn request(&mut self, slots: impl IntoIterator<Item = SpreadSlot>, cache: Arc<designcraft_compose::Cache>, ctx: &egui::Context) {
        if self.pending || self.settings.as_ref().is_some_and(|s| s.separations) {
            return;
        }
        let Some(slot) = slots.into_iter().find(|s| !self.textures.contains_key(&s.spread) && !self.failed.contains(&s.spread)) else { return };
        let (Some(doc), Some(settings)) = (self.doc.clone(), self.settings.clone()) else { return };
        let job = Job { generation: self.generation, slot, doc, cache, settings };
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.worker.is_none() {
                self.worker = Worker::spawn(ctx.clone());
            }
            if let Some(worker) = &self.worker {
                let mut job = job;
                worker.generation.store(job.generation, std::sync::atomic::Ordering::Relaxed);
                job.settings.options.cancellation =
                    Some(designcraft_render::Cancellation { generation: worker.generation.clone(), expected: job.generation });
                self.pending = worker.tx.try_send(job).is_ok();
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.accept(render(job), ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_tools::CanvasLayout;

    fn fixture() -> (crate::DesignApp, egui::Context, Settings) {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({"width": 64, "height": 64, "pages": 3, "facingPages": false})).unwrap();
        app.run("frame.create", serde_json::json!({"rect": [8, 8, 56, 56], "content": "unassigned"})).unwrap();
        app.run("object.fill", serde_json::json!({"swatch": "[Black]"})).unwrap();
        let settings = Settings { options: RenderOptions::default(), parents: false, colors: None, proof: None, separations: false }.coarse();
        (app, egui::Context::default(), settings)
    }

    fn begin(previews: &mut Previews, app: &crate::DesignApp, settings: Settings, ctx: &egui::Context) {
        let st = app.session.active().unwrap();
        previews.begin(st.uid, st.revision, st.doc.clone(), settings, ctx);
    }

    fn done(generation: u64, spread: SpreadRef) -> Done {
        Done { generation, spread, image: Some(Rendered { width: 1, height: 1, pixels: vec![255; 4] }) }
    }

    #[test]
    fn tiny_preview_preserves_artwork_and_rejects_invalid_bounds() {
        let (app, _, settings) = fixture();
        let st = app.session.active().unwrap();
        let mut slot = CanvasLayout::new(&st.doc, false).slots[0];
        let image =
            render(Job { generation: 1, slot, doc: st.doc.clone(), cache: app.session.cache.clone(), settings: settings.clone() }).image.unwrap();
        assert!(image.width <= SIDE && image.height <= SIDE);
        let center = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
        assert!(image.pixels[center..center + 3].iter().all(|v| *v < 80));
        assert_eq!(image.pixels[center + 3], 255);
        assert_eq!(&image.pixels[0..4], &[255; 4]);
        slot.bounds.x1 = f64::INFINITY;
        assert!(dimensions(slot).is_none());
        slot.bounds.x1 = slot.bounds.x0;
        assert!(dimensions(slot).is_none());
    }

    #[test]
    fn edits_parent_switches_and_display_changes_reject_stale_results() {
        let (mut app, ctx, settings) = fixture();
        let mut previews = Previews::default();
        begin(&mut previews, &app, settings.clone(), &ctx);
        let old = previews.generation;
        previews.accept(done(old, SpreadRef::Doc(0)), &ctx);
        assert!(previews.get(SpreadRef::Doc(0)).is_some());
        app.run("object.fill", serde_json::json!({"swatch": "[Paper]"})).unwrap();
        begin(&mut previews, &app, settings.clone(), &ctx);
        previews.accept(done(old, SpreadRef::Doc(0)), &ctx);
        assert!(previews.get(SpreadRef::Doc(0)).is_none());
        for next in [
            Settings { parents: true, ..settings.clone() },
            Settings { proof: Some(Default::default()), ..settings.clone() },
            Settings { separations: true, ..settings.clone() },
            Settings { options: RenderOptions { plate: Some(0), ..settings.options.clone() }, ..settings.clone() },
        ] {
            let old = previews.generation;
            begin(&mut previews, &app, next, &ctx);
            previews.accept(done(old, SpreadRef::Doc(0)), &ctx);
            assert!(previews.textures.is_empty());
        }
        previews.clear();
        previews.accept(done(previews.generation, SpreadRef::Doc(0)), &ctx);
        assert!(previews.textures.is_empty(), "closing a document must not restore its pixels");
    }

    #[test]
    fn composite_scroll_previews_yield_to_print_separations() {
        let (app, ctx, settings) = fixture();
        let mut previews = Previews::default();
        begin(&mut previews, &app, settings.clone(), &ctx);
        previews.accept(done(previews.generation, SpreadRef::Doc(0)), &ctx);
        assert!(previews.get(SpreadRef::Doc(0)).is_some());
        begin(&mut previews, &app, Settings { separations: true, ..settings }, &ctx);
        let rendered = previews.rendered;
        let layout = CanvasLayout::new(&app.session.active().unwrap().doc, false);
        previews.request(layout.slots, app.session.cache.clone(), &ctx);
        assert!(!previews.pending, "the print preview must own the next ink render");
        assert_eq!(previews.rendered, rendered);
        assert!(previews.textures.is_empty());
        assert!(previews.get(SpreadRef::Doc(0)).is_none());
    }

    #[test]
    fn preview_textures_have_a_fixed_budget_and_refresh_lru_on_display() {
        let (app, ctx, settings) = fixture();
        let mut previews = Previews::default();
        begin(&mut previews, &app, settings, &ctx);
        for i in 0..LIMIT {
            previews.accept(done(previews.generation, SpreadRef::Doc(i)), &ctx);
        }
        assert!(previews.get(SpreadRef::Doc(0)).is_some());
        previews.accept(done(previews.generation, SpreadRef::Doc(LIMIT)), &ctx);
        assert_eq!(previews.textures.len(), LIMIT);
        assert!(previews.get(SpreadRef::Doc(0)).is_some());
        assert!(previews.get(SpreadRef::Doc(1)).is_none());
    }
}
