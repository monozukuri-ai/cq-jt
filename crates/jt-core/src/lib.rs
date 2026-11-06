//! Python-independent JT container inspection and immutable model foundation.
#![doc = include_str!("../README.md")]

mod compression;
pub mod container;
pub mod error;
#[cfg(feature = "glb")]
pub mod export;
mod io;
pub mod limits;
pub mod mesh;
pub mod payload;
pub mod structure;
pub use container::{inspect_bytes, inspect_file, ByteOrder, ContainerInfo, JtDocument};
pub use error::{InspectError, ParseError, ParseErrorKind};
pub use limits::ReadLimits;
pub use payload::{decode_bytes, decode_file, DecodedDocument, DecodedSegment, LogicalElement};
pub use structure::{
    read_structure_bytes, read_structure_file, StructureDocument, StructureOptions,
};

use std::{collections::HashSet, error::Error, fmt};

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Source JT version, independent of model API and package versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JtVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceMetadata {
    pub source_id: String,
    pub label: Option<String>,
    pub version: Option<JtVersion>,
    /// Metres per source length unit. None means unknown, not millimetres.
    pub meters_per_unit: Option<f64>,
}

/// Half-open range within an explicitly named byte domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    pub source_id: String,
    pub byte_domain: String,
    pub start_offset: u64,
    pub end_offset: u64,
}

impl SourceSpan {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.source_id.is_empty()
            || self.byte_domain.is_empty()
            || self.end_offset < self.start_offset
        {
            return Err(ModelError::new(
                "invalid_source_span",
                "source span requires a source, byte domain and ordered offsets",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub source: Option<SourceSpan>,
}

/// Uninterpreted bytes. A type identifier does not imply decoded semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawElement {
    /// Original 16 identifier bytes; no endian-dependent UUID conversion.
    pub type_id: [u8; 16],
    pub object_id: Option<i64>,
    pub raw_data: Vec<u8>,
    pub source: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError {
    pub code: &'static str,
    pub message: String,
}

impl ModelError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ModelError {}

/// Validated owned snapshot. Private fields prevent stale source references.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    sources: Vec<SourceMetadata>,
    raw_elements: Vec<RawElement>,
    diagnostics: Vec<Diagnostic>,
}

impl Scene {
    pub fn new(
        sources: Vec<SourceMetadata>,
        raw_elements: Vec<RawElement>,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Self, ModelError> {
        let mut ids = HashSet::new();
        for source in &sources {
            if source.source_id.is_empty() || !ids.insert(source.source_id.as_str()) {
                return Err(ModelError::new(
                    "invalid_source_id",
                    "source IDs must be nonempty and unique",
                ));
            }
            if source.version.is_some_and(|v| v.major == 0) {
                return Err(ModelError::new(
                    "invalid_version",
                    "major version must be positive",
                ));
            }
            if source
                .meters_per_unit
                .is_some_and(|v| !v.is_finite() || v <= 0.0)
            {
                return Err(ModelError::new(
                    "invalid_units",
                    "meters_per_unit must be finite and positive or absent",
                ));
            }
        }
        let validate_span = |span: &SourceSpan| -> Result<(), ModelError> {
            span.validate()?;
            if !ids.contains(span.source_id.as_str()) {
                return Err(ModelError::new(
                    "missing_source",
                    format!("span references unknown source {:?}", span.source_id),
                ));
            }
            Ok(())
        };
        for raw in &raw_elements {
            if let Some(span) = &raw.source {
                validate_span(span)?;
            }
        }
        for diagnostic in &diagnostics {
            if diagnostic.code.is_empty() {
                return Err(ModelError::new(
                    "invalid_diagnostic",
                    "diagnostic code must be nonempty",
                ));
            }
            if let Some(span) = &diagnostic.source {
                validate_span(span)?;
            }
        }
        Ok(Self {
            sources,
            raw_elements,
            diagnostics,
        })
    }

    pub fn sources(&self) -> &[SourceMetadata] {
        &self.sources
    }
    pub fn raw_elements(&self) -> &[RawElement] {
        &self.raw_elements
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SourceMetadata {
        SourceMetadata {
            source_id: "part".into(),
            label: None,
            version: Some(JtVersion { major: 9, minor: 5 }),
            meters_per_unit: None,
        }
    }

    fn span() -> SourceSpan {
        SourceSpan {
            source_id: "part".into(),
            byte_domain: "segment/opaque/inflated".into(),
            start_offset: 1 << 63,
            end_offset: (1 << 63) + 3,
        }
    }

    #[test]
    fn preserves_unknown_bytes_missing_units_and_byte_domain() {
        let raw = RawElement {
            type_id: [255; 16],
            object_id: Some(i64::MIN),
            raw_data: vec![0, 255, 0],
            source: Some(span()),
        };
        let scene = Scene::new(vec![source()], vec![raw.clone()], vec![]).unwrap();
        assert_eq!(scene.raw_elements(), &[raw]);
        assert_eq!(scene.sources()[0].meters_per_unit, None);
        assert_eq!(
            scene.raw_elements()[0]
                .source
                .as_ref()
                .unwrap()
                .start_offset,
            1 << 63
        );
    }

    #[test]
    fn rejects_duplicate_or_empty_source_ids() {
        assert_eq!(
            Scene::new(vec![source(), source()], vec![], vec![])
                .unwrap_err()
                .code,
            "invalid_source_id"
        );
        let mut input = source();
        input.source_id.clear();
        assert!(Scene::new(vec![input], vec![], vec![]).is_err());
    }

    #[test]
    fn rejects_invalid_units_without_inventing_defaults() {
        for units in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut input = source();
            input.meters_per_unit = Some(units);
            assert_eq!(
                Scene::new(vec![input], vec![], vec![]).unwrap_err().code,
                "invalid_units"
            );
        }
        let mut input = source();
        input.meters_per_unit = Some(0.001);
        assert_eq!(
            Scene::new(vec![input], vec![], vec![]).unwrap().sources()[0].meters_per_unit,
            Some(0.001)
        );
    }

    #[test]
    fn validates_sources_in_both_raw_elements_and_diagnostics() {
        let diagnostic = Diagnostic {
            code: "opaque".into(),
            message: "kept".into(),
            severity: DiagnosticSeverity::Warning,
            source: Some(span()),
        };
        assert_eq!(
            Scene::new(vec![], vec![], vec![diagnostic])
                .unwrap_err()
                .code,
            "missing_source"
        );
        let raw = RawElement {
            type_id: [0; 16],
            object_id: None,
            raw_data: vec![],
            source: Some(span()),
        };
        assert_eq!(
            Scene::new(vec![], vec![raw], vec![]).unwrap_err().code,
            "missing_source"
        );
    }

    #[test]
    fn rejects_reversed_or_unnamed_byte_ranges() {
        let mut input = span();
        input.end_offset = input.start_offset - 1;
        assert_eq!(input.validate().unwrap_err().code, "invalid_source_span");
        input = span();
        input.byte_domain.clear();
        assert!(input.validate().is_err());
    }

    #[test]
    fn validates_version_and_diagnostic_codes() {
        let mut input = source();
        input.version = Some(JtVersion { major: 0, minor: 1 });
        assert_eq!(
            Scene::new(vec![input], vec![], vec![]).unwrap_err().code,
            "invalid_version"
        );
        let d = Diagnostic {
            code: String::new(),
            message: String::new(),
            severity: DiagnosticSeverity::Info,
            source: None,
        };
        assert_eq!(
            Scene::new(vec![], vec![], vec![d]).unwrap_err().code,
            "invalid_diagnostic"
        );
        assert_eq!(
            Scene::default(),
            Scene::new(vec![], vec![], vec![]).unwrap()
        );
    }
}
