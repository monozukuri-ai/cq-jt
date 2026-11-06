//! JT 9.5 LSG structures/identifiers: Copyright 2010 Siemens Product Lifecycle
//! Management Software Inc. Used under the JT Data Format Reference IP License
//! Terms (9.5 Rev A, section 1). No Siemens SDK code is included.
//! JT 10.0 structures/identifiers: Copyright 2015 Siemens Product Lifecycle
//! Management Software Inc., used under the same terms (10.0 Rev-B, section 1).
mod reader;
pub(crate) mod resolve;
mod value;
pub use value::*;

use crate::{DecodedDocument, InspectError, ParseError, ReadLimits};
use std::{path::Path, sync::Arc};

#[derive(Debug, Clone)]
pub struct StructureDocument {
    pub model: StructureScene,
    pub document: Arc<DecodedDocument>,
    pub referenced_documents: Vec<Arc<DecodedDocument>>,
}

pub fn read_structure_bytes(
    bytes: &[u8],
    source_id: &str,
    limits: &ReadLimits,
    options: &StructureOptions,
) -> Result<StructureDocument, ParseError> {
    read_structure_bytes_profile(bytes, source_id, limits, options, false)
}
pub(crate) fn read_structure_bytes_profile(
    bytes: &[u8],
    source_id: &str,
    limits: &ReadLimits,
    options: &StructureOptions,
    external_boundaries: bool,
) -> Result<StructureDocument, ParseError> {
    // Reject other semantic layouts before interpreting their element bodies.
    let container = crate::inspect_bytes(bytes, source_id, limits)?;
    let experimental = (container.version.major, container.version.minor) == (10, 6);
    if !(matches!(
        (container.version.major, container.version.minor),
        (9, 5) | (10, 0 | 3)
    ) || experimental && options.experimental_jt106)
    {
        return Err(crate::compression::failure(
            &crate::error::span(source_id, 0, 80),
            crate::ParseErrorKind::UnsupportedVersion,
            "unsupported_structure_version",
            "structure reading requires JT 9.5, 10.0 or 10.3; specification-only 10.6 requires experimental_jt106=true",
        ));
    }
    let document = Arc::new(crate::decode_bytes(bytes, source_id, limits)?);
    let parsed = reader::parse(&document, options)?;
    let mut model = resolve::resolve(parsed, &document, options, external_boundaries)?;
    if experimental {
        model.diagnostics.push(crate::Diagnostic {
            code: "unverified_jt106_profile".into(),
            message: "JT 10.6 Rev D experimental profile: synthetic controls only; real-file compatibility is not qualified".into(),
            severity: crate::DiagnosticSeverity::Warning,
            source: Some(crate::error::span(source_id, 0, 80)),
        });
    }
    Ok(StructureDocument {
        model,
        document,
        referenced_documents: Vec::new(),
    })
}

pub fn read_structure_file(
    path: &Path,
    source_id: &str,
    limits: &ReadLimits,
    options: &StructureOptions,
) -> Result<StructureDocument, InspectError> {
    let bytes = crate::container::read_file(path, source_id, limits)?;
    Ok(read_structure_bytes(&bytes, source_id, limits, options)?)
}
