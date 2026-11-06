use super::{reader::Parsed, value::*};
use crate::{
    compression::failure, DecodedDocument, Diagnostic, DiagnosticSeverity, ParseError,
    ParseErrorKind as K, SourceMetadata, SourceSpan,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) fn valid_transform(m: &Matrix4) -> bool {
    if m.iter().any(|v| !v.is_finite()) || m[12..] != [0., 0., 0., 1.] {
        return false;
    }
    let mut norms = [0.; 3];
    for col in 0..3 {
        norms[col] = (0..3).map(|r| m[r * 4 + col] * m[r * 4 + col]).sum::<f64>();
        if !norms[col].is_finite() || norms[col] == 0. {
            return false;
        }
    }
    for a in 0..3 {
        for b in a + 1..3 {
            let dot = (0..3).map(|r| m[r * 4 + a] * m[r * 4 + b]).sum::<f64>();
            if dot.abs() > 1e-6 * norms[a].sqrt() * norms[b].sqrt() {
                return false;
            }
        }
    }
    true
}
fn multiply(a: &Matrix4, b: &Matrix4, source: &SourceSpan) -> Result<Matrix4, ParseError> {
    let mut out = [0.; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = (0..4).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum();
        }
    }
    if out.iter().any(|v| !v.is_finite()) {
        return Err(failure(
            source,
            K::InvalidGraph,
            "transform_overflow",
            "composed transform is not finite",
        ));
    }
    Ok(out)
}
pub fn meters_per_unit(value: &str) -> Option<f64> {
    Some(match value.to_ascii_lowercase().as_str() {
        "millimeters" => 0.001,
        "centimeters" => 0.01,
        "meters" => 1.,
        "inches" => 0.0254,
        "feet" => 0.3048,
        "yards" => 0.9144,
        "micrometers" => 1e-6,
        "decimeters" => 0.1,
        "kilometers" => 1000.,
        "mils" => 0.0000254,
        "miles" => 1609.344,
        _ => return None,
    })
}
struct Status<'a> {
    options: &'a StructureOptions,
    complete: bool,
    diagnostics: Vec<Diagnostic>,
    missing: Vec<MissingReference>,
}
impl Status<'_> {
    fn unresolved(
        &mut self,
        owner: i32,
        target: String,
        kind: &str,
        source: &SourceSpan,
    ) -> Result<(), ParseError> {
        let message = format!("object {owner}: unresolved {kind} {target}");
        if self.options.mode == ReadMode::Strict {
            return Err(failure(
                source,
                K::MissingReference,
                "missing_reference",
                message,
            ));
        }
        self.complete = false;
        self.diagnostics.push(Diagnostic {
            code: "missing_reference".into(),
            message,
            severity: DiagnosticSeverity::Warning,
            source: Some(source.clone()),
        });
        self.missing.push(MissingReference {
            owner_object_id: owner,
            target,
            kind: kind.into(),
            source: source.clone(),
        });
        Ok(())
    }
    fn unsupported(
        &mut self,
        code: &'static str,
        message: &str,
        source: &SourceSpan,
    ) -> Result<(), ParseError> {
        if self.options.mode == ReadMode::Strict {
            return Err(failure(source, K::UnsupportedEncoding, code, message));
        }
        self.complete = false;
        self.diagnostics.push(Diagnostic {
            code: code.into(),
            message: message.into(),
            severity: DiagnosticSeverity::Warning,
            source: Some(source.clone()),
        });
        Ok(())
    }
}

pub(super) fn resolve(
    mut p: Parsed,
    document: &DecodedDocument,
    options: &StructureOptions,
    external_boundaries: bool,
) -> Result<StructureScene, ParseError> {
    let mut status = Status {
        options,
        complete: p.complete,
        diagnostics: p.diagnostics,
        missing: vec![],
    };
    let nodes_by_object = p
        .nodes
        .iter()
        .map(|n| (n.node.object_id, n.node.id))
        .collect::<BTreeMap<_, _>>();
    let attrs_by_object = p
        .attributes
        .iter()
        .map(|a| (a.object_id, a.id))
        .collect::<BTreeMap<_, _>>();
    let atoms_by_object = p
        .atoms
        .iter()
        .enumerate()
        .map(|(i, a)| (a.object_id, i))
        .collect::<BTreeMap<_, _>>();
    let mut blocked_nodes = BTreeSet::new();
    for raw in &mut p.nodes {
        for id in &raw.attrs {
            if let Some(local) = attrs_by_object.get(id) {
                raw.node.attribute_ids.push(*local);
            } else {
                blocked_nodes.insert(raw.node.id);
                status.unresolved(
                    raw.node.object_id,
                    id.to_string(),
                    "attribute",
                    &raw.node.source,
                )?;
            }
        }
        for id in &raw.children {
            if let Some(local) = nodes_by_object.get(id) {
                raw.node.child_ids.push(*local);
            } else {
                status.unresolved(
                    raw.node.object_id,
                    id.to_string(),
                    "child node",
                    &raw.node.source,
                )?;
            }
        }
        // A Switch index addresses the original list, so it cannot be used after
        // dropping missing edges. Keep the selected source child explicitly.
        if let Some(selected) = raw.node.selected_child {
            raw.node.selected_child = if selected < 0 {
                Some(-1)
            } else {
                Some(
                    if nodes_by_object.contains_key(&raw.children[selected as usize]) {
                        raw.children[..selected as usize]
                            .iter()
                            .filter(|id| nodes_by_object.contains_key(id))
                            .count() as i32
                    } else {
                        -1
                    },
                )
            };
        }
    }
    for binding in &mut p.bindings {
        if !nodes_by_object.contains_key(&binding.owner_object_id)
            && !attrs_by_object.contains_key(&binding.owner_object_id)
        {
            status.unresolved(
                binding.owner_object_id,
                binding.owner_object_id.to_string(),
                "property owner",
                &binding.source,
            )?;
        }
        match atoms_by_object
            .get(&binding.key_atom_id)
            .map(|&i| &p.atoms[i].value)
        {
            Some(PropertyValue::String(key)) => binding.key = Some(key.clone()),
            Some(_) => status.unsupported(
                "invalid_property_key_type",
                "property key must be a supported string atom",
                &binding.source,
            )?,
            None => status.unresolved(
                binding.owner_object_id,
                binding.key_atom_id.to_string(),
                "property key atom",
                &binding.source,
            )?,
        }
        if !atoms_by_object.contains_key(&binding.value_atom_id) {
            status.unresolved(
                binding.owner_object_id,
                binding.value_atom_id.to_string(),
                "property value atom",
                &binding.source,
            )?;
        }
    }
    let segments = document
        .container
        .segments
        .iter()
        .map(|s| (s.guid_raw, s.segment_type))
        .collect::<BTreeMap<_, _>>();
    for atom in &p.atoms {
        if let PropertyValue::LateLoaded(reference) = &atom.value {
            match segments.get(&reference.segment_guid) {
                Some(kind) if i64::from(*kind) == i64::from(reference.segment_type) => {}
                Some(_) => {
                    return Err(failure(
                        &atom.source,
                        K::InvalidGraph,
                        "late_segment_type_mismatch",
                        "late-loaded property and TOC segment types disagree",
                    ))
                }
                None => status.unresolved(
                    atom.object_id,
                    reference
                        .segment_guid
                        .iter()
                        .flat_map(|b| {
                            const H: &[u8; 16] = b"0123456789abcdef";
                            [H[(b >> 4) as usize] as char, H[(b & 15) as usize] as char]
                        })
                        .collect::<String>(),
                    "late-loaded segment",
                    &atom.source,
                )?,
            }
        }
    }
    let mut bindings_by_owner = BTreeMap::<i32, Vec<usize>>::new();
    for (index, binding) in p.bindings.iter().enumerate() {
        bindings_by_owner
            .entry(binding.owner_object_id)
            .or_default()
            .push(index);
    }
    let mut nodes = p.nodes.into_iter().map(|raw| raw.node).collect::<Vec<_>>();
    for node in &mut nodes {
        let mut names = BTreeMap::<&str, Arc<str>>::new();
        for &index in bindings_by_owner.get(&node.object_id).into_iter().flatten() {
            let b = &p.bindings[index];
            let Some(key) = b.key.as_deref() else {
                blocked_nodes.insert(node.id);
                continue;
            };
            let Some(value) = atoms_by_object
                .get(&b.value_atom_id)
                .map(|&i| &p.atoms[i].value)
            else {
                if key == "JT_PROP_MEASUREMENT_UNITS" {
                    blocked_nodes.insert(node.id);
                }
                continue;
            };
            if let PropertyValue::LateLoaded(reference) = value {
                node.payload_references.push(reference.clone());
            }
            if matches!(key, "JT_PROP_NAME" | "Name::" | "JT_PROP_MEASUREMENT_UNITS") {
                if let PropertyValue::String(value) = value {
                    if names
                        .insert(key, value.clone())
                        .is_some_and(|old| old != *value)
                    {
                        return Err(failure(
                            &b.source,
                            K::InvalidGraph,
                            "ambiguous_property",
                            "conflicting name or measurement-unit properties",
                        ));
                    }
                } else {
                    if key == "JT_PROP_MEASUREMENT_UNITS" {
                        blocked_nodes.insert(node.id);
                    }
                    status.unsupported(
                        "invalid_core_property_type",
                        "name and measurement units must be string atoms",
                        &b.source,
                    )?;
                }
            }
        }
        node.name = names
            .get("JT_PROP_NAME")
            .or_else(|| names.get("Name::"))
            .cloned();
        if node.name.is_none() && node.kind == NodeKind::Partition {
            node.name = node.file_name.as_deref().map(Arc::from);
        }
        node.measurement_units = names.get("JT_PROP_MEASUREMENT_UNITS").cloned();
        if let Some(unit) = &node.measurement_units {
            if meters_per_unit(unit).is_none() {
                status.diagnostics.push(Diagnostic {
                    code: "unknown_measurement_units".into(),
                    message: format!("unknown model units: {unit}"),
                    severity: DiagnosticSeverity::Warning,
                    source: Some(node.source.clone()),
                });
            }
        }
        if node.id != 0 && node.kind == NodeKind::Partition && !external_boundaries {
            status.unresolved(
                node.object_id,
                node.file_name.clone().unwrap_or_default(),
                "external partition (not resolved in the structure profile)",
                &node.source,
            )?;
        }
    }
    validate_graph(&nodes, options)?;
    let mut parts = Vec::new();
    let mut part_by_node = BTreeMap::new();
    let mut work = 0u64;
    for node in &nodes {
        if node.kind == NodeKind::Part {
            let mut shapes = BTreeSet::new();
            let mut seen = BTreeSet::new();
            let mut stack = vec![node.id];
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                work += 1;
                if work > u64::from(options.max_references) {
                    return Err(failure(
                        &node.source,
                        K::LimitExceeded,
                        "traversal_limit_exceeded",
                        "part-definition traversal exceeds max_references",
                    ));
                }
                let n = &nodes[id as usize];
                if n.kind == NodeKind::TriStripShape {
                    shapes.insert(id);
                }
                if n.kind != NodeKind::Part || id == node.id {
                    stack.extend(n.child_ids.iter().copied());
                }
            }
            let id = parts.len() as u32;
            part_by_node.insert(node.id, id);
            parts.push(PartDefinition {
                id,
                node_id: node.id,
                name: node.name.clone(),
                shape_node_ids: shapes.into_iter().collect(),
                source: node.source.clone(),
            });
        }
    }
    let instances = instantiate(
        &nodes,
        &p.attributes,
        &p.materials,
        &part_by_node,
        &blocked_nodes,
        &mut status,
        Coordinates::Source,
    )?;
    let source = SourceMetadata {
        source_id: document.container.source_id.clone(),
        label: Some(document.container.source_id.clone()),
        version: Some(document.container.version),
        meters_per_unit: nodes[0]
            .measurement_units
            .as_deref()
            .and_then(meters_per_unit),
    };
    Ok(StructureScene {
        sources: vec![source.clone()],
        coordinates_in_meters: false,
        blocked_node_ids: blocked_nodes.into_iter().collect(),
        source,
        lsg_segment_index: p.segment_index,
        lsg_guid: document.container.lsg_guid_raw,
        root_node_id: 0,
        nodes,
        attributes: p.attributes,
        materials: p.materials,
        atoms: p.atoms,
        bindings: p.bindings,
        parts,
        instances,
        opaque_objects: p.opaque,
        missing_references: status.missing,
        diagnostics: status.diagnostics,
        complete: status.complete,
    })
}

pub(crate) fn instantiate_assembly(
    model: &mut StructureScene,
    options: &StructureOptions,
    units: Option<f64>,
) -> Result<(), ParseError> {
    validate_graph(&model.nodes, options)?;
    let parts = model.parts.iter().map(|p| (p.node_id, p.id)).collect();
    let blocked = model.blocked_node_ids.iter().copied().collect();
    let mut status = Status {
        options,
        complete: model.complete,
        diagnostics: std::mem::take(&mut model.diagnostics),
        missing: std::mem::take(&mut model.missing_references),
    };
    model.instances = instantiate(
        &model.nodes,
        &model.attributes,
        &model.materials,
        &parts,
        &blocked,
        &mut status,
        Coordinates::Meters(units),
    )?;
    model.complete = status.complete;
    model.diagnostics = status.diagnostics;
    model.missing_references = status.missing;
    model.coordinates_in_meters = true;
    Ok(())
}
fn validate_graph(nodes: &[GraphNode], o: &StructureOptions) -> Result<(), ParseError> {
    let mut color = vec![0u8; nodes.len()];
    for first in 0..nodes.len() {
        if color[first] != 0 {
            continue;
        }
        color[first] = 1;
        let mut stack = vec![(first, 0)];
        while let Some((index, next)) = stack.last_mut() {
            let node = &nodes[*index];
            if *next == node.child_ids.len() {
                color[*index] = 2;
                stack.pop();
                continue;
            }
            let child = node.child_ids[*next] as usize;
            *next += 1;
            if color[child] == 1 {
                return Err(failure(
                    &node.source,
                    K::InvalidGraph,
                    "graph_cycle",
                    "LSG contains a child/instance cycle",
                ));
            }
            if color[child] == 0 {
                if stack.len() >= o.max_depth as usize {
                    return Err(failure(
                        &node.source,
                        K::LimitExceeded,
                        "depth_limit_exceeded",
                        "LSG traversal exceeds max_depth",
                    ));
                }
                color[child] = 1;
                stack.push((child, 0));
            }
        }
    }
    Ok(())
}
#[derive(Clone)]
struct State {
    world: Matrix4,
    transform_final: bool,
    material_final: bool,
    material_field_final: u32,
    material: EffectiveMaterial,
    unit_node_id: Option<u32>,
    meters: Option<f64>,
    source_id: Option<String>,
}
enum Coordinates {
    Source,
    Meters(Option<f64>),
}
fn instantiate(
    nodes: &[GraphNode],
    attributes: &[Attribute],
    materials: &[Material],
    parts: &BTreeMap<u32, u32>,
    blocked_nodes: &BTreeSet<u32>,
    status: &mut Status<'_>,
    coordinates: Coordinates,
) -> Result<Vec<Instance>, ParseError> {
    let normalize = matches!(coordinates, Coordinates::Meters(_));
    let fallback_units = match coordinates {
        Coordinates::Source => None,
        Coordinates::Meters(u) => u,
    };
    let initial = State {
        world: IDENTITY,
        transform_final: false,
        material_final: false,
        material_field_final: 0,
        material: EffectiveMaterial::default(),
        unit_node_id: None,
        meters: None,
        source_id: None,
    };
    let mut stack = vec![(0u32, None, initial, 1u32)];
    let mut instances = Vec::new();
    while let Some((node_id, parent_id, mut state, depth)) = stack.pop() {
        let n = &nodes[node_id as usize];
        if normalize && state.source_id.as_deref() != Some(&n.source.source_id) {
            state.meters = None;
            state.unit_node_id = None;
            state.source_id = Some(n.source.source_id.clone());
        }
        if n.flags & 1 != 0 || blocked_nodes.contains(&node_id) {
            continue;
        }
        if depth > status.options.max_depth {
            return Err(failure(
                &n.source,
                K::LimitExceeded,
                "depth_limit_exceeded",
                "instance path exceeds max_depth",
            ));
        }
        if instances.len() >= status.options.max_instances as usize {
            return Err(failure(
                &n.source,
                K::LimitExceeded,
                "instance_limit_exceeded",
                "occurrence count exceeds max_instances",
            ));
        }
        let mut local = IDENTITY;
        for &id in &n.attribute_ids {
            let attr = &attributes[id as usize];
            if attr.state_flags & 4 != 0 {
                continue;
            }
            let force = attr.state_flags & 2 != 0;
            let final_flag = attr.state_flags & 1 != 0;
            if let Some(matrix) = attr.transform {
                if !state.transform_final || force {
                    local = multiply(&local, &matrix, &attr.source)?;
                    state.transform_final |= final_flag;
                }
            }
            if let Some(id) = attr.material_id {
                if !state.material_final || force {
                    apply_material(
                        &mut state.material,
                        &materials[id as usize],
                        &mut state.material_field_final,
                        force,
                    );
                    state.material_final |= final_flag;
                }
            }
        }
        if let Some(unit) = &n.measurement_units {
            let meters = meters_per_unit(unit);
            if state.meters.zip(meters).is_some_and(|(a, b)| a != b) {
                status.unsupported("mixed_unit_transform_unsupported","mixed units on a single path require a conversion profile; occurrence branch omitted",&n.source)?;
                continue;
            }
            state.unit_node_id = Some(n.id);
            state.meters = meters;
        }
        let source_meters = state.meters.or(fallback_units);
        if normalize
            && (local[3] != 0.
                || local[7] != 0.
                || local[11] != 0.
                || n.kind == NodeKind::TriStripShape)
        {
            let Some(units) = source_meters else {
                status.unsupported(
                    "unknown_assembly_units",
                    "external assembly geometry/translation needs explicit units; branch omitted",
                    &n.source,
                )?;
                continue;
            };
            for i in [3, 7, 11] {
                local[i] *= units;
            }
        }
        state.world = multiply(&state.world, &local, &n.source)?;
        let id = instances.len() as u32;
        instances.push(Instance {
            id,
            node_id,
            parent_id,
            part_id: parts.get(&node_id).copied(),
            local_transform: local,
            world_transform: state.world,
            unit_node_id: state.unit_node_id,
            meters_per_unit: if normalize { Some(1.) } else { state.meters },
            source_meters_per_unit: if normalize {
                source_meters
            } else {
                state.meters
            },
            material: state.material.clone(),
            source: n.source.clone(),
        });
        let children = n
            .child_ids
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                n.selected_child
                    .is_none_or(|selected| selected >= 0 && *index == selected as usize)
            })
            .map(|(_, child)| *child)
            .filter(|&child| {
                nodes[child as usize].flags & 1 == 0 && !blocked_nodes.contains(&child)
            })
            .collect::<Vec<_>>();
        if instances
            .len()
            .saturating_add(stack.len())
            .saturating_add(children.len())
            > status.options.max_instances as usize
        {
            return Err(failure(
                &n.source,
                K::LimitExceeded,
                "instance_limit_exceeded",
                "pending occurrences exceed max_instances",
            ));
        }
        for child in children.into_iter().rev() {
            stack.push((child, Some(id), state.clone(), depth.saturating_add(1)));
        }
    }
    Ok(instances)
}
fn apply_material(
    state: &mut EffectiveMaterial,
    m: &Material,
    final_fields: &mut u32,
    force: bool,
) {
    let flags = m.inhibit_flags;
    let modern = m.field_final_flags.is_some();
    let masks = if modern {
        [1, 1 << 6, 1 << 7, 1 << 1, 1 << 2, 1 << 3, 1 << 4]
    } else {
        [
            1,
            (1 << 1) | (1 << 7),
            (1 << 1) | (1 << 8),
            1 << 2,
            1 << 3,
            1 << 4,
            1 << 5,
        ]
    };
    let mut allowed = [false; 7];
    for i in 0..7 {
        // Field-final state is kept by logical field, across file generations.
        allowed[i] = flags & masks[i] == 0 && (*final_fields & (1 << i) == 0 || force);
        if allowed[i] {
            state.origins[i] = Some(m.object_id);
            state.origin_sources[i] = Some(m.source.source_id.clone());
            if m.field_final_flags.is_some_and(|f| f & masks[i] != 0) {
                *final_fields |= 1 << i;
            }
        }
    }
    if allowed[0] {
        state.ambient = Some(m.ambient);
        state.origins[0] = Some(m.object_id);
    }
    if allowed[1] {
        state.diffuse_rgb = Some(m.diffuse[..3].try_into().expect("RGB width"));
        state.origins[1] = Some(m.object_id);
    }
    if allowed[2] {
        state.diffuse_alpha = Some(m.diffuse[3]);
        state.origins[2] = Some(m.object_id);
    }
    if allowed[3] {
        state.specular = Some(m.specular);
        state.origins[3] = Some(m.object_id);
    }
    if allowed[4] {
        state.emission = Some(m.emission);
        state.origins[4] = Some(m.object_id);
    }
    if allowed[5] {
        state.blending = Some(m.data_flags & 0x10 != 0);
        state.origins[5] = Some(m.object_id);
    }
    if allowed[6] {
        state.override_vertex_colors = Some(m.data_flags & 0x20 != 0);
        state.origins[6] = Some(m.object_id);
    }
}
