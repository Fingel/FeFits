use crate::{
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
    None,
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
            _ => Ok(AlgoParams::None),
        }
    }
}

impl Header {
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
    use crate::card::{Card, CardValue};

    use super::*;

    fn str_card(keyword: &str, value: &str) -> Card {
        Card::Value {
            keyword: keyword.to_string(),
            value: CardValue::String(value.to_string()),
            comment: None,
        }
    }

    fn int_card(keyword: &str, value: i64) -> Card {
        Card::Value {
            keyword: keyword.to_string(),
            value: CardValue::Integer(value),
            comment: None,
        }
    }

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
            assert_eq!(AlgoParams::from_header(&h, &cmp).unwrap(), AlgoParams::None);
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

    // ---znaxis* ---

    #[test]
    fn test_znaxisn() {
        let mut header = Header::new();
        let znaxis = Card::Value {
            keyword: "ZNAXIS".to_string(),
            value: CardValue::Integer(2),
            comment: None,
        };
        let znaxis1 = Card::Value {
            keyword: "ZNAXIS1".to_string(),
            value: CardValue::Integer(100),
            comment: None,
        };
        let znaxis2 = Card::Value {
            keyword: "ZNAXIS2".to_string(),
            value: CardValue::Integer(50),
            comment: None,
        };

        header.append(znaxis);
        header.append(znaxis1);
        header.append(znaxis2);

        assert_eq!(header.znaxis().unwrap(), 2);
        assert_eq!(header.znaxisn(1).unwrap(), 100);
        assert_eq!(header.znaxisn(2).unwrap(), 50);
    }
}
