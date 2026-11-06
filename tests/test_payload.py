from dataclasses import FrozenInstanceError, replace
import gc
import json
import struct
import subprocess
import sys
import zlib
import lzma

import pytest
import cq_jt
from jt_fixture import container_for_payloads, end_marker, logical_element, lsg_payload


@pytest.mark.parametrize("version", ["8.0", "8.1", "9.5", "10.0", "10.3", "10.6"])
@pytest.mark.parametrize("endian", ["<", ">"])
@pytest.mark.parametrize("codec", ["none", "zlib", "xz"])
def test_decode_metadata_bytes_and_owned_lifetime(tmp_path, version, endian, codec):
    decoded = lsg_payload(version, endian)
    data = container_for_payloads([(1, decoded, codec)], version=version, endian=endian)
    path = tmp_path / "テスト.jt"
    path.write_bytes(data)
    document = cq_jt.decode_bytes(data, source_id="test")
    from_file = cq_jt.decode_file(path, source_id="test")
    assert document.container == from_file.container
    assert document.segments == from_file.segments
    assert document.total_output_bytes == len(decoded)
    assert document.geometry_decoded is False
    assert document.segment_bytes(0) == decoded
    segment, = document.segments
    assert segment.codec == {"none": "None", "zlib": "ZLIB", "xz": "XZ/LZMA2"}[codec]
    assert segment.elements_framed
    if codec == "none":
        assert segment.decoded_source.byte_domain == "file"
        assert segment.decoded_source == segment.encoded_source
    else:
        assert segment.decoded_source.byte_domain == "segment/000102030405060708090a0b0c0d0e0f/inflated"
        assert segment.decoded_source.start_offset == 0
    elements = segment.elements
    assert len(elements) == (3 if version == "10.6" else 4)
    assert elements[0].type_id == b"\x31" * 16
    assert elements[0].object_id == (None if version.startswith("8.") else -7)
    assert elements[0].base_type == 255
    assert document.element_bytes(0, 0) == b"\0\xffopaque"
    raw = document.raw_element(0, 0)
    assert raw == cq_jt.RawElement(elements[0].type_id, elements[0].object_id, b"\0\xffopaque", elements[0].data_source)
    assert document.element_bytes(0, len(elements) - 1) == b""
    assert elements[-1].is_end_marker
    assert segment.tail_source.end_offset - segment.tail_source.start_offset == 6
    assert any(d.code == "unparsed_segment_tail" for d in document.diagnostics)
    # Returned snapshots and copied raw bytes survive the native owner.
    del data, document, from_file
    gc.collect()
    assert raw.raw_data == b"\0\xffopaque" and segment.elements == elements
    with pytest.raises(FrozenInstanceError):
        elements[0].object_id = 0


def test_unknown_segment_and_element_base_type_are_retained():
    payload = lsg_payload()
    document = cq_jt.decode_bytes(container_for_payloads([(1, payload, "zlib"), (250, b"unknown bytes", "raw")]))
    assert document.segment_bytes(1) == b"unknown bytes"
    assert document.segments[1].elements == ()
    assert not document.segments[1].elements_framed
    assert any(d.code == "segment_payload_uninterpreted" for d in document.diagnostics)
    data = logical_element(base_type=123) + end_marker() + end_marker()
    doc = cq_jt.decode_bytes(container_for_payloads([(1, data, "zlib")]))
    assert doc.segments[0].elements[0].base_type == 123
    assert doc.element_bytes(0, 0) == b"\0\xffopaque"


@pytest.mark.parametrize("codec", ["none", "zlib", "xz"])
def test_output_and_element_limits_are_inclusive_and_document_wide(codec):
    payload = lsg_payload()
    data = container_for_payloads([(1, payload, codec), (7, logical_element(), "raw")])
    total = len(payload) + len(logical_element())
    limits = cq_jt.ReadLimits(max_segment_output_bytes=len(payload), max_total_output_bytes=total, max_elements=5)
    assert cq_jt.decode_bytes(data, limits=limits).total_output_bytes == total
    for limits in [replace(limits, max_segment_output_bytes=len(payload)-1),
                   replace(limits, max_total_output_bytes=total-1),
                   replace(limits, max_segment_output_bytes=0),
                   replace(limits, max_total_output_bytes=0),
                   replace(limits, max_elements=4), replace(limits, max_elements=0)]:
        with pytest.raises(cq_jt.JtLimitError):
            cq_jt.decode_bytes(data, limits=limits)


def test_xz_dictionary_and_block_limits_precede_allocation():
    data = container_for_payloads([(1, lsg_payload(), "xz")])
    for limits, code in [(cq_jt.ReadLimits(max_lzma_dictionary_bytes=4095), "dictionary_limit_exceeded"),
                         (cq_jt.ReadLimits(max_lzma_dictionary_bytes=0), "dictionary_limit_exceeded"),
                         (cq_jt.ReadLimits(max_xz_blocks=0), "xz_block_limit_exceeded")]:
        with pytest.raises(cq_jt.JtLimitError) as caught:
            cq_jt.decode_bytes(data, limits=limits)
        assert caught.value.code == code
    assert cq_jt.decode_bytes(data, limits=cq_jt.ReadLimits(max_lzma_dictionary_bytes=4096, max_xz_blocks=1))


@pytest.mark.parametrize("field", ["max_segment_output_bytes", "max_total_output_bytes", "max_lzma_dictionary_bytes", "max_elements", "max_xz_blocks"])
@pytest.mark.parametrize("bad", [-1, True, 1.0, 2**64])
def test_new_limits_reject_invalid_values(field, bad):
    with pytest.raises(ValueError):
        cq_jt.decode_bytes(container_for_payloads(), limits=replace(cq_jt.ReadLimits(), **{field: bad}))


@pytest.mark.parametrize("payload,code", [
    (b"\0\0\0\0", "invalid_element_length"),
    (struct.pack("<i", 2**31-1) + b"\0" * 25, "invalid_element_length"),
    (struct.pack("<i", 16) + b"\x31" * 16, "invalid_element_header"),
    (struct.pack("<i", 17) + b"\xff" * 16 + b"\0", "invalid_element_header"),
    (logical_element() + end_marker(), "missing_end_marker"),
    (logical_element() + b"\x01", "unexpected_eof"),
])
def test_bad_elements_report_decoded_domain_offsets(payload, code):
    with pytest.raises(cq_jt.JtParseError) as caught:
        cq_jt.decode_bytes(container_for_payloads([(1, payload, "zlib")]), source_id="bad")
    assert caught.value.code == code
    assert caught.value.source.source_id == "bad"
    assert caught.value.source.byte_domain.endswith("/inflated")
    assert caught.value.source.end_offset <= len(payload)


def encoded_container(stream, algorithm=2, flag=2):
    # Replace the already framed stream and fix TOC and segment lengths.
    b = bytearray(container_for_payloads())
    position = 137
    b = b[:position + 24] + struct.pack("<IiB", flag, len(stream) + 1, algorithm) + stream
    length = len(b) - position
    struct.pack_into("<I", b, 129, length)
    struct.pack_into("<i", b, position + 20, length)
    return bytes(b)


@pytest.mark.parametrize("algorithm,flag,stream", [(9, 9, b"opaque"), (1, 3, b"opaque"),
    (3, 3, lzma.compress(b"abc", format=lzma.FORMAT_ALONE)),
    (3, 3, lzma.compress(b"abc", filters=[{"id": lzma.FILTER_DELTA, "dist": 1}, {"id": lzma.FILTER_LZMA2}]))])
def test_unsupported_encodings_fail_explicitly(algorithm, flag, stream):
    expected = cq_jt.JtUnsupportedError if algorithm != 3 or stream.startswith(b"\xfd7zXZ") else cq_jt.JtParseError
    with pytest.raises(expected):
        cq_jt.decode_bytes(encoded_container(stream, algorithm, flag))


@pytest.mark.parametrize("codec", ["zlib", "xz"])
def test_checksum_truncation_trailing_and_expansion_bombs(codec):
    original = lsg_payload()
    stream = zlib.compress(original) if codec == "zlib" else lzma.compress(original)
    algorithm = 2 if codec == "zlib" else 3
    for invalid in [stream[:-1], stream + b"junk", stream + stream,
                    stream[:len(stream)//2] + bytes([stream[len(stream)//2] ^ 0xff]) + stream[len(stream)//2+1:]]:
        with pytest.raises(cq_jt.JtParseError):
            cq_jt.decode_bytes(encoded_container(invalid, algorithm, algorithm))
    bomb = zlib.compress(b"\0" * 1_000_000) if codec == "zlib" else lzma.compress(b"\0" * 1_000_000)
    with pytest.raises(cq_jt.JtLimitError) as caught:
        cq_jt.decode_bytes(encoded_container(bomb, algorithm, algorithm), limits=cq_jt.ReadLimits(max_segment_output_bytes=1024))
    assert caught.value.code == "output_limit_exceeded"


@pytest.mark.parametrize("index,error", [(-1, IndexError), (2**100, IndexError), (10, IndexError), (True, TypeError), (0.0, TypeError)])
def test_explicit_bytes_access_checks_indices(index, error):
    doc = cq_jt.decode_bytes(container_for_payloads())
    for call in [lambda: doc.segment_bytes(index), lambda: doc.element_bytes(index, 0),
                 lambda: doc.element_bytes(0, index), lambda: doc.raw_element(0, index)]:
        with pytest.raises(error): call()


@pytest.mark.parametrize("bad", [True, 2, "1", None])
def test_payload_api_guard(monkeypatch, bad):
    data = container_for_payloads()
    doc = cq_jt.decode_bytes(data)
    monkeypatch.setattr(cq_jt.payload, "PAYLOAD_API_VERSION", bad)
    for call in [lambda: cq_jt.decode_bytes(data), lambda: doc.segments,
                 lambda: doc.segment_bytes(0), lambda: doc.raw_element(0, 0)]:
        with pytest.raises(ImportError, match="PAYLOAD_API_VERSION"): call()


def test_decode_cli_and_io_errors(tmp_path):
    path = tmp_path / "input.jt"
    path.write_bytes(container_for_payloads())
    command = [sys.executable, "-m", "cq_jt", "decode", str(path)]
    result = subprocess.run(command + ["--json"], text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["schema_version"] == 1 and report["outer_decoded"]
    assert report["geometry_decoded"] is False and report["element_payloads_decoded"] is False
    assert report["segments"][0]["elements"][0]["type_id_raw_hex"] == "31" * 16
    assert subprocess.run(command, capture_output=True).returncode == 0
    limited = subprocess.run(command + ["--json", "--max-total-output-bytes", "0"], text=True, capture_output=True)
    assert limited.returncode == 1 and json.loads(limited.stdout)["error"]["code"] == "output_limit_exceeded"
    assert subprocess.run(command + ["--max-elements", str(2**32)], capture_output=True).returncode == 2
    with pytest.raises(FileNotFoundError): cq_jt.decode_file(path.with_name("absent.jt"))
    with pytest.raises(TypeError): cq_jt.decode_file(bytes(path))
    with pytest.raises(TypeError): cq_jt.decode_bytes(bytearray(path.read_bytes()))


def vli(value):
    out = bytearray()
    while value >= 128:
        out.append((value & 127) | 128)
        value >>= 7
    return bytes(out) + bytes([value])


def read_vli(data, at):
    value, shift = 0, 0
    while True:
        b = data[at]; at += 1
        value |= (b & 127) << shift
        if b < 128: return value, at
        shift += 7


def xz_blocks(parts, dictionaries=None, forged_sizes=None):
    blocks = b""
    records = b""
    dictionaries = dictionaries or [4096] * len(parts)
    for index, (part, dictionary) in enumerate(zip(parts, dictionaries)):
        stream = lzma.compress(part, filters=[{"id": lzma.FILTER_LZMA2, "dict_size": dictionary}])
        index_length = (struct.unpack_from("<I", stream, len(stream)-8)[0]+1)*4
        start = len(stream)-12-index_length
        count, at = read_vli(stream, start+1)
        assert count == 1
        unpadded, at = read_vli(stream, at)
        unpacked, at = read_vli(stream, at)
        blocks += stream[12:start]
        records += vli(unpadded) + vli(unpacked if forged_sizes is None else forged_sizes[index])
    index_data = b"\0" + vli(len(parts)) + records
    index_data += b"\0" * (-len(index_data) % 4)
    index_data += struct.pack("<I", zlib.crc32(index_data))
    flags = struct.pack("<I", len(index_data)//4-1) + stream[6:8]
    footer = struct.pack("<I", zlib.crc32(flags)) + flags + b"YZ"
    return stream[:12] + blocks + index_data + footer


def test_multiple_xz_blocks_are_checked_before_decode():
    payload = lsg_payload()
    split = len(payload)//2
    stream = xz_blocks([payload[:split], payload[split:]])
    assert lzma.decompress(stream) == payload
    data = encoded_container(stream, 3, 3)
    assert cq_jt.decode_bytes(data).segment_bytes(0) == payload
    with pytest.raises(cq_jt.JtLimitError) as caught:
        cq_jt.decode_bytes(data, limits=cq_jt.ReadLimits(max_xz_blocks=1))
    assert caught.value.code == "xz_block_limit_exceeded"
    later_large_dict = xz_blocks([payload[:split], payload[split:]], [4096, 8*1024*1024])
    with pytest.raises(cq_jt.JtLimitError) as caught:
        cq_jt.decode_bytes(encoded_container(later_large_dict, 3, 3), limits=cq_jt.ReadLimits(max_lzma_dictionary_bytes=4096))
    assert caught.value.code == "dictionary_limit_exceeded"
    # Both forged sizes add up correctly; each block must still match its chunks.
    forged = xz_blocks([payload[:split], payload[split:]], forged_sizes=[split-1, len(payload)-split+1])
    with pytest.raises(cq_jt.JtParseError): cq_jt.decode_bytes(encoded_container(forged, 3, 3))


@pytest.mark.parametrize("check", [lzma.CHECK_NONE, lzma.CHECK_CRC32, lzma.CHECK_CRC64, lzma.CHECK_SHA256])
def test_supported_xz_checks(check):
    stream = lzma.compress(lsg_payload(), check=check)
    assert cq_jt.decode_bytes(encoded_container(stream, 3, 3)).segment_bytes(0) == lsg_payload()


def test_enormous_xz_dictionary_is_rejected_even_for_tiny_output():
    stream = bytearray(lzma.compress(lsg_payload(), filters=[{"id": lzma.FILTER_LZMA2, "dict_size": 4096}]))
    # Single-filter block header at 12, properties byte at 16. Update CRC so a
    # structurally valid 4 GiB dictionary is rejected by the allocation guard.
    stream[16] = 40
    header_size = (stream[12] + 1)*4
    struct.pack_into("<I", stream, 12+header_size-4, zlib.crc32(stream[12:12+header_size-4]))
    with pytest.raises(cq_jt.JtLimitError) as caught:
        cq_jt.decode_bytes(encoded_container(stream, 3, 3))
    assert caught.value.code == "dictionary_limit_exceeded"


def test_bytes_outside_declared_compression_frame_are_rejected():
    stream = zlib.compress(lsg_payload())
    data = bytearray(encoded_container(stream + b"padding"))
    struct.pack_into("<i", data, 137+28, len(stream)+1)
    # Container inspection only checks that this range fits the segment; decoding requires the
    # complete segment body to be accounted for by its declared frame.
    assert cq_jt.inspect_bytes(bytes(data)).segment_count == 1
    with pytest.raises(cq_jt.JtParseError) as caught:
        cq_jt.decode_bytes(bytes(data))
    assert caught.value.code == "unexpected_segment_trailer"
