# API guide

cq-jt performs JT container inspection, bounded outer decompression, structure and triangle-mesh reading, and GLB export in Rust. Python provides typed immutable snapshots and explicit array copies.

See [compatibility](compatibility.md) for supported profiles. The APIs below expose different levels of interpretation; container inspection and outer decompression do not establish that geometry is supported.

## Read and export

```python
from pathlib import Path
import cq_jt as jt

scene = jt.read(Path("assembly.jt"), options=jt.ReadOptions(lod="highest"))
print(scene.complete, scene.diagnostics)
report = jt.export_glb(scene, Path("assembly.glb"))
print(report.instance_count, report.placed_triangle_count)

# Return a GLB copy instead of writing a file:
data, report = jt.export_glb_bytes(scene)

# Combine reading and file export:
report = jt.convert("assembly.jt", "converted.glb",
                    read_options=jt.ReadOptions(lod="lowest"),
                    export_options=jt.ExportOptions(up_axis="z"))
```

`read` aliases `read_file`. File APIs accept `str` or a `PathLike` returning `str`. `read_bytes(data, source_id="upload.jt", options=...)` accepts bytes and performs no external file reads. File-based source IDs default to the input path; bytes-based source IDs default to `"input"`. Empty source IDs are rejected.

`ReadOptions` contains `lod`, `structure: StructureOptions`, `mesh: MeshOptions` and `resolver: ResolverOptions`. `ReadLimits` is supplied separately as `limits=...`.

`NativeMeshScene` owns immutable Rust meshes and retains decoded source buffers. It exposes `structure`, lightweight `meshes` metadata, selected mesh `instances`, `lod_choices`, `diagnostics` and `complete`. Numerical arrays are not automatically converted to Python objects. `to_model()` returns a frozen metadata-only `MeshScene` snapshot; it is not an editable geometry model or a round trip back into Rust.

`geometry_decoded=True` means the requested mesh profile ran. Check `complete` and diagnostics for omissions. This does not describe unselected LODs, precise geometry or compatibility beyond the [supported profile](compatibility.md).

## Mesh arrays and LOD

```python
for placement in scene.instances:
    mesh = scene.meshes[placement.mesh_id]
    state = scene.structure.instances[placement.structure_instance_id]
    print(mesh.triangle_count, state.world_transform, state.material)

buffers = scene.mesh_buffers(0)
print(buffers.positions.dtype, buffers.positions.shape)
```

`mesh_buffers(mesh_id)` explicitly copies packed bytes. Each `PackedArray` has `data: bytes`, `dtype: str` and `shape: tuple[int, int]`. Arrays are little-endian and C-contiguous. `view()` returns a read-only byte memoryview, not a typed numerical array.

| Array | dtype | Shape | Meaning |
| --- | --- | --- | --- |
| positions | f64 | (vertices, 3) | Source coordinates for a single file; metres for resolved assemblies; placements are not baked in |
| triangles | u32 | (triangles, 3) | Position indices with original winding |
| normals | f64 | (normal records, 3) | Stored or decoded normal directions |
| normal_indices | u32 | (triangles, 3) | Independent corner-normal indices; None when normals are absent |
| face_groups | i32 | (triangles, 1) | Stored face-group numbers; precise B-Rep associations are not interpreted |
| vertex_flags | u32 | (attribute records, 1) | Stored flags, or an empty (0, 1) array |

Position and normal indices are independent. For a single-index graphics API, combine each `(position_index, normal_index)` pair into a vertex to preserve sharp edges. Stored normals are checked for finite values and nonzero referenced vectors, but are not rewritten to unit length. Normalize for display and use the inverse transpose for nonuniform placement scales.

Quantized coordinates use `min + code * (max-min)/(2**bits-1)`. `quantization_step` records the source step size. Unquantized f32 coordinates are widened to f64. Original LSG polygon counts may differ from reconstructed triangle counts; discrepancies remain diagnostics rather than replacing the stored metadata.

NumPy is optional. If installed, `from cq_jt.arrays import as_numpy` and `as_numpy(buffers.positions)` create a read-only view over the copied packed bytes.

RangeLOD `highest` selects its first near-distance child; `lowest` selects its last far-distance child. Ordinary unordered LOD requires an explicit child index, such as `lod=0`. Indices refer to each reached LOD's original child list. Invalid indices fail. Selection follows LSG references rather than segment order, and does not display all LODs at once. Switch and Ignore state also affect reachable placements.

## External references and units

```python
options = jt.ReadOptions(resolver=jt.ResolverOptions(root=Path("cad")))
scene = jt.read("cad/assembly.jt", options=options)
print(scene.structure.sources, scene.structure.coordinates_in_meters)

# Supply missing assembly units at read time:
options = jt.ReadOptions(resolver=jt.ResolverOptions(meters_per_unit=0.001))

# Read a single file without resolving external Partitions:
single = jt.ReadOptions(resolver=jt.ResolverOptions(enabled=False))
```

`read_file`, `read` and `convert` resolve leaf Partition filenames reached through selected LOD paths. Paths are relative to the referring file. The default allowed root is the input's parent directory. Canonicalized paths must remain within that root; absolute paths, URLs, drive names, escaping symlinks and reference cycles are rejected. Resolution does not fetch network resources. Path validation is not an operating-system sandbox.

Each canonical source is read once and its definitions and geometry are shared. Original object IDs remain scoped to source IDs. The Rust API also exposes `mesh::resolver::SourceResolver` and `read_resolved_bytes` for explicitly supplied resolution. Python callbacks are not used during Rust IO.

`read_bytes`, `read_structure_file` and `read_structure_bytes` do not open external references. Strict mode rejects unresolved required references; best-effort mode returns explicit omissions.

A resolved assembly has `structure.coordinates_in_meters=True`. Mesh positions, bounds, quantization steps and instance local/world translations are normalized to metres; rotations, scales, hierarchy and geometry sharing are retained. `Instance.meters_per_unit=1.0`, while `source_meters_per_unit` retains the effective source conversion factor. Source metadata, stored transform attributes, `GraphNode.shape` bounds and property values remain in source units.

At a file boundary, units are resolved from that source rather than guessed from its parent file. Geometry or nonzero translations with missing units require `ResolverOptions.meters_per_unit`. Known units are never replaced. Mixed units within one source path remain unsupported. A single file without external references retains its source coordinates and units.

`ExportOptions.meters_per_unit` cannot repair missing units that prevented assembly composition during reading.

| ResolverOptions field | Default | Meaning |
| --- | --- | --- |
| enabled | True | Resolve local external files during file-based reads |
| root | None | Use the input's parent directory unless overridden |
| max_files | 64 | Unique sources, including the root |
| max_depth | 32 | External-reference depth, including reused subtrees; maximum setting 64 |
| max_references | 4,096 | Reached external references |
| max_total_input_bytes | 512 MiB | Input bytes across unique sources |
| meters_per_unit | None | Supplement missing assembly units |

Decoding, node/reference and mesh/CODEC budgets are shared across unique assembly sources. Final instance expansion is also bounded. Missing files and unsupported profiles can produce partial results in best-effort mode; malformed data, cycles, path escapes and exceeded limits remain fatal.

## GLB export

`export_glb_bytes(scene, options=...)` returns `(bytes, ExportReport)`. `export_glb` and `convert` write a file and return the report. Mesh decoding and GLB encoding remain in Rust; CPU work and file IO release the GIL.

Existing output files are rejected by default. Set `overwrite=True` on `export_glb` or `convert` to permit replacement. Output is validated and fully built before writing a temporary file in the destination directory and committing it. Validation failures preserve existing files. Parent directories are not automatically created.

### Missing information and coordinate policy

```python
options = jt.ExportOptions(
    meters_per_unit=0.001,              # Supplement missing units only
    default_color=(0.7, 0.7, 0.7, 1.0), # Supplement missing RGB/alpha only
    up_axis="z",                       # Only when the input is known to be Z-up
    color_space="linear",
)
jt.export_glb(scene, "preview.glb", options=options)
```

Coordinates and translations are converted to metres. Unknown geometry units or unknown units on a nonzero ancestor translation require an explicit factor. Zero-translation ancestors do not require units for rotation/scale alone. Known units and colors are retained. Supplements are reported and do not modify the original scene.

`up_axis="preserve"` is the default. `"y"` explicitly declares Y-up without changing coordinates. `"z"` adds a root transform mapping `(x,y,z)` to `(x,z,-y)`. The reader does not infer up direction.

Partial scenes are rejected unless `allow_partial=True`. For partial reads, also request `ReadOptions(structure=StructureOptions(mode="best_effort"))`. Reports and `asset.extras.jt.sourceComplete` retain False. Empty meshes are recorded and omitted; an output with no display triangles fails.

### Geometry and materials

Only selected LOD meshes and necessary ancestors are exported. Local transform hierarchies are preserved. Identical geometry buffers are shared; different placement materials can create separate glTF mesh variants without duplicating geometry buffers. Each local transform must be decomposable into translation, rotation and scale. Shear arising from a scaled ancestor and rotated child is preserved through hierarchy. Mirrored scales are retained without reversing winding twice.

Position/normal index pairs are unified. Copied normals are normalized; stored values remain unchanged. If normals are absent, NORMAL is omitted and glTF flat-shading behavior applies. Buffers use little-endian f32 positions/normals and u32 indices; POSITION bounds are calculated from encoded values.

`color_space="linear"` treats RGB values as linear; `"srgb"` converts RGB to linear. This is caller policy, not automatic detection. Alpha is not color-space converted. Diffuse maps to baseColor and emission to emissive, with metallic=0 and roughness=1. Stored blending takes precedence; if absent, alpha determines BLEND/OPAQUE. `doubleSided` is False. Original legacy material values and source-scoped IDs remain in extras, but matching the CAD shader is not guaranteed. GLB extras contain source ordinals, not input file paths.

### Precision and report

`max_position_error_meters=1e-6` bounds each local position component's f32 encoding error after metre conversion. The report's value is the measured maximum. Nonfinite values, zero normals, invalid indices, overflow, excess error and triangles newly collapsed by f32 encoding are rejected. Originally degenerate triangles are retained and counted. Matrices retain JSON f64 values with additional f32 range checks. This is not a world-coordinate or GPU precision guarantee; ancestor scales can amplify errors.

| ExportOptions limit | Default |
| --- | --- |
| max_output_bytes | 512 MiB |
| max_json_bytes | 64 MiB |
| max_vertices | 10,000,000 unified shared vertices |
| max_nodes | 1,000,000 including required ancestors |
| max_materials | 100,000 |
| max_mesh_variants | 100,000 geometry/material combinations |

The report includes completeness, output byte length, node/geometry/mesh/material/instance counts, units, supplements, coordinate/color policy, measured encoding error, normalized-normal and degenerate-triangle counts, omitted empty meshes and diagnostics. `geometry_count` counts shared shapes, `mesh_count` counts material variants, `triangle_count` counts shared geometry once, and `placed_triangle_count` includes repeated placements.

## Structure-only reading

```python
structure = jt.read_structure_file("part.jt")
# structure = jt.read_structure_bytes(data, source_id="upload.jt")
for part in structure.parts:
    print(part.id, part.name, part.shape_node_ids)
for placement in structure.part_instances:
    print(placement.part_id, placement.parent_id, placement.world_transform)
model = structure.to_model()
```

`NativeStructure` owns immutable structure and shared decoded documents. It does not decode meshes or resolve external files by itself. `to_model()` explicitly copies a frozen `StructureScene`; writing a structure snapshot back to Rust is not supported.

| Property | Meaning |
| --- | --- |
| source / sources | Root/all-source metadata, JT versions and explicitly declared root units |
| coordinates_in_meters | Whether resolved assembly geometry/translations were normalized |
| nodes | Shared node definitions, local IDs, original IDs, children/attributes, names, units and shape references |
| attributes / materials | Transform/material definitions and original flags/values |
| atoms / bindings | LSG property values and source-scoped owner/key/value references |
| parts | Shared part definitions and shape-node IDs |
| instances | Per-path occurrences, including hierarchy and shape nodes |
| part_instances | Only occurrences with a part ID |
| document / documents | Root decoded document and tuple of all shared source documents |
| diagnostics / missing_references / opaque_objects | Explicit unsupported, missing and uninterpreted data with sources |
| complete | Whether this requested structure profile has omissions |
| geometry_decoded | Always False for structure-only snapshots |

Local IDs index their respective arrays. Original JT object IDs are only unique within a source. Instance `parent_id`, `node_id` and `part_id` refer to instance, node and part arrays respectively.

Names use `JT_PROP_NAME`, then `Name::`, with filename as a Partition fallback. Missing names are None. `model.properties(object_id, source_id=...)` returns resolved `(key, value)` pairs in order. The source argument is required when the owner ID occurs in multiple sources.

Property atoms support strings, i32, f32, six-component dates, Vector4 and late-loaded references. Strings follow file byte order. Dates do not acquire a guessed time zone. Unsupported optional date-v3 data is retained as opaque metadata with an information diagnostic. Late-loaded references retain GUID/type/payload ID and optional reserved value; payload IDs are distinct from logical object IDs.

Transforms are row-major f64 matrices operating on column vectors: `world = parent_world * local`. JT row-vector matrices are transposed. Attributes accumulate in order. Nonfinite, projective, singular and individually sheared transforms fail. `JT_PROP_MEASUREMENT_UNITS` is inherited along a source path; missing/unknown units are None, not assumed millimetres.

Materials inherit per path without sharing sibling state. Final/Force/Ignore, version-specific inhibits and field-final flags determine effective values. `EffectiveMaterial.origins` and `origin_sources` retain the original object/source for each material field. Missing colors are not filled during reading. Metadata absent in a file layout, such as modern `ShapeMetadata.transformed_bounds` and `quantization`, remains None.

LOD alternatives remain explicit in the structure snapshot. Mesh reading selects reachable alternatives separately.

## Container inspection and outer decoding

```python
info = jt.inspect_file("part.jt")
# info = jt.inspect_bytes(data, source_id="upload.jt")
print(info.version, info.byte_order, info.segment_type_counts)
for segment in info.segments:
    print(segment.guid_raw.hex(), segment.segment_type, segment.offset, segment.length)
assert info.payload_decoded is False

document = jt.decode_file("part.jt")
# document = jt.decode_bytes(data, source_id="upload.jt")
raw = document.raw_element(0, 0)
assert raw.raw_data == document.element_bytes(0, 0)
assert document.geometry_decoded is False
```

`inspect_*` validates header/TOC/segment ranges, identities, overlaps, lengths, compression framing and the LSG root. It retains unknown segment types without interpreting their bodies. `ContainerInfo` is a frozen dataclass. GUIDs remain 16 bytes in their original file order. Segment length includes its 24-byte header. Inspection does not decompress data or validate geometry.

`decode_*` returns a Rust-owned `NativeDocument` with bounded ZLIB/XZ output. Its metadata includes `container`, `segments`, `total_output_bytes` and `diagnostics`. `segment_bytes`, `element_bytes` and `raw_element` explicitly copy selected data to Python. Indices are nonnegative integers; out-of-range indices raise IndexError, and bool/float indices raise TypeError. Returned Python values survive destruction of the native owner.

Logical elements retain declared length, GUID, base type, object ID, list index, end-marker status and source/data spans. End markers have an all-FF GUID. JT 8.x object IDs remain None at this layer because they are stored in type-specific bodies. Unknown element bodies and segment tails remain opaque; `elements_framed=False` indicates that element framing was not interpreted.

The decoder uses two LSG lists below 10.5 and one from 10.5 onward. Other accepted container layouts and unverified writer variants may expose opaque tails; outer decoding does not imply complete scene interpretation.

ZLIB requires its wrapper and checksum. XZ supports one LZMA2 filter per block, one stream, multiple blocks and the supported None/CRC32/CRC64/SHA-256 checks. Filter chains, raw LZMA, concatenated streams and extra bytes outside declared frames are rejected.

## Limits, errors and ownership

Limits apply per call and are admission limits, not guarantees of maximum process RSS. Temporary decoder state, retained documents and explicit Python copies also consume memory. A zero limit rejects applicable nonempty data. Integer limits reject bool, float, negatives and values beyond their declared unsigned width.

| Options | Default limits |
| --- | --- |
| ReadLimits | Input 512 MiB; segments 1,000,000; output/segment 256 MiB; total output 512 MiB; LZMA dictionary 64 MiB; logical elements 1,000,000; XZ blocks/stream 65,536 |
| StructureOptions | Nodes 1,000,000; instances 1,000,000; graph depth 256; references 4,000,000; stored UTF-16 bytes/string 1 MiB |
| MeshOptions | Shared meshes 100,000; vertices 10,000,000; triangles 10,000,000; mesh buffers 512 MiB; CODEC values 64,000,000; packets 1,000,000; recursion 16 (maximum setting 64) |
| Per-mesh topology | Edges 60,000,000; work 200,000,000 |

CODEC recursion limits can be tightened by the caller. Mesh/CODEC totals count shared geometry once, while placement expansion is separately bounded. Outer segment decoding occurs before selected shape interpretation; this is not a lazy LSG-only loader.

Strict mode is the default. `StructureOptions(mode="best_effort")` can report partial data for missing references or unsupported required display data. It does not suppress malformed input, cycles, invalid transforms, hash mismatches or exceeded limits.

| Exception | Meaning |
| --- | --- |
| JtParseError | Invalid format or malformed data |
| JtLimitError | Exceeded reader limits; derives from JtParseError |
| JtUnsupportedError | Unsupported version or encoding |
| JtReferenceError | Missing required reference |
| JtExportError | Invalid/unsupported output or exceeded export limits |
| JtModelError | Invalid generic model metadata |
| OSError subclasses | File IO errors |
| TypeError / ValueError | Invalid argument types or ranges |

JT errors expose `code`, `source` and `offset`. Source spans are half-open ranges within a named byte domain. `file` means original-file offsets; `segment/<raw-guid-hex>/inflated` means offsets in an expanded segment. These domains must not be confused. Parse work and IO run without the Python GIL; conversion to typed Python values occurs at the binding boundary.

## Generic opaque model

This model validates metadata and preserves bytes; constructing one does not parse JT geometry.

```python
from cq_jt import JtVersion, RawElement, Scene, SourceMetadata, SourceSpan

span = SourceSpan("part", "segment/example/inflated", 0, 3)
model = Scene(
    sources=(SourceMetadata("part", "part.jt", JtVersion(9, 5)),),
    raw_elements=(RawElement(bytes(16), raw_data=b"\x00\xff\x00", source=span),),
)
native = model.to_native()
assert native.to_model() == model
assert native.sources[0].meters_per_unit is None
```

`Scene` construction, `validate_scene(model)` and `NativeScene.from_model(model)` validate source identities, spans and metadata in Rust. Leaf dataclasses alone do not perform this validation. Python/native snapshots do not retain mutable references to each other's data, and conversion does not use JSON.

## CLI and schema versions

```bash
python -m cq_jt inspect part.jt --json
python -m cq_jt decode part.jt --json --max-total-output-bytes 67108864
python -m cq_jt mesh assembly.jt --lod 1 --external-max-files 32
python -m cq_jt convert assembly.jt preview.glb --resolve-root cad --json
python -m cq_jt convert assembly.jt preview.glb --assembly-meters-per-unit .001
python -m cq_jt mesh part.jt --no-resolve
```

CLI exit codes are 0 for a successful report (including reported partial results), 1 for format/IO/export errors, and 2 for argument errors. JSON output includes metadata and diagnostics, not numerical mesh buffers. Error JSON includes `error.code`, `message`, `source` and `offset`. Always check `complete`; exit 0 alone does not mean a complete scene.

`--max-export-vertices` and `--max-export-nodes` control export limits independently from read limits. Other policies include `--overwrite`, `--allow-partial`, `--color-space`, `--up-axis` and `--experimental-jt106`. Use subcommand `--help` for all limits.

| Interface | API version | JSON schema version |
| --- | --- | --- |
| Generic model | 1 | — |
| Container inspection | 2 | 1 |
| Outer payload decoding | 1 | 1 |
| Structure | 3 | 2 |
| Mesh | 2 | 2 |
| GLB export report | 1 | 1 |

Package, JT format, API and JSON schema versions are independent. Mismatched Python/native API versions raise ImportError. Structure/mesh JSON profile names identify the source version or assembly, with `-experimental` for any scene containing experimental JT 10.6 data.

## Experimental reading

See [experimental JT 10.6](compatibility.md#experimental-jt-106) before setting `StructureOptions.experimental_jt106=True`. Real native-export compatibility remains unverified. Experimental warnings survive structure composition and GLB export; successful parsing or `complete=True` does not change that status.
