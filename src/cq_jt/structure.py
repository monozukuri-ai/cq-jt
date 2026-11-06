"""Immutable JT structure snapshots; source-scoped IDs, no geometry buffers."""
from __future__ import annotations
from dataclasses import asdict, dataclass
from . import _native
from .model import Diagnostic, SourceMetadata, SourceSpan

STRUCTURE_API_VERSION = 3
STRUCTURE_JSON_SCHEMA_VERSION = 2
if getattr(_native, "STRUCTURE_API_VERSION", None) != STRUCTURE_API_VERSION:
    raise ImportError("cq-jt requires native structure API 3; install a matching wheel or rebuild the extension")

@dataclass(frozen=True, slots=True)
class StructureOptions:
    mode: str = "strict"
    max_nodes: int = 1_000_000
    max_instances: int = 1_000_000
    max_depth: int = 256
    max_references: int = 4_000_000
    max_string_bytes: int = 1_048_576
    experimental_jt106: bool = False

@dataclass(frozen=True, slots=True)
class LateLoadedReference:
    segment_guid: bytes
    segment_type: int
    payload_object_id: int
    reserved: int | None

PropertyValue = str | int | float | tuple[int, ...] | tuple[float, ...] | LateLoadedReference | None

@dataclass(frozen=True, slots=True)
class PropertyAtom:
    object_id: int
    type_id: bytes
    state_flags: int | None
    kind: str
    value: PropertyValue
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class PropertyBinding:
    owner_object_id: int
    key_atom_id: int
    value_atom_id: int
    key: str | None
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class ShapeMetadata:
    transformed_bounds: tuple[float, ...] | None
    local_bounds: tuple[float, ...]
    area: float
    vertex_count_range: tuple[int, int]
    node_count_range: tuple[int, int]
    polygon_count_range: tuple[int, int]
    size: int
    compression_level: float
    vertex_version: int
    vertex_binding: int
    quantization: tuple[int, int, int, int] | None
    vertex_binding2: int | None

@dataclass(frozen=True, slots=True)
class GraphNode:
    id: int
    object_id: int
    kind: str
    flags: int
    attribute_ids: tuple[int, ...]
    child_ids: tuple[int, ...]
    name: str | None
    measurement_units: str | None
    file_name: str | None
    partition_flags: int | None
    reserved: int | None
    lod_ranges: tuple[float, ...]
    lod_center: tuple[float, float, float] | None
    selected_child: int | None
    shape: ShapeMetadata | None
    payload_references: tuple[LateLoadedReference, ...]
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class Material:
    id: int
    object_id: int
    version: int
    state_flags: int
    inhibit_flags: int
    data_flags: int
    ambient: tuple[float, float, float, float]
    diffuse: tuple[float, float, float, float]
    specular: tuple[float, float, float, float]
    emission: tuple[float, float, float, float]
    shininess: float
    reflectivity: float | None
    bumpiness: float | None
    field_final_flags: int | None
    palette_index: int | None
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class Attribute:
    id: int
    object_id: int
    state_flags: int
    inhibit_flags: int
    field_final_flags: int | None
    palette_index: int | None
    transform: tuple[float, ...] | None
    material_id: int | None
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class EffectiveMaterial:
    ambient: tuple[float, float, float, float] | None
    diffuse_rgb: tuple[float, float, float] | None
    diffuse_alpha: float | None
    specular: tuple[float, float, float, float] | None
    emission: tuple[float, float, float, float] | None
    blending: bool | None
    override_vertex_colors: bool | None
    origins: tuple[int | None, ...]
    origin_sources: tuple[str | None, ...]

@dataclass(frozen=True, slots=True)
class PartDefinition:
    id: int
    node_id: int
    name: str | None
    shape_node_ids: tuple[int, ...]
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class Instance:
    id: int
    node_id: int
    parent_id: int | None
    part_id: int | None
    local_transform: tuple[float, ...]
    world_transform: tuple[float, ...]
    unit_node_id: int | None
    meters_per_unit: float | None
    source_meters_per_unit: float | None
    material: EffectiveMaterial
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class MissingReference:
    owner_object_id: int
    target: str
    kind: str
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class OpaqueObject:
    object_id: int
    type_id: bytes
    element_index: int
    source: SourceSpan

@dataclass(frozen=True, slots=True)
class StructureScene:
    source: SourceMetadata
    sources: tuple[SourceMetadata, ...]
    coordinates_in_meters: bool
    blocked_node_ids: tuple[int, ...]
    lsg_segment_index: int
    lsg_guid: bytes
    root_node_id: int
    nodes: tuple[GraphNode, ...]
    attributes: tuple[Attribute, ...]
    materials: tuple[Material, ...]
    atoms: tuple[PropertyAtom, ...]
    bindings: tuple[PropertyBinding, ...]
    parts: tuple[PartDefinition, ...]
    instances: tuple[Instance, ...]
    opaque_objects: tuple[OpaqueObject, ...]
    missing_references: tuple[MissingReference, ...]
    diagnostics: tuple[Diagnostic, ...]
    complete: bool

    @property
    def part_instances(self) -> tuple[Instance, ...]:
        return tuple(i for i in self.instances if i.part_id is not None)

    def properties(self, object_id: int, *, source_id: str | None = None) -> tuple[tuple[str, PropertyValue], ...]:
        """Resolved LSG properties, retaining duplicate spellings and order."""
        sources={b.source.source_id for b in self.bindings if b.owner_object_id==object_id}
        if source_id is None:
            if len(sources)>1: raise ValueError("object ID occurs in multiple sources; specify source_id")
            source_id=next(iter(sources),self.source.source_id)
        atoms = {a.object_id: a for a in self.atoms if a.source.source_id==source_id}
        return tuple((b.key, atoms[b.value_atom_id].value) for b in self.bindings
                     if b.source.source_id==source_id and b.owner_object_id == object_id and b.key is not None and b.value_atom_id in atoms)

    def to_dict(self) -> dict:
        """Structure JSON 2; every bytes/GUID value is raw-order hexadecimal."""
        def normalize(value):
            if isinstance(value, bytes): return value.hex()
            if isinstance(value, (list, tuple)): return [normalize(v) for v in value]
            if isinstance(value, dict): return {k: normalize(v) for k, v in value.items()}
            return value
        version=self.source.version
        profile="structure-assembly" if self.coordinates_in_meters else f"structure-{version.major}.{version.minor}"
        if any(s.version is not None and (s.version.major,s.version.minor)==(10,6) for s in self.sources):
            profile += "-experimental"
        return {"schema_version": STRUCTURE_JSON_SCHEMA_VERSION, "profile": profile,
                "geometry_decoded": False, **normalize(asdict(self))}
