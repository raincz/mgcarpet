# The `.mgcl` level package format

Version 1 — this document is normative. Any tool that reads or writes
`.mgcl` files should treat this file as the specification; changes to the
format must land in the same commit as changes to this document.

## Design goals

- One self-contained file per level, cheap to load, trivial to inspect.
- A **superset of both games**: Magic Carpet 1, Hidden Worlds, and Magic
  Carpet 2 levels use one schema; earlier games simply omit the features
  they don't have.
- **Expanded, not seeded**: everything the original games generate from
  seeds at runtime (terrain, most importantly) is stored pre-generated.
  Engines and editors never deal with seeds, RNG sequencing, or original
  Bullfrog formats.
- Community-friendly: any language with a zip library and a JSON parser
  can read and write levels. Original *game-derived* packages must not be
  redistributed (they are derived from copyrighted data), but
  community-authored levels in this format are original works.

## Container

A `.mgcl` file is a **ZIP archive with all members stored uncompressed**
(compression method 0, "stored").

- Writers MUST NOT compress members and MUST write deterministic output
  (fixed timestamps), so identical content yields identical bytes.
- Readers MAY reject members using any compression method other than
  stored.
- Member names are case-sensitive and use `/` as the path separator.
- Tools MUST preserve members they do not understand when rewriting a
  package (read-modify-write, not regenerate-from-model).

## Members

| Member | Required | Format | Content |
|---|---|---|---|
| `meta.json` | yes | JSON | Format version, game, level identity, provenance |
| `things.json` | yes | JSON | Entity and marker records |
| `genparams.json` | no | JSON | Original terrain-generation parameters (provenance) |
| `level.json` | no | JSON | MC2 level header: environment, player slots (absent on MC1) |
| `wizards.json` | no | JSON | Per-wizard configuration: AI stats, spell loadouts (MC2 since v1, MC1 since v2) |
| `stages.json` | no | JSON | MC2 mission script: stage checkpoints and variables |
| `terrain/height.bin` | no | binary | Expanded heightmap (see below) |
| `terrain/type.bin` | no | binary | Expanded terrain-type map |
| `terrain/shading.bin` | no | binary | Per-tile light level (see below) |
| `terrain/angle.bin` | no | binary | Per-tile texture orientation/flags (see below) |

MC1/Hidden Worlds terrain is always present (the importer generates it
natively); MC2 terrain is present when the importer had the MC2 oracle
tool available. Readers must tolerate absence, and the terrain members
always appear together.

### `meta.json`

```json
{
  "format_version": 1,
  "bake_epoch": 1,
  "game": "mc1",
  "level": 0,
  "source": {
    "archive": "LEVELS.DAT",
    "entry": 0,
    "entry_sha256": "<hex sha-256 of the raw archive entry>"
  },
  "overlay": "mc1/levels/LEV00032.DAT",
  "importer": { "name": "mgc-import", "version": "0.1.0" }
}
```

- `format_version` (integer): this document's version. Breaking changes
  bump it; additive changes (new members, new optional JSON fields) do
  not.
- `bake_epoch` (integer, `mgc_formats::BAKE_EPOCH`): the bake CONTENT
  epoch — bumped when the importer's output changes under an unchanged
  schema (a decode fix, corrected tables, a new member), so consumers
  can tell a baked tree is stale. Artifacts baked before the field
  existed deserialize as 0 (always stale). The game shell checks it at
  startup and rebakes automatically (see "Versioning and evolution").
- `game`: `"mc1"`, `"mc1hw"` (Hidden Worlds), or `"mc2"`.
- `level`: index of the level in its source archive (campaign order for
  retail levels).
- `source`: provenance of game-derived packages — enough to regenerate
  and to verify the input was the expected retail data. Absent on
  community-authored levels.
- `overlay` (optional): overlay-relative path of the community
  replacement file the payload came from (`gamedata/overlay/`,
  docs/MODDING.md). Present = a MODDED package: `source` is kept (the
  member slot it replaces) but `entry_sha256` hashes the overlay file
  rather than the retail archive entry — so save identity checks keep
  working — and the package must never feed goldens or conformance. A
  bake that applied any overlay file also writes a `MODDED` marker
  file at the baked root listing every substitution; a pristine bake
  removes it.
- `importer`: tool and version that produced the package.

### `things.json`

All placed objects, in a single array ordered by original slot index:

```json
{
  "things": [
    { "slot": 0, "kind": "entity", "class": 2, "model": 0,
      "x": 45, "y": 144, "dis_id": 65535, "swi_sz": 0, "swi_id": 65535,
      "parent": 0, "child": 0 }
  ]
}
```

- `slot`: index in the original entity table. `parent`/`child` fields
  reference these indices (linked lists: paths, wizard groups), so slots
  are preserved verbatim from the source data.
- `kind`: `"entity"` (a real placed thing) or `"marker"` (MC1-only:
  class-0 records observed in retail data as structured coordinate
  chains, probably terrain-feature nodes; semantics not yet confirmed).
  In **MC2**, class 0 is Conditional Spawn — a real gameplay entity that
  materializes when its trigger group fires — and is `"entity"`.
  Garbage slots present in retail files (uninitialized editor memory)
  are **not** baked.
- `class`/`model`: original Bullfrog type IDs, preserved exactly so any
  package can be diffed against reference-engine behavior. The known
  class/model → name mapping is documentation, not data. Note the IDs
  are per-game namespaces (e.g. spell pickups are class 12 in MC1,
  class 15 in MC2).
- Coordinates are on the game's 256×256 logical grid.
- `dis_id`, `swi_sz`, `swi_id`: disposition and switch linkage as in the
  original format; `65535` (0xFFFF) means "none".
- **MC2 field mapping**: values are byte-order-normalized from the
  on-disk representation (little-endian in GOG retail data). MC2 semantics
  occupy the shared fields as follows: `swi_sz` holds MC2's unnamed
  word at record offset 0x0A; `swi_id` holds the **stage tag** (MC1's
  switch-target ID repurposed for the mission system — still trigger
  linkage); `parent`/`child` hold context parameters 1/2 (building
  type, teleport destination X/Y, or path parent/child links depending
  on entity type); `par3` (MC2 only, absent on MC1 records) holds
  context parameter 3.

### `genparams.json`

The original `GEN_MAP` block, kept for provenance and for editors that
want to "reroll" terrain via original-engine oracles. Engines MUST NOT
require it: the authoritative terrain is the expanded `terrain/*` data.

```json
{
  "pre_header": 135538,
  "seed": 1921, "off": 41339, "raise": 2834, "gnarl": 0,
  "river": 1, "sourc": 0, "snlin": 200, "snflt": 50,
  "bhlin": 30, "bhflt": 16, "rkste": 18,
  "footer": [35, 1, 0, 0, 0, 0]
}
```

Field names are Bullfrog's own (recovered from their GAM save-game text
format). `raise` is signed. Game-specific fields: `pre_header` and
`footer` are MC1-only; `lriver` (river length/count) is MC2-only. Each
is omitted where not applicable.

### `level.json` (MC2 only)

```json
{
  "level_id": 3, "gfx_type": 0, "map_type": "day",
  "players": [1, 1, 0, 0, 0, 0, 0, 0],
  "basic_height": 0, "unk07": 10, "number_of_players": 3
}
```

- `map_type`: `"day"`, `"night"`, or `"cave"` — selects the entire asset
  set the original engine loads (sprites, sky, palette, tables, blocks).
- `players`: authored starting-castle LEVEL per wizard color (0 = none,
  N = a level N−1 castle built at that wizard's spawn).
- `basic_height` (epoch 8; alias `unk05`): the cave ceiling mirror
  pivot; meaningless off-cave.
- `number_of_players` (epoch 9; alias `unk09`): colors 0..n−1 spawn
  wizard carpets — 0 = the human, 1..n−1 = AI rivals.
- `unk07`: unexplained original header field, preserved verbatim
  (retail repurposes it as runtime objective scratch).

### `wizards.json`

Exactly 8 blocks; slot 0 is the human player, 1–7 the AI wizards.
Both games bake one; per-game fields are omitted where not
applicable.

MC2 (from the level header block):

```json
{
  "wizards": [
    { "aggression": 128, "reflexes": 100, "perception": 80, "life": 500,
      "starting_spells": [1, 0, ...26 values...],
      "starting_spell_levels": [0, ...],
      "blocked_spells":  [0, ...] }
  ]
}
```

Spell arrays have 26 entries indexed by MC2 spell ID (0 = Fireball …
25 = Cave In); `starting_spells` values are upgrade tiers 0–3.
`starting_spell_levels` (epoch 9; alias `unknown_spells`) is the
per-spell STARTING XP LEVEL 0–2 — the AI rivals' spell-XP seed
(docs/traces/mc2-rivals-spawn-mortality.md §3).

MC1/Hidden Worlds (format 2+; from the level record's 8 × 216-byte
per-player table at offset 37072 and the decoded 12-byte tail —
which `genparams.json`'s `footer` still preserves verbatim):

```json
{
  "wizards": [
    { "aggression": 128, "accuracy": 128, "tempo": 128,
      "castle_level": 0,
      "starting_spells": [1, 0, ...24 values...],
      "allowed_spells":  [1, 1, ...24 values...] }
  ],
  "player_count": 2,
  "tail_38800": 35
}
```

- `player_count`: active wizard slots (the engine services only slots
  below it; slot 0 = the human).
- `aggression`/`accuracy`/`tempo` (0–255): the AI personality —
  remc1 Type_160 `u16_522`/`u16_524`/`u16_526` (hate and war
  thresholds / aim cone / decision-and-burst cadence, turn agility,
  respawn delay).
- `castle_level`: 0 = no starting castle, N = the wizard spawns with
  a castle at level N−1 (AI slots; nonzero values on slot 0 appear
  on some levels — semantics for the human unverified).
- `starting_spells` (the record's var_230883) and `allowed_spells`
  (var_230983), 24 flags by MC1 spell ID: an AI wizard's level-start
  book grants spells flagged in BOTH; the human grant intersects
  `allowed_spells` with campaign-collected flags; `allowed_spells`
  also gates what an AI may learn mid-level.
- `tail_38800`: unexplained tail word, preserved verbatim.

### `stages.json` (MC2 only)

The mission script, preserved verbatim from the level's stage tables
(unused 0xFF-filled slots are omitted):

```json
{
  "checkpoints": [ { "index": 0, "stage": 1, "x": 115, "y": 212 } ],
  "variables":   [ { "index": 0, "stage": 1, "x": 10, "y": 20, "data": 0 } ]
}
```

Checkpoints and variables together encode the objective sequence.
Observed decoding of checkpoint records (confirmed against the spec's
gameplay-verified tutorial script — e.g. MC2 level 0 bakes to
`(5, 0, 115, 212)` = fly-to (115,212), `(7, 103, 0, 0)` = kill creatures
linked to entity slot 103): `index` holds the **objective opcode**,
`stage` the opcode's parameter (entity slot, mana threshold, or wizard
id), `x`/`y` the target coordinates. Opcodes per remc2's BasicTerrain.h:
0 = collect mana, 1/7 = kill creature, 2 = destroy structure,
3 = kill enemy wizard, 5 = release point / fly-to, 8 = kill all players,
9 = destroy building. Field names stay as the originals pending full
confirmation; the data is the raw source of truth. Objective
display text lives in the game's `ETEXT.DAT` and is not part of the
package.

### `terrain/height.bin`, `terrain/type.bin`, `terrain/shading.bin`, `terrain/angle.bin`

65,536 bytes each: one byte per tile on the 256x256 grid, row-major,
index `y * 256 + x`, matching the original engine's in-memory layout
(height in `height.bin`, per-tile terrain/texture type in `type.bin`,
light level in `shading.bin`, texture orientation in `angle.bin`).
`shading.bin` and `angle.bin` are optional (additive members).

`shading.bin` indexes the shade dimension of the game's color-remap
tables — the engine resolves a tile's displayed color as
`palette[tables[shading * 256 + tables[0x14000 + type]]]` (both table
slices are baked as assets: `shade-lut-N.bin`, `tile-colors-N.bin`).

`angle.bin` is the generator's per-tile angle/flags byte. Bits 4-6
select one of 8 UV orientations for the tile's terrain texture
(dihedral: bit 4 = flip x, bit 5 = flip y, bit 6 = swap axes —
decoded from the engine's `UVTable_D4350`, world-space/base-0 rows;
critical for one-directional transition tiles like shorelines). The
other bits carry generator flags (terrain class in bits 0-2, deep
water — MC1 — or cave ceiling — MC2 — in bit 3, visibility in bit 7)
and are preserved verbatim.

The content is the **pristine output of the original generation
algorithm**, generated natively by the importer for both games. MC2:
`mc2_terrain.rs`, a port of the remc2-carved algorithm, byte-for-byte
identical to the retired `mc2-genlevel` oracle across all 165 levels ×
5 planes (which was itself validated against remc2's DOSBox-derived
regression fixtures; the C++ lives in git history). MC1/Hidden Worlds:
`mc1_terrain.rs`, a port of MC1's own generator (from the remc1
decompilation — docs/ROADMAP.md "MC1 reference generator found"), whose
heightmap reproduces the previously-validated oracle output
near-byte-exactly and whose type layer is MC1's real classifier. Entity-driven terrain
modification (walls, canyons, building flattening) is deliberately NOT
baked in: engines apply those at load time from `things.json`, exactly
as the original engine does after generation (implemented for MC1/HW
in `mgc_sim::features` — the GenerateFeatures port; it additionally
consumes the `search.bin` ring table and `build.{tab,dat}.bin`
building footprints from the level's asset bundle). Vertical-scale
and water-level semantics will be documented as the renderer work
firms them up.

## Versioning and evolution

1. New members and new optional JSON fields are additive — no version
   bump; old readers ignore them.
2. Changing the meaning or layout of an existing member bumps
   `format_version`; readers reject versions they don't know.
3. Changing the CONTENT the importer emits — a decode fix, corrected
   tables, a new baked member — without touching the schema bumps
   `bake_epoch` (`mgc_formats::BAKE_EPOCH`, stamped into `meta.json`
   and every `bundle.json`). Schema versions answer "can this reader
   parse it"; the epoch answers "is this bake current". The game
   shell compares stamps at startup and reruns the full bake
   (`mgc_import::bake::bake_all` — the same path as `mgc-import
   bake`) when anything is missing or stale.
4. This file is updated in the same change as the code.

# Asset bundles

The second engine-facing format (Rust types + loader:
`mgc_formats::bundle`): everything translated from one game's asset
catalogs, as a directory of uniformly-named members. Where `.mgcl`
carries what is *level*-scoped, a bundle carries what is *world*-scoped
— palettes, color LUTs, terrain textures, sprites, terrain-feature
data; text will join additively. Sounds and music live in per-game
audio bundles (see "Audio bundles" below).

One schema, many *variants*: game and environment differences are
expressed as bundle instances, never as layout differences. Current
variants: `mc1-temperate`, `mc1-arctic` (MC1's two complete world
tilesets; Hidden Worlds levels use the arctic one) and MC2's four
environment graphics sets `mc2-day`, `mc2-night`, `mc2-night-fog`,
`mc2-cave` (per-level selector: the header's `map_type`, with night
levels whose `gfx_type` has bit 1 set using the fog set — remc2
Level.cpp:890). The engine resolves a variant id
(`baked/assets/<variant>/`); Bullfrog catalog names, RNC, sprite RLE,
and FLC animation encodings all die in the importer.

All integers little-endian; all pixel data 8-bit palette indices
(palette-as-LUT is the engine's authenticity baseline — RGBA is a
render-time resolve, and index 0 is the sprite-transparent index).

| member | contents |
|---|---|
| `bundle.json` | manifest: `format_version`, `bake_epoch` (same semantics as `meta.json`), `variant`, `game`, importer, source catalog files + raw-file sha256 |
| `palette.bin` | 256 x RGBA8, VGA 6-bit expanded (`v<<2\|v>>4`); index 0 has alpha 0 |
| `shade-lut.bin` | 64 rows x 256: `shade level x palette index -> final palette index` (the light/fog remap; the TABLES blob at +0x0000 in BOTH games — row 32 ≈ identity, row 0 = the fog/sky color, row 63 = black; epochs ≤ 4 mis-carved MC2's at +0x4000, which is the sprite blend matrix — see docs/traces/mc2-transparency-drawlist.md) |
| `tile-colors.bin` | 256: terrain type -> flat map color index (TABLES blob +0x14000, both games) |
| `terrain-atlas.bin` + `.json` | terrain texture atlas, square cells; the json gives `{cell, width, cells}`; the terrain-type byte indexes cells row-major |
| `sprites.bin` + `sprites.json` | one 8bpp atlas of all world-sprite frames + its index (below); atlas width doubles from 1024 as needed to stay under the 8192 texture-dimension baseline |
| `search.bin` | 32x32 ring-order grid (terrain-feature digs / search scans) — same 1024-byte format in BOTH games (remc2 sub_101C0) |
| `build.tab.bin`, `build.dat.bin` | MC1: building footprint RLE maps (terrain-feature building pass) |
| `bldgprm.bin` | MC2: building-parameter table, `BLDGPRM.DAT` verbatim — 4-byte records `{u16 word, u8 flags, u8 chain-next}`; retail loads 76 records into a 77-slot table (remc2 sub_539A0); flags: 0x10 load-pass split, 8 drawing/LOS, 4 cave presence, 1 solid |
| `spells.bin` | MC2: spell table, `SPELLS.DAT` verbatim — 26 rows x 80 bytes (remc2 Spells.h): `{i8, u8 enabled, 3 x 26-byte subspell tiers}`, each tier `{i32 subSpellIndex, i32 manaCost, i32 maxManaLimit, i32 xpos1, i32 xpos2, i16 hintText, i16 word_0x18, i8 life, u8 fontType}`; feeds the par1-authored class-10 effect overrides (GetSpellIndex map 9→18, 11→16, 15→17, 17→9, 22→21, 67→20, 71→15) and class-15 cast costs; the retail CD table differs from the decompile's Spells.cpp fallback — the CD wins |
| `ui-sprites.bin` + `.json` | 2D UI sprite library (HSPR: spell icons, HUD panel, mana-bar frames, level pips, map markers), same atlas + `SpriteIndex` schema as `sprites` with one frame per entry and `group == id`; entries 6..=29 are the 24 spell icons keyed by internal spell type, 83/84 the advertised-trigger map X-markers (MC1 only until MC2's UI track) |
| `web-sprites.bin` + `.json` | MC2 (since bake epoch 15): the fullscreen spider-web/paralyze viewport overlay bank (`DATA/HWEBD0-0` day / `HWEBN0-0` night+night-fog / `HWEBC0-0` cave — the hi-res set; VGA `MWEB*` not baked), same atlas + `SpriteIndex` schema as `ui-sprites`: a 6×4 grid of 24 equal 8bpp tiles (transparent index 0, sprite ids 1..=24) covering 640×480; retail tiles them over the view while the paralyze web (`mobilizeCounter`) is live (remc2 EF:21668-710), hard on/off, no fade |
| `book-palette.bin` | 256 x RGBA8 like `palette.bin`: the book/spellbook screen's own palette (MC1 `DATA/BOOK.PAL`) |
| `etext.json` | the game's sentence bank (`DATA/ETEXT.DAT`, since bake epoch 14): a JSON string array indexed by the engine's sentence id, empty slots preserved (`""`) so ids stay aligned. MC2 (471 entries): 23..=47 the map-screen level briefings (portal L → 23+L), 48..=158 the per-level objective/completion blocks indexed by remc2 GameUI.cpp:20-42's `IndexLevelText_DB4EE`/`LevelEndText_DB507` tables (objective k of level L → `IndexLevelText[L]+k`, completion → `LevelEndText[L]`), 284/285 the secret-realm lines. MC1 (80 entries): 60/61 the win message. English base; localized `LANGUAGE/L<n>.TXT` overlays share the indices and are not baked |
| `sky.bin` | the parallax sky bitmap (since bake epoch 14): 256x256 8bpp raw, row-major — retail tiles it infinitely on both axes (sample index low byte = u, high byte = v; remc2 DrawSky_40950). Sources: MC1 `SKY.DAT` (temperate) / `SKY1-0.DAT` (arctic), MC2 `SKYD0-0.DAT` (day) / `SKYN0-0.DAT` (night AND night-fog — remc2 ReadAndDecompress.cpp:41/88). ABSENT on `mc2-cave`: retail never loads a cave sky (`SKYC0-0.DAT` ships dormant on the CD) |
| `blend-lut.bin` | 64KB blend matrix (TABLES +0x4000..+0x14000 in BOTH games, the slice between the shade LUT and map colors; ≈ `nearest_palette(⅓·src + ⅔·dst)`): 2D blits resolve `blend[src \| dest<<8]` — UI sprites (spell icons) only show their true colors composited through it (remc1 `strPal.byte_BB934_BB924`, sub_main.cpp:27444); the same matrix is the world-sprite translucency table (remc2 `T[0x4000 + (src<<8)\|dst]`, raster modes 2/3 — docs/traces/mc2-transparency-drawlist.md) |

`sprites.json` (`mgc_formats::bundle::SpriteIndex`): `atlas_width`,
`atlas_height`, and one entry per original sprite id (dense — ids are
the original engine's world-sprite numbering; known-corrupt retail
entries stay as frame-less placeholders). Each entry: `id`, `group`
(first id of its rotation/animation family, from the TMAPS TAB group
field), `width`/`height` (all frames share one size), original
`flags`, and `frames[]` of atlas `{x, y}` positions — frame 0 is the
base image, further frames are the pre-decoded FLC animation. The
flags high byte is the engine's *draw type* (how facing/animation pick
a family member — see `mgc_sim::mc1_sprite_stats`); flags bit 0 marks
animated entries.

Since bake epoch 22 `SpriteIndex` also carries `pointer_base`
(serde-default 0): on `ui-sprites` banks, the index where the appended
retail `DATA/POINTERS` cursor bank begins (its entries follow at
`pointer_base + k`, k = the retail bank index — [0] is a null "don't
draw" sentinel in both games; MC1's in-game arrow+mana-ball is [1] and
its menu HAND variants [2..=8]; MC2's grey cursor is per map type: day
[1], night [9], cave [10]). 0 = no pointer bank (non-UI atlases, older
bakes).

Provenance per source (MC1): `PAL{N}-0.DAT`, `TABLES.DAT` /
`DTABLES.DAT`, `BLK{N}-1.DAT`, `TMAPS{N}-0.DAT/.TAB`,
`BUILD{N}-0.TAB/.DAT`, `SEARCH.DAT` for N = 0 (temperate) / 1
(arctic). MC2 (all from the CD image): `PAL{D,N,F,C}-0.DAT`,
`TABLES{D,N,C}.DAT`, `BLOCK32.DAT` (day) / `BL32{N,F,C}0-0.DAT`,
`TMAPS{0,1,2}-0.DAT/.TAB` (digit = map_type ordinal; fog shares
night's tables and TMAPS), `SEARCH.DAT`, `BLDGPRM.DAT` (since bake
epoch 2 / the Phase-3 slice), `SPELLS.DAT` (since bake epoch 6).
MC2 bundles carry `search.bin` + `bldgprm.bin` + `spells.bin` in
place of MC1's `build.*.bin`. The versioning/
evolution rules above apply unchanged (`bundle.json`
`format_version`).

## Movie bundles

The full-screen FMV streams bake as their own per-game bundle
instances, `mc1-movies` and `mc2-movies` — the intro chain, MC1's
win/lose movies, MC2's six cutscenes and both endings. Loader:
`crate::movie::MovieSet` (mgc-app).

| member | contents |
|---|---|
| `bundle.json` | same manifest schema as graphics bundles |
| `movies.json` | `MovieIndex`: `movies[]` of `{name, file, frames, width, height, source}`; `name` is the lowercased source stem (`intro`, `outro`, `logo`, `cut1`, `levelw1`, `title-01`), which is how the engine's sequence tables refer to it |
| `movies/<name>.fmv` | the ORIGINAL stream, byte for byte |
| `font.bin` + `font.json` | the subtitle strip's font — `SFONT1` (both games ship it), HSPR glyph masks packed like every other baked font |
| `subtitles.json` | the string table the scripts' subtitle cues index: a JSON string array — MC1 `DATA/ETEXT.DAT` (80 entries, the intro narration at 0..=16), MC2 `LANGUAGE/L2.TXT` (471 entries, English; L1 is French) |

This is the one bundle that does not translate its input, and the
exception is deliberate. A decoded frame is 320x200 8bpp = 64 KB and
MC1's `INTRO.DAT` is 3165 of them, so pre-decoding would turn a 75 MB
stream into ~200 MB of canvases no runtime can hold. The engine
therefore keeps the original bytes and decodes one frame at a time
through `mgc_import::fmv::FmvCursor`, exactly as retail's
`PlayInfoFmv` does. The whole set is ~107 MB for MC1 and ~139 MB for
MC2; that is local disk only, since bundles are baked from the
player's own install and never shipped.

The format is a 12-byte Bullfrog header `{u32 header_size=12, u16
magic=0xAF12, u16 frame_count, u16 width, u16 height}` wrapping
Autodesk Animator FLIC frame chunks (BRUN, LC, SS2, BLACK, COLOR,
COPY, PSTAMP, prefix). Every stream in both games is 320x200. It
carries **no audio stream** — there is no field for one — but the
movies are not silent. Their soundtrack is assembled at playback time
by the per-movie event script, which cues sample-bank loads, one-shot
and looping effects and the narration voice clips out of the ordinary
`sounds.bin` banks, over the MIDI `INTRO`/`CUTS` sub-songs, with
subtitle lines against the narration. The scripts are compiled-in game
code, not file data, and live in `mgc-app`'s `movie::script`.

Streams are indexed by source stem only: which movie plays when is
the engine's business, so no role or sequence mapping is baked in.

## Audio bundles

Audio is per-GAME, not per-graphics-variant (MC1's sample/music bank
digits are level/screen selectors, not tileset pairs; duplicating the
redbook rip into four MC2 environment bundles would be absurd), so it
bakes as its own bundle instances: `mc1-audio`, `mc2-audio` — same
manifest schema, audio members only. Loader: `mgc_formats::bundle::
AudioBundle`.

| member | contents |
|---|---|
| `bundle.json` | same manifest schema as graphics bundles |
| `sounds.bin` | one deduplicated blob of raw PCM (unsigned 8-bit mono — the original sample data byte-for-byte) |
| `sounds.json` | `SoundIndex`: `sample_rate` (22050 — the best tier shipped by both retail games), `encoding` (`"pcm8"`), `banks[]` of `{bank, entries[]}`; each entry `{id, name, offset, len}` — `id` is the ENGINE sound id (the original bank-table index; the mixer's 47 request slots index bank 0 directly) |
| `music.json` | `MusicIndex`: `tracks[]` of `{bank, name, file, danger_file?, gm_file?, gm_danger_file?, awe_file?, awe_danger_file?, source}` |
| `music/*.flac` | one FLAC stream per track; in-game songs split into a base AMBIENT mix (`file`) plus a sample-aligned DANGER stem (`danger_file`, `*-danger.flac`) the runtime overlays with the original's combat gain ramp. MC1: the combat layers are MIDI channels 3/4/5 at CC7 0, faded by CC7 ramps (remc1 sub_20BD0/sub_20D00); `file` = the OPL3/FM render, `gm_file`/`gm_danger_file` (`*-gm[-danger].flac`, 44100 Hz STEREO, same ambient/stem contract) carry the General MIDI arrangement (`MUSIC<bank>-2`) rendered through oxisynth + a GM soundfont at import — present only when a soundfont is found (`MGC_SOUNDFONT` override, then the shipped/distro locations), FM always the fallback. MC2 (`mc2-night/day/cave/menu`): `file` IS the GM render (no FM fallback yet — the F section is a future faithful-alternate) of the MUSIC.DAT G-driver bank-0 ("C2") XMI sub-songs — bank 1 is the hidden `-music2` "C1"/MC1-set alternate, not baked as the default; the war/danger layers are the cc119-TAGGED channels (expression-zeroed in peace, combat-ramped — remc2 Sound.cpp:851/5880, docs/traces/mc2-music-dat-xmi.md), split into the same ambient + danger-stem pair. `awe_file`/`awe_danger_file` (`*-awe[-danger].flac`, 44100 Hz stereo, same ambient/stem contract; BAKE_EPOCH 31) carry the Sound Blaster AWE32 arrangement rendered at import by `mgc-awe32` (the AWE32Emu port) on the bundled wave ROM (`assets/static/awe32.rom.zst`) — MC2 from the game's own AWE32 scores (MUSIC.DAT driver slot W, `C2*.WTB`) through `SBAWE32.MDI` + `BULLFROG.SBK` with the MDI driver's start-up channel state (master volume 127 for the menu, 100 otherwise; the ambient keeps the war channels at the CC119-trigger mute, the stem plays them un-muted), MC1 from the GM score with the AWE32 preset tables embedded in CARPET.EXE (master 100). The runtime default when present (then GM, then FM). XMI ticks run on AIL's fixed 120 Hz clock (`FF 51` tempo metas ignored, BAKE_EPOCH 30). Songs without a muted danger layer (menu/intro) have no stem — except where the AWE32 score itself tags war channels (MC2's AWE32 SETUP/INTRO). Paired stems share one per-song normalization factor (peak of the ambient+stem SUM → −0.8 dBFS) so the runtime overlay cannot clip |
| `speech.json` (MC2) | `SpeechIndex`: `clips[]` of `{row, segment, file, ms, source}` — the CD voiceover pre-sliced at import by the compiled `CdTracks_DB080` segment table (docs/traces/mc2-voiceover-triggers.md; the runtime plays whole clips, never seeks). `row` = 0-based level number (table row r slices rip track r+2 — TrackIdx counts AUDIO tracks; row 27 = dead data); segment 0 = the map-screen intro line, N+1 = objective row N's line, 9 = the level-completion line; rows 25/26 = the secret-level one-liners |
| `speech/*.flac` (MC2) | one 44100 Hz stereo FLAC per clip (`level-RR-seg-S.flac`), cut by retail's truncating frames→ms law (`× 1000/75`) |

Sources: MC1 `DATA/SNDS<bank>-<q>.DAT/.TAB` (bank 0 = the 47-sound
gameplay bank, 1..13 auxiliary sets; `q` = the original's free-RAM
quality tier, always baked from `-1` = 22050 Hz) and
`DATA/MUSIC{0,1}-{0,2}.DAT/.TAB` — the `-0` AdLib HMP songs rendered
through OPL3 (nuked-opl3) with the game's own `INST.BNK`/`DRUM.BNK`
AdLib patches at import, 44100 Hz mono FLAC (`0-cgame1` …
`1-cintro6`), plus the `-2` General MIDI arrangement via oxisynth
when a soundfont is available (above). MC2
`SOUND/SOUND.DAT` (10 banks, best shipped tier = 8-bit 22050; the
per-sample WAV containers are stripped to keep `sounds.bin` raw PCM),
`SOUND/MUSIC.DAT` (the AIL XMI music bank: trailer u32 → 4-driver ×
2-bank directory; gameplay = G driver bank 0 ("C2"), six single-song
`FORM XDIR…CAT XMID` containers GAME1/2/3/SETUP/INTRO/CUTS, all six baked — INTRO and
CUTS as `mc2-intro`/`mc2-cuts`, the score for the movie bundle's
audio-less streams; MapType
picks GAMEn — Night/Day/Cave — and the menu plays SETUP; XMI → SMF
via the summed-run delta / embedded note-duration / strip-cc110-119
laws in docs/traces/mc2-music-dat-xmi.md, division 60 + tempo
pass-through, whole-song cc116/117 loop = loop the FLAC), and the 27
redbook audio tracks inside the CD image (`game.gog` + `game.ins`
cue) — the redbook is the per-level objective VOICEOVER (sliced into
`speech/`, above), never gameplay music.

## Impostor packs

Every world sprite (billboard) the renderer draws goes through an
**impostor pack** — a picture-based stand-in model: per visual family,
a table of pre-rendered VIEWS picked by where the camera stands, plus
the family's animation. Retail art is the most basic pack; richer
packs (more yaw views, elevation rings, art baked from meshes) are the
same model with bigger axes. Types and the lookup law live in
`mgc_formats::impostor`; the renderer installs a pack with
`Renderer::load_impostors`. The sim never sees a pack — sprite sizes
and draw types stay on the retail bank.

Every graphics variant bundle (`mc1-temperate`, `mc1-arctic`,
`mc2-day`, `mc2-night`, `mc2-night-fog`, `mc2-cave`) carries its pack
since bake epoch 27:

| member | contents |
|---|---|
| `impostors.json` | `ImpostorIndex` (below) |
| `impostors.bin` | the atlas texels, `PixelFormat::byte_len` bytes |

The importer bakes it from the variant's TMAPS bank
(`mgc_import::impostors::bake`): one model per family the game's
sprite table names (MC1/HW sprite-stats rows; MC2 sprite-param rows,
draw type = the bank's flags high byte), the bank's **asset
corrections** applied, retail's dispatch tabulated, then repacked into
the pack's own tight atlas (only texels some picture shows; a shared
rect stored once; shelves tallest first). `sprites.json` / `.bin` stay
the uncorrected retail bank — the SIM's view of the art (sizes, draw
types); the renderer reads only the pack.

Asset corrections are retail data that is plainly wrong art, fixed in
the picture only — never a sim input, never an option:

- **MC2 TMAPS 452** (the standing troglodyte's eighth view) ships as an
  uncropped 320x200 authoring canvas around a 53x72 creature; retail
  scales the whole canvas to the creature's height, so from that angle
  a standing troglodyte shrank to a third of its size and floated. The
  pack's picture is the canvas cut to its bounding box. (Shipped
  2026-09-30 as the patch option `mc2_troglodyte_sprite_crop`; ruled a
  correction and moved into the bake 2026-10-06.)

Instrument (mgc-app): `MGC_IMPOSTOR_PACK=<dir>` draws from the pack in
`<dir>` (`impostors.json` + `impostors.bin`) instead of the bundle's —
for putting an experimental pack side by side with retail. It is a
whole-pack replacement that must cover every family the level draws
(an indexed pack resolves through the level's palette, so make it from
the same bank); an unreadable or invalid pack falls back to the
bundle's with a warning. `ImpostorPack::repacked` re-atlases any pack.

`ImpostorIndex` (`version` = 1):

| field | meaning |
|---|---|
| `atlas_width`, `atlas_height` | atlas size, texels |
| `pixels` | `indexed8` — 1 byte/texel, a palette index resolved through the level's palette + glow set at load, 0 = transparent (retail art); `rgba8` — 4 bytes/texel sRGB RGBA (alpha 0 = transparent, any other alpha opaque) followed by a second plane of 1 MATERIAL byte/texel (bit 0 = emissive: resists darkness, not fog) |
| `images[]` | `{x, y, w, h, anchor}`: an atlas rect; `anchor` = where the entity's position (its feet) sits in the drawn quad, fractions from the left / bottom edge — retail `[0.5, 0.0]`, mirrored with a mirrored cell |
| `clips[]` | `{first, len, playback}`: a run of images. `still` = always the first; `flc_loop` = retail's FLC stream — image 0 the base, free-running it loops images `1..len` one per animation turn in lockstep and never returns to 0; an entity may seek an explicit image (the sim-positioned one-shots) |
| `models[]` | sorted by `key` (unique): `{key: {sprite_base, draw_type}, yaw_sectors, pitches[], poses, cells[]}` |

A model's `cells` (`{clip, mirror}`; `clip: null` draws nothing — a
missing or known-corrupt retail member) are indexed `(pose ·
len(pitches) + ring) · yaw_sectors + sector`:

- **pose** — the entity's animation byte (retail's frame offset for the
  2..=16 / 22..=36 animation draw types); past the last pose shows the
  last.
- **ring** — the `pitches` entry (degrees, 0 = level, positive = camera
  above) nearest the camera's elevation seen from the entity. Retail
  has one ring, `[0.0]`.
- **sector** — relative yaw (entity facing − camera yaw) on the
  engine's 2048-step circle, `floor(steps · yaw_sectors / 2048)`.
  Sector 0 starts AT relative yaw 0 (retail's convention), so a view
  baked for sector `s` is rendered at its middle, `(s + ½) · 360° /
  yaw_sectors`. 16 sectors reproduce the engine's `(rel >> 7) & 15`.

The drawn quad is the entity's world height tall (from the sim's
sprite table, `× h_stretch`) and the image's pixel aspect wide.

**Retail tabulation** (`impostor::retail`): view draw types 17-20 get
16 sectors — 17 = 8 views with the back half mirrored (`15 − s`), 18 =
16 views, 19 / 20 = the 5- / 3-picture folds (`byte_906E8` /
`byte_906F8`) mirrored from sector 8; the animation types get one
sector and `pose_count` poses (the type's own frame count); everything
else (0/1/21, unknown) one cell on the base. Each sprite becomes one
clip (its FLC stream if flagged animated, else its still), shared by
every family that draws it. Parity with the pre-impostor renderer is
asserted over every baked bundle by content
(`baked_impostors_reproduce_the_sprite_bank_dispatch`).
