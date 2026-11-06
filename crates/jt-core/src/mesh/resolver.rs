//! Bounded local JT partition resolution. No network or Python callbacks.
use super::{scene, Mesh, MeshInstance, MeshScene, ReadOptions};
use crate::{
    compression::failure, structure::*, Diagnostic, DiagnosticSeverity, InspectError, ParseError,
    ParseErrorKind as K, ReadLimits, SourceSpan,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct ResolverOptions {
    pub enabled: bool,
    pub root: Option<PathBuf>,
    pub max_files: u32,
    pub max_depth: u32,
    pub max_references: u32,
    pub max_total_input_bytes: u64,
    /// Supplement missing units when composing an assembly, never replace known units.
    pub meters_per_unit: Option<f64>,
}
impl Default for ResolverOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            root: None,
            max_files: 64,
            max_depth: 32,
            max_references: 4096,
            max_total_input_bytes: 512 * 1024 * 1024,
            meters_per_unit: None,
        }
    }
}
/// `locate` supplies a stable identity without reading content, allowing cache reuse.
/// `read` must honor the supplied input bound before allocating/reading the whole file.
pub trait SourceResolver {
    fn locate(&mut self, owner: &str, reference: &str) -> Result<String, InspectError>;
    fn read(&mut self, key: &str, limits: &ReadLimits) -> Result<Vec<u8>, InspectError>;
}
pub struct LocalFileResolver {
    root: PathBuf,
}
fn fail(source: &str, kind: K, code: &'static str, message: impl Into<String>) -> ParseError {
    failure(
        &SourceSpan {
            source_id: source.into(),
            byte_domain: "file".into(),
            start_offset: 0,
            end_offset: 0,
        },
        kind,
        code,
        message,
    )
}
impl LocalFileResolver {
    pub fn new(root: &Path) -> Result<Self, std::io::Error> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "resolver root must be a directory",
            ));
        }
        Ok(Self { root })
    }
    fn check(&self, path: &Path) -> Result<String, InspectError> {
        let path = path.canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(fail(
                &path.to_string_lossy(),
                K::InvalidGraph,
                "external_path_outside_root",
                "JT reference escapes the resolver root",
            )
            .into());
        }
        path.to_str().map(str::to_owned).ok_or_else(|| {
            fail(
                "resolver",
                K::InvalidGraph,
                "invalid_external_path",
                "external path is not UTF-8",
            )
            .into()
        })
    }
}
impl SourceResolver for LocalFileResolver {
    fn locate(&mut self, owner: &str, reference: &str) -> Result<String, InspectError> {
        let name = reference.replace('\\', "/");
        if name.is_empty() || name.contains(':') || name.contains('\0') || name.starts_with('/') {
            return Err(fail(
                owner,
                K::InvalidGraph,
                "invalid_external_path",
                "external reference must be a relative local filename",
            )
            .into());
        }
        let parent = Path::new(owner).parent().unwrap_or(&self.root);
        self.check(&parent.join(name))
    }
    fn read(&mut self, key: &str, limits: &ReadLimits) -> Result<Vec<u8>, InspectError> {
        let checked = self.check(Path::new(key))?;
        crate::container::read_file(Path::new(&checked), &checked, limits)
    }
}
pub(super) fn read_local(
    path: &Path,
    source_id: &str,
    limits: &ReadLimits,
    options: &ReadOptions,
) -> Result<MeshScene, InspectError> {
    if options.resolver.max_files == 0 {
        return Err(fail(
            source_id,
            K::LimitExceeded,
            "external_file_limit",
            "max_files=0 rejects the root JT file",
        )
        .into());
    }
    let root = options.resolver.root.as_deref().unwrap_or_else(|| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    });
    let mut resolver = LocalFileResolver::new(root)?;
    let key = resolver.check(path)?;
    let mut read_limits = *limits;
    read_limits.max_input_bytes = read_limits
        .max_input_bytes
        .min(options.resolver.max_total_input_bytes);
    let bytes = resolver.read(&key, &read_limits)?;
    Ok(read_resolved_bytes(
        &bytes,
        &key,
        source_id,
        &mut resolver,
        limits,
        options,
    )?)
}
/// Resolve with a caller-provided Rust resolver; `read_bytes` itself performs no IO.
pub fn read_resolved_bytes(
    bytes: &[u8],
    root_key: &str,
    source_id: &str,
    resolver: &mut dyn SourceResolver,
    limits: &ReadLimits,
    options: &ReadOptions,
) -> Result<MeshScene, ParseError> {
    if !options.resolver.enabled {
        return super::read_bytes(bytes, source_id, limits, options);
    }
    if options.resolver.max_depth > 64
        || options
            .resolver
            .meters_per_unit
            .is_some_and(|v| !v.is_finite() || v <= 0.)
    {
        return Err(fail(
            source_id,
            K::InvalidGraph,
            "invalid_resolver_options",
            "resolver depth must be <=64 and supplied units finite/positive",
        ));
    }
    let mut loader = Loader {
        resolver,
        limits: *limits,
        options,
        files: vec![],
        cache: BTreeMap::new(),
        active: BTreeSet::new(),
        links: vec![],
        diagnostics: vec![],
        missing: vec![],
        references: 0,
        input: 0,
        expanded: 0,
        segments: 0,
        elements: 0,
        vertices: 0,
        triangles: 0,
        mesh_bytes: 0,
        meshes: 0,
        codec_values: 0,
        codec_packets: 0,
        structure_references: 0,
        heights: vec![],
    };
    loader.load(root_key, source_id, Some(bytes), 0)?;
    if loader.references == 0 {
        return Ok(loader.files.remove(0));
    }
    loader.compose()
}
struct Loader<'a> {
    resolver: &'a mut dyn SourceResolver,
    limits: ReadLimits,
    options: &'a ReadOptions,
    files: Vec<MeshScene>,
    cache: BTreeMap<String, usize>,
    active: BTreeSet<String>,
    links: Vec<(usize, u32, usize)>,
    diagnostics: Vec<Diagnostic>,
    missing: Vec<MissingReference>,
    references: u32,
    input: u64,
    expanded: u64,
    segments: u32,
    elements: u32,
    vertices: u64,
    triangles: u64,
    mesh_bytes: u64,
    meshes: u32,
    codec_values: u64,
    codec_packets: u64,
    structure_references: u32,
    heights: Vec<u32>,
}
impl Loader<'_> {
    fn load(
        &mut self,
        key: &str,
        source_id: &str,
        root: Option<&[u8]>,
        depth: u32,
    ) -> Result<usize, ParseError> {
        if key.is_empty() || self.active.contains(key) {
            return Err(fail(
                source_id,
                K::InvalidGraph,
                "external_reference_cycle",
                "external JT reference cycle or empty source identity",
            ));
        }
        if depth > self.options.resolver.max_depth
            || self.cache.get(key).is_some_and(|&i| {
                self.heights[i] > self.options.resolver.max_depth.saturating_sub(depth)
            })
        {
            return Err(fail(
                source_id,
                K::LimitExceeded,
                "external_file_limit",
                "external reference depth exceeds its limit, including reused subtrees",
            ));
        }
        if let Some(&index) = self.cache.get(key) {
            return Ok(index);
        }
        if self.files.len() >= self.options.resolver.max_files as usize {
            return Err(fail(
                source_id,
                K::LimitExceeded,
                "external_file_limit",
                "external file count exceeds its limit",
            ));
        }
        if self
            .files
            .iter()
            .any(|s| s.structure.model.source.source_id == source_id)
        {
            return Err(fail(
                source_id,
                K::InvalidGraph,
                "duplicate_external_source_id",
                "source IDs must remain unique across files",
            ));
        }
        let mut limits = self.limits;
        limits.max_input_bytes = limits.max_input_bytes.min(
            self.options
                .resolver
                .max_total_input_bytes
                .saturating_sub(self.input),
        );
        limits.max_total_output_bytes = limits.max_total_output_bytes.saturating_sub(self.expanded);
        limits.max_segments = limits.max_segments.saturating_sub(self.segments);
        limits.max_elements = limits.max_elements.saturating_sub(self.elements);
        let owned;
        let bytes = if let Some(b) = root {
            b
        } else {
            owned = self
                .resolver
                .read(key, &limits)
                .map_err(|e| self.io_error(source_id, e))?;
            &owned
        };
        limits.check_input_size(bytes.len() as u64, source_id)?;
        let mut options = self.options.clone();
        options.structure.max_references = options
            .structure
            .max_references
            .saturating_sub(self.structure_references);
        options.structure.max_nodes = options.structure.max_nodes.saturating_sub(
            self.files
                .iter()
                .map(|s| s.structure.model.nodes.len() as u32)
                .sum::<u32>(),
        );
        options.mesh.max_meshes = options.mesh.max_meshes.saturating_sub(self.meshes);
        options.mesh.max_vertices = options.mesh.max_vertices.saturating_sub(self.vertices);
        options.mesh.max_triangles = options.mesh.max_triangles.saturating_sub(self.triangles);
        options.mesh.max_mesh_bytes = options.mesh.max_mesh_bytes.saturating_sub(self.mesh_bytes);
        options.mesh.max_codec_values = options
            .mesh
            .max_codec_values
            .saturating_sub(self.codec_values);
        options.mesh.max_codec_packets = options
            .mesh
            .max_codec_packets
            .saturating_sub(self.codec_packets);
        let document = Arc::new(crate::structure::read_structure_bytes_profile(
            bytes,
            source_id,
            &limits,
            &options.structure,
            true,
        )?);
        let scene = scene::from_structure(document, &options)?;
        let source = &scene.structure.model;
        let used = source
            .nodes
            .iter()
            .map(|n| (n.attribute_ids.len() + n.child_ids.len()) as u64)
            .sum::<u64>()
            + source.bindings.len() as u64 * 2
            + source
                .missing_references
                .iter()
                .filter(|m| m.kind == "attribute" || m.kind == "child node")
                .count() as u64;
        let total = u64::from(self.structure_references) + used;
        if total > u64::from(self.options.structure.max_references) {
            return Err(fail(
                source_id,
                K::LimitExceeded,
                "reference_limit_exceeded",
                "all source graphs exceed max_references",
            ));
        }
        self.structure_references = total as u32;
        let index = self.files.len();
        let refs = scene
            .structure
            .model
            .instances
            .iter()
            .filter(|i| scene.active[i.id as usize])
            .map(|i| &scene.structure.model.nodes[i.node_id as usize])
            .filter(|n| n.id != 0 && n.kind == NodeKind::Partition)
            .map(|n| n.id)
            .collect::<BTreeSet<_>>();
        self.input += bytes.len() as u64;
        self.expanded += scene.structure.document.total_output_bytes;
        self.segments += scene.structure.document.segments.len() as u32;
        self.elements += scene
            .structure
            .document
            .segments
            .iter()
            .map(|s| s.elements.len() as u32)
            .sum::<u32>();
        self.vertices += scene
            .meshes
            .iter()
            .map(|m| m.positions.len() as u64)
            .sum::<u64>();
        self.triangles += scene
            .meshes
            .iter()
            .map(|m| m.triangles.len() as u64)
            .sum::<u64>();
        self.mesh_bytes += scene.meshes.iter().map(Mesh::buffer_bytes).sum::<u64>();
        self.meshes += scene.meshes.len() as u32;
        self.codec_values += scene.codec_values_used;
        self.codec_packets += scene.codec_packets_used;
        self.files.push(scene);
        self.heights.push(0);
        self.cache.insert(key.into(), index);
        self.active.insert(key.into());
        for node_id in refs {
            self.references = self
                .references
                .checked_add(1)
                .filter(|v| *v <= self.options.resolver.max_references)
                .ok_or_else(|| {
                    fail(
                        source_id,
                        K::LimitExceeded,
                        "external_reference_limit",
                        "too many external partition references",
                    )
                })?;
            let node = self.files[index].structure.model.nodes[node_id as usize].clone();
            if !node.child_ids.is_empty() {
                return Err(failure(
                    &node.source,
                    K::InvalidGraph,
                    "invalid_external_partition",
                    "external partition must be a leaf",
                ));
            }
            let name = node.file_name.as_deref().unwrap_or("");
            let result = self
                .resolver
                .locate(key, name)
                .map_err(|e| self.io_error(source_id, e))
                .and_then(|target| self.load(&target, &target, None, depth + 1));
            match result {
                Ok(target) => {
                    self.heights[index] = self.heights[index].max(self.heights[target] + 1);
                    self.links.push((index, node_id, target));
                }
                Err(e)
                    if self.options.structure.mode == ReadMode::BestEffort
                        && matches!(
                            e.kind,
                            K::MissingReference | K::UnsupportedEncoding | K::UnsupportedVersion
                        ) =>
                {
                    self.diagnostics.push(Diagnostic {
                        code: e.code.into(),
                        message: e.message,
                        severity: DiagnosticSeverity::Warning,
                        source: Some(node.source.clone()),
                    });
                    self.missing.push(MissingReference {
                        owner_object_id: node.object_id,
                        target: name.into(),
                        kind: "external partition".into(),
                        source: node.source,
                    });
                }
                Err(e) => return Err(e),
            }
        }
        self.active.remove(key);
        Ok(index)
    }
    fn io_error(&self, source: &str, error: InspectError) -> ParseError {
        match error {
            InspectError::Parse(e) => e,
            InspectError::Io(e) => fail(
                source,
                K::MissingReference,
                "external_file_unavailable",
                format!("cannot read external JT file: {e}"),
            ),
        }
    }
    fn compose(self) -> Result<MeshScene, ParseError> {
        let meshes_complete = self.files.iter().all(|s| s.complete);
        let mut model = self.files[0].structure.model.clone();
        model.nodes.clear();
        model.attributes.clear();
        model.materials.clear();
        model.atoms.clear();
        model.bindings.clear();
        model.parts.clear();
        model.instances.clear();
        model.sources.clear();
        model.blocked_node_ids.clear();
        model.opaque_objects.clear();
        model.missing_references = self.missing;
        model.diagnostics = self.diagnostics;
        model.complete = model.missing_references.is_empty();
        let mut offsets = Vec::new();
        let mut shape_meshes = BTreeMap::new();
        for (file_index, file) in self.files.iter().enumerate() {
            let source = &file.structure.model;
            let (node_offset, attr_offset, material_offset, part_offset) = (
                model.nodes.len() as u32,
                model.attributes.len() as u32,
                model.materials.len() as u32,
                model.parts.len() as u32,
            );
            offsets.push(node_offset);
            model.sources.push(source.source.clone());
            model.complete &= source.complete;
            model.diagnostics.extend(source.diagnostics.clone());
            model
                .missing_references
                .extend(source.missing_references.clone());
            model
                .blocked_node_ids
                .extend(source.blocked_node_ids.iter().map(|i| i + node_offset));
            for mut n in source.nodes.clone() {
                n.id += node_offset;
                for i in &mut n.attribute_ids {
                    *i += attr_offset;
                }
                for i in &mut n.child_ids {
                    *i += node_offset;
                }
                model.nodes.push(n);
            }
            for mut a in source.attributes.clone() {
                a.id += attr_offset;
                if let Some(ref mut i) = a.material_id {
                    *i += material_offset;
                }
                model.attributes.push(a);
            }
            for mut m in source.materials.clone() {
                m.id += material_offset;
                model.materials.push(m);
            }
            for mut p in source.parts.clone() {
                p.id += part_offset;
                p.node_id += node_offset;
                for i in &mut p.shape_node_ids {
                    *i += node_offset;
                }
                model.parts.push(p);
            }
            model.atoms.extend(source.atoms.clone());
            model.bindings.extend(source.bindings.clone());
            model.opaque_objects.extend(source.opaque_objects.clone());
            for occurrence in &file.instances {
                let node = source.instances[occurrence.structure_instance_id as usize].node_id
                    + node_offset;
                shape_meshes.insert(node, (file_index, occurrence.mesh_id));
            }
        }
        for (file, node, target) in self.links {
            model.nodes[(offsets[file] + node) as usize]
                .child_ids
                .push(offsets[target]);
        }
        crate::structure::resolve::instantiate_assembly(
            &mut model,
            &self.options.structure,
            self.options.resolver.meters_per_unit,
        )?;
        let structure = Arc::new(StructureDocument {
            model,
            document: self.files[0].structure.document.clone(),
            referenced_documents: self
                .files
                .iter()
                .skip(1)
                .map(|f| f.structure.document.clone())
                .collect(),
        });
        let selection = scene::select(&structure.model, self.options)?;
        let mut meshes = Vec::<Mesh>::new();
        let mut instances = Vec::new();
        let mut cache = BTreeMap::new();
        let (mut vertices, mut triangles, mut mesh_bytes) = (0u64, 0u64, 0u64);
        for state in &structure.model.instances {
            if !selection.active[state.id as usize] {
                continue;
            }
            let Some(&(file, source_mesh)) = shape_meshes.get(&state.node_id) else {
                continue;
            };
            let units = state.source_meters_per_unit.ok_or_else(|| {
                failure(
                    &state.source,
                    K::UnsupportedEncoding,
                    "unknown_assembly_units",
                    "geometry units are unknown",
                )
            })?;
            let key = (file, source_mesh, units.to_bits());
            let id = if let Some(&id) = cache.get(&key) {
                id
            } else {
                let original = &self.files[file].meshes[source_mesh as usize];
                vertices += original.positions.len() as u64;
                triangles += original.triangles.len() as u64;
                mesh_bytes += original.buffer_bytes();
                if meshes.len() >= self.options.mesh.max_meshes as usize
                    || vertices > self.options.mesh.max_vertices
                    || triangles > self.options.mesh.max_triangles
                    || mesh_bytes > self.options.mesh.max_mesh_bytes
                {
                    return Err(failure(
                        &original.source,
                        K::LimitExceeded,
                        "assembly_mesh_limit",
                        "normalized assembly exceeds mesh limits",
                    ));
                }
                let mut m = original.clone();
                m.id = meshes.len() as u32;
                for p in &mut m.positions {
                    for x in p {
                        *x *= units;
                        if !x.is_finite() {
                            return Err(failure(
                                &m.source,
                                K::InvalidGeometry,
                                "assembly_coordinate_overflow",
                                "unit conversion overflow",
                            ));
                        }
                    }
                }
                m.bounds = m.bounds.map(|b| b.map(|x| x * units));
                m.quantization_step = m.quantization_step.map(|x| x * units);
                let id = m.id;
                meshes.push(m);
                cache.insert(key, id);
                id
            };
            instances.push(MeshInstance {
                id: instances.len() as u32,
                structure_instance_id: state.id,
                mesh_id: id,
            });
        }
        let mut diagnostics = selection.diagnostics;
        for file in &self.files {
            diagnostics.extend(
                file.diagnostics
                    .iter()
                    .skip(file.structure.model.diagnostics.len())
                    .cloned(),
            );
        }
        diagnostics.push(Diagnostic {
            code: "assembly_coordinates_in_meters".into(),
            message: format!(
                "resolved {} JT sources; geometry and translations normalized to meters",
                self.files.len()
            ),
            severity: DiagnosticSeverity::Info,
            source: None,
        });
        if self.options.resolver.meters_per_unit.is_some() {
            diagnostics.push(Diagnostic {
                code: "assembly_units_supplied".into(),
                message: "caller supplied fallback units for missing assembly metadata".into(),
                severity: DiagnosticSeverity::Info,
                source: None,
            });
        }
        Ok(MeshScene {
            structure,
            meshes,
            instances,
            lod_choices: selection.choices,
            diagnostics,
            complete: selection.complete && meshes_complete,
            active: selection.active,
            codec_values_used: self.codec_values,
            codec_packets_used: self.codec_packets,
        })
    }
}
