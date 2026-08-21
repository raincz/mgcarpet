//! MC2 class-15 spell tokens — the SPELL JARS. Trace bank:
//! docs/traces/mc2-class15-spell-tokens.md (`EF:` = remc2
//! EventsFunctions.cpp, `L:` = Level.cpp).
//!
//! One creator serves all 26 spells (`AddSpellXX_XX_51120` EF:54124
//! behind the 26 thin `AddSpellNN` wrappers): class 15, model = the
//! spell index 0..25, actionIndex = 3*model, sprite 77 for EVERY
//! model, pickup box 768/768/1280. Each model owns three consecutive
//! action states: 3M = the spell EFFECT (gated on an active cast —
//! inert for a fresh token), 3M+1 = pickup, 3M+2 = self-replenishing
//! pickup (collection drops a fresh state-3M+2 token in place). The
//! authored THING's `swi_id` selects the state (`actionIndex +=
//! stageTag`, >= 3 -> junk state 253 — the shared class-12/15 spawn
//! case, EF:33209-33217).

use crate::engine::features::Gen;

impl Gen {
    /// `AddSpellXX_XX_51120` (EF:54124) — the shared token ctor:
    /// maxLife/life 0, byte[0] &= 0xF7 (untargetable), map-linked,
    /// fixed sprite 77 (the jar), pickup half-extents 768/768/1280
    /// (SetEntityShiftRot EF:32874). No RNG. A fresh token's pickup
    /// path never reads the per-spell mana/tier fields.
    pub(crate) fn mc2_spawn_spell_token(
        &mut self,
        model: u8,
        x: u16,
        y: u16,
        z: i16,
    ) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 15;
            e.model65 = model;
            e.max_life = 0;
            e.tick70 = model.wrapping_mul(3);
            e.flags &= !0x8;
        }
        self.link(i, x, y, z);
        self.mc2_set_sprite(i, 77);
        self.extents(i, 768, 1280);
        self.refill_life(i);
        // `SetSpell_6D5E0(event, 0)` runs INSIDE the ctor (the
        // AddSpellXX tail, L:51120→1505): tier-0 row wiring with the
        // PARENT-LESS cost — an authored jar's `parentId_0x28` is 0,
        // so `GetSpellManaCost` (L:1714-18) skips the spell-2 castle
        // ladder and the +3000 arm and returns the raw tier cost.
        // mc2l0 t=3169 slot 114: the dis-fired (15,2) scroll records
        // maxMana 1000, mana 1000/111 = 9. (The World-side
        // `mc2_set_spell` is the LIVE twin — its cost ladder reads
        // the human's castle, wrong for an unowned ground jar; the
        // dev-spells cheat arm never applies at spawn.)
        if let Some(row) = self.assets.spells.get(model as usize) {
            let sub = row.tiers[0];
            let cost = sub.mana_cost;
            let e = &mut self.ent[i];
            e.f71 = 0;
            e.f30 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
            e.f28 = sub.word_0x18.max(0) as u16;
            e.f59 = (sub.font_type & 1 == 0) as u8;
            e.f136 = sub.max_mana_limit;
            e.max_life = cost.max(0) as u32;
            e.f140 = if e.f28 != 0 {
                cost / e.f28 as i32
            } else {
                cost
            };
        }
        Some(i)
    }
}
