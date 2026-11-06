//! Shared direct conversions; no Python module is registered here.
use jt_core::{
    Diagnostic, DiagnosticSeverity, JtVersion, ModelError, RawElement, Scene, SourceMetadata,
    SourceSpan,
};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyInt, PyTuple},
};

pub const MODEL_API_VERSION: u32 = 1;
mod inspect;
mod payload;
pub mod structure;
pub use inspect::{
    container_info_to_python, ensure_python_inspect_api, parse_error_to_python,
    read_limits_from_python, INSPECT_API_VERSION,
};
pub use payload::{
    decoded_segments_to_python, document_diagnostics_to_python, ensure_python_payload_api,
    raw_logical_element_to_python, PAYLOAD_API_VERSION,
};
pyo3::import_exception!(cq_jt.errors, JtModelError);

fn invalid(message: impl ToString) -> PyErr {
    JtModelError::new_err((message.to_string(), "invalid_model"))
}

fn core_error(error: ModelError) -> PyErr {
    JtModelError::new_err((error.message, error.code))
}

pub fn ensure_python_model_api(py: Python<'_>) -> PyResult<()> {
    let compatible = py
        .import("cq_jt.model")
        .and_then(|module| module.getattr("MODEL_API_VERSION"))
        .is_ok_and(|version| {
            version.get_type().is(py.get_type::<PyInt>())
                && version.extract::<u32>().ok() == Some(MODEL_API_VERSION)
        });
    if !compatible {
        return Err(pyo3::exceptions::PyImportError::new_err(
            "jt-py-bridge requires cq_jt.model.MODEL_API_VERSION == 1; install a matching cq-jt wheel",
        ));
    }
    Ok(())
}

fn require_class(value: &Bound<'_, PyAny>, name: &str) -> PyResult<()> {
    let class = value.py().import("cq_jt.model")?.getattr(name)?;
    if !value.get_type().is(&class) {
        return Err(invalid(format!("expected cq_jt.model.{name}")));
    }
    Ok(())
}

fn integer(value: &Bound<'_, PyAny>) -> PyResult<()> {
    if !value.get_type().is(value.py().get_type::<PyInt>()) {
        return Err(invalid(
            "identifier or offset must be an integer, not bool or float",
        ));
    }
    Ok(())
}

fn u64_value(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    integer(value)?;
    value
        .extract()
        .map_err(|_| invalid("offset must fit an unsigned 64-bit integer"))
}

fn optional<T>(
    value: &Bound<'_, PyAny>,
    read: impl FnOnce(&Bound<'_, PyAny>) -> PyResult<T>,
) -> PyResult<Option<T>> {
    if value.is_none() {
        Ok(None)
    } else {
        read(value).map(Some)
    }
}

fn version_from_python(value: &Bound<'_, PyAny>) -> PyResult<JtVersion> {
    require_class(value, "JtVersion")?;
    let major = value.getattr("major")?;
    let minor = value.getattr("minor")?;
    integer(&major)?;
    integer(&minor)?;
    Ok(JtVersion {
        major: major.extract()?,
        minor: minor.extract()?,
    })
}

fn source_from_python(value: &Bound<'_, PyAny>) -> PyResult<SourceMetadata> {
    require_class(value, "SourceMetadata")?;
    let units = value.getattr("meters_per_unit")?;
    if !units.is_none()
        && !units
            .get_type()
            .is(units.py().get_type::<pyo3::types::PyFloat>())
        && !units.get_type().is(units.py().get_type::<PyInt>())
    {
        return Err(invalid(
            "meters_per_unit must be a built-in float, integer or None",
        ));
    }
    Ok(SourceMetadata {
        source_id: value.getattr("source_id")?.extract()?,
        label: value.getattr("label")?.extract()?,
        version: optional(&value.getattr("version")?, version_from_python)?,
        meters_per_unit: units.extract()?,
    })
}

fn span_from_python(value: &Bound<'_, PyAny>) -> PyResult<SourceSpan> {
    require_class(value, "SourceSpan")?;
    Ok(SourceSpan {
        source_id: value.getattr("source_id")?.extract()?,
        byte_domain: value.getattr("byte_domain")?.extract()?,
        start_offset: u64_value(&value.getattr("start_offset")?)?,
        end_offset: u64_value(&value.getattr("end_offset")?)?,
    })
}

fn raw_from_python(value: &Bound<'_, PyAny>) -> PyResult<RawElement> {
    require_class(value, "RawElement")?;
    let type_id = value.getattr("type_id")?;
    let type_id = type_id
        .cast::<PyBytes>()?
        .as_bytes()
        .try_into()
        .map_err(|_| invalid("type_id must contain exactly 16 bytes"))?;
    Ok(RawElement {
        type_id,
        object_id: optional(&value.getattr("object_id")?, |v| {
            integer(v)?;
            v.extract()
        })?,
        raw_data: value
            .getattr("raw_data")?
            .cast::<PyBytes>()?
            .as_bytes()
            .to_vec(),
        source: optional(&value.getattr("source")?, span_from_python)?,
    })
}

fn diagnostic_from_python(value: &Bound<'_, PyAny>) -> PyResult<Diagnostic> {
    require_class(value, "Diagnostic")?;
    let severity = value.getattr("severity")?;
    require_class(&severity, "DiagnosticSeverity")?;
    let severity = match severity.getattr("value")?.extract::<String>()?.as_str() {
        "info" => DiagnosticSeverity::Info,
        "warning" => DiagnosticSeverity::Warning,
        "error" => DiagnosticSeverity::Error,
        _ => return Err(invalid("unknown diagnostic severity")),
    };
    Ok(Diagnostic {
        code: value.getattr("code")?.extract()?,
        message: value.getattr("message")?.extract()?,
        severity,
        source: optional(&value.getattr("source")?, span_from_python)?,
    })
}

/// Copy typed fields into Rust, then validate without holding the Python interpreter.
pub fn scene_from_python(value: &Bound<'_, PyAny>) -> PyResult<Scene> {
    let py = value.py();
    ensure_python_model_api(py)?;
    let result = (|| {
        require_class(value, "Scene")?;
        let sources = value
            .getattr("sources")?
            .try_iter()?
            .map(|v| source_from_python(&v?))
            .collect::<PyResult<Vec<_>>>()?;
        let raw = value
            .getattr("raw_elements")?
            .try_iter()?
            .map(|v| raw_from_python(&v?))
            .collect::<PyResult<Vec<_>>>()?;
        let diagnostics = value
            .getattr("diagnostics")?
            .try_iter()?
            .map(|v| diagnostic_from_python(&v?))
            .collect::<PyResult<Vec<_>>>()?;
        py.detach(move || Scene::new(sources, raw, diagnostics))
            .map_err(core_error)
    })();
    result.map_err(|error: PyErr| {
        if error.is_instance_of::<JtModelError>(py) {
            error
        } else {
            invalid(error)
        }
    })
}

fn construct<'py>(
    py: Python<'py>,
    name: &str,
    args: impl pyo3::call::PyCallArgs<'py>,
) -> PyResult<Bound<'py, PyAny>> {
    py.import("cq_jt.model")?.getattr(name)?.call1(args)
}

fn optional_to_python<'py, T>(
    py: Python<'py>,
    value: Option<&T>,
    write: impl FnOnce(Python<'py>, &T) -> PyResult<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    match value {
        Some(value) => write(py, value),
        None => Ok(py.None().into_bound(py)),
    }
}

fn span_to_python<'py>(py: Python<'py>, value: &SourceSpan) -> PyResult<Bound<'py, PyAny>> {
    construct(
        py,
        "SourceSpan",
        (
            &value.source_id,
            &value.byte_domain,
            value.start_offset,
            value.end_offset,
        ),
    )
}

fn source_to_python<'py>(py: Python<'py>, value: &SourceMetadata) -> PyResult<Bound<'py, PyAny>> {
    let version = optional_to_python(py, value.version.as_ref(), |py, v| {
        construct(py, "JtVersion", (v.major, v.minor))
    })?;
    construct(
        py,
        "SourceMetadata",
        (
            &value.source_id,
            &value.label,
            version,
            value.meters_per_unit,
        ),
    )
}

fn raw_to_python<'py>(py: Python<'py>, value: &RawElement) -> PyResult<Bound<'py, PyAny>> {
    let source = optional_to_python(py, value.source.as_ref(), span_to_python)?;
    construct(
        py,
        "RawElement",
        (
            PyBytes::new(py, &value.type_id),
            value.object_id,
            PyBytes::new(py, &value.raw_data),
            source,
        ),
    )
}

fn diagnostic_to_python<'py>(py: Python<'py>, value: &Diagnostic) -> PyResult<Bound<'py, PyAny>> {
    let severity = py
        .import("cq_jt.model")?
        .getattr("DiagnosticSeverity")?
        .call1((match value.severity {
            DiagnosticSeverity::Info => "info",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Error => "error",
        },))?;
    let source = optional_to_python(py, value.source.as_ref(), span_to_python)?;
    construct(
        py,
        "Diagnostic",
        (&value.code, &value.message, severity, source),
    )
}

pub fn sources_to_python<'py>(py: Python<'py>, scene: &Scene) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_model_api(py)?;
    PyTuple::new(
        py,
        scene
            .sources()
            .iter()
            .map(|v| source_to_python(py, v))
            .collect::<PyResult<Vec<_>>>()?,
    )
}

pub fn raw_elements_to_python<'py>(
    py: Python<'py>,
    scene: &Scene,
) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_model_api(py)?;
    PyTuple::new(
        py,
        scene
            .raw_elements()
            .iter()
            .map(|v| raw_to_python(py, v))
            .collect::<PyResult<Vec<_>>>()?,
    )
}

pub fn diagnostics_to_python<'py>(py: Python<'py>, scene: &Scene) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_model_api(py)?;
    PyTuple::new(
        py,
        scene
            .diagnostics()
            .iter()
            .map(|v| diagnostic_to_python(py, v))
            .collect::<PyResult<Vec<_>>>()?,
    )
}

pub fn scene_to_python<'py>(py: Python<'py>, scene: &Scene) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_model_api(py)?;
    construct(
        py,
        "Scene",
        (
            sources_to_python(py, scene)?,
            raw_elements_to_python(py, scene)?,
            diagnostics_to_python(py, scene)?,
        ),
    )
}
#[cfg(feature = "glb")]
pub mod export;
pub mod mesh;
