use crate::error::Result;

use super::{Card, CardValue};

// encoding side - splits a long Value string into a Value card followed by n CONTINUE cards

/// 4.2.1.2 splits a string value into one or more records.
///
/// If the escaped string fits in a single record, returns a single Value card.
/// Otherwise the value is split across a Value card + however many CONTINUE cards are
/// necessary to fit the value.
///
/// The comment is placed only on the last record. If the final value chunk is
/// too large to share a CONTINUE card with the comment, a trailing CONTINUE card
/// is appended that carries only the comment.
pub(super) fn split_long_string(
    keyword: &str,
    s: &str,
    comment: &Option<String>,
) -> Result<Vec<[u8; 80]>> {
    // No need to split
    if escaped_len(s) <= 68 {
        return Ok(vec![
            Card::Value {
                keyword: keyword.to_string(),
                value: CardValue::String(s.to_string()),
                comment: comment.clone(),
            }
            .encode()?,
        ]);
    }

    let mut records = Vec::new();

    // First card: value field is 70 bytes, '<content>&' = 3 overhead:  67 🤷 escaped chars max.
    let end = escape_split_index(s, 67);
    records.push(
        Card::Value {
            keyword: keyword.to_string(),
            value: CardValue::String(format!("{}&", &s[..end])),
            comment: None,
        }
        .encode()?,
    );
    let mut remaining = &s[end..];

    // CONTINUE: 71 bytes, non-last: '<content>&' = 3 overhead = 68 total. last: (no &) = 69 (nice)
    // When a comment is present, the last card's max is reduced to make room.
    let max_last = comment_max_last(comment);
    let mut comment_written = false;

    while !remaining.is_empty() {
        let is_last = escaped_len(remaining) <= max_last;
        let max = if is_last { max_last } else { 68 };
        let end = escape_split_index(remaining, max);
        let chunk = &remaining[..end];
        let record = if is_last {
            comment_written = true;
            Card::Continue {
                value: chunk.to_string(),
                comment: comment.clone(),
            }
            .encode()?
        } else {
            Card::Continue {
                value: format!("{chunk}&"),
                comment: None,
            }
            .encode()?
        };
        records.push(record);
        remaining = &remaining[end..];
    }

    if comment.is_some() && !comment_written {
        records.push(
            Card::Continue {
                value: String::new(),
                comment: comment.clone(),
            }
            .encode()?,
        );
    }

    Ok(records)
}

/// Escaped string length: raw length plus one extra byte per `'` that must be doubled.
fn escaped_len(s: &str) -> usize {
    s.len() + s.bytes().filter(|&b| b == b'\'').count()
}

/// Returns the byte index at which to split `s` so its escaped prefix fits within `max` bytes.
fn escape_split_index(s: &str, max: usize) -> usize {
    let mut esc = 0usize;
    for (i, b) in s.bytes().enumerate() {
        let w = if b == b'\'' { 2 } else { 1 };
        if esc + w > max {
            return i;
        }
        esc += w;
    }
    s.len()
}

/// Maximum escaped-char count for the last CONTINUE card, leaving room for the comment.
fn comment_max_last(comment: &Option<String>) -> usize {
    let overhead = comment.as_deref().map_or(0, |c| 3 + c.len()); // " / " + comment text
    69_usize.saturating_sub(overhead)
}

// Decoding

/// Attempts to stitch a CONTINUE card onto the prev Card's string value to create a single
/// value card with a length that can exceed the standard 80 character width.
pub(crate) fn stitch_continue(prev: Option<&mut Card>, current: &Card) -> bool {
    if let Card::Continue {
        value: cont,
        comment: cont_comment,
    } = current
        && let Some(Card::Value {
            value: CardValue::String(s),
            comment,
            ..
        }) = prev
        && s.ends_with('&')
    {
        s.pop();
        s.push_str(cont);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_string_single_record() {
        let card = Card::Value {
            keyword: "OBJECT".to_string(),
            value: CardValue::String("Crab Nebula".to_string()),
            comment: None,
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], card.encode().unwrap());
    }

    #[test]
    fn test_at_68_limit_no_split() {
        // Exactly 68 escaped chars: sits at the <= 68 guard so no split
        let s = "A".repeat(68);
        let card = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String(s),
            comment: None,
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].starts_with(b"KEY"));
    }

    #[test]
    fn test_at_68_limit_quotes_no_split() {
        // 34 single quotes = 34 * 2 = 68, still one record
        let s = "'".repeat(34);
        let card = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String(s),
            comment: None,
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_69_chars_splits_two_records() {
        // 69 chars just over the 68-escaped-char threshold for one card
        let s = "A".repeat(69);
        let card = Card::Value {
            keyword: "LONGKEY".to_string(),
            value: CardValue::String(s),
            comment: Some("a comment".to_string()),
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].starts_with(b"LONGKEY"));
        assert!(records[0].contains(&b'&'));
        assert!(records[1].starts_with(b"CONTINUE"));
        assert!(records[1].windows(9).any(|w| w == b"a comment".as_ref()));
    }

    #[test]
    fn test_continue_boundary_two_records() {
        // 67 (first card max) + 69 (exactly fills a last CONTINUE) = 136 chars = 2 records
        let s = "C".repeat(67 + 69);
        let card = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String(s),
            comment: None,
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[1].starts_with(b"CONTINUE"));
    }

    #[test]
    fn test_continue_boundary_three_records() {
        // 67 + 70 = 137 chars = 3 records
        let s = "C".repeat(67 + 70);
        let card = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String(s),
            comment: None,
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_very_long_string_three_records() {
        // First: 67 chars, CONTINUE-1: 68 chars, CONTINUE-2: 65 chars
        let s = "B".repeat(200);
        let card = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String(s),
            comment: None,
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 3);
        assert!(records[0].starts_with(b"KEY"));
        assert!(records[1].starts_with(b"CONTINUE "));
        assert!(records[2].starts_with(b"CONTINUE "));
    }

    #[test]
    fn test_quotes_in_long_string() {
        // Single quotes double the escaped length
        let s = "'".repeat(35); // 35 raw chars is 70 escaped chars, > 68
        let card = Card::Value {
            keyword: "QUOTED".to_string(),
            value: CardValue::String(s),
            comment: None,
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 2);
        let first = std::str::from_utf8(&records[0]).unwrap();
        assert!(first.contains("&'"));
    }

    #[test]
    fn test_comment_on_last_record_only() {
        // 200 'D's: first=67, CONTINUE=68, CONTINUE=65.
        // 65 chars leaves only 4 bytes after its closing quote which is not enough for
        // " / telescope comment" (20). A trailing CONTINUE carries the comment so 4 records total.
        let s = "D".repeat(200);
        let comment = "telescope comment";
        let card = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String(s),
            comment: Some(comment.to_string()),
        };
        let records = card.encode_records().unwrap();
        assert_eq!(records.len(), 4);

        let comment_bytes = comment.as_bytes();
        for r in &records[..records.len() - 1] {
            assert!(
                !r.windows(comment_bytes.len()).any(|w| w == comment_bytes),
                "comment appeared on a non-last record"
            );
        }
        let last = &records[records.len() - 1];
        assert!(last.starts_with(b"CONTINUE "));
        assert!(
            last.windows(comment_bytes.len())
                .any(|w| w == comment_bytes)
        );
    }

    #[test]
    fn test_stitch_basic() {
        let mut last = Card::Value {
            keyword: "LONGSTR".to_string(),
            value: CardValue::String("hello &".to_string()),
            comment: None,
        };
        let cont = Card::Continue {
            value: "world".to_string(),
            comment: Some("the comment".to_string()),
        };
        assert!(stitch_continue(Some(&mut last), &cont));
        assert_eq!(
            last,
            Card::Value {
                keyword: "LONGSTR".to_string(),
                value: CardValue::String("hello world".to_string()),
                comment: Some("the comment".to_string()),
            }
        );
    }

    #[test]
    fn test_stitch_multiple() {
        let mut last = Card::Value {
            keyword: "LONGSTR".to_string(),
            value: CardValue::String("nothing &".to_string()),
            comment: Some("youth".to_string()),
        };
        assert!(stitch_continue(
            Some(&mut last),
            &Card::Continue {
                value: "is &".to_string(),
                comment: Some("is".to_string())
            }
        ));
        assert!(stitch_continue(
            Some(&mut last),
            &Card::Continue {
                value: "permanent".to_string(),
                comment: Some("fleeting".to_string())
            }
        ));
        assert_eq!(
            last,
            Card::Value {
                keyword: "LONGSTR".to_string(),
                value: CardValue::String("nothing is permanent".to_string()),
                comment: Some("youth is fleeting".to_string()),
            }
        );
    }

    #[test]
    fn test_stitch_no_ampersand_returns_false() {
        let mut last = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String("no ampersand".to_string()),
            comment: None,
        };
        let cont = Card::Continue {
            value: "orphaned".to_string(),
            comment: None,
        };
        assert!(!stitch_continue(Some(&mut last), &cont));
        // last is unchanged
        assert_eq!(
            last,
            Card::Value {
                keyword: "KEY".to_string(),
                value: CardValue::String("no ampersand".to_string()),
                comment: None,
            }
        );
    }

    #[test]
    fn test_stitch_non_continue_returns_false() {
        let mut last = Card::Value {
            keyword: "KEY".to_string(),
            value: CardValue::String("hello &".to_string()),
            comment: None,
        };
        assert!(!stitch_continue(Some(&mut last), &Card::End));
        assert!(!stitch_continue(Some(&mut last), &Card::Blank));
    }

    #[test]
    fn test_stitch_none_last_returns_false() {
        let cont = Card::Continue {
            value: "value".to_string(),
            comment: None,
        };
        assert!(!stitch_continue(None, &cont));
    }
}
