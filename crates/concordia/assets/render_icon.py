"""Export the original Concordia portrait to application PNG/ICO assets.

Maintainer-only: requires Pillow (`python -m pip install Pillow`). End users need
only the prebuilt Rust executable; neither Python nor Pillow ships with it.
Run: python crates/concordia/assets/render_icon.py
The generated portrait remains the unmodified master, including its alpha.
"""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent


def main():
    with Image.open(ROOT / 'concordia-portrait.png') as master:
        portrait = master.convert('RGBA')
        portrait.thumbnail((256, 256), Image.Resampling.LANCZOS)
        portrait.save(ROOT / 'concordia.png', optimize=True)
        portrait.save(ROOT / 'concordia.ico', format='ICO',
                      sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    print('Exported transparent portrait PNG and seven Windows icon sizes.')


if __name__ == '__main__':
    main()
