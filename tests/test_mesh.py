import gc
import json
from dataclasses import FrozenInstanceError, replace
import struct
import subprocess
import sys

import pytest
import cq_jt as jt
from mesh_fixture import mesh_scene, tetra_payload


def unpack(array):
    fmt={"f64":"d","u32":"I","i32":"i"}[array.dtype]
    values=tuple(v[0] for v in struct.iter_unpack("<"+fmt,array.data))
    return tuple(values[i:i+array.shape[1]] for i in range(0,len(values),array.shape[1]))


@pytest.mark.parametrize("endian",["<",">"])
@pytest.mark.parametrize("codec",["zlib","xz","none"])
def test_authored_tetra_buffers_sharing_placement_and_ownership(endian,codec,tmp_path):
    data=mesh_scene(endian=endian,codec=codec)
    s=jt.read_bytes(data)
    assert s.complete and s.geometry_decoded
    assert len(s.meshes)==1 and len(s.instances)==2
    assert [i.mesh_id for i in s.instances]==[0,0]
    assert isinstance(s.meshes,tuple) and isinstance(s.instances,tuple)
    m=s.meshes[0]
    assert (m.vertex_count,m.triangle_count,m.normal_count,m.cover_faces)==(4,4,4,0)
    assert m.bounds==(0.,0.,-1.,1.,1.,0.) and m.codec_depth==0
    buffers=s.mesh_buffers(0)
    assert buffers.positions.dtype=="f64" and buffers.positions.shape==(4,3)
    assert unpack(buffers.positions)==((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.))
    assert unpack(buffers.triangles)==((0,1,2),(2,1,3),(2,3,0),(3,1,0))
    assert unpack(buffers.normal_indices)==unpack(buffers.triangles)
    assert buffers.face_groups.shape==(4,1) and buffers.vertex_flags.shape==(0,1)
    assert buffers.positions.view().readonly
    model=s.to_model();instances=s.structure.instances
    a,b=(instances[i.structure_instance_id] for i in s.instances)
    point=(1.,0.,0.,1.)
    world=lambda i:tuple(sum(i.world_transform[r*4+k]*point[k] for k in range(4)) for r in range(3))
    assert world(a)==(10.,1.,0.) and world(b)==(11.,20.,0.)
    assert b.material.diffuse_rgb==(0.,0.,1.) and b.material.diffuse_alpha==.5
    assert a.meters_per_unit is None
    with pytest.raises(FrozenInstanceError):model.complete=False
    with pytest.raises(AttributeError):s.complete=False
    path=tmp_path/"authored.jt";path.write_bytes(data)
    assert jt.read_file(path,source_id="input").to_model()==model
    del s;gc.collect()
    assert len(buffers.positions.data)==96 and model.meshes[0]==m


@pytest.mark.parametrize("lod,index",[("highest",0),("lowest",1),(0,0),(1,1)])
def test_lod_selects_one_edge_per_occurrence(lod,index):
    s=jt.read_bytes(mesh_scene(lod=True),options=jt.ReadOptions(lod=lod))
    assert s.complete and len(s.meshes)==1 and len(s.instances)==2
    assert s.lod_choices[0].child_index==index
    selected=s.lod_choices[0].child_node_id
    assert all(s.structure.instances[i.structure_instance_id].node_id==selected for i in s.instances)


def test_unordered_lod_requires_explicit_index():
    data=mesh_scene(lod=True,unordered=True)
    with pytest.raises(jt.JtUnsupportedError):jt.read_bytes(data)
    partial=jt.read_bytes(data,options=jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort")))
    assert not partial.complete and not partial.instances
    assert jt.read_bytes(data,options=jt.ReadOptions(lod=1)).complete
    with pytest.raises(jt.JtUnsupportedError):jt.read_bytes(data,options=jt.ReadOptions(lod=2))


def test_duplicate_lod_edges_and_missing_first_child_do_not_overlay_or_shift():
    data=mesh_scene(lod=True,lod_children=(4,4))
    for lod in ("highest","lowest"):
        s=jt.read_bytes(data,options=jt.ReadOptions(lod=lod))
        assert len(s.instances)==2
    data=mesh_scene(lod=True,lod_children=(999,4))
    s=jt.read_bytes(data,options=jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort")))
    assert not s.complete and not s.instances


@pytest.mark.parametrize("normals",[True,False])
@pytest.mark.parametrize("version",[1,2])
def test_rep_versions_optional_normals_and_cover_removal(normals,version):
    s=jt.read_bytes(mesh_scene(payload=tetra_payload(normals=normals,version=version,cover=True)))
    assert s.meshes[0].triangle_count==3 and s.meshes[0].cover_faces==1
    b=s.mesh_buffers(0)
    assert (b.normal_indices is None)==(not normals)
    assert b.normals.shape==(4 if normals else 0,3)


@pytest.mark.parametrize("field",list(jt.MeshOptions.__dataclass_fields__))
def test_mesh_limits_are_enforced(field):
    if field=="max_codec_depth":
        # The authored packet only uses depth zero; strict depth zero is valid.
        assert jt.read_bytes(mesh_scene(),options=jt.ReadOptions(mesh=jt.MeshOptions(max_codec_depth=0))).complete
        return
    options=jt.ReadOptions(mesh=replace(jt.MeshOptions(),**{field:0}))
    with pytest.raises(jt.JtLimitError):jt.read_bytes(mesh_scene(),options=options)


@pytest.mark.parametrize("field",list(jt.MeshOptions.__dataclass_fields__))
@pytest.mark.parametrize("value",[True,-1,1.2,2**65])
def test_invalid_limits_rejected_before_decoding(field,value):
    with pytest.raises((ValueError,TypeError)):
        jt.read_bytes(b"",options=jt.ReadOptions(mesh=replace(jt.MeshOptions(),**{field:value})))


@pytest.mark.parametrize("lod",[True,-1,2**32,1.1,"all","1",None])
def test_invalid_lod_values(lod):
    with pytest.raises((ValueError,TypeError)):
        jt.read_bytes(b"",options=jt.ReadOptions(lod=lod))


def test_unsupported_payload_is_partial_but_corruption_is_always_fatal():
    data=mesh_scene(missing=True)
    with pytest.raises(jt.JtUnsupportedError):jt.read_bytes(data)
    s=jt.read_bytes(data,options=jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort")))
    assert not s.complete and s.instances==() and s.meshes==()
    for flags in ({"wrong_hash":True},{"zero_normal":True}):
        for mode in ("strict","best_effort"):
            with pytest.raises(jt.JtParseError):jt.read_bytes(mesh_scene(payload=tetra_payload(**flags)),options=jt.ReadOptions(structure=jt.StructureOptions(mode=mode)))
    with pytest.raises(jt.JtUnsupportedError):jt.read_bytes(mesh_scene(payload=tetra_payload(binding_extra=0x10)))


@pytest.mark.parametrize("index",[-1,True,0.1,1,2**100])
def test_buffer_indices_are_checked(index):
    with pytest.raises((TypeError,IndexError)):jt.read_bytes(mesh_scene()).mesh_buffers(index)


def test_mesh_api_guard(monkeypatch):
    monkeypatch.setattr(jt.mesh,"MESH_API_VERSION",99)
    with pytest.raises(ImportError):jt.read_bytes(mesh_scene())


def test_argument_types_and_shared_buffer_budget(tmp_path):
    for options in (True,1,jt.MeshOptions(),jt.StructureOptions()):
        with pytest.raises((ValueError,TypeError)):jt.read_bytes(b"",options=options)
    with pytest.raises((ValueError,TypeError)):jt.read_bytes(b"",options=jt.ReadOptions(mesh=True))
    with pytest.raises((ValueError,TypeError)):jt.read_bytes(b"",options=jt.ReadOptions(mesh=jt.MeshOptions(max_codec_depth=65)))
    with pytest.raises(TypeError):jt.read_file(b"path.jt")
    with pytest.raises(OSError):jt.read_file(tmp_path/"absent.jt")
    # The 304-byte physical mesh is charged once even with two placements.
    assert jt.read_bytes(mesh_scene(),options=jt.ReadOptions(mesh=jt.MeshOptions(max_vertices=4,max_triangles=4,max_mesh_bytes=304))).complete
    with pytest.raises(jt.JtLimitError):jt.read_bytes(mesh_scene(),options=jt.ReadOptions(mesh=jt.MeshOptions(max_mesh_bytes=303)))


def test_cli_json_partial_errors_and_arguments(tmp_path):
    path=tmp_path/"scene.jt";path.write_bytes(mesh_scene(lod=True))
    run=lambda *args:subprocess.run([sys.executable,"-m","cq_jt","mesh",str(path),*args],capture_output=True,text=True)
    report=json.loads(run("--json","--lod","lowest").stdout)
    assert report["geometry_decoded"] and report["complete"] and report["profile"]=="mesh-9.5"
    assert len(report["meshes"])==1 and len(report["instances"])==2
    assert report["lod_choices"][0]["child_index"]==1
    assert run("--max-triangles","0").returncode==1
    assert run("--lod","-1").returncode==2
    assert run("--max-codec-depth","65").returncode==2
    path.write_bytes(mesh_scene(missing=True))
    result=run("--json","--mode","best_effort");assert result.returncode==0 and not json.loads(result.stdout)["complete"]
    assert run("--json").returncode==1
    path.unlink();assert run("--json").returncode==1
