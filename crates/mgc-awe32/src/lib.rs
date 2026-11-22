//! # mgc-awe32 - AWE32Emu in pure Rust
//!
//! A 1:1 transliteration of the rendering path of **AWE32Emu** by
//! **turican0** (<https://github.com/turican0/AWE32Emu>, the copy in
//! `reference/AWE32Emu`) in its 86box-chip configuration (`--rom`, the
//! default chip): MIDI/XMI -> Creative driver logic (`dos` = Miles
//! `SBAWE32.MDI`, and `sdk`) -> EMU8000 register writes -> the EMU8000 chip
//! **from 86Box** (`snd_emu8k.c`, with AWE32Emu's corrections measured on a
//! real Sound Blaster AWE32) -> 16-bit stereo PCM at 44100 Hz.
//!
//! The output is bit-identical to the C++ build on Linux/x86-64 for the
//! Magic Carpet recipes (see the crate's tests). Every ported function cites
//! its C/C++ source as `file:line`.
//!
//! ## Licence and credits
//!
//! * The chip (`chip.rs`) is a port of `snd_emu8k.c` from **86Box**
//!   (<https://86box.net>), GPL-2.0-or-later, with the AWE32Emu changes.
//! * The driver layer, SoundFont/MDI loader, sequencer and render loop are
//!   ports of AWE32Emu's own code by turican0, who agreed to ports and
//!   redistribution.
//! * The combined crate is distributed under GPL-3.0-or-later, like the
//!   rest of mgcarpet.
//!
//! The wave ROM (`awe32.raw`, Creative's 1 MB GM ROM) is not part of this
//! crate; it is passed in as bytes.
//!
//! ## Module map (C/C++ file -> module)
//!
//! | AWE32Emu source | module |
//! |---|---|
//! | `86box/snd_emu8k.c`, `86box/include/86box/snd_emu8k.h` | [`chip`] |
//! | `Emu8000Box.cpp/.h` | [`chip_box`] |
//! | `Emu8000.cpp/.h` (Box86 path only) | [`core`] |
//! | `Emu8000Regs.h`, `Awe32Driver.h`, `Awe32Curves.h`, `Awe32InitArrays.h` | [`driver`] |
//! | `SoundFont.cpp/.h` | [`soundfont`] |
//! | `Synth.cpp/.h` (`dos`, `sdk`) | [`synth`] |
//! | `MidiTypes.h`, `MidiFile.cpp`, `XmiFile.cpp`, `Sequencer.cpp` | [`midi`] |
//! | `main.cpp` (conf, render loop, `--replay`), `WavWriter.h` | [`render`], [`replay`], [`wav`] |
//!
//! ## Use
//!
//! ```no_run
//! use mgc_awe32::{render, BankSpec, Driver, RenderOptions, Score};
//! # let (rom, mdi, sbk, conf, xmi) = (vec![], vec![], vec![], String::new(), vec![]);
//! let mut o = RenderOptions::new(&rom, Driver::Dos);
//! o.banks = vec![BankSpec::sf(&mdi), BankSpec::sf(&sbk)];
//! o.conf = Some(&conf);              // reference/AWE32Emu/conf/mc2.conf
//! o.master_volume = Some(100);       // gameplay 100, menu 127
//! let out = render(&o, Score::Xmi(&xmi)).unwrap();
//! // out.pcm: interleaved stereo i16 @ 44100 Hz
//! ```

// Transliteration style, kept on purpose (see the workspace lint policy):
// the C `switch` arms end in `return;`, C clamps are written as two `if`s
// (f64::clamp differs on NaN), `(a + b - 1) / b` stays as in main.cpp.
#![allow(
    clippy::needless_return,
    clippy::manual_clamp,
    clippy::excessive_precision,
    clippy::manual_div_ceil
)]

pub mod chip;
pub mod chip_box;
pub mod core;
pub mod driver;
mod libm;
pub mod midi;
pub mod render;
pub mod replay;
pub mod soundfont;
pub mod synth;
pub mod wav;

/// Magic Carpet 2's MDI-driver start-up state for all sixteen channels
/// (`conf/mc2.conf`, AWE32Emu's measured file, copied verbatim): CC7
/// 127, CC91 0, the 12-semitone bend range, `master_volume 100` and
/// `trigger_mute 1` (CC119 value 0 → CC11 0: the war tracks start
/// silent). Pass as [`RenderOptions::conf`].
pub const MC2_CONF: &str = include_str!("../conf/mc2.conf");

pub use driver::Driver;
pub use render::{
    BankSource, BankSpec, Conf, RenderOptions, Rendered, Score, parse_tracks, render,
};
