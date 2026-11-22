//! SoundFont bank loader - SoundFont 1.0 (`.SBK`), SF2 and the GM tables
//! compiled into `SBAWE32.MDI` (`SoundFont.cpp` / `SoundFont.h`), plus the
//! conversion of one layer to EMU8000 registers (`MakeVoiceParams`).

use crate::driver::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// SoundFont.h:30 generator numbers.
#[allow(non_upper_case_globals)]
pub mod generator {
    pub const StartAddrsOffset: usize = 0;
    pub const EndAddrsOffset: usize = 1;
    pub const StartloopAddrsOffset: usize = 2;
    pub const EndloopAddrsOffset: usize = 3;
    pub const StartAddrsCoarseOffset: usize = 4;
    pub const ModLfoToPitch: usize = 5;
    pub const VibLfoToPitch: usize = 6;
    pub const ModEnvToPitch: usize = 7;
    pub const InitialFilterFc: usize = 8;
    pub const InitialFilterQ: usize = 9;
    pub const ModLfoToFilterFc: usize = 10;
    pub const ModEnvToFilterFc: usize = 11;
    pub const EndAddrsCoarseOffset: usize = 12;
    pub const ModLfoToVolume: usize = 13;
    pub const ChorusEffectsSend: usize = 15;
    pub const ReverbEffectsSend: usize = 16;
    pub const Pan: usize = 17;
    pub const DelayModLFO: usize = 21;
    pub const FreqModLFO: usize = 22;
    pub const DelayVibLFO: usize = 23;
    pub const FreqVibLFO: usize = 24;
    pub const DelayModEnv: usize = 25;
    pub const AttackModEnv: usize = 26;
    pub const HoldModEnv: usize = 27;
    pub const DecayModEnv: usize = 28;
    pub const SustainModEnv: usize = 29;
    pub const ReleaseModEnv: usize = 30;
    pub const KeynumToModEnvHold: usize = 31;
    pub const KeynumToModEnvDecay: usize = 32;
    pub const DelayVolEnv: usize = 33;
    pub const AttackVolEnv: usize = 34;
    pub const HoldVolEnv: usize = 35;
    pub const DecayVolEnv: usize = 36;
    pub const SustainVolEnv: usize = 37;
    pub const ReleaseVolEnv: usize = 38;
    pub const KeynumToVolEnvHold: usize = 39;
    pub const KeynumToVolEnvDecay: usize = 40;
    pub const Instrument: usize = 41;
    pub const KeyRange: usize = 43;
    pub const VelRange: usize = 44;
    pub const StartloopAddrsCoarseOffset: usize = 45;
    pub const Keynum: usize = 46;
    pub const Velocity: usize = 47;
    pub const InitialAttenuation: usize = 48;
    pub const EndloopAddrsCoarseOffset: usize = 50;
    pub const CoarseTune: usize = 51;
    pub const FineTune: usize = 52;
    pub const SampleID: usize = 53;
    pub const SampleModes: usize = 54;
    pub const Sf1RootPitchCents: usize = 55;
    pub const ScaleTuning: usize = 56;
    pub const ExclusiveClass: usize = 57;
    pub const OverridingRootKey: usize = 58;
    pub const Count: usize = 60;
}

/// SoundFont.h:76 `GenSet`
#[derive(Clone, Copy)]
pub struct GenSet {
    pub value: [i16; generator::Count],
    pub present: [bool; generator::Count],
}

impl Default for GenSet {
    fn default() -> Self {
        GenSet {
            value: [0; generator::Count],
            present: [false; generator::Count],
        }
    }
}

impl GenSet {
    pub fn set(&mut self, op: i32, v: i16) {
        if op >= 0 && (op as usize) < generator::Count {
            self.value[op as usize] = v;
            self.present[op as usize] = true;
        }
    }
    pub fn has(&self, op: usize) -> bool {
        op < generator::Count && self.present[op]
    }
    pub fn get(&self, op: usize, fallback: i32) -> i32 {
        if self.has(op) {
            self.value[op] as i32
        } else {
            fallback
        }
    }
    /// SoundFont.cpp:235 `FillFrom`
    pub fn fill_from(&mut self, other: &GenSet) {
        for i in 0..generator::Count {
            if other.present[i] && !self.present[i] {
                self.value[i] = other.value[i];
                self.present[i] = true;
            }
        }
    }
    /// SoundFont.cpp:242 `OverrideFrom`
    pub fn override_from(&mut self, other: &GenSet) {
        for i in 0..generator::Count {
            if other.present[i] {
                self.value[i] = other.value[i];
                self.present[i] = true;
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Version {
    Sf1,
    Sf2,
}

/// SoundFont.h:97
#[derive(Clone, Debug)]
pub struct Sample {
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub sample_rate: u32,
    pub original_key: u8,
    pub correction: i8,
    pub in_rom: bool,
}

/// SoundFont.h:107
#[derive(Clone)]
pub struct Zone {
    pub gen_set: GenSet,
    pub key_lo: i32,
    pub key_hi: i32,
    pub vel_lo: i32,
    pub vel_hi: i32,
    pub sample_id: i32,
    pub instrument: i32,
}

impl Default for Zone {
    fn default() -> Self {
        Zone {
            gen_set: GenSet::default(),
            key_lo: 0,
            key_hi: 127,
            vel_lo: 0,
            vel_hi: 127,
            sample_id: -1,
            instrument: -1,
        }
    }
}

#[derive(Clone, Default)]
pub struct Instrument {
    pub name: String,
    pub global: GenSet,
    pub zones: Vec<Zone>,
}

#[derive(Clone, Default)]
pub struct Preset {
    pub name: String,
    pub bank: i32,
    pub program: i32,
    pub global: GenSet,
    pub zones: Vec<Zone>,
}

/// SoundFont.h:131 `Region` (the sample is an index into `Bank::samples`).
#[derive(Clone, Copy)]
pub struct Region {
    pub sample: usize,
    pub gen_set: GenSet,
    pub sf1_atten_units: i32,
}

/// SoundFont.h:142 `Bank`
#[derive(Clone)]
pub struct Bank {
    pub version: Version,
    pub name: String,
    pub rom_name: String,
    pub samples_in_rom: bool,
    pub sample_data: Vec<i16>,
    pub samples: Vec<Sample>,
    pub instruments: Vec<Instrument>,
    pub presets: Vec<Preset>,
}

fn rd_u16(b: &[u8], p: usize) -> u16 {
    b[p] as u16 | ((b[p + 1] as u16) << 8)
}
fn rd_u32(b: &[u8], p: usize) -> u32 {
    b[p] as u32 | ((b[p + 1] as u32) << 8) | ((b[p + 2] as u32) << 16) | ((b[p + 3] as u32) << 24)
}

/// SoundFont.cpp:26 `CStr`
fn cstr(p: &[u8], max_len: usize) -> String {
    let mut n = 0;
    while n < max_len && n < p.len() && p[n] != 0 {
        n += 1;
    }
    let mut s: String = p[..n].iter().map(|&c| c as char).collect();
    while s.ends_with(' ') {
        s.pop();
    }
    s
}

#[derive(Clone, Copy)]
struct Chunk {
    offset: usize,
    size: u32,
}

/// SoundFont.cpp:38 `WalkChunks`
fn walk_chunks(buf: &[u8], mut pos: usize, end: usize, out: &mut HashMap<[u8; 4], Chunk>) {
    while pos + 8 <= end {
        let id: [u8; 4] = [buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]];
        let size = rd_u32(buf, pos + 4);
        let data = pos + 8;
        if data > end {
            break;
        }
        if &id == b"LIST" && data + 4 <= end {
            walk_chunks(buf, data + 4, (data + size as usize).min(end), out);
        } else {
            out.entry(id).or_insert(Chunk { offset: data, size });
        }
        pos = data + size as usize + (size & 1) as usize;
    }
}

// ---------------------------------------------------------------------------
// unit conversions, SoundFont.cpp:60-185
// ---------------------------------------------------------------------------

/// SoundFont.cpp:65
fn rate_divisor(index: i32) -> i32 {
    let group = (index >> 4) & 7;
    let m = index & 15;
    if group == 0 {
        m + 1
    } else {
        (m + 17) << (group - 1)
    }
}

/// C `std::lround` (half away from zero).
fn lround(x: f64) -> i64 {
    x.round() as i64
}

/// SoundFont.cpp:89
fn attack_rate_from_ms(ms: f64) -> i32 {
    if ms <= 0.0 {
        return 0x7F;
    }
    for r in 1..=0x7F {
        if ms > lround(11878.0 / rate_divisor(r - 1) as f64) as f64 {
            return r;
        }
    }
    0x7E
}
const ATTACK_DEFAULT_RATE: i32 = 0x7D;

/// SoundFont.cpp:109
fn decay_rate_from_ms(ms: f64) -> i32 {
    if ms <= 0.0 {
        return 0x7F;
    }
    for r in 1..=0x7F {
        if ms > 47513.0 / rate_divisor(r - 1) as f64 {
            return (r - 1).max(0);
        }
    }
    0x7F
}

/// SoundFont.cpp:116
fn hold_from_ms(ms: f64) -> i32 {
    let steps = (ms / (HOLD_SEC_PER_STEP * 1000.0)) as i32;
    (127 - steps).clamp(0, 127)
}

/// SoundFont.cpp:142
fn delay_from_timecents(timecents: i32) -> i32 {
    if timecents <= -12000 {
        return DELAY_NONE as i32;
    }
    if timecents >= 5484 {
        return 0;
    }
    let q = (((timecents + 12516) as i64) << 16) / 1200;
    let q = q as i32;
    let value = (0x10000 + (q & 0xFFFF)) >> (16 - (q >> 16));
    let reg = DELAY_NONE as i32 - value;
    if reg >= 0 { reg } else { 0 }
}

/// SoundFont.cpp:175
fn delay_from_ms(ms: f64) -> i32 {
    let steps = (ms * 1000.0 / 725.0) as i32;
    (DELAY_NONE as i32 - steps).clamp(0, 0x8000)
}

/// SoundFont.cpp:197
fn pitch_from_cents(cents: f64) -> i32 {
    let mut v = lround(cents) as i32 + 0x41A0;
    if v < 0 {
        v = 0;
    }
    let mut oct = v / 1200;
    if oct > 15 {
        oct = 15;
    }
    let rem = v % 1200;
    (oct << 12) | (rem * 3 + (rem * 31) / 75)
}

/// SoundFont.cpp:208
fn timecents_to_ms(tc: i32) -> f64 {
    crate::libm::pow(2.0, tc as f64 / 1200.0) * 1000.0
}

/// SoundFont.cpp:211
fn filter_fc_from_abs_cents(cents: i32) -> i32 {
    (lround((cents as f64 - CUTOFF_BASE_CENTS) / CUTOFF_CENTS_STEP) as i32).clamp(0, 255)
}

fn clamp_s8(v: i32) -> i8 {
    v.clamp(-128, 127) as i8
}
fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

// ---------------------------------------------------------------------------
// GM bank compiled into SBAWE32.MDI, SoundFont.cpp:252-475
// ---------------------------------------------------------------------------

fn rd16(d: &[u8], p: usize) -> u16 {
    if p + 1 < d.len() {
        d[p] as u16 | ((d[p + 1] as u16) << 8)
    } else {
        0
    }
}
fn wr16(o: &mut Vec<u8>, v: u16) {
    o.push((v & 0xFF) as u8);
    o.push((v >> 8) as u8);
}
fn wr32(o: &mut Vec<u8>, v: u32) {
    wr16(o, (v & 0xFFFF) as u16);
    wr16(o, (v >> 16) as u16);
}
fn wr_chunk(o: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    o.extend_from_slice(id);
    wr32(o, body.len() as u32);
    o.extend_from_slice(body);
    if body.len() & 1 == 1 {
        o.push(0);
    }
}
fn wr_list(o: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]) {
    let mut b = ty.to_vec();
    b.extend_from_slice(body);
    wr_chunk(o, b"LIST", &b);
}

/// Which conversion a `Preimage` inverts (the C++ keys its cache by the
/// lambda's type and `lo`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Conv {
    FilterFc,
    S8x2,
    Freq,
    Q,
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
}

fn conv_apply(c: Conv, x: i32) -> i32 {
    match c {
        Conv::FilterFc => (x * 2).clamp(0, 255),
        Conv::S8x2 => clamp_s8(x * 2) as i32,
        Conv::Freq => (x * 2) & 0xFF,
        Conv::Q => x >> 3,
        Conv::Delay => delay_from_ms(x as f64),
        Conv::Attack => attack_rate_from_ms(x as f64),
        Conv::Hold => hold_from_ms(x as f64),
        Conv::Decay => decay_rate_from_ms(x as f64),
        Conv::Sustain => (x * 4 / 3).clamp(0, 0x7F),
    }
}

/// SoundFont.cpp:299 `Preimage`: smallest v in [lo, hi] with f(v) == target.
fn preimage(target: i32, lo: i32, hi: i32, c: Conv) -> Option<i32> {
    static CACHE: OnceLock<Mutex<HashMap<Conv, HashMap<i32, i32>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    let inv = cache.entry(c).or_insert_with(|| {
        let mut inv = HashMap::new();
        let mut v = hi;
        while v >= lo {
            inv.insert(conv_apply(c, v), v);
            v -= 1;
        }
        inv
    });
    inv.get(&target).copied()
}

/// SoundFont.cpp:317 `MdiToSf1`. Returns (value, unmapped 0/1).
fn mdi_to_sf1(op: i32, raw: u16) -> (i16, i32) {
    let s = raw as i16 as i32;
    let r = match op as usize {
        generator::InitialFilterFc => preimage(raw as i32, 0, 127, Conv::FilterFc),
        generator::ModLfoToFilterFc | generator::ModEnvToFilterFc | generator::ModLfoToVolume => {
            preimage(s, -128, 127, Conv::S8x2)
        }
        generator::FreqModLFO | generator::FreqVibLFO => {
            preimage((raw & 0xFF) as i32, 0, 255, Conv::Freq)
        }
        generator::InitialFilterQ => preimage(raw as i32, 0, 127, Conv::Q),
        generator::DelayModLFO
        | generator::DelayVibLFO
        | generator::DelayModEnv
        | generator::DelayVolEnv => preimage(raw as i32, 0, 65535, Conv::Delay),
        generator::AttackModEnv | generator::AttackVolEnv => {
            preimage(raw as i32, 0, 65535, Conv::Attack)
        }
        generator::HoldModEnv => preimage(raw as i32, 0, 65535, Conv::Hold),
        generator::DecayModEnv | generator::ReleaseModEnv | generator::ReleaseVolEnv => {
            preimage(raw as i32, 0, 65535, Conv::Decay)
        }
        generator::SustainModEnv | generator::SustainVolEnv => {
            preimage(raw as i32, 0, 255, Conv::Sustain)
        }
        _ => return (s as i16, 0),
    };
    match r {
        Some(v) => (v as i16, 0),
        // the C++ leaves `v` at its initial value `s` when no preimage exists
        None => (s as i16, 1),
    }
}

/// `BuildSbkFromMdi` (SoundFont.cpp:350): finds the GM preset/instrument/
/// sample tables of `SBAWE32.MDI` by their shape and rebuilds an SF1 RIFF
/// image from them. Works on any bytes that embed those tables (the MDI
/// driver, or CARPET.EXE / HIDDEN.EXE of Magic Carpet 1). Returns the RIFF
/// image and the number of values without an exact SF1 counterpart.
pub fn build_sbk_from_mdi(d: &[u8]) -> Result<(Vec<u8>, i32), String> {
    build_sbk_from_mdi_aligned(d, true, true, false)
}

/// The GM-table scan for bytes that are not `SBAWE32.MDI` itself (MC1's
/// CARPET.EXE / HIDDEN.EXE). The tables there are byte-identical to the
/// MDI's except for alignment: the C++ pads the end of `pgen` / `igen` to
/// an even FILE offset, which is right for the MDI but not for a copy
/// placed at another parity inside an EXE. The four padding choices are
/// tried in order (the MDI's own first) and the first whose tables pass a
/// consistency check wins. Not part of AWE32Emu; on the MDI itself it gives
/// the same image as [`build_sbk_from_mdi`].
pub fn build_sbk_from_mdi_tables(d: &[u8]) -> Result<(Vec<u8>, i32), String> {
    let mut first_err = None;
    for (pp, pi) in [(true, true), (false, true), (true, false), (false, false)] {
        match build_sbk_from_mdi_aligned(d, pp, pi, true) {
            Ok(r) => return Ok(r),
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    Err(first_err.unwrap())
}

fn build_sbk_from_mdi_aligned(
    d: &[u8],
    pad_pgen: bool,
    pad_igen: bool,
    check: bool,
) -> Result<(Vec<u8>, i32), String> {
    let mut unmapped = 0;
    let mut ptab = 0usize;
    let mut pend = 0usize;
    let mut t = 4usize;
    while t + 4 <= d.len() && pend == 0 {
        if rd16(d, t) == 0xFF80 {
            let mut p = t;
            let mut n = 0;
            while p >= 4
                && ((rd16(d, p - 4) >> 8) == 0 || (rd16(d, p - 4) >> 8) == 0x80)
                && (rd16(d, p - 4) & 0xFF) < 128
                && rd16(d, p - 2) < rd16(d, p + 2)
            {
                p -= 4;
                n += 1;
            }
            if n > 50 {
                ptab = p;
                pend = t;
            }
        }
        t += 1;
    }
    if pend == 0 {
        return Err("SBAWE32.MDI: preset table not found".into());
    }

    let n_presets = (pend - ptab) / 4;
    let n_pbag = rd16(d, pend + 2);
    let mut pos = pend + 4;
    let mut pbag = vec![0u16; n_pbag as usize + 1];
    for x in pbag.iter_mut() {
        *x = rd16(d, pos);
        pos += 2;
    }
    let pgen_at = pos;
    let n_pgen = *pbag.last().unwrap() as usize;
    pos = pgen_at + 3 * n_pgen;
    if pad_pgen {
        pos += pos & 1;
    }
    let mut inst: Vec<u16> = vec![rd16(d, pos)];
    pos += 2;
    while pos + 2 <= d.len() && rd16(d, pos) >= *inst.last().unwrap() {
        inst.push(rd16(d, pos));
        pos += 2;
    }
    let mut ibag = vec![0u16; *inst.last().unwrap() as usize + 1];
    for x in ibag.iter_mut() {
        *x = rd16(d, pos);
        pos += 2;
    }
    let igen_at = pos;
    let n_igen = *ibag.last().unwrap() as usize;
    let mut shdr_at = igen_at + 3 * n_igen;
    if pad_igen {
        shdr_at += shdr_at & 1;
    }
    if shdr_at > d.len() {
        return Err("SBAWE32.MDI: tables exceed the file".into());
    }
    if check {
        // generators of both levels must be known ops, every preset zone
        // must name an instrument, every instrument zone a sample
        let n_inst = inst.len() - 1;
        for i in 0..n_pgen {
            let op = d[pgen_at + 3 * i] as usize;
            if op >= generator::Count {
                return Err("MDI tables: bad pgen".into());
            }
            if op == generator::Instrument && rd16(d, pgen_at + 3 * i + 1) as usize >= n_inst {
                return Err("MDI tables: bad instrument index".into());
            }
        }
        for i in 0..n_igen {
            let op = d[igen_at + 3 * i] as usize;
            if op >= generator::Count {
                return Err("MDI tables: bad igen".into());
            }
        }
        if ibag.windows(2).any(|w| w[1] < w[0]) || n_inst == 0 {
            return Err("MDI tables: bad ibag".into());
        }
    }

    let mut gens = |at: usize, count: usize, max_sample: &mut i32| -> Vec<u8> {
        let mut o = Vec::new();
        for i in 0..count {
            let op = d[at + 3 * i] as i32;
            let raw = rd16(d, at + 3 * i + 1);
            let mut v = raw as i16;
            if op != generator::KeyRange as i32 && op != generator::VelRange as i32 {
                let (nv, u) = mdi_to_sf1(op, raw);
                v = nv;
                unmapped += u;
            }
            if op == generator::SampleID as i32 {
                *max_sample = (*max_sample).max(raw as i32);
            }
            wr16(&mut o, op as u16);
            wr16(&mut o, v as u16);
        }
        wr32(&mut o, 0);
        o
    };
    let mut max_sample = -1;
    let mut dummy = -1;
    let pgen_b = gens(pgen_at, n_pgen, &mut dummy);
    let igen_b = gens(igen_at, n_igen, &mut max_sample);

    let n_samples = (max_sample + 1) as usize;
    if shdr_at + 16 * n_samples > d.len() {
        return Err("SBAWE32.MDI: sample table truncated".into());
    }
    if check {
        // SF1 sample headers: start <= loop start <= loop end <= end
        for i in 0..n_samples {
            let p = shdr_at + 16 * i;
            let (st, en, ls, le) = (
                rd_u32(d, p),
                rd_u32(d, p + 4),
                rd_u32(d, p + 8),
                rd_u32(d, p + 12),
            );
            if !(st <= ls && ls <= le && le <= en) {
                return Err("MDI tables: bad sample header".into());
            }
        }
    }

    let name20 = |o: &mut Vec<u8>, s: &str| {
        let b = s.as_bytes();
        for i in 0..20 {
            o.push(if i < b.len() { b[i] } else { 0 });
        }
    };
    let (mut phdr_b, mut pbag_b, mut inst_b, mut ibag_b, mut snam_b) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let pmod_b = vec![0u8; 10];
    let imod_b = vec![0u8; 10];
    for k in 0..=n_presets {
        let id = if k < n_presets {
            rd16(d, ptab + 4 * k)
        } else {
            0
        };
        let bag = if k < n_presets {
            rd16(d, ptab + 4 * k + 2)
        } else {
            n_pbag
        };
        let nm = if k < n_presets {
            format!("MDI {}:{}", id >> 8, id & 0xFF)
        } else {
            "EOP".to_string()
        };
        name20(&mut phdr_b, &nm);
        wr16(&mut phdr_b, id & 0xFF);
        wr16(&mut phdr_b, id >> 8);
        wr16(&mut phdr_b, bag);
        wr32(&mut phdr_b, 0);
        wr32(&mut phdr_b, 0);
        wr32(&mut phdr_b, 0);
    }
    for &x in &pbag {
        wr16(&mut pbag_b, x);
        wr16(&mut pbag_b, 0);
    }
    for i in 0..inst.len() {
        let nm = if i + 1 < inst.len() {
            format!("MDI inst {}", i)
        } else {
            "EOI".to_string()
        };
        name20(&mut inst_b, &nm);
        wr16(&mut inst_b, inst[i]);
    }
    for &x in &ibag {
        wr16(&mut ibag_b, x);
        wr16(&mut ibag_b, 0);
    }
    let shdr_b = d[shdr_at..shdr_at + 16 * n_samples].to_vec();
    for i in 0..n_samples {
        name20(&mut snam_b, &format!("ROM {}", i));
    }

    let (mut info, mut ifil, mut pdta) = (Vec::new(), Vec::new(), Vec::new());
    wr16(&mut ifil, 1);
    wr16(&mut ifil, 0);
    wr_chunk(&mut info, b"ifil", &ifil);
    let mut nm = b"SBAWE32.MDI GM".to_vec();
    nm.push(0);
    wr_chunk(&mut info, b"INAM", &nm);
    let mut sdta_body = Vec::new();
    wr_chunk(&mut sdta_body, b"snam", &snam_b);
    wr_chunk(&mut pdta, b"phdr", &phdr_b);
    wr_chunk(&mut pdta, b"pbag", &pbag_b);
    wr_chunk(&mut pdta, b"pmod", &pmod_b);
    wr_chunk(&mut pdta, b"pgen", &pgen_b);
    wr_chunk(&mut pdta, b"inst", &inst_b);
    wr_chunk(&mut pdta, b"ibag", &ibag_b);
    wr_chunk(&mut pdta, b"imod", &imod_b);
    wr_chunk(&mut pdta, b"igen", &igen_b);
    wr_chunk(&mut pdta, b"shdr", &shdr_b);

    let mut body = b"sfbk".to_vec();
    wr_list(&mut body, b"INFO", &info);
    wr_list(&mut body, b"sdta", &sdta_body);
    wr_list(&mut body, b"pdta", &pdta);
    let mut riff = b"RIFF".to_vec();
    wr32(&mut riff, body.len() as u32);
    riff.extend_from_slice(&body);
    Ok((riff, unmapped))
}

impl Bank {
    /// `SoundFont::Load` (SoundFont.cpp:477) on bytes: an `AIL3MDI` file
    /// goes through [`build_sbk_from_mdi`], anything else must be RIFF
    /// (SF1 `.SBK` or SF2).
    pub fn load(bytes: &[u8]) -> Result<Bank, String> {
        if bytes.len() >= 7 && &bytes[..7] == b"AIL3MDI" {
            return Self::load_mdi_tables(bytes);
        }
        Self::load_riff(bytes)
    }

    /// The MDI path of `Load` without the `AIL3MDI` magic check: scans any
    /// bytes (SBAWE32.MDI, CARPET.EXE, HIDDEN.EXE) for the embedded GM tables.
    pub fn load_mdi_tables(bytes: &[u8]) -> Result<Bank, String> {
        let (riff, _unmapped) = build_sbk_from_mdi_tables(bytes)?;
        Self::load_riff(&riff)
    }

    /// The RIFF part of `Load`, SoundFont.cpp:494.
    pub fn load_riff(buf: &[u8]) -> Result<Bank, String> {
        if buf.len() < 12 || &buf[..4] != b"RIFF" {
            return Err("Missing RIFF header".into());
        }
        let mut c: HashMap<[u8; 4], Chunk> = HashMap::new();
        let riff_end = buf.len().min(8 + rd_u32(buf, 4) as usize);
        walk_chunks(buf, 12, riff_end, &mut c);
        let need = |id: &[u8; 4]| c.get(id).copied();

        let ifil = need(b"ifil")
            .filter(|i| i.size >= 2)
            .ok_or("Missing chunk ifil")?;
        let major = rd_u16(buf, ifil.offset);
        let mut bank = Bank {
            version: if major < 2 {
                Version::Sf1
            } else {
                Version::Sf2
            },
            name: String::new(),
            rom_name: String::new(),
            samples_in_rom: false,
            sample_data: Vec::new(),
            samples: Vec::new(),
            instruments: Vec::new(),
            presets: Vec::new(),
        };
        if let Some(n) = need(b"INAM") {
            bank.name = cstr(&buf[n.offset..], n.size as usize);
        }
        if let Some(r) = need(b"irom") {
            bank.rom_name = cstr(&buf[r.offset..], r.size as usize);
        }
        if let Some(smpl) = need(b"smpl") {
            let n = smpl.size as usize / 2;
            bank.sample_data = (0..n)
                .map(|i| rd_u16(buf, smpl.offset + 2 * i) as i16)
                .collect();
        }

        let shdr = need(b"shdr").ok_or("Missing chunk shdr")?;
        if bank.version == Version::Sf1 {
            let snam = need(b"snam");
            let count = shdr.size / 16;
            let mut first_own = count;
            for i in 0..count {
                if rd_u32(buf, shdr.offset + i as usize * 16) == 0 {
                    first_own = i;
                    break;
                }
            }
            let by_position = first_own < count;
            for i in 0..count {
                let p = shdr.offset + i as usize * 16;
                let mut name = String::new();
                if let Some(sn) = snam {
                    if (i + 1) * 20 <= sn.size {
                        name = cstr(&buf[sn.offset + i as usize * 20..], 20);
                    }
                }
                let in_rom = bank.sample_data.is_empty()
                    || if by_position {
                        i < first_own
                    } else {
                        name.starts_with('*')
                    };
                bank.samples.push(Sample {
                    start: rd_u32(buf, p),
                    end: rd_u32(buf, p + 4),
                    loop_start: rd_u32(buf, p + 8),
                    loop_end: rd_u32(buf, p + 12),
                    name,
                    sample_rate: 44100,
                    original_key: 60,
                    correction: 0,
                    in_rom,
                });
            }
        } else {
            let count = shdr.size / 46;
            for i in 0..count {
                let p = shdr.offset + i as usize * 46;
                let name = cstr(&buf[p..], 20);
                let ty = rd_u16(buf, p + 44);
                let mut sr = rd_u32(buf, p + 36);
                if sr == 0 {
                    sr = 44100;
                }
                let in_rom = (ty & 0x8000) != 0 || name.starts_with('*');
                bank.samples.push(Sample {
                    start: rd_u32(buf, p + 20),
                    end: rd_u32(buf, p + 24),
                    loop_start: rd_u32(buf, p + 28),
                    loop_end: rd_u32(buf, p + 32),
                    sample_rate: sr,
                    original_key: buf[p + 40],
                    correction: buf[p + 41] as i8,
                    in_rom,
                    name,
                });
            }
            if bank.samples.last().is_some_and(|s| s.name == "EOS") {
                bank.samples.pop();
            }
        }

        let (Some(pbag), Some(pgen), Some(inst), Some(ibag), Some(igen), Some(phdr)) = (
            need(b"pbag"),
            need(b"pgen"),
            need(b"inst"),
            need(b"ibag"),
            need(b"igen"),
            need(b"phdr"),
        ) else {
            return Err("One of the chunks phdr/pbag/pgen/inst/ibag/igen is missing".into());
        };

        let gen_at = |ch: Chunk, i: u32| -> (i32, i16) {
            let p = ch.offset + i as usize * 4;
            (rd_u16(buf, p) as i32, rd_u16(buf, p + 2) as i16)
        };
        let bag_gen_index =
            |ch: Chunk, i: u32| -> u32 { rd_u16(buf, ch.offset + i as usize * 4) as u32 };

        // SoundFont.cpp:645 readZones
        let read_zones = |bag_ch: Chunk,
                          gen_ch: Chunk,
                          bag_first: u32,
                          bag_last: u32,
                          terminator: usize,
                          global: &mut GenSet,
                          zones: &mut Vec<Zone>| {
            let n_gen = gen_ch.size / 4;
            for b in bag_first..bag_last {
                let g0 = bag_gen_index(bag_ch, b);
                let g1 = bag_gen_index(bag_ch, b + 1);
                let mut z = Zone::default();
                let mut has_terminator = false;
                let mut g = g0;
                while g < g1 && g < n_gen {
                    let (op, val) = gen_at(gen_ch, g);
                    if op == generator::KeyRange as i32 {
                        let raw = val as u16;
                        z.key_lo = (raw & 0xFF) as i32;
                        z.key_hi = ((raw >> 8) & 0xFF) as i32;
                    } else if op == generator::VelRange as i32 {
                        let raw = val as u16;
                        z.vel_lo = (raw & 0xFF) as i32;
                        z.vel_hi = ((raw >> 8) & 0xFF) as i32;
                    } else if op == terminator as i32 {
                        has_terminator = true;
                        if terminator == generator::SampleID {
                            z.sample_id = val as u16 as i32;
                        } else {
                            z.instrument = val as u16 as i32;
                        }
                    }
                    z.gen_set.set(op, val);
                    g += 1;
                }
                if !has_terminator {
                    if b == bag_first {
                        *global = z.gen_set;
                    }
                    continue;
                }
                zones.push(z);
            }
        };

        let n_inst = inst.size / 22;
        let mut i = 0;
        while i + 1 < n_inst {
            let p = inst.offset + i as usize * 22;
            let mut ins = Instrument {
                name: cstr(&buf[p..], 20),
                ..Default::default()
            };
            let b0 = rd_u16(buf, p + 20) as u32;
            let b1 = rd_u16(buf, p + 22 + 20) as u32;
            read_zones(
                ibag,
                igen,
                b0,
                b1,
                generator::SampleID,
                &mut ins.global,
                &mut ins.zones,
            );
            bank.instruments.push(ins);
            i += 1;
        }

        let n_preset = phdr.size / 38;
        let mut i = 0;
        while i + 1 < n_preset {
            let p = phdr.offset + i as usize * 38;
            let mut pr = Preset {
                name: cstr(&buf[p..], 20),
                program: rd_u16(buf, p + 20) as i32,
                bank: rd_u16(buf, p + 22) as i32,
                ..Default::default()
            };
            let b0 = rd_u16(buf, p + 24) as u32;
            let b1 = rd_u16(buf, p + 38 + 24) as u32;
            read_zones(
                pbag,
                pgen,
                b0,
                b1,
                generator::Instrument,
                &mut pr.global,
                &mut pr.zones,
            );
            bank.presets.push(pr);
            i += 1;
        }
        Ok(bank)
    }

    /// SoundFont.cpp:724 `FindPreset`
    pub fn find_preset(&self, bank_num: i32, program: i32) -> Option<&Preset> {
        self.presets
            .iter()
            .find(|p| p.bank == bank_num && p.program == program)
    }

    /// SoundFont.cpp:731 `Select`
    pub fn select(&self, bank_num: i32, program: i32, key: i32, velocity: i32) -> Vec<Region> {
        let mut out = Vec::new();
        let Some(preset) = self.find_preset(bank_num, program) else {
            return out;
        };
        for pz in &preset.zones {
            if key < pz.key_lo || key > pz.key_hi {
                continue;
            }
            if velocity < pz.vel_lo || velocity > pz.vel_hi {
                continue;
            }
            if pz.instrument < 0 || pz.instrument >= self.instruments.len() as i32 {
                continue;
            }
            let ins = &self.instruments[pz.instrument as usize];
            for iz in &ins.zones {
                if key < iz.key_lo || key > iz.key_hi {
                    continue;
                }
                if velocity < iz.vel_lo || velocity > iz.vel_hi {
                    continue;
                }
                if iz.sample_id < 0 || iz.sample_id >= self.samples.len() as i32 {
                    continue;
                }
                let mut r = Region {
                    sample: iz.sample_id as usize,
                    gen_set: ins.global,
                    sf1_atten_units: -1,
                };
                r.gen_set.override_from(&iz.gen_set);
                let mut preset_gen = preset.global;
                preset_gen.override_from(&pz.gen_set);
                if self.version == Version::Sf1 {
                    let mut units = 0;
                    let mut any = false;
                    if r.gen_set.has(generator::InitialAttenuation) {
                        units += 127 - r.gen_set.value[generator::InitialAttenuation] as i32;
                        any = true;
                    }
                    if preset_gen.has(generator::InitialAttenuation) {
                        units += 127 - preset_gen.value[generator::InitialAttenuation] as i32;
                        any = true;
                    }
                    r.sf1_atten_units = if any { units } else { -1 };
                }
                r.gen_set.fill_from(&preset_gen);
                out.push(r);
            }
        }
        out
    }
}

/// SoundFont.h:170 `VoiceParams`
#[derive(Clone, Copy, Debug)]
pub struct VoiceParams {
    pub ccca: u32,
    pub sample_start: u32,
    pub patch_pan: i32,
    pub psst: u32,
    pub csl: u32,
    pub ip: u16,
    pub ip_cents: i32,
    pub ifatn: u16,
    pub patch_atten_units: u8,
    pub pefe: u16,
    pub fmmod: u16,
    pub tremfrq: u16,
    pub fm2frq2: u16,
    pub envvol: u16,
    pub atkhldv: u16,
    pub dcysusv: u16,
    pub envval: u16,
    pub atkhld: u16,
    pub dcysus: u16,
    pub lfo1val: u16,
    pub lfo2val: u16,
    pub reverb_send: u8,
    pub release_rate: u8,
    pub release_mod_rate: u8,
    pub looping: bool,
    pub sample_end_addr: u32,
    pub loop_end_addr: u32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams {
            ccca: 0,
            sample_start: 0,
            patch_pan: 64,
            psst: 0,
            csl: 0,
            ip: 0,
            ip_cents: 0,
            ifatn: 0xFF00,
            patch_atten_units: 0,
            pefe: 0,
            fmmod: 0,
            tremfrq: 0,
            fm2frq2: 0,
            envvol: 0x8000,
            atkhldv: 0x7F7F,
            dcysusv: 0x7F00,
            envval: 0x8000,
            atkhld: 0x7F7F,
            dcysus: 0x7F00,
            lfo1val: 0x8000,
            lfo2val: 0x8000,
            reverb_send: 0,
            release_rate: 0,
            release_mod_rate: 0,
            looping: true,
            sample_end_addr: 0,
            loop_end_addr: 0,
        }
    }
}

/// `MakeVoiceParams`, SoundFont.cpp:813
pub fn make_voice_params(
    bank: &Bank,
    region: &Region,
    key: i32,
    _velocity: i32,
    dram_base: u32,
    rom_pool_base: u32,
    drv: Driver,
) -> VoiceParams {
    let mut vp = VoiceParams::default();
    let g = &region.gen_set;
    let s = &bank.samples[region.sample];
    let sf1 = bank.version == Version::Sf1;

    let in_rom = s.in_rom || bank.samples_in_rom;
    let base = if in_rom { rom_pool_base } else { dram_base };
    let (start, mut loop_start, mut loop_end);
    let gi = |op: usize| g.get(op, 0) as u32;
    if sf1 {
        let off = if in_rom { 0 } else { dram_base };
        start = s
            .start
            .wrapping_add(off)
            .wrapping_add(gi(generator::StartAddrsOffset))
            .wrapping_add(32768u32.wrapping_mul(gi(generator::StartAddrsCoarseOffset)));
        loop_start = s
            .loop_start
            .wrapping_add(off)
            .wrapping_add(gi(generator::StartloopAddrsOffset))
            .wrapping_add(32768u32.wrapping_mul(gi(generator::StartloopAddrsCoarseOffset)));
        loop_end = s
            .loop_end
            .wrapping_add(off)
            .wrapping_add(gi(generator::EndloopAddrsOffset))
            .wrapping_add(32768u32.wrapping_mul(gi(generator::EndloopAddrsCoarseOffset)));
    } else {
        start = base
            .wrapping_add(s.start)
            .wrapping_add(gi(generator::StartAddrsOffset))
            .wrapping_add(32768u32.wrapping_mul(gi(generator::StartAddrsCoarseOffset)))
            .wrapping_sub(1);
        loop_start = base
            .wrapping_add(s.loop_start)
            .wrapping_add(gi(generator::StartloopAddrsOffset))
            .wrapping_add(32768u32.wrapping_mul(gi(generator::StartloopAddrsCoarseOffset)))
            .wrapping_sub(2);
        loop_end = base
            .wrapping_add(s.loop_end)
            .wrapping_add(gi(generator::EndloopAddrsOffset))
            .wrapping_add(32768u32.wrapping_mul(gi(generator::EndloopAddrsCoarseOffset)))
            .wrapping_sub(3);
    }

    vp.sample_end_addr = if sf1 && in_rom {
        s.end
    } else {
        base.wrapping_add(s.end)
    };
    vp.loop_end_addr = loop_end;
    let sample_modes = g.get(generator::SampleModes, 0);
    vp.looping = (sample_modes & 1) != 0;
    if !vp.looping {
        let end = if sf1 && in_rom {
            s.end
        } else {
            base.wrapping_add(s.end)
        };
        loop_start = end.wrapping_add(4);
        loop_end = end.wrapping_add(8);
    }

    let mut q = if sf1 {
        g.get(generator::InitialFilterQ, 0) >> 3
    } else {
        lround(
            g.get(generator::InitialFilterQ, 0) as f64 / 10.0 / RESONANCE_MAX_DB
                * CCCA_Q_MAX as f64,
        ) as i32
    };
    q = q.clamp(0, CCCA_Q_MAX);
    vp.sample_start = start;
    vp.ccca = ((q as u32) << CCCA_Q_SHIFT)
        | (start.wrapping_sub(DEFAULT_DRIVER_START_OFFSET) & CCCA_ADDRESS_MASK);

    vp.patch_pan = if sf1 {
        g.get(generator::Pan, 64)
    } else {
        (64 + lround(g.get(generator::Pan, 0) as f64 * 127.0 / 1000.0) as i32).clamp(0, 127)
    };
    let pan = 0x17F - 2 * (vp.patch_pan + 64);
    vp.psst = ((clamp_u8(pan) as u32) << PAN_SHIFT) | (loop_start & LOOP_ADDRESS_MASK);

    let chorus = if sf1 {
        g.get(generator::ChorusEffectsSend, 0)
    } else {
        lround(g.get(generator::ChorusEffectsSend, 0) as f64 * 255.0 / 1000.0) as i32
    };
    vp.csl = ((clamp_u8(chorus) as u32) << CHORUS_SHIFT)
        | (loop_end.wrapping_add(if vp.looping { 1 } else { 0 }) & LOOP_ADDRESS_MASK);

    vp.reverb_send = clamp_u8(if sf1 {
        g.get(generator::ReverbEffectsSend, 28)
    } else {
        lround(g.get(generator::ReverbEffectsSend, 0) as f64 * 255.0 / 1000.0) as i32
    });

    let mut root_key = g.get(generator::OverridingRootKey, -1);
    if root_key < 0 {
        root_key = s.original_key as i32;
    }
    let mut cents = 0.0f64;
    if sf1 && g.has(generator::Sf1RootPitchCents) {
        cents = -(g.value[generator::Sf1RootPitchCents] as f64 - root_key as f64 * 100.0);
    }
    cents += s.correction as f64;
    cents += g.get(generator::CoarseTune, 0) as f64 * 100.0;
    cents += g.get(generator::FineTune, 0) as f64;

    let mut pitch_cents = (key - root_key) as f64 * 100.0 + cents;
    if g.get(generator::ScaleTuning, 100) == 1 {
        pitch_cents = ((pitch_cents as i32) / 2) as f64;
    }
    let cents_total = pitch_cents + (s.sample_rate as f64 / 44100.0).log2() * 1200.0;
    vp.ip_cents = cents_total as i32;
    vp.ip = pitch_from_cents(cents_total).clamp(0, 65535) as u16;

    if sf1 {
        let dflt = if is_dos_like(drv) { 110 } else { 127 };
        let units = if region.sf1_atten_units >= 0 {
            region.sf1_atten_units
        } else {
            127 - dflt
        };
        vp.patch_atten_units = units.clamp(0, 255) as u8;
    } else {
        vp.patch_atten_units =
            (lround(g.get(generator::InitialAttenuation, 0) as f64 / 10.0 / ATTEN_DB_PER_STEP)
                as i32)
                .clamp(0, 255) as u8;
    }

    let cutoff = if !g.has(generator::InitialFilterFc) {
        255
    } else if sf1 {
        (g.value[generator::InitialFilterFc] as i32 * 2).clamp(0, 255)
    } else {
        filter_fc_from_abs_cents(g.value[generator::InitialFilterFc] as i32)
    };
    vp.ifatn = (cutoff << 8) as u16;

    let mod_amount = |op: usize, sf2_scale: f64, sf1_scale: i32| -> i8 {
        if !g.has(op) {
            return 0;
        }
        if sf1 {
            clamp_s8(g.value[op] as i32 * sf1_scale)
        } else {
            clamp_s8(lround(g.value[op] as f64 / sf2_scale) as i32)
        }
    };
    let k_pitch_cents_per_step = 1200.0 * PEFE_PITCH_OCTAVES / 127.0;
    let k_pefe_fc_cents_per_step = 1200.0 * PEFE_FILTER_OCTAVES / 127.0;
    let k_fmmod_fc_cents_per_step = 1200.0 * FMMOD_FILTER_OCTAVES / 127.0;
    let k_trem_cb_per_step = TREMOLO_MAX_DB * 10.0 / 127.0;

    vp.pefe = ((mod_amount(generator::ModEnvToPitch, k_pitch_cents_per_step, 1) as u8 as u16) << 8)
        | mod_amount(generator::ModEnvToFilterFc, k_pefe_fc_cents_per_step, 2) as u8 as u16;

    let lfo1_freq = if sf1 {
        if g.has(generator::FreqModLFO) {
            (g.value[generator::FreqModLFO] as i32 * 2) & 0xFF
        } else {
            128
        }
    } else {
        (lround(
            8.176 * crate::libm::pow(2.0, g.get(generator::FreqModLFO, 0) as f64 / 1200.0)
                / LFO_HZ_PER_STEP,
        ) as i32)
            .clamp(0, 255)
    };
    let lfo2_freq = if sf1 {
        if g.has(generator::FreqVibLFO) {
            (g.value[generator::FreqVibLFO] as i32 * 2) & 0xFF
        } else {
            0
        }
    } else {
        (lround(
            8.176 * crate::libm::pow(2.0, g.get(generator::FreqVibLFO, 0) as f64 / 1200.0)
                / LFO_HZ_PER_STEP,
        ) as i32)
            .clamp(0, 255)
    };

    vp.fmmod = ((mod_amount(generator::ModLfoToPitch, k_pitch_cents_per_step, 1) as u8 as u16)
        << 8)
        | mod_amount(generator::ModLfoToFilterFc, k_fmmod_fc_cents_per_step, 2) as u8 as u16;
    vp.tremfrq = ((mod_amount(generator::ModLfoToVolume, k_trem_cb_per_step, 2) as u8 as u16) << 8)
        | (lfo1_freq as u16);
    vp.fm2frq2 = ((mod_amount(generator::VibLfoToPitch, k_pitch_cents_per_step, 1) as u8 as u16)
        << 8)
        | (lfo2_freq as u16);

    let time_ms = |op: usize, default_ms: f64| -> f64 {
        if !g.has(op) {
            return default_ms;
        }
        if sf1 {
            g.value[op] as u16 as f64
        } else {
            timecents_to_ms(g.value[op] as i32)
        }
    };
    let sustain_reg = |op: usize| -> i32 {
        if !g.has(op) {
            return 0;
        }
        if sf1 {
            return (g.value[op] as i32 * 4 / 3).clamp(0, 0x7F);
        }
        (127 - lround(g.value[op] as f64 / 10.0 / SUSTAIN_DB_PER_STEP) as i32).clamp(0, 127)
    };
    let key_scaled = |time_op: usize, key_op: usize, subtract: bool| -> f64 {
        let amount = g.get(key_op, 0);
        if amount == 0 {
            return time_ms(time_op, 0.0);
        }
        if sf1 {
            let base = time_ms(time_op, 0.0);
            let delta = ((if subtract { key - 60 } else { 60 - key }) * amount) as f64;
            return 0.0f64.max(if subtract { base - delta } else { base + delta });
        }
        let tc = if g.has(time_op) {
            g.value[time_op] as i32
        } else {
            -12000
        };
        let adj = tc + (if subtract { -(key - 60) } else { 60 - key }) * amount;
        timecents_to_ms(adj)
    };
    let delay_reg = |op: usize| -> u16 {
        if sf1 {
            return delay_from_ms(time_ms(op, 0.0)) as u16;
        }
        delay_from_timecents(if g.has(op) {
            g.value[op] as i32
        } else {
            -12000
        }) as u16
    };

    vp.envvol = delay_reg(generator::DelayVolEnv);
    vp.atkhldv = ((hold_from_ms(key_scaled(
        generator::HoldVolEnv,
        generator::KeynumToVolEnvHold,
        false,
    )) << 8)
        | if g.has(generator::AttackVolEnv) {
            attack_rate_from_ms(time_ms(generator::AttackVolEnv, 0.0))
        } else {
            ATTACK_DEFAULT_RATE
        }) as u16;
    vp.dcysusv = ((sustain_reg(generator::SustainVolEnv) << 8)
        | decay_rate_from_ms(key_scaled(
            generator::DecayVolEnv,
            generator::KeynumToVolEnvDecay,
            true,
        ))) as u16;
    vp.release_rate = decay_rate_from_ms(time_ms(generator::ReleaseVolEnv, 0.0)) as u8;

    vp.envval = delay_reg(generator::DelayModEnv);
    vp.atkhld = ((hold_from_ms(key_scaled(
        generator::HoldModEnv,
        generator::KeynumToModEnvHold,
        false,
    )) << 8)
        | if g.has(generator::AttackModEnv) {
            attack_rate_from_ms(time_ms(generator::AttackModEnv, 0.0))
        } else {
            ATTACK_DEFAULT_RATE
        }) as u16;
    vp.dcysus = ((sustain_reg(generator::SustainModEnv) << 8)
        | decay_rate_from_ms(key_scaled(
            generator::DecayModEnv,
            generator::KeynumToModEnvDecay,
            true,
        ))) as u16;
    vp.release_mod_rate = decay_rate_from_ms(time_ms(generator::ReleaseModEnv, 0.0)) as u8;

    vp.lfo1val = delay_reg(generator::DelayModLFO);
    vp.lfo2val = delay_reg(generator::DelayVibLFO);
    vp
}
