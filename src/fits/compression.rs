use crate::{
    card::CardValue,
    error::{Error, Result},
    header::Header,
};

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
    use crate::card::Card;

    use super::*;

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
