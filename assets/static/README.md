# Static bundled assets

Redistributable third-party files the game ships (as opposed to
user-supplied game data in `gamedata/` or derived output in `baked/`).

## GeneralUser GS — the General MIDI soundfont

`GeneralUser-GS.sf2` (~31 MB) is the soundfont the pure-Rust synth in
[`crates/mgc-import/src/synth.rs`](../../crates/mgc-import/src/synth.rs)
uses to render the GM music arrangement at bake time. Shipping it makes
GM the out-of-the-box default on every platform — no system fluidsynth.

- **Source:** <https://github.com/mrbumpy409/GeneralUser-GS> (GeneralUser
  GS v2.0.x by S. Christian Collins).
- **`GeneralUser-GS.sf2`** and **`GeneralUser-GS-LICENSE.txt`** are both
  committed — the font is a static vendored dependency (it changes maybe
  once every few years, so it's a one-time ~31 MB object, not history
  churn), which keeps fresh clones and releases self-contained: dev
  builds read the `.sf2` from here, releases copy both files beside the
  binary (`.github/workflows/release.yml`). The font's license is
  bundled with every release, as its terms require.

If the `.sf2` is absent at bake time the synth falls back to a distro GM
font, and failing that to the AdLib **FM** arrangement — the game still
runs, just without the GM upgrade.

### Using a different soundfont

Any General MIDI `.sf2`/`.sf3` works — no code change needed. Two ways:

- Set `MGC_SOUNDFONT=/path/to/font.sf2` before the bake (highest
  priority in discovery); or
- install one to a standard location — the fallback list already
  recognizes `FluidR3_GM.sf2` (a larger, MIT-licensed GM font),
  `TimGM6mb.sf2`, and the common `default.sf2` paths.

Rebake after changing it (bump `mgc_formats::BAKE_EPOCH`, or delete the
baked `music/` output) to re-render the GM tracks with the new font.
FluidR3 in particular is a fine MIT-licensed alternative for anyone who
prefers it to the shipped GeneralUser GS.

## awe32.rom.zst — the Sound Blaster AWE32 wave ROM

`awe32.rom.zst` is the AWE32's 1 MB General MIDI wave ROM (the card's
`awe32.raw` dump), zstd-compressed to keep the shipped assets small. The
AWE32 renderer (`crates/mgc-awe32`, ported from
[AWE32Emu](https://github.com/turican0/AWE32Emu)) plays the AWE32 music
arrangement from it at bake time, the arrangement both games' authors
tuned for (MC2 ships its own AWE32 scores and instrument bank). It is
compiled into the bake tool, so nothing has to sit beside the binary.

The ROM content is Creative Technology's. The same dump ships with
AWE32Emu and remc2; it is bundled here on the same basis, for this free,
non-commercial project, and will be removed on the rights holder's
request.
