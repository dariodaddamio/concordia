"""Render concordia.svg's geometric mark to ICO using only Python's standard library.

Run from any directory: python crates/concordia/assets/render_icon.py
The SVG is the master; geometry and colors below mirror its three visible shapes.
Supersampling preserves the mark's edges at native Windows titlebar sizes.
"""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent
C = [(184,62),(152,40),(88,40),(40,88),(40,168),(88,216),
     (152,216),(184,194),(158,166),(138,180),(104,180),(76,152),
     (76,104),(104,76),(138,76),(158,90)]
SIGNAL = [(168,104),(200,128),(168,152),(152,136),(164,128),(152,120)]


def polygon(x, y, points):
    inside = False
    previous = points[-1]
    for current in points:
        ax, ay = previous
        bx, by = current
        if (ay > y) != (by > y) and x < (bx-ax)*(y-ay)/(by-ay)+ax:
            inside = not inside
        previous = current
    return inside


def rounded(x, y, inset, radius):
    if not (inset <= x <= 256-inset and inset <= y <= 256-inset):
        return False
    dx = max(inset+radius-x, 0, x-(256-inset-radius))
    dy = max(inset+radius-y, 0, y-(256-inset-radius))
    return dx*dx+dy*dy <= radius*radius


def blend(a, b, t):
    return tuple(round(v+(w-v)*max(0,min(1,t))) for v,w in zip(a,b))


def pixel(x, y):
    if not rounded(x, y, 7, 55):
        return (0, 0, 0, 0)
    color = (49, 67, 87)
    if rounded(x, y, 9, 53):
        color = blend((28,44,62), (16,24,36), ((x-8)+(y-8))/480)
    if polygon(x,y,C):
        # SVG object-bounding-box gradient (0,0) -> (.6,1).
        color = blend((138,243,222), (71,173,206),
                      (((x-40)/144)*.6+(y-40)/176)/1.36)
    if polygon(x,y,SIGNAL):
        color = blend((255,228,175), (234,184,109), (y-104)/48)
    return (*color,255)


def png(size):
    scale = 4
    data = bytearray()
    for y in range(size):
        data.append(0)
        for x in range(size):
            samples = [pixel((x+(sx+.5)/scale)*256/size,
                             (y+(sy+.5)/scale)*256/size)
                       for sy in range(scale) for sx in range(scale)]
            alpha = sum(p[3] for p in samples)
            color = [round(sum(p[c]*p[3] for p in samples)/alpha)
                     if alpha else 0 for c in range(3)]
            data.extend((*color, round(alpha/len(samples))))
    def chunk(kind, body):
        return struct.pack('>I',len(body))+kind+body+struct.pack('>I',zlib.crc32(kind+body))
    return (b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',size,size,8,6,0,0,0))
            +chunk(b'IDAT',zlib.compress(data,9))+chunk(b'IEND',b''))


def main():
    sizes = (16, 32, 48, 256)
    frames = [png(size) for size in sizes]
    directory = bytearray(struct.pack('<HHH',0,1,len(frames)))
    offset = 6+16*len(frames)
    for size, frame in zip(sizes,frames):
        directory.extend(struct.pack('<BBBBHHII',size%256,size%256,0,0,1,32,len(frame),offset))
        offset += len(frame)
    (ROOT/'concordia.ico').write_bytes(directory+b''.join(frames))
    (ROOT/'concordia.png').write_bytes(frames[-1])


if __name__ == '__main__':
    main()
