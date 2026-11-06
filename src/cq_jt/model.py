"""Typed model values. Rust validates every complete Scene snapshot."""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from . import _native

MODEL_API_VERSION = 1


@dataclass(frozen=True, slots=True)
class JtVersion:
    """JT file version, independent of model and package versions."""

    major: int
    minor: int


@dataclass(frozen=True, slots=True)
class SourceMetadata:
    source_id: str
    label: str | None = None
    version: JtVersion | None = None
    meters_per_unit: float | None = None


@dataclass(frozen=True, slots=True)
class SourceSpan:
    """Half-open offsets within a named file or inflated byte domain."""

    source_id: str
    byte_domain: str
    start_offset: int
    end_offset: int


class DiagnosticSeverity(str, Enum):
    INFO = "info"
    WARNING = "warning"
    ERROR = "error"


@dataclass(frozen=True, slots=True)
class Diagnostic:
    code: str
    message: str
    severity: DiagnosticSeverity = DiagnosticSeverity.WARNING
    source: SourceSpan | None = None


@dataclass(frozen=True, slots=True)
class RawElement:
    """Opaque bytes; type_id contains exactly 16 original identifier bytes."""

    type_id: bytes
    object_id: int | None = None
    raw_data: bytes = b""
    source: SourceSpan | None = None


@dataclass(frozen=True, slots=True)
class Scene:
    """Validated sources, opaque elements and diagnostics; no JT parsing yet."""

    sources: tuple[SourceMetadata, ...] = ()
    raw_elements: tuple[RawElement, ...] = ()
    diagnostics: tuple[Diagnostic, ...] = ()

    def __post_init__(self) -> None:
        for name in ("sources", "raw_elements", "diagnostics"):
            object.__setattr__(self, name, tuple(getattr(self, name)))
        _native.validate_scene(self)

    def to_native(self) -> _native.NativeScene:
        """Copy values into an immutable Rust-owned snapshot without JSON."""
        return _native.NativeScene.from_model(self)
