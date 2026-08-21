//! MC2 class-15 CAST MACHINERY + SPELL-XP — the spell column.
//! Ported from the remc2 cast chain (`EF:` = EventsFunctions.cpp,
//! `L:` = Level.cpp cites).
//!
//! Shape: a learned spell IS a class-15 pool entity (the collected
//! jar, re-purposed). Casting is multi-tick: the gate (`sub_5F660`)
//! arms the manifestation's cast timer; the manifestation's EFFECT
//! state (strF0[3·model]) then fires every tick while armed — first
//! tick spawns (the `sub_6DCA0` projectile dispatch or the direct-
//! effect arm) and commits the mana; the timer counts down; expiry
//! applies a pending tier change.
//!
//! Manifestation entity field map (class-15, this module):
//! `word_0x2E_46` armed cast timer → f26 · `word_0x30_48` duration &
//! mana divisor → f28 · `word_0x2C_44` pending tier+1 → f44 ·
//! `byte_0x46_70` live tier → f71 · `subSpellIndex_0x2A_42` payload
//! → f30 · `manaRegen_0x88_136` upkeep → f136 · `maxMana_0x8C_140`
//! full cost → max_life · `mana_0x90_144` per-tick mana → f140 ·
//! `word_0x36_54` cooldown → f54 · `parentId_0x28_40` owner → id24.
//!
//! The per-player `str_611` block lives on [`Mc2Spellbook`] — the
//! human wizard is out-of-pool (chassis convention), so the arrays
//! hang off `World` instead of a `dword_0xA4_164x` record. Single-
//! player laws only: the MP `xpos2` ladder and `sub_6DAD0` are out
//! of scope until rivals cast MC2-natively.
//!
//! Notes:
//! - The mana commit (`sub_68DE0` EF:55569) stamps the full cost as
//!   a negative caster manaRegen; [`World::mana_debit`] is the same
//!   mechanism (MC1's :64936 negative-delta stamp) — the regen tick
//!   applies it next turn, clamped at 0.
//! - Direct-effect spells map onto the existing Player channels
//!   (shield/invisible/rebound/beyond-sight/heal/accelerate/
//!   teleport); per-spell numeric payloads (heal rate, boost factor)
//!   reuse the MC1 channel plumbing pending their own deep trace.
//! - The castle spell (2) routes to [`World::cast_castle`]; retail's
//!   `sub_69AB0` build-queue variant is OPEN; the MC2 mana ladder
//!   (L:1729-55) applies either way.

use crate::engine::features::{Gen, lcg32};
use crate::engine::world::{AimLock, LifeState, PlayerPose, World};
use crate::mc1::mobs::{MobCtx, PLAYER_TARGET};
use crate::mc2::spells::Mc2SubSpell;

/// Notification lives, in ticks (retail message-life `a3`): the
/// level-up path sets 200 (EF:44012), the change-spell toast 20
/// (EF:37926).
const NOTIFY_TICKS_LEVELUP: u16 = 200;
const NOTIFY_TICKS_SELECT: u16 = 20;
/// The plain-toast ink: retail draws it in the CLRD-0 code `0xF00` =
/// RGB(255,0,0) resolved to the nearest palette index (remc2
/// EF:22128). We carry the intended truecolor red.
const NOTIFY_RED: [u8; 3] = [255, 0, 0];

/// The 26-spell `str_611` subset for one wizard (spell-XP trace §0).
/// All arrays are keyed by spell index 0..25 (`spell_t`).
#[derive(Clone, Copy)]
pub(crate) struct Mc2Spellbook {
    /// `SpellsEnabled_0x333`: pool slot of the spell's class-15
    /// manifestation, 0 = not learned.
    pub(crate) ent: [u16; 26],
    /// `spellsExperience_0x2CB` — volatile (this-level) XP.
    pub(crate) xp_vol: [i32; 26],
    /// `SpellExperience_0x263` — banked (campaign-carried) XP.
    pub(crate) xp_bank: [i32; 26],
    /// `SpellLevels_0x41D` — derived level 0..2.
    pub(crate) levels: [u8; 26],
    /// `array_0x437` — the selected tier per spell (≤ level).
    pub(crate) sel: [u8; 26],
    /// `SpellIndexLeft/Right_0x451/0x453` — quick-slot bindings
    /// (spell index, -1 = none).
    pub(crate) left: i8,
    pub(crate) right: i8,
    /// `array_0x3B5` — per-spell cycle-ring membership: 0 = none,
    /// 1 = the LEFT-button ring, 2 = RIGHT. Written ONLY by the
    /// pane's SHIFT+click (cmd 0x26, a raw byte store EF:37951);
    /// the normal equip routes never touch it. Deliberately kept on
    /// spell LOSS (sub_69300 clears possession + the equip pointer,
    /// not this) — the cycle walk skips unpossessed members.
    pub(crate) ring: [u8; 26],
}

impl Default for Mc2Spellbook {
    fn default() -> Self {
        Mc2Spellbook {
            ent: [0; 26],
            xp_vol: [0; 26],
            xp_bank: [0; 26],
            levels: [0; 26],
            sel: [0; 26],
            left: -1,
            right: -1,
            ring: [0; 26],
        }
    }
}

impl Mc2Spellbook {
    /// Hash-transparency gate: a never-touched book hashes like the
    /// pre-field struct (the MC1 goldens hold across the layout
    /// change — the bldgprm/spells pattern).
    pub(crate) fn is_pristine(&self) -> bool {
        self.ent == [0; 26]
            && self.xp_vol == [0; 26]
            && self.xp_bank == [0; 26]
            && self.levels == [0; 26]
            && self.sel == [0; 26]
            && self.left == -1
            && self.right == -1
            && self.ring == [0; 26]
    }
}

/// Manual, NOT derived: `ring` is folded only when populated, behind
/// a field tag (the transparent-while-clear discipline) — every book
/// pinned before the ring existed feeds the identical byte stream.
/// Field order up to `right` must stay the old derive order.
impl std::hash::Hash for Mc2Spellbook {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Mc2Spellbook {
            ent,
            xp_vol,
            xp_bank,
            levels,
            sel,
            left,
            right,
            ring,
        } = self;
        ent.hash(h);
        xp_vol.hash(h);
        xp_bank.hash(h);
        levels.hash(h);
        sel.hash(h);
        left.hash(h);
        right.hash(h);
        if *ring != [0; 26] {
            h.write_u8(0xB5);
            ring.hash(h);
        }
    }
}

/// Read-only spell-book snapshot for the app (pane tiles, XP bars,
/// hand indicators).
#[derive(Clone, Copy)]
pub struct Mc2BookView {
    pub owned: [bool; 26],
    pub levels: [u8; 26],
    pub sel: [u8; 26],
    /// Effective XP (banked + volatile) per spell.
    pub xp: [i32; 26],
    /// The per-tier XP thresholds (`xpos1_E`, the single-player
    /// ladder) — the flyout's unlock-progress bar bounds
    /// (EF:22633-71).
    pub xpos: [[i32; 3]; 26],
    /// The SELECTED tier's cast cost (`GetSpellManaCost_6D710` —
    /// castle rides the upgrade ladder).
    pub cost: [u32; 26],
    /// EVERY tier's cast cost — the flyout's broke test recomputes
    /// `GetSpellManaCost` per tier (EF:22609), not per selection.
    pub cost_tier: [[u32; 3]; 26],
    /// Cast-in-progress (`word_0x2E_46` > 0) — the HUD hand-panel
    /// highlight (retail's burst-counter frame swap).
    pub armed: [bool; 26],
    /// Retail's HUD EXPIRY-BLINK eligibility (DrawSpellIcon_2E260
    /// GameUI.cpp:351-54): a flag-4 long-runner whose cast window
    /// (`word_0x2E_46`) is live inside its last 31 ticks. While set,
    /// retail SKIPS the whole panel (and the CTRL-pane cell,
    /// EF:22493-99) on odd turns (`colorIndex_121[1]` = Turn & 1).
    pub expiring: [bool; 26],
    /// Retail's `canSummon`/`canSubSummon` PER TIER (the pane
    /// grey-out, EF:22503-08 grid / EF:22602-08 flyout): the tier's
    /// `maxManaLimit_A` castle-pool prerequisite is zero, or the own
    /// castle's stored mana covers it. False = the dark box +
    /// ghosted icon (SPELL_ICON_PANEL2 + transparent draw). Hand
    /// mana is NOT part of THIS flag, and on the GRID and the HUD
    /// hand panels retail truly ignores it (a broke-but-eligible
    /// spell stays lit with an empty shot meter) — but the FLYOUT
    /// tile ADDITIONALLY keys on `mana / cost` per tier
    /// (EF:22609/:22618, player retail-verified 2026-08-21): the
    /// app combines this flag with `cost_tier` there. The grid keys
    /// on the SELECTED tier (`castable[s][sel[s]]`).
    pub castable: [[bool; 3]; 26],
    pub left: i8,
    pub right: i8,
    /// `array_0x3B5` cycle-ring membership (0/1=left/2=right).
    pub ring: [u8; 26],
}

/// One `sub_6DCA0` projectile arm (cast-path trace §2): the class-9
/// subtype to spawn, the impact (class, model), whether the tier's
/// `life_0x1A` charge byte rides along. The tier's `subSpellIndex_2`
/// payload ALWAYS rides — every effect-state skeleton copies it onto
/// the projectile (fireball EF:55864).
pub(crate) struct DispatchArm {
    pub(crate) subtype: u8,
    pub(crate) impact: (u8, u8),
    pub(crate) charge: bool,
}

/// Class-9 creator parameters (low-band trace + flyers trace Part 1):
/// (subtype, action, speed, maxLife, behavior row (always a real
/// str_D7BD6 index — a 255 would panic the BEHAVIOR lookup),
/// sprite). model = subtype throughout except 0x1C (model 28 rides
/// the fireball body). All creators: mana 50, no RNG.
const CREATORS: [(u8, u8, i16, u32, u8, u16); 19] = [
    (0, 0, 384, 21, 64, 340), // fireball (SummonFireball_4D2E0 EF:34729)
    // The BASIC possession bolt (`SummonManaPosession_4D3B0` EF:34764)
    // — tier `life_0x1A` 0 only, launched by `sub_69900` (EF:56039).
    // Same speed/life/row/sprite as its leveled (9,17) twin; the two
    // differ ONLY in action (1 vs 18) and in the ShiftRot fov factor
    // (5/2 vs 2 — see `mc2_spawn_cast_proj`).
    (1, 1, 384, 10, 61, 209),
    (2, 2, 384, 21, 60, 211),   // earthquake shot (sub_4D470 EF:34788)
    (3, 3, 384, 21, 60, 76),    // meteor shot (sub_4D500 EF:34810)
    (4, 4, 384, 21, 60, 210),   // volcano shot (sub_4D590 EF:34832)
    (5, 5, 384, 21, 60, 211),   // crater shot (sub_4D620 EF:34854)
    (8, 8, 384, 21, 63, 214),   // (sub_4D7D0 EF:34920)
    (9, 9, 384, 9, 63, 216),    // thunder bolt (sub_4D860 EF:34942)
    (12, 12, 384, 5, 60, 216),  // charged thunder (sub_4DA20 EF:35009)
    (17, 18, 384, 10, 61, 209), // possession (subtype 0x11, EF:35132)
    (22, 23, 384, 21, 60, 211), // gravity well (0x16, EF:35155)
    (23, 24, 384, 21, 60, 211), // tremor (0x17, EF:35199)
    (24, 25, 384, 20, 60, 281), // summon (0x18, EF:35221; maxLife &= 0xFC)
    (25, 26, 384, 10, 61, 321), // alliance (0x19, EF:35244)
    (26, 27, 384, 21, 60, 320), // whirlwind (0x1A, EF:35266)
    (28, 29, 384, 21, 64, 340), // charged fireball (0x1C, EF:34752)
    (29, 30, 384, 10, 60, 66),  // magic mine (0x1D, EF:35310)
    (30, 31, 384, 21, 60, 211), // cave-in (0x1E, EF:35288)
    (10, 10, 384, 21, 60, 18),  // castle ball (sub_4D900 EF:34965)
];

impl Gen {
    /// The shared class-9 creator body (low-band trace preamble):
    /// NewEvent + fields + `byte[0] &= 0xF7` + map link + life copy +
    /// sprite. Launch yaw/pitch are the launcher's job.
    pub(crate) fn mc2_spawn_cast_proj(
        &mut self,
        subtype: u8,
        x: u16,
        y: u16,
        z: i16,
    ) -> Option<usize> {
        let (_, action, speed, life, row, sprite) = *CREATORS.iter().find(|c| c.0 == subtype)?;
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            // Subtype 0x1C is the fireball body under model 28
            // (sub_4D380's override) — model = subtype otherwise.
            e.model65 = subtype;
            e.tick70 = action;
            e.f126 = speed;
            e.f128 = speed;
            e.f140 = 50;
            e.max_life = life;
            e.row156 = row;
            e.flags = (e.flags & !8) | super::proj::F_MC2PROJ;
            // The BASIC possession bolt is the one class-9 creator
            // that narrows `xtype_0x41_65` off the NewEvent −1
            // wildcard: `SummonManaPosession_4D3B0` stamps 10
            // (EF:34775; the leveled (9,17) `sub_4DDD0` does NOT).
            // Retail's own possession probe `sub_108B0` never reads
            // it — the lane only bites if the bolt ever runs the
            // generic `sub_10780` — so `mc2_flyer_tick` skips
            // `mc2_proj_filter` on the claim arm to keep it inert
            // exactly like retail (worm/building claims survive).
            if subtype == 1 {
                e.f66 = 10;
            }
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, sprite);
        // The POSSESSION pair alone re-boxes after the sprite set
        // (`SetEntityShiftRot_49EA0`): the basic (9,1) takes
        // `(2*pitch, 5*fov/2)` (EF:34781) and the leveled (9,17)
        // `(2*pitch, 2*fov)` (EF:35148). Sprite 209's row is
        // (speed_6 0, rotSpeed_8 150) → pitch/roll 0 either way, fov
        // 187 vs 150 — the z half-extent the claim probe and the cave
        // ceiling glide both read. Every other class-9 creator stops
        // at `SetEntityIndexAndRot_49CD0`.
        if matches!(subtype, 1 | 17) {
            let e = &self.ent[i];
            let shift = e.f80.wrapping_mul(2);
            let fov = if subtype == 1 {
                5 * e.f84 / 2
            } else {
                2 * e.f84
            };
            self.mc2_shift_rot(i, shift, fov);
        }
        Some(i)
    }

    /// Fool's Mana retaliation (`sub_36680` EF:26615), run from a
    /// (10,57) sphere's own tick while the ch1 claim latch
    /// (`word_0x68_104` → the ch1 mail SOURCE, @0x68) is set —
    /// mc1/combat.rs `ball_tick`. Retail field homes, all imported:
    /// parentId = id24 (@0x28), tier = f71 (@0x46), payload = f44
    /// (@0x2A), counter = f26 (@0x10). Returns true when the sphere is
    /// spent and must be consumed. The ONLY no-trap arm is
    /// `parentId == claimer` (EF:26623) — it clears the channel and the
    /// sphere lives on; there is no "is this a cast decoy" gate, which
    /// is why the AUTHORED ground spheres (parentId 0, tier 0) trap
    /// every possessor. Per tier: 0 → one fireball at the possessor,
    /// done; 1 → a fireball every other tick, up to 8, then done; 2/3 →
    /// one lightning bolt, then despawn after two ticks; >3 → nothing,
    /// ever (retail's fallthrough returns 0 and never clears the latch,
    /// so the sphere freezes claimed). The projectile homes the
    /// possessor (docs/spell-audit/fools-mana.md §2b).
    pub(crate) fn mc2_fools_retaliate(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let claimer = self.ent[i].mail[1].1;
        if self.ent[i].id24 == claimer {
            // EF:26623-27: the owner cannot be fooled by its own —
            // clear the channel (amount and source) and carry on.
            self.ent[i].mail[1] = (0, 0);
            return false;
        }
        let tier = self.ent[i].f71;
        match tier {
            0 => {
                self.mc2_fools_bolt(i, 0, (10, 0), claimer, ctx);
                // XP lands when the trap SPRINGS (EF:26636), not on
                // the cast.
                self.mc2_fools_award(i);
                true
            }
            1 => {
                let c = self.ent[i].f26;
                self.ent[i].f26 = c.wrapping_add(1);
                if c >= 8 {
                    self.mc2_fools_award(i); // after the 8th (EF:26646)
                    return true;
                }
                // The fireball fires when the POST-increment counter
                // is even = old counter ODD (EF:26648 `!(++c & 1)`).
                if c & 1 != 0 {
                    self.mc2_fools_bolt(i, 0, (10, 0), claimer, ctx);
                }
                false
            }
            2 | 3 => {
                let c = self.ent[i].f26;
                self.ent[i].f26 = c.wrapping_add(1);
                if c == 0 {
                    self.mc2_fools_bolt(i, 9, (10, 23), claimer, ctx);
                    return false;
                }
                // Despawn at old counter 2 (`v3+1 > 2`, EF:26661).
                let done = c > 1;
                if done {
                    self.mc2_fools_award(i); // on despawn (EF:26663)
                }
                done
            }
            // Tier > 3 falls out of `sub_36680` with v5 = 0 (EF:26665):
            // no trap, no transfer, and the latch is never cleared —
            // the sphere is claimed forever and never moves again.
            _ => false,
        }
    }

    /// `sub_6D8B0(parentId, 0x16, 1)` at the trap's SPEND points —
    /// via the XP mail (this is `Gen`; the book lives on `World`).
    /// parentId rides id24 (@0x28); an AUTHORED sphere carries its own
    /// slot there (retail: 0), so the level's bait credits nobody —
    /// which is what `sub_6D8B0(0, …)` does.
    fn mc2_fools_award(&mut self, i: usize) {
        let owner = self.ent[i].id24;
        if owner == PLAYER_TARGET {
            self.mc2_cast_xp.0.push((owner, 22, 1));
        }
    }

    /// Spawn one Fool's-Mana retaliation projectile from the sphere,
    /// homing the possessor: fireball (`sub_36770`, subtype 0, impact
    /// (10,0), sound 9) or thunder bolt (`sub_36850`, subtype 9, impact
    /// (10,23)). Owner = the trap's parentId (id24) so the flyer's
    /// autoaim never turns it on the caster; the tier's damage payload
    /// (`subSpellIndex_0x2A_42` → f44) rides onto the projectile's own
    /// f44 (EF:26691/26722). Retail's `sub_655C0` aims at the CLAIMER
    /// entity — the human wizard included (retail humans are in-pool);
    /// our out-of-pool human resolves through the ctx pose, the same
    /// sentinel resolution every creature attack aim uses
    /// ([`Gen::mc2_target`]). A reaped pool claimer falls back to the
    /// sphere's launch heading.
    fn mc2_fools_bolt(
        &mut self,
        i: usize,
        subtype: u8,
        impact: (u8, u8),
        claimer: u16,
        ctx: &MobCtx,
    ) {
        let (x, y, z, owner, payload, heading) = {
            let e = &self.ent[i];
            // The MUZZLE LIFT (`position.z += array_0x52_82.fov`,
            // EF:26688 fireball / EF:26718 lightning): the bolt leaves
            // from the TOP of the launcher's own box, not its origin.
            // `a1x` there is the SPHERE, so the fov is the sphere's —
            // exactly the same law shape as the possession cast's
            // `position.z += a2x->array_0x52_82.fov` (EF:56054 /
            // EF:55969), where the launcher is the wizard. mc2l24
            // t=1322: retail's fireball leaves at z=898 off a z=846
            // sphere with afov 42.
            //
            // The self-detonation this was deferred over is NOT a
            // probe-filter gap: retail's `sub_10780` (EF:3739) has no
            // launcher exclusion at all. What keeps the bolt off its
            // own sphere is (a) the tier-0 sphere is UNMAPPED and
            // class-zeroed at the end of its OWN tick — the entity
            // walk runs `sub_57F20` (Events.cpp:551, :5209:
            // `SetMapEntity_57E50` + `class = 0` + free-stack push)
            // the instant `DisableEntityDrawing04_57F10` latches
            // byte[1]&4 — and (b) retail probes ONCE, at the END of a
            // full 384-unit step (`sub_65C20` EF:63126-29: MoveEntity,
            // CopyEntityPosition, THEN `sub_10780`). Our soft kill
            // leaves the sphere linked until the tick-top reap, and
            // our anti-tunnel chord march probes sub-steps the retail
            // probe never visits — so the launcher is excluded here,
            // at the source, by owner identity: the bolt inherits the
            // sphere's `id24`, and `victim_scan`'s `c.id24 != id`
            // (retail's `a1x->id_0x1A_26 != v5x->id_0x1A_26`,
            // EF:3769) then drops it, together with the co-located
            // (10,0) consume poof once that inherits the owner too.
            let lift = e.f84 as i16;
            (
                e.x,
                e.y,
                e.z.wrapping_add(lift),
                e.id24,
                e.f44 as i32,
                e.f30,
            )
        };
        let Some(pr) = self.mc2_spawn_cast_proj(subtype, x, y, z) else {
            return;
        };
        // The fireball's water-spawn splash (EF:26690-95, inside the
        // spawn-success arm): a (10,5) splash + sound 27 when the
        // sphere sits on water.
        if subtype == 0 && self.cap_bit(x, y) == 1 {
            if let Some(s) = self.mc2_spawn_splash(x, y, z) {
                self.ent[s].id24 = owner;
                self.snd(27, s);
            }
        }
        let (yaw, pitch) = if claimer == PLAYER_TARGET {
            let (tx, ty, tz) = (ctx.px, ctx.py, ctx.pz);
            let yaw = Self::angle_between(x, y, tx, ty);
            let dh = Self::isqrt(Self::dist2_sq(x, y, tx, ty) as u32) as i32;
            (yaw, Self::pitch_toward(z, tz, dh))
        } else if (claimer as usize) < self.ent.len()
            && self.ent[claimer as usize].flags & 0x400 == 0
        {
            let t = &self.ent[claimer as usize];
            let (tx, ty, tz) = (t.x, t.y, t.z.wrapping_add(t.f78 as i16));
            let yaw = Self::angle_between(x, y, tx, ty);
            let dh = Self::isqrt(Self::dist2_sq(x, y, tx, ty) as u32) as i32;
            (yaw, Self::pitch_toward(z, tz, dh))
        } else {
            (heading, 0)
        };
        {
            let e = &mut self.ent[pr];
            e.id24 = owner;
            e.f68 = impact.0;
            e.f69 = impact.1;
            e.f44 = payload.clamp(0, u16::MAX as i32) as u16;
            e.f30 = yaw;
            e.f34 = yaw;
            e.f32 = pitch;
            e.f36 = pitch;
            e.f146 = claimer; // homing lock on the possessor
        }
        // The LIGHTNING bolt re-rows to the homing row 64 and stamps
        // the claimer's class/model as its xtype/xsubtype filter
        // (sub_36850 EF:26701-20); the fireball stamps none.
        if subtype == 9 {
            let (tc, tm) = if claimer == PLAYER_TARGET || claimer as usize >= self.ent.len() {
                (3, 0)
            } else {
                (
                    self.ent[claimer as usize].class64,
                    self.ent[claimer as usize].model65,
                )
            };
            let e = &mut self.ent[pr];
            e.row156 = 64;
            e.f66 = tc;
            e.f67 = tm;
        }
        if subtype == 0 {
            // Sound 9 rides the NEW fireball (EF:26689), not the
            // sphere.
            self.snd(9, pr);
        }
    }
}

impl World {
    // ---- SetSpell / mana cost / level law --------------------------------

    /// `GetSpellManaCost_6D710` (L:1714): the tier's `manaCost_6`;
    /// the castle spell (2) rescales to the upgrade ladder at the
    /// OWN castle's current entity level (L:1729-55 — the verbatim
    /// table, default rung 300M). The `byte_0x1BE_446` +3000 arm
    /// (castle-less surcharge, L:1723-26) is OPEN — field meaning
    /// untraced; omitted.
    pub(crate) fn mc2_spell_mana_cost(&self, spell: usize, tier: usize) -> i32 {
        let Some(row) = self.g.assets.spells.get(spell) else {
            return 0;
        };
        let base = row.tiers[tier.min(2)].mana_cost;
        if spell == 2 {
            // `GetSpellManaCost_6D710` (L:1714-85): the castle upgrade
            // cost is the OWN castle level's ladder rung times the
            // spell-LEVEL (tier) multiplier — this is why a fire/
            // lightning castle (tier 1/2) needs far more possessed-
            // but-uncollected mana than the pool can hold, so you
            // can't over-build (docs/spell-audit/castle-and-cost.md).
            // With no own castle, retail returns the tier's base
            // manaCost (L:1717-21). The `byte_0x1BE_446` +3000
            // rebuild surcharge is still OPEN (field untraced).
            let Some(c) = self.player_castle() else {
                return base;
            };
            const LADDER: [i64; 8] = [
                1000,
                10000,
                20000,
                40000,
                80000,
                160000,
                320000,
                300_000_000,
            ];
            let lvl = self.g.ent[c].f26.clamp(0, 7) as usize;
            let mut result = LADDER[lvl];
            if lvl < 7 {
                // The tier multiplier (L:1725-33): ×1.25 / ×1.5 via the
                // 320/256 · 384/256 fixed-point idiom (round toward
                // zero; the ladder rungs divide evenly). Level 7 (the
                // 300M cap) takes no multiply.
                result = match tier.min(2) {
                    1 => (result * 320) >> 8,
                    2 => (result * 384) >> 8,
                    _ => result,
                };
            }
            return result.clamp(0, i32::MAX as i64) as i32;
        }
        base
    }

    /// The MC2 per-tier spell name shown on the CTRL-pane hover and the
    /// spell-change toast (`SetSpellHelpPopupCoordinates_88D40` case 0 /
    /// EF:37925): resolve the LIVE `hint_text` lang index (post the
    /// Day/non-Day `level_init_patch`) to its retail `L2.TXT` string, so
    /// each tier reads as its own name ("Possession"/"Mana Magnet"/"Mana
    /// Lock"), not one generic label. Empty when mc2 spell data is absent
    /// (docs/spell-audit/spell-names.md).
    pub fn mc2_spell_name(&self, spell: usize, tier: usize) -> &'static str {
        let idx = self
            .g
            .assets
            .spells
            .get(spell)
            .map_or(0, |r| r.tiers[tier.min(2)].hint_text);
        super::spells::lang(idx)
    }

    /// The level-up banner text (`sub_6DC40_improve_ability` EF:44011):
    /// "Your ability to cast %s has improved." with the spell's UPPERCASE
    /// base name (lang 160+spell) substituted.
    pub fn mc2_relevel_message(&self, spell: usize) -> String {
        super::spells::lang(159).replace("%s", super::spells::lang(160 + spell as i16))
    }

    /// `SetSpell_6D5E0` (L:1505): wire the tier's SPELLS row into the
    /// manifestation. Mid-cast, the change is deferred (`word_0x2C_44
    /// = tier+1`, applied by `sub_6D880` when the timer expires).
    pub(crate) fn mc2_set_spell(&mut self, m: usize, tier: u8) {
        let spell = self.g.ent[m].model65 as usize;
        let Some(row) = self.g.assets.spells.get(spell) else {
            return;
        };
        let count = (row.byte_0 as i16).max(1);
        let t = (tier as i16).min(count - 1).max(0) as usize;
        if self.g.ent[m].f26 > 0 {
            self.g.ent[m].f44 = (t + 1) as u16;
            return;
        }
        let sub = row.tiers[t];
        let cost = self.mc2_spell_mana_cost(spell, t);
        let e = &mut self.g.ent[m];
        e.f71 = t as u8;
        e.f30 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
        e.f28 = sub.word_0x18.max(0) as u16;
        // `byte_0x3B_59 = (fontType_0x1B & 1) == 0` (L:1519) — THE
        // per-tier cadence flag: 1 = CLICK-to-fire, 0 = RAPID
        // (auto-repeat while held; in the CD table only fireball
        // tier 1 "Repeat Fireball" and lightning tier 0 —
        // docs/traces/mc2-cast-input.md §2).
        e.f59 = (sub.font_type & 1 == 0) as u8;
        e.f136 = sub.max_mana_limit;
        e.max_life = cost.max(0) as u32;
        e.f140 = if e.f28 != 0 {
            cost / e.f28 as i32
        } else {
            cost
        };
        if self.dev_spells {
            // Retail's OWN cheat flag (`OptionsSettingFlag_24 & 0x20`,
            // L:1531-35): no castle-upkeep gate, 1 mana per tick —
            // the dev-spells instrument mirrors it so upkeep spells
            // (Cave-In's 100k castle-pool gate) stay castable.
            let e = &mut self.g.ent[m];
            e.f136 = 0;
            e.f140 = 1;
        }
    }

    /// `sub_6D9C0` (EF:43873) — the single-player level law: level =
    /// the highest tier (scanning down from the row's tier count)
    /// whose `xpos1_E` the effective XP (banked + volatile) reaches.
    /// Also: the castle XP hard-cap at 7 (EF:43885), the selected-
    /// tier clamp, the notification (msg 159/160+idx → sound 61,
    /// `sub_6DC40_improve_ability` EF:44007), and the optional bank
    /// commit capped at tier-2's threshold.
    pub(crate) fn mc2_relevel(&mut self, spell: usize, bank: bool, notify: bool) {
        let Some(row) = self.g.assets.spells.get(spell).copied() else {
            return;
        };
        let owned = self.mc2_book.ent[spell] != 0;
        // Retail's v5 gate (EF:43876-78): `(array_0x3E9 ||
        // SpellsEnabled) && (isCaveLevel || spell != 25)` — the port
        // unifies grant + manifestation into `ent` (the OR collapses;
        // no path sets one without the other), so v5 = owned + the
        // CAVE gate: Cave-In (25) never notifies or banks on a
        // surface level. The LEVEL derive stays unconditional.
        let v5 = owned && (self.g.is_cave() || spell != 25);
        // Castle XP clamp (EF:43885-86; `setting_byte2_23` guard OPEN
        // — single-player campaign always clamps).
        if self.mc2_book.xp_vol[2] > 7 {
            self.mc2_book.xp_vol[2] = 7;
        }
        let xp = self.mc2_book.xp_vol[spell] + self.mc2_book.xp_bank[spell];
        let mut v6 = row.byte_0 as i32;
        loop {
            v6 -= 1;
            if v6 < 0 || xp >= row.tiers[(v6 as usize).min(2)].xpos1 {
                break;
            }
        }
        let lvl = v6.max(0) as u8;
        if lvl != self.mc2_book.levels[spell] {
            self.mc2_book.levels[spell] = lvl;
            if v5 && notify {
                // `sub_6DC40_improve_ability`: on-screen message
                // (string 159 + 160+idx) + sound 61.
                let msg = self.mc2_relevel_message(spell);
                self.set_notification(msg, NOTIFY_TICKS_LEVELUP, NOTIFY_RED);
                self.g.snd_player(61);
            }
        }
        // Under the all-spells instrument the selection cap is the
        // tier count, not the XP level — so a mid-play XP award must
        // not yank a dev-selected high tier back down (it would desync
        // the pane and the next commit).
        let sel_cap = if self.dev_spells {
            (row.byte_0 as i16 - 1).clamp(0, 255) as u8
        } else {
            self.mc2_book.levels[spell]
        };
        if self.mc2_book.sel[spell] > sel_cap {
            self.mc2_book.sel[spell] = sel_cap;
        }
        if v5 && bank {
            let cap = row.tiers[2].xpos1;
            self.mc2_book.xp_bank[spell] = xp.min(cap);
        }
    }

    /// `sub_6D8B0` (EF:58228) — the XP award: `amount` onto the
    /// volatile XP of the owner's spell. Retail's own guard is
    /// `class == 3 && model == 0` — the HUMAN wizard ONLY
    /// (EF:58240-41). Rival owners
    /// are a structural no-op: retail rivals have NO spell-XP
    /// progression — their tiers are the authored map levels for
    /// life, and the tier-down walk supplies the dynamics. The
    /// castle arm re-syncs the manifestation tier; the level
    /// re-derives (per-award calls never bank, `a4=0`).
    pub(crate) fn mc2_award_xp(&mut self, owner: u16, spell: usize, amount: i32) {
        if spell >= 26 {
            return;
        }
        if owner != PLAYER_TARGET {
            return; // the model-0 guard — rivals never accrue
        }
        if self.player.state != LifeState::Alive {
            return;
        }
        self.mc2_book.xp_vol[spell] += amount;
        if spell == 2 {
            let m = self.mc2_book.ent[2] as usize;
            if m != 0 {
                self.mc2_set_spell(m, self.mc2_book.sel[2]);
            }
        }
        self.mc2_relevel(spell, false, true);
    }

    /// The pane/book view (the CTRL pane + HUD consume this): per
    /// spell — owned, level, selected tier, effective XP; plus the
    /// quick-slot bindings. Read-only snapshot.
    pub fn mc2_book_view(&self) -> Mc2BookView {
        let mut owned = [false; 26];
        let mut xp = [0i32; 26];
        let mut xpos = [[0i32; 3]; 26];
        let mut cost = [0u32; 26];
        let mut cost_tier = [[0u32; 3]; 26];
        let mut armed = [false; 26];
        let mut expiring = [false; 26];
        let mut castable = [[false; 3]; 26];
        // Retail's canSummon castle-pool probe (EF:22504-05): the own
        // castle's STORED mana, resolved once for the whole pane.
        let castle_mana = self.player_castle().map_or(0, |c| self.g.ent[c].f140);
        for s in 0..26 {
            owned[s] = self.mc2_book.ent[s] != 0;
            xp[s] = self.mc2_book.xp_vol[s] + self.mc2_book.xp_bank[s];
            let tier = (self.mc2_book.sel[s] as usize).min(2);
            if let Some(row) = self.g.assets.spells.get(s) {
                for t in 0..3 {
                    xpos[s][t] = row.tiers[t].xpos1;
                    // `canSummon`/`canSubSummon` (EF:22503-08 /
                    // EF:22602-08): the tier's `maxManaLimit_A` is
                    // zero, or the castle pool covers it (no castle ⇒
                    // any nonzero requirement greys). Read from the
                    // SPELLS table, not the manifestation — the dev
                    // instrument zeroes the manifestation's copy.
                    let mml = row.tiers[t].max_mana_limit;
                    castable[s][t] = mml <= 0 || castle_mana as i64 >= mml as i64;
                }
            }
            cost[s] = self.mc2_spell_mana_cost(s, tier).max(0) as u32;
            for t in 0..3 {
                cost_tier[s][t] = self.mc2_spell_mana_cost(s, t).max(0) as u32;
            }
            let m = self.mc2_book.ent[s] as usize;
            armed[s] = m != 0 && self.g.ent[m].f26 > 0;
            // The blink set is retail's `isEnabled_1 & 4`, stamped in
            // CODE, not the SPELLS file (SetDefaultSpells_5C0A0
            // Spells.cpp:122-30): Speed Up/Morph/Shield/Rebound/
            // Invisible/Beyond Sight/Duel. Threshold 32, half MC1's.
            expiring[s] = m != 0
                && matches!(s, 3 | 4 | 6 | 8 | 11 | 12 | 14)
                && (1..32).contains(&self.g.ent[m].f26);
        }
        Mc2BookView {
            owned,
            levels: self.mc2_book.levels,
            sel: self.mc2_book.sel,
            xp,
            xpos,
            cost,
            cost_tier,
            armed,
            expiring,
            castable,
            left: self.mc2_book.left,
            right: self.mc2_book.right,
            ring: self.mc2_book.ring,
        }
    }

    // ---- selection (the 0x1F/0x20 "Change Spell" handler) ----------------

    /// EF:37898-37928: persist the chosen tier, bind the quick-slot,
    /// apply via SetSpell, sound 14. `hand`: 0 = left, 1 = right.
    /// The select-time hint text (`hintText_0x16x`) is the app's
    /// concern (it owns the notification surface).
    pub fn mc2_select_spell(&mut self, spell: u8, tier: u8, hand: u8) {
        let s = spell as usize;
        if s >= 26 {
            // Unbind semantic (spell out of range clears the hand —
            // the pane's empty-slot commit).
            if hand == 0 {
                self.mc2_book.left = -1;
            } else {
                self.mc2_book.right = -1;
            }
            return;
        }
        // Dev instrument: selecting an unowned spell under the
        // all-spells toggle self-grants the manifestation.
        if self.mc2_book.ent[s] == 0 && self.dev_spells {
            self.mc2_dev_grant(s);
        }
        if self.mc2_book.ent[s] == 0 {
            return;
        }
        // Normally the selectable tier is capped at the XP-earned
        // level; the all-spells (G) instrument keeps EVERY tier
        // exercisable, matching the app's pane (main.rs:2035). Without
        // the dev arm the sim casts tier 0 while the selector shows
        // tier N.
        let cap = if self.dev_spells {
            self.g
                .assets
                .spells
                .get(s)
                .map_or(0, |r| (r.byte_0 as i16 - 1).clamp(0, 255) as u8)
        } else {
            self.mc2_book.levels[s]
        };
        let t = tier.min(cap);
        self.mc2_book.sel[s] = t;
        if hand == 0 {
            self.mc2_book.left = spell as i8;
        } else {
            self.mc2_book.right = spell as i8;
        }
        let m = self.mc2_book.ent[s] as usize;
        self.mc2_set_spell(m, t);
        self.g.snd_player(14);
        // The change-spell toast (EF:37925): the chosen TIER's own name
        // ("Possession" / "Mana Magnet" / "Thunderstorm"), so a level-N
        // pick reads as its distinct spell, not a generic label.
        let name = self.mc2_spell_name(s, t as usize);
        self.set_notification(name, NOTIFY_TICKS_SELECT, NOTIFY_RED);
    }

    /// Retail cmd 0x26 (SHIFT+click fast-bind, EF:37950-53): a raw
    /// byte store into the cycle ring + sound 14. No equip
    /// side-effect — ring membership is a separate concept from what
    /// sits on each button. The toggle/move truth table lives in the
    /// SENDER (the app's pane click, PI:856-878); the sim just
    /// stores.
    pub(crate) fn mc2_ring_set(&mut self, spell: u8, val: u8) {
        let s = spell as usize;
        if s >= 26 {
            return;
        }
        self.mc2_book.ring[s] = val.min(2);
        self.g.snd_player(14);
    }

    /// The rest of retail's cross-level carry (`sub_549A0` L:1261-68
    /// beyond what `mc2_grant_plausible` re-derives): the per-spell
    /// selected tier (`array_0x437`), the cycle ring (`array_0x3B5`,
    /// carried RAW — even for spells not possessed this level), and
    /// the hand pointers, kept only if the spell is possessed here
    /// (the L:1332-35 load validation; otherwise the canonical
    /// level-start binding from the grant pass stands). Call AFTER
    /// `mc2_grant_plausible` — the tier clamp reads the levels that
    /// pass derived.
    pub fn mc2_install_selector_carry(
        &mut self,
        sel: &[u8; 26],
        ring: &[u8; 26],
        left: i8,
        right: i8,
    ) {
        if !matches!(self.game(), crate::ids::GameId::Mc2) {
            return;
        }
        for s in 0..26 {
            self.mc2_book.ring[s] = ring[s].min(2);
            // sel ≤ levels holds in any well-formed carry (select
            // clamps at write); min() only guards a foreign save.
            self.mc2_book.sel[s] = sel[s].min(self.mc2_book.levels[s]);
            let m = self.mc2_book.ent[s] as usize;
            if m != 0 {
                // Push the carried tier into the live manifestation
                // (retail's sub_55AB0 SetSpells to array_0x437).
                self.mc2_set_spell(m, self.mc2_book.sel[s]);
            }
        }
        if (0..26).contains(&(left as i32)) && self.mc2_book.ent[left as usize] != 0 {
            self.mc2_book.left = left;
        }
        if (0..26).contains(&(right as i32)) && self.mc2_book.ent[right as usize] != 0 {
            self.mc2_book.right = right;
        }
    }

    /// The dev-toggle grant: a manifestation with no jar (state 3M,
    /// hidden by the draw filter), wired like a pickup.
    /// Test hook: grant one spell through the PICKUP path, so a test
    /// can pin that the jar law still overwrites the left hand.
    #[cfg(test)]
    pub(crate) fn mc2_dev_grant_for_test(&mut self, spell: usize) {
        self.mc2_dev_grant(spell);
    }

    fn mc2_dev_grant(&mut self, spell: usize) {
        let (px, py, pz) = self.human_pose;
        if let Some(m) = self.g.mc2_spawn_spell_token(spell as u8, px, py, pz) {
            self.mc2_adopt_manifestation(m, spell);
        }
    }

    /// Install a plausible MC2 spellbook — the MC2 arm of the
    /// `plausible_spellbook` playtest instrument (MC1's lives in
    /// `campaign::plausible_spellbook` + `grant_spells`). For each
    /// `(spell, banked_xp)`: learn the spell if unowned (a hidden
    /// manifestation like the dev grant) and set its BANKED
    /// (campaign-carried) XP, then re-derive the tier from the SPELLS
    /// `xpos1` ladder — the same thresholds a real playthrough crosses,
    /// so a plausible scroll count yields a plausible tier. `banked_xp`
    /// is the app's campaign estimate (jar union → learned set; scroll
    /// census → XP). No-op off-MC2 (the book is MC2-only state).
    pub fn mc2_grant_plausible(&mut self, grants: &[(u8, i32)]) {
        if !matches!(self.game(), crate::ids::GameId::Mc2) {
            return;
        }
        for &(spell, xp) in grants {
            let s = spell as usize;
            if s >= 26 {
                continue;
            }
            if self.mc2_book.ent[s] == 0 {
                self.mc2_dev_grant(s);
            }
            // Grant can fail if the event pool is exhausted; skip XP.
            if self.mc2_book.ent[s] == 0 {
                continue;
            }
            self.mc2_book.xp_bank[s] = xp.max(0);
            // Derive the level from the new banked XP (no re-bank, no
            // level-up toast — this is an init-time install).
            self.mc2_relevel(s, false, false);
        }
        // Level-start binding, not the pickup law (see the fn doc).
        self.mc2_rebind_hands_canonical();
    }

    /// The collect wiring shared by the jar pickup and the dev grant
    /// (token trace §3, EF:55715-49): the token BECOMES the wizard's
    /// spell object — state 3M, cooldown 64, owner rebound; grant +
    /// quick-slot bind + SetSpell at the chosen tier.
    pub(crate) fn mc2_adopt_manifestation(&mut self, m: usize, spell: usize) {
        {
            let e = &mut self.g.ent[m];
            e.tick70 = (spell as u8).wrapping_mul(3);
            e.f54 = 64;
            e.id24 = PLAYER_TARGET;
            e.f26 = 0;
            e.f44 = 0;
        }
        self.mc2_book.ent[spell] = m as u16;
        // The stolen-jar hand hint (`word_0x4A_74` → f36, sub_68FF0
        // EF:55728-40): 1 = re-equip the RIGHT hand, 2 = the LEFT —
        // the hand the wraith yanked it from; cleared after use.
        // Without a hint (fresh jars, dev grants) the quick-slot v12
        // law applies: left if free (or both taken), else right
        // (EF:55735-49).
        let hint = self.g.ent[m].f36;
        self.g.ent[m].f36 = 0;
        if hint == 2 {
            self.mc2_book.left = spell as i8;
        } else if hint == 1 {
            self.mc2_book.right = spell as i8;
        } else if self.mc2_book.left == -1 || self.mc2_book.right != -1 {
            self.mc2_book.left = spell as i8;
        } else {
            self.mc2_book.right = spell as i8;
        }
        self.mc2_relevel(spell, false, false);
        self.mc2_set_spell(m, self.mc2_book.sel[spell]);
    }

    /// `sub_69300` (EF:55792-826) — the m26 wraith SPELL-STEAL: yank
    /// the equipped jar out of the given hand (1 = right, 2 = left,
    /// the roll's 4/5 in [`Gen::m26_tick`]). The empty-hand and
    /// slot-0 aborts (EF:19354-58/19366-70) and the `word_0x36_54`
    /// re-steal lock (EF:55800) all run AFTER the %63 draw — a
    /// locked or empty-handed roll is simply spent. Jar-entity field
    /// homes: f38 = the wraith (`word_0x26_38`), f26 = the arc
    /// counter (`dword_0x10_16`), f36 = the hand hint
    /// (`word_0x4A_74`), tick70 = 78 (the shared class-15 detach
    /// action). The per-spell tier (`array_0x437` → sel) is NOT
    /// touched — XP survives the theft. Retail's `byte[0] &= ~1`
    /// in-hand bit is write-only in the port (its lone retail reader
    /// is the presentation-side owned-jar tint, unmodeled).
    pub(crate) fn mc2_spell_steal(&mut self, wraith: u16, hand: u8) {
        let spell = if hand == 1 {
            self.mc2_book.right
        } else {
            self.mc2_book.left
        };
        if spell < 0 {
            return;
        }
        let s = spell as usize;
        let m = self.mc2_book.ent[s] as usize;
        if m == 0 {
            return;
        }
        if self.g.ent[m].f54 != 0 {
            return; // the 64-tick re-steal lock
        }
        {
            let e = &mut self.g.ent[m];
            e.f38 = wraith;
            e.tick70 = 78;
            e.f26 = 0;
        }
        // Snap the jar onto the player (CopyEntityPosition, EF:55810).
        let (px, py, pz) = self.human_pose;
        self.g.move_relink(m, px, py, pz);
        // Unlearn (`SpellEnabled[model] = 0`, EF:55811) — the pane
        // greys out and the ground jar becomes collectible again.
        self.mc2_book.ent[s] = 0;
        self.g.mc2_spell_tokens.0 &= !(1 << s);
        // Unequip every hand holding the model; the hint remembers
        // the LAST cleared hand (left wins on the both-hands edge,
        // EF:55814-24 — independent ifs, verbatim).
        self.g.ent[m].f36 = 0;
        if self.mc2_book.right == spell {
            self.mc2_book.right = -1;
            self.g.ent[m].f36 = 1;
        }
        if self.mc2_book.left == spell {
            self.mc2_book.left = -1;
            self.g.ent[m].f36 = 2;
        }
        self.entities_dirty = true;
    }

    // ---- the cast gate ----------------------------------------------------

    /// `sub_5F380`'s per-button dispatch (EF:60748) under the
    /// press/hold law (docs/traces/mc2-cast-input.md §1-2): the fire
    /// bits are EDGE-triggered per press
    /// (`HandleMouseButtons_18F80`, PI:2027-76); a HELD button
    /// re-fires only when the bound tier is RAPID (`byte_0x3B_59 !=
    /// 1`) and its cast window is live — that is the whole
    /// click-vs-Repeat-Fireball difference.
    ///
    /// The two retail registers behind `edge`/`held` are the two
    /// halves of `MouseButtonState_18059C`, rebuilt every poll at
    /// EF:49675-83: bit 0/1 = the ISR PRESS LATCH
    /// (`x_WORD_180746`/`180744`), bit 2/3 = the HELD state
    /// (`x_WORD_18074C`/`18074A`). `HandleMouseButtons_18F80` fires a
    /// non-rapid spell off bit 0 ALONE and clears it (PI:2043-49),
    /// and the frame tail clears the global latch whenever bit 0 is
    /// down (PI:1049-52) — hence exactly ONE cast per physical click,
    /// however long the button is held. Measured on mc2l4 0+4000: 409
    /// recorded press edges, 404 retail possession arms, and the port
    /// (once its edge lane was alive) 408.
    pub(crate) fn mc2_cast_input(&mut self, edge: (bool, bool), held: (bool, bool)) {
        if self.player.state != LifeState::Alive {
            return;
        }
        let fires = |w: &World, spell: i8, edge: bool, held: bool| {
            if spell < 0 {
                return false;
            }
            let m = w.mc2_book.ent[spell as usize] as usize;
            if m == 0 {
                return false;
            }
            // `byte_0x3B_59 == 1` is the CLICK-ONLY family; every
            // other value takes the repeat arm (PI:2043 vs PI:2050 —
            // the test is `== 1`, not `!= 0`).
            edge || (held && w.g.ent[m].f59 != 1 && w.g.ent[m].f26 > 0)
        };
        if fires(self, self.mc2_book.left, edge.0, held.0) {
            self.mc2_cast_gate(self.mc2_book.left as usize, false);
        }
        if fires(self, self.mc2_book.right, edge.1, held.1) {
            self.mc2_cast_gate(self.mc2_book.right as usize, true);
        }
    }

    /// `sub_5F660` (EF:60874) — the cast gate: the per-model
    /// re-arm/retrigger switch, then the mana gate (`mana <
    /// maxMana` → fail sound 29), then the arm (`sub_5F7B0`
    /// EF:60973: timer = duration).
    fn mc2_cast_gate(&mut self, spell: usize, right: bool) {
        let m = self.mc2_book.ent[spell] as usize;
        if m == 0 {
            return;
        }
        // Cave-In is CAVE-ONLY: refused off-cave (EF:43883/48253,
        // PI:849; the icon grey-out EF:22470 is the UI's side).
        if spell == 25 && !self.g.is_cave() {
            return;
        }
        let (armed, tier) = (self.g.ent[m].f26, self.g.ent[m].f71);
        match self.g.ent[m].model65 {
            // Fireball: tier < 2 re-arms freely; the charged tier
            // refuses while airborne (EF:60895-98 → LABEL_16).
            0 => {
                if tier >= 2 && armed > 0 {
                    return;
                }
            }
            // Possess: an active cast is not re-armed/re-charged —
            // the marker timer never refreshes — but the re-press
            // raises the `byte_0x3C_60 = 1` RELEASE SIGNAL (→ f56),
            // records the firing hand, and runs the invis-break law,
            // IN THAT ORDER and with NO mana gate: retail's arm is
            // `byte_0x3C_60 = 1; byte[1] &= 0xFC; dword |= v3;
            // sub_5F7E0(); v7 = 1; goto LABEL_23` (EF:60900-07) —
            // LABEL_23 buzzes only on `v6`, which this path never
            // sets, so a broke wizard re-pressing possession gets the
            // signal and NO sound 29. The tier-0 consumer discards
            // the signal (`sub_69640` EF:56013); the higher tiers
            // spend it on a re-fire.
            1 if armed > 0 => {
                self.g.ent[m].f56 = 1;
                self.g.ent[m].f50 = if right { 512 } else { 256 };
                self.mc2_arm_invis_break(spell);
                return;
            }
            // Castle: a re-cast while the ball flies buzzes
            // (EF:60908-13).
            2 if armed > 0 => {
                self.g.snd_player(29);
                return;
            }
            // Lightning: tier 0 re-arms freely (the RAPID stream);
            // tier 1+ refuses while armed (EF:60929-33).
            7 if tier >= 1 && armed > 0 => return,
            // The channel retriggers: an active cast is EXTENDED
            // (metamorph 7 ticks, the rest 1), no re-charge
            // (EF:60914-28).
            4 | 6 | 8 | 0xB | 0xC | 0xE if armed > 0 => {
                self.g.ent[m].f26 = if self.g.ent[m].model65 == 4 { 7 } else { 1 };
                return;
            }
            // The LABEL_16 band: no re-arm while active
            // (EF:60946-48).
            9 | 0xA | 0xD | 0xF | 0x10..=0x18 if armed > 0 => return,
            _ => {}
        }
        // THE MANA GATE (EF:60953): caster mana vs the tier's full
        // cost. Insufficient → UI flash + sound 29 (EF:60964-67).
        let cost = self.g.ent[m].max_life;
        if !self.dev_spells && (self.player.mana as u64) < cost as u64 {
            self.g.snd_player(29);
            return;
        }
        // `sub_5F7B0`: ARM — timer = duration; the effect state now
        // fires. A zero-duration row still casts for one tick. The
        // firing button is recorded (retail: `dword |= a3` 256/512
        // on the CASTER, EF:60973-82; ours rides the manifestation,
        // same information) — the launch reads it for the hand
        // muzzle.
        self.g.ent[m].f26 = self.g.ent[m].f28.max(1) as i16;
        self.g.ent[m].f50 = if right { 512 } else { 256 };
        // A release signal left over from the marker's last tick
        // must not refire into the fresh arm.
        self.g.ent[m].f56 = 0;
        self.mc2_arm_invis_break(spell);
    }

    /// The Invisibility per-tier break-on-self-cast law (`sub_5F7E0`
    /// EF:60987, run from the arm path `sub_5F7B0` AND the possess
    /// re-press): arming ANY spell may break an active cloak.
    /// `s = byte_0x1BF_447` (invis strength): T0 (s=1) any cast
    /// breaks; T1 (s=2) breaks on everything except possess (spell
    /// 1); T2 (s=3) nothing breaks. The invis FIRST cast doesn't
    /// self-break — strength is still 0 here (set on the invis
    /// effect's first tick). On break we also zero the invis
    /// window's `f26` so the mana-regen block lifts with the cloak
    /// (functional termination must clear the burst).
    /// docs/spell-audit/rival-spells.md §2.
    fn mc2_arm_invis_break(&mut self, spell: usize) {
        let s = self.player.invis_strength;
        if s != 0 && (s < 2 || (s <= 2 && spell != 1)) {
            self.player.invisible = false;
            self.player.invis_strength = 0;
            let inv = self.mc2_book.ent[0xB] as usize;
            if inv != 0 {
                self.g.ent[inv].f26 = 0;
            }
        }
    }

    // ---- the per-tick effect states ---------------------------------------

    /// `sub_68D50` (EF:55548) — may the cast proceed this tick?
    /// Caster alive; upkeep spells need the own castle's pool to
    /// cover `manaRegen`; the first tick re-checks the full cost.
    fn mc2_afford(&self, m: usize) -> bool {
        if self.player.state != LifeState::Alive {
            return false;
        }
        let e = &self.g.ent[m];
        if e.f136 > 0 {
            let ok = self
                .player_castle()
                .is_some_and(|c| self.g.ent[c].f140 >= e.f136);
            if !ok {
                return false;
            }
        }
        // `.max(1)` mirrors the arm/first-tick sites — a zero-
        // duration row arms f26=1; comparing against a raw 0 would
        // skip the first-tick full-cost re-check.
        if e.f26 as u16 == e.f28.max(1) {
            return self.dev_spells || self.player.mana as u64 >= e.max_life as u64;
        }
        true
    }

    /// The canonical effect-state skeleton (cast-path trace §1.5),
    /// run for every learned manifestation each tick: while armed —
    /// afford-check, FIRST-tick spawn + mana commit (`sub_68DE0` =
    /// the negative-delta stamp, [`World::mana_debit`]), countdown,
    /// pending-tier apply at expiry (`sub_6D880`); plus the cooldown
    /// tick (`word_0x36_54--`).
    pub(crate) fn mc2_cast_tick(&mut self, p: PlayerPose, ctx: &MobCtx) {
        for spell in 0..26usize {
            let m = self.mc2_book.ent[spell] as usize;
            if m == 0 {
                continue;
            }
            // Retail runs the effect body as the class-15 entity's own
            // action 3M (3M+1/3M+2 are the pickup states, 78 the
            // wraith-steal arc), so the port's book-driven loop tests
            // for it — which also disambiguates the death scatter's
            // BOOLEAN 1 marker (`sub_5E310` EF:60146,
            // `mc2_scatter_spells`) from a real slot-1 manifestation.
            if self.g.ent.get(m).is_none_or(|e| {
                e.class64 != 15 || e.model65 as usize != spell || e.tick70 as usize != spell * 3
            }) {
                continue;
            }
            self.mc2_manifestation_tick(spell, m, p, ctx);
        }
    }

    /// ONE manifestation's effect state — the body of
    /// [`World::mc2_cast_tick`]'s loop, split out because retail runs it
    /// as the class-15 entity's OWN action at its OWN pool slot, not
    /// from the caster's dispatch (see
    /// [`World::mc2_manifestation_pass`]).
    pub(crate) fn mc2_manifestation_tick(
        &mut self,
        spell: usize,
        m: usize,
        p: PlayerPose,
        ctx: &MobCtx,
    ) {
        {
            // CASTLE (2) is not a timed cast — its "active" window is an
            // UPGRADE LOCK driven by the castle's transform, exactly like
            // retail's `sub_69AB0`/`sub_5F890` (the manifestation timer is
            // never counted down; the castle build/upgrade/DOWNGRADE
            // entity pins it and clears it on completion). It must be
            // evaluated every tick — including an externally-forced
            // downgrade the player never cast — so it lives outside the
            // `f26 > 0` gate below.
            if spell == 2 {
                self.mc2_castle_spell_tick(m, p, ctx);
                if self.g.ent[m].f54 > 0 {
                    self.g.ent[m].f54 -= 1;
                }
                return;
            }
            if self.g.ent[m].f26 > 0 {
                // `sub_68DE0` (EF:55569) has two halves keyed on the
                // FIRST burst tick (`word_0x2E_46 == word_0x30_48`):
                // first tick stamps the negative-cost debit; EVERY
                // later live tick pins the caster's regen accumulator
                // to 0 — the "an active spell blocks mana
                // regeneration" law (docs/spell-audit/mana-regen.md).
                // Read `first` before the countdown.
                let first = self.g.ent[m].f26 as u16 == self.g.ent[m].f28.max(1);
                if self.mc2_afford(m) {
                    if first {
                        self.mc2_spell_fire(spell, m, p, ctx);
                        let cost = self.g.ent[m].max_life;
                        self.mana_debit(cost);
                        self.mc2_same_frame_debit(m);
                    } else if self.g.ent[m].f56 != 0 {
                        // The possess re-press RELEASE SIGNAL
                        // (`byte_0x3C_60`, raised by the cast gate's
                        // model-1 armed arm). Its effect-state consumer
                        // (`sub_68DE0` EF:55987-56013) is TIER-GATED on
                        // `byte_0x46_70`: for TIER 0 (plain possession)
                        // the signal is simply CLEARED — NO second bolt,
                        // NO mana debit — while the marker runs; only
                        // the higher tiers (Mana Magnet/Lock) re-spawn
                        // (`sub_69900`, on a 3-tick counter). Recorded
                        // retail (mc2l30/l0/l4, all tier-0 possess)
                        // fires exactly ONE delivery bolt per arm and
                        // none while armed — the earlier "re-cast
                        // freely, all tiers" reading over-fired (the
                        // (9,17) re-press family, no retail counterpart
                        // at any input latency: mc2l30 452->355, mc2l0
                        // 445->312, mc2l4 1393->1208). Tier 1/2 keep the
                        // coarse full-delivery re-fire below (`sub_69900`
                        // spawn untraced; not in the current corpus).
                        self.g.ent[m].f56 = 0;
                        if self.g.ent[m].f71 > 0 {
                            self.mc2_spell_fire(spell, m, p, ctx);
                            let cost = self.g.ent[m].max_life;
                            self.mana_debit(cost);
                            self.mc2_same_frame_debit(m);
                        }
                    }
                } else {
                    // Can't afford mid-cast → collapse to one tick
                    // (EF:63... skeleton line `word_0x2E_46 = 1`).
                    self.g.ent[m].f26 = 1;
                }
                // Mid-burst regen suppression (`sub_68DE0` else
                // branch): the first-tick debit already drove
                // `mana_delta` negative, so the `> 0` guard preserves
                // it; every later tick clamps the positive regen the
                // wizard tick recomputed this frame (world.rs:1225).
                if !first {
                    self.suppress_regen();
                }
                // SPEED's slipstream trail (`GetScroll_69DB0`
                // EF:56251-59): every 4th tick of the live window
                // — keyed on the TOKEN's phase byte
                // (`byte_0x3E_62 & 3`, our f63, NOT the caster's) —
                // drop a (10,2) ambient puff at the CASTER with the
                // ctor's life QUADRUPLED (8 → 32) and the caster's id.
                // `NewAdd0A02_4E430` (EF:35375) is a bare 4-field
                // ctor: maxLife 8, action 2, no sprite, and NO map
                // link (it writes `position_0x4C_76` directly), which
                // is why the trail hangs in the air where the carpet
                // was. mc2l3 t=15500+ is the instrument: one puff
                // every 4 ticks marching along the boosted flight
                // path, 175 of them across the take.
                if spell == 3
                    && self.g.ent[m].f63 & 3 == 0
                    && let Some(s) = self.g.mc2_spawn_speed_puff(p.x, p.y, p.z)
                {
                    self.g.ent[s].act_life *= 4;
                    self.g.ent[s].id24 = PLAYER_TARGET;
                }
                // The duel no-grip fizzle (`sub_6B610` abort arm,
                // EF:57280): 28 ticks into the window with NO duel
                // lock formed → collapse the charge to 1 (expires
                // next tick). With a lock the window runs full.
                if spell == 14
                    && self.mc2_duel.is_none()
                    && self.g.ent[m].f26 > 1
                    && self.g.ent[m]
                        .f28
                        .max(1)
                        .saturating_sub(self.g.ent[m].f26 as u16)
                        >= 28
                {
                    self.g.ent[m].f26 = 1;
                }
                self.g.ent[m].f26 -= 1;
                if self.g.ent[m].f26 == 0 {
                    self.mc2_cast_expire(spell, m);
                }
            }
            if self.g.ent[m].f54 > 0 {
                self.g.ent[m].f54 -= 1;
            }
        }
    }

    /// The CASTLE spell (2) tick — the UPGRADE LOCK, ported from retail's
    /// `sub_69AB0` + `sub_5F890` (EF:56086/61029). The manifestation's
    /// cast timer `f26` (`word_0x2E_46`) is NEVER a countdown for the
    /// castle: on the cast tick it fires once (spawns the build ball) and
    /// commits the cost, then it is HELD at `f28 - 1` (`word_0x30_48 - 1`)
    /// while the castle is transforming and cleared to 0 the moment the
    /// transform completes — so the "cast in progress" glow, and the
    /// re-cast block in [`World::mc2_cast_gate`], last exactly as long as
    /// the tower build/upgrade/downgrade, not a fixed 101 ticks. Because
    /// the lock is driven by the transform (not a cast), an externally
    /// forced downgrade (an enemy razing the castle level by level) also
    /// raises it — you get the "split second between transforms" to cast
    /// a rebuild, faithful to both games.
    fn mc2_castle_spell_tick(&mut self, m: usize, p: PlayerPose, ctx: &MobCtx) {
        let dur = self.g.ent[m].f28.max(1) as i16;
        // A fresh cast arms `f26 = f28` in `mc2_cast_gate`; that sentinel
        // is the only entry that fires + debits (the transform sets 100,
        // never `dur`).
        if self.g.ent[m].f26 == dur {
            if self.mc2_afford(m) {
                self.mc2_spell_fire(2, m, p, ctx); // cast_castle: spawns the ball
                let cost = self.g.ent[m].max_life;
                self.mana_debit(cost);
                self.mc2_same_frame_debit(m);
            } else {
                self.g.ent[m].f26 = 0;
                return;
            }
        }
        // `sub_5F890`: the manifestation active-state tracks the castle
        // transform (the flying build ball, or the castle mid-transform).
        let active = self.mc2_castle_lock_active();
        let was = self.g.ent[m].f26 > 0;
        if active {
            self.g.ent[m].f26 = dur - 1; // word_0x30_48 - 1
        } else if was {
            self.g.ent[m].f26 = 0;
            self.mc2_cast_expire(2, m);
            // `sub_60780` (EF:61670): every castle HP/CAP stamp also
            // re-runs SetSpell on the manifestation's OWN tier
            // (deferral suppressed — retail zeroes word_46 around the
            // call), so the cached cast cost (`max_life`, the mana
            // gate's word) tracks the castle level BOTH ways —
            // including a DOWNGRADE, which awards no XP (demolish or
            // an enemy razing a level would otherwise leave the old
            // rung cached and ding an affordable rebuild as
            // unaffordable). Ported at the lock-release edge instead
            // of retail's mid-transform stamp: observably equivalent,
            // since the cast gate is armed-blocked for the whole
            // transform — WHILE A CASTLE STANDS. Retail's stamp rides
            // the castle's own HP/CAP writes, so castle DEATH leaves
            // the old rung cached (the MC2 face of the first-castle
            // lockout): under the `castle_recast_cost` retail arm the
            // castle-less release skips the re-sync exactly like
            // retail; the patched arm re-syncs to the base-cost
            // rebuild.
            if (self.patches.castle_recast_cost && !self.strict_retail)
                || self.player_castle().is_some()
            {
                let tier = self.g.ent[m].f71;
                self.mc2_set_spell(m, tier);
            }
        }
    }

    /// Is the human's castle-spell UPGRADE LOCK engaged? — true while a
    /// human-owned castle build ball is in flight, or the human's castle
    /// is in any non-standing-idle transform state (build/upgrade/
    /// downgrade/settle). Mirrors where retail calls `sub_5F890(*,1)`
    /// (throughout the transform) vs `(*,0)` (return to the standing
    /// action-4 idle).
    fn mc2_castle_lock_active(&self) -> bool {
        // The cast in transit: the (9,10) castle ball still flying.
        if self.g.ent.iter().skip(1).any(|e| {
            e.class64 == 9 && e.model65 == 10 && e.id24 == PLAYER_TARGET && e.flags & 0x400 == 0
        }) {
            return true;
        }
        // The castle mid-transform: idle = action 4, no settle timer,
        // no armed upgrade (`mc2_castle_standing`/`_build`/`_destroy`).
        if let Some(c) = self.player_castle() {
            let e = &self.g.ent[c];
            let idle = e.tick70 == 4 && e.f50 == 0 && e.flags & super::castle::F_UPGRADE_ARMED == 0;
            !idle
        } else {
            false
        }
    }

    /// Cast-window expiry: apply the pending tier (`sub_6D880`
    /// EF:58215) and drop the armed-window player effects.
    pub(crate) fn mc2_cast_expire(&mut self, spell: usize, m: usize) {
        if self.g.ent[m].f44 > 0 {
            let t = (self.g.ent[m].f44 - 1) as u8;
            self.g.ent[m].f44 = 0;
            self.mc2_set_spell(m, t);
        }
        // Armed-window effects end with the window (shield
        // `dword &= 0xFFBFBFFF` EF:56496-tail, invis `&= 0xDF`
        // EF:57068-tail, and the sight/rebound analogues).
        match spell {
            6 => self.player.shield = false,
            8 => {
                self.player.rebound = false;
                self.g.mc2_rebound_precise.0 = 0;
            }
            // Duel window over → the lock dissolves (the EF:59916
            // enforcement liveness term reads the charge; a dead
            // charge ends the duel on its next pass — collapsed to
            // the expiry edge here).
            14 => self.mc2_duel = None,
            // Metamorph teardown (`sub_6A030` expiry EF:56394): despawn
            // the pose-puppet, un-hide the carpet, sound 60.
            4 => {
                let c = self.g.ent[m].f146 as usize;
                if c != 0 && c < self.g.ent.len() && self.g.ent[c].class64 == 5 {
                    self.g.ent[c].flags |= 0x400;
                }
                self.g.ent[m].f146 = 0;
                self.player.metamorph = 0;
                self.g.snd_player(60);
            }
            0xB => {
                self.player.invisible = false;
                self.player.invis_strength = 0;
            }
            0xC => self.player.beyond_sight = false,
            5 => self.player.heal_active = false,
            3 => {
                self.player.accel = 0;
                self.player.accel_mc2_factor = 0;
            }
            // Teleport's window end repeats the flight target-speed
            // zero (`sub_6AD60` countdown-out arm, EF:57046
            // `speed_0xc_12 = 0` beside the `sub_6D880` teardown).
            0xA => self.pending_speed_zero = true,
            _ => {}
        }
    }

    // ---- first-tick fire: dispatch + direct effects ------------------------

    /// The `sub_6DCA0` arm table (cast-path trace §2/§2.1): spell →
    /// (class-9 subtype, impact class/model, payload/charge flags).
    /// The charged variants (fireball 28/(10,76), thunder 12/(9,9))
    /// key on the tier's `life_0x1A`.
    pub(crate) fn mc2_dispatch_arm(spell: usize, life: i8) -> Option<DispatchArm> {
        let arm = |subtype, impact, charge| {
            Some(DispatchArm {
                subtype,
                impact,
                charge,
            })
        };
        match spell {
            0 if life >= 2 => arm(28, (10, 76), false),
            0 => arm(0, (10, 0), false),
            // Lightning L1/L2 (subtype 12): retail's `sub_66FD0` HARD-
            // CODES the detonation to spawn the `(10,38)` lightning
            // burst (NOT the bolt's own `(9,9)`, which retail keeps only
            // to chain a second-order beam FROM the burst;
            // docs/spell-audit/lightning.md §5.B). Route straight to
            // `(10,38)`; the second-order `(9,9)` chain off the burst
            // is deferred (its `(10,38)` internals untraced).
            7 if matches!(life, 1 | 2) => arm(12, (10, 38), false),
            7 => arm(9, (10, 23), false),
            9 => arm(3, (10, 17), true),
            15 => arm(23, (10, 71), true),
            16 => arm(5, (10, 11), true),
            17 => arm(2, (10, 15), true),
            18 => arm(4, (10, 9), true),
            20 => arm(22, (10, 67), true),
            // Steal Mana (13): a class-9 subtype-8 homing bolt whose
            // impact is the (10,25) "steal" burst (`sub_6B3E0` →
            // `sub_6DCA0(…,0xD,…)`, EF:57195; docs/spell-audit/
            // steal-mana.md). The bolt carries the tier's `sub_spell`
            // in f44 (2000/4000/10) — the drain amount the (10,25)
            // impact stamps into the struck wizard's ch3 inbox.
            13 => arm(8, (10, 25), false),
            21 => arm(26, (10, 22), true),
            25 => arm(30, (10, 89), true),
            _ => None,
        }
    }

    /// First cast tick — spawn the spell's effect. Projectile spells
    /// route through the `sub_6DCA0` dispatch; the direct-effect
    /// spells (cast-path trace §2.2) write player state or spawn
    /// their entity directly.
    fn mc2_spell_fire(&mut self, spell: usize, m: usize, p: PlayerPose, ctx: &MobCtx) {
        let tier = self.g.ent[m].f71 as usize;
        let row = self.g.assets.spells.get(spell).copied();
        let mut sub = row.map_or(Mc2SubSpell::default(), |r| r.tiers[tier.min(2)]);
        // The 0x15/0x19 arms divide the payload by the charge
        // (`subSpellIndex_2 / life_0x1A` when charged, EF:44189-219).
        if matches!(spell, 21 | 25) && sub.life > 0 {
            sub.sub_spell /= sub.life as i32;
        }

        // The projectile band (10 spells → sub_6DCA0, EF:44020).
        if let Some(arm) = Self::mc2_dispatch_arm(spell, sub.life) {
            // Cast sound v6: fireball 9, thunder charged 9 /
            // uncharged 23, default 15 (EF:44233 + §2 table).
            let v6 = match spell {
                0 => 9,
                7 if matches!(sub.life, 1 | 2) => 9,
                7 => 23,
                _ => 15,
            };
            // Lightning T3 (`life_0x1A == 2`): the cast site
            // `sub_6A5C0` loops `(life != 1) + 1` spawns, fanning
            // the pair's yaw ±113 (≈±19.9°) off the aim heading
            // (EF:56599-56656) — "two L2 bolts side by side". The
            // twins cross-link via f52 (word_0x34_52, EF:56651-56);
            // retail's only consumer is the beacon drone-lock
            // despawn arm (sub_66FD0 EF:58727-33, unported) — the
            // link keeps the state shape for when that lands. The
            // cast sound rides EACH spawn (sub_6DCA0 tail,
            // EF:44224-33), and the loop tolerates a full pool.
            let fan: &[u16] = if spell == 7 && sub.life == 2 {
                &[113, 113u16.wrapping_neg()]
            } else {
                &[0]
            };
            let mut twin: Option<usize> = None;
            for &off in fan {
                let Some(i) = self.mc2_launch(spell, m, &arm, sub, p) else {
                    continue;
                };
                if off != 0 {
                    let yaw = p.heading.wrapping_add(off) & 0x7FF;
                    let e = &mut self.g.ent[i];
                    e.f30 = yaw;
                    e.f34 = yaw;
                }
                if let Some(t) = twin {
                    self.g.ent[i].f52 = t as u16;
                    self.g.ent[t].f52 = i as u16;
                }
                twin = Some(i);
                self.g.snd_player(v6);
            }
            return;
        }

        // The direct-effect band (§2.2).
        match spell {
            // posses (`sub_69640` EF:55915), sound 40. The tier gate is
            // the SUBSPELL's `life_0x1A`, and it picks a different
            // ENTITY, not just a different payload (EF:55946-49):
            //
            //   life 0    → `sub_69900` (EF:56039) spawns the BASIC
            //               **(9,1)** bolt, impact (10,12);
            //   life 1..3 → the inline arm spawns **(9,17)**
            //               (EF:55950), `byte_0x44_68` = 54 (life 1) /
            //               69 (life 2) / the NewEvent 0 (life 3);
            //   life > 3  → the `<= 3` gate fails: NOTHING is cast.
            //
            // Per-tier delivery (docs/spell-audit/possession.md): T0
            // plain claim `(10,12)`; T1 Mana Magnet — claim + the
            // `(10,54)` attract aura (range 15); T2 Mana Lock — FORCED
            // claim ((10,70) steal pulse) + the `(10,69)` aura
            // (range 20).
            //
            // Row 1's `life` column IS (0,1,2) on the baked CD, so the
            // tier index stands in when there is no SPELLS row at all
            // (unit fixtures) — the port used to key on the tier index
            // ALONE and always launched (9,17): full-take mc2l24 read
            // (9,1) 362 missing / 0 extra.
            1 => {
                let tier = self.g.ent[m].f71 as usize;
                let life = row.map_or(tier as i8, |_| sub.life);
                let arm = match life {
                    0 => Some((1u8, (10u8, 12u8))),
                    1 => Some((17, (10, 54))),
                    2 => Some((17, (10, 69))),
                    3 => Some((17, (10, 0))),
                    _ => None,
                };
                if let Some((subtype, impact)) = arm
                    && let Some(i) = self.mc2_launch(
                        spell,
                        m,
                        &DispatchArm {
                            subtype,
                            impact,
                            charge: false,
                        },
                        sub,
                        p,
                    )
                {
                    // `sub_69900`'s launch tail (EF:56050-67) — the
                    // (9,17) arm writes the same lanes
                    // (EF:55956/55966/55968), so both share it:
                    //   `mana_0x90_144` = the TOKEN's mana — now the
                    //     universal `mc2_launch` copy (the l24 corpus
                    //     records 33),
                    //   `dword_0x10_16` = 200 on the basic bolt (@0x10
                    //     → f26); the leveled arm instead squares the
                    //     token's `subSpellIndex << 8`.
                    // The `position.z += caster fov` of EF:56054 /
                    // EF:55969 is already carried by `muzzle`, which
                    // launches at pose z + PLAYER_HH.
                    // DELIBERATE: retail also stamps `word_0x26_38` =
                    // the token's SLOT (@0x26 → f40), but the port
                    // spends f40 on the spell INDEX — the impact XP
                    // back-ref (`mc2_proj_impact`), which retail
                    // hard-codes per handler (`sub_6D8B0(id, 1, 1)`,
                    // EF:63314/59052). The lane is not compared; the
                    // XP wiring wins.
                    let token_sub = self.g.ent[m].f30 as i32;
                    {
                        let e = &mut self.g.ent[i];
                        e.f26 = if subtype == 1 {
                            200
                        } else {
                            let v = token_sub << 8;
                            (v.wrapping_mul(v)) as i16
                        };
                        // BOTH possession arms take the carpet boost
                        // RAW — `v2x->actSpeed += a2x->actSpeed`
                        // (EF:56048 / EF:55953) with no clamp. The
                        // [384, 0x2000] clamp `mc2_launch` applies is
                        // `sub_6DCA0`'s alone (EF:44226-31), and it
                        // both floors a REVERSING carpet's bolt at
                        // 384 and drops the negative term outright.
                        // mc2l4 t=13 slot 303 records speed **336** =
                        // 384 − 48 on a backing carpet.
                        e.f126 = 384i32.saturating_add(p.speed as i32) as i16;
                    }
                    // Sound 40 only on a successful spawn.
                    self.g.snd_player(40);
                }
            }
            // castle: the castle-ball cast (the MC1 machinery on the
            // MC2 column — the sub_69AB0 build queue is the castle
            // column's banked follow-up), sound 15.
            2 => {
                // The hand pick feeds MC1's muzzle anchor only; the
                // MC2 lane always spawns at the carpet (cast_castle's
                // mc2 gate), so the side is inert here.
                self.cast_castle(p, false);
                self.g.snd_player(15);
            }
            // speed_up: the accelerate channel (`GetScroll_69DB0`
            // EF:56189), sound 19. The per-tier factor `subSpellIndex`
            // = {2,3,4} drives 160/240/320 sustained (not the MC1
            // fixed 3.0/2.0) — docs/spell-audit/speed.md. MC2's one
            // spell doubles as MC1's Accelerate AND Accelerate
            // Backwards: the direction is the caster's CURRENT
            // velocity sign (EF:56212-15 — `v2 = speed_0xc_12 >= 0 ?
            // 1 : -1`, standstill counts as forward; retail re-derives
            // it every effect tick, but the hard speed override makes
            // the sign self-sustaining, so the cast-time latch is the
            // same law).
            3 => {
                self.player.accel = if p.speed >= 0 { 1 } else { -1 };
                self.player.accel_held = true;
                self.player.accel_mc2_factor = sub.sub_spell.clamp(1, 8) as i8;
                self.mc2_award_xp(PLAYER_TARGET, 3, 1);
                self.g.snd_player(19);
            }
            // heal (EF:56432), sound 25.
            5 => {
                self.player.heal_active = true;
                self.mc2_award_xp(PLAYER_TARGET, 5, 1);
                self.g.snd_player(25);
            }
            // shield (EF:56496): armed-window flag.
            6 => {
                self.player.shield = true;
                self.mc2_award_xp(PLAYER_TARGET, 6, 1);
            }
            // rebound (`sub_6AA00` EF:56721-51): armed-window flag +
            // the tier's LAW bit — `life==1` (T3) stamps PRECISE
            // (byte0xc[0]|=0x10: exact return down the reverse ray,
            // doubled payload), `life==0` scatter (byte[1]|=0x80).
            // Durations ride the table (125/251/125); the deflection
            // itself lives in `mc2_rebound_deflect` at the movers'
            // victim-hit gates.
            8 => {
                self.player.rebound = true;
                self.g.mc2_rebound_precise.0 = (sub.life == 1) as i32;
                self.mc2_award_xp(PLAYER_TARGET, 8, 1);
            }
            // teleport (`sub_6AD60` EF:56860): the real per-tier
            // relocation — to own castle / save+return toggle / cycle
            // all castles (docs/spell-audit/teleport.md). Sound 22 is
            // played inside on a castle success (silent random hop).
            0xA => {
                self.mc2_cast_teleport(m, p);
                self.mc2_award_xp(PLAYER_TARGET, 10, 1);
            }
            // invisible (EF:57068): set the flag AND the per-tier
            // break strength (`byte_0x1BF_447 = life_0x1A` = {1,2,3}),
            // which the arm-path break law consults (mc2_cast_gate).
            0xB => {
                self.player.invisible = true;
                self.player.invis_strength = sub.life.clamp(0, 3) as i8;
                self.mc2_award_xp(PLAYER_TARGET, 11, 1);
            }
            // beyond_sight (EF:57132).
            0xC => {
                self.player.beyond_sight = true;
                self.mc2_award_xp(PLAYER_TARGET, 12, 1);
            }
            // summon_army (`sub_6C170` EF:57638): the (9,24) carrier
            // flies forward and LANDS to spawn a ring of allied class-5
            // creatures. Impact (10,72); charge=true carries the tier's
            // creature MODEL (life = 19/2/25/16) in f71 (the ring's army
            // size + model). Sound 9 (docs/spell-audit/summon-creatures.md).
            0x13 => {
                if self
                    .mc2_launch(
                        spell,
                        m,
                        &DispatchArm {
                            subtype: 24,
                            impact: (10, 72),
                            charge: true,
                        },
                        sub,
                        p,
                    )
                    .is_some()
                {
                    self.g.snd_player(9);
                }
            }
            // fools_mana (`sub_6C870` EF:57868): a SHOTGUN of six
            // neutral fake-mana decoys, each a trap that detonates on
            // an enemy's possession claim. Cast sound 11 once after
            // the burst (docs/spell-audit/fools-mana.md).
            0x16 => {
                // Retail's sub_6C870 cast awards no XP (the trap's
                // SPEND points in sub_36680 do); sound 11 gates on the
                // burst spawning (EF:57924).
                if self.mc2_cast_fools_mana(m, p, sub) {
                    self.g.snd_player(11);
                }
            }
            // magic_mine (`sub_6CAC0` EF:57960): the (9,29) carrier flies
            // forward and LANDS to place a persistent (10,78) proximity
            // mine. Impact (10,78); charge=true so the tier rides f71
            // (blast intensity) while f44 carries the tier lifespan
            // (subSpell). Sound 15 (docs/spell-audit/magic-mine.md).
            0x17 => {
                if self
                    .mc2_launch(
                        spell,
                        m,
                        &DispatchArm {
                            subtype: 29,
                            impact: (10, 78),
                            charge: true,
                        },
                        sub,
                        p,
                    )
                    .is_some()
                {
                    self.g.snd_player(15);
                }
            }
            // alliance: class-9 subtype 25 direct (`sub_6CD20`
            // EF:58039), sound 9. Impact = the (10,74) CONVERSION
            // executor (NOT a fire — it allies the target rather than
            // burning it). charge=true carries the tier's area radius
            // in f71 (life = 16/26/32 tiles); f44 already rides the
            // tier's subSpell (610/1100/2710) = the charm DURATION,
            // not damage.
            0x18 => {
                if self
                    .mc2_launch(
                        spell,
                        m,
                        &DispatchArm {
                            subtype: 25,
                            impact: (10, 74),
                            charge: true,
                        },
                        sub,
                        p,
                    )
                    .is_some()
                {
                    self.g.snd_player(9);
                }
            }
            // metamorph (`sub_6A030` EF:56294): transform the caster
            // into a pooled class-5 creature (pose-puppet), carpet hidden.
            4 => self.mc2_cast_metamorph(m, sub, p),
            // duel (`sub_6B610` EF:57258): spawn the (10,26) DUEL
            // TETHER at the caster carrying the tier + owner, cast
            // sound 9 (EF:57316). The grip → lock → enforcement
            // machinery lives in world.rs (`mc2_duel_tether_tick` /
            // `mc2_duel_enforce`); docs/spell-audit/duel.md.
            0xE => self.mc2_cast_duel(sub, p),
            _ => {}
        }
        let _ = ctx;
    }

    /// The launch block shared by every projectile arm (cast-path
    /// trace §1.5, EF:55853-55886): spawn at the caster, owner id,
    /// payload, muzzle height, launch angles from the carpet pose,
    /// speed boost from the caster's flight speed (clamped 384..
    /// 0x2000 — EF:44226-31), and the local-player muzzle sprite 42.
    /// `sub_6C870` (EF:57868) — Fool's Mana: throw SIX neutral
    /// FAKE-mana spheres from the caster's hand in a ±85 yaw cone. Each
    /// carries a random mana value (the disguise) but is a TRAP — the
    /// retail homes verbatim: parentId (id24) = caster (EF:57905), tier
    /// `byte_0x46_70` (f71) = `life` (EF:57907), damage payload
    /// `subSpellIndex_0x2A_42` (f44) = `subSpellIndex_2` (EF:57906),
    /// colour neutral `playerEntityIndex` (f144) = 0 (EF:57908). When a
    /// NON-owner possession claims one, the sphere's tick springs the
    /// tier retaliation (mc1/combat.rs `ball_tick` →
    /// [`Gen::mc2_fools_retaliate`]) instead of handing over the mana
    /// (docs/spell-audit/fools-mana.md). The trap machinery is the
    /// (10,57) TICK's, not a cast flag: the authored ground spheres run
    /// the identical path off their NewEvent defaults.
    fn mc2_cast_fools_mana(&mut self, m: usize, p: PlayerPose, sub: Mc2SubSpell) -> bool {
        let right = self.g.ent[m].f50 == 512;
        let (mx, my, _mz) = self.muzzle(p, right);
        let payload = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
        let tier = sub.life.clamp(0, 3) as u8;
        let mut spawned = false;
        for _ in 0..6 {
            let z = self.g.ground_z(mx, my) as i16;
            let Some(s) = self.g.mc2_spawn_mana_sphere(57, mx, my, z) else {
                break;
            };
            // ±85 yaw fan (retail `caster.yaw - 85 + rng % 0xAA`), given
            // a short outward toss so the six scatter like thrown mana.
            let r = self.g.ent_rand(s);
            let yaw = p.heading.wrapping_add((r % 0xAA) as u16).wrapping_sub(85) & 0x7FF;
            let mut pos = (mx, my, z);
            Gen::polar_step(&mut pos, yaw, 0, 96);
            let e = &mut self.g.ent[s];
            e.id24 = PLAYER_TARGET; // parentId = caster (the skip-gate)
            e.f71 = tier; // byte_0x46_70 retaliation tier {0,1,2}
            e.f44 = payload; // subSpellIndex damage payload
            e.f144 = 0; // NEUTRAL — no owner colour (the "fool")
            e.f30 = yaw; // launch heading (fallback retaliation aim)
            e.dest_x = pos.0.wrapping_sub(mx);
            e.dest_y = pos.1.wrapping_sub(my);
            spawned = true;
        }
        spawned
    }

    /// `sub_6A030` (EF:56294) — Metamorph: spawn ONE class-5 creature
    /// (model = the tier's `life`: 2 Day / 19 non-Day, 25, 16) at the
    /// caster and mark it a pose-PUPPET (StageVar2/site_z = 12, action
    /// `8*M+7`) allied to the player. The wizard keeps normal control
    /// and casting; the carpet is hidden (`player.metamorph`) and the
    /// creature draws in its place. The manifestation links the creature
    /// (`word_0x96_150` → f146) for teardown at the cast-window expiry
    /// (mc2_cast_expire). Sound 60; XP on the fire tick. No control
    /// rebinding is needed — the creature is slaved to the live player
    /// pose (docs/spell-audit/summon-creatures.md Part A).
    /// `sub_6B610` first-tick body (EF:57297-57316): the (10,26)
    /// duel tether — class 10, model/action 26, life 8, sprite row
    /// 284, +44 = 200 (the ch4 grip amount), stamped with the
    /// caster (`byte_0x46_70` → owner, ours `id24`) and the TIER
    /// (`subSpellIndex_0x2A_42` copy, ours `f71`), spawned at the
    /// caster's position; `PrepareEventSound(…, -1, 9)`.
    fn mc2_cast_duel(&mut self, sub: Mc2SubSpell, p: PlayerPose) {
        let tier = self.mc2_book.sel[14];
        let z = self.g.ground_z(p.x, p.y) as i16;
        if let Some(t) = self.g.new_event() {
            {
                let e = &mut self.g.ent[t];
                e.class64 = 10;
                e.model65 = 26;
                e.tick70 = 26;
                e.max_life = 8;
                e.f44 = 200;
                e.f71 = tier;
                e.id24 = PLAYER_TARGET;
                e.flags &= !8;
            }
            self.g.link(t, p.x, p.y, z);
            self.g.refill_life(t);
            self.g.mc2_set_sprite(t, 284);
        }
        let _ = sub;
        self.g.snd_player(9);
    }

    fn mc2_cast_metamorph(&mut self, m: usize, sub: Mc2SubSpell, p: PlayerPose) {
        let model = sub.life.max(0) as u8;
        let z = self.g.ground_z(p.x, p.y) as i16;
        let Some(s) = self.g.mc2_spawn_creature_model(model, p.x, p.y, z) else {
            return;
        };
        {
            let e = &mut self.g.ent[s];
            e.site_z = 12; // StageVar2 = 12 (metamorph pose-puppet)
            e.tick70 = model.wrapping_mul(8).wrapping_add(7); // action 8*M+7
            e.id24 = PLAYER_TARGET; // caster's team → allied
            e.f26 = 0; // scream-loop timer: cry on the first tick
        }
        self.g.ent[m].f146 = s as u16; // manifestation link (word_0x96_150)
        self.player.metamorph = model; // hide the carpet, draw the creature
        self.mc2_award_xp(PLAYER_TARGET, 4, 1);
        self.g.snd_player(60);
    }

    /// Returns the spawned projectile's slot (None = pool full).
    fn mc2_launch(
        &mut self,
        spell: usize,
        m: usize,
        arm: &DispatchArm,
        sub: Mc2SubSpell,
        p: PlayerPose,
    ) -> Option<usize> {
        // Hand muzzle: launch from the firing hand's side (recorded
        // at arm time; the MC1 lateral-step law stands in until the
        // retail hand-offset trace lands).
        let right = self.g.ent[m].f50 == 512;
        let (mx, my, mz) = self.muzzle(p, right);
        let Some(i) = self.g.mc2_spawn_cast_proj(arm.subtype, mx, my, mz) else {
            return None; // pool full: no projectile, NO cast sound
            // (retail gates the sound on the spawn, EF:44224-39)
        };
        {
            let e = &mut self.g.ent[i];
            e.id24 = PLAYER_TARGET;
            e.f68 = arm.impact.0;
            e.f69 = arm.impact.1;
            // The tier payload rides every projectile (EF:55864 —
            // the effect-state copy; carried damage / claim amount).
            e.f44 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
            if arm.charge {
                e.f71 = sub.life.max(0) as u8;
            }
            // Launch = the carpet's facing; the projectile's z gets
            // the muzzle lift (pos.z += caster fov — the carpet
            // sits at pose z already).
            e.f30 = p.heading;
            e.f32 = p.pitch;
            e.f34 = p.heading;
            e.f36 = p.pitch;
            // Speed boost: caster actSpeed onto the base, clamped
            // (EF:44226-31).
            let boosted = (e.f126 as i32 + p.speed.max(0) as i32).clamp(384, 0x2000);
            e.f126 = boosted as i16;
        }
        // Every retail cast site copies the hand token's mana onto
        // the spawned projectile (`v6x->mana_0x90_144 =
        // a1x->mana_0x90_144` — the sub_693F0 fire block EF:55865
        // and each instant handler: army EF:57745, fools EF:57817,
        // mine EF:57992, alliance EF:58151). The lane is COMPARED
        // (mc2l0 t=2798 slot 172: retail 20 = the fireball hand's
        // purse; the class-9 ctor default 50 must not survive).
        let token_mana = self.g.ent[m].f140;
        self.g.ent[i].f140 = token_mana;
        // Back-ref for the impact XP award (`word_0x26_38` → the
        // spell entity; ours carries the spell INDEX in f40 — a
        // projectile never uses the attacker latch).
        self.g.ent[i].f40 = spell as u16;
        // The local player's FIREBALL swaps to the star-shaped
        // muzzle/aim sprite 42 (`SetEntityIndex_49C90(v17x, 42)`,
        // gated local-player && spell 0 — EF:30291): index + frame
        // only, the row-340 extent quad stays (49C90, not 49CD0).
        if spell == 0 {
            let e = &mut self.g.ent[i];
            e.type86 = 42;
            e.frame88 = 0;
        }
        Some(i)
    }

    /// The crosshair instrument's MC2 arm (P-class; `aim_preview`
    /// routes MC2-bound hands here): the target the hand's spell's
    /// PROJECTILE would acquire on its FIRST flight tick if launched
    /// this instant — the pure [`Gen::mc2_aim_scan`] twin under the
    /// launch pose. Retail MC2 draws NO reticle (the aim feedback IS
    /// the sprite-42 projectile curving, docs/traces/mc2-autoaim.md
    /// §4/mc2-mouse-aim.md §4); this is an opt-in predictor
    /// (deliberate), not a faithful surface. None = non-acquiring
    /// spell or empty cone.
    pub(crate) fn mc2_aim_preview(
        &self,
        p: PlayerPose,
        right: bool,
        spell: usize,
    ) -> Option<AimLock> {
        // The would-be projectile subtype: the sub_6DCA0 band + the
        // direct class-9 arms (possess/summon/mine/alliance).
        let tier = self.mc2_book.sel.get(spell).copied().unwrap_or(0) as usize;
        let life = self
            .g
            .assets
            .spells
            .get(spell)
            .map_or(0, |r| r.tiers[tier.min(2)].life);
        let subtype = Self::mc2_dispatch_arm(spell, life)
            .map(|a| a.subtype)
            .or(match spell {
                // Possession picks its ENTITY off the tier's life:
                // 0 → the basic (9,1), 1..3 → the leveled (9,17)
                // (EF:55946-49). Both share the model-1/0x11 aim list.
                1 if life == 0 => Some(1),
                1 => Some(17),
                0x13 => Some(24),
                0x17 => Some(29),
                0x18 => Some(25),
                _ => None,
            })?;
        let (_, _, speed, max_life, _, _) = *CREATORS.iter().find(|c| c.0 == subtype)?;
        let (mx, my, mz) = self.muzzle(p, right);
        let probe = super::proj::AimProbe {
            x: mx,
            y: my,
            z: mz,
            yaw: p.heading,
            pitch: p.pitch,
            model: subtype,
            own: PLAYER_TARGET,
            reach: speed as i64 * max_life as i64,
        };
        let slot = self.g.mc2_aim_scan(&probe, None)?;
        let e = &self.g.ent[slot as usize];
        Some(AimLock {
            x: e.x as f32 / 256.0,
            z: e.y as f32 / 256.0,
            // The acquire aims at the +78 half-height point
            // (`sub_655C0`) — castles at the raw z (the flag).
            alt: e.aim_z() as f32 / 256.0,
        })
    }

    // ---- level-init defaults / death scatter -------------------------------

    /// The MC2 level-start book: FIREBALL (0) and POSSESS (1) at 0 XP
    /// (MC1 by contrast inits spell-less). The adopt order binds
    /// fireball → left, possess → right via the pickup's own v12
    /// quick-slot law.
    ///
    /// CORRECTION — do NOT re-derive from the name this once cited:
    /// `SetDefaultSpells_5C0A0` (`Spells.cpp:110`) grants NOTHING. It
    /// only rewrites the static SPELLS table's `isEnabled_1` /
    /// `fontType_0x1B` / `maxManaLimit_A` flags. The real grant is
    /// `InitialiseSpells_54A50`'s gate (`EventsFunctions.cpp:38721-62`):
    /// walk indices 0..25 ascending, enable the human's entitled set —
    /// the level's authored `starting_spells` row on campaign level 0
    /// or a direct `--level N` launch, the CARRIED book thereafter,
    /// always minus `blocked_spells` — then first enabled → left,
    /// second → right at tier 0.
    ///
    /// Spells are HOARDED across levels in both games — neither engine
    /// re-grants a book each level. Retail's fallback to the authored
    /// row applies ONLY when there is no carry (campaign level 0, or a
    /// direct `--level N`); on campaign levels > 0 the carried book is
    /// the whole story, and the level's `allowed`/`blocked` rows only
    /// ever TAKE spells away (MC1 does this from index 025; MC2 not at
    /// all in campaign).
    ///
    /// So seeding `{0, 1}` unconditionally here is a floor retail does
    /// not have: a spell permanently lost to the wraith steal would be
    /// handed back by us and not by retail. Correct for level 000
    /// (whose row IS `{0,1}`) and harmless for the hands, but OPEN —
    /// see docs/ROADMAP.md. The campaign carry itself is already right
    /// (`apply_campaign_book`).
    pub(crate) fn mc2_seed_default_spells(&mut self) {
        for s in [0usize, 1] {
            if self.mc2_book.ent[s] != 0 {
                continue;
            }
            let (px, py, pz) = self.human_pose;
            if let Some(m) = self.g.mc2_spawn_spell_token(s as u8, px, py, pz) {
                self.mc2_adopt_manifestation(m, s);
                self.g.mc2_spell_tokens.0 |= 1 << s;
            }
        }
        self.mc2_rebind_hands_canonical();
    }

    /// The MC2 level-init hand assignment (`InitialiseSpells_54A50`,
    /// EF:38664-38762): clear both hands, then walk the spell indices
    /// in canonical order — `spellIndex_D94FF` (GameUI.cpp:59) is the
    /// IDENTITY over 0..25 — binding the first enabled spell to the
    /// LEFT hand and the second to the RIGHT. Fewer than two enabled
    /// leaves the remaining hand at -1, and the cast path suppresses
    /// that button. Tier stays 0 (`SubSpellIndex*` derive from the
    /// per-level-zeroed `array_0x437`, EF:38659 / :59421).
    ///
    /// A level start must NOT reuse the jar-pickup law in
    /// [`Self::mc2_adopt_manifestation`]: that binds left, then right,
    /// then OVERWRITES left for every further spell, so a batch of N
    /// grants ends with left = the LAST granted and right = the
    /// SECOND. mc2:003's `{0,1,2,3,4,6,11,12}` came out Beyond Sight
    /// (12) / Possession (1) instead of Fireball / Possession. The
    /// pickup law is right for actual pickups and is unchanged.
    pub(crate) fn mc2_rebind_hands_canonical(&mut self) {
        self.mc2_book.left = -1;
        self.mc2_book.right = -1;
        for s in 0..26usize {
            if self.mc2_book.ent[s] == 0 {
                continue;
            }
            if self.mc2_book.left == -1 {
                self.mc2_book.left = s as i8;
            } else if self.mc2_book.right == -1 {
                self.mc2_book.right = s as i8;
                break;
            }
        }
    }

    /// Wizard-death token scatter (`sub_5E310` EF:60137-62): every
    /// owned manifestation becomes a collectible jar again — state
    /// 3M+1, the in-book bit cleared, scattered ±256 around the
    /// CORPSE, life `rand%90 + 200`.
    ///
    /// The book entry becomes a BOOLEAN 1, not 0 (EF:60146): that
    /// marker is the whole memory of what the wizard knew, and
    /// `sub_5CF40` re-mints exactly the entries that are non-zero.
    /// Zeroing it here (as this arm did while it was unwired) would
    /// have made every death a permanent spellbook wipe.
    ///
    /// The HANDS are untouched — `SpellIndexLeft/Right` are outside
    /// this loop and survive death (mc2l3 keeps 0/1 across both).
    ///
    /// DEVIATION: retail rolls the three draws per token off the dying
    /// WIZARD's private LCG (`a1x->rand_0x14_20`), which this port has
    /// no home for — the human owns no pool record, so its private
    /// stream is outside the sim. A COPY of the token's own seed
    /// stands in: same constants, same shape, different offsets. Two
    /// things it deliberately is NOT — the world stream (which would
    /// desync every other entity's draws on the landing tick) and the
    /// token's live `rand` field (retail's scatter never writes it;
    /// mc2l3 t=15300 keeps all 26 seeds at their allocation values).
    /// OPEN: import `carpet.rand` and roll the real stream.
    pub(crate) fn mc2_scatter_spells(&mut self, p: PlayerPose) {
        for spell in 0..26usize {
            let m = self.mc2_book.ent[spell] as usize;
            if m == 0 {
                continue;
            }
            self.mc2_book.ent[spell] = 1; // the boolean "still known" marker
            let mut r = self.g.ent[m].rand;
            let r1 = lcg32(&mut r);
            let r2 = lcg32(&mut r);
            let x = p.x.wrapping_add((r1 & 0x1FF) as u16).wrapping_sub(256);
            let y = p.y.wrapping_add((r2 & 0x1FF) as u16).wrapping_sub(256);
            let life = (lcg32(&mut r) % 0x5A + 200) as i32;
            {
                let e = &mut self.g.ent[m];
                e.tick70 = (spell as u8).wrapping_mul(3).wrapping_add(1);
                e.act_life = life;
                e.f26 = 0;
                e.flags &= !1;
            }
            self.g.move_relink(m, x, y, p.z);
        }
        self.g.mc2_spell_tokens.0 = 0;
    }
}

// ------------------------------------------------------------ snapshot

use crate::snapshot::{Reader, Snap, SnapshotError, Writer};

impl Snap for Mc2Spellbook {
    fn put(&self, w: &mut Writer) {
        let Mc2Spellbook {
            ent,
            xp_vol,
            xp_bank,
            levels,
            sel,
            left,
            right,
            ring,
        } = self;
        w.put(ent);
        w.put(xp_vol);
        w.put(xp_bank);
        w.put(levels);
        w.put(sel);
        w.put(left);
        w.put(right);
        w.put(ring);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Mc2Spellbook {
            ent: r.get()?,
            xp_vol: r.get()?,
            xp_bank: r.get()?,
            levels: r.get()?,
            sel: r.get()?,
            left: r.get()?,
            right: r.get()?,
            ring: r.get()?,
        })
    }
}
