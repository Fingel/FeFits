use std::{collections::HashMap, io::Read};

use crate::{
    Bitpix,
    card::{Card, CardValue, long_string::stitch_continue},
    error::{Error, Result},
    extension::XtensionType,
    io::{BlockReader, BlockWriter, padded_size},
};

#[derive(Debug, Default)]
pub struct Header {
    cards: Vec<Card>,
    map: HashMap<String, Vec<usize>>,
}

impl Header {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cards(&self) -> impl Iterator<Item = &Card> {
        self.cards.iter()
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn get(&self, keyword: &str) -> Option<&Card> {
        self.map
            .get(&keyword.to_uppercase())
            .and_then(|indices| self.cards.get(*indices.first()?))
    }

    pub fn get_value(&self, keyword: &str) -> Option<&CardValue> {
        self.get(keyword)?.value()
    }

    pub fn find(&self, pattern: &str) -> impl Iterator<Item = &Card> + '_ {
        let pattern = pattern.to_uppercase();
        self.cards
            .iter()
            .filter(move |card| glob_match(pattern.as_bytes(), card.keyword().as_bytes()))
    }

    pub fn append(&mut self, card: Card) {
        let index = self.cards.len();
        self.map
            .entry(card.keyword().to_uppercase())
            .or_default()
            .push(index);
        self.cards.push(card);
    }

    pub fn remove(&mut self, keyword: &str) -> Option<Card> {
        let keyword = keyword.to_uppercase();
        let indices = self.map.get_mut(&keyword)?;
        let index = indices.remove(0);
        if indices.is_empty() {
            self.map.remove(&keyword);
        }
        self.update_indices(index, false);
        Some(self.cards.remove(index))
    }

    pub fn set(&mut self, card: Card) {
        let keyword = card.keyword().to_uppercase();
        if let Some(indices) = self.map.get(&keyword) {
            self.cards[indices[0]] = card;
        } else {
            self.append(card);
        }
    }

    pub fn read_from_block_reader<R: Read>(reader: &mut BlockReader<R>) -> Result<(Header, u64)> {
        let mut header = Header::new();
        let mut blocks_read = 0u64;
        loop {
            let block = match reader.read_block()? {
                Some(b) => b,
                None if blocks_read == 0 => {
                    return Err(Error::Io(std::io::Error::from(
                        std::io::ErrorKind::UnexpectedEof,
                    )));
                }
                None => return Err(Error::InvalidHeader("missing END card before EOF".into())),
            };
            blocks_read += 1;
            for record in block.records() {
                let card = Card::try_from(record)?;
                let is_end = card == Card::End;
                if !stitch_continue(header.cards.last_mut(), &card) {
                    header.append(card);
                }
                if is_end {
                    return Ok((header, blocks_read));
                }
            }
        }
    }

    pub fn write_to_writer<W: std::io::Write>(&self, writer: &mut W) -> Result<u64> {
        let mut bw = BlockWriter::new(writer);
        let mut has_end = false;
        for card in &self.cards {
            if matches!(card, Card::End) {
                has_end = true;
            }
            for record in card.encode_records()? {
                bw.write_record(&record)?;
            }
            if has_end {
                break;
            }
        }
        if !has_end {
            bw.write_record(&Card::End.encode()?)?;
        }
        bw.finish()
    }

    // 4.4.1 Mandatory keywords

    pub fn bitpix(&self) -> Result<Bitpix> {
        match self.get_value("BITPIX") {
            None => Err(Error::MissingKeyword("BITPIX")),
            Some(CardValue::Integer(i)) => (*i).try_into(),
            Some(_) => Err(Error::InvalidKeywordValue {
                keyword: "BITPIX",
                value: "non-integer".into(),
                reason: "must be an integer",
            }),
        }
    }

    pub fn naxis(&self) -> Result<usize> {
        match self.get_value("NAXIS") {
            None => Err(Error::MissingKeyword("NAXIS")),
            // 4.4.1 The value field shall contain a non-negative integer no greater than 999
            Some(CardValue::Integer(i)) if (0..=999).contains(i) => Ok(*i as usize),
            Some(_) => Err(Error::InvalidKeywordValue {
                keyword: "NAXIS",
                value: "non-integer".into(),
                reason: "must be an integer between 0 and 999",
            }),
        }
    }

    /// Image dimension along axis `n`, regardless of whether the HDU is tile-compressed.
    pub fn image_naxisn(&self, n: usize) -> Result<u64> {
        if self.get_value("ZIMAGE").and_then(|v| v.as_bool()) == Some(true) {
            self.znaxisn(n)
        } else {
            self.naxisn(n)
        }
    }

    pub fn naxisn(&self, n: usize) -> Result<u64> {
        let naxis = format!("NAXIS{n}");
        match self.get_value(&naxis) {
            None => Err(Error::InvalidHeader(format!("missing NAXIS{n} keyword"))),
            Some(CardValue::Integer(i)) if *i >= 0 => Ok(*i as u64),
            Some(_) => Err(Error::InvalidHeader(format!(
                "NAXIS{n} value must be a non-negative integer"
            ))),
        }
    }

    pub fn xtension(&self) -> Result<XtensionType> {
        match self.get("XTENSION") {
            None => Err(Error::MissingKeyword("XTENSION")),
            Some(Card::Xtension { x, .. }) => Ok(*x),
            Some(_) => Err(Error::InvalidKeywordValue {
                keyword: "XTENSION",
                value: "non-string".into(),
                reason: "must be a recognized extension type string",
            }),
        }
    }

    // 4.4.2.5 Keywords that describe arrays.

    pub fn bscale(&self) -> Result<f64> {
        match self.get_value("BSCALE") {
            None => Ok(1.0),
            Some(CardValue::Float(f)) => Ok(*f),
            Some(CardValue::Integer(i)) => Ok(*i as f64),
            Some(v) => Err(Error::InvalidKeywordValue {
                keyword: "BSCALE",
                value: format!("{v:?}"),
                reason: "must be numeric",
            }),
        }
    }

    pub fn bzero(&self) -> Result<f64> {
        match self.get_value("BZERO") {
            None => Ok(0.0),
            Some(CardValue::Float(f)) => Ok(*f),
            Some(CardValue::Integer(i)) => Ok(*i as f64),
            Some(v) => Err(Error::InvalidKeywordValue {
                keyword: "BZERO",
                value: format!("{v:?}"),
                reason: "must be numeric",
            }),
        }
    }

    /// 4.4.1.2
    /// Nbits = |BITPIX| × GCOUNT × (PCOUNT + NAXIS1 × NAXIS2 × · · · × NAXISm),
    /// Note that PCOUNT is also the total length in bytes of the supplemtntal data area (heap)
    /// for binary tables with variable-length columns.
    pub fn data_len(&self) -> Result<u64> {
        let naxis_count = self.naxis()?;
        if naxis_count == 0 {
            return Ok(0);
        }
        let naxis_product: u64 =
            (1..=naxis_count).try_fold(1u64, |acc, n| self.naxisn(n).map(|v| acc * v))?;

        let gcount = match self.get_value("GCOUNT") {
            Some(v) => v.as_integer().ok_or_else(|| Error::InvalidKeywordValue {
                keyword: "GCOUNT",
                value: format!("{v:?}"),
                reason: "must be an integer",
            })? as u64, // FITS standard forbids negative GCOUNT/PCOUNT - as u64 is safe(ish)
            None => 1,
        };

        let pcount = match self.get_value("PCOUNT") {
            Some(v) => v.as_integer().ok_or_else(|| Error::InvalidKeywordValue {
                keyword: "PCOUNT",
                value: format!("{v:?}"),
                reason: "must be an integer",
            })? as u64,
            None => 0,
        };

        let unpadded_size = self.bitpix()?.byte_width() as u64 * gcount * (pcount + naxis_product);
        Ok(padded_size(unpadded_size))
    }

    fn update_indices(&mut self, from_idx: usize, increment: bool) {
        let increment: i64 = if increment { 1 } else { -1 };
        for index_sets in self.map.values_mut() {
            for idx in index_sets.iter_mut() {
                if *idx >= from_idx {
                    *idx = (*idx as i64 + increment) as usize;
                }
            }
        }
    }
}

// Wildcard matcher
// empty pattern => text must also be empty
// '*' => matches zero chars (skip it) OR one char (advance text, retry)
// '?' => matches exactly one char
// byte => must equal the current text byte
fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    match pattern[0] {
        b'*' => {
            glob_match(&pattern[1..], text) || (!text.is_empty() && glob_match(pattern, &text[1..]))
        }
        b'?' => !text.is_empty() && glob_match(&pattern[1..], &text[1..]),
        byte => !text.is_empty() && byte == text[0] && glob_match(&pattern[1..], &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use crate::{card::CardValue, io::Block, testutil::*};

    use super::*;

    fn build_card(keyword: &str, value: &str, comment: Option<&str>) -> Card {
        Card::Value {
            keyword: keyword.to_string(),
            value: CardValue::String(value.to_string()),
            comment: comment.map(|c| c.to_string()),
        }
    }

    fn make_block(cards: &[Card]) -> Block {
        let mut block = Block::zeroed();
        for (i, card) in cards.iter().enumerate() {
            block.set_record(i, &card.encode().unwrap());
        }
        block
    }

    #[test]
    fn test_append_remove() {
        let mut header = Header::new();
        let card1 = build_card("KEYWORD1", "VALUE1", Some("Comment1"));
        let card2 = build_card("KEYWORD2", "VALUE2", Some("Comment2"));
        header.append(card1.clone());
        header.append(card2.clone());

        assert_eq!(header.get("KEYWORD1"), Some(&card1));
        assert_eq!(header.get("KEYWORD2"), Some(&card2));

        let removed_card = header.remove("KEYWORD1");
        assert_eq!(removed_card, Some(card1));
        assert_eq!(header.get("KEYWORD1"), None);
        assert_eq!(header.get("KEYWORD2"), Some(&card2));

        let removed_card = header.remove("KEYWORD2");
        assert_eq!(removed_card, Some(card2));
        assert_eq!(header.get("KEYWORD2"), None);
    }

    #[test]
    fn test_multiple_values() {
        let mut header = Header::new();
        let card1 = build_card("KEYWORD", "VALUE1", Some("Comment1"));
        let card2 = build_card("KEYWORD", "VALUE2", Some("Comment2"));
        header.append(card1.clone());
        header.append(card2.clone());

        assert_eq!(header.get("KEYWORD"), Some(&card1));

        let removed_card = header.remove("KEYWORD");
        assert_eq!(removed_card, Some(card1)); // first occurrence
        assert_eq!(header.get("KEYWORD"), Some(&card2));

        let removed_card = header.remove("KEYWORD");
        assert_eq!(removed_card, Some(card2));
        assert_eq!(header.get("KEYWORD"), None);
    }

    #[test]
    fn test_set() {
        let mut header = Header::new();
        let card1 = build_card("KEYWORD", "VALUE1", Some("Comment1"));
        let card2 = build_card("KEYWORD", "VALUE2", Some("Comment2"));
        let card3 = build_card("KEYWORD2", "VALUE3", Some("Comment3"));
        header.set(card1.clone());
        assert_eq!(header.get("KEYWORD"), Some(&card1));

        header.set(card2.clone());
        assert_eq!(header.get("KEYWORD"), Some(&card2));

        // test append
        header.set(card3.clone());
        assert_eq!(header.get("KEYWORD"), Some(&card2));
        assert_eq!(header.get("KEYWORD2"), Some(&card3));
    }

    #[test]
    fn read_from_block_reader() {
        let cards = vec![
            build_card("KEYWORD1", "VALUE1", Some("Comment1")),
            build_card("KEYWORD2", "VALUE2", Some("Comment2")),
            Card::End,
        ];
        let block = make_block(&cards);
        let mut reader = BlockReader::new(block.as_bytes());
        let (header, blocks_consumed) = Header::read_from_block_reader(&mut reader).unwrap();
        assert_eq!(blocks_consumed, 1);
        assert_eq!(header.get("KEYWORD1"), Some(&cards[0]));
        assert_eq!(header.get("KEYWORD2"), Some(&cards[1]));
    }

    #[test]
    /// Trims block to the end of the cards to test EOF handling when END is missing
    fn read_from_block_reader_no_end() {
        let cards = vec![
            build_card("KEYWORD1", "VALUE1", Some("Comment1")),
            build_card("KEYWORD2", "VALUE2", Some("Comment2")),
        ];
        let block = make_block(&cards);
        let card_bytes = cards.len() * 80;
        let mut reader = BlockReader::new(&block.as_bytes()[..card_bytes]);
        let result = Header::read_from_block_reader(&mut reader);
        assert!(matches!(result, Err(Error::InvalidBlock(_))));
    }

    #[test]
    fn test_orphaned_continue() {
        let cards = vec![
            Card::Value {
                keyword: "STRKEY".to_string(),
                value: CardValue::String("no ampersand".to_string()),
                comment: None,
            },
            Card::Continue {
                value: "orphaned".to_string(),
                comment: None,
            },
            Card::End,
        ];
        let block = make_block(&cards);
        let mut reader = BlockReader::new(block.as_bytes());
        let (header, _) = Header::read_from_block_reader(&mut reader).unwrap();
        assert_eq!(
            header.get("STRKEY"),
            Some(&Card::Value {
                keyword: "STRKEY".to_string(),
                value: CardValue::String("no ampersand".to_string()),
                comment: None,
            })
        );
        // orphaned continue gets appended anyway
        assert!(header.get("CONTINUE").is_some());
    }

    #[test]
    fn test_find() {
        let mut header = Header::new();
        header.append(build_card("NAXIS", "0", None));
        header.append(build_card("NAXIS1", "800", None));
        header.append(build_card("NAXIS2", "600", None));
        header.append(build_card("SIMPLE", "T", None));

        let keywords: Vec<_> = header.find("NAX*").map(|c| c.keyword()).collect();
        assert_eq!(keywords, ["NAXIS", "NAXIS1", "NAXIS2"]);

        let keywords: Vec<_> = header.find("NAXIS?").map(|c| c.keyword()).collect();
        assert_eq!(keywords, ["NAXIS1", "NAXIS2"]);

        assert_eq!(header.find("SIMPLE").count(), 1);
        assert_eq!(header.find("MISSING").count(), 0);

        // case insensitive
        let keywords: Vec<_> = header.find("nax*").map(|c| c.keyword()).collect();
        assert_eq!(keywords, ["NAXIS", "NAXIS1", "NAXIS2"]);
    }

    #[test]
    fn test_bitpix_parsing() {
        let mut header = Header::new();
        let card = Card::Value {
            keyword: "BITPIX".to_string(),
            value: CardValue::Integer(16),
            comment: None,
        };
        header.append(card);
        assert_eq!(header.bitpix().unwrap(), Bitpix::SignedShort);

        let card = Card::Value {
            keyword: "BITPIX".to_string(),
            value: CardValue::Integer(-32),
            comment: None,
        };
        header.set(card);
        assert_eq!(header.bitpix().unwrap(), Bitpix::Float);
    }

    #[test]
    fn test_invalid_bitpix() {
        // Missing
        let mut header = Header::new();
        assert!(matches!(
            header.bitpix(),
            Err(Error::MissingKeyword("BITPIX"))
        ));

        // Invalid type
        header.set(build_card("BITPIX", "invalid", None));
        assert!(matches!(
            header.bitpix(),
            Err(Error::InvalidKeywordValue {
                keyword: "BITPIX",
                ..
            })
        ));

        // Invalid type (float)
        let card = Card::Value {
            keyword: "BITPIX".to_string(),
            value: CardValue::Float(64.0),
            comment: None,
        };
        header.set(card);
        assert!(matches!(
            header.bitpix(),
            Err(Error::InvalidKeywordValue {
                keyword: "BITPIX",
                ..
            })
        ));

        // Invalid integer
        let card = Card::Value {
            keyword: "BITPIX".to_string(),
            value: CardValue::Integer(99),
            comment: None,
        };
        header.set(card);
        assert!(matches!(
            header.bitpix(),
            Err(Error::InvalidKeywordValue {
                keyword: "BITPIX",
                ..
            })
        ));
    }

    #[test]
    fn test_naxis() {
        let mut header = Header::new();
        let card = Card::Value {
            keyword: "NAXIS".to_string(),
            value: CardValue::Integer(2),
            comment: None,
        };
        header.append(card);
        assert_eq!(header.naxis().unwrap(), 2);
    }

    #[test]
    fn test_naxis_invalid() {
        let mut header = Header::new();
        assert!(matches!(
            header.naxis(),
            Err(Error::MissingKeyword("NAXIS"))
        ));

        let card = Card::Value {
            keyword: "NAXIS".to_string(),
            value: CardValue::Integer(1000),
            comment: None,
        };
        header.append(card);
        assert!(matches!(
            header.naxis(),
            Err(Error::InvalidKeywordValue {
                keyword: "NAXIS",
                reason: "must be an integer between 0 and 999",
                ..
            })
        ));

        let card = Card::Value {
            keyword: "NAXIS".to_string(),
            value: CardValue::Integer(-1),
            comment: None,
        };
        header.set(card);
        assert!(matches!(
            header.naxis(),
            Err(Error::InvalidKeywordValue {
                keyword: "NAXIS",
                reason: "must be an integer between 0 and 999",
                ..
            })
        ));
    }

    #[test]
    fn test_naxisn() {
        let mut header = Header::new();
        let naxis = Card::Value {
            keyword: "NAXIS".to_string(),
            value: CardValue::Integer(2),
            comment: None,
        };
        let naxis1 = Card::Value {
            keyword: "NAXIS1".to_string(),
            value: CardValue::Integer(800),
            comment: None,
        };
        let naxis2 = Card::Value {
            keyword: "NAXIS2".to_string(),
            value: CardValue::Integer(600),
            comment: None,
        };

        header.append(naxis);
        header.append(naxis1);
        header.append(naxis2);

        assert_eq!(header.naxisn(1).unwrap(), 800);
        assert_eq!(header.naxisn(2).unwrap(), 600);
    }

    #[test]
    fn test_naxisn_invalid() {
        let mut header = Header::new();
        assert!(
            matches!(header.naxisn(1), Err(Error::InvalidHeader(msg)) if msg.contains("missing NAXIS1 keyword"))
        );
        let naxis = Card::Value {
            keyword: "NAXIS".to_string(),
            value: CardValue::Integer(1),
            comment: None,
        };
        let naxis1 = Card::Value {
            keyword: "NAXIS1".to_string(),
            value: CardValue::Integer(-800),
            comment: None,
        };
        header.append(naxis);
        header.append(naxis1);

        assert!(
            matches!(header.naxisn(1), Err(Error::InvalidHeader(msg)) if msg.contains("NAXIS1 value must be a non-negative integer"))
        );
    }

    #[test]
    fn test_xtension() {
        let mut header = Header::new();
        let card = Card::Xtension {
            x: XtensionType::Image,
            comment: None,
        };
        header.append(card);
        assert_eq!(header.xtension().unwrap(), XtensionType::Image);
    }

    #[test]
    fn test_xtension_invalid() {
        let mut header = Header::new();
        assert!(matches!(
            header.xtension(),
            Err(Error::MissingKeyword("XTENSION"))
        ));

        header.append(Card::Value {
            keyword: "XTENSION".to_string(),
            value: CardValue::String("IMAGE".to_string()),
            comment: None,
        });
        assert!(matches!(
            header.xtension(),
            Err(Error::InvalidKeywordValue {
                keyword: "XTENSION",
                reason: "must be a recognized extension type string",
                ..
            })
        ));
    }

    #[test]
    fn write_to_writer_round_trip() {
        let mut header = Header::new();
        header.append(build_card("OBJECT", "Crab Nebula", None));
        header.append(Card::End);

        let mut buf = Vec::new();
        let blocks = header.write_to_writer(&mut buf).unwrap();
        assert_eq!(blocks, 1);
        assert_eq!(buf.len(), 2880);

        let mut reader = BlockReader::new(buf.as_slice());
        let (roundtripped, _) = Header::read_from_block_reader(&mut reader).unwrap();
        assert_eq!(roundtripped.get("OBJECT"), header.get("OBJECT"));
    }

    #[test]
    fn write_to_writer_long_string_round_trip() {
        // 200 'D's with a comment - the encoder produces a trailing CONTINUE to carry the comment.
        let original = Card::Value {
            keyword: "LONGKEY".to_string(),
            value: CardValue::String("D".repeat(200)),
            comment: Some("telescope comment".to_string()),
        };
        let mut header = Header::new();
        header.append(original.clone());
        header.append(Card::End);

        let mut buf = Vec::new();
        header.write_to_writer(&mut buf).unwrap();

        let mut reader = BlockReader::new(buf.as_slice());
        let (roundtripped, _) = Header::read_from_block_reader(&mut reader).unwrap();
        assert_eq!(roundtripped.get("LONGKEY"), Some(&original));
    }

    #[test]
    fn write_to_writer_appends_end() {
        let mut header = Header::new();
        header.append(build_card("SIMPLE", "T", None));
        // no END card

        let mut buf = Vec::new();
        header.write_to_writer(&mut buf).unwrap();

        let mut reader = BlockReader::new(buf.as_slice());
        // missing end would error here
        assert!(Header::read_from_block_reader(&mut reader).is_ok());
    }

    #[test]
    fn test_data_len_naxis_zero() {
        let mut h = Header::new();
        h.append(int_card("NAXIS", 0));
        assert_eq!(h.data_len().unwrap(), 0);
    }

    #[test]
    fn test_data_len_2d_image() {
        let mut h = Header::new();
        h.append(int_card("NAXIS", 2));
        // unpadded = 16/8 * 100 * 50 = 10000, padded = 4 * 2880 = 11520
        h.append(int_card("BITPIX", 16));
        h.append(int_card("NAXIS1", 100));
        h.append(int_card("NAXIS2", 50));
        assert_eq!(h.data_len().unwrap(), 11520);
    }

    #[test]
    fn test_data_len_with_pcount() {
        let mut h = Header::new();
        h.append(int_card("NAXIS", 2));
        // unpadded = 8/8 * 1 * (200 + 100*50) = 5200, padded = 2 * 2880 = 5760
        h.append(int_card("BITPIX", 8));
        h.append(int_card("GCOUNT", 1));
        h.append(int_card("PCOUNT", 200));
        h.append(int_card("NAXIS1", 100));
        h.append(int_card("NAXIS2", 50));
        assert_eq!(h.data_len().unwrap(), 5760);
    }

    #[test]
    fn test_data_len_exactly_one_block() {
        let mut h = Header::new();
        // unpadded = |-32|/8 * 720 = 2880, padded = 2880 (exactly one block)
        h.append(int_card("BITPIX", -32));
        h.append(int_card("NAXIS", 1));
        h.append(int_card("NAXIS1", 720));
        assert_eq!(h.data_len().unwrap(), 2880);
    }
}
