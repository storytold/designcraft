# Variable-font paragraph style regressions (#1210)

Default tests generate a small variable-font name/axis table around the existing bundled OFL outlines. They exercise cold catalog discovery in both the Family menu and Style popup, selection, and retry on reopening a popup after a font file was unavailable. No download or system-font installation is required. Run them with `cargo test -p designcraft-ui-egui --lib synthetic_variable`.

The real-font tests remain explicit opt-in tests: missing fonts fail rather than silently pass.
They use SIL OFL 1.1 Bricolage Grotesque from google/fonts commit
`47d54ca1ebb14fd864695f1d39dc734f39017c33`, path
`ofl/bricolagegrotesque/BricolageGrotesque[opsz,wdth,wght].ttf`.
SHA256: `413e7357809ddd12fd80a96a8a396de0e401638d4acd3cb3e37532f0472ac682`.
Download OFL.txt from the same directory; keep both outside this repository.

Set `DESIGNCRAFT_TEST_VARIABLE_FONT` to the absolute font file path. Then run:

```text
cargo test -p designcraft-ui-egui cold_family_menu_contains_named_instances -- --ignored --nocapture
cargo test -p designcraft-ui-egui named_instance_style_widget_and_api_agree -- --ignored --nocapture
```

On Windows, for the cold Style-combo test, make a temporary directory containing
`Microsoft/Windows/Fonts/` with that font inside. Set `LOCALAPPDATA` **only for the test
process** to this temporary directory, then run the following test alone in a fresh process:

```text
cargo test -p designcraft-ui-egui cold_style_combo_contains_named_instances -- --ignored --nocapture
```

Do not install the fixture globally or preload it through add_font for this cold test.
On other platforms use the platform's test font directory before the process starts.
The test asserts that SemiBold is absent before loading, without depending on the default style name.
It then opens the actual Style combo, selects SemiBold and verifies c.fontStyle.
The menu-cache test creates an isolated database whose font directory holds the fixture.
The warm widget test clicks Regular -> SemiBold -> Bold -> Regular, compares Confirm with
style.paragraph.edit, preserves an inherited family and Based On, serializes/reopens the
native document, and verifies Preview off, Cancel and single-step Confirm/Undo/Redo.

Before the patch, both cold tests fail: the menu caches only `96pt ExtraBold`, and the
Style combo offers no SemiBold. After preloading, selection and persistence already work.
This proves a discovery/availability failure, not a loss of a selected style in the engine.
No font binaries should be committed. A desktop visual/manual check on macOS and Linux
is separate from these headless egui input tests.
