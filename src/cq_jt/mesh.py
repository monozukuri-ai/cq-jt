"""Metadata for Rust-owned meshes; packed arrays are copied only on request."""
from __future__ import annotations
from dataclasses import asdict, dataclass, field
import os
from .model import Diagnostic, SourceSpan
from .structure import StructureOptions, StructureScene

MESH_API_VERSION = 2
MESH_JSON_SCHEMA_VERSION = 2


@dataclass(frozen=True, slots=True)
class ResolverOptions:
    enabled: bool = True
    root: str | os.PathLike[str] | None = None
    max_files: int = 64
    max_depth: int = 32
    max_references: int = 4096
    max_total_input_bytes: int = 512 * 1024 * 1024
    meters_per_unit: float | None = None


@dataclass(frozen=True, slots=True)
class MeshOptions:
    max_meshes: int = 100_000
    max_vertices: int = 10_000_000
    max_triangles: int = 10_000_000
    max_mesh_bytes: int = 512 * 1024 * 1024
    max_codec_values: int = 64_000_000
    max_codec_packets: int = 1_000_000
    max_codec_depth: int = 16
    max_topology_edges: int = 60_000_000
    max_topology_work: int = 200_000_000


@dataclass(frozen=True, slots=True)
class ReadOptions:
    lod: str | int = "highest"
    structure: StructureOptions = field(default_factory=StructureOptions)
    mesh: MeshOptions = field(default_factory=MeshOptions)
    resolver: ResolverOptions = field(default_factory=ResolverOptions)


@dataclass(frozen=True, slots=True)
class MeshInfo:
    id: int
    segment_index: int
    element_index: int
    vertex_records_object_id: int
    source: SourceSpan
    vertex_count: int
    triangle_count: int
    normal_count: int
    vertex_flag_count: int
    bounds: tuple[float, ...] | None
    quantization: tuple[int, ...]
    quantization_step: tuple[float, float, float]
    topology_hash: int
    coordinate_hash: int | None
    normal_hash: int | None
    cover_faces: int
    codec_depth: int


@dataclass(frozen=True, slots=True)
class MeshInstance:
    id: int
    structure_instance_id: int
    mesh_id: int


@dataclass(frozen=True, slots=True)
class LodChoice:
    node_id: int
    child_index: int
    child_node_id: int


@dataclass(frozen=True, slots=True)
class PackedArray:
    """An explicit owned copy, little endian; dtype is f64, u32 or i32."""
    data: bytes
    dtype: str
    shape: tuple[int, int]

    def view(self) -> memoryview:
        return memoryview(self.data)


@dataclass(frozen=True, slots=True)
class MeshBuffers:
    positions: PackedArray
    triangles: PackedArray
    normals: PackedArray
    normal_indices: PackedArray | None
    face_groups: PackedArray
    vertex_flags: PackedArray


@dataclass(frozen=True, slots=True)
class MeshScene:
    """Metadata snapshot; use NativeMeshScene.mesh_buffers for numerical data."""
    structure: StructureScene
    meshes: tuple[MeshInfo, ...]
    instances: tuple[MeshInstance, ...]
    lod_choices: tuple[LodChoice, ...]
    diagnostics: tuple[Diagnostic, ...]
    complete: bool

    @property
    def geometry_decoded(self) -> bool:
        return True

    def to_dict(self) -> dict:
        version=self.structure.source.version
        profile="mesh-assembly" if self.structure.coordinates_in_meters else f"mesh-{version.major}.{version.minor}"
        if any(s.version is not None and (s.version.major,s.version.minor)==(10,6) for s in self.structure.sources):
            profile += "-experimental"
        return {"schema_version": MESH_JSON_SCHEMA_VERSION, "profile": profile,
                "geometry_decoded": True, "complete": self.complete,
                "structure": self.structure.to_dict(),
                "meshes": [asdict(m) for m in self.meshes],
                "instances": [asdict(i) for i in self.instances],
                "lod_choices": [asdict(c) for c in self.lod_choices],
                "diagnostics": [asdict(d) for d in self.diagnostics]}


def _check_native_api() -> None:
    from . import _native
    if type(getattr(_native, "MESH_API_VERSION", None)) is not int or _native.MESH_API_VERSION != MESH_API_VERSION:
        raise ImportError("native mesh reading requires MESH_API_VERSION == 2; rebuild the extension")


_check_native_api()
