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
}
