use std::io::Read;

use indexmap::IndexMap;

use crate::{
    card::{Card, CardValue},
    error::{Error, Result},
    io::BlockReader,
};

#[derive(Debug, Default)]
pub struct Header {
    cards: Vec<Card>,
    map: IndexMap<String, Vec<usize>>,
}

impl Header {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, keyword: &str) -> Option<&Card> {
        self.map
            .get(&keyword.to_uppercase())
            .and_then(|indices| self.cards.get(*indices.first()?))
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
            self.map.shift_remove(&keyword);
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

    pub fn read_from_block_reader<R: Read>(reader: &mut BlockReader<R>) -> Result<(Header, u64)> {
        let mut header = Header::new();
        let blocks_before = reader.blocks_read;
        loop {
            let block = reader.read_block().map_err(|e| match e {
                Error::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {
                    Error::InvalidHeader("missing END card before EOF".into())
                }
                other => other,
            })?;
            for record in block.records() {
                let card = Card::try_from(record)?;
                let is_end = card == Card::End;
                if !header.stitch_continue(&card) {
                    header.append(card);
                }
                if is_end {
                    let blocks_consumed = reader.blocks_read - blocks_before;
                    return Ok((header, blocks_consumed));
                }
            }
        }
    }

    /// 4.2.1.2 Continued string (long-string) keywords
    fn stitch_continue(&mut self, card: &Card) -> bool {
        // we hit a CONTINUE card
        if let Card::Continue { value: cont, comment: cont_comment } = card
            // ... and the previous card is a string value
            && let Some(Card::Value {
                value: CardValue::String(s),
                comment,
                ..
            }) = self.cards.last_mut()
            // ... and the string ends with '&'
            && s.ends_with('&')
        {
            s.pop();
            s.push_str(cont); // ... concat the val to the previous string
            // Comments also get continued
            if let Some(cont_comment) = cont_comment {
                match comment {
                    Some(c) if c.ends_with('&') => {
                        c.pop();
                        c.push_str(cont_comment);
                    }
                    Some(c) => {
                        c.push(' ');
                        c.push_str(cont_comment);
                    }
                    None => *comment = Some(cont_comment.clone()),
                }
            }
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use crate::{card::CardValue, io::Block};

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
        assert!(result.is_err());
        assert!(
            matches!(result, Err(Error::InvalidHeader(msg)) if msg.contains("missing END card"))
        );
    }

    #[test]
    fn test_continue_stitching() {
        let cards = vec![
            Card::Value {
                keyword: "LONGSTR".to_string(),
                value: CardValue::String("hello &".to_string()),
                comment: None,
            },
            Card::Continue {
                value: "world".to_string(),
                comment: Some("the comment".to_string()),
            },
            Card::End,
        ];
        let block = make_block(&cards);
        let mut reader = BlockReader::new(block.as_bytes());
        let (header, _) = Header::read_from_block_reader(&mut reader).unwrap();
        assert_eq!(
            header.get("LONGSTR"),
            Some(&Card::Value {
                keyword: "LONGSTR".to_string(),
                value: CardValue::String("hello world".to_string()),
                comment: Some("the comment".to_string()),
            })
        );
    }

    #[test]
    fn test_multiple_continue_stitching() {
        let cards = vec![
            Card::Value {
                keyword: "LONGSTR".to_string(),
                value: CardValue::String("nothing &".to_string()),
                comment: Some("youth".to_string()),
            },
            Card::Continue {
                value: "is &".to_string(),
                comment: Some("is".to_string()),
            },
            Card::Continue {
                value: "permanent".to_string(),
                comment: Some("fleeting".to_string()),
            },
            Card::End,
        ];
        let block = make_block(&cards);
        let mut reader = BlockReader::new(block.as_bytes());
        let (header, _) = Header::read_from_block_reader(&mut reader).unwrap();
        assert_eq!(
            header.get("LONGSTR"),
            Some(&Card::Value {
                keyword: "LONGSTR".to_string(),
                value: CardValue::String("nothing is permanent".to_string()),
                comment: Some("youth is fleeting".to_string()),
            })
        );
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
}
