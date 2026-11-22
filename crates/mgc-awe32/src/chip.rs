//! The EMU8000 chip: a 1:1 transliteration of `86box/snd_emu8k.c` (86Box,
//! with the AWE32Emu changes measured on a real AWE32) and its header
//! `86box/include/86box/snd_emu8k.h`.
//!
//! License: GPL-2.0-or-later (86Box). Citations `snd_emu8k.c:<line>` refer
//! to `reference/AWE32Emu/AWE32Emu/src/86box/snd_emu8k.c`.
//!
//! The build configuration ported is the one AWE32Emu compiles:
//! `RESAMPLER_BSPLINE4`, `FILTER_CHAMBERLIN`, `REVERB_FITTED`, no
//! `EMU8K_DEBUG_REGISTERS`, no state dump. Code that this configuration
//! never reaches is left out and listed here so nothing is silently lost:
//!
//! * the other resamplers (linear/cubic/point3/sinc) and `cubic_table`;
//! * the MOOG/INITIAL/CONSTANT filters and `filt_coeffs` (Chamberlin is on);
//! * the upstream reverb engine (`emu8k_work_reverb` and its comb/allpass
//!   state, `reverb_engine`): `REVERB_FITTED` replaces it. The INIT writes
//!   that only fed that engine are dropped (they changed nothing audible);
//! * `chortable`, `env_mod_hertz_to_octave` (computed, never read);
//! * the `EMU8K_STATE_DUMP` layered state dump (off unless an env var is
//!   set; it does not change the output);
//! * 86Box I/O glue (`emu8k_change_addr`, `io_sethandler`) and logging.
//!
//! C integer semantics are kept: `int` arithmetic that may overflow uses
//! wrapping operations, float -> int casts use [`cvtt_f32`] / [`cvtt_f64`]
//! (x86 `cvttss2si`/`cvttsd2si`: out of range or NaN gives `i32::MIN`,
//! where a plain Rust `as` would saturate).

use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// snd_emu8k.h defines
// ---------------------------------------------------------------------------

/// snd_emu8k.h:5
pub const EMU8K_MEM_ADDRESS_MASK: u32 = 0xFF_FFFF;
/// snd_emu8k.h:6
pub const EMU8K_RAM_MEM_START: u32 = 0x20_0000;
/// snd_emu8k.h:7
pub const EMU8K_FM_MEM_ADDRESS: u32 = 0xFF_FFE0;
/// snd_emu8k.h:9
pub const EMU8K_LFOCHORUS_SIZE: usize = 0x4000;
/// sound.h `WTBUFLEN` = 44100 / 45.
pub const WTBUFLEN: usize = 44100 / 45;

const ENV_STOPPED: i32 = 0;
const ENV_DELAY: i32 = 1;
const ENV_ATTACK: i32 = 2;
const ENV_HOLD: i32 = 3;
const ENV_SUSTAIN: i32 = 5;
const ENV_RAMP_DOWN: i32 = 7;
const ENV_RAMP_UP: i32 = 8;

/// x86 `cvttss2si`: what a C `(int32_t) float` compiles to on x86-64.
#[inline(always)]
pub fn cvtt_f32(x: f32) -> i32 {
    if x >= -2147483648.0f32 && x < 2147483648.0f32 {
        x as i32
    } else {
        i32::MIN
    }
}

/// x86 `cvttsd2si`: what a C `(int32_t) double` compiles to on x86-64.
#[inline(always)]
pub fn cvtt_f64(x: f64) -> i32 {
    if x > -2147483649.0f64 && x < 2147483648.0f64 {
        x as i32
    } else {
        i32::MIN
    }
}

// ---------------------------------------------------------------------------
// Static tables (snd_emu8k.c:152-210, filled in emu8k_init 2983-3103)
// ---------------------------------------------------------------------------

struct Tables {
    freqtable: Vec<i64>, // [65536]
    attentable: [i32; 256],
    env_vol_db_to_vol_target: Vec<i32>, // [65537]
    env_vol_amplitude_to_db: Vec<i32>,  // [65537]
    env_attack_to_samples: [i32; 128],
    env_decay_to_dbs_or_oct: [i32; 128],
    lfotable: Vec<i32>, // [65536]
    lfofreqtospeed: [i64; 256],
}

/// snd_emu8k.c:2989-3099 - the table part of `emu8k_init`.
fn build_tables() -> Tables {
    let mut freqtable = vec![0i64; 65536];
    for c in 0..0x10000i32 {
        // snd_emu8k.c:2990
        freqtable[c as usize] =
            ((((c - 0xe000) as f64) / 4096.0).exp2() * 65536.0 * 65536.0) as u64 as i64;
    }
    freqtable[0] = 0;

    let mut attentable = [0i32; 256];
    let mut out: f64 = 65535.0;
    for c in 0..256 {
        attentable[c] = cvtt_f64(out);
        out /= 1.09018f64.sqrt();
    }
    attentable[255] = 0;

    let mut env_vol_db_to_vol_target = vec![0i32; 65537];
    out = 65535.0;
    for c in 0..0x10000 {
        env_vol_db_to_vol_target[c] = cvtt_f64(out);
        out /= 1.00016923970;
    }
    env_vol_db_to_vol_target[0x10000 - 1] = 0;
    env_vol_db_to_vol_target[0x10000] = 0;

    let mut env_vol_amplitude_to_db = vec![0i32; 65537];
    for c in 1..0x10000 {
        out = -680.32142884264 * 20.0 * ((c as f64) / 65535.0).log10();
        env_vol_amplitude_to_db[c] = cvtt_f64(out);
    }
    env_vol_amplitude_to_db[0] = 65535;
    env_vol_amplitude_to_db[0x10000] = 0;

    // snd_emu8k.c:3053-3066
    let mut env_attack_to_samples = [0i32; 128];
    for c in 0..128i32 {
        let millis: f32 = if c == 0 {
            0.0
        } else {
            let idx = (if c > 127 { 127 } else { c }) - 1;
            let group = (idx >> 4) & 7;
            let m = idx & 15;
            let div = if group == 0 {
                m + 1
            } else {
                (m + 17) << (group - 1)
            };
            (11878.0f64 / div as f64) as f32
        };
        env_attack_to_samples[c as usize] = cvtt_f64(44.1 * millis as f64);
    }

    // snd_emu8k.c:3071-3079
    let mut env_decay_to_dbs_or_oct = [0i32; 128];
    for c in 1..128i32 {
        let idx = c - 1;
        let group = (idx >> 4) & 7;
        let m = idx & 15;
        let div = if group == 0 {
            m + 1
        } else {
            (m + 17) << (group - 1)
        };
        let db_per_s = 100.0 / (47.513 / div as f64);
        env_decay_to_dbs_or_oct[c as usize] =
            cvtt_f64(db_per_s / 44100.0 * ((1i32 << 21) as f64 / 96.0) * 65536.0 + 0.5);
    }

    // snd_emu8k.c:3087-3093
    let mut lfotable = vec![0i32; 65536];
    for c in 0..65536i32 {
        let d = (c + 16384) & 65535;
        lfotable[c as usize] = if d >= 32768 {
            32768 + ((32768 - d) * 2)
        } else {
            (d * 2) - 32768
        };
    }
    // snd_emu8k.c:3095-3099
    let mut lfofreqtospeed = [0i64; 256];
    out = 0.01;
    for c in 0..256 {
        lfofreqtospeed[c] = (out * 65536.0 / 44100.0 * 65536.0 * 65536.0) as u64 as i64;
        out += 0.042;
    }

    Tables {
        freqtable,
        attentable,
        env_vol_db_to_vol_target,
        env_vol_amplitude_to_db,
        env_attack_to_samples,
        env_decay_to_dbs_or_oct,
        lfotable,
        lfofreqtospeed,
    }
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(build_tables)
}

/// snd_emu8k.c:245 (with the AWE32Emu change at [7]).
const FILTER_ATTEN: [i32; 16] = [
    65536, 61869, 57079, 53269, 49145, 44820, 40877, 37690, 32845, 30653, 28607, 26392, 24630,
    22463, 20487, 18470,
];

/// snd_emu8k.c:264
const EMU8K_ATTACK_SHAPE: [f64; 21] = [
    0.000, 0.000, 0.005, 0.058, 0.116, 0.203, 0.280, 0.326, 0.372, 0.433, 0.493, 0.537, 0.580,
    0.621, 0.662, 0.703, 0.744, 0.807, 0.899, 0.947, 1.000,
];
/// snd_emu8k.c:275
const EMU8K_OVERSHOOT: [f64; 9] = [0.075, 0.081, 0.067, 0.053, 0.040, 0.029, 0.018, 0.007, 0.0];
const EMU8K_OVERSHOOT_STEP: i32 = 2205;

/// snd_emu8k.c:280
#[inline]
fn emu8k_overshoot_q12(samples: i32) -> i32 {
    if samples < 0 || samples >= 8 * EMU8K_OVERSHOOT_STEP {
        return 4096;
    }
    let i = (samples / EMU8K_OVERSHOOT_STEP) as usize;
    let f = (samples % EMU8K_OVERSHOOT_STEP) as f64 / EMU8K_OVERSHOOT_STEP as f64;
    cvtt_f64(
        4096.0 * (1.0 + EMU8K_OVERSHOOT[i] + (EMU8K_OVERSHOOT[i + 1] - EMU8K_OVERSHOOT[i]) * f),
    )
}

/// snd_emu8k.c:292
#[inline]
fn emu8k_attack_amp_index(phase: i32) -> i32 {
    let mut pos = phase as f64 / (1i32 << 21) as f64 * 20.0;
    if pos < 0.0 {
        pos = 0.0;
    }
    if pos > 20.0 {
        pos = 20.0;
    }
    let mut idx = cvtt_f64(pos);
    if idx > 19 {
        idx = 19;
    }
    let iu = idx as usize;
    let amp = EMU8K_ATTACK_SHAPE[iu]
        + (EMU8K_ATTACK_SHAPE[iu + 1] - EMU8K_ATTACK_SHAPE[iu]) * (pos - idx as f64);
    cvtt_f64(amp * 65536.0)
}

/// snd_emu8k.c:308
#[inline]
fn emu8k_mod_attack_value(phase: i32) -> i32 {
    let mut x = phase as f64 / (1i32 << 21) as f64;
    if x < 0.0 {
        x = 0.0;
    }
    if x > 1.0 {
        x = 1.0;
    }
    cvtt_f64((1.0 - libm_pow(1.0 - x, 13.5)) * (1i32 << 21) as f64)
}

/// snd_emu8k.c:320
#[inline]
fn emu8k_ramp_step(env: &mut Envelope) -> i32 {
    let s: i64 = env.ramp_amount_db_oct as i64 + env.ramp_frac as i64;
    env.ramp_frac = (s & 0xFFFF) as i32;
    (s >> 16) as i32
}

// Chamberlin filter, snd_emu8k.c:336-363
const EMU8K_CHAM_BASE_HZ: f64 = 101.81;
const EMU8K_CHAM_CENTS: f64 = 29.3843;
const EMU8K_CHAM_Q0: f64 = 0.931;
const EMU8K_CHAM_DB_PER_Q: f64 = 1.5;
const EMU8K_CHAM_TOP_DB: [f64; 16] = [
    0.0, 0.5, 1.25, 1.75, 2.5, 3.25, 4.25, 5.0, 6.25, 7.25, 9.0, 10.75, 12.5, 13.75, 15.25, 16.75,
];
const EMU8K_CHAM_TOP_LO_HZ: f64 = 2653.0;
const EMU8K_CHAM_TOP_HI_HZ: f64 = 6870.0;

/// snd_emu8k.c:354
#[inline]
fn emu8k_cham_q_db(q: i32, fc: f64) -> f64 {
    let mut t =
        (fc / EMU8K_CHAM_TOP_LO_HZ).log2() / (EMU8K_CHAM_TOP_HI_HZ / EMU8K_CHAM_TOP_LO_HZ).log2();
    if t < 0.0 {
        t = 0.0;
    }
    if t > 1.0 {
        t = 1.0;
    }
    (1.0 - t) * q as f64 * EMU8K_CHAM_DB_PER_Q + t * EMU8K_CHAM_TOP_DB[q as usize]
}

// Equalizer, snd_emu8k.c:366-441
#[derive(Clone, Copy)]
struct EqShelf {
    gain_db: f64,
    f0: f64,
}
const fn sh(gain_db: f64, f0: f64) -> EqShelf {
    EqShelf { gain_db, f0 }
}
const EMU8K_EQ_TREBLE: [EqShelf; 12] = [
    sh(-11.9, 1811.0),
    sh(-8.5, 1542.0),
    sh(-5.9, 1313.0),
    sh(-4.1, 1211.0),
    sh(-1.2, 1031.0),
    sh(0.0, 1000.0),
    sh(1.9, 1261.0),
    sh(3.5, 1671.0),
    sh(5.97, 2231.0),
    sh(7.90, 2457.0),
    sh(9.69, 2188.0),
    sh(11.16, 2231.0),
];
const EMU8K_EQ_BASS: [EqShelf; 12] = [
    sh(-11.9, 224.0),
    sh(-8.4, 273.0),
    sh(-5.9, 321.0),
    sh(-4.0, 362.0),
    sh(-1.1, 426.0),
    sh(0.0, 100.0),
    sh(2.0, 500.0),
    sh(3.5, 564.0),
    sh(6.0, 636.0),
    sh(8.0, 718.0),
    sh(9.6, 778.0),
    sh(12.0, 914.0),
];
const EMU8K_EQ_BASS_PARM: [[u16; 2]; 12] = [
    [0xD26A, 0xD36A],
    [0xD25B, 0xD35B],
    [0xD24C, 0xD34C],
    [0xD23D, 0xD33D],
    [0xD21F, 0xD31F],
    [0xC208, 0xC308],
    [0xC219, 0xC319],
    [0xC22A, 0xC32A],
    [0xC24C, 0xC34C],
    [0xC26E, 0xC36E],
    [0xC248, 0xC384],
    [0xC26A, 0xC36A],
];
const EMU8K_EQ_TREBLE_PARM: [[u16; 8]; 12] = [
    [
        0x821E, 0xC26A, 0x031E, 0xC36A, 0x021E, 0xD208, 0x831E, 0xD308,
    ],
    [
        0x821E, 0xC25B, 0x031E, 0xC35B, 0x021E, 0xD208, 0x831E, 0xD308,
    ],
    [
        0x821E, 0xC24C, 0x031E, 0xC34C, 0x021E, 0xD208, 0x831E, 0xD308,
    ],
    [
        0x821E, 0xC23D, 0x031E, 0xC33D, 0x021E, 0xD208, 0x831E, 0xD308,
    ],
    [
        0x821E, 0xC21F, 0x031E, 0xC31F, 0x021E, 0xD208, 0x831E, 0xD308,
    ],
    [
        0x821E, 0xD208, 0x031E, 0xD308, 0x021E, 0xD208, 0x831E, 0xD308,
    ],
    [
        0x821E, 0xD208, 0x031E, 0xD308, 0x021D, 0xD219, 0x831D, 0xD319,
    ],
    [
        0x821E, 0xD208, 0x031E, 0xD308, 0x021C, 0xD22A, 0x831C, 0xD32A,
    ],
    [
        0x821E, 0xD208, 0x031E, 0xD308, 0x021A, 0xD24C, 0x831A, 0xD34C,
    ],
    [
        0x821E, 0xD208, 0x031E, 0xD308, 0x0219, 0xD26E, 0x8319, 0xD36E,
    ],
    [
        0x821D, 0xD219, 0x031D, 0xD319, 0x0219, 0xD26E, 0x8319, 0xD36E,
    ],
    [
        0x821C, 0xD22A, 0x031C, 0xD32A, 0x0219, 0xD26E, 0x8319, 0xD36E,
    ],
];

/// snd_emu8k.c:406
fn emu8k_eq_word_match(reg: u16, table: u16) -> bool {
    let swapped = (reg & 0xFF00) | ((reg & 0x0F) << 4) | ((reg & 0xF0) >> 4);
    reg == table || swapped == table
}

/// snd_emu8k.c:413
fn emu8k_eq_design(q: &mut [f64; 5], s: &EqShelf, high: bool) {
    let a = libm_pow(10.0, s.gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * s.f0 / 44100.0;
    let cw = w0.cos();
    let alpha = w0.sin() / 2.0 * ((a + 1.0 / a) * (1.0 / 0.5 - 1.0) + 2.0).sqrt();
    let sq = 2.0 * a.sqrt() * alpha;
    let (b0, b1, b2, a0, a1, a2);
    if high {
        b0 = a * ((a + 1.0) + (a - 1.0) * cw + sq);
        b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cw);
        b2 = a * ((a + 1.0) + (a - 1.0) * cw - sq);
        a0 = (a + 1.0) - (a - 1.0) * cw + sq;
        a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cw);
        a2 = (a + 1.0) - (a - 1.0) * cw - sq;
    } else {
        b0 = a * ((a + 1.0) - (a - 1.0) * cw + sq);
        b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cw);
        b2 = a * ((a + 1.0) - (a - 1.0) * cw - sq);
        a0 = (a + 1.0) + (a - 1.0) * cw + sq;
        a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cw);
        a2 = (a + 1.0) + (a - 1.0) * cw - sq;
    }
    q[0] = b0 / a0;
    q[1] = b1 / a0;
    q[2] = b2 / a0;
    q[3] = a1 / a0;
    q[4] = a2 / a0;
}

// Fitted reverb, snd_emu8k.c:449-548
const EMU8K_RV_SLOTS: [[u8; 2]; 28] = [
    [1, 0x03],
    [1, 0x05],
    [4, 0x1F],
    [1, 0x07],
    [2, 0x14],
    [2, 0x16],
    [1, 0x0F],
    [1, 0x17],
    [1, 0x1F],
    [2, 0x07],
    [2, 0x0F],
    [2, 0x17],
    [2, 0x1D],
    [2, 0x1F],
    [3, 0x01],
    [3, 0x03],
    [1, 0x09],
    [1, 0x0B],
    [1, 0x11],
    [1, 0x13],
    [1, 0x19],
    [1, 0x1B],
    [2, 0x01],
    [2, 0x03],
    [2, 0x09],
    [2, 0x0B],
    [2, 0x11],
    [2, 0x13],
];
const EMU8K_RV_PARM: [[u16; 28]; 8] = [
    [
        0xB488, 0xA450, 0x9550, 0x84B5, 0x383A, 0x3EB5, 0x72F4, 0x72A4, 0x7254, 0x7204, 0x7204,
        0x7204, 0x4416, 0x4516, 0xA490, 0xA590, 0x842A, 0x852A, 0x842A, 0x852A, 0x8429, 0x8529,
        0x8429, 0x8529, 0x8428, 0x8528, 0x8428, 0x8528,
    ],
    [
        0xB488, 0xA458, 0x9558, 0x84B5, 0x383A, 0x3EB5, 0x7284, 0x7254, 0x7224, 0x7224, 0x7254,
        0x7284, 0x4448, 0x4548, 0xA440, 0xA540, 0x842A, 0x852A, 0x842A, 0x852A, 0x8429, 0x8529,
        0x8429, 0x8529, 0x8428, 0x8528, 0x8428, 0x8528,
    ],
    [
        0xB488, 0xA460, 0x9560, 0x84B5, 0x383A, 0x3EB5, 0x7284, 0x7254, 0x7224, 0x7224, 0x7254,
        0x7284, 0x4416, 0x4516, 0xA490, 0xA590, 0x842C, 0x852C, 0x842C, 0x852C, 0x842B, 0x852B,
        0x842B, 0x852B, 0x842A, 0x852A, 0x842A, 0x852A,
    ],
    [
        0xB488, 0xA470, 0x9570, 0x84B5, 0x383A, 0x3EB5, 0x7284, 0x7254, 0x7224, 0x7224, 0x7254,
        0x7284, 0x4448, 0x4548, 0xA440, 0xA540, 0x842B, 0x852B, 0x842B, 0x852B, 0x842A, 0x852A,
        0x842A, 0x852A, 0x8429, 0x8529, 0x8429, 0x8529,
    ],
    [
        0xB488, 0xA470, 0x9570, 0x84B5, 0x383A, 0x3EB5, 0x7254, 0x7234, 0x7224, 0x7254, 0x7264,
        0x7294, 0x44C3, 0x45C3, 0xA404, 0xA504, 0x842A, 0x852A, 0x842A, 0x852A, 0x8429, 0x8529,
        0x8429, 0x8529, 0x8428, 0x8528, 0x8428, 0x8528,
    ],
    [
        0xB4FF, 0xA470, 0x9570, 0x84B5, 0x383A, 0x3EB5, 0x7234, 0x7234, 0x7234, 0x7234, 0x7234,
        0x7234, 0x4448, 0x4548, 0xA440, 0xA540, 0x842A, 0x852A, 0x842A, 0x852A, 0x8429, 0x8529,
        0x8429, 0x8529, 0x8428, 0x8528, 0x8428, 0x8528,
    ],
    [
        0xB4FF, 0xA470, 0x9500, 0x84B5, 0x333A, 0x39B5, 0x7204, 0x7204, 0x7204, 0x7204, 0x7204,
        0x72F4, 0x4400, 0x4500, 0xA4FF, 0xA5FF, 0x8420, 0x8520, 0x8420, 0x8520, 0x8420, 0x8520,
        0x8420, 0x8520, 0x8420, 0x8520, 0x8420, 0x8520,
    ],
    [
        0xB4FF, 0xA490, 0x9590, 0x8474, 0x333A, 0x39B5, 0x7204, 0x7204, 0x7204, 0x7204, 0x7204,
        0x72F4, 0x4400, 0x4500, 0xA4FF, 0xA5FF, 0x8420, 0x8520, 0x8420, 0x8520, 0x8420, 0x8520,
        0x8420, 0x8520, 0x8420, 0x8520, 0x8420, 0x8520,
    ],
];
const EMU8K_RV_ROOM: [[f32; 4]; 8] = [
    [0.613, 0.045, 0.0, 0.347],
    [0.583, 0.08, 0.0, 0.587],
    [0.647, 0.03, 0.0, 0.620],
    [0.692, 0.09, 0.0, 0.849],
    [0.672, 0.06, 0.0, 0.988],
    [0.694, 0.10, 0.0, 0.912],
    [0.0, 0.0, 200.0, 0.38],
    [0.7, 0.0, 150.0, 0.54],
];
const EMU8K_RV_ECHO_PRE_MS: f32 = 0.0;
struct RvEchoParm {
    per: i32,
    d: [i32; 2],
    fb: f32,
    c: [f32; 2],
    g: [f32; 2],
}
const EMU8K_RV_ECHO_PARM: [RvEchoParm; 2] = [
    RvEchoParm {
        per: 5149,
        d: [5060, 4906],
        fb: 0.432,
        c: [1.0, 1.0],
        g: [0.945, 0.945],
    },
    RvEchoParm {
        per: 10408,
        d: [5094, 10187],
        fb: 0.313,
        c: [1.0, 1.0],
        g: [0.915, 0.496],
    },
];
const EMU8K_RV_ER_TAP: [[i32; 17]; 2] = [
    [
        57, 250, 327, 432, 596, 702, 778, 1030, 1135, 1388, 1493, 1570, 1646, 1927, 2003, 2197,
        2273,
    ],
    [
        44, 220, 309, 397, 573, 662, 750, 1014, 1102, 1367, 1455, 1544, 1632, 1896, 1984, 2161,
        2249,
    ],
];
const EMU8K_RV_ER_W: [f32; 17] = [
    0.4528, 0.3256, 0.3362, 0.1924, 0.1673, 0.4171, 0.1225, 0.2145, 0.3240, 0.1304, 0.1673, 0.1871,
    0.1265, 0.1897, 0.1049, 0.0949, 0.0837,
];
const EMU8K_RV_ER_GAIN: [f32; 8] = [0.639, 0.417, 0.409, 0.408, 0.324, 0.435, 0.0, 0.0];

// ---------------------------------------------------------------------------
// State (snd_emu8k.h:86-467)
// ---------------------------------------------------------------------------

/// `emu8k_envelope_t`, snd_emu8k.h:86
#[derive(Clone, Copy, Default)]
struct Envelope {
    state: i32,
    delay_samples: i32,
    hold_samples: i32,
    attack_samples: i32,
    value_amp_hz: i32,
    value_db_oct: i32,
    sustain_value_db_oct: i32,
    attack_amount_amp_hz: i32,
    ramp_amount_db_oct: i32,
    ramp_frac: i32,
}

/// `emu8k_voice_t`, snd_emu8k.h:171. The C unions are kept as their full
/// words; the sub-fields are the accessor functions below.
#[derive(Clone, Copy, Default)]
struct Voice {
    cpf: u32,
    ptrx: u32,
    cvcf: u32,
    volumeslide_last: i32,
    vtft: u32,
    z2: u32,
    z1: u32,
    psst: u32,
    csl: u32,
    ccca: u32,
    envvol: u16,
    dcysusv: u16,
    envval: u16,
    dcysus: u16,
    atkhldv: u16,
    lfo1val: u16,
    lfo2val: u16,
    atkhld: u16,
    ip: u16,
    ifatn: u16,
    pefe: u16,
    fmmod: u16,
    tremfrq: u16,
    fm2frq2: u16,
    env_engine_on: i32,
    /// emu8k_mem_internal_t: fract_lw 0..16, fract 16..32, int 32..64
    addr: u64,
    loop_start: u64,
    loop_end: u64,
    initial_att: i32,
    initial_filter: i32,
    vol_envelope: Envelope,
    mod_envelope: Envelope,
    lfo1_speed: i64,
    lfo2_speed: i64,
    lfo1_count: u64,
    lfo2_count: u64,
    lfo1_delay_samples: i32,
    lfo2_delay_samples: i32,
    vol_l: i32,
    vol_r: i32,
    fixed_modenv_filter_height: i16,
    fixed_modenv_pitch_height: i16,
    fixed_lfo1_filt_mod: i16,
    fixed_lfo1_vibrato: i16,
    fixed_lfo1_tremolo: i16,
    fixed_lfo2_vibrato: i16,
    filterq_idx: i32,
    filt_att: i32,
    cham_lp: f64,
    cham_bp: f64,
    overshoot_samples: i32,
    filt_oct_target: f64,
    filt_oct_curr: f64,
}

#[inline(always)]
fn mem_int(a: u64) -> u32 {
    (a >> 32) as u32
}
#[inline(always)]
fn set_mem_int(a: &mut u64, v: u32) {
    *a = (*a & 0xFFFF_FFFF) | ((v as u64) << 32);
}
#[inline(always)]
fn mem_fract(a: u64) -> u16 {
    (a >> 16) as u16
}

impl Voice {
    #[inline(always)]
    fn cpf_curr_pitch(&self) -> u16 {
        (self.cpf >> 16) as u16
    }
    #[inline(always)]
    fn set_cpf_curr_pitch(&mut self, v: u16) {
        self.cpf = (self.cpf & 0xFFFF) | ((v as u32) << 16);
    }
    #[inline(always)]
    fn set_cpf_curr_frac_addr(&mut self, v: u16) {
        self.cpf = (self.cpf & 0xFFFF_0000) | v as u32;
    }
    #[inline(always)]
    fn ptrx_revb_send(&self) -> u8 {
        (self.ptrx >> 8) as u8
    }
    #[inline(always)]
    fn ptrx_pit_target(&self) -> u16 {
        (self.ptrx >> 16) as u16
    }
    #[inline(always)]
    fn set_ptrx_pit_target(&mut self, v: u16) {
        self.ptrx = (self.ptrx & 0xFFFF) | ((v as u32) << 16);
    }
    #[inline(always)]
    fn cvcf_curr_volume(&self) -> u16 {
        (self.cvcf >> 16) as u16
    }
    #[inline(always)]
    fn set_cvcf_curr_volume(&mut self, v: u16) {
        self.cvcf = (self.cvcf & 0xFFFF) | ((v as u32) << 16);
    }
    #[inline(always)]
    fn set_cvcf_curr_filt_ctoff(&mut self, v: u16) {
        self.cvcf = (self.cvcf & 0xFFFF_0000) | v as u32;
    }
    #[inline(always)]
    fn vtft_vol_target(&self) -> u16 {
        (self.vtft >> 16) as u16
    }
    #[inline(always)]
    fn set_vtft_vol_target(&mut self, v: u16) {
        self.vtft = (self.vtft & 0xFFFF) | ((v as u32) << 16);
    }
    #[inline(always)]
    fn vtft_filter_target(&self) -> u16 {
        self.vtft as u16
    }
    #[inline(always)]
    fn set_vtft_filter_target(&mut self, v: u16) {
        self.vtft = (self.vtft & 0xFFFF_0000) | v as u32;
    }
    #[inline(always)]
    fn psst_pan(&self) -> u8 {
        (self.psst >> 24) as u8
    }
    #[inline(always)]
    fn csl_chor_send(&self) -> u8 {
        (self.csl >> 24) as u8
    }
    #[inline(always)]
    fn ccca_qcontrol(&self) -> u8 {
        (self.ccca >> 24) as u8
    }
    #[inline(always)]
    fn ifatn_attenuation(&self) -> u8 {
        self.ifatn as u8
    }
    #[inline(always)]
    fn ifatn_init_filter(&self) -> u8 {
        (self.ifatn >> 8) as u8
    }
    #[inline(always)]
    fn pefe_modenv_filter_height(&self) -> i8 {
        self.pefe as u8 as i8
    }
    #[inline(always)]
    fn pefe_modenv_pitch_height(&self) -> i8 {
        (self.pefe >> 8) as u8 as i8
    }
    #[inline(always)]
    fn fmmod_lfo1_filt_mod(&self) -> i8 {
        self.fmmod as u8 as i8
    }
    #[inline(always)]
    fn fmmod_lfo1_vibrato(&self) -> i8 {
        (self.fmmod >> 8) as u8 as i8
    }
}

/// `READ16_SWITCH`, snd_emu8k.c:695
#[inline(always)]
fn read16(addr: u16, var: u32) -> u16 {
    if addr & 2 == 0 {
        (var & 0xffff) as u16
    } else {
        ((var >> 16) & 0xffff) as u16
    }
}

/// `WRITE16_SWITCH`, snd_emu8k.c:705
#[inline(always)]
fn write16(addr: u16, var: &mut u32, val: u32) {
    if addr & 2 == 0 {
        *var = (*var & 0xffff_0000) | val;
    } else {
        *var = (*var & 0x0000_ffff) | (val << 16);
    }
}

/// `emu8k_chorus_eng_t`, snd_emu8k.h:102
struct ChorusEng {
    write: i32,
    feedback: i32,
    delay_samples_central: i32,
    lfodepth_multip: f64,
    delay_offset_samples_right: f64,
    lfo_inc: u64,
    lfo_pos: u64,
    chorus_left_buffer: Vec<i32>, // [EMU8K_LFOCHORUS_SIZE]
}

const ROM_WORDS: usize = 0x80000;
const BLOCK_SIZE_WORDS: usize = 0x10000;

/// `emu8k_t`, snd_emu8k.h:370
pub struct Emu8k {
    voice: [Voice; 32],

    hwcf1: u16,
    hwcf2: u16,
    hwcf3: u16,
    hwcf4: u32,
    hwcf5: u32,
    hwcf6: u32,
    hwcf7: u32,

    init1: [u16; 32],
    init2: [u16; 32],
    init3: [u16; 32],
    init4: [u16; 32],

    smalr: u32,
    smarr: u32,
    smalw: u32,
    smarw: u32,
    smld_buffer: u16,
    smrd_buffer: u16,

    wc: u16,
    id: u16,

    eq_bass: i32,
    eq_treble: i32,
    eq_coef: [[f64; 5]; 2],
    eq_z: [[[f64; 2]; 2]; 2],

    rv_comb: Vec<[[f32; 1664]; 8]>, // [2]
    rv_comb_len: [[i32; 8]; 2],
    rv_comb_pos: [[i32; 8]; 2],
    rv_comb_store: [[f32; 8]; 2],
    rv_ap: Vec<[[f32; 600]; 4]>, // [2]
    rv_ap_len: [[i32; 4]; 2],
    rv_ap_pos: [[i32; 4]; 2],
    rv_pre: Vec<f32>, // [13231]
    rv_pre_len: i32,
    rv_pre_pos: i32,
    rv_preset: i32,
    rv_feedback: f32,
    rv_damp: f32,
    rv_in_gain: f32,
    rv_out_gain: f32,
    rv_echo_mode: i32,
    rv_echo_x: Vec<f32>, // [16384]
    rv_echo_z: Vec<f32>, // [16384]
    rv_echo_pos: i32,
    rv_echo_per: i32,
    rv_echo_d: [i32; 2],
    rv_echo_fb: f32,
    rv_echo_c: [f32; 2],
    rv_echo_g: [f32; 2],
    rv_er: Vec<f32>, // [2400]
    rv_er_pos: i32,
    rv_er_gain: f32,

    /// Sound memory: [ROM 0x80000 words][empty block 0x10000 words][RAM].
    /// `ram_pointers[hb]` is the word offset of block `hb` in it - the
    /// C code's array of pointers (snd_emu8k.h:449) as offsets.
    mem: Vec<i16>,
    ram_pointers: [u32; 0x100],
    ram_base: usize,
    has_ram: bool,
    ram_end_addr: u32,

    cur_reg: i32,
    cur_voice: i32,

    chorus_engine: ChorusEng,
    chorus_in_buffer: [i32; WTBUFLEN],
    reverb_in_buffer: [i32; WTBUFLEN],

    pos: i32,
    /// Output of the current block, interleaved L/R (`buffer`).
    pub buffer: [i32; WTBUFLEN * 2],

    // Process-wide statics of the C file, per chip here (snd_emu8k.c:142-144).
    random_helper: i32,
    dmareadbit: u32,
    dmawritebit: u32,

    t: &'static Tables,
}

impl Emu8k {
    /// `emu8k_init` (snd_emu8k.c:2926) on a zeroed `emu8k_t`. `rom` is the
    /// raw ROM file (what `rom_fopen` + `fread` would read; at least
    /// 1048574 bytes). `onboard_ram` in KB. The I/O address is not needed
    /// (the ports are passed absolute, 0x620/0xA20/0xE20...).
    pub fn new(rom: &[u8], onboard_ram: i32) -> Result<Box<Emu8k>, String> {
        // snd_emu8k.c:2937-2948
        if rom.len() < 1048574 {
            return Err(format!(
                "wave ROM has only {} B, the 86box chip needs 1 MB",
                rom.len()
            ));
        }
        let mut onboard_ram = onboard_ram;
        if onboard_ram > 0x7000 {
            onboard_ram = 0x7000;
        }
        let ram_words = if onboard_ram > 0 {
            onboard_ram as usize * 512
        } else {
            0
        };
        let ram_base = ROM_WORDS + BLOCK_SIZE_WORDS;
        let mut mem = vec![0i16; ram_base + ram_words];
        let n = rom.len().min(1048576);
        for i in 0..n / 2 {
            mem[i] = i16::from_le_bytes([rom[2 * i], rom[2 * i + 1]]);
        }
        if n & 1 == 1 {
            // fread of an odd byte count fills the low byte of the last word
            mem[n / 2] = rom[n - 1] as i16;
        }
        if mem[3] as u16 == 0x314d && mem[4] as u16 == 0x474d {
            mem.copy_within(1..ROM_WORDS, 0);
            mem[0x7ffff] = 0;
        }

        let empty = ROM_WORDS as u32;
        let mut ram_pointers = [0u32; 0x100];
        let mut j = 0usize;
        while j < 0x8 {
            ram_pointers[j] = (j * BLOCK_SIZE_WORDS) as u32;
            j += 1;
        }
        while j < 0x20 {
            ram_pointers[j] = empty;
            j += 1;
        }
        let ram_end_addr;
        if onboard_ram > 0 {
            let i_end = (onboard_ram >> 7) as usize;
            for i in 0..i_end {
                ram_pointers[j] = (ram_base + i * BLOCK_SIZE_WORDS) as u32;
                j += 1;
            }
            ram_end_addr = EMU8K_RAM_MEM_START + ((onboard_ram as u32) << 9);
        } else {
            ram_end_addr = EMU8K_RAM_MEM_START;
        }
        while j < 0x100 {
            ram_pointers[j] = empty;
            j += 1;
        }

        let mut e = Box::new(Emu8k {
            voice: [Voice::default(); 32],
            hwcf1: 0,
            hwcf2: 0,
            hwcf3: 0,
            hwcf4: 0,
            hwcf5: 0,
            hwcf6: 0,
            hwcf7: 0,
            init1: [0; 32],
            init2: [0; 32],
            init3: [0; 32],
            init4: [0; 32],
            smalr: 0,
            smarr: 0,
            smalw: 0,
            smarw: 0,
            smld_buffer: 0,
            smrd_buffer: 0,
            wc: 0,
            id: 0,
            eq_bass: 0,
            eq_treble: 0,
            eq_coef: [[0.0; 5]; 2],
            eq_z: [[[0.0; 2]; 2]; 2],
            rv_comb: vec![[[0.0; 1664]; 8]; 2],
            rv_comb_len: [[0; 8]; 2],
            rv_comb_pos: [[0; 8]; 2],
            rv_comb_store: [[0.0; 8]; 2],
            rv_ap: vec![[[0.0; 600]; 4]; 2],
            rv_ap_len: [[0; 4]; 2],
            rv_ap_pos: [[0; 4]; 2],
            rv_pre: vec![0.0; 13231],
            rv_pre_len: 0,
            rv_pre_pos: 0,
            rv_preset: 0,
            rv_feedback: 0.0,
            rv_damp: 0.0,
            rv_in_gain: 0.0,
            rv_out_gain: 0.0,
            rv_echo_mode: 0,
            rv_echo_x: vec![0.0; 16384],
            rv_echo_z: vec![0.0; 16384],
            rv_echo_pos: 0,
            rv_echo_per: 0,
            rv_echo_d: [0; 2],
            rv_echo_fb: 0.0,
            rv_echo_c: [0.0; 2],
            rv_echo_g: [0.0; 2],
            rv_er: vec![0.0; 2400],
            rv_er_pos: 0,
            rv_er_gain: 0.0,
            mem,
            ram_pointers,
            ram_base,
            has_ram: onboard_ram > 0,
            ram_end_addr,
            cur_reg: 0,
            cur_voice: 0,
            chorus_engine: ChorusEng {
                write: 0,
                feedback: 0,
                delay_samples_central: 0,
                lfodepth_multip: 0.0,
                delay_offset_samples_right: 0.0,
                lfo_inc: 0,
                lfo_pos: 0,
                chorus_left_buffer: vec![0; EMU8K_LFOCHORUS_SIZE],
            },
            chorus_in_buffer: [0; WTBUFLEN],
            reverb_in_buffer: [0; WTBUFLEN],
            pos: 0,
            buffer: [0; WTBUFLEN * 2],
            random_helper: 0,
            dmareadbit: 0,
            dmawritebit: 0,
            t: tables(),
        });

        e.reset_buffer();
        // snd_emu8k.c:3080-3082
        e.eq_bass = -1;
        e.eq_treble = -1;
        e.rv_init();
        // snd_emu8k.c:3174-3177
        e.hwcf1 = 0x59;
        e.hwcf2 = 0x20;
        e.hwcf3 = 0x00;
        Ok(e)
    }

    /// The onboard DRAM (from 0x200000), as `emu8k->ram`.
    pub fn ram_mut(&mut self) -> &mut [i16] {
        let b = self.ram_base;
        &mut self.mem[b..]
    }

    /// `ram_end_addr - EMU8K_RAM_MEM_START` (Emu8000Box::RamWords).
    pub fn ram_words(&self) -> usize {
        if !self.has_ram {
            0
        } else {
            (self.ram_end_addr - EMU8K_RAM_MEM_START) as usize
        }
    }

    /// `EMU8K_READ`, snd_emu8k.c:790
    #[inline(always)]
    fn read(&self, addr: u32) -> i16 {
        let lw = (addr & 0xFFFF) as usize;
        let hb = ((addr >> 16) & 0xFF) as usize;
        self.mem[self.ram_pointers[hb] as usize + lw]
    }

    /// `EMU8K_READ_INTERP_BSPLINE4`, snd_emu8k.c:880
    #[inline(always)]
    fn read_interp_bspline4(&self, a: u32, fract: u16) -> i32 {
        let g: f32 = fract as f32 / 65536.0f32;
        let g2 = g * g;
        let g3 = g2 * g;
        let u = 1.0f32 - g;
        let p0 = self.read(a) as f32;
        let p1 = self.read(a.wrapping_add(1)) as f32;
        let p2 = self.read(a.wrapping_add(2)) as f32;
        let p3 = self.read(a.wrapping_add(3)) as f32;
        cvtt_f32(
            (u * u * u * p0
                + (3.0f32 * g3 - 6.0f32 * g2 + 4.0f32) * p1
                + (-3.0f32 * g3 + 3.0f32 * g2 + 3.0f32 * g + 1.0f32) * p2
                + g3 * p3)
                / 6.0f32,
        )
    }

    /// `EMU8K_WRITE`, snd_emu8k.c:926
    fn write_mem(&mut self, addr: u32, val: u16) {
        let mut addr = addr & EMU8K_MEM_ADDRESS_MASK;
        if !self.has_ram || addr < EMU8K_RAM_MEM_START || addr >= EMU8K_FM_MEM_ADDRESS {
            return;
        }
        while addr >= self.ram_end_addr {
            addr -= self.ram_end_addr - EMU8K_RAM_MEM_START;
        }
        let i = self.ram_base + (addr - EMU8K_RAM_MEM_START) as usize;
        self.mem[i] = val as i16;
    }

    // -----------------------------------------------------------------------
    // Fitted reverb, snd_emu8k.c:550-693
    // -----------------------------------------------------------------------

    /// snd_emu8k.c:551
    fn rv_set_preset(&mut self, preset: i32) {
        let r = EMU8K_RV_ROOM[preset as usize];
        self.rv_preset = preset;
        self.rv_er_gain = EMU8K_RV_ER_GAIN[preset as usize];
        self.rv_echo_mode = if preset == 6 {
            1
        } else if preset == 7 {
            2
        } else {
            0
        };
        if self.rv_echo_mode != 0 {
            let e = &EMU8K_RV_ECHO_PARM[(preset - 6) as usize];
            self.rv_echo_per = e.per;
            self.rv_echo_fb = e.fb;
            for ch in 0..2 {
                self.rv_echo_d[ch] = e.d[ch];
                self.rv_echo_c[ch] = e.c[ch];
                self.rv_echo_g[ch] = e.g[ch];
            }
            self.rv_echo_pos = 0;
            self.rv_echo_x.fill(0.0);
            self.rv_echo_z.fill(0.0);
        }
        self.rv_feedback = 0.7f32 + r[0] * 0.28f32;
        self.rv_damp = r[1];
        self.rv_in_gain = 1.0f32 - self.rv_feedback;
        self.rv_out_gain = r[3];
        let ms = if self.rv_echo_mode != 0 {
            EMU8K_RV_ECHO_PRE_MS
        } else {
            r[2]
        };
        let len = cvtt_f32(ms * 44100.0f32 / 1000.0f32);
        if len != self.rv_pre_len {
            self.rv_pre_len = len;
            self.rv_pre_pos = 0;
            self.rv_pre.fill(0.0);
        }
    }

    /// snd_emu8k.c:583
    fn rv_init(&mut self) {
        const COMB: [i32; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        const ALLPASS: [i32; 4] = [556, 441, 341, 225];
        for ch in 0..2 {
            let spread = if ch != 0 { 23 } else { 0 };
            for i in 0..8 {
                self.rv_comb_len[ch][i] = COMB[i] + spread;
            }
            for i in 0..4 {
                self.rv_ap_len[ch][i] = ALLPASS[i] + spread;
            }
        }
        self.rv_pre_len = -1;
        self.rv_set_preset(4);
    }

    /// snd_emu8k.c:599
    fn rv_decode(&mut self) {
        let mut best = -1;
        let mut best_hits = 0;
        for p in 0..8 {
            let mut hits = 0;
            for i in 0..28 {
                let slot = EMU8K_RV_SLOTS[i][1] as usize;
                let v = match EMU8K_RV_SLOTS[i][0] {
                    1 => self.init1[slot],
                    2 => self.init2[slot],
                    3 => self.init3[slot],
                    _ => self.init4[slot],
                };
                hits += (v == EMU8K_RV_PARM[p][i]) as i32;
            }
            if hits > best_hits {
                best_hits = hits;
                best = p as i32;
            }
        }
        if best_hits >= 24 && best != self.rv_preset {
            self.rv_set_preset(best);
        }
    }

    /// `emu8k_work_reverb_fitted`, snd_emu8k.c:625. Reads
    /// `reverb_in_buffer[start..]`, adds into `buffer[2*start..]`.
    fn work_reverb_fitted(&mut self, start: usize, count: usize) {
        self.rv_decode();
        for n in 0..count {
            let mut inp: f32 = self.reverb_in_buffer[start + n] as f32 / 32768.0f32;
            if self.rv_pre_len > 0 {
                let pp = self.rv_pre_pos as usize;
                let delayed = self.rv_pre[pp];
                self.rv_pre[pp] = inp;
                self.rv_pre_pos += 1;
                if self.rv_pre_pos >= self.rv_pre_len {
                    self.rv_pre_pos = 0;
                }
                inp = delayed;
            }
            let ob = 2 * (start + n);
            if self.rv_echo_mode != 0 {
                let pos = self.rv_echo_pos;
                let old = ((pos - self.rv_echo_per) & 16383) as usize;
                self.rv_echo_z[pos as usize] =
                    self.rv_echo_fb * (self.rv_echo_x[old] + self.rv_echo_z[old]);
                self.rv_echo_x[pos as usize] = inp;
                for ch in 0..2 {
                    let p = ((pos - self.rv_echo_d[ch]) & 16383) as usize;
                    let v = self.rv_echo_c[ch] * self.rv_echo_x[p] + self.rv_echo_z[p];
                    self.buffer[ob + ch] = self.buffer[ob + ch]
                        .wrapping_add(cvtt_f32(v * self.rv_echo_g[ch] * 32768.0f32));
                }
                self.rv_echo_pos = (pos + 1) & 16383;
                continue;
            }
            let scaled = inp * self.rv_in_gain;
            let mut er = [0.0f32; 2];
            if self.rv_er_gain > 0.0f32 {
                self.rv_er[self.rv_er_pos as usize] = inp;
                for ch in 0..2 {
                    let mut sum = 0.0f32;
                    for t in 0..17 {
                        let mut p = self.rv_er_pos - EMU8K_RV_ER_TAP[ch][t];
                        if p < 0 {
                            p += 2400;
                        }
                        sum += self.rv_er[p as usize] * EMU8K_RV_ER_W[t];
                    }
                    er[ch] = sum * self.rv_er_gain;
                }
                self.rv_er_pos += 1;
                if self.rv_er_pos >= 2400 {
                    self.rv_er_pos = 0;
                }
            }
            for ch in 0..2 {
                let mut acc = 0.0f32;
                for i in 0..8 {
                    let pos = self.rv_comb_pos[ch][i] as usize;
                    let out = self.rv_comb[ch][i][pos];
                    self.rv_comb_store[ch][i] =
                        out * (1.0f32 - self.rv_damp) + self.rv_comb_store[ch][i] * self.rv_damp;
                    self.rv_comb[ch][i][pos] =
                        scaled + self.rv_comb_store[ch][i] * self.rv_feedback;
                    self.rv_comb_pos[ch][i] = (pos as i32 + 1) % self.rv_comb_len[ch][i];
                    acc += out;
                }
                acc *= 0.125f32;
                for i in 0..4 {
                    let pos = self.rv_ap_pos[ch][i] as usize;
                    let buf = self.rv_ap[ch][i][pos];
                    self.rv_ap[ch][i][pos] = acc + buf * 0.5f32;
                    self.rv_ap_pos[ch][i] = (pos as i32 + 1) % self.rv_ap_len[ch][i];
                    acc = buf - acc;
                }
                self.buffer[ob + ch] = self.buffer[ob + ch]
                    .wrapping_add(cvtt_f32((acc * self.rv_out_gain + er[ch]) * 32768.0f32));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Port reads, snd_emu8k.c:941-1347
    // -----------------------------------------------------------------------

    /// `emu8k_inw` (snd_emu8k.c:1341 -> `emu8k_inw_untraced` 942; the trace
    /// hooks are empty).
    pub fn inw(&mut self, addr: u16) -> u16 {
        let cv = self.cur_voice as usize;
        match addr & 0xF02 {
            0x600 | 0x602 => {
                let v = &self.voice[cv];
                match self.cur_reg {
                    0 => return read16(addr, v.cpf),
                    1 => return read16(addr, v.ptrx),
                    2 => return read16(addr, v.cvcf),
                    3 => return read16(addr, v.vtft),
                    4 => return read16(addr, v.z2),
                    5 => return read16(addr, v.z1),
                    6 => return read16(addr, v.psst),
                    7 => return read16(addr, v.csl),
                    _ => {}
                }
            }
            0xA00 => match self.cur_reg {
                0 => return read16(addr, self.voice[cv].ccca),
                1 => match self.cur_voice {
                    9 => return read16(addr, self.hwcf4),
                    10 => return read16(addr, self.hwcf5),
                    13 => return read16(addr, self.hwcf6),
                    14 => return read16(addr, self.hwcf7),
                    20 => return read16(addr, self.smalr),
                    21 => return read16(addr, self.smarr),
                    22 => return read16(addr, self.smalw),
                    23 => return read16(addr, self.smarw),
                    26 => {
                        let val = self.smld_buffer;
                        self.smld_buffer = self.read(self.smalr) as u16;
                        self.smalr = (self.smalr + 1) & EMU8K_MEM_ADDRESS_MASK;
                        return val;
                    }
                    29 => return (self.hwcf1 & 0xfe) | (self.hwcf3 & 0x01),
                    30 => {
                        return ((self.hwcf2 >> 4) & 0x0e)
                            | (self.hwcf1 & 0x01)
                            | (if self.hwcf3 & 0x02 != 0 { 0x10 } else { 0 })
                            | (if self.hwcf3 & 0x04 != 0 { 0x40 } else { 0 })
                            | (if self.hwcf3 & 0x08 != 0 { 0x20 } else { 0 })
                            | (if self.hwcf3 & 0x10 != 0 { 0x80 } else { 0 });
                    }
                    31 => return self.hwcf2 & 0x1f,
                    _ => {}
                },
                2 => return self.init1[cv],
                3 => return self.init3[cv],
                4 => return self.voice[cv].envvol,
                5 => return self.voice[cv].dcysusv,
                6 => return self.voice[cv].envval,
                7 => return self.voice[cv].dcysus,
                _ => {}
            },
            0xA02 => match self.cur_reg {
                0 => return read16(addr, self.voice[cv].ccca),
                1 => match self.cur_voice {
                    9 => return read16(addr, self.hwcf4),
                    10 => return read16(addr, self.hwcf5),
                    13 => return read16(addr, self.hwcf6),
                    14 => return read16(addr, self.hwcf7),
                    20 => {
                        let r = read16(addr, self.smalr | self.dmareadbit);
                        self.dmareadbit = 0;
                        return r;
                    }
                    21 => {
                        let r = read16(addr, self.smarr | self.dmareadbit);
                        self.dmareadbit = 0;
                        return r;
                    }
                    22 => {
                        let r = read16(addr, self.smalw | self.dmawritebit);
                        self.dmawritebit = 0;
                        return r;
                    }
                    23 => {
                        let r = read16(addr, self.smarw | self.dmawritebit);
                        self.dmawritebit = 0;
                        return r;
                    }
                    26 => {
                        let val = self.smrd_buffer;
                        self.smrd_buffer = self.read(self.smarr) as u16;
                        self.smarr = (self.smarr + 1) & EMU8K_MEM_ADDRESS_MASK;
                        return val;
                    }
                    27 => return self.wc,
                    _ => {}
                },
                2 => return self.init2[cv],
                3 => return self.init4[cv],
                4 => return self.voice[cv].atkhldv,
                5 => return self.voice[cv].lfo1val,
                6 => return self.voice[cv].atkhld,
                7 => return self.voice[cv].lfo2val,
                _ => {}
            },
            0xE00 => match self.cur_reg {
                0 => return self.voice[cv].ip,
                1 => return self.voice[cv].ifatn,
                2 => return self.voice[cv].pefe,
                3 => return self.voice[cv].fmmod,
                4 => return self.voice[cv].tremfrq,
                5 => return self.voice[cv].fm2frq2,
                6 => return 0xffff,
                7 => return 0x0c | (if self.id & 0x0002 != 0 { 0xff02 } else { 0 }),
                _ => {}
            },
            0xE02 => {
                self.random_helper = (self.random_helper + 1) & 0x1F;
                return ((((0x80 | self.random_helper) << 8) | (self.cur_reg << 5) | self.cur_voice)
                    & 0xFFFF) as u16;
            }
            _ => {}
        }
        0xffff
    }

    // -----------------------------------------------------------------------
    // Port writes, snd_emu8k.c:1349-2113
    // -----------------------------------------------------------------------

    /// Starts the volume envelope (shared by DCYSUSV and ATKHLDV writes,
    /// snd_emu8k.c:1653-1668 / 1928-1943).
    #[inline]
    fn trigger_vol_env(v: &mut Voice) {
        v.vol_envelope.value_amp_hz = 0;
        v.overshoot_samples = -1;
        let e = &mut v.vol_envelope;
        if e.delay_samples != 0 {
            e.state = ENV_DELAY;
        } else if e.attack_amount_amp_hz == 0 {
            e.state = ENV_STOPPED;
        } else {
            e.state = ENV_ATTACK;
        }
    }

    /// Starts the modulation envelope (snd_emu8k.c:1672-1688 / 1967-1982).
    #[inline]
    fn trigger_mod_env(e: &mut Envelope) {
        e.value_amp_hz = 0;
        e.value_db_oct = 0;
        if e.delay_samples != 0 {
            e.state = ENV_DELAY;
        } else if e.attack_amount_amp_hz == 0 {
            e.state = ENV_STOPPED;
        } else {
            e.state = ENV_ATTACK;
        }
    }

    /// `emu8k_outw`, snd_emu8k.c:1350. `pos_global` is
    /// `wavetable_pos_global` at the time of the write (the update to it
    /// runs first, as in the C code).
    pub fn outw(&mut self, addr: u16, val: u16, pos_global: i32) {
        self.update(pos_global);
        let t = self.t;
        let cv = self.cur_voice as usize;
        let val32 = val as u32;

        match addr & 0xF02 {
            0x600 | 0x602 => {
                let v = &mut self.voice[cv];
                match self.cur_reg {
                    0 => {
                        write16(addr, &mut v.cpf, val32);
                        return;
                    }
                    1 => {
                        write16(addr, &mut v.ptrx, val32);
                        return;
                    }
                    2 => {
                        write16(addr, &mut v.cvcf, val32);
                        return;
                    }
                    3 => {
                        write16(addr, &mut v.vtft, val32);
                        return;
                    }
                    4 => {
                        write16(addr, &mut v.z2, val32);
                        return;
                    }
                    5 => {
                        write16(addr, &mut v.z1, val32);
                        return;
                    }
                    6 => {
                        write16(addr, &mut v.psst, val32);
                        set_mem_int(&mut v.loop_start, v.psst & EMU8K_MEM_ADDRESS_MASK);
                        if addr & 2 != 0 {
                            v.vol_l = v.psst_pan() as i32;
                            v.vol_r = 255 - v.psst_pan() as i32;
                        }
                        return;
                    }
                    7 => {
                        write16(addr, &mut v.csl, val32);
                        set_mem_int(&mut v.loop_end, v.csl & EMU8K_MEM_ADDRESS_MASK);
                        return;
                    }
                    _ => {}
                }
            }
            0xA00 => match self.cur_reg {
                0 => {
                    let v = &mut self.voice[cv];
                    write16(addr, &mut v.ccca, val32);
                    set_mem_int(&mut v.addr, v.ccca & EMU8K_MEM_ADDRESS_MASK);
                    return;
                }
                1 => match self.cur_voice {
                    9 => {
                        write16(addr, &mut self.hwcf4, val32);
                        return;
                    }
                    10 => {
                        write16(addr, &mut self.hwcf5, val32);
                        return;
                    }
                    13 => {
                        write16(addr, &mut self.hwcf6, val32);
                        return;
                    }
                    14 => {
                        write16(addr, &mut self.hwcf7, val32);
                        return;
                    }
                    20 => {
                        write16(addr, &mut self.smalr, val32);
                        return;
                    }
                    21 => {
                        write16(addr, &mut self.smarr, val32);
                        return;
                    }
                    22 => {
                        write16(addr, &mut self.smalw, val32);
                        return;
                    }
                    23 => {
                        write16(addr, &mut self.smarw, val32);
                        return;
                    }
                    26 => {
                        self.write_mem(self.smalw, val);
                        self.smalw = (self.smalw + 1) & EMU8K_MEM_ADDRESS_MASK;
                        return;
                    }
                    29 => {
                        self.hwcf1 = val;
                        return;
                    }
                    30 => {
                        self.hwcf2 = val;
                        return;
                    }
                    31 => {
                        self.hwcf3 = val;
                        return;
                    }
                    _ => {}
                },
                2 => {
                    // snd_emu8k.c:1540. The decoded values only feed the
                    // upstream reverb engine, which REVERB_FITTED does not run.
                    self.init1[cv] = val;
                    return;
                }
                3 => {
                    // snd_emu8k.c:1598
                    self.init3[cv] = val;
                    if self.init1[0] != 0x03FF {
                        match self.cur_voice {
                            9 => self.chorus_engine.feedback = (val & 0xFF) as i32,
                            12 => self.chorus_engine.delay_samples_central = (val & 0x1FFF) as i32,
                            _ => {} // 1, 3: upstream reverb engine only
                        }
                    }
                    return;
                }
                4 => {
                    let v = &mut self.voice[cv];
                    v.envvol = val;
                    v.vol_envelope.delay_samples = if val & 0x8000 != 0 {
                        0
                    } else {
                        (0x8000 - (val as i32 & 0x7FFF)) << 5
                    };
                    return;
                }
                5 => {
                    // snd_emu8k.c:1631
                    let hwcf3 = &mut self.hwcf3;
                    let v = &mut self.voice[cv];
                    v.dcysusv = val;
                    let old_on = v.env_engine_on;
                    v.env_engine_on = (val & 0x0080 == 0) as i32;

                    if v.env_engine_on != 0 && old_on != v.env_engine_on {
                        if *hwcf3 != 0x04 {
                            *hwcf3 = 0x04;
                        }
                        v.cham_lp = 0.0;
                        v.cham_bp = 0.0;
                        v.lfo1_count = 0;
                        v.lfo2_count = 0;
                        if v.atkhldv & 0x8000 == 0 {
                            Self::trigger_vol_env(v);
                        }
                        if v.atkhld & 0x8000 == 0 {
                            Self::trigger_mod_env(&mut v.mod_envelope);
                        }
                    }

                    let vol_env = &mut v.vol_envelope;
                    let sus = ((val >> 8) & 0x7F) as i32;
                    vol_env.sustain_value_db_oct = if sus == 0 {
                        1 << 21
                    } else {
                        (0x7F - sus) << 14
                    };
                    vol_env.ramp_amount_db_oct = t.env_decay_to_dbs_or_oct[(val & 0x7F) as usize];
                    if val & 0x8000 != 0 {
                        if vol_env.state == ENV_DELAY
                            || vol_env.state == ENV_ATTACK
                            || vol_env.state == ENV_HOLD
                        {
                            vol_env.value_db_oct = t.env_vol_amplitude_to_db
                                [emu8k_attack_amp_index(vol_env.value_amp_hz) as usize]
                                << 5;
                            if vol_env.value_db_oct > (1 << 21) {
                                vol_env.value_db_oct = 1 << 21;
                            }
                        }
                        vol_env.state = if vol_env.value_db_oct >= vol_env.sustain_value_db_oct {
                            ENV_RAMP_DOWN
                        } else {
                            ENV_RAMP_UP
                        };
                    }
                    return;
                }
                6 => {
                    let v = &mut self.voice[cv];
                    v.envval = val;
                    v.mod_envelope.delay_samples = if val & 0x8000 != 0 {
                        0
                    } else {
                        (0x8000 - (val as i32 & 0x7FFF)) << 5
                    };
                    return;
                }
                7 => {
                    // snd_emu8k.c:1712
                    let v = &mut self.voice[cv];
                    v.dcysus = val;
                    let mod_env = &mut v.mod_envelope;
                    let sus = ((val >> 8) & 0x7F) as i32;
                    mod_env.sustain_value_db_oct = if sus == 0 {
                        0
                    } else {
                        (1 << 21) - ((0x7F - sus) << 14)
                    };
                    mod_env.ramp_amount_db_oct = t.env_decay_to_dbs_or_oct[(val & 0x7F) as usize];
                    if val & 0x8000 != 0 {
                        if mod_env.state == ENV_DELAY
                            || mod_env.state == ENV_ATTACK
                            || mod_env.state == ENV_HOLD
                        {
                            mod_env.value_db_oct = emu8k_mod_attack_value(mod_env.value_amp_hz);
                            if mod_env.value_db_oct >= (1 << 21) {
                                mod_env.value_db_oct = (1 << 21) - 1;
                            }
                        }
                        mod_env.state = if mod_env.value_db_oct >= mod_env.sustain_value_db_oct {
                            ENV_RAMP_DOWN
                        } else {
                            ENV_RAMP_UP
                        };
                    }
                    return;
                }
                _ => {}
            },
            0xA02 => match self.cur_reg {
                0 => {
                    let v = &mut self.voice[cv];
                    write16(addr, &mut v.ccca, val32);
                    set_mem_int(&mut v.addr, v.ccca & EMU8K_MEM_ADDRESS_MASK);
                    let paramq = (v.ccca >> 28) as usize;
                    v.filt_att = FILTER_ATTEN[paramq];
                    v.filterq_idx = paramq as i32;
                    return;
                }
                1 => match self.cur_voice {
                    9 => {
                        write16(addr, &mut self.hwcf4, val32);
                        if self.init1[0] != 0x03FF {
                            let tmp = (self.hwcf4 & 0x1FFFFF) as i32;
                            self.chorus_engine.delay_offset_samples_right = (tmp as f64) / 256.0;
                        }
                        return;
                    }
                    10 => {
                        write16(addr, &mut self.hwcf5, val32);
                        if self.init1[0] != 0x03FF {
                            self.chorus_engine.lfo_inc = (self.hwcf5 as u64) << 24;
                        }
                        return;
                    }
                    13 => {
                        write16(addr, &mut self.hwcf6, val32);
                        return;
                    }
                    14 => {
                        write16(addr, &mut self.hwcf7, val32);
                        return;
                    }
                    20 => {
                        write16(addr, &mut self.smalr, val32 & 0xFF);
                        self.dmareadbit = 0x8000;
                        return;
                    }
                    21 => {
                        write16(addr, &mut self.smarr, val32 & 0xFF);
                        self.dmareadbit = 0x8000;
                        return;
                    }
                    22 => {
                        write16(addr, &mut self.smalw, val32 & 0xFF);
                        return;
                    }
                    23 => {
                        write16(addr, &mut self.smarw, val32 & 0xFF);
                        return;
                    }
                    26 => {
                        self.dmawritebit = 0x8000;
                        self.write_mem(self.smarw, val);
                        self.smarw = self.smarw.wrapping_add(1);
                        return;
                    }
                    _ => {}
                },
                2 => {
                    // snd_emu8k.c:1810 - upstream reverb engine only.
                    self.init2[cv] = val;
                    return;
                }
                3 => {
                    // snd_emu8k.c:1885
                    self.init4[cv] = val;
                    if self.init1[0] != 0x03FF && self.cur_voice == 0x3 {
                        self.chorus_engine.lfodepth_multip = (val & 0xFF) as f64;
                    }
                    // 0x1F: upstream reverb engine only
                    return;
                }
                4 => {
                    // snd_emu8k.c:1910
                    let v = &mut self.voice[cv];
                    v.atkhldv = val;
                    let vol_env = &mut v.vol_envelope;
                    vol_env.attack_samples = t.env_attack_to_samples[(val & 0x7F) as usize];
                    if vol_env.attack_samples == 0 {
                        vol_env.attack_amount_amp_hz = 0;
                    } else {
                        vol_env.attack_amount_amp_hz = (1 << 21) / vol_env.attack_samples;
                    }
                    vol_env.hold_samples = 4096 * (0x7F - ((val as i32 >> 8) & 0x7F));
                    if val & 0x8000 == 0 && v.env_engine_on != 0 {
                        v.lfo1_count = 0;
                        v.lfo2_count = 0;
                        Self::trigger_vol_env(v);
                    }
                    return;
                }
                5 => {
                    let v = &mut self.voice[cv];
                    v.lfo1val = val;
                    v.lfo1_delay_samples = if val & 0x8000 != 0 {
                        0
                    } else {
                        (0x8000 - (val as i32 & 0x7FFF)) << 5
                    };
                    return;
                }
                6 => {
                    // snd_emu8k.c:1954
                    let v = &mut self.voice[cv];
                    v.atkhld = val;
                    let mod_env = &mut v.mod_envelope;
                    mod_env.attack_samples = t.env_attack_to_samples[(val & 0x7F) as usize];
                    if mod_env.attack_samples == 0 {
                        mod_env.attack_amount_amp_hz = 0;
                    } else {
                        mod_env.attack_amount_amp_hz = (1 << 21) / mod_env.attack_samples;
                    }
                    mod_env.hold_samples = 4096 * (0x7F - ((val as i32 >> 8) & 0x7F));
                    if val & 0x8000 == 0 && v.env_engine_on != 0 {
                        Self::trigger_mod_env(mod_env);
                    }
                    return;
                }
                7 => {
                    let v = &mut self.voice[cv];
                    v.lfo2val = val;
                    v.lfo2_delay_samples = if val & 0x8000 != 0 {
                        0
                    } else {
                        (0x8000 - (val as i32 & 0x7FFF)) << 5
                    };
                    return;
                }
                _ => {}
            },
            0xE00 => {
                let v = &mut self.voice[cv];
                match self.cur_reg {
                    0 => {
                        v.ip = val;
                        v.set_ptrx_pit_target((t.freqtable[val as usize] >> 18) as u16);
                        return;
                    }
                    1 => {
                        // snd_emu8k.c:2005
                        v.ifatn = val;
                        v.initial_att = ((v.ifatn_attenuation() as i32) << 21) / 0xFF;
                        v.set_vtft_vol_target(t.attentable[v.ifatn_attenuation() as usize] as u16);
                        v.initial_filter = ((v.ifatn_init_filter() as i32) << 21) / 0xFF;
                        if v.ifatn_init_filter() == 0xFF {
                            v.set_vtft_filter_target(0xFFFF);
                        } else {
                            v.set_vtft_filter_target((v.initial_filter >> 5) as u16);
                        }
                        return;
                    }
                    2 => {
                        v.pefe = val;
                        let h = v.pefe_modenv_filter_height() as i32;
                        let divider = if h < 0 { 0x80 } else { 0x7F };
                        v.fixed_modenv_filter_height = (h * 0x4000 / divider) as i16;
                        let h = v.pefe_modenv_pitch_height() as i32;
                        let divider = if h < 0 { 0x80 } else { 0x7F };
                        v.fixed_modenv_pitch_height = (h * 0x4000 / divider) as i16;
                        return;
                    }
                    3 => {
                        v.fmmod = val;
                        let h = v.fmmod_lfo1_filt_mod() as i32;
                        let divider = if h < 0 { 0x80 } else { 0x7F };
                        v.fixed_lfo1_filt_mod = (h * 0x4000 / divider) as i16;
                        let h = v.fmmod_lfo1_vibrato() as i32;
                        let divider = if h < 0 { 0x80 } else { 0x7F };
                        v.fixed_lfo1_vibrato = (h * 0x4000 / divider) as i16;
                        return;
                    }
                    4 => {
                        v.tremfrq = val;
                        v.lfo1_speed = t.lfofreqtospeed[(val & 0xFF) as usize];
                        let h = (val >> 8) as u8 as i8 as i32;
                        let divider = if h < 0 { 0x80 } else { 0x7F };
                        v.fixed_lfo1_tremolo = (h * 0x4000 / divider) as i16;
                        return;
                    }
                    5 => {
                        v.fm2frq2 = val;
                        v.lfo2_speed = t.lfofreqtospeed[(val & 0xFF) as usize];
                        let h = (val >> 8) as u8 as i8 as i32;
                        let divider = if h < 0 { 0x80 } else { 0x7F };
                        v.fixed_lfo2_vibrato = (h * 0x4000 / divider) as i16;
                        return;
                    }
                    7 => {
                        self.id = val;
                        return;
                    }
                    _ => {}
                }
            }
            0xE02 => {
                self.cur_voice = (val & 31) as i32;
                self.cur_reg = ((val >> 5) & 7) as i32;
                return;
            }
            _ => {}
        }
    }

    /// `emu8k_outb`, snd_emu8k.c:2105
    pub fn outb(&mut self, addr: u16, val: u8, pos_global: i32) {
        if addr & 1 != 0 {
            self.outw(addr & !1, (val as u16) << 8, pos_global);
        } else {
            self.outw(addr, val as u16, pos_global);
        }
    }

    // -----------------------------------------------------------------------
    // Effects, snd_emu8k.c:2129-2356
    // -----------------------------------------------------------------------

    /// `emu8k_work_chorus`, snd_emu8k.c:2144
    fn work_chorus(&mut self, start: usize, count: usize) {
        let engine = &mut self.chorus_engine;
        for pos in 0..count {
            let phase = (((mem_int(engine.lfo_pos) & 0xFFFF) as f64)
                + mem_fract(engine.lfo_pos) as f64 / 65536.0)
                / 65536.0;
            let tri = if phase < 0.5 {
                4.0 * phase - 1.0
            } else {
                3.0 - 4.0 * phase
            };
            let offset_lfo = tri * engine.lfodepth_multip;
            let delay_l = engine.delay_samples_central as f64 + offset_lfo;
            let delay_r = delay_l - engine.delay_offset_samples_right;

            let dat_l = chorus_tap(&engine.chorus_left_buffer, engine.write, delay_l);
            let dat_r = chorus_tap(&engine.chorus_left_buffer, engine.write, delay_r);

            engine.chorus_left_buffer[engine.write as usize] = self.chorus_in_buffer[start + pos]
                .wrapping_add(dat_l.wrapping_mul(engine.feedback) >> 8);

            engine.write += 1;
            engine.write %= EMU8K_LFOCHORUS_SIZE as i32;
            engine.lfo_pos = engine.lfo_pos.wrapping_add(engine.lfo_inc);
            let li = mem_int(engine.lfo_pos) & 0xFFFF;
            set_mem_int(&mut engine.lfo_pos, li);

            let ob = 2 * (start + pos);
            self.buffer[ob] = self.buffer[ob].wrapping_add(dat_l);
            self.buffer[ob + 1] = self.buffer[ob + 1].wrapping_add(dat_r);
        }
    }

    /// `emu8k_work_eq`, snd_emu8k.c:2294
    fn work_eq(&mut self, start: usize, count: usize) {
        let v: [u16; 10] = [
            self.init4[0x01],
            self.init4[0x11],
            self.init3[0x11],
            self.init3[0x13],
            self.init3[0x1B],
            self.init4[0x07],
            self.init4[0x0B],
            self.init4[0x0D],
            self.init4[0x17],
            self.init4[0x19],
        ];
        let mut bass = if self.eq_bass < 0 { 5 } else { self.eq_bass };
        let mut treble = if self.eq_treble < 0 {
            9
        } else {
            self.eq_treble
        };
        for i in 0..12 {
            if emu8k_eq_word_match(v[0], EMU8K_EQ_BASS_PARM[i][0])
                && emu8k_eq_word_match(v[1], EMU8K_EQ_BASS_PARM[i][1])
            {
                bass = i as i32;
                break;
            }
        }
        for i in 0..12 {
            let mut ok = true;
            let mut k = 0;
            while k < 8 && ok {
                ok = emu8k_eq_word_match(v[2 + k], EMU8K_EQ_TREBLE_PARM[i][k]);
                k += 1;
            }
            if ok {
                treble = i as i32;
                break;
            }
        }
        if bass != self.eq_bass || treble != self.eq_treble {
            self.eq_bass = bass;
            self.eq_treble = treble;
            emu8k_eq_design(&mut self.eq_coef[0], &EMU8K_EQ_BASS[bass as usize], false);
            emu8k_eq_design(
                &mut self.eq_coef[1],
                &EMU8K_EQ_TREBLE[treble as usize],
                true,
            );
        }

        let mut idx = 2 * start;
        for _n in 0..count {
            for ch in 0..2 {
                let mut x = self.buffer[idx] as f64;
                for s in 0..2 {
                    let q = &self.eq_coef[s];
                    let z = &mut self.eq_z[s][ch];
                    let y = q[0] * x + z[0];
                    z[0] = q[1] * x - q[3] * y + z[1];
                    z[1] = q[2] * x - q[4] * y;
                    x = y;
                }
                self.buffer[idx] = lrint(x) as i32;
                idx += 1;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Rendering, snd_emu8k.c:2363-2905
    // -----------------------------------------------------------------------

    /// `emu8k_update` (snd_emu8k.c:2872) without the state dump, i.e.
    /// `emu8k_update_chunk` (2364) up to `pos_global`.
    pub fn update(&mut self, pos_global: i32) {
        if self.pos >= pos_global {
            return;
        }
        let num_samples = (pos_global - self.pos) as usize;
        let t = self.t;
        let start = self.pos as usize;
        let end = pos_global as usize;
        let hwcf3_unmute = self.hwcf3 & 0x04 != 0;

        for c in 0..32 {
            let mut v = self.voice[c];

            for pos in start..end {
                let mut dat: i32;

                if v.cvcf_curr_volume() != 0 {
                    dat = self.read_interp_bspline4(mem_int(v.addr), mem_fract(v.addr));

                    // FILTER_CHAMBERLIN, snd_emu8k.c:2412
                    if v.filterq_idx != 0 || v.ifatn_init_filter() != 0xFF || v.filt_oct_curr < 0.0
                    {
                        let mut fc = EMU8K_CHAM_BASE_HZ
                            * (v.ifatn_init_filter() as i32 as f64 * EMU8K_CHAM_CENTS / 1200.0
                                + v.filt_oct_curr)
                                .exp2_pow();
                        if fc < EMU8K_CHAM_BASE_HZ {
                            fc = EMU8K_CHAM_BASE_HZ;
                        }
                        if fc > 44100.0 / 6.0 {
                            fc = 44100.0 / 6.0;
                        }
                        let f = 2.0 * (std::f64::consts::PI * fc / 44100.0).sin();
                        let qd = 1.0
                            / (EMU8K_CHAM_Q0
                                * libm_pow(10.0, emu8k_cham_q_db(v.filterq_idx, fc) / 20.0));
                        let inp = dat as f64 * v.filt_att as f64 / 65536.0;
                        v.cham_lp += f * v.cham_bp;
                        let hp = inp - v.cham_lp - qd * v.cham_bp;
                        v.cham_bp += f * hp;
                        let mut out = v.cham_lp;
                        if out > 32767.0 {
                            out = 32767.0;
                        } else if out < -32768.0 {
                            out = -32768.0;
                        }
                        dat = cvtt_f64(out);
                    }

                    if hwcf3_unmute && (v.ccca & 0x0400_0000) == 0 {
                        dat = dat.wrapping_mul(v.cvcf_curr_volume() as i32) >> 16;
                        if v.overshoot_samples >= 0 {
                            dat = ((dat as i64 * emu8k_overshoot_q12(v.overshoot_samples) as i64)
                                >> 12) as i32;
                            v.overshoot_samples += 1;
                            if v.overshoot_samples >= 8 * EMU8K_OVERSHOOT_STEP {
                                v.overshoot_samples = -1;
                            }
                        }

                        self.buffer[pos * 2] =
                            self.buffer[pos * 2].wrapping_add(dat.wrapping_mul(v.vol_l) >> 8);
                        self.buffer[pos * 2 + 1] =
                            self.buffer[pos * 2 + 1].wrapping_add(dat.wrapping_mul(v.vol_r) >> 8);

                        let rs = v.ptrx_revb_send();
                        if rs > 0 {
                            self.reverb_in_buffer[pos] = self.reverb_in_buffer[pos]
                                .wrapping_add(dat.wrapping_mul(rs as i32) >> 8);
                        }
                        let cs = v.csl_chor_send();
                        if cs > 0 {
                            self.chorus_in_buffer[pos] = self.chorus_in_buffer[pos]
                                .wrapping_add(dat.wrapping_mul(cs as i32) >> 8);
                        }
                    }
                }

                if v.env_engine_on != 0 {
                    let mut attenuation: i32 = v.initial_att;
                    let mut filtercut: i32 = v.initial_filter;
                    let mut currentpitch: i32 = v.ip as i32;

                    // volume envelope, snd_emu8k.c:2543
                    {
                        let volenv = &mut v.vol_envelope;
                        match volenv.state {
                            ENV_DELAY => {
                                volenv.delay_samples -= 1;
                                if volenv.delay_samples <= 0 {
                                    volenv.state = ENV_ATTACK;
                                    volenv.delay_samples = 0;
                                }
                                attenuation = 0x1FFFFF;
                            }
                            ENV_ATTACK => {
                                volenv.value_amp_hz = volenv
                                    .value_amp_hz
                                    .wrapping_add(volenv.attack_amount_amp_hz);
                                if volenv.value_amp_hz >= (1 << 21) {
                                    volenv.value_amp_hz = 1 << 21;
                                    v.overshoot_samples = 0;
                                    volenv.value_db_oct = 0;
                                    if volenv.hold_samples != 0 {
                                        volenv.state = ENV_HOLD;
                                    } else {
                                        volenv.state = ENV_RAMP_UP;
                                    }
                                }
                                attenuation = attenuation.wrapping_add(
                                    t.env_vol_amplitude_to_db
                                        [emu8k_attack_amp_index(volenv.value_amp_hz) as usize]
                                        << 5,
                                );
                            }
                            ENV_HOLD => {
                                volenv.hold_samples -= 1;
                                if volenv.hold_samples <= 0 {
                                    volenv.state = ENV_RAMP_UP;
                                }
                                attenuation = attenuation.wrapping_add(volenv.value_db_oct);
                            }
                            ENV_RAMP_DOWN => {
                                let st = emu8k_ramp_step(volenv);
                                volenv.value_db_oct = volenv.value_db_oct.wrapping_sub(st);
                                if volenv.value_db_oct <= volenv.sustain_value_db_oct {
                                    volenv.value_db_oct = volenv.sustain_value_db_oct;
                                    volenv.state = ENV_SUSTAIN;
                                }
                                attenuation = attenuation.wrapping_add(volenv.value_db_oct);
                            }
                            ENV_RAMP_UP => {
                                let st = emu8k_ramp_step(volenv);
                                volenv.value_db_oct = volenv.value_db_oct.wrapping_add(st);
                                if volenv.value_db_oct >= volenv.sustain_value_db_oct {
                                    volenv.value_db_oct = volenv.sustain_value_db_oct;
                                    volenv.state = ENV_SUSTAIN;
                                }
                                attenuation = attenuation.wrapping_add(volenv.value_db_oct);
                            }
                            ENV_SUSTAIN => {
                                attenuation = attenuation.wrapping_add(volenv.value_db_oct);
                            }
                            ENV_STOPPED => {
                                attenuation = 0x1FFFFF;
                            }
                            _ => {}
                        }
                    }

                    // modulation envelope, snd_emu8k.c:2612
                    {
                        let modenv = &mut v.mod_envelope;
                        match modenv.state {
                            ENV_DELAY => {
                                modenv.delay_samples -= 1;
                                if modenv.delay_samples <= 0 {
                                    modenv.state = ENV_ATTACK;
                                    modenv.delay_samples = 0;
                                }
                            }
                            ENV_ATTACK => {
                                modenv.value_amp_hz = modenv
                                    .value_amp_hz
                                    .wrapping_add(modenv.attack_amount_amp_hz);
                                modenv.value_db_oct = emu8k_mod_attack_value(modenv.value_amp_hz);
                                if modenv.value_amp_hz >= (1 << 21) {
                                    modenv.value_amp_hz = 1 << 21;
                                    modenv.value_db_oct = 1 << 21;
                                    if modenv.hold_samples != 0 {
                                        modenv.state = ENV_HOLD;
                                    } else {
                                        modenv.state = ENV_RAMP_DOWN;
                                    }
                                }
                            }
                            ENV_HOLD => {
                                modenv.hold_samples -= 1;
                                if modenv.hold_samples <= 0 {
                                    modenv.state = ENV_RAMP_DOWN;
                                }
                            }
                            ENV_RAMP_DOWN => {
                                let st = emu8k_ramp_step(modenv);
                                modenv.value_db_oct = modenv.value_db_oct.wrapping_sub(st);
                                if modenv.value_db_oct <= modenv.sustain_value_db_oct {
                                    modenv.value_db_oct = modenv.sustain_value_db_oct;
                                    modenv.state = ENV_SUSTAIN;
                                }
                            }
                            ENV_RAMP_UP => {
                                let st = emu8k_ramp_step(modenv);
                                modenv.value_db_oct = modenv.value_db_oct.wrapping_add(st);
                                if modenv.value_db_oct >= modenv.sustain_value_db_oct {
                                    modenv.value_db_oct = modenv.sustain_value_db_oct;
                                    modenv.state = ENV_SUSTAIN;
                                }
                            }
                            _ => {}
                        }
                    }

                    // LFOs, snd_emu8k.c:2667
                    if v.lfo1_delay_samples != 0 {
                        v.lfo1_delay_samples -= 1;
                    } else {
                        v.lfo1_count = v.lfo1_count.wrapping_add(v.lfo1_speed as u64);
                        let li = mem_int(v.lfo1_count) & 0xFFFF;
                        set_mem_int(&mut v.lfo1_count, li);
                    }
                    if v.lfo2_delay_samples != 0 {
                        v.lfo2_delay_samples -= 1;
                    } else {
                        v.lfo2_count = v.lfo2_count.wrapping_add(v.lfo2_speed as u64);
                        let li = mem_int(v.lfo2_count) & 0xFFFF;
                        set_mem_int(&mut v.lfo2_count, li);
                    }

                    let modenv_value = v.mod_envelope.value_db_oct;
                    let lfo1 = t.lfotable[mem_int(v.lfo1_count) as usize];
                    let lfo2 = t.lfotable[mem_int(v.lfo2_count) as usize];

                    if v.fixed_modenv_pitch_height != 0 {
                        currentpitch = currentpitch.wrapping_add(
                            (modenv_value >> 9).wrapping_mul(v.fixed_modenv_pitch_height as i32)
                                >> 14,
                        );
                    }
                    if v.fixed_lfo1_vibrato != 0 {
                        let lfo1_vibrato = lfo1.wrapping_mul(v.fixed_lfo1_vibrato as i32) >> 17;
                        currentpitch = currentpitch.wrapping_add(lfo1_vibrato);
                    }
                    if v.fixed_lfo2_vibrato != 0 {
                        let lfo2_vibrato = lfo2.wrapping_mul(v.fixed_lfo2_vibrato as i32) >> 17;
                        currentpitch = currentpitch.wrapping_add(lfo2_vibrato);
                    }
                    if v.fixed_modenv_filter_height != 0 {
                        filtercut = filtercut.wrapping_add(
                            (modenv_value >> 9).wrapping_mul(v.fixed_modenv_filter_height as i32)
                                >> 5,
                        );
                    }
                    if v.fixed_lfo1_filt_mod != 0 {
                        let lfo1_filtmod = lfo1.wrapping_mul(v.fixed_lfo1_filt_mod as i32) >> 9;
                        filtercut = filtercut.wrapping_add(lfo1_filtmod);
                    }
                    if v.fixed_lfo1_tremolo != 0 {
                        let lfo1_tremolo = lfo1.wrapping_mul(v.fixed_lfo1_tremolo as i32) >> 11;
                        if lfo1_tremolo < 0 {
                            attenuation = attenuation.wrapping_sub(lfo1_tremolo);
                        }
                    }

                    // snd_emu8k.c:2716
                    v.filt_oct_target = modenv_value as f64 / (1i32 << 21) as f64
                        * v.pefe_modenv_filter_height() as i32 as f64
                        / 127.0
                        * 6.0
                        + lfo1 as f64 / 32768.0 * v.fmmod_lfo1_filt_mod() as i32 as f64 / 127.0
                            * 3.0;

                    if currentpitch > 0xFFFF {
                        currentpitch = 0xFFFF;
                    }
                    if currentpitch < 0 {
                        currentpitch = 0;
                    }
                    if attenuation > 0x1FFFFF {
                        attenuation = 0x1FFFFF;
                    }
                    if attenuation < 0 {
                        attenuation = 0;
                    }
                    if filtercut > 0x1FFFFF {
                        filtercut = 0x1FFFFF;
                    }
                    if filtercut < 0 {
                        filtercut = 0;
                    }

                    v.set_vtft_vol_target(
                        t.env_vol_db_to_vol_target[(attenuation >> 5) as usize] as u16,
                    );
                    v.set_vtft_filter_target((filtercut >> 5) as u16);
                    v.set_ptrx_pit_target((t.freqtable[currentpitch as usize] >> 18) as u16);
                }

                // snd_emu8k.c:2751
                v.addr = v.addr.wrapping_add((v.cpf_curr_pitch() as u64) << 18);
                if v.addr >= v.loop_end {
                    let ia = mem_int(v.addr)
                        .wrapping_sub(mem_int(v.loop_end).wrapping_sub(mem_int(v.loop_start)))
                        & EMU8K_MEM_ADDRESS_MASK;
                    set_mem_int(&mut v.addr, ia);
                }

                v.set_cpf_curr_pitch(v.ptrx_pit_target());
                // emu8k_vol_slide, snd_emu8k.c:2348
                let target = v.vtft_vol_target() as i32;
                let d = target.wrapping_sub(v.volumeslide_last);
                if d > 0 {
                    v.volumeslide_last = v.volumeslide_last.wrapping_add((d + 63) >> 6);
                } else if d < 0 {
                    v.volumeslide_last = v.volumeslide_last.wrapping_add(d >> 6);
                }
                v.set_cvcf_curr_volume(v.volumeslide_last as u16);
                v.set_cvcf_curr_filt_ctoff(v.vtft_filter_target());
                v.filt_oct_curr = v.filt_oct_target;
            }

            // snd_emu8k.c:2765
            v.ccca = ((v.ccca_qcontrol() as u32) << 24) | mem_int(v.addr);
            v.set_cpf_curr_frac_addr(mem_fract(v.addr));
            self.voice[c] = v;
        }

        // effects, snd_emu8k.c:2783
        self.work_reverb_fitted(start, num_samples);
        self.work_chorus(start, num_samples);
        self.work_eq(start, num_samples);
        for i in 2 * start..2 * end {
            self.buffer[i] = ((self.buffer[i] as i64 * 57737) >> 16) as i32;
        }

        self.wc = self.wc.wrapping_add(num_samples as u16);
        self.pos = pos_global;
    }

    /// `emu8k_reset_buffer`, snd_emu8k.c:2899
    pub fn reset_buffer(&mut self) {
        self.pos = 0;
        self.buffer.fill(0);
        self.chorus_in_buffer.fill(0);
        self.reverb_in_buffer.fill(0);
    }
}

/// `emu8k_chorus_tap`, snd_emu8k.c:2130
#[inline]
fn chorus_tap(line: &[i32], write: i32, delay: f64) -> i32 {
    let readdouble = write as f64 - delay;
    let mut read = cvtt_f64(readdouble.floor());
    let fraction_part = cvtt_f64((readdouble - read as f64) * 65536.0);
    read %= EMU8K_LFOCHORUS_SIZE as i32;
    if read < 0 {
        read += EMU8K_LFOCHORUS_SIZE as i32;
    }
    let next_value = (read + 1) % EMU8K_LFOCHORUS_SIZE as i32;
    let a = line[read as usize];
    let b = line[next_value as usize];
    a.wrapping_add(b.wrapping_sub(a).wrapping_mul(fraction_part) >> 16)
}

/// C `lrint` in the default rounding mode, then the implicit `long` ->
/// `int32_t` conversion (truncation).
#[inline(always)]
fn lrint(x: f64) -> i64 {
    let r = x.round_ties_even();
    if r >= -9.223372036854775808e18 && r < 9.223372036854775808e18 {
        r as i64
    } else {
        i64::MIN
    }
}

/// glibc `pow`, called as such. See the crate docs on why the chip's
/// `pow(2.0, x)` / `pow(10.0, x)` must not be rewritten by LLVM into
/// `exp2` / `exp10`: the libm results may differ in the last bit.
#[inline(always)]
pub(crate) fn libm_pow(x: f64, y: f64) -> f64 {
    crate::libm::pow(x, y)
}

trait Exp2Pow {
    fn exp2_pow(self) -> f64;
}
impl Exp2Pow for f64 {
    /// `pow(2.0, self)` exactly as the C code calls it.
    #[inline(always)]
    fn exp2_pow(self) -> f64 {
        libm_pow(2.0, self)
    }
}
