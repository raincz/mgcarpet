# mgcarpet

A modern, cross-platform engine port/reimplementation of Bullfrog's
**Magic Carpet** (1994), its **Hidden Worlds** expansion, and
**Magic Carpet 2: The Netherworlds** (1995), written from scratch in Rust
on wgpu (WGSL shaders), winit and cpal.

Questions, bug reports, playtesting: the Magic Carpet Discord —
https://discord.gg/GR55HCbJJ4

## About

This is a from-scratch engine reimplementation: it ships no game content and
requires original game data from a legally owned copy (both games are sold
on [GOG](https://www.gog.com/en/game/magic_carpet) —
[MC2 here](https://www.gog.com/en/game/magic_carpet_2_the_netherworlds)).
The goal is one engine that plays both games and the expansion — with
fixed-timestep simulation (game speed independent of framerate), GPU
rendering with real draw distance, save-anywhere, and modern platform
support — while treating the original gameplay as the specification:
behavioral fidelity by default, deviations deliberate and opt-in.

It is built with heavy use of Claude Code (Fable / Opus) as well as other
AI tools.

However, this is not a "cleanroom AI decompile", the likes of which
have been popping up lately. Decompile has been used primarily in the
early stages as a reference, and is not a fundamental part. All
features are implemented deliberately, most to faithfully match what the
original look & feel of the retail game was like, but never blindly or
mindlessly. And given the buggy state of the retail games, a substantial
amount of deliberate usually subtle deviations have been implemented to
improve playability, graphics, effects, etc., over the original legendary,
but nonetheless dated game.

The primary mechanism for ensuring correctness is not 1:1 decompile,
rather per-tick comparisons to recorded retail gameplay, which results
in faithfulness of the outcome, rather than the means. Even though the
code structure, especially for the sim, may resemble what retail looked
like, it is nonetheless cleaned up and fixed, wherever that was needed.

And most importantly, the sim, and with it the general game behaviour,
is where the attempts at real faithfulness end. The entire graphics
rendering pipeline is intentionally discarded and built from scratch,
with the intent to look like the original retail game, while at the same
time leveraging all modern technologies that make it far more stable
and playable.

![Magic Carpet 1 - Final level](docs/media/mc1-l49.webp)
![Hidden Worlds - Level 20 rough start](docs/media/mc1hw-l20.webp)
![Magic Carpet 2 - Final level Hydra](docs/media/mc2-l20.webp)

## Quickstart (playtesting)

1. Get the `mgcarpet` binary: a
   [release](../../releases) archive for Linux, Windows or macOS, or
   build it yourself (below). The macOS archive is a universal binary
   (Apple Silicon + Intel) that CI builds and unit-tests but nobody has
   yet played on real hardware — reports welcome. It is unsigned, so
   macOS quarantines it on download; release it once with
   `xattr -dr com.apple.quarantine <the unpacked folder>` (or right-click
   → Open on the binary).
2. Unpack the archive anywhere and run `mgcarpet`. It opens the
   **launcher**, with a card for each of the three games.
3. **Add your game data from the launcher.** On a first run the
   **Game data** dialog opens by itself (later, it's the
   **Game data…** button at the bottom left, or click a game that is
   still crossed out). Press **Add game data…**, say what you have, and
   pick it:

   * **GOG installer** — the `setup_magic_carpet_….exe` you downloaded
     from GOG. Nothing needs installing first, and no Wine: it is
     unpacked with the [innoextract](https://constexpr.org/innoextract/)
     tool, which every Linux distribution packages (macOS: `brew install
     innoextract`). The dialog tells you if it's missing. On Windows,
     the simplest is to run the installer as usual and pick the
     installed folder instead.
   * **Installed GOG game** — the folder GOG's installer put the game
     in (it contains `game.gog` for Magic Carpet 2, or a `CARPET.CD`
     folder for Magic Carpet Plus).
   * **Original CD image** — a `.cue` with its `.img` or `.bin`
     (Magic Carpet 2).

   The launcher checks what you picked, copies what it needs into its
   own `gamedata/` folder (next to the binary, when started from its
   folder — so the original can go afterwards), and then prepares the
   game data: a minute or two, a few minutes the first time, which
   also renders the music. **Leave the launcher open until the green
   "please wait" message is gone**;
   the games' cards light up when it is done. Any subset works: Magic
   Carpet Plus brings Magic Carpet and Hidden Worlds, Magic Carpet 2
   is separate.
4. Pick a game and press Start. You can also skip the launcher and
   double-click the campaign launcher for the game you want to play:

   * `magic-carpet-1` — the Magic Carpet campaign
   * `magic-carpet-hidden-worlds` — the Hidden Worlds campaign
   * `magic-carpet-2` — the Magic Carpet 2 campaign

   Each one simply starts `mgcarpet --campaign <game>` from its own
   folder: level order, exits and retail-format saves included.

   The `mgcarpet` binary itself does the same and more, from a shell:

   ```sh
   ./mgcarpet                 # the launcher

   ./mgcarpet --level mc1:9   # a specific level (mc1 | mc1hw | mc2)

   # a full campaign (mc1 | mc1hw | mc2); like retail, it starts a
   # fresh unsaved run — save (or load) from the in-game menu, or
   # resume save slot N directly with --slot
   ./mgcarpet --campaign <game> [--slot N]
   ```

   The prepared ("baked") data is redone by itself after an upgrade
   that changes it (it carries an epoch stamp). To keep the game data
   somewhere other than `gamedata/`, set `MGC_GAMEDATA` or
   `"gamedata"` in `mgcarpet.json`. Placing the files by hand still
   works — [gamedata/README.md](gamedata/README.md) describes the
   layout, the command-line importer and community mods.

   Most tweakable options are available in several ways:
   * Runtime menu (displayed when game is paused)
   * Key mappings for runtime toggles
   * CLI flags
   * json config file entries

   Notably, options that affect the sim as a whole can only be changed
   on startup, ie. NOT at runtime.

   Config lives in `mgcarpet.json` (sparse overrides) next to the
   generated `mgcarpet.json.defaults`, which documents every option with
   its faithful-authentic default. `--help` lists the CLI flags. To
   bootstrap `mgcarpet.json.defaults`, simply run the game once. Then copy
   the file to `mgcarpet.json` and make your desired tweaks.

## Current Status

Both games and the expansion are playable end to end. The simulation is
bit-exact against thorough retail gameplay recordings across the certified
takes, strange and quirky retail behaviour included, and every remaining
divergence is registered rather than papered over.

Deviations from the retail game are deliberate - bug fixes or playability
improvements - and nearly all of them can be switched back to the retail
behaviour with a toggle.

Multiplayer is functional, but mostly redesigned to take the good from
the original game, while also bringing in stability as well as other modes
of playing.

The main issue of all retail Magic Carpet games is the dreaded entity pool
with its highly limited size and frequent overflows, resulting in strange
gameplay issues, sometimes crashes. This port turns it into a parameter
(default 20000 slots against retail's 1000; set `entity_pool_size` to
1000 for the retail limit), one of the main reasons why gameplay of
levels with heavy activity tends to be a lot smoother. Increasing the parameter also allows playing some of
the original hidden levels that were removed from the campaign for various
reasons, one of them being that they don't even load correctly due to limited
entity pool size.

### How "bit-exact" is checked

The original executables are the oracle. Retail play was recorded under
an instrumented DOSBox that wrote down the complete state of every game
entity on every game turn, together with the player's input. The port
replays that input through its own simulation and must reproduce the
recorded state exactly, turn by turn. Every campaign level of all three
games has been played through this way and replays identically (a few
recordings tore mid-take; those are certified up to the tear); where the retail
executable diverges from its own data (it reads past the ends of tables,
and damages its own memory in a few known places), the tick is
investigated until the cause is known and registered, never papered
over.

Rendering, visuals and audio is specifically excluded from exactness and
reimplemented from scratch, visually compared and tweaked. Most visual
deviations from retail are deliberate, given the age of the original game.

The faithfulness evidence that travels with the repository is in
[docs/CONFORMANCE-REPORT.md](docs/CONFORMANCE-REPORT.md): per level, the
retail state pairs the suite replays on every change, and the finite
list of ticks where retail diverges by definition. The deliberate
differences are in [docs/DEVIATIONS.md](docs/DEVIATIONS.md) — everything
not listed there is the retail game.

## Architecture

```
gamedata/  (your game data, added by the launcher — never committed)
    │
    ▼
mgc-import  ── the only code that understands Bullfrog formats (RNC,
    │          DAT/TAB, XMI, seeded terrain generation). Runs once per
    │          machine; expands everything into baked packages. All
    │          procedural/seeded data is expanded here — the engine never
    │          sees a seed.
    ▼
mgc-formats ── the baked package format: the sole data contract
    │
    ▼
mgc-sim     ── pure, headless, deterministic simulation core
mgc-render  ── wgpu renderer (reads sim state, interpolates between ticks)
mgc-audio   ── cpal output + the ported original mixer + FLAC music
mgc-app     ── winit shell: window, input, fixed-timestep game loop
```

Verification strategy: original-engine output is the oracle. Expanded
terrain and parsed data are validated byte-for-byte against reference
dumps produced by the original code ([Magic Carpet 2 HD][remc2] for MC2,
instrumented DOSBox for MC1). Fixtures live outside git (derived from copyrighted data); their
SHA-256 hashes are committed as pins.

## Building

```sh
cargo build --workspace
cargo test --workspace
```

Rust 1.88+ (edition 2024). Linux needs the ALSA headers for audio
(`libasound2-dev` on Debian/Ubuntu, `alsa-lib` headers elsewhere);
Windows and macOS need no system dependencies.

## Game data

The engine consumes *baked* packages, generated from the original game
data under `gamedata/`. The launcher adds that data and bakes it by
itself (see Quickstart); `mgc-import` exposes the same machinery as a
standalone tool:

```sh
cargo run -p mgc-import -- add gamedata <installer, folder or .cue>  # import a source
cargo run -p mgc-import -- sources gamedata   # what is found, and what is used
cargo run -p mgc-import -- scan gamedata      # integrity-check the data
cargo run -p mgc-import -- bake gamedata baked   # bake everything
```

The layout, the supported sources and community mods are described in
[gamedata/README.md](gamedata/README.md).

## The level package format

Levels bake to `.mgcl` files — stored (uncompressed) ZIP containers with
JSON for structured data and raw binary for bulk grids, one schema
covering both games. The format is explicitly documented in
**[docs/FORMAT.md](docs/FORMAT.md)**; that document is normative and
evolves in lockstep with the code. Community-authored levels in this
format are original works and freely shareable (unlike packages baked
from the copyrighted game data, which stay on your machine).

## Credits and prior art

This project stands on years of community reverse engineering. It would
not exist without:

- **Tomáš Veselý (turican0)** — the original
  [remc2](https://github.com/turican0/remc2), the assembly→C++
  reconstruction of Magic Carpet 2 that made the engine family
  understandable at all.
- **Tim Hobbs (thobbsinteractive) and contributors** —
  [Magic Carpet 2 HD](https://github.com/thobbsinteractive/magic-carpet-2-hd),
  the actively maintained continuation of remc2, whose codebase serves as
  this project's behavioral reference, and whose `DataFileRNC`
  implementation our RNC decoder is ported from.
- **Michael Howard** —
  [MagicCarpetFileFormat](https://github.com/michaelhoward/MagicCarpetFileFormat),
  the MC1 level format specification our parser is built against.
- **Moburma** — [Magic Carpet tooling](https://github.com/Moburma)
  (MCDatExtractor, MCLevelReader, MCLevelEdit, BullfrogSoundExtractor)
  and the extensive unused-content documentation on
  [The Cutting Room Floor](https://tcrf.net/Magic_Carpet_(DOS)).
- **lab313ru** —
  [rnc_propack_source](https://github.com/lab313ru/rnc_propack_source),
  the decompiled original RNC ProPack.
- **Bullfrog Productions** — for the 1994 original that was so far ahead
  of its time we're still catching up to it.

## License

GPL-3.0-or-later. The RNC decompressor is ported from remc2's
implementation (itself derived from the decompiled original ProPack).

[remc2]: https://github.com/thobbsinteractive/magic-carpet-2-hd
