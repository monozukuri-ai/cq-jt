use super::{
    diagnostic_to_python, span_to_python,
    structure::{ensure_python_structure_api, structure_options_from_python, structure_to_python},
};
use jt_core::mesh::*;
use pyo3::{
    exceptions::{PyImportError, PyValueError},
    prelude::*,
    types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyTuple},
};
pub const MESH_API_VERSION: u32 = 2;
pub fn ensure_python_mesh_api(py: Python<'_>) -> PyResult<()> {
    ensure_python_structure_api(py)?;
    let version = py.import("cq_jt.mesh")?.getattr("MESH_API_VERSION")?;
    if version.get_type().is(py.get_type::<PyInt>())
        && version.extract::<u32>().ok() == Some(MESH_API_VERSION)
    {
        Ok(())
    } else {
        Err(PyImportError::new_err(
            "native mesh reading requires cq_jt.mesh.MESH_API_VERSION == 2",
        ))
    }
}
pub fn read_options_from_python(
    py: Python<'_>,
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<ReadOptions> {
    ensure_python_mesh_api(py)?;
    let Some(v) = value else {
        return Ok(ReadOptions::default());
    };
    let module = py.import("cq_jt.mesh")?;
    if !v.get_type().is(module.getattr("ReadOptions")?) {
        return Err(PyValueError::new_err(
            "options must be cq_jt.ReadOptions or None",
        ));
    }
    let lod = v.getattr("lod")?;
    let lod = if lod.get_type().is(py.get_type::<PyInt>()) {
        LodSelection::Index(
            lod.extract()
                .map_err(|_| PyValueError::new_err("LOD index must fit u32"))?,
        )
    } else {
        match lod.extract::<String>()?.as_str() {
            "highest" => LodSelection::Highest,
            "lowest" => LodSelection::Lowest,
            _ => {
                return Err(PyValueError::new_err(
                    "LOD must be highest, lowest or a nonnegative integer",
                ))
            }
        }
    };
    let structure = structure_options_from_python(py, Some(&v.getattr("structure")?))?;
    let m = v.getattr("mesh")?;
    if !m.get_type().is(module.getattr("MeshOptions")?) {
        return Err(PyValueError::new_err("mesh must be cq_jt.MeshOptions"));
    }
    let number = |name: &str| -> PyResult<u64> {
        let v = m.getattr(name)?;
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
    let mesh = MeshOptions {
        max_meshes: count("max_meshes")?,
        max_vertices: number("max_vertices")?,
        max_triangles: number("max_triangles")?,
        max_mesh_bytes: number("max_mesh_bytes")?,
        max_codec_values: number("max_codec_values")?,
        max_codec_packets: number("max_codec_packets")?,
        max_codec_depth: count("max_codec_depth")?,
        max_topology_edges: number("max_topology_edges")?,
        max_topology_work: number("max_topology_work")?,
    };
    if mesh.max_codec_depth > 64 {
        return Err(PyValueError::new_err("max_codec_depth must not exceed 64"));
    }
    let r = v.getattr("resolver")?;
    if !r.get_type().is(module.getattr("ResolverOptions")?) {
        return Err(PyValueError::new_err(
            "resolver must be cq_jt.ResolverOptions",
        ));
    }
    let enabled = r.getattr("enabled")?;
    if !enabled.get_type().is(py.get_type::<PyBool>()) {
        return Err(PyValueError::new_err("resolver.enabled must be bool"));
    }
    let number = |name: &str| -> PyResult<u64> {
        let value = r.getattr(name)?;
        if !value.get_type().is(py.get_type::<PyInt>()) {
            return Err(PyValueError::new_err(format!(
                "resolver.{name} must be an integer, not bool/float"
            )));
        }
        value
            .extract()
            .map_err(|_| PyValueError::new_err(format!("resolver.{name} must fit u64")))
    };
    let count = |name: &str| -> PyResult<u32> {
        u32::try_from(number(name)?)
            .map_err(|_| PyValueError::new_err(format!("resolver.{name} must fit u32")))
    };
    let units = r.getattr("meters_per_unit")?;
    let units = if units.is_none() {
        None
    } else {
        if !units.get_type().is(py.get_type::<PyFloat>())
            && !units.get_type().is(py.get_type::<PyInt>())
        {
            return Err(PyValueError::new_err(
                "resolver units must be a number, not bool",
            ));
        }
        let u = units.extract::<f64>()?;
        if !u.is_finite() || u <= 0. {
            return Err(PyValueError::new_err(
                "resolver units must be finite and positive",
            ));
        }
        Some(u)
    };
    let resolver = resolver::ResolverOptions {
        enabled: enabled.extract()?,
        root: r.getattr("root")?.extract::<Option<std::path::PathBuf>>()?,
        max_files: count("max_files")?,
        max_depth: count("max_depth")?,
        max_references: count("max_references")?,
        max_total_input_bytes: number("max_total_input_bytes")?,
        meters_per_unit: units,
    };
    if resolver.max_depth > 64 {
        return Err(PyValueError::new_err(
            "resolver.max_depth must not exceed 64",
        ));
    }
    Ok(ReadOptions {
        structure,
        mesh,
        lod,
        resolver,
    })
}
fn new<'py>(py: Python<'py>, name: &str, kw: &Bound<'py, PyDict>) -> PyResult<Bound<'py, PyAny>> {
    py.import("cq_jt.mesh")?.getattr(name)?.call((), Some(kw))
}
macro_rules! dto{($py:expr,$name:literal,{$($key:literal=>$value:expr),*$(,)?})=>{{let kw=PyDict::new($py);$(kw.set_item($key,$value)?;)*new($py,$name,&kw)}};}
pub fn meshes_to_python<'py>(py: Python<'py>, s: &MeshScene) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_mesh_api(py)?;
    PyTuple::new(py,s.meshes.iter().map(|m|dto!(py,"MeshInfo",{
        "id"=>m.id,"segment_index"=>m.segment_index,"element_index"=>m.element_index,"vertex_records_object_id"=>m.vertex_records_object_id,"source"=>span_to_python(py,&m.source)?,"vertex_count"=>m.positions.len(),"triangle_count"=>m.triangles.len(),"normal_count"=>m.normals.len(),"vertex_flag_count"=>m.vertex_flags.len(),
        "bounds"=>m.bounds.map(|a|PyTuple::new(py,a)).transpose()?,"quantization"=>PyTuple::new(py,m.quantization)?,"quantization_step"=>PyTuple::new(py,m.quantization_step)?,"topology_hash"=>m.topology_hash,"coordinate_hash"=>m.coordinate_hash,"normal_hash"=>m.normal_hash,"cover_faces"=>m.cover_faces,"codec_depth"=>m.codec_depth
    })).collect::<PyResult<Vec<_>>>()?)
}
pub fn instances_to_python<'py>(py: Python<'py>, s: &MeshScene) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_mesh_api(py)?;
    PyTuple::new(py,s.instances.iter().map(|i|dto!(py,"MeshInstance",{"id"=>i.id,"structure_instance_id"=>i.structure_instance_id,"mesh_id"=>i.mesh_id})).collect::<PyResult<Vec<_>>>()?)
}
pub fn choices_to_python<'py>(py: Python<'py>, s: &MeshScene) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_mesh_api(py)?;
    PyTuple::new(py,s.lod_choices.iter().map(|i|dto!(py,"LodChoice",{"node_id"=>i.node_id,"child_index"=>i.child_index,"child_node_id"=>i.child_node_id})).collect::<PyResult<Vec<_>>>()?)
}
pub fn mesh_diagnostics_to_python<'py>(
    py: Python<'py>,
    s: &MeshScene,
) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_mesh_api(py)?;
    PyTuple::new(
        py,
        s.diagnostics
            .iter()
            .map(|d| diagnostic_to_python(py, d))
            .collect::<PyResult<Vec<_>>>()?,
    )
}
pub fn mesh_scene_to_python<'py>(py: Python<'py>, s: &MeshScene) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_mesh_api(py)?;
    dto!(py,"MeshScene",{"structure"=>structure_to_python(py,&s.structure.model)?,"meshes"=>meshes_to_python(py,s)?,"instances"=>instances_to_python(py,s)?,"lod_choices"=>choices_to_python(py,s)?,"diagnostics"=>mesh_diagnostics_to_python(py,s)?,"complete"=>s.complete})
}
fn packed<'py>(
    py: Python<'py>,
    data: &[u8],
    dtype: &str,
    n: usize,
    components: usize,
) -> PyResult<Bound<'py, PyAny>> {
    dto!(py,"PackedArray",{"data"=>PyBytes::new(py,data),"dtype"=>dtype,"shape"=>PyTuple::new(py,[n,components])?})
}
fn f64s<'py>(py: Python<'py>, v: &[[f64; 3]]) -> PyResult<Bound<'py, PyAny>> {
    let bytes: Vec<_> = v.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    packed(py, &bytes, "f64", v.len(), 3)
}
fn u32s<'py>(py: Python<'py>, v: &[[u32; 3]]) -> PyResult<Bound<'py, PyAny>> {
    let bytes: Vec<_> = v.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    packed(py, &bytes, "u32", v.len(), 3)
}
pub fn mesh_buffers_to_python<'py>(py: Python<'py>, m: &Mesh) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_mesh_api(py)?;
    let groups: Vec<_> = m.face_groups.iter().flat_map(|v| v.to_le_bytes()).collect();
    let flags: Vec<_> = m
        .vertex_flags
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    dto!(py,"MeshBuffers",{"positions"=>f64s(py,&m.positions)?,"triangles"=>u32s(py,&m.triangles)?,"normals"=>f64s(py,&m.normals)?,"normal_indices"=>m.normal_indices.as_ref().map(|v|u32s(py,v)).transpose()?,"face_groups"=>packed(py,&groups,"i32",m.face_groups.len(),1)?,"vertex_flags"=>packed(py,&flags,"u32",m.vertex_flags.len(),1)?})
}
