"""GLB export options and reports; encoding is implemented by the Rust core."""
from __future__ import annotations
from dataclasses import asdict, dataclass
from .model import Diagnostic

EXPORT_API_VERSION = 1
EXPORT_JSON_SCHEMA_VERSION = 1


@dataclass(frozen=True, slots=True)
class ExportOptions:
    allow_partial: bool = False
    meters_per_unit: float | None = None
    default_color: tuple[float, float, float, float] | None = None
    color_space: str = "linear"
    up_axis: str = "preserve"
    max_position_error_meters: float = 1e-6
    max_output_bytes: int = 512 * 1024 * 1024
    max_json_bytes: int = 64 * 1024 * 1024
    max_vertices: int = 10_000_000
    max_nodes: int = 1_000_000
    max_materials: int = 100_000
    max_mesh_variants: int = 100_000


@dataclass(frozen=True, slots=True)
class ExportReport:
    source_complete: bool
    byte_length: int
    node_count: int
    geometry_count: int
    mesh_count: int
    material_count: int
    instance_count: int
    vertex_count: int
    triangle_count: int
    placed_triangle_count: int
    meters_per_unit: float
    unit_fallback_used: bool
    color_space: str
    up_axis: str
    max_position_error_meters: float
    normalized_normal_count: int
    original_degenerate_triangles: int
    omitted_empty_meshes: tuple[int, ...]
    diagnostics: tuple[Diagnostic, ...]

    def to_dict(self) -> dict:
        return {"schema_version": EXPORT_JSON_SCHEMA_VERSION, "format": "glb-2.0", **asdict(self)}


def _check_native_api() -> None:
    from . import _native
    version = getattr(_native, "EXPORT_API_VERSION", None)
    if type(version) is not int or version != EXPORT_API_VERSION:
        raise ImportError("native GLB export requires EXPORT_API_VERSION == 1; rebuild the extension")


_check_native_api()
