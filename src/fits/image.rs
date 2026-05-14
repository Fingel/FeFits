/// See 4.4.2.5 Table 11 For the conversion conventions for signed to unsigned int values
#[derive(Debug)]
pub enum ImageData {
    I8(Vec<i8>),   // BITPIX=8, BZERO=-128 (u8 stored, i8 physical)
    U8(Vec<u8>),   // BITPIX=8, no scaling
    I16(Vec<i16>), // BITPIX=16, no scaling
    U16(Vec<u16>), // BITPIX=16, BZERO=32768 (i16 stored, u16 physical)
    I32(Vec<i32>), // BITPIX=32, no scaling
    U32(Vec<u32>), // BITPIX=32, BZERO=2147483648 (i32 stored, u32 physical)
    I64(Vec<i64>), // BITPIX=64, no scaling
    U64(Vec<u64>), // BITPIX=64, BZERO=9223372036854775808 (i64 stored, u64 physical)
    F32(Vec<f32>), // BITPIX=-32, no scaling
    F64(Vec<f64>), // BITPIX=-64, no scaling OR any type with non-identity BSCALE/BZERO
}

impl ImageData {
    pub fn len(&self) -> usize {
        match self {
            ImageData::I8(v) => v.len(),
            ImageData::U8(v) => v.len(),
            ImageData::I16(v) => v.len(),
            ImageData::U16(v) => v.len(),
            ImageData::I32(v) => v.len(),
            ImageData::U32(v) => v.len(),
            ImageData::I64(v) => v.len(),
            ImageData::U64(v) => v.len(),
            ImageData::F32(v) => v.len(),
            ImageData::F64(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn into_f64(self) -> Vec<f64> {
        match self {
            ImageData::I8(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::U8(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::I16(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::U16(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::I32(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::U32(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::I64(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::U64(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::F32(v) => v.into_iter().map(|x| x as f64).collect(),
            ImageData::F64(v) => v,
        }
    }
}
