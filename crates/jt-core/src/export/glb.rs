use super::{geometry, material, ExportError, ExportOptions, ExportReport, GlbOutput, UpAxis};
use crate::{
    mesh::MeshScene,
    structure::{Instance, IDENTITY},
    Diagnostic, DiagnosticSeverity, SourceSpan,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
};

struct Builder<'a> {
    options: &'a ExportOptions,
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    json_used: u64,
}
impl Builder<'_> {
    fn metadata(&mut self, value: &Value) -> Result<(), ExportError> {
        let mut counter = Counter {
            count: 0,
            limit: self.options.max_json_bytes.saturating_sub(self.json_used),
        };
        serde_json::to_writer(&mut counter, value)
            .map_err(|_| ExportError::new("export_json_limit", "GLB JSON budget exceeded", None))?;
        self.json_used += counter.count;
        Ok(())
    }
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), ExportError> {
        let end = (self.bin.len() as u64)
            .checked_add(bytes.len() as u64)
            .filter(|&n| n <= self.options.max_output_bytes && n <= u32::MAX as u64)
            .ok_or_else(|| {
                ExportError::new(
                    "export_byte_limit",
                    "GLB buffer exceeds its byte limit",
                    None,
                )
            })?;
        self.bin
            .try_reserve((end - self.bin.len() as u64) as usize)
            .map_err(|_| {
                ExportError::new("allocation_failed", "cannot allocate GLB buffer", None)
            })?;
        self.bin.extend_from_slice(bytes);
        Ok(())
    }
    fn accessor(
        &mut self,
        bytes: &[u8],
        count: usize,
        kind: &str,
        component: u32,
        target: u32,
        bounds: Option<[f32; 6]>,
    ) -> Result<u32, ExportError> {
        while self.bin.len() % 4 != 0 {
            self.bytes(&[0])?;
        }
        let view = json!({"buffer":0,"byteOffset":self.bin.len(),"byteLength":bytes.len(),"target":target});
        self.metadata(&view)?;
        self.bytes(bytes)?;
        let mut accessor = json!({"bufferView":self.views.len(),"componentType":component,"count":count,"type":kind});
        if let Some(b) = bounds {
            accessor["min"] = json!([b[0], b[1], b[2]]);
            accessor["max"] = json!([b[3], b[4], b[5]]);
        }
        self.metadata(&accessor)?;
        let id = self.accessors.len() as u32;
        self.views.push(view);
        self.accessors.push(accessor);
        Ok(id)
    }
}
struct Counter {
    count: u64,
    limit: u64,
}
impl Write for Counter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = self
            .count
            .checked_add(b.len() as u64)
            .filter(|&n| n <= self.limit)
            .ok_or_else(|| io::Error::other("JSON limit exceeded"))?;
        self.count = n;
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct LimitedVec {
    bytes: Vec<u8>,
    limit: u64,
}
impl Write for LimitedVec {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if b.len() as u64 > self.limit.saturating_sub(self.bytes.len() as u64) {
            return Err(io::Error::other("JSON limit exceeded"));
        }
        self.bytes
            .try_reserve(b.len())
            .map_err(|_| io::Error::other("allocation failed"))?;
        self.bytes.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn error(
    code: &'static str,
    message: impl Into<String>,
    source: Option<&SourceSpan>,
) -> ExportError {
    ExportError::new(code, message, source)
}
fn diagnostic(code: &str, message: &str, source: Option<SourceSpan>) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        severity: DiagnosticSeverity::Info,
        source,
    }
}
fn validate_options(options: &ExportOptions) -> Result<(), ExportError> {
    if options
        .meters_per_unit
        .is_some_and(|v| !v.is_finite() || v <= 0.)
        || !options.max_position_error_meters.is_finite()
        || options.max_position_error_meters < 0.
    {
        return Err(error(
            "invalid_export_option",
            "units must be positive and precision tolerance nonnegative and finite",
            None,
        ));
    }
    if options
        .default_color
        .is_some_and(|c| c.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v)))
    {
        return Err(error(
            "invalid_export_option",
            "default_color must be finite RGBA within 0..1",
            None,
        ));
    }
    Ok(())
}
fn matrix(state: &Instance, units: f64) -> Result<[f64; 16], ExportError> {
    let mut m = state.local_transform;
    if m.iter().any(|x| !x.is_finite()) || m[12..] != [0., 0., 0., 1.] {
        return Err(error(
            "invalid_export_transform",
            "node matrix must be finite and affine",
            Some(&state.source),
        ));
    }
    let mut axes = [[0.; 3]; 3];
    for col in 0..3 {
        let a = [m[col], m[4 + col], m[8 + col]];
        let length = a[0].hypot(a[1]).hypot(a[2]);
        if length == 0. || !length.is_finite() || a.iter().all(|&x| x as f32 == 0.) {
            return Err(error(
                "invalid_export_transform",
                "node transform is singular or underflows glTF floating point range",
                Some(&state.source),
            ));
        }
        axes[col] = a.map(|x| x / length);
    }
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        if (0..3).map(|i| axes[a][i] * axes[b][i]).sum::<f64>().abs() > 1e-5 {
            return Err(error(
                "non_trs_transform",
                "local matrix contains shear; glTF requires TRS decomposability",
                Some(&state.source),
            ));
        }
    }
    for i in [3, 7, 11] {
        m[i] *= units;
    }
    if m.iter().any(|&x| !(x as f32).is_finite()) {
        return Err(error(
            "transform_out_of_range",
            "node transform exceeds glTF floating point range",
            Some(&state.source),
        ));
    }
    Ok(std::array::from_fn(|i| m[(i % 4) * 4 + i / 4]))
}
pub fn export_glb_bytes(
    scene: &MeshScene,
    options: &ExportOptions,
) -> Result<GlbOutput, ExportError> {
    validate_options(options)?;
    if !scene.complete && !options.allow_partial {
        return Err(error(
            "partial_scene_requires_permission",
            "partial scene export requires allow_partial=True",
            None,
        ));
    }
    let graph = &scene.structure.model;
    let mut needed = vec![false; graph.instances.len()];
    let mut count = 0u64;
    let mut selected = BTreeMap::new();
    let mut mesh_ids = BTreeSet::new();
    let mut omitted = BTreeSet::new();
    for (index, occurrence) in scene.instances.iter().enumerate() {
        let mesh = scene
            .meshes
            .get(occurrence.mesh_id as usize)
            .filter(|m| m.id == occurrence.mesh_id)
            .ok_or_else(|| error("invalid_mesh_reference", "unknown mesh ID", None))?;
        if occurrence.id as usize != index
            || selected
                .insert(occurrence.structure_instance_id, occurrence.mesh_id)
                .is_some()
        {
            return Err(error(
                "invalid_mesh_occurrence",
                "duplicate or non-contiguous mesh occurrence",
                Some(&mesh.source),
            ));
        }
        if mesh.triangles.is_empty() {
            omitted.insert(mesh.id);
            selected.remove(&occurrence.structure_instance_id);
            continue;
        }
        mesh_ids.insert(mesh.id);
        let mut at = Some(occurrence.structure_instance_id);
        while let Some(id) = at {
            let state = graph
                .instances
                .get(id as usize)
                .filter(|s| s.id == id && (s.node_id as usize) < graph.nodes.len())
                .ok_or_else(|| {
                    error(
                        "invalid_structure_reference",
                        "invalid structure occurrence",
                        Some(&mesh.source),
                    )
                })?;
            if state.parent_id.is_some_and(|p| p >= id) {
                return Err(error(
                    "invalid_export_hierarchy",
                    "parents must precede their children in the acyclic occurrence tree",
                    Some(&state.source),
                ));
            }
            if needed[id as usize] {
                break;
            }
            needed[id as usize] = true;
            count += 1;
            if count + u64::from(options.up_axis == UpAxis::Z) > u64::from(options.max_nodes) {
                return Err(error(
                    "export_node_limit",
                    "export hierarchy exceeds max_nodes",
                    Some(&state.source),
                ));
            }
            at = state.parent_id;
        }
    }
    if mesh_ids.is_empty() {
        return Err(error(
            "empty_scene",
            "scene contains no nonempty selected mesh",
            None,
        ));
    }
    let mut units: Option<f64> = None;
    let mut fallback = false;
    for &id in selected.keys() {
        let state = &graph.instances[id as usize];
        let value = match state.meters_per_unit {
            Some(v) => v,
            None => {
                fallback = true;
                options.meters_per_unit.ok_or_else(|| {
                    error(
                        "unknown_units",
                        "selected geometry has unknown units; supply meters_per_unit explicitly",
                        Some(&state.source),
                    )
                })?
            }
        };
        if !value.is_finite() || value <= 0. {
            return Err(error(
                "invalid_export_units",
                "unit factor must be finite and positive",
                Some(&state.source),
            ));
        }
        if units.is_some_and(|u| u != value) {
            return Err(error(
                "mixed_export_units",
                "this monolithic GLB profile requires one length unit across selected geometry",
                Some(&state.source),
            ));
        }
        units = Some(value);
    }
    // An unannotated ancestor with zero translation carries only dimensionless
    // rotation/scale. Nonzero unknown translations require an explicit fallback.
    for (index, state) in graph
        .instances
        .iter()
        .enumerate()
        .filter(|(i, _)| needed[*i])
    {
        if state.id as usize != index {
            return Err(error(
                "invalid_structure_reference",
                "non-contiguous structure IDs",
                Some(&state.source),
            ));
        }
        let factor = if let Some(v) = state.meters_per_unit {
            Some(v)
        } else if [3, 7, 11].iter().any(|&i| state.local_transform[i] != 0.) {
            fallback = true;
            Some(options.meters_per_unit.ok_or_else(|| {
                error(
                    "unknown_units",
                    "ancestor translation has unknown units; supply meters_per_unit explicitly",
                    Some(&state.source),
                )
            })?)
        } else {
            None
        };
        if factor.is_some_and(|v| !v.is_finite() || v <= 0. || Some(v) != units) {
            return Err(error(
                "mixed_export_units",
                "ancestor units disagree with selected geometry",
                Some(&state.source),
            ));
        }
    }
    let units = units.unwrap();
    let mut diagnostics = scene.diagnostics.clone();
    if fallback {
        diagnostics.push(diagnostic(
            "export_units_supplied",
            "missing unit metadata was supplemented by the caller",
            None,
        ));
    }
    diagnostics.push(diagnostic("gltf_material_mapping","diffuse/emission mapped to nonmetal PBR (roughness=1); legacy ambient/specular/shininess/reflectivity retained as extras",None));
    diagnostics.push(diagnostic(
        "gltf_axis_policy",
        if options.up_axis == UpAxis::Preserve {
            "source axes preserved; no source up-axis was inferred"
        } else {
            "caller specified the source up-axis"
        },
        None,
    ));
    let mut builder = Builder {
        options,
        bin: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
        json_used: 1024,
    };
    let (mut vertices, mut triangles, mut normalized, mut degenerates, mut maximum_error) =
        (0u64, 0u64, 0u64, 0u64, 0f64);
    let mut geometries = BTreeMap::new();
    for &id in &mesh_ids {
        let mesh = &scene.meshes[id as usize];
        let p = geometry::prepare(
            mesh,
            units,
            options,
            options.max_vertices.saturating_sub(vertices),
            options
                .max_output_bytes
                .saturating_sub(builder.bin.len() as u64),
        )?;
        let position_bytes: Vec<_> = p
            .positions
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let position = builder.accessor(
            &position_bytes,
            p.positions.len(),
            "VEC3",
            5126,
            34962,
            Some(p.bounds),
        )?;
        let normal = if let Some(n) = &p.normals {
            let bytes: Vec<_> = n.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
            normalized += n.len() as u64;
            Some(builder.accessor(&bytes, n.len(), "VEC3", 5126, 34962, None)?)
        } else {
            None
        };
        let bytes: Vec<_> = p.indices.iter().flat_map(|v| v.to_le_bytes()).collect();
        let indices = builder.accessor(&bytes, p.indices.len(), "SCALAR", 5125, 34963, None)?;
        geometries.insert(id, (position, normal, indices));
        vertices += p.positions.len() as u64;
        triangles += mesh.triangles.len() as u64;
        degenerates += p.degenerates;
        maximum_error = maximum_error.max(p.error);
    }
    let mut nodes = Vec::new();
    let mut materials = Vec::new();
    let mut material_cache = BTreeMap::new();
    let mut meshes = Vec::new();
    let mut variants = BTreeMap::new();
    let mut node_map = BTreeMap::new();
    let mut children = BTreeMap::<u32, Vec<u32>>::new();
    let mut roots = Vec::new();
    let mut placed = 0u64;
    let mut default_material_used = false;
    let original_materials: BTreeMap<_, _> = graph
        .materials
        .iter()
        .map(|m| ((m.source.source_id.clone(), m.object_id), m))
        .collect();
    let source_indices: BTreeMap<_, _> = graph
        .sources
        .iter()
        .enumerate()
        .map(|(i, s)| (s.source_id.clone(), i))
        .collect();
    if source_indices.is_empty()
        || source_indices.len() != graph.sources.len()
        || source_indices.contains_key("")
    {
        return Err(error(
            "invalid_export_sources",
            "source identities must be nonempty and unique",
            None,
        ));
    }
    if original_materials.len() != graph.materials.len() {
        return Err(error(
            "invalid_material_origin",
            "duplicate original material IDs",
            None,
        ));
    }
    for (index, state) in graph.instances.iter().enumerate() {
        if !needed[index] {
            continue;
        }
        let original = &graph.nodes[state.node_id as usize];
        if original.name.as_ref().is_some_and(|n| {
            n.len() as u64 > options.max_json_bytes.saturating_sub(builder.json_used)
        }) {
            return Err(error(
                "export_json_limit",
                "node name exceeds remaining JSON budget",
                Some(&state.source),
            ));
        }
        let source_index = source_indices
            .get(&original.source.source_id)
            .ok_or_else(|| {
                error(
                    "invalid_export_sources",
                    "node refers to an unknown source",
                    Some(&original.source),
                )
            })?;
        let mut node = json!({"extras":{"jt":{"objectId":original.object_id,"sourceIndex":source_index,"nodeId":state.node_id,"instanceId":state.id,"partId":state.part_id,"kind":original.kind.name()}}});
        if let Some(name) = &original.name {
            node["name"] = json!(name.as_ref());
        }
        let m = matrix(state, units)?;
        if state.local_transform != IDENTITY {
            node["matrix"] = json!(m);
        }
        if let Some(&mesh_id) = selected.get(&state.id) {
            let (mat, used_default) = material::material(
                &state.material,
                &original_materials,
                &source_indices,
                options,
                &state.source,
            )?;
            default_material_used |= used_default;
            let key = serde_json::to_string(&mat).map_err(|_| {
                error(
                    "invalid_material",
                    "cannot serialize material",
                    Some(&state.source),
                )
            })?;
            let material_id = if let Some(&id) = material_cache.get(&key) {
                id
            } else {
                if materials.len() >= options.max_materials as usize {
                    return Err(error(
                        "export_material_limit",
                        "material count exceeds max_materials",
                        Some(&state.source),
                    ));
                }
                builder.metadata(&mat)?;
                let id = materials.len() as u32;
                materials.push(mat);
                material_cache.insert(key, id);
                id
            };
            let mesh_id = if let Some(&id) = variants.get(&(mesh_id, material_id)) {
                id
            } else {
                if meshes.len() >= options.max_mesh_variants as usize {
                    return Err(error(
                        "export_mesh_limit",
                        "mesh variant count exceeds max_mesh_variants",
                        Some(&state.source),
                    ));
                }
                let (position, normal, indices) = geometries[&mesh_id];
                let mut attributes = json!({"POSITION":position});
                if let Some(normal) = normal {
                    attributes["NORMAL"] = json!(normal);
                }
                let mesh = json!({"primitives":[{"attributes":attributes,"indices":indices,"material":material_id,"mode":4}],"extras":{"jtMeshId":mesh_id}});
                builder.metadata(&mesh)?;
                let id = meshes.len() as u32;
                meshes.push(mesh);
                variants.insert((mesh_id, material_id), id);
                id
            };
            node["mesh"] = json!(mesh_id);
            placed = placed
                .checked_add(scene.meshes[selected[&state.id] as usize].triangles.len() as u64)
                .ok_or_else(|| {
                    error(
                        "export_count_overflow",
                        "placed triangle count overflows u64",
                        Some(&state.source),
                    )
                })?;
        }
        builder.metadata(&node)?;
        let id = nodes.len() as u32;
        node_map.insert(state.id, id);
        nodes.push(node);
        if let Some(parent) = state.parent_id {
            children.entry(parent).or_default().push(id);
        } else {
            roots.push(id);
        }
    }
    for (parent, ids) in children {
        builder.json_used += ids.len() as u64 * 12 + 16;
        nodes[node_map[&parent] as usize]["children"] = json!(ids);
    }
    if options.up_axis == UpAxis::Z {
        let axis = json!({"name":"JT Z-up to glTF Y-up","matrix":[1.,0.,0.,0.,0.,0.,-1.,0.,0.,1.,0.,0.,0.,0.,0.,1.],"children":roots});
        builder.metadata(&axis)?;
        roots = vec![nodes.len() as u32];
        nodes.push(axis);
    }
    if default_material_used {
        diagnostics.push(diagnostic(
            "export_default_color_supplied",
            "missing diffuse fields were supplemented by default_color",
            None,
        ));
    }
    if !omitted.is_empty() {
        diagnostics.push(diagnostic(
            "empty_mesh_omitted",
            "selected empty meshes have no GLB primitive",
            None,
        ));
    }
    if degenerates > 0 {
        diagnostics.push(diagnostic(
            "source_degenerate_triangles",
            "original degenerate triangles retained; no new collapse introduced by f32 conversion",
            None,
        ));
    }
    let sources:Vec<_>=graph.sources.iter().enumerate().map(|(i,s)|json!({"index":i,"version":s.version.map(|v|format!("{}.{}",v.major,v.minor)),"metersPerUnit":s.meters_per_unit})).collect();
    for s in &sources {
        builder.metadata(s)?;
    }
    let mut root = json!({"asset":{"version":"2.0","generator":format!("cq-jt {}",crate::CORE_VERSION),"extras":{"jt":{"sourceComplete":scene.complete,"metersPerUnit":units,"unitFallbackUsed":fallback,"inputUpAxis":options.up_axis.name(),"inputColorSpace":options.color_space.name(),"sources":sources,"assemblyCoordinatesInMeters":graph.coordinates_in_meters}}},"scene":0,"scenes":[{"nodes":roots}],"nodes":nodes,"meshes":meshes,"materials":materials,"buffers":[{"byteLength":builder.bin.len()}],"bufferViews":builder.views,"accessors":builder.accessors});
    if graph
        .sources
        .iter()
        .any(|s| s.version.is_some_and(|v| (v.major, v.minor) == (10, 6)))
    {
        root["asset"]["extras"]["jt"]["unverifiedProfiles"] =
            json!(["JT 10.6 Rev D: synthetic controls only"]);
    }
    // Keep report counts before moving the JSON's arrays into serialization.
    let node_count = root["nodes"].as_array().unwrap().len() as u64;
    let mesh_count = root["meshes"].as_array().unwrap().len() as u64;
    let material_count = root["materials"].as_array().unwrap().len() as u64;
    let mut writer = LimitedVec {
        bytes: Vec::new(),
        limit: options.max_json_bytes,
    };
    serde_json::to_writer(&mut writer, &root)
        .map_err(|_| error("export_json_limit", "GLB JSON exceeds max_json_bytes", None))?;
    let mut json = writer.bytes;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    while builder.bin.len() % 4 != 0 {
        builder.bytes(&[0])?;
    }
    let length = 28u64 + json.len() as u64 + builder.bin.len() as u64;
    if length > options.max_output_bytes || length > u32::MAX as u64 {
        return Err(error(
            "export_byte_limit",
            "complete GLB exceeds max_output_bytes or GLB's u32 length",
            None,
        ));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length as usize)
        .map_err(|_| error("allocation_failed", "cannot allocate GLB output", None))?;
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&(length as u32).to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"JSON");
    bytes.extend(json);
    bytes.extend_from_slice(&(builder.bin.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"BIN\0");
    bytes.extend(builder.bin);
    let report = ExportReport {
        source_complete: scene.complete,
        byte_length: length,
        node_count,
        geometry_count: mesh_ids.len() as u64,
        mesh_count,
        material_count,
        instance_count: selected.len() as u64,
        vertex_count: vertices,
        triangle_count: triangles,
        placed_triangle_count: placed,
        meters_per_unit: units,
        unit_fallback_used: fallback,
        color_space: options.color_space,
        up_axis: options.up_axis,
        max_position_error_meters: maximum_error,
        normalized_normal_count: normalized,
        original_degenerate_triangles: degenerates,
        omitted_empty_meshes: omitted.into_iter().collect(),
        diagnostics,
    };
    Ok(GlbOutput { bytes, report })
}
