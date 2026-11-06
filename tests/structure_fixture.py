"""Authored JT 9.5 structure with known placement/material expectations."""
import struct
from jt_fixture import container_for_payloads, end_marker, logical_element

IDENTITY = (1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)

def guid(code, endian="<", family="common"):
    if family == "common": return struct.pack(endian+"IHH", code, 0x2ac8, 0x11d1)+bytes.fromhex("9b6b0080c7bb5997")
    if family == "part": return struct.pack(endian+"IHH", code, 0x38fb, 0x11d1)+bytes.fromhex("a506006097bdc6e1")
    return struct.pack(endian+"IHH", 0xe0b05be5, 0xfbbd, 0x11d1)+bytes.fromhex("a3a700aa00d10954")

def mb(value, endian="<"):
    data=value.encode("utf-16-le" if endian=="<" else "utf-16-be")
    return struct.pack(endian+"i",len(data)//2)+data

def element(code, object_id, payload, base, endian="<", family="common"):
    return logical_element(payload,type_id=guid(code,endian,family),object_id=object_id,base_type=base,endian=endian)

def base_node(attributes=(),flags=0,endian="<"):
    return struct.pack(endian+"hII",1,flags,len(attributes))+struct.pack(endian+"i"*len(attributes),*attributes)

def group(children=(),attributes=(),flags=0,endian="<"):
    return base_node(attributes,flags,endian)+struct.pack(endian+"hI",1,len(children))+struct.pack(endian+"i"*len(children),*children)

def partition(children=(),attributes=(),name="root.jt",flags=0,endian="<"):
    return group(children,attributes,flags,endian)+struct.pack(endian+"i",0)+mb(name,endian)+struct.pack(endian+"7f6i",*[0.]*7,*[0]*6)

def translation(x=0,y=0,z=0):
    values=list(IDENTITY);values[3]=x;values[7]=y;values[11]=z;return tuple(values)

def transform(matrix,object_id,flags=8,width=8,endian="<",inhibit=0):
    raw=tuple(matrix[col*4+row] for row in range(4) for col in range(4))
    mask=sum(0x8000>>i for i,value in enumerate(raw) if value!=IDENTITY[i])
    data=struct.pack(endian+"hBIhH",1,flags,inhibit,1,mask)
    data+=struct.pack(endian+("d" if width==8 else "f")*mask.bit_count(),*(v for i,v in enumerate(raw) if mask & (0x8000>>i)))
    return element(0x10dd1083,object_id,data,3,endian)

def material(rgba,object_id,flags=8,inhibit=0,endian="<"):
    data=struct.pack(endian+"hBIhH",1,flags,inhibit,2,0x10)
    data+=struct.pack(endian+"18f",*(0.2,0.2,0.2,1),*rgba,*(0.5,0.5,0.5,1),*(0.,0.,0.,1),16.,0.1)
    return element(0x10dd1030,object_id,data,3,endian)

def shape(attributes=(),endian="<"):
    b=base_node(attributes,endian=endian)+struct.pack(endian+"h",1)
    bounds=(0.,0.,0.,1.,1.,1.)
    b+=struct.pack(endian+"13f",*bounds,*bounds,6.)
    b+=struct.pack(endian+"7ifhQ4BQ",8,8,1,1,12,12,128,0.,2,0,0,0,0,0,0)
    return b

def string_atom(object_id,value,endian="<"):
    return element(0x10dd106e,object_id,struct.pack(endian+"hIh",1,0,1)+mb(value,endian),5,endian)

def property_table(rows,endian="<"):
    out=struct.pack(endian+"hI",1,len(rows))
    for owner,pairs in rows:
        out+=struct.pack(endian+"i",owner)
        for key,value in pairs:out+=struct.pack(endian+"ii",key,value)
        out+=struct.pack(endian+"i",0)
    return out

def scene_fixture(*,endian="<",root_flags=0,root_material_flags=8,blue_flags=8,blue_inhibit=0,
                  root_transform_flags=8,second_transform_flags=8,part_attributes=(),
                  root_children=(1,2),part_children=(4,),extra_graph=(),extra_atoms=(),extra_rows=(),
                  unit="Millimeters",codec="zlib",shape_ref=True):
    root=element(0x10dd103e,0,partition(root_children,(100,101),flags=root_flags,endian=endian),1,endian)
    i1=element(0x10dd102a,1,base_node((102,),endian=endian)+struct.pack(endian+"hi",1,3),0,endian)
    i2=element(0x10dd102a,2,base_node((103,104),endian=endian)+struct.pack(endian+"hi",1,3),0,endian)
    part=element(0xce357244,3,group(part_children,part_attributes,endian=endian)+struct.pack(endian+"hhi",1,1,0),1,endian,"part")
    sh=element(0x10dd1077,4,shape(endian=endian),2,endian)
    rotate=(0.,-1.,0.,0.,1.,0.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)
    attributes=[transform(translation(10),100,root_transform_flags,endian=endian),material((1.,0.,0.,1.),101,root_material_flags,endian=endian),
                transform(rotate,102,endian=endian),transform(translation(y=20),103,second_transform_flags,endian=endian),
                material((0.,0.,1.,0.5),104,blue_flags,blue_inhibit,endian)]
    atoms=[string_atom(200,"JT_PROP_NAME",endian),string_atom(201,"組立",endian),string_atom(202,"JT_PROP_MEASUREMENT_UNITS",endian),
           string_atom(203,unit,endian),string_atom(204,"共有部品",endian)]
    rows=[(0,[(200,201),(202,203)]),(3,[(200,204)])]
    if shape_ref:
        atoms += [string_atom(205,"JT_LLPROP_SHAPEIMPL",endian),element(0,206,struct.pack(endian+"hIh",1,0,1)+bytes(range(1,17))+struct.pack(endian+"iii",7,99,1),8,endian,"late")]
        rows += [(4,[(205,206)])]
    payload=b"".join([root,i1,i2,part,sh,*attributes,*extra_graph])+end_marker(endian)+b"".join([*atoms,*extra_atoms])+end_marker(endian)+property_table(rows+list(extra_rows),endian)
    entries=[(1,payload,codec)]
    if shape_ref:entries.append((7,logical_element(b"mesh opaque",object_id=99,base_type=4,endian=endian),"raw"))
    return container_for_payloads(entries,endian=endian)
