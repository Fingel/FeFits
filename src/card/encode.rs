use std::fmt;

use crate::{
    card::{Card, CardValue, long_string::split_long_string},
    error::{Error, Result},
    extension::XtensionType,
};

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.encode() {
            Ok(bytes) => {
                let s = std::str::from_utf8(&bytes).expect("encode always produces ASCII");
                write!(f, "{}", s.trim_end())
            }
            Err(e) => write!(f, "<invalid card: {e}>"),
        }
    }
}

impl Card {
    pub fn encode(&self) -> Result<[u8; 80]> {
        match self {
            Card::End => Ok(encode_end()),
            Card::Blank => Ok(encode_blank()),
            Card::Comment(s) => Ok(encode_comment(s)),
            Card::History(s) => Ok(encode_history(s)),
            Card::Value {
                keyword,
                value,
                comment,
            } => encode_value(keyword, value, comment),
            Card::Xtension { x, comment } => encode_xtension(x, comment),
            Card::Continue { value, comment } => encode_continue(value, comment),
        }
    }

    /// Encodes into one or more 80-byte records. This is necessary to support CONTINUE cards
    pub fn encode_records(&self) -> Result<Vec<[u8; 80]>> {
        if let Card::Value {
            keyword,
            value: CardValue::String(s),
            comment,
        } = self
        {
            return split_long_string(keyword, s, comment);
        }
        Ok(vec![self.encode()?])
    }
}

fn encode_end() -> [u8; 80] {
    let mut bytes = [b' '; 80];
    bytes[..3].copy_from_slice(b"END");
    bytes
}

fn encode_blank() -> [u8; 80] {
    [b' '; 80]
}

fn encode_comment(s: &str) -> [u8; 80] {
    let mut bytes = [b' '; 80];
    bytes[..8].copy_from_slice(b"COMMENT ");
    let len = s.len().min(72);
    bytes[8..8 + len].copy_from_slice(&s.as_bytes()[..len]);
    bytes
}

fn encode_history(s: &str) -> [u8; 80] {
    let mut bytes = [b' '; 80];
    bytes[..8].copy_from_slice(b"HISTORY ");
    let len = s.len().min(72);
    bytes[8..8 + len].copy_from_slice(&s.as_bytes()[..len]);
    bytes
}

fn encode_value(keyword: &str, value: &CardValue, comment: &Option<String>) -> Result<[u8; 80]> {
    let kw_len = keyword.len();
    if kw_len > 8 {
        return Err(Error::InvalidCard(format!(
            "keyword too long for card: '{keyword}'"
        )));
    }
    let v = format_value(value)?;
    if v.len() > 70 {
        return Err(Error::InvalidCard(format!(
            "encoded value too long for card: {v}"
        )));
    }
    let mut bytes = [b' '; 80];
    bytes[..kw_len].copy_from_slice(&keyword.as_bytes()[..kw_len]);
    bytes[8] = b'=';
    bytes[9] = b' ';
    let combined = match comment {
        Some(c) => format!("{v} / {c}"),
        None => v,
    };
    let len = combined.len().min(70);
    bytes[10..10 + len].copy_from_slice(&combined.as_bytes()[..len]);
    Ok(bytes)
}

fn encode_continue(value: &str, comment: &Option<String>) -> Result<[u8; 80]> {
    let escaped = value.replace('\'', "''");
    let quoted = format!("'{escaped}'");
    if quoted.len() > 71 {
        return Err(Error::InvalidCard(
            "encoded continue value too long for card".into(),
        ));
    }
    let combined = match comment {
        Some(c) => format!("{quoted} / {c}"),
        None => quoted,
    };
    let mut bytes = [b' '; 80];
    bytes[..9].copy_from_slice(b"CONTINUE ");
    let len = combined.len().min(71);
    bytes[9..9 + len].copy_from_slice(&combined.as_bytes()[..len]);
    Ok(bytes)
}

fn encode_xtension(x: &XtensionType, comment: &Option<String>) -> Result<[u8; 80]> {
    encode_value("XTENSION", &CardValue::String(x.to_string()), comment)
}

fn format_value(value: &CardValue) -> Result<String> {
    let result = match value {
        CardValue::String(s) => {
            let escaped = s.replace('\'', "''");
            format!("'{escaped:<8}'") // minimum 8-char content per spec
        }
        CardValue::Integer(n) => format!("{n:>20}"),
        CardValue::Float(f) => format_float(*f)?,
        CardValue::Logical(b) => format!("{:>20}", if *b { "T" } else { "F" }),
        CardValue::Undefined => String::new(),
        CardValue::ComplexInteger(a, b) => format!("({a}, {b})"),
        CardValue::ComplexFloat(a, b) => {
            let a = format_float(*a)?;
            let b = format_float(*b)?;
            format!("({a}, {b})")
        }
    };
    Ok(result)
}

fn format_float(f: f64) -> Result<String> {
    // We are using ryu here for both the round-trip guarantee and also
    // for the formatting heuristics that produce a more compact representation.
    // The fact that it might be slightly faster is a bonus.
    if !f.is_finite() {
        return Err(Error::InvalidCard(format!("non-finite float value: {f}")));
    }
    let mut buf = ryu::Buffer::new();
    Ok(buf.format_finite(f).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_blank(bytes: &[u8]) -> bool {
        bytes.iter().all(|&b| b == b' ')
    }

    #[test]
    fn test_encode_end() {
        let card = Card::End;
        let encoded = card.encode().unwrap();
        assert_eq!(&encoded[..3], b"END");
        assert!(is_blank(&encoded[3..]));
    }

    #[test]
    fn test_encode_blank() {
        let card = Card::Blank;
        let encoded = card.encode().unwrap();
        assert_eq!(encoded, [b' '; 80]);
    }

    #[test]
    fn test_comment() {
        let text = "Comment your code and data";
        let card = Card::Comment(text.to_string());
        let encoded = card.encode().unwrap();
        assert_eq!(&encoded[..8], b"COMMENT ");
        assert_eq!(&encoded[8..8 + text.len()], text.as_bytes());
        assert!(is_blank(&encoded[8 + text.len()..]));
    }

    #[test]
    fn test_history() {
        let text = "History teaches us";
        let card = Card::History(text.to_string());
        let encoded = card.encode().unwrap();
        assert_eq!(&encoded[..8], b"HISTORY ");
        assert_eq!(&encoded[8..8 + text.len()], text.as_bytes());
        assert!(is_blank(&encoded[8 + text.len()..]));
    }

    #[test]
    fn test_value_string() {
        let card = Card::Value {
            keyword: "OBJECT".to_string(),
            value: CardValue::String("Dumbbell Nebula".to_string()),
            comment: None,
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"OBJECT  = 'Dumbbell Nebula'"));
    }

    #[test]
    fn test_value_string_short() {
        let card = Card::Value {
            keyword: "FILTER".to_string(),
            value: CardValue::String("R".to_string()),
            comment: Some("Value padded to 8 chars".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"FILTER  = 'R       ' / Value padded to 8 chars"));
    }

    #[test]
    fn test_value_string_too_long() {
        let card = Card::Value {
            keyword: "LONG".to_string(),
            value: CardValue::String("a".repeat(100)),
            comment: None,
        };
        let encoded = card.encode();
        assert!(encoded.is_err());
        assert!(matches!(
            encoded.err().unwrap(),
            Error::InvalidCard(msg) if msg.contains("encoded value too long")
        ));
    }

    #[test]
    fn test_value_string_escaped() {
        let card = Card::Value {
            keyword: "OBJECT".to_string(),
            value: CardValue::String("D'umbbell Nebula".to_string()),
            comment: None,
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"OBJECT  = 'D''umbbell Nebula'"));
    }

    #[test]
    fn test_value_string_comment() {
        let card = Card::Value {
            keyword: "OBJECT".to_string(),
            value: CardValue::String("Dumbbell Nebula".to_string()),
            comment: Some("Name of object observed".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"OBJECT  = 'Dumbbell Nebula' / Name of object observed"));
    }

    #[test]
    fn test_value_string_comment_truncated() {
        let mut to_be_trunc = String::from("Name of object observed");
        to_be_trunc.extend(std::iter::repeat_n('a', 100));
        let card = Card::Value {
            keyword: "OBJECT".to_string(),
            value: CardValue::String("Dumbbell Nebula".to_string()),
            comment: Some(to_be_trunc),
        };
        let encoded = card.encode().unwrap();
        let mut expected = String::from("OBJECT  = 'Dumbbell Nebula' / Name of object observed");
        let fill_len = 80 - expected.len();
        expected.extend(std::iter::repeat_n('a', fill_len));
        assert!(encoded.len() == 80);
        assert_eq!(encoded, expected.as_bytes());
    }

    #[test]
    fn test_value_integer() {
        let card = Card::Value {
            keyword: "NAXIS1".to_string(),
            value: CardValue::Integer(650),
            comment: Some("Width of table row in bytes".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(
            &encoded.starts_with(b"NAXIS1  =                  650 / Width of table row in bytes")
        );
    }

    #[test]
    fn test_value_logical() {
        let card = Card::Value {
            keyword: "DETECT".to_string(),
            value: CardValue::Logical(true),
            comment: Some("Object detected in image".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"DETECT  =                    T / Object detected in image"));
    }

    #[test]
    fn test_value_float() {
        let card = Card::Value {
            keyword: "EXPTIME".to_string(),
            value: CardValue::Float(60.0),
            comment: Some("Exposure time in seconds".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"EXPTIME = 60.0 / Exposure time in seconds"));
    }

    #[test]
    fn test_value_float_e() {
        let card = Card::Value {
            keyword: "EXPTIME".to_string(),
            value: CardValue::Float(60.1234e-3),
            comment: Some("Exposure time in seconds".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"EXPTIME = 0.0601234 / Exposure time in seconds"));
    }

    #[test]
    fn test_value_float_large() {
        let card = Card::Value {
            keyword: "EXPTIME".to_string(),
            value: CardValue::Float(6.01234e16),
            comment: Some("Exposure time in seconds".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"EXPTIME = 6.01234e16 / Exposure time in seconds"));
    }

    #[test]
    fn test_value_float_comment_truncated() {
        let f = f64::MAX;
        let float_str = format_float(f).unwrap();
        let card = Card::Value {
            keyword: "CRVAL1".to_string(),
            value: CardValue::Float(f),
            comment: Some("a".repeat(100)),
        };
        let encoded = card.encode().unwrap();
        let prefix = format!("CRVAL1  = {float_str} / ");
        let comment_space = 80 - prefix.len();
        let mut expected = prefix;
        expected.extend(std::iter::repeat_n('a', comment_space));
        assert_eq!(encoded.len(), 80);
        assert_eq!(&encoded, expected.as_bytes());
    }

    #[test]
    fn test_non_finite_float() {
        let card = Card::Value {
            keyword: "EXPTIME".to_string(),
            value: CardValue::Float(f64::NAN),
            comment: None,
        };
        let result = card.encode();
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            Error::InvalidCard(msg) if msg.contains("non-finite float value")
        ));
    }

    #[test]
    fn test_value_complex_int() {
        let card = Card::Value {
            keyword: "COMPLEX".to_string(),
            value: CardValue::ComplexInteger(1, -1),
            comment: None,
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"COMPLEX = (1, -1)"));
    }

    #[test]
    fn test_value_complex_float() {
        let card = Card::Value {
            keyword: "COMPLEX".to_string(),
            value: CardValue::ComplexFloat(1.234, -0.4321),
            comment: None,
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"COMPLEX = (1.234, -0.4321)"));
    }

    #[test]
    fn test_value_undefined() {
        let card = Card::Value {
            keyword: "UNDEF".to_string(),
            value: CardValue::Undefined,
            comment: None,
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"UNDEF   =                    "));
    }

    #[test]
    fn test_xtension() {
        let card = Card::Xtension {
            x: XtensionType::Image,
            comment: None,
        };
        let encoded = card.encode().unwrap();
        assert!(&encoded.starts_with(b"XTENSION= 'IMAGE   '"));
    }

    #[test]
    fn test_continue() {
        let card = Card::Continue {
            value: "...and that's how I lost $1m".to_string(),
            comment: Some("continued string".to_string()),
        };
        let encoded = card.encode().unwrap();
        assert!(
            &encoded.starts_with(b"CONTINUE '...and that''s how I lost $1m' / continued string")
        );
    }
}
