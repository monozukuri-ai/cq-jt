//! JT 9.5 mesh structures and algorithms: Copyright 2010 Siemens Product
//! Lifecycle Management Software Inc. JT Data Format Reference IP License Terms.
//! JT 10.0 structures/algorithms: Copyright 2015 Siemens Product Lifecycle
//! Management Software Inc., used under JT 10.0 Rev-B section 1.
mod codec;
mod hash;
mod reader;
pub mod resolver;
mod scene;
mod topology;
use crate::{ByteOrder, ParseError, SourceSpan};
pub use scene::*;

#[derive(Debug, Clone, Copy)]
pub struct MeshOptions {
    pub max_meshes: u32,
    pub max_vertices: u64,
    pub max_triangles: u64,
    pub max_mesh_bytes: u64,
    pub max_codec_values: u64,
    pub max_codec_packets: u64,
    pub max_codec_depth: u32,
    pub max_topology_edges: u64,
    pub max_topology_work: u64,
}
impl Default for MeshOptions {
    fn default() -> Self {
        Self {
            max_meshes: 100_000,
            max_vertices: 10_000_000,
            max_triangles: 10_000_000,
            max_mesh_bytes: 512 * 1024 * 1024,
            max_codec_values: 64_000_000,
            max_codec_packets: 1_000_000,
            max_codec_depth: 16,
            max_topology_edges: 60_000_000,
            max_topology_work: 200_000_000,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Mesh {
    pub id: u32,
    pub segment_index: usize,
    pub element_index: usize,
    /// JT 9.5 I32 / JT 10 U32, widened without changing their signedness.
    pub vertex_records_object_id: i64,
    pub source: SourceSpan,
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub normal_indices: Option<Vec<[u32; 3]>>,
    pub face_groups: Vec<i32>,
    pub vertex_flags: Vec<u32>,
    pub bounds: Option<[f64; 6]>,
    pub quantization: [u8; 4],
    pub quantization_step: [f64; 3],
    pub topology_hash: u32,
    pub coordinate_hash: Option<u32>,
    pub normal_hash: Option<u32>,
    pub cover_faces: u64,
    pub codec_depth: u32,
}
impl Mesh {
    pub fn buffer_bytes(&self) -> u64 {
        (self.positions.len() as u64 + self.normals.len() as u64) * 24
            + self.triangles.len() as u64 * 12
            + self
                .normal_indices
                .as_ref()
                .map_or(0, |v| v.len() as u64 * 12)
            + (self.face_groups.len() as u64 + self.vertex_flags.len() as u64) * 4
    }
}
pub fn decode_mesh_element(
    bytes: &[u8],
    order: ByteOrder,
    source: SourceSpan,
    options: &MeshOptions,
) -> Result<Mesh, ParseError> {
    reader::parse(
        bytes,
        order,
        source,
        options,
        &mut codec::Budget {
            remaining: options.max_codec_values,
            packets: options.max_codec_packets,
            depth: options.max_codec_depth.min(64),
            deepest: 0,
        },
        reader::Layout::Legacy95,
        false,
    )
}
