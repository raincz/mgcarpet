//! RETAIL CHEAT REPLAY, MC2 arm (`engine::world::cheats`).
//!
//! A recording made with retail's cheat menu on carries world
//! mutations no input reconstruction can produce, so a free replay
//! must apply them or diverge permanently. These pin the three that
//! the l0 per-spell takes actually depend on — grant, tier, cost —
//! against the real SPELLS table, because every one of them reads it.
//!
//! Corpus provenance: `recordings/mc2l0-test.mgcr`, whose 103 cheat
//! fires took `mgc-conform replay`'s bit-exact horizon from 138
//! boundaries (a hard wall at the first cheat) to 1119.
//!
//! Self-skips without baked mc2 data (game data is optional).

use mgc_formats::recover::Cheat;
use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::World;
use mgc_sim::ids::GameId;
use std::path::PathBuf;

#[path = "common/mod.rs"]
mod common;

fn baked_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    (p.join("mc2/level-000.mgcl").exists()
        && p.join("assets/mc2-night/build.tab.bin").exists()
        && !common::modded_bake(&p))
    .then_some(p)
}

fn build_world(root: &std::path::Path) -> Option<World> {
    let file = std::fs::File::open(root.join("mc2/level-000.mgcl")).unwrap();
    let pkg: mgc_formats::LevelPackage = mgc_formats::mgcl::read(file).unwrap();
    let terrain = pkg.terrain.as_ref()?;
    let planes = Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone().unwrap(),
        angle: terrain.angle.clone().unwrap(),
        ceiling: terrain.ceiling.clone().unwrap_or_default(),
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).unwrap();
    let mut assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap()
    .with_bldgprm(bundle.bldgprm.as_deref().unwrap_or_default());
    let sp = bundle.spells.as_deref()?;
    assets = assets.with_spells(sp).unwrap();
    let seed = pkg.gen_params.as_ref().map_or(0, |g| g.seed);
    Some(World::new_for_game(
        planes,
        &pkg.things.things,
        seed,
        assets,
        GameId::Mc2,
    ))
}

macro_rules! world {
    () => {
        match baked_root().and_then(|r| build_world(&r)) {
            Some(w) => w,
            None => {
                eprintln!("skipping: no baked mc2 data");
                return;
            }
        }
    };
}

/// `access all spells` (sub-code 1, EF:37789) mints a real class-15
/// manifestation per unheld spell — the port's ownership test IS that
/// pool slot, so there is no cheaper way to own a spell — and grants
/// them at LEVEL 0. That last part is why a take exercising tier 1/2
/// needs [`Cheat::SpellXp`] as well.
#[test]
fn all_spells_mints_every_manifestation_and_confers_no_tier() {
    let mut w = world!();
    assert!(w.apply_cheat(Cheat::AllSpells));
    let view = w.mc2_book_view();
    assert!(
        view.owned.iter().all(|&o| o),
        "every one of the 26 spells must be owned after the cheat"
    );
    assert!(
        view.levels.iter().all(|&l| l == 0),
        "the grant confers no tier — that is the XP cheat's job"
    );
    assert!(w.cheat_mode(), "any cheat latches retail's tester flag");
}

/// `More Spell Experience Points` (sub-code 8, EF:37864) = +100
/// volatile XP on all 26, then ONE re-derive pass (`sub_6DB50(0, 0)`).
///
/// ⚠⚠ The castle (spell 2) XP clamp is CHEAT-GATED in retail: EF:43885
/// only clamps `xp_vol[2] > 7` when `setting_byte2_23 >= 0`, and that
/// word IS the tester flag. mc2l0-test measures the open arm directly
/// — castle `xp_vol` runs to 7,900 and `levels[2]` reaches 2 on the
/// very first press. Clamping under a cheat would peg every cheated
/// take's castle ladder near tier 0 and break the thing the take
/// exists to exercise.
#[test]
fn spell_xp_raises_tiers_and_leaves_the_castle_unclamped() {
    let mut w = world!();
    assert!(w.apply_cheat(Cheat::AllSpells));
    for _ in 0..8 {
        assert!(w.apply_cheat(Cheat::SpellXp));
    }
    let view = w.mc2_book_view();
    assert!(
        view.xp[2] >= 800,
        "castle XP must not clamp to 7 under the cheat, got {}",
        view.xp[2]
    );
    assert!(
        view.levels.iter().any(|&l| l > 0),
        "800 XP on every spell has to lift some tier off 0"
    );
}

/// `Free Spell Usage` (sub-code 9, EF:37872) is retail's
/// `OptionsSettingFlag_24 & 0x20`, and its ONLY reader is the shared
/// sub-spell build (L:1530) — so it reaches a manifestation when that
/// manifestation is next BUILT, never by re-stamping what already
/// exists. It is a toggle.
///
/// The port splits retail's shared body across `Gen`'s ctor and
/// `World::mc2_set_spell`; `mc2_new_spell_token` is what keeps the
/// ctor arm honest (mc2l0-test t=1058 mints a (15,2) at mana 1, 430
/// ticks after the toggle, on a path with no `mc2_set_spell` in it).
#[test]
fn free_spell_reaches_newly_built_manifestations_only() {
    let mut w = world!();
    assert!(w.apply_cheat(Cheat::AllSpells));
    let paid = w.debug_spell_mana_lanes(2).expect("castle manifestation");
    assert!(
        paid.1 > 1,
        "a normally built castle spell charges more than 1, got {paid:?}"
    );

    assert!(w.apply_cheat(Cheat::FreeSpell));
    assert_eq!(
        w.debug_spell_mana_lanes(2),
        Some(paid),
        "the toggle must not re-stamp an already-built manifestation"
    );

    // Grant AFTER the toggle, so the manifestations are BUILT under
    // the flag — the way a mid-take jar pickup would be.
    let mut on = world!();
    assert!(on.apply_cheat(Cheat::FreeSpell));
    assert!(on.apply_cheat(Cheat::AllSpells));
    assert_eq!(
        on.debug_spell_mana_lanes(2),
        Some((0, 1)),
        "built under free-spell: upkeep 0, one mana a shot"
    );

    // ...and it is a TOGGLE, so twice is off again.
    let mut off = world!();
    assert!(off.apply_cheat(Cheat::FreeSpell));
    assert!(off.apply_cheat(Cheat::FreeSpell));
    assert!(off.apply_cheat(Cheat::AllSpells));
    assert_eq!(
        off.debug_spell_mana_lanes(2),
        Some(paid),
        "toggled back off before the grant"
    );
}
