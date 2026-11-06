from dataclasses import FrozenInstanceError, replace
import gc
import json
import struct
import subprocess
import sys
import pytest
import cq_jt
from jt_fixture import container_for_payloads, end_marker, logical_element
from structure_fixture import (scene_fixture, element, group, partition, base_node, shape, material,
    transform, translation, IDENTITY, string_atom, mb, property_table)

def point(matrix,p):
    return tuple(sum(matrix[r*4+k]*(*p,1)[k] for k in range(4)) for r in range(3))

@pytest.mark.parametrize("endian",["<",">"])
@pytest.mark.parametrize("codec",["none","zlib","xz"])
def test_known_shared_part_placement_materials_and_units(tmp_path,endian,codec):
    data=scene_fixture(endian=endian,codec=codec)
    path=tmp_path/"構成.jt";path.write_bytes(data)
    native=cq_jt.read_structure_bytes(data,source_id="fixture")
    model=native.to_model()
    assert model==cq_jt.read_structure_file(path,source_id="fixture",options=cq_jt.StructureOptions()).to_model()
    assert native.complete and not native.geometry_decoded
    assert len(native.parts)==1 and len(native.instances)==7 and len(native.part_instances)==2
    assert native.parts[0].name=="共有部品"
    first,second=native.part_instances
    assert first.part_id==second.part_id==0
    assert first.parent_id!=second.parent_id
    # T(10,0) * R(90) applied to a known point; multiplication order matters.
    assert point(first.world_transform,(1,0,0))==(10.,1.,0.)
    assert point(second.world_transform,(1,0,0))==(11.,20.,0.)
    assert first.material.diffuse_rgb==(1.,0.,0.)
    assert second.material.diffuse_rgb==(0.,0.,1.)
    assert second.material.diffuse_alpha==0.5 and second.material.blending is True
    assert first.material.origins[1]==101 and second.material.origins[1]==104
    assert native.source.meters_per_unit==first.meters_per_unit==second.meters_per_unit==0.001
    assert first.unit_node_id==0
    assert model.properties(3)==(("JT_PROP_NAME","共有部品"),)
    assert native.parts[0].shape_node_ids==(4,)
    assert model.nodes[4].shape.polygon_count_range==(12,12)
    assert model.nodes[4].payload_references[0].segment_type==7
    assert native.document.element_bytes(1,0)==b"mesh opaque"
    assert native.atoms==model.atoms and native.bindings==model.bindings and native.nodes==model.nodes
    assert isinstance(first.world_transform,tuple) and isinstance(native.materials[0].diffuse,tuple)
    assert isinstance(native.attributes[0].transform,tuple) and isinstance(model.nodes[4].shape.quantization,tuple)
    domain=model.nodes[0].source.byte_domain
    assert domain==("file" if codec=="none" else "segment/000102030405060708090a0b0c0d0e0f/inflated")
    assert model.source.source_id=="fixture"
    assert model.to_dict()==json.loads(json.dumps(model.to_dict()))
    assert model.to_dict()["geometry_decoded"] is False
    with pytest.raises(FrozenInstanceError): native.parts[0].name="bad"
    snapshot=native.parts[0];object.__setattr__(snapshot,"name","changed")
    assert native.parts[0].name=="共有部品"
    document=native.document
    del data,native;gc.collect()
    assert document.element_bytes(1,0)==b"mesh opaque"
    assert point(model.part_instances[0].world_transform,(1,0,0))==(10.,1.,0.)

def test_material_final_force_ignore_inhibit_and_sibling_isolation():
    def colors(**kw):return [i.material for i in cq_jt.read_structure_bytes(scene_fixture(**kw)).part_instances]
    a,b=colors(root_material_flags=9)
    assert a.diffuse_rgb==b.diffuse_rgb==(1.,0.,0.)
    a,b=colors(root_material_flags=9,blue_flags=10)
    assert a.diffuse_rgb==(1.,0.,0.) and b.diffuse_rgb==(0.,0.,1.)
    a,b=colors(blue_flags=12)
    assert a.diffuse_rgb==b.diffuse_rgb==(1.,0.,0.)
    a,b=colors(blue_inhibit=1<<7)
    assert b.diffuse_rgb==(1.,0.,0.) and b.diffuse_alpha==0.5
    assert b.origins[1:3]==(101,104)
    a,b=colors(blue_inhibit=1<<1)
    assert b.diffuse_rgb==(1.,0.,0.) and b.diffuse_alpha==1.
    green=material((0.,1.,0.,1.),105)
    a,b=colors(root_material_flags=9,blue_flags=10,part_attributes=(105,),extra_graph=(green,))
    assert a.diffuse_rgb==(1.,0.,0.) and b.diffuse_rgb==(0.,0.,1.) # Force does not clear Final.

def test_transform_final_force_and_mirrors():
    native=cq_jt.read_structure_bytes(scene_fixture(root_transform_flags=9))
    assert [point(i.world_transform,(1,0,0)) for i in native.part_instances]==[(11.,0.,0.)]*2
    native=cq_jt.read_structure_bytes(scene_fixture(root_transform_flags=9,second_transform_flags=10))
    assert point(native.part_instances[1].world_transform,(1,0,0))==(11.,20.,0.)
    mirror=list(IDENTITY);mirror[0]=-2;mirror[5]=3;mirror[10]=4
    native=cq_jt.read_structure_bytes(scene_fixture(part_attributes=(105,),extra_graph=(transform(mirror,105),)))
    assert point(native.part_instances[0].world_transform,(1,1,1))==(7.,-2.,4.)

@pytest.mark.parametrize("width",[4,8])
def test_transform_precision_is_explicitly_detected(width):
    extra=transform(translation(z=0.125),105,width=width)
    native=cq_jt.read_structure_bytes(scene_fixture(part_attributes=(105,),extra_graph=(extra,)))
    assert point(native.part_instances[0].world_transform,(0,0,0))==(10.,0.,0.125)

@pytest.mark.parametrize("index,value",[(0,0.),(0,float("nan")),(0,float("inf")),(12,1.),(1,0.5)])
def test_invalid_transform_is_fatal_even_in_best_effort(index,value):
    matrix=list(IDENTITY);matrix[index]=value
    data=scene_fixture(part_attributes=(105,),extra_graph=(transform(matrix,105),))
    for mode in ["strict","best_effort"]:
        with pytest.raises(cq_jt.JtParseError):cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode=mode))

def test_cycles_missing_children_and_unknown_objects():
    for mode in ["strict","best_effort"]:
        with pytest.raises(cq_jt.JtParseError) as caught:
            cq_jt.read_structure_bytes(scene_fixture(part_children=(0,)),options=cq_jt.StructureOptions(mode=mode))
        assert caught.value.code=="graph_cycle"
    data=scene_fixture(root_children=(1,9999))
    with pytest.raises(cq_jt.JtReferenceError) as caught:cq_jt.read_structure_bytes(data)
    assert caught.value.source.byte_domain.endswith("/inflated")
    partial=cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode="best_effort"))
    assert not partial.complete and len(partial.part_instances)==1
    assert partial.missing_references[0].target=="9999"
    opaque=element(0x10dd9999,9999,b"opaque bytes",5)
    data=scene_fixture(extra_atoms=(opaque,))
    with pytest.raises(cq_jt.JtUnsupportedError):cq_jt.read_structure_bytes(data)
    partial=cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode="best_effort"))
    assert not partial.complete
    assert partial.opaque_objects[0].object_id==9999
    model=partial.to_model();atom=next(a for a in model.atoms if a.object_id==9999)
    assert atom.kind=="opaque" and atom.state_flags is None
    assert partial.document.element_bytes(model.lsg_segment_index,partial.opaque_objects[0].element_index)==b"opaque bytes"

def test_property_reference_failures_and_duplicates():
    data=scene_fixture(extra_rows=((4,[(200,9999)]),),shape_ref=False)
    with pytest.raises(cq_jt.JtReferenceError):cq_jt.read_structure_bytes(data)
    partial=cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode="best_effort"))
    assert not partial.complete and partial.missing_references[0].kind=="property value atom"
    for rows,code in [(((0,[(200,201)]),),"duplicate_property_owner"),(((4,[(200,201),(200,204)]),),"duplicate_property_key")]:
        with pytest.raises(cq_jt.JtParseError) as caught:cq_jt.read_structure_bytes(scene_fixture(extra_rows=rows,shape_ref=False))
        assert caught.value.code==code
    with pytest.raises(cq_jt.JtParseError) as caught:
        cq_jt.read_structure_bytes(scene_fixture(extra_atoms=(string_atom(200,"duplicate"),)))
    assert caught.value.code=="duplicate_object_id"

@pytest.mark.parametrize("unit,expected",[("meters",1.),("Feet",0.3048),("micrometers",1e-6),("Millimeters",0.001),("unknown",None)])
def test_units_are_preserved_without_assumptions(unit,expected):
    native=cq_jt.read_structure_bytes(scene_fixture(unit=unit))
    assert native.nodes[0].measurement_units==unit
    assert native.source.meters_per_unit==expected
    assert all(i.meters_per_unit==expected for i in native.instances)
    if expected is None:assert any(d.code=="unknown_measurement_units" for d in native.diagnostics)

def test_mixed_units_and_external_partitions_are_explicitly_partial():
    data=scene_fixture(extra_atoms=(string_atom(210,"meters"),),extra_rows=((1,[(202,210)]),))
    with pytest.raises(cq_jt.JtUnsupportedError) as caught:cq_jt.read_structure_bytes(data)
    assert caught.value.code=="mixed_unit_transform_unsupported"
    partial=cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode="best_effort"))
    assert not partial.complete
    assert len(partial.part_instances)==1 and partial.part_instances[0].meters_per_unit==0.001
    external=element(0x10dd103e,10,partition(name="../not-opened.jt"),1)
    data=scene_fixture(root_children=(1,10),extra_graph=(external,))
    with pytest.raises(cq_jt.JtReferenceError):cq_jt.read_structure_bytes(data)
    partial=cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode="best_effort"))
    assert not partial.complete and any("external partition" in r.kind for r in partial.missing_references)

def test_switch_ignore_and_lod_alternatives_are_preserved():
    switch=element(0x10dd10f3,10,group((1,2))+struct.pack("<hi",1,1),1)
    data=scene_fixture(root_children=(10,),extra_graph=(switch,))
    native=cq_jt.read_structure_bytes(data)
    assert len(native.part_instances)==1 and point(native.part_instances[0].world_transform,(0,0,0))==(10.,20.,0.)
    hidden=scene_fixture(root_flags=1)
    assert cq_jt.read_structure_bytes(hidden,options=cq_jt.StructureOptions(max_instances=0)).instances==()
    lod=element(0x10dd104c,10,group((1,2))+struct.pack("<hIihI5f",1,0,0,1,2,1.,10.,0.,0.,0.),1)
    native=cq_jt.read_structure_bytes(scene_fixture(root_children=(10,),extra_graph=(lod,)))
    node=next(n for n in native.nodes if n.object_id==10)
    assert node.kind=="range_lod" and node.lod_ranges==(1.,10.)
    assert len(native.part_instances)==2 # No implied highest-LOD selection.

def test_scalar_property_types_and_unicode_are_lossless():
    def scalar(code,i,fmt,values):return element(code,i,struct.pack("<hIh",1,0,1)+struct.pack("<"+fmt,*values),5)
    atoms=(scalar(0x10dd102b,210,"i",(-2**31,)),scalar(0x10dd1019,211,"f",(0.125,)),
           element(0xce357246,212,struct.pack("<hIh6h",1,0,1,2026,11,6,12,34,56),5,family="part"),
           string_atom(213,"部品🔩"),
           logical_element(struct.pack("<hIh4f",1,0,1,1.,2.,3.,4.),type_id=struct.pack("<IHH",0x2e7db4be,0xc71a,0x4b18)+bytes.fromhex("9d07c7227e9fef76"),object_id=214,base_type=5))
    m=cq_jt.read_structure_bytes(scene_fixture(extra_atoms=atoms)).to_model()
    values={a.object_id:a.value for a in m.atoms}
    assert values[210]==-2**31 and values[211]==0.125
    assert values[212]==(2026,11,6,12,34,56) and values[213]=="部品🔩"
    assert values[214]==(1.,2.,3.,4.)

@pytest.mark.parametrize("field,limit,code",[("max_nodes",4,"node_limit_exceeded"),("max_instances",6,"instance_limit_exceeded"),
    ("max_depth",3,"depth_limit_exceeded"),("max_references",1,"reference_limit_exceeded"),("max_string_bytes",1,"string_limit_exceeded")])
def test_structure_limits_are_enforced(field,limit,code):
    with pytest.raises(cq_jt.JtLimitError) as caught:
        cq_jt.read_structure_bytes(scene_fixture(),options=replace(cq_jt.StructureOptions(),**{field:limit}))
    assert caught.value.code==code
    assert cq_jt.read_structure_bytes(scene_fixture(),options=cq_jt.StructureOptions(max_nodes=5,max_instances=7,max_depth=4,max_references=18,max_string_bytes=50)).complete

@pytest.mark.parametrize("field",["max_nodes","max_instances","max_depth","max_references","max_string_bytes"])
@pytest.mark.parametrize("value",[-1,True,1.0,2**64])
def test_bad_structure_options(field,value):
    with pytest.raises(ValueError):cq_jt.read_structure_bytes(scene_fixture(),options=replace(cq_jt.StructureOptions(),**{field:value}))

@pytest.mark.parametrize("version",["8.1","10.1","10.2","10.6"])
def test_other_semantic_versions_are_rejected(version):
    data=container_for_payloads(version=version)
    with pytest.raises(cq_jt.JtUnsupportedError) as caught:cq_jt.read_structure_bytes(data)
    assert caught.value.code=="unsupported_structure_version"

@pytest.mark.parametrize("bad",[True,1,None,"2"])
def test_structure_api_guard(monkeypatch,bad):
    native=cq_jt.read_structure_bytes(scene_fixture())
    monkeypatch.setattr(cq_jt.structure,"STRUCTURE_API_VERSION",bad)
    for call in [lambda:cq_jt.read_structure_bytes(scene_fixture()),lambda:native.to_model(),lambda:native.parts,lambda:native.document]:
        with pytest.raises(ImportError,match="STRUCTURE_API_VERSION"):call()

def test_bad_strings_versions_and_materials_fail_closed():
    bad=element(0x10dd106e,999,struct.pack("<hIhiH",1,0,1,1,0xd800),5)
    with pytest.raises(cq_jt.JtParseError) as caught:cq_jt.read_structure_bytes(scene_fixture(extra_atoms=(bad,)))
    assert caught.value.code=="invalid_utf16"
    bad=material((float("nan"),0.,0.,1.),105)
    with pytest.raises(cq_jt.JtParseError):cq_jt.read_structure_bytes(scene_fixture(extra_graph=(bad,)))
    bad=element(0x10dd101b,10,struct.pack("<h",2)+group(),1)
    with pytest.raises(cq_jt.JtUnsupportedError):cq_jt.read_structure_bytes(scene_fixture(extra_graph=(bad,)))
    assert not cq_jt.read_structure_bytes(scene_fixture(extra_graph=(bad,)),options=cq_jt.StructureOptions(mode="best_effort")).complete

def test_structure_io_and_argument_errors(tmp_path):
    with pytest.raises(FileNotFoundError):cq_jt.read_structure_file(tmp_path/"absent.jt")
    with pytest.raises(TypeError):cq_jt.read_structure_file(bytes(tmp_path))
    with pytest.raises(TypeError):cq_jt.read_structure_bytes(bytearray(scene_fixture()))
    with pytest.raises(ValueError):cq_jt.read_structure_bytes(scene_fixture(),options=object())
    with pytest.raises(ValueError):cq_jt.read_structure_bytes(scene_fixture(),options=cq_jt.StructureOptions(mode="guess"))

def test_structure_cli(tmp_path):
    path=tmp_path/"fixture.jt";path.write_bytes(scene_fixture())
    command=[sys.executable,"-m","cq_jt","structure",str(path)]
    result=subprocess.run(command+["--json"],text=True,capture_output=True)
    assert result.returncode==0,result.stderr
    assert json.loads(result.stdout)==cq_jt.read_structure_file(path).to_model().to_dict()
    assert subprocess.run(command,capture_output=True).returncode==0
    limited=subprocess.run(command+["--json","--max-instances","0"],text=True,capture_output=True)
    assert limited.returncode==1 and json.loads(limited.stdout)["error"]["code"]=="instance_limit_exceeded"
    assert subprocess.run(command+["--max-depth",str(2**32)],capture_output=True).returncode==2
    path.write_bytes(scene_fixture(root_children=(9999,)))
    assert subprocess.run(command+["--json"],capture_output=True).returncode==1
    partial=subprocess.run(command+["--json","--mode","best_effort"],text=True,capture_output=True)
    assert partial.returncode==0 and not json.loads(partial.stdout)["complete"]

def test_graph_and_property_lists_are_not_interchangeable():
    for data in [scene_fixture(extra_graph=(string_atom(999,"wrong list"),)),
                 scene_fixture(extra_atoms=(element(0x10dd101b,999,group(),1),))]:
        for mode in ["strict","best_effort"]:
            with pytest.raises(cq_jt.JtParseError) as caught:
                cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode=mode))
            assert caught.value.code=="invalid_lsg_list"

def test_missing_attributes_never_fabricate_default_placements():
    data=scene_fixture(part_attributes=(9999,))
    with pytest.raises(cq_jt.JtReferenceError):cq_jt.read_structure_bytes(data)
    partial=cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode="best_effort"))
    assert not partial.complete and partial.part_instances==()
    assert len(partial.parts)==1 and partial.missing_references[0].kind=="attribute"

@pytest.mark.parametrize("endian",["<",">"])
def test_jt95_conditional_guid_and_toc_header_overlap(endian):
    original=scene_fixture(endian=endian)
    info=cq_jt.inspect_bytes(original)
    data=bytearray(original[:105]+b"\xab"*16+original[105:])
    struct.pack_into(endian+"i",data,81,1)
    struct.pack_into(endian+"i",data,85,121)
    for segment in info.segments:
        struct.pack_into(endian+"i",data,segment.toc_source.start_offset+32,segment.offset+16)
    scene=cq_jt.read_structure_bytes(bytes(data))
    assert scene.complete and scene.document.container.header_size==121
    assert scene.document.container.extra_header_guid==b"\xab"*16
    struct.pack_into(endian+"i",data,85,105)
    with pytest.raises(cq_jt.JtParseError) as caught:cq_jt.read_structure_bytes(bytes(data))
    assert caught.value.code=="toc_overlaps_header"

def test_late_segment_references_preserve_or_reject_missing_and_mismatched_types():
    def late(guid,kind,reserved=1):
        return element(0,999,struct.pack("<hIh",1,0,1)+guid+struct.pack("<iii",kind,0,reserved),8,family="late")
    data=scene_fixture(extra_atoms=(late(b"x"*16,7),))
    with pytest.raises(cq_jt.JtReferenceError):cq_jt.read_structure_bytes(data)
    assert not cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(mode="best_effort")).complete
    for data,code in [(scene_fixture(extra_atoms=(late(bytes(range(1,17)),17),)),"late_segment_type_mismatch"),
                      (scene_fixture(extra_atoms=(late(bytes(range(1,17)),7,0),)),"invalid_late_reference")]:
        with pytest.raises(cq_jt.JtParseError) as caught:cq_jt.read_structure_bytes(data)
        assert caught.value.code==code
