#![cfg(feature = "glb")]
use jt_core::{
    export::{export_glb_bytes, ExportOptions},
    mesh::{read_bytes, ReadOptions},
    ReadLimits,
};
fn scene() -> jt_core::mesh::MeshScene {
    let text: String = include_str!("data/tetra9-scene.hex")
        .split_whitespace()
        .collect();
    let bytes: Vec<_> = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect();
    read_bytes(
        &bytes,
        "authored",
        &ReadLimits::default(),
        &ReadOptions::default(),
    )
    .unwrap()
}
fn options() -> ExportOptions {
    ExportOptions {
        meters_per_unit: Some(0.001),
        default_color: Some([1., 0., 0., 1.]),
        ..ExportOptions::default()
    }
}
#[test]
fn core_glb_has_shared_geometry_and_resolved_material_variants() {
    let output = export_glb_bytes(&scene(), &options()).unwrap();
    let data = &output.bytes;
    assert_eq!(&data[..4], b"glTF");
    assert_eq!(
        u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize,
        data.len()
    );
    let length = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&data[20..20 + length]).unwrap();
    assert_eq!(output.report.geometry_count, 1);
    assert_eq!(output.report.mesh_count, 2);
    assert_eq!(output.report.placed_triangle_count, 8);
    assert_eq!(
        json["meshes"][0]["primitives"][0]["attributes"],
        json["meshes"][1]["primitives"][0]["attributes"]
    );
    // A single corner has a different normal at the same position: split that
    // vertex, normalize the output copy, and keep the source value untouched.
    let mut crease = scene();
    crease.meshes[0].normal_indices.as_mut().unwrap()[0][0] = 1;
    crease.meshes[0].normals[1] = [17., 0., 0.];
    let output = export_glb_bytes(&crease, &options()).unwrap();
    assert_eq!(output.report.vertex_count, 5);
    assert_eq!(output.report.normalized_normal_count, 5);
    assert_eq!(crease.meshes[0].normals[1], [17., 0., 0.]);
    let n = u32::from_le_bytes(output.bytes[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&output.bytes[20..20 + n]).unwrap();
    let attributes = &json["meshes"][0]["primitives"][0]["attributes"];
    let vector = |attribute: &str, vertex: usize| {
        let accessor = &json["accessors"][attributes[attribute].as_u64().unwrap() as usize];
        let view = &json["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
        let offset = 28 + n + view["byteOffset"].as_u64().unwrap() as usize + vertex * 12;
        std::array::from_fn::<_, 3, _>(|i| {
            f32::from_le_bytes(
                output.bytes[offset + i * 4..offset + i * 4 + 4]
                    .try_into()
                    .unwrap(),
            )
        })
    };
    assert_eq!(vector("POSITION", 0), vector("POSITION", 4));
    assert_eq!(vector("NORMAL", 0), [1., 0., 0.]);
    assert_eq!(vector("NORMAL", 4), [0., 0., 1.]);
}
#[test]
fn mutable_rust_models_cannot_bypass_export_validation() {
    let mut s = scene();
    s.instances[0].mesh_id = 99;
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "invalid_mesh_reference"
    );
    let mut s = scene();
    s.meshes[0].triangles[0][0] = 999;
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "invalid_position_index"
    );
    let mut s = scene();
    s.meshes[0].normals[0] = [0.; 3];
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "invalid_normal"
    );
    let mut s = scene();
    s.meshes[0].positions[0][0] = f64::INFINITY;
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "position_out_of_range"
    );
    let mut s = scene();
    let structure = std::sync::Arc::make_mut(&mut s.structure);
    structure.model.instances[1].parent_id = Some(1);
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "invalid_export_hierarchy"
    );
    let mut s = scene();
    let structure = std::sync::Arc::make_mut(&mut s.structure);
    structure.model.instances[0].local_transform[0] = 1e-100;
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "invalid_export_transform"
    );
    let mut s = scene();
    s.complete = false;
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "partial_scene_requires_permission"
    );
}
#[test]
fn f32_range_rounding_and_collapse_are_checked() {
    let mut s = scene();
    s.meshes[0].positions[0][0] = 1e50;
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "position_out_of_range"
    );
    let mut s = scene();
    s.meshes[0].positions[0][0] = 1e8 + 1.;
    assert_eq!(
        export_glb_bytes(&s, &options()).unwrap_err().code,
        "position_precision_loss"
    );
    let mut s = scene();
    for p in &mut s.meshes[0].positions {
        p[0] += 1e8;
        p[1] += 1e8;
        p[2] += 1e8;
    }
    let mut o = options();
    o.max_position_error_meters = 1.;
    assert_eq!(
        export_glb_bytes(&s, &o).unwrap_err().code,
        "triangle_precision_loss"
    );
    let mut o = options();
    o.max_output_bytes = 0;
    assert_eq!(
        export_glb_bytes(&scene(), &o).unwrap_err().code,
        "export_byte_limit"
    );
}
