use crate::Bitpix;

mod sealed {
    pub trait Sealed {}
}

pub trait Pixel: sealed::Sealed + Sized {
    const BITPIX: Bitpix;
    fn from_be_bytes(bytes: &[u8]) -> Self;
    fn to_be_bytes(self) -> impl AsRef<[u8]>;
}

impl sealed::Sealed for u8 {}
impl Pixel for u8 {
    const BITPIX: Bitpix = Bitpix::UnsignedByte;
    fn from_be_bytes(bytes: &[u8]) -> Self {
        bytes[0]
    }
    fn to_be_bytes(self) -> impl AsRef<[u8]> {
        u8::to_be_bytes(self)
    }
}

impl sealed::Sealed for i16 {}
impl Pixel for i16 {
    const BITPIX: Bitpix = Bitpix::SignedShort;
    fn from_be_bytes(bytes: &[u8]) -> Self {
        i16::from_be_bytes(bytes.try_into().unwrap())
    }
    fn to_be_bytes(self) -> impl AsRef<[u8]> {
        i16::to_be_bytes(self)
    }
}

impl sealed::Sealed for i32 {}
impl Pixel for i32 {
    const BITPIX: Bitpix = Bitpix::SignedInt;
    fn from_be_bytes(bytes: &[u8]) -> Self {
        i32::from_be_bytes(bytes.try_into().unwrap())
    }
    fn to_be_bytes(self) -> impl AsRef<[u8]> {
        i32::to_be_bytes(self)
    }
}

impl sealed::Sealed for i64 {}
impl Pixel for i64 {
    const BITPIX: Bitpix = Bitpix::SignedLong;
    fn from_be_bytes(bytes: &[u8]) -> Self {
        i64::from_be_bytes(bytes.try_into().unwrap())
    }
    fn to_be_bytes(self) -> impl AsRef<[u8]> {
        i64::to_be_bytes(self)
    }
}

impl sealed::Sealed for f32 {}
impl Pixel for f32 {
    const BITPIX: Bitpix = Bitpix::Float;
    fn from_be_bytes(bytes: &[u8]) -> Self {
        f32::from_be_bytes(bytes.try_into().unwrap())
    }
    fn to_be_bytes(self) -> impl AsRef<[u8]> {
        f32::to_be_bytes(self)
    }
}

impl sealed::Sealed for f64 {}
impl Pixel for f64 {
    const BITPIX: Bitpix = Bitpix::Double;
    fn from_be_bytes(bytes: &[u8]) -> Self {
        f64::from_be_bytes(bytes.try_into().unwrap())
    }
    fn to_be_bytes(self) -> impl AsRef<[u8]> {
        f64::to_be_bytes(self)
    }
}
