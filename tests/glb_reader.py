"""Small independent GLB/accessor reader used only for assertions."""
import json
import struct


class Glb:
    def __init__(self, data):
        assert data[:4] == b"glTF"
        version, length = struct.unpack_from("<II", data, 4)
        assert version == 2 and length == len(data) and len(data) % 4 == 0
        n, kind = struct.unpack_from("<I4s", data, 12)
        assert kind == b"JSON" and n % 4 == 0
        self.json = json.loads(data[20:20+n])
        length, kind = struct.unpack_from("<I4s", data, 20+n)
        assert kind == b"BIN\0" and length % 4 == 0
        self.bin = data[28+n:]
        assert length == len(self.bin)
        assert 0 <= len(self.bin) - self.json["buffers"][0]["byteLength"] <= 3

    def accessor(self, index):
        a = self.json["accessors"][index]
        v = self.json["bufferViews"][a["bufferView"]]
        assert v["buffer"] == 0 and v.get("byteOffset", 0) % 4 == 0
        width = {"SCALAR":1, "VEC3":3}[a["type"]]
        fmt = {5126:"f", 5125:"I"}[a["componentType"]] * width
        start = v.get("byteOffset", 0) + a.get("byteOffset", 0)
        end = start + struct.calcsize("<"+fmt) * a["count"]
        assert end <= v.get("byteOffset", 0) + v["byteLength"] <= len(self.bin)
        result = tuple(struct.iter_unpack("<"+fmt, self.bin[start:end]))
        if "min" in a:
            assert tuple(min(v[i] for v in result) for i in range(width)) == tuple(a["min"])
            assert tuple(max(v[i] for v in result) for i in range(width)) == tuple(a["max"])
        return result

    def matrices(self):
        identity = (1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.)
        world = {}
        stack = [(i, identity) for i in self.json["scenes"][self.json["scene"]]["nodes"]]
        while stack:
            index, parent = stack.pop()
            assert index not in world, "cycle or multiple parents"
            n = self.json["nodes"][index]
            raw = n.get("matrix", identity)
            local = tuple(raw[c*4+r] for r in range(4) for c in range(4))
            matrix = tuple(sum(parent[r*4+k]*local[k*4+c] for k in range(4)) for r in range(4) for c in range(4))
            world[index] = matrix
            stack.extend((i, matrix) for i in n.get("children", ()))
        assert len(world) == len(self.json["nodes"])
        return world

    def world_positions(self):
        matrices = self.matrices()
        result = {}
        for i, node in enumerate(self.json["nodes"]):
            if "mesh" not in node:
                continue
            p = self.json["meshes"][node["mesh"]]["primitives"][0]
            m = matrices[i]
            result[node["extras"]["jt"]["instanceId"]] = tuple(tuple(sum(m[r*4+k]*v[k] for k in range(3))+m[r*4+3] for r in range(3)) for v in self.accessor(p["attributes"]["POSITION"]))
        return result
