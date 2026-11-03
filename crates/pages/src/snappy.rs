//! The compression of iWork archives: a sequence of chunks, each a zero byte, a 24-bit
//! little-endian length and one raw Snappy block (no stream identifier, no checksums). Copies never
//! reach back across chunks, so each block decodes on its own and the results are concatenated.

use crate::{PagesError, Result};

/// The most one block may expand to (iWork writes at most 64 KiB per chunk).
const MAX_BLOCK: usize = 16 << 20;

/// Decode an `.iwa` stream, failing once the total would grow past `limit` bytes.
pub(crate) fn unframe(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let header = data.get(pos..pos.saturating_add(4)).ok_or_else(|| bad("truncated chunk header"))?;
        let &[kind, l0, l1, l2] = header else { return Err(bad("truncated chunk header")) };
        if kind != 0 {
            return Err(PagesError::Unsupported(format!("archive chunk type {kind} (only Snappy chunks are read)")));
        }
        let len = usize::from(l0) | usize::from(l1) << 8 | usize::from(l2) << 16;
        let start = pos + 4;
        let block = data.get(start..start.saturating_add(len)).ok_or_else(|| bad("truncated chunk"))?;
        decompress_block(block, &mut out, limit)?;
        pos = start + len;
    }
    Ok(out)
}

fn bad(msg: &str) -> PagesError {
    PagesError::Invalid(format!("compressed archive: {msg}"))
}

/// One element of a Snappy block.
enum Op<'a> {
    Literal(&'a [u8]),
    Copy { len: usize, offset: usize },
}

/// The element starting at `p` and the position after it.
fn next_op(block: &[u8], p: usize) -> Result<(Op<'_>, usize)> {
    let tag = *block.get(p).ok_or_else(|| bad("truncated element"))?;
    let p = p + 1;
    let field = |n: usize| block.get(p..p.saturating_add(n)).ok_or_else(|| bad("truncated element"));
    let le = |bytes: &[u8]| bytes.iter().rev().fold(0usize, |acc, &b| acc << 8 | usize::from(b));
    Ok(match tag & 3 {
        0 => {
            let n = usize::from(tag >> 2);
            let (len, p) = if n >= 60 { (le(field(n - 59)?).saturating_add(1), p + n - 59) } else { (n + 1, p) };
            let lit = block.get(p..p.saturating_add(len)).ok_or_else(|| bad("truncated literal"))?;
            (Op::Literal(lit), p + len)
        }
        1 => {
            let b = field(1)?;
            (Op::Copy { len: usize::from((tag >> 2) & 7) + 4, offset: usize::from(tag & 0xE0) << 3 | le(b) }, p + 1)
        }
        2 => (Op::Copy { len: usize::from(tag >> 2) + 1, offset: le(field(2)?) }, p + 2),
        _ => (Op::Copy { len: usize::from(tag >> 2) + 1, offset: le(field(4)?) }, p + 4),
    })
}

/// Append one raw Snappy block to `out`.
pub(crate) fn decompress_block(block: &[u8], out: &mut Vec<u8>, limit: usize) -> Result<()> {
    let (expected, mut p) = crate::proto::varint(block, 0).ok_or_else(|| bad("missing block length"))?;
    let expected = usize::try_from(expected).ok().filter(|&n| n <= MAX_BLOCK).ok_or_else(|| bad("block too large"))?;
    if out.len().saturating_add(expected) > limit {
        return Err(PagesError::Invalid("the archive expands past the size limit".into()));
    }
    let base = out.len();
    out.reserve(expected);
    while p < block.len() {
        let (op, next) = next_op(block, p)?;
        p = next;
        let produced = out.len() - base;
        match op {
            Op::Literal(lit) => {
                if produced + lit.len() > expected {
                    return Err(bad("block longer than declared"));
                }
                out.extend_from_slice(lit);
            }
            Op::Copy { len, offset } => {
                if offset == 0 || offset > produced || produced + len > expected {
                    return Err(bad("copy outside the block"));
                }
                // Copies may overlap what they write (a run), so go byte by byte.
                let from = out.len() - offset;
                for i in from..from + len {
                    let byte = out.get(i).copied().ok_or_else(|| bad("copy outside the block"))?;
                    out.push(byte);
                }
            }
        }
    }
    if out.len() - base != expected {
        return Err(bad("block shorter than declared"));
    }
    Ok(())
}
