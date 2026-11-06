//! Bounded glTF 2.0 binary export. Geometry stays shared across material variants.
mod geometry;
mod glb;
mod material;
mod output;
use crate::{Diagnostic, SourceSpan};
pub use glb::export_glb_bytes;
pub use output::{export_glb, ExportFileError};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    Linear,
    Srgb,
}
impl ColorSpace {
    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Srgb => "srgb",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpAxis {
    Preserve,
    Y,
    Z,
}
impl UpAxis {
    pub fn name(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
}
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub allow_partial: bool,
    /// Applied only to missing units; known units are never overwritten.
    pub meters_per_unit: Option<f64>,
    pub default_color: Option<[f64; 4]>,
    pub color_space: ColorSpace,
    pub up_axis: UpAxis,
    pub max_position_error_meters: f64,
    pub max_output_bytes: u64,
    pub max_json_bytes: u64,
    pub max_vertices: u64,
    pub max_nodes: u32,
    pub max_materials: u32,
    pub max_mesh_variants: u32,
}
impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            allow_partial: false,
            meters_per_unit: None,
            default_color: None,
            color_space: ColorSpace::Linear,
            up_axis: UpAxis::Preserve,
            max_position_error_meters: 1e-6,
            max_output_bytes: 512 * 1024 * 1024,
            max_json_bytes: 64 * 1024 * 1024,
            max_vertices: 10_000_000,
            max_nodes: 1_000_000,
            max_materials: 100_000,
            max_mesh_variants: 100_000,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
    pub source: Option<SourceSpan>,
}
impl ExportError {
    pub(super) fn new(
        code: &'static str,
        message: impl Into<String>,
        source: Option<&SourceSpan>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            source: source.cloned(),
        }
    }
}
impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl Error for ExportError {}
#[derive(Debug, Clone)]
pub struct ExportReport {
    pub source_complete: bool,
    pub byte_length: u64,
    pub node_count: u64,
    pub geometry_count: u64,
    pub mesh_count: u64,
    pub material_count: u64,
    pub instance_count: u64,
    pub vertex_count: u64,
    pub triangle_count: u64,
    pub placed_triangle_count: u64,
    pub meters_per_unit: f64,
    pub unit_fallback_used: bool,
    pub color_space: ColorSpace,
    pub up_axis: UpAxis,
    pub max_position_error_meters: f64,
    pub normalized_normal_count: u64,
    pub original_degenerate_triangles: u64,
    pub omitted_empty_meshes: Vec<u32>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Clone)]
pub struct GlbOutput {
    pub bytes: Vec<u8>,
    pub report: ExportReport,
}
