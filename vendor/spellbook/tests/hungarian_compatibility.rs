// SPDX-License-Identifier: MPL-2.0
use spellbook::Dictionary;

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
