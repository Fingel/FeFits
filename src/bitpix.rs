use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bitpix {
    UnsignedByte = 8,
    SignedShort = 16,
    SignedInt = 32,
    SignedLong = 64,
    Float = -32,
    Double = -64,
}

impl TryFrom<i64> for Bitpix {
    type Error = Error;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            8 => Ok(Bitpix::UnsignedByte),
            16 => Ok(Bitpix::SignedShort),
            32 => Ok(Bitpix::SignedInt),
            64 => Ok(Bitpix::SignedLong),
            -32 => Ok(Bitpix::Float),
            -64 => Ok(Bitpix::Double),
            other => Err(Error::InvalidKeywordValue {
                keyword: "BITPIX",
                value: other.to_string(),
                reason: "must be one of 8, 16, 32, 64, -32, or -64",
            }),
        }
    }
}

impl Bitpix {
    pub fn byte_width(&self) -> usize {
        match self {
            Bitpix::UnsignedByte => 1,
            Bitpix::SignedShort => 2,
            Bitpix::SignedInt | Bitpix::Float => 4,
            Bitpix::SignedLong | Bitpix::Double => 8,
        }
    }
}
