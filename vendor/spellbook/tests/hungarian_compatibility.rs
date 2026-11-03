// SPDX-License-Identifier: MPL-2.0
extern crate alloc;
use spellbook::{DefaultHashBuilder, Dictionary};
type Flag = core::num::NonZeroU16;
#[allow(dead_code)]
#[path = "../src/umbra_slice.rs"]
mod safe_storage;

#[test]
fn compound_negative_syllable_modifier_respects_limit() {
    let aff = |limit| {
        format!(
            "SET UTF-8\nCOMPOUNDMIN 1\nCOMPOUNDFLAG C\nCOMPOUNDROOT Y\n\
             COMPOUNDWORDMAX 2\nCOMPOUNDSYLLABLE {limit} ae\n"
        )
    };
    let dic = "2\nbaa/C\nda/CIY\n";
    // The I flag at the end subtracts one syllable: 3 - 1 = 2.
    assert!(Dictionary::new(&aff(2), dic).unwrap().check("baada"));
    assert!(!Dictionary::new(&aff(1), dic).unwrap().check("baada"));
}

#[test]
fn compound_final_part_accepts_licensed_suffix_chain() {
    let aff = "SET UTF-8\nCOMPOUNDMIN 1\nCOMPOUNDFLAG C\nCOMPOUNDFORBIDFLAG F\n\
                   SFX A Y 1\nSFX A 0 s/B .\nSFX B Y 1\nSFX B 0 x .\n\
                   SFX E Y 1\nSFX E 0 s/BF .\n";
    let dic = "4\nsun/C\nflower/CA\nrose/A\nweed/CE\n";
    let dict = Dictionary::new(aff, dic).unwrap();
    for word in ["sunflowersx", "sunflower", "flowersx"] {
        assert!(dict.check(word), "{word}");
    }
    for word in ["sunrosesx", "sunflowersxx", "sunweedsx", "sunflowersxsun"] {
        assert!(!dict.check(word), "{word}");
    }
}

#[test]
fn compound_rule_final_suffix_must_match_the_rule() {
    let aff = "SET UTF-8\nCOMPOUNDMIN 1\nCOMPOUNDRULE 1\nCOMPOUNDRULE AB\n\
                   COMPOUNDFORBIDFLAG F\nFORBIDDENWORD X\n\
                   SFX S Y 1\nSFX S 0 -ok .\nSFX T Y 1\nSFX T 0 -bad/F .\n";
    // The final part has a homonym with unrelated flags: try all candidates.
    let dic = "5\n2/A\n6/BST\n7/S\n8/BSX\n6/CS\n";
    let dict = Dictionary::new(aff, dic).unwrap();
    for word in ["26-ok", "6-ok", "26-bad", "28-ok"] {
        assert!(dict.check(word), "{word}");
    }
    for word in ["27-ok", "226-ok", "26-no", "8-ok"] {
        assert!(!dict.check(word), "{word}");
    }
}

#[test]
fn terminal_suffix_forbid_flag_matches_hunspell_without_relaxing_chains() {
    let aff = "SET UTF-8\nCOMPOUNDMIN 1\nCOMPOUNDFLAG C\nCOMPOUNDFORBIDFLAG F\n\
                   FORBIDDENWORD X\nBREAK 0\n\
                   SFX D Y 1\nSFX D 0 q/F .\n\
                   SFX A Y 1\nSFX A 0 s/BF .\nSFX B Y 1\nSFX B 0 x .\n";
    let dic = "4\nsun/C\nbud/CD\nweed/CA\nbad/CDX\n";
    let dict = Dictionary::new(aff, dic).unwrap();
    for word in ["sunbudq", "budq"] {
        assert!(dict.check(word), "{word}");
    }
    for word in ["sunbudqsun", "sunweedsx", "sunbadq", "badq", "sunbudqq"] {
        assert!(!dict.check(word), "{word}");
    }
    let banned = Dictionary::new(aff, "3\nsun/C\nbud/CD\nsunbudq/X\n").unwrap();
    assert!(!banned.check("sunbudq"), "an explicit whole-word ban still wins");
}

fn hungarian() -> Dictionary {
    Dictionary::new(include_str!("../../../assets/dictionaries/hu_HU/hu_HU.aff"), include_str!("../../../assets/dictionaries/hu_HU/hu_HU.dic"))
        .unwrap()
}

#[test]
fn agrees_with_hunspell_1_7_5_on_recorded_corpora() {
    let dict = hungarian();
    let corpora = [
        include_str!("../../../tools/hu-language-tests/fixtures/main.tsv"),
        include_str!("../../../tools/hu-language-tests/fixtures/focused.tsv"),
        include_str!("../../../tools/hu-language-tests/fixtures/extended.tsv"),
    ];
    let mut count = 0;
    for corpus in corpora {
        for line in corpus.lines() {
            let (word, verdict) = line.split_once('\t').unwrap();
            assert_eq!(dict.check(word), verdict == "1", "{word}");
            count += 1;
        }
    }
    assert_eq!(count, 550);
}

#[test]
fn supplies_expected_hungarian_corrections() {
    let dict = hungarian();
    let mut count = 0;
    for line in include_str!("../../../tools/hu-language-tests/fixtures/suggestions.tsv").lines() {
        let (word, expected) = line.split_once('\t').unwrap();
        let mut suggestions = Vec::new();
        dict.suggest(word, &mut suggestions);
        assert!(suggestions.iter().any(|s| s == expected), "{word}: {suggestions:?}");
        count += 1;
    }
    assert_eq!(count, 10);
}

#[test]
fn ngram_suggestions_preserve_crossproduct_suffixes() {
    let aff = "SET UTF-8\nPFX P Y 1\nPFX P 0 un .\nSFX S Y 1\nSFX S 0 ed .\n";
    let dict = Dictionary::new(aff, "1\nrecord/PS\n").unwrap();
    let mut suggestions = Vec::new();
    // Multiple substitutions require the ngram path, not single-edit corrections.
    dict.suggester().with_ngram_suggestions(false).suggest("unrxxorded", &mut suggestions);
    assert!(!suggestions.iter().any(|s| s == "unrecorded"));
    dict.suggest("unrxxorded", &mut suggestions);
    assert!(suggestions.iter().any(|s| s == "unrecorded"), "{suggestions:?}");
}

#[test]
fn ngram_prefix_strip_uses_the_checked_suffixed_stem() {
    let aff = "SET UTF-8\nPFX P Y 1\nPFX P recorded unrecorded .\n\
               SFX S Y 1\nSFX S record recorded .\n";
    let dict = Dictionary::new(aff, "1\nrecord/PS\n").unwrap();
    // The prefix's strip matches only the suffixed stem. Applying it to the bare
    // root panicked even though the crossproduct condition had already passed.
    let mut suggestions = Vec::new();
    dict.suggest("unrecordedxrecorded", &mut suggestions);
}

#[test]
fn alternating_three_affixes_check_the_restored_middle_stem() {
    for (complex, middle, good, bad) in [
        ("", "PFX P Y 1\nPFX P 0 un {condition}\nSFX A Y 1\nSFX A 0 s/BP .\nSFX B Y 1\nSFX B 0 x .\n", "f", "u"),
        ("COMPLEXPREFIXES\n", "SFX S Y 1\nSFX S 0 ed {condition}\nPFX A Y 1\nPFX A 0 re/BS .\nPFX B Y 1\nPFX B 0 un .\n", "r", "d"),
    ] {
        let word = if complex.is_empty() { "unflowersx" } else { "unreflowered" };
        for (condition, accepted) in [(good, true), (bad, false)] {
            let aff = format!("SET UTF-8\n{complex}{}", middle.replace("{condition}", condition));
            let dict = Dictionary::new(&aff, "1\nflower/A\n").unwrap();
            assert_eq!(dict.check(word), accepted, "{word}, condition={condition}");
        }
    }
}

#[test]
fn simplified_triple_requires_two_equal_preceding_characters() {
    let aff = "SET UTF-8\nCOMPOUNDMIN 1\nCOMPOUNDBEGIN B\nCOMPOUNDEND E\nSIMPLIFIEDTRIPLE\n";
    for (first, second, valid, invalid) in
        [("ball", "legény", "ballegény", "balegény"), ("ább", "bc", "ábbc", "ábc"), ("aáá", "ábc", "aáábc", "aábc")]
    {
        let dic = format!("2\n{first}/B\n{second}/E\n");
        let dict = Dictionary::new(aff, &dic).unwrap();
        assert!(dict.check(valid), "{valid}");
        let ordinary = Dictionary::new(&aff.replace("SIMPLIFIEDTRIPLE\n", ""), &dic).unwrap();
        assert!(!ordinary.check(valid), "{valid} requires SIMPLIFIEDTRIPLE");
        let short_first = &first[..first.char_indices().last().unwrap().0];
        let negative_dic = format!("2\n{short_first}/B\n{second}/E\n");
        let negative = Dictionary::new(aff, &negative_dic).unwrap();
        assert!(!negative.check(invalid), "{invalid}");
    }
}

#[test]
fn review_compound_rules_try_each_first_part_homonym() {
    let aff = "SET UTF-8\nCOMPOUNDMIN 1\nCOMPOUNDRULE 4\nCOMPOUNDRULE AB\nCOMPOUNDRULE CD\nCOMPOUNDRULE AEF\nCOMPOUNDRULE CGH\n";
    for first in ["foo/A\nfoo/C", "foo/C\nfoo/A"] {
        let dic = format!("8\n{first}\nbar/B\nbaz/D\nmid/E\nmid/G\nend/F\nfin/H\n");
        let dict = Dictionary::new(aff, &dic).unwrap();
        for word in ["foobar", "foobaz", "foomidend", "foomidfin"] {
            assert!(dict.check(word), "{word}; {first}");
        }
        for word in ["fooend", "foofin", "foobarmid", "foomidbar"] {
            assert!(!dict.check(word), "{word}; {first}");
        }
    }
}

#[test]
fn review_rep_suggestions_validate_the_final_word() {
    for (replacement, accepted) in [("foo_bogus", false), ("foo_bar", true), ("foo_bar_bogus", false), ("foo_bar_baz", true)] {
        let aff = format!("SET UTF-8\nREP 1\nREP zz {replacement}\n");
        let dict = Dictionary::new(&aff, "3\nfoo\nbar\nbaz\n").unwrap();
        let mut suggestions = Vec::new();
        dict.suggest("zz", &mut suggestions);
        let expected = replacement.replace('_', " ");
        assert_eq!(suggestions.contains(&expected), accepted, "{replacement}: {suggestions:?}");
    }
}

#[test]
fn review_add_rejects_oversized_utf8_entries_without_panicking() {
    let mut dict = Dictionary::new("SET UTF-8\n", "1\nword\n").unwrap();
    for entry in ["a".repeat(65_536), "é".repeat(32_768)] {
        assert!(dict.add(&entry).is_err());
        assert!(dict.check("word"));
    }
    assert!(dict.add(&"a".repeat(65_535)).is_ok());
    assert!(dict.add(&format!("{}a", "é".repeat(32_767))).is_ok());
}

#[test]
fn review_owned_storage_handles_unicode_lengths_and_threads() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Dictionary>();
    for len in [0, 1, 3, 7, 8, 14, 15, 256, 65_535] {
        let values: Vec<u16> = (0..len).map(|i| (i % 256) as u16).collect();
        let slice = safe_storage::UmbraSlice::<u16, 3, 4>::try_from(values.as_slice()).unwrap();
        assert_eq!(slice.as_slice(), values);
        assert_eq!(slice.clone().as_slice(), values);
    }
    assert!(safe_storage::UmbraSlice::<u16, 3, 4>::try_from(vec![1u16; 65_536].as_slice()).is_err());
    for text in ["", "őű", "aé中😀z", "árvíztűrő tükörfúrógép"] {
        let stem = safe_storage::UmbraString::from(text);
        assert_eq!(stem.as_str(), text);
        assert_eq!(stem.as_bytes(), text.as_bytes());
        assert_eq!(stem.clone(), stem);
    }
}
#[test]
fn review_unicode_suggestions_survive_swaps_moves_and_replacements() {
    let aff = "SET UTF-8\nTRY aáéíóöőúüűzrvtk\n";
    let dict = Dictionary::new(aff, "2\nárvíz\ntűz\n").unwrap();
    for typo in ["rávíz", "árívz", "írváz", "árví", "árvízz", "tüz", "űtz"] {
        let mut suggestions = Vec::new();
        dict.suggest(typo, &mut suggestions);
        assert!(!suggestions.is_empty(), "{typo}");
        assert!(suggestions.iter().all(|s| dict.check(s)), "{typo}: {suggestions:?}");
    }
}
