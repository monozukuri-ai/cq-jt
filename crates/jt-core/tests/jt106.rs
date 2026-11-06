//! Authored specification controls, not evidence of native JT 10.6 compatibility.
use jt_core::{
    mesh::{read_bytes, ReadOptions},
    ParseErrorKind, ReadLimits,
};

fn bytes(text: &str) -> Vec<u8> {
    let hex: String = text.split_whitespace().collect();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn jt106_requires_opt_in_and_retains_qualification_warning() {
    for hex in [
        include_str!("data/tetra106-le.hex"),
        include_str!("data/tetra106-be.hex"),
    ] {
        let data = bytes(hex);
        let limits = ReadLimits::default();
        let mut options = ReadOptions::default();
        assert_eq!(
            read_bytes(&data, "control", &limits, &options)
                .unwrap_err()
                .kind,
            ParseErrorKind::UnsupportedVersion
        );
        options.structure.experimental_jt106 = true;
        let scene = read_bytes(&data, "control", &limits, &options).unwrap();
        assert!(scene.complete);
        assert_eq!(
            scene.meshes[0].positions,
            [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., -1.]]
        );
        assert_eq!(scene.meshes[0].triangles.len(), 4);
        assert!(scene
            .diagnostics
            .iter()
            .any(|d| d.code == "unverified_jt106_profile"));
        for end in 0..data.len() {
            assert!(read_bytes(&data[..end], "cut", &limits, &options).is_err());
        }
    }
}
