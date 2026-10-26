//! The document's inks as a [`craft_prepress::InkSet`]: the four process inks, each spot swatch
//! with its color as the alternate, and the Ink Manager settings (aliases, spots to process).
//!
//! The document saves the Ink Manager by swatch name ([`designcraft_doc::InkManager`]); the set
//! is built from it whenever output needs it, so renaming or editing a swatch never leaves a
//! stale copy.

use craft_prepress::{Alternate, Ink, InkId, InkSet, Spot};
use designcraft_color::swatch::{ColorType, Swatch, SwatchValue};
use designcraft_doc::Document;

fn is_spot(w: &Swatch) -> bool {
    matches!(w.value, SwatchValue::Color { color_type: ColorType::Spot, .. })
}

/// The inks of `doc` with its Ink Manager settings applied. An alias whose target isn't a spot
/// swatch, or that would make a loop, is left out: that spot prints on its own plate.
pub fn ink_set(doc: &Document) -> InkSet {
    let mut set = InkSet::new();
    for w in doc.swatches.iter().filter(|w| is_spot(w)) {
        let SwatchValue::Color { color, .. } = w.value else { continue };
        let alternate = match color {
            designcraft_color::Color::Cmyk { c, m, y, k } => Alternate::Cmyk([c, m, y, k]),
            other => {
                let lab = other.to_lab();
                Alternate::Lab([lab.l, lab.a, lab.b])
            }
        };
        set.add(Ink::Spot(Spot { name: w.name.clone(), alternate, solidity: None }));
    }
    let spot_id = |set: &InkSet, name: &str| doc.swatch(name).filter(|w| is_spot(w)).and_then(|_| set.lookup(name));
    for name in &doc.inks.to_process {
        if let Some(id) = spot_id(&set, name) {
            set.set_to_process(id, true);
        }
    }
    for (from, to) in &doc.inks.aliases {
        if let (Some(from), Some(to)) = (spot_id(&set, from), spot_id(&set, to)) {
            set.alias(from, to);
        }
    }
    set.set_all_to_process(doc.inks.all_to_process);
    set
}

/// How spot swatch `name` prints: the spot swatch whose plate (or, converted, whose color) it
/// takes after aliases, and whether it prints as process color. A name the set doesn't hold
/// prints as itself on its own plate.
pub fn spot_output<'a>(doc: &'a Document, set: &InkSet, name: &'a str) -> (&'a str, bool) {
    let Some(id) = set.lookup(name) else { return (name, false) };
    let end = std::iter::successors(Some(id), |i| set.alias_of(*i)).take(set.len()).last().unwrap_or(id);
    let target = if end == id { name } else { swatch_of(doc, set, end).unwrap_or(name) };
    (target, set.plate_of(id).is_none())
}

/// The first spot swatch that is ink `id`.
fn swatch_of<'a>(doc: &'a Document, set: &InkSet, id: InkId) -> Option<&'a str> {
    doc.swatches.iter().filter(|w| is_spot(w)).find(|w| set.lookup(&w.name) == Some(id)).map(|w| w.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_color::Color;
    use designcraft_doc::build::NewDocument;

    fn spot(d: &mut Document, name: &str, color: Color) {
        d.swatches.push(Swatch {
            name: name.into(),
            value: SwatchValue::Color { color, color_type: ColorType::Spot },
            locked: false,
            named: true,
            hidden: false,
        });
    }

    #[test]
    fn aliases_follow_chains_and_converted_inks_print_as_process() {
        let mut d = Document::new(&NewDocument::default());
        spot(&mut d, "Gold", Color::Cmyk { c: 0.0, m: 0.2, y: 0.8, k: 0.1 });
        spot(&mut d, "Bronze", Color::Rgb { r: 0.6, g: 0.4, b: 0.2 });
        spot(&mut d, "Copper", Color::Cmyk { c: 0.0, m: 0.5, y: 0.6, k: 0.2 });
        d.inks.aliases = vec![("Gold".into(), "Bronze".into()), ("Bronze".into(), "Copper".into()), ("Copper".into(), "Gold".into())];
        d.inks.to_process = vec!["Copper".into()];
        let set = ink_set(&d);
        assert_eq!(set.spots().map(|(_, s)| s.name.as_str()).collect::<Vec<_>>(), ["Gold", "Bronze", "Copper"]);
        // The loop-closing alias is refused, so Gold → Bronze → Copper, which is converted.
        assert_eq!(spot_output(&d, &set, "Gold"), ("Copper", true));
        assert_eq!(spot_output(&d, &set, "Copper"), ("Copper", true));
        d.inks.to_process.clear();
        let set = ink_set(&d);
        assert_eq!(spot_output(&d, &set, "Bronze"), ("Copper", false));
        assert_eq!(spot_output(&d, &set, "Unknown"), ("Unknown", false));
        d.inks.all_to_process = true;
        assert!(spot_output(&d, &ink_set(&d), "Gold").1);
    }
}
