use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use crate::{error::Result, extension::XtensionType, header::Header, io::BlockReader};

#[derive(Debug, PartialEq, Eq, Clone)]
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

#[derive(Debug, PartialEq, Eq, Clone)]
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
}

impl Fits<File> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Fits::from_reader(File::open(path)?)
    }
}

impl<R: Read + Seek> Fits<R> {
    pub fn from_reader(reader: R) -> Result<Self> {
        let mut fits = Self {
            reader,
            index: Vec::new(),
        };
        fits.scan_all()?;
        Ok(fits)
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn hdu(&self, n: usize) -> Option<&HduEntry> {
        self.index.get(n)
    }

    pub fn hdu_by_name(&self, name: &str) -> Option<&HduEntry> {
        self.index
            .iter()
            .find(|hdu| hdu.name.as_deref() == Some(name))
    }

    pub fn iter_hdus(&self) -> impl Iterator<Item = &HduEntry> {
        self.index.iter()
    }

    pub fn read_header(&mut self, entry: &HduEntry) -> Result<Header> {
        self.reader.seek(SeekFrom::Start(entry.header_offset))?;
        let mut block_reader = BlockReader::new(&mut self.reader);
        let (header, _) = Header::read_from_block_reader(&mut block_reader)?;
        Ok(header)
    }

    fn scan_one(&mut self, offset: u64) -> Result<Option<u64>> {
        self.reader.seek(SeekFrom::Start(offset))?;

        let mut block_reader = BlockReader::new(&mut self.reader);
        let (header, blocks_read) = match Header::read_from_block_reader(&mut block_reader) {
            Ok(result) => result,
            Err(crate::error::Error::Io(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof && !self.index.is_empty() =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let data_offset = offset + blocks_read * 2880;
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

        self.index.push(HduEntry {
            header_offset: offset,
            data_offset,
            data_len,
            kind,
            name,
            version,
        });
        Ok(Some(data_offset + data_len))
    }

    fn scan_all(&mut self) -> Result<()> {
        let mut offset = 0u64;
        while let Some(next_offset) = self.scan_one(offset)? {
            offset = next_offset;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::{
        card::{Card, CardValue},
        extension::XtensionType,
        header::Header,
    };

    fn int_card(keyword: &str, value: i64) -> Card {
        Card::Value {
            keyword: keyword.to_string(),
            value: CardValue::Integer(value),
            comment: None,
        }
    }

    fn make_primary_header(axes: &[i64]) -> Header {
        let mut h = Header::new();
        h.append(Card::Value {
            keyword: "SIMPLE".into(),
            value: CardValue::Logical(true),
            comment: None,
        });
        h.append(int_card("BITPIX", 8));
        h.append(int_card("NAXIS", axes.len() as i64));
        for (i, &n) in axes.iter().enumerate() {
            h.append(int_card(&format!("NAXIS{}", i + 1), n));
        }
        h.append(Card::End);
        h
    }

    fn make_image_extension(axes: &[i64]) -> Header {
        let mut h = Header::new();
        h.append(Card::Xtension {
            x: XtensionType::Image,
            comment: None,
        });
        h.append(int_card("BITPIX", 8));
        h.append(int_card("NAXIS", axes.len() as i64));
        for (i, &n) in axes.iter().enumerate() {
            h.append(int_card(&format!("NAXIS{}", i + 1), n));
        }
        h.append(int_card("PCOUNT", 0));
        h.append(int_card("GCOUNT", 1));
        h.append(Card::End);
        h
    }

    fn write_hdu(header: &Header) -> Vec<u8> {
        let mut buf = Vec::new();
        header.write_to_writer(&mut buf).unwrap();
        let data_len = header.data_len().unwrap() as usize;
        buf.extend(std::iter::repeat_n(0u8, data_len));
        buf
    }

    #[test]
    fn test_primary_no_data() {
        let buf = write_hdu(&make_primary_header(&[]));
        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        assert_eq!(fits.len(), 1);
        let entry = fits.hdu(0).unwrap();
        assert_eq!(entry.kind, HduKind::Primary);
        assert_eq!(entry.header_offset, 0);
        assert_eq!(entry.data_offset, 2880);
        assert_eq!(entry.data_len, 0);
        assert_eq!(entry.name, None);
        assert_eq!(entry.version, None);
    }

    #[test]
    fn test_empty_file_is_error() {
        assert!(Fits::from_reader(Cursor::new(vec![])).is_err());
    }

    #[test]
    fn test_two_hdus() {
        // BITPIX=8, NAXIS1=100 = 100 bytes = 1 data block (2880 padded)
        // second HDU header starts at 1 header block + 1 data block = 2 * 2880
        let mut buf = write_hdu(&make_primary_header(&[100]));
        buf.extend(write_hdu(&make_image_extension(&[])));

        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        assert_eq!(fits.len(), 2);
        assert_eq!(fits.hdu(0).unwrap().kind, HduKind::Primary);
        assert_eq!(fits.hdu(0).unwrap().data_len, 2880);
        assert_eq!(fits.hdu(1).unwrap().kind, HduKind::Image);
        assert_eq!(fits.hdu(1).unwrap().header_offset, 2 * 2880);
    }

    #[test]
    fn test_2d_primary_with_extension() {
        // BITPIX=8, NAXIS1=100, NAXIS2=50 = 5000 unpadded bytes = 2 data blocks (5760 padded)
        // second HDU header starts at 1 header block + 2 data blocks = 3 * 2880
        let mut buf = write_hdu(&make_primary_header(&[100, 50]));
        buf.extend(write_hdu(&make_image_extension(&[])));

        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        assert_eq!(fits.len(), 2);
        assert_eq!(fits.hdu(1).unwrap().header_offset, 3 * 2880);
    }

    #[test]
    fn test_extname_extver() {
        let mut ext = Header::new();
        ext.append(Card::Xtension {
            x: XtensionType::Image,
            comment: None,
        });
        ext.append(int_card("BITPIX", 8));
        ext.append(int_card("NAXIS", 0));
        ext.append(int_card("PCOUNT", 0));
        ext.append(int_card("GCOUNT", 1));
        ext.append(Card::Value {
            keyword: "EXTNAME".into(),
            value: CardValue::String("SCI".into()),
            comment: None,
        });
        ext.append(int_card("EXTVER", 2));
        ext.append(Card::End);

        let mut buf = write_hdu(&make_primary_header(&[]));
        buf.extend(write_hdu(&ext));

        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        assert_eq!(fits.hdu(1).unwrap().name, Some("SCI".into()));
        assert_eq!(fits.hdu(1).unwrap().version, Some(2));
    }
}
