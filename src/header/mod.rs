use indexmap::IndexMap;

use crate::card::Card;

#[derive(Debug, Default)]
pub struct Header {
    cards: Vec<Card>,
    map: IndexMap<String, Vec<usize>>,
}

impl Header {
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
}

#[cfg(test)]
mod tests {
    use crate::card::CardValue;

    use super::*;

    fn build_card(keyword: &str, value: &str, comment: Option<&str>) -> Card {
        Card::Value {
            keyword: keyword.to_string(),
            value: CardValue::String(value.to_string()),
            comment: comment.map(|c| c.to_string()),
        }
    }

    #[test]
    fn test_append_remove() {
        let mut header = Header::default();
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
        let mut header = Header::default();
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
        let mut header = Header::default();
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
}
