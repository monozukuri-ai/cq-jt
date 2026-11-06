use super::{construct, diagnostic_to_python, ensure_python_inspect_api, span_to_python};
use jt_core::{DecodedDocument, DecodedSegment, LogicalElement};
use pyo3::{
    exceptions::{PyImportError, PyIndexError},
    prelude::*,
    types::{PyBytes, PyDict, PyInt, PyTuple},
};

pub const PAYLOAD_API_VERSION: u32 = 1;

pub fn ensure_python_payload_api(py: Python<'_>) -> PyResult<()> {
    ensure_python_inspect_api(py)?;
    let compatible = py
        .import("cq_jt.payload")
        .and_then(|m| m.getattr("PAYLOAD_API_VERSION"))
        .is_ok_and(|v| {
            v.get_type().is(py.get_type::<PyInt>())
                && v.extract::<u32>().ok() == Some(PAYLOAD_API_VERSION)
        });
    if compatible {
        Ok(())
    } else {
        Err(PyImportError::new_err(
            "native decoding requires cq_jt.payload.PAYLOAD_API_VERSION == 1",
        ))
    }
}
fn new<'py>(
    py: Python<'py>,
    name: &str,
    kwargs: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyAny>> {
    py.import("cq_jt.payload")?
        .getattr(name)?
        .call((), Some(kwargs))
}
fn element_to_python<'py>(py: Python<'py>, e: &LogicalElement) -> PyResult<Bound<'py, PyAny>> {
    let kw = PyDict::new(py);
    kw.set_item("declared_length", e.declared_length)?;
    kw.set_item("type_id", PyBytes::new(py, &e.type_id))?;
    kw.set_item("base_type", e.base_type)?;
    kw.set_item("object_id", e.object_id)?;
    kw.set_item("list_index", e.list_index)?;
    kw.set_item("is_end_marker", e.is_end_marker)?;
    kw.set_item("source", span_to_python(py, &e.source)?)?;
    kw.set_item("data_source", span_to_python(py, &e.data_source)?)?;
    new(py, "LogicalElement", &kw)
}
fn segment_to_python<'py>(py: Python<'py>, s: &DecodedSegment) -> PyResult<Bound<'py, PyAny>> {
    let kw = PyDict::new(py);
    kw.set_item("segment_index", s.segment_index)?;
    kw.set_item("guid_raw", PyBytes::new(py, &s.guid_raw))?;
    kw.set_item("segment_type", s.segment_type)?;
    kw.set_item("codec", s.codec)?;
    kw.set_item("encoded_source", span_to_python(py, &s.encoded_source)?)?;
    kw.set_item("decoded_source", span_to_python(py, &s.decoded_source)?)?;
    kw.set_item("tail_source", span_to_python(py, &s.tail_source)?)?;
    kw.set_item("elements_framed", s.elements_framed)?;
    kw.set_item(
        "elements",
        PyTuple::new(
            py,
            s.elements
                .iter()
                .map(|e| element_to_python(py, e))
                .collect::<PyResult<Vec<_>>>()?,
        )?,
    )?;
    new(py, "DecodedSegmentInfo", &kw)
}
pub fn decoded_segments_to_python<'py>(
    py: Python<'py>,
    document: &DecodedDocument,
) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_payload_api(py)?;
    PyTuple::new(
        py,
        document
            .segments
            .iter()
            .map(|s| segment_to_python(py, s))
            .collect::<PyResult<Vec<_>>>()?,
    )
}
pub fn document_diagnostics_to_python<'py>(
    py: Python<'py>,
    document: &DecodedDocument,
) -> PyResult<Bound<'py, PyTuple>> {
    ensure_python_payload_api(py)?;
    PyTuple::new(
        py,
        document
            .diagnostics
            .iter()
            .map(|d| diagnostic_to_python(py, d))
            .collect::<PyResult<Vec<_>>>()?,
    )
}
pub fn raw_logical_element_to_python<'py>(
    py: Python<'py>,
    segment: &DecodedSegment,
    index: usize,
) -> PyResult<Bound<'py, PyAny>> {
    ensure_python_payload_api(py)?;
    let e = segment
        .elements
        .get(index)
        .ok_or_else(|| PyIndexError::new_err("element index out of range"))?;
    // Copy straight into Python bytes, without a temporary Rust RawElement copy.
    construct(
        py,
        "RawElement",
        (
            PyBytes::new(py, &e.type_id),
            e.object_id,
            PyBytes::new(
                py,
                segment
                    .element_bytes(index)
                    .ok_or_else(|| PyIndexError::new_err("element range is unavailable"))?,
            ),
            span_to_python(py, &e.data_source)?,
        ),
    )
}
