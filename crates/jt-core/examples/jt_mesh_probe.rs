use jt_core::{
    decode_file,
    mesh::{decode_mesh_element, MeshOptions},
    ReadLimits,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args().skip(1) {
        let doc = decode_file(std::path::Path::new(&path), &path, &ReadLimits::default())?;
        for segment in &doc.segments {
            for (i, e) in segment.elements.iter().enumerate() {
                let id = match doc.container.byte_order {
                    jt_core::ByteOrder::Little => {
                        u32::from_le_bytes(e.type_id[..4].try_into().unwrap())
                    }
                    jt_core::ByteOrder::Big => {
                        u32::from_be_bytes(e.type_id[..4].try_into().unwrap())
                    }
                };
                if id == 0x10dd10ab {
                    match decode_mesh_element(segment.element_bytes(i).unwrap(),doc.container.byte_order,e.data_source.clone(),&MeshOptions::default()){
                Ok(mesh)=>println!("{path} segment {}: vertices {} normals {} triangles {} cover {} bounds {:?} step {:?}",segment.segment_index,mesh.positions.len(),mesh.normals.len(),mesh.triangles.len(),mesh.cover_faces,mesh.bounds,mesh.quantization_step),
                Err(e)=>return Err(format!("{path} segment {}: {} ({}) {:?}",segment.segment_index,e,e.code,e.source).into()),
            }
                }
            }
        }
    }
    Ok(())
}
