# MC2 AUTO-AIM — Verbatim Trace (target acquisition, scoring, crosshair)

All citations are `file:line` relative to `/home/rain/projects/mgcarpet/reference/remc2/remc2/engine/`.
Files: `EventsFunctions.cpp` (EF), `Events.cpp` (E), `Player.cpp`, `Sound.cpp`, `GameUI.cpp`, `engine_support.h`, `global_types.h`.

Cross-refs (do not re-derive):
- `docs/traces/mc2-class9-spell-projectiles.md` — flight states; sketched `sub_67CB0`/`sub_68940`/scorers.
- `docs/traces/mc2-player-cast-path.md` — launch block; `sub_68E50` called right after `sub_6DCA0`.
- `docs/traces/mc2-possession-delivery.md` — possession flight-side.

---

## 0. THE ACQUISITION LAW — one-line summary

**All player-projectile auto-aim is PROJECTILE-SIDE, one-shot, at spawn.** There is **no per-tick caster crosshair scan** — the player's own wizard `word_0x96_150` is NEVER written by a "what am I looking at" pick (§3). Instead: on the FIRST flight tick, if the projectile has no live locked target, it runs `sub_67CB0(self)` (EF:54710) which scans entity lists keyed on the **projectile's own `model_0x40_64`**, scores every candidate with `sub_68490`/`sub_685D0`, and writes the winner into the **projectile's** `word_0x96_150` plus desired `roll`/`fov`. Thereafter `sub_65610` homes every tick. The "aiming crosshair" the player sees is the in-world muzzle/spell sprite (`SetEntityIndex_49C90(proj, 42)`, local-player only, EF:55878) attached to that projectile — there is **no separate HUD reticle draw** in GameUI (§4).

---

## 1. `sub_67CB0(self)` — one-shot target acquisition (EF:54710-55094)

Signature: `signed int sub_67CB0(type_entity_0x6E8E* a1x)` (EF:54710). Called on the FIRST flight tick (guarded by the state's `byte[0]&2` init gate) from: fireball flight `sub_65C20` (EF:63106), possession `CastPosses_65F60` (EF:63249), generic `sub_65820` (EF:62907), state-8 `sub_662E0` (EF:63450), lightning-II `sub_66FD0` (EF:58735/58980), state-B `sub_66FB0` (EF:63589). Returns **1** if a target was found (caller then does a limited turn toward `roll`/`fov` or a full snap `yaw=roll; pitch=fov`), **0** otherwise (caller locks straight: `roll=yaw; fov=pitch`).

The whole function is one `switch (a1x->model_0x40_64)` (EF:54769) — **keyed on the PROJECTILE's model** (the class-9 spawn model set by `sub_6DCA0`, e.g. fireball=0, meteor=3, volcano=4, possession=1, lightning=9, crater=5, quake=2, cave-in=30). Default case returns 0 (EF:55012-55013 — no auto-aim).

### Entity-list identities (engine_support.h:336-369)
- `dword_38519` = the **wizards/players/castles/balloons** list (class 3: 1=player, 2=castle, 3=balloon; engine_support.h:355-365).
- `bytearray_38403x[29]` = the **29 per-class-index creature buckets** (engine_support.h:336). Iterated by `next_0`.
- `bytearray_38403x[88/4]` = **bucket 22** = the head-linked **multipart/worm** list (walked via `word_0x34_52` link chain).
- `dword_38523` = the **mana-sphere** list (possession case).
- `dword_38527` = the **buildings** list (possession case).
- `dword_38535` = the friendly **drone/guide** list (used by `sub_68940`, §6).

### Common accumulator pattern
Each branch keeps `best = 0`, `bestScore = -1` (unsigned 0xFFFFFFFF). For every candidate, `score = sub_68490(...)` (or `sub_685D0` for model-2/castle targets). **Lower score wins**: `if (score < bestScore) { best = cand; bestScore = score; }` (e.g. EF:54795-54799). `-1` from a scorer means "out of cone/range" and (being 0xFFFFFFFF unsigned) never beats a real score. First-scanned wins ties (strict `<`). On success: `self->word_0x96_150 = best - D41A0_0.struct_0x6E8E` (store target index), then `sub_655C0(self, best)` writes desired aim (§1.6), return 1.

### Range gate `word_160_0x1c_28`
The squared 3-D acquisition range is `Entities[self->id_0x1A_26]->dword_0xA0_160x->word_160_0x1c_28` — i.e. the **OWNER's** behavior-row lock range (`v43x = Entities_EA3E4[a1x->id_0x1A_26]`, EF:54781; `v10 = v43x->dword_0xA0_160x->word_160_0x1c_28`, EF:54787). Gate: `sub_583F0_distance_3d(&cand.pos, &self/cand.pos) <= range` (EF:54788). [RESOLVED 2026-08-25 — EF:54788 is a remc2 **source** typo, not a retail quirk and not a Hex-Rays slip. The original decompiler output (remc2 `ab199daed`, the initial upload, still raw) reads `sub_583F0((x_WORD *)(i + 76), (x_WORD *)(a1 + 76))` = `dist(cand.pos, self.pos)` (offset 76 = 0x4C = position, two distinct objects); remc2 commit `22bf3758a5` ("190419-01", turican0, 2019-04-19) struct-ified the pointer arithmetic and mistyped `a1` as `ix` on this one line, converting the three siblings at EF:54871 / 54897 / 54943 correctly in the same hunk. The gate is LIVE for the case-0 group, the two-point form is correct, and the port already does this — so no EXE disassembly is needed. ⚠ The blame chase passes through two decoys: a 2026-02-02 `axis_0x4C_76`→`position_0x4C_76` rename (`28b7c76336`) and a 2025-12-22 file split (`3e041c9d30`); run git INSIDE reference/remc2, which this repo gitignores.]

### 1.1 Case {0, 3, 4, 0x12, 0x13, 0x16, 0x1A, 0x1C, 0x1E} — the OFFENSIVE branch (EF:54771-54852)
Projectile models: 0 fireball, 3 meteor, 4 volcano, 0x12, 0x13, 0x16, 0x1A, 0x1C(charged-fireball), 0x1E. Three scans, in order:
1. **Wizards** `dword_38519` (EF:54783): skip `cand.id == self.id` (own owner) and `cand.byte[0] & 0x20` (invisible). Range-gated. Score `= (cand.model==2) ? sub_685D0(self,cand,0x71,0x71) : sub_68490(self,cand,0x71,0x71)` (EF:54790) — castles (model 2) use the box-corner scorer.
2. **Creatures** all 29 buckets **except bucket 22** (`if (j != 22)`, EF:54805): skip own-owner, require `cand.byte_0x39_57` (alive/active flag) and NOT (`StageVar2_0x49_73==14 && parentId==self.id`) — i.e. skip a creature you're already possessing (EF:54810-54812). Score `sub_68490(...,0x71,0x71)`. **No range gate here** — cone/dist handled inside the scorer.
3. **Only if no target yet** (`if (!v8x)`, EF:54824): the **worm/multipart bucket 22** — walk each head's link chain via `word_0x34_52` (EF:54830), score every segment `sub_68490(...,0x71,0x71)`.

On success (EF:54847-54852): `word_0x96_150 = best`; `sub_68BD0(self,best)` (EF:54848 — if best is a class-5 model-0 tree, marks `fontTypeIndex_0x3D_61=32`); `sub_655C0(self,best)` (aim); **if best is a class-3 player wizard (`class==3 && model==0`) → `sub_5EF70(best)`** (EF:54850-54851: sets that victim-wizard's `dword_0xA4_164x->word_0x36_54 = 100`, a "you are being targeted" alarm timer, EF:60598-60608). Return 1.

### 1.2 Case {1, 0x11} — POSSESSION branch (EF:54853-54858 → LABEL at 55015)
Projectile models 1 (possession) and 0x11. Sets `best=0`, `bestScore=-1`, `v26x = dword_38523` (**mana-sphere list**), then `break` into the shared tail at EF:55015. The tail scans THREE lists:
1. **Mana-spheres** `dword_38523` (EF:55015-55046): filter by model — `model < 0x27` accepted (LABEL_105 continue) OR `model==0x27` with `playerEntityIndex_0x94_148 != self.owner` OR `model==57` with `parentId_0x28_40 != self.owner` (EF:55017-55031). Require `byte_0x39_57`. Score `sub_68490(...,0x71,0x71)`.
2. **Buildings** `dword_38527` (EF:55047-55071): skip `playerEntityIndex_0x94_148 == self.owner` (own buildings), require `byte_0x39_57`, and **skip if the building param `str_D93C0_bldgprmbuffer[byte_0x46_70].byte_2 & 8`** (un-possessable flag, EF:55053). Score `sub_68490(...,0x71,0x71)`. (Contains a `debugcounter_249226` no-op, EF:55055-55059.)
3. **Worm bucket 22** `bytearray_38403x[88/4]` (EF:55072-55089): link-chain walk, score `sub_68490(...,0x71,0x71)`.

On success: `word_0x96_150 = best`; `sub_655C0(self,best)`; return 1 (EF:55090-55094). **This is how a possession projectile pre-locks a dwelling/sphere** — the model-1 branch scans buildings `dword_38527` directly (answers §5).

### 1.3 Case {7, 8, 0xB, 0xC} — wizards-only branch (EF:54859-54888)
Scans ONLY `dword_38519` (wizards), range-gated by owner's `word_160_0x1c_28` (correct two-point distance EF:54871), skip own-owner + invisible, score `sub_68490(...,0x71,0x71)`. On success writes `word_0x96_150`, `sub_655C0`, and `sub_5EF70(best)` if best is a player-wizard. Return 1.

### 1.4 Case 9 — LIGHTNING branch (EF:54889-54933)
Two scans: (1) wizards `dword_38519` with range gate `= self.minSpeed_0x84_132 * self.maxLife_0x4` (EF:54896 — **range = speed·maxLife = the projectile's own reach, not the row field**), score `sub_685D0` for castles else `sub_68490(...,0x71,0x71)`. (2) ALL 29 creature buckets (bucket 22 NOT excluded here), skip own-owner, require `byte_0x39_57` and not-already-possessed (`StageVar2!=14 || word_0x2C_44!=self.owner`), score `sub_68490(self,cand,0x71,0x200)` — **pitch cone 0x200 (=90°), i.e. lightning ignores vertical alignment**. On success: `word_0x96_150`, `sub_655C0`, return 1. (No `sub_5EF70` alarm.)

### 1.5 Case 0x10 & Case 0x19 (EF:54934-55011)
- **0x10**: wizards `dword_38519` (range = owner row field) + all 29 creature buckets, scorers with **yaw cone 0x100** (`sub_68490/685D0(...,0x100,0x71)`, EF:54945/54967) — wider horizontal cone. `sub_5EF70` alarm on wizard hit.
- **0x19** (cave-in): all 29 creature buckets only, extra filter `sub_3A7F0(cand)` (EF:54994 — on-ground/terrain-valid test), score `sub_68490(...,0x71,0x71)`. No wizard scan, no alarm.

### 1.6 What gets written on success — `sub_655C0(self, target)` (EF:62772)
```c
void sub_655C0(self a1x, target a2x) {
    sub_65580(a2x);                                              // raise target z by its box (EF:62750)
    a1x->roll_0x20_32 = Maths::sub_581E0_maybe_tan2(&a1x->pos, &a2x->pos);   // DESIRED yaw  → roll
    a1x->fov_0x22_34  = Maths::sub_58210_radix_tan (&a1x->pos, &a2x->pos);   // DESIRED pitch → fov
    sub_655A0(a2x);                                              // lower target z back (EF:62761)
}
```
So `word_0x96_150` = the locked target index; `roll_0x20_32`/`fov_0x22_34` = the **desired** yaw/pitch toward it. The caller then either snaps (`yaw=roll; pitch=fov`, possession EF:63250-63251) or turns toward it capped at 34/tick (fireball EF:63108-63113). Every subsequent tick, `sub_65610(self, Entities[word_0x96_150])` re-aims with the behavior-row turn caps (class-9 trace §"Homing helper").

---

## 2. THE SCORERS (verbatim)

Both take `(self a1x, cand a2x, yawCone a3, pitchCone a4)` and return a squared-lateral-offset "how far off my aim axis is this target" — **lower = better aligned + closer**; `-1` = rejected. Both raise/lower the candidate z via `sub_65580`/`sub_655A0` around the read.

### `sub_68490` — the standard scorer (EF:55101-55153)
```c
int sub_68490(self a1y, cand a2x, uint16 a3, uint16 a4) {
    sub_65580(a2x);
    v5  = Maths::sub_581E0_maybe_tan2(&a1y->pos, &a2x->pos);   // bearing yaw to target
    v13 = sub_582B0(a1y->yaw_0x1C_28, v5);                     // |yaw error| (shortest arc)
    if (v13 <= a3) {                                          // within yaw cone?
        v7  = Maths::sub_58210_radix_tan(&a1y->pos, &a2x->pos);// bearing pitch
        v14 = sub_582B0(a1y->pitch_0x1E_30, v7);              // |pitch error|
        if (v14 <= a4) {                                     // within pitch cone?
            v8 = Maths::EuclideanDistXYZ_58490(&a1y->pos, &a2x->pos);   // 3D distance
            if (v8 <= 5120) {                               // HARD MAX RANGE 5120
                sub_655A0(a2x);
                v9  = v8 * sin[0x200+v13];   // dist · cos(yawErr)
                v10 = v8 * sin[v13];         // dist · sin(yawErr)
                v11 = v8 * sin[0x200+v14];   // dist · cos(pitchErr)
                v12 = 4 * sin[v14] * v8 >> 16;                // 4·dist·sin(pitchErr)  (weighted)
                result = (v11>>16)*(v11>>16)                  // cos(pitch)² component
                       + (v9 >>16)*(v9 >>16)                  // cos(yaw)²  component
                       + (4*v10>>16)*(4*v10>>16)              // (4·sin(yaw))²  — yaw error weighted ×4
                       + v12*v12;                             // (4·sin(pitch))² — pitch error weighted ×4
            } else { sub_655A0(a2x); result = -1; }          // too far
        } else { sub_655A0(a2x); result = -1; }              // pitch cone miss
    } else { sub_655A0(a2x); result = -1; }                  // yaw cone miss
    return result;
}
```
Weighting: the ON-axis components (`cos` terms `v9`, `v11`) enter squared at ×1; the OFF-axis (angular-error) components (`sin` terms `v10`, `v12`) enter at ×4 before squaring (×16 in the sum). So the score is dominated by **angular misalignment** — the best target is the one most directly in front of the projectile, distance a secondary tie-break. Hard caps: yaw error ≤ a3 (0x71=113 default, 0x100=256 wide), pitch error ≤ a4 (0x71, or 0x200=512 for lightning), 3-D distance ≤ **5120**.

### `sub_685D0` — the castle/box scorer (EF:55157-55189)
Used only when `cand.model_0x40_64 == 2` (castle). Identical cone/range gates (`> a3`/`> a4`/`> 5120` → return -1) but the on/off-axis weighting is transposed:
```c
    v11 = dist·cos(yawErr);  v12 = dist·sin(yawErr);
    v13 = dist·cos(pitchErr); v14 = 4·dist·sin(pitchErr)>>16;
    return (4*v12>>16)*(4*v12>>16)    // (4·sin(yaw))²  — yaw error ×4
         + (v11>>16)*(v11>>16)         // cos(yaw)²
         + (v13>>16)*(v13>>16)         // cos(pitch)²
         + v14*v14;                    // (4·sin(pitch))²
```
Functionally the same law (angular-error ×4-weighted, distance secondary) — the castle variant exists because castles are large and the caller passes it a slightly different geometry read; the formula is equivalent modulo term order.

**RNG in scorers/acquisition: ZERO draws.** Deterministic.

---

## 3. THE CASTER'S TARGET — there is NO player crosshair pick

**Traced every writer of a class-3 player wizard's own `word_0x96_150`:**
- The player tick chain is `AddPlayer03_00_5E010` (EF:59955) → `sub_5F380` (EF:60748) → `sub_5EFA0` (EF:60613).
  - `sub_5F380` (EF:60748-60863): handles speed/strafe and the three fire buttons (`sub_5F660`, EF:60852/60855/60859). **It never scans for or writes `word_0x96_150`.**
  - `sub_5EFA0` (EF:60613): at EF:60635-60639 it only CLEARS a stale target (`if word_0x96_150 && (target.life<=0 || target.byte[1]&4) → word_0x96_150 = 0`). It **never acquires** one.
  - `AddPlayer03` (EF:59955-60042): mana/life regen, castle docking, charge counter (`byte_0x154_340++`, EF:59991-59992). No target scan.
- The `word_0x96_150 = …` writers that DO target a wizard are all **rival-AI** decision functions (`sub_13E40` EF:6259, `sub_14030` EF:6284, and the `sub_146C0` family EF:6115-6395) — these run for **AI wizards only**, choosing whom to attack; they are gated behind the AI cast dispatcher, not the human input path.
- The only human-path writes to a player's `word_0x96_150` are **re-locks after a hit**: `sub_686D0(self, victim)` (EF:55193) — when a class-3 model-0/1 player's projectile impacts, it copies the victim into the OWNER's `word_0x96_150` (EF:55210) as an auto-retarget convenience. This is a consequence of firing, not an aim source.

**Conclusion:** In retail MC2 the human player does **not** carry a per-tick "what am I looking at" lock on their wizard. Auto-aim is entirely the **projectile** acquiring its own target at spawn (§1). The wizard's `word_0x96_150`, when set, is either the post-hit retarget (`sub_686D0`) or AI-only. A port that wants "fireballs curve to targets" must implement the **projectile-side `sub_67CB0`**, not a caster crosshair. (This is the PLAYER-OBSERVED GAP: our port omitted `sub_67CB0`, so projectiles fly straight.)

---

## 4. THE AIMING CROSSHAIR UI — it is the in-world projectile sprite, not a HUD reticle

**Searched all GameUI*.cpp / GameRender*.cpp / ViewPort.cpp for a crosshair/lock/reticle draw:** none exists. GameUI's draw functions are the top status bar (`DrawTopStatusBar_2D710` EF/GameUI:123), spell icon (`DrawSpellIcon_2E260` GameUI:341), minimap (`DrawMinimap*` GameUI:2256+), objective textbox — **no reticle sprite, no lock indicator, no target-tracking overlay**. The player's `word_0x96_150` is **never read by any GameUI/GameRender code** (grep clean).

What the player sees as an "aiming crosshair" is the **muzzle/spell effect entity attached to the projectile**, shown only to the local player:
```c
// fireball effect state sub_693F0, right after spawn (EF:55877-55878):
if (v1x->dword_0xA4_164x->playerColorIndex_0x38_56 == D41A0_0.LevelIndex_0xc)   // local player
    SetEntityIndex_49C90(v6x, 42);                                              // sprite index 42
```
`SetEntityIndex_49C90(proj, 42)` (EF:32830) sets `proj->word_0x5A_90 = 42` — the local-player fireball's sprite becomes index 42 (a distinct "your shot" sprite; remote players' projectiles keep the default). Same pattern at EF:63048 (fireball flight re-assert) and EF:30291 (creature). **The projectile carries its own aim** (`roll`/`fov` from `sub_655C0`, homed by `sub_65610`), so the visual "lock" is simply the projectile curving toward the acquired `word_0x96_150` target — there is **no separate locked/free state sprite and no lead-prediction reticle**. Lead/prediction lives only in the FLIGHT homing (`sub_65610` re-aims at the target's current position every tick, class-9 trace §"Homing helper"), not in any UI element.

[OPEN: whether sprite 42 is drawn as a screen-space overlay by the render pipeline vs a world entity — it is set as the entity's `word_0x5A_90` sprite (a world sprite), so it renders in-world at the projectile, not as a fixed screen reticle. No HUD-anchored crosshair was found.]

---

## 5. THE POSSESSION CASE

A possession spell (model 1) routes: `sub_69640` (EF:55915) → `_4A190(pos,9,17)` spawns a class-9 **subtype 17 → projectile model 1** possession projectile (player-cast trace §2.2). That projectile flies under `CastPosses_65F60` (state 0x01, class-9 trace §"State 0x01").

**Acquisition (pre-lock):** on its first tick (`byte[0]|=2`), `CastPosses_65F60` calls `sub_67CB0(self)` (EF:63249). With `model==1` this enters the **case {1,0x11} possession branch** (§1.2), which scans `dword_38523` (mana-spheres), **`dword_38527` (buildings/dwellings)**, and worm bucket 22 — scoring each with `sub_68490`, picking the best-aligned, writing it into the projectile's `word_0x96_150`, and snapping aim (`yaw=roll; pitch=fov`, EF:63250-63251). **So possession DOES pre-lock a building via `sub_67CB0`'s model-1 branch** (the `dword_38527` scan), keyed on the projectile's own model — NOT via the caster's target.

**Delivery (flight-side):** the specialized probe `sub_108B0` (EF:3783) ray-marches map cells along the projectile's pitch and accepts possession victims only — class-5/model-22, or class-10/model∈{0x27,0x28,0x2D(45),57} (EF:3823-3843). This is the impact-side contact test (class-9 trace §"State 0x01" step 4), independent of the pre-lock. The pre-lock (`sub_67CB0` model-1 → `dword_38527`) steers the projectile TOWARD a dwelling; `sub_108B0` confirms the actual hit. Both are needed for a faithful port.

---

## 6. `sub_68940(self)` — friendly homing-drone lock (EF:55315-55390)

Runs BEFORE `sub_67CB0` in most flight states (`if (sub_68940(self) || sub_67CB0(self))`, EF:62907/63450/63589). Model gate (EF:55330-55355): models {0,1,2,3,4,5,8,9,0xC,0x16,0x17,0x1A,0x1C,0x1E,30}. Only if owner (`id_0x1A_26`) is a **class-3** entity (EF:55360): scan `dword_38535` (drone list) for `model==78 && word_0x32_50==self.id && word_0x36_54==-1` (an unclaimed guide drone belonging to this projectile), within owner's `word_160_0x1c_28`, front cone `sub_582B0(yaw,bearing) < 0xAA` (~30°), nearest wins. On hit: `word_0x96_150 = drone`, `sub_655C0(self, drone)` (aim), return 1. This is the "guided" spell variant — a friendly drone acts as a waypoint. Not offensive acquisition; runs first so a laid guide-drone overrides auto-target.

---

## 7. `sub_68E50(caster a1x, proj a2x, spellEntity a3x)` — post-spawn helper (EF:55595-55672)

Called immediately after every `sub_6DCA0` spawn (EF:55861 etc.). **It does NOT touch `word_0x96_150` and does NOT seed the target lock from the caster's target.** It is a **multipart-position/muzzle-offset** helper:
```c
signed int sub_68E50(caster a1x, proj a2x, spellEntity a3x) {
    v14x = a2x->position_0x4C_76;                       // start at projectile pos
    v3 = a1x->struct_byte_0xc_12_15.byte[1];
    if (v3 & 1) {                                       // caster flag: offset LEFT hand
        if (a3x->model==4 && a3x->word_0x96_150)        //   (volcano-type: anchor to spell's target z)
            v14x.z = Entities[tgt]->box.yaw + Entities[tgt]->pos.z;
        else
            MoveEntity_57FA0(&v14x, (a1x->yaw - 512)&0x7ff, 0, 256);   // step 256 to the caster's LEFT
        if (getTerrainAlt(&v14x) > v14x.z) v14x = a2x->pos;           // don't clip terrain
        for (each linked segment via word_0x34_52) CopyEntityPosition(seg, &v14x);  // move multipart chain
        CopyEntityPosition(a2x, &v14x);                 // reposition projectile
    } else if (v3 & 2) {                                // caster flag: offset RIGHT hand
        … same with (a1x->yaw + 512)&0x7ff …            // step 256 to the caster's RIGHT
    } else {
        CopyEntityPosition(a2x, &v14x);                 // no offset
    }
    return 0;
}
```
So `sub_68E50` positions the projectile (and any linked multipart segments) at the correct **left/right muzzle** based on the caster's hand flags (`byte[1] & 1` / `& 2`), clamped above terrain. It performs **no target acquisition and no target inheritance** — acquisition is 100% `sub_67CB0`/`sub_68940` on the projectile's first FLIGHT tick, which happens on a LATER tick than the spawn. The projectile's `word_0x96_150` is left at whatever `_4A190`/`sub_6DCA0` set (0 = unlocked), so the flight state's init branch runs the auto-aim. (Confirms class-9 trace's OPEN item: `sub_68E50` = muzzle/multipart positioning, not aim-seeding.)

---

## 8. THE ACQUISITION LAW (compact, per projectile model)

| proj model | branch (§) | lists scanned (in order) | range gate | scorer(s) & cones | wizard alarm |
|---|---|---|---|---|---|
| 0,3,4,0x12,0x13,0x16,0x1A,0x1C,0x1E | 1.1 | wizards `dword_38519` → 28 creature buckets (skip #22) → worm bucket 22 (only if none) | owner `word_160_0x1c_28` | `685D0` if castle else `68490`, cones 0x71/0x71 | yes (`sub_5EF70`→100) |
| 1, 0x11 (possession) | 1.2 | mana-spheres `dword_38523` → **buildings `dword_38527`** → worm bucket 22 | (none; scorer dist≤5120) | `68490`, 0x71/0x71 | no |
| 7,8,0xB,0xC | 1.3 | wizards only `dword_38519` | owner `word_160_0x1c_28` | `68490`, 0x71/0x71 | yes |
| 9 (lightning) | 1.4 | wizards `dword_38519` → all 29 creature buckets | `speed·maxLife` | `685D0`/`68490`, yaw 0x71 **pitch 0x200** | no |
| 0x10 | 1.5 | wizards → all 29 creature buckets | owner `word_160_0x1c_28` | `685D0`/`68490`, **yaw 0x100**/0x71 | yes |
| 0x19 (cave-in) | 1.5 | all 29 creature buckets (filter `sub_3A7F0`) | (none) | `68490`, 0x71/0x71 | no |
| any other | default | — | — | — (return 0) | no |

**Scorer law (both):** reject if `|yawErr| > cone_yaw`, `|pitchErr| > cone_pitch`, or `dist3D > 5120`; else `score = cos²-components·1 + sin²(angularErr)-components·16` — i.e. **minimize angular misalignment first, distance second**. Lowest score wins; first-scanned breaks ties. Zero RNG.

**Player-target/crosshair law:** the human player has **no per-tick target scan and no HUD reticle**. Auto-aim is the projectile's own one-shot `sub_67CB0` on first flight tick, writing the projectile's `word_0x96_150` + desired `roll`/`fov`; homing (`sub_65610`) then curves it toward the moving target each tick with behavior-row turn caps. The visible "crosshair" is the local-player projectile sprite (`SetEntityIndex_49C90(proj, 42)`). Post-hit, `sub_686D0` copies the victim into the player-wizard's `word_0x96_150` as a retarget hint (read only by AI/nothing in UI). Possession pre-locks a dwelling via the model-1 branch's `dword_38527` (buildings) scan, then confirms contact with `sub_108B0`.

---

## 9. OPEN / uncertain
- ~~**EF:54788 self-self distance**~~ **CLOSED 2026-08-25** — a remc2 source typo introduced by commit `22bf3758a5` (2019-04-19); the original decompiler output is the two-point form and the port matches it. No EXE disassembly needed. See the "Range gate" note above. ⚠ Measured scope: *not* a load-bearing fork on the recorded corpus — across 15 full-pool censuses over five takes the class-3 roster only ever holds the human's own family, so with EF:54785's same-id skip a player-cast case-0 projectile has no class-3 candidate at all.
- **NEW (open, different head): EF:5665 self-self yaw in `sub_131F0`** — the rival possess-approach state 4 reads `a1x->roll_0x20_32 = Maths::sub_581E0_maybe_tan2(&a1x->position_0x4C_76, &a1x->position_0x4C_76)`. Same corruption class, different provenance: the 2018 original (`ab199daed`) is `a1[16] = sub_581E0(a1 + 38, (x_WORD *)(v1 + 76))` — `a1` is `x_WORD*`, so `tan2(SELF.pos, TARGET.pos)` written to byte 0x20 — mangled in two steps by remc2 `43c558727c` (2019-03-29) then `d3d52a12d6` (2019-03-30). A whole-engine audit finds exactly TWO such sites (this and EF:54788), so the sub-class is bounded at two. The port does NOT carry this one: it merges retail states 4 and 6 into `Mc2AiState::Possess` (mc2/rivals.rs:191) and approaches at 1024/3072 with no 0x20 write, where retail state 4 uses 256/2048 (EF:5666). **Un-dug — needs its own pre-image + corpus treatment before anything is touched.**
- **Sprite 42 render path** — set as the projectile's world sprite (`word_0x5A_90`), rendered in-world, NOT a screen-anchored HUD crosshair. No HUD reticle exists. (If the player expects a fixed-screen reticle, that is NOT in retail MC2 — the "aim feedback" is the curving projectile itself.)
- **`sub_3A7F0`** (cave-in candidate filter, EF:54994) body not transcribed here — an on-ground/valid-terrain predicate; port can approximate as "target sits on terrain".
- **`word_160_0x1c_28` values per behavior row** — see class-9 trace behavior-row table (row 60-65 all 4096; the acquisition range is the OWNER's row, i.e. the wizard's own `dword_0xA0_160x`, which differs from the projectile's row). Confirm which owner-row a human wizard carries.
- **`sub_68BD0`** (EF:54848, on offensive-branch success) marks a class-5 model-0 tree's `fontTypeIndex_0x3D_61=32` — cosmetic (targeted-tree highlight?); benign for aim.
