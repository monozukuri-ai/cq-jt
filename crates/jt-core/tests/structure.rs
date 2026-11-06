use jt_core::structure::{PropertyValue, ReadMode};
use jt_core::{read_structure_bytes, ReadLimits, StructureOptions};

fn guid(code: u32) -> [u8; 16] {
    let mut g = [
        0x9b, 0x6b, 0, 0x80, 0xc7, 0xbb, 0x59, 0x97, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    g.copy_within(..8, 8);
    g[..4].copy_from_slice(&code.to_le_bytes());
    g[4..8].copy_from_slice(&[0xc8, 0x2a, 0xd1, 0x11]);
    g
}
fn element(code: u32, id: i32, base: u8, data: &[u8]) -> Vec<u8> {
    let mut b = (21 + data.len() as i32).to_le_bytes().to_vec();
    b.extend(guid(code));
    b.push(base);
    b.extend(id.to_le_bytes());
    b.extend(data);
    b
}
fn group(children: &[i32], attrs: &[i32]) -> Vec<u8> {
    let mut b = 1i16.to_le_bytes().to_vec();
    b.extend(0u32.to_le_bytes());
    b.extend((attrs.len() as i32).to_le_bytes());
    for a in attrs {
        b.extend(a.to_le_bytes());
    }
    b.extend(1i16.to_le_bytes());
    b.extend((children.len() as i32).to_le_bytes());
    for c in children {
        b.extend(c.to_le_bytes());
    }
    b
}
fn mb(value: &str) -> Vec<u8> {
    let data = value.encode_utf16().collect::<Vec<_>>();
    let mut b = (data.len() as i32).to_le_bytes().to_vec();
    for c in data {
        b.extend(c.to_le_bytes());
    }
    b
}
fn atom(id: i32, value: &str) -> Vec<u8> {
    let mut b = 1i16.to_le_bytes().to_vec();
    b.extend(0u32.to_le_bytes());
    b.extend(1i16.to_le_bytes());
    b.extend(mb(value));
    element(0x10dd106e, id, 5, &b)
}
fn transform(id: i32, mask: u16, values: &[f64]) -> Vec<u8> {
    let mut b = 1i16.to_le_bytes().to_vec();
    b.push(8);
    b.extend(0u32.to_le_bytes());
    b.extend(1i16.to_le_bytes());
    b.extend(mask.to_le_bytes());
    for v in values {
        b.extend(v.to_le_bytes());
    }
    element(0x10dd1083, id, 3, &b)
}
fn payload(child: i32) -> Vec<u8> {
    let mut root = group(&[1, child], &[100]);
    root.extend(0i32.to_le_bytes());
    root.extend(mb("root.jt"));
    root.extend([0; 52]);
    let mut b = element(0x10dd103e, 0, 1, &root);
    for (id, attrs) in [(1, vec![101]), (2, vec![])] {
        let mut data = group(&[], &attrs);
        data.truncate(10 + attrs.len() * 4);
        data.extend(1i16.to_le_bytes());
        data.extend(3i32.to_le_bytes());
        b.extend(element(0x10dd102a, id, 0, &data));
    }
    let mut part = group(&[], &[]);
    part.extend(1i16.to_le_bytes());
    part.extend(1i16.to_le_bytes());
    part.extend(0i32.to_le_bytes());
    let mut e = element(0xce357244, 3, 1, &part);
    e[8..12].copy_from_slice(&[0xfb, 0x38, 0xd1, 0x11]);
    e[12..20].copy_from_slice(&[0xa5, 6, 0, 0x60, 0x97, 0xbd, 0xc6, 0xe1]);
    b.extend(e);
    b.extend(transform(100, 8, &[10.]));
    b.extend(transform(101, 0xcc00, &[0., 1., -1., 0.]));
    let mut marker = 16i32.to_le_bytes().to_vec();
    marker.extend([0xff; 16]);
    b.extend(&marker);
    b.extend(atom(200, "JT_PROP_MEASUREMENT_UNITS"));
    b.extend(atom(201, "Millimeters"));
    b.extend(atom(202, "JT_PROP_NAME"));
    b.extend(atom(203, "shared"));
    b.extend(marker);
    b.extend(1i16.to_le_bytes());
    b.extend(2i32.to_le_bytes());
    for row in [[0, 200, 201, 0], [3, 202, 203, 0]] {
        for x in row {
            b.extend(i32::to_le_bytes(x));
        }
    }
    b
}
fn container(data: &[u8]) -> Vec<u8> {
    let mut b = vec![b' '; 80];
    b[..19].copy_from_slice(b"Version 9.5 fixture");
    b[75..].copy_from_slice(b" \n\r\n ");
    b.push(0);
    b.extend(0i32.to_le_bytes());
    b.extend(105i32.to_le_bytes());
    b.extend([7; 16]);
    b.extend(1i32.to_le_bytes());
    b.extend([7; 16]);
    b.extend(137i32.to_le_bytes());
    b.extend((33 + data.len() as u32).to_le_bytes());
    b.extend((1u32 << 24).to_le_bytes());
    b.extend([7; 16]);
    b.extend(1i32.to_le_bytes());
    b.extend((33 + data.len() as i32).to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend((1 + data.len() as i32).to_le_bytes());
    b.push(1);
    b.extend(data);
    b
}
#[test]
fn structure_reuses_parts_and_composes_noncommuting_matrices() {
    let doc = read_structure_bytes(
        &container(&payload(2)),
        "test",
        &ReadLimits::default(),
        &StructureOptions::default(),
    )
    .unwrap();
    let m = &doc.model;
    assert_eq!(m.nodes.len(), 4);
    assert_eq!(m.parts.len(), 1);
    assert_eq!(m.instances.len(), 5);
    assert!(m.complete);
    assert_eq!(m.parts[0].name.as_deref(), Some("shared"));
    let instances = m
        .instances
        .iter()
        .filter(|i| i.part_id.is_some())
        .collect::<Vec<_>>();
    assert_eq!(instances[0].world_transform[3], 10.);
    assert_eq!(instances[0].world_transform[1], -1.);
    assert_eq!(instances[0].world_transform[4], 1.);
    assert_eq!(instances[1].world_transform[1], 0.);
    assert_eq!(instances[1].world_transform[0], 1.);
    assert!(instances.iter().all(|i| i.meters_per_unit == Some(0.001)));
    assert!(
        matches!(&m.atoms[0].value,PropertyValue::String(s) if s.as_ref()=="JT_PROP_MEASUREMENT_UNITS")
    );
}
#[test]
fn missing_references_are_partial_and_limits_stop_path_expansion() {
    let data = container(&payload(999));
    let options = StructureOptions {
        mode: ReadMode::BestEffort,
        ..StructureOptions::default()
    };
    assert_eq!(
        read_structure_bytes(
            &data,
            "test",
            &ReadLimits::default(),
            &StructureOptions::default()
        )
        .unwrap_err()
        .code,
        "missing_reference"
    );
    let partial = read_structure_bytes(&data, "test", &ReadLimits::default(), &options).unwrap();
    assert!(!partial.model.complete);
    assert_eq!(partial.model.missing_references[0].target, "999");
    let limits = StructureOptions {
        max_instances: 4,
        ..StructureOptions::default()
    };
    assert_eq!(
        read_structure_bytes(
            &container(&payload(2)),
            "test",
            &ReadLimits::default(),
            &limits
        )
        .unwrap_err()
        .code,
        "instance_limit_exceeded"
    );
}
#[test]
fn typed_payload_truncations_and_bit_mutations_are_bounded() {
    let raw = payload(2);
    let o = StructureOptions {
        max_nodes: 64,
        max_instances: 64,
        max_depth: 16,
        max_references: 512,
        max_string_bytes: 256,
        ..StructureOptions::default()
    };
    for end in 0..raw.len() {
        assert!(
            read_structure_bytes(&container(&raw[..end]), "test", &ReadLimits::default(), &o)
                .is_err()
        );
    }
    for at in 0..raw.len() {
        for bit in 0..8 {
            let mut b = raw.clone();
            b[at] ^= 1 << bit;
            let _ = read_structure_bytes(&container(&b), "test", &ReadLimits::default(), &o);
        }
    }
}
