//! Images embedded in a PageMaker file: TIFF and BMP data kept in the file's image chunks, and
//! their pixel size from their own headers (so a graphic can be matched to its image by size).

use crate::bytes::Bytes;

/// The largest embedded image taken.
pub const MAX_IMAGE: usize = 64 << 20;

/// An embedded image: its bytes, MIME type and pixel size.
#[derive(Clone, Debug, PartialEq)]
pub struct Embedded {
    pub data: Vec<u8>,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

/// The image in `data`, if it is a TIFF or BMP whose size can be read.
pub fn embedded(data: Vec<u8>) -> Option<Embedded> {
    if data.len() > MAX_IMAGE {
        return None;
    }
    let (mime, (width, height)) = match data.get(..4)? {
        [b'I', b'I', 42, 0] | [b'M', b'M', 0, 42] => ("image/tiff", tiff_size(&data)?),
        [b'B', b'M', ..] => ("image/bmp", bmp_size(&data)?),
        _ => return None,
    };
    (width > 0 && height > 0).then_some(Embedded { data, mime, width, height })
}

/// Width and height from a TIFF's first image directory.
fn tiff_size(t: &[u8]) -> Option<(u32, u32)> {
    let b = Bytes::new(t, t.first() == Some(&b'I'));
    let ifd = b.u32(4)? as usize;
    let n = usize::from(b.u16(ifd)?).min(512);
    let (mut w, mut h) = (None, None);
    for k in 0..n {
        let e = ifd.checked_add(2 + 12 * k)?;
        let tag = b.u16(e)?;
        let value = match b.u16(e + 2)? {
            3 => u32::from(b.u16(e + 8)?),
            _ => b.u32(e + 8)?,
        };
        match tag {
            256 => w = Some(value),
            257 => h = Some(value),
            _ => {}
        }
    }
    Some((w?, h?))
}

/// Width and height from a BMP's info header.
fn bmp_size(t: &[u8]) -> Option<(u32, u32)> {
    let b = Bytes::new(t, true);
    let w = b.u32(18)? as i32;
    let h = b.u32(22)? as i32;
    Some((w.unsigned_abs(), h.unsigned_abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tiff_and_bmp_sizes_and_refuses_the_rest() {
        let mut tiff = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 2, 0];
        tiff.extend([0, 1, 3, 0, 1, 0, 0, 0, 20, 0, 0, 0]);
        tiff.extend([1, 1, 4, 0, 1, 0, 0, 0, 10, 0, 0, 0]);
        let e = embedded(tiff).unwrap();
        assert_eq!((e.mime, e.width, e.height), ("image/tiff", 20, 10));
        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[18..22].copy_from_slice(&7i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-3i32).to_le_bytes());
        assert_eq!(embedded(bmp).map(|e| (e.width, e.height)), Some((7, 3)));
        assert_eq!(embedded(b"II*\0\xff\xff\xff\xff".to_vec()), None);
        assert_eq!(embedded(b"%!PS".to_vec()), None);
        assert_eq!(embedded(vec![]), None);
    }
}
