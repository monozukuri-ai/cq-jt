# Compatibility and limitations

Support is defined by the JT version, local element versions, compression and shape representation together. Recognizing a container does not establish that its geometry can be read.

## Versions and data

| Operation | Accepted versions | Scope |
| --- | --- | --- |
| Container inspection | 8.0–8.2, 9.0–9.5, 10.0–10.6 | Header, table of contents, segment ranges and framing |
| Outer decoding | Accepted container versions | Bounded ZLIB or XZ/LZMA2 streams and logical-element boundaries |
| Structure and triangle meshes | 9.5, 10.0, 10.3 | The limited display profiles described below |
| Experimental structure and meshes | 10.6, explicit opt-in | Specification-derived controls; native export compatibility is unverified |
| GLB export | Successfully read mesh scenes | Selected LOD, placements, units, approximate materials and shared geometry |

Both byte orders have authored tests. Real big-endian file compatibility has not been established. Not every accepted container version or element variant has a real-file compatibility check.

The normal display profile includes Base, Group, Partition, Instance, Part, Metadata, LOD, RangeLOD, Switch and TriStripShape nodes, geometric transforms and materials. Meshes use three-component positions, optional normals and vertex flags, separate position/normal indices, supported integer CODECs, quantization and topological reconstruction. JT 9.5 supports the bounded TopoMesh local-version 1/2 profile; 10.0/10.3 support local version 1.

Unsupported data includes UVs, vertex colors, textures, auxiliary vertex fields, lines, points, per-face export materials, nonempty attribute palettes, PMI interpretation, precise B-Rep geometry and late-loaded metadata interpretation. Multiple shape elements inside a payload directory are outside the current mesh profile.

Unknown data and source spans remain available through decoded documents where possible. Unsupported required display data fails in strict mode. Best-effort mode reports omissions and sets `complete=False`. Corruption, cycles, hash failures and resource-limit violations remain fatal.

## Experimental JT 10.6

JT 10.6 remains disabled by default. To enable the limited experimental reader:

```python
import cq_jt as jt

options = jt.ReadOptions(
    structure=jt.StructureOptions(experimental_jt106=True),
)
scene = jt.read("input106.jt", options=options)
```

```bash
python -m cq_jt convert input106.jt preview.glb --experimental-jt106 --json
```

The profile uses one LSG element list, Partition nodes with an explicit untransformed bounding box and flags=1, supported structure/material elements, and TopoMesh local version 1. Nonquantized position and normal hashes use the scalar update rule from the JT 10.6 Rev D reference. It does not enable 10.5 or other versions, guess another layout, or retry another hash rule after a mismatch.

Each experimental source emits `unverified_jt106_profile`. JSON profile names end with `-experimental`; GLB includes `asset.extras.jt.unverifiedProfiles`, and export reports preserve the warning. `complete=True` describes the requested display profile's completeness, not verified compatibility with native JT 10.6 exports.

## Interpretation boundaries

- LOD selection is static. RangeLOD `highest` selects the first near-distance child and `lowest` the last far-distance child; these names do not compare triangle counts.
- Assemblies with resolved external references normalize mesh coordinates and placement translations to metres. A single file without external references retains its source coordinates and units.
- Units, up direction and color space are not inferred. Explicit supplements fill missing information without replacing known units or colors.
- Materials are approximated as nonmetallic PBR for GLB. Stored legacy fields remain in extras, but matching the original CAD renderer is not guaranteed.
- Mesh export does not produce precise solids, native editing history, semantic PMI or manufacturing-ready geometry.

## Runtime platforms

Python requires CPython 3.11 or later. Wheels use the CPython 3.11 stable ABI. Local verification covers Linux x86_64 on CPython 3.11–3.14. Windows, macOS, Linux arm64 and older glibc environments are not yet verified. A wheel's ABI or platform tag alone does not establish testing on every matching system.
