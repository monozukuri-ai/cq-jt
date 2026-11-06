use super::mesh::NativeMeshScene;
use jt_core::export::{self, ExportFileError};
use jt_py_bridge::export::*;
use pyo3::{prelude::*, types::PyBytes};
use std::path::PathBuf;
#[pyfunction]
#[pyo3(signature=(scene,*,options=None))]
fn export_glb_bytes<'py>(
    py: Python<'py>,
    scene: &NativeMeshScene,
    options: Option<&Bound<'py, PyAny>>,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let options = export_options_from_python(py, options)?;
    let scene = scene.inner.clone();
    let output = py
        .detach(move || export::export_glb_bytes(&scene, &options))
        .map_err(|e| export_error_to_python(py, e))?;
    Ok((
        PyBytes::new(py, &output.bytes),
        export_report_to_python(py, &output.report)?,
    ))
}
#[pyfunction]
#[pyo3(signature=(scene,path,*,options=None,overwrite=false))]
fn export_glb<'py>(
    py: Python<'py>,
    scene: &NativeMeshScene,
    path: PathBuf,
    options: Option<&Bound<'py, PyAny>>,
    overwrite: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let options = export_options_from_python(py, options)?;
    let scene = scene.inner.clone();
    let report = py
        .detach(move || export::export_glb(&scene, &path, &options, overwrite))
        .map_err(|e| match e {
            ExportFileError::Io(e) => PyErr::from(e),
            ExportFileError::Export(e) => export_error_to_python(py, e),
        })?;
    export_report_to_python(py, &report)
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("EXPORT_API_VERSION", EXPORT_API_VERSION)?;
    module.add_function(wrap_pyfunction!(export_glb, module)?)?;
    module.add_function(wrap_pyfunction!(export_glb_bytes, module)?)?;
    Ok(())
}
