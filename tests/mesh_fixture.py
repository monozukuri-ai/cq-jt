"""Authored closed tetrahedron, independent of any downloaded JT file."""
import struct
from jt_fixture import container_for_payloads, end_marker, logical_element
from structure_fixture import (base_node, element, group, partition, property_table,
                               string_atom, guid, material, transform, translation)

MASK = 0xffffffff


def mix(a, b, c):
    for x, shift in (("a",13),("b",8),("c",13),("a",12),("b",16),("c",5),("a",3),("b",10),("c",15)):
        if x == "a": a = ((a-b-c) ^ (c >> shift)) & MASK
        elif x == "b": b = ((b-c-a) ^ ((a << shift) & MASK)) & MASK
        else: c = ((c-a-b) ^ (b >> shift)) & MASK
    return a,b,c


def jt_hash(values, seed=0, shorts=False):
    a=b=0x9e3779b9; c=seed
    block=6 if shorts else 3
    start=0
    while start+block<=len(values):
        row=[v & (0xffff if shorts else MASK) for v in values[start:start+block]]
        if shorts: row=[row[i]|row[i+1]<<16 for i in (0,2,4)]
        a,b,c=mix((a+row[0])&MASK,(b+row[1])&MASK,(c+row[2])&MASK);start+=block
    c=(c+len(values))&MASK
    for i,value in enumerate(values[start:]):
        value &= 0xffff if shorts else MASK
        if shorts:
            if i<2: a=(a+(value<<(16*(i%2))))&MASK
            elif i<4: b=(b+(value<<(16*(i%2))))&MASK
            else: c=(c+(value<<16))&MASK
        elif i==0: a=(a+value)&MASK
        else: b=(b+value)&MASK
    return mix(a,b,c)[2]


def packet(values, endian="<"):
    if not values: return struct.pack(endian+"i",0)
    return struct.pack(endian+"iBi",len(values),0,len(values)*4)+struct.pack(endian+"I"*len(values),*(v&MASK for v in values))


def tetra_payload(endian="<", *, normals=True, version=2, binding_extra=0, wrong_hash=False, zero_normal=False, cover=False):
    binding=2 | (8 if normals else 0) | binding_extra
    out=struct.pack(endian+"hhQhih",1,1,binding,2,7,version)
    degrees=[[3,3,3],[3],[],[],[],[],[],[]]
    valences=[3]*4;groups=[0]*4;flags=[0,0,0,int(cover)]
    masks=[[],[1]*4,[],[],[],[],[],[]]
    vectors=[*degrees,valences,groups,flags,*masks,[],[],[],[],[]]
    h=0
    for i,v in enumerate(vectors):
        out += struct.pack(endian+"i",0) if i==21 else packet(v,endian)
        h=jt_hash(v,h,shorts=i==10)
    out+=struct.pack(endian+"I",h^int(wrong_hash))
    out+=struct.pack(endian+"Q4Bii",binding,0,0,0,0,4,4)
    positions=[(0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.)]
    out+=struct.pack(endian+"iB",4,3)+b"".join(struct.pack(endian+"ffB",0.,0.,0) for _ in range(3))
    h=0
    for k in range(3):
        bits=[struct.unpack("<I",struct.pack("<f",p[k]))[0] for p in positions]
        out+=packet([v>>23 for v in bits],endian)+packet([v&0x7fffff for v in bits],endian)
        for v in bits:h=jt_hash([v],h)
    out+=struct.pack(endian+"I",h)
    if normals:
        vectors=[(0.,0.,1.),(1.,0.,0.),(0.,1.,0.),(0.,0.,-1.)]
        if zero_normal: vectors[0]=(0.,0.,0.)
        out+=struct.pack(endian+"iBB",4,3,0);h=0
        for k in range(3):
            bits=[struct.unpack("<I",struct.pack("<f",p[k]))[0] for p in vectors]
            out+=packet([v>>23 for v in bits],endian)+packet([v&0x7fffff for v in bits],endian)
            for v in bits:h=jt_hash([v],h)
        out+=struct.pack(endian+"I",h)
    if version==2: out+=struct.pack(endian+"hQ",1,binding)
    return out+struct.pack(endian+"h",1)


def mesh_scene(*, endian="<", lod=False, unordered=False, codec="zlib", payload=None, missing=False, lod_children=(4,6), unit=None, unit_owner=0, root_color=None, root_matrix=None, first_matrix=None, second_matrix=None, root_children=(1,2)):
    p=lambda fmt,*v: struct.pack(endian+fmt,*v)
    nodes=[element(0x10dd103e,0,partition(root_children,(100,105) if root_color is not None else (100,),endian=endian),1,endian),
           element(0x10dd102a,1,base_node((101,),endian=endian)+p("hi",1,3),0,endian),
           element(0x10dd102a,2,base_node((102,103),endian=endian)+p("hi",1,3),0,endian),
           element(0xce357244,3,group((5 if lod else 4,),endian=endian)+p("hhi",1,1,0),1,endian,"part")]
    def shape():
        bounds=(0.,0.,-1.,1.,1.,0.)
        return base_node(endian=endian)+p("h13f7ifhQ4BQ",1,*bounds,*bounds,1.,4,4,1,1,4,4,128,0.,2,2,0,0,0,0,2)
    nodes.append(element(0x10dd1077,4,shape(),2,endian))
    if lod:
        nodes.append(element(0x10dd102c if unordered else 0x10dd104c,5,group(lod_children,endian=endian)+p("hIi",1,0,0)+(b"" if unordered else p("hI3f",1,0,0.,0.,0.)),1,endian))
        nodes.append(element(0x10dd1077,6,shape(),2,endian))
    rotate=(0.,-1.,0.,0.,1.,0.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)
    nodes.extend([transform(root_matrix if root_matrix is not None else translation(10),100,endian=endian),transform(first_matrix if first_matrix is not None else rotate,101,endian=endian),transform(second_matrix if second_matrix is not None else translation(y=20),102,endian=endian),material((0.,0.,1.,0.5),103,endian=endian)])
    if root_color is not None:nodes.append(material(root_color,105,endian=endian))
    atoms=[string_atom(200,"JT_LLPROP_SHAPEIMPL",endian)]
    atoms.append(element(0,201,p("hIh",1,0,1)+bytes(range(1,17))+p("iii",7,99,1),8,endian,"late"))
    rows=[(4,[(200,201)])]
    if lod: rows.append((6,[(200,201)]))
    if unit is not None:
        atoms.extend([string_atom(202,"JT_PROP_MEASUREMENT_UNITS",endian),string_atom(203,unit,endian)])
        owner=next((row for row in rows if row[0]==unit_owner),None)
        if owner:owner[1].append((202,203))
        else:rows.append((unit_owner,[(202,203)]))
    lsg=b"".join(nodes)+end_marker(endian)+b"".join(atoms)+end_marker(endian)+property_table(rows,endian)
    elements=logical_element(payload if payload is not None else tetra_payload(endian),endian=endian,type_id=guid(0x10dd10ab,endian),object_id=0,base_type=4)+end_marker(endian)
    if missing: elements=logical_element(b"unsupported",endian=endian)+end_marker(endian)
    return container_for_payloads([(1,lsg,codec),(7,elements,"raw")],endian=endian)
