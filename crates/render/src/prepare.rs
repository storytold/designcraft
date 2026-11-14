//! Work a graphic needs before it draws, other than decoding its pixels: rewriting a placed PDF
//! with layers hidden, measuring a Bounding Box crop, finding an image's tone. Each needs the
//! file's bytes, and for a linked file that means reading it. A frame that doesn't wait for
//! images ([`crate::RenderOptions::defer_images`]) never does this work on its own thread: it
//! queues it here ([`later`]), draws a stand-in, and draws again when
//! [`crate::images::generation`] advances.

use std::hash::{DefaultHasher, Hash, Hasher};

/// The outcome of work a graphic needs before it draws.
pub(crate) enum Prepared<T> {
    Ready(T),
    /// Queued ([`later`]); the frame is drawn again when it is done.
    Pending,
    /// It can't be done: the file is unreadable or can't be had.
    Failed,
}

/// Names one piece of work, so a request made each frame while it waits is queued once.
pub(crate) fn job_key(what: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    what.hash(&mut h);
    h.finish()
}

/// What [`later`] did with a job.
pub(crate) enum Queued {
    /// Queued, or already running: the frame counts as waiting for images.
    Yes,
    /// Done: it ran now (no background thread can run it), or it panicked before and isn't run
    /// again. What it made, if anything, is where it records it.
    Ran,
}

/// Run `job` on a background thread, at most once while it is queued or running under `key`.
/// The image generation advances when it is done.
pub(crate) fn later(key: u64, job: impl FnOnce() + Send + 'static) -> Queued {
    background::queue(key, Box::new(job))
}

#[cfg(not(target_arch = "wasm32"))]
mod background {
    use std::collections::VecDeque;
    use std::sync::{Condvar, Mutex, PoisonError};

    use super::Queued;

    type Job = Box<dyn FnOnce() + Send>;

    /// Threads: the work reads files, which may be slow, so one slow read doesn't hold up the rest.
    const THREADS: usize = 2;
    /// Jobs kept waiting; the oldest are dropped beyond this (asked for again if still in view).
    const MAX_QUEUED: usize = 64;

    /// Jobs remembered as having panicked; the oldest is forgotten beyond this.
    const MAX_PANICKED: usize = 64;

    struct Queue {
        jobs: VecDeque<(u64, Job)>,
        busy: Vec<u64>,
        threads: usize,
        /// Jobs that panicked: they record nothing, and run again they would panic every frame.
        panicked: VecDeque<u64>,
    }

    impl Queue {
        fn panicked(&mut self, key: u64) {
            self.panicked.push_back(key);
            while self.panicked.len() > MAX_PANICKED {
                self.panicked.pop_front();
            }
        }
    }

    static QUEUE: Mutex<Queue> = Mutex::new(Queue { jobs: VecDeque::new(), busy: Vec::new(), threads: 0, panicked: VecDeque::new() });
    static READY: Condvar = Condvar::new();

    fn lock() -> std::sync::MutexGuard<'static, Queue> {
        QUEUE.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn queue(key: u64, job: Job) -> Queued {
        let mut q = lock();
        if q.panicked.contains(&key) {
            return Queued::Ran;
        }
        if q.busy.contains(&key) || q.jobs.iter().any(|(k, _)| *k == key) {
            crate::images::defer();
            return Queued::Yes;
        }
        if q.threads < THREADS {
            if std::thread::Builder::new().name("designcraft-prepare".into()).spawn(run).is_ok() {
                q.threads += 1;
            } else if q.threads == 0 {
                drop(q);
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                    lock().panicked(key);
                }
                return Queued::Ran;
            }
        }
        q.jobs.push_back((key, job));
        while q.jobs.len() > MAX_QUEUED {
            q.jobs.pop_front();
        }
        READY.notify_one();
        crate::images::defer();
        Queued::Yes
    }

    fn next() -> (u64, Job) {
        let mut q = lock();
        loop {
            // Newest first: what was just scrolled into view is prepared before what was scrolled past.
            if let Some((key, job)) = q.jobs.pop_back() {
                q.busy.push(key);
                return (key, job);
            }
            q = READY.wait(q).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Takes a job off the busy list when dropped, however it ended.
    struct Busy(u64);

    impl Drop for Busy {
        fn drop(&mut self) {
            lock().busy.retain(|k| *k != self.0);
        }
    }

    /// Counts its thread out when dropped, so a new one is started for the next job.
    struct Alive;

    impl Drop for Alive {
        fn drop(&mut self) {
            let mut q = lock();
            q.threads = q.threads.saturating_sub(1);
        }
    }

    fn run() {
        let _alive = Alive;
        loop {
            let (key, job) = next();
            let busy = Busy(key);
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                lock().panicked(key);
            }
            drop(busy);
            crate::images::advance();
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod background {
    use super::Queued;

    /// No threads: run in place.
    pub(super) fn queue(_: u64, job: Box<dyn FnOnce() + Send>) -> Queued {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
        Queued::Ran
    }
}
