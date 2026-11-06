import struct
from dataclasses import replace
import pytest
import cq_jt as jt
from modern_fixture import modern_scene
from glb_reader import Glb


@pytest.mark.parametrize("version",["10.0","10.3"])
@pytest.mark.parametrize("endian",["<",">"])
@pytest.mark.parametrize("bits",[0,3,13])
def test_authored_modern_layouts_and_corner_normals(version,endian,bits):
    scene=jt.read_bytes(modern_scene(version=version,endian=endian,bits=bits))
    assert scene.complete and not scene.structure.coordinates_in_meters
    assert len(scene.meshes)==1 and len(scene.instances)==1
    mesh=scene.meshes[0];buffers=scene.mesh_buffers(0)
    assert (mesh.vertex_count,mesh.triangle_count,mesh.normal_count)==(4,4,4)
    assert tuple(struct.iter_unpack("<3d",buffers.positions.data))==((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.))
    normals=tuple(struct.iter_unpack("<3d",buffers.normals.data))
    for actual,expected in zip(normals,((0.,0.,1.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.)),strict=True):assert actual==pytest.approx(expected,abs=1e-12)
    assert scene.structure.nodes[2].shape.transformed_bounds is None
    assert scene.structure.nodes[2].shape.quantization is None
    assert scene.structure.materials[0].bumpiness==pytest.approx(.2)
    assert scene.structure.materials[0].field_final_flags==0
    assert scene.structure.atoms[-1].value.reserved is None
    glb,report=jt.export_glb_bytes(scene)
    assert report.triangle_count==4 and report.meters_per_unit==.001
    assert Glb(glb).json["materials"][0]["alphaMode"]=="BLEND"


def test_modern_palette_boundary_and_truncated_data():
    assert jt.read_bytes(modern_scene(base_version=2)).complete
    with pytest.raises(jt.JtUnsupportedError) as e:jt.read_bytes(modern_scene(base_version=2,palette=17))
    assert e.value.code=="unsupported_attribute_palette"
    data=modern_scene()
    for i in range(len(data)):
        with pytest.raises(jt.JtError):jt.read_bytes(data[:i])
    # The normalized decoder still honors its CODEC allocation budget.
    with pytest.raises(jt.JtLimitError):jt.read_bytes(data,options=jt.ReadOptions(mesh=jt.MeshOptions(max_codec_values=1)))


def test_modern_field_final_is_independent_of_diffuse_alpha_and_force():
    scene=jt.read_bytes(modern_scene(parent_final=True))
    state=scene.structure.instances[-1].material
    assert state.diffuse_rgb==(0.,1.,0.) and state.diffuse_alpha==.5
    assert state.origins[1:3]==(4,3)
    scene=jt.read_bytes(modern_scene(parent_final=True,child_force=True))
    assert scene.structure.instances[-1].material.diffuse_rgb==(1.,0.,0.)
