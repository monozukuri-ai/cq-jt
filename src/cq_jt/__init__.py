"""Rust JT container inspection and owned scene model foundation."""
from importlib import import_module as _import_module

try:
    _native = _import_module("cq_jt._native")
except ModuleNotFoundError as error:
    if error.name != "cq_jt._native":
        raise
    raise ImportError(
        "cq-jt requires its Rust extension. Install a built wheel, or run "
        "`uv sync --group dev` from the source checkout."
    ) from error

from .errors import JtError, JtModelError, JtParseError, JtLimitError, JtUnsupportedError, JtReferenceError
from .model import (
    MODEL_API_VERSION, Diagnostic, DiagnosticSeverity, JtVersion, RawElement,
    Scene, SourceMetadata, SourceSpan,
)
from .container import (
    INSPECT_API_VERSION, INSPECT_JSON_SCHEMA_VERSION, ByteOrder,
    CompressionHeader, ContainerInfo, ReadLimits, SegmentInfo,
)
from .payload import PAYLOAD_API_VERSION, PAYLOAD_JSON_SCHEMA_VERSION, DecodedSegmentInfo, LogicalElement
from .api import decode_bytes, decode_file, inspect_bytes, inspect_file, validate_scene
from .structure import (
    STRUCTURE_API_VERSION, STRUCTURE_JSON_SCHEMA_VERSION, StructureOptions, StructureScene,
    GraphNode, PartDefinition, Instance, Attribute, Material, EffectiveMaterial,
    PropertyAtom, PropertyBinding, LateLoadedReference, ShapeMetadata, MissingReference, OpaqueObject,
)
from .api import read_structure_bytes, read_structure_file
from .mesh import (MESH_API_VERSION, MESH_JSON_SCHEMA_VERSION, MeshOptions, ReadOptions, ResolverOptions,
                   MeshInfo, MeshInstance, LodChoice, PackedArray, MeshBuffers, MeshScene)
from .api import read, read_file, read_bytes
from .export import EXPORT_API_VERSION, EXPORT_JSON_SCHEMA_VERSION, ExportOptions, ExportReport
from .api import export_glb, export_glb_bytes, convert
from .errors import JtExportError

__version__ = _native.CORE_VERSION
NativeScene = _native.NativeScene
NativeDocument = _native.NativeDocument
NativeStructure = _native.NativeStructure
NativeMeshScene = _native.NativeMeshScene

__all__ = [
    "MODEL_API_VERSION", "Diagnostic", "DiagnosticSeverity", "JtError",
    "JtModelError", "JtVersion", "NativeScene", "RawElement", "Scene",
    "SourceMetadata", "SourceSpan", "validate_scene",
    "INSPECT_API_VERSION", "INSPECT_JSON_SCHEMA_VERSION", "ByteOrder",
    "CompressionHeader", "ContainerInfo", "ReadLimits", "SegmentInfo",
    "JtParseError", "JtLimitError", "JtUnsupportedError", "inspect_bytes", "inspect_file",
    "PAYLOAD_API_VERSION", "PAYLOAD_JSON_SCHEMA_VERSION", "DecodedSegmentInfo",
    "LogicalElement", "NativeDocument", "decode_bytes", "decode_file",
    "STRUCTURE_API_VERSION", "STRUCTURE_JSON_SCHEMA_VERSION", "StructureOptions", "StructureScene",
    "NativeStructure", "GraphNode", "PartDefinition", "Instance", "Attribute", "Material", "EffectiveMaterial",
    "PropertyAtom", "PropertyBinding", "LateLoadedReference", "ShapeMetadata", "MissingReference", "OpaqueObject",
    "JtReferenceError", "read_structure_bytes", "read_structure_file",
    "MESH_API_VERSION", "MESH_JSON_SCHEMA_VERSION", "MeshOptions", "ReadOptions", "ResolverOptions",
    "MeshInfo", "MeshInstance", "LodChoice", "PackedArray", "MeshBuffers", "MeshScene",
    "NativeMeshScene", "read", "read_file", "read_bytes",
    "EXPORT_API_VERSION", "EXPORT_JSON_SCHEMA_VERSION", "ExportOptions", "ExportReport",
    "JtExportError", "export_glb", "export_glb_bytes", "convert",
]
