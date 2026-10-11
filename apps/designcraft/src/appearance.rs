//! Linux desktop colour scheme for System: winit reports none on many desktops
//! (and none through XWayland, which DesignCraft uses by default).
//!
//! Numeric/nested portal decoding and subscribe-before-Read ordering are adapted
//! from @luniiya's MIT OR Apache-2.0 implementation in DesignCraft PR #271,
//! head 4620d2747edb7d87b5427b945d845489fce5393b.

#[cfg(any(target_os = "linux", test))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

#[cfg(any(target_os = "linux", test))]
struct Shared {
    value: AtomicU8,
    published: AtomicBool,
    closed: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}

#[cfg(any(target_os = "linux", test))]
impl Shared {
    fn get(&self) -> Option<egui::Theme> {
        decode(u32::from(self.value.load(Ordering::Acquire)))
    }

    fn publish(&self, theme: Option<egui::Theme>) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let code = match theme {
            Some(egui::Theme::Dark) => 1,
            Some(egui::Theme::Light) => 2,
            None => 0,
        };
        // Publish before requesting a frame, including a late initial unavailable result.
        let changed = self.value.swap(code, Ordering::AcqRel) != code;
        let first = !self.published.swap(true, Ordering::AcqRel);
        if (first || changed) && !self.closed.load(Ordering::Acquire) {
            (self.wake)();
        }
    }
}

/// The existing boxed service owns this guard. Cached reads never perform I/O.
#[cfg(any(target_os = "linux", test))]
struct Watcher {
    shared: Arc<Shared>,
    stop: async_channel::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(any(target_os = "linux", test))]
impl Watcher {
    fn start(ctx: &egui::Context, address: Option<String>) -> Option<Self> {
        let ctx = ctx.clone();
        Self::spawn(move || ctx.request_repaint(), move |shared| portal::watch(shared, address))
    }

    fn spawn<F, T>(wake: impl Fn() + Send + Sync + 'static, task: T) -> Option<Self>
    where
        F: std::future::Future<Output = ()> + 'static,
        T: FnOnce(Arc<Shared>) -> F + Send + 'static,
    {
        let shared =
            Arc::new(Shared { value: AtomicU8::new(0), published: AtomicBool::new(false), closed: AtomicBool::new(false), wake: Box::new(wake) });
        let worker_shared = Arc::clone(&shared);
        let (stop, stopped) = async_channel::bounded::<()>(1);
        let worker = std::thread::Builder::new()
            .name("appearance-portal".into())
            .spawn(move || {
                // Cancel the whole task, including connection/authentication, subscription,
                // initial Read and idle receive. A flag around a blocking iterator cannot do this.
                futures_lite::future::block_on(futures_lite::future::or(
                    async {
                        let _ = stopped.recv().await;
                    },
                    task(worker_shared),
                ));
            })
            .ok()?;
        Some(Self { shared, stop, worker: Some(worker) })
    }

    fn get(&self) -> Option<egui::Theme> {
        self.shared.get()
    }

    fn into_service(self) -> designcraft_ui_egui::SystemThemeFn {
        // Calling through the guard keeps its cancellation/join ownership in the closure.
        Box::new(move |_| self.get())
    }
}

#[cfg(any(target_os = "linux", test))]
impl Drop for Watcher {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        self.stop.close();
        if let Some(worker) = self.worker.take() {
            // A worker panic must not propagate into the window or its unsaved document.
            let _ = worker.join();
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn decode(code: u32) -> Option<egui::Theme> {
    match code {
        1 => Some(egui::Theme::Dark),
        2 => Some(egui::Theme::Light),
        _ => None,
    }
}

pub fn service(ctx: &egui::Context) -> Option<designcraft_ui_egui::SystemThemeFn> {
    #[cfg(target_os = "linux")]
    {
        Some(Watcher::start(ctx, None)?.into_service())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ctx;
        None
    }
}

#[cfg(any(target_os = "linux", test))]
mod portal {
    use super::{Shared, decode};
    use futures_lite::StreamExt;
    use std::sync::Arc;

    pub(super) const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    pub(super) const PATH: &str = "/org/freedesktop/portal/desktop";
    pub(super) const INTERFACE: &str = "org.freedesktop.portal.Settings";
    pub(super) const NAMESPACE: &str = "org.freedesktop.appearance";
    pub(super) const KEY: &str = "color-scheme";

    pub(super) fn code(value: zbus::zvariant::OwnedValue) -> Option<u32> {
        let mut value: zbus::zvariant::Value<'_> = value.into();
        // Settings.Read uses variants; some portals nest another variant inside it.
        for _ in 0..4 {
            match value {
                zbus::zvariant::Value::Value(inner) => value = *inner,
                other => return u32::try_from(other).ok(),
            }
        }
        None
    }

    pub(super) async fn watch(shared: Arc<Shared>, address: Option<String>) {
        if watch_inner(&shared, address).await.is_err() {
            shared.publish(None);
        }
    }

    async fn watch_inner(shared: &Shared, address: Option<String>) -> zbus::Result<()> {
        let builder = match address.as_deref() {
            Some(address) => zbus::connection::Builder::address(address)?,
            None => zbus::connection::Builder::session()?,
        };
        let connection = builder.method_timeout(std::time::Duration::from_secs(2)).build().await?;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(DESTINATION)?
            .path(PATH)?
            .interface(INTERFACE)?
            .member("SettingChanged")?
            .arg(0, NAMESPACE)?
            .arg(1, KEY)?
            .build();
        // Subscribe before Read so transitions during startup remain queued.
        let mut signals = zbus::MessageStream::for_match_rule(rule, &connection, Some(64)).await?;
        let reply = connection.call_method(Some(DESTINATION), PATH, Some(INTERFACE), "Read", &(NAMESPACE, KEY)).await;
        let initial = reply.ok().and_then(|m| m.body().deserialize::<zbus::zvariant::OwnedValue>().ok()).and_then(code).and_then(decode);
        shared.publish(initial);
        while let Some(message) = signals.next().await {
            let message = message?;
            let Ok((namespace, key, changed)) = message.body().deserialize::<(String, String, zbus::zvariant::OwnedValue)>() else { continue };
            if namespace == NAMESPACE && key == KEY {
                shared.publish(code(changed).and_then(decode));
            }
        }
        shared.publish(None);
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests_appearance.rs"]
mod tests;
