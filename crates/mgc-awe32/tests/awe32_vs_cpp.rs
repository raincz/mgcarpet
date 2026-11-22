//! Bit-exactness against the C++ AWE32Emu build.
//!
//! The expected hashes are FNV-1a-64 of the 16-bit PCM (the WAV data
//! chunk) that `AWE32Emu` (reference/AWE32Emu, CMake Release, gcc 14.2,
//! glibc 2.41, x86-64) wrote for the same inputs and flags; the Rust output
//! was also compared byte for byte (`cmp`) with those WAVs, and with 16 more
//! full-length renders (all six MC2 W-slot pieces, masters 100/127, masks,
//! sdk, SMF) and their port-write traces, see the crate report.
//!
//! Tests skip (with a note on stderr) when the ROM or the game files are
//! not present.

use mgc_awe32::{BankSpec, Driver, RenderOptions, Score, midi, render, replay};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Option<Vec<u8>> {
    let p = root().join(rel);
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("skipping: {} not present", p.display());
            None
        }
    }
}

const ROM: &str = "reference/AWE32Emu/awe32.raw";
const MDI: &str = "reference/carpet2/patched/sound/sbawe32.mdi";
const SBK: &str = "reference/carpet2/patched/sound/bullfrog.sbk";
const CONF: &str = "reference/AWE32Emu/conf/mc2.conf";
const MUSIC: &str = "reference/carpet2/patched/sound/music.dat";

fn fnv(pcm: &[i16]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for s in pcm {
        for b in s.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// A 6 s type-1 SMF (3 tracks, tempo change, running status, program
/// change, CC7/11/64/1/91/93, pitch bend, channel pressure, drums).
const SYNTH_MID: [u8; 290] = [
    0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x01, 0x00, 0x03, 0x00, 0x78, 0x4D, 0x54,
    0x72, 0x6B, 0x00, 0x00, 0x00, 0x13, 0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, 0x83, 0x60, 0xFF,
    0x51, 0x03, 0x05, 0x16, 0x15, 0x00, 0xFF, 0x2F, 0x00, 0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00,
    0x65, 0x00, 0xC0, 0x00, 0x00, 0xC1, 0x30, 0x00, 0xB0, 0x07, 0x64, 0x00, 0xB1, 0x5B, 0x50, 0x00,
    0xB1, 0x5D, 0x40, 0x00, 0x90, 0x3C, 0x64, 0x00, 0x40, 0x50, 0x00, 0x43, 0x48, 0x00, 0x91, 0x30,
    0x50, 0x78, 0xB0, 0x40, 0x7F, 0x00, 0x80, 0x3C, 0x00, 0x00, 0x90, 0x3E, 0x70, 0x78, 0xE1, 0x00,
    0x50, 0x3C, 0xE1, 0x00, 0x30, 0x3C, 0xE1, 0x00, 0x40, 0x78, 0xB0, 0x40, 0x00, 0x00, 0x80, 0x3E,
    0x00, 0x00, 0xB1, 0x0B, 0x40, 0x00, 0xD1, 0x60, 0x81, 0x70, 0x81, 0x30, 0x00, 0x00, 0x80, 0x40,
    0x00, 0x00, 0x80, 0x43, 0x00, 0x00, 0xB0, 0x01, 0x60, 0x00, 0x90, 0x48, 0x7F, 0x83, 0x60, 0x80,
    0x48, 0x00, 0x00, 0xFF, 0x2F, 0x00, 0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x84, 0x00, 0x99,
    0x24, 0x7F, 0x1E, 0x89, 0x24, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99,
    0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99,
    0x24, 0x7F, 0x1E, 0x89, 0x24, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99,
    0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99,
    0x24, 0x7F, 0x1E, 0x89, 0x24, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99,
    0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99,
    0x24, 0x7F, 0x1E, 0x89, 0x24, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99,
    0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x3C, 0x99, 0x2A, 0x50, 0x1E, 0x89, 0x2A, 0x00, 0x00, 0xFF,
    0x2F, 0x00,
];

fn w5(music: &[u8]) -> Vec<u8> {
    let blob = midi::music_dat_slot(music, 0, 3).expect("W slot");
    let pieces = midi::split_xmi_bank(blob);
    assert_eq!(pieces.len(), 6);
    pieces[5].to_vec()
}

#[test]
fn mc1_recipe_smf_dos() {
    let (Some(rom), Some(mdi)) = (read(ROM), read(MDI)) else {
        return;
    };
    let mut o = RenderOptions::new(&rom, Driver::Dos);
    o.banks = vec![BankSpec::sf(&mdi)];
    o.master_volume = Some(100);
    let r = render(&o, Score::Smf(&SYNTH_MID)).unwrap();
    assert_eq!(r.pcm.len(), 530520);
    assert_eq!(fnv(&r.pcm), 0x10cb285d45f0758d);
}

#[test]
fn mc1_recipe_smf_sdk() {
    let (Some(rom), Some(mdi)) = (read(ROM), read(MDI)) else {
        return;
    };
    let mut o = RenderOptions::new(&rom, Driver::Sdk);
    o.banks = vec![BankSpec::sf(&mdi)];
    o.master_volume = Some(100);
    let r = render(&o, Score::Smf(&SYNTH_MID)).unwrap();
    assert_eq!(fnv(&r.pcm), 0x98524d34278ecdb0);
}

#[test]
fn mc1_tables_from_carpet_exe_render_the_same() {
    // MC1 installs have no SBAWE32.MDI; CARPET.EXE embeds the same tables.
    let (Some(rom), Some(exe)) = (read(ROM), read("reference/carpet/CARPET.EXE")) else {
        return;
    };
    let mut o = RenderOptions::new(&rom, Driver::Dos);
    o.banks = vec![BankSpec::mdi_tables(&exe)];
    o.master_volume = Some(100);
    let r = render(&o, Score::Smf(&SYNTH_MID)).unwrap();
    assert_eq!(fnv(&r.pcm), 0x10cb285d45f0758d);
}

#[test]
fn mc2_banks_and_conf_on_smf() {
    let (Some(rom), Some(mdi), Some(sbk), Some(conf)) =
        (read(ROM), read(MDI), read(SBK), read(CONF))
    else {
        return;
    };
    let conf = String::from_utf8(conf).unwrap();
    let mut o = RenderOptions::new(&rom, Driver::Dos);
    o.banks = vec![BankSpec::sf(&mdi), BankSpec::sf(&sbk)];
    o.conf = Some(&conf); // master_volume 100 from the conf
    let r = render(&o, Score::Smf(&SYNTH_MID)).unwrap();
    assert_eq!(fnv(&r.pcm), 0xcc033be57f94fe4a);
}

#[test]
fn mc2_w_slot_xmi_master100() {
    let (Some(rom), Some(mdi), Some(sbk), Some(conf), Some(music)) =
        (read(ROM), read(MDI), read(SBK), read(CONF), read(MUSIC))
    else {
        return;
    };
    let conf = String::from_utf8(conf).unwrap();
    let xmi = w5(&music);
    let mut o = RenderOptions::new(&rom, Driver::Dos);
    o.banks = vec![BankSpec::sf(&mdi), BankSpec::sf(&sbk)];
    o.conf = Some(&conf);
    o.master_volume = Some(100);
    let r = render(&o, Score::Xmi(&xmi)).unwrap();
    assert_eq!(r.pcm.len(), 10717272);
    assert_eq!(fnv(&r.pcm), 0x26adbd87481ea752);
}

#[test]
fn mc2_w_slot_xmi_master127_tracks_1_2_3() {
    let (Some(rom), Some(mdi), Some(sbk), Some(conf), Some(music)) =
        (read(ROM), read(MDI), read(SBK), read(CONF), read(MUSIC))
    else {
        return;
    };
    let conf = String::from_utf8(conf).unwrap();
    let xmi = w5(&music);
    let mut o = RenderOptions::new(&rom, Driver::Dos);
    o.banks = vec![BankSpec::sf(&mdi), BankSpec::sf(&sbk)];
    o.conf = Some(&conf);
    o.master_volume = Some(127);
    o.channel_mask = mgc_awe32::parse_tracks("1,2,3");
    let r = render(&o, Score::Xmi(&xmi)).unwrap();
    assert_eq!(fnv(&r.pcm), 0x1cda196a541bf205);
}

#[test]
fn chip_replay_of_own_trace_matches_cpp_replay() {
    // Rust render with --trace, then the Rust chip replays that trace; the
    // hash is that of AWE32Emu --replay on the C++ trace of the same render
    // (the two traces are byte-identical).
    let (Some(rom), Some(mdi), Some(sbk), Some(conf), Some(music)) =
        (read(ROM), read(MDI), read(SBK), read(CONF), read(MUSIC))
    else {
        return;
    };
    let conf = String::from_utf8(conf).unwrap();
    let xmi = w5(&music);
    let mut o = RenderOptions::new(&rom, Driver::Dos);
    o.banks = vec![BankSpec::sf(&mdi), BankSpec::sf(&sbk)];
    o.conf = Some(&conf);
    o.master_volume = Some(100);
    o.trace = true;
    let r = render(&o, Score::Xmi(&xmi)).unwrap();
    assert_eq!(
        fnv(&r.pcm),
        0x26adbd87481ea752,
        "trace mode must not change the PCM"
    );
    let (writes, bytes) = replay::parse_trace(r.trace.as_deref().unwrap());
    assert_eq!(bytes, 0);
    let pcm = replay::replay(&rom, &r.dram, &writes, 8192).unwrap();
    assert_eq!(pcm.len(), 10761530);
    assert_eq!(fnv(&pcm), 0x74108b7e097af08);
}
