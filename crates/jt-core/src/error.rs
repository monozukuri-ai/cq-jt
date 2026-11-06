use crate::SourceSpan;
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    InvalidHeader,
    UnsupportedVersion,
    InvalidToc,
    InvalidSegment,
    InvalidCompression,
    InvalidElement,
    UnsupportedEncoding,
    MissingReference,
    InvalidGraph,
    InvalidGeometry,
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub code: &'static str,
    pub message: String,
    pub source: Option<SourceSpan>,
}

impl ParseError {
    pub(crate) fn at(
        kind: ParseErrorKind,
        code: &'static str,
        message: impl Into<String>,
        source_id: &str,
        offset: u64,
        length: u64,
    ) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            source: (!source_id.is_empty()).then(|| span(source_id, offset, length)),
        }
    }
}

pub(crate) fn span(source_id: &str, offset: u64, length: u64) -> SourceSpan {
    SourceSpan {
        source_id: source_id.into(),
        byte_domain: "file".into(),
        start_offset: offset,
        end_offset: offset.saturating_add(length),
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl Error for ParseError {}

#[derive(Debug)]
pub enum InspectError {
    Io(std::io::Error),
    Parse(ParseError),
}
impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Parse(e) => e.fmt(f),
        }
    }
}
impl Error for InspectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Io(e) => e,
            Self::Parse(e) => e,
        })
    }
}
impl From<std::io::Error> for InspectError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<ParseError> for InspectError {
    fn from(value: ParseError) -> Self {
        Self::Parse(value)
    }
}
