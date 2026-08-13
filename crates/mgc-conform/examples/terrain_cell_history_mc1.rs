//! Truth-channel cell history: walk the recording's terrain channel and
//! print every tick where a watched cell's (height, type) changes —
//! retail's own plane evolution, the ground truth for scorch/paint
//! archaeology (angle has no truth channel; h/ty changes are the only
//! recorded shadow of a dig/retile).
//!
//! usage: terrain_cell_history_mc1 <mgcr> <x,y>… [--until <t>]
use mgc_formats::mgcr::Recording;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: terrain_cell_history_mc1 <mgcr> <x,y>… [--until <t>]");
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
    let mut last: Vec<Option<(u8, u8)>> = vec![None; cells.len()];
    let mut pending: Option<mgc_formats::mgcr::TerrainBlock> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t > until {
            break;
        }
        // Same phase as the pair probes: block@t applies at the top of
        // the NEXT record, so after applying `pending` the image holds
        // terrain@t.
        if let Some(block) = pending.take() {
            timg.apply(&block).expect("terrain");
        }
        pending = tick.terrain.clone();
        if let Some((h, ty, _, _)) = timg.measured() {
            for (k, &(cx, cy)) in cells.iter().enumerate() {
                let now = (h[cy * 256 + cx], ty[cy * 256 + cx]);
                if last[k] != Some(now) {
                    println!(
                        "t={} cell ({cx},{cy}) h={} ty={}{}",
                        tick.t,
                        now.0,
                        now.1,
                        if last[k].is_none() { " (first)" } else { "" }
                    );
                    last[k] = Some(now);
                }
            }
        }
    }
}
