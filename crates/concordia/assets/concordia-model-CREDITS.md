# Concordia sculpture source

The anatomical bust is adapted from **Classical female bust**, authored by Innerscene Corp
from MakeHuman/MPFB CC0 graphical assets and dedicated to **CC0 1.0 Universal**.

- Publisher and license: https://www.innerscene.com/tools/library/3d-parts/classical-female-bust-fd4546a4
- Download: https://www.innerscene.com/api/library/classical-female-bust-fd4546a4/download
- MakeHuman graphical-asset license: https://www.makehumancommunity.org/about/license.html
- CC0 dedication: https://creativecommons.org/publicdomain/zero/1.0/
- Reviewed original GLB: 622,848 bytes; SHA256 `8f052b37af987cb4fac8b1c89eb23a6b80fbc2453232bd38892384bf4a4bd48f`
- Derived mesh: 170,220 bytes; SHA256 `802c2cc2e96c6d016db152fd8620e61e511a180b53a627becb18a83f0290201c`

The embedded GLB copyright explicitly dedicates the asset to CC0. The original has six named
mesh parts, no animation or skinning, and meter units. Conversion retains positions, smooth normals
and all 13,148 triangles / 7,610 vertices, quantized to signed 16-bit values. It removes UVs,
the marble texture, PBR materials and their texture-transform extension, since the native renderer
uses the user's selected color. No decoder, network fetch or external URI is used at runtime.
`scripts/prepare-concordia-model.py` reproduces the derived file from the exact hash-checked original.
The reviewed original is preserved in ignored local evidence under `target/native-window-proof/05c-model-source/`.

Concordia's olive crown, local renderer, palette and decorative signal treatment are project additions.
This is a classical bust adapted as Concordia's brand sculpture, not a claim that the source depicts
an authenticated historical portrait of the goddess. The separate bundled icon illustration is
original project artwork described in `docs/brand-direction.md`.
