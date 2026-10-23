//! Rebuild only the Hungarian dictionary. Run from repository root.
//! cargo run --manifest-path tools/hu-hyphenation/Cargo.toml --offline
use hyphenation_commons::{
    dictionary::{
        extended::{Builder, Exceptions, Extended, Patterns, Subregion, Tally},
        Locus,
    },
    Language,
};
use std::{error::Error, fs};
fn parse(line: &str) -> Result<(String, Tally), Box<dyn Error>> {
    let (raw, change) = line.split_once('/').map_or((line, None), |(a, b)| (a, Some(b)));
    let mut word = String::new();
    let mut standard = Vec::new();
    for c in raw.chars() {
        if let Some(value) = c.to_digit(10) {
            standard.push(Locus { index: u8::try_from(word.len())?, value: value as u8 });
        } else {
            word.push(c);
        }
    }
    let subregion = if let Some(change) = change {
        let mut args = change.split(',');
        let rep = args.next().ok_or("missing replacement")?;
        let start = args
            .next()
            .ok_or("missing replacement start")?
            .parse::<usize>()?
            .checked_sub(1)
            .ok_or("zero start")?
            .checked_add(usize::from(word.starts_with('.')))
            .ok_or("start overflow")?;
        let cut = args.next().ok_or("missing replacement length")?.parse::<usize>()?;
        if args.next().is_some() {
            return Err(format!("unexpected replacement arguments: {line}").into());
        }
        let (left, right) = rep.split_once('=').ok_or("missing replacement break")?;
        if !left.chars().chain(right.chars()).all(char::is_alphabetic) {
            return Err("non-letter replacement".into());
        }
        let offsets: Vec<_> = word.char_indices().map(|(i, _)| i).chain(std::iter::once(word.len())).collect();
        let a = *offsets.get(start).ok_or("start outside pattern")?;
        let b = *offsets.get(start.checked_add(cut).ok_or("overflow")?).ok_or("end outside pattern")?;
        let locus = standard
            .iter()
            .filter(|l| l.value % 2 == 1 && (a..=b).contains(&(l.index as usize)))
            .max_by_key(|l| l.value)
            .ok_or("missing odd replacement point")?;
        Some((
            *locus,
            Subregion {
                left: locus.index as usize - a,
                right: b - locus.index as usize,
                substitution: format!("{left}{right}"),
                breakpoint: left.len(),
            },
        ))
    } else {
        None
    };
    Ok((word, Tally { standard, subregion }))
}
fn main() -> Result<(), Box<dyn Error>> {
    let dir = "assets/hyphenation/hu";
    let input = fs::read_to_string(format!("{dir}/hyph_hu_HU.dic"))?;
    let mut pairs = Vec::new();
    for line in input.lines() {
        if line.is_empty()
            || line.starts_with('%')
            || line == "UTF-8"
            || line.starts_with("COMPOUNDLEFTHYPHENMIN ")
            || line.starts_with("COMPOUNDRIGHTHYPHENMIN ")
        {
            continue;
        }
        pairs.push(parse(line)?);
    }
    let n = pairs.len();
    let hu: Extended = Builder { language: Language::Hungarian, patterns: Patterns::from_iter(pairs)?, exceptions: Exceptions::default() }.into();
    let output = bincode::serialize(&hu)?;
    fs::write(format!("{dir}/hu.extended.bincode"), &output)?;
    println!("{n} Hungarian patterns, {} serialized bytes", output.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_upstream_rules_fail_without_panicking() {
        for rule in [
            ".mas5száz8s3zok1ni/sz=3,1,1,14",
            "gic5csetek/cs=,3,1 ,1,9",
            "as5szony/sz=,0,1",
            "as5szony/sz=,999999999999999999999,1",
            "as5szony/sz=,3,999999999999999999999",
        ] {
            assert!(parse(rule).is_err(), "{rule}");
        }
    }
    #[test]
    fn corrected_rules_insert_the_required_letter() {
        for rule in [".mas5száz8s3zok1ni/sz=,3,1", "gic5csetek/cs=,3,1"] {
            let (_, tally) = parse(rule).unwrap();
            let (locus, sub) = tally.subregion.unwrap();
            assert!(locus.value % 2 == 1);
            assert_eq!(sub.left, 1);
            assert_eq!(sub.right, 0);
            assert_eq!(sub.breakpoint, 2);
        }
    }
}
