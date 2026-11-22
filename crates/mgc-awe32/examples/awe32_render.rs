//! `awe32_render <score.xmi|.mid> --rom R [--sf B]... [--mdi-tables F] --driver dos|sdk
//!   [--conf C] [--master-volume N] [--tracks L] [--trace T] --wav OUT`
//! - the Rust side of the AWE32Emu `--wav` render, same flags.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut score = None;
    let (mut rom, mut conf, mut wav, mut trace) = (None, None, None, None);
    let mut banks: Vec<(bool, Vec<u8>)> = Vec::new();
    let mut driver = mgc_awe32::Driver::Dos;
    let mut master = None;
    let mut mask = 0xFFFFu16;
    let mut i = 1;
    while i < a.len() {
        let v = a.get(i + 1).cloned().unwrap_or_default();
        match a[i].as_str() {
            "--rom" => rom = Some(std::fs::read(&v).expect("rom")),
            "--sf" => banks.push((false, std::fs::read(&v).expect("bank"))),
            "--mdi-tables" => banks.push((true, std::fs::read(&v).expect("bank"))),
            "--conf" => conf = Some(std::fs::read_to_string(&v).expect("conf")),
            "--wav" => wav = Some(v),
            "--trace" => trace = Some(v),
            "--master-volume" => master = Some(v.parse::<i32>().expect("volume")),
            "--tracks" => mask = mgc_awe32::parse_tracks(&v),
            "--driver" => {
                driver = match v.as_str() {
                    "dos" => mgc_awe32::Driver::Dos,
                    "sdk" => mgc_awe32::Driver::Sdk,
                    _ => panic!("driver dos|sdk"),
                }
            }
            s => {
                score = Some(s.to_string());
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    let score = score.expect("score");
    let bytes = std::fs::read(&score).expect("score file");
    let rom = rom.expect("--rom");
    let mut o = mgc_awe32::RenderOptions::new(&rom, driver);
    o.banks = banks
        .iter()
        .map(|(m, b)| {
            if *m {
                mgc_awe32::BankSpec::mdi_tables(b)
            } else {
                mgc_awe32::BankSpec::sf(b)
            }
        })
        .collect();
    o.conf = conf.as_deref();
    o.master_volume = master;
    o.channel_mask = mask;
    o.trace = trace.is_some();
    let sc = if score.to_ascii_lowercase().ends_with(".xmi") {
        mgc_awe32::Score::Xmi(&bytes)
    } else {
        mgc_awe32::Score::Smf(&bytes)
    };
    let t = std::time::Instant::now();
    let r = mgc_awe32::render(&o, sc).expect("render");
    let secs = r.pcm.len() as f64 / 2.0 / 44100.0;
    let el = t.elapsed().as_secs_f64();
    eprintln!(
        "{:.1} s audio in {:.2} s ({:.1}x realtime)",
        secs,
        el,
        secs / el
    );
    for w in &r.warnings {
        eprintln!("warning: {}", w);
    }
    if let (Some(p), Some(t)) = (trace, r.trace) {
        std::fs::write(&p, t).expect("trace");
    }
    std::fs::write(
        wav.expect("--wav"),
        mgc_awe32::wav::wav_bytes(&r.pcm, 44100),
    )
    .expect("wav");
}
