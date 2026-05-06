use super::Card;
use crate::error::Result;

impl Card {
    pub fn encode(&self) -> Result<[u8; 80]> {
        match self {
            Card::End => Ok(encode_end()),
            Card::Blank => Ok(encode_blank()),
            Card::Comment(s) => Ok(encode_comment(s)),
            _ => unimplemented!(),
        }
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
}
