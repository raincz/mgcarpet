//! File-independent checks of the small parsers, plus the MDI table scan.

use mgc_awe32::render::ConfMessage;
use mgc_awe32::soundfont::{Bank, build_sbk_from_mdi, build_sbk_from_mdi_tables};
use mgc_awe32::{Conf, parse_tracks};

#[test]
fn tracks_mask() {
    assert_eq!(parse_tracks("1,2,3"), 0b111);
    assert_eq!(parse_tracks("-8,-9"), !(0b11u16 << 7));
    assert_eq!(parse_tracks("1,2,-2"), 0b1);
    assert_eq!(parse_tracks("16"), 0x8000);
}

#[test]
fn conf_parse() {
    let c = Conf::parse(
        "# x\nmaster_volume 100\ntrigger_mute 1\ncc 7 127 # vol\nprogram 0\nbend 8192\n",
    )
    .unwrap();
    assert_eq!(c.master, 100);
    assert!(c.trigger_mute);
    assert_eq!(
        c.messages,
        vec![
            ConfMessage::Control(7, 127),
            ConfMessage::Program(0),
            ConfMessage::Bend(8192)
        ]
    );
    assert!(Conf::parse("master_volume 200").is_err());
    assert!(Conf::parse("foo 1").is_err());
}

#[test]
fn conf_file_mc2() {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../reference/AWE32Emu/conf/mc2.conf"
    );
    let Ok(t) = std::fs::read_to_string(p) else {
        eprintln!("skipping: {p} not present");
        return;
    };
    let c = Conf::parse(&t).unwrap();
    assert_eq!(
        (c.master, c.trigger_mute, c.messages.len()),
        (100, true, 15)
    );
}

#[test]
fn mdi_tables_scan_on_mc1_executables() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/");
    let Ok(mdi) = std::fs::read(format!("{base}carpet2/patched/sound/sbawe32.mdi")) else {
        eprintln!("skipping: sbawe32.mdi not present");
        return;
    };
    let (riff, unmapped) = build_sbk_from_mdi(&mdi).unwrap();
    assert_eq!(unmapped, 0);
    assert_eq!(build_sbk_from_mdi_tables(&mdi).unwrap().0, riff);
    let b = Bank::load(&mdi).unwrap();
    assert_eq!(
        (b.presets.len(), b.instruments.len(), b.samples.len()),
        (129, 178, 152)
    );
    for exe in ["carpet/CARPET.EXE", "carpet/HIDDEN.EXE"] {
        let Ok(d) = std::fs::read(format!("{base}{exe}")) else {
            eprintln!("skipping: {exe} not present");
            continue;
        };
        // the plain (C++) scan trips over the EXE's different padding
        assert!(build_sbk_from_mdi(&d).is_err());
        assert_eq!(build_sbk_from_mdi_tables(&d).unwrap().0, riff, "{exe}");
    }
}
