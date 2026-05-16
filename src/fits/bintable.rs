use crate::error::{Error, Result};

/// The data type of elements stored in the heap for a Variable-Length Array column.
/// Represents the `t` character in the TFORM `rPt(emax)` / `rQt(emax)` format.
/// P and Q are not valid element types. 7.3.5
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VlaElementType {
    Logical,
    Bit,
    UnsignedByte,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
    Complex32,
    Complex64,
    Char,
}

impl VlaElementType {
    pub fn from_char(c: char, tform: &str) -> Result<Self> {
        match c {
            'L' => Ok(Self::Logical),
            'X' => Ok(Self::Bit),
            'B' => Ok(Self::UnsignedByte),
            'I' => Ok(Self::Int16),
            'J' => Ok(Self::Int32),
            'K' => Ok(Self::Int64),
            'E' => Ok(Self::Float32),
            'D' => Ok(Self::Float64),
            'C' => Ok(Self::Complex32),
            'M' => Ok(Self::Complex64),
            'A' => Ok(Self::Char),
            _ => Err(Error::InvalidHeader(format!(
                "unknown VLA element type '{c}' in TFORM '{tform}'"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vla_element_type_from_char() {
        assert_eq!(
            // 'B' is the element type char extracted from "PB(1800)" by TForm::parse
            // the full string is passed only for error context
            VlaElementType::from_char('B', "PB(100)").unwrap(), // 7.3.5 rPt(emax) format
            VlaElementType::UnsignedByte
        );
        assert_eq!(
            VlaElementType::from_char('J', "PJ").unwrap(),
            VlaElementType::Int32
        );
        assert!(VlaElementType::from_char('Z', "PZ").is_err());
    }
}
