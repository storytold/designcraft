use super::*;
use std::collections::HashSet;

/// Every per-language table `tr` looks up, by interface language.
const LANGUAGE_TABLES: &[(&str, &[(&str, &str)])] = &[
    ("ar", ar::TABLE),
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
    // Keep the repaired Georgian menu captions present with their exact translations.
    assert_eq!(tr("ka", "File Info…"), "ფაილის ინფორმაცია…");
    assert_eq!(tr("ka", "Check for Updates…"), "განახლებების შემოწმება…");
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

/// Checks language-specific context precedence and fallback to the plain translation.
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
fn only_arabic_lays_out_right_to_left() {
    for (code, _) in LANGUAGES {
        assert_eq!(is_rtl(code), *code == "ar", "{code}");
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
    // UI labels passed to `tr` that no other catalog has yet.
    keys.extend([
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
