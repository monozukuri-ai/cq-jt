use super::{index, owned_input, structure::NativeStructure};
use jt_core::{mesh::MeshScene, InspectError};
use jt_py_bridge::{mesh::*, parse_error_to_python, read_limits_from_python};
use pyo3::{
    exceptions::PyIndexError,
    prelude::*,
    types::{PyBytes, PyTuple},
};
use std::{path::PathBuf, sync::Arc};
#[pyclass(frozen, module = "cq_jt._native")]
pub struct NativeMeshScene {
    pub(crate) inner: Arc<MeshScene>,
}
#[pymethods]
impl NativeMeshScene {
    #[getter]
    fn structure(&self, py: Python<'_>) -> PyResult<NativeStructure> {
        ensure_python_mesh_api(py)?;
        Ok(NativeStructure {
            inner: self.inner.structure.clone(),
        })
    }
    #[getter]
    fn meshes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        meshes_to_python(py, &self.inner)
    }
    #[getter]
    fn instances<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        instances_to_python(py, &self.inner)
    }
    #[getter]
    fn lod_choices<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        choices_to_python(py, &self.inner)
    }
    #[getter]
    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        mesh_diagnostics_to_python(py, &self.inner)
    }
    #[getter]
    fn complete(&self) -> bool {
        self.inner.complete
    }
    #[getter]
    fn geometry_decoded(&self) -> bool {
        true
    }
    fn to_model<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        mesh_scene_to_python(py, &self.inner)
    }
    fn mesh_buffers<'py>(&self, mesh_id: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let mesh = self
            .inner
            .meshes
            .get(index(mesh_id)?)
            .ok_or_else(|| PyIndexError::new_err("mesh ID out of range"))?;
        mesh_buffers_to_python(mesh_id.py(), mesh)
    }
    fn __repr__(&self) -> String {
        format!(
            "NativeMeshScene(meshes={}, instances={}, complete={})",
            self.inner.meshes.len(),
            self.inner.instances.len(),
            self.inner.complete
        )
    }
}
#[pyfunction]
#[pyo3(signature=(data,*,source_id="input",limits=None,options=None))]
fn read_bytes(
    data: &Bound<'_, PyBytes>,
    source_id: &str,
    limits: Option<&Bound<'_, PyAny>>,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<NativeMeshScene> {
    let py = data.py();
    let options = read_options_from_python(py, options)?;
    let limits = read_limits_from_python(py, limits)?;
    limits
        .check_input_size(data.as_bytes().len() as u64, source_id)
        .map_err(|e| parse_error_to_python(py, e))?;
    let bytes = owned_input(py, data.as_bytes(), source_id)?;
    let source_id = source_id.to_owned();
    let result = py
        .detach(move || jt_core::mesh::read_bytes(&bytes, &source_id, &limits, &options))
        .map_err(|e| parse_error_to_python(py, e))?;
    Ok(NativeMeshScene {
        inner: Arc::new(result),
    })
}
#[pyfunction]
#[pyo3(signature=(path,*,source_id=None,limits=None,options=None))]
fn read_file(
    py: Python<'_>,
    path: PathBuf,
    source_id: Option<String>,
    limits: Option<&Bound<'_, PyAny>>,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<NativeMeshScene> {
    let options = read_options_from_python(py, options)?;
    let limits = read_limits_from_python(py, limits)?;
    let source_id = source_id.unwrap_or_else(|| path.to_string_lossy().into_owned());
    let result = py
        .detach(move || jt_core::mesh::read_file(&path, &source_id, &limits, &options))
        .map_err(|e| match e {
            InspectError::Io(e) => PyErr::from(e),
            InspectError::Parse(e) => parse_error_to_python(py, e),
        })?;
    Ok(NativeMeshScene {
        inner: Arc::new(result),
    })
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("MESH_API_VERSION", MESH_API_VERSION)?;
    module.add_class::<NativeMeshScene>()?;
    module.add_function(wrap_pyfunction!(read_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(read_file, module)?)?;
    Ok(())
}
