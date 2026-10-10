//! The Linux desktop colour scheme for Appearance Mode › Auto: winit reports none on many desktops
//! (and none through XWayland, which DesignCraft uses by default).

#[cfg(target_os = "linux")]
const PORTAL_NAMESPACE: &str = "org.freedesktop.appearance";
#[cfg(target_os = "linux")]
const PORTAL_KEY: &str = "color-scheme";

#[cfg(target_os = "linux")]
fn portal_proxy() -> Option<zbus::blocking::Proxy<'static>> {
    let conn = zbus::blocking::connection::Builder::session().ok()?.method_timeout(std::time::Duration::from_secs(2)).build().ok()?;
    zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings").ok()
}

#[cfg(target_os = "linux")]
fn portal_theme(proxy: &zbus::blocking::Proxy<'_>) -> Option<egui::Theme> {
    let value: zbus::zvariant::OwnedValue = proxy.call("Read", &(PORTAL_NAMESPACE, PORTAL_KEY)).ok()?;
    decode(portal_code(value)?)
}

#[cfg(target_os = "linux")]
fn portal_code(value: zbus::zvariant::OwnedValue) -> Option<u32> {
    let mut value: zbus::zvariant::Value<'_> = value.into();
    // The Settings.Read result is a variant; some portals nest another variant inside it.
    for _ in 0..4 {
        match value {
            zbus::zvariant::Value::Value(inner) => value = *inner,
            other => return u32::try_from(other).ok(),
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn decode(code: u32) -> Option<egui::Theme> {
    match code {
        1 => Some(egui::Theme::Dark),
        2 => Some(egui::Theme::Light),
        _ => None,
    }
}

/// One-shot fallback for desktops without the settings portal. It runs once at start-up, by
/// absolute path, and is never repeated.
#[cfg(target_os = "linux")]
fn gtk_theme() -> Option<egui::Theme> {
    let exe = ["/usr/bin/gsettings", "/bin/gsettings", "/usr/local/bin/gsettings"].into_iter().find(|p| std::path::Path::new(p).is_file())?;
    let output = std::process::Command::new(exe).args(["get", "org.gnome.desktop.interface", "color-scheme"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_gtk_scheme(std::str::from_utf8(&output.stdout).ok()?)
}

#[cfg(target_os = "linux")]
fn parse_gtk_scheme(s: &str) -> Option<egui::Theme> {
    match s.trim().trim_matches('\'') {
        "prefer-dark" => Some(egui::Theme::Dark),
        "prefer-light" => Some(egui::Theme::Light),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn code_of(theme: Option<egui::Theme>) -> u8 {
    match theme {
        Some(egui::Theme::Dark) => 1,
        Some(egui::Theme::Light) => 2,
        None => 0,
    }
}

/// Reads the colour scheme once, then follows the portal's `SettingChanged` signal. There is no
/// polling: the worker sleeps in the signal iterator and wakes the UI only when the value changes.
pub fn service() -> Option<designcraft_ui_egui::SystemThemeFn> {
    #[cfg(target_os = "linux")]
    {
        use std::sync::{
            Arc, OnceLock,
            atomic::{AtomicU8, Ordering},
        };
        let value = Arc::new(AtomicU8::new(0));
        let wake: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
        let (worker_value, worker_wake) = (Arc::clone(&value), Arc::clone(&wake));
        let (ready, wait) = std::sync::mpsc::channel();
        if std::thread::Builder::new()
            .name("appearance-portal".into())
            .spawn(move || {
                let proxy = portal_proxy();
                // Subscribe before reading so a change during startup stays queued.
                let signals = proxy.as_ref().and_then(|p| p.receive_signal("SettingChanged").ok());
                let initial = proxy.as_ref().and_then(portal_theme).or_else(gtk_theme);
                // Startup can finish after the UI's bounded wait: wake its first frame too.
                if worker_value.swap(code_of(initial), Ordering::Relaxed) != code_of(initial)
                    && let Some(ctx) = worker_wake.get()
                {
                    ctx.request_repaint();
                }
                let _ = ready.send(());
                let Some(signals) = signals else { return };
                for message in signals {
                    let Ok((namespace, key, changed)) = message.body().deserialize::<(String, String, zbus::zvariant::OwnedValue)>() else {
                        continue;
                    };
                    if namespace != PORTAL_NAMESPACE || key != PORTAL_KEY {
                        continue;
                    }
                    let code = code_of(portal_code(changed).and_then(decode));
                    if worker_value.swap(code, Ordering::Relaxed) != code
                        && let Some(ctx) = worker_wake.get()
                    {
                        ctx.request_repaint();
                    }
                }
            })
            .is_err()
        {
            return None;
        }
        let _ = wait.recv_timeout(std::time::Duration::from_millis(250));
        Some(Box::new(move |ctx| {
            let _ = wake.set(ctx.clone());
            decode(u32::from(value.load(Ordering::Relaxed)))
        }))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn portal_scheme_values() {
        assert_eq!(super::decode(0), None);
        assert_eq!(super::decode(1), Some(egui::Theme::Dark));
        assert_eq!(super::decode(2), Some(egui::Theme::Light));
        assert_eq!(super::decode(9), None);
        let nested = zbus::zvariant::Value::Value(Box::new(zbus::zvariant::Value::Value(Box::new(zbus::zvariant::Value::U32(2)))));
        assert_eq!(super::portal_code(zbus::zvariant::OwnedValue::try_from(nested).unwrap()), Some(2));
        assert_eq!(super::parse_gtk_scheme("'prefer-light'\n"), Some(egui::Theme::Light));
        assert_eq!(super::parse_gtk_scheme("'prefer-dark'\n"), Some(egui::Theme::Dark));
        assert_eq!(super::parse_gtk_scheme("'default'\n"), None);
    }
}
