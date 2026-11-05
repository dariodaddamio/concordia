# Concordia brand direction

The Roman goddess Concordia provides the portrait, rather than an abstract letter mark.
Gold/bronze pixel art suits a terminal-oriented tool; it remains a compact accent on Home and
the bundled application icon. Build 05c replaces the Home portrait with an original, rotating
3D Concordia bust: classical facial profile, swept hair, olive crown and museum plinth.
The anatomical base is an adapted CC0 classical bust; [source and conversion](../crates/concordia/assets/concordia-model-CREDITS.md)
record its exact provenance. The compact mesh is bundled, with a local Rust depth-buffer renderer
and no external runtime or asset fetching.
Small windows omit it to preserve chat and goal space. `/config` selects its color and the
running-window icon tint together; Explorer retains the bundled original icon.

The **Opifex Studio** theme follows the local Studio host identity tokens: near-black `#050506`,
amber `#FFB000` and bone `#D8D0C0`. Scan texture and an occasional brief tear are confined to
the sculpture. Motion can be disabled, respects the Windows animation preference, and stops
outside Home. Other themes show the sculpture without the glitch treatment. No chat text or
attachment is distorted. The original portrait below remains the source for the compact icon.

The reference is the warm terminal aesthetic of the
[official Hermes CLI banner](https://github.com/NousResearch/hermes-agent/blob/main/hermes_cli/banner.py).
The illustration is original and uses Concordia's subject rather than Hermes' caduceus.

Built-in imagegen prompt:

> Create an original application brand portrait for CONCORDIA, an elegant fast command-line coding
> tool. Subject: the Roman goddess Concordia, personification of harmony, as a classical Roman
> marble bust in three-quarter profile looking right, calm dignified feminine face, braided
> pinned-up Roman hair with a modest classical diadem and draped stola. A small olive sprig beside
> her shoulder conveys harmony. Visual treatment: beautiful deliberate retro terminal pixel art,
> sharply drawn chunky pixel clusters with a warm antique gold / amber / bronze restricted palette,
> luminous pale gold highlights, darker bronze shadows. Make her recognizably a classical goddess,
> with sculptural face and graceful silhouette, not a robot, not a letter or abstract symbol.
> Square composition, centered bust, generous clear margins, strong silhouette readable as a small
> Windows application icon. No text, no lettering, no frame, no drop shadow, no scenery, no symbols
> from other brands. Transparent background. This is an original Concordia goddess illustration in
> the spirit of the gold-and-bronze retro CLI aesthetic, not a reproduction of any existing logo.
> The asset will also be displayed as a compact home-page portrait. Favor crisp pixels and restrained
> detail over photographic realism.
