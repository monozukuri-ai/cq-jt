import json
import math
from dataclasses import FrozenInstanceError, replace
import struct
import subprocess
import sys

import pytest
import cq_jt as jt
from mesh_fixture import mesh_scene, tetra_payload
from structure_fixture import IDENTITY
from glb_reader import Glb


def control(**kwargs):
    return jt.read_bytes(mesh_scene(unit="Millimeters",root_color=(1.,0.,0.,1.),**kwargs))


def test_glb_shares_accessors_across_material_variants_and_preserves_world_positions(tmp_path):
    scene=control()
    data,report=jt.export_glb_bytes(scene)
    glb=Glb(data);g=glb.json
    assert report.source_complete and not report.unit_fallback_used
    assert (report.geometry_count,report.mesh_count,report.material_count,report.instance_count)==(1,2,2,2)
    assert (report.vertex_count,report.triangle_count,report.placed_triangle_count)==(4,4,8)
    assert len(data)==report.byte_length
    a,b=[m["primitives"][0] for m in g["meshes"]]
    assert a["attributes"]==b["attributes"] and a["indices"]==b["indices"]
    assert g["materials"][a["material"]]["pbrMetallicRoughness"]["baseColorFactor"]==[1.,0.,0.,1.]
    assert g["materials"][b["material"]]["pbrMetallicRoughness"]["baseColorFactor"]==[0.,0.,1.,.5]
    assert g["materials"][b["material"]]["alphaMode"]=="BLEND"
    positions=glb.accessor(a["attributes"]["POSITION"]);normals=glb.accessor(a["attributes"]["NORMAL"])
    assert positions[1]==pytest.approx((.001,0.,0.),abs=1e-10)
    assert all(math.hypot(*n)==pytest.approx(1.,abs=1e-6) for n in normals)
    world=glb.world_positions();first,second=(i.structure_instance_id for i in scene.instances)
    assert world[first][1]==pytest.approx((.01,.001,0.),abs=1e-9)
    assert world[second][1]==pytest.approx((.011,.02,0.),abs=1e-9)
    assert glb.accessor(a["indices"])==((0,),(1,),(2,),(2,),(1,),(3,),(2,),(3,),(0,),(3,),(1,),(0,))
    path=tmp_path/"shape.glb"
    actual=jt.export_glb(scene,path)
    assert path.read_bytes()==data and actual==report
    with pytest.raises(FileExistsError):jt.export_glb(scene,path)
    assert path.read_bytes()==data
    assert not list(tmp_path.glob(".cq-jt-*.tmp"))
    assert jt.export_glb(scene,path,overwrite=True).byte_length==len(data)
    with pytest.raises(FrozenInstanceError):report.node_count=0
    assert json.loads(json.dumps(report.to_dict()))["format"]=="glb-2.0"


def test_units_and_colors_require_explicit_supplementation(tmp_path):
    scene=jt.read_bytes(mesh_scene())
    with pytest.raises(jt.JtExportError,match="units"):jt.export_glb_bytes(scene)
    with pytest.raises(jt.JtExportError,match="diffuse"):jt.export_glb_bytes(scene,options=jt.ExportOptions(meters_per_unit=.001))
    data,report=jt.export_glb_bytes(scene,options=jt.ExportOptions(meters_per_unit=.001,default_color=(.2,.4,.6,1.)))
    assert report.unit_fallback_used
    assert {d.code for d in report.diagnostics}>={"export_units_supplied","export_default_color_supplied"}
    path=tmp_path/"existing.glb";path.write_bytes(data)
    with pytest.raises(jt.JtExportError):jt.export_glb(scene,path,overwrite=True)
    assert path.read_bytes()==data
    # Missing units on ancestors are safe only if their local translations are zero.
    scene=jt.read_bytes(mesh_scene(unit="Millimeters",unit_owner=4,root_color=(1.,0.,0.,1.),root_matrix=IDENTITY,second_matrix=IDENTITY))
    assert not jt.export_glb_bytes(scene)[1].unit_fallback_used
    scene=jt.read_bytes(mesh_scene(unit="Millimeters",unit_owner=4,root_color=(1.,0.,0.,1.)))
    with pytest.raises(jt.JtExportError):jt.export_glb_bytes(scene)
    assert jt.export_glb_bytes(scene,options=jt.ExportOptions(meters_per_unit=.001))[1].unit_fallback_used
    with pytest.raises(jt.JtExportError):jt.export_glb_bytes(scene,options=jt.ExportOptions(meters_per_unit=.0254))


def test_srgb_and_explicit_axis_conversion():
    scene=jt.read_bytes(mesh_scene(unit="Meters",root_color=(.5,.5,.5,1.)))
    data,report=jt.export_glb_bytes(scene,options=jt.ExportOptions(color_space="srgb",up_axis="z"))
    g=Glb(data)
    assert g.json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"][:3]==pytest.approx((.21404114048223255,)*3)
    assert report.up_axis=="z"
    first=scene.instances[0].structure_instance_id
    assert g.world_positions()[first][3]==pytest.approx((10.,-1.,0.),abs=1e-7)


def test_local_trs_hierarchy_handles_sheared_world_and_mirrored_scale():
    angle=math.sqrt(.5)
    rotate=(angle,-angle,0.,0.,angle,angle,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)
    parent=(2.,0.,0.,10.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)
    mirror=(-1.,0.,0.,0.,0.,2.,0.,20.,0.,0.,3.,0.,0.,0.,0.,1.)
    scene=control(root_matrix=parent,first_matrix=rotate,second_matrix=mirror)
    data,_=jt.export_glb_bytes(scene)
    g=Glb(data);world=g.world_positions()
    for placement in scene.instances:
        state=scene.structure.instances[placement.structure_instance_id]
        expected=tuple(tuple((sum(state.world_transform[r*4+k]*v[k] for k in range(3))+state.world_transform[r*4+3])*.001 for r in range(3)) for v in ((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.)))
        for a,b in zip(world[state.id],expected,strict=True):assert a==pytest.approx(b,abs=1e-9)
    primitives=[m["primitives"][0] for m in g.json["meshes"]]
    assert primitives[0]["indices"]==primitives[1]["indices"] # no double winding flip


def test_partial_requires_explicit_permission_and_selected_lod_only():
    data=mesh_scene(unit="Millimeters",root_color=(1.,0.,0.,1.),root_children=(1,2,999))
    scene=jt.read_bytes(data,options=jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort")))
    with pytest.raises(jt.JtExportError) as error:jt.export_glb_bytes(scene)
    assert error.value.code=="partial_scene_requires_permission"
    data,report=jt.export_glb_bytes(scene,options=jt.ExportOptions(allow_partial=True))
    assert not report.source_complete and not Glb(data).json["asset"]["extras"]["jt"]["sourceComplete"]
    for level in (0,1):
        scene=jt.read_bytes(mesh_scene(unit="Millimeters",root_color=(1.,0.,0.,1.),lod=True),options=jt.ReadOptions(lod=level))
        assert jt.export_glb_bytes(scene)[1].instance_count==2


def test_no_normals_uses_gltf_flat_normal_semantics():
    scene=control(payload=tetra_payload(normals=False))
    data,report=jt.export_glb_bytes(scene)
    assert "NORMAL" not in Glb(data).json["meshes"][0]["primitives"][0]["attributes"]
    assert report.normalized_normal_count==0


@pytest.mark.parametrize("field",["max_output_bytes","max_json_bytes","max_vertices","max_nodes","max_materials","max_mesh_variants"])
def test_export_limits_fail_before_writing(field,tmp_path):
    path=tmp_path/"out.glb"
    with pytest.raises(jt.JtExportError):jt.export_glb(control(),path,options=replace(jt.ExportOptions(),**{field:0}))
    assert not path.exists() and not list(tmp_path.iterdir())


@pytest.mark.parametrize("field,value",[("allow_partial",1),("meters_per_unit",True),("meters_per_unit",0),("meters_per_unit",float("nan")),("meters_per_unit",-1),("default_color",[1.,0.,0.,1.]),("default_color",(1.,0.,0.)),("default_color",(2.,0.,0.,1.)),("default_color",(True,0.,0.,1.)),("max_position_error_meters",-1),("max_position_error_meters",float("inf")),("max_output_bytes",True),("max_output_bytes",-1),("max_nodes",2**32),("max_output_bytes",2**64),("color_space","unknown"),("up_axis","x")])
def test_bad_export_options(field,value):
    with pytest.raises((ValueError,TypeError)):
        jt.export_glb_bytes(control(),options=replace(jt.ExportOptions(),**{field:value}))


def test_api_guard_and_argument_types(monkeypatch,tmp_path):
    scene=control()
    for options in (True,{},jt.ReadOptions()):
        with pytest.raises(ValueError):jt.export_glb_bytes(scene,options=options)
    with pytest.raises(TypeError):jt.export_glb_bytes(scene.to_model())
    with pytest.raises(TypeError):jt.export_glb(scene,b"out.glb")
    with pytest.raises(TypeError):jt.export_glb(scene,tmp_path/"a.glb",overwrite=1)
    with pytest.raises(OSError):jt.export_glb(scene,tmp_path/"missing"/"a.glb")
    monkeypatch.setattr(jt.export,"EXPORT_API_VERSION",99)
    with pytest.raises(ImportError):jt.export_glb_bytes(scene)


def test_convert_cli_and_public_api(tmp_path):
    input=tmp_path/"input.jt";output=tmp_path/"output.glb";input.write_bytes(mesh_scene(unit="Millimeters",root_color=(1.,0.,0.,1.)))
    run=lambda *extra:subprocess.run([sys.executable,"-m","cq_jt","convert",str(input),str(output),*extra],text=True,capture_output=True)
    result=run("--json");assert result.returncode==0,result.stderr
    report=json.loads(result.stdout);assert report["instance_count"]==2 and report["format"]=="glb-2.0"
    before=output.read_bytes();assert run("--json").returncode==1 and output.read_bytes()==before
    assert run("--json","--overwrite").returncode==0
    assert run("--meters-per-unit","nan").returncode==2
    assert run("--up-axis","x").returncode==2
    assert run("--max-output-bytes","1","--overwrite").returncode==1
    assert output.read_bytes()==before
    assert jt.convert(input,output,overwrite=True).byte_length==len(before)
