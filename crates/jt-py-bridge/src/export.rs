use super::{diagnostic_to_python, mesh::ensure_python_mesh_api, span_to_python};
use jt_core::export::*;
use pyo3::{
    exceptions::{PyImportError, PyValueError},
    prelude::*,
    types::{PyBool, PyDict, PyFloat, PyInt, PyTuple},
};
pub const EXPORT_API_VERSION: u32 = 1;
pyo3::import_exception!(cq_jt.errors, JtExportError);
pub fn ensure_python_export_api(py: Python<'_>) -> PyResult<()> {
    ensure_python_mesh_api(py)?;
    let v = py.import("cq_jt.export")?.getattr("EXPORT_API_VERSION")?;
    if v.get_type().is(py.get_type::<PyInt>())
        && v.extract::<u32>().ok() == Some(EXPORT_API_VERSION)
    {
        Ok(())
    } else {
        Err(PyImportError::new_err(
            "native GLB export requires cq_jt.export.EXPORT_API_VERSION == 1",
        ))
    }
}
pub fn export_options_from_python(
    py: Python<'_>,
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<ExportOptions> {
    ensure_python_export_api(py)?;
    let Some(v) = value else {
        return Ok(ExportOptions::default());
    };
    if !v
        .get_type()
        .is(py.import("cq_jt.export")?.getattr("ExportOptions")?)
    {
        return Err(PyValueError::new_err(
            "options must be cq_jt.ExportOptions or None",
        ));
    }
    let number = |name: &str| -> PyResult<u64> {
        let field = v.getattr(name)?;
        if !field.get_type().is(py.get_type::<PyInt>()) {
            return Err(PyValueError::new_err(format!(
                "{name} must be an integer, not bool or float"
            )));
        }
        field
            .extract()
            .map_err(|_| PyValueError::new_err(format!("{name} must fit u64")))
    };
    let count = |name: &str| -> PyResult<u32> {
        u32::try_from(number(name)?)
            .map_err(|_| PyValueError::new_err(format!("{name} must fit u32")))
    };
    let float = |field: &Bound<'_, PyAny>| -> PyResult<f64> {
        if !field.get_type().is(py.get_type::<PyFloat>())
            && !field.get_type().is(py.get_type::<PyInt>())
        {
            return Err(PyValueError::new_err(
                "export numeric values must be int or float, not bool",
            ));
        }
        field.extract()
    };
    let partial = v.getattr("allow_partial")?;
    if !partial.get_type().is(py.get_type::<PyBool>()) {
        return Err(PyValueError::new_err("allow_partial must be bool"));
    }
    let units = v.getattr("meters_per_unit")?;
    let units = if units.is_none() {
        None
    } else {
        Some(float(&units)?)
    };
    let color = v.getattr("default_color")?;
    let color = if color.is_none() {
        None
    } else {
        let c = color
            .cast::<PyTuple>()
            .map_err(|_| PyValueError::new_err("default_color must be a four-component tuple"))?;
        if c.len() != 4 {
            return Err(PyValueError::new_err(
                "default_color must have four components",
            ));
        }
        let mut out = [0.; 4];
        for (i, f) in c.iter().enumerate() {
            out[i] = float(&f)?;
        }
        Some(out)
    };
    let space = match v.getattr("color_space")?.extract::<String>()?.as_str() {
        "linear" => ColorSpace::Linear,
        "srgb" => ColorSpace::Srgb,
        _ => return Err(PyValueError::new_err("color_space must be linear or srgb")),
    };
    let axis = match v.getattr("up_axis")?.extract::<String>()?.as_str() {
        "preserve" => UpAxis::Preserve,
        "y" => UpAxis::Y,
        "z" => UpAxis::Z,
        _ => return Err(PyValueError::new_err("up_axis must be preserve, y or z")),
    };
    Ok(ExportOptions {
        allow_partial: partial.extract()?,
        meters_per_unit: units,
        default_color: color,
        color_space: space,
        up_axis: axis,
        max_position_error_meters: float(&v.getattr("max_position_error_meters")?)?,
        max_output_bytes: number("max_output_bytes")?,
        max_json_bytes: number("max_json_bytes")?,
        max_vertices: number("max_vertices")?,
        max_nodes: count("max_nodes")?,
        max_materials: count("max_materials")?,
        max_mesh_variants: count("max_mesh_variants")?,
    })
}
pub fn export_error_to_python(py: Python<'_>, e: ExportError) -> PyErr {
    let source = match e.source {
        Some(s) => match span_to_python(py, &s) {
            Ok(v) => v.unbind(),
            Err(e) => return e,
        },
        None => py.None(),
    };
    JtExportError::new_err((e.message, e.code, source))
}
pub fn export_report_to_python<'py>(
    py: Python<'py>,
    v: &ExportReport,
) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_export_api(py)?;
    let kw = PyDict::new(py);
    macro_rules! field {
        ($name:ident) => {
            kw.set_item(stringify!($name), v.$name)?;
        };
    }
    field!(source_complete);
    field!(byte_length);
    field!(node_count);
    field!(geometry_count);
    field!(mesh_count);
    field!(material_count);
    field!(instance_count);
    field!(vertex_count);
    field!(triangle_count);
    field!(placed_triangle_count);
    field!(meters_per_unit);
    field!(unit_fallback_used);
    field!(max_position_error_meters);
    field!(normalized_normal_count);
    field!(original_degenerate_triangles);
    kw.set_item("color_space", v.color_space.name())?;
    kw.set_item("up_axis", v.up_axis.name())?;
    kw.set_item(
        "omitted_empty_meshes",
        PyTuple::new(py, &v.omitted_empty_meshes)?,
    )?;
    kw.set_item(
        "diagnostics",
        PyTuple::new(
            py,
            v.diagnostics
                .iter()
                .map(|d| diagnostic_to_python(py, d))
                .collect::<PyResult<Vec<_>>>()?,
        )?,
    )?;
    py.import("cq_jt.export")?
        .getattr("ExportReport")?
        .call((), Some(&kw))
}
