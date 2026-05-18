use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use crate::{
    Bitpix,
    error::{Error, Result},
    extension::XtensionType,
    fits::image::ImageData,
    header::Header,
    io::BlockReader,
    pixel::Pixel,
};
use bintable::BinTableLayout;
use compression::{AlgoParams, CmpType, CompressionHeader};

pub mod bintable;
pub mod compression;
mod image;
pub mod rice;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum HduKind {
    Primary,
    Image,
    BinaryTable,
    AsciiTable,
    CompressedImage,
}

fn hdu_kind_from_extension_header(header: &Header) -> Result<HduKind> {
    match header.xtension()? {
        XtensionType::Image => Ok(HduKind::Image),
        XtensionType::AsciiTable => Ok(HduKind::AsciiTable),
        XtensionType::BinaryTable => {
            if header.get_value("ZIMAGE").and_then(|v| v.as_bool()) == Some(true) {
                Ok(HduKind::CompressedImage)
            } else {
                Ok(HduKind::BinaryTable)
            }
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

impl HduEntry {
    pub fn is_image(&self) -> bool {
        matches!(
            self.kind,
            HduKind::Primary | HduKind::Image | HduKind::CompressedImage
        )
    }
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

fn convert_compressed_pixels(
    pixels: Vec<i32>,
    bitpix: Bitpix,
    bscale: f64,
    bzero: f64,
) -> ImageData {
    if bscale == 1.0 && bzero == 0.0 {
        return match bitpix {
            Bitpix::UnsignedByte => ImageData::U8(pixels.into_iter().map(|v| v as u8).collect()),
            Bitpix::SignedShort => ImageData::I16(pixels.into_iter().map(|v| v as i16).collect()),
            Bitpix::SignedInt => ImageData::I32(pixels),
            _ => ImageData::I32(pixels),
        };
    }

    if bscale == 1.0 {
        if bitpix == Bitpix::UnsignedByte && bzero == -128.0 {
            return ImageData::I8(
                pixels
                    .into_iter()
                    .map(|v| (v as u8 ^ (1u8 << 7)) as i8)
                    .collect(),
            );
        }
        if bitpix == Bitpix::SignedShort && bzero == 32768.0 {
            return ImageData::U16(
                pixels
                    .into_iter()
                    .map(|v| (v as u16) ^ (1u16 << 15))
                    .collect(),
            );
        }
        if bitpix == Bitpix::SignedInt && bzero == 2147483648.0 {
            return ImageData::U32(
                pixels
                    .into_iter()
                    .map(|v| (v as u32) ^ (1u32 << 31))
                    .collect(),
            );
        }
    }

    ImageData::F64(
        pixels
            .into_iter()
            .map(|v| bzero + bscale * v as f64)
            .collect(),
    )
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

    /// Returns the index of the first HDU that contains a 2D (or higher) image
    pub fn find_image(&mut self) -> Result<Option<usize>> {
        let candidates: Vec<(usize, HduKind)> = self
            .index
            .iter()
            .filter(|e| e.is_image())
            .map(|e| (e.index, e.kind.clone()))
            .collect();

        for (idx, kind) in candidates {
            let header = self.read_header(idx)?;

            let (naxis, w, h) = if kind == HduKind::CompressedImage {
                let Ok(naxis) = header.znaxis() else { continue };
                let Ok(w) = header.znaxisn(1) else { continue };
                let Ok(h) = header.znaxisn(2) else { continue };
                (naxis, w, h)
            } else {
                let Ok(naxis) = header.naxis() else { continue };
                let Ok(w) = header.naxisn(1) else { continue };
                let Ok(h) = header.naxisn(2) else { continue };
                (naxis, w, h)
            };

            if naxis >= 2 && w > 0 && h > 0 {
                return Ok(Some(idx));
            }
        }
        Ok(None)
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
            HduKind::CompressedImage => {
                return Err(Error::UnsupportedFeature(
                    "reading tile-compressed images is not yet implemented".into(),
                ));
            }
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

    fn read_image_native(&mut self, n: usize, bitpix: Bitpix) -> Result<ImageData> {
        match bitpix {
            Bitpix::UnsignedByte => self.read_image_as::<u8>(n).map(ImageData::U8),
            Bitpix::SignedShort => self.read_image_as::<i16>(n).map(ImageData::I16),
            Bitpix::SignedInt => self.read_image_as::<i32>(n).map(ImageData::I32),
            Bitpix::SignedLong => self.read_image_as::<i64>(n).map(ImageData::I64),
            Bitpix::Float => self.read_image_as::<f32>(n).map(ImageData::F32),
            Bitpix::Double => self.read_image_as::<f64>(n).map(ImageData::F64),
        }
    }

    pub fn read_image_raw(&mut self, n: usize) -> Result<ImageData> {
        let bitpix = self.read_header(n)?.bitpix()?;
        self.read_image_native(n, bitpix)
    }

    pub fn read_image(&mut self, n: usize) -> Result<ImageData> {
        if self.index.get(n).ok_or(Error::HduNotFound(n))?.kind == HduKind::CompressedImage {
            return self.read_compressed_image(n);
        }

        let header = self.read_header(n)?;
        let bitpix = header.bitpix()?;
        let bscale = header.bscale()?;
        let bzero = header.bzero()?;

        // No scaling needed
        if bscale == 1.0 && bzero == 0.0 {
            return self.read_image_native(n, bitpix);
        }

        // 4.4.2.5 Table 11: Unsigned integer convention, BSCALE must be 1.0
        // MSB/sign bit flip from footnote 9
        if bscale == 1.0 {
            if bitpix == Bitpix::UnsignedByte && bzero == -128.0 {
                let raw = self.read_image_as::<u8>(n)?;
                return Ok(ImageData::I8(
                    raw.into_iter().map(|v| (v ^ (1u8 << 7)) as i8).collect(),
                ));
            }
            if bitpix == Bitpix::SignedShort && bzero == 32768.0 {
                let raw = self.read_image_as::<i16>(n)?;
                return Ok(ImageData::U16(
                    raw.into_iter().map(|v| (v as u16) ^ (1u16 << 15)).collect(),
                ));
            }
            if bitpix == Bitpix::SignedInt && bzero == 2147483648.0 {
                let raw = self.read_image_as::<i32>(n)?;
                return Ok(ImageData::U32(
                    raw.into_iter().map(|v| (v as u32) ^ (1u32 << 31)).collect(),
                ));
            }
            if bitpix == Bitpix::SignedLong && bzero == 9223372036854775808.0 {
                let raw = self.read_image_as::<i64>(n)?;
                return Ok(ImageData::U64(
                    raw.into_iter().map(|v| (v as u64) ^ (1u64 << 63)).collect(),
                ));
            }
        }

        // Arbitrary BSCALE/BZERO scaling
        let pixels: Vec<f64> = match bitpix {
            Bitpix::UnsignedByte => self
                .read_image_as::<u8>(n)?
                .into_iter()
                .map(|v| bzero + bscale * v as f64)
                .collect(),
            Bitpix::SignedShort => self
                .read_image_as::<i16>(n)?
                .into_iter()
                .map(|v| bzero + bscale * v as f64)
                .collect(),
            Bitpix::SignedInt => self
                .read_image_as::<i32>(n)?
                .into_iter()
                .map(|v| bzero + bscale * v as f64)
                .collect(),
            Bitpix::SignedLong => self
                .read_image_as::<i64>(n)?
                .into_iter()
                .map(|v| bzero + bscale * v as f64)
                .collect(),
            Bitpix::Float => self
                .read_image_as::<f32>(n)?
                .into_iter()
                .map(|v| bzero + bscale * v as f64)
                .collect(),
            Bitpix::Double => self
                .read_image_as::<f64>(n)?
                .into_iter()
                .map(|v| bzero + bscale * v)
                .collect(),
        };
        Ok(ImageData::F64(pixels))
    }

    fn read_compressed_image(&mut self, n: usize) -> Result<ImageData> {
        let data_offset = self.index[n].data_offset;

        let header = self.read_header(n)?;
        let layout = BinTableLayout::from_header(&header)?;
        let comp = CompressionHeader::from_header(&header)?;

        if comp.cmp_type != CmpType::Rice {
            return Err(Error::UnsupportedFeature(format!(
                "tile compression type {:?} is not supported; only Rice is supported",
                comp.cmp_type
            )));
        }

        if comp.quantize.is_some() {
            return Err(Error::UnsupportedFeature(
                "quantized float tile compression (ZQUANTIZ) is not yet supported".into(),
            ));
        }

        let (block_size, byte_pix) = match comp.algo_params {
            AlgoParams::Rice {
                block_size,
                byte_pix,
            } => (block_size, byte_pix),
            _ => unreachable!(),
        };

        let col = layout
            .column_by_name("COMPRESSED_DATA")
            .ok_or_else(|| {
                Error::InvalidHDU("no COMPRESSED_DATA column in tile-compressed image".into())
            })?
            .clone();

        let total_pixels: usize = comp.tiles.image_shape.iter().map(|&d| d as usize).product();
        let mut output = vec![0i32; total_pixels];

        for tile_index in 0..comp.tiles.total_tiles() {
            let n_pixels = comp.tiles.tile_n_pixels(tile_index);
            let descriptor =
                layout.read_vla_descriptor(&mut self.reader, data_offset, tile_index, &col)?;
            let bytes = layout.read_heap_bytes(&mut self.reader, data_offset, descriptor, &col)?;
            let pixels = rice::rice_decompress(&bytes, n_pixels, block_size, byte_pix)?;
            comp.tiles.place_tile(&pixels, tile_index, &mut output);
        }

        let bscale = header.bscale()?;
        let bzero = header.bzero()?;
        Ok(convert_compressed_pixels(
            output,
            comp.bitpix,
            bscale,
            bzero,
        ))
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
            hdu_kind_from_extension_header(&header)?
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
        testutil::*,
    };

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

    fn make_compressed_image_extension(znaxis1: i64, znaxis2: i64) -> Header {
        let mut h = Header::new();
        h.append(Card::Xtension {
            x: XtensionType::BinaryTable,
            comment: None,
        });
        h.append(bool_card("ZIMAGE", true));
        h.append(int_card("BITPIX", 8));
        h.append(int_card("NAXIS", 2));
        h.append(int_card("NAXIS1", 8)); // bytes per tile row in the table
        h.append(int_card("NAXIS2", 1)); // one tile
        h.append(int_card("PCOUNT", 0));
        h.append(int_card("GCOUNT", 1));
        h.append(Card::Value {
            keyword: "ZCMPTYPE".into(),
            value: CardValue::String("RICE_1".into()),
            comment: None,
        });
        h.append(int_card("ZNAXIS", 2));
        h.append(int_card("ZNAXIS1", znaxis1));
        h.append(int_card("ZNAXIS2", znaxis2));
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
        let expected: Vec<i16> = [1i16, -1i16].to_vec();
        let data: Vec<u8> = expected.iter().flat_map(|v| v.to_be_bytes()).collect();
        let buf = write_hdu(&header, &data);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.read_image_as::<i16>(0).unwrap(), expected);
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
        let expected: Vec<i16> = [1i16, 2i16].to_vec();
        let data: Vec<u8> = expected.iter().flat_map(|v| v.to_be_bytes()).collect();
        let buf = write_hdu(&header, &data);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        let image = fits.read_image_raw(0).unwrap();
        match image {
            ImageData::I16(pixels) => assert_eq!(pixels, expected),
            _ => panic!("expected ImageData::I16"),
        }
    }

    #[test]
    fn test_read_image_u16_bzero() {
        // BITPIX=16 + BZERO=32768: i16 stored, u16 physical via MSB flip
        let mut h = Header::new();
        h.append(Card::Value {
            keyword: "SIMPLE".into(),
            value: CardValue::Logical(true),
            comment: None,
        });
        h.append(int_card("BITPIX", 16));
        h.append(int_card("NAXIS", 1));
        h.append(int_card("NAXIS1", 2));
        h.append(float_card("BZERO", 32768.0));
        h.append(float_card("BSCALE", 1.0));
        h.append(Card::End);
        let data: Vec<u8> = [i16::MIN, i16::MAX]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let buf = write_hdu(&h, &data);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        match fits.read_image(0).unwrap() {
            ImageData::U16(pixels) => assert_eq!(pixels, vec![0u16, 65535u16]),
            _ => panic!("expected ImageData::U16"),
        }
    }

    #[test]
    fn test_read_image_f64_scaling() {
        // Arbitrary BSCALE/BZERO on integer data
        let mut h = Header::new();
        h.append(Card::Value {
            keyword: "SIMPLE".into(),
            value: CardValue::Logical(true),
            comment: None,
        });
        h.append(int_card("BITPIX", 8));
        h.append(int_card("NAXIS", 1));
        h.append(int_card("NAXIS1", 2));
        h.append(float_card("BSCALE", 2.0));
        h.append(float_card("BZERO", 10.0));
        h.append(Card::End);
        // physical = 10.0 + 2.0 * stored: [5, 0] = [20.0, 10.0]
        let buf = write_hdu(&h, &[5u8, 0u8]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        match fits.read_image(0).unwrap() {
            ImageData::F64(pixels) => assert_eq!(pixels, vec![20.0f64, 10.0f64]),
            _ => panic!("expected ImageData::F64"),
        }
    }

    #[test]
    fn test_is_image() {
        let entry = |kind| HduEntry {
            index: 0,
            header_offset: 0,
            data_offset: 0,
            data_len: 0,
            kind,
            name: None,
            version: None,
        };
        assert!(entry(HduKind::Primary).is_image());
        assert!(entry(HduKind::Image).is_image());
        assert!(entry(HduKind::CompressedImage).is_image());
        assert!(!entry(HduKind::BinaryTable).is_image());
        assert!(!entry(HduKind::AsciiTable).is_image());
    }

    #[test]
    fn test_scan_detects_compressed_image() {
        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&make_compressed_image_extension(100, 50), &[]));
        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.hdu(1).unwrap().kind, HduKind::CompressedImage);
    }

    #[test]
    fn test_scan_plain_bintable_not_compressed() {
        let mut h = Header::new();
        h.append(Card::Xtension {
            x: XtensionType::BinaryTable,
            comment: None,
        });
        h.append(int_card("BITPIX", 8));
        h.append(int_card("NAXIS", 2));
        h.append(int_card("NAXIS1", 8));
        h.append(int_card("NAXIS2", 1));
        h.append(int_card("PCOUNT", 0));
        h.append(int_card("GCOUNT", 1));
        h.append(Card::End);

        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&h, &[]));
        let fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.hdu(1).unwrap().kind, HduKind::BinaryTable);
    }

    #[test]
    fn test_read_image_compressed_invalid_layout_errors() {
        // The mock header has no TFIELDS, so BinTableLayout::from_header fails.
        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&make_compressed_image_extension(100, 50), &[]));
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert!(fits.read_image(1).is_err());
    }

    #[test]
    fn test_find_image_compressed() {
        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&make_compressed_image_extension(100, 50), &[]));
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.find_image().unwrap(), Some(1));
    }

    #[test]
    fn test_find_image_skips_compressed_with_zero_dimension() {
        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&make_compressed_image_extension(0, 50), &[]));
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.find_image().unwrap(), None);
    }

    #[test]
    fn test_find_image_primary_2d() {
        let buf = write_hdu(&make_primary_header(16, &[100, 50]), &[]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.find_image().unwrap(), Some(0));
    }

    #[test]
    fn test_find_image_skips_empty_primary() {
        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]); // no image data
        buf.extend(write_hdu(&make_image_extension(16, &[100, 50]), &[]));
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.find_image().unwrap(), Some(1));
    }

    #[test]
    fn test_find_image_skips_1d() {
        // Not 2d
        let buf = write_hdu(&make_primary_header(16, &[100]), &[]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.find_image().unwrap(), None);
    }

    #[test]
    fn test_find_image_skips_zero_dimension() {
        // NAXIS1 = 0, skip
        let buf = write_hdu(&make_primary_header(16, &[0, 50]), &[]);
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();
        assert_eq!(fits.find_image().unwrap(), None);
    }

    // --- convert_compressed_pixels ---

    #[test]
    fn test_convert_no_scaling() {
        assert!(matches!(
            convert_compressed_pixels(vec![1, 2], Bitpix::UnsignedByte, 1.0, 0.0),
            ImageData::U8(v) if v == vec![1u8, 2]
        ));
        assert!(matches!(
            convert_compressed_pixels(vec![-1, 32767], Bitpix::SignedShort, 1.0, 0.0),
            ImageData::I16(v) if v == vec![-1i16, 32767]
        ));
        assert!(matches!(
            convert_compressed_pixels(vec![i32::MIN, i32::MAX], Bitpix::SignedInt, 1.0, 0.0),
            ImageData::I32(v) if v == vec![i32::MIN, i32::MAX]
        ));
    }

    #[test]
    fn test_convert_unsigned_conventions() {
        // BZERO=-128: u8 stored as signed → flip MSB to get i8
        // Stored 0→physical -128, stored 128→physical 0, stored 255→physical 127
        assert!(matches!(
            convert_compressed_pixels(vec![0, 128, 255], Bitpix::UnsignedByte, 1.0, -128.0),
            ImageData::I8(v) if v == vec![-128i8, 0, 127]
        ));
        // BZERO=32768: i16 stored → flip MSB to get u16
        // Stored -32768→physical 0, stored 0→physical 32768, stored 32767→physical 65535
        assert!(matches!(
            convert_compressed_pixels(vec![-32768, 0, 32767], Bitpix::SignedShort, 1.0, 32768.0),
            ImageData::U16(v) if v == vec![0u16, 32768, 65535]
        ));
        // BZERO=2147483648: i32 stored → flip MSB to get u32
        assert!(matches!(
            convert_compressed_pixels(vec![i32::MIN, 0, i32::MAX], Bitpix::SignedInt, 1.0, 2147483648.0),
            ImageData::U32(v) if v == vec![0u32, 2147483648, u32::MAX]
        ));
    }

    #[test]
    fn test_convert_arbitrary_scaling() {
        // bscale=2.0, bzero=10.0: physical = 10 + 2*stored
        match convert_compressed_pixels(vec![5, 0, -3], Bitpix::SignedInt, 2.0, 10.0) {
            ImageData::F64(v) => assert_eq!(v, vec![20.0, 10.0, 4.0]),
            other => panic!("expected F64, got {other:?}"),
        }
    }

    // --- read_compressed_image ---

    fn make_rice_bintable_header(
        znaxis1: i64,
        znaxis2: i64,
        zbitpix: i64,
        byte_pix: i64,
        heap_bytes: i64,
        extra: impl FnOnce(&mut Header),
    ) -> Header {
        let mut h = Header::new();
        h.append(Card::Xtension {
            x: XtensionType::BinaryTable,
            comment: None,
        });
        h.append(int_card("BITPIX", 8));
        h.append(int_card("NAXIS", 2));
        h.append(int_card("NAXIS1", 8)); // P descriptor = 8 bytes/row
        h.append(int_card("NAXIS2", znaxis1 * znaxis2 / znaxis1)); // one tile per row
        h.append(int_card("PCOUNT", heap_bytes));
        h.append(int_card("GCOUNT", 1));
        h.append(int_card("TFIELDS", 1));
        h.append(str_card("TTYPE1", "COMPRESSED_DATA"));
        h.append(str_card("TFORM1", &format!("1PB({heap_bytes})")));
        h.append(bool_card("ZIMAGE", true));
        h.append(str_card("ZCMPTYPE", "RICE_1"));
        h.append(int_card("ZNAXIS", 2));
        h.append(int_card("ZNAXIS1", znaxis1));
        h.append(int_card("ZNAXIS2", znaxis2));
        h.append(int_card("ZBITPIX", zbitpix));
        h.append(int_card("ZVAL1", 4)); // block_size=4
        h.append(int_card("ZVAL2", byte_pix));
        extra(&mut h);
        h.append(Card::End);
        h
    }

    fn p_descriptor(count: i32, offset: i32) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&count.to_be_bytes());
        buf[4..8].copy_from_slice(&offset.to_be_bytes());
        buf
    }

    #[test]
    fn test_read_compressed_image_i32() {
        // 4x1 Rice-compressed image, pixels [100, 101, 102, 103].
        // Compressed bytes from test_rice_sequential_increasing.
        let compressed = [0x00u8, 0x00, 0x00, 0x64, 0x0C, 0x92];
        let h = make_rice_bintable_header(4, 1, 32, 4, 6, |_| {});
        let mut data = p_descriptor(6, 0).to_vec();
        data.extend_from_slice(&compressed);

        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&h, &data));
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        match fits.read_image(1).unwrap() {
            ImageData::I32(pixels) => assert_eq!(pixels, vec![100, 101, 102, 103]),
            other => panic!("expected I32, got {other:?}"),
        }
    }

    #[test]
    fn test_read_compressed_image_u16_bzero() {
        // 2x1 Rice-compressed image, original pixels [42, 42] stored as i16.
        // With BZERO=32768 the physical type is u16.
        // Low-entropy Rice: first pixel [0x00, 0x2A] + FS byte 0x00.
        let compressed = [0x00u8, 0x2A, 0x00];
        let h = make_rice_bintable_header(2, 1, 16, 2, 3, |h| {
            h.append(float_card("BZERO", 32768.0));
        });
        let mut data = p_descriptor(3, 0).to_vec();
        data.extend_from_slice(&compressed);

        let mut buf = write_hdu(&make_primary_header(8, &[]), &[]);
        buf.extend(write_hdu(&h, &data));
        let mut fits = Fits::from_reader(Cursor::new(buf)).unwrap();

        // 42 stored as i16 + BZERO=32768: physical = (42u16) ^ 0x8000 = 32810
        match fits.read_image(1).unwrap() {
            ImageData::U16(pixels) => assert_eq!(pixels, vec![32810u16, 32810]),
            other => panic!("expected U16, got {other:?}"),
        }
    }
}
