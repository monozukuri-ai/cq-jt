from dataclasses import FrozenInstanceError, asdict, replace
from importlib import machinery, metadata
from decimal import Decimal
import pickle

import pytest

import cq_jt
from cq_jt import (
    Diagnostic, DiagnosticSeverity, JtModelError, JtVersion, NativeScene,
    RawElement, Scene, SourceMetadata, SourceSpan, validate_scene,
)


def sample_scene() -> Scene:
    span = SourceSpan("part", "segment/opaque/inflated", 2**63, 2**63 + 3)
    return Scene(
        sources=(SourceMetadata("part", "part.jt", JtVersion(9, 5), None),),
        raw_elements=(RawElement(bytes(range(16)), -(2**63), b"\0\xff\0", span),),
        diagnostics=(Diagnostic("opaque", "Retained é\0bytes", DiagnosticSeverity.WARNING, span),),
    )


def test_native_module_and_versions():
    assert any(cq_jt._native.__file__.endswith(s) for s in machinery.EXTENSION_SUFFIXES)
    assert cq_jt.__version__ == cq_jt._native.CORE_VERSION == metadata.version("cq-jt")
    assert cq_jt.MODEL_API_VERSION == cq_jt._native.MODEL_API_VERSION == 1
    assert cq_jt.MESH_API_VERSION == cq_jt._native.MESH_API_VERSION == 2
    assert cq_jt.read is cq_jt.read_file


def test_round_trip_preserves_typed_values_and_opaque_bytes():
    model = sample_scene()
    native = model.to_native()
    assert native.to_model() == model
    assert asdict(native.to_model()) == asdict(model)
    assert native.sources == model.sources
    assert native.raw_elements == model.raw_elements
    assert native.diagnostics == model.diagnostics
    assert native.sources[0].meters_per_unit is None
    assert native.raw_elements[0].source.start_offset == 2**63
    assert native.raw_elements[0].type_id == bytes(range(16))
    assert pickle.loads(pickle.dumps(model)) == model
    assert "sources=1" in repr(native)
    validate_scene(model)


def test_native_snapshot_does_not_alias_python_objects():
    model = sample_scene()
    native = NativeScene.from_model(model)
    original = native.to_model()
    object.__setattr__(model.raw_elements[0], "raw_data", b"mutated")
    object.__setattr__(model.sources[0], "label", "changed")
    assert native.to_model() == original
    view = native.to_model()
    object.__setattr__(view.raw_elements[0], "raw_data", b"changed view")
    assert native.to_model() == original
    with pytest.raises(AttributeError):
        native.sources = ()
    with pytest.raises(FrozenInstanceError):
        model.sources = ()


def test_scene_canonicalizes_collection_inputs_and_accepts_empty_models():
    source = SourceMetadata("part")
    source_list = [source]
    model = Scene(source_list, [], [])
    source_list.clear()
    assert model.sources == (source,)
    assert Scene().to_native().to_model() == Scene()
    assert replace(model) == model


@pytest.mark.parametrize("units", [0.0, -0.0, -1.0, float("nan"), float("inf")])
def test_invalid_units_have_structured_errors(units):
    with pytest.raises(JtModelError) as caught:
        Scene((SourceMetadata("part", meters_per_unit=units),))
    assert caught.value.code == "invalid_units"
    assert isinstance(caught.value, ValueError)


def test_known_units_are_not_reinterpreted():
    model = Scene((SourceMetadata("part", meters_per_unit=0.0254),))
    assert model.to_native().sources[0].meters_per_unit == 0.0254


def test_source_id_and_diagnostic_reference_validation():
    source = SourceMetadata("part")
    for sources in [(source, source), (SourceMetadata(""),)]:
        with pytest.raises(JtModelError, match="source IDs"):
            Scene(sources)
    with pytest.raises(JtModelError) as caught:
        Scene(diagnostics=sample_scene().diagnostics)
    assert caught.value.code == "missing_source"


@pytest.mark.parametrize("span", [
    SourceSpan("part", "", 0, 1), SourceSpan("part", "file", 3, 2),
    SourceSpan("missing", "file", 0, 1), SourceSpan("part", "file", -1, 1),
    SourceSpan("part", "file", True, 1), SourceSpan("part", "file", 0.0, 1),
    SourceSpan("part", "file", 0, 2**64),
])
def test_invalid_spans_cannot_cross_native_boundary(span):
    with pytest.raises(JtModelError):
        Scene((SourceMetadata("part"),), (RawElement(bytes(16), source=span),))


@pytest.mark.parametrize("raw", [
    RawElement(bytes(15)), RawElement(bytes(17)), RawElement(bytearray(16)),
    RawElement(bytes(16), raw_data=bytearray(b"data")),
    RawElement(bytes(16), object_id=True), RawElement(bytes(16), object_id=1.0),
    RawElement(bytes(16), object_id=2**63), RawElement(bytes(16), object_id=-(2**63)-1),
])
def test_invalid_raw_fields_are_rejected(raw):
    with pytest.raises(JtModelError):
        Scene(raw_elements=(raw,))


@pytest.mark.parametrize("source", [
    SourceMetadata("part", version=JtVersion(0, 1)),
    SourceMetadata("part", version=JtVersion(True, 5)),
    SourceMetadata("part", version=JtVersion(9, 2**16)),
    SourceMetadata("part", meters_per_unit=True),
    SourceMetadata("part", meters_per_unit=Decimal("0.001")),
    SourceMetadata("part", meters_per_unit="0.001"),
])
def test_invalid_metadata_is_rejected(source):
    with pytest.raises(JtModelError):
        Scene((source,))


def test_diagnostic_types_are_not_silently_coerced():
    for diagnostic in [Diagnostic("", "empty code"), Diagnostic("code", "message", "warning")]:
        with pytest.raises(JtModelError):
            Scene(diagnostics=(diagnostic,))
    with pytest.raises(JtModelError):
        NativeScene.from_model(object())


def test_mismatched_model_api_is_rejected_in_both_directions(monkeypatch):
    import cq_jt.model as model_module

    model = sample_scene()
    native = model.to_native()
    monkeypatch.setattr(model_module, "MODEL_API_VERSION", 2)
    with pytest.raises(ImportError, match="MODEL_API_VERSION"):
        NativeScene.from_model(model)
    with pytest.raises(ImportError, match="MODEL_API_VERSION"):
        native.to_model()


def test_bool_is_not_a_model_api_version(monkeypatch):
    import cq_jt.model as model_module

    model = Scene()
    monkeypatch.setattr(model_module, "MODEL_API_VERSION", True)
    with pytest.raises(ImportError):
        NativeScene.from_model(model)


def test_missing_model_api_version_is_an_import_error(monkeypatch):
    import cq_jt.model as model_module

    model = Scene()
    monkeypatch.delattr(model_module, "MODEL_API_VERSION")
    with pytest.raises(ImportError, match="MODEL_API_VERSION"):
        NativeScene.from_model(model)
