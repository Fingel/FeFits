use std::io::{Read, Seek, SeekFrom};

use crate::{error::Result, extension::XtensionType, header::Header, io::BlockReader};

#[derive(Debug, PartialEq, Eq)]
pub enum HduKind {
    Primary,
    Image,
    BinaryTable,
    AsciiTable,
}

impl From<XtensionType> for HduKind {
    fn from(x: XtensionType) -> Self {
        match x {
            XtensionType::Image => HduKind::Image,
            XtensionType::BinaryTable => HduKind::BinaryTable,
            XtensionType::AsciiTable => HduKind::AsciiTable,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct HduEntry {
    pub header_offset: u64,
    pub data_offset: u64,
    pub data_len: u64,
    pub kind: HduKind,
    pub name: Option<String>,
    pub version: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Fits<R: Read + Seek> {
    reader: R,
    index: Vec<HduEntry>,
    fully_scanned: bool,
    last_scanned_offset: u64,
}

impl<R: Read + Seek> Fits<R> {
    fn scan_one(&mut self) -> Result<bool> {
        // TODO: test
        self.reader
            .seek(SeekFrom::Start(self.last_scanned_offset))?;

        let mut block_reader = BlockReader::new(&mut self.reader);
        let (header, blocks_read) = match Header::read_from_block_reader(&mut block_reader) {
            Ok(result) => result,
            Err(crate::error::Error::Io(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof && !self.index.is_empty() =>
            {
                return Ok(false); // Clean EOF and the start of the next HDU is fine
            }
            Err(e) => return Err(e),
        };
        let header_offset = self.last_scanned_offset;
        let data_offset = header_offset + blocks_read * 2880;
        let data_len = header.data_len()?;
        let kind = if self.index.is_empty() {
            HduKind::Primary
        } else {
            header.xtension()?.into()
        };
        let name = header
            .get("EXTNAME")
            .and_then(|card| card.value())
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());
        let version = header
            .get("EXTVER")
            .and_then(|card| card.value())
            .and_then(|v| v.as_integer());

        let entry = HduEntry {
            header_offset,
            data_offset,
            data_len,
            kind,
            name,
            version,
        };
        self.index.push(entry);
        self.last_scanned_offset = data_offset + data_len;

        Ok(true)
    }

    fn scan_all(&mut self) -> Result<()> {
        // TODO: test
        while self.scan_one()? {}
        self.fully_scanned = true;
        Ok(())
    }
}
