"""Clean-install a wheel and wait for normal native interpreter shutdown. Requires uv."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

from build_python_release import build_environment
from check_python_release import check_project, check_wheel

SMOKE = r'''
import gc, importlib.machinery, importlib.metadata, json, pathlib, struct, subprocess, sys, tempfile
import cq_jt
from cq_jt import Diagnostic, JtVersion, RawElement, Scene, SourceMetadata, SourceSpan
for module in (cq_jt, cq_jt._native):
    assert pathlib.Path(module.__file__).resolve().is_relative_to(pathlib.Path(sys.prefix).resolve()), module.__file__
assert any(cq_jt._native.__file__.endswith(s) for s in importlib.machinery.EXTENSION_SUFFIXES)
assert cq_jt.__version__ == cq_jt._native.CORE_VERSION == importlib.metadata.version("cq-jt")
assert cq_jt.MODEL_API_VERSION == cq_jt._native.MODEL_API_VERSION == 1
span = SourceSpan("input", "segment/opaque/inflated", 2**63, 2**63 + 3)
model = Scene((SourceMetadata("input", "part.jt", JtVersion(9, 5)),),
              (RawElement(bytes(range(16)), -(2**63), b"\0\xff\0", span),),
              (Diagnostic("opaque", "kept", source=span),))
for _ in range(50):
    native = model.to_native()
    assert native.to_model() == model
assert native.sources[0].meters_per_unit is None
assert native.raw_elements[0].source.start_offset == 2**63
try:
    Scene((SourceMetadata("invalid", meters_per_unit=0.0),))
except cq_jt.JtModelError as error:
    assert isinstance(error, ValueError) and error.code == "invalid_units"
else:
    raise AssertionError("invalid model was accepted")
# Generate framing locally; no third-party JT data or compression stream.
guid = bytes(range(16))
header = b"Version 9.5 smoke".ljust(75, b" ") + b" \n\r\n "
header += struct.pack("<Bii", 0, 0, 105) + guid
toc = struct.pack("<i", 1) + guid + struct.pack("<iII", 137, 33, 1 << 24)
segment = guid + struct.pack("<iiIiB", 1, 33, 0, 1, 1)
data = header + toc + segment
assert len(data) == 170
info = cq_jt.inspect_bytes(data, source_id="smoke")
assert info.version == JtVersion(9, 5) and info.segment_count == 1
assert info.lsg_guid_raw == guid and info.segments[0].guid_raw == guid
assert info.segments[0].compression.label == "None"
assert info.segments[0].header_source == SourceSpan("smoke", "file", 137, 161)
assert info.payload_decoded is False
assert cq_jt.INSPECT_API_VERSION == cq_jt._native.INSPECT_API_VERSION == 2
try:
    cq_jt.inspect_bytes(data, limits=cq_jt.ReadLimits(max_segments=0))
except cq_jt.JtLimitError as error:
    assert error.code == "segment_limit_exceeded" and error.offset == 105
else:
    raise AssertionError("segment limit was ignored")
with tempfile.TemporaryDirectory() as directory:
    path = pathlib.Path(directory) / "smoke.jt"
    path.write_bytes(data)
    assert cq_jt.inspect_file(path, source_id="smoke") == info
    command = [sys.executable, "-I", "-X", "faulthandler", "-m", "cq_jt", "inspect", str(path), "--json"]
    result = subprocess.run(command, check=True, capture_output=True, text=True)
    assert json.loads(result.stdout) == cq_jt.inspect_file(path).to_dict()
    path.write_bytes(b"broken")
    result = subprocess.run(command, capture_output=True, text=True)
    assert result.returncode == 1
    assert json.loads(result.stdout)["error"]["code"] == "unexpected_eof"
# Synthetic logical payloads and independent stdlib codecs; no JT originals.
import zlib, lzma
element = struct.pack("<i", 24) + bytes([49])*16 + b"\xff" + struct.pack("<i", -7) + b"\0\xffx"
marker = struct.pack("<i", 16) + b"\xff"*16
payload = element + marker + marker + b"\x01\0\0\0\0\0"
for flag, algorithm, encoded in [(2, 2, zlib.compress(payload)), (3, 3, lzma.compress(payload)), (0, 1, payload)]:
    length = 33 + len(encoded)
    data = header + struct.pack("<i", 1) + guid + struct.pack("<iII", 137, length, 1 << 24)
    data += guid + struct.pack("<iiIiB", 1, length, flag, len(encoded)+1, algorithm) + encoded
    document = cq_jt.decode_bytes(data, source_id="smoke")
    assert cq_jt.PAYLOAD_API_VERSION == cq_jt._native.PAYLOAD_API_VERSION == 1
    assert document.segment_bytes(0) == payload
    assert document.raw_element(0, 0).raw_data == b"\0\xffx"
    assert document.segments[0].elements[0].object_id == -7
    assert document.geometry_decoded is False
    try:
        cq_jt.decode_bytes(data, limits=cq_jt.ReadLimits(max_total_output_bytes=0))
    except cq_jt.JtLimitError as error:
        assert error.code == "output_limit_exceeded"
    else:
        raise AssertionError("output limit was ignored")
    with tempfile.TemporaryDirectory() as directory:
        path = pathlib.Path(directory)/"payload.jt"
        path.write_bytes(data)
        assert cq_jt.decode_file(path).segment_bytes(0) == payload
        command = [sys.executable, "-I", "-m", "cq_jt", "decode", str(path), "--json"]
        report = json.loads(subprocess.run(command, check=True, capture_output=True, text=True).stdout)
        assert report["outer_decoded"] and report["geometry_decoded"] is False
gc.collect()
# Author a small shared-part graph in the fresh environment, independently of
# repository fixtures. This is structure data only, not a CAD model.
def element(code, object_id, base, body, part=False):
    type_id = struct.pack("<IHH", code, 0x38fb if part else 0x2ac8, 0x11d1)
    type_id += bytes.fromhex("a506006097bdc6e1" if part else "9b6b0080c7bb5997")
    return struct.pack("<i",21+len(body))+type_id+bytes([base])+struct.pack("<i",object_id)+body
def node(attributes=()):
    return struct.pack("<hII",1,0,len(attributes))+struct.pack("<"+"i"*len(attributes),*attributes)
def group(children=(), attributes=()):
    return node(attributes)+struct.pack("<hI",1,len(children))+struct.pack("<"+"i"*len(children),*children)
root = element(0x10dd103e,0,1,group((1,2),(100,101))+struct.pack("<ii7f6i",0,0,*([0.]*7),*([0]*6)))
i1 = element(0x10dd102a,1,0,node((102,))+struct.pack("<hi",1,3))
i2 = element(0x10dd102a,2,0,node((103,104))+struct.pack("<hi",1,3))
part = element(0xce357244,3,1,group()+struct.pack("<hhi",1,1,0),True)
def transform(object_id, mask, values):
    return element(0x10dd1083,object_id,3,struct.pack("<hBIhH",1,8,0,1,mask)+struct.pack("<"+"d"*len(values),*values))
def material(object_id, color):
    return element(0x10dd1030,object_id,3,struct.pack("<hBIhH",1,8,0,2,0x10)+struct.pack("<18f",*(0.,0.,0.,1.),*color,*(0.,0.,0.,1.),*(0.,0.,0.,1.),16.,0.))
payload = root+i1+i2+part+transform(100,8,[10.])+material(101,(1.,0.,0.,1.))+transform(102,0xcc00,[0.,1.,-1.,0.])+transform(103,4,[20.])+material(104,(0.,0.,1.,0.5))+marker+marker+struct.pack("<hi",1,0)
encoded = zlib.compress(payload);length=33+len(encoded)
data = header+struct.pack("<i",1)+guid+struct.pack("<iII",137,length,1<<24)
data += guid+struct.pack("<iiIiB",1,length,2,len(encoded)+1,2)+encoded
scene = cq_jt.read_structure_bytes(data)
assert cq_jt.STRUCTURE_API_VERSION == cq_jt._native.STRUCTURE_API_VERSION == 3
assert scene.complete and scene.geometry_decoded is False
assert len(scene.parts)==1 and len(scene.part_instances)==2
first,second=scene.part_instances
assert first.part_id==second.part_id==0
assert (first.world_transform[0]+first.world_transform[3],first.world_transform[4]+first.world_transform[7])==(10.,1.)
assert second.world_transform[7]==20.
assert first.material.diffuse_rgb==(1.,0.,0.) and second.material.diffuse_rgb==(0.,0.,1.)
assert second.material.diffuse_alpha==0.5 and first.meters_per_unit is None
assert isinstance(scene.to_model().instances[0].world_transform,tuple)
try:
    cq_jt.read_structure_bytes(data,options=cq_jt.StructureOptions(max_instances=1))
except cq_jt.JtLimitError as error:
    assert error.code=="instance_limit_exceeded"
else:
    raise AssertionError("structure limit ignored")
with tempfile.TemporaryDirectory() as directory:
    path=pathlib.Path(directory)/"structure.jt";path.write_bytes(data)
    assert cq_jt.read_structure_file(path,source_id="input").parts==scene.parts
    report=json.loads(subprocess.run([sys.executable,"-I","-m","cq_jt","structure",str(path),"--json"],check=True,text=True,capture_output=True).stdout)
    assert report["complete"] and report["geometry_decoded"] is False
# Frozen authored tetrahedron from tests/mesh_fixture.py, with two placements.
mesh_input = zlib.decompress(bytes.fromhex("789c0b4b2d2acecccf53b0d4335528aecc2bc9482dc94c562848acccc94f4c51200d70f17229308040268860646266616563e7e0e4e2e6e1e5e367c222b6142854cf08d6c2882625a002d4b014ac89811d5d1f23541f483a1e4457cc599d9690b04178877681d3eb450fdee4b4b53ddeadbcdc98e9a8c40196193e8c977d02e48499593630170a3adb094c33786198c2e859d4a02690bd30927f1b5b62add68c34b649bb6e88181f146d8ec87498c3773d81e3889eebb2e775d1312231a78e4d30a87dbfb1bde74c01af80b4438e984577e342e98ed3177312931c2fb730f0fe6ebc21d724f9a1c3fcc6e603ba490dd15c877803647d3e6c68fdc9c662d2b043f2ace599091a0f131c021c163c7ad0e09babb529aacd78c5ba3f0ca577dbff1a08aef9b65471f641a6d0cc47131855782f5c5da67cc5c890c3608eede364f6c32b16b2a42d4eec0960e9b49c675b7c2c8ce1c4ef7eb63b8d499519df36cf4b70582297a8f0f2a8c02181f30f985b18791c79fe3918cd539c71b0f9a0e089a2d40f291f582fd9f14c7398eabfd8dce6c6efdf8b1e3c7acab35a42afa8ad608e8b51d7c4ce99ad537be7364fee9edd3ebd3f9e61cabfc90b3fddb8d3e611a1c03eb7e5173bc32b93b9e1e851c3ce00899a52205e2d7057e084d645c1d9d90c0dc777474e67014726107231400013033b98640671788098190983e39e05cac105c0460aa0e9644696c0a903af718c689814303f9483970b89cf828489f20b08d413e3640c897a5c12788d0265179c3a36ed79fc0ce272baba0fd97cbcee3b7e8d7f1922453182d5fc470300b13fd963"))
mesh_scene = cq_jt.read_bytes(mesh_input)
assert cq_jt.MESH_API_VERSION == cq_jt._native.MESH_API_VERSION == 2
assert mesh_scene.complete and mesh_scene.geometry_decoded
assert len(mesh_scene.meshes)==1 and len(mesh_scene.instances)==2
mesh=mesh_scene.meshes[0];buffers=mesh_scene.mesh_buffers(mesh.id)
assert (mesh.vertex_count,mesh.triangle_count,mesh.normal_count)==(4,4,4)
assert buffers.positions.dtype=="f64" and buffers.positions.shape==(4,3)
assert tuple(struct.iter_unpack("<3I",buffers.triangles.data))==((0,1,2),(2,1,3),(2,3,0),(3,1,0))
assert buffers.positions.view().readonly
assert mesh_scene.instances[0].mesh_id==mesh_scene.instances[1].mesh_id
snapshot=mesh_scene.to_model()
try:
    cq_jt.read_bytes(mesh_input,options=cq_jt.ReadOptions(mesh=cq_jt.MeshOptions(max_triangles=0)))
except cq_jt.JtLimitError:
    pass
else:
    raise AssertionError("mesh limit ignored")
with tempfile.TemporaryDirectory() as directory:
    path=pathlib.Path(directory)/"mesh.jt";path.write_bytes(mesh_input)
    assert cq_jt.read(path,source_id="input").to_model()==snapshot
    report=json.loads(subprocess.run([sys.executable,"-I","-m","cq_jt","mesh",str(path),"--json"],check=True,text=True,capture_output=True).stdout)
    assert report["complete"] and report["geometry_decoded"] and len(report["instances"])==2
    assert cq_jt.EXPORT_API_VERSION == cq_jt._native.EXPORT_API_VERSION == 1
    options=cq_jt.ExportOptions(meters_per_unit=.001,default_color=(1.,0.,0.,1.))
    glb,export_report=cq_jt.export_glb_bytes(mesh_scene,options=options)
    assert glb[:4]==b"glTF" and struct.unpack_from("<II",glb,4)==(2,len(glb))
    n,kind=struct.unpack_from("<I4s",glb,12);assert kind==b"JSON"
    meta=json.loads(glb[20:20+n])
    assert meta["asset"]["version"]=="2.0" and len(meta["meshes"])==2
    assert export_report.instance_count==2 and export_report.placed_triangle_count==8
    assert export_report.source_complete and export_report.unit_fallback_used
    output=path.with_suffix(".glb")
    assert cq_jt.convert(path,output,export_options=options).byte_length==export_report.byte_length
    assert output.read_bytes()==glb
    try:
        cq_jt.export_glb(mesh_scene,output,options=options)
    except FileExistsError:
        pass
    else:
        raise AssertionError("existing GLB was overwritten")
    assert output.read_bytes()==glb
    command=[sys.executable,"-I","-m","cq_jt","convert",str(path),str(output),"--json","--overwrite","--meters-per-unit",".001","--default-color","1","0","0","1"]
    converted=json.loads(subprocess.run(command,check=True,text=True,capture_output=True).stdout)
    assert converted["instance_count"]==2 and output.read_bytes()==glb

# Authored modern JT and external assembly; no third-party CAD input.
modern_input=zlib.decompress(bytes.fromhex("789c0b4b2d2acecccf533034d0335028aecc2bc9482dc94c562848acccc94f4c51200970f172293080402e0314303231b3b0b2b173707271f3f0f2f1336111db08555ac708d181262da0cf04911785d0ece8fa19a17a41d27120ba62cedc406f6fa640917f69c7b6dbba457d156f52e258e8db3561c2c2d495dd19898e8edf588c9bc2fe718beb2a3785bd75be357519db67e6a6a82fda3157cad797bba5175cdd59937ecef2bbcd3ecbef6c0e87fe98441787bdf9c6f275fbf3b96dc724964c6a5b6623d929f47b8713affb0e07d1768b034c7d2fffbb3febbc9af567f1fe9ca3ef986f649bc4ef9f5b79f8e5aedc028d82b70b57bf2e6b0c3896611a50af3889d9c3fee5e26fbd7f26bdbdfeb65330758e70f7bcc8b61cf9a21b335f946ee6947e171834296fc7bb059f82f7e695bb5e99ddf3a72ac5dbe0e3860fdbaf042d381fb4767187d1a71bc7d44a252d974c74625475bb74e9f60449d53ae32d5c1dfdd9b7ca6675b99f5bd8143c8febdc8f13a68bbabcb74905ff782772ce2267938db31beb9a8981f39e4a9a9dfb9cd739d37eb77ffcd97da2bbab0f28af2930bc522895b9ebe167f6a685052a568696a6b96c57f71dcbf590be127024ead6293143c7358bffd887a4f4df478f1d7668cc3c0586fc6a81bb0227b42e0acece666838be3b723a0b243eb9a0117c10a864ed52831f3efbf6f8cf8edfa936a9e2d2434e50ec31820c616406a9e1016266240c4e132c500e2e00b647004d2733b2044e1d788d6344c3c4823bdfef377121f159903051fe8080067b3cce8548e2f047c3fe7f6957ff43aca3c850846a90a13e7c3627184112ffd100004bf9f75e"))
modern_scene=cq_jt.read_bytes(modern_input)
assert modern_scene.complete and modern_scene.meshes[0].triangle_count==4
assert modern_scene.structure.source.version==JtVersion(10,0)
assert modern_scene.structure.nodes[2].shape.transformed_bounds is None
assert cq_jt.export_glb_bytes(modern_scene)[1].triangle_count==4
assembly_input=zlib.decompress(bytes.fromhex("789c0b4b2d2acecccf53b0d4335528aecc2bc9482dc94c562848acccc94f4c51200d70f17229308040268860646266616563e7e0e4e2e6e1e5e367c422d60914aa054b303062530f926302d2b120ba62ced6e08d7e870d046a7d9b1c9a14656f4a482c6076e2124a5230719275d8a2e0c5be6c95c63a290d91ad0606829dfd090f827f30446930cb1fd87e60f36cb3bc143181f4bbd6f75fde2c178f7fc7f8e3ddf2dbfa354fe6eb09ecb7996ff74ca29929e2ca04a9951d538fe8f6a44cbbe5dafe9a2ddb42fd9f5bd8ca561f4eadb32b583a9d227fab6f3275cdd37a2d60bdecd91b4ec960eb9b674ca5725ee89f9508b6fa3faba8f5d704cfcc76ad8b3241ce917a523b9d9e5b71961f3e93fbeace47991f47c50a6d4ff706ad63157f7f98b97e8dc31ee5b5cf2bb7383f9dbc6ce527d38975321376bfbdf4fbd409b1e4f0997f8cd77b1c4f6adcd12f7ca5b0255946e8f50a050749d537b3f8f667b67f3ec296f7d34ed7ae60fa6389a4ce8972d30fa89ebb704c76d5b2ec5fb74cbbdba452fe35b867f74c99121ed823f556ae3c3467d63ad98cebe7844fff71be1d23b85a704df08653723ef15ae633ed7f56a64cdf74c451e8379bcef47d95004b49b63f"))
with tempfile.TemporaryDirectory() as directory:
    root=pathlib.Path(directory)/"assembly.jt";root.write_bytes(assembly_input)
    (root.parent/"part.jt").write_bytes(mesh_input)
    options=cq_jt.ReadOptions(resolver=cq_jt.ResolverOptions(meters_per_unit=.001))
    assembled=cq_jt.read(root,options=options)
    assert assembled.complete and assembled.structure.coordinates_in_meters
    assert len(assembled.structure.sources)==len(assembled.structure.documents)==2
    assert len(assembled.meshes)==1 and len(assembled.instances)==4
    assert cq_jt.export_glb_bytes(assembled)[1].placed_triangle_count==16
    converted=json.loads(subprocess.run([sys.executable,"-I","-m","cq_jt","convert",str(root),str(root.with_suffix(".glb")),"--json","--assembly-meters-per-unit",".001"],check=True,text=True,capture_output=True).stdout)
    assert converted["instance_count"]==4

# Authored 10.6 specification control; default rejection remains the contract.
jt106_input=zlib.decompress(bytes.fromhex("789c0b4b2d2acecccf533034d0335328aecc2bc9482dc94c562848acccc94f4c51200970f172293080402e0314303231b3b0b2b173707271f3f0f2f1336111db08555acd08d181262da0c304911785d0ece8fa19a17a41d2d120ba62ceea9484840dc23bb40b9c5e2f7af026a7adedf16ee5e5c64c47250eb0cc38c8f1b4d1c05c40dd688ad11483148327c622864b8c9618bc304c61fcf9842156a14cd3c8f6fbed478b1e0499b5b5bcaceeb76d896edf3087f31843d4a903017dcfa2d4d466b0091cec5933bb7161a2bdc06d678319f9e799542227d4b03c363f90f3a4c9e853c30d86b37a691b3eca145ce7349863fb3899fdf08ab0c6465b2db789c90dabca6daa9ff5e87ad42b9c0833918ef43f50abcea9b247c249e29124934493c49fc6149b66fd66ed66656703678d8023a2673e33afe660bd34cf21cca059d3ccd9ff102fabeb0a7d6735673e8134a3274cca5b583aba25ba1add122f5670ad0b94782725afe0b24363c641e6178b181694f92f38c8e86059bce0d4f28bd2726ffed606167c2c6a0be511b774b6f03bcc9b94929496989afcfcc19347cf1e3e7d2c9ed0ceaa27c0cdf0b1dee400871fbf0203d7acc901661e3c4f765c0eb063987baffe367ad4b043a3e52930d8570bdc1538a1755170763643c3f1dd91d3592091c9058ddd8340256b971afcf0d9b7c77f76fc4eb54915971e7282a28e1164082333480d0f1033236170826081727001b03d02683a99912570eac06b1c231a2616dcf97ebf890b89cf828489f2070434d8e3712e4412873f1af66fdaf3f819c43a8a0c45a806197afc1aff324690c47f340000bc79dad7"))
try:
    cq_jt.read_bytes(jt106_input)
except cq_jt.JtUnsupportedError:
    pass
else:
    raise AssertionError("unqualified 10.6 enabled by default")
jt106_options=cq_jt.ReadOptions(structure=cq_jt.StructureOptions(experimental_jt106=True))
jt106_scene=cq_jt.read_bytes(jt106_input,options=jt106_options)
assert jt106_scene.complete and jt106_scene.to_model().to_dict()["profile"]=="mesh-10.6-experimental"
assert any(d.code=="unverified_jt106_profile" for d in jt106_scene.diagnostics)
glb,report=cq_jt.export_glb_bytes(jt106_scene)
n,kind=struct.unpack_from("<I4s",glb,12)
assert json.loads(glb[20:20+n])["asset"]["extras"]["jt"]["unverifiedProfiles"]
with tempfile.TemporaryDirectory() as directory:
    path=pathlib.Path(directory)/"spec106.jt";path.write_bytes(jt106_input)
    out=json.loads(subprocess.run([sys.executable,"-I","-m","cq_jt","convert",str(path),str(path.with_suffix(".glb")),"--experimental-jt106","--json"],check=True,text=True,capture_output=True).stdout)
    assert any(d["code"]=="unverified_jt106_profile" for d in out["diagnostics"])

del mesh_scene;gc.collect()
assert len(buffers.positions.data)==96
gc.collect()
print(f"Installed model/inspect/decode/structure/mesh/GLB/CLI smoke passed: Python {sys.version.split()[0]}, cq-jt {cq_jt.__version__}", flush=True)
'''


def clean_install(wheel: Path, interpreter: str) -> None:
    environment = dict(os.environ)
    for name in ("PYTHONPATH", "PYTHONHOME", "VIRTUAL_ENV"):
        environment.pop(name, None)
    with tempfile.TemporaryDirectory(prefix="cq-jt-wheel-") as directory:
        root = Path(directory)
        venv = root / "venv"
        subprocess.run(["uv", "venv", "--python", interpreter, "--no-python-downloads", str(venv)], cwd=root, env=environment, check=True)
        python = venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        subprocess.run(["uv", "pip", "install", "--python", str(python), "--only-binary=:all:", "--no-deps", str(wheel.resolve())], cwd=root, env=environment, check=True)
        subprocess.run(["uv", "pip", "check", "--python", str(python)], cwd=root, env=environment, check=True)
        subprocess.run([str(python), "-I", "-X", "faulthandler", "-c", SMOKE], cwd=root, env=environment, check=True)
        print(f"Clean wheel smoke and interpreter shutdown passed: {interpreter}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--python", action="append", default=[])
    parser.add_argument("--sdist", action="store_true", help="Rebuild a source distribution before installing")
    args = parser.parse_args()
    interpreters = args.python or [sys.executable]
    if args.sdist:
        sources = list(args.dist.resolve().glob("*.tar.gz"))
        if len(sources) != 1:
            parser.error("expected exactly one source distribution")
        with tempfile.TemporaryDirectory(prefix="cq-jt-sdist-") as directory:
            root = Path(directory)
            with tarfile.open(sources[0]) as archive:
                archive.extractall(root, filter="data")
            projects = list(root.glob("*/pyproject.toml"))
            if len(projects) != 1:
                parser.error("expected one unpacked Python project")
            output = root / "wheels"
            subprocess.run(["maturin", "build", "--release", "--locked", "--out", str(output), "--interpreter", sys.executable], cwd=projects[0].parent, env=build_environment(projects[0].parent), check=True)
            wheels = list(output.glob("*.whl"))
            if len(wheels) != 1:
                parser.error("expected one wheel rebuilt from sdist")
            check_wheel(wheels[0], check_project())
            for interpreter in interpreters:
                clean_install(wheels[0], interpreter)
    else:
        wheels = list(args.dist.resolve().glob("*.whl"))
        if len(wheels) != 1:
            parser.error("expected exactly one wheel")
        for interpreter in interpreters:
            clean_install(wheels[0], interpreter)


if __name__ == "__main__":
    main()
