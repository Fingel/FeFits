use crate::{
    card::CardValue,
    error::{Error, Result},
    header::Header,
};

/// Compression algorithms. Note that for now only Rice is supported,
/// the other algorithms will parse but are not implemented.
/// Spec H.1–H.5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmpType {
    Rice,
    Gzip1,
    Gzip2,
    HCompress,
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
