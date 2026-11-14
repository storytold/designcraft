//! App policy for preserving TIFF samples as DeviceCMYK; pixel decoding stays shared.
use std::io::Cursor;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TiffInkPolicy {
    Preserve,
    Rgb,
    Refuse,
}

pub(super) fn inspect(bytes: &[u8]) -> Option<TiffInkPolicy> {
    use tiff::tags::Tag;
    let info = craft_image_codecs::tiff_info(bytes).ok()?;
    let index = info.default_page()?;
    let page = info.pages.get(index)?;
    if page.photometric != Some(5) || !matches!(page.samples_per_pixel, 4 | 5) {
        return Some(TiffInkPolicy::Rgb);
    }
    // The shared page picker skips reduced-resolution IFDs. Inspect that same main-chain page.
    let main_index = info.pages.get(..index)?.iter().filter(|page| page.parent.is_none()).count();
    let mut metadata = tiff::decoder::Decoder::new(Cursor::new(bytes)).ok()?;
    metadata.seek_to_image(main_index).ok()?;
    // Premultiplied CMYK samples cannot be passed through as straight printing inks.
    if page.samples_per_pixel == 5
        && metadata.find_tag_unsigned_vec::<u16>(Tag::ExtraSamples).ok()?.and_then(|values| values.first().copied()) != Some(2)
    {
        return Some(TiffInkPolicy::Refuse);
    }
    // InkSet 2 names other inks. Treating those bytes as DeviceCMYK silently changes output.
    let other_inks = metadata.find_tag_unsigned::<u16>(Tag::Unknown(332)).ok()?.is_some_and(|set| set != 1);
    if other_inks || page.planar || !matches!(page.bits_per_sample, 8 | 16) || page.sample_format != 1 {
        return Some(TiffInkPolicy::Rgb);
    }
    Some(TiffInkPolicy::Preserve)
}
