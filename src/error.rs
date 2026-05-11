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
    InvalidHeader(String),
    #[error("Missing required keyword: {0}")]
    MissingKeyword(&'static str),
    #[error("Keyword {keyword} has invalid value: {value}: {reason}")]
    InvalidKeywordValue {
        keyword: &'static str,
        value: String,
        reason: &'static str,
    },
    #[error("InvalidHDU: {0}")]
    InvalidHDU(String), // TODO split into structured variants: UnknownXtension type
    #[error("Unsupported Feature: {0}")]
    UnsupportedFeature(String),
    #[error("HDU index {0} not found")]
    HduNotFound(usize),
    #[error("Checksum Mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: u32, actual: u32 },
    #[error("Type mismatch: {0}")]
    TypeMismatch(String), // TODO Split into structured variants: Actual/expected
    #[error("UTF-8 encoding error: {0}")]
    EncodingError(#[from] std::str::Utf8Error),
}

pub type Result<T> = std::result::Result<T, Error>;
