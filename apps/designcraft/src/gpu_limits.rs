//! What the graphics device may do with a window that outgrows it, on every native platform.
//!
//! The window's surface is a texture, so its sides can't exceed the device's largest texture side,
//! and eframe asks for exactly 8192 whatever the GPU can do. A window across monitors that add up
//! to more (3440 + 3840 + 1080 pixels side by side), or one a mixed-DPI drag on Windows scaled up
//! (9048×5757 between 250 % and 100 % monitors), then failed `Surface::configure`, and wgpu's
//! default error handler panics: the app closed with the documents in it. DirectX 12, Vulkan and
//! Metal hardware offer 16384.
//!
//! The same goes for buffers: egui-wgpu doubles its vertex buffer when a frame outgrows it, and
//! wgpu's default largest buffer is 256 MiB. A frame of a 160-page catalogue with 137 MB of vertices
//! asked for a 275 MB buffer, and egui-wgpu panicked when the device refused it. DirectX 12 and
//! Vulkan hardware offer far more.

use eframe::{
    NativeOptions,
    egui_wgpu::WgpuSetup,
    wgpu::{Device, Limits},
};
use std::sync::{Arc, Mutex, PoisonError};

/// `required` with the largest texture side and buffer the adapter's `supported` limits offer.
/// Taking the adapter's own values also never asks a smaller (downlevel) adapter for more than it
/// has.
pub fn adapter_size_limits(mut required: Limits, supported: &Limits) -> Limits {
    required.max_texture_dimension_2d = supported.max_texture_dimension_2d;
    required.max_buffer_size = supported.max_buffer_size;
    required
}

/// Makes the device eframe creates ask for the adapter's largest texture side and buffer
/// ([`adapter_size_limits`]), keeping eframe's other device requirements.
pub fn request_adapter_sizes(options: &mut NativeOptions) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        let device_descriptor = Arc::clone(&create.device_descriptor);
        create.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = device_descriptor(adapter);
            descriptor.required_limits = adapter_size_limits(descriptor.required_limits, &adapter.limits());
            descriptor
        });
    }
}

/// Logs wgpu errors no error scope caught instead of panicking. A window larger than even the
/// adapter's texture side then keeps its last surface (wgpu validates a configuration before
/// replacing the old one) and draws wrongly until it shrinks, instead of closing the app.
pub fn log_uncaptured_errors(device: &Device) {
    // A surface that can't be configured fails again on every frame: log each message once in a row.
    let last = Mutex::new(String::new());
    device.on_uncaptured_error(Arc::new(move |error| {
        let message = error.to_string();
        let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
        if *last != message {
            log::error!("graphics: {message}");
            *last = message;
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_texture_size_is_the_adapters_own() {
        // A mixed-DPI drag's 9048×5757 needs more than eframe's 8192; a downlevel adapter must not
        // be asked for more than it has.
        for base in [Limits::default(), Limits::downlevel_webgl2_defaults()] {
            let eframe_request = Limits { max_texture_dimension_2d: 8192, ..base.clone() };
            for side in [4096, 8192, 16384, 32768] {
                let supported = Limits { max_texture_dimension_2d: side, ..base.clone() };
                let limits = adapter_size_limits(eframe_request.clone(), &supported);
                assert_eq!(limits.max_texture_dimension_2d, side);
                // The drag's 9048×5757 surface fits from 16384 up.
                assert_eq!(side >= 16384, 9048 <= limits.max_texture_dimension_2d);
                // Everything else eframe asks for is unchanged.
                assert_eq!(Limits { max_texture_dimension_2d: 8192, ..limits.clone() }, eframe_request);
                assert!(limits.check_limits(&supported));
            }
        }
    }

    #[test]
    fn largest_buffer_is_the_adapters_own() {
        // egui-wgpu grew its vertex buffer to 275 MB for 137 MB of vertices: more than wgpu's
        // default 256 MiB, within what DirectX 12 hardware offers. A downlevel adapter offering
        // less is asked for no more.
        let eframe_request = Limits::default();
        for size in [128 << 20, 256 << 20, 2 << 30] {
            let supported = Limits { max_buffer_size: size, ..Limits::default() };
            let limits = adapter_size_limits(eframe_request.clone(), &supported);
            assert_eq!(limits.max_buffer_size, size);
            assert_eq!(size >= 2 << 30, limits.max_buffer_size >= 275_114_400);
            assert!(limits.check_limits(&supported));
        }
    }

    #[test]
    fn native_options_keep_creating_their_own_device() {
        // The wrapper only replaces the device descriptor: eframe still creates the instance,
        // adapter and device, and main.rs can still choose the backends.
        let mut options = NativeOptions::default();
        request_adapter_sizes(&mut options);
        assert!(matches!(options.wgpu_options.wgpu_setup, WgpuSetup::CreateNew(_)));
    }
}
