"""Public operations backed by the Rust core."""
from __future__ import annotations

import os
from . import _native
from .model import Scene
from .container import ContainerInfo, ReadLimits
from .structure import StructureOptions
from .mesh import ReadOptions
from .export import ExportOptions, ExportReport


def validate_scene(scene: Scene) -> None:
    """Validate a model through Rust, including source-reference closure."""
    _native.validate_scene(scene)


def inspect_bytes(data: bytes, *, source_id: str = "input", limits: ReadLimits | None = None) -> ContainerInfo:
    """Inspect header/TOC/segment framing in Rust without decoding payloads."""
    return _native.inspect_bytes(data, source_id=source_id, limits=limits)


def inspect_file(path: str | os.PathLike[str], *, source_id: str | None = None, limits: ReadLimits | None = None) -> ContainerInfo:
    """Read within Rust's input limit, then inspect container framing."""
    path = os.fspath(path)
    if not isinstance(path, str):
        raise TypeError("path must resolve to str, not bytes")
    return _native.inspect_file(path, source_id=source_id, limits=limits)


def decode_bytes(data: bytes, *, source_id: str = "input", limits: ReadLimits | None = None) -> _native.NativeDocument:
    """Decode outer streams and frame elements; object payloads stay opaque."""
    return _native.decode_bytes(data, source_id=source_id, limits=limits)


def decode_file(path: str | os.PathLike[str], *, source_id: str | None = None, limits: ReadLimits | None = None) -> _native.NativeDocument:
    """Decode within limits into Rust-owned buffers, without reconstructing geometry."""
    path = os.fspath(path)
    if not isinstance(path, str):
        raise TypeError("path must resolve to str, not bytes")
    return _native.decode_file(path, source_id=source_id, limits=limits)


def read_structure_bytes(data: bytes, *, source_id: str = "input", limits: ReadLimits | None = None,
                         options: StructureOptions | None = None) -> _native.NativeStructure:
    """Read one supported JT structure; meshes and external files stay unread."""
    return _native.read_structure_bytes(data, source_id=source_id, limits=limits, options=options)


def read_structure_file(path: str | os.PathLike[str], *, source_id: str | None = None,
                        limits: ReadLimits | None = None, options: StructureOptions | None = None) -> _native.NativeStructure:
    """Read structure into Rust-owned state without resolving external paths."""
    path = os.fspath(path)
    if not isinstance(path, str):
        raise TypeError("path must resolve to str, not bytes")
    return _native.read_structure_file(path, source_id=source_id, limits=limits, options=options)


def read_bytes(data: bytes, *, source_id: str = "input", limits: ReadLimits | None = None,
               options: ReadOptions | None = None) -> _native.NativeMeshScene:
    """Decode a supported JT mesh profile without opening external files."""
    return _native.read_bytes(data, source_id=source_id, limits=limits, options=options)


def read_file(path: str | os.PathLike[str], *, source_id: str | None = None,
              limits: ReadLimits | None = None, options: ReadOptions | None = None) -> _native.NativeMeshScene:
    """Read hierarchy and one LOD per branch, without resolving external paths."""
    path = os.fspath(path)
    if not isinstance(path, str):
        raise TypeError("path must resolve to str, not bytes")
    return _native.read_file(path, source_id=source_id, limits=limits, options=options)


read = read_file


def export_glb_bytes(scene: _native.NativeMeshScene, *, options: ExportOptions | None = None) -> tuple[bytes, ExportReport]:
    """Return an explicit GLB byte copy and its export report."""
    return _native.export_glb_bytes(scene, options=options)


def export_glb(scene: _native.NativeMeshScene, path: str | os.PathLike[str], *, options: ExportOptions | None = None,
               overwrite: bool = False) -> ExportReport:
    """Build in Rust and atomically write a GLB; existing files require overwrite=True."""
    path = os.fspath(path)
    if not isinstance(path, str):
        raise TypeError("path must resolve to str, not bytes")
    if type(overwrite) is not bool:
        raise TypeError("overwrite must be bool")
    return _native.export_glb(scene, path, options=options, overwrite=overwrite)


def convert(path: str | os.PathLike[str], output: str | os.PathLike[str], *, read_options: ReadOptions | None = None,
            export_options: ExportOptions | None = None, limits: ReadLimits | None = None, overwrite: bool = False) -> ExportReport:
    """Read a JT mesh scene and write GLB through the same public operations."""
    return export_glb(read_file(path, options=read_options, limits=limits), output, options=export_options, overwrite=overwrite)
