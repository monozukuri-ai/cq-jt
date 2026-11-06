# Third party notices

The Python extension uses PyO3 0.29.2 (MIT OR Apache-2.0) and its Rust
dependencies. Cargo.lock records the resolved versions. `licenses/manifest.json`
identifies all locked dependencies and their license texts, including build
dependencies. For dual MIT/Apache-2.0 licenses this distribution retains the
MIT text; target-lexicon's Apache-2.0 WITH LLVM-exception and unicode-ident's
additional Unicode-3.0 notice are retained separately. Their original
copyright and license terms continue to apply.

Outer compression uses flate2 1.1.10 (MIT OR Apache-2.0, Rust backend),
crc32fast 1.5.0 (MIT OR Apache-2.0), and lzma-rust2 0.15.8 (Apache-2.0),
with their locked Rust dependencies. Their license texts are in licenses/;
the MIT alternative is retained where offered. lzma-rust2 is a Rust port of
tukaani xz for java and a continuation of lzma-rust:
https://github.com/hasenbanck/lzma-rust2/
No external shared libz or liblzma is used by this package. The bounded XZ
framing checks follow the public-domain XZ file format specification 1.2.1:
https://tukaani.org/xz/xz-file-format-1.2.1.txt

The package layout and Rust/Python boundary were informed by cq-acis
(MIT, https://github.com/monozukuri-ai/cq-acis). The scene model is specific
to this project; ACIS entity implementations are not included.

JT is a Siemens format and trademark. The JT data structures and identifiers
used by the container reader are Copyright 2026 Siemens. They are used under
the Siemens JT Data Format Reference Intellectual Property License Terms,
JT 10.6 Rev D (10 February 2026), page ii, for interpreting JT input. Those
terms permit the necessary use of the listed structures and operators with
an appropriate copyright notice; they do not grant a license to Siemens SDK
code, documentation, trademarks or patents. Any applicable patent permission
is governed separately by Siemens' official clarification notice.

The reader's implementation code is MIT licensed. This distribution includes
no Siemens SDK code, specification documents or sample CAD files. The research
probe, downloaded third-party source snapshots and research material are also
excluded from Python distributions. Official specification access and notices:
https://www.siemens.com/en-us/products/plm-components/jt/jt-open-program/

JT 9.5 structures and identifiers are Copyright 2010 Siemens Product Lifecycle
Management Software Inc. They are used under the JT 9.5 Rev A Data Format
Reference Intellectual Property License Terms, section 1, for interpreting JT
input. The structure reader was checked against that Siemens specification and
the pinned legacy source references; no third-party JT reader code is included.

The JT 9.5 mesh data structures and decoding algorithms follow sections
7.2.2 and 8, and Appendices C, D and E of that reference, under the same
Siemens copyright notice and IP license terms. The small hash implementation
in `mesh/hash.rs` follows Bob Jenkins' 1996 algorithm from Appendix D, whose
notice permits private, educational and commercial use without restriction.
No Java or C# runtime or third-party JT reader is bundled or required.

JT 10.0 structures and identifiers are Copyright 2015 Siemens Product Lifecycle
Management Software Inc. (JT File Format Reference Version 10.0 Rev-B, cover;
internal pages also carry a 2014 notice). They are used under section 1 of that
reference for interpreting JT input. The 10.0/10.3 reader was checked against
this specification, later Siemens references and pinned MIT JTReader source.
No JTReader source, .NET SDK or development-only comparison dependency is
included in or required by this distribution.

The opt-in experimental JT 10.6 structure/mesh profile uses the structures and
operators from JT 10.6 Rev D, Copyright 2026 Siemens, under the same IP license
terms noted above. Its authored specification controls are not native CAD exports
and do not establish JT 10.6 file compatibility.
