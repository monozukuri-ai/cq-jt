//! Python module registration and immutable Rust-owned snapshot.
use jt_core::{InspectError, ParseError, ParseErrorKind, ReadLimits, Scene, SourceSpan};
use jt_py_bridge::{
    container_info_to_python, decoded_segments_to_python, diagnostics_to_python,
    document_diagnostics_to_python, ensure_python_payload_api, parse_error_to_python,
    raw_elements_to_python, raw_logical_element_to_python, read_limits_from_python,
    scene_from_python, scene_to_python, sources_to_python,
};
use pyo3::{
    exceptions::{PyIndexError, PyTypeError},
    prelude::*,
    types::{PyBytes, PyInt, PyTuple},
};
use std::{path::PathBuf, sync::Arc};
mod export;
mod mesh;
mod structure;

#[pyclass(frozen, module = "cq_jt._native")]
pub struct NativeScene {
    inner: Arc<Scene>,
}

impl NativeScene {
    pub fn from_scene(scene: Scene) -> Self {
        Self {
            inner: Arc::new(scene),
        }
    }
    pub fn scene(&self) -> &Scene {
        &self.inner
    }
}

#[pymethods]
impl NativeScene {
    #[staticmethod]
    fn from_model(model: &Bound<'_, PyAny>) -> PyResult<Self> {
        scene_from_python(model).map(Self::from_scene)
    }

    fn to_model<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        scene_to_python(py, &self.inner)
    }

    #[getter]
    fn sources<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        sources_to_python(py, &self.inner)
    }

    #[getter]
    fn raw_elements<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        raw_elements_to_python(py, &self.inner)
    }

    #[getter]
    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        diagnostics_to_python(py, &self.inner)
    }

    fn __repr__(&self) -> String {
        format!(
            "NativeScene(sources={}, raw_elements={}, diagnostics={})",
            self.inner.sources().len(),
            self.inner.raw_elements().len(),
            self.inner.diagnostics().len()
        )
    }
}

#[pyfunction]
fn validate_scene(model: &Bound<'_, PyAny>) -> PyResult<()> {
    scene_from_python(model).map(|_| ())
}

#[pyclass(frozen, module = "cq_jt._native")]
pub struct NativeDocument {
    inner: Arc<jt_core::DecodedDocument>,
}
fn index(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    if !value.get_type().is(value.py().get_type::<PyInt>()) {
        return Err(PyTypeError::new_err(
            "index must be an integer, not bool or float",
        ));
    }
    value
        .extract()
        .map_err(|_| PyIndexError::new_err("index must be nonnegative and fit usize"))
}
#[pymethods]
impl NativeDocument {
    #[getter]
    fn container<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        ensure_python_payload_api(py)?;
        container_info_to_python(py, &self.inner.container)
    }
    #[getter]
    fn segments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        decoded_segments_to_python(py, &self.inner)
    }
    #[getter]
    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        document_diagnostics_to_python(py, &self.inner)
    }
    #[getter]
    fn total_output_bytes(&self) -> u64 {
        self.inner.total_output_bytes
    }
    #[getter]
    fn geometry_decoded(&self) -> bool {
        false
    }
    fn segment_bytes<'py>(
        &self,
        segment_index: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let py = segment_index.py();
        ensure_python_payload_api(py)?;
        let segment = self
            .inner
            .segments
            .get(index(segment_index)?)
            .ok_or_else(|| PyIndexError::new_err("segment index out of range"))?;
        Ok(PyBytes::new(py, segment.bytes()))
    }
    fn element_bytes<'py>(
        &self,
        segment_index: &Bound<'py, PyAny>,
        element_index: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let py = segment_index.py();
        ensure_python_payload_api(py)?;
        let segment = self
            .inner
            .segments
            .get(index(segment_index)?)
            .ok_or_else(|| PyIndexError::new_err("segment index out of range"))?;
        let bytes = segment
            .element_bytes(index(element_index)?)
            .ok_or_else(|| PyIndexError::new_err("element index out of range"))?;
        Ok(PyBytes::new(py, bytes))
    }
    fn raw_element<'py>(
        &self,
        segment_index: &Bound<'py, PyAny>,
        element_index: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let segment = self
            .inner
            .segments
            .get(index(segment_index)?)
            .ok_or_else(|| PyIndexError::new_err("segment index out of range"))?;
        raw_logical_element_to_python(segment_index.py(), segment, index(element_index)?)
    }
    fn __repr__(&self) -> String {
        format!(
            "NativeDocument(segments={}, output_bytes={}, geometry_decoded=False)",
            self.inner.segments.len(),
            self.inner.total_output_bytes
        )
    }
}

#[pyfunction]
#[pyo3(signature = (data, *, source_id="input", limits=None))]
fn inspect_bytes<'py>(
    data: &Bound<'py, PyBytes>,
    source_id: &str,
    limits: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = data.py();
    let limits = read_limits_from_python(py, limits)?;
    limits
        .check_input_size(data.as_bytes().len() as u64, source_id)
        .map_err(|e| parse_error_to_python(py, e))?;
    let bytes = owned_input(py, data.as_bytes(), source_id)?;
    let source_id = source_id.to_owned();
    let result = py
        .detach(move || jt_core::inspect_bytes(&bytes, &source_id, &limits))
        .map_err(|e| parse_error_to_python(py, e))?;
    container_info_to_python(py, &result)
}

fn owned_input(py: Python<'_>, data: &[u8], source_id: &str) -> PyResult<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(data.len()).map_err(|_| {
        parse_error_to_python(
            py,
            ParseError {
                kind: ParseErrorKind::LimitExceeded,
                code: "allocation_failed",
                message: "cannot allocate owned input buffer".into(),
                source: Some(SourceSpan {
                    source_id: source_id.into(),
                    byte_domain: "file".into(),
                    start_offset: 0,
                    end_offset: 0,
                }),
            },
        )
    })?;
    bytes.extend_from_slice(data);
    Ok(bytes)
}

#[pyfunction]
#[pyo3(signature = (data, *, source_id="input", limits=None))]
fn decode_bytes(
    data: &Bound<'_, PyBytes>,
    source_id: &str,
    limits: Option<&Bound<'_, PyAny>>,
) -> PyResult<NativeDocument> {
    let py = data.py();
    ensure_python_payload_api(py)?;
    let limits = read_limits_from_python(py, limits)?;
    limits
        .check_input_size(data.as_bytes().len() as u64, source_id)
        .map_err(|e| parse_error_to_python(py, e))?;
    let bytes = owned_input(py, data.as_bytes(), source_id)?;
    let source_id = source_id.to_owned();
    let result = py
        .detach(move || jt_core::decode_bytes(&bytes, &source_id, &limits))
        .map_err(|e| parse_error_to_python(py, e))?;
    Ok(NativeDocument {
        inner: Arc::new(result),
    })
}

#[pyfunction]
#[pyo3(signature = (path, *, source_id=None, limits=None))]
fn decode_file(
    py: Python<'_>,
    path: PathBuf,
    source_id: Option<String>,
    limits: Option<&Bound<'_, PyAny>>,
) -> PyResult<NativeDocument> {
    ensure_python_payload_api(py)?;
    let limits = read_limits_from_python(py, limits)?;
    let source_id = source_id.unwrap_or_else(|| path.to_string_lossy().into_owned());
    let result = py
        .detach(move || jt_core::decode_file(&path, &source_id, &limits))
        .map_err(|e| match e {
            InspectError::Io(e) => PyErr::from(e),
            InspectError::Parse(e) => parse_error_to_python(py, e),
        })?;
    Ok(NativeDocument {
        inner: Arc::new(result),
    })
}

#[pyfunction]
#[pyo3(signature = (path, *, source_id=None, limits=None))]
fn inspect_file<'py>(
    py: Python<'py>,
    path: PathBuf,
    source_id: Option<String>,
    limits: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let limits = read_limits_from_python(py, limits)?;
    let source_id = source_id.unwrap_or_else(|| path.to_string_lossy().into_owned());
    let result = py
        .detach(move || jt_core::inspect_file(&path, &source_id, &limits))
        .map_err(|error| match error {
            InspectError::Io(error) => PyErr::from(error),
            InspectError::Parse(error) => parse_error_to_python(py, error),
        })?;
    container_info_to_python(py, &result)
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    structure::register(module)?;
    mesh::register(module)?;
    export::register(module)?;
    module.add("CORE_VERSION", jt_core::CORE_VERSION)?;
    module.add("MODEL_API_VERSION", jt_py_bridge::MODEL_API_VERSION)?;
    module.add("INSPECT_API_VERSION", jt_py_bridge::INSPECT_API_VERSION)?;
    module.add("PAYLOAD_API_VERSION", jt_py_bridge::PAYLOAD_API_VERSION)?;
    module.add(
        "DEFAULT_MAX_INPUT_BYTES",
        ReadLimits::default().max_input_bytes,
    )?;
    module.add("DEFAULT_MAX_SEGMENTS", ReadLimits::default().max_segments)?;
    let limits = ReadLimits::default();
    module.add(
        "DEFAULT_MAX_SEGMENT_OUTPUT_BYTES",
        limits.max_segment_output_bytes,
    )?;
    module.add(
        "DEFAULT_MAX_TOTAL_OUTPUT_BYTES",
        limits.max_total_output_bytes,
    )?;
    module.add(
        "DEFAULT_MAX_LZMA_DICTIONARY_BYTES",
        limits.max_lzma_dictionary_bytes,
    )?;
    module.add("DEFAULT_MAX_ELEMENTS", limits.max_elements)?;
    module.add("DEFAULT_MAX_XZ_BLOCKS", limits.max_xz_blocks)?;
    module.add_class::<NativeScene>()?;
    module.add_class::<NativeDocument>()?;
    module.add_function(wrap_pyfunction!(validate_scene, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_file, module)?)?;
    module.add_function(wrap_pyfunction!(decode_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(decode_file, module)?)?;
    Ok(())
}
