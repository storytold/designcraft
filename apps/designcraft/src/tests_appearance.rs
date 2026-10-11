//! Controlled watcher lifetime checks. Private-bus tests run with an isolated dbus-daemon.
use super::*;
use std::sync::mpsc;
#[cfg(unix)]
use std::sync::{Mutex, Weak};
use std::time::{Duration, Instant};

fn joined_drop(reader: Watcher) {
    let (done, wait) = mpsc::channel();
    let dropper = std::thread::spawn(move || {
        drop(reader);
        done.send(()).unwrap();
    });
    wait.recv_timeout(Duration::from_secs(1)).expect("owner drop must cancel and join promptly");
    dropper.join().unwrap();
}

#[test]
fn standard_numeric_and_nested_portal_values() {
    assert_eq!(decode(0), None);
    assert_eq!(decode(1), Some(egui::Theme::Dark));
    assert_eq!(decode(2), Some(egui::Theme::Light));
    assert_eq!(decode(u32::MAX), None);
    let nested = zbus::zvariant::Value::Value(Box::new(zbus::zvariant::Value::Value(Box::new(zbus::zvariant::Value::U32(2)))));
    assert_eq!(portal::code(zbus::zvariant::OwnedValue::try_from(nested).unwrap()), Some(2));
}

#[test]
fn owner_drop_cancels_pending_task_and_releases_shared_state() {
    struct Pending(Arc<AtomicBool>);
    impl Drop for Pending {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let marker = cancelled.clone();
    let (entered, wait) = mpsc::channel();
    let reader = Watcher::spawn(
        || panic!("pending task must not wake"),
        move |_shared| async move {
            let _pending = Pending(marker);
            entered.send(()).unwrap();
            futures_lite::future::pending::<()>().await;
        },
    )
    .unwrap();
    let weak = Arc::downgrade(&reader.shared);
    wait.recv_timeout(Duration::from_secs(1)).unwrap();
    joined_drop(reader);
    assert!(cancelled.load(Ordering::Acquire));
    assert!(weak.upgrade().is_none());
}

#[test]
fn worker_panic_does_not_escape_service_drop() {
    let (entered, wait) = mpsc::channel();
    let watcher = Watcher::spawn(
        || {},
        move |_| async move {
            entered.send(()).unwrap();
            panic!("synthetic worker panic");
        },
    )
    .unwrap();
    let weak = Arc::downgrade(&watcher.shared);
    wait.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(watcher.into_service());
    assert!(weak.upgrade().is_none());
}

#[test]
fn unavailable_initial_result_wakes_and_boxed_service_releases_context() {
    let ctx = egui::Context::default();
    // Consume the initial repaint requests before observing the real service wake.
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
    }
    let marker = Arc::new(());
    let context_weak = Arc::downgrade(&marker);
    let (wake, wait) = mpsc::channel();
    ctx.set_request_repaint_callback(move |_| {
        let _retain_context_marker = &marker;
        wake.send(()).unwrap();
    });
    // Address parsing fails before any connection/socket I/O, on every test host.
    let watcher = Watcher::start(&ctx, Some("invalid-address".into())).unwrap();
    let cache_weak = Arc::downgrade(&watcher.shared);
    wait.recv_timeout(Duration::from_secs(1)).expect("initial None must repaint even when the cache starts at None");
    assert!(watcher.shared.published.load(Ordering::Acquire));
    let service = watcher.into_service();
    assert_eq!(service(&ctx), None);
    drop(ctx);
    assert!(context_weak.upgrade().is_some());
    let start = Instant::now();
    drop(service);
    assert!(start.elapsed() < Duration::from_secs(1), "boxed service joins promptly after unavailable setup");
    assert!(cache_weak.upgrade().is_none());
    assert!(context_weak.upgrade().is_none());
    assert!(wait.try_recv().is_err(), "no repaint after service/context drop");
}

#[cfg(unix)]
#[test]
fn owner_drop_interrupts_connection_authentication() {
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    let scratch = tempfile::Builder::new().prefix("agent-work.").tempdir_in("/tmp").unwrap();
    let socket = scratch.path().join("auth");
    let listener = UnixListener::bind(&socket).unwrap();
    let (entered, wait) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut auth = [0; 256];
        assert!(socket.read(&mut auth).unwrap() > 0, "real authentication must begin");
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(3)).unwrap();
        // Keep the peer alive and send no authentication response until owner drop has returned.
        socket
    });
    let address = format!("unix:path={}", socket.display());
    let ctx = egui::Context::default();
    let reader = Watcher::start(&ctx, Some(address)).unwrap();
    let weak = Arc::downgrade(&reader.shared);
    wait.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    drop(reader.into_service());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(weak.upgrade().is_none());
    release.send(()).unwrap();
    let mut socket = peer.join().unwrap();
    // Authentication can arrive in several writes; drain buffered bytes before observing EOF.
    let mut bytes = [0; 256];
    while socket.read(&mut bytes).unwrap() != 0 {}
}

#[cfg(unix)]
mod bus {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::AtomicU32;
    use zbus::{
        message::Header,
        object_server::SignalEmitter,
        zvariant::{OwnedValue, Value},
    };

    struct Daemon(Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct Settings {
        scheme: Arc<AtomicU32>,
        entered: async_channel::Sender<String>,
        release: async_channel::Receiver<()>,
    }
    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl Settings {
        async fn read(&self, namespace: &str, key: &str, #[zbus(header)] header: Header<'_>) -> OwnedValue {
            assert_eq!((namespace, key), (portal::NAMESPACE, portal::KEY));
            let captured = self.scheme.load(Ordering::Acquire);
            self.entered.send(header.sender().unwrap().to_string()).await.unwrap();
            self.release.recv().await.unwrap();
            OwnedValue::try_from(Value::Value(Box::new(Value::U32(captured)))).unwrap()
        }
        #[zbus(signal)]
        async fn setting_changed(emitter: &SignalEmitter<'_>, namespace: &str, key: &str, value: Value<'_>) -> zbus::Result<()>;
    }

    async fn timed<T>(future: impl std::future::Future<Output = T>) -> T {
        futures_lite::future::or(future, async {
            async_io::Timer::after(Duration::from_secs(2)).await;
            panic!("private-bus phase exceeded two-second deadline");
        })
        .await
    }

    async fn fixture() -> (Daemon, String, zbus::Connection, Arc<AtomicU32>, async_channel::Receiver<String>, async_channel::Sender<()>) {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("isolated dbus-daemon prerequisite");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
        assert!(address.starts_with("unix:"));
        let address = address.trim().to_string();
        let daemon = Daemon(child);
        let scheme = Arc::new(AtomicU32::new(2));
        let (entered, observed) = async_channel::bounded(4);
        let (release, held) = async_channel::bounded(4);
        let connection = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .name(portal::DESTINATION)
            .unwrap()
            .serve_at(portal::PATH, Settings { scheme: scheme.clone(), entered, release: held })
            .unwrap()
            .build()
            .await
            .unwrap();
        (daemon, address, connection, scheme, observed, release)
    }

    fn reader(address: String) -> (Watcher, async_channel::Receiver<Option<egui::Theme>>, Weak<Shared>) {
        let weak_slot: Arc<Mutex<Weak<Shared>>> = Arc::new(Mutex::new(Weak::new()));
        let callback_slot = weak_slot.clone();
        let (wake, observed) = async_channel::unbounded();
        let reader = Watcher::spawn(
            move || {
                let shared = callback_slot.lock().unwrap().upgrade().expect("publication retains owner state");
                wake.try_send(shared.get()).unwrap();
            },
            move |shared| portal::watch(shared, Some(address)),
        )
        .unwrap();
        let weak = Arc::downgrade(&reader.shared);
        *weak_slot.lock().unwrap() = weak.clone();
        (reader, observed, weak)
    }

    async fn alive_and_released(server: &zbus::Connection, client: &str) {
        let bus = zbus::fdo::DBusProxy::new(server).await.unwrap();
        assert!(!bus.get_id().await.unwrap().is_empty(), "bus remains alive after owner drop");
        assert!(bus.name_has_owner(portal::DESTINATION.try_into().unwrap()).await.unwrap(), "portal remains alive");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if !bus.name_has_owner(client.try_into().unwrap()).await.unwrap() {
                break;
            }
            assert!(Instant::now() < deadline, "reader connection/subscription must be released");
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
    }

    #[test]
    #[ignore = "requires test-owned private dbus-daemon"]
    fn private_bus_startup_and_idle_owner_drop() {
        futures_lite::future::block_on(async {
            let (_daemon, address, server, scheme, entered, release) = fixture().await;
            let (reader, wakes, weak) = reader(address);
            let client = timed(entered.recv()).await.unwrap();
            // Initial Read holds a captured Light value. Signal Dark before releasing that reply.
            async_io::Timer::after(Duration::from_millis(300)).await;
            scheme.store(1, Ordering::Release);
            let emitter = SignalEmitter::new(&server, portal::PATH).unwrap();
            Settings::setting_changed(&emitter, portal::NAMESPACE, portal::KEY, Value::U32(1)).await.unwrap();
            release.send(()).await.unwrap();
            assert_eq!(timed(wakes.recv()).await.unwrap(), Some(egui::Theme::Light), "initial publication precedes wake");
            assert_eq!(timed(wakes.recv()).await.unwrap(), Some(egui::Theme::Dark), "startup signal is retained and publication precedes wake");
            assert_eq!(reader.get(), Some(egui::Theme::Dark));
            // Events continue updating the cache without any getter-triggered D-Bus call.
            Settings::setting_changed(&emitter, portal::NAMESPACE, portal::KEY, Value::U32(2)).await.unwrap();
            assert_eq!(timed(wakes.recv()).await.unwrap(), Some(egui::Theme::Light));
            assert_eq!(reader.get(), Some(egui::Theme::Light));
            // Let the worker return to its idle receive. No later signal or reply releases it.
            async_io::Timer::after(Duration::from_millis(50)).await;
            joined_drop(reader);
            assert!(weak.upgrade().is_none(), "joined worker releases context/cache owner state");
            alive_and_released(&server, &client).await;
            assert!(wakes.try_recv().is_err(), "no final signal and no post-drop repaint");
            println!(
                "PASS real private bus: delayed initial Light wake, queued Dark, idle owner drop/join, shared state and client released, portal/bus alive, no final signal"
            );
        });
    }

    #[test]
    #[ignore = "requires test-owned private dbus-daemon"]
    fn private_bus_owner_drop_interrupts_held_read() {
        futures_lite::future::block_on(async {
            let (_daemon, address, server, _scheme, entered, release) = fixture().await;
            let (reader, wakes, weak) = reader(address);
            let client = timed(entered.recv()).await.unwrap();
            let start = Instant::now();
            drop(reader.into_service());
            assert!(start.elapsed() < Duration::from_secs(1), "cancel Read before its two-second timeout");
            assert!(weak.upgrade().is_none());
            alive_and_released(&server, &client).await;
            assert!(wakes.try_recv().is_err(), "unreleased Read cannot publish after owner drop");
            // The held portal remains alive until after all assertions; release is only cleanup.
            release.send(()).await.unwrap();
            println!("PASS real held Read cancelled/joined without reply, portal/bus alive and client released");
        });
    }

    #[test]
    #[ignore = "requires test-owned private dbus-daemon"]
    fn private_bus_boxed_service_take_replace_and_app_drop_release_context() {
        use designcraft_ui_egui::{DesignApp, Services};
        futures_lite::future::block_on(async {
            let (_daemon, address, server, _scheme, entered, release) = fixture().await;
            for boundary in ["closure", "take", "replace", "app"] {
                let ctx = egui::Context::default();
                let context_marker = Arc::new(());
                let context_weak = Arc::downgrade(&context_marker);
                let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let observed_wakes = wakes.clone();
                ctx.set_request_repaint_callback(move |_| {
                    let _keep_context_marker = &context_marker;
                    observed_wakes.fetch_add(1, Ordering::AcqRel);
                });
                let watcher = Watcher::start(&ctx, Some(address.clone())).unwrap();
                let cache_weak = Arc::downgrade(&watcher.shared);
                let service = watcher.into_service();
                let client = timed(entered.recv()).await.unwrap();
                release.send(()).await.unwrap();
                let deadline = Instant::now() + Duration::from_secs(2);
                while service(&ctx) != Some(egui::Theme::Light) {
                    assert!(Instant::now() < deadline, "real service must publish initial palette");
                    async_io::Timer::after(Duration::from_millis(10)).await;
                }
                drop(ctx);
                assert!(context_weak.upgrade().is_some(), "watcher owns the only remaining Context");
                let start = Instant::now();
                match boundary {
                    "closure" => drop(service),
                    "take" => {
                        let mut services = Services { system_theme: Some(service), ..Default::default() };
                        drop(services.system_theme.take());
                    }
                    "replace" => {
                        let mut services = Services { system_theme: Some(service), ..Default::default() };
                        services.system_theme = Some(Box::new(|_| None));
                        assert_eq!(services.system_theme.as_ref().unwrap()(&egui::Context::default()), None);
                    }
                    "app" => {
                        let services = Services { system_theme: Some(service), ..Default::default() };
                        drop(DesignApp::new(designcraft_engine::Session::new(), services));
                    }
                    _ => panic!("unexpected test service boundary {boundary}"),
                }
                assert!(start.elapsed() < Duration::from_secs(1), "{boundary} must cancel and join promptly");
                assert!(cache_weak.upgrade().is_none(), "{boundary} releases the cache and joined task");
                assert!(context_weak.upgrade().is_none(), "{boundary} releases the actual egui Context callback");
                let count_after_drop = wakes.load(Ordering::Acquire);
                alive_and_released(&server, &client).await;
                async_io::Timer::after(Duration::from_millis(50)).await;
                assert_eq!(wakes.load(Ordering::Acquire), count_after_drop, "no repaint after joined drop");
                println!(
                    "PASS actual boxed service {boundary}: joined worker, cache/Context/client released while portal/bus alive, no final signal"
                );
            }
        });
    }
}
