# Traditional Chinese (Taiwan) draft

Choose Preferences > General > Interface Language > 繁體中文. The canonical preference is `zh-TW`; the lookup also accepts `zh-Hant`. System Hant fonts lead the UI fallback order. User documents and command identifiers stay unchanged.

Traditional Chinese uses 13 px body/button text and 12 px small text so dense strokes remain readable. Switching the interface language reapplies these text styles; English keeps its existing sizes. Native rendering and clipping still need visual review at the user's Windows display scale.

The editable language pack is `crates/ui-egui/src/i18n/zh_tw.rs` (2271 entries). It is an AI-assisted draft, partly converted from this project's Simplified Chinese column with OpenCC s2twp and corrected for Taiwan terms. Native review is still pending. Missing captions and diagnostics fall back to English; this is not a claim of complete localization or an official release.

For updates, retain existing translations, add new English keys, keep placeholders exact, then run `cargo test -p designcraft-ui-egui i18n`, `cargo xtask i18n-report` and `cargo xtask ci`. No Python or OpenCC is required to build or run DesignCraft.
