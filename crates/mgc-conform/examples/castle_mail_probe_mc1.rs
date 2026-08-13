//! Mailbox pair-probe: run ONE verify-grade pair (import state@N,
//! tick) and print the PORT's post-tick mailbox + life/f70/victim for
//! the watched slots next to retail's state@N+1 — the microscope for
//! delivery laws the locked obs schema can't diff (castle ch0 mail).
//!
//! usage: castle_mail_probe_mc1 <mgcr> --pair <t> <slot>…
use mgc_formats::mgcr::{Recording, decode_retail_mc1};
use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::mc1::rivals::RivalConfig;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: castle_mail_probe_mc1 <mgcr> --pair <t> <slot>…");
    let mut pair_t = None;
    let mut near: Option<(f64, f64)> = None;
    let mut slots: Vec<usize> = Vec::new();
    let rest: Vec<String> = args.collect();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--pair" => {
                pair_t = Some(rest[i + 1].parse::<u64>().unwrap());
                i += 2;
            }
            "--near" => {
                let (x, y) = rest[i + 1].split_once(',').unwrap();
                near = Some((x.parse::<f64>().unwrap(), y.parse::<f64>().unwrap()));
                i += 2;
            }
            s => {
                slots.push(s.parse().unwrap());
                i += 1;
            }
        }
    }
    let pair_t = pair_t.expect("--pair <t> required");

    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    let game = rec.header.game.clone();
    let level = rec.header.level.expect("no level");
    let (mut world, pristine) = build_world(&PathBuf::from("baked"), &game, level);

    let mut timg = rec
        .header
        .channels
        .terrain
        .as_ref()
        .map(mgc_formats::mgcr::TerrainImage::new);
    let mut pending_terrain: Option<mgc_formats::mgcr::TerrainBlock> = None;
    let mut prev: Option<(u64, mgc_formats::mgcr::RetailMc1, PlayerCommand)> = None;
    let mut prev_cmd = PlayerCommand::default();
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if let Some(img) = timg.as_mut() {
            if let Some(block) = pending_terrain.take() {
                img.apply(&block).expect("terrain");
            }
            pending_terrain = tick.terrain.clone();
        }
        let Some(state) = &tick.state else {
            prev = None;
            continue;
        };
        let st = decode_retail_mc1(state).expect("decode");
        let cmd = fire_bits(&st);
        if let Some((pt, pst, pcmd)) = prev.take() {
            if pt == pair_t && tick.t == pt + 1 {
                let pcmd = {
                    let rc = mgc_formats::recover::recover_pair_mc1(&pst, &st, tick.input.as_ref());
                    PlayerCommand {
                        equip_left: rc.equip_left.map(mgc_sim::mc1::spells::SpellId),
                        equip_right: rc.equip_right.map(mgc_sim::mc1::spells::SpellId),
                        demolish: rc.demolish,
                        ..pcmd
                    }
                };
                world.restore_planes(&pristine);
                if let Some(img) = timg.as_ref() {
                    if let Some((h, ty, ceil, an)) = img.measured() {
                        world
                            .install_measured_terrain(h, ty, ceil, an)
                            .expect("terrain");
                    }
                }
                let report = world.retail_import_mc1(&pst).expect("import");
                world.set_prev_fire(prev_cmd.fire_left, prev_cmd.fire_right);
                let e = &st.ents[report.human_slot as usize];
                let pose = PlayerPose {
                    x: e.x,
                    y: e.y,
                    z: e.z,
                    heading: e.f30,
                    pitch: e.f32,
                    speed: e.f126,
                };
                world.tick(pose, pcmd);
                let (_, ev) = world.debug_pool();
                if let Some((nx, ny)) = near {
                    for (s, re) in pst.ents.iter().enumerate() {
                        if re.class64 == 0 {
                            continue;
                        }
                        let (ex, ey) = (re.x as f64 / 256.0, re.y as f64 / 256.0);
                        let hit = (nx < 0.0 && re.class64 == 3 && re.model65 == 3)
                            || ((ex - nx).abs() < 8.0 && (ey - ny).abs() < 8.0);
                        if hit {
                            println!(
                                "pre@{pt}: slot {s} ({},{}) f70={} life={} f140={} f146={} at ({ex:.3},{ey:.3},{}) f30={} f34={} flags={:#x}",
                                re.class64,
                                re.model65,
                                re.f70,
                                re.act_life,
                                re.f140,
                                re.f146,
                                re.z,
                                re.f30,
                                re.f34,
                                re.flags
                            );
                        }
                    }
                }
                for &s in &slots {
                    let re = &st.ents[s];
                    println!(
                        "retail@{}: slot {s} ({},{}) f70={} life={} f146={} mail={:?} \
                         pos=({},{},{}) f30={} f34={} f46={} f126={} f140={} f144={} dest=({},{})",
                        tick.t,
                        re.class64,
                        re.model65,
                        re.f70,
                        re.act_life,
                        re.f146,
                        re.mail,
                        re.x,
                        re.y,
                        re.z,
                        re.f30,
                        re.f34,
                        re.f46,
                        re.f126,
                        re.f140,
                        re.f144,
                        re.dest_x,
                        re.dest_y
                    );
                    match ev.iter().find(|d| d.slot == s) {
                        Some(d) => {
                            let l = world.debug_launch(s).unwrap();
                            println!(
                                "  port@{}: slot {s} ({},{}) f70={} life={} f146={} mail={:?} \
                                 pos=({},{},{}) f30={} f34={} f46={} f126={} f140={} f144={} dest=({},{})",
                                tick.t,
                                d.class,
                                d.model,
                                d.state,
                                d.life,
                                d.chase,
                                world.debug_mail(s),
                                l.0,
                                l.1,
                                l.2,
                                l.3,
                                l.4,
                                l.5,
                                l.6,
                                l.7,
                                l.8,
                                l.9,
                                l.10
                            );
                        }
                        None => println!("  port@{}: slot {s} NOT LIVE", tick.t),
                    }
                }
                return;
            }
        }
        prev_cmd = cmd;
        prev = Some((tick.t, st, cmd));
    }
    eprintln!("pair {pair_t} not found");
}

fn fire_bits(st: &mgc_formats::mgcr::RetailMc1) -> PlayerCommand {
    st.wizards
        .get(st.local_player as usize)
        .map(|w| {
            let (fire_left, fire_right) = if w.move_bits == 48 {
                (false, false)
            } else {
                mgc_formats::recover::mc1_fire(w.move_bits)
            };
            PlayerCommand {
                fire_left,
                fire_right,
                ..Default::default()
            }
        })
        .unwrap_or_default()
}

fn build_world(baked: &std::path::Path, game: &str, level: u32) -> (World, Planes) {
    let lp = baked.join(game).join(format!("level-{level:03}.mgcl"));
    let file = std::fs::File::open(&lp).expect("level");
    let pkg: mgc_formats::LevelPackage = mgc_formats::mgcl::read(file).expect("mgcl");
    let (variant, game_id) = if game == "mc1hw" {
        ("mc1-arctic", mgc_sim::ids::GameId::Mc1Hw)
    } else {
        ("mc1-temperate", mgc_sim::ids::GameId::Mc1)
    };
    let bundle =
        mgc_formats::bundle::Bundle::load(&baked.join("assets").join(variant)).expect("bundle");
    let terrain = pkg.terrain.as_ref().expect("terrain");
    let planes = Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone().expect("shading"),
        angle: terrain.angle.clone().expect("angle"),
        ceiling: Vec::new(),
    };
    let mut assets = FeatureAssets::parse(
        bundle.search.as_ref().expect("search"),
        bundle.build_tab.as_ref().expect("build tab"),
        bundle.build_dat.as_ref().expect("build dat"),
    )
    .expect("assets");
    if let Some(prm) = bundle.bldgprm.as_deref() {
        assets = assets.with_bldgprm(prm);
    }
    if let Some(sp) = bundle.spells.as_deref() {
        assets = assets.with_spells(sp).expect("spells");
    }
    let seed = pkg.gen_params.as_ref().map_or(0, |g| g.seed);
    let mut w = World::new_for_game(planes, &pkg.things.things, seed, assets, game_id);
    if let Some(f) = pkg.gen_params.as_ref().and_then(|g| g.footer) {
        w.set_win_pct(f[0]);
    }
    let (wizards, player_count) = rival_configs(pkg.wizards.as_ref());
    w.set_wizards(&wizards, player_count);
    let pristine = w.planes_clone();
    (w, pristine)
}

fn rival_configs(wizards: Option<&mgc_formats::Wizards>) -> ([Option<RivalConfig>; 8], u16) {
    let mut out: [Option<RivalConfig>; 8] = Default::default();
    let Some(w) = wizards else { return (out, 1) };
    let count = w.player_count.unwrap_or(1).min(8);
    for (slot, cfg) in w.wizards.iter().enumerate().take(8).skip(1) {
        let (Some(acc), Some(tempo), Some(allowed_mask)) =
            (cfg.accuracy, cfg.tempo, cfg.allowed_spells.as_ref())
        else {
            continue;
        };
        let mut book = [false; 24];
        let mut allowed = [false; 24];
        for s in 0..24 {
            let a = allowed_mask.get(s).copied().unwrap_or(0) != 0;
            allowed[s] = a;
            book[s] = a && cfg.starting_spells.get(s).copied().unwrap_or(0) != 0;
        }
        out[slot] = Some(RivalConfig {
            aggression: cfg.aggression.clamp(0, 255) as u8,
            accuracy: acc.clamp(0, 255) as u8,
            tempo: tempo.clamp(0, 255) as u8,
            castle_level: cfg.castle_level.unwrap_or(0),
            book,
            allowed,
        });
    }
    (out, count)
}
