use super::{owned_input, NativeDocument};
use jt_core::{InspectError, StructureDocument};
use jt_py_bridge::{parse_error_to_python, read_limits_from_python, structure::*};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyTuple},
};
use std::{path::PathBuf, sync::Arc};

#[pyclass(frozen, module = "cq_jt._native")]
pub struct NativeStructure {
    pub(crate) inner: Arc<StructureDocument>,
}
#[pymethods]
impl NativeStructure {
    #[getter]
    fn sources<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_sources(py, &self.inner.model)
    }
    #[getter]
    fn coordinates_in_meters(&self) -> bool {
        self.inner.model.coordinates_in_meters
    }
    #[getter]
    fn documents<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        ensure_python_structure_api(py)?;
        PyTuple::new(
            py,
            std::iter::once(&self.inner.document)
                .chain(&self.inner.referenced_documents)
                .map(|d| Py::new(py, NativeDocument { inner: d.clone() }))
                .collect::<PyResult<Vec<_>>>()?,
        )
    }
    fn to_model<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        structure_to_python(py, &self.inner.model)
    }
    #[getter]
    fn source<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        structure_source(py, &self.inner.model)
    }
    #[getter]
    fn nodes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_nodes(py, &self.inner.model)
    }
    #[getter]
    fn attributes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_attributes(py, &self.inner.model)
    }
    #[getter]
    fn materials<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_materials(py, &self.inner.model)
    }
    #[getter]
    fn atoms<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_atoms(py, &self.inner.model)
    }
    #[getter]
    fn bindings<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_bindings(py, &self.inner.model)
    }
    #[getter]
    fn parts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_parts(py, &self.inner.model)
    }
    #[getter]
    fn instances<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_instances(py, &self.inner.model)
    }
    #[getter]
    fn part_instances<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_part_instances(py, &self.inner.model)
    }
    #[getter]
    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_diagnostics(py, &self.inner.model)
    }
    #[getter]
    fn missing_references<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_missing(py, &self.inner.model)
    }
    #[getter]
    fn opaque_objects<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        structure_opaque(py, &self.inner.model)
    }
    #[getter]
    fn document(&self, py: Python<'_>) -> PyResult<NativeDocument> {
        ensure_python_structure_api(py)?;
        Ok(NativeDocument {
            inner: self.inner.document.clone(),
        })
    }
    #[getter]
    fn complete(&self) -> bool {
        self.inner.model.complete
    }
    #[getter]
    fn geometry_decoded(&self) -> bool {
        false
    }
    fn __repr__(&self) -> String {
        format!(
            "NativeStructure(parts={}, part_instances={}, complete={}, geometry_decoded=False)",
            self.inner.model.parts.len(),
            self.inner
                .model
                .instances
                .iter()
                .filter(|i| i.part_id.is_some())
                .count(),
            self.inner.model.complete
        )
    }
}
#[pyfunction]
#[pyo3(signature=(data,*,source_id="input",limits=None,options=None))]
fn read_structure_bytes(
    data: &Bound<'_, PyBytes>,
    source_id: &str,
    limits: Option<&Bound<'_, PyAny>>,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<NativeStructure> {
    let py = data.py();
    let options = structure_options_from_python(py, options)?;
    let limits = read_limits_from_python(py, limits)?;
    limits
        .check_input_size(data.as_bytes().len() as u64, source_id)
        .map_err(|e| parse_error_to_python(py, e))?;
    let bytes = owned_input(py, data.as_bytes(), source_id)?;
    let source_id = source_id.to_owned();
    let result = py
        .detach(move || jt_core::read_structure_bytes(&bytes, &source_id, &limits, &options))
        .map_err(|e| parse_error_to_python(py, e))?;
    Ok(NativeStructure {
        inner: Arc::new(result),
    })
}
#[pyfunction]
#[pyo3(signature=(path,*,source_id=None,limits=None,options=None))]
fn read_structure_file(
    py: Python<'_>,
    path: PathBuf,
    source_id: Option<String>,
    limits: Option<&Bound<'_, PyAny>>,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<NativeStructure> {
    let options = structure_options_from_python(py, options)?;
    let limits = read_limits_from_python(py, limits)?;
    let source_id = source_id.unwrap_or_else(|| path.to_string_lossy().into_owned());
    let result = py
        .detach(move || jt_core::read_structure_file(&path, &source_id, &limits, &options))
        .map_err(|e| match e {
            InspectError::Io(e) => PyErr::from(e),
            InspectError::Parse(e) => parse_error_to_python(py, e),
        })?;
    Ok(NativeStructure {
        inner: Arc::new(result),
    })
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("STRUCTURE_API_VERSION", STRUCTURE_API_VERSION)?;
    module.add_class::<NativeStructure>()?;
    module.add_function(wrap_pyfunction!(read_structure_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(read_structure_file, module)?)?;
    Ok(())
}
