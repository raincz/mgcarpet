# The map-screen mana roster ("sorcerer scores") — MC1 + MC2

Retail trace for the full-screen map's wizard scoreboard, ported
2026-07-24 (`ui::roster_quads`, `entities::roster_team_colors`, the
trigger block in `main.rs`). Both games draw ONE shared screen; only
the trigger and small dressing details differ.

## Triggers

- **MC1** (remc1 sub_main.cpp): full-map view = view-mode case 4
  (:26777). The roster draws every frame the cursor sits in the bottom
  strip — a bare `mouse.y >= 382` test (:26838-39), no hit rect. Drawer
  = `sub_22880` (:27009).
- **MC2** (remc2 engine/): HOLD-ALT while in the map scene.
  `ProcessPlayerInput` case 6/7 (PlayerInput.cpp:951) checks scancode
  0x38 (hardcoded 56 + the two remappable ALT slots `keys[6]`/`[9]`,
  EF:43126/43128) → `HandleButtonClick_191B0(20, 7)` → per-player
  `MenuState = SHOW_MAP_SORCERER_SCORES (7)`; ALT up reverts to state 6.
  The map render path dispatches state 7 to
  `DrawSorcererScores_2D1D0(scale)` (EF:21952-56, def EF:22207).
  A decompile reading had retail MC2 pausing the sim under any map
  menu state (`SetPausedMenuOpen_41AF0`, GameUI.cpp:651) — REFUTED by
  the player's retail recollection (2026-07-24): the MC2 map keeps
  NORMAL CONTROLS live — you can fly and even cast, HUD-less.
  Gameplay outranks; see the CONFLICTED note in
  mc2-walker-wander-ai.md. Retail MC1's map, by contrast, SUSPENDS
  flight input and turns the pointer into the ball+arrow cursor for
  book spell-selection. PORT (player-ruled 2026-07-24): the map
  suspends movement input iff the BOOK is on it (`tick_input`,
  `book_open && selector.map_book`) — MC2 keep-flies (retail), MC1
  default suspends (retail), and an MC1 run with the selector in the
  MC2 position EXCLUSIVELY adopts the keep-flying map wholesale. The
  bookless map keeps the pointer GRABBED and hidden (round-2 ruling:
  the freed pointer both showed the stock OS cursor and killed
  classic-model steering, whose virtual stick is grab-gated) —
  mouse-look, fire and quick-equip stay live under it; the roster
  comes up on ALT (hover needs a live pointer and is gated on
  !grabbed), the CTRL pane frees the cursor only while held.
- **PORT** (player ruling 2026-07-24): both triggers work in BOTH games
  (hover strip = below the map pane bottom, `BOOK_MAP_H` 416 / MC2 400;
  ALT = the existing `alt_held` latch). Deliberate unification —
  neither input means anything else on the map screen.

## The screen (shared)

Centered grid, one row per IN-PLAY wizard (slot in-play flag +6 == 1 —
cleared only on elimination: quit, castle-less death or the banish
opcode (MC1 :48585/:48825/:55622; MC2 EF:37614/:37663/:37777), never
on a temporary death, so a respawning wizard keeps the row). Tiles from
the UI sprite bank, same indices in BOTH games (already in our baked
`ui-sprites` atlases): **[85]** = 104×38 head tile, **[86]** = 36×38
matrix cell (MC2 names them SPELL_TILE / SPELL_TILE_MINI,
GameBitmapIndexes.h:21-22).

Per row (all offsets native px, ×scale):
- head tile at (x0, y); name text at (+8, +6); the BIG NUMBER at
  (+8, +20), format `%d`.
- 8 matrix cells from x0 + head_w, stepping cell_w; cell number at
  (+8, +10), format `%03d`.
- rows step by the tile height (38 in both banks).

Dressing (CORRECTED 2026-07-24 against retail screenshots — two
research-agent misreadings fixed):
- BOTH games FILL the tile interiors (inset 4, size − 8) with the
  wizard's box color, opaque raw palette memset: MC1 `sub_24C20` →
  `sub_61640/616C0` (:72757+, plain memset — the LUT-blend branch
  needs flag bit 4, which only TEXT drawing sets, as bit 6); MC2
  `DrawLine_2BC80` — a MISNAMED filled-rect blitter (Basic.cpp:1865,
  memset per row; args are x, y, WIDTH, HEIGHT despite the "end"
  names). The first reading ("MC2 draws a border") came from trusting
  the function name — the body is a fill.
- The SELF-cell is a black fill in both games, no number (MC1
  `byte_AD167[1]` ink :27109; MC2 clrd `[0]` EF:22333 — not a "white
  border": that reading misparsed the fill args).
- Absent/dead columns draw NOTHING; the player confirmed columns
  exist only for the rivals actually in the level. On elimination the
  PORT compacts the columns in both games (player-ruled deviation,
  see DEVIATIONS.md): MC2 retail compacts (blackBarX advances only
  inside the alive branch, EF:22318-56); MC1's decompiled loop
  advances unconditionally (:27100-24 — v9 += cellW outside the
  branch), which would leave a one-column hole at a dead wizard —
  unverified against retail, reads as a bug, MC1 follows MC2.
- Centering quirks (kept faithful): MC1 widths = living·cell_w +
  head_w (:27042-47); MC2 = living·cell_w + ONE CELL width (EF:22264)
  though the head tile is wider.
- 8 columns = the 8 wizard SLOTS (the human + 7 rivals) — there is no
  8th rival; column i == row i is the self-cell.

Port presentation notes:
- Solid UI tints draw RAW onto the sRGB swapchain (shader returns the
  tint; the surface encodes) — palette bytes fed straight through get
  double-gamma'd and wash out LIGHTER than retail. The roster colors
  decode sRGB→linear at `entities::roster_team_colors` so the display
  round-trips to the palette color. Any future palette-resolved SOLID
  UI color needs the same decode (atlas sprites don't — their
  textures are sRGB-typed).
- Draw order: on the map screen the projected stamps (castle flags,
  balloons, exit marks, guide path) draw UNDER the app UI so the
  roster/book overlay them — retail paints the roster after the map
  marks (sub_22880 after sub_48710; DrawSorcererScores after
  DrawMinimapMarks EF:21942/21952). In FLIGHT the stamps stay on top
  so minimap dots read over the radar frame art (mgc-render
  `render()`, the two-region ui buffer).

Colors: MC1 `byte_99B58[16]` pairs — even = box tint, odd = text
(:27087-88; our `entities::TEAM_COLORS`). MC2
`playersColors_E88E0x[slot][0]`=border, `[1]`=text (EF:22277-78;
`GetTrueWizardNumber` is identity single-player) — the map-env
day/night/cave tables our dot pass already carries.

Names: MC1 slot name field +14357 (defaults `off_99B68`: Zanzamar,
Vodor, Gryshnak, Mahmoud, Syed, Raschid, Alhabbal, Scheherazade;
human overridden by the entered player name). MC2
`WizardName_0x39f` (human = entered name, AI = `WizardsNames_D93A0`).
Port: campaign save name (MC1 slot label / MC2 `player_name`), else
the game's slot-0 table name.

## WHAT the big number is (the leak instrument)

The wizard's **census mana total** — MC1 entity +136 read raw
(:27091), MC2 `maxMana_0x8C_140` (EF:22288). Recomputed every tick
(MC1 `sub_48230/sub_48340` :56839/:56909; MC2 `sub_60F00` EF:61959):
reset to the intrinsic base, then `+= f140` of every owned mana-holding
entity (castle bank, castle pieces, balloon cargo, owned ground balls,
claimed houses). It is NOT the spendable pool (+140/0x90, which regens
toward this ceiling). Port: `player.mana_max` / `RivalView.mana_max`
(`World::recompute_mana`), which is the same census — so the roster
number is exactly the quantity a mana leak shrinks.

## The %03d matrix = the KILL TALLY

MC1's un-named `Type_160 +30` int16[8] and MC2's
`type_str_164.word_0x26_38[8]` are the same field: row-wizard's kills
of column-wizard (MC2 increments at the wizard-death handler EF:60113;
tooltip case 96 "Number of times you have killed %s"). Port:
`World::kill_tally` (already modeled at that exact offset —
`player_kill_row` cites "Type_160+30 on the human").

## Castle destroy/rebuild mana law (the audited leak — FIXED)

Retail MC1 conserves the castle bank through damage cycles by
SPILLING, never erasing: piece death and downgrade route stored f140
through `sub_27690` / the ejector `sub_47130` (:56160-56245: spill =
stored − cap when houses+stored exceed cap; a LEVEL-0 castle spills
ALL stored) into owned (10,39) balls the census re-sums. Downgrade
(`sub_47A70` :56498) = 10% cap haircut + eject + un-stamp + ladder
reset; upgrades never re-charge banked mana.

Two total-death (`!level` arm, :56531-37) facts:
1. GLOSSARY CORRECTED 2026-07-25: `sub_46D20(a1, 0)` is NOT a balloon
   release — it clears entity `+48` of the slot in wizext +708 =
   `var_676.var_u16[16]` = the owner's SPELL-16 (Create Castle)
   MANIFESTATION, i.e. the charge pin (the archived ROADMAP's
   playtest-5 glossary correction; this trace briefly regressed it).
   Retail's arm therefore never touches the fleet: the balloons keep
   flying at the freed castle slot's stale coordinates with cargo
   intact, forever unless a rebuilt castle's dispatcher re-adopts
   them. **The port used to despawn balloons here (0x400) with no
   spill, erasing in-flight cargo from the census — the
   destroy/rebuild mana leak, fixed 2026-07-24 (release-to-idle),
   then PLAYER-RULED 2026-07-25: the castle-less quota is zero, so
   total death now DEMOLISHES the fleet through the cull's spill
   (`corpse_drop`/sub_27690 — loaded balloons leave an owned ball,
   empty ones vanish), conserving the census.** Deviation registered
   in docs/DEVIATIONS.md; pinned by
   `castle_total_death_demolishes_balloons_spilling_cargo`.
2. Retail frees the castle WITHOUT a final eject: the residual bank
   (≤ ~90% of the level-1 cap) vanishes — a shipped-engine leak. The
   port deliberately scatters it through the ejector's level-0 rule
   instead (docs/DEVIATIONS.md entry: castle_downgrade total-death
   residual bank).

Balloon fate at castle shrink — the full retail law (MC1):
- **Over-quota cull** (dispatcher tail `sub_47400` :56399-411): while
  the castle LIVES, any balloon beyond the fleet quota (downgrades,
  incl. the level-0 bare flag's quota 0; also a rebuild at a lower
  level re-adopting orphans — the slot register lives on the WIZARD's
  Type_160 +52, surviving castle death) is freed, cargo first spilled
  via `sub_27690` — which spawns a ball only `if mana > 0`, so an
  EMPTY culled balloon vanishes without a trace while a loaded one
  leaves a ball. (This is why "only loaded balloons persist" is the
  gameplay impression; the release itself has no cargo condition.)
  Ported 2026-07-24; pinned by `balloon_cull_over_quota_spills_the_cargo`.
- **Total death**: retail runs no dispatcher pass afterwards, so ALL
  balloons orphan alive — loaded or empty, unconditionally. The port
  deviates (player-ruled 2026-07-25): `castle_downgrade`'s !level arm
  demolishes the fleet with the same spill as the cull.
- **MC2 differs by design**: MC2 castles never touch this column
  (game-keyed dispatch, world.rs:2133-40); `mc2_castle_destroy`
  converts balloons to mana spheres (`TransformEntityToManaSphere`,
  over-quota members too) — dead balloons, conserved mana.

Known faithful leak that remains: eject/spill ball spawn failure on a
full entity pool loses that share (retail `sub_373F0` null return has
no fallback; our eject `break`s the same way).
