use super::*;
use std::collections::HashSet;

/// Every per-language table `tr` looks up, by interface language.
const LANGUAGE_TABLES: &[(&str, &[(&str, &str)])] = &[
    ("ar", ar::TABLE),
    ("he", he::TABLE),
    ("cs", cs::TABLE),
    ("de", de::TABLE),
    ("fr", fr::TABLE),
    ("hu", hu::TABLE),
    ("it", it::TABLE),
    ("ja", ja::TABLE),
    ("ka", ka::TABLE),
    ("pl", pl::TABLE),
    ("pt-br", pt_br::TABLE),
    ("ru", ru::TABLE),
    ("sv", sv::TABLE),
    ("uk", uk::TABLE),
];

/// Every per-language table `tr_context` looks up.
const CONTEXT_TABLES: &[(&str, &[(&str, &str, &str)])] = &[
    ("ar", ar::CONTEXT),
    ("cs", cs::CONTEXT),
    ("hu", hu::CONTEXT),
    ("ja", ja::CONTEXT),
    ("pl", pl::CONTEXT),
    ("ru", ru::CONTEXT),
    ("sv", sv::CONTEXT),
    ("uk", uk::CONTEXT),
];

/// The languages of the shared tables' columns, in column order.
const SHARED_COLUMNS: [&str; 5] = ["de", "fr", "es", "ja", "zh"];

/// The German and Japanese tables translate the whole interface and win over the shared columns.
const WHOLE_INTERFACE: [&str; 2] = ["de", "ja"];

/// What `tr` shows for a row: an empty one is still untranslated.
fn or_english<'a>(text: &'a str, english: &'a str) -> &'a str {
    if text.trim().is_empty() { english } else { text }
}

/// Checks cached catalog lookup across language switches, including source-text fallback.
#[test]
fn cached_lookup_preserves_every_translation_across_language_switches() {
    let own = |lang: &str, key: &str| {
        LANGUAGE_TABLES.iter().find(|(code, _)| *code == lang).and_then(|(_, table)| table.iter().find(|(k, _)| *k == key)).map(|(_, text)| *text)
    };
    for (key, translations) in TABLE {
        for (lang, shared) in SHARED_COLUMNS.into_iter().zip(translations) {
            // A language's own table is looked up before the shared column.
            let expected = own(lang, key).filter(|text| !text.trim().is_empty()).unwrap_or(*shared);
            assert_eq!(tr(lang, key), or_english(expected, key), "{lang}: {key}");
        }
        assert_eq!(tr("", key), *key);
    }
    for (lang, table) in LANGUAGE_TABLES {
        for (key, expected) in *table {
            if !expected.trim().is_empty() {
                assert_eq!(tr(lang, key), *expected, "{lang}: {key}");
            }
            assert_eq!(tr("", key), *key);
        }
    }
    // Strings no catalog has stay as they are, in every language.
    let unknown = String::from("A user-defined untranslated label");
    for lang in LANGUAGES.iter().map(|(code, _)| *code).chain(["en", "unknown"]) {
        assert!(std::ptr::eq(tr(lang, &unknown), unknown.as_str()), "{lang}");
        // Fixed standard names stay identical.
        assert_eq!(tr(lang, "PDF/X-4"), "PDF/X-4", "{lang}");
    }
}

/// Checks Hungarian catalog integrity, context captions and count substitutions, including
/// English fallback and preservation of user-defined style and workspace names.
#[test]
fn hungarian_catalog_preserves_placeholders_context_and_counts() {
    let catalog: HashMap<_, _> = hu::TABLE.iter().copied().collect();
    assert_eq!(catalog.len(), hu::TABLE.len(), "duplicate Hungarian keys");
    let placeholders = |s: &str| {
        let mut names: Vec<_> = s.split('{').skip(1).filter_map(|part| part.split_once('}').map(|(name, _)| name.to_owned())).collect();
        names.sort();
        names
    };
    for (key, translation) in hu::TABLE {
        assert!(!translation.trim().is_empty(), "empty {key}");
        assert_eq!(placeholders(key), placeholders(translation), "placeholders: {key}");
        assert_eq!(key.ends_with('…'), translation.ends_with('…'), "ellipsis: {key}");
    }
    assert!(!is_rtl("hu"));
    assert_eq!(tr("hu", "File"), "Fájl");
    assert_eq!(tr("hu", "Bleed"), "Kifutó");
    assert_eq!(tr("hu", "Arrangement"), "Elrendezés");
    assert_eq!(tr("hu", "Unknown label"), "Unknown label");
    assert_eq!(tr("hu", "Columns"), "Hasábok");
    assert_eq!(tr_context("hu", "Columns", "table"), "Oszlopok");
    assert_eq!(tr("hu", "Columns:"), "Hasábok:");
    assert_eq!(tr_context("hu", "Columns:", "grid"), "Oszlopok:");
    assert_eq!(tr_context("hu", "Columns:", "unknown"), "Hasábok:");
    assert_eq!(tr("hu", "Group"), "Csoportosítás");
    assert_eq!(tr_context("hu", "Group", "selection"), "Csoport");
    assert_eq!(tr_context("hu", "Columns", "unknown"), "Hasábok");
    assert_eq!(tr_context("", "Columns", "table"), "Columns");
    for n in [0, 1, 2, 5, 21] {
        assert_eq!(count_label("hu", "pages", n), format!("{n} oldal"));
        assert_eq!(count_label("hu", "objects", n), format!("{n} objektum"));
        assert_eq!(count_label("hu", "errors", n), format!("{n} hiba"));
        assert_eq!(
            tr("hu", "{pages} Pages in {spreads} Spreads").replace("{pages}", &n.to_string()).replace("{spreads}", &n.to_string()),
            format!("{n} oldal, {n} oldalpáron")
        );
    }
    assert_eq!(style_name("hu", "[Basic Paragraph]"), "[Alapbekezdés]");
    assert_eq!(style_name("hu", "Saját stílus"), "Saját stílus");
    assert_eq!(workspace_name("hu", "Essentials"), "Alapok");
    assert_eq!(workspace_name("hu", "Saját munkaterület"), "Saját munkaterület");
}

/// Checks every Latin catalog letter and all Hungarian accented vowels against the
/// configured bundled UI faces, with system and optional craft-fonts fallbacks disabled.
#[test]
fn hungarian_catalog_glyphs_exist_in_the_bundled_ui_fonts() {
    let definitions = crate::theme::font_definitions(&[], "hu");
    let db = designcraft_fonts::FontDb::with_font_dirs(Vec::new());
    db.set_system_fallback(false);
    let letters: std::collections::BTreeSet<_> = hu::TABLE
        .iter()
        .flat_map(|(_, text)| text.chars())
        .chain(hu::CONTEXT.iter().flat_map(|(_, _, text)| text.chars()))
        .chain("ÁáÉéÍíÓóÖöŐőÚúÜüŰű".chars())
        .filter(|c| c.is_alphabetic() && *c <= '\u{024f}')
        .collect();
    for (name, family, style, ui_family) in [
        ("ui", "Source Sans 3", "Regular", egui::FontFamily::Proportional),
        ("ui-semibold", "Source Sans 3", "Semibold", egui::FontFamily::Name("semibold".into())),
        ("mono", "JetBrains Mono", "Regular", egui::FontFamily::Monospace),
    ] {
        assert_eq!(definitions.families[&ui_family].first().map(String::as_str), Some(name));
        let face = db.face(family, style);
        assert_eq!(face.data(), definitions.font_data[name].bytes(), "configured UI face {name}");
        for letter in &letters {
            assert_ne!(face.glyph_for(*letter), 0, "missing {letter} in {family} {style}");
        }
    }
    eprintln!(
        "Hungarian glyph coverage: {} Latin catalog letters, including ŐőŰű, in Source Sans 3 Regular/Semibold and JetBrains Mono Regular; no system/craft fallback",
        letters.len()
    );
    if let Some(dir) = std::env::var_os("DESIGNCRAFT_HU_REVIEW_SHOTS") {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(820.0, 260.0)).build_ui_state(
            |ui, ready: &mut bool| {
                if !*ready {
                    ui.ctx().set_fonts(crate::theme::font_definitions(&[], "hu"));
                    *ready = true;
                    return;
                }
                ui.style_mut().visuals = egui::Visuals::light();
                ui.painter().rect_filled(ui.ctx().content_rect(), 0.0, egui::Color32::WHITE);
                ui.heading("Magyar ékezetek · ő, ű / Ő, Ű");
                for (name, family) in [
                    ("Source Sans 3 Regular", egui::FontFamily::Proportional),
                    ("Source Sans 3 Semibold", egui::FontFamily::Name("semibold".into())),
                    ("JetBrains Mono Regular", egui::FontFamily::Monospace),
                ] {
                    ui.label(egui::RichText::new(name).color(egui::Color32::DARK_GRAY));
                    ui.label(
                        egui::RichText::new("Árvíztűrő tükörfúrógép · Őő Űű · ÁÉÍÓÖŐÚÜŰ")
                            .font(egui::FontId::new(22.0, family))
                            .color(egui::Color32::BLACK),
                    );
                }
            },
            false,
        );
        h.run_steps(3);
        h.render().unwrap().save(std::path::Path::new(&dir).join("hungarian-bundled-ui-fonts.png")).unwrap();
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "hu".into();
        app.run("file.new", serde_json::json!({"pages": 1})).unwrap();
        app.run(
            "frame.create",
            serde_json::json!({"rect": [60, 60, 420, 180], "content": "text", "text": "Magyar kiadvány
Árvíztűrő tükörfúrógép
Őő Űű · ÁÉÍÓÖŐÚÜŰ"}),
        )
        .unwrap();
        app.run("type.char", serde_json::json!({"attrs": {"fontFamily": "Source Sans 3", "size": 28}})).unwrap();
        app.run("window.panel.activate", serde_json::json!({"panel": "character"})).unwrap();
        let mut interface = crate::test_window::open(app, egui::vec2(1400.0, 900.0));
        interface.render().unwrap().save(std::path::Path::new(&dir).join("designcraft-hungarian-interface.png")).unwrap();
    }
}

/// Checks translated labels and source-text fallback, plus key uniqueness and nonempty rows
/// in the shared, Arabic, Brazilian Portuguese and Italian catalogs.
#[test]
fn translates_known_strings_and_keeps_the_rest() {
    assert_eq!(tr("de", "File"), "Datei");
    assert_eq!(tr("ja", "Swatches"), "スウォッチ");
    assert_eq!(tr("fr", "Something new"), "Something new");
    assert_eq!(tr("", "File"), "File");
    assert_eq!(tr("ar", "File"), "ملف");
    assert_eq!(tr("ar", "Unknown label"), "Unknown label");
    assert_eq!(tr("pt-br", "File"), "Arquivo");
    assert_eq!(tr("pt-br", "New Document…"), "Novo documento…");
    assert_eq!(tr("pt-br", "Unknown label"), "Unknown label");
    assert_eq!(tr("it", "Edit"), "Modifica");
    assert_eq!(tr("it", "New Document…"), "Nuovo documento…");
    assert_eq!(tr("it", "Unknown label"), "Unknown label");
    assert!(is_rtl("ar"));
    for (i, (en, translation)) in ar::TABLE.iter().enumerate() {
        assert!(ar::TABLE[..i].iter().all(|(key, _)| key != en), "duplicate {en}");
        assert!(!translation.is_empty(), "{en}");
    }
    for (i, (en, translation)) in pt_br::TABLE.iter().enumerate() {
        assert!(pt_br::TABLE[..i].iter().all(|(key, _)| key != en), "duplicate {en}");
        assert!(!translation.is_empty(), "{en}");
    }
    for (i, (en, translation)) in it::TABLE.iter().enumerate() {
        assert!(it::TABLE[..i].iter().all(|(key, _)| key != en), "duplicate {en}");
        assert!(!translation.is_empty(), "{en}");
    }
    // Italian entries are for keys the shared or Arabic table knows. A new English string
    // falls back to English until it is translated, so it doesn't fail here.
    for (en, _) in it::TABLE {
        assert!(TABLE.iter().any(|(key, _)| key == en) || ar::TABLE.iter().any(|(key, _)| key == en), "it: unknown key {en}");
    }
    // Every row is unique and complete.
    for (i, (en, t)) in TABLE.iter().enumerate() {
        assert!(TABLE[..i].iter().all(|(e, _)| e != en), "duplicate {en}");
        assert!(t.iter().all(|x| !x.is_empty()), "{en}");
    }
}

/// The Data Merge arrangement combo takes its accessible name from `tr(lang, "Arrangement")`.
/// A row in only the shared table leaves the languages whose lookup returns directly with the
/// English name.
#[test]
fn arrangement_accessible_name_is_localized_in_every_interface_language() {
    for (code, name) in LANGUAGES {
        let label = tr(code, "Arrangement");
        if code.is_empty() {
            assert_eq!(label, "Arrangement", "{name}");
        } else {
            assert_ne!(label, "Arrangement", "{name} still announces the English accessible name");
            assert!(!label.trim().is_empty(), "{name}");
        }
    }
}

/// File › Place Contact Sheet… and its dialog read in every interface language, including those
/// whose own catalog is all `tr` looks in.
#[test]
fn contact_sheet_strings_are_translated_in_every_interface_language() {
    let strings = [
        "Place Contact Sheet…",
        "Place Contact Sheet",
        "Place loaded images on new pages, in file order.",
        "Columns",
        "Rows",
        "Gutter",
        "Page margin",
    ];
    for (code, name) in LANGUAGES.iter().filter(|(code, _)| !code.is_empty()) {
        for s in strings {
            let text = tr(code, s);
            assert!(text != s && !text.trim().is_empty(), "{name}: {s:?} is still English");
        }
    }
    assert_eq!(["Columns", "Rows", "Gutter"].map(|s| tr("es", s)), ["Columnas", "Filas", "Medianil"]);
}

/// The Links panel's captions for a status its background check hasn't found yet, and for when
/// the check can't run.
#[test]
fn links_panel_check_captions_are_localized_in_every_interface_language() {
    for (code, name) in LANGUAGES {
        for key in ["Checking", "Link status is unavailable."] {
            let label = tr(code, key);
            if code.is_empty() {
                assert_eq!(label, key, "{name}");
            } else {
                assert_ne!(label, key, "{name} still shows the English caption");
                assert!(!label.trim().is_empty(), "{name}: {key}");
            }
        }
    }
}

/// Checks language-specific context precedence and fallback to the plain translation.
#[test]
fn duplicate_layer_menu_item_is_localized_in_every_interface_language() {
    let key = "Duplicate Layer \"{name}\"";
    for (code, name) in LANGUAGES {
        let label = tr(code, key);
        if code.is_empty() {
            assert_eq!(label, key, "{name}");
        } else {
            assert_ne!(label, key, "{name} shows the Layers panel's Duplicate Layer item in English");
            assert!(label.contains("{name}"), "{name}");
        }
    }
}

#[test]
fn context_captions_come_from_their_context_or_the_plain_translation() {
    for (lang, table) in CONTEXT_TABLES {
        for (key, scope, text) in *table {
            if !text.trim().is_empty() {
                assert_eq!(tr_context(lang, key, scope), *text, "{lang}: {key} ({scope})");
            }
        }
    }
    for (key, scope, row) in CONTEXT {
        for (lang, shared) in SHARED_COLUMNS.into_iter().zip(row) {
            // A language's own context table is looked up before the shared one.
            let own = CONTEXT_TABLES
                .iter()
                .filter(|(code, _)| *code == lang)
                .flat_map(|(_, table)| table.iter())
                .find(|(k, s, _)| k == key && s == scope)
                .map(|(_, _, text)| *text)
                .filter(|text| !text.trim().is_empty());
            let expected = own.or(Some(*shared).filter(|text| !text.trim().is_empty())).unwrap_or_else(|| tr(lang, key));
            assert_eq!(tr_context(lang, key, scope), expected, "{lang}: {key} ({scope})");
        }
    }
    // An unknown context, or a language without one, keeps the plain translation.
    let keys = CONTEXT.iter().map(|(key, _, _)| *key).chain(CONTEXT_TABLES.iter().flat_map(|(_, table)| table.iter().map(|(key, _, _)| *key)));
    for key in keys {
        for lang in LANGUAGES.iter().map(|(code, _)| *code).chain(["en"]) {
            assert_eq!(tr_context(lang, key, "no such context"), tr(lang, key), "{lang}: {key}");
        }
        assert_eq!(tr_context("", key, "count"), key);
    }
}

/// Keeps quantity labels distinct from numeric values and checks translated Pages-panel captions.
#[test]
fn a_column_count_and_a_list_number_read_differently() {
    for (lang, count, numeral) in [
        ("hu", "Darabszám:", "Szám:"),
        ("de", "Anzahl:", "Zahl:"),
        ("ja", "数:", "番号:"),
        ("uk", "Кількість:", "Номер:"),
        ("ar", "العدد:", "الرقم:"),
        ("fr", "Nombre :", "Numéro:"),
        ("zh", "数量：", "编号："),
    ] {
        assert_eq!((tr_context(lang, "Number:", "count"), tr(lang, "Number:")), (count, numeral), "{lang}");
    }
    assert_eq!(tr_context("en", "Number:", "count"), "Number:");
    assert_eq!(tr("de", "Gutter:"), "Spaltenabstand:");
    // The Pages panel menu.
    for key in [
        "Insert Pages…",
        "Move Pages…",
        "New Parent…",
        "Override All Parent Page Items",
        "Remove All Local Overrides",
        "Detach All Objects from Parent",
        "Create Alternate Layout…",
        "Allow Selected Spread to Shuffle",
    ] {
        assert_ne!(tr("pt-br", key), key);
    }
    assert_ne!(tr("ar", "Apply Parent to Pages"), "Apply Parent to Pages");
}

/// The tables `tr` reads, as (language, key, translation), context captions included.
fn entries() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut entries: Vec<_> = LANGUAGE_TABLES.iter().flat_map(|(lang, table)| table.iter().map(|(key, text)| (*lang, *key, *text))).collect();
    for (lang, i) in SHARED_COLUMNS.into_iter().zip(0..) {
        entries.extend(TABLE.iter().filter_map(|(key, row)| row.get(i).map(|text| (lang, *key, *text))));
        entries.extend(CONTEXT.iter().filter_map(|(key, _, row)| row.get(i).map(|text| (lang, *key, *text))));
    }
    entries.extend(CONTEXT_TABLES.iter().flat_map(|(lang, table)| table.iter().map(|(key, _, text)| (*lang, *key, *text))));
    entries
}

/// Every catalog has one row per key and keeps the English placeholders. A string a catalog
/// doesn't translate shows in English, so a missing or empty row fails nothing here; the
/// `missing_translations_report` lists them.
#[test]
fn every_catalog_has_unique_keys_and_keeps_placeholders() {
    for (lang, i) in SHARED_COLUMNS.iter().zip(0..) {
        assert_eq!(column(lang), Some(i), "{lang}");
    }
    for (lang, table) in LANGUAGE_TABLES {
        let keys: HashSet<_> = table.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys.len(), table.len(), "{lang}: duplicate keys");
    }
    let shared: HashSet<_> = TABLE.iter().map(|(key, _)| *key).collect();
    assert_eq!(shared.len(), TABLE.len(), "duplicate keys in the shared table");
    let shared_contexts: HashSet<_> = CONTEXT.iter().map(|(key, scope, _)| (*key, *scope)).collect();
    assert_eq!(shared_contexts.len(), CONTEXT.len(), "duplicate keys in the shared context table");
    for (lang, table) in CONTEXT_TABLES {
        let keys: HashSet<_> = table.iter().map(|(key, scope, _)| (*key, *scope)).collect();
        assert_eq!(keys.len(), table.len(), "{lang}: duplicate context keys");
    }
    // A per-language table is looked up before the shared table, so a key in both hides the
    // shared column, except in the tables that translate the whole interface.
    for (lang, table) in LANGUAGE_TABLES.iter().filter(|(lang, _)| column(lang).is_some() && !WHOLE_INTERFACE.contains(lang)) {
        let both: Vec<_> = table.iter().map(|(key, _)| *key).filter(|key| shared.contains(key)).collect();
        assert!(both.is_empty(), "{lang} keys also in the shared table: {both:?}");
    }

    let placeholders = |s: &'static str| {
        let mut names: Vec<_> = s.split('{').skip(1).filter_map(|part| part.split_once('}').map(|(name, _)| name)).collect();
        names.sort_unstable();
        names
    };
    for (lang, key, text) in entries().into_iter().filter(|(_, _, text)| !text.trim().is_empty()) {
        assert_eq!(placeholders(key), placeholders(text), "{lang}: placeholders of {key:?}");
    }
}

/// The strings each interface language shows in English, for the `cargo xtask ci` log. It
/// always passes: `cargo xtask i18n-report` runs it.
#[test]
#[ignore = "a report, run by `cargo xtask i18n-report`"]
fn missing_translations_report() {
    let interface = interface_strings();
    let catalogs: Vec<(&str, HashSet<&str>)> = LANGUAGE_TABLES
        .iter()
        .map(|(lang, table)| (*lang, table.iter().map(|(key, _)| *key).collect()))
        .chain([("shared", TABLE.iter().map(|(key, _)| *key).collect())])
        .collect();
    let mut keys: Vec<&str> = interface.iter().copied().chain(catalogs.iter().flat_map(|(_, keys)| keys.iter().copied())).collect();
    keys.sort_unstable();
    keys.dedup();
    // `tr` hands back the string it was given when it has no translation.
    let missing = |lang: &str| -> Vec<&str> {
        keys.iter()
            .copied()
            .filter(|key| {
                let english = (*key).to_owned();
                std::ptr::eq(tr(lang, &english), english.as_str())
            })
            .collect()
    };
    let report: Vec<(&str, Vec<&str>)> = LANGUAGES.iter().filter(|(code, _)| !code.is_empty()).map(|(code, _)| (*code, missing(code))).collect();
    println!("i18n report: {} interface strings; a missing translation shows in English.", keys.len());
    for (lang, missing) in &report {
        println!("  {lang:<6} {:>5} missing", missing.len());
    }
    for (lang, missing) in report.iter().filter(|(_, missing)| !missing.is_empty()) {
        println!("{lang} ({}): {}", missing.len(), missing.iter().map(|key| format!("{key:?}")).collect::<Vec<_>>().join(", "));
    }
    // A catalog key neither the interface list nor another catalog knows is stale, or a label
    // `interface_strings` doesn't list yet.
    for (name, own) in &catalogs {
        let mut unknown: Vec<_> = own
            .iter()
            .filter(|key| !interface.contains(*key) && !catalogs.iter().any(|(other, keys)| other != name && keys.contains(*key)))
            .collect();
        unknown.sort_unstable();
        if !unknown.is_empty() {
            println!("{name}: {} rows for keys the interface list doesn't have: {unknown:?}", unknown.len());
        }
    }
}

#[test]
fn arabic_and_hebrew_lay_out_right_to_left() {
    for (code, _) in LANGUAGES {
        assert_eq!(is_rtl(code), matches!(*code, "ar" | "he"), "{code}");
    }
}

/// Built-in style and workspace names follow the interface language; names people give their
/// own styles and workspaces are document data and stay as typed.
#[test]
fn only_built_in_style_and_workspace_names_are_translated() {
    for (code, _) in LANGUAGES {
        for name in [designcraft_doc::NO_PARA_STYLE, designcraft_doc::BASIC_PARAGRAPH, designcraft_doc::NO_CHAR_STYLE] {
            assert_eq!(style_name(code, name), tr(code, name), "{code}");
        }
        assert_eq!(workspace_name(code, "Essentials"), tr(code, "Essentials"), "{code}");
        for custom in ["Group", "Columns", "My layout style"] {
            assert_eq!(style_name(code, custom), custom, "{code}");
            assert_eq!(workspace_name(code, custom), custom, "{code}");
        }
    }
}

/// Languages with several plural forms put the category first, so every number reads the same.
#[test]
fn category_first_counts_put_the_category_before_every_number() {
    assert!(!category_first("de") && !category_first(""));
    for lang in ["ka", "uk", "ru", "pl"] {
        assert!(category_first(lang), "{lang}");
        for noun in ["pages", "objects", "errors"] {
            for n in [0, 1, 2, 4, 5, 11, 12, 21, 22, 25, 101, 111] {
                assert_eq!(count_label(lang, noun, n), format!("{}: {n}", tr(lang, noun)), "{lang}: {noun}");
            }
        }
    }
    assert_eq!(count_label("", "objects", 2), "2 objects");
}

/// Every English string the interface passes to `tr`, as far as the command and tool tables and
/// the list of UI-only labels know them.
fn interface_strings() -> HashSet<&'static str> {
    let mut keys = HashSet::new();
    // Command labels and menu names reach `tr` from the engine and the UI command table.
    for spec in designcraft_engine::command_specs() {
        keys.insert(spec.label);
        keys.extend(spec.menu.iter().copied());
    }
    keys.extend(crate::menus::UI_COMMANDS.iter().map(|(_, label, _, _)| *label));
    // Tool names reach `tr` from the Tools panel.
    keys.extend(designcraft_tools::TOOL_GROUPS.iter().flat_map(|group| group.iter()).map(|tool| tool.label));
    // So do the About window's tab names, app descriptions and the credits' column and sort labels.
    keys.extend(crate::about::ABOUT_TABS);
    keys.extend(crate::about::SIBLINGS.iter().map(|(_, _, what)| *what));
    keys.extend(crate::credits::NameMode::ALL.iter().map(|mode| mode.label()));
    keys.extend(crate::credits::SortKey::ALL.iter().flat_map(|key| [key.label().0, key.label().1]));
    // Docking chrome labels reach `tr` through craft-ui's localizer.
    keys.extend(crate::panel_docking::DOCK_LABELS.iter().map(|(_, english)| *english));
    // File › New's Units list passes each unit's name to `tr`.
    keys.extend(crate::new_document::NEW_DOCUMENT_UNITS.map(|unit| unit.label()));
    // UI labels passed to `tr` that no other catalog has yet.
    keys.extend([
        "Auto-Sizing",
        "Color Theme",
        "Imported {count} shortcuts from “{set}”; {unmatched} items not matched, {unreadable} unreadable.",
        "Minimum Height",
        "Minimum Width",
        "No Line Breaks",
        "Reference Point",
        "This document uses unavailable fonts. Choose replacements, or select Done to continue with fallback fonts.",
        "180°",
        "Create Links When Placing Text Files",
        "Delete Character Style…",
        "Delete Paragraph Style…",
        "Delete the character style \"{name}\"?",
        "Delete the paragraph style \"{name}\"?",
        "IT",
        "Replace with:",
        "Show the Typography panel",
        "Stroke Behind Fill",
        "Stylistic Set",
        "The selected fonts have no OpenType features.",
        "This story was edited in the layout: you'll be asked before those edits are replaced",
        "This style is in use. Text using it will be reassigned to:",
        "VA",
        "Y:",
        "“{}” has been edited in this layout. Updating replaces those edits with the text from the file.",
        "(any)",
        "(unchanged)",
        "[No Language]",
        "Cell Number:",
        "Differs from this document",
        "Digits:",
        "Change fill to:",
        "Chinese: Simplified",
        "Chinese: Traditional",
        "Don't Save",
        "Email Address:",
        "Email:",
        "Find fill:",
        "H:",
        "Message:",
        "Organization:",
        "Phone:",
        "W:",
        "Subject:",
        "Text:",
        "Unsaved Changes",
        "“{}” has changes that are not saved. Save them before closing?",
        "% of all commits",
        "{count} conflict(s): {names}",
        "{count} match(es)",
        "{count} replacement(s) made",
        "{contributors} contributors · {commits} commits",
        "{label}: {name} (double-click: Color Picker)",
        "All",
        "Align To",
        "Arrangement",
        "Auto Rename",
        "Character Style",
        "Company",
        "DesignCraft page",
        "Equal fixed heights for the selected rows; cell text may become overset.",
        "Found",
        "Grab bag",
        "Include Inside Edges",
        "Key Object",
        "Load Styles",
        "layer",
        "layers",
        "Lines +/−",
        "New Cell Style",
        "New Table Style",
        "No contributor data was built into this copy.",
        "No matches",
        "No model credits were built into this copy.",
        "Object Style",
        "Paragraph Style",
        "PDF/X-1a:2003",
        "Press keys…",
        "Range:",
        "Redefine Document Style",
        "Report an issue",
        "Reverse the order",
        "Sort Entries in Alphabetical Order",
        "Story {story} ← story {parent}",
        "Story Editor — {count} words",
        "Text Anchor",
        "Transparency Flattener",
        "Use Document Style Definition",
        // Status-bar messages.
        "Alternate layout",
        "Couldn't write recovery data: {error}",
        "Export a PDF, IDML or package to share — no cloud account needed.",
        "Import cancelled: the originating document has closed",
        "Invalid panel layout; restored the default dock",
        "PDF import cancelled: the originating document has closed",
        "Packaged {count} files into {dir}",
        "Relinked {count}",
        "Sent {count} page(s) to {printer}",
        "the default printer",
        "{name}: import cancelled while a dialog is open; choose the file again",
        "{name}: {count} step(s) done",
    ]);
    // The Keyboard Shortcuts dialog's strings (craft-keymap) reach `tr` too.
    keys.extend(crate::shortcuts_dialog::strings());
    keys
}

#[test]
fn czech_counts_use_the_plural_form_for_the_number() {
    for (noun, [one, few, other]) in cs::PLURALS {
        for (n, form) in [(0, other), (1, one), (2, few), (4, few), (5, other), (11, other), (22, other)] {
            let expected = if form.trim().is_empty() { format!("{n} {}", tr("cs", noun)) } else { format!("{n} {form}") };
            assert_eq!(count_label("cs", noun, n), expected, "{noun}");
        }
    }
    // A noun without plural forms keeps the plain translation.
    assert_eq!(count_label("cs", "a new noun", 3), "3 a new noun");
}

/// Hebrew is an RTL *interface* option, without changing document language or user-defined data.
/// This also locks the initial catalog to English labels in the application's own source.
#[test]
fn hebrew_catalog_is_selectable_rtl_and_preserves_english_keys() {
    let mut seen = HashSet::new();
    assert_eq!(he::TABLE.len(), 1534, "Hebrew source-linked table changed");
    for (source, translated) in he::TABLE {
        assert!(seen.insert(*source), "duplicate Hebrew source key: {source}");
        assert!(!translated.trim().is_empty(), "empty Hebrew: {source}");
        // Arabic is a separate, incomplete locale, not an English source-key inventory.
        // Valid native UI labels may be localized in Hebrew before they appear in Arabic.
        let fields = |text: &str| {
            let mut fields: Vec<String> =
                text.split('{').skip(1).filter_map(|part| part.split_once('}').map(|(field, _)| field.to_owned())).collect();
            fields.sort_unstable();
            fields
        };
        assert_eq!(fields(source), fields(translated), "placeholders: {source}");
        assert_eq!(source.ends_with('…'), translated.ends_with('…'), "ellipsis: {source}");
        assert_eq!(tr("he", source), *translated, "Hebrew lookup: {source}");
        assert!(translated.chars().any(|c| ('\u{05d0}'..='\u{05ea}').contains(&c)), "no Hebrew letters in {source}");
    }
    // These native UI labels are English-source-linked, even though Arabic lacks entries.
    // A locale table must not be treated as the canonical inventory of another locale.
    for (source, expected) in [
        ("Activate Panel", "הפעלת חלונית"),
        ("Don't Save", "לא לשמור"),
        ("Relinked {count}", "קושרו מחדש: {count}"),
        ("About", "אודות"),
        ("Quick Apply", "החלה מהירה"),
        ("Press keys…", "הקש על מקשים…"),
        ("Stop Preview", "עצור תצוגה מקדימה"),
        ("Reload Fonts", "טעינת גופנים מחדש"),
    ] {
        assert_eq!(tr("he", source), expected, "native Hebrew UI label: {source}");
    }
    assert!(LANGUAGES.iter().any(|(code, native)| *code == "he" && *native == "עברית"));
    assert!(picker_languages().iter().any(|(code, native)| *code == "he" && *native == "עברית"));
    assert!(is_rtl("he"), "Hebrew menus and panels must use the existing RTL UI layout");
    assert!(is_rtl("ar"), "Arabic RTL support must remain unchanged");
    assert!(!is_rtl("en"), "English UI must remain LTR");
    assert_eq!(tr("he", "File"), "קובץ");
    assert_eq!(tr("he", "Save As…"), "שמירה בשם…");
    assert_eq!(tr("he", "Arrangement"), "סידור");
    assert_eq!(tr("he", "Font Size"), "גודל גופן");
    assert_eq!(tr("he", "Hyphenation…"), "חלוקת מילים…");
    assert_eq!(tr("he", "Panel Options…"), "אפשרויות חלונית…");
    assert_eq!(tr("he", "Create Flex Layout"), "יצירת פריסת Flex");
    assert_eq!(tr("he", "Go to Page…"), "מעבר לעמוד…");
    assert_eq!(tr("he", "Right to Left"), "מימין לשמאל");
    assert_eq!(tr("he", "Fit Page in Window"), "התאמת עמוד לחלון");
    assert_eq!(tr("he", "Primary Text Frame"), "מסגרת טקסט ראשית");
    assert_eq!(tr("he", "No document open"), "אין מסמך פתוח");
    assert_eq!(tr("he", "Merge Cells"), "מיזוג תאים");
    assert_eq!(tr("he", "Export IDML…"), "ייצוא כ־IDML…");
    assert_eq!(tr("he", "Preflight"), "בדיקה לפני הפקה");
    assert_eq!(tr("he", "Missing Glyphs"), "גליפים חסרים");
    assert_eq!(tr("he", "Save Recovery Data"), "שמירת נתוני שחזור");
    assert_eq!(tr("he", "Apply Table Style"), "החלת סגנון טבלה");
    assert_eq!(tr("he", "Tagged PDF"), "PDF מתויג");
    assert_eq!(tr("he", "Insert Footnote"), "הוספת הערת שוליים");
    assert_eq!(tr("he", "No errors"), "לא נמצאו שגיאות");
    assert_eq!(tr("he", "Hyphenate"), "חלוקת מילים באמצעות מקפים");
    assert_eq!(tr("he", "Edit/Spelling"), "עריכה/איות");
    assert_eq!(tr("he", "Object/Clipping Path"), "אובייקט/נתיב חיתוך");
    assert_eq!(tr("he", "Object/Transform Again"), "אובייקט/חזרה על שינוי הצורה");
    assert_eq!(tr("he", "Type/Insert Special Character/Symbols"), "טקסט/הוספת תו מיוחד/סמלים");
    assert_eq!(tr("he", "Type/Hyperlinks & Cross-References"), "טקסט/היפר־קישורים והפניות צולבות");
    assert_eq!(tr("he", "Type/Track Changes"), "טקסט/מעקב אחר שינויים");
    assert_eq!(tr("he", "View/Grids & Guides"), "תצוגה/רשתות וקווי עזר");
    assert_eq!(tr("he", "Window/Type & Tables"), "חלון/טקסט וטבלאות");
    assert_eq!(tr("he", "Type on a Path"), "טקסט על נתיב");
    assert_eq!(tr("he", "Document Footnote Options..."), "אפשרויות הערות שוליים במסמך...");
    assert_eq!(tr("he", "Object States"), "מצבי אובייקט");
    assert_eq!(tr("he", "Color Blindness – Protanopia"), "עיוורון צבעים – פרוטנופיה");
    assert_eq!(tr("he", "Smart Text Reflow"), "זרימת טקסט חכמה");
    assert_eq!(tr("he", "Baseline Grid"), "רשת קווי בסיס");
    assert_eq!(tr("he", "Frame Fitting"), "התאמת מסגרת");
    assert_eq!(tr("he", "Smart Guides"), "קווי עזר חכמים");
    assert_eq!(tr("he", "Apply Parent to Pages…"), "החלת עמוד אב על עמודים…");
    assert_eq!(tr("he", "Convert Text to Table"), "המרת טקסט לטבלה");
    assert_eq!(tr("he", "Margins and Columns"), "שוליים ועמודות");
    assert_eq!(tr("he", "This swatch can't be edited."), "לא ניתן לערוך את דוגמית הצבע הזו.");
    assert_eq!(tr("he", "Layer Options for \"{name}\"…"), "אפשרויות השכבה ״{name}״…");
    assert_eq!(tr("he", "No text selected"), "לא נבחר טקסט");
    assert_eq!(tr("he", "Delete Hyperlink"), "מחיקת היפר־קישור");
    assert_eq!(tr("he", "New Book…"), "ספר חדש…");
    assert_eq!(tr("he", "No data source"), "אין מקור נתונים");
    assert_eq!(tr("he", "Check Spelling"), "בדיקת איות");
    assert_eq!(tr("he", "Insert Row Below"), "הוספת שורה מתחת");
    assert_eq!(tr("he", "Paragraph Shading"), "הצללת פסקה");
    assert_eq!(tr("he", "New Hyperlink…"), "היפר־קישור חדש…");
    assert_eq!(tr("he", "Table Panel"), "חלונית טבלה");
    assert_eq!(tr("he", "Delete Row"), "מחיקת שורה");
    assert_eq!(tr("he", "Insert Column"), "הוספת עמודה");
    assert_eq!(tr("he", "Show Baseline Grid"), "הצגת רשת קווי הבסיס");
    assert_eq!(tr("he", "Select Table"), "בחירת טבלה");
    assert_eq!(tr("he", "Create Alternate Layout…"), "יצירת פריסה חלופית…");
    assert_eq!(tr("he", "Relink…"), "קישור מחדש…");
    assert_eq!(tr("he", "Page (1–{count}):"), "עמוד (1–{count}):");
    assert_eq!(tr("he", "A user-defined untranslated label"), "A user-defined untranslated label");
}

#[test]
fn hebrew_parallel_round4_document_translations() {
    for (source, expected) in [
        ("Book", "ספר"),
        ("Recent files", "קבצים אחרונים"),
        (
            "Number of Recent Items to Display:",
            "מספר הפריטים האחרונים להצגה:",
        ),
        ("Place", "הצבה"),
        ("Place and Link", "הצבה וקישור"),
        ("Place with Import Options…", "הצבה עם אפשרויות ייבוא…"),
        ("Update Link", "עדכון קישור"),
        ("Records per page", "רשומות בעמוד"),
        (
            "The library is empty. Select objects and Add Selection.",
            "הספרייה ריקה. בחרו אובייקטים ולחצו על ״הוספת הבחירה״.",
        ),
        (
            "A PNG or JPEG file; left empty, the first page is the cover.",
            "קובץ PNG או JPEG; אם משאירים ריק, העמוד הראשון ישמש ככריכה.",
        ),
    ] {
        assert_eq!(
            tr("he", source),
            expected,
            "Hebrew localization regression: {source}"
        );
    }
}

#[test]
fn hebrew_parallel_round4_typography_translations() {
    for (source, expected) in [
        ("Apply Superscript", "החלת כתב עילי"),
        ("Superscript", "כתב עילי"),
        ("Superscript:", "כתב עילי:"),
        ("List Type:", "סוג רשימה:"),
        (
            "Minimum Space Before First Footnote:",
            "מרווח מזערי לפני הערת השוליים הראשונה:",
        ),
        ("Text After:", "טקסט אחרי:"),
        ("Underline", "קו תחתון"),
        ("Underline On", "הפעלת קו תחתון"),
        ("Underline Options", "אפשרויות קו תחתון"),
        (
            "Hyperlink from the selected text or frames",
            "היפר־קישור מהטקסט או מהמסגרות שנבחרו",
        ),
    ] {
        assert_eq!(
            tr("he", source),
            expected,
            "Hebrew localization regression: {source}"
        );
    }
}

#[test]
fn hebrew_parallel_round4_appearance_translations() {
    for (source, expected) in [
        ("Drop Shadow", "הטלת צל"),
        ("Effects: Drop Shadow", "אפקטים: הטלת צל"),
        ("Inner Shadow", "צל פנימי"),
        ("Alternating Row Fills", "מילוי שורות לסירוגין"),
        ("Align Left", "יישור לשמאל"),
        ("Align Center", "יישור למרכז"),
        ("Align Right", "יישור לימין"),
        ("Swap Fill and Stroke", "החלפת מילוי וקו מתאר"),
        ("Toggle Fill and Stroke", "החלפה בין מילוי לקו מתאר"),
        ("Default Fill and Stroke", "מילוי וקו מתאר כברירת מחדל"),
    ] {
        assert_eq!(
            tr("he", source),
            expected,
            "Hebrew localization regression: {source}"
        );
    }
}

#[test]
fn hebrew_grokbot_context_reviewed_live_ui_labels() {
    for (source, expected) in [
        ("Desired", "רצוי"),
        ("forcedLineBreak", "מעבר שורה כפוי"),
        ("emSpace", "רווח Em"),
        ("enSpace", "רווח En"),
        ("dropcap", "אות פתיח"),
    ] {
        assert!(ar::TABLE.iter().any(|(english, _)| english == source), "missing source key: {source}");
        assert_eq!(tr("he", source), expected, "new Hebrew UI label: {source}");
    }
    assert_eq!(tr("en", "Desired"), "Desired", "English UI must remain unchanged");
}

#[test]
fn hebrew_core_dialog_controls_are_translated() {
    for (source, expected) in [
        ("Direction", "כיוון"),
        ("Horizontal", "אופקי"),
        ("Vertical", "אנכי"),
        ("On", "מופעל"),
        ("Off", "כבוי"),
        ("None", "ללא"),
        ("[None]", "[ללא]"),
        ("Quick Actions", "פעולות מהירות"),
        ("More Options", "אפשרויות נוספות"),
        ("Dialog", "תיבת דו־שיח"),
        ("General", "כללי"),
        ("Justification", "יישור לשני הצדדים"),
    ] {
        assert_eq!(tr("he", source), expected, "Hebrew native UI control: {source}");
    }
}
#[test]
fn hebrew_source_linked_native_tool_and_window_labels() {
    for (source, expected) in [
        ("Display name", "שם לתצוגה"),
        ("Maximize/Restore Window", "הגדל/שחזר חלון"),
        ("Menus", "תפריטים"),
        ("Minimize Window", "מזער חלון"),
        ("Move Tab", "העבר כרטיסייה"),
        ("Page Options", "אפשרויות עמוד"),
        ("Polygon Frame Tool", "כלי מסגרת מצולעת"),
        ("Polygon Tool", "כלי מצולע"),
        ("Reorder Objects", "סדר אובייקטים מחדש"),
        ("Report an Issue", "דווח על בעיה"),
        ("Resize Window", "שנה את גודל החלון"),
        ("Rotate Tool", "כלי סיבוב"),
        ("Scissors Tool", "כלי מספריים"),
        ("Scripts", "סקריפטים"),
        ("Select Page", "בחר עמוד"),
        ("Selection Tool", "כלי בחירה"),
        ("Shear Tool", "כלי הטיה"),
        ("Smooth Tool", "כלי החלקה"),
        ("Type Tool", "כלי טקסט"),
        ("Type on a Path Tool", "כלי טקסט על נתיב"),
        ("Unassigned", "לא משויך"),
        ("Unsaved Changes", "שינויים שלא נשמרו"),
        ("Username", "שם משתמש"),
        ("Versions", "גרסאות"),
        ("Vertical Type Tool", "כלי טקסט אנכי"),
        ("Zoom Tool", "כלי הגדלה"),
    ] {
        assert!(
            fr::TABLE.iter().any(|(english, _)| english == source),
            "official English source key missing: {source}"
        );
        assert_eq!(
            tr("he", source),
            expected,
            "Hebrew native UI label regression: {source}"
        );
    }
}

#[test]
fn hebrew_source_linked_data_merge_styles_and_export_labels() {
    for (source, expected) in [
        ("Convert Rows", "המרת שורות"),
        ("Data Fields", "שדות נתונים"),
        ("Define Lists", "הגדרת רשימות"),
        ("Figure Space", "רווח ברוחב ספרה"),
        ("Generate Live Caption", "יצירת כיתוב דינמי"),
        ("Import InDesign Shortcut Set", "ייבוא ערכת קיצורי דרך של InDesign"),
        ("Insert Field", "הוספת שדה"),
        ("List Styles in File", "הצגת סגנונות בקובץ"),
        ("None (keep transparency)", "ללא (שמירה על שקיפות)"),
        ("Place with Import Options", "הצבה עם אפשרויות ייבוא"),
        ("Preview Change", "תצוגה מקדימה של שינוי"),
        ("Preview Record", "תצוגה מקדימה של רשומה"),
        ("Rectangle Frame Tool", "כלי מסגרת מלבנית"),
        ("Redefine Character Style", "הגדרה מחדש של סגנון תו"),
        ("Redefine Paragraph Style", "הגדרה מחדש של סגנון פסקה"),
        ("Remove Data Source", "הסרת מקור נתונים"),
        ("Remove Field", "הסרת שדה"),
        ("Resize Row or Column", "שינוי גודל שורה או עמודה"),
        ("Reset Bar Position", "איפוס מיקום הסרגל"),
        ("Files to export: {n}", "קבצים לייצוא: {n}"),
        ("Put each version in its own folder", "שמירת כל גרסה בתיקייה נפרדת"),
        ("Message:", "הודעה:"),
        ("Range:", "טווח:"),
        ("Organization:", "ארגון:"),
    ] {
        assert!(
            fr::TABLE.iter().any(|(english, _)| english == source),
            "official English source key missing: {source}"
        );
        assert_eq!(tr("he", source), expected, "Hebrew native UI label: {source}");
    }
}

#[test]
fn hebrew_source_linked_typography_panels_and_search_night05() {
    for (source, expected) in [
        ("Cell Number:", "מספר תא:"),
        ("Change fill to:", "שינוי המילוי ל:"),
        ("Company", "חברה"),
        ("Contributors", "תורמים"),
        ("Dark", "כהה"),
        ("Email Address:", "כתובת דוא״ל:"),
        ("Graphic", "גרפיקה"),
        ("Hair Space", "רווח דקיק"),
        ("Light", "בהיר"),
        ("Lines added", "שורות שנוספו"),
        ("Lines deleted", "שורות שנמחקו"),
        ("Low Resolution", "רזולוציה נמוכה"),
        ("Medium Resolution", "רזולוציה בינונית"),
        ("PDF import cancelled: the originating document has closed", "ייבוא PDF בוטל: מסמך המקור נסגר"),
        ("Pin Bar Position", "קיבוע מיקום הסרגל"),
        ("Reverse the order", "היפוך הסדר"),
        ("Ruler Zero Point", "נקודת האפס בסרגל"),
        ("Select Story", "בחירת רצף טקסט"),
        ("Select Text Unit", "בחירת יחידת טקסט"),
        ("Simulate Paper Color", "הדמיית צבע הנייר"),
        ("Step and Repeat", "שכפול בצעדים קבועים"),
        ("Story Editor — {count} words", "עורך רצף טקסט — {count} מילים"),
        ("Subscript:", "כתב תחתי:"),
        ("Swap Text Fill and Stroke", "החלפת המילוי וקו המתאר של הטקסט"),
        ("Thread Text Frames", "שרשור מסגרות טקסט"),
        ("To Footer", "לכותרת תחתונה"),
        ("To Header", "לכותרת עליונה"),
        ("Update Data Source", "עדכון מקור הנתונים"),
        ("[No Language]", "[ללא שפה]"),
        ("{count} match(es)", "{count} התאמות"),
        ("{count} replacement(s) made", "בוצעו {count} החלפות"),
    ] {
        assert!(
            fr::TABLE.iter().any(|(english, _)| english == source),
            "official English source key missing: {source}"
        );
        assert_eq!(tr("he", source), expected, "Hebrew UI label: {source}");
    }
    assert_eq!(tr("en", "Step and Repeat"), "Step and Repeat");
}

#[test]
fn hebrew_typography_table_and_view_source_labels_day05() {
    // Official DesignCraft English keys; no translation content is copied from other locales.
    for (source, expected) in [
        ("Alpha Channel", "ערוץ אלפא"),
        ("View/Structure", "תצוגה/מבנה"),
        ("Kerning/Tracking Step", "צעד ריווח בין אותיות"),
        ("Object Style Options…", "אפשרויות סגנון אובייקט…"),
        ("Punctuation Space", "רווח סימני פיסוק"),
        ("Quarter Space", "רבע רווח"),
        ("Registered Trademark Symbol", "סימן מסחר רשום"),
        ("Section Symbol", "סימן סעיף"),
        ("Set OpenType Features", "הגדרת תכונות OpenType"),
        ("Single Left Quotation Mark", "גרש פותח"),
        ("Single Right Quotation Mark", "גרש סוגר"),
        ("Sixth Space", "שישית רווח"),
        ("Straight Double Quotation Marks", "מרכאות כפולות ישרות"),
        ("Straight Single Quotation Mark (Apostrophe)", "גרש ישר (אפוסטרוף)"),
        ("Table/Convert Cell Type", "טבלה/המרת סוג תא"),
        ("Table/Delete", "טבלה/מחיקה"),
        ("Table/Insert", "טבלה/הוספה"),
        ("Table/Select", "טבלה/בחירה"),
        ("Thin Space", "רווח דק"),
        ("Third Space", "שליש רווח"),
        ("To Body", "לגוף הטקסט"),
        ("Trademark Symbol", "סימן מסחר"),
        ("Vertical Type on a Path Tool", "כלי טקסט אנכי על נתיב"),
        ("View/Proof Setup", "תצוגה/הגדרת הגהת צבע"),
        ("View/Separations Preview", "תצוגה/תצוגה מקדימה של הפרדות צבע"),
        ("Working CMYK", "מרחב CMYK לעבודה"),
    ] {
        assert!(fr::TABLE.iter().any(|(english, _)| english == source), "official English source missing: {source}");
        assert_eq!(tr("he", source), expected, "Hebrew UI label: {source}");
    }
    assert_eq!(tr("en", "Object Style Options…"), "Object Style Options…");
}

#[test]
fn hebrew_layout_color_typography_and_panel_controls_are_localized() {
    // Original English keys are independently present in the official source-language catalog.
    for (source, expected) in [
        ("All Inks", "כל צבעי הדפוס"),
        ("Ink Limit 300%", "מגבלת דיו 300%"),
        ("Internet Standard RGB (sRGB)", "RGB תקני לאינטרנט (sRGB)"),
        ("Monitor RGB", "RGB של הצג"),
        ("Get Cell Stroke", "קבלת קו המתאר של התא"),
        ("Global Light", "תאורה כללית"),
        ("List Composite Fonts", "רשימת גופנים מורכבים"),
        ("Set Composite Font", "הגדרת גופן מורכב"),
        ("View/Extras", "תצוגה/עזרים"),
        ("View/Rotate Spread", "תצוגה/סיבוב מפתח עמודים"),
        ("View/Screen Mode", "תצוגה/מצב מסך"),
        ("Transform Again Individually", "חזרה על שינוי צורה לכל פריט בנפרד"),
        ("Transform Sequence Again", "חזרה על רצף שינויי צורה"),
        ("Transform Sequence Again Individually", "חזרה על רצף שינויי צורה לכל פריט בנפרד"),
        ("Title Case", "אות גדולה בתחילת כל מילה"),
        ("Sentence case", "אות גדולה בתחילת משפט"),
        ("lowercase", "אותיות קטנות"),
        ("Name (A–Z)", "שם (A–Z)"),
        ("Invalid panel layout; restored the default dock", "פריסת החלוניות אינה תקינה; סידור החלוניות המוגדר כברירת מחדל שוחזר"),
        ("Text:", "טקסט:"),
        ("Subject:", "נושא:"),
        ("URL:", "כתובת URL:"),
        ("Load DTD", "טעינת DTD"),
        ("Validate from Root Element", "אימות מרכיב השורש"),
        ("Layout/Index", "פריסה/אינדקס"),
        ("Layout/Pages", "פריסה/עמודים"),
    ] {
        assert!(fr::TABLE.iter().any(|(english, _)| english == source), "official English source missing: {source}");
        assert_eq!(tr("he", source), expected, "Hebrew UI label: {source}");
    }
}

#[test]
fn hebrew_repository_help_asset_and_desktop_publishing_labels_are_localized() {
    // These keys are English-source entries in the official UI catalog, never copied translations.
    for (source, expected) in [
        ("(any)", "(כלשהו)"),
        ("(unchanged)", "(ללא שינוי)"),
        ("ArtCraft App Page", "עמוד היישום ב־ArtCraft"),
        ("ArtCraft Website", "אתר ArtCraft"),
        ("Binary assets added", "נכסים בינאריים שנוספו"),
        ("Binary assets removed", "נכסים בינאריים שהוסרו"),
        ("Chinese: Simplified", "סינית: מפושטת"),
        ("Chinese: Traditional", "סינית: מסורתית"),
        ("Commit Preview", "תצוגה מקדימה של קומיט"),
        ("DesignCraft Website", "אתר DesignCraft"),
        ("DesignCraft on GitHub", "DesignCraft ב־GitHub"),
        ("DesignCraft page", "עמוד DesignCraft"),
        ("First commit", "הקומיט הראשון"),
        ("Join the ArtCraft Discord", "הצטרפות לקהילת ArtCraft ב־Discord"),
        ("Last commit", "הקומיט האחרון"),
        ("Legacy Macintosh RGB", "RGB מדור קודם של Macintosh"),
        ("Line delta", "שינוי במספר השורות"),
        ("Lines +/−", "שורות +/−"),
        ("Medium Dark", "כהה בינוני"),
        ("Medium Light", "בהיר בינוני"),
        ("Merged PRs", "בקשות משיכה שמוזגו"),
        ("Models", "מודלים"),
        ("+ Add Version", "+ הוספת גרסה"),
        ("No contributor data was built into this copy.", "עותק זה נבנה ללא נתוני תורמים."),
        ("No model credits were built into this copy.", "עותק זה נבנה ללא פרטי ייחוס למודלים."),
        ("PRs", "בקשות משיכה"),
        ("Picas", "פיקות"),
        ("Real name", "שם אמיתי"),
        ("Report an issue", "דיווח על בעיה"),
        ("image editing", "עריכת תמונות"),
        ("layer", "שכבה"),
        ("layers", "שכבות"),
        ("motion graphics and visual effects", "גרפיקה בתנועה ואפקטים חזותיים"),
        ("photo library and raw development", "ספריית תמונות ופיתוח קובצי RAW"),
        ("reading, organizing and protecting PDFs", "קריאה, ארגון והגנה על קובצי PDF"),
        ("the default printer", "מדפסת ברירת המחדל"),
        ("vector illustration", "איור וקטורי"),
        ("video editing, color and sound", "עריכת וידאו, צבע וקול"),
        ("{contributors} contributors · {commits} commits", "תורמים: {contributors} · קומיטים: {commits}"),
        ("{label}: {name} (double-click: Color Picker)", "{label}: {name} (לחיצה כפולה: בורר הצבעים)"),
        ("Story {story} ← story {parent}", "רצף טקסט {story} ← רצף טקסט {parent}"),
        ("{count} conflict(s): {names}", "התנגשויות ({count}): {names}"),
        ("{name}: {count} step(s) done", "{name}: מספר השלבים שהושלמו — {count}"),
        ("Sent {count} page(s) to {printer}", "מספר העמודים שנשלחו אל {printer}: {count}"),
    ] {
        assert!(
            fr::TABLE.iter().any(|(english, _)| english == source),
            "official English source missing: {source}"
        );
        assert_eq!(tr("he", source), expected, "Hebrew UI label: {source}");
    }
}

#[test]
fn hebrew_night04_live_native_dialog_controls() {
    for (source, expected) in [
        ("Weight:", "משקל:"),
        ("Tint:", "גוון:"),
        ("Offset:", "היסט:"),
        ("Size:", "גודל:"),
        ("All", "הכול"),
        ("Auto-Fit", "התאמה אוטומטית"),
        ("Horizontal Scale:", "קנה מידה אופקי:"),
        ("Position:", "מיקום:"),
        ("Vertical Scale:", "קנה מידה אנכי:"),
        ("Name:", "שם:"),
        ("Stroke Behind Fill", "קו מתאר מאחורי המילוי"),
        ("Width:", "רוחב:"),
        ("lines", "שורות"),
        ("Align On:", "יישור לפי:"),
        ("Angle:", "זווית:"),
        ("Based On", "מבוסס על"),
        ("Based On:", "מבוסס על:"),
        ("Cap:", "קצה קו:"),
        ("Color Mode", "מצב צבע"),
        ("Delete Preset", "מחיקת ערכת הגדרות"),
        ("Digits", "ספרות"),
        ("Font Style", "סגנון גופן"),
        ("Go To", "מעבר אל"),
        ("Home", "בית"),
        ("Include Roman Characters", "כלילת תווים לטיניים"),
        ("Invert", "היפוך"),
        ("Join:", "חיבור:"),
        ("Leader:", "תו מילוי:"),
        ("Merge options", "אפשרויות מיזוג"),
        ("Prefix:", "קידומת:"),
        ("Preset:", "ערכת הגדרות:"),
        ("Record", "רשומה"),
        ("Replace with:", "החלפה ב:"),
        ("Strikethrough", "קו חוצה"),
        ("Style Name", "שם הסגנון"),
        ("Text Color", "צבע הטקסט"),
        ("Update", "עדכון"),
        ("Found", "נמצאו"),
        ("Accept", "אישור"),
        ("Accept All", "אישור הכול"),
        ("All Caps", "אותיות רישיות"),
        ("Case sensitive", "התאמה לאותיות רישיות וקטנות"),
        ("Clear All", "ניקוי הכול"),
        ("Compress images", "דחיסת תמונות"),
        ("Create", "יצירה"),
        ("Crop", "חיתוך"),
        ("Crop Marks", "סימני חיתוך"),
        ("Cover Image:", "תמונת כריכה:"),
        ("Copies:", "עותקים:"),
        ("Custom", "מותאם אישית"),
        ("Default", "ברירת מחדל"),
        ("Cancelled: the document the dialog was for has closed", "בוטל: המסמך שעבורו נפתחה תיבת הדו־שיח נסגר"),
        ("DesignCraft Updates", "עדכוני DesignCraft"),
        ("Dimensions Include Stroke Weight", "המידות כוללות את עובי קו המתאר"),
        ("Distance", "מרחק"),
        ("Convert Shape", "המרת צורה"),
        ("Color Theme", "ערכת צבעים"),
        ("Add at least one version.", "יש להוסיף גרסה אחת לפחות."),
        ("Choose at least one layout.", "יש לבחור פריסה אחת לפחות."),
        ("Format:", "תבנית:"),
        ("Apply to Stroke", "החלה על קו המתאר"),
        ("Clear overrides", "ניקוי דריסות עיצוב"),
        ("Crop to:", "חיתוך לפי:"),
        ("Color Type", "סוג צבע"),
    ] {
        assert_eq!(tr("he", source), expected, "Hebrew native control: {source}");
        assert_eq!(tr("en", source), source, "English must remain unchanged: {source}");
    }
}

#[test]
fn hebrew_night04_live_page_export_and_font_controls() {
    for (source, expected) in [
        ("[Custom]", "[מותאם אישית]"),
        ("Add to the name", "הוספה לשם"),
        ("Adjust page elements to document changes", "התאמת רכיבי העמוד לשינויים במסמך"),
        ("All Classes", "כל הסיווגים"),
        ("All pages", "כל העמודים"),
        ("Apply the changes; objects follow the new page size and margins", "החלת השינויים; האובייקטים יותאמו לגודל העמוד ולשוליים החדשים"),
        ("Apply to:", "החל על:"),
        ("Automatic Page Numbering", "מספור עמודים אוטומטי"),
        ("Auto-Size", "התאמת גודל אוטומטית"),
        ("Auto-Sizing", "התאמת גודל אוטומטית"),
        ("Between Entry and Number:", "בין הרשומה למספר:"),
        ("Binding:", "כריכה:"),
        ("Bleed and Slug", "גלישה ושטח מידע"),
        ("Bleed Marks", "סימני גלישה"),
        ("Bleed, no marks", "גלישה ללא סימנים"),
        ("Book Name:", "שם הספר:"),
        ("Both pages of the parent spread", "שני עמודי כפולת האב"),
        ("Both pages of the spread", "שני עמודי הכפולה"),
        ("Colors and Inks: {process} process, {spot} spot", "צבעים ודיו: {process} צבעי תהליך, {spot} צבעי ספוט"),
        ("Column Gutter", "מרווח בין טורים"),
        ("Compress Images", "דחיסת תמונות"),
        ("Corner Size", "גודל הפינות"),
        ("Create PDF Bookmarks", "יצירת סימניות PDF"),
        ("Document Chapter Numbering", "מספור פרקי המסמך"),
        ("Document Name", "שם המסמך"),
        ("Duplicate version", "שכפול גרסה"),
        ("Embedded", "מוטמע"),
        ("Embedded images: {embedded}", "תמונות מוטמעות: {embedded}"),
        ("Entry Style:", "סגנון רשומה:"),
        ("Filters:", "מסננים:"),
        ("Fixed", "קבוע"),
        ("Folder name", "שם התיקייה"),
        ("Fonts: {used} in use, {missing} unavailable", "גופנים: {used} בשימוש, {missing} אינם זמינים"),
        ("Gray", "אפור"),
        ("Height:", "גובה:"),
        ("Include Paragraph Styles:", "כלול סגנונות פסקה:"),
        ("Include Prefix when Numbering Pages", "כלול קידומת במספור העמודים"),
        ("Increment Every:", "הגדל בכל:"),
        ("Large Sample Text Size", "גודל טקסט לדוגמה — גדול"),
        ("Level:", "רמה:"),
        (
            "Links and Images: {found} linked, {modified} changed, {missing} not found",
            "קישורים ותמונות: {found} מקושרים, {modified} שונו, {missing} לא נמצאו",
        ),
        ("Medium Sample Text Size", "גודל טקסט לדוגמה — בינוני"),
        ("Min:", "מינימום:"),
        ("Minimum Height", "גובה מזערי"),
        ("Minimum Width", "רוחב מזערי"),
        ("Mode:", "מצב:"),
        ("No Line Breaks", "ללא מעברי שורה"),
        ("Optical Margin Alignment", "יישור שוליים אופטי"),
        ("Optical Margin Size", "גודל שוליים אופטיים"),
        ("Orientation:", "כיוון העמוד:"),
        ("Other Styles:", "סגנונות אחרים:"),
        ("Page Number:", "מספר עמוד:"),
        ("Page Size:", "גודל העמוד:"),
        ("Preset Details", "פרטי הגדרה קבועה מראש"),
        ("Preset name", "שם ההגדרה הקבועה מראש"),
        ("Preset saved.", "ההגדרה הקבועה מראש נשמרה."),
        ("Print: marks + bleed", "הדפסה: סימנים וגלישה"),
        ("Remove Existing Ruler Guides", "הסרת קווי העזר הקיימים"),
        ("Replace Existing Table of Contents", "החלפת תוכן העניינים הקיים"),
        ("Save Document Preset", "שמירת הגדרות מסמך"),
        ("Save Document Preset As:", "שמירת הגדרות מסמך בשם:"),
        ("Save Preset", "שמירת הגדרה קבועה מראש"),
        ("Section Marker:", "סמן מקטע:"),
        ("Section Prefix:", "קידומת מקטע:"),
        ("Show Recently Added", "הצגת גופנים שנוספו לאחרונה"),
        ("Show Similar Fonts", "הצגת גופנים דומים"),
        ("Similar to {family}", "דומים ל־{family}"),
        ("Small Sample Text Size", "גודל טקסט לדוגמה — קטן"),
        ("Sort Entries in Alphabetical Order", "מיון רשומות לפי סדר אלפביתי"),
        ("Start Page Numbering at:", "התחלת מספור העמודים מ־:"),
        ("Start Section", "התחלת מקטע"),
        ("Styles in Table of Contents", "סגנונות בתוכן העניינים"),
        ("The document has one layout: all its pages are exported.", "במסמך יש פריסה אחת: כל העמודים ייוצאו."),
        ("This page", "עמוד זה"),
        ("Title:", "כותרת:"),
        ("Use Custom Baseline Grid", "שימוש ברשת קווי בסיס מותאמת אישית"),
    ] {
        assert_eq!(tr("he", source), expected, "Hebrew live control: {source}");
        assert_eq!(tr("en", source), source, "Unchanged English: {source}");
    }
}

#[test]
fn hebrew_catalog_has_unique_source_keys_including_multiline_entries() {
    let mut keys = HashSet::new();
    for (source, translated) in he::TABLE {
        assert!(keys.insert(*source), "duplicate Hebrew source key: {source}");
        assert!(!translated.trim().is_empty(), "empty Hebrew translation: {source}");
    }
}
