#[cfg(not(target_arch = "wasm32"))]
mod sysfonts;

#[cfg(not(target_arch = "wasm32"))]
mod fallback;

#[cfg(not(target_arch = "wasm32"))]
mod docfonts;

mod vertical;
