use crate::{
    card::CardValue,
    error::{Error, Result},
    header::Header,
};

/// The data type of elements stored in the heap for a Variable-Length Array column.
/// Represents the `t` character in the TFORM `rPt(emax)` / `rQt(emax)` format.
/// P and Q are not valid element types. 7.3.5
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VlaElementType {
    Logical,
    Bit,
    UnsignedByte,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
    Complex32,
    Complex64,
    Char,
}

impl VlaElementType {
    /// Bytes per element in the heap. 7.3.2, Table 18
    pub fn byte_size(&self) -> u64 {
        match self {
            Self::Logical => 1,
            Self::Bit => 1,
            Self::UnsignedByte => 1,
            Self::Int16 => 2,
            Self::Int32 => 4,
            Self::Int64 => 8,
            Self::Float32 => 4,
            Self::Float64 => 8,
            Self::Complex32 => 8,
            Self::Complex64 => 16,
            Self::Char => 1,
        }
    }

    pub fn from_char(c: char, tform: &str) -> Result<Self> {
        match c {
            'L' => Ok(Self::Logical),
            'X' => Ok(Self::Bit),
            'B' => Ok(Self::UnsignedByte),
            'I' => Ok(Self::Int16),
            'J' => Ok(Self::Int32),
            'K' => Ok(Self::Int64),
            'A' => Ok(Self::Char),
            'E' => Ok(Self::Float32),
            'D' => Ok(Self::Float64),
            'C' => Ok(Self::Complex32),
            'M' => Ok(Self::Complex64),
            _ => Err(Error::InvalidHeader(format!(
                "unknown VLA element type '{c}' in TFORM '{tform}'"
            ))),
        }
    }
}

/// Parsed TFORM keyword describing a binary table column's data type and storage.
/// 7.3.2, Table 18
///
/// The `u32` on these variants is the repeat count (elements per row).
/// This is the `r` in`rTa`.
/// VLA variants always occupy exactly 8 bytes (P) or 16 bytes (Q) in the row
/// actual array data lives in the heap - this is where compressed images live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TForm {
    Logical(u32),      // L - 1 bytes per element
    Bit(u32),          // X - packed bits ceil(r/8) bytes
    UnsignedByte(u32), // B - 1 bytes per element
    Int16(u32),        // I - 2 bytes per element
    Int32(u32),        // J - 4 bytes per element
    Int64(u32),        // K - 8 bytes per element
    Char(u32),         // A - 1 bytes per element
    Float32(u32),      // E - 4 bytes per element
    Float64(u32),      // D - 8 bytes per element
    Complex32(u32),    // C - 8 bytes (f32 real + f32 imag)
    Complex64(u32),    // M - 16 bytes (f64 real + f64 imag)
    VarArrayP {
        element_type: VlaElementType,
        emax: u64,
    }, // P - 8 bytes
    VarArrayQ {
        element_type: VlaElementType,
        emax: u64,
    }, // Q - 16 bytes
}

impl TForm {
    /// Bytes this column occupies in a single table row.
    /// See Table 18 section 7.3.2
    pub fn row_bytes(&self) -> u64 {
        match self {
            TForm::Logical(r) => *r as u64,
            TForm::Bit(r) => (*r as u64).div_ceil(8),
            TForm::UnsignedByte(r) => *r as u64,
            TForm::Int16(r) => *r as u64 * 2,
            TForm::Int32(r) => *r as u64 * 4,
            TForm::Int64(r) => *r as u64 * 8,
            TForm::Char(r) => *r as u64,
            TForm::Float32(r) => *r as u64 * 4,
            TForm::Float64(r) => *r as u64 * 8,
            TForm::Complex32(r) => *r as u64 * 8,
            TForm::Complex64(r) => *r as u64 * 16,
            TForm::VarArrayP { .. } => 8,
            TForm::VarArrayQ { .. } => 16,
        }
    }

    pub fn is_vla(&self) -> bool {
        matches!(self, TForm::VarArrayP { .. } | TForm::VarArrayQ { .. })
    }

    /// Parse a TFORM string value. General format is `rTa` (7.3.2): `r` is an
    /// optional repeat count (defaults to 1), `T` is the type code, and `a` is
    /// optional trailing characters whose meaning is type-specific. For P/Q, 7.3.5
    /// defines `a` as `t(emax)` — the element type and optional max element count.
    /// All other types do not parse the `a` field and return from this function early.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();

        // finds where the leading digits stop and then parses the repeat count
        // No digits means default repeat count of 1
        let digit_end = s.bytes().take_while(|b| b.is_ascii_digit()).count();
        let repeat: u32 = if digit_end == 0 {
            1
        } else {
            s[..digit_end]
                .parse::<u32>()
                .map_err(|_| Error::InvalidHeader(format!("invalid repeat count in TFORM '{s}'")))?
        };
        let rest = &s[digit_end..];

        let type_char = rest
            .chars()
            .next()
            .ok_or_else(|| Error::InvalidHeader(format!("empty TFORM '{s}'")))?;
        let after_type = &rest[type_char.len_utf8()..];

        match type_char {
            'L' => Ok(TForm::Logical(repeat)),
            'X' => Ok(TForm::Bit(repeat)),
            'B' => Ok(TForm::UnsignedByte(repeat)),
            'I' => Ok(TForm::Int16(repeat)),
            'J' => Ok(TForm::Int32(repeat)),
            'K' => Ok(TForm::Int64(repeat)),
            'A' => Ok(TForm::Char(repeat)),
            'E' => Ok(TForm::Float32(repeat)),
            'D' => Ok(TForm::Float64(repeat)),
            'C' => Ok(TForm::Complex32(repeat)),
            'M' => Ok(TForm::Complex64(repeat)),
            // Only P and Q are VLA types and need to read the optional `a` of rTa
            'P' | 'Q' => {
                let elem_char = after_type.chars().next().ok_or_else(|| {
                    Error::InvalidHeader(format!("missing element type in VLA TFORM '{s}'"))
                })?;
                let element_type = VlaElementType::from_char(elem_char, s)?;
                // Now parse the (emax) part of the VLA TFORM string
                let after_elem = &after_type[elem_char.len_utf8()..];
                let emax = parse_emax(after_elem);
                if type_char == 'P' {
                    Ok(TForm::VarArrayP { element_type, emax })
                } else {
                    Ok(TForm::VarArrayQ { element_type, emax })
                }
            }
            c => Err(Error::InvalidHeader(format!(
                "unknown TFORM type code '{c}' in '{s}'"
            ))),
        }
    }
}

fn parse_emax(s: &str) -> u64 {
    if s.starts_with('(') {
        let close = s.find(')').unwrap_or(s.len());
        s[1..close].parse().unwrap_or(0)
    } else {
        0
    }
}

/// Metadata for a single binary table column. 7.3.2
///
/// Bundles the column header keywords (TTYPEn, TFORMn, TUNITn) with
/// `byte_offset`, which is computed by summing
/// `row_bytes()` across all preceding columns.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDesc {
    /// Column name from TTYPEn. Optional per spec.
    pub name: Option<String>,
    /// Physical unit from TUNITn. Optional per spec.
    pub unit: Option<String>,
    /// Parsed TFORMn.
    pub form: TForm,
    /// Byte offset of this column within a row.
    pub byte_offset: u64,
}

/// Description of a FITS binary table. 7.3
#[derive(Debug, Clone, PartialEq)]
pub struct BinTableLayout {
    /// Number of rows (NAXIS2).
    pub nrows: u64,
    /// Bytes per row (NAXIS1).
    pub row_width: u64,
    /// Total heap size in bytes (PCOUNT). Zero for tables without VLA columns. 7.3.5
    pub pcount: u64,
    /// Byte offset from the start of the main data table to the heap (THEAP).
    /// Defaults to `nrows x row_width` when THEAP is absent. 7.3.5
    pub heap_offset: u64,
    pub columns: Vec<ColumnDesc>,
}

impl BinTableLayout {
    pub fn from_header(h: &Header) -> Result<Self> {
        let nrows = h.naxisn(2)?;
        let row_width = h.naxisn(1)?;

        let pcount = match h.get_value("PCOUNT") {
            Some(v) => v.as_integer().ok_or_else(|| Error::InvalidKeywordValue {
                keyword: "PCOUNT",
                value: format!("{v:?}"),
                reason: "must be an integer",
            })? as u64,
            None => 0,
        };

        let tfields = match h.get_value("TFIELDS") {
            // TODO: this pattern is getting annoying. Need to add h.get_value_as
            // or get_value::<T>
            Some(CardValue::Integer(i)) if *i >= 0 => *i as usize,
            Some(v) => {
                return Err(Error::InvalidKeywordValue {
                    keyword: "TFIELDS",
                    value: format!("{v:?}"),
                    reason: "must be a non-negative integer",
                });
            }
            None => return Err(Error::MissingKeyword("TFIELDS")),
        };

        let mut columns = Vec::with_capacity(tfields);
        let mut computed_width = 0u64;

        for n in 1..=tfields {
            let form_str = match h.get_value(&format!("TFORM{n}")) {
                Some(CardValue::String(s)) => s.clone(),
                Some(v) => {
                    return Err(Error::InvalidHeader(format!(
                        "TFORM{n} must be a string, got {v:?}"
                    )));
                }
                None => return Err(Error::InvalidHeader(format!("missing TFORM{n}"))),
            };

            let form = TForm::parse(&form_str)?;
            let byte_offset = computed_width;
            computed_width += form.row_bytes();

            let name = h
                .get_value(&format!("TTYPE{n}"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());

            let unit = h
                .get_value(&format!("TUNIT{n}"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());

            columns.push(ColumnDesc {
                name,
                unit,
                form,
                byte_offset,
            });
        }

        if computed_width != row_width {
            return Err(Error::InvalidHeader(format!(
                "computed row width {computed_width} does not match NAXIS1 {row_width}"
            )));
        }

        let heap_offset = match h.get_value("THEAP") {
            Some(v) => v.as_integer().ok_or_else(|| Error::InvalidKeywordValue {
                keyword: "THEAP",
                value: format!("{v:?}"),
                reason: "must be an integer",
            })? as u64,
            None => row_width * nrows,
        };

        Ok(BinTableLayout {
            nrows,
            row_width,
            pcount,
            heap_offset,
            columns,
        })
    }

    /// Find the first column whose name matches (case-insensitive). 7.3.2
    pub fn column_by_name(&self, name: &str) -> Option<&ColumnDesc> {
        self.columns.iter().find(|c| {
            c.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{card::Card, extension::XtensionType, header::Header, testutil::*};

    fn make_bintable_header(tfields: i64, naxis1: i64, naxis2: i64) -> Header {
        let mut h = Header::new();
        h.append(Card::Xtension {
            x: XtensionType::BinaryTable,
            comment: None,
        });
        h.append(int_card("BITPIX", 8));
        h.append(int_card("NAXIS", 2));
        h.append(int_card("NAXIS1", naxis1));
        h.append(int_card("NAXIS2", naxis2));
        h.append(int_card("PCOUNT", 0));
        h.append(int_card("GCOUNT", 1));
        h.append(int_card("TFIELDS", tfields));
        h
    }

    #[test]
    fn test_layout_single_column() {
        let mut h = make_bintable_header(1, 4, 10);
        h.append(str_card("TTYPE1", "TIME"));
        h.append(str_card("TFORM1", "1J"));
        h.append(str_card("TUNIT1", "s"));

        let layout = BinTableLayout::from_header(&h).unwrap();
        assert_eq!(layout.nrows, 10);
        assert_eq!(layout.row_width, 4);
        assert_eq!(layout.pcount, 0);
        assert_eq!(layout.heap_offset, 40); // default: row_width x nrows
        assert_eq!(layout.columns.len(), 1);
        let col = &layout.columns[0];
        assert_eq!(col.name.as_deref(), Some("TIME"));
        assert_eq!(col.form, TForm::Int32(1));
        assert_eq!(col.unit.as_deref(), Some("s"));
        assert_eq!(col.byte_offset, 0);
    }

    #[test]
    fn test_layout_column_offsets() {
        // I(2 bytes) + J(4 bytes) + D(8 bytes) = 14 bytes per row
        let mut h = make_bintable_header(3, 14, 5);
        h.append(str_card("TFORM1", "1I"));
        h.append(str_card("TFORM2", "1J"));
        h.append(str_card("TFORM3", "1D"));

        let layout = BinTableLayout::from_header(&h).unwrap();
        assert_eq!(layout.columns[0].byte_offset, 0);
        assert_eq!(layout.columns[1].byte_offset, 2);
        assert_eq!(layout.columns[2].byte_offset, 6);
    }

    #[test]
    fn test_layout_vla_column() {
        // PB(1800) = 8 bytes in row heap has the actual compressed data
        let mut h = make_bintable_header(1, 8, 3);
        h.append(str_card("TTYPE1", "COMPRESSED_DATA"));
        h.append(str_card("TFORM1", "PB(1800)"));
        h.set(int_card("PCOUNT", 5400));

        let layout = BinTableLayout::from_header(&h).unwrap();
        assert_eq!(layout.pcount, 5400);
        assert!(layout.columns[0].form.is_vla());
        assert_eq!(layout.columns[0].form.row_bytes(), 8);
    }

    #[test]
    fn test_layout_explicit_theap() {
        let mut h = make_bintable_header(1, 8, 3);
        h.append(str_card("TFORM1", "PB(100)"));
        h.append(int_card("PCOUNT", 300));
        h.append(int_card("THEAP", 2880)); // gap aligns heap to block boundary

        let layout = BinTableLayout::from_header(&h).unwrap();
        assert_eq!(layout.heap_offset, 2880);
    }

    #[test]
    fn test_layout_column_by_name() {
        let mut h = make_bintable_header(2, 16, 1);
        h.append(str_card("TTYPE1", "RA"));
        h.append(str_card("TFORM1", "1D"));
        h.append(str_card("TTYPE2", "DEC"));
        h.append(str_card("TFORM2", "1D"));

        let layout = BinTableLayout::from_header(&h).unwrap();
        assert_eq!(layout.column_by_name("DEC").unwrap().byte_offset, 8);
        assert_eq!(layout.column_by_name("dec").unwrap().byte_offset, 8);
        assert!(layout.column_by_name("FLUX").is_none());
    }

    #[test]
    fn test_layout_row_width_mismatch_errors() {
        let mut h = make_bintable_header(1, 10, 5); // sets naxis1 to 10
        h.append(str_card("TFORM1", "1J")); // 4 bytes

        assert!(BinTableLayout::from_header(&h).is_err());
    }

    #[test]
    fn test_layout_missing_tfields_errors() {
        let mut h = Header::new();
        h.append(int_card("NAXIS", 2));
        h.append(int_card("NAXIS1", 4));
        h.append(int_card("NAXIS2", 1));

        assert!(matches!(
            BinTableLayout::from_header(&h).unwrap_err(),
            Error::MissingKeyword("TFIELDS")
        ));
    }

    #[test]
    fn test_layout_missing_tform_errors() {
        let h = make_bintable_header(1, 4, 1);
        // TFIELDS=1 but no TFORM1
        assert!(BinTableLayout::from_header(&h).is_err());
    }

    #[test]
    fn test_vla_element_type_from_char() {
        assert_eq!(
            // 'B' is the element type char extracted from "PB(1800)" by TForm::parse
            // the full string is passed only for error context
            VlaElementType::from_char('B', "PB(100)").unwrap(), // 7.3.5 rPt(emax) format
            VlaElementType::UnsignedByte
        );
        assert_eq!(
            VlaElementType::from_char('J', "PJ").unwrap(),
            VlaElementType::Int32
        );
        assert!(VlaElementType::from_char('Z', "PZ").is_err());
    }

    #[test]
    fn test_tform_parse_no_repeat_defaults_to_one() {
        assert_eq!(TForm::parse("J").unwrap(), TForm::Int32(1));
        assert_eq!(TForm::parse("D").unwrap(), TForm::Float64(1));
    }

    #[test]
    fn test_tform_parse_all_fixed_types() {
        assert_eq!(TForm::parse("1L").unwrap(), TForm::Logical(1));
        assert_eq!(TForm::parse("1X").unwrap(), TForm::Bit(1));
        assert_eq!(TForm::parse("1B").unwrap(), TForm::UnsignedByte(1));
        assert_eq!(TForm::parse("1I").unwrap(), TForm::Int16(1));
        assert_eq!(TForm::parse("1J").unwrap(), TForm::Int32(1));
        assert_eq!(TForm::parse("1K").unwrap(), TForm::Int64(1));
        assert_eq!(TForm::parse("1E").unwrap(), TForm::Float32(1));
        assert_eq!(TForm::parse("1D").unwrap(), TForm::Float64(1));
        assert_eq!(TForm::parse("1C").unwrap(), TForm::Complex32(1));
        assert_eq!(TForm::parse("1M").unwrap(), TForm::Complex64(1));
        assert_eq!(TForm::parse("1A").unwrap(), TForm::Char(1));
    }

    #[test]
    fn test_tform_parse_repeat() {
        assert_eq!(TForm::parse("100E").unwrap(), TForm::Float32(100));
        assert_eq!(TForm::parse("8A").unwrap(), TForm::Char(8));
    }

    #[test]
    fn test_tform_parse_vla_p_with_emax() {
        // 7.3.5 example: TFORM8 = 'PB(1800)'
        assert_eq!(
            TForm::parse("PB(1800)").unwrap(),
            TForm::VarArrayP {
                element_type: VlaElementType::UnsignedByte,
                emax: 1800
            }
        );
    }

    #[test]
    fn test_tform_parse_vla_p_with_explicit_r() {
        // r=1 is the typical explicit form
        assert_eq!(
            TForm::parse("1PB(1800)").unwrap(),
            TForm::VarArrayP {
                element_type: VlaElementType::UnsignedByte,
                emax: 1800
            }
        );
    }

    #[test]
    fn test_tform_parse_vla_q() {
        assert_eq!(
            TForm::parse("QB(32768)").unwrap(),
            TForm::VarArrayQ {
                element_type: VlaElementType::UnsignedByte,
                emax: 32768
            }
        );
    }

    #[test]
    fn test_tform_parse_vla_no_emax() {
        assert_eq!(
            TForm::parse("PJ").unwrap(),
            TForm::VarArrayP {
                element_type: VlaElementType::Int32,
                emax: 0
            }
        );
    }

    #[test]
    fn test_tform_parse_unknown_type_errors() {
        assert!(TForm::parse("1Z").is_err());
    }

    #[test]
    fn test_tform_parse_vla_unknown_element_errors() {
        assert!(TForm::parse("PZ").is_err());
    }

    #[test]
    fn test_tform_row_bytes_fixed() {
        assert_eq!(TForm::Int16(3).row_bytes(), 6);
        assert_eq!(TForm::Int32(1).row_bytes(), 4);
        assert_eq!(TForm::Float64(2).row_bytes(), 16);
        assert_eq!(TForm::Complex32(1).row_bytes(), 8);
        assert_eq!(TForm::Complex64(1).row_bytes(), 16);
        assert_eq!(TForm::Char(20).row_bytes(), 20);
    }

    #[test]
    fn test_tform_row_bytes_bit_packs() {
        // X packs bits so this is ceil(r/8) bytes total
        assert_eq!(TForm::Bit(1).row_bytes(), 1);
        assert_eq!(TForm::Bit(8).row_bytes(), 1);
        assert_eq!(TForm::Bit(9).row_bytes(), 2);
        assert_eq!(TForm::Bit(16).row_bytes(), 2);
        assert_eq!(TForm::Bit(17).row_bytes(), 3);
    }

    #[test]
    fn test_tform_row_bytes_vla_is_descriptor_size() {
        // VLA row bytes = descriptor size only regardless of emax (the declared max number of elements)
        assert_eq!(
            TForm::VarArrayP {
                element_type: VlaElementType::UnsignedByte,
                emax: 99999
            }
            .row_bytes(),
            8
        );
        assert_eq!(
            TForm::VarArrayQ {
                element_type: VlaElementType::Int32,
                emax: 99999
            }
            .row_bytes(),
            16
        );
    }
}
