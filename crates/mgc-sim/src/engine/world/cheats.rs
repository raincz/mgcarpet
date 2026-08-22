//! RETAIL CHEAT REPLAY — the one recorded input verb that mutates the
//! world instead of steering it.
//!
//! Both engines expose a cheat menu on control opcode 30 (`0x1E`,
//! `param1` = the sub-code): remc1 :48827-49010, remc2
//! EF:37784-37895. A recording made with cheats on (the `l0`
//! per-spell takes: fly an uncomplicated level, grant yourself the
//! spell under test) therefore carries world mutations that no amount
//! of input reconstruction can produce, and a free-running replay
//! diverges permanently at the first one.
//!
//! The witness is the handler's OWN on-screen message
//! ([`mgc_formats::recover::Cheat`], detected in `recover`): the
//! control command carrying the opcode is memset inside the same event
//! pass, but the toast lands in the per-player block and survives into
//! the capture.
//!
//! ## Fidelity
//!
//! This is a DEV-TOOL seam, not a conformance law: the goal is that a
//! cheated take keeps replaying (the player owns the spell they are
//! about to cast, at the tier they cast it), not that the cheat's own
//! tick reproduces byte-for-byte. Where retail touches state the port
//! does not model, the write is dropped and noted rather than faked:
//!
//! - the in-hand bit `struct_byte_0xc[0] |= 1` on each granted
//!   manifestation, and its `&= ~1` sweep over class-11 records — a
//!   write-only bit here (its lone retail reader is the owned-jar
//!   tint, unmodeled; see `mc2_spell_steal`);
//! - MC2's 10-slot acquisition list `SpellIndexes_0x39B` — MC1's twin
//!   IS modeled (`mc1_acq_push`, it resolves the hand indices), MC2's
//!   has no port consumer.
//!
//! Everything else follows retail's handler verbatim, including the
//! deliberate omissions: the grant does NOT rebind the hands and does
//! NOT re-derive tiers (that is [`Cheat::SpellXp`]'s job, which is why
//! a take exercising tier 1/2 needs both cheats).

use super::{PLAYER_LIFE_MAX, World};
use crate::ids::GameId;
use crate::mc1::mobs::PLAYER_TARGET;
use crate::mc1::spells::{SPELL_COUNT, SpellId};
use mgc_formats::recover::Cheat;

/// The mana a [`Cheat::MoreMana`] sphere carries (remc1 :48910, remc2
/// EF:37822). It is stamped as CLAIMED by the caster, so the wizard's
/// ceiling (`Σ +140` over claimed records) jumps by this much and the
/// `mana = mana_max` that follows banks the lot.
const CHEAT_SPHERE_MANA: i32 = 100_000;

/// One press of [`Cheat::SpellXp`] (remc2 EF:37867).
const CHEAT_XP_STEP: i32 = 100;

impl World {
    /// Apply a cheat the recording says the player fired. Returns
    /// false when this build does not implement it for this game —
    /// consumers should REPORT that rather than swallow it, since the
    /// replay will diverge from that tick on.
    ///
    /// Latches [`World::cheat_mode`] on any accepted cheat: retail
    /// gates several ordinary-looking behaviours on its tester flag
    /// (`setting_byte2_23 < 0`), which is not in the captured closure —
    /// but a cheat firing at all proves the flag is set.
    pub fn apply_cheat(&mut self, cheat: Cheat) -> bool {
        self.cheat_mode = true;
        match self.game() {
            GameId::Mc2 => self.mc2_apply_cheat(cheat),
            _ => self.mc1_apply_cheat(cheat),
        }
    }

    /// True once a recorded cheat has fired — retail's tester flag,
    /// inferred. See [`World::apply_cheat`].
    pub fn cheat_mode(&self) -> bool {
        self.cheat_mode
    }

    /// Would [`World::apply_cheat`] handle this one on this game? Lets
    /// a replay driver report an unhandled cheat up front instead of
    /// leaving the resulting divergence unexplained.
    pub fn cheat_supported(&self, cheat: Cheat) -> bool {
        let mc2 = matches!(self.game(), GameId::Mc2);
        match cheat {
            Cheat::AllSpells | Cheat::MoreMana | Cheat::Heal => true,
            Cheat::SpellXp | Cheat::FreeSpell => mc2,
            _ => false,
        }
    }

    /// remc1 :48836-49010 — the MC1 cheat menu (sub-codes 1..7,
    /// bound to ALT+F1..F7).
    fn mc1_apply_cheat(&mut self, cheat: Cheat) -> bool {
        match cheat {
            // :48838-48884 — mint the owned manifestation of every
            // spell not already held. `grant_spell` IS this ctor loop
            // (class-12 record + `mc1_acq_push`), and like retail it
            // leaves the hands alone.
            Cheat::AllSpells => {
                self.mc1_cheat_all_spells();
                true
            }
            // :48906-48915.
            Cheat::MoreMana => {
                self.cheat_mana_sphere();
                true
            }
            // :48993 — `actLife = <full>`.
            Cheat::Heal => {
                self.player.life = PLAYER_LIFE_MAX;
                true
            }
            _ => false,
        }
    }

    /// remc2 EF:37786-37893 — the MC2 cheat menu (sub-codes 1..10,
    /// bound to ALT+F1..F10).
    fn mc2_apply_cheat(&mut self, cheat: Cheat) -> bool {
        match cheat {
            // EF:37789-37816.
            Cheat::AllSpells => {
                self.mc2_cheat_all_spells();
                true
            }
            // EF:37820-37827.
            Cheat::MoreMana => {
                self.cheat_mana_sphere();
                true
            }
            // EF:37857 — `life = maxLife`.
            Cheat::Heal => {
                self.player.life = PLAYER_LIFE_MAX;
                true
            }
            // EF:37864-37870: +100 volatile XP on every spell, then
            // ONE re-derive pass over all 26 (`sub_6DB50(0, 0)` =
            // bank off, notify off). The order matters — retail awards
            // the whole batch before re-levelling any of it.
            Cheat::SpellXp => {
                for s in 0..26 {
                    self.mc2_book.xp_vol[s] += CHEAT_XP_STEP;
                }
                for s in 0..26 {
                    self.mc2_relevel(s, false, false);
                }
                true
            }
            // EF:37872-37882 — `OptionsSettingFlag_24 ^= 0x20`. Retail
            // reads it only when a manifestation's sub-spell is next
            // (re)built (L:1530), so like retail the toggle does not
            // re-stamp what is already built.
            Cheat::FreeSpell => {
                self.mc2_free_spells = !self.mc2_free_spells;
                true
            }
            _ => false,
        }
    }

    /// remc1 :48838-48884 — mint the owned manifestation of every
    /// spell not already held. `grant_spell` IS the ctor loop
    /// (class-12 record, `mc1_acq_push`, no hand rebind — all three
    /// match retail), but it is also the DEV/plausible instruments'
    /// entry point, where an owned token is deliberately left parked
    /// and flag-clean. Retail's cheat does two things on top, and both
    /// are graded, so they live here rather than in the shared path:
    /// the ctor takes the CASTER'S POSITION (:48866 passes
    /// `&actEvent->position`) and the handler stamps
    /// `flags |= 0x40001` (:48885). (Its `+132 = 0` has no
    /// destination — the port models the class-12 castle requirement
    /// from the static table, never as an entity field; see
    /// `grant_spell`.)
    /// ⚠⚠ And it must NOT publish the owned register. THE OWNED
    /// REGISTER IS DERIVED (`mc1_owned_rebuild`, sub_45C10): retail's
    /// handler tests `+676` and writes only the ACQUISITION list
    /// `+532`, so `+676` keeps its pre-cheat content until the
    /// CARPET's own dispatch rebuilds it — and the carpet is walk slot
    /// 630, below almost every jar in the pool. A jar polling earlier
    /// in the same pass therefore still reads "not owned" and does NOT
    /// take the already-known flag touch (:64789-91). `grant_spell`
    /// publishes the register eagerly, which is right for the dev
    /// instruments and wrong here, so the pre-cheat register is put
    /// back: mc1l0-test slot 139 (a ground Possess jar at walk slot
    /// 139) is exactly this — retail holds it at flags 4 through the
    /// cheat tick, the eager port pushed it to 5.
    fn mc1_cheat_all_spells(&mut self) {
        let (x, y, z) = self.human_pose;
        let owned_before = self.player.owned;
        for s in 0..SPELL_COUNT as u8 {
            if owned_before[s as usize] != 0 {
                continue;
            }
            // The pool can refuse; retail simply skips that spell.
            let Some(m) = self.grant_spell(SpellId(s)) else {
                continue;
            };
            self.g.link(m, x, y, z);
            self.g.ent[m].flags |= 0x0004_0001;
        }
        self.player.owned = owned_before;
        self.cheat_clear_class11_bit0();
    }

    /// The shared `more mana` body (remc1 :48907-48913, remc2
    /// EF:37821-37826): a (10,39) sphere at the caster, carrying
    /// [`CHEAT_SPHERE_MANA`] and stamped as ALREADY CLAIMED by the
    /// caster, then the wizard's own pool topped to its (now much
    /// higher) ceiling.
    fn cheat_mana_sphere(&mut self) {
        let (x, y, z) = self.human_pose;
        if let Some(b) = self.g.spawn_mana_ball(x, y, z) {
            self.g.ent[b].f140 = CHEAT_SPHERE_MANA;
            self.g.ent[b].f144 = PLAYER_TARGET;
            self.g.ball_resize(b);
        }
        // Retail assigns the CEILING word verbatim; the ceiling is
        // recomputed from the claimed set every tick, so the sphere
        // above is what actually makes this large.
        self.player.mana = self.player.mana_max;
    }

    /// remc2 EF:37789-37816 — grant every unheld spell as a real
    /// class-15 manifestation parented to the player, with its mana
    /// upkeep zeroed. NOT a pickup: no cooldown, no hand rebind, no
    /// tier derive (see the module doc).
    fn mc2_cheat_all_spells(&mut self) {
        let (x, y, z) = self.human_pose;
        for s in 0..26usize {
            if self.mc2_book.ent[s] != 0 {
                continue;
            }
            // The pool can refuse; retail simply skips that spell.
            let Some(m) = self.mc2_new_spell_token(s as u8, x, y, z) else {
                continue;
            };
            {
                let e = &mut self.g.ent[m];
                // `manaRegen_0x88 = 0` + `parentId_0x28 = caster`.
                e.f136 = 0;
                e.id24 = PLAYER_TARGET;
            }
            self.mc2_book.ent[s] = m as u16;
        }
        self.cheat_clear_class11_bit0();
    }

    /// `Gen::mc2_spawn_spell_token` plus retail's FREE-SPELL arm.
    ///
    /// Retail applies that arm inside `SetSpell_6D5E0`'s shared
    /// sub-spell body (L:1530-35), which the ctor tail runs too
    /// (L:51120→1505) — so a jar minted while the cheat is on is born
    /// at `manaRegen = 0, mana = 1`, not just one re-tiered later. The
    /// port splits that body in two (`Gen`'s ctor and `World`'s
    /// `mc2_set_spell`) and only the latter can see the flag, so the
    /// World side re-stamps here. Measured: mc2l0-test t=1058 mints a
    /// (15,2) at slot 113 with mana 1 — 430 ticks after the free-spell
    /// toggle, on a path with no `mc2_set_spell` in it at all.
    pub(crate) fn mc2_new_spell_token(
        &mut self,
        model: u8,
        x: u16,
        y: u16,
        z: i16,
    ) -> Option<usize> {
        let m = self.g.mc2_spawn_spell_token(model, x, y, z)?;
        if self.mc2_free_spells {
            let e = &mut self.g.ent[m];
            e.f136 = 0;
            e.f140 = 1;
        }
        Some(m)
    }

    /// The tail both `access all spells` handlers share (remc1
    /// :48889-93, remc2 EF:37812-15): drop bit 0 of every class-11
    /// record's flag word. Retail's own reason is the ground-jar
    /// tint, but the bit IS graded on the MC1 obs lane, so the sweep
    /// is load-bearing for replay: without it the port carries a stale
    /// set bit forever (mc1l0-test slot 139, retail 4 / port 5, the
    /// first field divergence after the grant itself was fixed).
    fn cheat_clear_class11_bit0(&mut self) {
        for i in 1..self.g.ent.len() {
            if self.g.ent[i].class64 == 11 {
                self.g.ent[i].flags &= !1;
            }
        }
    }
}
