//! Background canvas rendering (native): a thread that owns its own `Renderer` and renders
//! requested regions while the UI keeps drawing the previous texture (shifted/scaled).
//! On wasm the canvas renders synchronously.

use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};

use designcraft_compose::Cache;
use designcraft_doc::Document;
use designcraft_geom::Affine;
use designcraft_render::{Placed, RenderOptions, Rendered};

pub struct Job {
    pub token: u64,
    pub doc: Arc<Document>,
    pub cache: Arc<Cache>,
    pub placed: Vec<Placed>,
    pub w: u32,
    pub h: u32,
    pub view: Affine,
    pub opts: RenderOptions,
}

pub struct Done {
    pub token: u64,
    pub image: Rendered,
    pub ms: f64,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Worker {
    tx: std::sync::mpsc::Sender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
    generation: Arc<AtomicU64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    pub fn spawn(ctx: egui::Context) -> Option<Self> {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        let generation = Arc::new(AtomicU64::new(0));
        std::thread::Builder::new()
            .name("designcraft-render".into())
            .spawn(move || {
                let mut r = designcraft_render::Renderer::new();
                while let Ok(mut job) = jobs.recv() {
                    // Only the newest job matters.
                    while let Ok(newer) = jobs.try_recv() {
                        job = newer;
                    }
                    let t0 = crate::now_ms();
                    let image = r.render(&job.doc, &job.cache, &job.placed, job.w, job.h, job.view, &job.opts);
                    designcraft_render::images::trim_originals();
                    if image.width == 0 || job.opts.cancellation.as_ref().is_some_and(designcraft_render::Cancellation::cancelled) {
                        continue;
                    }
                    if done_tx.send(Done { token: job.token, image, ms: crate::now_ms() - t0 }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Worker { tx, rx, generation })
    }
    pub fn submit(&self, mut job: Job) {
        self.generation.store(job.token, Ordering::Relaxed);
        job.opts.cancellation = Some(designcraft_render::Cancellation { generation: self.generation.clone(), expected: job.token });
        let _ = self.tx.send(job);
    }
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
    pub fn poll(&self) -> Option<Done> {
        let mut last = None;
        while let Ok(d) = self.rx.try_recv() {
            last = Some(d);
        }
        last
    }
}

#[cfg(target_arch = "wasm32")]
pub struct Worker;
