"""Convert the reviewed CC0 source GLB into Concordia's bounded, texture-free mesh.

Usage: python scripts/prepare-concordia-model.py PATH_TO_REVIEWED_GLB
Source/provenance: crates/concordia/assets/concordia-model-CREDITS.md
No network, model service, Blender installation or runtime decoder is required.
"""
import hashlib
import json
from pathlib import Path
import struct
import sys

source = Path(sys.argv[1])
raw = source.read_bytes()
assert len(raw) == 622848
assert hashlib.sha256(raw).hexdigest() == '8f052b37af987cb4fac8b1c89eb23a6b80fbc2453232bd38892384bf4a4bd48f'
assert raw[:4] == b"glTF"
assert struct.unpack_from("<II", raw, 4) == (2, len(raw))
json_size, json_kind = struct.unpack_from("<II", raw, 12)
assert json_kind == 0x4E4F534A
model = json.loads(raw[20:20+json_size])
binary_size, binary_kind = struct.unpack_from("<II", raw, 20+json_size)
assert binary_kind == 0x004E4942
binary = raw[28+json_size:28+json_size+binary_size]
assert len(binary) == binary_size
assert len(model['buffers']) == 1 and 'uri' not in model['buffers'][0]
assert model['asset']['copyright'].find('CC0') >= 0

def accessor(index):
    a = model['accessors'][index]
    view = model['bufferViews'][a['bufferView']]
    assert not a.get('sparse') and view['buffer'] == 0
    width = {'VEC3': 3, 'SCALAR': 1}[a['type']]
    code = {5126: 'f', 5123: 'H'}[a['componentType']]
    item = struct.Struct('<'+code*width)
    start = view.get('byteOffset', 0)+a.get('byteOffset', 0)
    stride = view.get('byteStride', item.size)
    assert a['count'] < 40000 and stride >= item.size
    assert start+(a['count']-1)*stride+item.size <= len(binary)
    return [item.unpack_from(binary, start+i*stride) for i in range(a['count'])]

vertices, triangles, parts = [], [], []
for mesh in model['meshes']:
    start = len(vertices)
    for primitive in mesh['primitives']:
        assert primitive.get('mode', 4) == 4
        positions = accessor(primitive['attributes']['POSITION'])
        normals = accessor(primitive['attributes']['NORMAL'])
        indices = [item[0] for item in accessor(primitive['indices'])]
        assert len(positions) == len(normals) and len(indices) % 3 == 0
        base = len(vertices)
        for p, n in zip(positions, normals):
            # Normalize the 0.580477 m bust into the renderer's fixed display volume.
            position = [p[0]*3.8, (p[1]-0.285)*3.8, p[2]*3.8]
            assert all(-1.5 <= x <= 1.5 for x in position)
            assert all(-1.001 <= x <= 1.001 for x in n)
            vertices.append(tuple(round(x/1.5*32767) for x in position)+tuple(round(x*32767) for x in n))
        assert all(index < len(positions) for index in indices)
        triangles.extend(tuple(base+index for index in indices[i:i+3]) for i in range(0,len(indices),3))
    parts.append({'name':mesh['name'],'vertex_start':start,'vertex_end':len(vertices)})
assert len(vertices) < 10000 and len(triangles) < 15000
data = bytearray(b'CCM1'+struct.pack('<II',len(vertices),len(triangles)))
for vertex in vertices: data.extend(struct.pack('<6h',*vertex))
for triangle in triangles: data.extend(struct.pack('<3H',*triangle))
root = Path(__file__).resolve().parents[1]
destination = root/'crates/concordia/assets/concordia.mesh'
destination.write_bytes(data)
print(json.dumps({'source_sha256':hashlib.sha256(raw).hexdigest(),'mesh_sha256':hashlib.sha256(data).hexdigest(),'source_bytes':len(raw),'mesh_bytes':len(data),'vertices':len(vertices),'triangles':len(triangles),'parts':parts},indent=2))
