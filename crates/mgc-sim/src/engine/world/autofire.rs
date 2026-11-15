//! AUTOFIRE — the sim's read-only half of the app's
//! `controls.preferences.autofire` input macro (player-set
//! 2026-09-30, a deliberate deviation neither original has).
//!
//! The macro itself lives in the app's input layer: while a fire
//! button is held it re-issues ordinary one-tick CLICKS at a low fixed
//! rate, so the sim sees nothing it could not already see from a
//! patient finger and no cast law moves. All the app needs from here is
//! what the hand is holding — [`World::autofire_hand`] — and that is a
//! pure read: no state, nothing hashed, nothing snapshotted.
//!
//! ## The list
//!
//! Explicit, per game (player ruling): click-only spells that launch a
//! PROJECTILE. Effect spells (speed, shield, heal, …), the blasts and
//! everything whose re-press means something other than "another shot"
//! (MC1 Teleport returns you, Castle buzzes) stay manual, and so do the
//! terrain sculptors for now — adding one is an edit to the two masks
//! below. A spell that already repeats under a held button (MC1's
//! +60==0 set; an MC2 tier whose `byte_0x3B_59` is not 1 — Repeat
//! Fireball) is manual by construction: the raw level must reach it.

use super::{Hx, LifeState, World};
use crate::ids::GameId;

/// What one hand offers the autofire macro this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutofireHand {
    /// Not an autofire spell (or an empty hand): the button's raw
    /// level passes through untouched.
    Manual,
    /// A listed spell, but the wizard cannot cast at all (dead, the
    /// level won, MC2's ending flight running): the chain BREAKS and
    /// needs a fresh click.
    Halted,
    /// A listed spell whose next cast would be refused — its own
    /// cadence gate is still closed, or the purse/castle does not
    /// cover it. The chain holds and fires on the first open tick.
    Wait,
    /// A listed spell and a click now casts.
    Ready,
}

/// MC1 / Hidden Worlds: Fireball, Possess, Meteor, Duel, Steal Mana,
/// and spell 20 — HW's homing meteor, and base MC1's Wall of Fire
/// (player ruling 2026-11-15: it launches a click-only bolt like the
/// rest, so it gets the slow held-button repeat too).
fn mc1_listed(id: usize) -> bool {
    matches!(id, 0 | 3 | 7 | 11 | 13 | 20)
}

/// MC2: Fireball, Possession, Meteor, Steal Mana, Duel, Alliance —
/// every tier (the per-tier click flag drops Repeat Fireball).
fn mc2_listed(spell: i8) -> bool {
    matches!(spell, 0 | 1 | 9 | 13 | 14 | 24)
}

impl World {
    /// The autofire view of one hand (`right` = the RMB hand). Read
    /// BEFORE the tick whose input it shapes, so the purse it judges
    /// is the one the command-site gates will read.
    pub fn autofire_hand(&self, hx: Hx, right: bool) -> AutofireHand {
        let halted =
            self.human_state(hx) != LifeState::Alive || self.won || self.mc2_endseq[hx].is_some();
        if matches!(self.game, GameId::Mc2) {
            self.autofire_hand_mc2(hx, right, halted)
        } else {
            self.autofire_hand_mc1(hx, right, halted)
        }
    }

    /// No MC1 launcher refuses a re-press mid-burst (the command arm
    /// is a bare reload, `sub_46B00_46E40` LABEL_32), so the only
    /// closed gate is the token's own: castle ladder + full cost
    /// (`sub_55DD0_56300`).
    fn autofire_hand_mc1(&self, hx: Hx, right: bool, halted: bool) -> AutofireHand {
        let hand = if right {
            self.players[hx].right
        } else {
            self.players[hx].left
        };
        let Some(id) = hand.map(|s| s.0 as usize) else {
            return AutofireHand::Manual;
        };
        if !mc1_listed(id) || self.players[hx].owned.get(id).is_none_or(|&m| m == 0) {
            return AutofireHand::Manual;
        }
        if halted {
            AutofireHand::Halted
        } else if self.mc1_token_gate(hx, id) {
            AutofireHand::Ready
        } else {
            AutofireHand::Wait
        }
    }

    /// MC2's cast gate (`sub_5F660`, [`World::mc2_cast_gate`]) is per
    /// model, and a press into a LIVE window is only "another shot"
    /// on two arms: Fireball below the charged tier re-arms freely,
    /// and a tiered Possession spends the re-press on a free basic
    /// claim bolt. Everywhere else on the list it is refused (Meteor,
    /// Steal Mana, Alliance, charged Fireball), discarded (tier-0
    /// Possession) or rewrites the running cast's timer instead of
    /// launching (Duel's channel-retrigger arm, EF:60914-28), so the
    /// macro waits the window out.
    fn autofire_hand_mc2(&self, hx: Hx, right: bool, halted: bool) -> AutofireHand {
        let spell = if right {
            self.mc2_book[hx].right
        } else {
            self.mc2_book[hx].left
        };
        if !mc2_listed(spell) {
            return AutofireHand::Manual;
        }
        let m = self.mc2_book[hx].ent[spell as usize] as usize;
        let Some(e) = self.g.ent.get(m).filter(|_| m != 0) else {
            return AutofireHand::Manual;
        };
        // `byte_0x3B_59 == 1` is the click-only family; any other
        // value already repeats while held.
        if e.f59 != 1 {
            return AutofireHand::Manual;
        }
        if halted {
            return AutofireHand::Halted;
        }
        if e.f26 > 0 {
            match (spell, e.f71) {
                (0, 0 | 1) => {}
                // The re-press arm has no mana gate (EF:60900-07).
                (1, 1..) => return AutofireHand::Ready,
                _ => return AutofireHand::Wait,
            }
        }
        // The castle-pool prerequisite (`sub_68D50`) and the gate's
        // full-cost test (EF:60953), as [`World::mc2_afford`] and the
        // cast gate read them.
        let pool_ok = e.f136 <= 0
            || self
                .seat_castle(hx)
                .is_some_and(|c| self.g.ent[c].f140 >= e.f136);
        let purse_ok = self.dev_spells || self.human_mana(hx) as u64 >= e.max_life as u64;
        if pool_ok && purse_ok {
            AutofireHand::Ready
        } else {
            AutofireHand::Wait
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::mc1::spells::SpellId;
    use crate::mc2::spells::{MC2_SPELL_ROWS, Mc2SpellRow};

    fn planes() -> Planes {
        Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        }
    }

    fn assets(spells: Vec<Mc2SpellRow>) -> FeatureAssets {
        FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells,
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        }
    }

    /// An MC1-family world owning every spell with a real purse: the
    /// jars are blue-blessed (no castle ladder), the dev bypass is off.
    fn mc1_world(game: GameId) -> World {
        let mut w = World::new_for_game(planes(), &[], 1, assets(Vec::new()), game);
        w.set_dev_spells(Hx::FIRST, true);
        w.grant_all_spells(Hx::FIRST);
        w.debug_bless_owned_spells(Hx::FIRST);
        w.set_dev_spells(Hx::FIRST, false);
        w.set_human_mana(Hx::FIRST, 1000);
        w
    }

    fn mc1_hand(w: &mut World, id: u8) -> AutofireHand {
        w.players[Hx::FIRST].left = Some(SpellId(id));
        w.autofire_hand(Hx::FIRST, false)
    }

    #[test]
    fn mc1_lists_the_click_projectiles_only() {
        let mut w = mc1_world(GameId::Mc1);
        for id in 0..24u8 {
            // 20 = Wall of Fire (player ruling 2026-11-15).
            let want = if matches!(id, 0 | 3 | 7 | 11 | 13 | 20) {
                AutofireHand::Ready
            } else {
                AutofireHand::Manual
            };
            // Meteor's 10,000 needs a purse to read Ready.
            w.set_human_mana(Hx::FIRST, 100_000);
            assert_eq!(mc1_hand(&mut w, id), want, "MC1 spell {id}");
        }
        w.players[Hx::FIRST].left = None;
        assert_eq!(
            w.autofire_hand(Hx::FIRST, false),
            AutofireHand::Manual,
            "empty hand"
        );
        // Each hand is judged on its own spell.
        w.players[Hx::FIRST].right = Some(SpellId(3));
        assert_eq!(
            w.autofire_hand(Hx::FIRST, true),
            AutofireHand::Ready,
            "right hand"
        );
        w.players[Hx::FIRST].right = Some(SpellId(4));
        assert_eq!(
            w.autofire_hand(Hx::FIRST, true),
            AutofireHand::Manual,
            "right hand"
        );
    }

    #[test]
    fn hidden_worlds_adds_its_homing_meteor() {
        let mut w = mc1_world(GameId::Mc1Hw);
        w.set_human_mana(Hx::FIRST, 100_000);
        assert_eq!(mc1_hand(&mut w, 20), AutofireHand::Ready);
    }

    #[test]
    fn mc1_waits_on_a_short_purse_and_halts_on_death() {
        let mut w = mc1_world(GameId::Mc1);
        w.set_human_mana(Hx::FIRST, 199);
        assert_eq!(mc1_hand(&mut w, 0), AutofireHand::Wait, "fireball is 200");
        w.set_human_mana(Hx::FIRST, 200);
        assert_eq!(mc1_hand(&mut w, 0), AutofireHand::Ready);
        w.set_human_state(Hx::FIRST, LifeState::Dead);
        assert_eq!(mc1_hand(&mut w, 0), AutofireHand::Halted);
        // A manual spell stays manual whatever the wizard's state.
        assert_eq!(mc1_hand(&mut w, 23), AutofireHand::Manual);
        w.set_human_state(Hx::FIRST, LifeState::Alive);
        w.won = true;
        assert_eq!(mc1_hand(&mut w, 0), AutofireHand::Halted, "level won");
    }

    /// A flat MC2 world holding the six listed spells plus Shield,
    /// every row click-only at cost 100 with no castle prerequisite.
    fn mc2_world() -> World {
        let mut spells = vec![Mc2SpellRow::default(); MC2_SPELL_ROWS];
        for s in [0usize, 1, 6, 9, 13, 14, 24] {
            spells[s].byte_0 = 3;
            for t in 0..3 {
                spells[s].tiers[t].mana_cost = 100;
                spells[s].tiers[t].word_0x18 = 5;
            }
        }
        let mut w = World::new_for_game(planes(), &[], 1, assets(spells), GameId::Mc2);
        w.mc2_grant_plausible(
            Hx::FIRST,
            &[(0, 0), (1, 0), (6, 0), (9, 0), (13, 0), (14, 0), (24, 0)],
        );
        w.set_human_mana(Hx::FIRST, 1000);
        w
    }

    fn mc2_hand(w: &mut World, spell: i8) -> AutofireHand {
        w.mc2_book[Hx::FIRST].left = spell;
        w.autofire_hand(Hx::FIRST, false)
    }

    #[test]
    fn mc2_lists_the_click_projectiles_only() {
        let mut w = mc2_world();
        for s in [0i8, 1, 9, 13, 14, 24] {
            assert_eq!(mc2_hand(&mut w, s), AutofireHand::Ready, "MC2 spell {s}");
        }
        assert_eq!(mc2_hand(&mut w, 6), AutofireHand::Manual, "shield");
        assert_eq!(mc2_hand(&mut w, 7), AutofireHand::Manual, "unlearned");
        assert_eq!(mc2_hand(&mut w, -1), AutofireHand::Manual, "empty hand");
        // A tier that repeats on its own (Repeat Fireball) is manual.
        let m = w.mc2_book[Hx::FIRST].ent[0] as usize;
        w.g.ent[m].f59 = 0;
        assert_eq!(mc2_hand(&mut w, 0), AutofireHand::Manual);
    }

    #[test]
    fn mc2_waits_out_a_live_window_except_where_a_repress_is_a_shot() {
        let mut w = mc2_world();
        let arm = |w: &mut World, spell: usize, tier: u8| {
            let m = w.mc2_book[Hx::FIRST].ent[spell] as usize;
            w.g.ent[m].f26 = 3;
            w.g.ent[m].f71 = tier;
        };
        // Fireball: free re-arm below the charged tier.
        arm(&mut w, 0, 0);
        assert_eq!(mc2_hand(&mut w, 0), AutofireHand::Ready);
        arm(&mut w, 0, 2);
        assert_eq!(mc2_hand(&mut w, 0), AutofireHand::Wait);
        // Possession: tier 0 discards the re-press, the tiers spend it
        // — and that arm has no mana gate.
        arm(&mut w, 1, 0);
        assert_eq!(mc2_hand(&mut w, 1), AutofireHand::Wait);
        arm(&mut w, 1, 1);
        w.set_human_mana(Hx::FIRST, 0);
        assert_eq!(mc2_hand(&mut w, 1), AutofireHand::Ready);
        w.set_human_mana(Hx::FIRST, 1000);
        // Everything else waits — Duel above all, whose re-press
        // rewrites the running cast's timer.
        for s in [9usize, 13, 14, 24] {
            arm(&mut w, s, 0);
            assert_eq!(mc2_hand(&mut w, s as i8), AutofireHand::Wait, "spell {s}");
        }
    }

    #[test]
    fn mc2_waits_on_a_short_purse_and_halts_on_death() {
        let mut w = mc2_world();
        w.set_human_mana(Hx::FIRST, 99);
        assert_eq!(mc2_hand(&mut w, 9), AutofireHand::Wait);
        w.set_human_mana(Hx::FIRST, 100);
        assert_eq!(mc2_hand(&mut w, 9), AutofireHand::Ready);
        w.set_human_state(Hx::FIRST, LifeState::Falling);
        assert_eq!(mc2_hand(&mut w, 9), AutofireHand::Halted);
    }
}
