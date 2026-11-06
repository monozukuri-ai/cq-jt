"""Specification-derived controls only; these are not native JT 10.6 exports."""
from dataclasses import replace
import json
import struct
import subprocess
import sys

import pytest
import cq_jt as jt
from assembly_fixture import assembly
from modern_fixture import modern_scene
from glb_reader import Glb


OPTIONS = jt.ReadOptions(structure=jt.StructureOptions(experimental_jt106=True))


@pytest.mark.parametrize("endian",["<",">"])
@pytest.mark.parametrize("bits",[0,3,13])
def test_specification_tetra_unified_lsg_and_scalar_hash(endian,bits):
    data=modern_scene(version="10.6",endian=endian,bits=bits,parent_final=True)
    with pytest.raises(jt.JtUnsupportedError):jt.read_bytes(data)
    scene=jt.read_bytes(data,options=OPTIONS)
    assert scene.complete and scene.structure.source.version==jt.JtVersion(10,6)
    assert scene.to_model().to_dict()['profile']=="mesh-10.6-experimental"
    assert scene.structure.to_model().to_dict()['profile']=="structure-10.6-experimental"
    assert len(scene.meshes)==len(scene.instances)==1
    buffers=scene.mesh_buffers(0)
    assert tuple(struct.iter_unpack("<3d",buffers.positions.data))==((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.))
    normals=tuple(struct.iter_unpack("<3d",buffers.normals.data))
    for a,b in zip(normals,((0.,0.,1.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.)),strict=True):assert a==pytest.approx(b,abs=1e-12)
    assert scene.structure.instances[-1].material.diffuse_rgb==(0.,1.,0.)
    assert scene.structure.atoms[-1].value.reserved==1
    segment=next(s for s in scene.structure.document.segments if s.segment_type==1)
    assert {e.list_index for e in segment.elements}=={0}
    assert sum(e.is_end_marker for e in segment.elements)==1
    assert any(d.code=="unverified_jt106_profile" for d in scene.diagnostics)
    glb,report=jt.export_glb_bytes(scene)
    assert report.triangle_count==4
    assert any(d.code=="unverified_jt106_profile" for d in report.diagnostics)
    assert Glb(glb).json["asset"]["extras"]["jt"]["unverifiedProfiles"]


@pytest.mark.parametrize("mode",["strict","best_effort"])
def test_jt106_hash_rule_is_not_guessed_from_saved_hash(mode):
    data=modern_scene(version="10.6",scalar_hash=False)
    with pytest.raises(jt.JtParseError,match="coordinate hash mismatch"):
        jt.read_bytes(data,options=replace(OPTIONS,structure=replace(OPTIONS.structure,mode=mode)))


def test_jt106_partition_box_variant_is_explicit():
    with pytest.raises(jt.JtUnsupportedError) as error:
        jt.read_bytes(modern_scene(version="10.6",partition_flags=0),options=OPTIONS)
    assert error.value.code=="unsupported_jt106_partition_bounds"


@pytest.mark.parametrize("endian",["<",">"])
def test_jt106_float64_transform_and_physical_glb_coordinates(endian):
    scene=jt.read_bytes(modern_scene(version="10.6",endian=endian,translation=(2.,3.,4.)),options=OPTIONS)
    state=scene.structure.instances[-1]
    assert [state.world_transform[i] for i in (3,7,11)]==[2.,3.,4.]
    data,_=jt.export_glb_bytes(scene)
    assert Glb(data).world_positions()[state.id][0]==pytest.approx((.002,.003,.004),abs=1e-12)


@pytest.mark.parametrize("mode",["strict","best_effort"])
def test_jt106_does_not_fall_back_to_the_legacy_split_lists(mode):
    data=modern_scene(version="10.6",split_lists=True)
    with pytest.raises(jt.JtError):
        jt.read_bytes(data,options=replace(OPTIONS,structure=replace(OPTIONS.structure,mode=mode)))


@pytest.mark.parametrize("value",[1,None,"true"])
def test_experimental_option_requires_bool(value):
    with pytest.raises(ValueError,match="experimental_jt106 must be bool"):
        jt.read_bytes(modern_scene(),options=replace(OPTIONS,structure=jt.StructureOptions(experimental_jt106=value)))


def test_jt106_external_source_provenance_and_cli(tmp_path):
    root=tmp_path/"assembly.jt";root.write_bytes(assembly())
    part=tmp_path/"part.jt";part.write_bytes(modern_scene(version="10.6"))
    with pytest.raises(jt.JtUnsupportedError):jt.read(root)
    scene=jt.read(root,options=OPTIONS)
    assert scene.complete and len(scene.structure.sources)==2
    assert scene.to_model().to_dict()['profile']=="mesh-assembly-experimental"
    assert len(scene.meshes)==1 and len(scene.instances)==2
    assert any(d.code=="unverified_jt106_profile" and d.source.source_id==str(part.resolve()) for d in scene.diagnostics)
    for command in (["structure",str(part)],["mesh",str(part)],["convert",str(root),str(tmp_path/"out.glb")]):
        result=subprocess.run([sys.executable,"-m","cq_jt",*command,"--experimental-jt106","--json"],capture_output=True,text=True)
        assert result.returncode==0,result.stdout+result.stderr
        assert any(d['code']=="unverified_jt106_profile" for d in json.loads(result.stdout)['diagnostics'])


def test_jt106_prefix_truncations_and_limits_remain_fatal():
    data=modern_scene(version="10.6")
    for i in range(len(data)):
        with pytest.raises(jt.JtError):jt.read_bytes(data[:i],options=OPTIONS)
    with pytest.raises(jt.JtLimitError):jt.read_bytes(data,options=replace(OPTIONS,mesh=jt.MeshOptions(max_codec_values=1)))
    with pytest.raises(jt.JtLimitError):jt.read_bytes(data,options=replace(OPTIONS,structure=replace(OPTIONS.structure,max_string_bytes=1)))
    with pytest.raises(jt.JtUnsupportedError) as error:
        jt.read_bytes(modern_scene(version="10.6",base_version=2,palette=1),options=OPTIONS)
    assert error.value.code=="unsupported_attribute_palette"
