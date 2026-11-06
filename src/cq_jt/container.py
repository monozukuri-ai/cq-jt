"""Typed inspection results. Payloads are not decompressed by these APIs."""
from __future__ import annotations

from dataclasses import asdict, dataclass, field
from enum import Enum

from . import _native
from .model import Diagnostic, JtVersion, SourceSpan

INSPECT_API_VERSION = 2
INSPECT_JSON_SCHEMA_VERSION = 1

if getattr(_native, "INSPECT_API_VERSION", None) != INSPECT_API_VERSION:
    raise ImportError("cq-jt requires native inspection API 2; install a matching wheel or rebuild the extension")


class ByteOrder(str, Enum):
    LITTLE = "little"
    BIG = "big"


@dataclass(frozen=True, slots=True)
class ReadLimits:
    """Maximum accepted input and TOC count; zero rejects nonempty input/tables."""

    max_input_bytes: int = _native.DEFAULT_MAX_INPUT_BYTES
    max_segments: int = _native.DEFAULT_MAX_SEGMENTS
    max_segment_output_bytes: int = _native.DEFAULT_MAX_SEGMENT_OUTPUT_BYTES
    max_total_output_bytes: int = _native.DEFAULT_MAX_TOTAL_OUTPUT_BYTES
    max_lzma_dictionary_bytes: int = _native.DEFAULT_MAX_LZMA_DICTIONARY_BYTES
    max_elements: int = _native.DEFAULT_MAX_ELEMENTS
    max_xz_blocks: int = _native.DEFAULT_MAX_XZ_BLOCKS


@dataclass(frozen=True, slots=True)
class CompressionHeader:
    flag: int
    declared_length: int
    algorithm: int
    label: str
    source: SourceSpan


@dataclass(frozen=True, slots=True)
class SegmentInfo:
    guid_raw: bytes
    offset: int
    length: int
    segment_type: int
    type_name: str
    attributes: int
    toc_source: SourceSpan
    header_source: SourceSpan
    payload_source: SourceSpan
    compression: CompressionHeader | None


@dataclass(frozen=True, slots=True)
class ContainerInfo:
    source_id: str
    size_bytes: int
    version: JtVersion
    version_field: bytes
    header_comment: str
    byte_order: ByteOrder
    header_field: int
    header_size: int
    extra_header_guid: bytes | None
    toc_offset: int
    toc_entry_width: int
    lsg_guid_raw: bytes
    segments: tuple[SegmentInfo, ...]
    diagnostics: tuple[Diagnostic, ...]
    payload_decoded: bool = field(default=False, init=False)

    @property
    def segment_count(self) -> int:
        return len(self.segments)

    @property
    def segment_type_counts(self) -> dict[int, int]:
        counts: dict[int, int] = {}
        for segment in self.segments:
            counts[segment.segment_type] = counts.get(segment.segment_type, 0) + 1
        return dict(sorted(counts.items()))

    def to_dict(self) -> dict:
        """JSON schema 1. GUID hex is raw byte order, not a UUID string."""
        return {
            "schema_version": INSPECT_JSON_SCHEMA_VERSION,
            "source_id": self.source_id, "size_bytes": self.size_bytes,
            "version": f"{self.version.major}.{self.version.minor}",
            "version_field_hex": self.version_field.hex(),
            "header_comment": self.header_comment, "byte_order": self.byte_order.value,
            "header_field": self.header_field, "header_size": self.header_size,
            "extra_header_guid_hex": None if self.extra_header_guid is None else self.extra_header_guid.hex(),
            "toc_offset": self.toc_offset, "toc_entry_width": self.toc_entry_width,
            "lsg_guid_raw_hex": self.lsg_guid_raw.hex(),
            "segment_count": self.segment_count,
            "segment_type_counts": {str(k): v for k, v in self.segment_type_counts.items()},
            "container_checks": "passed", "payload_decoded": False,
            "segments": [{
                "guid_raw_hex": s.guid_raw.hex(), "offset": s.offset, "length": s.length,
                "type": s.segment_type, "type_name": s.type_name, "attributes": s.attributes,
                "toc_source": asdict(s.toc_source), "header_source": asdict(s.header_source),
                "payload_source": asdict(s.payload_source),
                "compression_header": None if s.compression is None else asdict(s.compression),
            } for s in self.segments],
            "diagnostics": [asdict(d) for d in self.diagnostics],
        }
