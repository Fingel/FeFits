use crate::card::{Card, CardValue};

pub fn int_card(keyword: &str, value: i64) -> Card {
    Card::Value {
        keyword: keyword.to_string(),
        value: CardValue::Integer(value),
        comment: None,
    }
}

pub fn str_card(keyword: &str, value: &str) -> Card {
    Card::Value {
        keyword: keyword.to_string(),
        value: CardValue::String(value.to_string()),
        comment: None,
    }
}

pub fn float_card(keyword: &str, value: f64) -> Card {
    Card::Value {
        keyword: keyword.to_string(),
        value: CardValue::Float(value),
        comment: None,
    }
}

pub fn bool_card(keyword: &str, value: bool) -> Card {
    Card::Value {
        keyword: keyword.to_string(),
        value: CardValue::Logical(value),
        comment: None,
    }
}
