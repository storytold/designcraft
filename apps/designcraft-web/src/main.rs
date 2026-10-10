//! DesignCraft in the browser.
//!
//! Runs the same [`designcraft_ui_egui::DesignApp`] as the desktop app through eframe's web
//! runner (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release`
//! from this directory (output in `dist/web`).
//!
//! Differences from the desktop app:
//! - no TCP control channel (browsers can't listen on sockets);
//! - File → Open / Place use the browser file picker; bytes arrive asynchronously through
//!   `Services::inbox` (`.designcraft` → `file.openBytes`, images → `file.place`);
//! - Save and Export trigger a browser download;
//! - dropped files are read asynchronously by `web::WebShell` and delivered through the inbox.
//!
//! URL query flags: `?webgl` forces the WebGL2 backend instead of WebGPU; `?sample` opens the
//! sample magazine on start.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web;

// Pure helpers: compiled for the browser and for the host's unit tests.
#[cfg(any(target_arch = "wasm32", test))]
mod mime;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("designcraft-web only runs in the browser: build it with `trunk build --release` in apps/designcraft-web");
}
