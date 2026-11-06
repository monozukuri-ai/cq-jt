use super::{codec::Budget, reader, Mesh, MeshOptions};
use crate::{
    compression::failure,
    structure::{NodeKind, ReadMode, StructureDocument, StructureOptions},
    Diagnostic, DiagnosticSeverity, InspectError, ParseError, ParseErrorKind as K, ReadLimits,
    SourceSpan,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodSelection {
    Highest,
    Lowest,
    Index(u32),
}
#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub structure: StructureOptions,
    pub mesh: MeshOptions,
    pub lod: LodSelection,
    pub resolver: super::resolver::ResolverOptions,
}
impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            structure: StructureOptions::default(),
            mesh: MeshOptions::default(),
            lod: LodSelection::Highest,
            resolver: super::resolver::ResolverOptions::default(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LodChoice {
    pub node_id: u32,
    pub child_index: u32,
    pub child_node_id: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshInstance {
    pub id: u32,
    pub structure_instance_id: u32,
    pub mesh_id: u32,
}
#[derive(Debug, Clone)]
pub struct MeshScene {
    pub structure: Arc<StructureDocument>,
    pub meshes: Vec<Mesh>,
    pub instances: Vec<MeshInstance>,
    pub lod_choices: Vec<LodChoice>,
    pub diagnostics: Vec<Diagnostic>,
    pub complete: bool,
    pub(crate) active: Vec<bool>,
    pub(crate) codec_values_used: u64,
    pub(crate) codec_packets_used: u64,
}
fn unsupported(source: &SourceSpan, code: &'static str, message: impl Into<String>) -> ParseError {
    failure(source, K::UnsupportedEncoding, code, message)
}
fn partial(
    error: ParseError,
    mode: ReadMode,
    complete: &mut bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ParseError> {
    if mode == ReadMode::Strict
        || !matches!(error.kind, K::UnsupportedEncoding | K::MissingReference)
    {
        return Err(error);
    }
    *complete = false;
    diagnostics.push(Diagnostic {
        code: error.code.into(),
        message: error.message,
        severity: DiagnosticSeverity::Warning,
        source: error.source,
    });
    Ok(())
}
pub fn read_bytes(
    bytes: &[u8],
    source_id: &str,
    limits: &ReadLimits,
    options: &ReadOptions,
) -> Result<MeshScene, ParseError> {
    let structure = Arc::new(crate::read_structure_bytes(
        bytes,
        source_id,
        limits,
        &options.structure,
    )?);
    from_structure(structure, options)
}
pub fn read_file(
    path: &Path,
    source_id: &str,
    limits: &ReadLimits,
    options: &ReadOptions,
) -> Result<MeshScene, InspectError> {
    if options.resolver.enabled {
        return super::resolver::read_local(path, source_id, limits, options);
    }
    let bytes = crate::container::read_file(path, source_id, limits)?;
    Ok(read_bytes(&bytes, source_id, limits, options)?)
}
pub(crate) struct Selection {
    pub active: Vec<bool>,
    pub choices: Vec<LodChoice>,
    pub diagnostics: Vec<Diagnostic>,
    pub complete: bool,
}
pub(crate) fn select(
    graph: &crate::structure::StructureScene,
    options: &ReadOptions,
) -> Result<Selection, ParseError> {
    let mut complete = graph.complete;
    let mut diagnostics = graph.diagnostics.clone();
    let mut selected = BTreeMap::new();
    let mut choices = Vec::new();
    let mut active = vec![false; graph.instances.len()];
    let mut blocked = BTreeSet::new();
    let mut ordinal_cursors = BTreeMap::<u32, usize>::new();
    let unresolved_owners: BTreeSet<_> = graph
        .missing_references
        .iter()
        .filter(|m| m.kind == "child node")
        .map(|m| m.owner_object_id)
        .collect();
    // Structure resolution emits parent-before-child paths. Decide only at reachable LOD nodes.
    for instance in &graph.instances {
        let node = &graph.nodes[instance.node_id as usize];
        let reachable = match instance.parent_id {
            None => true,
            Some(parent) => {
                let p = &graph.instances[parent as usize];
                let parent_node = &graph.nodes[p.node_id as usize];
                let ordinal = if matches!(parent_node.kind, NodeKind::Lod | NodeKind::RangeLod) {
                    let cursor = ordinal_cursors.entry(parent).or_default();
                    let offset = parent_node.child_ids[*cursor..]
                        .iter()
                        .position(|&id| id == instance.node_id)
                        .ok_or_else(|| {
                            failure(
                                &node.source,
                                K::InvalidGraph,
                                "invalid_lod_occurrence",
                                "LOD occurrence does not match its ordered child edges",
                            )
                        })?;
                    let ordinal = *cursor + offset;
                    *cursor = ordinal + 1;
                    ordinal as u32
                } else {
                    0
                };
                active[parent as usize]
                    && !blocked.contains(&p.node_id)
                    && selected
                        .get(&p.node_id)
                        .is_none_or(|&selected_index| selected_index == ordinal)
            }
        };
        if !reachable {
            continue;
        }
        active[instance.id as usize] = true;
        if matches!(node.kind, NodeKind::Lod | NodeKind::RangeLod)
            && !selected.contains_key(&node.id)
            && !blocked.contains(&node.id)
        {
            let unresolved_children = unresolved_owners.contains(&node.object_id);
            let index = if unresolved_children {
                None
            } else {
                match options.lod {
                    LodSelection::Index(i) => Some(i as usize),
                    LodSelection::Highest if node.kind == NodeKind::RangeLod => Some(0),
                    LodSelection::Lowest if node.kind == NodeKind::RangeLod => {
                        node.child_ids.len().checked_sub(1)
                    }
                    _ => None,
                }
            };
            if let Some(index) = index.filter(|&i| i < node.child_ids.len()) {
                let child = node.child_ids[index];
                selected.insert(node.id, index as u32);
                choices.push(LodChoice {
                    node_id: node.id,
                    child_index: index as u32,
                    child_node_id: child,
                });
            } else {
                partial(unsupported(&node.source,"ambiguous_lod_selection","LOD selection is unavailable; unordered LOD requires an explicit valid child index"),options.structure.mode,&mut complete,&mut diagnostics)?;
                blocked.insert(node.id);
            }
        }
    }
    Ok(Selection {
        active,
        choices,
        diagnostics,
        complete,
    })
}
pub(crate) fn from_structure(
    structure: Arc<StructureDocument>,
    options: &ReadOptions,
) -> Result<MeshScene, ParseError> {
    let graph = &structure.model;
    let Selection {
        active,
        choices,
        mut diagnostics,
        mut complete,
    } = select(graph, options)?;
    let mut meshes = Vec::new();
    let mut instances = Vec::new();
    let mut cache = BTreeMap::new();
    let mut failed = BTreeSet::new();
    let mut budget = Budget {
        remaining: options.mesh.max_codec_values,
        packets: options.mesh.max_codec_packets,
        depth: options.mesh.max_codec_depth.min(64),
        deepest: 0,
    };
    let mut vertices = 0u64;
    let mut triangles = 0u64;
    let mut output_bytes = 0u64;
    for instance in &graph.instances {
        if !active[instance.id as usize] {
            continue;
        }
        let node = &graph.nodes[instance.node_id as usize];
        if node.kind != NodeKind::TriStripShape {
            continue;
        }
        let refs: Vec<_> = node
            .payload_references
            .iter()
            .filter(|r| r.segment_type == 6 || (7..=16).contains(&r.segment_type))
            .collect();
        if refs.len() != 1 {
            partial(
                unsupported(
                    &node.source,
                    "ambiguous_shape_reference",
                    "shape requires exactly one supported late-loaded mesh reference",
                ),
                options.structure.mode,
                &mut complete,
                &mut diagnostics,
            )?;
            continue;
        }
        let reference = refs[0];
        let segment = structure
            .document
            .segments
            .iter()
            .find(|s| s.guid_raw == reference.segment_guid);
        let Some(segment) = segment else {
            partial(
                failure(
                    &node.source,
                    K::MissingReference,
                    "missing_mesh_segment",
                    "mesh segment is missing",
                ),
                options.structure.mode,
                &mut complete,
                &mut diagnostics,
            )?;
            continue;
        };
        // 9.5 payload IDs are packaging IDs, not the shape logical object ID.
        // Accept the unambiguous singleton layout observed in this profile only.
        let elements: Vec<_> = segment
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_end_marker)
            .collect();
        if elements.len() != 1 {
            partial(
                unsupported(
                    &node.source,
                    "ambiguous_mesh_element",
                    "multi-element mesh segment has no supported payload directory",
                ),
                options.structure.mode,
                &mut complete,
                &mut diagnostics,
            )?;
            continue;
        }
        let (element_index, element) = elements[0];
        let key = (segment.segment_index, element_index);
        if failed.contains(&key) {
            continue;
        }
        let id = if let Some(id) = cache.get(&key) {
            *id
        } else {
            if meshes.len() >= options.mesh.max_meshes as usize {
                return Err(failure(
                    &element.source,
                    K::LimitExceeded,
                    "mesh_count_limit",
                    "mesh count limit exceeded",
                ));
            }
            let mut expected = [
                0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0, 0x80, 0xc7, 0xbb,
                0x59, 0x97,
            ];
            if structure.document.container.byte_order == crate::ByteOrder::Big {
                expected[..4].reverse();
                expected[4..6].reverse();
                expected[6..8].reverse();
            }
            if element.type_id != expected {
                failed.insert(key);
                partial(
                    unsupported(
                        &element.source,
                        "unsupported_shape_lod_type",
                        "selected payload is not a TriStripSetShapeLOD",
                    ),
                    options.structure.mode,
                    &mut complete,
                    &mut diagnostics,
                )?;
                continue;
            }
            let mut limits = options.mesh;
            limits.max_vertices = limits.max_vertices.saturating_sub(vertices);
            limits.max_triangles = limits.max_triangles.saturating_sub(triangles);
            limits.max_mesh_bytes = limits.max_mesh_bytes.saturating_sub(output_bytes);
            let result = reader::parse(
                segment.element_bytes(element_index).unwrap(),
                structure.document.container.byte_order,
                element.data_source.clone(),
                &limits,
                &mut budget,
                match (
                    structure.document.container.version.major,
                    structure.document.container.version.minor,
                ) {
                    (10, 6) => reader::Layout::Rev106,
                    (10, _) => reader::Layout::Modern10,
                    _ => reader::Layout::Legacy95,
                },
                segment.codec != "None",
            );
            let mut mesh = match result {
                Ok(mesh) => mesh,
                Err(error) => {
                    failed.insert(key);
                    partial(
                        error,
                        options.structure.mode,
                        &mut complete,
                        &mut diagnostics,
                    )?;
                    continue;
                }
            };
            vertices += mesh.positions.len() as u64;
            triangles += mesh.triangles.len() as u64;
            output_bytes += mesh.buffer_bytes();
            let expected_depth = if structure.document.container.version.major == 10 {
                8
            } else {
                3
            };
            if mesh.codec_depth > expected_depth {
                diagnostics.push(Diagnostic{code:"extended_codec_depth".into(),message:format!("observed integer packet recursion depth {}; exceeds the file version description of {expected_depth}",mesh.codec_depth),severity:DiagnosticSeverity::Info,source:Some(mesh.source.clone())});
            }
            let id = meshes.len() as u32;
            mesh.id = id;
            mesh.segment_index = key.0;
            mesh.element_index = key.1;
            meshes.push(mesh);
            cache.insert(key, id);
            id
        };
        let mesh = &meshes[id as usize];
        if let Some(metadata) = &node.shape {
            let n = mesh.triangles.len() as i64;
            if n < i64::from(metadata.polygon_count_range[0])
                || (metadata.polygon_count_range[1] >= 0
                    && n > i64::from(metadata.polygon_count_range[1]))
            {
                diagnostics.push(Diagnostic{code:"lsg_polygon_count_differs".into(),
                    message:format!("LSG polygon range {:?}; topology contains {n} visible triangles. Original strip connector counts are not a triangle ground truth",metadata.polygon_count_range),
                    severity:DiagnosticSeverity::Info,source:Some(node.source.clone())});
            }
            if let Some(bounds) = mesh.bounds {
                for j in 0..3 {
                    let tolerance = mesh.quantization_step[j]
                        + 1e-5
                            * (1.
                                + metadata.local_bounds[j]
                                    .abs()
                                    .max(metadata.local_bounds[j + 3].abs()));
                    if bounds[j] < metadata.local_bounds[j] - tolerance
                        || bounds[j + 3] > metadata.local_bounds[j + 3] + tolerance
                    {
                        return Err(failure(
                            &node.source,
                            K::InvalidGeometry,
                            "shape_bounds_mismatch",
                            "decoded coordinates exceed the LSG local bounding box",
                        ));
                    }
                }
            }
        }
        instances.push(MeshInstance {
            id: instances.len() as u32,
            structure_instance_id: instance.id,
            mesh_id: id,
        });
    }
    Ok(MeshScene {
        structure,
        meshes,
        instances,
        lod_choices: choices,
        diagnostics,
        complete,
        active,
        codec_values_used: options.mesh.max_codec_values - budget.remaining,
        codec_packets_used: options.mesh.max_codec_packets - budget.packets,
    })
}
