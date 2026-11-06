"""Logical framing metadata; native buffers are copied only on explicit access."""
from __future__ import annotations
from dataclasses import dataclass
from .model import SourceSpan

PAYLOAD_API_VERSION = 1
PAYLOAD_JSON_SCHEMA_VERSION = 1


@dataclass(frozen=True, slots=True)
class LogicalElement:
    declared_length: int
    type_id: bytes
    base_type: int | None
    object_id: int | None
    list_index: int
    is_end_marker: bool
    source: SourceSpan
    data_source: SourceSpan


@dataclass(frozen=True, slots=True)
class DecodedSegmentInfo:
    segment_index: int
    guid_raw: bytes
    segment_type: int
    codec: str
    encoded_source: SourceSpan
    decoded_source: SourceSpan
    tail_source: SourceSpan
    elements_framed: bool
    elements: tuple[LogicalElement, ...]
