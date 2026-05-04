use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid Block: {0}")]
    InvalidBlock(String),
    #[error("Invalid Card: {0}")]
    InvalidCard(String),
    #[error("InvalidHeader: {0}")]
    InvalidHeader(String), // TODO: MissingKeyword, InvalidKeyword, etc
    #[error("InvalidHDU: {0}")]
    InvalidHDU(String), // TODO split into structured variants: UnknownXtension type
    #[error("Unsupported Feature: {0}")]
    UnsupportedFeature(String),
    #[error("Checksum Mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: u32, actual: u32 },
    #[error("Type mismatch: {0}")]
    TypeMismatch(String), // TODO Split into structured variants: Actual/expected
    #[error("UTF-8 encoding error: {0}")]
    EncodingError(#[from] std::str::Utf8Error),
}
