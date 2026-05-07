use crate::{
    error::{Error, Result},
    extension::XtensionType,
};

mod encode;

pub struct RawCard([u8; 80]);

impl RawCard {
    /// 4.1.2.1. Keyword name (Bytes 1 through 8), trimmed of trailing spaces
    pub fn keyword(&self) -> &str {
        // SAFETY: constructor validates all bytes are printable ASCII (0x20–0x7E)
        unsafe { std::str::from_utf8_unchecked(&self.0[0..8]).trim_ascii_end() }
    }

    /// 4.1.2.2. Value indicator (Bytes 9 and 10)
    pub fn value_indicator(&self) -> &str {
        // SAFETY: constructor validates all bytes are printable ASCII (0x20–0x7E)
        unsafe { std::str::from_utf8_unchecked(&self.0[8..10]) }
    }

    /// 4.1.2.3. Value/comment (Bytes 11 through 80)
    pub fn value_comment(&self) -> &str {
        // SAFETY: constructor validates all bytes are printable ASCII (0x20–0x7E)
        unsafe { std::str::from_utf8_unchecked(&self.0[10..80]) }
    }

    /// 4.4.2.4 Keywords without a value indicator (COMMENT, HISTORY, CONTINUE, blank keyword)
    /// Bytes 9 through 80
    pub fn after_keyword(&self) -> &str {
        // SAFETY: constructor validates all bytes are printable ASCII (0x20–0x7E)
        unsafe { std::str::from_utf8_unchecked(&self.0[8..]) }
    }
}

impl TryFrom<&[u8; 80]> for RawCard {
    type Error = Error;
    fn try_from(bytes: &[u8; 80]) -> Result<Self> {
        if !bytes.iter().all(|&b| is_fits_printable(b)) {
            return Err(Error::InvalidCard(format!(
                "Card contains non-printable ASCII characters: {:?}",
                bytes
            )));
        }
        Ok(RawCard(*bytes))
    }
}

/// Section 3.3.1
/// Validate that bytes are printable ASCII characters
fn is_fits_printable(b: u8) -> bool {
    // Astropy allows tab so we might want to add that here.
    b.is_ascii() && !b.is_ascii_control()
}

#[derive(Debug, PartialEq, Clone)]
pub enum CardValue {
    String(String),
    Logical(bool),
    Integer(i64),
    Float(f64),
    ComplexInteger(i64, i64),
    ComplexFloat(f64, f64),
    Undefined,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Card {
    Value {
        keyword: String,
        value: CardValue,
        comment: Option<String>,
    },
    Comment(String), // bytes 9–80, no value indicator
    History(String), // bytes 9–80, no value indicator
    Continue {
        value: String,
        comment: Option<String>,
    },
    Xtension {
        x: XtensionType,
        comment: Option<String>,
    },
    Blank,
    End,
}

impl TryFrom<RawCard> for Card {
    type Error = Error;
    fn try_from(raw: RawCard) -> Result<Self> {
        validate_keyword(raw.keyword())?;
        match raw.keyword() {
            "" => {
                if raw.after_keyword().trim().is_empty() {
                    Ok(Card::Blank)
                } else {
                    Ok(Card::Comment(parse_comment_string(raw.after_keyword())))
                }
            }
            "END" => Ok(Card::End),
            "COMMENT" => Ok(Card::Comment(parse_comment_string(raw.after_keyword()))),
            "HISTORY" => Ok(Card::History(parse_comment_string(raw.after_keyword()))),
            "CONTINUE" => {
                let (value, comment) = parse_continue(raw.after_keyword())?;
                Ok(Card::Continue { value, comment })
            }
            "XTENSION" => {
                if raw.value_indicator() != "= " {
                    return Err(Error::InvalidCard(
                        "XTENSION card is missing value indicator '= '".into(),
                    ));
                }
                let (rest, s) = parse_string(raw.value_comment())?;
                let x = s.parse::<XtensionType>()?;
                Ok(Card::Xtension {
                    x,
                    comment: extract_comment(rest),
                })
            }
            _ => {
                if raw.value_indicator() != "= " {
                    return Err(Error::InvalidCard(format!(
                        "unrecognized keyword '{}' without value indicator",
                        raw.keyword()
                    )));
                }
                let (value, comment) = parse_value(raw.value_comment())?;
                Ok(Card::Value {
                    keyword: raw.keyword().to_owned(),
                    value,
                    comment,
                })
            }
        }
    }
}

/// 4.1.2.1
fn validate_keyword(kw: &str) -> Result<()> {
    if kw
        .bytes()
        .all(|b| matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(Error::InvalidCard(format!(
            "invalid keyword characters: '{kw}'"
        )))
    }
}

fn parse_comment_string(value: &str) -> String {
    value.trim_ascii_end().to_owned()
}

fn parse_value(input: &str) -> Result<(CardValue, Option<String>)> {
    let trimmed = input.trim_start();

    // strings are the only type where '/' can appear in the value itself
    if trimmed.starts_with('\'') {
        let (rest, s) = parse_string(trimmed)?;
        return Ok((CardValue::String(s), extract_comment(rest)));
    }

    let (value_str, comment) = split_value_comment(trimmed);
    let value_str = value_str.trim();
    let value = match value_str.chars().next() {
        None => CardValue::Undefined,
        Some('(') => parse_complex(value_str)?,
        Some('T') => CardValue::Logical(true),
        Some('F') => CardValue::Logical(false),
        _ => parse_number(value_str)?,
    };

    Ok((value, comment))
}

// 4.2.1.2: byte 9 is a required space, bytes 10–80 are a string value field
fn parse_continue(input: &str) -> Result<(String, Option<String>)> {
    let (rest, s) = parse_string(input)?;
    Ok((s, extract_comment(rest)))
}

fn split_value_comment(input: &str) -> (&str, Option<String>) {
    match input.find('/') {
        Some(i) => (&input[..i], extract_comment(&input[i..])),
        None => (input, None),
    }
}

fn extract_comment(input: &str) -> Option<String> {
    input
        .trim_start()
        .strip_prefix('/')
        .map(|s| s.trim_ascii_end().to_string())
}

/// 4.2.1.1
/// A single quote is represented within a string as two successive single quotes
/// e.g., O’HARA = 'O''HARA'
fn parse_string(input: &str) -> Result<(&str, String)> {
    let inner = input
        .trim_start()
        .strip_prefix('\'')
        .ok_or_else(|| Error::InvalidCard("expected opening quote".into()))?;

    let mut chars = inner.char_indices().peekable();
    let mut out = String::new();

    loop {
        match chars.next() {
            None => return Err(Error::InvalidCard("unclosed string".into())),
            Some((_, '\'')) if chars.peek().map(|(_, c)| *c) == Some('\'') => {
                // This is an escaped quote, consume the next quote and add one quote to the output
                chars.next();
                out.push('\'');
            }
            Some((i, '\'')) => {
                // end of the string, the rest of the content is probably a comment
                let rest = &inner[i + 1..];
                return Ok((rest, out.trim_end().to_string()));
            }
            Some((_, c)) => out.push(c),
        }
    }
}
/// 4.2.4 The exponent, if present, consists of an exponent
/// letter followed by an integer. Letters in the exponential form
/// (’E’ or ’D’) shall be upper case.
fn is_float_str(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    u.contains('.') || u.contains('E') || u.contains('D')
}

/// 4.2.5 complex integer, 4.2.6 complex floating-point
fn parse_complex(input: &str) -> Result<CardValue> {
    let close = input
        .find(')')
        .ok_or_else(|| Error::InvalidCard("unclosed complex".into()))?;
    let inner = &input[1..close];

    let comma = inner
        .find(',')
        .ok_or_else(|| Error::InvalidCard("complex missing comma".into()))?;
    let a = inner[..comma].trim();
    let b = inner[comma + 1..].trim();

    if is_float_str(a) || is_float_str(b) {
        Ok(CardValue::ComplexFloat(parse_float(a)?, parse_float(b)?))
    } else {
        Ok(CardValue::ComplexInteger(
            a.parse()
                .map_err(|_| Error::InvalidCard(format!("invalid complex component: {a}")))?,
            b.parse()
                .map_err(|_| Error::InvalidCard(format!("invalid complex component: {b}")))?,
        ))
    }
}

/// 4.2.3 integer, 4.2.4 real floating-point
fn parse_number(s: &str) -> Result<CardValue> {
    if is_float_str(s) {
        Ok(CardValue::Float(parse_float(s)?))
    } else {
        s.parse::<i64>()
            .map(CardValue::Integer)
            .map_err(|_| Error::InvalidCard(format!("invalid integer: {s}")))
    }
}

/// 4.2.4 (footnote 4) d/D normalised to E for parsing
fn parse_float(s: &str) -> Result<f64> {
    s.replace(['d', 'D'], "E")
        .parse::<f64>()
        .map_err(|_| Error::InvalidCard(format!("invalid float: {s}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn right_pad(s: &str) -> [u8; 80] {
        let mut bytes = [b' '; 80];
        let s_bytes = s.as_bytes();
        bytes[..s_bytes.len().min(80)].copy_from_slice(s_bytes);
        bytes
    }

    #[test]
    fn test_rawcard_constructor() {
        // Good case
        let valid = right_pad("SIMPLE  =                    T / FITS STANDARD");
        assert!(RawCard::try_from(&valid).is_ok());

        // Non-printbale ascii
        let invalid = right_pad("SIMPLE  =                   T / FITS STANDARD\x7F");
        assert!(RawCard::try_from(&invalid).is_err());

        // Newline
        let invalid = right_pad("SIMPLE  =    \n               T / FITS STANDARD");
        assert!(RawCard::try_from(&invalid).is_err());
    }

    #[test]
    fn test_rawcard_contructor_tab() {
        let invalid = right_pad("SIMPLE  =    \t               T / FITS STANDARD");
        assert!(RawCard::try_from(&invalid).is_err());
    }

    #[test]
    fn test_mangled_keyword() {
        // keyword needs to be 8 bytes
        let invalid = right_pad("SHORT=                  foo / bar");
        let card = RawCard::try_from(&invalid).unwrap();
        let card = Card::try_from(card);
        let err = card.unwrap_err();
        assert!(err.to_string().contains("invalid keyword characters"));
    }

    #[test]
    fn test_unrecognized_keyword_no_value_indicator() {
        let header = "FOOBAR  unrecognized keyword without value indicator";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let result = Card::try_from(card);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unrecognized keyword")
        );
    }

    #[test]
    fn test_logical_true() {
        let header = "SIMPLE  =                    T / FITS STANDARD";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "SIMPLE".to_string(),
                value: CardValue::Logical(true),
                comment: Some(" FITS STANDARD".to_string())
            }
        );
    }

    #[test]
    fn test_logical_false() {
        let header = "RUST    =                    F / FITS STANDARD";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "RUST".to_string(),
                value: CardValue::Logical(false),
                comment: Some(" FITS STANDARD".to_string())
            }
        );
    }

    #[test]
    fn test_integer() {
        let header = "BITPIX  =                   16 / bits per pixel";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "BITPIX".to_string(),
                value: CardValue::Integer(16),
                comment: Some(" bits per pixel".to_string())
            }
        );
    }

    #[test]
    fn test_integer_no_value() {
        let header = "NAXIS   =                    0 / no data";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "NAXIS".to_string(),
                value: CardValue::Integer(0),
                comment: Some(" no data".to_string())
            }
        );
    }

    #[test]
    fn test_integer_no_comment() {
        let header = "NAXIS1  =                  800";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "NAXIS1".to_string(),
                value: CardValue::Integer(800),
                comment: None
            }
        );
    }

    #[test]
    fn test_integer_negative() {
        let header = "OFFSET  =                -2048 / negative value";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "OFFSET".to_string(),
                value: CardValue::Integer(-2048),
                comment: Some(" negative value".to_string())
            }
        );
    }

    #[test]
    fn test_float() {
        let header = "BSCALE  =                  1.0";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "BSCALE".to_string(),
                value: CardValue::Float(1.0),
                comment: None
            }
        );
    }

    #[test]
    fn test_float_exp_notation() {
        let header = "CRVAL1  =      1.23456789E+02  / exponential notation";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "CRVAL1".to_string(),
                value: CardValue::Float(1.23456789e02),
                comment: Some(" exponential notation".to_string())
            }
        );
    }

    #[test]
    fn test_float_fortran_notation() {
        let header = "CRVAL2  =      2.50000000D-01  / D-exponent from Fortran";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "CRVAL2".to_string(),
                value: CardValue::Float(2.50000000e-01),
                comment: Some(" D-exponent from Fortran".to_string())
            }
        );
    }

    #[test]
    fn test_float_negative_w_exp() {
        let header = "CDELT1  =     -2.77777777E-04  / negative with exponent";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "CDELT1".to_string(),
                value: CardValue::Float(-2.77777777E-04),
                comment: Some(" negative with exponent".to_string())
            }
        );
    }

    #[test]
    fn test_string_with_escaped_quote() {
        let header = "OBSERVER= 'O''Brien'           / embedded quote unescapes";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "OBSERVER".to_string(),
                value: CardValue::String("O'Brien".to_string()),
                comment: Some(" embedded quote unescapes".to_string())
            }
        );
    }

    #[test]
    fn test_empty_string() {
        let header = "EMPTY   = ''                   / null string";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "EMPTY".to_string(),
                value: CardValue::String("".to_string()),
                comment: Some(" null string".to_string())
            }
        );
    }

    #[test]
    fn test_string_leading_spaces() {
        let header = "NOTES   = '  leading spaces'   / leading spaces are significant";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "NOTES".to_string(),
                value: CardValue::String("  leading spaces".to_string()),
                comment: Some(" leading spaces are significant".to_string())
            }
        );
    }

    #[test]
    fn test_string_trailing_spaces() {
        let header = "TRIMMED = 'trailing   '        / trailing spaces stripped";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "TRIMMED".to_string(),
                value: CardValue::String("trailing".to_string()),
                comment: Some(" trailing spaces stripped".to_string())
            }
        );
    }

    #[test]
    fn test_string_slash() {
        let header = "SLASH   = 'FOO/BAR'            / slash in both value and comment";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "SLASH".to_string(),
                value: CardValue::String("FOO/BAR".to_string()),
                comment: Some(" slash in both value and comment".to_string())
            }
        );
    }

    #[test]
    fn test_string_unclosed() {
        let header = "UNCLOSED= 'where was I going            / unclosed string";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let result = Card::try_from(card);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unclosed string"));
    }

    #[test]
    fn test_undefined_with_comment() {
        let header = "UNDEF   =                      / blank value field with comment";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "UNDEF".to_string(),
                value: CardValue::Undefined,
                comment: Some(" blank value field with comment".to_string())
            }
        );
    }

    #[test]
    fn test_undefined_without_comment() {
        let header = "UNDEF2  =";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "UNDEF2".to_string(),
                value: CardValue::Undefined,
                comment: None
            }
        );
    }

    #[test]
    fn test_complex_integer() {
        let header = "CMPXI   = (123, 45)            / complex integer";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "CMPXI".to_string(),
                value: CardValue::ComplexInteger(123, 45),
                comment: Some(" complex integer".to_string())
            }
        );
    }

    #[test]
    fn test_complex_float() {
        let header = "CMPXF   = (1.0, -1.0)         / complex float";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "CMPXF".to_string(),
                value: CardValue::ComplexFloat(1.0, -1.0),
                comment: Some(" complex float".to_string())
            }
        );
    }

    #[test]
    fn test_complex_float_exp() {
        let header = "CMPXE   = (1.23E2, -4.56E-1)  / complex float with exponents";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Value {
                keyword: "CMPXE".to_string(),
                value: CardValue::ComplexFloat(1.23e2, -4.56e-1),
                comment: Some(" complex float with exponents".to_string())
            }
        );
    }

    #[test]
    fn test_complex_float_unclosed() {
        let header = "CMPXE   = (1.23E2, -4.56E-1   / complex float unclosed";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let result = Card::try_from(card);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unclosed complex"));
    }

    #[test]
    fn test_comment() {
        let header = "COMMENT Observation taken during director's discretionary time";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Comment("Observation taken during director's discretionary time".to_string())
        );
    }

    #[test]
    fn test_history() {
        // Bytes 9–80 are the commentary; the two spaces after "HISTORY " are part of the text.
        let header = "HISTORY Reduced with banzai";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(card, Card::History("Reduced with banzai".to_string()));
    }

    #[test]
    fn test_blank() {
        let header = "";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(card, Card::Blank);
    }

    #[test]
    fn test_blank_with_content() {
        let header = "        blank keyword";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(card, Card::Comment("blank keyword".to_string()));
    }

    #[test]
    fn test_end() {
        let header = "END";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(card, Card::End)
    }

    #[test]
    fn test_continue_no_comment() {
        let header = "CONTINUE ' over multiple keyword cards.'";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Continue {
                value: " over multiple keyword cards.".to_string(),
                comment: None
            }
        );
    }

    #[test]
    fn test_continue_with_comment() {
        let header = "CONTINUE 'final segment' / continuation comment";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Continue {
                value: "final segment".to_string(),
                comment: Some(" continuation comment".to_string())
            }
        );
    }

    #[test]
    fn test_xtension_image() {
        let header = "XTENSION= 'IMAGE   '           / image extension";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let card = Card::try_from(card).unwrap();
        assert_eq!(
            card,
            Card::Xtension {
                x: XtensionType::Image,
                comment: Some(" image extension".to_string())
            }
        );
    }

    #[test]
    fn test_xtension_unknown() {
        let header = "XTENSION= 'UNKNOWN '";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let result = Card::try_from(card);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unknown XTENSION type")
        );
    }

    #[test]
    fn test_xtension_malformed() {
        let header = "XTENSION= IMAGE";
        let card = RawCard::try_from(&right_pad(header)).unwrap();
        let result = Card::try_from(card);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected opening quote")
        );
    }
}
