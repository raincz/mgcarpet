//! Player-mailbox timeline dump: per tick, the LOCAL carpet's pool
//! record mail (str_29885_90 — the block the grace memset :55367-71
//! wipes and the at-castle redirect :55357-60 forwards), the wizext
//! grace (u16_331), and the watched castle's ch0 mail — the raw
//! retail-truth microscope for the at-castle redirect protocol.
//! Usage: player_mail_dump_mc1 <mgcr> --from <t> --to <t> [--castle <slot>]
use mgc_formats::mgcr::{Recording, decode_retail_mc1};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: player_mail_dump_mc1 <mgcr> --from t --to t [--castle slot]");
    let (mut from, mut to, mut castle) = (0u64, u64::MAX, None::<usize>);
    let rest: Vec<String> = args.collect();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--from" => {
                from = rest[i + 1].parse().unwrap();
                i += 2;
            }
            "--to" => {
                to = rest[i + 1].parse().unwrap();
                i += 2;
            }
            "--castle" => {
                castle = Some(rest[i + 1].parse().unwrap());
                i += 2;
            }
            _ => i += 1,
        }
    }
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t < from {
            continue;
        }
        if tick.t > to {
            break;
        }
        let Some(state) = &tick.state else { continue };
        let st = decode_retail_mc1(state).expect("decode");
        let w = &st.wizards[st.local_player as usize];
        let carpet = &st.ents[w.play_index as usize];
        let cm = castle.map(|c| st.ents[c].mail[0]);
        println!(
            "t={} carpet={} at ({},{},{}) mail={:?} grace={} danger={} \
             knock=({},{}) regen_stall={} castle50={}{}",
            tick.t,
            w.play_index,
            carpet.x,
            carpet.y,
            carpet.z,
            carpet.mail,
            w.grace,
            w.danger,
            w.knock_mag,
            w.knock_dir,
            w.regen_stall,
            w.castle,
            cm.map(|m| format!(" castle_ch0={m:?}")).unwrap_or_default(),
        );
    }
}
