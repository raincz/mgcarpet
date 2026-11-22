//! MIDI -> EMU8000 register writes, the Creative DOS driver logic
//! (`Synth.cpp` / `Synth.h`), families `dos` (SBAWE32.MDI) and `sdk`
//! (AWE32 DOS SDK). The `win95` (SBAWE.VXD) paths are not ported.

use crate::core::Emu8000Core;
use crate::driver::*;
use crate::soundfont::{self, Bank, Region, VoiceParams};

const DEFAULT_WAVE_LEN: u32 = 64;
const USABLE_VOICES: usize = 30;
const ROM_POOL_BASE: u32 = 495;
const DRUM_BANK: i32 = 128;
const DRAM_RESERVE_DOS: u32 = 50;

/// Synth.cpp:47 `PitchIncrementSdk`
fn pitch_increment_sdk(ip: u16) -> u32 {
    if ip == 0xFFFF {
        return 0xFFFF;
    }
    let mut v: i32 = 1 << (ip >> 12);
    let mut step = |k: i32| {
        let prod = ((v & 0xFFFF) as u16 as i16 as i32).wrapping_mul(k);
        v = v.wrapping_add((prod / 10000) as i16 as i32);
    };
    if ip & 0x800 != 0 {
        step(0x102E);
    }
    if ip & 0x400 != 0 {
        step(0x0764);
    }
    if ip & 0x200 != 0 {
        step(0x0389);
    }
    v = v.wrapping_add(v >> 1);
    if (v as u32 >> 16) != 0 {
        0xFFFF
    } else {
        v as u32 & 0xFFFF
    }
}

fn make_dcysusv(sustain: i32, rate: i32) -> u16 {
    (((sustain & 0x7F) << 8) | (rate & 0x7F)) as u16
}
fn make_atkhld(hold: i32, attack: i32) -> u16 {
    (((hold & 0x7F) << 8) | (attack & 0x7F)) as u16
}
fn atten_db_to_units(db: f64) -> i32 {
    ((db / ATTEN_DB_PER_STEP).round() as i64 as i32).clamp(0, 255)
}

/// Synth.cpp:325 `MdiAddBend`
fn mdi_add_bend(ip: i32, offset: i32) -> u16 {
    let sum = (ip as u16 as i32) + offset;
    if sum < 0 {
        return sum as u16;
    }
    sum.min(0xFFFF) as u16
}

/// Synth.cpp:338 `SdkAddBend`
fn sdk_add_bend(ip: i32, offset: i32, note_on: bool) -> u16 {
    let sum = (ip as u16 as i32) + offset;
    if sum > 0xFFFF {
        return 0xFFFF;
    }
    if sum < 0 {
        return if note_on { 0xFFFF } else { 0 };
    }
    sum as u16
}

struct LoadedBank {
    bank: Bank,
    dram_base: u32,
}

/// Synth.h:93 `VoiceAlloc` (dos fields; the generic bookkeeping fields are
/// kept as in the C++ even where the dos path never reads them).
#[allow(dead_code)]
#[derive(Clone, Copy)]
struct VoiceAlloc {
    in_use: bool,
    held_by_sustain: bool,
    channel: u8,
    note: u8,
    velocity: u8,
    release_rate: u8,
    release_mod_rate: u8,
    age: u32,
    base_pitch: i32,
    mdi_state: u16,
    mdi_note_end: u32,
    mdi_end_addr: u32,
    mdi_velocity: u8,
    mdi_patch_atten: u8,
    sdk_rom: bool,
    mdi_mod_release: u8,
    mdi_vol_release: u8,
    mdi_fmmod_depth: i8,
}

impl Default for VoiceAlloc {
    fn default() -> Self {
        VoiceAlloc {
            in_use: false,
            held_by_sustain: false,
            channel: 0,
            note: 0,
            velocity: 0,
            release_rate: 0x40,
            release_mod_rate: 0,
            age: 0,
            base_pitch: 0,
            mdi_state: 0xFFFF,
            mdi_note_end: 0,
            mdi_end_addr: 0,
            mdi_velocity: 0,
            mdi_patch_atten: 0,
            sdk_rom: false,
            mdi_mod_release: 0,
            mdi_vol_release: 0,
            mdi_fmmod_depth: 0,
        }
    }
}

/// Synth.h:136 `ChannelState`
#[derive(Clone, Copy)]
struct ChannelState {
    program: u8,
    mod_wheel: u8,
    bank_msb: u8,
    bank_lsb: u8,
    volume: u8,
    expression: u8,
    pan: u8,
    reverb_send: u8,
    chorus_send: u8,
    sustain: bool,
    pitch_bend: i16,
    pitch_bend_range_semitones: u8,
    mdi_mod_div30: u8,
    mdi_pressure_div30: u8,
    mdi_bend_offset: i16,
    mdi_rpn_mode: bool,
    mdi_rpn_lsb: u8,
    mdi_rpn_msb: u8,
}

impl Default for ChannelState {
    fn default() -> Self {
        ChannelState {
            program: 0,
            mod_wheel: 0,
            bank_msb: 0,
            bank_lsb: 0,
            volume: 100,
            expression: 127,
            pan: 64,
            reverb_send: 0,
            chorus_send: 0,
            sustain: false,
            pitch_bend: 0,
            pitch_bend_range_semitones: 2,
            mdi_mod_div30: 0,
            mdi_pressure_div30: 0,
            mdi_bend_offset: 0,
            mdi_rpn_mode: false,
            mdi_rpn_lsb: 0,
            mdi_rpn_msb: 0,
        }
    }
}

/// `Synth` (Synth.h:26).
pub struct Synth {
    core: Emu8000Core,
    banks: Vec<LoadedBank>,
    next_dram_base: u32,
    channel_mask: u16,
    master_volume: i32,
    fallback_start: u32,
    fallback_loop_start: u32,
    fallback_loop_end: u32,
    fallback_unity_hz: f64,
    alloc: [VoiceAlloc; 32],
    channels: [ChannelState; 16],
    age_counter: u32,
}

impl Synth {
    /// `Synth::Synth` + `BuildDefaultWaveform` (Synth.cpp:79/89).
    pub fn new(core: Emu8000Core) -> Self {
        let mut s = Synth {
            core,
            banks: Vec::new(),
            next_dram_base: 0,
            channel_mask: 0xFFFF,
            master_volume: 127,
            fallback_start: 0,
            fallback_loop_start: 0,
            fallback_loop_end: 0,
            fallback_unity_hz: 0.0,
            alloc: [VoiceAlloc::default(); 32],
            channels: [ChannelState::default(); 16],
            age_counter: 0,
        };
        s.build_default_waveform();
        s
    }

    pub fn core(&mut self) -> &mut Emu8000Core {
        &mut self.core
    }

    /// Synth.h:63
    pub fn set_channel_mask(&mut self, mask: u16) {
        self.channel_mask = mask;
    }
    /// Synth.h:69
    pub fn set_master_volume(&mut self, v: i32) {
        self.master_volume = v;
    }

    fn effective_channel_volume(&self, cc7: i32) -> i32 {
        if self.master_volume >= 127 {
            cc7
        } else {
            cc7 * self.master_volume / 127
        }
    }

    /// Synth.cpp:89
    fn build_default_waveform(&mut self) {
        let n = (DEFAULT_WAVE_LEN + 8) as usize;
        if self.core.dram.len() < n {
            self.core.dram = vec![0; n];
        }
        for i in 0..n {
            let phase = 2.0 * std::f64::consts::PI * (i as u32 % DEFAULT_WAVE_LEN) as f64
                / DEFAULT_WAVE_LEN as f64;
            self.core.dram[i] = crate::chip::cvtt_f64(phase.sin() * 30000.0) as i16;
        }
        self.fallback_start = DRAM_OFFSET;
        self.fallback_loop_start = DRAM_OFFSET;
        self.fallback_loop_end = DRAM_OFFSET + DEFAULT_WAVE_LEN;
        self.fallback_unity_hz = 44100.0 / DEFAULT_WAVE_LEN as f64;
    }

    /// `LoadBank` (Synth.cpp:149) with an already parsed bank.
    pub fn load_bank(&mut self, mut bank: Bank, samples_in_rom: bool, midi_bank: i32) {
        bank.samples_in_rom = samples_in_rom;
        if midi_bank >= 0 {
            for p in bank.presets.iter_mut() {
                if p.bank == 0 {
                    p.bank = midi_bank;
                }
            }
        }
        if self.next_dram_base == 0 {
            self.next_dram_base = DRAM_OFFSET + DRAM_RESERVE_DOS;
        }
        let dram_base = self.next_dram_base;

        if !samples_in_rom && !bank.sample_data.is_empty() {
            let offset = (dram_base - DRAM_OFFSET) as usize;
            let needed = offset + bank.sample_data.len() + 8;
            if self.core.dram.len() < needed {
                self.core.dram.resize(needed, 0);
            }
            self.core.dram[offset..offset + bank.sample_data.len()]
                .copy_from_slice(&bank.sample_data);
            if offset == DRAM_RESERVE_DOS as usize {
                self.core.dram[..offset].fill(0);
            }
            if bank.version == soundfont::Version::Sf1 {
                // the DOS pre-emphasis y = clip16((6x[n] - x[n-1] - x[n+1]) >> 2)
                let shift = 2;
                let round: i32 = (1 << shift) - 1;
                let src = &bank.sample_data;
                let count = src.len();
                for s in &bank.samples {
                    if s.in_rom || s.end <= s.start {
                        continue;
                    }
                    let end_excl = s.end as usize;
                    let first = (s.start as usize).min(count);
                    let last = end_excl.min(count);
                    for i in first..last {
                        let xm = if i > 0 { src[i - 1] as i32 } else { 0 };
                        let xp = if i + 1 < count { src[i + 1] as i32 } else { 0 };
                        let num = 6 * src[i] as i32 - xm - xp;
                        let y = if num >= 0 {
                            num >> shift
                        } else {
                            -((-num + round) >> shift)
                        };
                        self.core.dram[offset + i] = y.clamp(-32768, 32767) as i16;
                    }
                }
            }
            self.next_dram_base += (bank.sample_data.len() + 8) as u32;
        }
        self.banks.push(LoadedBank { bank, dram_base });
    }

    // -----------------------------------------------------------------------
    // SBAWE32.MDI voice handling, Synth.cpp:317-822
    // -----------------------------------------------------------------------

    /// Synth.cpp:345 `AllocateVoiceMdi`
    fn allocate_voice_mdi(&mut self, state: u16) -> i32 {
        let mut best = 0xFFFF_FFFFu32;
        let mut chosen = USABLE_VOICES as i32 - 1;
        let mut done = false;
        let mut pass = 0;
        while pass < 2 && !done {
            let mut v = pass;
            while v < USABLE_VOICES {
                let s = self.alloc[v].mdi_state;
                if s == 0xFFFF || s < 0x1000 {
                    let mut score = self.core.read_driver(Reg::VTFT, v) >> 16;
                    if s != 0xFFFF {
                        score += 0x200;
                        if (s & 0xFF) == 0xFF {
                            score += 0x300;
                        }
                    }
                    if self.core.read_driver(Reg::DCYSUSV, v) & DCYSUSV_RELEASE == 0 {
                        score += 0x1000;
                    }
                    if score <= best {
                        best = score;
                        chosen = v as i32;
                        if score == 0 {
                            done = true;
                            break;
                        }
                    }
                }
                if self.alloc[v].mdi_end_addr != 0
                    && (self.core.read_driver(Reg::CCCA, v) & CCCA_ADDRESS_MASK)
                        >= self.alloc[v].mdi_end_addr
                {
                    chosen = v as i32;
                    done = true;
                    break;
                }
                v += 2;
            }
            pass += 1;
        }
        let a = &mut self.alloc[chosen as usize];
        a.mdi_state = state;
        a.mdi_end_addr = 0;
        a.in_use = false;
        a.held_by_sustain = false;
        self.core.write(Reg::DCYSUSV, chosen as usize, 0x807F);
        chosen
    }

    /// Synth.cpp:403 `AllocateVoiceSdk`
    fn allocate_voice_sdk(&mut self, state: u16) -> i32 {
        let mut best = 0xFFFF_FFFFu32;
        let mut chosen: i32 = -1;
        let mut done = false;
        let mut pass = 0;
        while pass < 2 && !done {
            let mut v = pass;
            while v < USABLE_VOICES {
                let s = self.alloc[v].mdi_state;
                if s == 0xFFFF || s < 0x1000 {
                    let mut score = self.core.read_driver(Reg::VTFT, v) >> 16;
                    if s == 0xFFFF {
                        if score == 0 {
                            chosen = v as i32;
                            done = true;
                            break;
                        }
                    } else {
                        score += 0x200;
                        if (s & 0xFF) == 0xFF {
                            score += 0x300;
                        } else if self.core.read_driver(Reg::DCYSUSV, v) & DCYSUSV_RELEASE == 0 {
                            score += 0x1300;
                        }
                    }
                    if score < best {
                        best = score;
                        chosen = v as i32;
                    }
                }
                if self.alloc[v].mdi_end_addr != 0
                    && (self.core.read_driver(Reg::CCCA, v) & CCCA_ADDRESS_MASK)
                        >= self.alloc[v].mdi_end_addr
                {
                    chosen = v as i32;
                    done = true;
                    break;
                }
                v += 2;
            }
            pass += 1;
        }
        if chosen < 0 {
            return -1;
        }
        let a = &mut self.alloc[chosen as usize];
        a.mdi_state = state;
        a.mdi_end_addr = 0;
        a.in_use = false;
        a.held_by_sustain = false;
        self.core.write(Reg::DCYSUSV, chosen as usize, 0x0080);
        self.core.write(Reg::VTFT, chosen as usize, 0x0000_FFFF);
        chosen
    }

    /// Synth.cpp:614 `NoteOffMdi`
    fn note_off_mdi(&mut self, voice: usize) {
        self.alloc[voice].mdi_state = 0xFF00;
        let a = self.alloc[voice];
        self.core
            .write(Reg::DCYSUS, voice, 0x8000 | a.mdi_mod_release as u32);
        self.core
            .write(Reg::DCYSUSV, voice, 0x8000 | a.mdi_vol_release as u32);
        if a.mdi_note_end != 0 {
            let csl = self.core.read_driver(Reg::CSL, voice);
            self.core.write(
                Reg::CSL,
                voice,
                (csl & 0xFF00_0000) | (a.mdi_note_end.wrapping_add(4)),
            );
            let psst = self.core.read_driver(Reg::PSST, voice);
            self.core
                .write(Reg::PSST, voice, (psst & 0xFF00_0000) | a.mdi_note_end);
            self.alloc[voice].mdi_end_addr = a.mdi_note_end;
        } else {
            self.alloc[voice].mdi_end_addr = 0;
        }
        let a = &mut self.alloc[voice];
        a.mdi_state = 0xFFFF;
        a.in_use = false;
        a.held_by_sustain = false;
    }

    /// Synth.cpp:642 `UpdateAttenMdi`
    fn update_atten_mdi(&mut self, channel: u8) {
        let ch = self.channels[channel as usize];
        let vol = self
            .effective_channel_volume(ch.volume as i32)
            .clamp(0, 127);
        let sdk = self.core.driver_variant() == Driver::Sdk;
        for v in 0..USABLE_VOICES {
            let a = self.alloc[v];
            if ((a.mdi_state >> 8) & 0x0F) as u8 != channel {
                continue;
            }
            let mut atten: i32;
            if vol <= 10 {
                atten = 0xFF;
            } else {
                let db = (velocity_db(a.mdi_velocity as i32, Driver::Dos)
                    + CHANNEL_VOLUME_DB[vol as usize] as i32) as u16;
                let patch = (((a.mdi_patch_atten as u16).wrapping_mul(0x19)) / 0x50) as u16;
                let sum = patch.wrapping_add(db);
                atten = ((sum << 3) / 3) as i32;
                if sdk {
                    if atten > 0xFF {
                        atten = 0xFF;
                    } else if ch.expression < 0x7F {
                        atten += (((EXPRESSION_DB[ch.expression as usize] as i32 * (0x100 - atten))
                            as u16)
                            >> 7) as i32;
                    }
                    if a.sdk_rom {
                        atten = (atten + 0x10).min(0xFF);
                    }
                } else if atten >= 0xFF {
                    atten = 0xFF;
                } else if ch.expression < 0x7F {
                    atten += EXPRESSION_DB[ch.expression as usize] as i32 * (0xFF - atten) / 0x7F;
                }
            }
            let ifatn = self.core.read_driver(Reg::IFATN, v) as u16;
            self.core.write(
                Reg::IFATN,
                v,
                ((ifatn & 0xFF00) | (atten as u16 & 0xFF)) as u32,
            );
        }
    }

    /// Synth.cpp:688 `UpdateFmmodMdi`
    fn update_fmmod_mdi(&mut self, channel: u8, value: i32) {
        self.channels[channel as usize].mdi_mod_div30 = (value / 30) as u8;
        let ch = self.channels[channel as usize];
        for v in 0..USABLE_VOICES {
            let a = self.alloc[v];
            let hi = (a.mdi_state >> 8) as i32;
            if hi == 0xFF || (hi & 0x0F) != channel as i32 {
                continue;
            }
            let mut depth = ch.mdi_pressure_div30 as i32 + a.mdi_fmmod_depth as i32 + value / 30;
            if depth > 0x7F {
                depth = 0x7F;
            }
            let fmmod = self.core.read_driver(Reg::FMMOD, v) as u16;
            self.core.write(
                Reg::FMMOD,
                v,
                ((fmmod & 0x00FF) | (((depth << 8) & 0xFF00) as u16)) as u32,
            );
        }
    }

    /// Synth.cpp:708 `PitchBendMdi`
    fn pitch_bend_mdi(&mut self, channel: u8, value: i16) {
        let ch = &mut self.channels[channel as usize];
        ch.pitch_bend = value;
        let range = if ch.pitch_bend_range_semitones != 0 {
            ch.pitch_bend_range_semitones as i32
        } else {
            2
        };
        let sdk = self.core.driver_variant() == Driver::Sdk;
        if sdk {
            ch.mdi_bend_offset = ((value as i32 * range) / 24) as i16;
        } else {
            let product = (value as i32).wrapping_mul(((range * 0x155) as u16) as i16 as i32);
            ch.mdi_bend_offset = (product / 0x2000) as i16;
        }
        let off = ch.mdi_bend_offset as i32;
        for v in 0..USABLE_VOICES {
            let a = self.alloc[v];
            let hi = (a.mdi_state >> 8) as i32;
            if hi == 0xFF || (hi & 0x0F) != channel as i32 {
                continue;
            }
            let ip = if sdk {
                sdk_add_bend(a.base_pitch, off, false)
            } else {
                mdi_add_bend(a.base_pitch, off)
            };
            self.core.write(Reg::IP, v, ip as u32);
        }
    }

    /// Synth.cpp:741 `SustainMdi`
    fn sustain_mdi(&mut self, channel: u8, value: u8) {
        if value >= 0x40 {
            self.channels[channel as usize].sustain = true;
            return;
        }
        self.channels[channel as usize].sustain = false;
        for v in 0..USABLE_VOICES {
            let s = self.alloc[v].mdi_state;
            if (s & 0xFF) == 0xFF && (s >> 8) != 0xFF && ((s >> 8) & 0x0F) as u8 == channel {
                self.note_off_mdi(v);
            }
        }
    }

    /// Synth.cpp:759 `AllNotesOffMdi`
    fn all_notes_off_mdi(&mut self, channel: u8, respect_sustain: bool) {
        let hold = respect_sustain && self.channels[channel as usize].sustain;
        for v in 0..USABLE_VOICES {
            let hi = (self.alloc[v].mdi_state >> 8) as i32;
            if hi == 0xFF || (hi & 0x0F) != channel as i32 {
                continue;
            }
            if hold {
                self.alloc[v].mdi_state |= 0x00FF;
            } else {
                self.note_off_mdi(v);
            }
        }
    }

    /// Synth.cpp:774 `ResetControllersMdi`
    fn reset_controllers_mdi(&mut self, channel: u8) {
        {
            let ch = &mut self.channels[channel as usize];
            ch.mdi_rpn_mode = false;
            ch.mdi_rpn_lsb = 0;
            ch.mdi_rpn_msb = 0;
            ch.mdi_mod_div30 = 0;
            ch.mod_wheel = 0;
        }
        self.sustain_mdi(channel, 0);
        self.channels[channel as usize].expression = 0x7F;
        self.update_atten_mdi(channel);
        self.pitch_bend_mdi(channel, 0);
        self.channels[channel as usize].mdi_pressure_div30 = 0;
        let m = self.channels[channel as usize].mdi_mod_div30 as i32 * 30;
        self.update_fmmod_mdi(channel, m);
    }

    /// Synth.cpp:791 `ControlChangeMdi`
    fn control_change_mdi(&mut self, channel: u8, controller: u8, value: u8) -> bool {
        let c = channel as usize;
        match controller {
            1 => {
                self.channels[c].mod_wheel = value;
                self.update_fmmod_mdi(channel, value as i32);
            }
            6 => {
                let ch = &mut self.channels[c];
                if ch.mdi_rpn_mode && ch.mdi_rpn_lsb == 0 && ch.mdi_rpn_msb == 0 {
                    ch.pitch_bend_range_semitones = value;
                }
            }
            7 => {
                self.channels[c].volume = value;
                self.update_atten_mdi(channel);
            }
            11 => {
                self.channels[c].expression = value;
                self.update_atten_mdi(channel);
            }
            64 => self.sustain_mdi(channel, value),
            100 => {
                self.channels[c].mdi_rpn_mode = true;
                self.channels[c].mdi_rpn_lsb = value;
            }
            101 => {
                self.channels[c].mdi_rpn_mode = true;
                self.channels[c].mdi_rpn_msb = value;
            }
            120 => self.all_notes_off_mdi(channel, false),
            121 => self.reset_controllers_mdi(channel),
            123 => self.all_notes_off_mdi(channel, true),
            _ => return false,
        }
        true
    }

    /// Synth.cpp:823 `ChannelPressure` (dos families)
    pub fn channel_pressure(&mut self, channel: u8, value: u8) {
        if channel >= 16 {
            return;
        }
        self.channels[channel as usize].mdi_pressure_div30 = value / 30;
        let m = self.channels[channel as usize].mdi_mod_div30 as i32 * 30;
        self.update_fmmod_mdi(channel, m);
    }

    /// Synth.cpp:861 `StartLayers` (dos families)
    fn start_layers(
        &mut self,
        bank_index: usize,
        regions: &[Region],
        channel: u8,
        note: u8,
        velocity: u8,
    ) {
        let drv = self.core.driver_variant();
        let vps: Vec<VoiceParams> = {
            let lb = &self.banks[bank_index];
            regions
                .iter()
                .map(|r| {
                    soundfont::make_voice_params(
                        &lb.bank,
                        r,
                        note as i32,
                        velocity as i32,
                        lb.dram_base,
                        ROM_POOL_BASE,
                        drv,
                    )
                })
                .collect()
        };
        let sdk = drv == Driver::Sdk;
        let mut voices = Vec::with_capacity(regions.len());
        for _ in 0..regions.len() {
            voices.push(if sdk {
                self.allocate_voice_sdk(0xFFFE)
            } else {
                self.allocate_voice_mdi(0xFFFE)
            });
        }
        for i in 0..regions.len() {
            if voices[i] >= 0 {
                self.start_voice(
                    voices[i] as usize,
                    channel,
                    note,
                    velocity,
                    &vps[i],
                    Some((bank_index, regions[i])),
                );
            }
        }
    }

    fn bank_number_for(&self, channel: u8) -> i32 {
        if channel == 9 {
            return DRUM_BANK;
        }
        self.channels[channel as usize].bank_msb as i32
    }

    /// Synth.cpp:973 `StartVoice` (dos + sdk branches).
    fn start_voice(
        &mut self,
        voice: usize,
        channel: u8,
        note: u8,
        velocity: u8,
        vp: &VoiceParams,
        src: Option<(usize, Region)>,
    ) {
        let ch = self.channels[channel as usize];
        let drv = self.core.driver_variant();
        let mut atten = compute_attenuation(
            self.effective_channel_volume(ch.volume as i32),
            velocity as i32,
            ch.expression as i32,
            vp.patch_atten_units as i32,
            drv,
        );

        let sample_in_rom = match src {
            Some((bi, r)) => {
                let b = &self.banks[bi].bank;
                b.samples[r.sample].in_rom || b.samples_in_rom
            }
            None => false,
        };
        if sample_in_rom && drv == Driver::Sdk {
            atten = (atten + 16).min(255);
        }

        let mut cutoff = ((vp.ifatn >> 8) & 0xFF) as i32;
        let attack_rate = (vp.atkhldv & ATKHLD_ATTACK_MASK) as i32;
        if channel != 9 && attack_rate < 0x7D {
            let v = (velocity as i32).max(0x46);
            cutoff = if drv == Driver::Dos {
                (cutoff * v + 0x40) / 0x7F
            } else {
                (cutoff * v + 0xA0) >> 7
            };
            cutoff = cutoff.clamp(0, 255);
        }
        let ifatn = ((cutoff << 8) | atten) as u16;

        let mut pan = 0x17F - 2 * (vp.patch_pan + ch.pan as i32);
        if pan >= 0xFE {
            pan = 0xFF;
        } else if pan < (if drv == Driver::Dos { 0 } else { 2 }) {
            pan = 0;
        }
        let psst = ((pan as u32) << PAN_SHIFT) | (vp.psst & LOOP_ADDRESS_MASK);

        let ch_chorus = ch.chorus_send as i32 * 90 / 100;
        let chorus = (((vp.csl >> CHORUS_SHIFT) & 0xFF) as i32 + ch_chorus).clamp(0, 255);
        let csl = ((chorus as u32) << CHORUS_SHIFT) | (vp.csl & LOOP_ADDRESS_MASK);
        let ch_reverb = ch.reverb_send as i32 * 90 / 100;
        let reverb = (vp.reverb_send as i32 + ch_reverb).clamp(0, 255);

        let pitch: i32 = if drv == Driver::Sdk {
            sdk_add_bend(vp.ip as i32, ch.mdi_bend_offset as i32, true) as i32
        } else {
            mdi_add_bend(vp.ip as i32, ch.mdi_bend_offset as i32) as i32
        };

        let mod_depth = ((((vp.fmmod >> 8) & 0xFF) as u8 as i8) as i32
            + ch.mdi_mod_div30 as i32
            + ch.mdi_pressure_div30 as i32)
            .min(0x7F);
        let fmmod = (((mod_depth as u8 as u16) << 8) | (vp.fmmod & 0xFF)) as u16;

        let reverb_byte = (reverb.clamp(0, 255) as u32) << REVERB_SHIFT;
        let pan_aux: u32 = if drv != Driver::Dos {
            (256 - pan).clamp(0, 255) as u32
        } else {
            0
        };
        let ccca = (vp.ccca & !CCCA_ADDRESS_MASK)
            | (vp.sample_start.wrapping_sub(start_address_offset(drv)) & CCCA_ADDRESS_MASK);

        let c = &mut self.core;
        if drv == Driver::Sdk {
            // Synth.cpp:1196
            let vol_instant = (vp.atkhldv & 0x7F) == 0x7F && vp.envvol >= 0x8000;
            let mod_instant = (vp.atkhld & 0x7F) == 0x7F && vp.envval >= 0x8000;
            let envvol_reg: u32 = if vol_instant {
                0xBFFF
            } else {
                vp.envvol as u32
            };
            let envval_reg: u32 = if mod_instant {
                0xBFFF
            } else {
                vp.envval as u32
            };

            let mut target_pitch = pitch as u32 & 0xFFFF;
            let mut filter = cutoff;
            if mod_instant {
                let depth: i16 = (((((vp.pefe >> 8) & 0xFF) as u8 as i8) as i16) << 4) as i16;
                let mut lo = (depth as u16 as u32).wrapping_add(target_pitch);
                let hi = (if depth < 0 { 0xFFFFu32 } else { 0 }).wrapping_add(lo >> 16);
                lo &= 0xFFFF;
                target_pitch = if (hi & 0xFFFF) != 0 { 0xFFFF } else { lo };
                filter = (cutoff + (vp.pefe & 0xFF) as u8 as i8 as i32).clamp(0, 255);
            }
            let increment = pitch_increment_sdk(target_pitch as u16);
            let vol_target: u32 = if vol_instant {
                volume_target(atten) as u32
            } else {
                0
            };
            let vtft = (vol_target << 16) | ((filter as u32) << 8);

            c.write(Reg::VTFT, voice, 0x0000_FFFF);
            c.write(Reg::ATKHLDV, voice, vp.atkhldv as u32);
            c.write(Reg::LFO1VAL, voice, vp.lfo1val as u32);
            c.write(Reg::ATKHLD, voice, vp.atkhld as u32);
            c.write(Reg::DCYSUS, voice, vp.dcysus as u32);
            c.write(Reg::LFO2VAL, voice, vp.lfo2val as u32);
            c.write(Reg::IP, voice, pitch as u16 as u32);
            c.write(Reg::IFATN, voice, ifatn as u32);
            c.write(Reg::PEFE, voice, vp.pefe as u32);
            c.write(Reg::FMMOD, voice, fmmod as u32);
            c.write(Reg::TREMFRQ, voice, vp.tremfrq as u32);
            c.write(Reg::FM2FRQ2, voice, vp.fm2frq2 as u32);
            c.write(Reg::ENVVAL, voice, envval_reg);
            c.write(Reg::ENVVOL, voice, envvol_reg);
            c.write(Reg::CVCF, voice, 0x0000_FFFF);
            c.write(Reg::PTRX, voice, 0);
            c.write(Reg::CPF, voice, 0);
            c.write(Reg::PSST, voice, psst);
            c.write(Reg::CSL, voice, csl);
            c.write(Reg::CCCA, voice, ccca & 0x00FF_FFFF);
            c.write(Reg::Unk0088, voice, 0);
            c.write(Reg::Unk0080, voice, 0);
            c.write(Reg::CCCA, voice, ccca);
            c.write(Reg::VTFT, voice, vtft);
            c.write(Reg::CVCF, voice, vtft);
            c.write(Reg::PTRX, voice, (increment << 16) | reverb_byte | pan_aux);
            c.write(Reg::CPF, voice, increment << 16);
        } else {
            // SBAWE32.MDI note-on, Synth.cpp:1253
            c.write(Reg::IP, voice, 0xE000);
            c.write(Reg::DCYSUSV, voice, DCYSUSV_OFF);
            c.write(Reg::VTFT, voice, 0x0000_FFFF);
            c.write(Reg::ENVVOL, voice, vp.envvol as u32);
            c.write(Reg::ATKHLDV, voice, vp.atkhldv as u32);
            c.write(Reg::ENVVAL, voice, vp.envval as u32);
            c.write(Reg::ATKHLD, voice, vp.atkhld as u32);
            c.write(Reg::DCYSUS, voice, vp.dcysus as u32);
            c.write(Reg::IP, voice, pitch as u16 as u32);
            c.write(Reg::IFATN, voice, ifatn as u32);
            c.write(Reg::LFO1VAL, voice, vp.lfo1val as u32);
            c.write(Reg::LFO2VAL, voice, vp.lfo2val as u32);
            c.write(Reg::PEFE, voice, vp.pefe as u32);
            c.write(Reg::FMMOD, voice, fmmod as u32);
            c.write(Reg::TREMFRQ, voice, vp.tremfrq as u32);
            c.write(Reg::FM2FRQ2, voice, vp.fm2frq2 as u32);
            let cur = c.read_driver(Reg::PTRX, voice);
            c.write(Reg::PTRX, voice, (cur & 0xFFFF_00FF) | reverb_byte);
            c.write(Reg::PSST, voice, psst);
            c.write(Reg::CSL, voice, csl);
            c.write(Reg::CCCA, voice, ccca);
        }

        c.write(Reg::DCYSUSV, voice, vp.dcysusv as u32);

        self.age_counter += 1;
        let mut a = VoiceAlloc {
            in_use: true,
            channel,
            note,
            velocity,
            release_rate: if vp.release_rate != 0 {
                vp.release_rate
            } else {
                0x40
            },
            release_mod_rate: vp.release_mod_rate,
            age: self.age_counter,
            base_pitch: vp.ip as i32,
            ..Default::default()
        };
        a.mdi_state = ((channel as u16) << 8) | note as u16;
        a.mdi_velocity = velocity;
        a.mdi_patch_atten = vp.patch_atten_units;
        a.sdk_rom = sample_in_rom;
        a.mdi_mod_release = vp.release_mod_rate;
        a.mdi_vol_release = vp.release_rate;
        a.mdi_fmmod_depth = ((vp.fmmod >> 8) & 0xFF) as u8 as i8;
        a.mdi_note_end = if vp.looping && vp.sample_end_addr.wrapping_sub(vp.loop_end_addr) >= 0x14
        {
            vp.sample_end_addr.wrapping_add(4)
        } else {
            0
        };
        a.mdi_end_addr = 0;
        self.alloc[voice] = a;
    }

    /// Synth.cpp:1424 `StartFallbackVoice`
    fn start_fallback_voice(&mut self, voice: usize, channel: u8, note: u8, velocity: u8) {
        let mut vp = VoiceParams {
            sample_start: self.fallback_start,
            ccca: self.fallback_start & CCCA_ADDRESS_MASK,
            psst: (128u32 << PAN_SHIFT) | (self.fallback_loop_start & LOOP_ADDRESS_MASK),
            csl: self.fallback_loop_end & LOOP_ADDRESS_MASK,
            ..Default::default()
        };
        let freq_hz = 440.0 * crate::libm::pow(2.0, (note as i32 - 69) as f64 / 12.0);
        let octaves = (freq_hz / self.fallback_unity_hz).log2();
        vp.ip = (PITCH_UNITY as f64 + octaves * PITCH_PER_OCTAVE as f64).clamp(0.0, 65535.0) as u16;
        let vel = (velocity as i32).max(1) as f64 / 127.0;
        vp.ifatn = ((0xFF << 8) | atten_db_to_units(-40.0 * vel.log10())) as u16;
        vp.envvol = 0x8000;
        vp.atkhldv = make_atkhld(0x7F, 0x7F);
        vp.dcysusv = make_dcysusv(0x7F, 0);
        vp.envval = 0x8000;
        vp.atkhld = make_atkhld(0x7F, 0x7F);
        vp.dcysus = make_dcysusv(0x7F, 0);
        vp.lfo1val = 0x8000;
        vp.lfo2val = 0x8000;
        vp.release_rate = 0x40;
        self.start_voice(voice, channel, note, velocity, &vp, None);
    }

    /// Synth.cpp:1454 `NoteOn`
    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if channel >= 16 || note > 127 {
            return;
        }
        if (self.channel_mask >> channel) & 1 == 0 {
            return;
        }
        if velocity == 0 {
            self.note_off(channel, note);
            return;
        }
        let bank_num = self.bank_number_for(channel);
        let program = self.channels[channel as usize].program as i32;
        let mut chain = [0i32; 3];
        let mut chain_len = 0;
        chain[chain_len] = bank_num;
        chain_len += 1;
        let drums = bank_num == DRUM_BANK;
        if drums {
            chain[chain_len] = -1;
            chain_len += 1;
        }
        if bank_num != 0 && !drums {
            chain[chain_len] = 0;
            chain_len += 1;
        }
        for pass in 0..chain_len {
            let standard_kit = chain[pass] == -1;
            let want_bank = if standard_kit { DRUM_BANK } else { chain[pass] };
            let want_program = if standard_kit { 0 } else { program };
            for i in (0..self.banks.len()).rev() {
                let regions = self.banks[i].bank.select(
                    want_bank,
                    want_program,
                    note as i32,
                    velocity as i32,
                );
                if regions.is_empty() {
                    continue;
                }
                self.start_layers(i, &regions, channel, note, velocity);
                return;
            }
        }
        for i in (0..self.banks.len()).rev() {
            let regions = self.banks[i]
                .bank
                .select(0, 0, note as i32, velocity as i32);
            if regions.is_empty() {
                continue;
            }
            self.start_layers(i, &regions, channel, note, velocity);
            return;
        }
        let fallback = if self.core.driver_variant() == Driver::Sdk {
            self.allocate_voice_sdk(0xFFFE)
        } else {
            self.allocate_voice_mdi(0xFFFE)
        };
        if fallback >= 0 {
            self.start_fallback_voice(fallback as usize, channel, note, velocity);
        }
    }

    /// Synth.cpp:1534 `NoteOff` (dos families)
    pub fn note_off(&mut self, channel: u8, note: u8) {
        if channel >= 16 {
            return;
        }
        let key = ((channel as u16) << 8) | note as u16;
        for v in 0..USABLE_VOICES {
            let s = self.alloc[v].mdi_state;
            if (s >> 8) == 0xFF || (s & 0x0FFF) != key {
                continue;
            }
            if self.channels[channel as usize].sustain {
                self.alloc[v].mdi_state |= 0x00FF;
            } else {
                self.note_off_mdi(v);
            }
        }
    }

    /// Synth.cpp:1566 `ProgramChange`
    pub fn program_change(&mut self, channel: u8, program: u8) {
        if channel >= 16 {
            return;
        }
        self.channels[channel as usize].program = program;
    }

    /// Synth.cpp:1595 `ControlChange`. For the dos families the MDI
    /// dispatcher takes 1, 6, 7, 11, 64, 100, 101, 120, 121, 123; of the
    /// generic switch only the stored values remain.
    pub fn control_change(&mut self, channel: u8, controller: u8, value: u8) {
        if channel >= 16 {
            return;
        }
        if self.control_change_mdi(channel, controller, value) {
            return;
        }
        let ch = &mut self.channels[channel as usize];
        match controller {
            0 => ch.bank_msb = value,
            32 => ch.bank_lsb = value,
            10 => ch.pan = value,
            91 => ch.reverb_send = value,
            93 => ch.chorus_send = value,
            _ => {}
        }
    }

    /// Synth.cpp:1636 `PitchBend`
    pub fn pitch_bend(&mut self, channel: u8, value: i16) {
        if channel >= 16 {
            return;
        }
        self.pitch_bend_mdi(channel, value);
    }

    /// Synth.cpp:1647 `RenderBlock`
    pub fn render_block(&mut self, out: &mut [i16]) {
        self.core.render_block(out);
    }
}
