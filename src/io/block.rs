use std::io::Read;

use crate::error::Error;

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

#[allow(dead_code)]
pub fn blocks_needed(n: u64) -> u64 {
    n.div_ceil(2880)
}

#[allow(dead_code)]
pub fn padded_size(n: u64) -> u64 {
    blocks_needed(n) * 2880
}

#[allow(dead_code)]
pub fn padding_bytes(n: u64) -> u64 {
    padded_size(n) - n
}

pub struct BlockReader<R: Read> {
    inner: R,
    blocks_read: u64,
}

impl<R: Read> BlockReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            blocks_read: 0,
        }
    }

    pub fn read_block(&mut self) -> Result<Block, Error> {
        let mut block = Block::zeroed();
        self.inner.read_exact(block.as_bytes_mut())?;
        self.blocks_read += 1;
        Ok(block)
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

    #[test]
    fn read_block() {
        let data = [1u8; 2880 * 2];
        let mut reader = BlockReader::new(&data[..]);

        let block = reader.read_block().unwrap();
        assert_eq!(block.as_bytes(), &data[..2880]);
        assert_eq!(reader.blocks_read, 1);

        let block = reader.read_block().unwrap();
        assert_eq!(block.as_bytes(), &data[2880..]);
        assert_eq!(reader.blocks_read, 2);
    }

    #[test]
    fn incomplete_read() {
        let data = [0; 1000];
        let mut reader = BlockReader::new(&data[..]);
        let result = reader.read_block();
        assert!(matches!(
            result,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof
        ));
    }
}
