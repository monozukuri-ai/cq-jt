from dataclasses import replace
import struct

import pytest
import cq_jt as jt
from assembly_fixture import assembly
from mesh_fixture import mesh_scene
from modern_fixture import modern_scene
from glb_reader import Glb


def files(tmp_path,**kwargs):
    root=tmp_path/"assembly.jt";part=tmp_path/"part.jt"
    root.write_bytes(assembly(**kwargs))
    part.write_bytes(mesh_scene(unit="Inches",root_color=(1.,0.,0.,1.)))
    return root,part


def test_external_cache_units_names_and_source_scoped_ids(tmp_path):
    root,part=files(tmp_path)
    scene=jt.read(root)
    assert scene.complete and scene.structure.coordinates_in_meters
    assert len(scene.structure.sources)==len(scene.structure.documents)==2
    assert len(scene.meshes)==1 and len(scene.instances)==4
    assert len({i.mesh_id for i in scene.instances})==1
    assert scene.structure.sources[0].source_id==str(root)
    assert scene.structure.sources[1].source_id==str(part.resolve())
    buffers=scene.mesh_buffers(0)
    assert struct.unpack_from("<3d",buffers.positions.data,24)==pytest.approx((.0254,0.,0.))
    states=[scene.structure.instances[i.structure_instance_id] for i in scene.instances]
    assert all(s.meters_per_unit==1. and s.source_meters_per_unit==.0254 for s in states)
    assert [s.world_transform[3] for s in states]==pytest.approx([.264]*4)
    assert [s.world_transform[7] for s in states]==pytest.approx([.02,.528,0.,.508])
    assert [s.world_transform[11] for s in states]==pytest.approx([0.,0.,.03,.03])
    model=scene.structure.to_model()
    with pytest.raises(ValueError,match="multiple sources"):model.properties(0)
    assert model.properties(0,source_id=str(root))[0][1]=="authored assembly"
    data,report=jt.export_glb_bytes(scene)
    glb=Glb(data)
    assert report.source_complete and report.geometry_count==1 and report.instance_count==4
    assert report.meters_per_unit==1. and report.placed_triangle_count==16
    assert len(glb.world_positions())==4
    originals=[o for m in glb.json["materials"] for o in m["extras"]["jt"]["originalMaterials"]]
    assert all(o["sourceIndex"]==1 for o in originals)
    assert str(tmp_path).encode() not in data


def test_inherited_final_color_across_file_boundary(tmp_path):
    root,_=files(tmp_path,final_color=True)
    scene=jt.read(root)
    assert all(scene.structure.instances[i.structure_instance_id].material.diffuse_rgb==pytest.approx((.2,.7,.4)) for i in scene.instances)
    data,report=jt.export_glb_bytes(scene)
    assert report.mesh_count==1 and report.material_count==1
    assert Glb(data).json["materials"][0]["extras"]["jt"]["originalMaterials"][0]["sourceIndex"]==0


def test_missing_file_partial_explicit_and_bytes_never_resolve(tmp_path):
    root,_=files(tmp_path,second="missing.jt")
    with pytest.raises(jt.JtReferenceError):jt.read(root)
    options=jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort"))
    scene=jt.read(root,options=options)
    assert not scene.complete and len(scene.instances)==2
    assert scene.structure.missing_references[0].target=="missing.jt"
    with pytest.raises(jt.JtExportError):jt.export_glb_bytes(scene)
    assert not jt.export_glb_bytes(scene,options=jt.ExportOptions(allow_partial=True))[1].source_complete
    with pytest.raises(jt.JtReferenceError):jt.read_bytes(root.read_bytes())
    with pytest.raises(jt.JtReferenceError):jt.read(root,options=jt.ReadOptions(resolver=jt.ResolverOptions(enabled=False)))


def test_cycles_and_path_escape_fail_even_best_effort(tmp_path):
    root,part=files(tmp_path)
    part.write_bytes(assembly(first="assembly.jt",second="assembly.jt"))
    options=jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort"))
    with pytest.raises(jt.JtParseError) as e:jt.read(root,options=options)
    assert e.value.code=="external_reference_cycle"
    outside=tmp_path.parent/(tmp_path.name+"-outside.jt")
    outside.write_bytes(mesh_scene())
    root.write_bytes(assembly(first="../"+outside.name))
    with pytest.raises(jt.JtParseError) as e:jt.read(root,options=options)
    assert e.value.code=="external_path_outside_root"
    link=tmp_path/"escape.jt"
    try:link.symlink_to(outside)
    except OSError:pytest.skip("symlink creation unavailable")
    root.write_bytes(assembly(first="escape.jt"))
    with pytest.raises(jt.JtParseError) as e:jt.read(root,options=options)
    assert e.value.code=="external_path_outside_root"


@pytest.mark.parametrize("path",["https://example.com/part.jt","/absolute.jt","C:\\cad\\part.jt","\\\\server\\part.jt",""])
def test_reference_paths_are_local_relative(tmp_path,path):
    root,_=files(tmp_path,first=path)
    with pytest.raises(jt.JtParseError) as e:jt.read(root)
    assert e.value.code=="invalid_external_path"


@pytest.mark.parametrize("field,value",[("max_files",1),("max_depth",0),("max_references",0),("max_total_input_bytes",1)])
def test_aggregate_file_limits(tmp_path,field,value):
    root,_=files(tmp_path)
    with pytest.raises(jt.JtLimitError):jt.read(root,options=jt.ReadOptions(resolver=replace(jt.ResolverOptions(),**{field:value})))


def test_missing_assembly_units_need_read_time_supplement(tmp_path):
    root,_=files(tmp_path,unit=None)
    with pytest.raises(jt.JtUnsupportedError,match="units"):jt.read(root)
    scene=jt.read(root,options=jt.ReadOptions(resolver=jt.ResolverOptions(meters_per_unit=.001)))
    assert scene.complete
    assert scene.structure.instances[0].source_meters_per_unit==.001
    assert all(scene.structure.instances[i.structure_instance_id].source_meters_per_unit==.0254 for i in scene.instances)


def test_shared_geometry_pays_global_budgets_once_and_root_accepts_pathlike(tmp_path):
    root,_=files(tmp_path)
    baseline=jt.read(root)
    total=sum(d.total_output_bytes for d in baseline.structure.documents)
    opts=jt.ReadOptions(mesh=jt.MeshOptions(max_meshes=1,max_vertices=4,max_triangles=4),resolver=jt.ResolverOptions(root=tmp_path))
    assert jt.read(root,options=opts,limits=jt.ReadLimits(max_total_output_bytes=total)).complete
    with pytest.raises(jt.JtLimitError):jt.read(root,options=opts,limits=jt.ReadLimits(max_total_output_bytes=total-1))
    # Each input graph fits this limit by itself, but both together do not.
    with pytest.raises(jt.JtLimitError):jt.read(root,options=replace(opts,structure=jt.StructureOptions(max_references=20)))


@pytest.mark.parametrize("mode",["strict","best_effort"])
def test_depth_limit_includes_cached_subtree(mode,tmp_path):
    root,_=files(tmp_path,first="shared.jt",second="middle.jt")
    (tmp_path/"shared.jt").write_bytes(assembly())
    (tmp_path/"middle.jt").write_bytes(assembly(first="shared.jt",second="shared.jt"))
    options=jt.ReadOptions(structure=jt.StructureOptions(mode=mode),resolver=jt.ResolverOptions(max_depth=2))
    with pytest.raises(jt.JtLimitError):jt.read(root,options=options)
    scene=jt.read(root,options=replace(options,resolver=jt.ResolverOptions(max_depth=3)))
    assert scene.complete and len(scene.structure.sources)==4 and len(scene.meshes)==1


@pytest.mark.parametrize("version",["10.0","10.3"])
def test_external_modern_part_in_legacy_assembly(version,tmp_path):
    root,part=files(tmp_path)
    part.write_bytes(modern_scene(version=version,bits=13))
    scene=jt.read(root)
    assert scene.complete and scene.structure.coordinates_in_meters
    assert [s.version for s in scene.structure.sources]==[jt.JtVersion(9,5),jt.JtVersion(10,int(version.split('.')[1]))]
    assert len(scene.meshes)==1 and len(scene.instances)==2
    data,report=jt.export_glb_bytes(scene)
    assert report.placed_triangle_count==8
    assert Glb(data).json["materials"][0]["extras"]["jt"]["originalMaterials"][0]["sourceIndex"]==1


@pytest.mark.parametrize("field,value",[("enabled",1),("root",True),("max_files",True),("max_files",-1),("max_files",2**32),("max_depth",65),("max_total_input_bytes",2**64),("meters_per_unit",True),("meters_per_unit",0.),("meters_per_unit",float("nan"))])
def test_bad_resolver_options(field,value):
    with pytest.raises((ValueError,TypeError)):
        jt.read_bytes(mesh_scene(),options=jt.ReadOptions(resolver=replace(jt.ResolverOptions(),**{field:value})))
