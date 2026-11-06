"""Generate authored GLB controls and independently inspect their numerical data."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import sys

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/"tests"))
from glb_reader import Glb
from mesh_fixture import mesh_scene, tetra_payload
from modern_fixture import modern_scene
from assembly_fixture import assembly
import tempfile
import cq_jt as jt


def unpack(array):
    fmt={"f64":"d","u32":"I"}[array.dtype]*array.shape[1]
    return tuple(struct.iter_unpack("<"+fmt,array.data))


def direction(m,n):
    a,b,c=([m[i],m[4+i],m[8+i]] for i in range(3))
    cross=lambda a,b:(a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0])
    co=(cross(b,c),cross(c,a),cross(a,b));det=sum(a[i]*co[0][i] for i in range(3))
    out=tuple(sum(co[k][i]*n[k] for k in range(3))/det for i in range(3))
    length=math.hypot(*out);return tuple(v/length for v in out)


def export_case(name,scene,options,folder,render=False,source_kind="authored_control"):
    data,report=jt.export_glb_bytes(scene,options=options);glb=Glb(data);world=glb.world_positions();matrices=glb.matrices()
    assert len(world)==report.instance_count
    output=folder/(name+".glb");output.write_bytes(data)
    expected=[];states=scene.structure.instances;max_normal_error=0.;max_local_normal_error=0.;checked_corners=0
    nodes={node["extras"]["jt"]["instanceId"]:(i,node) for i,node in enumerate(glb.json["nodes"]) if "mesh" in node}
    coordinate=lambda p: p if options.up_axis=="z" else (p[0],-p[2],p[1]) # glTF Y-up to Blender Z-up
    for occurrence in scene.instances:
        state=states[occurrence.structure_instance_id];buffers=scene.mesh_buffers(occurrence.mesh_id)
        positions=unpack(buffers.positions);triangles=unpack(buffers.triangles)
        if not triangles:continue
        units=state.meters_per_unit if state.meters_per_unit is not None else options.meters_per_unit
        assert units==report.meters_per_unit
        transform=lambda p:tuple((sum(state.world_transform[r*4+k]*p[k] for k in range(3))+state.world_transform[r*4+3])*units for r in range(3))
        raw=[transform(positions[index]) for tri in triangles for index in tri]
        if options.up_axis=="z":gltf=[(p[0],p[2],-p[1]) for p in raw]
        else:gltf=raw
        actual=world[state.id]
        actual_bounds=[min(p[i] for p in actual) for i in range(3)]+[max(p[i] for p in actual) for i in range(3)]
        target_bounds=[min(p[i] for p in gltf) for i in range(3)]+[max(p[i] for p in gltf) for i in range(3)]
        assert max(abs(a-b) for a,b in zip(actual_bounds,target_bounds,strict=True))<2e-6
        normal_samples=[]
        node_index,node=nodes[state.id]
        primitive=glb.json["meshes"][node["mesh"]]["primitives"][0]
        indices=glb.accessor(primitive["indices"])
        encoded_positions=glb.accessor(primitive["attributes"]["POSITION"])
        assert len(indices)==3*len(triangles)
        for corner,(encoded_index,) in enumerate(indices):
            original=positions[triangles[corner//3][corner%3]]
            assert max(abs(original[i]*units-encoded_positions[encoded_index][i]) for i in range(3))<=report.max_position_error_meters+1e-15
            checked_corners+=1
        if buffers.normal_indices is not None:
            normals=unpack(buffers.normals);normal_indices=unpack(buffers.normal_indices)
            glb_normals=glb.accessor(primitive["attributes"]["NORMAL"])
            pairs={(normal_indices[c//3][c%3],index) for c,(index,) in enumerate(indices)}
            for normal_index,encoded_index in pairs:
                original=normals[normal_index];length=math.hypot(*original)
                normal_error=math.dist(tuple(v/length for v in original),glb_normals[encoded_index])
                assert normal_error<1e-6,(name,state.id,normal_error)
                max_local_normal_error=max(max_local_normal_error,normal_error)
            for corner in range(3):
                index=triangles[0][corner];n=normal_indices[0][corner]
                target=direction(state.world_transform,normals[n])
                if options.up_axis=="z":target=(target[0],target[2],-target[1])
                actual_normal=direction(matrices[node_index],glb_normals[indices[corner][0]])
                normal_error=math.dist(actual_normal,target)
                assert normal_error<1e-6,(name,state.id,normal_error)
                max_normal_error=max(max_normal_error,normal_error)
                normal_samples.append({"position":coordinate(transform(positions[index])),"normal":coordinate(direction(state.world_transform,normals[n]))})
        points=[coordinate(p) for p in raw]
        rgb=state.material.diffuse_rgb or options.default_color[:3]
        alpha=state.material.diffuse_alpha if state.material.diffuse_alpha is not None else options.default_color[3]
        if options.color_space=="srgb":rgb=tuple(v/12.92 if v<=.04045 else ((v+.055)/1.055)**2.4 for v in rgb)
        blend=state.material.blending if state.material.blending is not None else alpha<1.
        expected.append({"instance_id":state.id,"triangles":len(triangles),"bounds":[min(p[i] for p in points) for i in range(3)]+[max(p[i] for p in points) for i in range(3)],"base_color":[*rgb,alpha if blend else 1.],"normal_samples":normal_samples})
    assert len(expected)==report.instance_count
    return {"name":name,"source_kind":source_kind,"path":str(output.resolve()),"sha256":hashlib.sha256(data).hexdigest(),"render":render,"report":report.to_dict(),"expected_objects":expected,"node_count":len(matrices),"checked_triangle_corners":checked_corners,"max_glb_local_normal_error":max_local_normal_error,"max_glb_normal_direction_error":max_normal_error}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--output-dir",type=Path,required=True);parser.add_argument("--report",type=Path,required=True);args=parser.parse_args()
    args.output_dir.mkdir(parents=True,exist_ok=True)
    basic={"root_color":(1.,0.,0.,1.),"unit":"Millimeters"}
    angle=math.sqrt(.5)
    rotate=(angle,-angle,0.,0.,angle,angle,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)
    parent=(2.,0.,0.,10.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)
    mirror=(-1.,0.,0.,0.,0.,2.,0.,20.,0.,0.,3.,0.,0.,0.,0.,1.)
    cases=[("shared-mm",basic,jt.ReadOptions(),jt.ExportOptions(),True),
           ("meters",{**basic,"unit":"Meters"},jt.ReadOptions(),jt.ExportOptions(),False),
           ("inches",{**basic,"unit":"Inches"},jt.ReadOptions(),jt.ExportOptions(),False),
           ("mirror-shear",{**basic,"root_matrix":parent,"first_matrix":rotate,"second_matrix":mirror},jt.ReadOptions(),jt.ExportOptions(),True),
           ("z-up-srgb",{**basic,"root_color":(.5,.5,.5,1.)},jt.ReadOptions(),jt.ExportOptions(up_axis="z",color_space="srgb"),False),
           ("no-normals",{**basic,"payload":tetra_payload(normals=False)},jt.ReadOptions(),jt.ExportOptions(),False),
           ("lod-highest",{**basic,"lod":True},jt.ReadOptions(lod="highest"),jt.ExportOptions(),False),
           ("lod-lowest",{**basic,"lod":True},jt.ReadOptions(lod="lowest"),jt.ExportOptions(),False),
           ("partial",{**basic,"root_children":(1,2,999)},jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort")),jt.ExportOptions(allow_partial=True),False),
           ("supplied-units-color",{},jt.ReadOptions(),jt.ExportOptions(meters_per_unit=.001,default_color=(.2,.4,.6,1.)),False)]
    records=[]
    for name,kwargs,read,options,render in cases:
        records.append(export_case(name,jt.read_bytes(mesh_scene(**kwargs),options=read),options,args.output_dir,render))
    for version in ("10.0", "10.3", "10.6"):
        read = jt.ReadOptions(structure=jt.StructureOptions(experimental_jt106=version=="10.6"))
        for endian, order in (("<", "le"), (">", "be")):
            for bits in (0, 3, 13):
                data = modern_scene(version=version, endian=endian, bits=bits, translation=(2., 3., 4.))
                records.append(export_case(f"jt{version}-{order}-normal{bits}", jt.read_bytes(data, options=read), jt.ExportOptions(), args.output_dir))
    with tempfile.TemporaryDirectory(prefix="cq-jt-export-controls-") as directory:
        folder = Path(directory)
        root = folder / "assembly.jt"
        root.write_bytes(assembly(final_color=True))
        (folder / "part.jt").write_bytes(mesh_scene(unit="Inches", root_color=(1., 0., 0., 1.)))
        records.append(export_case("external-mm-inches", jt.read(root), jt.ExportOptions(), args.output_dir))
        root.write_bytes(assembly(second="missing.jt"))
        partial = jt.read(root, options=jt.ReadOptions(structure=jt.StructureOptions(mode="best_effort")))
        records.append(export_case("external-missing", partial, jt.ExportOptions(allow_partial=True), args.output_dir))
        root.write_bytes(assembly())
        (folder / "part.jt").write_bytes(modern_scene(version="10.6"))
        read = jt.ReadOptions(structure=jt.StructureOptions(experimental_jt106=True))
        records.append(export_case("external-experimental", jt.read(root, options=read), jt.ExportOptions(), args.output_dir))
    args.report.write_text(json.dumps({"schema_version":1,"package_version":jt.__version__,"scope":"authored inputs; independent GLB coordinates, normals and world bounds; no native CAD compatibility claim","cases":records},indent=2,ensure_ascii=False)+"\n")
    print(json.dumps({"glb_files":len(records),"render_cases":sum(r["render"] for r in records)}))


if __name__=="__main__":main()
