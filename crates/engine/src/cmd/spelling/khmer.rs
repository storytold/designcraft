//! Khmer text in proofing. The shared craft-proof owns segmentation, dictionaries, preferred forms,
//! repetition signs and honorifics; these are what the review needs on top.

pub(super) fn character(c: char) -> bool {
    ('\u{1780}'..='\u{17ff}').contains(&c) || ('\u{19e0}'..='\u{19ff}').contains(&c)
}

#[cfg(test)]
mod tests;
