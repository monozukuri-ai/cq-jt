# cq-jt

A JT reader and GLB exporter with a Python-independent Rust core and thin Python bindings.

Read product structure, part placements, materials and selected triangle meshes from **JT 9.5, 10.0 and 10.3**. Inspect container boundaries, decode ZLIB/XZ streams, resolve local external JT references, and retain source information and opaque data.

JT 10.6 is available as an **explicit experimental opt-in**. Compatibility with native JT 10.6 exports has not been verified. See [compatibility and limitations](https://github.com/monozukuri-ai/cq-jt/blob/main/docs/compatibility.md).

## Requirements and installation

Python 3.11 or later is required. Install from [PyPI](https://pypi.org/project/cq-jt/):

```bash
python -m pip install cq-jt
python -m cq_jt --help
```

Prebuilt wheels are available for Linux x86_64 and ARM64 (glibc 2.35+), Windows x86_64, and macOS Intel and Apple Silicon. Installing a compatible wheel does not require Rust, CadQuery, NumPy, Java, .NET or a CAD application. See [compatibility and limitations](https://github.com/monozukuri-ai/cq-jt/blob/main/docs/compatibility.md) for the supported JT profiles.

If a compatible wheel is unavailable, pip builds from the source distribution, which requires Rust 1.83 or later.

To build from a source checkout, install Rust 1.83 or later and [uv](https://docs.astral.sh/uv/), then run:

```bash
uv sync --locked --group dev
uv run python -m cq_jt --help
```

## Quick start

```python
import cq_jt as jt

scene = jt.read("assembly.jt")
print(scene.complete, len(scene.meshes), len(scene.instances))

buffers = scene.mesh_buffers(0)  # Explicit packed-array copies
print(buffers.positions.dtype, buffers.positions.shape)
print(buffers.triangles.dtype, buffers.triangles.shape)

report = jt.export_glb(scene, "assembly.glb")
print(report.instance_count, report.placed_triangle_count)

# Read and export in one call:
# report = jt.convert("assembly.jt", "another.glb")
```

File reads resolve external Partition references within the input file's parent directory by default. Use `ResolverOptions(root=...)` to select another allowed root, or `enabled=False` to disable external reads. Bytes-based and structure-only APIs do not open external files.

Missing units or colors require explicit supplements. Partial scenes require explicit export permission. Existing output files are protected by default. GLB output contains display triangles and approximate PBR materials; precise geometry, PMI interpretation and CAD editing history are outside the supported profile.

## Command line

```bash
python -m cq_jt inspect part.jt --json
python -m cq_jt decode part.jt --json
python -m cq_jt structure part.jt --json
python -m cq_jt mesh part.jt --lod highest --json
python -m cq_jt convert part.jt part.glb --json
```

## Documentation

- [Documentation index](https://github.com/monozukuri-ai/cq-jt/blob/main/docs/README.md)
- [API guide](https://github.com/monozukuri-ai/cq-jt/blob/main/docs/model-api.md): reading, arrays, units, external references, export and errors
- [Compatibility](https://github.com/monozukuri-ai/cq-jt/blob/main/docs/compatibility.md): supported profiles and experimental JT 10.6
- [Conversion example](https://github.com/monozukuri-ai/cq-jt/blob/main/examples/README.md)
- [Rust core](https://github.com/monozukuri-ai/cq-jt/blob/main/crates/jt-core/README.md)

## Development

```bash
uv run pytest
cargo test -p jt-core --locked --features glb
uv run python scripts/build_python_release.py
python scripts/check_publication.py
```

The included tests and fixtures can run from a public checkout without acquired CAD data or proprietary tools.

## License

MIT. Dependency licenses, Siemens format notices and other attributions are included in [THIRD_PARTY_NOTICES.md](https://github.com/monozukuri-ai/cq-jt/blob/main/THIRD_PARTY_NOTICES.md) and [licenses/](https://github.com/monozukuri-ai/cq-jt/tree/main/licenses/).
