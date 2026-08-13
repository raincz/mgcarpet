//! Truth-channel cell history, ALL FOUR planes: print every tick where
//! a watched cell's (height, type, shading, angle) changes — the
//! angle plane carries the castle protection bits (0x80/0x08) and the
//! walkability nibble the type-only history probe cannot see.
//!
//! usage: terrain_cell4_mc1 <mgcr> <x,y>… [--until <t>]
use mgc_formats::mgcr::Recording;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: terrain_cell4_mc1 <mgcr> <x,y>… [--until <t>]");
    let mut cells: Vec<(usize, usize)> = Vec::new();
    let mut until = u64::MAX;
    let rest: Vec<String> = args.collect();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--until" => {
                until = rest[i + 1].parse().unwrap();
                i += 2;
            }
            s => {
                let (x, y) = s.split_once(',').expect("cell as x,y");
                cells.push((x.parse().unwrap(), y.parse().unwrap()));
                i += 1;
            }
        }
    }

    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    let mut timg = rec
        .header
        .channels
        .terrain
        .as_ref()
        .map(mgc_formats::mgcr::TerrainImage::new)
        .expect("no terrain channel");
    let mut last: Vec<Option<(u8, u8, u8, u8)>> = vec![None; cells.len()];
    let mut pending: Option<mgc_formats::mgcr::TerrainBlock> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t > until {
            break;
        }
        // Phase: after applying `pending` the image holds terrain at
        // the PREVIOUS record (block@t lands at the top of t+1).
        if let Some(block) = pending.take() {
            timg.apply(&block).expect("terrain");
        }
        pending = tick.terrain.clone();
        let (Some(h), Some(ty), Some(sh), Some(an)) = (
            timg.plane("height"),
            timg.plane("type"),
            timg.plane("shading"),
            timg.plane("angle"),
        ) else {
            continue;
        };
        for (k, &(cx, cy)) in cells.iter().enumerate() {
            let c = cy * 256 + cx;
            let now = (h[c], ty[c], sh[c], an[c]);
            if last[k] != Some(now) {
                println!(
                    "t<={} cell ({cx},{cy}) h={} ty={} sh={} an={:#04x}{}",
                    tick.t.saturating_sub(1),
                    now.0,
                    now.1,
                    now.2,
                    now.3,
                    if last[k].is_none() { " (first)" } else { "" }
                );
                last[k] = Some(now);
            }
        }
    }
}
