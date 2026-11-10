# Concordia portrait

The portrait depicts Concordia, Roman goddess of harmony, in an original warm gold/bronze
pixel-art treatment. `concordia-portrait.png` is the transparent, unmodified generated master;
`concordia.png` and `concordia.ico` are size exports consumed by Home, the README and Windows.
The unused earlier geometric SVG was removed; Git retains its history.

Generated with the built-in imagegen tool. Direction: classical Roman female marble bust in
three-quarter profile, braided pinned-up hair, diadem and stola; small olive sprig; crisp retro
terminal pixel clusters; restricted antique-gold/amber/bronze palette; square composition,
transparent background, no text/frame/scenery. The full generation prompt is in
[brand direction](../../../docs/archive/brand-direction.md).

The warm terminal treatment was informed by the
[Hermes CLI banner](https://github.com/NousResearch/hermes-agent/blob/main/hermes_cli/banner.py).
No Hermes image, caduceus, wordmark or source code is included in these assets.

Maintainers can regenerate the size exports with `python crates/concordia/assets/render_icon.py`
after installing Pillow. These tools are not runtime or end-user dependencies.

`build.rs` generates the Windows resource source with the icon, manifest and Cargo version;
there is no separate static resource file to maintain.
