# Stage-3 inventory — every single-human datum the sim carries

Read-only sweep of the working tree at 32f1c0b + uncommitted edits (10-10). All paths are relative to
`/home/rain/projects/mgcarpet/crates/mgc-sim/src/` unless stated. Line numbers were read from the files.

**Classes**
- **P** PER-SEAT — real per-human state; needs one copy per human seat.
- **R** DERIVABLE-FROM-RECORD — duplicates bytes that are (or belong) on the human's class-3 record; the record field is named.
- **G** GLOBAL — stays single (world-global, config, retail globals, presentation for the local viewer).
- **T** TRANSPORT — a writer→drain mailbox between world walkers and the human's flight drive/wizard pass.

**Counts** are `grep -o` hits, shown as `sim src / sim tests / other crates (app+conform+render+import)`, pattern `\.field\b`
(or `player\.field\b` for `Player`). They include the declaration, snapshot codec (`snap_write`/`snap_apply`), hash
destructure and in-file unit tests, so read them as an upper bound on logic sites. Names marked ⚠noisy share a
spelling with unrelated fields (e.g. `.kills`, `.stats`, `.completed`, `.duel`), so their counts are high.

Record-field shorthand: `x/y/z`, `f30` = +30 heading (MC2 `yaw_0x1C_28` lives at the same port field), `f32` = +32
pitch (MC2 @0x20 `roll_0x20_32`), `act_life` (+12), `max_life` (+8), `f136` (+136 ceiling), `f140` (+140 purse),
`tick70` (+70 state / MC2 action), `flags` (+16/+17 as a u32: 0x20 invisible, 0x300 hands, 0x4000 shield (+17 0x40),
0x8000 rebound (+17 0x80), 0x1000 shrine (+17 0x10)), `mail`, `next20/prev22` (+20/+22 chain links),
`f38` (+38 killer), `f46`, `f63` (tick counter), `f126` (+126 actual speed), `rand` (+4 entity LCG),
`f26` (MC2 `dword_0x10_16`).

What `World::mirror_human_record` (engine/world.rs:7928) writes today, FROM the registers:
`x,y,z ← human_pose`; `f30,f32 ← mc1_cast_pose.heading/pitch` (MC1) or `human_yaw` + the record's own `f32` (MC2);
`max_life ← PLAYER_LIFE_MAX`; `act_life ← player.life`; `f136 ← player.mana_max`; `f140 ← player.mana`;
`tick70 ← player.state` (0/2/3; 12 while `mc2_endseq`); `flags & (0x20|0x300|0x8000) ← hand_bits, player.invisible||ghost, player.rebound`.
Everything else on the record is not written by the mirror. Mail is the one datum already moved: `Gen::pmail()/pmail_mut()`
(engine/features.rs:6757/6765) return `ent[human_rec].mail` when `human_mail_home()` (:6745) holds.

---

## 1. `World::player: Player` (engine/world.rs:1079; struct at :129, Default :282, manual Hash :322)

`self.player.*` / `w.player.*`: **1028 / 0 / 9** hits in total. This is the largest block.

| Field | Def | Meaning | Class | Record home / note | Refs |
|---|---|---|---|---|---|
| `mana` | world.rs:132 | current purse (+140) | **R** | `f140` (already mirrored) | 189/0/2 |
| `mana_max` | :137 | ceiling (+136), census-recomputed each tick | **R** | `f136` (mirrored) | 68/0/2 |
| `mana_delta` | :148 | regen/debit delta (+132) | **P** | +132. The port `Ent` has no `f132`, so it stays a wizext register | 68/0/0 |
| `banked` | :151 | houses tally + own castle stored (HUD %, win numerator) | **P** | per-wizard (wizext +308 + castle). Census-derived | 19/0/0 |
| `world_mana` | :153 | world total (str_184.u32_188) | **G** | world-global; misplaced on Player | 21/0/0 |
| `left` / `right` | :155/:156 | hand equips (var_940/944) | **P** | wizext | 52/2/0, 22/0/0 |
| `owned` | :159 | owned-spell manifestation slots (var_676) | **P** | wizext table | 99/0/0 |
| `shield` | :163 | shield CHARGED (+17 0x40) | **R** | `flags & 0x4000`, not mirrored yet | 26/0/0 |
| `shield_armed` | :171 | MC2 shield ARMED (byte[2] & 0x40) | **R** | MC2 record byte[2] & 0x40; the port flag-bit mapping is unchecked | 7/0/0 |
| `invisible` | :172 | cloak (+16 0x20) | **R** | `flags & 0x20` (mirrored) | 38/0/0 |
| `rebound` | :173 | rebound (+17 0x80) | **R** | `flags & 0x8000` (mirrored) | 10/0/0 |
| `beyond_sight` | :174 | Beyond Sight active | **P** | not checked whether a record bit exists | 10/0/0 |
| `heal_active` | :175 | heal toggle active | **P** | | 12/0/0 |
| `accel` | :177 | accel channel direction ±1 | **P** | | 32/1/0 |
| `accel_held` | :181 | accel button held → 3.0 vs 2.0 | **P** | | 6/0/0 |
| `accel_mc2_factor` | :187 | MC2 Speed tier multiplier | **P** | | 18/0/0 |
| `invis_strength` | :193 | MC2 invis tier (byte_0x1BF_447) | **P** | wizext | 8/0/0 |
| `metamorph` | :200 | MC2 metamorph model | **P** | retail also sets caster byte[0] \|= 0x20 (hide), part of which is R | 8/0/0 |
| `speed_boost` | :203 | cached thrust-override factor (f32) | **P** | derived each tick | 14/0/2 |
| `teleport_return` | :208 | teleport return axis | **P** | retail keeps it on the MANIFESTATION (+150/+154), so it could move to the token record | 13/0/0 |
| `life` | :210 | actLife (+12) | **R** | `act_life` (mirrored) | 105/0/2 |
| `grace` | :216 | spawn grace (u16_331) | **P** | wizext | 44/0/0 |
| `regen_delay` | :219 | regen stall (u32_383) | **P** | wizext | 13/0/0 |
| `life_rate` | :226 | life-regen rate register (u16_341) | **P** | wizext | 14/0/0 |
| `state` | :228 | LifeState Alive/Falling/Dead | **R** | `tick70` 0/2/3 (mirrored) | 109/0/0 |
| `fall_speed` | :230 | death-fall vz (var_u16_29841_46) | **R** | most likely the carpet entity's `f46`; not verified against a capture | 25/1/0 |
| `killer` | :233 | killer latch (+38) | **R** | `f38`, not mirrored | 20/0/0 |
| `death_owned` / `death_owned_blue` | :238/:244 | spells owned at death / blue grant (Type_160+532, var_916) | **P** | wizext | 13/0/0, 3/0/0 |
| `hit_flash` | :246 | red overlay ticks | **P** (presentation) | HUD of that seat's viewer; nothing in the sim reads it | 7/0/0 |
| `lost` | :250 | castle-less SP death ⇒ restart | **G** | SP-only rule (MP has no restart) | 3/0/0 |
| `mc2_respawn_timer` | :279 | MC2 `dword_0x10_16` (0 or 1200) | **R** | MC2 `f26` (the field `lease2e` was split off). Moderately sure | 4/0/0 |

Player tally: **R 11** (mana, mana_max, shield, shield_armed, invisible, rebound, life, state, fall_speed, killer, mc2_respawn_timer) ·
**P 20** · **G 2** (world_mana, lost).

---

## 2. Other single-human `World` fields (engine/world.rs, struct at :728)

| Field | Def | Meaning | Class | Note | Refs |
|---|---|---|---|---|---|
| `mc2_carpet_stall` | :795 | carpet one-shot skip (byte[1] & 8) | **R** | MC2 record byte[1] & 8 | 8/0/1 |
| `mc2_carpet_slot` | :800 | MC2 human carpet record slot | **P** | becomes one slot per seat (the record id) | 63/0/0 |
| `mc1_frame` | :814 | MC1 player block +18 frame counter | **G** | hash-excluded tick function; per-block in retail but same value for everyone | 20/0/1 |
| `mc1_hud_drawn` | :824 | local view mode draws HUD ⇒ alert decrements run | **P** | a per-viewer INPUT feeding the per-seat alerts; conform seeds it from `pst.local_player` (mgc-conform/src/replay.rs:2561) | 6/0/1 |
| `mc1_carpet_slot` | :839 | MC1 human carpet record slot | **P** | as `mc2_carpet_slot` | 42/0/0 |
| `hand_bits` | :857 | caster +16 & 0x300 | **R** | `flags & 0x300` (mirrored) | 20/0/1 |
| `token_hand` | :867 | patch side table, token slot→hand | **G** | already keyed by token slot | 4/0/0 |
| `mc1_cast_pose` | :873 | prev-frame carpet pose for MC1 token muzzles | **R** | record x/y/z/`f30`/`f32` as of the previous frame (prev-pose echo; mirror sources f30/f32 from it) | 19/0/6 |
| `mc1_acq` | :889 | acquisition list Type_160+532 | **P** | wizext | 62/0/1 |
| `mc2_doom_meter` | :893 | doomsday HUD meter | **G** | | 5/0/0 |
| `pending_teleport` (+`_slot`) | :962/:973 | warp mailbox | **T** | see §6 | 33/0/1, 16/0/0 |
| `pending_speed_zero` | :980 | teleport zeroes target speed | **T** | §6 | 9/0/0 |
| `pending_speed_base` | :988 | speed-burst end restore | **T** | §6 | 21/0/0 |
| `mc1_v14` | :994 | Type_160 v_14 (speed press moved target) | **P** | wizext flight register | 19/0/1 |
| `mc2_cmd_speed` | :1005 | echo of `speed_0xc_12` for the speed token | **P** | echo of that seat's `Mc1State::tgt_speed`. It could be read from the flight state | 9/0/4 |
| `human_wiz_top` | :1011 | tick-top: human on class-3 roster | **R** | record `act_life >= 0` sampled at the chain build | 10/0/0 |
| `mc1_at_shrine` | :1037 | dolmen shrine stamp latch | **R** | record `flags & 0x1000` (+17 & 0x10), retail's real home | 9/0/0 |
| `fall_pre_z` | :1051 | fall-trail spawn z (mover scratch) | **P** | mover scratch | 8/0/0 |
| `mc1_fall_scratch` | :1066 | MC1 full fall scratch axis | **P** | | 2/0/0 |
| `mc1_fall_entry_z` | :1076 | tick-entry z while falling | **P** | | 2/0/0 |
| `wiz_charge` | :1090 | `[u8;8]` cast-charge per seat | **G** (already per-seat array) | indexed by seat (re-keyed at stage 1). conform hardwires `[0]` (§7) | 57/0/1 |
| `kill_tally` | :1099 | `[[u16;8];8]` | **G** (already per-seat) | | 11/0/0 |
| `human_pose` | :1103 | human pose for rival/world probes | **R** | record x/y/z | **123**/0/0 |
| `human_bucket_alive` | :1113 | tick-top bucket[0] membership | **R** | record membership at the chain build (as `human_wiz_top`) | 9/0/0 |
| `human_pose_prev` | :1118 | previous settled pose | **R** (prev echo) | per-seat prev copy of record x/y/z | 23/0/3 |
| `human_yaw` / `human_yaw_prev` | :1123/:1124 | heading echo / prev | **R** | `f30`. The prev copy is per-seat | 8/0/1, 7/0/1 |
| `duel` | :1132 | MC1 human duel latch | **P** | wizext +314..318 | 27/0/0 ⚠noisy |
| `mc2_duel` | :1138 | MC2 duel lock | **P** | wizext 322/326/330 | 18/0/0 |
| `mc2_book` | :1143 | the human's MC2 spellbook | **P** | biggest per-seat block | **268**/0/0 |
| `mc1_ring` | :1150 | MC1 cycle-ring membership | **P** | | 7/0/5 |
| `start_markers` | :1154 | marker tiles by seat | **G** | already per seat | 31/0/0 |
| `win_pct` | :1159 | level goal % | **G** | | 9/0/4 |
| `win_streak` | :1162 | banked-share streak | **P** | per-player in retail | 20/0/2 |
| `completed` | :1165 | completion latch (+13325 bit 2) | **P** | per-player in retail | 45/12/4 ⚠noisy |
| `dev_spells` / `dev_minted` | :1167/:1177 | dev all-spells; tokens it minted | **G** / **P** | toggle is config. `dev_minted` marks one human's tokens | 28/0/27, 24/0/0 |
| `cheat_mode` / `mc2_free_spells` | :1183/:1188 | retail tester flag; MC2 free-spell option | **G** | | — / 4/0/0 |
| `prev_fire` | :1215 | last tick's fire buttons (edge detect) | **P** | per-seat input edge | 8/0/0 |
| `accel_veto` | :1235 | brake veto from thrust | **P** | | 13/0/2 |
| `pending_respawn` | :1238 | respawn teleport mailbox | **T**/**P** | §6 | 18/0/3 |
| `pending_restart` | :1241 | castle-less death ⇒ level restart | **G** | SP rule | 10/0/1 |
| `invincible` / `ghost` / `inert` | :1245/:1252/:1257 | cheat switches | **G** (config) | ghost feeds `flags & 0x20` (R) and `Gen::player_ghost`. They become per-seat only if cheats become per-human | 4/0/16, 21/0/30, 13/0/16 |
| `notifications` | :1268 | toast stack | **G** | presentation | 13/0/1 |
| `won` | :1275 | level won terminator | **P**-ish | SP ending. In MP battle royale it is the last survivor | 23/2/1 ⚠noisy |
| `mc2_endseq` | :1283 | MC2 level-end sequence driving the human | **P** | retail swaps the player entity's action to 11/12 | 16/0/0 |
| `mc2_end_pending` | :1289 | trip mailbox → endseq | **T** | §6 | 3/0/0 |
| `mc2_recast_surcharge` | :1306 | wizext byte_0x1BE_446 | **P** | doc says "only the human's is modelled" | 9/0/0 |
| `rival_deaths` | :1127 | per-tick deaths for the ticker | **G** | | 7/0/0 |

---

## 3. `Gen` single-human fields (engine/features.rs, struct at :1127)

Gen is `#[derive(Hash)]`. Any new or reshaped field needs a hash opt-out, so per-seat versions must follow the `HashSilent`/manual-Hash pattern.

| Field | Def | Meaning | Class | Note | Refs |
|---|---|---|---|---|---|
| `mc2_pinned` / `mc1_pinned` | :1200/:1206 | the human record slot mirrored below World | **P** | `human_rec()` (:6705) = `max` of the two. Becomes the seat→record map | 18/0/0, 10/0/0 |
| `human_seat` | :1238 | the human's seat (LOCAL index) | **P** | `.human_seat` 53, `human_seat()` 43 calls | 53/0/1 |
| `human_tag` | :1244 | the human's owner tag (= record slot) | **P** | **`.human_tag.0` ≈ 480 sites**: every "is it the human" test | **480**/8/0 |
| `player_mail` | :1269 | fallback mailbox (no record) | **R** | already superseded by `pmail()` → record `mail`. 5 raw sites; `pmail()` 26 + `pmail_mut()` 28 | 5/0/0 |
| `player_damage` | :1271 | invincible-mode damage total | **P** (dev) | | 4/0/0 |
| `player_knock` | :1284 | knock/buffet (Type_160 v_24/v_22) | **T** | §6. Retail wizext ⇒ P-shaped | 45/0/0 |
| `mc1_buffet_post` | :1295 | kraken buffet on a RIVAL | **G** | already target-keyed | 4/0/0 |
| `player_spin` | :1308 | whirlwind heading delta | **T** | §6 | 2/0/0 |
| `player_whirl` | :1323 | whirlwind pose seizure | **T** | §6 | 10/0/0 |
| `ww_walk_published` | :1335 | within-walk "whirl already published" | **T** (helper) | must be per seat alongside `player_whirl` | 3/0/0 |
| `player_hurl` | :1359 | doomsday hurl beam | **T** | §6 | 6/0/0 |
| `player_flood_pull` | :1376 | quake/flood z pull + accumulated shove | **T** | §6 | 17/0/0 |
| `player_deflect_debit` | :1383 | rebound mana quarters owed | **T** | §6 | 6/0/0 |
| `mc2_debuffs` | :1390 | pending slow/stun web hits | **T** | §6 | 16/0/0 |
| `player_aggro` | :1420 | village-aggro timer (+528) | **P** | the rival twin `rival_wanted[8]` (:1428) is already per slot; merge into one `[i16;8]` | 52/0/0 |
| `player_invisible` | :1431 | cloak mirror for mob gates | **R** | `flags & 0x20` | 31/0/0 |
| `player_ghost` | :1436 | ghost echo | **G**/P | config echo | 5/0/0 |
| `player_vel` | :1443 | human displacement last tick | **R** | derivable from record pose − prev pose, per seat | 4/0/0 |
| `human_roll_0x20` | :1487 | MC2 @0x20 target-yaw residue | **R** | MC2 record `f32` (@0x20). The mirror leaves MC2 `f32` untouched, so it could live there | 7/0/0 |
| `player_rebound` | :1490 | rebound mirror | **R** | `flags & 0x8000` | 12/0/0 |
| `player_chain` | :1493 | virtual tile-chain seat | **R** | record `next20`/`prev22` once linked (`human_seat_at`, :6719). Fallback only | 31/0/0 |
| `kills` / `shots` / `hits` | :1496-1498 | Type_160 +359/+343/+347 | **P** | | 18/0/7, 24/0/3, 18/0/3 ⚠noisy |
| `stats` | :1503 | end-of-level ledger | **G** | needs per-human sections for an MP stats screen | 39/0/10 ⚠noisy |
| `player_danger` | :1512 | danger-music countdown (v_46) | **P** | | 34/0/0 |
| `banked_houses` | :1518 | human house tally (wizext +308) | **P** | rival twin `rival_banked_houses` (:1530) is per slot; merge | 24/0/2 |
| `castle_alert` / `player_alert` / `balloon_alert` | :1535/:1540/:1548 | HUD alert bytes Type_160+391..393 | **P** | per player block | 13/0/4, 15/0/4, 14/0/4 |
| `pal_flash` | :1551 | palette flash | **G** (local viewer) | armed only when owner == local player (§7) | 9/0/2 |
| `mc2_player_drain` | :1603 | m26 leech tally on the human | **P** | dead tally ("legacy tally, no reader", mc2/roster.rs:6467) | 5/0/0 |
| `mc2_scrolls` | :1615 | scroll tally | **P** | | 2/0/0 |
| `mc2_spell_tokens` | :1631 | human SpellEnabled half-mask | **P** | | 14/0/0 |
| `mc2_cast_xp` / `mc2_castle_lock_mail` / `mc2_steal_mail` | :1637/:1655/:1660 | owner-keyed mail | **G** | already carry owner id/slot; the drains assume the single human book | 26/0/0, 6/0/0, 7/0/0 |
| `mc2_rebound_precise` | :1748 | MC2 rebound tier (byte0xc[0] \|= 0x10) | **R** | record byte[0] & 0x10. ⚠ port `flags & 0x10` is the chain/reap bit, so the mapping needs care | 6/0/0 |
| `mc2_mobilize` / `mc2_slow` | :1787/:1807 | echo of Mc2Ext paralyze/slow | **P** (echo) | echo of that seat's `Mc2Ext.mobilize`/`move_speed` (retail record wizext +334/+332) | 14/0/0, 12/0/0 |
| (already per-slot) `castle_reg`, `rival_ents`, `rival_wanted`, `mc2_wanted`, `mc2_life_scale` | :1411/:1397/:1428/:1742/:1414 | | **G** | per-seat arrays/maps. `rival_ents[0]` = human, unused | — |

Helpers that encode "the ONE human" (logic, not data, but they set the stage-3 surface): `human_rec()` 27 calls,
`chain_human()` 20, `human_rides_chain()` 19, `human_seat_at()` 4, `human_mail_home()` 5, `pmail()/pmail_mut()` 54,
`World::player_castle()` (world.rs:15632, filters `id24 == human_tag`), `Gen::player_overlap(i, ctx)` (mc1/combat.rs:984),
`Gen::snd_player` (features.rs:6148).

---

## 4. `MobCtx` (mc1/mobs.rs:289)

The doc comment calls it "the wizard list of the original, reduced to the one human player". Built in production at
engine/world.rs:8907 (`tick_inner`, from the `player: PlayerPose` arg + `self.player.mana/mana_max/life`). It is
re-published mid-walk by `adopt_walk_pose` (:7889), which also re-stamps `human_pose`, relinks `player_chain` and calls
`mirror_human_record`. The wake variant is at :9296/:9312, with px/py from `mc2_wake_pose` (:8438). The load-time MC2
probe (:19518) uses the **first authored start marker**. 7 further non-test literals live in world.rs tests and module tests.

| Field | Def | Meaning | Class | Record home | `ctx.field` refs |
|---|---|---|---|---|---|
| `px`/`py`/`pz` | :290-292 | human pose (mid-walk phase) | **R** | record x/y/z | 116 / 115 / 74 |
| `pyaw` | :296 | human heading | **R** | `f30` | 21 |
| `pmana` | :299 | purse | **R** | `f140` | 6 |
| `pmana_max` | :305 | ceiling | **R** | `f136` | 4 |
| `pdead` | :311 | live "dead or falling" (life < 0) | **R** | `act_life < 0` | 43 |
| `pdead_top` | :316 | tick-top pdead | **R** | record membership at the chain build | 13 |
| `strict`/`patches`/`mc2_turn` | :321/:325/:334 | config / turn | **G** | | — |

The key fact for stage 3: all ~390 `ctx.p*` reads assume one target. With N humans each reader has to either walk the
class-3 chain and meet each human at his record (the stage-2 `chain_human` route) or take a seat argument. The record
already holds every one of these bytes once it is mirrored at the walk slot.

---

## 5. Flight state in `Simulation` (lib.rs:398) + `flight.rs`

`Simulation::step(&mut self, input: &FlightInput)` (lib.rs:723) → `World::tick(player: PlayerPose, cmd: PlayerCommand)`
(world.rs:8126) / `tick_inner(player, cmd, drive: Option<&mut FlightDrive>, post)` (:8455). `FlightDrive` (world.rs:661)
borrows `&mut Mc1State` + `Mc1Input` + `Mc2Drive{ext, accel_was_active}` + the `CarpetPropel` kernel. All of it is one human.

| Datum | Def | Meaning | Class | Note | Refs |
|---|---|---|---|---|---|
| `Simulation::tick` | lib.rs:401 | tick counter | **G** | | — |
| `flyer: Flyer` | lib.rs:402 | float pose/vel for renderer/camera | **P** | per seat (the local viewer reads its own) | 204/44/19 |
| `thrust_model` / `altitude_model` | :405/:406 | control tier | **P** (or G by MP rule) | per-human preference | 37/10/10, 17/4/10 |
| `carpet: Mc1State` | :410 | authoritative integer carpet | **P** | fields below | 87/15/7 |
| `carpet_mc2: Mc2Ext` | :415 | MC2 carpet channels | **P** | | 5/0/5 |
| `accel_was_active` | :418 | Accelerate falling-edge latch | **P** | | 7/0/1 |
| `aim_lead` / `turn_rate` | :425/:426 | enhanced chase steering | **P** | | 21/0/0, 10/0/0 |
| `lift_desired` | :431 | enhanced desired altitude | **P** | | 17/0/0 |
| `weightless` | :437 | cheat | **G**/P | config | 4/0/20 |
| `broll: BarrelRoll` | :441 | MC2 barrel-roll driver | **P** | reads `FlightInput.raw_dx` (local mouse) | 6/0/0 |
| `terrain_height`, `world` | :444/:448 | | **G** | | — |
| `FlightInput` | lib.rs:161 | one human's input | **P** | becomes `&[FlightInput]` | — |
| `PlayerPose` / `PlayerCommand` | world.rs:557 / :~600 | pose + command handed to the world | **P** | 311/108/19 hits for `PlayerPose` | |

`Mc1State` (flight.rs:85):

| Field | Line | Class | Record home |
|---|---|---|---|
| `x,y,z` | :88-91 | **R** | record x/y/z |
| `yaw` | :93 | **R** | `f30` |
| `roll_f`, `pitch_f` | :96-97 | **P** | Type_160 +327/+329 (wizext) |
| `aim_pitch` | :101 | **R** | `f32` |
| `eff_pitch` | :105 | **P** | mover scratch |
| `act_speed` | :108 | **R** | `f126` (+126) |
| `tgt_speed`, `strafe` | :109/:111 | **P** | Type_160 v_12 / v_16 |
| `tick_ctr` | :114 | **R** | `f63` |
| `rand` | :115 | **R** | entity `rand` (+4) |

`Mc2Ext` (flight.rs:620): `move_speed`, `move_speed_ctr`, `mobilize`, `mobilize_ctr`, `water_ctr`, `nudge_latch`,
`row` = **P** (wizext +332..336, +610, +609). `add` (:637) = **T**, an xAdd mailbox with no external writer today
(only the mover consumes and clears it, flight.rs:1137-1140). `whirl_bumps` (:655) and `flood_spin` (:660) = **T**
drain slots for `player_whirl.bumps` / `player_flood_pull.spin`.

---

## 6. Transport channels (T): writer → drain

All of them have to become per-seat, or target a record slot. The **writer** column is the world walker and the
**drain** column is the human's dispatch. Every drain sits in `step_player_flight` (MC1, world.rs ~7600-7740),
`step_player_flight_mc2` (~7220-7470), `mc2_carpet_dispatch` (~5660-5900), `tick_inner` tail, or `Simulation::step`.

| Channel | Def | Writers (fn @ file:line) | Drain (fn @ file:line) | Note |
|---|---|---|---|---|
| `Gen::player_knock` (dir, mag) | features.rs:1284 | `mob_chase` kraken buffet mc1/mobs.rs:1721; `flood_shove` mc2/flood.rs:828; `mc2_pyramid_do_summon` mc2/doomsday.rs:1700; `mc2_debuff_stamp_tick` mc2/proj.rs:1141/1143; `mc2_whirlwind_lift` mc2/tail.rs:2689; `player_mail_block` world.rs:6958; `apply_player_damage` :12073; `tick_inner` (duel pull) :9004 | `take_knock_step` world.rs:24058 (clamp ±128, −4/tick decay), called from `step_player_flight` :7731 and `step_player_flight_mc2` :7466; cleared in `mc2_carpet_dispatch` :5698/:5700, `step_player_flight_mc2` :7223/:7225, `mc1_mortality_pass` :12234, respawns :12644/:13300 | retail = wizext v_24/v_22, so a per-seat register rather than a pure mailbox; decays across ticks |
| `Gen::player_spin` | features.rs:1308 | `mc2_whirlwind_lift` mc2/tail.rs:2706 | `take_player_spin` world.rs:24078 ← `Simulation::mc2_walk_prelude` lib.rs:1335 | no decay |
| `Gen::player_whirl` (+`ww_walk_published`) | features.rs:1323/:1335 | `mc2_whirlwind_lift` mc2/tail.rs:2557/2670 (additive across funnels via `ww_walk_published`) | `take_player_whirl` world.rs:24150 ← `step_player_flight_mc2` :7415 and `tick_inner` :10701 (pinned/enhanced arm); `bumps` → `Mc2Ext.whirl_bumps` | can survive a tick boundary (highest funnel slot) |
| `Gen::player_hurl` | features.rs:1359 | `mc2_pyramid_do_summon` mc2/doomsday.rs:1711 | `take_player_hurl` world.rs:24105 ← `step_player_flight_mc2` :7367, `tick_inner` :10638; also read by `mc2_hurl_walk_pose` :24315; reset each tick at :8902 | one-turn life |
| `Gen::player_flood_pull` | features.rs:1376 | `flood_shove` / `flood_shove_human` mc2/flood.rs:844/819/881 | `take_player_flood_pull` world.rs:24115 ← `step_player_flight_mc2` :7377; `tick_inner` :10666-10674 (fresh adopt); `mc2_flood_walk_pose` :24358; reset at :8906; `spin` → `Mc2Ext.flood_spin` | accumulates dx/dy across visits |
| `Gen::player_deflect_debit` | features.rs:1383 | MC1 `proj_move_and_hit` mc1/combat.rs:6637; MC2 `mc2_rebound_deflect` mc2/proj.rs:878 | `mc1_wizard_pass` world.rs:6419 (pre-step); `tick_inner` tail :11157 (post-carpet leftovers) | purse read via `ctx.pmana − debit` |
| `Gen::mc2_debuffs` (slow, stun) | features.rs:1390 | `mc2_debuff_stamp_tick` mc2/proj.rs:1147/1192 | `take_mc2_debuffs` world.rs:24536 ← `step_player_flight_mc2` :7468 → `Mc2Ext` | |
| `Gen::mc2_steal_mail` | features.rs:1660 | `m26_tick` mc2/roster.rs:6496 | `tick_inner` :10607 and :11225 → `mc2_book` | victim implicitly = the human book |
| `Gen::mc2_player_drain` | features.rs:1603 | `m26_tick` mc2/roster.rs:6467 | none ("legacy tally, no reader") | dead channel |
| `World::pending_teleport` (+`_slot`) | world.rs:962/:973 | `portal_tick` :23525; `mc2_portal_tick` :23639; `cast_teleport` :15265/15280/15298; `mc2_cast_teleport` :15383; `mc2_teleport_to` :15435; `mc2_teleport_random` :15598; `tick_inner` :11004 | `take_teleport` :23669 ← `Simulation::step` lib.rs:1154; in-walk reads `step_player_flight(_mc2)` :7660/:7287, `mc2_carpet_dispatch` :5812, `warp_peek_*` :6756/:6788 | portal writers test THE human's pose |
| `World::pending_speed_zero` | world.rs:980 | `cast_teleport` :15306; `mc2_cast_teleport` :15416; `manifestation_tick` :18244; `mc2_cast_expire` mc2/cast.rs:4687 | `take_speed_zero` :23677 ← `step_player_flight` :7667, `step_player_flight_mc2` :7294, `Simulation::step` lib.rs:1189; tail clear :11546 | |
| `World::pending_speed_base` | world.rs:988 | `manifestation_tick` :18125/:18149; `mc2_manifestation_tick` mc2/cast.rs:3944/3988 | `step_player_flight` :7640, `step_player_flight_mc2` :7263, `take_speed_base` :18563 ← `Simulation::step` lib.rs:1087; tail clear :11545 | the token's owner is implicitly the human |
| `World::pending_respawn` | world.rs:1238 | `player_respawn` :12590; `mc2_player_respawn` :13260 | `take_respawn` :18910 ← `Simulation::step` lib.rs:1106 and `mc2_carpet_dispatch` :5887; peeked by `mc2_wake_pose` :8443, `tick_inner` :8971 | |
| `World::mc2_end_pending` | world.rs:1289 | `mc2_switch_tick` :22535 | `mc2_carpet_dispatch` :5669 → `mc2_endseq` | |
| `Mc2Ext::add` | flight.rs:637 | none | mover flight.rs:1137-1140 | dormant |

---

## 7. LOCAL / SEAT-0 retail leaks (from `multiplayer-step1-seat-k-research-2026-10-10.md`)

Keying today: **single** = reads the one human (`human_tag` / `human_pose` / `ctx` / `self.player` / `mc2_book`), so it is
seat-correct for 1 human at any seat but not N-ready. **seat** = `human_seat`. **0** = literal seat 0.

| # | Retail leak | Port site | Keyed to | Note |
|---|---|---|---|---|
| 1 | MC1 wake pass `sub_54F00` (SP only) | `Gen::mob_awake_pass` mc1/mobs.rs:4560, gate at :4580 (`dist2_sq(e, ctx.px/py)`); called world.rs:9328 with `wake_ctx` (:9296) | single | Retail MP skips the pass (memory ruling: MP = all awake). The port has no MP switch yet; the `awake_range 0` option exists |
| 2 | MC1 total-mana seed `sub_48230` (u32_188 = local human's u32_322) | `World::recompute_mana` world.rs:16115, seed :16161 | constant | seeds `WIZARD_BASE_MANA` (1000) / MC2 1 — not keyed to anyone. The total lives on `Player::world_mana` (misplaced G) |
| 3 | MC1 `sub_59B80` win trigger reads `str_13323[0]` | trigger state 4, world.rs:22412 (`self.completed && self.player_castle_bound()`) | single | `completed` / `win_streak` / `castle_reg` per human. Retail reads block 0 hardwired, so a seat-k human's latch is ignored in retail. The port reads the human's, i.e. it is already "fixed" |
| 4 | MC1 cheat heal `actLife = var_u16_8` | engine/world/cheats.rs:107-108 `self.player.life = PLAYER_LIFE_MAX` | single | Discrepancy: the port writes FULL life (comment ":48993 — actLife = <full>"); the memory says retail writes the local index. Not re-checked against remc1 |
| 5 | MC2 SP objectives seat 0 (EF:40980) | `objective_mc2` world.rs:20246: type 0 uses `player_castle()`, `g.banked_houses`, `player.world_mana` (:20365); type 5 `human_pose` (:20378); type 3 **`color != 0`** guard (world.rs:20423); type 8 checks only `mc2_rivals` | single, plus **0** for type 3 | `color != 0` is a literal seat-0 assumption (target color 0 = the human); wrong at seat k. MP has no objectives (ruling) |
| 6 | MC2 cave-drip seat 0 (EF:40758) | `tick_inner` world.rs:9348 (probe from `player`/`human_pose_prev`/`human_yaw_prev`, 2560 ahead) | single | the cave ambient sound (world.rs:~23882) tags `human_tag` |
| 7 | MC2 doomsday boss target | mc2/doomsday.rs: `f146 = human_tag` :762, `id24 = human_tag` :866/:972, `mc2_danger_poke(human_tag)` :671, `player_castle()` :1064/:1185, `ctx.pdead` gate :1559, hurl/knock :1700/:1711 | single | |
| 8 | MC2 scroll XP | `mc2_class14_tick` scroll collect world.rs:21640-21656 (`player_overlap` + `mc2_award_xp(human_tag, s, 4)` at :21656), `Gen::mc2_scrolls` | single | |
| 9 | MC2 spell level-up `sub_6DB50` / `sub_6D8B0` | `World::mc2_award_xp` mc2/cast.rs:2528 (`owner != human_tag` ⇒ return), `mc2_relevel` mc2/cast.rs:2457 on the single `mc2_book` | single | needs a per-seat book keyed by owner tag |
| 10 | MC2 rain XP flood (`sub_32CF0`, LOCAL player) | `mc2_summit91_tick` mc2/morph.rs:883 → `mc2_cast_xp.push((human_tag, s, amt))` | single | retail pays the LOCAL player only, which would desync in retail MP |
| 11 | Kill credit | MC2 rival death mc2/rivals.rs:10221-10223 **`if k == 0 { g.kills += 1 }`**; MC1 mc1/rivals.rs:6540 **`k == 0`** (kill-switch arm only); `kill_tally` human write world.rs:13088 uses `human_seat`; creature kills mc1/mobs.rs:3855 / mc2/mobs.rs:2163 / mc2/roster.rs:6040 key on `human_tag` | **0** (rival→`g.kills`), seat (tally), single (creatures) | the MC2 `k == 0` is a live seat-0 hardwire: at seat k the human's rival kills go uncounted and seat-0 AI kills are credited to the human |
| 12 | Fly-assistant + barrel-roll local mouse reads | fly assistant is app-side: mgc-app/src/config.rs:743/785 (auto-center of the virtual stick → `FlightInput.stick_*`); barrel roll `Simulation::broll` lib.rs:441, `BarrelRoll::tick(bank, input.raw_dx)` flight.rs:1306, called lib.rs:1031 | single (per FlightInput) | fine as per-seat input once `step(&[FlightInput])` |
| 13 | Hidden latch `byte_0xc & 1` (EF:55832) | `mc2_awake_pass` mc2/mobs.rs:6551, hidden skip :6664 (`flags & 1`), proximity :6681 (`ctx.px/py`) | bit = per-entity; proximity = single | EF:55832 is `sub_68C70` (remc2 EventsFunctions.cpp): the `byte[0]&1` test is on the CREATURE, and the LOCAL read is `array_0x2BDE[LevelIndex].playerIndex`'s position. The leak is the pose, not the bit |
| 14 | `paletteSubMod` end drift | `mc2_end_tick` world.rs:22649 (fade arm ~22815-22831) (fade/creep race modelled as constant one-tick win; `end_fade` :22893) | single (`mc2_endseq`) | retail's fade is a LOCAL palette clock driving sim creep, so it would desync in MP; MP has no level end |
| 15 | Book carry → LevelIndex | app: mgc-app/src/lib.rs:1254-1258 (allowed mask = `wizards[human_seat]`), `campaign_carry` → `World::grant_level_book` world.rs:14510 / `mc2_grant_start_book` mc2/cast.rs:5874 / `mc2_install_ring_carry` mc2/cast.rs | seat (mask) / single (book) | |

Extra seat-0 / local-keyed spots found during the sweep (not on the memory list):
- `World::mc2_apply_events` world.rs:19505-19518: the load probe ctx uses `start_markers.iter().flatten().next()`, the FIRST authored marker, not `start_markers[human_seat]` (**0-ish**).
- `pal_flash.arm(3)` mc1/combat.rs:7693 and `snd_player` gate on `own == human_tag` ("local player", retail `sub_44BE0` slot compare). Presentation, but local-keyed.
- conformance shadow export hardwires `wiz_charge[0]` (engine/world/conformance.rs:1539, :1682) and `rival_ents[0] = 0` (:847). Conformance is always seat 0, so harmless for now.
- mc1/rivals.rs:1648 falls back to `start_markers[0]` (kill-switch arm only); :1662 seat-0 rival sprite 44 is intended.
- `hate_team_of_tag` default team and `owner_team(..).unwrap_or(0)` are kept literal 0 BY RULING (memory).
- `Player::lost` / `pending_restart`: SP-only castle-less restart, keyed to the single human.

---

## 8. Statics / process-global state touching the human

- `crate::DEBUG_TICK` (lib.rs:37): debug only, **G**.
- `mc2::tail::WHIRL_CRANKS` (mc2/tail.rs:408): replay-harness telemetry counting whirlwind cranks handed to THE human carpet; must become per-seat or seat-filtered if the MC2 replay driver ever runs N humans. Not read by sim logic.
- `TAKE_VOLCANO_GUARD` (features.rs:110): per-process take flag, **G**.
- About 300 other `OnceLock<bool>` env kill switches, including the stage-1/2 switches `MGC_NO_HUMAN_SLOT_TAG`, `MGC_NO_HUMAN_RECORD_MIRROR`, `MGC_NO_HUMAN_MAIL_IN_RECORD`, `human_seat_at_record`, `human_area_at_record`, `human_on_wiz_chain`. These are **G**, but every one hard-codes the one-human arms they A/B.
- App inputs into the sim, all single-human today: `FlightInput` (local input mapper, virtual stick, fly assistant, strafe-edge barrel roll); `set_mc1_hud_drawn` (local view mode); `set_invincible/ghost/inert/dev_spells`, `weightless`, `set_thrust_model/altitude_model` (local config); `--seat` → `human_seat` (mgc-app/src/lib.rs:1081-1258, `player_start_seat` lib.rs:1563). The app's rival spawn and book mask are already keyed by `human_seat`.

---

## 9. Size estimate

Distinct data items inventoried (this report's rows; transport helpers and already-per-seat arrays included):

| Class | Count | Biggest |
|---|---|---|
| **P** per-seat | ~86 (Player 20, World 21, Gen 20, Simulation 13, Mc1State 5, Mc2Ext 7) | `mc2_book` (268), `Player` P-fields (≈560 combined refs), `human_tag` (480), `flyer` (204), `Mc1State` P-fields, `human_seat` (96), `carpet slot` regs (105), `player_aggro` (52), `mc1_acq` (62) |
| **R** record-derivable | ~43 (Player 11, World 10, Gen 7, MobCtx 8, Mc1State 7) | `human_pose` (123), `player.life`/`state`/`mana` (≈400), `ctx.p*` (≈390), `Mc1State x/y/z/yaw` (≈450 incl. non-human `s.` hits ⚠noisy), `player_chain` (31), `player_invisible` (31) |
| **G** global | ~37 | `world_mana`, `start_markers`, `kill_tally`, `wiz_charge`, `castle_reg`, config cheats, presentation |
| **T** transport | 15 (+3 Mc2Ext drain slots) | `player_knock` (45, also a decaying register), `pending_teleport` (33), `pending_speed_base` (21), `player_flood_pull` (17) |

Rough site counts:
- **Identity of "the human"**: `human_tag.0` ≈480 + `human_rec/chain_human/human_rides_chain/human_seat_at/pmail*` ≈130 + `human_seat` ≈96 ≈ **700 sites**, mostly mechanical (`== tag` → "is a human tag" / owner→seat lookup).
- **`self.player.*`**: ≈1030 sites. The ≈11 R fields can be served by reads/writes through the record (`ent[rec].f140`, …), which is mechanical once the record is authoritative. The ≈20 P fields need a `players[seat]` block.
- **MobCtx `p*`**: ≈390 reads. Each becomes either a chain-walk meet (stage-2 pattern) or a seat lookup. This is the highest-semantic-risk group, because phase (mid-walk republish vs tick-top) is load-bearing.
- **Flight (`Simulation` + `FlightDrive`)**: ≈60 struct-level sites in lib.rs + world.rs step/dispatch, with `PlayerPose` 311 sites (mostly tests: 108 in sim tests).
- **Transport channels**: ≈180 sites total; ~40 writer sites need to pick a target seat (most writers already have the victim record in hand).
- **mgc-conform / app**: `.player.*` 9, `human_*` ~20, plus the conform shadow seat-0 hardwires. Small.

Order of magnitude: **≈2,300–2,500 touched sites** in mgc-sim (≈60% mechanical renames to a per-seat block or record field;
≈400–500 needing real thought: MobCtx phase readers, transport writer targeting, `mc2_book`/XP owner routing,
objectives/kill credit). This agrees with the 10-10 estimate (≈1000+ with 150–200 real logic) once tests and the
`Player` block are counted.

### Biggest risks
1. **MobCtx phase**: `px/py/pz` are re-published mid-walk at the carpet's slot (`adopt_walk_pose`), `pdead` vs `pdead_top`, and `mc2_wake_pose` peeks the pending respawn. N humans means N walk-slot adoptions in slot order. Getting the phase wrong moves fixtures silently.
2. **`player_knock` is a register, not a mailbox** (decays −4/tick, clamped, cleared by death/respawn) and has 8 writer sites in 6 files. `player_whirl` can survive a tick boundary and accumulates within a walk (`ww_walk_published`).
3. **Hash/snapshot shape**: `Player` has a manual full-destructure Hash with transparent-at-pristine rules; `Gen` derives Hash. Moving to per-seat blocks must keep seat-0 output byte-identical, or the goldens re-pin by design.
4. **Live seat-0 hardwires**: mc2/rivals.rs:10222 `k == 0` kill credit and world.rs:20423 objective `color != 0` are wrong at seat k today. `mc2_apply_events` uses the first marker.
5. **`mc2_book` (268 refs)** + `mc2_award_xp`'s `owner != human_tag` guard + the scroll / rain floods: the XP economy assumes one book; the rain flood is LOCAL-only in retail.
6. **Records not yet mirrored but R-classified** (shield 0x4000, killer `f38`, shrine 0x1000, `mc2_rebound_precise` byte[0]&0x10 vs the port's chain/reap bit 0x10, `human_roll_0x20` on MC2 `f32`, `mc2_carpet_stall` byte[1]&8): bit-mapping collisions need checking before the record becomes authoritative.

---

## Slice 5c status (10-14) — every seat is a first-class human

- **Walkers read humans off their records.** The `MobCtx` `p*` lanes are no longer "the human": every walker reads
  the seat it means through `Gen::seat_at / seat_yaw / seat_dead / seat_dead_top / seat_purse(ctx, hx)` (the ctx's own
  seat reads exactly as before; any other seat reads his record). Switch: `MGC_NO_HUMAN_ROSTER_READ`. The `p*` lanes
  survive only as (a) the record-less fallback of unit rigs and the pre-record load settle, and (b) two deliberate
  phase views a caller hands in: the carpet's own warp-peek probe (`regen_boost` → `player_overlap_at`) and the
  load settle's marker pose. The wake pre-pass takes every seat's reference pose (`*_awake_pass_refs`).
- **Seat resolution per site**: a walker holding a target resolves its seat (`hx_of_tag`); a scan judges every seat
  (off-roster seats ahead of the walk, on-roster seats at their own record via `hx_of_rec`); area writers, the quake
  and the whirlwind run their human arm per seat (`WwSeat`); `MailTarget::Player(Hx)` carries the seat. Retail's
  `Entities[playerIndex]` reads (doomsday pyramid, MC2 stage engine + speech ramp, level-start checkpoint, cave drip,
  hydra blast-sound listener) answer to `Gen::local_seat()` (seat 0).
- **The turn**: `World::tick_inner(&mut [SeatTurn])` — seat-only blocks loop the seats in order, global blocks run
  once, each seat's carpet dispatches at its own record's walk slot. `World::tick_flight_seats`,
  `Simulation::step_seats(&[FlightInput])` (one `FlightInput` per seat). `single_hx()` is retired.
- **Seating**: `World::add_human_seat(colour)` before `seat_mc1_wizards` / `seat_mc2_wizards`;
  `Simulation::with_world` sizes the flight tier to the world's seats.
- **Save**: `SNAPSHOT_VERSION` 30 — identity carries the seat count, every per-seat lane is written per seat.
- **Tests**: `crates/mgc-sim/tests/mp_two_seats.rs` (a second seat flies; determinism; one-seat `step_seats` ≡ `step`;
  two-seat save round trip; seat count is identity).
- **Open (not wired)**: the app is single-seat (`LOCAL_HX`) — there is no N-human input path yet (no networking, ruled
  CLI-only); MP-only retail rules (e.g. the scroll's 50-XP MP grant) are not modelled.

## Castle per seat (10-14 #3)

- **`World::player_castle()` is retired.** It returned the lowest-numbered live (3,2) owned by ANY human, so with two
  humans seat 1 read seat 0's castle. Every caller now names its seat: `World::seat_castle(hx)` (the per-seat pool
  scan) — at-castle regen probes, the demolish, the ladder-sync price, the respawn anchor, the castle-spell price and
  canSummon pool probe, `mc2_afford`, the lock release, the stage-0 objective row, autofire's pool gate and the
  `castle_scan` conformance lane. The register readers' kill-switch arms (`player_castle_bound`,
  `mc2_demolish_castle`, `mc2_teleport_castle`, `mc2_price_castle`) fall back to the SEAT's scan.
- **The castle-spell locks are per seat**: `castle_lock_active(hx)` / `castle_build_lives(hx)` (MC1) and
  `mc2_castle_lock_active(hx)` / `mc2_castle_ball_aloft(hx)` (MC2) test only that seat's ball, token and castle — one
  human's castle mid-transform no longer fizzles another's Create Castle.
- **The doomsday pyramid / citadel devour** measures the castle-build shot against `playerIndex`'s castle
  (`seat_castle(local_seat())`), as retail does.
- One seat: the seat's tag is the only human tag, so every scan is the same scan — corpus byte-identical. Unit test
  `each_seat_resolves_its_own_castle`.

## The MobCtx human lanes retired (10-14 #3)

- `MobCtx` now carries only `phx` (the seat the walk serves) and the world's rule flags (`strict`, `patches`,
  `mc2_turn`). The tick-top copy of each human — pose, heading, purse, ceiling, life sign and the tick-top death
  latch — lives in `Gen::seat_view[hx]` ([`SeatView`], hash-silent, not on the wire), seeded at the tick top and
  re-published at the same phase points as before (the carpet's own slot, the warp and respawn peeks, the below-carpet
  view, `ctx_from_record`). Every seat has one, so `seat_dead_top(hx)` no longer special-cases the ctx's seat.
- Readers go through `Gen::view(hx)` / `view_mut(hx)` (and `World::view` / `view_mut`). The record stays the
  authoritative source; the view is the record-less fallback, the switch arms' restore and the two deliberate phase
  views (carpet warp-peek probe, wake pre-pass reference).
- Test rigs: the ctx helpers take the `Gen` they serve and write its seat view (`ctx_at(&mut g, px, py, …)`).
- Gate: 122/122 identical; 4 kill switches (`MGC_NO_HUMAN_ROSTER_READ`, `_RECORD_MIRROR`, `_LIVE_PURSE`,
  `_CTX_FROM_RECORD`) × 4 takes byte-identical old vs new binary (`_RECORD_MIRROR` moves both MC1 takes off the
  default, so the view path is exercised); 1577 tests; 633/633.

## Retail MP-only rules — census (10-14, read-only decompile sweep)

MP flag: MC2 `setting_byte1_22 & 0x10` (41 `MULTIPLAYER_MODE` tests + raw `& 0x10` at EF:38959; remc2's own
post-2026-07 additions — `MatchScore*`, `DestroyPlayerCastle`, WINDY cheat, EF:31445/31455/37587/37629/37641/37707/
37741/37771/38286 — are NOT retail). MC1/HW `str_AE408->var_u8_0 & 0x10` (18 tests + `&0x110` at :52110/:54568),
identical in both binaries; local node = `str_AE400->var_u16_8`, seat count = `var_u16_10`.

**MC2 sim-affecting (SP arm → MP arm):**
1. EF:40374 awake pre-pass `sub_68BF0` → skipped (everything awake). Port: always runs.
2. EF:41446-55 scroll XP 4 + `sub_6DB50` → **50** + `sub_6DBD0` (local seat only). Port: 4 only.
3. EF:40981/54677/41196 objectives evaluate row 0 → rows 0..NumberOfPlayers-1.
4. EF:40841 `InitStages`: map stages → ONE stage type 8 (last wizard standing) for all 8 rows.
5. EF:38449 `sub_53160` non-local seats AI → none AI; seats ≥ node count never spawn (EF:37825 and the other
   count-bounded walkers).
6. EF:39688 NumberOfPlayers = map value → node count.
7. EF:38437 carried book copied in → skipped.
8. EF:38959 wizard spell row = own seat → row 0 for everyone.
9. EF:38988 human spell levels floored to the map's level (≤2), XP = xpos2+1.
10. EF:39016 starting spells from the carry → from the map's StartingSpells.
11. EF:58570 per-cast relevel `sub_6D9C0` (xpos1, every wizard) → `sub_6DAD0` (xpos2, +1 step, LOCAL wizard only).
12. EF:44277/44300 `sub_6DB50` relevel ↔ `sub_6DBD0` swap roles.
13. EF:40759 cave drip: every 8th turn, seat 0 → every tick, extra LCG step, seat = rand % NumberOfPlayers.
14. EF:37332 type-31 switch (hidden-level entrance) → never created.
15. EF:37928-41 castle-less respawn request: lost + level over → "banished": wizard killed, spell entities removed,
    seat inactive (level > 24: local `byte[2] |= 0x10`). Also leave/banish cases 2/0x1D (EF:37881/38069).
16. EF:40167/40182 disk checkpoint save/restore → none. EF:40132 scroll census → skipped.

**MC1/HW sim-affecting:** A :52324 awake pre-pass skipped · B :49153 no AI seats · C :49201 spell row 0 for all ·
D :49231 human book = map allowed ∧ pregrant (no carry) · E :51536 seat count = node count · F :52110 **no
mana-goal win** · G :48623 castle-less respawn → banished · H :48603/:48607 checkpoint keys ignored.

UI/net-only (not sim): XP bar thresholds, objective speech/box, mana-goal tick marks, colour/name permutation, pause /
speed / save-load / `r` cheat SP-only, fly assistant forced off, menu FMVs skipped.

⚠ Several MP arms are keyed to the LOCAL node (scroll XP, per-cast relevel, banish flag) — in retail each node
credits only itself; a one-sim port must decide whether to apply them per seat.
