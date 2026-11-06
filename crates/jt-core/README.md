# jt-core

The optional `glb` feature exports selected mesh scenes as bounded glTF 2.0
binary data. It preserves local transform hierarchies, converts known units to
metres, shares geometry across material variants, unifies position/normal
indices and normalizes copied normals. Missing units/colours require explicit
supplements; partial scenes require explicit permission. Legacy materials are
approximated by nonmetallic PBR, with original fields retained in extras.
Output limits and local f32 position error are configurable. Files are built
fully before same-directory commit, protecting existing files by default.
JSON dependencies are not enabled for the default read-only core.

```rust,no_run
# #[cfg(feature = "glb")]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use jt_core::{mesh::{read_file, ReadOptions}, ReadLimits};
use jt_core::export::{export_glb, ExportOptions};
let scene = read_file(std::path::Path::new("part.jt"), "part", &ReadLimits::default(), &ReadOptions::default())?;
let report = export_glb(&scene, std::path::Path::new("part.glb"), &ExportOptions::default(), false)?;
println!("{} placed triangles", report.placed_triangle_count);
# Ok(())
# }
# #[cfg(not(feature = "glb"))]
# fn main() {}
```

```bash
cargo test -p jt-core --locked --features glb
```

`mesh::read_bytes` / `mesh::read_file` add the selected JT 9.5/10.0/10.3 triangle-mesh
profile. `MeshScene` shares its `StructureDocument`, stores unique meshes and
selected shape occurrences, and retains separate coordinate and corner-normal
indices. Packed-array copies are made only by the Python wrapper on request.
`ReadOptions` controls LOD, structure mode and mesh/CODEC/work limits. Range LOD
uses first/last children for near/far selection; unordered LOD requires an index.
Int32CDP2/3, quantization, Deering normals and dual VFMesh topology are decoded with
stored-hash checks. UV, vertex colour, auxiliary records and PMI/B-Rep are outside
this profile. Other JT semantic versions are rejected.

JT 10.6 has a specification-only experimental profile, enabled explicitly with
`options.structure.experimental_jt106 = true`. It has authored little/big-endian
controls, but no qualified native 10.6 file. Default reads still reject 10.6.
Warnings survive composition and export; GLB extras also mark the unverified
profile. It uses one LSG list, a bounded explicit-box Partition variant and the
Rev D scalar hash rule. `complete` is profile completeness, not qualification.
This switch does not enable 10.5 or other versions.

File reads resolve local external Partition references by default, within a
canonical allowed root (the input parent unless overridden). `ResolverOptions`
bounds unique files, depth, references and aggregate input bytes; decoding and
mesh budgets are shared across the assembly. Canonical files are decoded once.
Resolved geometry and translations are normalized to metres; definitions retain
raw source metadata. Missing assembly units require an explicit resolver
supplement. `read_bytes` performs no external IO; `read_resolved_bytes` accepts
an explicit Rust `SourceResolver`. Structure-only APIs also perform no external IO.

```rust,no_run
use jt_core::{mesh::{read_file, ReadOptions}, ReadLimits};
let scene = read_file(std::path::Path::new("part.jt"), "part", &ReadLimits::default(), &ReadOptions::default())?;
assert!(scene.complete);
for mesh in &scene.meshes {
    println!("{} vertices, {} triangles", mesh.positions.len(), mesh.triangles.len());
}
# Ok::<(), jt_core::InspectError>(())
```

`read_structure_bytes` / `read_structure_file` interpret the JT 9.5/10.0/10.3 LSG profile
into a `StructureDocument`: shared part definitions, per-path node occurrences,
column-vector transforms, inherited units/material fields, property atoms and
bindings. `StructureOptions` limits graph size, depth, references, strings and
path expansion. Strict mode rejects unsupported required data/missing references;
best effort reports explicit partial results. Cycles and malformed data are fatal.
All LOD alternatives remain explicit; external files are never opened. This uses
the bounded outer buffers for the whole input, not a lazy LSG-only decoder. There is
no mesh/PMI/B-Rep or late-metadata interpretation in this profile.

```bash
cargo run -p jt-core --example jt_structure_probe -- path/to/jt95.jt
```

Python-independent JT container inspection and scene model foundation for cq-jt.
`inspect_bytes` / `inspect_file` validate header,
TOC and segment framing within `ReadLimits`; `JtDocument` owns the bytes and
exposes checked segment slices. These inspection APIs do not decompress payloads.

`decode_bytes` / `decode_file` expand outer ZLIB or XZ/LZMA2 streams within
per-segment/total output, dictionary, block and element limits. `DecodedDocument`
owns the output buffers; logical headers retain raw GUIDs, lengths, IDs and source
spans. `DecodedSegment::element_bytes` borrows an opaque object payload without
copying it. LSG property tables and other tails remain uninterpreted. This does
not reconstruct scene nodes, meshes, PMI or B-Rep geometry.

Codecs use pure Rust flate2 and lzma-rust2, with no shared libz/liblzma dependency.
The initial XZ profile permits one LZMA2 filter per block and one stream. A bounded
preflight validates block dictionaries, index/chunk sizes and offsets before
decoding. Unknown segment structures are retained without guessing their layout.

```bash
cargo run -p jt-core --example jt_probe -- path/to/part.jt
```

Inspection retains raw GUID bytes and file-domain source spans, checks ranges,
overlaps, matching segment headers and the LSG root, and reports unknown types.
Read-only container layouts use 32-bit offsets for JT 8/9 and 64-bit for JT 10,
with both byte orders. Real-file checks cover little-endian 8.0, 8.1, 9.5, 10.0
and 10.3; big endian and 10.6 have synthetic tests only.

The immutable validated `Scene` retains source metadata, source spans, opaque
elements and diagnostics. Container inspection does not reconstruct a Scene.

```rust
use jt_core::{Scene, SourceMetadata};

let source = SourceMetadata {
    source_id: "input".into(),
    label: Some("part.jt".into()),
    version: None,
    meters_per_unit: None,
};
let scene = Scene::new(vec![source], vec![], vec![])?;
assert_eq!(scene.sources().len(), 1);
# Ok::<(), jt_core::ModelError>(())
```

Model validation checks source identities, spans and metadata. It does not
prove that opaque bytes are valid JT or that CAD geometry has been decoded.
