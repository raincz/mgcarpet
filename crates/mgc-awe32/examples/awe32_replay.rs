//! `awe32_replay <rom> <dram.raw> <trace> <out.wav>` - the Rust side of
//! `AWE32Emu --replay` (the DRAM image is the `.dram.raw` the C++ writes
//! next to a `--trace`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 5 {
        eprintln!("usage: awe32_replay <rom> <dram.raw> <trace> <out.wav>");
        std::process::exit(2);
    }
    let rom = std::fs::read(&a[1]).expect("rom");
    let dram: Vec<i16> = std::fs::read(&a[2])
        .expect("dram")
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let text = std::fs::read_to_string(&a[3]).expect("trace");
    let (writes, _) = mgc_awe32::replay::parse_trace(&text);
    let t = std::time::Instant::now();
    let pcm = mgc_awe32::replay::replay(&rom, &dram, &writes, 8192).expect("replay");
    let secs = pcm.len() as f64 / 2.0 / 44100.0;
    eprintln!(
        "{} writes, {:.1} s audio in {:.2} s",
        writes.len(),
        secs,
        t.elapsed().as_secs_f64()
    );
    std::fs::write(&a[4], mgc_awe32::wav::wav_bytes(&pcm, 44100)).expect("write");
}
