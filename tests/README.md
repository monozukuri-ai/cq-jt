# Tests

The included Python fixtures and Rust hexadecimal fixtures are authored controls covered by the project's MIT license. They contain no acquired third-party CAD files.

```bash
uv run pytest
cargo test -p jt-core --locked --features glb
```

Python tests exercise the native extension, public API and CLI. Rust tests run independently of Python. The fixtures include multiple versions, byte orders, compression, shared geometry, external references and malformed inputs.

To generate synthetic GLBs and inspect their numerical data independently:

```bash
uv run python scripts/check_glb_exports.py --output-dir build/glb --report build/exports.json
```

Create the report's parent directory first. The CI workflow additionally checks these GLBs with Khronos glTF Validator.
