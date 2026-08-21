# MC2 SPELL-SELECTOR UI — Verbatim Trace (remc2 / Magic Carpet 2)

All citations are `file:line` relative to `/home/rain/projects/mgcarpet/reference/remc2/remc2/engine/`.
Files: `PlayerInput.cpp` (PI), `EventsFunctions.cpp` (EF), `GameUI.cpp` (UI), `Level.cpp` (L),
`ViewPort.cpp` (VP), `GameBitmapIndexes.h` (GBI), `MenuState.h`, `global_types.h`, `Spells.h`.
Cross-reference: `docs/traces/mc2-class15-spell-tokens.md` for the 26-spell enum, the
`SpellEnabled[]` grant model, `SetSpell_6D5E0`, and `GetSpellManaCost_6D710`.

This documents the docked bottom **spell selector** shown while CTRL is held (the "spellbook
replacement"). It is a live-input, per-frame-redrawn overlay — NOT a modal book. The map screen
reuses it identically.

---

## 0. TL;DR field map (the persistent state that survives the selector)

Everything lives in the per-player `type_str_611` (`global_types.h:174-216`), reached via
`playerEntity->dword_0xA4_164x->str_611` (wizard entity → its `Type_str_164` → `str_611`):

| field | offset | type | meaning |
|---|---|---|---|
| `SpellsEnabled_0x333_819x.SpellEnabled[26]` | 0x333 | `int16[26]` | per-spell: entity index of the live class-15 spell object (0 = not possessed). This is the possession check. |
| `array_0x3E9_1001x.SpellIndex[26]` | 0x3E9 | `uint8[26]` | per-spell "spell present / learned" flag (map-start grant + save). |
| `array_0x403_1027x.SpellIndex[26]` | 0x403 | `uint8[26]` | per-spell "collectible/jar present" flag (set on token pickup, see tokens trace §3). |
| **`array_0x437_1079x.SpellIndex[26]`** | 0x437 | `uint8[26]` | **THE persistent per-spell selected level** (0/1/2). Written by the submenu; read by every cast route. |
| `SpellLevels_0x41D_1053z.SpellIndex[26]` | 0x41D | `uint8[26]` | per-spell **max unlocked level** (0..2), from map settings / experience. Caps the submenu. |
| `array_0x3B5_949x.SpellIndex[26]` | 0x3B5 | `uint8[26]` | per-spell mouse-button binding: 0=none, 1=LEFT button, 2=RIGHT button. |
| `SpellExperience_0x263_611x.SpellExperience[26]` | 0x263 | `int32[26]` | per-spell XP (drives the unlock-progress bar). |
| `spellsExperience_0x2CB_715x[26]` | 0x2CB | `int32[26]` | per-spell XP delta accumulator. |
| `SpellIndexes_0x39B_923x.SpellIndex[10]` | 0x39B | `int8[10]` | 10-slot "learned order" list (used for number/scroll cycling & AI), NOT a per-key hotbar. |
| `SpellIndexLeft_0x451_1105` / `SpellIndexRight_0x453_1107` | 0x451/0x453 | `int16` | spell index currently bound to LMB / RMB (−1 = none). |
| `SubSpellIndexLeft_1109` / `SubSpellIndexRight_1110` | — | `int8` | the level for the left/right bound spell (mirror of `array_0x437[spell]`). |
| `byte_0x457_1111` | 0x457 | `int8` | selector sub-state: 0 = hovering the 26-grid, 1 = submenu open committing to LEFT, 2 = committing to RIGHT. |
| `spellIndex_0x458_1112` | 0x458 | `int8` | the **grid index** (0..25) the cursor is currently over. |
| `subSpellIndex_0x459_1113` | 0x459 | `int8` | the level (0..2) the cursor is currently over in the open submenu. |

The live spell entity (class 15) additionally stores `byte_0x46_70` = its active level tier (see
tokens trace §2 `SetSpell_6D5E0`); the top-HUD icon shows this as a Roman numeral.

Grid index → spell index remap (`UI:59`):
```c
char spellIndex_D94FF[29] = { 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25, 0,3,0 };
```
For grid slots 0..25 it is the identity (grid slot N = `spell_t` N). Slots 26/27/28 are aliases
(fireball, speed_up, fireball) used by other callers, never by the 2×13 grid. Spell names =
`spell_t` enum (`global_types.h:135-162`): 0 fireball … 25 cave_in.

---

## 1. Activation & input

### 1.1 The key is CTRL (scancode 0x1D), hold-to-open

Default keybind table (`EF:43108` `sub_5BCC0_set_any_variables1`):
```c
x_BYTE_EB39E_keys[5] = 0x1D;//2bc3A3//CTRL     (EF:43118)
// others: [0]=0x48 UP, [1]=0x50 DOWN, [2]=0x4B LEFT, [3]=0x4D RIGHT,
//         [4]=0x1C ENTER, [6]/[9]=0x38 ALT, [7]/[8]=0x36 RSHIFT
```
Every frame the input snapshot sets a virtual "button" bit 0x10 iff CTRL is held (`EF:49684`,
`EF:49723`, `EF:49770`, `EF:50443`, all identical):
```c
if (pressedKeys_180664[x_BYTE_EB39E_keys[5]])//CTRL
    unk_18058Cstr.MouseButtonState_18059C |= 0x10;
```
`MouseButtonState_18059C` bits: 0x01=LMB, 0x02=RMB, 0x04=LMB2, 0x08=RMB2, **0x10=CTRL**.

In normal-flight input (menu states 0/4), holding CTRL opens the selector (`PI:505`):
```c
if (unk_18058Cstr.MouseButtonState_18059C & 0x10)   // CTRL held
{
    if (v8x->life_0x8 >= 0)
        HandleButtonClick_191B0(20, 5);             // request MenuState -> 5 (SHOW_BOTTOM_MENU)
}
```
`HandleButtonClick_191B0(20, n)` queues a network command "change menu state to n" (command 20 =
default case, `PI:1144`). It is applied by the command processor which flips `MenuState_0x3DF`.
While in the selector state, `MouseButtonState & 0x10` (CTRL) is checked again each frame; the
selector **closes** when CTRL is released — the closing branch (`PI:895`) sets `v33=1` when none
of bits 0x10/0x04/0x08 is set:
```c
LABEL_122:
    if (!(unk_18058Cstr.MouseButtonState_18059C & 0x10)   // CTRL up
        && !(unk_18058Cstr.MouseButtonState_18059C & 4)
        && !(unk_18058Cstr.MouseButtonState_18059C & 8))
        v33 = 1;
    if (v33) {
        if (MenuState == 5) HandleButtonClick_191B0(20, 0); // back to flight
        else                HandleButtonClick_191B0(20, 6); // back to map
    }
```
So it is **hold-to-open, release-to-close** (CTRL is a momentary hold, not a toggle). The two exit
targets are state 0 (flight, from `SHOW_BOTTOM_MENU=5`) and state 6 (map, from
`SHOW_MAP_BOTTOM_MENU=8`).

`MenuState` enum (`MenuState.h`): `NONE=0`, `SHOW_CHAT_MENU=3`, **`SHOW_BOTTOM_MENU=5`**,
`SHOW_MAP_SORCERER_SCORES=7`, **`SHOW_MAP_BOTTOM_MENU=8`**, options=9..14. (state 4 and 6 are the
non-enum "flight" and "map" idle states.)

### 1.2 Mouse pointer hijack & flight-look suspension

On entering the bottom-menu state, `MoveCursorToSelectedSpell_6D200` warps the OS/virtual mouse to
the currently-selected spell's box (see §4.2), and the cursor sprite is made visible. From the
menu-state transition (`UI:657`):
```c
if (newMenuState && (newMenuState < 6u || newMenuState > 7u)) {
    if (joystick==7|1|2) SetCursor_8CD27(pointers[CURSOR_SPRITE_INDEX_D419E]); // show pointer
}
```
(For the software-cursor path the pointer sprite is blitted at mouse-xy directly by the draw
routine, `EF:22686` / `UI:4631`.)

Flight/mouse-look is suspended structurally: while `MenuState==5` (or 8), input dispatch takes the
`case 5:`/`case 8:` branch of `MouseAndKeysEvents_17A00` (`PI:806`) instead of the flight branch,
so mouse motion drives the selector cursor, not the carpet. The fly-assistant is also disabled in
these states (`sub_1A7A0_fly_asistant`, `PI:1990`: skips when `MenuState==5||8||3`). On close,
`SetCenterScreenForFlyAssistant_6EDB0` re-centres look (`GameUI.cpp:723`, transition-close path).

### 1.3 The per-frame input handler (states 5 & 8) — verbatim

`MouseAndKeysEvents_17A00`, `case 5: case 8:` (`PI:806-929`). Key structure:
```c
case 5:
case 8:
    v12x = Entities_EA3E4[...playerIndex...];
    ProcessKeyboardPresses_17190();
    if (v12x->life_0x8 < 0) { v33 = 1; }              // dead -> force close
    else {
        v16 = v12x->...str_611.byte_0x457_1111;       // sub-state
        if (v16) {                                    // 1 or 2 = submenu open (committing L/R)
            if (v16 <= 2) {
                if ((v16!=1 || MouseBtn&4) && (byte_0x457!=2 || MouseBtn&8)) {
                    // still holding the extra button -> LIVE-track the hovered level:
                    v23 = SelectSpell_6D4F0(&v12x->...str_611, mouse.x);   // level under cursor
                    v12x->...str_611.subSpellIndex_0x459_1113 = v23;
                    HandleButtonClick_191B0(41, v23);                     // (cmd 0x29) set level
                } else {
                    // button released -> COMMIT the level to the L or R mouse binding:
                    if (byte_0x457_1111 == 1)
                        PlayerAction = 31;   // 0x1F = bind LEFT
                    else
                        PlayerAction = 32;   // 0x20 = bind RIGHT
                    str_0x6E3E_byte1 = spellIndex_0x458_1112;   // which spell
                    str_0x6E3E_byte2 = subSpellIndex_0x459_1113;// which level
                    byte_0x457_1111 = 0;                         // back to grid hover
                    MoveCursorToSelectedSpell_6D200(...);
                }
            }
        }
        else {                                        // sub-state 0 = hovering the 26-grid
            v34 = 1;
            v17 = SelectSpellCategory_6D420(mouse.x, mouse.y);  // grid slot under cursor
            v12x->...str_611.spellIndex_0x458_1112 = v17;
            v18 = spellIndex_D94FF[v17];
            // availability: must possess it (SpellEnabled != 0), and cave_in(25) only on cave level
            v19 = 1;
            if (!SpellEnabled[v18] || (!isCaveLevel_D41B6 && v18==25)) v19 = 0;
            if (!v19) goto LABEL_122;
            sub_6D4C0(&...str_611);   // subSpellIndex_0x459_1113 = array_0x437[spell] (show stored lvl)
            if (!(MouseBtn&1) && !(MouseBtn&2)) goto LABEL_122;  // no click -> just hover
            if (pressedKeys[42] || pressedKeys[EB39E[7]]) {      // SHIFT+click -> quick set both/none
                HandleButtonClick_191B0(38, v18);  // cmd 0x26: set array_0x3B5 (mouse binding) directly
                ... str_0x6E3E_byte2 encodes LEFT(1)/RIGHT(2)/both from LMB/RMB ...
            }
            if (MouseBtn&1 && MouseBtn&2) {          // LMB+RMB together -> fire/steer this spell
                HandleButtonClick_191B0(6, 64);
                str_0x6E3E_byte1 = spellIndex_0x458_1112;
            } else {
                // single click -> OPEN the level submenu, committing to L (LMB) or R (RMB):
                byte_0x457_1111 = ((MouseBtn&1)==0) + 1;   // LMB->1, RMB->2
                MoveCursorToSelectedSpell_6D200(...);
                HandleButtonClick_191B0(40, spellIndex_0x458_1112);  // cmd 0x28: select spell
                str_0x6E3E_byte2 = byte_0x457_1111;
            }
        }
    }
LABEL_122:
    ... close check (§1.1) ...
    ComputeMousePlayerMovement_17060(...);   // (menu variant)
    LastPressedKey_1806E4 = 0;
    MouseButtonState_18059C &= 0xFC;
```
Summary of the click model:
- **Hover** a grid box → `spellIndex_0x458_1112` tracks it; `sub_6D4C0` (`PI:2273`) shows that
  spell's stored level as the highlighted submenu level.
- **LMB click** on a box → opens the level submenu in "bind-to-LEFT" mode (`byte_0x457=1`).
- **RMB click** → same, "bind-to-RIGHT" (`byte_0x457=2`).
- In the submenu, **while the button stays held** the level under the cursor is live-tracked
  (`SelectSpell_6D4F0`); **on release** it commits (cmd 31/32) → writes `array_0x437[spell]=level`
  and binds that spell to LMB/RMB (see §4.3, §5).
- **LMB+RMB together** on a box = fire/steer (command 6, sub-op 64).
- **SHIFT+click** = fast-bind without the submenu (command 0x26).

`sub_6D4C0` (`PI:2273`) — "load stored level for hovered spell":
```c
char sub_6D4C0(type_str_611* a1x) {  //24e4c0
    a1x->subSpellIndex_0x459_1113 = a1x->array_0x437_1079x.SpellIndex[spellIndex_D94FF[a1x->spellIndex_0x458_1112]];
    return a1x->subSpellIndex_0x459_1113;
}
```

---

## 2. Pane layout — verbatim geometry

Draw entry: `DrawBottomSpellsMenu_2ECC0` (`EF:22401`), called from the HUD compositor for
`MenuState==SHOW_BOTTOM_MENU` (`EF:21788`) and `SHOW_MAP_BOTTOM_MENU` (`EF:21959`).

### 2.1 Anchor & scale (`EF:22432-22453`)
```c
x_D41A0_BYTEARRAY_4_struct.spellOnCursor_50 = -1;
int16_t posX = 0, posY = 0; uint8_t scale = 1;
if (x_WORD_180660_VGA_type_resolution & 1) posY = 400;   // 320x200-derived (half-res)
else                                        posY = 480;   // 640x480 baseline
if (res != 1 && !DefaultResolutions()) {                  // arbitrary window
    scale = gameUiScale;
    posX = (screenWidth_18062C - 640*scale) / 2;          // centre the 640-wide pane
    posY = screenHeight_180624;                           // pane bottom = screen bottom
}
posIconsXStart = EDGE_PANEL.width * scale;                // left frame width
spellIconHeight = SPELL_ICON_PANEL.height * scale;
posIconsY2 = posY - 2*spellIconHeight;                    // TOP row Y  (two rows tall)
spellIconWidth = SPELL_ICON_PANEL.width * scale;
posIconsY  = posY - 2*spellIconHeight;                    // running Y (starts at top row)
```
So the pane is **bottom-anchored**, 640 logical px wide (centred + scaled on wide screens), and
**two `SPELL_ICON_PANEL` rows tall**, flush to the bottom edge (`posY`). Its top edge is
`posY − 2·iconHeight`. Left and right ends carry an `EDGE_PANEL` frame column (drawn only on the
first row iteration, `EF:22461` left, `EF:22571` right).

### 2.2 The 2×13 grid (`EF:22456-22575`)
```c
iconYIdx = 0;
while (iconYIdx < 2) {                    // 2 ROWS
    if (!iconYIdx) DrawBitmap(posX, posIconsY, EDGE_PANEL);   // left frame (row 0)
    posIconsX = posIconsXStart;
    iconXIdx = 0;
    while (iconXIdx < 13) {               // 13 COLUMNS
        spellIndex2 = spellIndex_D94FF[spellIconIndex];       // spellIconIndex = row*13 + col
        ... draw box + icon at (posX + posIconsX, posIconsY) ...
        iconXIdx++;
        posIconsX  += spellIconWidth;     // step one box right
        spellIconIndex++;
    }
    if (!iconYIdx) DrawBitmap(posX + posIconsX, posIconsY, EDGE_PANEL);  // right frame
    posIconsY += SPELL_ICON_PANEL.height * scale;  // step down one row
    iconYIdx++;
}
```
So: **row 0 = grid indices 0..12** (spells fireball..beyond_sight), **row 1 = indices 13..25**
(steal_mana..cave_in). Box origin for slot `(col,row)`:
```
x = posX + EDGE_PANEL.width  + col * SPELL_ICON_PANEL.width      (all * scale)
y = (posY - 2*iconHeight) + row * SPELL_ICON_PANEL.height
```

### 2.3 Selection frame (`EF:22577`)
```c
DrawBitmap(posX + posIconsXStart + spellIconWidth*(spellIndex_0x458_1112 % 13),
           (spellIndex_0x458_1112 / 13)*spellIconHeight + posIconsY2,
           SPELL_ICON_FRAME);
```
A highlight frame (`SPELL_ICON_FRAME=90`) is drawn over the box `spellIndex_0x458_1112` (the hovered
slot): column `= idx%13`, row `= idx/13`.

### 2.4 Per-box drawing states (`EF:22468-22562`)
For each box, `spellIndex2 = spellIndex_D94FF[slot]`:
- **Not usable** (`SPELLS_BEGIN_BUFFER_str[spell].byte_0 == 0`, i.e. spell has no subspell tiers,
  OR `spell==25 cave_in && !isCaveLevel_D41B6`) → draw empty `SPELL_ICON_PANEL` only (`EF:22470-22475`).
- **Possessed** (`SpellEnabled[spell] > 0`, resolves to a live spell entity):
  - Choose the icon's stored level: if this is the hovered slot, use `subSpellIndex_0x459_1113`
    and set `spellOnCursor_50 = slot`; else use `array_0x437[spell]` (`EF:22482-22490`).
  - **Affordability** `canSummon` (`EF:22503-22508`): true if the level's `maxManaLimit_A == 0`
    (free) OR the player HAS a castle (`CastleEntityIndex_0x3A_58 != 0`) whose castle mana
    `>= maxManaLimit_A`.
  - Box background: `canSummon ? SPELL_ICON_PANEL(89) : SPELL_ICON_PANEL2(91)` (dark/greyed)
    (`EF:22535-22539`).
  - Spell icon: `spell + SPELL_FIREBALL_SMALL(97)`; **opaque** (`DrawBitmap_2BB40`) if affordable,
    **transparent/ghosted** (`DrawTransparentBitmap_2DE80`) if not (`EF:22541-22544`). → **icon
    index formula: `HSPR index = spell_index + 97`** (26 small spell icons, 97..122).
  - If affordable AND hovered, instead of the plain box it draws a live **per-shot mana meter**
    (`SPELL_TILE_BAR(87)` + `DrawLine` fill), showing `mana % cost` remainder and `mana / cost`
    charges (`EF:22511-22530`).
  - Left/right mouse-binding tags: `array_0x3B5_949x.SpellIndex[spell]` 1→draw `SPELL_TOP_LEFT_CORNER(149)`
    on the box, 2→`SPELL_TOP_RIGHT_CORNER(150)` at `xAdd2` from the right (`EF:22546-22553`).
- **Not possessed but learnable/present** (`array_0x3E9_1001x[spell] || array_0x403_1027x[spell]`)
  → draw empty box + the spell icon **colourised grey** (`DrawColourizedBitmap(..., 0xA6)`,
  `EF:22558-22561`) as a "not yet collected" hint.

### 2.5 The top-left "I / II / III" boxes the player saw

Those are NOT part of the CTRL grid and NOT keybinding slots. They are the **top HUD active-spell
tiles** for the LEFT and RIGHT mouse spells, drawn by `DrawSpellIcon_2E260` at the top of the
screen every frame (`EF:21750` left, `EF:21758` right), independent of the selector.
`DrawSpellIcon_2E260` (`UI:341`) blits the big spell icon (`model + SPELL_FIREBALL_BIG(123)`,
`UI:374`) on a `SPELL_TOPTILE_BAR(3)`/`_GLOW(2)` tile and overprints the level as a **Roman
numeral** (`UI:375`):
```c
char* SpellLevelText_DB06C[5] = { "I","II","III","IV","V" };   // UI:19
DrawText_2BC10(SpellLevelText_DB06C[playerEvent->byte_0x46_70], ...);  // UI:375
```
`byte_0x46_70` = the live spell entity's active level tier (0→"I", 1→"II", 2→"III"). So the "box
labeled I" = the currently-selected LEFT (or RIGHT) mouse spell at level 1. It also draws the
spell's mana pool + regen shimmer (`UI:380-403`).

### 2.6 Sprite/HSPR resource summary (`GBI`, all from `MSPRD00.DAT` tab
`filearray_2aa18c[filearrayindex_MSPRD00DATTAB].posistruct[index]`)

| purpose | const | index |
|---|---|---|
| left/right pane frame column | `EDGE_PANEL` | 88 |
| grid box (normal / affordable) | `SPELL_ICON_PANEL` | 89 |
| grid box (dark / unaffordable) | `SPELL_ICON_PANEL2` | 91 |
| grid selection highlight frame | `SPELL_ICON_FRAME` | 90 |
| per-shot mana meter tile | `SPELL_TILE_BAR` | 87 |
| **small spell icon (grid)** | `SPELL_FIREBALL_SMALL + spell` | **97 + spell (97..122)** |
| big spell icon (top HUD) | `SPELL_FIREBALL_BIG + model` | 123 + spell |
| left-bound corner tag | `SPELL_TOP_LEFT_CORNER` | 149 |
| right-bound corner tag | `SPELL_TOP_RIGHT_CORNER` | 150 |
| submenu box (empty / locked) | `SPELL_ICON2_PANEL2` | 163 |
| submenu box (unlocked, affordable) | `SPELL_ICON2_PANEL2_WITH_FRAME` | 161 |
| submenu box (unlocked, unaffordable) | `SPELL_ICON2_PANEL2_WITH_FRAME_DARK` | 162 |
| submenu selected-level gold frame | `SPELL_GOLD_FRAME` | 164 |
| submenu level-number bg (0/1/2) | `SPELL_BACKGROUND_NUMBER1 + lvl` | 165 + lvl (165..167) |
| **submenu level icon** | `SPELL_SUB_FIREBALL1_SMALL + 3*spell + lvl` | **179 + 3·spell + lvl** |
| top HUD spell tile / glow | `SPELL_TOPTILE_BAR / _GLOW` | 3 / 2 |
| software cursor pointer | `CURSOR_SPRITE_INDEX_D419E` (in `POINTERSDATTAB`) | var |

---

## 3. Spell list & ordering (the 26 boxes)

Grid slot → spell = `spellIndex_D94FF[slot]` = identity for 0..25. So the two rows are exactly the
`spell_t` enum order:

**Row 0 (slots 0–12):** 0 fireball, 1 possession, 2 castle, 3 speed_up, 4 metamorph, 5 heal,
6 shield, 7 lightning, 8 rebound, 9 meteor, 10 teleport, 11 invisible, 12 beyond_sight.
**Row 1 (slots 13–25):** 13 steal_mana, 14 duel, 15 tremor, 16 crater, 17 earthquake, 18 volcano,
19 summon_army, 20 gravity_well, 21 whirlwind, 22 fools_mana, 23 magic_mine, 24 alliance,
25 cave_in.

(Note: the `spell_t` names are the model-indexed enum, matching the tokens trace §5 role table.
The comment block in `Spells.h:28-54` is a DIFFERENT Bullfrog subtype numbering — do NOT use it for
grid order.)

**Availability / possession checks** (all per-spell, index = spell 0..25):
- **Possessed & castable:** `SpellsEnabled_0x333_819x.SpellEnabled[spell] > 0` — nonzero = entity
  index of the live class-15 spell object (set on token pickup or possession creation; see tokens
  trace §3 grant + §6 cast path). The grid draws a real icon only when this is set (`EF:22478`).
- **Learned / present but not live:** `array_0x3E9_1001x[spell]` (learned at map start / save) or
  `array_0x403_1027x[spell]` (a spell-jar/token pickup flag) → drawn as a grey ghost (`EF:22558`).
  `sub_55AB0` (`L:1304`) turns a "present" spell into a live `SpellEnabled` entity next frame.
- **Grid-usable at all:** `SPELLS_BEGIN_BUFFER_str[spell].byte_0 != 0` (spell defines ≥1 tier) and,
  for cave_in(25), only when `isCaveLevel_D41B6` (`EF:22470`, `PI:849`).

Relation to class-15 spell tokens / jars: the tokens trace documents that collecting a token sets
`SpellEnabled[model] = tokenIndex` and `array_0x403_1027x[model] = 1` (tokens trace §3, `EF:55726-55727`).
Those are exactly the arrays the grid reads here — so **a picked-up spell jar lights up its grid box
and makes it selectable**; class-15 SPELL JARS (tokens trace, mc2::tokens) feed this UI directly.

---

## 4. Level sub-menu (the 3-level flyout)

Opens when the cursor is over a possessed box (drawn every frame if `spellOnCursor_50 == slot`, or
if the spell is mid-cast `skipToLabel43`). Layout drawn at `EF:22578-22676`.

### 4.1 Geometry (`EF:22582-22597`)
```c
spellIndex = spellIndex_D94FF[selectedSpellIndex];                 // hovered spell
spellIndex3 = SpellLevels_0x41D_1053z.SpellIndex[spellIndex];      // MAX unlocked level (0..2)
xAdd  = 3 * SPELL_ICON2_PANEL2.width * scale;                       // total flyout width = 3 boxes
posY2 = posIconsY2 - SPELL_ICON2_PANEL2.height * scale;             // sits ABOVE the grid top row
posSubMenuSpellX = spellIconWidth/2                                 // centre over the hovered box…
                 + spellIconWidth*(spellIndex_0x458_1112 % 13)
                 + posIconsXStart - xAdd/2;                          // …minus half the flyout width
if (posSubMenuSpellX > 640*scale - xAdd) posSubMenuSpellX = 640*scale - xAdd;  // clamp to right edge
else if (posSubMenuSpellX < 0) posSubMenuSpellX = 0;                            // clamp to left edge
posSubMenuIconWidth = SPELL_ICON2_PANEL2.width * scale;
```
So the flyout is a **horizontal row of 3 boxes, directly above the hovered grid box**, centred and
edge-clamped. Each level box steps right by `posSubMenuIconWidth`.

### 4.2 The 3 level entries (`EF:22598-22631`) — ALWAYS 3 boxes, locked ones shown empty
```c
for (subSpellIndex2 = 0; subSpellIndex2 < 3; subSpellIndex2++) {
    canSubSummon = (maxManaLimit_A==0) || (haveCastle && maxManaLimit_A <= castleMana);
    manaPart = canSubSummon ? mana / GetSpellManaCost_6D710(player, spell, subSpellIndex2) : 0;

    if (subSpellIndex2 > spellIndex3) {          // LOCKED level (beyond max unlocked)
        DrawBitmap(posSubMenuSpellX, posY2, SPELL_ICON2_PANEL2);      // empty box, no icon
    } else {                                     // UNLOCKED level
        bitmap = (canSubSummon && manaPart) ? SPELL_ICON2_PANEL2_WITH_FRAME
                                            : SPELL_ICON2_PANEL2_WITH_FRAME_DARK;
        DrawBitmap(posSubMenuSpellX, posY2, bitmap);
        DrawBitmap(posSubMenuSpellX+6, posY2+10, SPELL_BACKGROUND_NUMBER1 + subSpellIndex2); // "1/2/3"
        int lvlIcon = subSpellIndex2 + 3*spellIndex + SPELL_SUB_FIREBALL1_SMALL;              // per-level icon
        if (canSubSummon) DrawBitmap(..+18, ..+6, lvlIcon);            // opaque
        else              DrawTransparentBitmap(..+18, ..+6, lvlIcon); // ghosted (unaffordable)
    }
    if (subSpellIndex2 == subSpellIndex_0x459_1113)                    // the currently-chosen level
        DrawBitmap(posSubMenuSpellX, posY2, SPELL_GOLD_FRAME);         // gold highlight
    ... XP progress bar for the next tier (EF:22633-22671) ...
    posSubMenuSpellX += posSubMenuIconWidth;
}
```
- **Always shows all 3 boxes**; levels above `SpellLevels_0x41D_1053z` (max unlocked) are drawn as
  empty `SPELL_ICON2_PANEL2` with no icon (i.e. **locked levels ARE shown, blank**).
- **Per-level icon** = `179 + 3·spell + level` (`SPELL_SUB_FIREBALL1_SMALL=179`); each spell owns 3
  consecutive sub-icons.
- The chosen level (`subSpellIndex_0x459_1113`) gets the `SPELL_GOLD_FRAME(164)`.
- Unaffordable unlocked levels are ghosted; a per-tier **XP bar** shows progress toward unlocking
  the next level, using `subspell[lvl].xpos1_E/xpos2_0x12` bounds vs `SpellExperience[spell]`
  (`EF:22633-22671`; multiplayer uses `xpos2`, single-player `xpos1`).

### 4.3 WHERE the selected level is stored, and how casting consumes it

**Hover live-tracking** while a submenu button is held: `SelectSpell_6D4F0(str_611, mouse.x)`
(`PI:2175`) computes the level (0..2) under the cursor from the flyout x-geometry, and **clamps to
the max unlocked level** (`PI:2214-2217`):
```c
subCategoryIdx = ((mouseX - posXOffSet) - spellMenuXPos16) / (SPELL_ICON2_PANEL2.width * scale);
maxIdx = a1x->SpellLevels_0x41D_1053z.SpellIndex[spellIndex_D94FF[a1x->spellIndex_0x458_1112]];
if (subCategoryIdx > maxIdx) return maxIdx;    // cannot hover a locked level
if (subCategoryIdx < 0)      subCategoryIdx = 0;
return subCategoryIdx;
```
The caller writes it to `subSpellIndex_0x459_1113` and issues command 0x29 (`PI:825-827`).

**Commit (button release)** issues command 31 (bind LEFT) or 32 (bind RIGHT) with
`byte1=spell, byte2=level`. The command processor (`EF:37898`, `case 0x1F/0x20 "Change Spell"`):
```c
case 0x1F:  // 31 = set LEFT
case 0x20:  // 32 = set RIGHT
    spellIndex = spellIndex_D94FF[byte1];
    // *** PERSIST the chosen level for this spell: ***
    actEvent->...str_611.array_0x437_1079x.SpellIndex[spellIndex] = byte2;      // EF:37905
    if (PlayerAction == 0x1F) {                                                 // LEFT
        str_611.SpellIndexLeft_0x451_1105  = spellIndex;                        // EF:37910
        str_611.SubSpellIndexLeft_1109     = byte2;
        x_D41A0_...leftSpellPlayerIndex_38400 = 8;                              // flash timer
    } else if (PlayerAction == 32) {                                            // RIGHT
        str_611.SpellIndexRight_0x453_1107 = spellIndex;                        // EF:37916
        str_611.SubSpellIndexRight_1110    = byte2;
        x_D41A0_...rightSpellPlayerIndex_38401 = 8;
    }
    // push the level into the LIVE spell entity so the next cast uses it:
    SetSpell_6D5E0(Entities_EA3E4[SpellEnabled[spellIndex]], byte2);            // EF:37921
    CopyAxisForSpellWithLife_6D830(Entities_EA3E4[SpellEnabled[spellIndex]], byte2);
    // hint text for the chosen tier:
    strcpy(notification, langindex[SPELLS_BEGIN_BUFFER_str[spellIndex].subspell[byte2].hintText_0x16x]);
```
So **the persistent per-spell selected level lives in `array_0x437_1079x.SpellIndex[spell]`** (one
byte per spell, 26 bytes at str_611 offset 0x437). `SetSpell_6D5E0` then copies that level into the
live class-15 entity: it sets `entity->byte_0x46_70 = level` and derives
`subSpellIndex_0x2A_42 = SPELLS_BEGIN_BUFFER_str[model].subspell[level].subSpellIndex_2`, plus mana
cost/regen (tokens trace §2, `L:1505`). **Casting reads the level via `subspell[level]` — i.e. the
`spell·3 + level` sub-icon/behaviour tuple** — because `SPELLS_BEGIN_BUFFER_str[spell].subspell[3]`
is a 3-element array indexed by the stored level (`Spells.h:20-25`; the sub-icon math `3·spell+lvl`
at `EF:22624` mirrors this).

**Persistence & save format:** `array_0x437` is part of `type_str_611`, saved with the player. The
`SpellLevels_0x41D_1053z` (max unlocked) and `SpellExperience_0x263` are serialised together with
`SpellsEnabled`/`SpellIndexes` in the recorder/save path (`EF:38680-38683`, `EF:38769-38774`) —
confirming these four per-spell arrays are the persisted spell state. (`array_0x437` itself is
re-derived/clamped by `SetSpell` each frame but is authoritative between casts.)

**Quickselect reuse of the stored level — CONFIRMED.** The mouse-cycle path `sub_18DA0` (§5) reads
`array_0x437_1079x.SpellIndex[spell]` as `byte2` when it re-binds a spell to LMB/RMB (`PI:1897`,
`PI:1939`), and `sub_6D4C0` shows it on hover (`PI:2275`). The map-start/possession creation path
`sub_55AB0` also passes `array_0x437[spell]` into `SetSpell` (`L:1319`). So **every selection route
uses the SAME stored per-spell level**; there is no separate per-binding level except the mirror
`SubSpellIndexLeft/Right` which is written from it.

---

## 5. Quickselect / mouse-binding keys

MC2's "quickselect" is **per-spell mouse-button binding (LEFT vs RIGHT)**, not a numeric hotbar.
- `array_0x3B5_949x.SpellIndex[spell]`: 0 = unbound, 1 = bound to LEFT mouse, 2 = bound to RIGHT.
  Set via the submenu commit (implicitly, by which button opened it) or directly by SHIFT+click
  command 0x26 (`EF:37950`).
- `SpellIndexLeft/Right` hold the spell index currently on each button; `SubSpellIndexLeft/Right`
  the level (mirror of `array_0x437[spell]`).
- **Cycling** (SHIFT+LMB / SHIFT+RMB during flight, `PI:528/533/541/546`) calls
  `sub_18DA0(entity, side, dir)` (`PI:1839`), which walks `spellIndex_D94FF[]` looking for the next
  possessed spell whose `array_0x3B5` matches the side (1 for left, 2 for right), then emits command
  31/32 with `byte2 = array_0x437[spell]` — i.e. **binding is per-spell, and it carries that spell's
  stored level** (`PI:1876-1897` right, `PI:1918-1939` left). This confirms *per-spell, not
  per-level* binding.
- **Firing:** LMB casts the LEFT spell, RMB the RIGHT (`HandleMouseButtons_18F80`, `PI:2027-2072`):
  it reads `SpellEnabled[SpellIndexLeft]` / `[SpellIndexRight]` and issues movement/fire command 6
  with sub-op 16 (left) / (right). `word_0x2E_46 > 0` on the spell entity = actively casting.

Number keys 1–8 (scancodes 2–9): in flight (`PI:613-619`) they emit
`HandleButtonClick_191B0(43, key-2)`. **OPEN:** command 43 (0x2B) in the command processor
(`EF:37999`) is the *name/message-slot* selector (`byte_0x3E0_2BE4_12222`), NOT a spell hotbar; no
`case` reads it as a spell quickcast. Either remc2 mis-labels this or the number keys drive the
chat/name UI, not spell selection. `SpellIndexes_0x39B_923x` (10 slots) is a learned-order list used
for save/replay and AI cycling (`EF:38680`, `EF:38764`), not a keyboard hotbar. I found no scancode
that directly casts spell N. Tried: grepping all `HandleButtonClick_191B0(43`, all reads of
`SpellIndexes`, and the whole command switch `EF:37592-38057`. Conclusion: **MC2's per-spell
selection is mouse-driven (the CTRL grid + LMB/RMB binding); there is no numeric per-key spell
hotbar in this decompile.** (Our port should treat LEFT/RIGHT binding as the "quickselect".)

---

## 6. Map screen interaction

**No in-map spellbook.** On the map screen (`MenuState` 6/7/8, `EF:21792` `case 6/7/8/0xB/0xC/0xE`),
the composite is: minimap on the LEFT, the **live 3D world rendered into a narrow right-hand
viewport**, and the CTRL selector overlays it identically (via `SHOW_MAP_BOTTOM_MENU=8`, drawn by
the same `DrawBottomSpellsMenu_2ECC0`, `EF:21959`).

Map-screen live-view geometry (`EF:21804-21871`):
```c
if (res == 1) { locViewportPosx = 384; locViewportWidth = 256; locViewportHeight = 400; locMinimapHeight = 400; }
else {
    locViewportPosx = 0.6 * screenWidth_18062C;   // world starts at 60% across
    if (locViewportPosx > 384) locViewportPosx = 384;   // capped at x=384
    locViewportWidth  = screenWidth_18062C - locViewportPosx;   // world fills the rest
    locViewportHeight = screenHeight_180624;
    locMinimapHeight  = min(screenHeight_180624, 400);
    if (scale > 1) { locViewportPosx *= scale; locViewportWidth = screenWidth - locViewportPosx; ... }
}
DrawMinimap_63600(0,0, player.x, player.y, locViewportPosx-2, locMinimapHeight, yaw, 204/scale, 1); // minimap = left strip
DrawMinimapEntites_61880(...);   // same 204/scale (EF:21851-60)
viewPort.SetRenderViewPortSize_40BF0(locViewportPosx, 0, locViewportWidth, locViewportHeight);  // EF:21862
m_ptrGameRender->DrawWorld_411A0(playerX, playerY, yaw, z+128, pitch, roll, fov);               // EF:21864
```
On entering the map/bottom-map states, the viewport rect is also set by the menu-state transition
(`UI:770-782`):
```c
case 6: case 7: case SHOW_MAP_BOTTOM_MENU: case SHOW_MAP_GAME_OPTIONS: ...:
    if (!DefaultResolutions() && res != 1)
        viewPort.SetViewPortScreenCoordinates_2CA60(384*scale, 0, screenWidth-384, screenHeight-80); // VP:151
    else
        viewPort.SetViewPortScreenCoordinates_2CA60(384, 0, 256, 400);
```
So on the classic 640×480 map screen: **destination rect = x∈[384,640), y∈[0,400)** (256×400) for
the live world; on wide screens **x∈[384·scale, screenW), y∈[0, screenH−80)**. The minimap occupies
`x∈[0, ~384)`.

**Minimap zoom law** (traced 2026-07-19, GameUI.cpp:2256-2411): `DrawMinimap_63600(x, y, posX,
posY, width, height, yaw, scaling, fillMode)` — `scaling` = **world-units per pixel** (204 on the
map screen, 256 on the flight radar; 256 units = 1 tile). The terrain blit draws a SQUARE region of
side `height·scaling` units centered on the player, yaw-rotated, wrapping at 256 tiles via byte
truncation → **map screen = 400·204/256 = 318.75 tiles vertically** (the whole world + ~25% wrap
repeat), flight radar = 128·256/256 = 128 tiles, circular. Quirk: the terrain blit's horizontal
span is width-independent (`height·scaling` squished into the 382px strip) while
`DrawMinimapEntities` (GameUI.cpp:1074-94) is isotropic (`width·scaling` = 304.4 tiles) — a ~4.6%
terrain/entity horizontal misalignment in retail. Our port uses the isotropic entity law for both
layers (see DEVIATIONS mgc-render `MC2_MAP_VIEW_SPAN_TILES`).

**Non-aspect "stretch":** there is no explicit source-cutout blit. The world is rendered *directly*
into the 256-wide (vs the flight-screen's wider) viewport by `DrawWorld_411A0` with the SAME
`fov`/`pitch`/`roll` as flight (`EF:21864-21871`). Because the projection maps the same FOV into a
narrower destination width without narrowing FOV, the result reads as horizontally squeezed — the
"stretched non-aspect live view" the player observes. `SetRenderViewPortSize_40BF0` just repoints
the render buffer start to `(locViewportPosx, 0)` and sets the render width/height
(`VP:51-70`); the projection is unchanged. **OPEN (mild):** confirming the exact horizontal-scale
math is inside `DrawWorld_411A0`/the rasteriser (`GameRenderOriginal.cpp`), not traced here — but
the viewport rect above is the authoritative destination geometry, and no separate stretch-blit of
a cutout exists (the world is rendered natively into the rect).

The CTRL selector on the map screen is byte-identical to the flight one: same
`DrawBottomSpellsMenu_2ECC0`, same `SelectSpellCategory_6D420`/`SelectSpell_6D4F0` input, same
`byte_0x457/spellIndex_0x458/subSpellIndex_0x459` state; only the close target differs (→ state 6
map instead of state 0 flight, `PI:899-902`), and sound is suppressed for the map variant
(`UI:757-758`).

---

## 7. Mana / level gating

The selector **greys but does not disable** — it always lets you hover/select; affordability is a
visual state, gating happens at cast time.

Per-box affordability (`canSummon`, `EF:22503`):
```c
canSummon = (SPELLS_BEGIN_BUFFER_str[spell].subspell[level].maxManaLimit_A == 0)   // free tier
         || (haveCastle && maxManaLimit_A <= Entities_EA3E4[CastleEntityIndex]->mana_0x90_144);
```
`maxManaLimit_A` is the level's **mana threshold to be usable at all** (needs enough *castle* mana);
`manaCost_6` is the **per-shot cost**. The two panels key on DIFFERENT predicates (corrected
2026-08-21 after player retail-verification):

- **Grid box (89/91)**: castle-pool ONLY. `!canSummon` → dark panel 91 + ghosted icon
  (`EF:22537`, `EF:22544`). Hand mana never darkens a grid box — a broke-but-eligible spell
  stays bright with an empty shot meter on hover (`EF:22515-22529`).
- **Flyout level box (161/162)**: the tile is `canSubSummon && manaPart` where
  `manaPart = playerMana / GetSpellManaCost(spell, tier)` per tier (`EF:22609`, `EF:22618`) —
  so an unaffordable single cast ALSO darkens the tier's frame (162), while the icon ghosts on
  the pool test ALONE (`EF:22625-28`): pool-ok-but-broke = dark frame + LIT icon; pool-fail =
  dark frame + ghosted icon.

Locked **levels** (level > `SpellLevels_0x41D_1053z[spell]`, the max unlocked) are drawn as empty
boxes (`EF:22611`), and `SelectSpell_6D4F0` clamps the cursor so a locked level cannot be selected
(`PI:2216`). So both **level-unlock** (XP-based, `SpellLevels`) and **mana-affordability**
(castle-mana-based, `maxManaLimit_A`) gate the presentation; only the level-unlock actually blocks
*selection*.

**Per-level mana cost table** = `SPELLS_BEGIN_BUFFER_str[spell].subspell[level]` (`Spells.h:7-25`):
```c
struct subspell {  // length 26, three per spell
    int32 subSpellIndex_2;   // behaviour/sub-id
    int32 manaCost_6;        // per-shot mana cost   <-- cost
    int32 maxManaLimit_A;    // castle-mana needed to enable this tier  <-- gate
    int32 xpos1_E;           // XP low bound (single-player unlock curve)
    int32 xpos2_0x12;        // XP low bound (multiplayer curve)
    int16 hintText_0x16x;    // lang-string id
    int16 word_0x18;         // fire duration / cost divisor
    int8  life_0x1A;
    uint8 fontType_0x1B;
};
struct { int8 byte_0 /*#tiers*/; uint8 isEnabled_1; subspell subspell[3]; } SPELLS_BEGIN_BUFFER_str[26];
```
The actual numeric costs live in the data blob `SPELLS_BEGIN_BUFFER_str` (loaded from game data,
not a C literal). `GetSpellManaCost_6D710` (`L:1714`, tokens trace §2) returns
`subspell[level].manaCost_6`, special-casing the castle spell (index 2) to scale by castle upgrade
level (1000/10000/…/300000000) and `+3000` when `byte_0x1BE_446` with no castle. **OPEN:** the
per-spell/per-level numeric cost values are data-driven (in `MSPRD00`/spell data), not in source;
port must read them from the loaded `SPELLS_BEGIN_BUFFER_str` table, not hardcode.

---

## 8. Port checklist (mgcarpet)

1. **Input:** CTRL (0x1D) hold → open docked bottom pane (state 5 flight / 8 map); release → close.
   Suspend mouse-look + fly-assistant while open; show software pointer; warp pointer to the current
   spell box on open.
2. **Layout:** bottom-anchored, 640-wide (centre+scale on wide screens), two `SPELL_ICON_PANEL`
   rows, `EDGE_PANEL` frame columns. Box `(col,row)` at
   `x = edgeW + col·iconW`, `y = (bottom − 2·iconH) + row·iconH`.
3. **Grid:** 2×13, row0 = spells 0..12, row1 = 13..25 (identity order = `spell_t`). cave_in(25) only
   on cave levels. Icon = HSPR `97 + spell`; possessed via `SpellEnabled[spell] != 0`; ghost via
   `array_0x3E9`/`array_0x403`.
4. **Submenu:** 3 boxes above the hovered box; icon = `179 + 3·spell + level`; number bg `165+level`;
   locked levels (`> SpellLevels[spell]`) shown empty; gold frame on the chosen level; XP progress
   bar per tier.
5. **State:** persist per-spell level in `array_0x437[spell]` (byte, 26). LMB→bind LEFT (cmd31),
   RMB→bind RIGHT (cmd32); both write `array_0x437[spell]=level`, set `SpellIndexLeft/Right` +
   `SubSpellIndexLeft/Right`, then `SetSpell` pushes the level into the live entity (`byte_0x46_70`,
   `subspell[level]`). All routes read the same `array_0x437` level.
6. **Quickselect = per-spell LMB/RMB binding** (`array_0x3B5`: 1=left, 2=right); SHIFT+click fast-bind;
   SHIFT+LMB/RMB in flight cycles to the next spell on that button (carrying its stored level). No
   numeric hotbar.
7. **Map screen:** minimap left strip, live world in right viewport (dest `x∈[384,640) y∈[0,400)` at
   640×480; `x∈[384·scale,screenW) y∈[0,screenH−80)` wide), same FOV → looks non-aspect. Same CTRL
   selector overlay. No in-map spellbook.
8. **Gating:** grey (dark panel + ghost icon) for unaffordable (`maxManaLimit_A` vs castle mana) and
   for locked levels; only level-lock blocks selection. Per-shot cost = `subspell[level].manaCost_6`
   (data-driven; castle spell scales by upgrade level).

---

### Certain vs OPEN
- **CERTAIN:** CTRL=0x1D hold-to-open, states 5/8; full pane geometry (bottom-anchored, 2×13, edge
  frames, box formula); all sprite indices incl. icon formulas `97+spell` / `179+3·spell+lvl`; grid
  order = identity `spell_t`; possession via `SpellEnabled[]`; submenu always-3-boxes with locked
  shown empty and clamp; **persistent level = `array_0x437[spell]`**, consumed via `SetSpell` →
  `subspell[level]`; LEFT/RIGHT binding is per-spell and reuses the stored level; map-screen
  viewport dest rects; grey-not-disable gating; cost table struct.
- **OPEN:** (a) numeric-key spell casting — command 43 in this decompile maps to name-slot select,
  not a spell hotbar; no per-key cast path found (MC2 selection appears fully mouse-driven).
  (b) exact horizontal-scale math of the map live-view is inside `DrawWorld_411A0` rasteriser (not
  traced); the destination rect is authoritative and there is no separate cutout-stretch blit.
  (c) numeric per-spell/per-level mana costs are data-driven in `SPELLS_BEGIN_BUFFER_str` (game
  data), not source literals.

---

## ADDENDUM (cycle-ring session) — corrections + the queued-list law

A dedicated trace of the SHIFT/ALT mechanics (decompile re-read) corrected
three points above and settled the persistence law. Citations as before
(PI = PlayerInput.cpp, EF = EventsFunctions.cpp, L = Level.cpp).

1. **`array_0x3B5` is a CYCLE-RING membership flag, not "which button
   holds this spell".** It is written ONLY by SHIFT+click (cmd 0x26 — a
   raw byte store, EF:37950-53, no equip side-effect). The normal
   click→submenu equip (cmd 0x1F/0x20) never touches it. §1's "quick set
   both/none" reading was wrong; the sender (PI:856-878) implements a
   toggle/move: SHIFT+same-button on a member removes it (byte2=0), any
   other SHIFT+click adds/moves it to the clicked button's ring. One byte
   ⇒ a spell is exclusive to one ring; there is no both=3 state.
2. **Cycling** (`sub_18DA0` PI:1839-1942): SHIFT = forward (dir 0), ALT =
   backward (dir 1) — flight call sites PI:528/533/541/546, map screen
   PI:955-977. LMB = ring 1, RMB = ring 2. Walks from the equipped spell
   ±1 with 0↔25 wrap; a candidate needs `SpellEnabled != 0` AND
   `array_0x3B5 == side`; 26 fruitless steps → return with NO command
   (the all-unavailable no-op, PI:1889/1931); the equipped spell is the
   lap's LAST candidate, so a single-member ring re-selects itself. On a
   hit it emits cmd 31/32 with `byte2 = array_0x437[spell]` — the stored
   level rides along.
3. **Corner tags are keyed on `array_0x3B5`,** not the equipped hand
   (EF:22546-53 reads the ring array), and drawn hover-only (or
   mid-cast) as §2.4 said. Our pane draws ring ∪ equipped — see
   DEVIATIONS.
4. **Persistence:** the ring and `array_0x437` carry across campaign
   levels (`sub_549A0` L:1261-68 via the `byteindex_256ar` template) and
   ride the whole-D41A0 SLEV save (L:199). Spell LOSS (`sub_69300`
   EF:55811-24) clears possession + the equip pointer(s) but NOT the
   ring — a lost spell stays a member and is merely skipped. Equip
   pointers are validated at level load (drop to −1 unless possessed,
   L:1332-35); the ring is never validated.

Port: `Mc2Spellbook::ring` / `World::mc1_ring` (MC1 = enhancement twin),
command `spell_ring` (cmd 0x26), app walk `ring_next` (sub_18DA0), carry
`mc2_install_selector_carry` + the `.mgcs` header's `mc1_spell_ring`;
wheel/SHIFT+wheel = `gameplay.enhancement.wheel_spells` (no retail
analogue).
