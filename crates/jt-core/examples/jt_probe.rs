use jt_core::{inspect_file, ReadLimits};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: cargo run -p jt-core --example jt_probe -- file.jt")?;
    let info = inspect_file(
        Path::new(&path),
        &path.to_string_lossy(),
        &ReadLimits::default(),
    )?;
    println!(
        "JT {}.{}: {} bytes, {} segments, payloads not decoded",
        info.version.major,
        info.version.minor,
        info.size_bytes,
        info.segments.len()
    );
    for (kind, count) in info.segment_type_counts() {
        println!("{kind}: {count}");
    }
    Ok(())
}
