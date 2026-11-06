# Conversion example

[convert_to_glb.py](convert_to_glb.py) reads a supported JT input and writes its selected display meshes to GLB. It uses the public API and has no optional Python dependencies.

```bash
python examples/convert_to_glb.py assembly.jt assembly.glb
python examples/convert_to_glb.py part.jt preview.glb --lod lowest --up-axis z
python examples/convert_to_glb.py unknown.jt preview.glb --meters-per-unit .001 --default-color .7 .7 .7 1
```

The package must be installed in the selected Python environment. Supply your own JT file. External JT references are resolved within the input's parent directory by default. Existing output files require `--overwrite`.

Use `--up-axis z` only when the source is known to be Z-up. `--meters-per-unit` and `--default-color` supply missing export information; known values are retained. Missing units during external assembly composition require `ResolverOptions.meters_per_unit` through the API, or `--assembly-meters-per-unit` through the package CLI.

For additional options, including experimental JT 10.6, use `python -m cq_jt convert --help` and the [API guide](../docs/model-api.md).
