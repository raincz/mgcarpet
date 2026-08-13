//! Scorch-gate pair-probe: run ONE verify-grade pair and print, for a
//! watched (10,0) fire, the recorded terrain truth at BOTH boundaries
//! next to the port's live ground sample at the fire's cell — the
//! mid-tick terraform phase microscope behind mc1hw-fire-churn-rand
//! (the gate `z − ground <= 128` flips on the ground sample, not on
//! the draw structure).
//!
//! usage: scorch_gate_probe_mc1 <mgcr> --pair <t> <slot>…
use mgc_formats::mgcr::{Recording, decode_retail_mc1};
use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::mc1::rivals::RivalConfig;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: scorch_gate_probe_mc1 <mgcr> --pair <t> <slot>…");
    let mut pair_t = None;
    let mut slots: Vec<usize> = Vec::new();
    let rest: Vec<String> = args.collect();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--pair" => {
                pair_t = Some(rest[i + 1].parse::<u64>().unwrap());
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
                // Image currently holds terrain@N (block@pt applied at
                // the top of this record — the mail probe's phase).
                let h_at = |img: &mgc_formats::mgcr::TerrainImage, cx: usize, cy: usize| {
                    img.measured()
                        .map(|(h, ty, _, _)| (h[cy * 256 + cx] as i32, ty[cy * 256 + cx]))
                };
                // Port plane snapshot at a cell: (height, type, angle).
                let port_cell = |w: &World, cx: usize, cy: usize| {
                    let n = 256 * 256;
                    let mut h = vec![0u8; n];
                    let mut ty = vec![0u8; n];
                    let mut sh = vec![0u8; n];
                    let mut an = vec![0u8; n];
                    w.copy_planes_into(mgc_sim::engine::features::TerrainPlanes {
                        height: &mut h,
                        tile_type: &mut ty,
                        shading: &mut sh,
                        angle: &mut an,
                    });
                    (h[cy * 256 + cx], ty[cy * 256 + cx], an[cy * 256 + cx])
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

                // Cells of interest: each watched slot's tile at N+1
                // (the fire may spawn mid-pair — its position is only
                // in the END state).
                let cells: Vec<(usize, u16, u16, usize, usize)> = slots
                    .iter()
                    .map(|&s| {
                        let re = &st.ents[s];
                        (s, re.x, re.y, (re.x >> 8) as usize, (re.y >> 8) as usize)
                    })
                    .collect();
                for &(s, x, y, cx, cy) in &cells {
                    println!(
                        "pre@{pt}: slot {s} cell ({cx},{cy}) image(h,ty)@N={:?} \
                         port(h,ty,an)={:?} port_ground={}",
                        timg.as_ref().and_then(|img| h_at(img, cx, cy)),
                        port_cell(&world, cx, cy),
                        world.ground_z_engine(x, y)
                    );
                }
                let he = &st.ents[report.human_slot as usize];
                let pose = PlayerPose {
                    x: he.x,
                    y: he.y,
                    z: he.z,
                    heading: he.f30,
                    pitch: he.f32,
                    speed: he.f126,
                };
                // Snapshot the port planes pre-tick for the cell diff.
                let n = 256 * 256;
                let (mut ph0, mut pt0, mut pa0) = (vec![0u8; n], vec![0u8; n], vec![0u8; n]);
                {
                    let mut sh = vec![0u8; n];
                    world.copy_planes_into(mgc_sim::engine::features::TerrainPlanes {
                        height: &mut ph0,
                        tile_type: &mut pt0,
                        shading: &mut sh,
                        angle: &mut pa0,
                    });
                }
                let truth0: Option<(Vec<u8>, Vec<u8>)> = timg
                    .as_ref()
                    .and_then(|img| img.measured())
                    .map(|(h, ty, _, _)| (h.to_vec(), ty.to_vec()));
                world.tick(pose, pcmd);
                // Advance the image to terrain@N+1 for the end truth.
                if let (Some(img), Some(block)) = (timg.as_mut(), pending_terrain.take()) {
                    img.apply(&block).expect("terrain n+1");
                }
                // Per-cell diff: every cell where the port OR the truth
                // changed h/ty this tick, side by side (angle = port
                // only; truth has no angle channel).
                let (mut ph1, mut pt1, mut pa1) = (vec![0u8; n], vec![0u8; n], vec![0u8; n]);
                {
                    let mut sh = vec![0u8; n];
                    world.copy_planes_into(mgc_sim::engine::features::TerrainPlanes {
                        height: &mut ph1,
                        tile_type: &mut pt1,
                        shading: &mut sh,
                        angle: &mut pa1,
                    });
                }
                if let (Some((th0, tt0)), Some((th1, tt1))) = (
                    truth0,
                    timg.as_ref()
                        .and_then(|img| img.measured())
                        .map(|(h, ty, _, _)| (h, ty)),
                ) {
                    println!("-- cell diff over pair {pt} (port vs truth), changed h/ty only:");
                    for cy in 0..256usize {
                        for cx in 0..256usize {
                            let t = cy * 256 + cx;
                            let port_chg = ph0[t] != ph1[t] || pt0[t] != pt1[t];
                            let truth_chg = th0[t] != th1[t] || tt0[t] != tt1[t];
                            if port_chg || truth_chg {
                                println!(
                                    "  ({cx},{cy}) truth h {}->{} ty {}->{} | port h {}->{} ty {}->{} an {}->{}{}{}",
                                    th0[t],
                                    th1[t],
                                    tt0[t],
                                    tt1[t],
                                    ph0[t],
                                    ph1[t],
                                    pt0[t],
                                    pt1[t],
                                    pa0[t],
                                    pa1[t],
                                    if port_chg && !truth_chg {
                                        "  PORT-ONLY"
                                    } else {
                                        ""
                                    },
                                    if truth_chg && !port_chg {
                                        "  TRUTH-ONLY"
                                    } else {
                                        ""
                                    },
                                );
                            }
                        }
                    }
                }
                for &(s, x, y, cx, cy) in &cells {
                    let re = &st.ents[s];
                    println!(
                        "post@{}: slot {s} ({},{}) retail z={} life={} rand={:#010x} at \
                         ({:.2},{:.2}) | image(h,ty)@N+1={:?} port(h,ty,an)={:?} port_ground={} \
                         gate_margin(retail z-g)={}",
                        tick.t,
                        re.class64,
                        re.model65,
                        re.z,
                        re.act_life,
                        re.rand,
                        re.x as f64 / 256.0,
                        re.y as f64 / 256.0,
                        timg.as_ref().and_then(|img| h_at(img, cx, cy)),
                        port_cell(&world, cx, cy),
                        world.ground_z_engine(x, y),
                        re.z as i32 - world.ground_z_engine(x, y) as i32,
                    );
                    match world.debug_launch(s) {
                        Some(l) => println!(
                            "  port@{}: slot {s} pos=({},{},{}) rand={:#010x} port z-g={}",
                            tick.t,
                            l.0,
                            l.1,
                            l.2,
                            world.debug_rand(s).unwrap_or(0),
                            l.2 as i32 - world.ground_z_engine(l.0, l.1) as i32
                        ),
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
