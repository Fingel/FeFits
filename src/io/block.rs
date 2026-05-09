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

    pub fn blank() -> Self {
        Block([b' '; 2880])
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

    pub fn set_record(&mut self, index: usize, record: &[u8; 80]) {
        let start = index * 80;
        let end = start + 80;
        self.0[start..end].copy_from_slice(record);
    }
}

pub fn blocks_needed(n: u64) -> u64 {
    n.div_ceil(2880)
}

pub fn padded_size(n: u64) -> u64 {
    blocks_needed(n) * 2880
}

pub fn padding_bytes(n: u64) -> u64 {
    padded_size(n) - n
}

pub struct BlockReader<R: Read> {
    inner: R,
    pub blocks_read: u64,
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

pub struct BlockWriter<W: std::io::Write> {
    inner: W,
    buffer: Block,
    slot: usize, // 0..36
    pub blocks_written: u64,
}

impl<W: std::io::Write> BlockWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            buffer: Block::blank(),
            slot: 0,
            blocks_written: 0,
        }
    }

    pub fn write_record(&mut self, record: &[u8; 80]) -> Result<(), Error> {
        self.buffer.set_record(self.slot, record);
        self.slot += 1;
        if self.slot == 36 {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.inner.write_all(self.buffer.as_bytes())?;
        self.blocks_written += 1;
        self.buffer = Block::blank();
        self.slot = 0;
        Ok(())
    }

    pub fn finish(mut self) -> Result<u64, Error> {
        if self.slot > 0 {
            self.flush()?;
        }
        Ok(self.blocks_written)
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

    #[test]
    fn alignment() {
        assert_eq!(blocks_needed(0), 0);
        assert_eq!(blocks_needed(1), 1);
        assert_eq!(blocks_needed(2880), 1);
        assert_eq!(blocks_needed(2881), 2);

        assert_eq!(padded_size(400), 2880);
        assert_eq!(padded_size(2880), 2880);
        assert_eq!(padded_size(2881), 5760);

        assert_eq!(padding_bytes(400), 2480);
        assert_eq!(padding_bytes(2880), 0);
    }
}
