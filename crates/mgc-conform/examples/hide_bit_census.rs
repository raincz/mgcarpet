//! Census of retail's byte[0] bit-0 billboard hide (the `& 0x21`
//! draw skip, MC1 :36830 / MC2 GameRenderOriginal.cpp:1936) per
//! (class, model), off the RECORDED retail pool. Prints one line per
//! family: `game class model state rows bit0_rows` (state only for class 5 and (10,45)). Samples every 8th tick.
//!
//! usage: hide_bit_census <mgcr>...  (mc2* files decode as MC2)
use mgc_formats::mgcr::Recording;
use std::collections::BTreeMap;

fn main() {
    let mut tally: BTreeMap<(bool, u8, u8, u8), (u64, u64)> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let mc2 = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("mc2"));
        let Ok(mut rec) = Recording::open(std::path::Path::new(&path)) else {
            eprintln!("skip {path}: open");
            continue;
        };
        let mut n = 0u64;
        while let Some(r) = rec.next_tick() {
            let Ok(tick) = r else { break };
            n += 1;
            if n % 8 != 0 {
                continue;
            }
            let Some(state) = &tick.state else { continue };
            let mut add = |c: u8, m: u8, f: u32, st: u8| {
                let st = if c == 5 || (c == 10 && m == 45) {
                    st
                } else {
                    0
                };
                let t = tally.entry((mc2, c, m, st)).or_default();
                t.0 += 1;
                t.1 += (f & 1) as u64;
            };
            if mc2 {
                let Ok(st) = mgc_formats::mgcr::decode_retail_mc2(state) else {
                    continue;
                };
                for e in &st.ents {
                    if e.class3f != 0 {
                        add(e.class3f, e.model40, e.flags, e.action45);
                    }
                }
            } else {
                let Ok(st) = mgc_formats::mgcr::decode_retail_mc1(state) else {
                    continue;
                };
                for e in &st.ents {
                    if e.class64 != 0 {
                        add(e.class64, e.model65, e.flags, e.f70);
                    }
                }
            }
        }
    }
    for ((mc2, c, m, st), (rows, b0)) in tally {
        let g = if mc2 { "mc2" } else { "mc1" };
        println!("{g}\t{c}\t{m}\t{st}\t{rows}\t{b0}");
    }
}
