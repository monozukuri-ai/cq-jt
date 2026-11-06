use jt_core::{
    mesh::{
        resolver::{read_resolved_bytes, SourceResolver},
        ReadOptions,
    },
    InspectError, ParseErrorKind, ReadLimits,
};
fn bytes(text: &str) -> Vec<u8> {
    let s: String = text.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
struct Memory {
    data: Vec<u8>,
    reads: usize,
}
impl SourceResolver for Memory {
    fn locate(&mut self, _owner: &str, reference: &str) -> Result<String, InspectError> {
        Ok(reference.into())
    }
    fn read(&mut self, key: &str, limits: &ReadLimits) -> Result<Vec<u8>, InspectError> {
        assert_eq!(key, "part.jt");
        self.reads += 1;
        limits.check_input_size(self.data.len() as u64, key)?;
        Ok(self.data.clone())
    }
}
#[test]
fn rust_resolver_caches_sources_and_converts_units_without_python() {
    let root = bytes(include_str!("data/assembly9.hex"));
    let mut memory = Memory {
        data: bytes(include_str!("data/tetra9-scene.hex")),
        reads: 0,
    };
    let mut options = ReadOptions::default();
    options.resolver.meters_per_unit = Some(0.001);
    let scene = read_resolved_bytes(
        &root,
        "root.jt",
        "root",
        &mut memory,
        &ReadLimits::default(),
        &options,
    )
    .unwrap();
    assert_eq!(memory.reads, 1);
    assert_eq!(scene.structure.model.sources.len(), 2);
    assert_eq!(scene.meshes.len(), 1);
    assert_eq!(scene.instances.len(), 4);
    assert!(scene.structure.model.coordinates_in_meters);
    assert_eq!(scene.meshes[0].positions[1], [0.001, 0., 0.]);
    assert!(scene
        .structure
        .model
        .instances
        .iter()
        .all(|i| i.meters_per_unit == Some(1.)));
    options.resolver.max_total_input_bytes = root.len() as u64;
    let error = read_resolved_bytes(
        &root,
        "root.jt",
        "root",
        &mut memory,
        &ReadLimits::default(),
        &options,
    )
    .unwrap_err();
    assert_eq!(error.kind, ParseErrorKind::LimitExceeded);
}
#[test]
fn modern_core_scene_and_all_input_truncations_are_checked() {
    let input = bytes(include_str!("data/tetra10.hex"));
    let options = ReadOptions::default();
    let scene =
        jt_core::mesh::read_bytes(&input, "modern", &ReadLimits::default(), &options).unwrap();
    assert_eq!(scene.meshes[0].triangles.len(), 4);
    assert!(scene.complete);
    for n in 0..input.len() {
        assert!(
            jt_core::mesh::read_bytes(&input[..n], "cut", &ReadLimits::default(), &options)
                .is_err()
        );
    }
}
