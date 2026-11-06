use crate::{Diagnostic, SourceMetadata, SourceSpan};
use std::sync::Arc;

pub type Matrix4 = [f64; 16];
pub const IDENTITY: Matrix4 = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadMode {
    Strict,
    BestEffort,
}
#[derive(Debug, Clone, Copy)]
pub struct StructureOptions {
    pub mode: ReadMode,
    pub max_nodes: u32,
    pub max_instances: u32,
    pub max_depth: u32,
    pub max_references: u32,
    pub max_string_bytes: u64,
    /// Opt in to the specification-derived 10.6 profile; real-file qualification is pending.
    pub experimental_jt106: bool,
}
impl Default for StructureOptions {
    fn default() -> Self {
        Self {
            mode: ReadMode::Strict,
            max_nodes: 1_000_000,
            max_instances: 1_000_000,
            max_depth: 256,
            max_references: 4_000_000,
            max_string_bytes: 1024 * 1024,
            experimental_jt106: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LateLoadedReference {
    pub segment_guid: [u8; 16],
    pub segment_type: i32,
    pub payload_object_id: i32,
    pub reserved: Option<i32>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    String(Arc<str>),
    Integer(i32),
    Float(f64),
    Date([i16; 6]),
    Vector([f64; 4]),
    LateLoaded(LateLoadedReference),
    Opaque,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyAtom {
    pub object_id: i32,
    pub type_id: [u8; 16],
    pub state_flags: Option<u32>,
    pub value: PropertyValue,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyBinding {
    pub owner_object_id: i32,
    pub key_atom_id: i32,
    pub value_atom_id: i32,
    pub key: Option<Arc<str>>,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Base,
    Group,
    Partition,
    Instance,
    Part,
    Metadata,
    Lod,
    RangeLod,
    Switch,
    TriStripShape,
}
impl NodeKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Group => "group",
            Self::Partition => "partition",
            Self::Instance => "instance",
            Self::Part => "part",
            Self::Metadata => "metadata",
            Self::Lod => "lod",
            Self::RangeLod => "range_lod",
            Self::Switch => "switch",
            Self::TriStripShape => "tri_strip_shape",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeMetadata {
    pub transformed_bounds: Option<[f64; 6]>,
    pub local_bounds: [f64; 6],
    pub area: f64,
    pub vertex_count_range: [i32; 2],
    pub node_count_range: [i32; 2],
    pub polygon_count_range: [i32; 2],
    pub size: u32,
    pub compression_level: f64,
    pub vertex_version: i16,
    pub vertex_binding: u64,
    pub quantization: Option<[u8; 4]>,
    pub vertex_binding2: Option<u64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub id: u32,
    pub object_id: i32,
    pub kind: NodeKind,
    pub flags: u32,
    /// Resolved local indices, not source object IDs.
    pub attribute_ids: Vec<u32>,
    pub child_ids: Vec<u32>,
    pub name: Option<Arc<str>>,
    pub measurement_units: Option<Arc<str>>,
    pub file_name: Option<String>,
    pub partition_flags: Option<i32>,
    pub reserved: Option<i32>,
    pub lod_ranges: Vec<f64>,
    pub lod_center: Option<[f64; 3]>,
    pub selected_child: Option<i32>,
    pub shape: Option<ShapeMetadata>,
    pub payload_references: Vec<LateLoadedReference>,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub id: u32,
    pub object_id: i32,
    pub version: i16,
    pub state_flags: u8,
    pub inhibit_flags: u32,
    pub data_flags: u16,
    pub ambient: [f64; 4],
    pub diffuse: [f64; 4],
    pub specular: [f64; 4],
    pub emission: [f64; 4],
    pub shininess: f64,
    pub reflectivity: Option<f64>,
    pub bumpiness: Option<f64>,
    pub field_final_flags: Option<u32>,
    pub palette_index: Option<u32>,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub id: u32,
    pub object_id: i32,
    pub state_flags: u8,
    pub inhibit_flags: u32,
    pub field_final_flags: Option<u32>,
    pub palette_index: Option<u32>,
    pub transform: Option<Matrix4>,
    pub material_id: Option<u32>,
    pub source: SourceSpan,
}

/// Only fields with defined inheritance rules are resolved. No default colour.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EffectiveMaterial {
    pub ambient: Option<[f64; 4]>,
    pub diffuse_rgb: Option<[f64; 3]>,
    pub diffuse_alpha: Option<f64>,
    pub specular: Option<[f64; 4]>,
    pub emission: Option<[f64; 4]>,
    pub blending: Option<bool>,
    pub override_vertex_colors: Option<bool>,
    /// Source object IDs for ambient/RGB/alpha/specular/emission/blend/override.
    pub origins: [Option<i32>; 7],
    pub origin_sources: [Option<String>; 7],
}
#[derive(Debug, Clone, PartialEq)]
pub struct PartDefinition {
    pub id: u32,
    pub node_id: u32,
    pub name: Option<Arc<str>>,
    pub shape_node_ids: Vec<u32>,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub id: u32,
    pub node_id: u32,
    pub parent_id: Option<u32>,
    pub part_id: Option<u32>,
    pub local_transform: Matrix4,
    pub world_transform: Matrix4,
    pub unit_node_id: Option<u32>,
    pub meters_per_unit: Option<f64>,
    pub source_meters_per_unit: Option<f64>,
    pub material: EffectiveMaterial,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MissingReference {
    pub owner_object_id: i32,
    pub target: String,
    pub kind: String,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, PartialEq)]
pub struct OpaqueObject {
    pub object_id: i32,
    pub type_id: [u8; 16],
    pub element_index: usize,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructureScene {
    pub source: SourceMetadata,
    pub sources: Vec<SourceMetadata>,
    pub coordinates_in_meters: bool,
    pub blocked_node_ids: Vec<u32>,
    pub lsg_segment_index: usize,
    pub lsg_guid: [u8; 16],
    pub root_node_id: u32,
    pub nodes: Vec<GraphNode>,
    pub attributes: Vec<Attribute>,
    pub materials: Vec<Material>,
    pub atoms: Vec<PropertyAtom>,
    pub bindings: Vec<PropertyBinding>,
    pub parts: Vec<PartDefinition>,
    /// One entry per traversed node path, including hierarchy/LOD nodes.
    pub instances: Vec<Instance>,
    pub opaque_objects: Vec<OpaqueObject>,
    pub missing_references: Vec<MissingReference>,
    pub diagnostics: Vec<Diagnostic>,
    pub complete: bool,
}
