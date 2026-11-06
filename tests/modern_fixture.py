"""Independent authored JT 10 writer for the bounded triangle profile."""
import struct
from jt_fixture import container_for_payloads, end_marker, logical_element
from structure_fixture import guid, mb, property_table
from mesh_fixture import jt_hash, packet


def modern_mesh(*, endian="<", bits=0, revision106=False):
    p=lambda fmt,*a:struct.pack(endian+fmt,*a)
    bindings=10
    degrees=[[3,3,3],[3],[],[],[],[],[],[]]
    vectors=[*degrees,[3]*4,[0]*4,[0]*4,[],[1]*4,[],[],[],[],[],[],[],[],[],[]]
    # Degrees, valences, groups, flags, eight masks, mask30, large, splits, positions.
    h=0;symbols=b""
    for i,v in enumerate(vectors):
        symbols+=p("i",0) if i==20 else packet(v,endian)
        h=jt_hash(v,h,shorts=i==10)
    body=p("BiB",1,7,1)+symbols+p("I",h)
    body+=p("Q4Bii",bindings,0,bits,0,0,4,4)
    positions=[(0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.)]
    body+=p("iB",4,3)+b"".join(p("ffB",0.,0.,0) for _ in range(3))
    h=0
    for axis in range(3):
        values=[struct.unpack("<I",struct.pack("<f",v[axis]))[0] for v in positions]
        body+=packet(values,endian)
        for value in ([values] if not revision106 else ([v] for v in values)):h=jt_hash(value,h)
    body+=p("I",h)+p("iBB",4,3,bits)
    if bits:
        codes=[(s<<(2*bits+3)) | (o<<(2*bits)) | (((1<<bits)-1)<<bits) for s,o in [(1,7),(5,7),(3,7),(1,0)]]
        body+=packet(codes,endian);h=jt_hash(codes)
    else:
        normals=[(0.,0.,1.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.)];h=0
        for axis in range(3):
            values=[struct.unpack("<I",struct.pack("<f",v[axis]))[0] for v in normals]
            body+=packet(values,endian)
            for value in ([values] if not revision106 else ([v] for v in values)):h=jt_hash(value,h)
    body+=p("I",h)
    embedded=struct.pack(endian+"IHH",0xf830a5ad,0xbe4c,0x4fbc)+bytes.fromhex("9b5fb9269278d2e1")
    return p("BBQ",1,1,bindings)+logical_element(body,type_id=embedded,object_id=1,base_type=9,endian=endian)+b"\1"


def modern_scene(*,version="10.0",endian="<",bits=0,final_flags=0,base_version=1,palette=0xffffffff,parent_final=False,child_force=False,scalar_hash=None,partition_flags=None,split_lists=False,translation=None):
    revision106=version=="10.6"
    p=lambda fmt,*a:struct.pack(endian+fmt,*a)
    node=lambda attrs=():p("BII",1,0,len(attrs))+p("i"*len(attrs),*attrs)
    group=lambda children,attrs=():node(attrs)+p("Bi",1,len(children))+p("i"*len(children),*children)
    element=lambda code,id,data,base,family="common":logical_element(data,type_id=guid(code,endian,family),object_id=id,base_type=base,endian=endian)
    flags=(1 if revision106 else 0) if partition_flags is None else partition_flags
    partition=p("i",flags) if revision106 else p("Bi",1,flags)
    root_attrs=((4,) if parent_final else ())+((5,) if translation is not None else ())
    root=element(0x10dd103e,0,group((1,),root_attrs)+partition+mb("authored.jt",endian)+p("7f6i",*[0.]*7,*[0]*6),1)
    part=element(0xce357244,1,group((2,))+p("BBi",1,1,0),1,"part")
    bounds=(0.,0.,-1.,1.,1.,0.)
    shape=element(0x10dd1077,2,node((3,))+p("B7f7ifBQ",1,*bounds,1.,4,4,1,1,4,4,128,0.,1,10),2)
    attr=p("BBIIBH",base_version,10 if child_force else 8,0,final_flags,1,0x10)+p("19f",*(.2,.2,.2,1.),*(1.,0.,0.,.5),*(.5,.5,.5,1.),*(0.,0.,0.,1.),16.,.1,.2)
    if base_version==2:attr+=p("I",palette)
    color=element(0x10dd1030,3,attr,3)
    if parent_final:
        parent=p("BBIIBH",1,8,0,1<<6,1,0x10)+p("19f",*(.2,.2,.2,1.),*(0.,1.,0.,.25),*(.5,.5,.5,1.),*(0.,0.,0.,1.),16.,.1,.2)
        color+=element(0x10dd1030,4,parent,3)
    if translation is not None:
        # JT stores the row-vector translation in elements 12/13/14.
        transform=p("BBIIBH3d",1,8,0,0,1,0x000e,*translation)
        color+=element(0x10dd1083,5,transform,3)
    local=1 if revision106 else 2
    atom=lambda id,text:element(0x10dd106e,id,p("BIB",local,0,local)+mb(text,endian),5)
    late=p("BIB",local,0,local)+bytes(range(1,17))+p("ii",7,0)+(p("i",1) if revision106 else b"")
    atoms=[atom(200,"JT_PROP_MEASUREMENT_UNITS"),atom(201,"Millimeters"),atom(202,"JT_LLPROP_SHAPEIMPL"),element(0,203,late,8,"late")]
    lsg=root+part+shape+color+(b"" if revision106 and not split_lists else end_marker(endian))+b"".join(atoms)+end_marker(endian)+property_table([(0,[(200,201)]),(2,[(202,203)])],endian)
    shape=logical_element(modern_mesh(endian=endian,bits=bits,revision106=revision106 if scalar_hash is None else scalar_hash),type_id=guid(0x10dd10ab,endian),object_id=0,base_type=4,endian=endian)+end_marker(endian)
    return container_for_payloads([(1,lsg,"zlib"),(7,shape,"raw")],version=version,endian=endian)
