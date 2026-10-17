//! Office Open XML — ECMA-376 5th edition (= ISO/IEC 29500:2016/2021).
//! `opc` = Part 2 Open Packaging Conventions. `xlsx` = Part 1 §18 SpreadsheetML. `docx` = Part 1 §17 WordprocessingML.
//! Patent note: ECMA-376 / ISO/IEC 29500 are covered by Microsoft's Open Specification Promise (docs/ooxml.md).

pub mod opc;
pub mod docx;
pub mod xlsx;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Zip(crate::zip::Error),
    Xml(crate::xml::Error),
    /// A required part or relationship is absent.
    Missing(String),
    /// Structurally invalid content (bad cell reference, unknown sheet, …).
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "zip: {e}"),
            Error::Xml(e) => write!(f, "{e}"),
            Error::Missing(s) => write!(f, "missing {s}"),
            Error::Invalid(s) => write!(f, "invalid: {s}"),
        }
    }
}
impl std::error::Error for Error {}

impl From<crate::zip::Error> for Error {
    fn from(e: crate::zip::Error) -> Self {
        Error::Zip(e)
    }
}
impl From<crate::xml::Error> for Error {
    fn from(e: crate::xml::Error) -> Self {
        Error::Xml(e)
    }
}
