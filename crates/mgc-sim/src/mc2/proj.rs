//! MC2 class-9 projectile family — the flyer core and the creature
//! attack thunks, ported from remc2 (trace bank:
//! docs/traces/mc2-class9-flyers.md; `EF:` = EventsFunctions.cpp,
//! `EV:` = Events.cpp cites).
//!
//! Field mapping additions over the [`super::mobs`] module doc:
//! `byte_0x43_67` impact class→f68 · `byte_0x44_68` impact model→f69
//! (the MC1 fields mean exactly this — detonation class/model) ·
//! `fov_0x22_34` desired-pitch→f36 · `roll_0x20_32` desired-yaw→f34
//! (as everywhere in the MC2 column) · `subSpellIndex_0x2A_42`
//! carried damage→f44 · `mana_0x90_144`→f140.
//!
//! Deliberate approximations and open items (cited, counted where
//! observable):
//! - The shielded-target ricochet `sub_68740` is ported
//!   ([`Gen::mc2_rebound_deflect`]). OPEN: the friendly-shield
//!   homing/detonate pair `sub_68940`/`sub_68AC0` (needs the (10,78)
//!   beacon column) and the rival-window mirror onto pool entities.
//! - The no-target acquisition `sub_67CB0` (EF:54710, model-keyed
//!   bucket sweeps) serves PLAYER-CAST spells; creature launches
//!   pre-lock `word_0x96_150`. A target-less flyer snapshots its aim
//!   once (the retail else-arm, EF:62914-16) and flies straight.
//! - Water splash spawns (10,5) (EF:62957-63, `mc2_spawn_splash`),
//!   gated inside the terrain-contact branch
//!   (docs/traces/mc2-projectile-terrain-water.md §3).
//! - An impact whose (f68, f69) effect is unported applies its f44
//!   as channel-0 area damage at the impact point (the effect IS the
//!   damage carrier in retail) and counts the misfit (deliberate).
//! - `(9,9)` creator body pending (the subtype 0-0x0C trace); interim
//!   fields marked OPEN below.

use super::behavior::BEHAVIOR;
use crate::engine::features::Gen;
use crate::mc1::combat::MailTarget;
use crate::mc1::mobs::{MobCtx, PLAYER_TARGET};

/// MC2-native projectile marker on [`Ent::flags`] (see
/// [`super::mobs`] for the other high bits). MC1-fallback projectiles
/// spawned on the MC2 column never carry it, so the class-9 dispatch
/// can tell the columns apart without guessing at state numbers.
pub(crate) const F_MC2PROJ: u32 = 1 << 29;
/// byte[0] bit 1 — the flyer's "aim acquired" latch (EF:62904).
pub(crate) const F_AIMED: u32 = 2;

/// A/B toggle for the chord march's muzzle admission (OPEN-7): set
/// `MGC_NO_MUZZLE_ADMISSION` to restore the pre-dig behaviour, where
/// every sub-step from the muzzle out could detonate the shot.
fn no_muzzle_admission() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MUZZLE_ADMISSION").is_some())
}

/// The virtual projectile the acquisition scan scores from — either
/// a live flyer on its first tick (`mc2_autoaim`) or the crosshair
/// instrument's would-be launch (`World::mc2_aim_preview`).
pub(crate) struct AimProbe {
    pub x: u16,
    pub y: u16,
    pub z: i16,
    pub yaw: u16,
    pub pitch: u16,
    /// The PROJECTILE model — keys the candidate lists.
    pub model: u8,
    pub own: u16,
    /// Lightning's wizard range = minSpeed · maxLife (EF:54896);
    /// unused for every other model.
    pub reach: i64,
}

impl Gen {
    // ---- class-9 creators ---------------------------------------------------

    /// `SummonFireball_4D2E0` (EF:34729) — the (9,0) bolt every
    /// creature ranged attack resolves into: action 0, speed 384,
    /// life 0x2000/384 = 21, mana 50, row 64, sprite 340. (The
    /// trailing `AddEvent2_847D0` dynamic light is presentation.)
    pub(crate) fn mc2_spawn_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            e.model65 = 0;
            e.tick70 = 0;
            e.f126 = 384;
            e.f128 = 384;
            e.f140 = 50;
            e.max_life = (0x2000 / 384) as u32; // 21
            e.row156 = 64;
            e.flags = (e.flags & !8) | F_MC2PROJ;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 340);
        Some(i)
    }

    /// `sub_4D500` (EF:34810) — the (9,3) METEOR SHOT (spell 9's
    /// projectile; the doomsday pyramid's case-9 summon): action 3,
    /// speed 384, life 21, mana 50, row 60 (yaw/pitch caps 22),
    /// sprite 76, untargetable. The launcher arms impact/damage/fuse
    /// (docs/traces/mc2-class9-m3-m26.md §1). No RNG in the ctor.
    pub(crate) fn mc2_spawn_meteor_shot(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            e.model65 = 3;
            e.tick70 = 3;
            e.f126 = 384;
            e.f128 = 384;
            e.f140 = 50;
            e.max_life = (0x2000 / 384) as u32; // 21
            e.row156 = 60;
            e.flags = (e.flags & !8) | F_MC2PROJ;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 76);
        Some(i)
    }

    /// `sub_4E180` (EF:35266) — the (9,26) WHIRLWIND SEED (spell 21's
    /// projectile; the pyramid's case-8 summon): the meteor-shot
    /// numbers under action 27 with sprite 320. Its impact
    /// owner-lock-clear (`sub_67890` EF:59181 — a player-avatar
    /// homing-lock release) only fires on the player-cast path; for
    /// the pyramid owner it is retail's own no-op (same doc §3.2).
    pub(crate) fn mc2_spawn_whirlwind_seed(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.mc2_spawn_meteor_shot(x, y, z)?;
        self.ent[i].model65 = 26;
        self.ent[i].tick70 = 27;
        self.mc2_set_sprite(i, 320);
        Some(i)
    }

    /// `sub_66180` (EF:63340, action 3) — the meteor shot's wrapper
    /// around the flyer core: every tick lay one damage-suppressed
    /// (10,0) spark (dword |= 0x10080) at a ±64-box jitter centered
    /// 96 units toward −x/−y of the shot (`rand%0x81 + pos − 160`
    /// per axis, EF:63356-59; 2 draws of its own stream), life 4,
    /// frame 3, yaw inherited. Retail lays it even on the impact tick
    /// (the class stays set until the removal pass). The fuse stamp
    /// onto the impact entity (`v1x->maxLife/life = byte_0x46_70`) is
    /// IDENTITY for the pyramid values (fuse 10 = the meteor ctor's
    /// maxLife 10); the charge-tiered player-cast fuse is separate.
    pub(crate) fn mc2_meteor_shot_tick(&mut self, i: usize, ctx: &MobCtx) {
        self.mc2_flyer_tick(i, ctx);
        let (x, y, z, id, yaw) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30)
        };
        let jx = (self.ent_rand(i) % 0x81) as u16;
        let jy = (self.ent_rand(i) % 0x81) as u16;
        let sx = x.wrapping_add(jx).wrapping_sub(160);
        let sy = y.wrapping_add(jy).wrapping_sub(160);
        if let Some(s) = self.mc2_spawn_fire(sx, sy, z) {
            let e = &mut self.ent[s];
            e.flags |= 0x10080;
            e.id24 = id;
            e.act_life = 4;
            e.frame88 = 3;
            e.f30 = yaw;
        }
    }

    /// `sub_4DC40` (EF:35071) — the (9,20) lob: action 21, speed 394,
    /// life 7680/394 = 19, sprite 196, NO behavior row (the launcher
    /// sets row 65).
    pub(crate) fn mc2_spawn_lob20(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            e.model65 = 20;
            e.tick70 = 21;
            e.f126 = 394;
            e.f128 = 394;
            e.max_life = (7680 / 394) as u32; // 19
            e.flags = (e.flags & !8) | F_MC2PROJ;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 196);
        Some(i)
    }

    /// `sub_4DCC0` (EF:35091) — the (9,21) arc: action 22, speed 394,
    /// life 19, sprite 319, ShiftRot(256, 512).
    pub(crate) fn mc2_spawn_lob21(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            e.model65 = 21;
            e.tick70 = 22;
            e.f126 = 394;
            e.f128 = 394;
            e.max_life = (7680 / 394) as u32;
            e.flags = (e.flags & !8) | F_MC2PROJ;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 319);
        self.mc2_shift_rot(i, 256, 512);
        Some(i)
    }

    /// `sub_4D860` (EF:34942) — the (9,9) bolt (m23's `sub_1D260`
    /// payload, also the player thunder family): action 9, speed 384,
    /// life 3584/384 = 9, mana 50, row 63, sprite 216. (The trailing
    /// `AddEvent2_847D0` sub-effect is presentation.)
    pub(crate) fn mc2_spawn_bolt9(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            e.model65 = 9;
            e.tick70 = 9;
            e.f126 = 384;
            e.f128 = 384;
            e.f140 = 50;
            e.max_life = (3584 / 384) as u32; // 9
            e.row156 = 63;
            e.flags = (e.flags & !8) | F_MC2PROJ;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 216);
        Some(i)
    }

    // ---- the shared flyer flight (sub_65820, EF:62882) ----------------------

    /// `sub_68740` (EF:55221-310) — the REBOUND deflection engine,
    /// gated at every projectile mover's victim-hit site (EF:62939
    /// generic flyer, 58892 archer arrow, 58770 lightning carrier,
    /// 63484/63162 variants). A victim with a live Rebound window
    /// throws the projectile back at its shooter. Cost gate first
    /// (`proj.mana/4 > victim.mana` → it hits normally); then the
    /// impact-pair whitelist: class 10 with subtype ∈ {0,1,9,11,15,
    /// 17,22,67,71,89} (the 0x44-0x46 range FAILS, EF:55247-53), OR
    /// model-13 arrows unconditionally.
    /// On deflect: sound 28, Rebound XP to the deflector
    /// (`sub_6D8B0(victim, 8, 1)` EF:55283), victim mana −quarter,
    /// heading reversed (`f34 = f30 + 0x400`, pitch negated). The
    /// PRECISE tier (T3, `mc2_rebound_precise`) returns it EXACTLY
    /// down the reverse ray with a DOUBLED payload; scatter fans
    /// `rand % 0x2D − 22` (MC2's own window — NOT MC1's 0x5B/45).
    /// The bolt re-owns to the victim, re-homes on the old shooter
    /// (f146), refills life, relinks at the victim, and flies on.
    ///
    /// Deliberate approximations: the human's mana debit is skipped
    /// (the wizard ledger is world-side); the returned bolt's
    /// xtype/xsubtype re-key (EF:55299) is ported for the human
    /// shooter only (3,0 — a pool shooter id has no O(1) slot
    /// resolve). OPEN: pool victims deflect on the authored 0x8000
    /// shield bit and always scatter — RIVAL windows are not yet
    /// mirrored onto their entities.
    pub(crate) fn mc2_rebound_deflect(&mut self, i: usize, hit: MailTarget, ctx: &MobCtx) -> bool {
        // The victim's live-window test (retail `word[0] & 0x8010`).
        let (active, precise) = match hit {
            MailTarget::Player => (self.player_rebound, self.mc2_rebound_precise.0 != 0),
            MailTarget::Pool(j) => (self.ent[j].flags & 0x8000 != 0, false),
        };
        if !active {
            return false;
        }
        // Whitelist (EF:55232-53).
        let (fc, fm, model) = {
            let e = &self.ent[i];
            (e.f68, e.f69, e.model65)
        };
        if !(model == 13
            || (fc == 10 && matches!(fm, 0 | 1 | 9 | 11 | 15 | 17 | 22 | 67 | 71 | 89)))
        {
            return false;
        }
        // Cost gate + debit (pool victims; the human skips,
        // deliberate).
        let quarter = (self.ent[i].f140 / 4).max(0);
        if let MailTarget::Pool(j) = hit {
            if quarter > self.ent[j].f140 {
                return false;
            }
            self.ent[j].f140 -= quarter;
        }
        self.snd(28, i); // the deflection twang
        let deflector = match hit {
            MailTarget::Player => PLAYER_TARGET,
            MailTarget::Pool(j) => self.ent[j].id24,
        };
        if deflector == PLAYER_TARGET {
            self.mc2_cast_xp.0.push((PLAYER_TARGET, 8, 1));
        }
        let shooter = self.ent[i].id24;
        let d = self.ent_rand(i);
        {
            let e = &mut self.ent[i];
            e.f34 = e.f30.wrapping_add(0x400) & 0x7FF;
            e.f32 = e.f32.wrapping_neg() & 0x7FF;
            if precise {
                e.f30 = e.f34;
                e.f44 = e.f44.saturating_mul(2);
            } else {
                e.f30 = (e.f34 as i32 + (d % 0x2D) as i32 - 22) as u16 & 0x7FF;
            }
            e.f146 = shooter; // re-home on the old shooter
            e.id24 = deflector;
            e.act_life = e.max_life as i32;
            if shooter == PLAYER_TARGET {
                e.f66 = 3; // the returned bolt collides with the
                e.f67 = 0; // human wizard's kind (EF:55299)
            }
        }
        match hit {
            MailTarget::Pool(j) => {
                let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].z);
                self.move_relink(i, jx, jy, jz);
            }
            MailTarget::Player => self.move_relink(i, ctx.px, ctx.py, ctx.pz),
        }
        true
    }

    /// Class filter of the victim probe `sub_10780` (EF:3766-69):
    /// `xtype == -1` admits anything, else class must match and
    /// `xsubtype == -1` or model must match. The human counts as
    /// class 3 model 0.
    pub(crate) fn mc2_proj_filter(&self, i: usize, hit: Option<MailTarget>) -> Option<MailTarget> {
        let (fc, fm) = (self.ent[i].f66, self.ent[i].f67);
        if fc == 0xFF {
            return hit;
        }
        match hit {
            Some(MailTarget::Pool(v)) => {
                let e = &self.ent[v];
                (e.class64 == fc && (fm == 0xFF || e.model65 == fm))
                    .then_some(hit)
                    .flatten()
            }
            Some(MailTarget::Player) => (fc == 3 && (fm == 0xFF || fm == 0))
                .then_some(hit)
                .flatten(),
            None => None,
        }
    }

    /// `sub_50780` (EF:36912) — the (10,65) STAGGER stamp ctor:
    /// action 0x46 = 70, byte[0] = (&0xF6)|1, position only — no
    /// life override, not map-linked, no extents, no sprite, no RNG.
    /// A one-tick carrier the projectile impact seam aims at its
    /// victim (the flyer copy hands it `word_0x96_150` → f146).
    pub(crate) fn mc2_spawn_stagger(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        let e = &mut self.ent[i];
        e.class64 = 10;
        e.model65 = 65;
        e.tick70 = 70;
        e.flags = (e.flags & !0x9) | 1;
        e.x = x;
        e.y = y;
        e.z = z;
        Some(i)
    }

    /// `sub_507C0` (EF:36928) — the (10,66) PARALYZE stamp: the
    /// stagger ctor + subSpell 200 (the mail its tick delivers).
    pub(crate) fn mc2_spawn_paralyze(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.mc2_spawn_stagger(x, y, z)?;
        self.ent[i].model65 = 66;
        self.ent[i].tick70 = 71;
        self.ent[i].f140 = 200;
        Some(i)
    }

    /// `sub_38E70` (EF:28400, action 0x46) / `sub_38F70` (EF:28424,
    /// action 0x47) — the one-tick wizard-debuff stamps: if the
    /// carried victim (`word_0x96_150` → f146) is a wizard body
    /// (class-3 model-0 / the human player), kick it BACKWARD
    /// (`moveBoost_0x1E_30 = -80`) with the 54..57 grunt
    /// (`54 + rand&3` on the stamp's own stream); the paralyze
    /// variant additionally mails its subSpell (200, `sub_11900`)
    /// and arms the mobilize stun. Then self-despawn.
    ///
    /// The flight-struct channels have ported homes:
    /// `moveSpeed_0x14C_332` 0..3 stagger ramp (65) and
    /// the `mobilizeCounter_0x14E_334` stun latch (66) queue through
    /// `Gen::mc2_debuffs` into the boundary's
    /// `flight::Mc2Ext`; the kick rides `player_knock` (backward =
    /// pyaw + half turn — the retail `moveBoost = −80`, EF:28411/
    /// 28437, both variants). The `SetPaletteModification_5C830`
    /// GREEN tint (subMod 3, EF:31935-32002: R and B darkened by
    /// `56*count>>8`, green untouched — NOT the subMod-2 red damage
    /// flash) is presentation — the app reads the slow level off
    /// the ext. Rival bodies take the mail + grunt
    /// (their brain owns their movement — no positional kick
    /// channel).
    pub(crate) fn mc2_debuff_stamp_tick(&mut self, i: usize, ctx: &MobCtx) {
        let (victim, id, amt) = {
            let e = &self.ent[i];
            (e.f146, e.id24, e.f140 as u32)
        };
        let paralyze = self.ent[i].tick70 == 71;
        if victim == PLAYER_TARGET {
            let grunt = 54 + (self.ent_rand(i) & 3) as u8;
            self.snd_player(grunt);
            self.player_knock = ((ctx.pyaw.wrapping_add(1024)) & 0x7FF, 80);
            if paralyze {
                self.mc2_debuffs.stun = self.mc2_debuffs.stun.saturating_add(1);
                self.mail_write(MailTarget::Player, 0, amt, id);
            } else {
                self.mc2_debuffs.slow = self.mc2_debuffs.slow.saturating_add(1);
            }
        } else {
            let v = victim as usize;
            if v != 0
                && v < self.ent.len()
                && self.ent[v].class64 == 3
                && self.ent[v].model65 == 0
                && self.ent[v].flags & 0x400 == 0
            {
                let grunt = 54 + (self.ent_rand(i) & 3) as u8;
                self.snd(grunt, v);
                if paralyze {
                    self.mail_write(MailTarget::Pool(v), 0, amt, id);
                }
            }
        }
        self.ent[i].flags |= 0x400;
    }

    /// Impact-effect spawn (the sub_65820 expiry block, EF:62972-96):
    /// spawn `(f68, f69)` at the flyer's position, hand it the id,
    /// heading, victim and carried damage. Routed: fire, big
    /// explosion, meteor, whirlwind, blast23, and the (10,65)/(10,66)
    /// debuff stamps (the (9,20)/(9,21) lobs' payloads). Unported
    /// effects apply the damage directly (deliberate) and count the
    /// misfit.
    ///
    /// ⚠ This seam folds THREE retail impact workers into one, and
    /// they do NOT agree about the spawned effect's leader — see the
    /// stamp block at the tail.
    fn mc2_proj_impact(&mut self, i: usize, victim: u16, ctx: &MobCtx) {
        let (fc, fm, x, y, z, id, yaw, pitch, dmg, act, lock) = {
            let e = &self.ent[i];
            (
                e.f68, e.f69, e.x, e.y, e.z, e.id24, e.f30, e.f32, e.f44, e.tick70, e.f146,
            )
        };
        let spawned = match (fc, fm) {
            (10, 0) => self.mc2_spawn_fire(x, y, z),
            (10, 1) => self.mc2_spawn_big_explosion(x, y, z),
            // The possession delivery (the basic `(9,1)` bolt's
            // payload): retail does NOT write the claim from the
            // bolt — it spawns a separate (10,12) CLAIM PULSE entity
            // (`_4A190(&pos, byte_0x43, byte_0x44)`, EF:63306-19 /
            // EF:59053-58) that broadcasts the ch1 channel from its
            // own 9-tick action (docs/traces/mc2-possession-delivery
            // .md §1-§2). The bolt then copies its id/yaw/pitch onto
            // it (EF:63315-17) — the claim's OWNER lane rides the
            // pulse's `id_0x1A_26`, so the pulse must carry the
            // caster or every intake reads the pulse's own slot.
            //
            // The port used to fold the broadcast into a single
            // `area_write` from the bolt: that dropped the entity
            // (the mc2l4 (10,12) 279-row missing family in the first
            // 4,000 pairs, l30 313, l4 779 full-take) AND narrowed
            // the claim to the bolt's own box for one tick instead of
            // the pulse's 512³ box for nine — retail's near-miss
            // claims come from exactly that reach.
            (10, 12) => {
                if let Some(p) = self.mc2_spawn_claim_pulse(x, y, z, false) {
                    let e = &mut self.ent[p];
                    e.id24 = id;
                    e.f30 = yaw;
                    e.f32 = pitch;
                }
                None
            }
            // Possession tiers 1/2 (docs/spell-audit/possession.md):
            // the weak claim pulse PLUS a persistent attract aura —
            // the Mana Magnet (model 54, range 15 tiles) / Mana Lock
            // (model 69, range 20). The aura (`mc2_spawn_aura` +
            // `mc2_aura_tick`) drags unowned mana spheres to the
            // caster and merges under the caster's owner. Range = the
            // tier's `subSpell` (15/20, CD spells.bin); the ctor
            // default 14 is for authored magnets.
            //
            // Retail gates BOTH children on an actual probe victim
            // (`if (v6x)`, EF:59032-59058): a ground stop / expiry
            // with no victim spawns neither the claim pulse nor the
            // aura (unlike basic possession's ground-miss pulse).
            // The claim pulse fires on ANY victim (balls and
            // dwellings claim alike — player retail-verified); the
            // AURA manifests ONLY when the victim is a mana sphere —
            // building/worm possession never magnets, and neither
            // does mid-terrain (PLAYER RETAIL-CERTIFIED 2026-07-27,
            // overruling a decompile pass that read EF:59048 as
            // unconditional inside the victim arm — the recorded
            // gameplay wins; MC1's magnet differs on BOTH counts:
            // it drops its pair mid-terrain too, and never magnets
            // buildings because its scan is balls-only).
            // Tier 2 (Mana Lock, model 69) delivers the FORCED claim:
            // retail's impact spawns the (10,70) steal pulse instead
            // of (10,12) when xsubtype == 69 (EF:59036-39), whose
            // action broadcasts force = 1 (sub_32120 → sub_112D0(1),
            // EF:23559) — the intakes steal unconditionally and set
            // the byte[2]&0x20 claim lock, which weak claims then
            // bounce off.
            (10, 54) | (10, 69) => {
                let struck =
                    victim != 0 && victim != PLAYER_TARGET && (victim as usize) < self.ent.len();
                if struck {
                    // EF:59036-45 — the pulse spawns FIRST (it takes
                    // the lower pool slot; the l24 `want=12 got=54`
                    // rows were the whole aura column shifted by this
                    // one missing allocation), model chosen by the
                    // PAYLOAD: (10,70) forced when the payload is
                    // (10,69), (10,12) weak otherwise.
                    if let Some(p) = self.mc2_spawn_claim_pulse(x, y, z, fm == 69) {
                        let e = &mut self.ent[p];
                        e.id24 = id;
                        e.f30 = yaw;
                        e.f32 = pitch;
                    }
                    let sphere = matches!(
                        (
                            self.ent[victim as usize].class64,
                            self.ent[victim as usize].model65,
                        ),
                        (10, 39) | (10, 40) | (10, 57)
                    );
                    if sphere {
                        if let Some(a) = self.mc2_spawn_aura(x, y, z) {
                            self.ent[a].model65 = fm;
                            self.ent[a].f26 = if fm == 54 { 15 } else { 20 };
                            self.ent[a].id24 = id;
                        }
                    }
                }
                None
            }
            // Crater (spell 16): the action wrapper `sub_66280`
            // (EF:63400-02) overrides the scorch ring's LIFE with the
            // tier charge (6/12/24) — the carve radius grows every
            // 3rd frame, so life IS the tier scaling.
            (10, 11) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_scorch_ring(x, y, z);
                if let Some(s) = s {
                    self.ent[s].act_life = charge as i32; // 6/12/24
                }
                s
            }
            // Meteor (spell 9): the action wrapper `sub_66180`
            // (EF:63372-73) overrides the impact's maxLife with the
            // tier charge `byte_0x46_70` (life_0x1A = 2/5/10) — the
            // per-tier fuse (docs/spell-audit/meteor.md).
            (10, 17) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_meteor(x, y, z);
                if let Some(s) = s {
                    let ml = (charge as u32).max(1);
                    self.ent[s].max_life = ml;
                    self.ent[s].act_life = ml as i32;
                }
                s
            }
            // The ground/quake family (docs/spell-audit/quake-family
            // .md + gravity-cavein.md): each spell's projectile impact
            // routes to a terrain effect whose handler is already
            // ported. The impact tail propagates the tier's
            // subSpell→f140 (damage) and leaves f71 as the ctor's
            // phase seed.
            // Tremor (spell 15): `sub_677D0` (EF:59128-32) sets BOTH
            // lives to `charge & 0xF0` (60/80/120 → 48/80/112) and
            // zeroes the phase seed (byte_0x46_70).
            (10, 71) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_fissure(x, y, z);
                if let Some(s) = s {
                    let ml = (charge & 0xF0) as u32;
                    self.ent[s].max_life = ml;
                    self.ent[s].act_life = ml as i32;
                    self.ent[s].f71 = 0;
                }
                s
            }
            // Earthquake (spell 17): the action wrapper `sub_66160`
            // (EF:63333-35) sets the trail's LIFE = 1× charge
            // (16/32/64), life ONLY — the 8× law belongs to
            // whirlwind's sub_678E0 alone.
            (10, 15) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_fire_trail(x, y, z);
                if let Some(s) = s {
                    self.ent[s].act_life = charge as i32; // 16/32/64
                }
                s
            }
            // Volcano (spell 18): `sub_66250` (EF:63388-90) overrides
            // the dome's MAX life (the radius law R = maxLife|1 →
            // 7/9/11 per tier) and zeroes the phase seed; act_life
            // (the raise duration 17) stays the ctor's.
            (10, 9) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_dome(x, y, z);
                if let Some(s) = s {
                    self.ent[s].max_life = charge as u32; // 7/9/11
                    self.ent[s].f71 = 0;
                }
                s
            }
            // Gravity Well (spell 20): `sub_677A0` (EF:59112-14) sets
            // the flood's LIFE = charge (16/26/40) + phase 0.
            (10, 67) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_flood(x, y, z);
                if let Some(s) = s {
                    self.ent[s].act_life = charge as i32; // 16/26/40
                    self.ent[s].f71 = 0;
                }
                s
            }
            // The whirlwind's action wrapper `sub_678E0` (class-9
            // action 27, EF:59109-22) overrides `AddWind`'s ctor life
            // with `8 * byte_0x46_70` (the tier charge) — THIS is what
            // scales Tornado I/II/III (row-21 tier lives 5/10/10 →
            // 40/80/80 ticks). Without it every tier casts at the ctor
            // default 500 and roams identically.
            (10, 22) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_whirlwind(x, y, z);
                if let Some(s) = s {
                    let ml = 8 * charge as u32;
                    self.ent[s].max_life = ml;
                    self.ent[s].act_life = ml as i32;
                }
                s
            }
            // The CHARGED/repeat fireball's firestorm (spell 0, tier
            // life>=2 → arm 28/(10,76)): retail's action `sub_65B50`
            // (EF:63012-53) spawns the (10,76) fire orb via
            // `sub_65C20`, overrides the head's maxLife to 30 — a
            // brief burst vs the level's 80-life authored firestorm —
            // then LEADERS the hub (EF:63029) and stamps the owner id
            // onto all 25 satellites, swapping the local human's to
            // the star sprite 42 (`SetEntityIndex_49C90` — index
            // only, the row-340 extent quad stays). The hub's leader
            // is the PROJECTILE'S OWN LOCK, not the struck victim —
            // the stamp block at the tail of this fn carries the law
            // and the reason the two structure kinds behave
            // differently. ⚠ `docs/traces/mc2-class10-m76-fire-
            // spheres.md` §7 read the missing struck-write as a
            // remc2 transcription gap; that is SUPERSEDED as the
            // explanation (though not disproven — remc1's twin
            // `sub_52ED0_53210` really does carry a struck-write).
            (10, 76) => {
                let s = self.mc2_spawn_fire_orb(x, y, z);
                if let Some(s) = s {
                    self.ent[s].max_life = 30;
                    self.ent[s].act_life = 30;
                    let human = id == PLAYER_TARGET;
                    let mut n = self.ent[s].f54 as usize;
                    while n != 0 {
                        self.ent[n].id24 = id;
                        if human {
                            self.ent[n].type86 = 42;
                            self.ent[n].frame88 = 0;
                        }
                        n = self.ent[n].f54 as usize;
                    }
                }
                s
            }
            // Steal Mana (13): the (10,25) burst stamps the struck
            // wizard's channel-3 "steal" inbox (retail `sub_33E20` →
            // `sub_10C80` type-3, EF:24817). The ch3 consumers are
            // already ported and drain the victim's personal mana +
            // credit the caster (rivals.rs `mc2_rival_intake`, world.rs
            // `apply_player_damage`). Retail spawns the burst ONLY on a
            // direct class-3 model-0/1 wizard hit (EF:63537) — so gate
            // on the victim: terrain / creature fizzles. Amount = the
            // tier's `sub_spell` (f44 = `dmg`): L1 2000 / L2 4000 / L3
            // 10. The bolt's u8 `f71` can't carry 2000, so we stamp
            // `mail[3]` DIRECTLY (the drain amount, not retail's tier
            // byte). L3's castle-% drain + (10,39) sphere re-emit is the
            // deferred tail — it lands here as the flat 10-point
            // fallback (faithful when the victim has no castle). The AoE
            // spread to OTHER nearby wizards is a refinement (direct
            // victim only for now). docs/spell-audit/steal-mana.md §5.
            (10, 25) => {
                let amt = dmg as u32;
                if victim == PLAYER_TARGET {
                    self.player_mail[3] = (amt, id);
                } else if victim != 0
                    && (victim as usize) < self.ent.len()
                    && self.ent[victim as usize].class64 == 3
                    && matches!(self.ent[victim as usize].model65, 0 | 1)
                {
                    self.ent[victim as usize].mail[3] = (amt, id);
                }
                None
            }
            // Magic Mine (spell 23): the (9,29) carrier lands and places
            // a PERSISTENT proximity mine `(10,78)` (`sub_50840`), not a
            // fireball. The carrier arrives ~15 tiles ahead (maxLife 10 ×
            // speed 384 fuse), snapped to ground by the spawner. It
            // carries the tier lifespan (f44 = subSpell = dmg) and the
            // tier index (f71 = charge); the impact tail stamps the owner
            // (id24). docs/spell-audit/magic-mine.md.
            (10, 78) => {
                let tier = self.ent[i].f71;
                self.mc2_spawn_magic_mine(x, y, tier, dmg as i32)
            }
            // Summon Army (spell 19): the (9,24) carrier lands and spawns
            // a ring of allied class-5 creatures (`sub_51800`→`sub_3A5B0`
            // node ring, collapsed to a direct ring here). The creature
            // MODEL rides f71 (19/2 firefly-or-bee, 25 Cymmerian, 16
            // wyvern), which also sets the army size. NOT a quake — the
            // `byte_0x44_68 = 72` is a MODEL
            // (docs/spell-audit/summon-creatures.md Part B).
            (10, 72) => {
                let model = self.ent[i].f71;
                self.mc2_spawn_summon_ring(x, y, model, id);
                None
            }
            // Alliance (spell 24): the (10,74) conversion executor
            // (`sub_50800` → class-10 action 0x51 = `sub_3A650`,
            // EF:36945/29637) — a SAME-SPECIES AREA CHARM centered on
            // the struck creature. Radius = the tier charge f71
            // (16/26/32 tiles), duration = f44 (subSpell 610/1100/
            // 2710 ticks), owner = the caster. ZERO damage anywhere —
            // neither the flyer path nor the handler hurts anything.
            // A victimless detonation (terrain hit) fizzles, like
            // retail's executor with no `word_0x96_150`.
            (10, 74) => {
                let radius = self.ent[i].f71 as i32;
                self.mc2_alliance_convert(victim, id, radius, dmg as i32);
                None
            }
            (10, 23) => self.mc2_spawn_blast23(x, y, z),
            // Lightning L1/L2 storm burst (`sub_66FD0`'s hard-coded
            // `(10,38)` spawn, EF:58813). Full retail internals (the
            // chained second-order `(9,9)` beam, exact life/sprite) are
            // untraced — interim: a one-shot area-damage flash carrying
            // the tier's subSpell (via the f140 tail below), so the
            // storm is visible + damaging and the (9,9) misfit is gone.
            (10, 38) => self.mc2_spawn_lightning_burst(x, y, z),
            (10, 65) => self.mc2_spawn_stagger(x, y, z),
            (10, 66) => self.mc2_spawn_paralyze(x, y, z),
            // The Cave-In ground effect: the action-31 wrapper's
            // post-impact fixup rides here (sub_67910 EF:59218-30 —
            // maxLife = the tier charge, phase reset to 0).
            (10, 89) => {
                let charge = self.ent[i].f71;
                let s = self.mc2_spawn_cave_in(x, y, z);
                if let Some(s) = s {
                    self.ent[s].max_life = charge as u32;
                    self.ent[s].f71 = 0;
                }
                s
            }
            _ => {
                self.note_misfit(fc as u16, fm as u16);
                let amt = dmg as u32;
                self.area_write(i, 0, amt, ctx, false, false);
                None
            }
        };
        if let Some(s) = spawned {
            // ⭐ THE LEADER STAMP IS NOT UNIVERSAL. Three retail impact
            // workers fold into this seam and only two of them
            // struck-stamp: `sub_65820` (the generic expiry, EF:62992)
            // and `CastPosses_65F60` (EF:63557) hand the effect the
            // STRUCK victim, but the FIREBALL worker `sub_65C20`
            // (EF:63057) never writes the effect's leader at all — it
            // only ZEROES the projectile's OWN homing lock when nothing
            // was struck (EF:63195-96). Its action-29 wrapper
            // `sub_65B50` then copies that lock onto the (10,76)
            // firestorm hub (EF:63027-29); action 0's wrapper
            // `CastPlayerFire_65B30` (EF:63005-09) copies nothing, so
            // the plain (10,0) splat keeps its memset 0.
            //
            // ⭐ That upstream lock IS the structural distinction
            // between a castle and a (10,45) building under a charged
            // fireball: `sub_67CB0` case 0x1C walks the class-3 list
            // (castles scored by `sub_685D0`, EF:54783/54790) and the
            // class-5 buckets, and NEVER the building list
            // `dword_38527` — which only the model 1/0x11 possession
            // arm reaches (EF:55047). A fireball can LOCK a castle; it
            // can never lock a building. [`Gen::mc2_aim_lists`] already
            // has this exactly (model 0x1C = wizards + creatures, no
            // buildings). Leader = a castle → the hub's phase-0 sizing
            // takes the 3392/640 bounds off `leader.f80` (`sub_339B0`
            // EF:24581-90) and the per-tick HARD SNAP re-centres the
            // ring on `leader.pos.z + leader.f78` (`sub_33C70`
            // EF:24722-45) = the engulf. Leader 0 → the AUTHORED
            // 192/480 compact ring (EF:35950-51) floats where the ball
            // died, riding the building's own stamped heightmap
            // (EF:27341) = "spins and runs above the flag".
            //
            // Damage is untouched either way (`sub_33C00` EF:24700-14,
            // 70 per satellite from the satellite's own quad).
            let leader = match act {
                29 => lock,
                0 => 0,
                _ => victim,
            };
            let e = &mut self.ent[s];
            e.id24 = id;
            e.f30 = yaw;
            // Every impact worker stamps BOTH bearing words —
            // `v->yaw; v->pitch` (sub_65C20 EF:63194-95, sub_65820
            // EF:62990-91, CastPosses EF:63316-17); mc2l0 t=2817's
            // (10,0) records pitch 97 = the dying bolt's.
            e.f32 = pitch;
            e.f146 = leader;
            e.f140 = dmg as i32; // subSpellIndex rides onto the effect
        }
        // The impact XP award (`sub_6D8B0(id, spell, 1)` on a victim
        // hit — EF:63189 fireball, EF:58411 lightning, the §1.1
        // table): player casts carry their spell index in f40; the
        // world tick drains the mail into the book. Rival and
        // creature owners never award — sub_6D8B0's own guard is
        // `class == 3 && model == 0`, the HUMAN wizard only
        // (EF:58240-41); retail rivals have no spell-XP progression.
        if victim != 0 && id == PLAYER_TARGET && self.ent[i].flags & F_MC2PROJ != 0 {
            let spell = self.ent[i].f40;
            if spell < 26 {
                self.mc2_cast_xp.0.push((id, spell, 1));
            }
        }
        self.ent[i].flags |= 0x400;
    }

    /// `sub_66750` (EF:58268) — the tier-0 LIGHTNING BEAM: a ONE-TICK
    /// hitscan, not a traveling ball. Retail re-aims ONCE at launch —
    /// `sub_66610` (EF:63583-99) runs the one-shot `sub_67CB0`
    /// acquisition and FULLY SNAPS yaw/pitch onto the pick — then
    /// walks a dead-STRAIGHT ray to the first blocker (no per-step
    /// homing anywhere in the beam) and detonates the `(10,23)` blast
    /// at the terminus, which for a victim hit IS the victim
    /// (position snap, EF:63604-08). The trail heading is saved AFTER
    /// the snap (EF:58306-08) — that is what makes the retail flash
    /// point at the locked target and jump target-to-target as each
    /// RAPID re-fire re-scans. The flyer core marches, probes, and
    /// applies the terrain/victim/impact law — run it to COMPLETION
    /// with the lock CLEARED so it flies straight; victim stamping
    /// rides the probe like retail's re-probe (EF:58401-21). Net:
    /// fire → instant aimed flash → gone, re-laid every RAPID tick
    /// (docs/spell-audit/lightning.md §5.A).
    pub(crate) fn mc2_lightning_beam_tick(&mut self, i: usize, ctx: &MobCtx) {
        // EF:58303 — the walk runs at minSpeed.
        self.ent[i].f126 = self.ent[i].f128;
        // The one-shot acquisition + full snap (`sub_66610`
        // EF:63586-98). A fresh cast always lands here (the a3==7
        // dispatch stamps no victim); no target = freeze the facing.
        if self.ent[i].flags & F_AIMED == 0 {
            self.ent[i].flags |= F_AIMED;
            if self.mc2_autoaim(i, ctx) {
                let e = &mut self.ent[i];
                e.f30 = e.f34;
                e.f32 = e.f36;
            } else {
                let e = &mut self.ent[i];
                e.f34 = e.f30;
                e.f36 = e.f32;
            }
        }
        let (sx, sy, sz, yaw, pitch, speed, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.f126.max(384), e.id24)
        };
        // Straight march: the beam never homes per-step, so the lock
        // is held aside for the walk (F_AIMED is latched, so the
        // flyer neither re-acquires nor steers). maxLife (~9) bounds
        // the reach; the 64 cap is a pure safety backstop.
        let lock = self.ent[i].f146;
        self.ent[i].f146 = 0;
        let mut steps = 0i32;
        for _ in 0..64 {
            steps += 1;
            self.mc2_flyer_tick(i, ctx);
            if self.ent[i].flags & 0x400 != 0 {
                break;
            }
        }
        self.ent[i].f146 = lock;
        // Enhanced-lightning presentation feed: the resolved strike,
        // muzzle → walked terminus (hash-silent, drained by the
        // frontend).
        let end = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        if self.bolt_fx.0.len() < 256 {
            self.bolt_fx.0.push(crate::engine::features::BoltStrike {
                start: (sx, sy, sz),
                end,
                owner: id,
            });
        }
        // Lay the VISIBLE jagged flash: `sub_66750`'s cosmetic sprite-216
        // trail (EF:58320) along the AIMED heading, `steps·8` nodes at
        // `actSpeed/8` spacing (EF:58321-23) — its end coincides with
        // the walked terminus by construction. `i` is despawned here
        // but its fields are still live.
        self.mc2_lay_lightning_trail(i, (sx, sy, sz), steps, yaw, pitch, speed, id);
    }

    /// `sub_66750`'s trail (EF:58320-58399): sprite-216 billboards along
    /// the beam every `actSpeed/8` (=48) units — `steps·8` of them, so
    /// the trail length equals the walked distance (EF:58321) —
    /// jittered by a ±1 random walk (amplitude clamp 8, tapering to 0
    /// at the far end). Each node is a 1-frame self-despawning
    /// class-9/model-9 billboard (action 14 = `sub_67410`). These ARE
    /// the visible flash.
    #[allow(clippy::too_many_arguments)]
    fn mc2_lay_lightning_trail(
        &mut self,
        src: usize,
        start: (u16, u16, i16),
        steps: i32,
        yaw: u16,
        pitch: u16,
        speed: i16,
        id: u16,
    ) {
        let (sx, sy, sz) = start;
        let spacing = (speed as i32 / 8).max(16); // 48 at actSpeed 384
        let n = (steps * 8).clamp(1, 96);
        let unit = (spacing / 4).max(1) as i16; // 12
        let perp = yaw.wrapping_add(512) & 0x7FF; // +90°
        let (mut wz, mut wp) = (0i32, 0i32);
        let jag =
            |w: i32, amp: i32, r: u32| (w + 2 * ((r % 0x9D) as i32 / 79) - 1).clamp(-amp, amp);
        for k in 1..=n {
            let mut p = (sx, sy, sz);
            Self::polar_step(&mut p, yaw, pitch, (spacing * k) as i16);
            let amp = ((n - k) / 2).clamp(0, 8);
            let r = self.ent_rand(src);
            wz = jag(wz, amp, r);
            let r = self.ent_rand(src);
            wp = jag(wp, amp, r);
            p.2 = p.2.wrapping_add((wz as i16).wrapping_mul(unit));
            Self::polar_step(&mut p, perp, 0, (wp as i16).wrapping_mul(unit));
            if let Some(nn) = self.mc2_spawn_lightning_node(p.0, p.1, p.2, src) {
                self.ent[nn].id24 = id;
            }
        }
    }

    /// One `sub_66750` trail billboard: class-9 model-9 sprite-216,
    /// action 14 (`sub_67410` = pure life-- decay). Born DEAD by the
    /// slot compare `maxLife = (node >= beam) - 1` (EF:58341): a node
    /// ahead of the beam's slot gets 0 (the ascending frame pass
    /// still ticks it this frame), one behind gets -1 — either way
    /// the disabled bit lands within a frame and the slot recycles
    /// on retail's schedule. No yaw write — the ctor leaves @0x1C 0.
    fn mc2_spawn_lightning_node(&mut self, x: u16, y: u16, z: i16, beam: usize) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            e.model65 = 9;
            e.tick70 = 14;
            e.max_life = if i >= beam { 0 } else { (-1i32) as u32 };
            e.flags = (e.flags & !8) | F_MC2PROJ;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 216);
        Some(i)
    }

    /// `sub_67410` (EF:58906, action 14) — the inert trail-node tick:
    /// pure `life--`, despawn at `< 0`. No flight, no logic.
    pub(crate) fn mc2_lightning_node_tick(&mut self, i: usize) {
        // EF:58910-12 — the life test reads the PRE-decrement value.
        // Nodes are born at 0/-1 (the EF:58341 slot compare), so the
        // flash lives ~one frame before the disabled bit lands.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
        }
    }

    /// `sub_65820` (EF:62882) — the shared class-9 flyer/projectile
    /// tick: per-tick homing with the behavior row's yaw/pitch caps
    /// (`sub_65610`, EF:62781 — caps v_2/v_6 via `sub_58350`), a ±2
    /// speed ramp toward minSpeed, the polar step, the tile-chain
    /// victim probe under the xtype/xsubtype filter, the per-state
    /// terrain law, the water splash, life expiry, and the
    /// (f68, f69) impact spawn.
    ///
    /// TERRAIN LAW (docs/traces/mc2-projectile-terrain-water.md):
    /// every ballistic state DETONATES on terrain contact
    /// (`getTerrainAlt > z`,
    /// EF:62950/63135 — the contact clamp only places the burst);
    /// POSSESSION (action 18) alone runs a PRE-move ground-raise
    /// (EF:63262-64) and therefore skims — and has NO water arm.
    /// The water test is NESTED inside the contact branch
    /// (EF:62956/63141): only a projectile flying AT the water
    /// surface splashes; flight over water never runs it.
    /// Would projectile `i`, standing at `at`, already be overlapping
    /// the victim the chord march just found? The muzzle-admission
    /// test (OPEN-7) — see the march in [`Self::mc2_flyer_tick`].
    fn mc2_hit_covers(
        &mut self,
        i: usize,
        at: (u16, u16, i16),
        h: MailTarget,
        ctx: &MobCtx,
    ) -> bool {
        let old = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        self.ent[i].x = at.0;
        self.ent[i].y = at.1;
        self.ent[i].z = at.2;
        let v = match h {
            MailTarget::Pool(j) => self.ent_overlap(i, j),
            MailTarget::Player => self.player_overlap(i, ctx),
        };
        self.ent[i].x = old.0;
        self.ent[i].y = old.1;
        self.ent[i].z = old.2;
        v
    }

    /// `CastCastleProjectile_66B30` (EF:58461) + its create-arm body
    /// `sub_66D00` (EF:58556) — the MC2 (9,10) castle ball. NOT the
    /// generic flyer: it homes on the DEST POINT (create) or the
    /// bound castle entity (upgrade, `word_0x96_150` → f146), runs
    /// the castle-cast SITE TEST as a per-tick tripwire, has NO
    /// water arm (retail builds on whatever it lands on), and its
    /// landing spawns the descriptor pair (f68,f69) — (3,2) create /
    /// (10,43) upgrade — AT THE BALL'S POSITION, owner-stamped
    /// (mc2l3 t=241-244: cast at 241, ball armed-unmoved at its
    /// birth boundary, homing turn 789→810 / 205→183, terrain
    /// contact during tick 244, castle (3,2) born same tick at the
    /// landing tile (26368,49408) — the generic flyer's water arm
    /// was eating exactly this build).
    ///
    /// The ARM state (retail byte0&2 → the imported flags bit 1):
    /// the record shows the ball ARMED at its first boundary with
    /// the launch site test already taken, so the cast folds the
    /// sub_66D00 head into mint time ([`World::cast_castle`]); an
    /// UNARMED ball here (an authored/THING spawn) takes the head
    /// as its first tick — site test at the launch pose, no move.
    pub(crate) fn mc2_castle_ball_tick(&mut self, i: usize) {
        let tgt = self.ent[i].f146 as usize;
        let upgrade_flight = tgt != 0 && tgt != PLAYER_TARGET as usize && tgt < self.ent.len();
        if !upgrade_flight && self.ent[i].flags & 2 == 0 {
            // sub_66D00's head: latch the arm bit, site-test the
            // launch pose, and do NOT move. A refusal despawns (the
            // sub_88D00 "can't build here" flash is app-side; the
            // cast lock is derived — `mc2_castle_lock_active`).
            self.ent[i].flags |= 2;
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            if !self.mc2_castle_cast_site_ok(x, y) {
                self.ent[i].flags |= 0x400;
            }
            return;
        }
        // ---- the shared flight: ease yaw/pitch toward the target
        // at the behavior row's caps (sub_58350's v_4 arg is dead —
        // the single-cap `turn_step` fold, the flyer's precedent),
        // ease speed ±2 toward min, one polar step. ----
        let (px, py, pz) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let (tx, ty, tz) = if upgrade_flight {
            let c = &self.ent[tgt];
            (c.x, c.y, c.aim_z())
        } else {
            let e = &self.ent[i];
            (e.dest_x, e.dest_y, e.site_z)
        };
        let tgt_yaw = Self::angle_between(px, py, tx, ty);
        let dh = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
        let tgt_pitch = Self::pitch_toward(pz, tz, dh);
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let (cy, cp) = (row.v_2, row.v_6);
        {
            let e = &mut self.ent[i];
            e.f34 = tgt_yaw;
            e.f36 = tgt_pitch;
            e.f30 = (e.f30 as i32 + Self::turn_step(e.f30, tgt_yaw, cy) as i32) as u16 & 0x7FF;
            e.f32 = (e.f32 as i32 + Self::turn_step(e.f32, tgt_pitch, cp) as i32) as u16 & 0x7FF;
            e.f126 += (e.f128 - e.f126).clamp(-2, 2);
        }
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        let mut pos = (px, py, pz);
        Self::polar_step(&mut pos, yaw, pitch, speed);
        let mut land = false;
        let mut refused = false;
        // Upgrade arrival: plain overlap with the target snaps the
        // ball onto it (EF:58496-99 / the sub_106C0 test).
        if upgrade_flight {
            let (ox, oy, oz) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            self.ent[i].x = pos.0;
            self.ent[i].y = pos.1;
            self.ent[i].z = pos.2;
            let hit = self.ent_overlap(i, tgt);
            self.ent[i].x = ox;
            self.ent[i].y = oy;
            self.ent[i].z = oz;
            if hit {
                let c = &self.ent[tgt];
                pos = (c.x, c.y, c.z);
                land = true;
            }
        }
        if !land {
            // Terrain contact — floor, or on caves the ceiling at
            // ceiling − fov (EF:58637-48, the same comma arm as the
            // generic core). NO water arm (sub_66D00 has none).
            let ground = self.ground_z(pos.0, pos.1) as i16;
            if ground > pos.2 {
                pos.2 = ground;
                land = true;
            } else if self.is_cave() {
                let c = (self.ceiling_z(pos.0, pos.1) - self.ent[i].f84 as i32) as i16;
                if pos.2 > c {
                    pos.2 = c;
                    land = true;
                }
            }
        }
        if !land {
            // Airborne: life countdown, then the site tripwire
            // (create only — EF:58650-56); a refusal lands HERE with
            // the 180° back-step below.
            self.ent[i].act_life -= 1;
            if self.ent[i].act_life < 0 {
                land = true;
            } else if !upgrade_flight && !self.mc2_castle_cast_site_ok(pos.0, pos.1) {
                land = true;
                refused = true;
            }
        }
        if refused {
            // The retreat step (EF:58662-69): re-step from the
            // committed position at yaw+0x400, live pitch and speed.
            let back = yaw.wrapping_add(0x400) & 0x7FF;
            Self::polar_step(&mut pos, back, pitch, speed);
        }
        self.move_relink(i, pos.0, pos.1, pos.2);
        if !land {
            return;
        }
        let own = self.ent[i].id24;
        let (fc, fm) = (self.ent[i].f68, self.ent[i].f69);
        // The stale-create guard (EF:58528-31): a (3,2) delivery
        // whose owner already holds a BOUND castle just despawns.
        if fc == 3 && self.mc2_castle_of(own).is_some() {
            self.ent[i].flags |= 0x400;
            return;
        }
        // `_4A190(&pos, byte67, byte68)` — the build. A pool-refused
        // spawn leaves the ball ALIVE to retry next tick (EF:58540-42
        // releases the caster's lock instead; ours is derived).
        let spawned = match (fc, fm) {
            (3, 2) => self.spawn_castle(pos.0, pos.1),
            (10, 43) => self.spawn_creator(43, pos.0, pos.1, pos.2),
            _ => None,
        };
        if let Some(c) = spawned {
            self.ent[c].id24 = own;
            self.ent[i].flags |= 0x400;
        }
    }

    pub(crate) fn mc2_flyer_tick(&mut self, i: usize, ctx: &MobCtx) {
        // Homing / acquisition (EF:62902-21).
        match self.mc2_target(self.ent[i].f146, ctx) {
            Some((tx, ty, tz)) => {
                // `sub_65610` steers at the target RAISED to its z-box
                // CENTER (`sub_65580` EF:62750: z += f78 unless MODEL
                // 2 — [`Ent::aim_z`]; `model_0x40_64` IS the model
                // byte, per its own value key "2 - castle": castles
                // home at the FLAG, not 8192 under the base; restored
                // by `sub_655A0` after). The acquisition sites apply
                // the same raise. Without it the meteor aims a
                // half-box low every homing tick and grazes under
                // small high-altitude flyers. The PLAYER is a raised
                // victim too — retail's player is a boxed pool wizard
                // and `sub_65580` lifts it like any other; the
                // pose-only player's box center is pz + PLAYER_HH.
                let target = self.ent[i].f146;
                let tz = if target == PLAYER_TARGET {
                    tz + crate::mc1::combat::PLAYER_HH as i16
                } else {
                    self.ent[target as usize].aim_z()
                };
                let e = &self.ent[i];
                let (yaw, pitch) = (e.f30, e.f32);
                let f34 = Self::angle_between(e.x, e.y, tx, ty);
                let dh = Self::isqrt(Self::dist2_sq(e.x, e.y, tx, ty) as u32) as i32;
                let f36 = Self::pitch_toward(e.z, tz, dh);
                let row = &BEHAVIOR[e.row156 as usize];
                let (cy, cp) = (row.v_2, row.v_6);
                let e = &mut self.ent[i];
                e.f34 = f34;
                e.f36 = f36;
                e.f30 = (yaw as i32 + Self::turn_step(yaw, f34, cy) as i32) as u16 & 0x7FF;
                e.f32 = (pitch as i32 + Self::turn_step(pitch, f36, cp) as i32) as u16 & 0x7FF;
            }
            None => {
                if self.ent[i].flags & F_AIMED == 0 {
                    self.ent[i].flags |= F_AIMED;
                    // One-shot acquisition (`sub_67CB0`): the
                    // FIREBALL states nudge yaw ≤34 units toward the
                    // lock and snap pitch (EF:63106-19, the
                    // "assisted not locked" launch feel —
                    // docs/traces/mc2-mouse-aim.md §5; action 29 =
                    // the charged body, same law, provenance OPEN);
                    // every other state snaps both axes (the generic
                    // init law, EF:62907-13). No target = snapshot and
                    // fly straight (the retail else-arm).
                    if self.mc2_autoaim(i, ctx) {
                        let (yaw, dy, dp, act) = {
                            let e = &self.ent[i];
                            (e.f30, e.f34, e.f36, e.tick70)
                        };
                        let e = &mut self.ent[i];
                        if matches!(act, 0 | 29) {
                            e.f30 =
                                (yaw as i32 + Self::turn_step(yaw, dy, 34) as i32) as u16 & 0x7FF;
                        } else {
                            e.f30 = dy;
                        }
                        e.f32 = dp;
                    } else {
                        let e = &mut self.ent[i];
                        e.f34 = e.f30;
                        e.f36 = e.f32;
                    }
                }
            }
        }
        // Speed ramp toward minSpeed (EF:62923-31) — the shared
        // `sub_65820` core only. States 0 (`sub_65C20`, moves at
        // actSpeed verbatim EF:63126), 1 (`CastPosses_65F60`,
        // EF:63261) and 29 (`sub_65B50` = a charged-impact wrapper
        // over the state-0 body, EF:63023) have NO ramp line;
        // folding them into this tick must not synthesize one.
        // Corpus: retail (9,0)/(9,1) speed holds constant across
        // whole flights while the ramp pulled the port toward 384
        // (the mc2 takes' +2/−2 speed family, sign = speed vs 384).
        if !matches!(self.ent[i].tick70, 0 | 1 | 29) {
            let e = &mut self.ent[i];
            if e.f126 < e.f128 {
                e.f126 += 2;
            } else if e.f126 > e.f128 {
                e.f126 -= 2;
            }
        }
        // Polar step + victim probe. Retail's probe (`sub_10780`,
        // EF:3739) ray-marches the MAP CELLS along the flight — an
        // end-point-only test TUNNELS at cast speeds (the boost
        // clamp allows up to 0x2000/tick, and several projectile
        // sprites carry a zero-width box, e.g. the fireball's row
        // 340 speed_6 = 0). March the chord in ≤128-unit sub-steps
        // and probe each; the movement itself stays the single polar
        // step (trajectory unchanged).
        // Possession rides the CLAIM probe `sub_108B0`
        // (`claim_victim_scan`) instead of the generic `sub_10780` —
        // it detonates only on claimable targets (mana spheres,
        // possessable buildings, worms) and flies through everything
        // else (the un-possessable factory sinks / spires). Every
        // other spell uses the generic any-solid probe. This is the
        // ONE player spell with the whitelist behavior, and
        // `sub_108B0` has exactly TWO callers, both possession:
        // `CastPosses_65F60` (action **1**, the basic (9,1) bolt,
        // EF:63285) and `sub_674C0` (action 18, the leveled (9,17),
        // EF:59003). Action 1 was missing from this gate, so every
        // basic possession bolt — including the (9,1)s the corpus
        // importer replays — ran the generic any-solid probe and
        // detonated on the first thing it grazed: the mc2l30 (10,12)
        // claim-pulse family came out 714 extra against retail's 258
        // (and the port's bolt skipped the skim clamp below, so its
        // z ran high the whole flight).
        let is_possess = matches!(self.ent[i].tick70, 1 | 18);
        let e = &self.ent[i];
        let start = (e.x, e.y, e.z);
        let mut pos = start;
        Self::polar_step(&mut pos, e.f30, e.f32, e.f126);
        let dx = pos.0.wrapping_sub(start.0) as i16 as i32;
        let dy = pos.1.wrapping_sub(start.1) as i16 as i32;
        let dz = (pos.2 as i32) - (start.2 as i32);
        let dist = Self::isqrt((dx * dx + dy * dy) as u32) as i32;
        let n = ((dist + 127) / 128).max(1);
        // Possession probes ONCE, at the committed endpoint, after
        // the skim clamps — retail's order is clamp → commit →
        // sub_108B0 (EF:63262-88). No march: every claim target
        // carries a real box (buildings ±2048), so the anti-tunnel
        // march — the documented deviation for zero-width GENERIC
        // sprite boxes — has nothing to close here, and marching
        // would admit mid-chord claims retail's single endpoint
        // probe never sees. The claim scan itself walks the retail
        // ring (see `claim_victim_scan`), not the march's square.
        if is_possess {
            let g = self.ground_z(pos.0, pos.1) as i16;
            if pos.2 < g {
                pos.2 = g;
            }
            if self.is_cave() {
                let c = (self.ceiling_z(pos.0, pos.1) - self.ent[i].f84 as i32) as i16;
                if pos.2 > c {
                    pos.2 = c;
                }
            }
            let hit = self.claim_victim_scan_at(i, pos);
            return self.mc2_proj_land(i, ctx, start, pos, hit);
        }
        // MUZZLE ADMISSION (fools-mana.md OPEN-7). The march is ours,
        // not retail's, and it can see something retail's single
        // end-of-step probe never can: an entity the projectile is
        // ALREADY inside at the START of the step. Retail resolves
        // such an entity at the step's END or not at all — if it had
        // been overlapping at the end of the PREVIOUS step, the
        // previous probe would have consumed the shot — so a victim
        // that already contains the launch point is admitted only at
        // `k == n`, retail's own probe point (EF:63126-29: MoveEntity
        // → CopyEntityPosition → sub_10780, once). Everything the
        // projectile ENTERS mid-chord still detonates at the sub-step,
        // which is the whole point of the anti-tunnel march.
        //
        // Nothing in the corpus exercises this: every launcher stamps
        // an owner the probe's `id24` gate already drops. It closes a
        // latent class — a projectile born co-located with a
        // targetable entity it does not own detonating on tick 1.
        let admit_muzzle = !no_muzzle_admission();
        let mut scanned = None;
        for k in 1..=n {
            let sub = (
                start.0.wrapping_add((dx * k / n) as u16),
                start.1.wrapping_add((dy * k / n) as u16),
                (start.2 as i32 + dz * k / n) as i16,
            );
            let found = self.victim_scan_at(i, sub, ctx);
            if admit_muzzle
                && k < n
                && let Some(h) = found
                && self.mc2_hit_covers(i, start, h, ctx)
            {
                continue; // retail probes this one at the endpoint
            }
            scanned = found;
            if scanned.is_some() {
                pos = sub;
                break;
            }
        }
        let hit = self.mc2_proj_filter(i, scanned);
        self.mc2_proj_land(i, ctx, start, pos, hit);
    }

    /// The shared landing tail of the MC2 flight: rebound gate,
    /// terrain/water contact, life countdown, impact/expiry. `hit`
    /// arrives ALREADY filtered — the xtype/xsubtype narrowing is
    /// retail's, but it lives INSIDE `sub_10780` (EF:3765-68);
    /// `sub_108B0` has no such filter (EF:3820-70, whitelist only),
    /// so the possession caller passes its claim hit through raw —
    /// the basic (9,1) bolt carries `xtype = 10` from its ctor
    /// (EF:34775), and running the generic filter over a CLAIM hit
    /// would swallow worm (5,22) and building (10,45) claims retail
    /// delivers.
    fn mc2_proj_land(
        &mut self,
        i: usize,
        ctx: &MobCtx,
        start: (u16, u16, i16),
        mut pos: (u16, u16, i16),
        hit: Option<MailTarget>,
    ) {
        let is_possess = matches!(self.ent[i].tick70, 1 | 18);
        // The Rebound gate (EF:62939): a shielded victim throws the
        // bolt back at its shooter — no impact, it flies on reversed.
        if let Some(h) = hit
            && self.mc2_rebound_deflect(i, h, ctx)
        {
            return;
        }
        if hit.is_none() {
            let ground = self.ground_z(pos.0, pos.1) as i16;
            // Terrain CONTACT — floor, or on caves the CEILING at
            // ceiling − fov (the comma arm at EF:62951-53/63136-38/
            // 63281-88; floor wins when both, sealed-gap case). A
            // fireball reaching the ceiling detonates exactly as if
            // it hit ground. Possession's post-move test is the same
            // law (EF:63279-90) — after its pre-clamps it only fires
            // across a sealed gap.
            let contact_z = if pos.2 < ground {
                Some(ground)
            } else if self.is_cave() {
                let c = (self.ceiling_z(pos.0, pos.1) - self.ent[i].f84 as i32) as i16;
                (pos.2 > c).then_some(c)
            } else {
                None
            };
            if let Some(cz) = contact_z {
                // Clamp z to PLACE the burst — offensive projectiles
                // never skim. The GENERIC core keeps the post-move
                // x/y under the clamped z (sub_65820 EF:62954), but
                // the FIREBALL body (sub_65C20, actions 0/29 — both
                // its dispatch wrappers, EF:63009/63023) commits the
                // saved PRE-move axis instead (`v16x`, EF:63139-40):
                // the burst REVERTS the dying move, x/y = tick entry,
                // z = the contact read at the post-move cell (mc2l0
                // t=2817 slot 172: retail parks at (14878,6442) 3083
                // where the port flew the full final step).
                pos.2 = cz;
                if matches!(self.ent[i].tick70, 0 | 29) {
                    pos.0 = start.0;
                    pos.1 = start.1;
                }
                // Water tile, nested in the contact branch
                // (EF:62956/63141): (10,5) splash, owner inherited,
                // despawn — no impact effect, no XP. Model gate: the
                // fireball/lightning states exempt 4 only, the
                // generic exempts {4,22,24,26}; models 22/24/26 fly
                // only generic states, so one set serves all.
                // Possession has NO water arm at all (EF:63279-95).
                if !is_possess
                    && !matches!(self.ent[i].model65, 4 | 22 | 24 | 26)
                    && self.cap_bit(pos.0, pos.1) == 1
                {
                    let own = self.ent[i].id24;
                    if let Some(s) = self.mc2_spawn_splash(pos.0, pos.1, pos.2) {
                        self.ent[s].id24 = own;
                    }
                    self.ent[i].flags |= 0x400;
                    return;
                }
                // Dry contact → fall through to the impact block
                // (v14/v20 = 1, EF:62964/63157).
            } else {
                // No contact: life countdown (EF:62966-70).
                self.ent[i].act_life -= 1;
                if self.ent[i].act_life >= 0 {
                    self.move_relink(i, pos.0, pos.1, pos.2);
                    return;
                }
            }
        }
        // Impact / expiry: land on the victim, spawn the effect.
        let victim = match hit {
            Some(MailTarget::Pool(v)) => {
                // Land at the victim's z-box CENTER, not its origin
                // (`sub_65580` raise → CopyEntityPosition → `sub_655A0`
                // restore, EF:62941-43): the impact effect spawns
                // where the box actually is, so its area write
                // (`sub_10C80`'s 3-D window) reaches the victim. At
                // the raw origin a tall-offset flyer (wyvern f78 ≈
                // 937 retail-derived) sat entirely above its own
                // burst. Model-2 exempt ([`Ent::aim_z`]) like every
                // sub_65580 site — castle hits land at the flag.
                let (vx, vy, vz) = {
                    let t = &self.ent[v];
                    (t.x, t.y, t.aim_z())
                };
                self.move_relink(i, vx, vy, vz);
                v as u16
            }
            Some(MailTarget::Player) => {
                // The player is a raised victim too (see the Pool arm:
                // retail's player wizard gets the same `sub_65580`
                // lift) — land at the box center so the burst's area
                // window brackets the player.
                self.move_relink(
                    i,
                    ctx.px,
                    ctx.py,
                    ctx.pz.wrapping_add(crate::mc1::combat::PLAYER_HH as i16),
                );
                PLAYER_TARGET
            }
            None => {
                self.move_relink(i, pos.0, pos.1, pos.2);
                0
            }
        };
        self.mc2_proj_impact(i, victim, ctx);
    }

    // ---- launch helpers ------------------------------------------------------

    /// `sub_5EF70` (EF:60598): poke the target wizard's danger timer.
    /// Pool wizards carry no reader yet (the rival MC2 column).
    pub(crate) fn mc2_danger_poke(&mut self, target: u16) {
        if target == PLAYER_TARGET {
            self.player_danger = 100;
        }
    }

    /// `sub_11900` (EF:4375) — the melee mailbox write: accumulate
    /// `amt` into the target's channel-0 inbox and stamp the attacker
    /// id (MC2 targets carry no per-channel mask; the human's inbox
    /// feeds the World intake).
    pub(crate) fn mc2_melee_write(&mut self, target: u16, amt: u32, src: u16) {
        let tgt = if target == PLAYER_TARGET {
            MailTarget::Player
        } else {
            MailTarget::Pool(target as usize)
        };
        self.mail_write(tgt, 0, amt, src);
    }

    /// The target's (class, model) for the projectile filter bytes —
    /// the human is faithfully (3, 0).
    fn mc2_target_cm(&self, target: u16) -> (u8, u8) {
        if target == PLAYER_TARGET || target as usize >= self.ent.len() {
            (3, 0)
        } else {
            let t = &self.ent[target as usize];
            (t.class64, t.model65)
        }
    }

    /// `sub_582B0` (Sound.cpp:6569) — shortest-arc absolute angular
    /// distance between two 11-bit engine angles.
    pub(crate) fn arc_err(a: u16, b: u16) -> u16 {
        let d = a.wrapping_sub(b) & 0x7FF;
        d.min(0x800 - d)
    }

    /// `sub_68490` (EF:55101) — the acquisition scorer: reject
    /// outside the yaw/pitch cones or beyond 3-D distance 5120,
    /// else score = on-axis(cos)² terms + (4·sin(err))² terms — the
    /// off-axis angular error weighted ×16, so alignment dominates
    /// and distance tie-breaks. The castle variant `sub_685D0`
    /// (EF:55157) is the same law modulo term order — one body
    /// serves both (docs/traces/mc2-autoaim.md §2). Lower = better;
    /// None = rejected. Zero RNG.
    #[allow(clippy::too_many_arguments)]
    fn mc2_aim_score(
        &self,
        probe: &AimProbe,
        tx: u16,
        ty: u16,
        tz: i16,
        yaw_cone: u16,
        pitch_cone: u16,
    ) -> Option<u64> {
        use crate::mc2::sin_lut::SIN_DB750;
        let yaw_err = Self::arc_err(probe.yaw, Self::angle_between(probe.x, probe.y, tx, ty));
        if yaw_err > yaw_cone {
            return None;
        }
        let d2 = Self::dist2_sq(probe.x, probe.y, tx, ty) as i64;
        let dh = Self::isqrt(d2 as u32) as i32;
        let pitch_err = Self::arc_err(probe.pitch, Self::pitch_toward(probe.z, tz, dh));
        if pitch_err > pitch_cone {
            return None;
        }
        // 2-D: retail's `v8 = EuclideanDistXYZ_58490` (EF:55125 /
        // castle twin EF:55181) never reads z — both the 5120 gate
        // and the score's projection terms ride the HORIZONTAL
        // distance; reading z here double-weights altitude and
        // rejects high/low targets early. The candidate prefilters
        // stay genuinely 3-D (sub_583F0, below).
        let dist = dh as i64;
        if dist > 5120 {
            return None;
        }
        let sin = |a: u16| SIN_DB750[a as usize] as i64;
        let cos = |a: u16| SIN_DB750[0x200 + a as usize] as i64;
        let v9 = (dist * cos(yaw_err)) >> 16;
        let v10 = (4 * dist * sin(yaw_err)) >> 16;
        let v11 = (dist * cos(pitch_err)) >> 16;
        let v12 = (4 * dist * sin(pitch_err)) >> 16;
        Some((v11 * v11 + v9 * v9 + v10 * v10 + v12 * v12) as u64)
    }

    // `sub_67CB0` (EF:54710) — the auto-target acquisition, split
    // into the pure scan (`mc2_aim_scan`, shared with the crosshair
    // instrument) + the mutating first-tick lock (`mc2_autoaim`).
    // Best scorer result wins, first-scanned breaks ties.
    // Deliberate approximations (cited):
    // - the owner's lock range rides the wizard row 59's v_28
    //   (4096 — every class-9 row carries the same value; the
    //   out-of-pool human has no row156);
    // - the awake gate `byte_0x39_57` → f58 nonzero (retail's own
    //   truthiness on the byte);
    // - bucket 22 = the worm family, approximated as model-22
    //   heads + their f54 chains;
    // - the cave-in `sub_3A7F0` on-ground filter → z within one
    //   step of the terrain;
    // - the offensive branch's EF:54788 self-self distance is a
    //   flagged decompile artifact — the correct two-point form of
    //   the parallel branches is used (trace §9).

    /// The model-keyed candidate lists + cones (trace §1/§8):
    /// (wizards, creatures, worms_always, spheres, buildings,
    /// yaw cone, pitch cone, wizard alarm, grounded-only). None =
    /// a model with no acquisition switch arm.
    #[allow(clippy::type_complexity)]
    fn mc2_aim_lists(model: u8) -> Option<(bool, bool, bool, bool, bool, u16, u16, bool, bool)> {
        Some(match model {
            0 | 3 | 4 | 0x12 | 0x13 | 0x16 | 0x1A | 0x1C | 0x1E => {
                (true, true, false, false, false, 0x71, 0x71, true, false)
            }
            1 | 0x11 => (false, false, true, true, true, 0x71, 0x71, false, false),
            7 | 8 | 0xB | 0xC => (true, false, false, false, false, 0x71, 0x71, true, false),
            9 => (true, true, true, false, false, 0x71, 0x200, false, false),
            0x10 => (true, true, false, false, false, 0x100, 0x71, true, false),
            0x19 => (false, true, false, false, false, 0x71, 0x71, false, true),
            _ => return None,
        })
    }

    /// The pure acquisition scan under an [`AimProbe`] — the scoring
    /// sweep of `sub_67CB0` with no writes (shared by the live
    /// first-tick lock and the crosshair instrument's preview).
    /// `human` = the human wizard candidate position (None = owner
    /// is the human, or invisible).
    pub(crate) fn mc2_aim_scan(
        &self,
        probe: &AimProbe,
        human: Option<(u16, u16, i16)>,
    ) -> Option<u16> {
        let (wizards, creatures, worms_always, spheres, buildings, yc, pc, _alarm, grounded) =
            Self::mc2_aim_lists(probe.model)?;
        let own = probe.own;
        let range = BEHAVIOR[crate::mc2::behavior::ROW_BASE].v_28 as i64;
        // Lightning's wizard range = the projectile's own reach
        // (minSpeed · maxLife, EF:54896).
        let wiz_range = if probe.model == 9 { probe.reach } else { range };
        let mut best: Option<(u16, u64)> = None;
        let consider =
            |g: &Self, best: &mut Option<(u16, u64)>, slot: u16, pos: (u16, u16, i16), yc, pc| {
                if let Some(sc) = g.mc2_aim_score(probe, pos.0, pos.1, pos.2, yc, pc) {
                    if best.is_none_or(|(_, b)| sc < b) {
                        *best = Some((slot, sc));
                    }
                }
            };
        if wizards {
            // The class-3 family list (wizards/castles/balloons),
            // own-owner and invisibles skipped; range-gated by the
            // owner row. The human is a candidate only for non-human
            // owners (player casts never self-target).
            // LIGHTNING's wizard scan runs a TIGHT pitch cone
            // (sub_67CB0 case 9: 0x71/0x71 for the wizard list vs
            // 0x71/0x200 for creatures, EF:54889-933 — the only
            // model where the two differ). The table pc stays
            // 0x200 for the creature/sphere branches below.
            let wiz_pc = if probe.model == 9 { 0x71 } else { pc };
            for v in 1..self.ent.len() {
                let e = &self.ent[v];
                if e.class64 != 3 || e.flags & 0x400 != 0 || e.act_life < 0 {
                    continue;
                }
                if e.id24 == own || v as u16 == own || e.flags & 0x20 != 0 {
                    continue;
                }
                let dz = (e.z as i64) - (probe.z as i64);
                let d = Self::isqrt(
                    (Self::dist2_sq(probe.x, probe.y, e.x, e.y) as i64 + dz * dz)
                        .min(u32::MAX as i64) as u32,
                ) as i64;
                if d > wiz_range {
                    continue;
                }
                // Castles (3,2) score at the RAW z — the retail walk
                // routes model 2 through the raw-position castle
                // scorer sub_685D0 (EF:54790/54899/54945), same
                // cones/score as the bracketed sub_68490.
                let pos = (e.x, e.y, e.aim_z());
                consider(self, &mut best, v as u16, pos, yc, wiz_pc);
            }
            if let Some((hx, hy, hz)) = human {
                let dz = (hz as i64) - (probe.z as i64);
                let d = Self::isqrt(
                    (Self::dist2_sq(probe.x, probe.y, hx, hy) as i64 + dz * dz).min(u32::MAX as i64)
                        as u32,
                ) as i64;
                if d <= wiz_range {
                    consider(self, &mut best, PLAYER_TARGET, (hx, hy, hz), yc, wiz_pc);
                }
            }
        }
        if creatures {
            // The per-model buckets, worm family (22) excluded here
            // (offensive scans it only as the fallback); awake gate;
            // multipart segments skip like the census.
            for v in 1..self.ent.len() {
                let e = &self.ent[v];
                if e.class64 != 5
                    || e.flags & 0x400 != 0
                    || e.act_life < 0
                    // The awake gate is retail's TRUTHINESS on the
                    // byte (`kx->byte_0x39_57`, EF:54811/54917/54964/
                    // 54992) — not a sign test. Read `<= 0` it
                    // rejected exactly the records the importer hands
                    // over carrying the −6 never-woken sentinel while
                    // admitting the natively-minted 250 that is the
                    // same byte.
                    || (e.f58 & 0xFF) == 0
                    || e.id24 == own
                {
                    continue;
                }
                if e.model65 == 22 && !worms_always {
                    continue;
                }
                if matches!(e.tick70, 0xB4 | 0xE8 | 0xEA) && !worms_always {
                    continue;
                }
                if grounded {
                    let g = self.ground_z(e.x, e.y) as i16;
                    if (e.z - g).unsigned_abs() > 256 {
                        continue;
                    }
                }
                let pos = (e.x, e.y, e.aim_z());
                consider(self, &mut best, v as u16, pos, yc, pc);
            }
        }
        if spheres {
            // The mana-sphere list: unowned or foreign spheres only,
            // AND AWAKE (`if (v26x->byte_0x39_57)`, EF:55024) — a
            // sleeping sphere never attracts the possession lock
            // (mc2l3 t=260: retail's bolt found no awake candidate
            // and flew straight at the cast attitude; the ungated
            // port snapped onto a dormant sphere). Ownership lane by
            // model (EF:55017-31): 39 reads playerEntityIndex@0x94
            // (f144), 57 the fused parentId (id24).
            // Membership is the TICK-TOP ball chain (`dword_38523`),
            // not the live pool: a sphere minted MID-tick is
            // invisible to this tick's acquisition (mc2l3 t=260: the
            // port locked a sphere a lower-slot kill had just
            // scattered; retail's chain predates it and the bolt
            // flew straight). The chain carries 39/40/57; the model
            // test below keeps 40 out like retail's `< 0x27` skip.
            for k in 0..self.ball_chain.list.len() {
                let v = self.ball_chain.list[k] as usize;
                let e = &self.ent[v];
                if !matches!(e.model65, 39 | 57)
                    || e.class64 != 10
                    || e.flags & 0x400 != 0
                    || e.act_life < 0
                    || (e.f58 & 0xFF) == 0
                {
                    continue;
                }
                let owner_lane = if e.model65 == 57 { e.id24 } else { e.f144 };
                if owner_lane == own {
                    continue;
                }
                consider(self, &mut best, v as u16, (e.x, e.y, e.aim_z()), yc, pc);
            }
        }
        if buildings {
            // The buildings list: skip own, the un-possessable
            // (bldgprm byte_2 & 8, EF:55053) and the ASLEEP
            // (`if (i3x->byte_0x39_57)`, EF:55051).
            for v in 1..self.ent.len() {
                let e = &self.ent[v];
                if e.class64 != 10
                    || e.model65 != 45
                    || e.flags & 0x400 != 0
                    || e.act_life < 0
                    || (e.f58 & 0xFF) == 0
                    || e.f144 == own
                {
                    continue;
                }
                if self
                    .assets
                    .bldgprm
                    .get(e.f71 as usize)
                    .is_some_and(|b| b.flags & 8 != 0)
                {
                    continue;
                }
                consider(self, &mut best, v as u16, (e.x, e.y, e.aim_z()), yc, pc);
            }
        }
        if worms_always
            || (best.is_none()
                && matches!(
                    probe.model,
                    0 | 3 | 4 | 0x12 | 0x13 | 0x16 | 0x1A | 0x1C | 0x1E
                ))
        {
            // The worm bucket: case 1 runs it UNCONDITIONALLY,
            // competing with the sphere/building candidates
            // (EF:55071 — no best-empty gate); the big case runs it
            // only as the no-candidate fallback (EF:54825).
            // An AWAKE model-22 HEAD admits
            // its f54 chain members as candidates — the head itself
            // is never scored and the members' own awake bytes are
            // not tested (EF:55071-85).
            for v in 1..self.ent.len() {
                let e = &self.ent[v];
                if e.class64 != 5
                    || e.model65 != 22
                    || e.flags & 0x400 != 0
                    || e.act_life < 0
                    || (e.f58 & 0xFF) == 0
                    || e.id24 == own
                {
                    continue;
                }
                let mut j = self.ent[v].f54 as usize;
                while j != 0 {
                    let s = &self.ent[j];
                    consider(self, &mut best, j as u16, (s.x, s.y, s.aim_z()), yc, pc);
                    j = self.ent[j].f54 as usize;
                }
            }
        }
        best.map(|(target, _)| target)
    }

    /// `sub_67CB0` (EF:54710) — the ONE-SHOT acquisition on a live
    /// flyer's first tick: run the pure scan under the projectile's
    /// own probe, then apply the lock (`sub_655C0` — f146 + desired
    /// aim, target z raised by its half-height) and the "you are
    /// targeted" alarm on wizard locks (`sub_5EF70`).
    pub(crate) fn mc2_autoaim(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let probe = {
            let e = &self.ent[i];
            AimProbe {
                x: e.x,
                y: e.y,
                z: e.z,
                yaw: e.f30,
                pitch: e.f32,
                model: e.model65,
                own: e.id24,
                reach: e.f128 as i64 * e.max_life as i64,
            }
        };
        let human = (probe.own != PLAYER_TARGET && !self.player_invisible)
            .then_some((ctx.px, ctx.py, ctx.pz));
        let Some(target) = self.mc2_aim_scan(&probe, human) else {
            return false;
        };
        let alarm = Self::mc2_aim_lists(probe.model).is_some_and(|l| l.7);
        // `sub_655C0`: the lock + the desired aim toward it (the
        // sub_65580 bracket — model-2 raw, [`Ent::aim_z`]).
        let (tx, ty, tz) = if target == PLAYER_TARGET {
            (ctx.px, ctx.py, ctx.pz)
        } else {
            let t = &self.ent[target as usize];
            (t.x, t.y, t.aim_z())
        };
        let e = &self.ent[i];
        let yaw = Self::angle_between(e.x, e.y, tx, ty);
        let dh = Self::isqrt(Self::dist2_sq(e.x, e.y, tx, ty) as u32) as i32;
        let pitch = Self::pitch_toward(e.z, tz, dh);
        let e = &mut self.ent[i];
        e.f146 = target;
        e.f34 = yaw;
        e.f36 = pitch;
        // `sub_68BD0` (EF:55453, called from the lock at EF:54848 —
        // the big model case only): a lock onto a class-5 model-0
        // victim arms the dragon's 32-tick dodge-alert window
        // ([`Gen::m0_dodge`]).
        if matches!(
            probe.model,
            0 | 3 | 4 | 0x12 | 0x13 | 0x16 | 0x1A | 0x1C | 0x1E
        ) && target != PLAYER_TARGET
        {
            let v = target as usize;
            if self.ent[v].class64 == 5 && self.ent[v].model65 == 0 {
                self.ent[v].f46 = 32;
            }
        }
        if alarm {
            let is_wizard = target == PLAYER_TARGET
                || (self.ent[target as usize].class64 == 3
                    && self.ent[target as usize].model65 == 0);
            if is_wizard {
                self.mc2_danger_poke(target);
            }
        }
        true
    }

    /// Shared field arming every launch thunk performs after the
    /// creator (id, aim, target hand-off, filter bytes).
    pub(crate) fn mc2_arm_proj(&mut self, p: usize, i: usize, target: u16, tpos: (u16, u16, i16)) {
        let (own, f146) = (self.ent[i].id24, self.ent[i].f146);
        let (px, py, pz) = (self.ent[p].x, self.ent[p].y, self.ent[p].z);
        self.ent[p].id24 = own;
        let yaw = Self::angle_between(px, py, tpos.0, tpos.1);
        let dh = Self::isqrt(Self::dist2_sq(px, py, tpos.0, tpos.1) as u32) as i32;
        self.ent[p].f30 = yaw;
        self.ent[p].f34 = yaw;
        let pitch = Self::pitch_toward(pz, tpos.2, dh);
        self.ent[p].f32 = pitch;
        self.ent[p].f36 = pitch;
        self.ent[p].f146 = f146;
        let (tc, tm) = self.mc2_target_cm(target);
        self.ent[p].f66 = tc;
        self.ent[p].f67 = tm;
    }

    // ---- the attack thunks (mc2_chase_attack-compatible) --------------------

    /// `sub_1CE80` (EF:9772): melee within 1024, damage = own f44.
    pub(crate) fn mc2_atk_melee_1024(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        self.mc2_atk_melee(i, target, ctx, 1024)
    }

    /// `sub_1CED0` (EF:9786): melee within 768.
    pub(crate) fn mc2_atk_melee_768(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        self.mc2_atk_melee(i, target, ctx, 768)
    }

    /// `sub_1CF20` (EF:9800): melee within 1536.
    pub(crate) fn mc2_atk_melee_1536(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        self.mc2_atk_melee(i, target, ctx, 1536)
    }

    fn mc2_atk_melee(&mut self, i: usize, target: u16, ctx: &MobCtx, range: u32) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let e = &self.ent[i];
        if Self::mc2_dist3((e.x, e.y, e.z), tpos) >= range {
            return false;
        }
        let (amt, src) = (self.ent[i].f44 as u32, self.ent[i].id24);
        self.mc2_melee_write(target, amt, src);
        true
    }

    /// `sub_1CC20` (EF:9680): the (9,0) bolt — impact (10,0) fire,
    /// row 65, subSpell 500, z-lift = own fov (f84), danger poke.
    pub(crate) fn mc2_atk_bolt(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let (x, y, z, lift) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f84 as i16)
        };
        let Some(p) = self.mc2_spawn_bolt(x, y, z.wrapping_add(lift)) else {
            return false;
        };
        self.ent[p].f68 = 10;
        self.ent[p].f69 = 0;
        self.ent[p].row156 = 65;
        self.ent[p].f44 = 500;
        self.mc2_arm_proj(p, i, target, tpos);
        self.mc2_danger_poke(target);
        true
    }

    /// `sub_1D0E0` (EF:9814): the (9,20) lob — impact (10,65),
    /// row 65, subSpell 780, z-lift = own fov.
    pub(crate) fn mc2_atk_lob20(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let (x, y, z, lift) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f84 as i16)
        };
        let Some(p) = self.mc2_spawn_lob20(x, y, z.wrapping_add(lift)) else {
            return false;
        };
        self.ent[p].f68 = 10;
        self.ent[p].f69 = 65;
        self.ent[p].row156 = 65;
        self.ent[p].f44 = 780;
        self.mc2_arm_proj(p, i, target, tpos);
        self.mc2_danger_poke(target);
        true
    }

    /// `sub_1D1A0` (EF:9847): the (9,21) arc — impact (10,66),
    /// row 65, subSpell 780, fixed z-lift 128.
    pub(crate) fn mc2_atk_lob21(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let Some(p) = self.mc2_spawn_lob21(x, y, z.wrapping_add(128)) else {
            return false;
        };
        self.ent[p].f68 = 10;
        self.ent[p].f69 = 66;
        self.ent[p].row156 = 65;
        self.ent[p].f44 = 780;
        self.mc2_arm_proj(p, i, target, tpos);
        self.mc2_danger_poke(target);
        true
    }

    /// `sub_1D260` (EF:9883): m23's (9,9) heavy bolt — spawned at
    /// pos + fov, impact (10,23), row 64, subSpell 4000.
    pub(crate) fn mc2_atk_heavy9(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let (x, y, z, lift) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f84 as i16)
        };
        let Some(p) = self.mc2_spawn_bolt9(x, y, z.wrapping_add(lift)) else {
            return false;
        };
        self.ent[p].f68 = 10;
        self.ent[p].f69 = 23;
        self.ent[p].row156 = 64;
        self.ent[p].f44 = 4000;
        self.mc2_arm_proj(p, i, target, tpos);
        self.mc2_danger_poke(target);
        true
    }

    /// `sub_1D460` (EF:9918): m18's 5-shot fan — yaw offsets −226,
    /// −113, 0, +113, +226, each a (9,0) with impact (10,0), row 61,
    /// subSpell 800, z-lift 200.
    pub(crate) fn mc2_atk_fan(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let mut fired = false;
        for off in [-226i32, -113, 0, 113, 226] {
            let Some(p) = self.mc2_spawn_bolt(x, y, z.wrapping_add(200)) else {
                continue;
            };
            self.ent[p].f68 = 10;
            self.ent[p].f69 = 0;
            self.ent[p].row156 = 61;
            self.ent[p].f44 = 800;
            self.mc2_arm_proj(p, i, target, tpos);
            let yaw = (self.ent[p].f30 as i32 + off) as u16 & 0x7FF;
            self.ent[p].f30 = yaw;
            self.ent[p].f34 = yaw;
            fired = true;
        }
        if fired {
            self.mc2_danger_poke(target);
        }
        fired
    }

    /// `sub_1CDA0` (EF:9742): m9's (9,13) arrow — z-lift = own roll
    /// (f82), subSpell 600 when owned (f144 set) else 400, sprite 195
    /// doubled (the arrow ctor's own), danger poke.
    pub(crate) fn mc2_atk_arrow(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let (x, y, z, lift, owned) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f82 as i16, e.f144 != 0)
        };
        let Some(p) = self.mc2_spawn_arrow(x, y, z.wrapping_add(lift)) else {
            return false;
        };
        self.ent[p].f44 = if owned { 600 } else { 400 };
        self.mc2_arm_proj(p, i, target, tpos);
        self.mc2_danger_poke(target);
        true
    }
}
