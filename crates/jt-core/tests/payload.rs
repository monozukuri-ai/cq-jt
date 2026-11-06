use jt_core::{decode_bytes, ReadLimits};

fn fixture(data: &[u8], major: u8) -> Vec<u8> {
    let mut out = vec![b' '; 80];
    let version = format!("Version {major}.0 test");
    out[..version.len()].copy_from_slice(version.as_bytes());
    out[75..80].copy_from_slice(b" \n\r\n ");
    out.push(0);
    out.extend(0i32.to_le_bytes());
    out.extend(105i32.to_le_bytes());
    out.extend([7; 16]);
    out.extend(1i32.to_le_bytes());
    out.extend([7; 16]);
    out.extend(137i32.to_le_bytes());
    out.extend((33 + data.len() as u32).to_le_bytes());
    out.extend((1u32 << 24).to_le_bytes());
    out.extend([7; 16]);
    out.extend(1i32.to_le_bytes());
    out.extend((33 + data.len() as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((1 + data.len() as i32).to_le_bytes());
    out.push(1);
    out.extend(data);
    out
}
fn payload(major: u8) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend((if major > 8 { 24i32 } else { 20 }).to_le_bytes());
    data.extend([1; 16]);
    data.push(123);
    if major > 8 {
        data.extend((-8i32).to_le_bytes());
    }
    data.extend([0, 0xff, 42]);
    for _ in 0..2 {
        data.extend(16i32.to_le_bytes());
        data.extend([0xff; 16]);
    }
    data.extend([1, 0, 0, 0, 0, 0]);
    data
}
#[test]
fn unknown_element_header_and_raw_payload_preserve_file_domain() {
    for major in [8, 9] {
        let raw = payload(major);
        let doc = decode_bytes(&fixture(&raw, major), "test", &ReadLimits::default()).unwrap();
        let segment = &doc.segments[0];
        assert_eq!(segment.bytes(), raw);
        assert_eq!(segment.elements[0].base_type, Some(123));
        assert_eq!(segment.elements[0].object_id, (major > 8).then_some(-8));
        assert_eq!(segment.element_bytes(0).unwrap(), &[0, 0xff, 42]);
        let element = segment.raw_element(0).unwrap();
        assert_eq!(element.raw_data, [0, 0xff, 42]);
        assert_eq!(element.source.unwrap().byte_domain, "file");
        assert_eq!(segment.decoded_source.start_offset, 170);
        assert_eq!(
            segment.tail_source.end_offset - segment.tail_source.start_offset,
            6
        );
        assert!(segment.element_bytes(3).is_none());
    }
}
#[test]
fn logical_element_truncations_and_count_limits_fail() {
    let raw = payload(9);
    // The property table is explicitly opaque; trim only within the lists.
    for end in 0..raw.len() - 6 {
        assert!(decode_bytes(&fixture(&raw[..end], 9), "test", &ReadLimits::default()).is_err());
    }
    let limits = ReadLimits {
        max_elements: 2,
        ..ReadLimits::default()
    };
    assert_eq!(
        decode_bytes(&fixture(&raw, 9), "test", &limits)
            .unwrap_err()
            .code,
        "element_limit_exceeded"
    );
    for at in 0..raw.len() - 6 {
        for bit in 0..8 {
            let mut data = raw.clone();
            data[at] ^= 1 << bit;
            let _ = decode_bytes(&fixture(&data, 9), "test", &ReadLimits::default());
        }
    }
}
