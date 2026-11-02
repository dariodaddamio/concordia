"""Render actual Concordia TestBackend frames as GIFs. No model or network calls.

First: cargo test -p concordia --lib write_gif_frames -- --ignored
Then:  python scripts/render-demo-gifs.py
Pillow is a local build-time tool only, never a product dependency.
"""
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "target" / "gif-frames"
DEST = ROOT / "docs" / "media"
FONT_DIR = Path("C:/Windows/Fonts")
SCALE = 2
CELL_W, CELL_H = 12, 26
PAD, TOP, BOTTOM = 18, 94, 108
BG, CHROME, FG, ACCENT = "#14161b", "#20242d", "#e5e7eb", "#65d9cb"


def font(name, size):
    return ImageFont.truetype(str(FONT_DIR / name), round(size * SCALE))


FONTS = {
    (False, False): font("consola.ttf", 22),
    (True, False): font("consolab.ttf", 22),
    (False, True): font("consolai.ttf", 22),
    (True, True): font("consolaz.ttf", 22),
}
SYMBOL = font("seguisym.ttf", 17)
TITLE = font("arialbd.ttf", 32)
LABEL = font("arial.ttf", 13)
CAPTION = font("arial.ttf", 38)


def wrap_words(draw, text, face, width):
    rows, line = [], ""
    for word in text.split():
        candidate = f"{line} {word}".strip()
        if line and draw.textlength(candidate, font=face) > width:
            rows.append(line)
            line = word
        else:
            line = candidate
    return rows + ([line] if line else [])


def render(scene, frame):
    width = frame["cols"] * CELL_W + 2 * PAD
    height = frame["rows"] * CELL_H + TOP + BOTTOM
    image = Image.new("RGB", (width * SCALE, height * SCALE), BG)
    draw = ImageDraw.Draw(image)
    xy = lambda values: tuple(round(v * SCALE) for v in values)
    draw.rectangle(xy((0, 0, width, TOP - 8)), fill=CHROME)
    draw.text(xy((PAD, 9)), "CONCORDIA  /  Synthetic demonstration · cursor overlay", font=LABEL, fill=ACCENT)
    draw.text(xy((PAD, 32)), scene["title"], font=TITLE, fill=FG)
    for cell in frame["cells"]:
        x, y = PAD + cell["x"] * CELL_W, TOP + cell["y"] * CELL_H
        if cell["bg"]:
            draw.rectangle(xy((x, y, x + cell["w"] * CELL_W, y + CELL_H)), fill=cell["bg"])
        if not cell["text"].strip():
            continue
        face = FONTS[(cell["bold"], cell["italic"])]
        if len(cell["text"]) == 1 and ord(cell["text"]) > 127 and not 0x2500 <= ord(cell["text"]) <= 0x257f:
            face = SYMBOL
        draw.text(xy((x, y + 21)), cell["text"], font=face, fill=cell["fg"], anchor="ls")
        if cell["underline"]:
            draw.line(xy((x, y + 21, x + cell["w"] * CELL_W, y + 21)), fill=cell["fg"], width=SCALE)
    baseline = TOP + frame["rows"] * CELL_H + 7
    draw.line(xy((PAD, baseline - 5, width - PAD, baseline - 5)), fill="#353d49", width=SCALE)
    lines = wrap_words(draw, frame["caption"], CAPTION, (width - 2 * PAD) * SCALE)
    for i, line in enumerate(lines[:2]):
        draw.text(xy((PAD, baseline + i * 44)), line, font=CAPTION, fill=FG)
    return image.resize((width, height), Image.Resampling.LANCZOS)


def pointer(base, position, shape, pulse=0):
    """Editorial cursor follows actual hit targets; no claim about host support."""
    image = base.copy()
    draw = ImageDraw.Draw(image)
    x, y = round(position[0]), round(position[1])
    if pulse:
        radius = round(10 + 20 * pulse)
        draw.ellipse((x-radius, y-radius, x+radius, y+radius), outline=ACCENT, width=3)
    if shape == "text":
        draw.line((x,y-9,x,y+10), fill="#101319", width=5)
        draw.line((x,y-9,x,y+10), fill="white", width=2)
        for dy in (-9, 10):
            draw.line((x-5,y+dy,x+5,y+dy), fill="white", width=2)
    elif shape == "pointer":
        # Raised index finger with a palm, rather than an ambiguous highlight box.
        points = [(x,y+21),(x-5,y+10),(x-5,y+6),(x-1,y+5),(x+3,y+10),
                  (x+3,y-8),(x+7,y-10),(x+10,y-7),(x+10,y+2),
                  (x+19,y+4),(x+23,y+10),(x+20,y+25),(x+4,y+25)]
        draw.polygon(points,fill="white",outline="#101319",width=2)
    else:
        points = [(x,y),(x,y+27),(x+7,y+21),(x+12,y+32),
                  (x+17,y+29),(x+12,y+18),(x+23,y+18)]
        draw.polygon(points,fill="white",outline="#101319",width=2)
    return image


def animate(scene, bases):
    frames, durations = [], []
    position = (bases[0].width * .78, TOP + 3 * CELL_H)
    shape = "default"
    for index, (state, base) in enumerate(zip(scene["frames"], bases)):
        target = state.get("cursor")
        ms = state["duration"]
        if target:
            destination = (PAD+(target["x"]+.5)*CELL_W, TOP+(target["y"]+.5)*CELL_H)
            # Move on the preceding UI, then show the result of the click.
            previous = bases[max(0,index-1)]
            for step in range(1,7):
                t = step / 6
                ease = t*t*(3-2*t)
                point = tuple(a+(b-a)*ease for a,b in zip(position,destination))
                frames.append(camera(pointer(previous,point,shape),scene["frames"][max(0,index-1)]))
                durations.append(60)
            position, shape = destination, target["shape"]
            ms -= 360
            if target.get("click"):
                for pulse in (.2,.5,.8,1):
                    frames.append(camera(pointer(base,position,shape,pulse),state))
                    durations.append(60)
                ms -= 240
        frames.append(camera(pointer(base,position,shape),state))
        durations.append(max(100,ms))
    return frames, durations


def camera(image, state):
    if not state.get("zoom"):
        return image
    zoom, fx, fy = state["zoom"]
    result = image.copy()
    width, height = image.width-2*PAD, state["rows"]*CELL_H
    crop_w, crop_h = round(width/zoom), round(height/zoom)
    x = max(0,min(width-crop_w,round(width*fx-crop_w/2)))
    y = max(0,min(height-crop_h,round(height*fy-crop_h/2)))
    x, y = (x//CELL_W)*CELL_W, (y//CELL_H)*CELL_H
    crop = image.crop((PAD+x,TOP+y,PAD+x+crop_w,TOP+y+crop_h))
    result.paste(crop.resize((width,height),Image.Resampling.LANCZOS),(PAD,TOP))
    return result


def main():
    DEST.mkdir(parents=True, exist_ok=True)
    for name in ("concordia-file-preview", "concordia-command-controls"):
        scene = json.loads((SOURCE / f"{name}.json").read_text(encoding="utf-8"))
        bases = [render(scene, frame) for frame in scene["frames"]]
        frames, durations = animate(scene,bases)
        # One shared palette keeps text and UI colors consistent between frames.
        samples = Image.new("RGB", (256, 180 * len(bases)), BG)
        for i, frame in enumerate(bases):
            thumbnail = frame.copy()
            thumbnail.thumbnail((256, 180))
            samples.paste(thumbnail, (0, i * 180))
        palette = samples.quantize(colors=256, method=Image.Quantize.MEDIANCUT)
        indexed = [frame.quantize(palette=palette, dither=Image.Dither.NONE) for frame in frames]
        out = DEST / f"{name}.gif"
        indexed[0].save(out, save_all=True, append_images=indexed[1:],
                        duration=durations, loop=0,
                        optimize=True, disposal=1,
                        comment=b"Actual Concordia UI/handlers. Synthetic events/replies. Editorial cursor overlay. No model calls.")
        # An internal review sheet is not part of the delivered GIF pair.
        sheet = Image.new("RGB", (frames[0].width * 2, frames[0].height * 2), BG)
        for position, index in enumerate((0, 2, 4, len(bases) - 1)):
            sheet.paste(camera(bases[index],scene["frames"][index]), ((position % 2) * frames[0].width, (position // 2) * frames[0].height))
        sheet.save(SOURCE / f"{name}-review.png")
        with Image.open(out) as check:
            assert check.is_animated and check.n_frames >= 5
            assert check.info.get("loop") == 0
            assert out.stat().st_size < 5 * 1024 * 1024
            print(json.dumps({"file": str(out.relative_to(ROOT)), "frames": check.n_frames,
                              "size": list(check.size), "bytes": out.stat().st_size,
                              "duration_ms": sum(f["duration"] for f in scene["frames"])}))


if __name__ == "__main__":
    main()
