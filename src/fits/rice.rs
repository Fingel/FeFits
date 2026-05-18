// Adapted from cfitsio/ricecomp.c:
//   fits_rdecomp        (i32, byte_pix=4) L868–L1037
//   fits_rdecomp_short  (i16, byte_pix=2) L1041–L1206
//   fits_rdecomp_byte   (u8,  byte_pix=1) L1208–L1371

use crate::error::{Error, Result};

// Number of bits in a 8-bit values excluding leading zeros
// cfitsio: ricecomp.c:L38–L58
#[rustfmt::skip]
const NONZERO_COUNT: [u32; 256] = [
    0,
    1,
    2, 2,
    3, 3, 3, 3,
    4, 4, 4, 4, 4, 4, 4, 4,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
    6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8
];

/// Decompress a Rice-encoded tile from a FITS binary table heap.
///
/// Returns decoded pixel values as `i32`. Callers should cast to the final pixel
/// type based on `ZBITPIX`:
/// - `byte_pix=1` (ZBITPIX=8): values are unsigned `u8` range (0–255).
/// - `byte_pix=2` (ZBITPIX=16): values are sign-extended from 16 bits.
/// - `byte_pix=4` (ZBITPIX=32): values are the full `i32` bit pattern.
///
/// # Parameters
/// - `input`: compressed bytes from the heap (output of `read_heap_bytes`)
/// - `n_pixels`: number of pixels to decode (tile size)
/// - `block_size`: Rice coding block size from `ZVAL1` (default 32)
/// - `byte_pix`: pixel byte width from `ZVAL2` (default 4); must be 1, 2, or 4
pub fn rice_decompress(
    input: &[u8],
    n_pixels: usize,
    block_size: u32,
    byte_pix: u32,
) -> Result<Vec<i32>> {
    // fsbits/fsmax/bbits per pixel size.
    // cfitsio: ricecomp.c:L919–L922
    let (fsbits, fsmax): (u32, u32) = match byte_pix {
        1 => (3, 6),
        2 => (4, 14),
        4 => (5, 25),
        _ => {
            return Err(Error::InvalidHDU(format!(
                "rice: byte_pix must be 1, 2, or 4, got {byte_pix}"
            )));
        }
    };
    let bbits = 1u32 << fsbits;
    let pixel_bits = byte_pix * 8;
    let pixel_mask: u32 = if pixel_bits >= 32 {
        u32::MAX
    } else {
        (1u32 << pixel_bits) - 1
    };

    if n_pixels == 0 {
        return Ok(Vec::new());
    }

    if input.len() < byte_pix as usize + 1 {
        return Err(Error::InvalidHDU(format!(
            "rice: input too short ({} bytes) for {byte_pix}-byte pixels",
            input.len()
        )));
    }

    // First byte_pix bytes encode the first pixel value, big-endian.
    // cfitsio: ricecomp.c:L929–L944
    let mut lastpix: u32 = 0;
    for &byte in &input[..byte_pix as usize] {
        lastpix = (lastpix << 8) | (byte as u32);
    }
    lastpix &= pixel_mask;

    let mut c = byte_pix as usize;
    let mut b: u32 = input[c] as u32; // bit buffer
    c += 1;
    let mut nbits: i32 = 8; // valid bits remaining in b

    let mut output = vec![0i32; n_pixels];
    let mut i: usize = 0;

    macro_rules! next_byte {
        () => {{
            if c >= input.len() {
                return Err(Error::InvalidHDU(
                    "rice: unexpected end of compressed data".into(),
                ));
            }
            let v = input[c] as u32;
            c += 1;
            v
        }};
    }

    while i < n_pixels {
        // Read fsbits to get the FS selector for this block.
        // cfitsio: ricecomp.c:L953–L960
        nbits -= fsbits as i32;
        while nbits < 0 {
            b = (b << 8) | next_byte!();
            nbits += 8;
        }
        let fs = (b >> nbits) as i32 - 1;
        b &= (1 << nbits) - 1;

        let imax = (i + block_size as usize).min(n_pixels);

        if fs < 0 {
            // Low-entropy: all differences are zero: repeat the last pixel.
            // cfitsio: ricecomp.c:L964–L966
            while i < imax {
                output[i] = pixel_to_i32(lastpix, byte_pix);
                i += 1;
            }
        } else if fs as u32 == fsmax {
            // High-entropy: pixels are stored as raw bbits-wide values.
            // cfitsio: ricecomp.c:L967–L996
            while i < imax {
                let k = bbits as i32 - nbits;
                // wrapping_shl handles k==32 (only when nbits==0 and b==0 anyway).
                let mut diff: u32 = b.wrapping_shl(k as u32);
                let mut k = k - 8;
                while k >= 0 {
                    b = next_byte!();
                    diff |= b << k as u32;
                    k -= 8;
                }
                if nbits > 0 {
                    b = next_byte!();
                    diff |= b >> (-k) as u32;
                    b &= (1 << nbits) - 1;
                } else {
                    b = 0;
                }
                // cfitsio: ricecomp.c:L984–L993
                diff = undo_mapping(diff);
                let curpix = diff.wrapping_add(lastpix) & pixel_mask;
                output[i] = pixel_to_i32(curpix, byte_pix);
                lastpix = curpix;
                i += 1;
            }
        } else {
            // Golomb-Rice coding: unary leading-zero count + fs binary trailing bits.
            // cfitsio: ricecomp.c:L998–L1026
            let fs = fs as u32;
            while i < imax {
                // Count leading zero bits by consuming zero bytes until b != 0.
                // cfitsio: ricecomp.c:L1000–L1008
                while b == 0 {
                    nbits += 8;
                    b = next_byte!();
                }
                let nzero = nbits as u32 - NONZERO_COUNT[b as usize];
                nbits -= (nzero + 1) as i32;
                b ^= 1 << nbits; // clear the leading one-bit
                // Read fs trailing bits.
                // cfitsio: ricecomp.c:L1010–L1016
                nbits -= fs as i32;
                while nbits < 0 {
                    b = (b << 8) | next_byte!();
                    nbits += 8;
                }
                let mut diff: u32 = (nzero << fs) | (b >> nbits as u32);
                b &= (1 << nbits) - 1;
                // cfitsio: ricecomp.c:L1018–L1025
                diff = undo_mapping(diff);
                let curpix = diff.wrapping_add(lastpix) & pixel_mask;
                output[i] = pixel_to_i32(curpix, byte_pix);
                lastpix = curpix;
                i += 1;
            }
        }
    }

    Ok(output)
}

// Sign-folding inverse: even mapped values are positive diffs, odd are negative.
// cfitsio: ricecomp.c:L989–L993 (high-entropy), L1018–L1022 (Rice)
#[inline]
fn undo_mapping(diff: u32) -> u32 {
    if (diff & 1) == 0 {
        diff >> 1
    } else {
        !(diff >> 1)
    }
}

#[inline]
fn pixel_to_i32(val: u32, byte_pix: u32) -> i32 {
    match byte_pix {
        1 => (val & 0xFF) as i32,
        2 => (val & 0xFFFF) as i16 as i32,
        _ => val as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_entropy_identical_pixels() {
        // pixels = [100, 100, 100, 100]: all zero differences → fs = -1 (low-entropy).
        // Compressed: first pixel (4 bytes) + 5 zero FS bits padded to 1 byte.
        let input = [0x00u8, 0x00, 0x00, 0x64, 0x00];
        assert_eq!(
            rice_decompress(&input, 4, 4, 4).unwrap(),
            vec![100, 100, 100, 100]
        );
    }

    #[test]
    fn test_rice_sequential_increasing() {
        // pixels = [100, 101, 102, 103]: diffs all +1, mapped to 2.
        // fs=0, Rice-coded as (0 leading zeros + 1) per pixel.
        let input = [0x00u8, 0x00, 0x00, 0x64, 0x0C, 0x92];
        assert_eq!(
            rice_decompress(&input, 4, 4, 4).unwrap(),
            vec![100, 101, 102, 103]
        );
    }

    #[test]
    fn test_rice_sequential_decreasing() {
        // pixels = [100, 97, 94, 91]: diffs all -3, mapped to 5.
        // fs=1, Rice-coded with 1 trailing bit per pixel.
        let input = [0x00u8, 0x00, 0x00, 0x64, 0x14, 0x66, 0x60];
        assert_eq!(
            rice_decompress(&input, 4, 4, 4).unwrap(),
            vec![100, 97, 94, 91]
        );
    }

    #[test]
    fn test_multi_block() {
        // 8 identical pixels across 2 blocks (block_size=4), both low-entropy.
        let input = [0x00u8, 0x00, 0x00, 0x64, 0x00, 0x00];
        assert_eq!(
            rice_decompress(&input, 8, 4, 4).unwrap(),
            vec![100, 100, 100, 100, 100, 100, 100, 100]
        );
    }

    #[test]
    fn test_single_pixel() {
        // A tile with exactly one pixel: only the literal first pixel + 1 FS byte.
        let input = [0x00u8, 0x00, 0x00, 0x64, 0x00];
        assert_eq!(rice_decompress(&input, 1, 4, 4).unwrap(), vec![100]);
    }

    #[test]
    fn test_empty_returns_empty() {
        assert_eq!(rice_decompress(&[], 0, 32, 4).unwrap(), vec![]);
    }

    #[test]
    fn test_input_too_short_returns_error() {
        // 4-byte input, but byte_pix=4 requires at least 5 bytes (first pixel + bit buffer).
        assert!(matches!(
            rice_decompress(&[0, 0, 0, 100], 1, 4, 4),
            Err(crate::error::Error::InvalidHDU(_))
        ));
    }

    #[test]
    fn test_invalid_byte_pix_returns_error() {
        assert!(matches!(
            rice_decompress(&[0; 8], 1, 4, 3),
            Err(crate::error::Error::InvalidHDU(_))
        ));
    }
}
