mod error;

/// Section 2.2;
/// A sequence of 2880 eight-bit bytes aligned on
/// 2880-byte boundaries in the FITS file, most commonly either
/// a header block or a data block. Special records are another
/// infrequently used type of FITS block. This block length was
/// chosen because it is evenly divisible by the byte and word
/// lengths of all known computer systems at the time FITS was
/// developed in 1979
pub struct Block([u8; 2880]);

impl Block {
    pub fn zeroed() -> Self {
        Block([0; 2880])
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    pub fn records(&self) -> impl Iterator<Item = &[u8; 80]> {
        self.0.as_chunks::<80>().0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_six_records() {
        let block = Block::zeroed();
        let records: Vec<&[u8; 80]> = block.records().collect();
        assert_eq!(records.len(), 36);
    }
}
