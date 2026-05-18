use crate::{
    Bitpix,
    card::CardValue,
    error::{Error, Result},
    header::Header,
};

/// Compression algorithms. Note that for now only Rice is supported,
/// the other algorithms will parse but are not implemented.
/// 10.1.2
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmpType {
    /// 10.4.1
    Rice,
    /// 10.4.2
    Gzip1,
    /// 10.4.2
    Gzip2,
    /// 10.4.4
    HCompress,
    /// 10.4.3
    Plio,
}

impl CmpType {
    pub fn from_header(h: &Header) -> Result<Self> {
        match h.get_value("ZCMPTYPE") {
            None => Err(Error::MissingKeyword("ZCMPTYPE")),
            Some(CardValue::String(s)) => Self::from_str(s),
            Some(_) => Err(Error::InvalidKeywordValue {
                keyword: "ZCMPTYPE",
                value: "non-string".into(),
                reason: "must be a string",
            }),
        }
    }

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "RICE_1" => Ok(Self::Rice),
            "GZIP_1" => Ok(Self::Gzip1),
            "GZIP_2" => Ok(Self::Gzip2),
            "HCOMPRESS_1" => Ok(Self::HCompress),
            "PLIO_1" => Ok(Self::Plio),
            other => Err(Error::UnsupportedFeature(format!(
                "unknown ZCMPTYPE '{other}'"
            ))),
        }
    }
}

/// Dithering strategy used when quantizing floating-point images to integers.
///
/// From the `ZQUANTIZ` keyword. None means no quantization was applied.
/// 10.2
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuantizeMethod {
    NoDither,
    SubtractiveDither1,
    SubtractiveDither2,
}

impl QuantizeMethod {
    pub fn from_header(h: &Header) -> Result<Option<Self>> {
        match h.get_value("ZQUANTIZ") {
            None => Ok(None),
            Some(CardValue::String(s)) => Self::from_str(s).map(Some),
            Some(_) => Err(Error::InvalidKeywordValue {
                keyword: "ZQUANTIZ",
                value: "non-string".into(),
                reason: "must be a string",
            }),
        }
    }

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "NO_DITHER" => Ok(Self::NoDither),
            "SUBTRACTIVE_DITHER_1" => Ok(Self::SubtractiveDither1),
            "SUBTRACTIVE_DITHER_2" => Ok(Self::SubtractiveDither2),
            other => Err(Error::UnsupportedFeature(format!(
                "unknown ZQUANTIZ '{other}'"
            ))),
        }
    }
}

/// Algorithm-specific tuning parameters
///
/// Only RICE_1 is supported at this time.
/// 10.1.2 (keywords), 10.4 (parameters for algorithms).
#[derive(Debug, Clone, PartialEq)]
pub enum AlgoParams {
    /// 10.4.1, Table 37 Keyword parameters for Rice compression
    Rice {
        block_size: u32,
        byte_pix: u32,
    },
    Other,
}

impl AlgoParams {
    pub fn from_header(h: &Header, cmp: &CmpType) -> Result<Self> {
        match cmp {
            CmpType::Rice => {
                let block_size = h
                    .get_value("ZVAL1")
                    .and_then(|v| v.as_integer())
                    .unwrap_or(32) as u32;
                let byte_pix = h
                    .get_value("ZVAL2")
                    .and_then(|v| v.as_integer())
                    .unwrap_or(4) as u32;
                Ok(AlgoParams::Rice {
                    block_size,
                    byte_pix,
                })
            }
            _ => Ok(AlgoParams::Other),
        }
    }
}

/// Tiling geometry of a tile-compressed image. 10.1.1
///
/// Tiles are rectangular subregions of the image. The last tile along any
/// axis may be smaller than tile_shape if the image dimension is not an
/// exact multiple of the tile size.
///
/// See the tests in this module for a better illustration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileGeometry {
    /// Image dimensions [ZNAXIS1, ZNAXIS2, ...]
    pub image_shape: Vec<u64>,
    /// Tile dimensions [ZTILE1, ZTILE2, ...].
    /// This is the shape of the sub-regions of the image.
    /// "Row by Row"  would be [ZNAXIS1, 1], so rows of width ZNAXIS1 and a height of 1.
    /// A value like [3, 3] would be squares of 3x3 pixels. This is useful for when
    /// only a specific region of the image is needed.
    pub tile_shape: Vec<u64>,
}

impl TileGeometry {
    pub fn from_header(h: &Header) -> Result<Self> {
        let naxis = h.znaxis()?;
        let image_shape: Vec<u64> = (1..=naxis).map(|n| h.znaxisn(n)).collect::<Result<_>>()?;
        let tile_shape: Vec<u64> = (1..=naxis)
            .map(|n| match h.get_value(&format!("ZTILE{n}")) {
                Some(CardValue::Integer(i)) if *i > 0 => Ok(*i as u64),
                Some(_) => Err(Error::InvalidHeader(format!(
                    "ZTILE{n} must be a positive integer"
                ))),
                None => Ok(if n == 1 { image_shape[0] } else { 1 }),
            })
            .collect::<Result<_>>()?;
        Ok(TileGeometry {
            image_shape,
            tile_shape,
        })
    }

    pub fn naxis(&self) -> usize {
        self.image_shape.len()
    }

    /// Number of tiles along axis n (1-based). 10.1.1
    pub fn ntiles_along(&self, n: usize) -> u64 {
        self.image_shape[n - 1].div_ceil(self.tile_shape[n - 1])
    }

    /// Total number of tiles across all axes.
    pub fn total_tiles(&self) -> u64 {
        (1..=self.naxis()).map(|n| self.ntiles_along(n)).product()
    }

    /// Number of pixels in tile `tile_index`, accounting for partial edge tiles.
    ///
    /// `tile_index` is the flat tile index (0..`total_tiles()`), matching the
    /// binary table row number.
    pub fn tile_n_pixels(&self, tile_index: u64) -> usize {
        let mut n = 1usize;
        let mut remaining = tile_index;
        // for dim in image_shape...
        for d in 0..self.image_shape.len() {
            let ntiles_d = self.image_shape[d].div_ceil(self.tile_shape[d]); // num. tiles in this axis
            let td = remaining % ntiles_d;
            remaining /= ntiles_d;
            n *= self.tile_shape[d].min(self.image_shape[d] - td * self.tile_shape[d]) as usize;
        }
        n
    }

    /// Copy `pixels` from tile `tile_index` into the correct positions of `output`.
    ///
    /// `tile_index` is the flat tile index (0..`total_tiles()`), matching the
    /// binary table row number. Tiles are ordered row-major (axis 0 fastest).
    /// Individual pixels are also row-major. Edge tiles may be smaller than
    /// the `tile_shape` but still occupy the same slot.
    pub fn place_tile(&self, pixels: &[i32], tile_index: u64, output: &mut [i32]) {
        let ndim = self.image_shape.len();

        // Step 1: convert tile_index into x, y tile coordinates.
        // Same as numpy's unravel_index(tile_index, ntiles_per_axis, order='F').
        let mut tile_coords = vec![0u64; ndim];
        let mut remaining = tile_index;
        for (d, tile_coord) in tile_coords.iter_mut().enumerate() {
            let ntiles_d = self.image_shape[d].div_ceil(self.tile_shape[d]);
            *tile_coord = remaining % ntiles_d;
            remaining /= ntiles_d;
        }

        // Step 2: actual pixel dimensions of this tile, clamped at image edges.
        let actual_dims: Vec<usize> = (0..ndim)
            .map(|d| {
                self.tile_shape[d].min(self.image_shape[d] - tile_coords[d] * self.tile_shape[d])
                    as usize
            })
            .collect();

        // Step 3: pixel coordinates of the tile's top-left corner in the output.
        let tile_start: Vec<usize> = (0..ndim)
            .map(|d| (tile_coords[d] * self.tile_shape[d]) as usize)
            .collect();

        // Step 4: copy rows. Axis 0 is contiguous in both tile and output (FITS order),
        // so we iterate over every combination of higher-axis coordinates and copy one
        // axis-0 slice at a time.
        //
        // strides[d] = number of output elements to advance when coord[d] increases by 1.
        let mut strides = vec![1usize; ndim];
        for d in 1..ndim {
            strides[d] = strides[d - 1] * self.image_shape[d - 1] as usize;
        }
        // Flat index of the tile's first pixel in output.
        let base: usize = (0..ndim).map(|d| tile_start[d] * strides[d]).sum();
        let row_width = actual_dims[0];
        // Total number of axis-0 rows across all higher dimensions.
        let n_rows: usize = if ndim > 1 {
            actual_dims[1..].iter().product()
        } else {
            1
        };

        for row in 0..n_rows {
            // Unravel `row` into higher-axis coords and compute the output offset.
            let mut rem = row;
            let mut row_offset = 0usize;
            for d in 1..ndim {
                let coord = rem % actual_dims[d];
                rem /= actual_dims[d];
                row_offset += coord * strides[d];
            }
            let out_start = base + row_offset;
            let pix_start = row * row_width;
            output[out_start..out_start + row_width]
                .copy_from_slice(&pixels[pix_start..pix_start + row_width]);
        }
    }
}

/// All compression metadata needed to decompress a tile-compressed image. 10.1
/// This is a collection of all the previous compression related structs and enums
/// defined earlier in this module + bitpix
#[derive(Debug, Clone, PartialEq)]
pub struct CompressionHeader {
    /// Original image pixel type (from ZBITPIX). 10.1.1
    pub bitpix: Bitpix,
    pub cmp_type: CmpType,
    pub algo_params: AlgoParams,
    pub quantize: Option<QuantizeMethod>,
    pub tiles: TileGeometry,
}

impl CompressionHeader {
    pub fn from_header(h: &Header) -> Result<Self> {
        let bitpix = h.zbitpix()?;
        let cmp_type = CmpType::from_header(h)?;
        let algo_params = AlgoParams::from_header(h, &cmp_type)?;
        let quantize = QuantizeMethod::from_header(h)?;
        let tiles = TileGeometry::from_header(h)?;
        Ok(CompressionHeader {
            bitpix,
            cmp_type,
            algo_params,
            quantize,
            tiles,
        })
    }
}

impl Header {
    pub fn zbitpix(&self) -> Result<Bitpix> {
        match self.get_value("ZBITPIX") {
            None => Err(Error::MissingKeyword("ZBITPIX")),
            Some(CardValue::Integer(i)) => {
                Bitpix::try_from(*i).map_err(|_| Error::InvalidKeywordValue {
                    keyword: "ZBITPIX",
                    value: i.to_string(),
                    reason: "must be one of 8, 16, 32, 64, -32, or -64",
                })
            }
            Some(_) => Err(Error::InvalidKeywordValue {
                keyword: "ZBITPIX",
                value: "non-integer".into(),
                reason: "must be an integer",
            }),
        }
    }

    pub fn znaxis(&self) -> Result<usize> {
        match self.get_value("ZNAXIS") {
            None => Err(Error::MissingKeyword("ZNAXIS")),
            Some(CardValue::Integer(i)) if (0..=999).contains(i) => Ok(*i as usize),
            Some(_) => Err(Error::InvalidKeywordValue {
                keyword: "ZNAXIS",
                value: "non-integer".into(),
                reason: "must be an integer between 0 and 999",
            }),
        }
    }

    pub fn znaxisn(&self, n: usize) -> Result<u64> {
        let keyword = format!("ZNAXIS{n}");
        match self.get_value(&keyword) {
            None => Err(Error::InvalidHeader(format!("missing ZNAXIS{n} keyword"))),
            Some(CardValue::Integer(i)) if *i >= 0 => Ok(*i as u64),
            Some(_) => Err(Error::InvalidHeader(format!(
                "ZNAXIS{n} value must be a non-negative integer"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    // --- CmpType ---

    #[test]
    fn test_cmptype_all_known_values() {
        let cases = [
            ("RICE_1", CmpType::Rice),
            ("GZIP_1", CmpType::Gzip1),
            ("GZIP_2", CmpType::Gzip2),
            ("HCOMPRESS_1", CmpType::HCompress),
            ("PLIO_1", CmpType::Plio),
        ];
        for (s, expected) in cases {
            let mut h = Header::new();
            h.set(str_card("ZCMPTYPE", s));
            assert_eq!(CmpType::from_header(&h).unwrap(), expected);
        }
    }

    #[test]
    fn test_cmptype_unknown_returns_unsupported() {
        let mut h = Header::new();
        h.set(str_card("ZCMPTYPE", "WAVELET_2"));
        assert!(matches!(
            CmpType::from_header(&h),
            Err(Error::UnsupportedFeature(_))
        ));
    }

    // --- AlgoParams ---

    #[test]
    fn test_algo_params_rice_defaults() {
        let h = Header::new();
        let params = AlgoParams::from_header(&h, &CmpType::Rice).unwrap();
        assert_eq!(
            params,
            AlgoParams::Rice {
                block_size: 32,
                byte_pix: 4
            }
        );
    }

    #[test]
    fn test_algo_params_rice_explicit() {
        let mut h = Header::new();
        h.set(int_card("ZVAL1", 16));
        h.set(int_card("ZVAL2", 4));
        let params = AlgoParams::from_header(&h, &CmpType::Rice).unwrap();
        assert_eq!(
            params,
            AlgoParams::Rice {
                block_size: 16,
                byte_pix: 4
            }
        );
    }

    #[test]
    fn test_algo_params_none_for_non_rice() {
        let h = Header::new();
        for cmp in [
            CmpType::Gzip1,
            CmpType::Gzip2,
            CmpType::HCompress,
            CmpType::Plio,
        ] {
            assert_eq!(
                AlgoParams::from_header(&h, &cmp).unwrap(),
                AlgoParams::Other
            );
        }
    }

    // --- QuantizeMethod ---

    #[test]
    fn test_quantize_method_all_known_values() {
        let cases = [
            ("NO_DITHER", QuantizeMethod::NoDither),
            ("SUBTRACTIVE_DITHER_1", QuantizeMethod::SubtractiveDither1),
            ("SUBTRACTIVE_DITHER_2", QuantizeMethod::SubtractiveDither2),
        ];
        for (s, expected) in cases {
            let mut h = Header::new();
            h.set(str_card("ZQUANTIZ", s));
            assert_eq!(QuantizeMethod::from_header(&h).unwrap(), Some(expected));
        }
    }

    #[test]
    fn test_quantize_method_absent_is_none() {
        assert_eq!(QuantizeMethod::from_header(&Header::new()).unwrap(), None);
    }

    #[test]
    fn test_quantize_method_unknown_returns_unsupported() {
        let mut h = Header::new();
        h.set(str_card("ZQUANTIZ", "SPECIAL_DITHER"));
        assert!(matches!(
            QuantizeMethod::from_header(&h),
            Err(Error::UnsupportedFeature(_))
        ));
    }

    // --- CompressionHeader ---

    fn full_compression_header() -> Header {
        let mut h = Header::new();
        h.set(int_card("ZBITPIX", 16));
        h.set(str_card("ZCMPTYPE", "RICE_1"));
        h.set(int_card("ZNAXIS", 2));
        h.set(int_card("ZNAXIS1", 100));
        h.set(int_card("ZNAXIS2", 50));
        h
    }

    #[test]
    fn test_compression_header_rice_defaults() {
        let h = full_compression_header();
        let ch = CompressionHeader::from_header(&h).unwrap();

        assert_eq!(ch.bitpix, Bitpix::SignedShort);
        assert_eq!(ch.cmp_type, CmpType::Rice);
        assert_eq!(
            ch.algo_params,
            AlgoParams::Rice {
                block_size: 32,
                byte_pix: 4
            }
        );
        assert_eq!(ch.quantize, None);
        assert_eq!(ch.tiles.image_shape, vec![100, 50]);
        // default row-by-row tiling
        assert_eq!(ch.tiles.tile_shape, vec![100, 1]);
    }

    #[test]
    fn test_compression_header_rice_params() {
        let mut h = full_compression_header();
        // Non-default Rice params: BLOCKSIZE=16, BYTEPIX=2
        h.set(int_card("ZVAL1", 16));
        h.set(int_card("ZVAL2", 2));
        // Non-default tiling: 32x32 pixel blocks
        h.set(int_card("ZTILE1", 32));
        h.set(int_card("ZTILE2", 32));
        let ch = CompressionHeader::from_header(&h).unwrap();

        assert_eq!(
            ch.algo_params,
            AlgoParams::Rice {
                block_size: 16,
                byte_pix: 2
            }
        );
        assert_eq!(ch.tiles.tile_shape, vec![32, 32]);
        // ceil(100/32)=4 tiles across, ceil(50/32)=2 tiles tall = 8 total
        assert_eq!(ch.tiles.total_tiles(), 8);
    }

    #[test]
    fn test_compression_header_missing_zbitpix() {
        let mut h = full_compression_header();
        h.remove("ZBITPIX");
        assert!(matches!(
            CompressionHeader::from_header(&h),
            Err(Error::MissingKeyword("ZBITPIX"))
        ));
    }

    // --- TileGeometry ---

    fn tile_geometry_header(znaxis: i64, axes: &[(i64, Option<i64>)]) -> Header {
        let mut h = Header::new();
        h.set(int_card("ZNAXIS", znaxis));
        for (i, (naxisn, tilen)) in axes.iter().enumerate() {
            let n = i + 1;
            h.set(int_card(&format!("ZNAXIS{n}"), *naxisn));
            if let Some(t) = tilen {
                h.set(int_card(&format!("ZTILE{n}"), *t));
            }
        }
        h
    }

    #[test]
    fn test_tile_geometry_default_row_by_row() {
        // Image: 8px wide (ZNAXIS1), 6px tall (ZNAXIS2).
        // FITS axis order is [width, height] - opposite of what you'd think
        // No ZTILE keywords means default to row-by-row: ZTILE1=ZNAXIS1=8, ZTILE2=1.
        // Each tile is then one full row - 8px wide 1px tall.
        let h = tile_geometry_header(2, &[(8, None), (6, None)]);
        let g = TileGeometry::from_header(&h).unwrap();

        assert_eq!(g.image_shape, vec![8, 6]); // [width, height]
        assert_eq!(g.tile_shape, vec![8, 1]); // [full_width, 1_row_tall]

        // row by row tiling
        assert_eq!(g.ntiles_along(1), 1); // width axis. Image is 8px wide so 1 tile across
        assert_eq!(g.ntiles_along(2), 6); // height axis. Image is 6px tall so 6 tiles tall
        assert_eq!(g.total_tiles(), 6); // just the product of the number of tiles along each axis
    }

    #[test]
    fn test_tile_geometry_square_tiles() {
        // Image: 8px wide, 6px tall. Tiles: 4x3 pixel blocks.
        let h = tile_geometry_header(2, &[(8, Some(4)), (6, Some(3))]);
        let g = TileGeometry::from_header(&h).unwrap();

        assert_eq!(g.image_shape, vec![8, 6]); // still [width, height]
        assert_eq!(g.tile_shape, vec![4, 3]); // non-default [tile_width, tile_height]
        assert_eq!(g.ntiles_along(1), 2); // width axis tiles are 4px wide so 2 tiles (ceil(8/4))
        assert_eq!(g.ntiles_along(2), 2); // height axes tiles are 3px tall so 2 tiles (ceil(6/3))
        assert_eq!(g.total_tiles(), 4); // just the product of the number of tiles along each axis
    }

    #[test]
    fn test_tile_geometry_partial_edge_tiles() {
        // Image not evenly divisible by tile size: the last tile along each
        // axis is smaller than tile_shape but still counts as one tile.
        // Image: 10px wide, 7px tall. Tiles: 3px wide, 3px tall.
        let h = tile_geometry_header(2, &[(10, Some(3)), (7, Some(3))]);
        let g = TileGeometry::from_header(&h).unwrap();

        assert_eq!(g.ntiles_along(1), 4); // width axes tiles are 3px wide so 4 tiles ceil(10/3): last tile only covers 1 pixel
        assert_eq!(g.ntiles_along(2), 3); // height axes tiles are 3px tall so 3 tiles ceil(7/3) : last tile only covers 1 pixel
        assert_eq!(g.total_tiles(), 12); // Still just the product of the number of tiles along each axis even if some contain less pixels
    }

    // ---znaxis* ---

    #[test]
    fn test_znaxisn() {
        let mut header = Header::new();
        header.append(int_card("ZNAXIS", 2));
        header.append(int_card("ZNAXIS1", 100));
        header.append(int_card("ZNAXIS2", 50));

        assert_eq!(header.znaxis().unwrap(), 2);
        assert_eq!(header.znaxisn(1).unwrap(), 100);
        assert_eq!(header.znaxisn(2).unwrap(), 50);
    }

    // --- tile_n_pixels ---

    fn geom(image: &[u64], tile: &[u64]) -> TileGeometry {
        TileGeometry {
            image_shape: image.to_vec(),
            tile_shape: tile.to_vec(),
        }
    }

    #[test]
    fn test_tile_n_pixels_full_tiles() {
        // 8x6 image with 4x3 tiles: no edge tiles, every tile is 4x3=12 pixels.
        let g = geom(&[8, 6], &[4, 3]);
        for t in 0..g.total_tiles() {
            assert_eq!(g.tile_n_pixels(t), 12, "tile {t}");
        }
    }

    #[test]
    fn test_tile_n_pixels_edge_tiles() {
        // 10x7 image with 4x3 tiles (3 wide × 3 tall = 9 tiles):
        //
        //      0    4    8 10
        //      |    |    | |
        //   0  [t0  |t1  |t2]   <- full rows (3 tall), right edge 2 wide
        //   3  [t3  |t4  |t5]
        //   6  [t6  |t7  |t8]   <- bottom edge (1 tall), corner t8 is 2x1
        //   7
        //
        // Tile indices increase along axis 0 (x) first (FITS/Fortran order): t0=(0,0), t1=(1,0), t2=(2,0), ...
        let g = geom(&[10, 7], &[4, 3]);
        assert_eq!(g.tile_n_pixels(0), 12); // t0 (tx=0,ty=0): full 4×3
        assert_eq!(g.tile_n_pixels(2), 6); // t2 (tx=2,ty=0): right edge, 2×3
        assert_eq!(g.tile_n_pixels(6), 4); // t6 (tx=0,ty=2): bottom edge, 4×1
        assert_eq!(g.tile_n_pixels(8), 2); // t8 (tx=2,ty=2): corner, 2×1
    }

    #[test]
    fn test_tile_n_pixels_row_by_row() {
        // Default tiling: each tile is exactly one full row.
        let g = geom(&[100, 50], &[100, 1]);
        for t in 0..50 {
            assert_eq!(g.tile_n_pixels(t), 100);
        }
    }

    // --- place_tile ---

    #[test]
    fn test_place_tile_row_by_row() {
        // 4x3 image, row-by-row tiling: tile t fills row t.
        let g = geom(&[4, 3], &[4, 1]);
        let mut out = vec![0i32; 12];
        g.place_tile(&[1, 2, 3, 4], 0, &mut out);
        g.place_tile(&[5, 6, 7, 8], 1, &mut out);
        g.place_tile(&[9, 10, 11, 12], 2, &mut out);
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn test_place_tile_square() {
        // 4x4 image, 2x2 tiles. Four tiles assemble into the correct 2D layout.
        // Image (row-major, width=4):
        //   row 0: [t0p0, t0p1, t1p0, t1p1]
        //   row 1: [t0p2, t0p3, t1p2, t1p3]
        //   row 2: [t2p0, t2p1, t3p0, t3p1]
        //   row 3: [t2p2, t2p3, t3p2, t3p3]
        let g = geom(&[4, 4], &[2, 2]);
        let mut out = vec![0i32; 16];
        g.place_tile(&[1, 2, 3, 4], 0, &mut out);
        g.place_tile(&[5, 6, 7, 8], 1, &mut out);
        g.place_tile(&[9, 10, 11, 12], 2, &mut out);
        g.place_tile(&[13, 14, 15, 16], 3, &mut out);
        assert_eq!(
            out,
            vec![
                1, 2, 5, 6, // row 0
                3, 4, 7, 8, // row 1
                9, 10, 13, 14, // row 2
                11, 12, 15, 16 // row 3
            ]
        );
    }

    #[test]
    fn test_place_tile_edge() {
        // 6x4 image, 4x3 tiles: right and bottom edge tiles are partial.
        // Tile 3 is the corner (tx=1,ty=1): 2 wide x 1 tall, goes to (x=4..6, y=3).
        let g = geom(&[6, 4], &[4, 3]);
        let mut out = vec![0i32; 24];
        g.place_tile(&[100, 200], 3, &mut out);
        // (x=4, y=3) -> index 3*6+4=22; (x=5, y=3) -> index 23
        assert_eq!(out[22], 100);
        assert_eq!(out[23], 200);
        // All other pixels untouched
        assert!(out[..22].iter().all(|&v| v == 0));
        assert!(out[24..].iter().all(|&v| v == 0));
    }
}
