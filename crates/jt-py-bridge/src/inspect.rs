use super::{construct, diagnostic_to_python, ensure_python_model_api, span_to_python};
use jt_core::{
    container::{CompressionHeader, SegmentInfo},
    ByteOrder, ContainerInfo, ParseError, ParseErrorKind, ReadLimits,
};
use pyo3::{
    exceptions::{PyImportError, PyValueError},
    prelude::*,
    types::{PyBytes, PyDict, PyInt, PyTuple},
};

pub const INSPECT_API_VERSION: u32 = 2;
pyo3::import_exception!(cq_jt.errors, JtParseError);
pyo3::import_exception!(cq_jt.errors, JtLimitError);
pyo3::import_exception!(cq_jt.errors, JtUnsupportedError);
pyo3::import_exception!(cq_jt.errors, JtReferenceError);

pub fn ensure_python_inspect_api(py: Python<'_>) -> PyResult<()> {
    ensure_python_model_api(py)?;
    let compatible = py
        .import("cq_jt.container")
        .and_then(|m| m.getattr("INSPECT_API_VERSION"))
        .is_ok_and(|v| {
            v.get_type().is(py.get_type::<PyInt>())
                && v.extract::<u32>().ok() == Some(INSPECT_API_VERSION)
        });
    if !compatible {
        return Err(PyImportError::new_err(
            "native inspection requires cq_jt.container.INSPECT_API_VERSION == 2",
        ));
    }
    Ok(())
}

pub fn read_limits_from_python(
    py: Python<'_>,
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<ReadLimits> {
    ensure_python_inspect_api(py)?;
    let Some(value) = value else {
        return Ok(ReadLimits::default());
    };
    if !value
        .get_type()
        .is(py.import("cq_jt.container")?.getattr("ReadLimits")?)
    {
        return Err(PyValueError::new_err(
            "limits must be cq_jt.ReadLimits or None",
        ));
    }
    let input = value.getattr("max_input_bytes")?;
    let segments = value.getattr("max_segments")?;
    if !input.get_type().is(py.get_type::<PyInt>())
        || !segments.get_type().is(py.get_type::<PyInt>())
    {
        return Err(PyValueError::new_err(
            "limits must be integers, not bool or float",
        ));
    }
    Ok(ReadLimits {
        max_input_bytes: input
            .extract()
            .map_err(|_| PyValueError::new_err("max_input_bytes must fit u64"))?,
        max_segments: segments
            .extract()
            .map_err(|_| PyValueError::new_err("max_segments must fit u32"))?,
        max_segment_output_bytes: limit_u64(py, value, "max_segment_output_bytes")?,
        max_total_output_bytes: limit_u64(py, value, "max_total_output_bytes")?,
        max_lzma_dictionary_bytes: limit_u64(py, value, "max_lzma_dictionary_bytes")?,
        max_elements: u32::try_from(limit_u64(py, value, "max_elements")?)
            .map_err(|_| PyValueError::new_err("max_elements must fit u32"))?,
        max_xz_blocks: u32::try_from(limit_u64(py, value, "max_xz_blocks")?)
            .map_err(|_| PyValueError::new_err("max_xz_blocks must fit u32"))?,
    })
}

fn limit_u64(py: Python<'_>, value: &Bound<'_, PyAny>, name: &str) -> PyResult<u64> {
    let field = value.getattr(name)?;
    if !field.get_type().is(py.get_type::<PyInt>()) {
        return Err(PyValueError::new_err(format!(
            "{name} must be an integer, not bool or float"
        )));
    }
    field
        .extract()
        .map_err(|_| PyValueError::new_err(format!("{name} must fit u64")))
}

pub fn parse_error_to_python(py: Python<'_>, error: ParseError) -> PyErr {
    let source = match error.source.as_ref() {
        Some(value) => match span_to_python(py, value) {
            Ok(value) => value.unbind(),
            Err(error) => return error,
        },
        None => py.None(),
    };
    match error.kind {
        ParseErrorKind::MissingReference => {
            JtReferenceError::new_err((error.message, error.code, source))
        }
        ParseErrorKind::LimitExceeded => JtLimitError::new_err((error.message, error.code, source)),
        ParseErrorKind::UnsupportedVersion | ParseErrorKind::UnsupportedEncoding => {
            JtUnsupportedError::new_err((error.message, error.code, source))
        }
        _ => JtParseError::new_err((error.message, error.code, source)),
    }
}

fn new<'py>(
    py: Python<'py>,
    name: &str,
    kwargs: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyAny>> {
    py.import("cq_jt.container")?
        .getattr(name)?
        .call((), Some(kwargs))
}
fn compression_to_python<'py>(
    py: Python<'py>,
    value: &CompressionHeader,
) -> PyResult<Bound<'py, PyAny>> {
    let kw = PyDict::new(py);
    kw.set_item("flag", value.flag)?;
    kw.set_item("declared_length", value.declared_length)?;
    kw.set_item("algorithm", value.algorithm)?;
    kw.set_item("label", value.label)?;
    kw.set_item("source", span_to_python(py, &value.source)?)?;
    new(py, "CompressionHeader", &kw)
}
fn segment_to_python<'py>(py: Python<'py>, value: &SegmentInfo) -> PyResult<Bound<'py, PyAny>> {
    let kw = PyDict::new(py);
    kw.set_item("guid_raw", PyBytes::new(py, &value.guid_raw))?;
    kw.set_item("offset", value.offset)?;
    kw.set_item("length", value.length)?;
    kw.set_item("segment_type", value.segment_type)?;
    kw.set_item("type_name", value.type_name)?;
    kw.set_item("attributes", value.attributes)?;
    kw.set_item("toc_source", span_to_python(py, &value.toc_source)?)?;
    kw.set_item("header_source", span_to_python(py, &value.header_source)?)?;
    kw.set_item("payload_source", span_to_python(py, &value.payload_source)?)?;
    let compression = match &value.compression {
        Some(c) => compression_to_python(py, c)?,
        None => py.None().into_bound(py),
    };
    kw.set_item("compression", compression)?;
    new(py, "SegmentInfo", &kw)
}
pub fn container_info_to_python<'py>(
    py: Python<'py>,
    value: &ContainerInfo,
) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_inspect_api(py)?;
    let kw = PyDict::new(py);
    kw.set_item("source_id", &value.source_id)?;
    kw.set_item("size_bytes", value.size_bytes)?;
    kw.set_item(
        "version",
        construct(py, "JtVersion", (value.version.major, value.version.minor))?,
    )?;
    kw.set_item("version_field", PyBytes::new(py, &value.version_field))?;
    kw.set_item("header_comment", &value.header_comment)?;
    let order =
        py.import("cq_jt.container")?
            .getattr("ByteOrder")?
            .call1((match value.byte_order {
                ByteOrder::Little => "little",
                ByteOrder::Big => "big",
            },))?;
    kw.set_item("byte_order", order)?;
    kw.set_item("header_field", value.header_field)?;
    kw.set_item("header_size", value.header_size)?;
    kw.set_item("toc_offset", value.toc_offset)?;
    kw.set_item("toc_entry_width", value.toc_entry_width)?;
    kw.set_item("lsg_guid_raw", PyBytes::new(py, &value.lsg_guid_raw))?;
    let extra = match value.extra_header_guid {
        Some(guid) => PyBytes::new(py, &guid).into_any(),
        None => py.None().into_bound(py),
    };
    kw.set_item("extra_header_guid", extra)?;
    kw.set_item(
        "segments",
        PyTuple::new(
            py,
            value
                .segments
                .iter()
                .map(|v| segment_to_python(py, v))
                .collect::<PyResult<Vec<_>>>()?,
        )?,
    )?;
    kw.set_item(
        "diagnostics",
        PyTuple::new(
            py,
            value
                .diagnostics
                .iter()
                .map(|v| diagnostic_to_python(py, v))
                .collect::<PyResult<Vec<_>>>()?,
        )?,
    )?;
    new(py, "ContainerInfo", &kw)
}
