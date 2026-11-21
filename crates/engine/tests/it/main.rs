#[cfg(not(target_arch = "wasm32"))]
mod book_safety;
#[cfg(target_arch = "wasm32")]
mod browser_print;
mod character_style_from_selection;
mod close_retention;
#[cfg(not(target_arch = "wasm32"))]
mod collection_state_atomicity;
mod combined_close_glyph;
mod distribute_locks;
mod export_page_input;
mod idml_bitmap_transparency;
mod insert_pages_bounds;
mod insert_pages_parent;
mod param_keys;
mod pdf_backend;
mod preflight_resolution;
mod primary_frame_columns;
#[cfg(not(target_arch = "wasm32"))]
mod save_state_atomicity;
mod stroke_tint_range;
mod text_decorations;
mod text_insert_memory;
mod text_thread_regressions;
mod thread_color_safety;
