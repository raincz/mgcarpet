//! Who wrote the player's damage mailbox? — list every pool slot that
//! CHANGED IDENTITY (freed, allocated, or re-used) between t-1 and t,
//! with its last-seen position relative to the human. The blast that
//! mails the carpet is typically gone by the next snapshot, so the
//! overlap scan at t finds nothing and the death diff finds it.
//!
//! Usage: blast_who_mc1 <mgcr> <t> [radius_tiles]
use mgc_formats::mgcr::{Recording, RetailEntMc1, decode_retail_mc1};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: blast_who_mc1 <mgcr> <t> [radius]");
    let t: u64 = args.next().expect("t").parse().expect("t");
    let rad: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(12.0);
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    let mut prev: Option<(u64, Vec<RetailEntMc1>, usize)> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        let Some(state) = &tick.state else { continue };
        let st = decode_retail_mc1(state).expect("decode");
        let hs = st.wizards[st.local_player as usize].play_index as usize;
        if tick.t == t {
            let (pt, pv, phs) = prev.expect("no previous tick");
            let p = &st.ents[hs];
            let pp = &pv[phs];
            println!(
                "t={t} (prev t={pt}) player ({:.2},{:.2}) z={} mail0={:?}",
                p.x as f32 / 256.0,
                p.y as f32 / 256.0,
                p.z,
                p.mail[0]
            );
            let show = |tag: &str, i: usize, e: &RetailEntMc1, px: u16, py: u16, pz: i16| {
                let d = (((e.x.wrapping_sub(px) as i16 as f32) / 256.0).powi(2)
                    + ((e.y.wrapping_sub(py) as i16 as f32) / 256.0).powi(2))
                .sqrt();
                if d > rad {
                    return;
                }
                println!(
                    "  {tag} slot {i} c{} m{} d={d:.2} dz={} life={} f44={} f58={} \
                     f66/67={}/{} f68/69={}/{} ext=({},{},{}) f78={} id24={}",
                    e.class64,
                    e.model65,
                    e.z as i32 - pz as i32,
                    e.act_life,
                    e.f44,
                    e.f58,
                    e.f66,
                    e.f67,
                    e.f68,
                    e.f69,
                    e.f80,
                    e.f82,
                    e.f84,
                    e.f78 as i16,
                    e.id24
                );
            };
            for i in 0..st.ents.len().min(pv.len()) {
                if i == hs {
                    continue;
                }
                let (o, n) = (&pv[i], &st.ents[i]);
                let died = o.class64 != 0 && n.class64 == 0;
                let born = o.class64 == 0 && n.class64 != 0;
                let reused = o.class64 != 0 && n.class64 != 0 && o.id24 != n.id24;
                if died || reused {
                    show("DIED  @prev", i, o, pp.x, pp.y, pp.z);
                }
                if born || reused {
                    show("BORN  @now ", i, n, p.x, p.y, p.z);
                }
            }
            return;
        }
        prev = Some((tick.t, st.ents, hs));
    }
    eprintln!("tick {t} not found");
}
