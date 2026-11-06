use super::{export_glb_bytes, ExportError, ExportOptions, ExportReport};
use crate::mesh::MeshScene;
use std::{
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
#[derive(Debug)]
pub enum ExportFileError {
    Export(ExportError),
    Io(io::Error),
}
impl fmt::Display for ExportFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Export(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl Error for ExportFileError {}
impl From<io::Error> for ExportFileError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
static COUNTER: AtomicU64 = AtomicU64::new(0);
/// Build fully before creating a temporary file; commit atomically in the same directory.
pub fn export_glb(
    scene: &MeshScene,
    path: &Path,
    options: &ExportOptions,
    overwrite: bool,
) -> Result<ExportReport, ExportFileError> {
    let output = export_glb_bytes(scene, options).map_err(ExportFileError::Export)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if path.file_name().is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "output must name a file").into());
    }
    let mut temporary = None;
    for _ in 0..16 {
        let candidate = parent.join(format!(
            ".cq-jt-{}-{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((file, Temporary(candidate)));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let (mut file, temp) = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot create a unique GLB temporary file",
        )
    })?;
    file.write_all(&output.bytes)?;
    file.sync_all()?;
    drop(file);
    if overwrite {
        fs::rename(&temp.0, path)?;
    } else {
        fs::hard_link(&temp.0, path)?;
    }
    Ok(output.report)
}
