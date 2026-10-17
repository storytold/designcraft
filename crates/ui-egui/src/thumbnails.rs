//! Per-window page thumbnails. Only visible rows request work, one page at a time; native
//! rasterization runs off the interface thread. Revisions invalidate affected spreads only.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::{Document, SpreadRef};
use egui::TextureHandle;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    page: usize,
    height: u32,
}

struct Job {
    generation: u64,
    key: Key,
    doc: Arc<Document>,
    cache: Arc<Cache>,
}
struct Done {
    generation: u64,
    key: Key,
    image: Option<designcraft_render::Rendered>,
}

fn render(job: Job) -> Done {
    let mut renderer = designcraft_render::Renderer::new();
    renderer.threads = 0;
    let image = job.doc.page(job.key.page).and_then(|page| {
        renderer.render_page(
            &job.doc,
            &job.cache,
            job.key.page,
            job.key.height as f64 / page.height.max(1.0),
            false,
            &designcraft_render::RenderOptions { printing_only: true, ..Default::default() },
        )
    });
    designcraft_render::images::trim_originals();
    Done { generation: job.generation, key: job.key, image }
}

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    tx: std::sync::mpsc::SyncSender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
}
#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    fn spawn(ctx: egui::Context) -> Option<Self> {
        let (tx, jobs) = std::sync::mpsc::sync_channel::<Job>(1);
        let (done, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("designcraft-thumbnails".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    if done.send(render(job)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Self { tx, rx })
    }
}

#[derive(Default)]
pub(crate) struct Thumbnails {
    uid: u64,
    revision: u64,
    colour: u64,
    generation: u64,
    doc: Option<Arc<Document>>,
    textures: HashMap<Key, (TextureHandle, u64)>,
    clock: u64,
    failed: HashSet<Key>,
    requested: Vec<Key>,
    pending: bool,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<Worker>,
    pub rendered: u64,
    pub reused: u64,
}

fn affected(old: &Document, new: &Document) -> Option<HashSet<usize>> {
    // The regional damage helper handles geometry, styles, threaded stories and parents;
    // asset replacement changes pixels even if every frame and its geometry stay unchanged.
    if old.font_scope != new.font_scope
        || old.conditions != new.conditions
        || old.inks != new.inks
        || old.endnote_options != new.endnote_options
        || old.assets.len() != new.assets.len()
        || old.assets.iter().any(|(id, a)| new.assets.get(id).is_none_or(|b| !Arc::ptr_eq(a, b)))
    {
        return None;
    }
    let regions = designcraft_render::damage::damage(old, new)?;
    let mut pages = HashSet::new();
    for (spread, _) in regions {
        let SpreadRef::Doc(si) = spread else { return None };
        let sp = new.spreads.get(si)?;
        let first = new.first_page_of_spread(si);
        pages.extend(first..first.saturating_add(sp.pages.len()));
    }
    Some(pages)
}

impl Thumbnails {
    pub fn document_uid(&self) -> u64 {
        self.uid
    }

    pub fn retain_open(&mut self, open: bool) {
        if self.doc.is_some() && !open {
            self.doc = None;
            self.textures.clear();
            self.failed.clear();
            self.requested.clear();
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn begin(&mut self, uid: u64, revision: u64, doc: Arc<Document>, ctx: &egui::Context) {
        #[cfg(target_arch = "wasm32")]
        let _ = ctx;
        let colour = designcraft_color::cms::generation();
        if self.doc.is_none()
            || uid != self.uid
            || revision != self.revision
            || colour != self.colour
            || self.doc.as_ref().is_some_and(|old| !Arc::ptr_eq(old, &doc))
        {
            let keep = if self.uid == uid && self.colour == colour { self.doc.as_ref().and_then(|old| affected(old, &doc)) } else { None };
            self.textures.retain(|key, _| keep.as_ref().is_some_and(|pages| !pages.contains(&key.page)));
            self.reused = self.reused.saturating_add(self.textures.len() as u64);
            self.failed.clear();
            self.generation = self.generation.wrapping_add(1);
            self.uid = uid;
            self.revision = revision;
            self.colour = colour;
            self.doc = Some(doc);
        }
        self.requested.clear();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(done) = self.worker.as_ref().and_then(|w| w.rx.try_recv().ok()) {
            self.accept(done, ctx);
        }
    }

    fn accept(&mut self, done: Done, ctx: &egui::Context) {
        self.pending = false;
        if done.generation != self.generation {
            return;
        }
        if let Some(image) = done.image {
            let ci = egui::ColorImage::from_rgba_premultiplied([image.width as usize, image.height as usize], &image.pixels);
            self.clock = self.clock.wrapping_add(1);
            if self.textures.len() >= 128
                && let Some(oldest) = self.textures.iter().min_by_key(|(_, (_, stamp))| *stamp).map(|(key, _)| *key)
            {
                self.textures.remove(&oldest);
            }
            self.textures.insert(
                done.key,
                (ctx.load_texture(format!("page-thumb-{}-{}", self.uid, done.key.page), ci, egui::TextureOptions::LINEAR), self.clock),
            );
            self.rendered = self.rendered.saturating_add(1);
        } else {
            self.failed.insert(done.key);
        }
    }

    pub fn get(&mut self, page: usize, height: u32) -> Option<TextureHandle> {
        let key = Key { page, height: height.clamp(1, 256) };
        if let Some((texture, stamp)) = self.textures.get_mut(&key) {
            self.clock = self.clock.wrapping_add(1);
            *stamp = self.clock;
            return Some(texture.clone());
        }
        if !self.failed.contains(&key) && !self.requested.contains(&key) {
            self.requested.push(key);
        }
        None
    }

    pub fn finish(&mut self, ctx: &egui::Context, cache: Arc<Cache>, current: usize) {
        if self.pending {
            return;
        }
        let Some(key) = self.requested.iter().min_by_key(|key| key.page.abs_diff(current)).copied() else { return };
        let Some(doc) = self.doc.clone() else { return };
        let job = Job { generation: self.generation, key, doc, cache };
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.worker.is_none() {
                self.worker = Worker::spawn(ctx.clone());
            }
            if let Some(worker) = &self.worker {
                self.pending = worker.tx.try_send(job).is_ok();
                return;
            }
        }
        // Browser/no-thread fallback: at most one small thumbnail per frame.
        self.accept(render(job), ctx);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::build::NewDocument;

    #[test]
    fn unrelated_spreads_keep_thumbnails_but_story_and_asset_changes_invalidate() {
        let doc = Document::new(&NewDocument { pages: 3, facing_pages: false, ..Default::default() });
        let mut next = doc.clone();
        let id = next.alloc();
        let layer = next.default_layer();
        let item = designcraft_doc::Item::new(
            designcraft_doc::ItemId(id),
            layer,
            designcraft_doc::Shape::Rectangle,
            designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 10.0, 10.0)),
        );
        next.insert_item(SpreadRef::Doc(1), item, None).unwrap();
        let changed = affected(&doc, &next).unwrap();
        assert_eq!(changed, HashSet::from([1]));
        next.settings.page_width += 10.0;
        assert!(affected(&doc, &next).is_none());
    }

    #[test]
    fn old_background_results_cannot_replace_a_newer_revision() {
        let ctx = egui::Context::default();
        let mut thumbs = Thumbnails { generation: 3, pending: true, ..Default::default() };
        thumbs.accept(Done { generation: 2, key: Key { page: 0, height: 37 }, image: None }, &ctx);
        assert!(!thumbs.pending);
        assert!(thumbs.failed.is_empty());
    }
    #[test]
    fn page_thumbnail_renders_at_each_requested_panel_size() {
        let doc = Arc::new(Document::new(&NewDocument { width: 200.0, height: 100.0, facing_pages: false, ..Default::default() }));
        for height in [24, 37, 55, 120] {
            let done = render(Job { generation: 1, key: Key { page: 0, height }, doc: doc.clone(), cache: Arc::new(Cache::new()) });
            let image = done.image.unwrap();
            assert_eq!((image.width, image.height), (height * 2, height));
            assert_eq!(image.pixel(height, height / 2), [255, 255, 255, 255]);
        }
    }
}
