use jt_core::{read_structure_file, ReadLimits, StructureOptions};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("provide a JT 9.5 path")?;
    let document = read_structure_file(
        Path::new(&path),
        &path,
        &ReadLimits::default(),
        &StructureOptions::default(),
    )?;
    let model = &document.model;
    println!("nodes={} parts={} node_instances={} part_instances={} materials={} atoms={} bindings={} complete={}",model.nodes.len(),model.parts.len(),model.instances.len(),model.instances.iter().filter(|i|i.part_id.is_some()).count(),model.materials.len(),model.atoms.len(),model.bindings.len(),model.complete);
    for p in &model.parts {
        println!(
            "part {} {:?}: {} shape nodes",
            p.id,
            p.name,
            p.shape_node_ids.len()
        );
    }
    Ok(())
}
