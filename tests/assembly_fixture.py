"""Authored external JT graph; actual part files can be attached separately."""
import struct
from jt_fixture import container_for_payloads, end_marker
from structure_fixture import base_node, element, partition, property_table, string_atom, transform, translation, material


def assembly(*, first="part.jt", second="part.jt", unit="Millimeters", final_color=False):
    nodes=[element(0x10dd103e,0,partition((1,2),(100,105)),1),
           element(0x10dd102a,1,base_node((101,))+struct.pack("<hi",1,3),0),
           element(0x10dd102a,2,base_node((102,))+struct.pack("<hi",1,4),0),
           element(0x10dd103e,3,partition(name=first),1),
           element(0x10dd103e,4,partition(name=second),1),
           transform(translation(x=10),100),transform(translation(y=20),101),transform(translation(z=30),102),
           material((.2,.7,.4,1.),105,flags=9 if final_color else 8)]
    atoms=[string_atom(202,"JT_PROP_NAME"),string_atom(203,"authored assembly")]
    rows=[(0,[(202,203)])]
    if unit is not None:
        atoms.extend([string_atom(200,"JT_PROP_MEASUREMENT_UNITS"),string_atom(201,unit)])
        rows[0][1].append((200,201))
    data=b"".join(nodes)+end_marker()+b"".join(atoms)+end_marker()+property_table(rows)
    return container_for_payloads([(1,data,"zlib")])
