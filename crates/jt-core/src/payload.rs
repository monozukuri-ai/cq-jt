//! JT logical element structures/identifiers: Copyright 2026 Siemens.
//! Used under the JT Data Format Reference Intellectual Property License Terms,
//! JT 10.6 Rev D page ii, sections 4.1 and 5.1, Annex A; legacy headers are
//! cross-checked against the pinned readers. Object payloads remain opaque.
use crate::{
    compression, container, error::span, io::Reader, ContainerInfo, Diagnostic, DiagnosticSeverity,
    InspectError, JtVersion, ParseError, ParseErrorKind, RawElement, ReadLimits, SourceSpan,
};
use std::{path::Path, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalElement {
    /// Length after the I32 length field, including GUID/base type/object ID.
    pub declared_length: u32,
    pub type_id: [u8; 16],
    pub base_type: Option<u8>,
    /// 8.x stores IDs in type-specific payloads, so outer decoding leaves those absent.
    pub object_id: Option<i32>,
    pub list_index: u32,
    pub is_end_marker: bool,
    pub source: SourceSpan,
    pub data_source: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct DecodedSegment {
    pub segment_index: usize,
    pub guid_raw: [u8; 16],
    pub segment_type: u32,
    pub codec: &'static str,
    pub encoded_source: SourceSpan,
    pub decoded_source: SourceSpan,
    pub elements: Vec<LogicalElement>,
    /// LSG property table or other trailing data after the final list marker.
    pub tail_source: SourceSpan,
    pub elements_framed: bool,
    data: Arc<[u8]>,
}
impl DecodedSegment {
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
    pub fn element_bytes(&self, index: usize) -> Option<&[u8]> {
        let source = &self.elements.get(index)?.data_source;
        let start = usize::try_from(
            source
                .start_offset
                .checked_sub(self.decoded_source.start_offset)?,
        )
        .ok()?;
        let end = usize::try_from(
            source
                .end_offset
                .checked_sub(self.decoded_source.start_offset)?,
        )
        .ok()?;
        self.data.get(start..end)
    }
    /// Explicit copy only; element metadata never duplicates its raw payload.
    pub fn raw_element(&self, index: usize) -> Option<RawElement> {
        let element = self.elements.get(index)?;
        Some(RawElement {
            type_id: element.type_id,
            object_id: element.object_id.map(i64::from),
            raw_data: self.element_bytes(index)?.to_vec(),
            source: Some(element.data_source.clone()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct DecodedDocument {
    pub container: ContainerInfo,
    pub segments: Vec<DecodedSegment>,
    pub total_output_bytes: u64,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn decode_file(
    path: &Path,
    source_id: &str,
    limits: &ReadLimits,
) -> Result<DecodedDocument, InspectError> {
    let bytes = container::read_file(path, source_id, limits)?;
    Ok(decode_bytes(&bytes, source_id, limits)?)
}

/// Decode outer compression and frame logical elements, not scene/geometry data.
pub fn decode_bytes(
    bytes: &[u8],
    source_id: &str,
    limits: &ReadLimits,
) -> Result<DecodedDocument, ParseError> {
    let container = crate::inspect_bytes(bytes, source_id, limits)?;
    let mut segments = Vec::new();
    let mut diagnostics = container.diagnostics.clone();
    let mut total_output_bytes = 0;
    let mut element_count = 0;
    for (segment_index, info) in container.segments.iter().enumerate() {
        let cap = limits.max_segment_output_bytes.min(
            limits
                .max_total_output_bytes
                .saturating_sub(total_output_bytes),
        );
        let (codec, start, length) = match &info.compression {
            Some(header) => {
                if u64::from(info.length) != 32 + header.declared_length as u64 {
                    return Err(compression::failure(
                        &header.source,
                        ParseErrorKind::InvalidCompression,
                        "unexpected_segment_trailer",
                        "bytes remain outside the declared compression frame",
                    ));
                }
                let codec = match (header.flag, header.algorithm) {
                    (2, 2) => "ZLIB",
                    (3, 3) => "XZ/LZMA2",
                    (flag, 1) if flag != 2 && flag != 3 => "None",
                    _ => {
                        return Err(compression::failure(
                            &header.source,
                            ParseErrorKind::UnsupportedEncoding,
                            "unsupported_compression",
                            "unsupported or contradictory compression flag/algorithm",
                        ))
                    }
                };
                (codec, info.offset + 33, header.declared_length as u64 - 1)
            }
            None => ("None", info.offset + 24, u64::from(info.length) - 24),
        };
        let encoded_source = span(source_id, start, length);
        // Container ranges have already been checked, including conversion to usize.
        let encoded = &bytes[start as usize..(start + length) as usize];
        let decoded = match codec {
            "ZLIB" => compression::zlib(encoded, cap, &encoded_source)?,
            "XZ/LZMA2" => compression::xz(encoded, cap, limits, &encoded_source)?,
            _ => compression::copy(encoded, cap, &encoded_source)?,
        };
        total_output_bytes += decoded.len() as u64;
        let decoded_source = if codec == "None" {
            encoded_source.clone()
        } else {
            SourceSpan {
                source_id: source_id.into(),
                byte_domain: format!("segment/{}/inflated", hex(&info.guid_raw)),
                start_offset: 0,
                end_offset: decoded.len() as u64,
            }
        };
        let known = matches!(info.segment_type, 1..=4 | 6..=18 | 20 | 23 | 24 | 30 | 31);
        let (elements, tail) = if known {
            frame_elements(
                &decoded,
                &decoded_source,
                container.version,
                container.byte_order,
                info.segment_type,
                limits,
                &mut element_count,
            )?
        } else {
            (Vec::new(), 0)
        };
        let tail_source = SourceSpan {
            start_offset: decoded_source.start_offset + tail as u64,
            ..decoded_source.clone()
        };
        diagnostics.push(Diagnostic {
            code: if known {
                "element_payloads_uninterpreted"
            } else {
                "segment_payload_uninterpreted"
            }
            .into(),
            message: "outer bytes retained; scene and CAD payload semantics are not decoded".into(),
            severity: DiagnosticSeverity::Info,
            source: Some(decoded_source.clone()),
        });
        if tail < decoded.len() && known {
            diagnostics.push(Diagnostic {
                code: "unparsed_segment_tail".into(),
                message: if info.segment_type == 1 {
                    "LSG property-table bytes retained without interpretation"
                } else {
                    "bytes following the element-list marker retained without interpretation"
                }
                .into(),
                severity: DiagnosticSeverity::Info,
                source: Some(tail_source.clone()),
            });
        }
        segments.try_reserve(1).map_err(|_| {
            compression::failure(
                &encoded_source,
                ParseErrorKind::LimitExceeded,
                "allocation_failed",
                "cannot allocate decoded segment table",
            )
        })?;
        segments.push(DecodedSegment {
            segment_index,
            guid_raw: info.guid_raw,
            segment_type: info.segment_type,
            codec,
            encoded_source,
            decoded_source,
            elements,
            tail_source,
            elements_framed: known,
            data: decoded.into(),
        });
    }
    Ok(DecodedDocument {
        container,
        segments,
        total_output_bytes,
        diagnostics,
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        result.push(DIGITS[(b >> 4) as usize] as char);
        result.push(DIGITS[(b & 15) as usize] as char);
    }
    result
}

fn frame_elements(
    data: &[u8],
    source: &SourceSpan,
    version: JtVersion,
    order: crate::ByteOrder,
    segment_type: u32,
    limits: &ReadLimits,
    total_count: &mut u32,
) -> Result<(Vec<LogicalElement>, usize), ParseError> {
    let mut elements = Vec::new();
    let mut at = 0;
    let mut list = 0;
    let lists = if segment_type == 1 && (version.major, version.minor) < (10, 5) {
        2
    } else {
        1
    };
    let reader = Reader {
        data,
        order,
        source_id: &source.source_id,
        kind: ParseErrorKind::InvalidElement,
    };
    let rebase = |mut error: ParseError| {
        if let Some(span) = &mut error.source {
            span.byte_domain.clone_from(&source.byte_domain);
            span.start_offset += source.start_offset;
            span.end_offset += source.start_offset;
        }
        error
    };
    let range = |start: usize, end: usize| SourceSpan {
        source_id: source.source_id.clone(),
        byte_domain: source.byte_domain.clone(),
        start_offset: source.start_offset + start as u64,
        end_offset: source.start_offset + end as u64,
    };
    while at < data.len() {
        if *total_count >= limits.max_elements {
            return Err(compression::failure(
                &range(at, at),
                ParseErrorKind::LimitExceeded,
                "element_limit_exceeded",
                "logical element count exceeds max_elements",
            ));
        }
        let length = reader.i32(at as u64).map_err(rebase)?;
        let end = if length >= 16 {
            at.checked_add(4 + length as usize)
                .filter(|&end| end <= data.len())
        } else {
            None
        }
        .ok_or_else(|| {
            compression::failure(
                &range(at, (at + 4).min(data.len())),
                ParseErrorKind::InvalidElement,
                "invalid_element_length",
                "logical element length is too short or extends beyond the segment",
            )
        })?;
        let type_id = reader.array::<16>(at as u64 + 4).map_err(rebase)?;
        let is_end_marker = type_id == [0xff; 16];
        let header = if is_end_marker {
            16
        } else if version.major > 8 {
            21
        } else {
            17
        };
        if (is_end_marker && length != 16) || length < header {
            return Err(compression::failure(
                &range(at, end),
                ParseErrorKind::InvalidElement,
                "invalid_element_header",
                "logical element header/marker does not match its declared length",
            ));
        }
        let (base_type, object_id) = if is_end_marker {
            (None, None)
        } else {
            (
                Some(reader.array::<1>(at as u64 + 20).map_err(rebase)?[0]),
                if version.major > 8 {
                    Some(reader.i32(at as u64 + 21).map_err(rebase)?)
                } else {
                    None
                },
            )
        };
        elements.try_reserve(1).map_err(|_| {
            compression::failure(
                &range(at, end),
                ParseErrorKind::LimitExceeded,
                "allocation_failed",
                "cannot allocate element metadata",
            )
        })?;
        elements.push(LogicalElement {
            declared_length: length as u32,
            type_id,
            base_type,
            object_id,
            list_index: list,
            is_end_marker,
            source: range(at, end),
            data_source: range(at + 4 + header as usize, end),
        });
        *total_count += 1;
        at = end;
        if is_end_marker {
            list += 1;
            if list == lists {
                return Ok((elements, at));
            }
        }
    }
    if segment_type == 1 {
        return Err(compression::failure(
            &range(at, at),
            ParseErrorKind::InvalidElement,
            "missing_end_marker",
            "LSG element lists have no complete end marker sequence",
        ));
    }
    Ok((elements, at))
}
