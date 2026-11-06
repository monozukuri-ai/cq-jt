//! Synthetic framing tests, not examples of valid JT geometry.
use jt_core::{inspect_bytes, ByteOrder, JtDocument, ParseErrorKind, ReadLimits};
use std::sync::Arc;

struct Fixture {
    data: Vec<u8>,
    order: ByteOrder,
    width: usize,
    toc: usize,
    offsets: [usize; 2],
}
impl Fixture {
    fn new(major: u16, minor: u16, order: ByteOrder, tail: bool, extra: bool) -> Self {
        let width = if major == 10 { 8 } else { 4 };
        let header = 101 + width + if extra { 16 } else { 0 };
        let toc_length = 4 + 2 * (24 + width);
        let lengths = [38, 29];
        let first = header + if tail { 0 } else { toc_length };
        let toc = if tail { first + 67 } else { header };
        let mut f = Self {
            data: vec![0; header + toc_length + 67],
            order,
            width,
            toc,
            offsets: [first, first + 38],
        };
        f.data[..75].fill(b' ');
        let text = format!("Version {major}.{minor} synthetic framing");
        f.data[..text.len()].copy_from_slice(text.as_bytes());
        f.data[75..80].copy_from_slice(b" \n\r\n ");
        f.data[80] = u8::from(order == ByteOrder::Big);
        f.put32(81, u32::from(extra));
        f.put_offset(85, toc as u64);
        f.data[85 + width..101 + width].fill(0x11);
        if extra {
            f.data[101 + width..header].fill(0xab);
        }
        f.put32(toc, 2);
        for (index, length) in lengths.into_iter().enumerate() {
            let entry = f.entry(index);
            let offset = f.offsets[index];
            let id = if index == 0 { 0x11 } else { 0x22 };
            let kind = if index == 0 { 1 } else { 250 };
            f.data[entry..entry + 16].fill(id);
            f.put_offset(entry + 16, offset as u64);
            f.put32(entry + 16 + width, length);
            f.put32(entry + 20 + width, (kind << 24) | 0x123);
            f.data[offset..offset + 16].fill(id);
            f.put32(offset + 16, kind);
            f.put32(offset + 20, length);
        }
        f.put32(first + 24, 2);
        f.put32(first + 28, 6);
        f.data[first + 32] = 2;
        // Intentionally not a valid compressed stream: container inspection must not decode it.
        f.data[first + 33..first + 38].fill(0xff);
        f
    }
    fn entry(&self, index: usize) -> usize {
        self.toc + 4 + index * (24 + self.width)
    }
    fn put32(&mut self, at: usize, value: u32) {
        let bytes = match self.order {
            ByteOrder::Little => value.to_le_bytes(),
            ByteOrder::Big => value.to_be_bytes(),
        };
        self.data[at..at + 4].copy_from_slice(&bytes);
    }
    fn put_offset(&mut self, at: usize, value: u64) {
        if self.width == 4 {
            self.put32(at, value as u32);
        } else {
            let bytes = match self.order {
                ByteOrder::Little => value.to_le_bytes(),
                ByteOrder::Big => value.to_be_bytes(),
            };
            self.data[at..at + 8].copy_from_slice(&bytes);
        }
    }
    fn error(&self, code: &str) {
        let error = inspect_bytes(&self.data, "fixture", &ReadLimits::default()).unwrap_err();
        assert_eq!(error.code, code, "{error:?}");
        let source = error.source.unwrap();
        assert_eq!(source.source_id, "fixture");
        assert_eq!(source.byte_domain, "file");
        assert!(source.end_offset <= self.data.len() as u64);
    }
}
fn fixture() -> Fixture {
    Fixture::new(9, 5, ByteOrder::Little, false, false)
}

#[test]
fn all_layouts_preserve_identities_ranges_and_unknown_types() {
    for (major, minor) in [(8, 0), (8, 1), (8, 2), (9, 5), (10, 0), (10, 3), (10, 6)] {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            for tail in [false, true] {
                let f = Fixture::new(major, minor, order, tail, major == 10);
                let info = inspect_bytes(&f.data, "fixture", &ReadLimits::default()).unwrap();
                assert_eq!((info.version.major, info.version.minor), (major, minor));
                assert_eq!(info.byte_order, order);
                assert_eq!(info.version_field.as_slice(), &f.data[..80]);
                assert_eq!(info.header_comment, "synthetic framing");
                assert_eq!(info.extra_header_guid, (major == 10).then_some([0xab; 16]));
                assert_eq!(info.toc_offset, f.toc as u64);
                assert_eq!(info.toc_entry_width as usize, 24 + f.width);
                assert_eq!(info.lsg_guid_raw, [0x11; 16]);
                for (i, segment) in info.segments.iter().enumerate() {
                    assert_eq!(segment.guid_raw, [if i == 0 { 0x11 } else { 0x22 }; 16]);
                    assert_eq!(segment.offset, f.offsets[i] as u64);
                    assert_eq!(segment.attributes & 0xffffff, 0x123);
                    assert_eq!(segment.toc_source.start_offset, f.entry(i) as u64);
                    assert_eq!(segment.header_source.end_offset, segment.offset + 24);
                    assert_eq!(segment.payload_source.start_offset, segment.offset + 24);
                    assert_eq!(
                        segment.payload_source.end_offset,
                        segment.offset + u64::from(segment.length)
                    );
                }
                assert_eq!(info.segments[0].compression.as_ref().unwrap().label, "ZLIB");
                assert_eq!(info.segments[1].type_name, "Unknown");
                assert!(info.segments[1].compression.is_none());
                assert_eq!(info.diagnostics[0].code, "unknown_segment_type");
                assert_eq!(info.segment_type_counts().get(&250), Some(&1));
            }
        }
    }
}

#[test]
fn legacy_space_marker_is_confined_to_version_8() {
    for major in [8, 9, 10] {
        let mut f = Fixture::new(major, 0, ByteOrder::Little, false, false);
        f.data[75..80].fill(b' ');
        let result = inspect_bytes(&f.data, "fixture", &ReadLimits::default());
        assert_eq!(result.is_ok(), major == 8);
    }
}

#[test]
fn jt95_conditional_reserved_guid_is_part_of_the_header() {
    for order in [ByteOrder::Little, ByteOrder::Big] {
        let mut f = Fixture::new(9, 5, order, false, true);
        let info = inspect_bytes(&f.data, "fixture", &ReadLimits::default()).unwrap();
        assert_eq!(info.header_size, 121);
        assert_eq!(info.extra_header_guid, Some([0xab; 16]));
        f.put_offset(85, 105);
        f.error("toc_overlaps_header");
    }
}

#[test]
fn rejects_header_and_version_errors() {
    let mut f = fixture();
    f.data[0] = 0xff;
    f.error("invalid_signature");
    let mut f = fixture();
    f.data[8] = b'x';
    f.error("invalid_signature");
    let mut f = fixture();
    f.data[10] = b'6';
    f.error("unsupported_version");
    let mut f = fixture();
    f.data[78] = 0;
    f.error("invalid_signature_terminator");
    let mut f = fixture();
    f.data[80] = 2;
    f.error("invalid_byte_order");
    let error = inspect_bytes(&fixture().data, "", &ReadLimits::default()).unwrap_err();
    assert_eq!(error.code, "invalid_source_id");
}

#[test]
fn rejects_invalid_toc_before_allocating() {
    let mut f = fixture();
    f.put32(85, u32::MAX);
    f.error("negative_toc_offset");
    let mut f = fixture();
    f.put32(85, 80);
    f.error("toc_overlaps_header");
    for count in [0, u32::MAX] {
        let mut f = fixture();
        f.put32(f.toc, count);
        f.error("invalid_entry_count");
    }
    let mut f = fixture();
    f.put32(f.toc, i32::MAX as u32);
    f.error("segment_limit_exceeded");
    let limits = ReadLimits {
        max_segments: u32::MAX,
        ..ReadLimits::default()
    };
    assert_eq!(
        inspect_bytes(&f.data, "fixture", &limits).unwrap_err().code,
        "unexpected_eof"
    );
    let mut f = Fixture::new(10, 6, ByteOrder::Big, false, false);
    f.put_offset(85, u64::MAX);
    f.error("unexpected_eof");
    let mut f = fixture();
    let entry = f.entry(1);
    f.data[entry..entry + 16].fill(0x11);
    f.error("duplicate_segment_guid");
}

#[test]
fn rejects_bad_segment_ranges_and_header_mismatches() {
    for length in [0, 23, u32::MAX] {
        let mut f = fixture();
        f.put32(f.entry(0) + 16 + f.width, length);
        f.error("invalid_segment_range");
    }
    let mut f = fixture();
    f.put32(f.entry(0) + 16, u32::MAX);
    f.error("negative_segment_offset");
    for offset in [80, 106] {
        let mut f = fixture();
        f.put_offset(f.entry(0) + 16, offset);
        f.error(if offset == 80 {
            "invalid_segment_range"
        } else {
            "segment_overlaps_toc"
        });
    }
    let mut f = Fixture::new(10, 6, ByteOrder::Little, false, false);
    f.put_offset(f.entry(0) + 16, u64::MAX - 1);
    f.error("invalid_segment_range");
    for field in [0, 16, 20] {
        let mut f = fixture();
        f.data[f.offsets[0] + field] ^= 1;
        f.error("segment_header_mismatch");
    }
    let mut f = fixture();
    f.put32(f.entry(0) + 20 + f.width, 2 << 24);
    f.error("segment_header_mismatch");
    let mut f = fixture();
    f.put32(f.entry(0) + 16 + f.width, 67);
    f.put32(f.offsets[0] + 20, 67);
    f.error("overlapping_segments");
}

#[test]
fn lsg_must_identify_the_correct_segment_type() {
    for id in [0x22, 0x33] {
        let mut f = fixture();
        f.data[85 + f.width..101 + f.width].fill(id);
        f.error("invalid_lsg_root");
    }
}

#[test]
fn compression_headers_are_bounded_without_decoding_streams() {
    for length in [0, u32::MAX, 7] {
        let mut f = fixture();
        f.put32(f.offsets[0] + 28, length);
        f.error("invalid_compressed_length");
    }
    let mut f = fixture();
    f.put32(f.entry(0) + 16 + f.width, 32);
    f.put32(f.offsets[0] + 20, 32);
    f.error("truncated_compression_header");
    for (flag, algorithm, label) in [(3, 3, "LZMA"), (0, 1, "None"), (9, 4, "Unknown")] {
        let mut f = fixture();
        f.put32(f.offsets[0] + 24, flag);
        f.data[f.offsets[0] + 32] = algorithm;
        let info = inspect_bytes(&f.data, "fixture", &ReadLimits::default()).unwrap();
        assert_eq!(info.segments[0].compression.as_ref().unwrap().label, label);
        assert_eq!(
            info.diagnostics
                .iter()
                .any(|d| d.code == "unknown_compression"),
            label == "Unknown"
        );
    }
}

#[test]
fn input_and_table_limits_are_inclusive() {
    let f = fixture();
    let limits = ReadLimits {
        max_input_bytes: f.data.len() as u64,
        max_segments: 2,
        ..ReadLimits::default()
    };
    assert!(inspect_bytes(&f.data, "fixture", &limits).is_ok());
    for limits in [
        ReadLimits {
            max_input_bytes: f.data.len() as u64 - 1,
            ..limits
        },
        ReadLimits {
            max_input_bytes: 0,
            ..limits
        },
        ReadLimits {
            max_segments: 1,
            ..limits
        },
        ReadLimits {
            max_segments: 0,
            ..limits
        },
    ] {
        assert_eq!(
            inspect_bytes(&f.data, "fixture", &limits).unwrap_err().kind,
            ParseErrorKind::LimitExceeded
        );
    }
}

#[test]
fn owned_document_keeps_bytes_and_checked_ranges_alive() {
    let f = fixture();
    let bytes: Arc<[u8]> = f.data.clone().into();
    let document =
        JtDocument::from_bytes(bytes.clone(), "fixture", &ReadLimits::default()).unwrap();
    drop(bytes);
    let clone = document.clone();
    drop(document);
    assert_eq!(
        clone.segment_bytes(0).unwrap(),
        &f.data[f.offsets[0]..f.offsets[1]]
    );
    assert!(clone.segment_bytes(2).is_none());
    assert_eq!(clone.info().source_id, "fixture");
}

#[test]
fn all_prefix_truncations_fail_and_deterministic_mutations_do_not_panic() {
    for major in [8, 9, 10] {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            for tail in [false, true] {
                let f = Fixture::new(major, 0, order, tail, major == 10);
                for end in 0..f.data.len() {
                    assert!(
                        inspect_bytes(&f.data[..end], "fixture", &ReadLimits::default()).is_err()
                    );
                }
                for index in 0..f.data.len() {
                    for bit in 0..8 {
                        let mut data = f.data.clone();
                        data[index] ^= 1 << bit;
                        let _ = inspect_bytes(&data, "fixture", &ReadLimits::default());
                    }
                }
            }
        }
    }
}
