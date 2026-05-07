use std::{fmt::Display, str::FromStr};

use crate::error::Error;

/// 4.4.1.2 XTENSION keyword values identifying the HDU type.
#[derive(Debug, PartialEq, Clone)]
pub enum XtensionType {
    Image,
}

impl FromStr for XtensionType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "IMAGE" => Ok(XtensionType::Image),
            other => Err(Error::UnsupportedFeature(format!(
                "unknown XTENSION type: '{other}'"
            ))),
        }
    }
}

impl Display for XtensionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XtensionType::Image => write!(f, "IMAGE"),
        }
    }
}
