from dataclasses import FrozenInstanceError
import json
import subprocess
import sys

import pytest
import cq_jt
from cq_jt import ByteOrder, JtLimitError, JtParseError, JtUnsupportedError, ReadLimits
from jt_fixture import fixture


@pytest.mark.parametrize("version", ["8.0", "8.1", "8.2", "9.5", "10.0", "10.3", "10.6"])
@pytest.mark.parametrize("endian", ["<", ">"])
@pytest.mark.parametrize("tail", [False, True])
def test_typed_file_and_bytes_parity(tmp_path, version, endian, tail):
    extra = version.startswith("10.")
    data = fixture(version, endian, tail=tail, extra=extra)
    path = tmp_path / "日本語 part.jt"
    path.write_bytes(data)
    info = cq_jt.inspect_file(path, source_id="test")
    assert info == cq_jt.inspect_bytes(data, source_id="test")
    assert cq_jt.inspect_file(path).source_id == str(path)
    assert (info.version.major, info.version.minor) == tuple(map(int, version.split(".")))
    assert info.byte_order == (ByteOrder.LITTLE if endian == "<" else ByteOrder.BIG)
    assert info.header_size == (125 if extra else 105)
    assert info.extra_header_guid == (b"\xab" * 16 if extra else None)
    assert info.version_field == data[:80]
    assert info.header_comment == "synthetic framing"
    assert info.segment_count == 2
    assert info.segment_type_counts == {1: 1, 7: 1}
    assert not info.payload_decoded
    assert not info.diagnostics
    assert isinstance(info.segments, tuple)
    for index, segment in enumerate(info.segments):
        assert segment.guid_raw == bytes(range(index * 16, (index + 1) * 16))
        assert data[segment.offset:segment.offset + 16] == segment.guid_raw
        assert segment.header_source.start_offset == segment.offset
        assert segment.header_source.end_offset == segment.payload_source.start_offset == segment.offset + 24
        assert segment.payload_source.end_offset == segment.offset + segment.length
        assert segment.toc_source.byte_domain == "file"
        assert segment.toc_source.source_id == "test"
        assert segment.toc_source.end_offset - segment.toc_source.start_offset == info.toc_entry_width
    assert info.segments[0].compression.label == "ZLIB"
    assert info.segments[1].compression is None
    report = info.to_dict()
    assert json.loads(json.dumps(report)) == report
    assert report["schema_version"] == 1
    assert report["lsg_guid_raw_hex"] == bytes(range(16)).hex()
    assert report["container_checks"] == "passed"
    assert report["payload_decoded"] is False


def test_inspection_is_immutable_and_unknown_segments_are_retained():
    info = cq_jt.inspect_bytes(fixture(unknown=True))
    assert info.segments[1].type_name == "Unknown"
    assert info.segments[1].guid_raw == bytes(range(16, 32))
    assert info.diagnostics[0].code == "unknown_segment_type"
    with pytest.raises(FrozenInstanceError):
        info.size_bytes = 0
    with pytest.raises(FrozenInstanceError):
        info.segments[0].offset = 0
    report = info.to_dict()
    report["segments"].clear()
    counts = info.segment_type_counts
    counts.clear()
    assert info.segment_count == 2 and info.segment_type_counts == {1: 1, 250: 1}


def test_typed_errors_keep_byte_provenance():
    data = bytearray(fixture())
    data[80] = 2
    with pytest.raises(JtParseError) as caught:
        cq_jt.inspect_bytes(bytes(data), source_id="broken")
    error = caught.value
    assert isinstance(error, ValueError)
    assert error.code == "invalid_byte_order"
    assert error.source == cq_jt.SourceSpan("broken", "file", 80, 81)
    assert error.offset == 80
    with pytest.raises(JtUnsupportedError) as caught:
        cq_jt.inspect_bytes(fixture("11.0"))
    assert isinstance(caught.value, NotImplementedError)
    assert caught.value.code == "unsupported_version"
    assert caught.value.source.start_offset == 8
    with pytest.raises(JtParseError, match="source_id") as caught:
        cq_jt.inspect_bytes(fixture(), source_id="")
    assert caught.value.source is None


def test_limits_apply_before_parse_and_allow_exact_boundary(tmp_path):
    data = fixture()
    limits = ReadLimits(len(data), 2)
    assert cq_jt.inspect_bytes(data, limits=limits).segment_count == 2
    for limits, code in [(ReadLimits(0), "input_limit_exceeded"),
                         (ReadLimits(len(data) - 1), "input_limit_exceeded"),
                         (ReadLimits(max_segments=1), "segment_limit_exceeded"),
                         (ReadLimits(max_segments=0), "segment_limit_exceeded")]:
        with pytest.raises(JtLimitError) as caught:
            cq_jt.inspect_bytes(data, limits=limits)
        assert caught.value.code == code
    # A sparse file exceeds the default limit without allocating or reading its contents.
    path = tmp_path / "oversize.jt"
    with path.open("wb") as stream:
        stream.truncate(ReadLimits().max_input_bytes + 1)
    with pytest.raises(JtLimitError) as caught:
        cq_jt.inspect_file(path)
    assert caught.value.code == "input_limit_exceeded"


@pytest.mark.parametrize("limits", [object(), {}, ReadLimits(-1), ReadLimits(2**64),
    ReadLimits(True), ReadLimits(1.0), ReadLimits(max_segments=-1),
    ReadLimits(max_segments=2**32), ReadLimits(max_segments=False)])
def test_invalid_limits_are_rejected(limits):
    with pytest.raises(ValueError):
        cq_jt.inspect_bytes(fixture(), limits=limits)


@pytest.mark.parametrize("data", [bytearray(b"a"), memoryview(b"a"), "a", None])
def test_input_requires_bytes(data):
    with pytest.raises(TypeError):
        cq_jt.inspect_bytes(data)


def test_file_argument_and_io_errors(tmp_path):
    with pytest.raises(TypeError):
        cq_jt.inspect_file(bytes(tmp_path))
    with pytest.raises(FileNotFoundError):
        cq_jt.inspect_file(tmp_path / "missing.jt")
    with pytest.raises(IsADirectoryError):
        cq_jt.inspect_file(tmp_path)


@pytest.mark.parametrize("api_version", [3, True, None, "2"])
def test_inspect_api_mismatch_is_not_silently_accepted(monkeypatch, api_version):
    monkeypatch.setattr(cq_jt.container, "INSPECT_API_VERSION", api_version)
    with pytest.raises(ImportError, match="INSPECT_API_VERSION"):
        cq_jt.inspect_bytes(fixture())


def run_cli(*args):
    return subprocess.run([sys.executable, "-m", "cq_jt", *map(str, args)], text=True, capture_output=True)


def test_cli_success_and_failure_contracts(tmp_path):
    path = tmp_path / "input.jt"
    path.write_bytes(fixture("10.3", ">", tail=True))
    success = run_cli("inspect", path, "--json")
    assert success.returncode == 0, success.stderr
    assert json.loads(success.stdout) == cq_jt.inspect_file(path).to_dict()
    human = run_cli("inspect", path)
    assert human.returncode == 0
    assert "JT 10.3, big endian" in human.stdout
    assert "payloads not decoded" in human.stdout
    limited = run_cli("inspect", path, "--json", "--max-segments", 1)
    assert limited.returncode == 1 and not limited.stderr
    assert json.loads(limited.stdout)["error"]["code"] == "segment_limit_exceeded"
    path.write_bytes(b"broken")
    failure = run_cli("inspect", path, "--json")
    assert failure.returncode == 1 and not failure.stderr
    report = json.loads(failure.stdout)
    assert report["container_checks"] == "failed"
    assert report["payload_decoded"] is False
    assert report["error"]["source"]["source_id"] == str(path)
    assert run_cli("inspect", path).returncode == 1
    missing = run_cli("inspect", path.with_name("missing.jt"), "--json")
    assert missing.returncode == 1
    assert json.loads(missing.stdout)["error"]["code"] == "io_error"
    for option, value in [("--max-input-bytes", -1), ("--max-input-bytes", 2**64), ("--max-segments", 2**32)]:
        usage = run_cli("inspect", path, option, value)
        assert usage.returncode == 2
    assert run_cli().returncode == 0


def test_all_truncated_prefixes_are_errors():
    for version in ["9.5", "10.6"]:
        data = fixture(version, ">", tail=True, extra=version.startswith("10."))
        for end in range(len(data)):
            with pytest.raises(JtParseError):
                cq_jt.inspect_bytes(data[:end])
