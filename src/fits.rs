use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use crate::{
    Bitpix,
    error::{Error, Result},
    extension::XtensionType,
    header::Header,
    io::BlockReader,
    pixel::Pixel,
};

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
    pub index: usize,
    pub header_offset: u64,
    pub data_offset: u64,
    pub data_len: u64,
    pub kind: HduKind,
    pub name: Option<String>,
    pub version: Option<i64>,
}

pub enum ImageData {
    U8(Vec<u8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
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

    pub fn hdu_by_name(&self, name: &str, version: Option<i64>) -> Option<&HduEntry> {
        if name.eq_ignore_ascii_case("PRIMARY") {
            // support looking up "PRIMARY" by name, even though technically it's not named
            return self.index.first().filter(|e| e.kind == HduKind::Primary);
        }
        let target_version = version.unwrap_or(1);
        self.index.iter().find(|hdu| {
            hdu.name.as_deref() == Some(name) && hdu.version.unwrap_or(1) == target_version
        })
    }

    pub fn iter_hdus(&self) -> impl Iterator<Item = &HduEntry> {
        self.index.iter()
    }

    pub fn read_header(&mut self, n: usize) -> Result<Header> {
        let offset = self
            .index
            .get(n)
            .ok_or(crate::error::Error::HduNotFound(n))?
            .header_offset;
        self.reader.seek(SeekFrom::Start(offset))?;
        let mut block_reader = BlockReader::new(&mut self.reader);
        let (header, _) = Header::read_from_block_reader(&mut block_reader)?;
        Ok(header)
    }

    /// Read an image as a vector of pixels of type `T` directly. If you already
    /// know the pixel type of the image, this will return a vector of that type directly.
    /// Otherwise, use read_image and match on the ImageData variant to determine the pixel type.
    pub fn read_image_as<T: Pixel>(&mut self, n: usize) -> Result<Vec<T>> {
        let entry = self.index.get(n).ok_or(Error::HduNotFound(n))?;
        match entry.kind {
            HduKind::Primary | HduKind::Image => {}
            ref k => return Err(Error::InvalidHDU(format!("HDU {n} is {k:?}, not an image"))),
        }
        let data_offset = entry.data_offset;

        let header = self.read_header(n)?;
        let actual = header.bitpix()?;
        if actual != T::BITPIX {
            return Err(Error::TypeMismatch(format!(
                "header has {actual:?}, caller requested {:?}",
                T::BITPIX
            )));
        }

        let naxis = header.naxis()?;
        let pixel_count: u64 = if naxis == 0 {
            0
        } else {
            (1..=naxis).try_fold(1u64, |acc, n| header.naxisn(n).map(|v| acc * v))?
        };

        if pixel_count == 0 {
            return Ok(Vec::new());
        }

        let unpadded_bytes = pixel_count * T::BITPIX.byte_width() as u64;
        self.reader.seek(SeekFrom::Start(data_offset))?;
        let mut raw = vec![0u8; unpadded_bytes as usize];
        self.reader.read_exact(&mut raw)?;

        Ok(raw
            .chunks_exact(T::BITPIX.byte_width())
            .map(T::from_be_bytes)
            .collect())
    }

    pub fn read_image(&mut self, n: usize) -> Result<ImageData> {
        let bitpix = self.read_header(n)?.bitpix()?;
        match bitpix {
            Bitpix::UnsignedByte => self.read_image_as::<u8>(n).map(ImageData::U8),
            Bitpix::SignedShort => self.read_image_as::<i16>(n).map(ImageData::I16),
            Bitpix::SignedInt => self.read_image_as::<i32>(n).map(ImageData::I32),
            Bitpix::SignedLong => self.read_image_as::<i64>(n).map(ImageData::I64),
            Bitpix::Float => self.read_image_as::<f32>(n).map(ImageData::F32),
            Bitpix::Double => self.read_image_as::<f64>(n).map(ImageData::F64),
        }
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
            index: self.index.len(),
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

    fn make_primary_header(bitpix: i64, axes: &[i64]) -> Header {
        let mut h = Header::new();
        h.append(Card::Value {
            keyword: "SIMPLE".into(),
            value: CardValue::Logical(true),
            comment: None,
        });
        h.append(int_card("BITPIX", bitpix));
        h.append(int_card("NAXIS", axes.len() as i64));
        for (i, &n) in axes.iter().enumerate() {
            h.append(int_card(&format!("NAXIS{}", i + 1), n));
        }
        h.append(Card::End);
        h
    }

    fn make_image_extension(bitpix: i64, axes: &[i64]) -> Header {
        let mut h = Header::new();
        h.append(Card::Xtension {
            x: XtensionType::Image,
            comment: None,
        });
        h.append(int_card("BITPIX", bitpix));
        h.append(int_card("NAXIS", axes.len() as i64));
        for (i, &n) in axes.iter().enumerate() {
            h.append(int_card(&format!("NAXIS{}", i + 1), n));
        }
        h.append(int_card("PCOUNT", 0));
        h.append(int_card("GCOUNT", 1));
        h.append(Card::End);
        h
    }

    fn write_hdu(header: &Header, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        header.write_to_writer(&mut buf).unwrap();
        buf.extend_from_slice(data);
        buf.extend(std::iter::repeat_n(
            0u8,
            if data.is_empty() {
                header.data_len().unwrap() as usize
            } else {
                crate::io::padding_bytes(data.len() as u64) as usize
            },
        ));
        buf
    }

    #[test]
    fn test_primary_no_data() {
        let buf = write_hdu(&make_primary_header(8, &[]), &[]);
        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        assert_eq!(fits.len(), 1);
        let entry = fits.hdu(0).unwrap();
        assert_eq!(entry.kind, HduKind::Primary);
        assert_eq!(entry.header_offset, 0);
        assert_eq!(entry.index, 0);
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
        let mut buf = write_hdu(&make_primary_header(8, &[100]), &[]);
        buf.extend(write_hdu(&make_image_extension(8, &[]), &[]));

        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        assert_eq!(fits.len(), 2);
        assert_eq!(fits.hdu(0).unwrap().kind, HduKind::Primary);
        assert_eq!(fits.hdu(0).unwrap().data_len, 2880);
        assert_eq!(fits.hdu(0).unwrap().index, 0);
        assert_eq!(fits.hdu(1).unwrap().kind, HduKind::Image);
        assert_eq!(fits.hdu(1).unwrap().header_offset, 2 * 2880);
        assert_eq!(fits.hdu(1).unwrap().index, 1);
    }

    #[test]
    fn test_2d_primary_with_extension() {
        // BITPIX=8, NAXIS1=100, NAXIS2=50 = 5000 unpadded bytes = 2 data blocks (5760 padded)
        // second HDU header starts at 1 header block + 2 data blocks = 3 * 2880
        let mut buf = write_hdu(&make_primary_header(8, &[100, 50]), &[]);
        buf.extend(write_hdu(&make_image_extension(8, &[]), &[]));

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

        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&ext, &[]));

        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        assert_eq!(fits.hdu(1).unwrap().name, Some("SCI".into()));
        assert_eq!(fits.hdu(1).unwrap().version, Some(2));
        assert_eq!(fits.hdu_by_name("SCI", Some(2)).unwrap().index, 1);
        assert!(fits.hdu_by_name("SCI", Some(1)).is_none()); // wrong version
    }

    #[test]
    fn test_hdu_by_name_version_default() {
        // Extension with no EXTVER keyword implicitly has version 1.
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
        ext.append(Card::End);

        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&ext, &[]));

        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert!(fits.hdu_by_name("SCI", None).is_some());
        assert!(fits.hdu_by_name("SCI", Some(1)).is_some());
        assert!(fits.hdu_by_name("SCI", Some(2)).is_none());
    }

    #[test]
    fn test_iter_hdus() {
        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&make_image_extension(8, &[]), &[]));

        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        let kinds: Vec<&HduKind> = fits.iter_hdus().map(|e| &e.kind).collect();
        assert_eq!(kinds, vec![&HduKind::Primary, &HduKind::Image]);
    }

    #[test]
    fn test_read_header() {
        let buf = write_hdu(&make_primary_header(8, &[100, 50]), &[]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        let header = fits.read_header(0).unwrap();
        assert_eq!(header.naxis().unwrap(), 2);
    }

    #[test]
    fn test_read_header_not_found() {
        let buf = write_hdu(&make_primary_header(8, &[100, 50]), &[]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        let result = fits.read_header(99);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::error::Error::HduNotFound(99)
        ));
    }

    #[test]
    fn test_read_header_by_name() {
        let buf = write_hdu(&make_primary_header(8, &[100, 50]), &[]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        let hdu = fits.hdu_by_name("PRIMARY", None).unwrap();
        let header = fits.read_header(hdu.index).unwrap();
        assert_eq!(header.naxis().unwrap(), 2);
    }

    #[test]
    fn test_read_image_as_u8() {
        let header = make_primary_header(8, &[3]);
        let buf = write_hdu(&header, &[1u8, 2, 3]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.read_image_as::<u8>(0).unwrap(), vec![1u8, 2, 3]);
    }

    #[test]
    fn test_read_image_as_i16_byte_swap() {
        let header = make_primary_header(16, &[2]);
        // 1i16 big-endian = 0x00, 0x01, -1i16 big-endian = 0xFF, 0xFF
        let buf = write_hdu(&header, &[0x00u8, 0x01, 0xFF, 0xFF]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.read_image_as::<i16>(0).unwrap(), vec![1i16, -1i16]);
    }

    #[test]
    fn test_read_image_as_wrong_type() {
        let header = make_primary_header(16, &[2]);
        let buf = write_hdu(&header, &[0u8; 4]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert!(matches!(
            fits.read_image_as::<u8>(0).unwrap_err(),
            crate::error::Error::TypeMismatch(_)
        ));
    }

    #[test]
    fn test_read_image_as_no_data() {
        let buf = write_hdu(&make_primary_header(8, &[]), &[]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert!(fits.read_image_as::<u8>(0).unwrap().is_empty());
    }

    #[test]
    fn test_read_image_dispatch() {
        let header = make_primary_header(16, &[2]);
        let buf = write_hdu(&header, &[0x00u8, 0x01, 0x00, 0x02]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        let image = fits.read_image(0).unwrap();
        match image {
            ImageData::I16(pixels) => assert_eq!(pixels, vec![1i16, 2i16]),
            _ => panic!("expected ImageData::I16"),
        }
    }
}
