//! Printing beyond the system spooler: network printers (WS-Print and IPP) that the person adds by
//! address, on `craft-print`'s registry. The module is `network`; it is not built for `wasm32`
//! (there is no socket to open there). Nothing connects until a printer is added and a job is
//! confirmed.
//!
//! This crate is the non-UI half of DesignCraft's print glue: the egui half
//! (`designcraft-ui-egui`'s `network_glue`) maps `craft_ui::printing::network`'s light types onto it.
//! The shape (a `Printer` that asks what a printer is, submits and follows a job, plus a
//! `DocumentConverter`) is the one `pdfcraft-print::network` has, so the generic part can be shared.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(not(target_arch = "wasm32"))]
pub mod network;
