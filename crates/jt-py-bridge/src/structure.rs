use super::{
    diagnostic_to_python, ensure_python_payload_api, optional_to_python, source_to_python,
    span_to_python,
};
use jt_core::structure::*;
use pyo3::{
    exceptions::{PyImportError, PyValueError},
    prelude::*,
    types::{PyBool, PyBytes, PyDict, PyInt, PyTuple},
};
pub const STRUCTURE_API_VERSION: u32 = 3;
pub fn ensure_python_structure_api(py: Python<'_>) -> PyResult<()> {
    ensure_python_payload_api(py)?;
    if py
        .import("cq_jt.structure")
        .and_then(|m| m.getattr("STRUCTURE_API_VERSION"))
        .is_ok_and(|v| {
            v.get_type().is(py.get_type::<PyInt>())
                && v.extract::<u32>().ok() == Some(STRUCTURE_API_VERSION)
        })
    {
        Ok(())
    } else {
        Err(PyImportError::new_err(
            "native structure reading requires cq_jt.structure.STRUCTURE_API_VERSION == 3",
        ))
    }
}
pub fn structure_options_from_python(
    py: Python<'_>,
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<StructureOptions> {
    ensure_python_structure_api(py)?;
    let Some(value) = value else {
        return Ok(StructureOptions::default());
    };
    if !value
        .get_type()
        .is(py.import("cq_jt.structure")?.getattr("StructureOptions")?)
    {
        return Err(PyValueError::new_err(
            "options must be cq_jt.StructureOptions or None",
        ));
    }
    let mode = match value.getattr("mode")?.extract::<String>()?.as_str() {
        "strict" => ReadMode::Strict,
        "best_effort" => ReadMode::BestEffort,
        _ => return Err(PyValueError::new_err("mode must be strict or best_effort")),
    };
    let number = |name: &str| -> PyResult<u64> {
        let v = value.getattr(name)?;
        if !v.get_type().is(py.get_type::<PyInt>()) {
            return Err(PyValueError::new_err(format!(
                "{name} must be an integer, not bool or float"
            )));
        }
        v.extract()
            .map_err(|_| PyValueError::new_err(format!("{name} must fit u64")))
    };
    let count = |name: &str| -> PyResult<u32> {
        u32::try_from(number(name)?)
            .map_err(|_| PyValueError::new_err(format!("{name} must fit u32")))
    };
    let experimental = value.getattr("experimental_jt106")?;
    if !experimental.get_type().is(py.get_type::<PyBool>()) {
        return Err(PyValueError::new_err("experimental_jt106 must be bool"));
    }
    Ok(StructureOptions {
        mode,
        max_nodes: count("max_nodes")?,
        max_instances: count("max_instances")?,
        max_depth: count("max_depth")?,
        max_references: count("max_references")?,
        max_string_bytes: number("max_string_bytes")?,
        experimental_jt106: experimental.extract()?,
    })
}
fn new<'py>(py: Python<'py>, name: &str, kw: &Bound<'py, PyDict>) -> PyResult<Bound<'py, PyAny>> {
    py.import("cq_jt.structure")?
        .getattr(name)?
        .call((), Some(kw))
}
macro_rules! dto{($py:expr,$name:literal,{$($key:literal=>$value:expr),*$(,)?})=>{{let kw=PyDict::new($py);$(kw.set_item($key,$value)?;)*new($py,$name,&kw)}};}
fn array<'py, T: Copy + IntoPyObject<'py>, const N: usize>(
    py: Python<'py>,
    value: &[T; N],
) -> PyResult<Bound<'py, PyTuple>> {
    PyTuple::new(py, value.iter().copied())
}
fn reference<'py>(py: Python<'py>, v: &LateLoadedReference) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"LateLoadedReference",{"segment_guid"=>PyBytes::new(py,&v.segment_guid),"segment_type"=>v.segment_type,"payload_object_id"=>v.payload_object_id,"reserved"=>v.reserved})
}
fn shape<'py>(py: Python<'py>, v: &ShapeMetadata) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"ShapeMetadata",{
    "transformed_bounds"=>optional_to_python(py,v.transformed_bounds.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"local_bounds"=>array(py,&v.local_bounds)?,"area"=>v.area,"vertex_count_range"=>array(py,&v.vertex_count_range)?,"node_count_range"=>array(py,&v.node_count_range)?,"polygon_count_range"=>array(py,&v.polygon_count_range)?,"size"=>v.size,"compression_level"=>v.compression_level,"vertex_version"=>v.vertex_version,"vertex_binding"=>v.vertex_binding,"quantization"=>optional_to_python(py,v.quantization.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"vertex_binding2"=>v.vertex_binding2})
}
fn node<'py>(py: Python<'py>, v: &GraphNode) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"GraphNode",{
    "id"=>v.id,"object_id"=>v.object_id,"kind"=>v.kind.name(),"flags"=>v.flags,"attribute_ids"=>PyTuple::new(py,&v.attribute_ids)?,"child_ids"=>PyTuple::new(py,&v.child_ids)?,"name"=>v.name.as_deref(),"measurement_units"=>v.measurement_units.as_deref(),"file_name"=>&v.file_name,"partition_flags"=>v.partition_flags,"reserved"=>v.reserved,"lod_ranges"=>PyTuple::new(py,&v.lod_ranges)?,"lod_center"=>optional_to_python(py,v.lod_center.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"selected_child"=>v.selected_child,"shape"=>optional_to_python(py,v.shape.as_ref(),shape)?,"payload_references"=>PyTuple::new(py,v.payload_references.iter().map(|v|reference(py,v)).collect::<PyResult<Vec<_>>>()?)?,"source"=>span_to_python(py,&v.source)?})
}
fn attribute<'py>(py: Python<'py>, v: &Attribute) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"Attribute",{
    "id"=>v.id,"object_id"=>v.object_id,"state_flags"=>v.state_flags,"inhibit_flags"=>v.inhibit_flags,"field_final_flags"=>v.field_final_flags,"palette_index"=>v.palette_index,"transform"=>optional_to_python(py,v.transform.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"material_id"=>v.material_id,"source"=>span_to_python(py,&v.source)?})
}
fn material<'py>(py: Python<'py>, v: &Material) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"Material",{
    "id"=>v.id,"object_id"=>v.object_id,"version"=>v.version,"state_flags"=>v.state_flags,"inhibit_flags"=>v.inhibit_flags,"data_flags"=>v.data_flags,"ambient"=>array(py,&v.ambient)?,"diffuse"=>array(py,&v.diffuse)?,"specular"=>array(py,&v.specular)?,"emission"=>array(py,&v.emission)?,"shininess"=>v.shininess,"reflectivity"=>v.reflectivity,"bumpiness"=>v.bumpiness,"field_final_flags"=>v.field_final_flags,"palette_index"=>v.palette_index,"source"=>span_to_python(py,&v.source)?})
}
fn effective<'py>(py: Python<'py>, v: &EffectiveMaterial) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"EffectiveMaterial",{
    "ambient"=>optional_to_python(py,v.ambient.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"diffuse_rgb"=>optional_to_python(py,v.diffuse_rgb.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"diffuse_alpha"=>v.diffuse_alpha,"specular"=>optional_to_python(py,v.specular.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"emission"=>optional_to_python(py,v.emission.as_ref(),|py,a|Ok(array(py,a)?.into_any()))?,"blending"=>v.blending,"override_vertex_colors"=>v.override_vertex_colors,"origins"=>array(py,&v.origins)?,"origin_sources"=>PyTuple::new(py,v.origin_sources.iter().map(|s|s.as_deref()))?})
}
fn atom<'py>(py: Python<'py>, v: &PropertyAtom) -> PyResult<Bound<'py, PyAny>> {
    let (kind, value) = match &v.value {
        PropertyValue::String(s) => ("string", s.as_ref().into_pyobject(py)?.into_any()),
        PropertyValue::Integer(n) => ("integer", n.into_pyobject(py)?.into_any()),
        PropertyValue::Float(n) => ("float", n.into_pyobject(py)?.into_any()),
        PropertyValue::Date(a) => ("date", array(py, a)?.into_any()),
        PropertyValue::Vector(a) => ("vector4", array(py, a)?.into_any()),
        PropertyValue::LateLoaded(v) => ("late_loaded", reference(py, v)?),
        PropertyValue::Opaque => ("opaque", py.None().into_bound(py)),
    };
    dto!(py,"PropertyAtom",{"object_id"=>v.object_id,"type_id"=>PyBytes::new(py,&v.type_id),"state_flags"=>v.state_flags,"kind"=>kind,"value"=>value,"source"=>span_to_python(py,&v.source)?})
}
fn binding<'py>(py: Python<'py>, v: &PropertyBinding) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"PropertyBinding",{"owner_object_id"=>v.owner_object_id,"key_atom_id"=>v.key_atom_id,"value_atom_id"=>v.value_atom_id,"key"=>v.key.as_deref(),"source"=>span_to_python(py,&v.source)?})
}
fn part<'py>(py: Python<'py>, v: &PartDefinition) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"PartDefinition",{"id"=>v.id,"node_id"=>v.node_id,"name"=>v.name.as_deref(),"shape_node_ids"=>PyTuple::new(py,&v.shape_node_ids)?,"source"=>span_to_python(py,&v.source)?})
}
fn instance<'py>(py: Python<'py>, v: &Instance) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"Instance",{
    "id"=>v.id,"node_id"=>v.node_id,"parent_id"=>v.parent_id,"part_id"=>v.part_id,"local_transform"=>array(py,&v.local_transform)?,"world_transform"=>array(py,&v.world_transform)?,"unit_node_id"=>v.unit_node_id,"meters_per_unit"=>v.meters_per_unit,"source_meters_per_unit"=>v.source_meters_per_unit,"material"=>effective(py,&v.material)?,"source"=>span_to_python(py,&v.source)?})
}
fn missing<'py>(py: Python<'py>, v: &MissingReference) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"MissingReference",{"owner_object_id"=>v.owner_object_id,"target"=>&v.target,"kind"=>&v.kind,"source"=>span_to_python(py,&v.source)?})
}
fn opaque<'py>(py: Python<'py>, v: &OpaqueObject) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"OpaqueObject",{"object_id"=>v.object_id,"type_id"=>PyBytes::new(py,&v.type_id),"element_index"=>v.element_index,"source"=>span_to_python(py,&v.source)?})
}
macro_rules! collection {
    ($name:ident,$field:ident,$convert:ident) => {
        pub fn $name<'py>(py: Python<'py>, m: &StructureScene) -> PyResult<Bound<'py, PyTuple>> {
            ensure_python_structure_api(py)?;
            PyTuple::new(
                py,
                m.$field
                    .iter()
                    .map(|v| $convert(py, v))
                    .collect::<PyResult<Vec<_>>>()?,
            )
        }
    };
}
collection!(structure_nodes, nodes, node);
collection!(structure_attributes, attributes, attribute);
collection!(structure_materials, materials, material);
collection!(structure_atoms, atoms, atom);
collection!(structure_bindings, bindings, binding);
collection!(structure_parts, parts, part);
collection!(structure_instances, instances, instance);
collection!(structure_missing, missing_references, missing);
collection!(structure_opaque, opaque_objects, opaque);
collection!(structure_diagnostics, diagnostics, diagnostic_to_python);
pub fn structure_part_instances<'py>(
    py: Python<'py>,
    m: &StructureScene,
) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_structure_api(py)?;
    PyTuple::new(
        py,
        m.instances
            .iter()
            .filter(|v| v.part_id.is_some())
            .map(|v| instance(py, v))
            .collect::<PyResult<Vec<_>>>()?,
    )
}
pub fn structure_source<'py>(py: Python<'py>, m: &StructureScene) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_structure_api(py)?;
    source_to_python(py, &m.source)
}
pub fn structure_sources<'py>(
    py: Python<'py>,
    m: &StructureScene,
) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_structure_api(py)?;
    PyTuple::new(
        py,
        m.sources
            .iter()
            .map(|s| source_to_python(py, s))
            .collect::<PyResult<Vec<_>>>()?,
    )
}
pub fn structure_to_python<'py>(
    py: Python<'py>,
    m: &StructureScene,
) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_structure_api(py)?;
    dto!(py,"StructureScene",{
    "source"=>structure_source(py,m)?,"sources"=>structure_sources(py,m)?,"coordinates_in_meters"=>m.coordinates_in_meters,"blocked_node_ids"=>PyTuple::new(py,&m.blocked_node_ids)?,"lsg_segment_index"=>m.lsg_segment_index,"lsg_guid"=>PyBytes::new(py,&m.lsg_guid),"root_node_id"=>m.root_node_id,
    "nodes"=>structure_nodes(py,m)?,"attributes"=>structure_attributes(py,m)?,"materials"=>structure_materials(py,m)?,"atoms"=>structure_atoms(py,m)?,"bindings"=>structure_bindings(py,m)?,"parts"=>structure_parts(py,m)?,"instances"=>structure_instances(py,m)?,"opaque_objects"=>structure_opaque(py,m)?,"missing_references"=>structure_missing(py,m)?,"diagnostics"=>structure_diagnostics(py,m)?,"complete"=>m.complete})
}
