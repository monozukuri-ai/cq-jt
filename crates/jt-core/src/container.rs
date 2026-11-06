//! JT container structures and identifiers: Copyright 2026 Siemens.
//! Used under Siemens JT Data Format Reference Intellectual Property License
//! Terms, JT 10.6 Rev D, page ii. No Siemens SDK license is granted here.
//! Layout sources: JT 10.6 section 4.1 and the pinned legacy reader references.

use crate::{error::span, io::Reader};
use crate::{
    Diagnostic, DiagnosticSeverity, InspectError, JtVersion, ParseError, ParseErrorKind,
    ReadLimits, SourceSpan,
};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::Arc,
};
use ParseErrorKind::{
    InvalidHeader, InvalidSegment, InvalidToc, LimitExceeded, UnsupportedVersion,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    Little,
    Big,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionHeader {
    pub flag: u32,
    /// Includes the algorithm byte; no compressed stream is decoded here.
    pub declared_length: i32,
    pub algorithm: u8,
    pub label: &'static str,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentInfo {
    pub guid_raw: [u8; 16],
    pub offset: u64,
    pub length: u32,
    pub segment_type: u32,
    pub type_name: &'static str,
    pub attributes: u32,
    pub toc_source: SourceSpan,
    pub header_source: SourceSpan,
    pub payload_source: SourceSpan,
    pub compression: Option<CompressionHeader>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub source_id: String,
    pub size_bytes: u64,
    pub version: JtVersion,
    pub version_field: [u8; 80],
    pub header_comment: String,
    pub byte_order: ByteOrder,
    pub header_field: i32,
    pub header_size: u64,
    pub extra_header_guid: Option<[u8; 16]>,
    pub toc_offset: u64,
    pub toc_entry_width: u32,
    pub lsg_guid_raw: [u8; 16],
    pub segments: Vec<SegmentInfo>,
    pub diagnostics: Vec<Diagnostic>,
}
impl ContainerInfo {
    pub fn segment_type_counts(&self) -> BTreeMap<u32, usize> {
        let mut counts = BTreeMap::new();
        for segment in &self.segments {
            *counts.entry(segment.segment_type).or_default() += 1;
        }
        counts
    }
}

/// Owned bytes and validated container ranges. Payloads remain uninterpreted.
#[derive(Debug, Clone)]
pub struct JtDocument {
    bytes: Arc<[u8]>,
    info: ContainerInfo,
}
impl JtDocument {
    pub fn from_bytes(
        bytes: Arc<[u8]>,
        source_id: &str,
        limits: &ReadLimits,
    ) -> Result<Self, ParseError> {
        let info = inspect_bytes(&bytes, source_id, limits)?;
        Ok(Self { bytes, info })
    }
    pub fn open(path: &Path, limits: &ReadLimits) -> Result<Self, InspectError> {
        let source_id = path.to_string_lossy();
        let bytes = read_file(path, &source_id, limits)?;
        Ok(Self::from_bytes(bytes.into(), &source_id, limits)?)
    }
    pub fn info(&self) -> &ContainerInfo {
        &self.info
    }
    /// Entire segment, including its 24-byte header.
    pub fn segment_bytes(&self, index: usize) -> Option<&[u8]> {
        let segment = self.info.segments.get(index)?;
        self.bytes.get(
            usize::try_from(segment.offset).ok()?
                ..usize::try_from(segment.offset.checked_add(u64::from(segment.length))?).ok()?,
        )
    }
}

pub(crate) fn read_file(
    path: &Path,
    source_id: &str,
    limits: &ReadLimits,
) -> Result<Vec<u8>, InspectError> {
    let mut file = File::open(path)?;
    limits.check_input_size(file.metadata()?.len(), source_id)?;
    let mut data = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        let count = match file.read(&mut chunk) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        limits.check_input_size((data.len() as u64).saturating_add(count as u64), source_id)?;
        data.try_reserve(count).map_err(|_| {
            ParseError::at(
                LimitExceeded,
                "allocation_failed",
                "cannot allocate input buffer",
                source_id,
                0,
                0,
            )
        })?;
        data.extend_from_slice(&chunk[..count]);
    }
    Ok(data)
}

pub fn inspect_file(
    path: &Path,
    source_id: &str,
    limits: &ReadLimits,
) -> Result<ContainerInfo, InspectError> {
    let bytes = read_file(path, source_id, limits)?;
    Ok(inspect_bytes(&bytes, source_id, limits)?)
}

fn reserve<T>(
    items: &mut Vec<T>,
    count: usize,
    source_id: &str,
    offset: u64,
) -> Result<(), ParseError> {
    items.try_reserve_exact(count).map_err(|_| {
        ParseError::at(
            LimitExceeded,
            "allocation_failed",
            "cannot allocate container table",
            source_id,
            offset,
            4,
        )
    })
}

fn version(field: &[u8; 80], source_id: &str) -> Result<(JtVersion, String), ParseError> {
    let invalid = || {
        ParseError::at(
            InvalidHeader,
            "invalid_signature",
            "expected an ASCII 'Version major.minor' header",
            source_id,
            0,
            80,
        )
    };
    if !field.is_ascii() || !field.starts_with(b"Version ") {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&field[8..75]).map_err(|_| invalid())?;
    let end = text.find(' ').ok_or_else(invalid)?;
    let (major, minor) = text[..end].split_once('.').ok_or_else(invalid)?;
    if major.is_empty()
        || minor.is_empty()
        || !major
            .bytes()
            .chain(minor.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }
    let major = major.parse::<u16>().map_err(|_| invalid())?;
    let minor = minor.parse::<u16>().map_err(|_| invalid())?;
    if !matches!((major, minor), (8, 0..=2) | (9, 0..=5) | (10, 0..=6)) {
        return Err(ParseError::at(
            UnsupportedVersion,
            "unsupported_version",
            format!("unsupported JT container layout {major}.{minor}"),
            source_id,
            8,
            end as u64,
        ));
    }
    if &field[75..] != b" \n\r\n " && !(major == 8 && &field[75..] == b"     ") {
        return Err(ParseError::at(
            InvalidHeader,
            "invalid_signature_terminator",
            "invalid ASCII/binary translation marker",
            source_id,
            75,
            5,
        ));
    }
    Ok((JtVersion { major, minor }, text[end..].trim().to_owned()))
}

/// Inspect structural boundaries only; success is not payload/geometry validation.
pub fn inspect_bytes(
    data: &[u8],
    source_id: &str,
    limits: &ReadLimits,
) -> Result<ContainerInfo, ParseError> {
    limits.check_input_size(data.len() as u64, source_id)?;
    if source_id.is_empty() {
        return Err(ParseError::at(
            InvalidHeader,
            "invalid_source_id",
            "source_id must not be empty",
            source_id,
            0,
            0,
        ));
    }
    let mut reader = Reader {
        data,
        order: ByteOrder::Little,
        source_id,
        kind: InvalidHeader,
    };
    let version_field = reader.array::<80>(0)?;
    let (version, header_comment) = version(&version_field, source_id)?;
    let byte_order = match reader.array::<1>(80)?[0] {
        0 => ByteOrder::Little,
        1 => ByteOrder::Big,
        _ => {
            return Err(ParseError::at(
                InvalidHeader,
                "invalid_byte_order",
                "byte order must be 0 or 1",
                source_id,
                80,
                1,
            ))
        }
    };
    reader.order = byte_order;
    let header_field = reader.i32(81)?;
    let width = if version.major == 10 { 8 } else { 4 };
    let toc_offset = if width == 8 {
        reader.u64(85)?
    } else {
        let value = reader.i32(85)?;
        u64::try_from(value).map_err(|_| {
            ParseError::at(
                InvalidToc,
                "negative_toc_offset",
                "TOC offset must be nonnegative",
                source_id,
                85,
                4,
            )
        })?
    };
    let lsg_guid_raw = reader.array::<16>(85 + width)?;
    let mut header_size = 101 + width;
    // JT 9.5 Rev A Figure 2 and JT 10.6 Figure 11 both define this field.
    let extra_header_guid =
        if (version.major == 10 || (version.major, version.minor) == (9, 5)) && header_field != 0 {
            let value = reader.array::<16>(header_size)?;
            header_size += 16;
            Some(value)
        } else {
            None
        };
    if toc_offset < header_size {
        return Err(ParseError::at(
            InvalidToc,
            "toc_overlaps_header",
            "TOC overlaps the file header",
            source_id,
            85,
            width,
        ));
    }
    reader.kind = InvalidToc;
    let count = reader.i32(toc_offset)?;
    if count <= 0 {
        return Err(ParseError::at(
            InvalidToc,
            "invalid_entry_count",
            "TOC must have a positive entry count",
            source_id,
            toc_offset,
            4,
        ));
    }
    let count = count as u32;
    if count > limits.max_segments {
        return Err(ParseError::at(
            LimitExceeded,
            "segment_limit_exceeded",
            "TOC exceeds max_segments",
            source_id,
            toc_offset,
            4,
        ));
    }
    let toc_entry_width = 24 + width as u32;
    // Check the entire TOC before allocating or calculating individual entry positions.
    let toc_length = 4 + u64::from(count) * u64::from(toc_entry_width);
    reader.slice(toc_offset, toc_length)?;
    let toc_end = toc_offset + toc_length;
    let mut segments = Vec::new();
    reserve(&mut segments, count as usize, source_id, toc_offset)?;
    let mut ranges = Vec::new();
    reserve(&mut ranges, count as usize, source_id, toc_offset)?;
    let mut ids = HashSet::new();
    ids.try_reserve(count as usize).map_err(|_| {
        ParseError::at(
            LimitExceeded,
            "allocation_failed",
            "cannot allocate segment identities",
            source_id,
            toc_offset,
            4,
        )
    })?;
    let mut diagnostics = Vec::new();
    for index in 0..count {
        let entry_offset = toc_offset + 4 + u64::from(index) * u64::from(toc_entry_width);
        let guid_raw = reader.array::<16>(entry_offset)?;
        if !ids.insert(guid_raw) {
            return Err(ParseError::at(
                InvalidToc,
                "duplicate_segment_guid",
                "duplicate GUID in TOC",
                source_id,
                entry_offset,
                16,
            ));
        }
        let offset = if width == 8 {
            reader.u64(entry_offset + 16)?
        } else {
            u64::try_from(reader.i32(entry_offset + 16)?).map_err(|_| {
                ParseError::at(
                    InvalidToc,
                    "negative_segment_offset",
                    "segment offset must be nonnegative",
                    source_id,
                    entry_offset + 16,
                    4,
                )
            })?
        };
        let length = reader.u32(entry_offset + 16 + width)?;
        let attributes = reader.u32(entry_offset + 20 + width)?;
        let end = offset.checked_add(u64::from(length));
        if length < 24 || offset < header_size || end.is_none_or(|end| end > data.len() as u64) {
            return Err(ParseError::at(
                InvalidSegment,
                "invalid_segment_range",
                "segment range lies outside the data region",
                source_id,
                entry_offset + 16,
                width + 4,
            ));
        }
        let end = offset + u64::from(length);
        if offset < toc_end && end > toc_offset {
            return Err(ParseError::at(
                InvalidSegment,
                "segment_overlaps_toc",
                "segment overlaps TOC",
                source_id,
                entry_offset + 16,
                width,
            ));
        }
        let segment_reader = Reader {
            kind: InvalidSegment,
            ..reader
        };
        let segment_type = segment_reader.i32(offset + 16)?;
        if segment_reader.array::<16>(offset)? != guid_raw
            || i64::from(segment_reader.i32(offset + 20)?) != i64::from(length)
            || i64::from(segment_type) != i64::from(attributes >> 24)
        {
            return Err(ParseError::at(
                InvalidSegment,
                "segment_header_mismatch",
                "TOC and segment header disagree on GUID, type or length",
                source_id,
                offset,
                24,
            ));
        }
        let segment_type = segment_type as u32;
        let type_name = segment_type_name(segment_type);
        if type_name == "Unknown" {
            diagnostics.push(Diagnostic {
                code: "unknown_segment_type".into(),
                message: format!(
                    "segment type {segment_type} retained without payload interpretation"
                ),
                severity: DiagnosticSeverity::Info,
                source: Some(span(source_id, offset, 24)),
            });
        }
        let compression = if supports_compression(segment_type) {
            if length < 33 {
                return Err(ParseError::at(
                    InvalidSegment,
                    "truncated_compression_header",
                    "segment is too short for its compression header",
                    source_id,
                    offset + 24,
                    u64::from(length) - 24,
                ));
            }
            let flag = segment_reader.u32(offset + 24)?;
            let declared_length = segment_reader.i32(offset + 28)?;
            let algorithm = segment_reader.array::<1>(offset + 32)?[0];
            if declared_length < 1 || 32 + declared_length as u64 > u64::from(length) {
                return Err(ParseError::at(
                    InvalidSegment,
                    "invalid_compressed_length",
                    "declared compression data range exceeds the segment",
                    source_id,
                    offset + 28,
                    4,
                ));
            }
            let label = match (flag, algorithm) {
                (2, 2) => "ZLIB",
                (3, 3) => "LZMA",
                (_, 1) => "None",
                _ => "Unknown",
            };
            if label == "Unknown" {
                diagnostics.push(Diagnostic {
                    code: "unknown_compression".into(),
                    message: format!(
                        "compression flag {flag}, algorithm {algorithm} retained without decoding"
                    ),
                    severity: DiagnosticSeverity::Info,
                    source: Some(span(source_id, offset + 24, 9)),
                });
            }
            Some(CompressionHeader {
                flag,
                declared_length,
                algorithm,
                label,
                source: span(source_id, offset + 24, 9),
            })
        } else {
            None
        };
        segments.push(SegmentInfo {
            guid_raw,
            offset,
            length,
            segment_type,
            type_name,
            attributes,
            toc_source: span(source_id, entry_offset, u64::from(toc_entry_width)),
            header_source: span(source_id, offset, 24),
            payload_source: span(source_id, offset + 24, u64::from(length) - 24),
            compression,
        });
        ranges.push((offset, end));
    }
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(ParseError::at(
                InvalidSegment,
                "overlapping_segments",
                "segments overlap",
                source_id,
                pair[1].0,
                24,
            ));
        }
    }
    if !segments
        .iter()
        .any(|s| s.guid_raw == lsg_guid_raw && s.segment_type == 1)
    {
        return Err(ParseError::at(
            InvalidToc,
            "invalid_lsg_root",
            "header LSG GUID does not identify an LSG segment",
            source_id,
            85 + width,
            16,
        ));
    }
    Ok(ContainerInfo {
        source_id: source_id.into(),
        size_bytes: data.len() as u64,
        version,
        version_field,
        header_comment,
        byte_order,
        header_field,
        header_size,
        extra_header_guid,
        toc_offset,
        toc_entry_width,
        lsg_guid_raw,
        segments,
        diagnostics,
    })
}

pub fn supports_compression(segment_type: u32) -> bool {
    matches!(segment_type, 1..=4 | 17 | 18 | 20 | 23 | 24 | 30 | 31 | 33)
}

pub fn segment_type_name(segment_type: u32) -> &'static str {
    match segment_type {
        1 => "Logical Scene Graph",
        2 => "JT B-Rep",
        3 => "PMI Data",
        4 => "Meta Data",
        6 => "Shape",
        7 => "Shape LOD0",
        8 => "Shape LOD1",
        9 => "Shape LOD2",
        10 => "Shape LOD3",
        11 => "Shape LOD4",
        12 => "Shape LOD5",
        13 => "Shape LOD6",
        14 => "Shape LOD7",
        15 => "Shape LOD8",
        16 => "Shape LOD9",
        17 => "XT B-Rep",
        18 => "Wireframe",
        20 => "ULP",
        23 => "STT",
        24 => "LWPA",
        30 => "MultiXT B-Rep",
        31 => "InfoSegment",
        33 => "STEP B-Rep",
        _ => "Unknown",
    }
}
