//! `mdi_tables_probe <file>...` - runs the SBAWE32.MDI GM-table scan
//! (`BuildSbkFromMdi`) over each file and prints what it found.
fn main() {
    let mut first: Option<Vec<u8>> = None;
    for p in std::env::args().skip(1) {
        let d = std::fs::read(&p).expect("read");
        match mgc_awe32::soundfont::build_sbk_from_mdi_tables(&d) {
            Ok((riff, unmapped)) => {
                let b = mgc_awe32::soundfont::Bank::load_riff(&riff).expect("riff");
                let same = first.as_ref().map(|f| {
                    if *f == riff {
                        "same RIFF as first"
                    } else {
                        "RIFF DIFFERS"
                    }
                });
                println!(
                    "{p}: {} presets, {} instruments, {} samples, RIFF {} B, unmapped {unmapped} {}",
                    b.presets.len(),
                    b.instruments.len(),
                    b.samples.len(),
                    riff.len(),
                    same.unwrap_or("")
                );
                if let Some(f) = &first {
                    let diffs: Vec<usize> = (0..f.len().min(riff.len()))
                        .filter(|&i| f[i] != riff[i])
                        .collect();
                    if !diffs.is_empty() {
                        println!(
                            "  {} differing bytes, first at {:?}",
                            diffs.len(),
                            &diffs[..diffs.len().min(16)]
                        );
                    }
                    if let Ok(dir) = std::env::var("MDI_PROBE_DUMP") {
                        let name = std::path::Path::new(&p)
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_string();
                        std::fs::write(format!("{dir}/{name}.sbk"), &riff).unwrap();
                    }
                }
                if first.is_none() {
                    first = Some(riff);
                }
            }
            Err(e) => println!("{p}: {e}"),
        }
    }
}
