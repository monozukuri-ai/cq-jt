# jt-py-bridge

Direct conversion between `jt_core::Scene` and `cq_jt.model.Scene`. The bridge
preserves source domains, identifiers, opaque bytes and absent metadata without
a JSON intermediate. It validates model API version 1 separately from package
and JT file versions. Inspection results and source-aware parse errors are also
converted directly to `cq_jt.container` dataclasses and `cq_jt.errors` exceptions,
with inspection API version 2 checked independently. Decoded logical metadata
and explicit raw-element access use payload API version 1.

Structure API version 3 and mesh API version 2 have independent guards. The
structure and mesh modules convert typed snapshots directly and expose explicit
little-endian packed-array copies without requiring NumPy or a JSON intermediate.
Resolver options are validated before detaching from Python; no Python callbacks
run during Rust IO. Source-scoped identifiers, optional modern fields and shared
raw documents are retained in typed snapshots.
Structure options include an exact-bool opt-in to the unqualified JT 10.6
experimental profile; both structure and mesh JSON remain version 2.

The optional `glb` feature adds export API version 1, typed option validation,
direct report conversion and source-aware export errors. Encoding remains in
`jt-core`; the final extension owns the GIL boundary and explicit bytes copy.

This crate exports no Python module and does not enable `extension-module`.
Install a matching cq-jt Python package when using its conversion functions.
