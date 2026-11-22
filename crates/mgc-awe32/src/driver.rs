//! Driver tables and register map: `Awe32Driver.h`, `Awe32Curves.h`,
//! `Awe32InitArrays.h` and the parts of `Emu8000Regs.h` the Box86 path uses.

// ---------------------------------------------------------------------------
// Awe32Driver.h
// ---------------------------------------------------------------------------

/// The Creative driver family (Awe32Driver.h:57). Only the DOS families are
/// ported: `win95` (SBAWE.VXD) needs the old "ours" core for one fallback
/// path and is not used by Magic Carpet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    /// `AWEUTIL.COM` + `SBAWE32.MDI` (Miles/AIL) - what Magic Carpet 2 uses.
    Dos,
    /// Creative AWE32 DOS SDK (RAWE32L.LIB, midieng.c).
    Sdk,
}

/// Awe32Driver.h:68 - both ported families are "dos-like".
#[inline]
pub fn is_dos_like(_d: Driver) -> bool {
    true
}

/// Awe32Driver.h:86 `StartAddressOffset`
#[inline]
pub fn start_address_offset(d: Driver) -> u32 {
    match d {
        Driver::Dos => 46,
        Driver::Sdk => 5,
    }
}

/// `StartAddressOffset(kDefaultDriver)` = win95 (4): SoundFont.cpp:317
/// precomputes CCCA with the default driver; Synth replaces the address.
pub const DEFAULT_DRIVER_START_OFFSET: u32 = 4;

// ---------------------------------------------------------------------------
// Emu8000Regs.h
// ---------------------------------------------------------------------------

pub const PORT_DATA0: u16 = 0x400;
pub const PORT_DATA0_HI: u16 = 0x402;
pub const PORT_DATA1: u16 = 0x800;
pub const PORT_DATA1_HI: u16 = 0x802;
pub const PORT_DATA3: u16 = 0xC00;
pub const PORT_POINTER: u16 = 0xC02;

/// Emu8000Regs.h:22 `Port` (index into portSel - 2).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Port {
    Data0 = 0,
    Data0Hi = 1,
    Data1 = 2,
    Data1Hi = 3,
    Data3 = 4,
}

/// Emu8000Regs.h:36
pub const fn make_sel(reg_index: u16, p: Port, voice: u16) -> u16 {
    (reg_index << 12) | ((p as u16 + 2) << 9) | (voice & 0x1F)
}

/// Emu8000Regs.h:47
pub const fn sel_to_pointer(sel: u16) -> u16 {
    ((sel & 0x7000) >> 7) | (sel & 0x1F)
}

/// Emu8000Regs.h:54 `Reg` (the "sel" with voice 0).
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum Reg {
    CPF = make_sel(0, Port::Data0, 0),
    PTRX = make_sel(1, Port::Data0, 0),
    CVCF = make_sel(2, Port::Data0, 0),
    VTFT = make_sel(3, Port::Data0, 0),
    Unk0080 = make_sel(4, Port::Data0, 0),
    Unk0088 = make_sel(5, Port::Data0, 0),
    PSST = make_sel(6, Port::Data0, 0),
    CSL = make_sel(7, Port::Data0, 0),
    CCCA = make_sel(0, Port::Data1, 0),
    HWCF = make_sel(1, Port::Data1, 0),
    INIT1 = make_sel(2, Port::Data1, 0),
    INIT3 = make_sel(3, Port::Data1, 0),
    ENVVOL = make_sel(4, Port::Data1, 0),
    DCYSUSV = make_sel(5, Port::Data1, 0),
    ENVVAL = make_sel(6, Port::Data1, 0),
    DCYSUS = make_sel(7, Port::Data1, 0),
    HWCF_HI = make_sel(1, Port::Data1Hi, 0),
    INIT2 = make_sel(2, Port::Data1Hi, 0),
    INIT4 = make_sel(3, Port::Data1Hi, 0),
    ATKHLDV = make_sel(4, Port::Data1Hi, 0),
    LFO1VAL = make_sel(5, Port::Data1Hi, 0),
    ATKHLD = make_sel(6, Port::Data1Hi, 0),
    LFO2VAL = make_sel(7, Port::Data1Hi, 0),
    IP = make_sel(0, Port::Data3, 0),
    IFATN = make_sel(1, Port::Data3, 0),
    PEFE = make_sel(2, Port::Data3, 0),
    FMMOD = make_sel(3, Port::Data3, 0),
    TREMFRQ = make_sel(4, Port::Data3, 0),
    FM2FRQ2 = make_sel(5, Port::Data3, 0),
    Unk6C = make_sel(6, Port::Data3, 0),
    ChipId = make_sel(7, Port::Data3, 0),
}

/// Emu8000Regs.h:105 `Sel`
#[inline]
pub fn sel(r: Reg, voice: usize) -> u16 {
    (r as u16) | (voice as u16 & 0x1F)
}

/// Emu8000Regs.h:112 `Hwcf`
pub mod hwcf {
    pub const HWCF4: u16 = 9;
    pub const HWCF5: u16 = 10;
    pub const HWCF6: u16 = 13;
    pub const HWCF7: u16 = 14;
    pub const SMALR: u16 = 20;
    pub const SMARR: u16 = 21;
    pub const SMALW: u16 = 22;
    pub const SMARW: u16 = 23;
    pub const HWCF1: u16 = 29;
    pub const HWCF2: u16 = 30;
    pub const HWCF3: u16 = 31;
}

pub const PITCH_UNITY: u16 = 0xE000;
pub const PITCH_PER_OCTAVE: i32 = 4096;
pub const CCCA_ADDRESS_MASK: u32 = 0x00FF_FFFF;
pub const CCCA_Q_SHIFT: u32 = 28;
pub const CCCA_Q_MAX: i32 = 15;
pub const LOOP_ADDRESS_MASK: u32 = 0x00FF_FFFF;
pub const PAN_SHIFT: u32 = 24;
pub const CHORUS_SHIFT: u32 = 24;
pub const REVERB_SHIFT: u32 = 8;
pub const DCYSUSV_RELEASE: u32 = 0x8000;
pub const DCYSUSV_OFF: u32 = 0x0080;
pub const SUSTAIN_DB_PER_STEP: f64 = 0.75;
pub const ATKHLD_ATTACK_MASK: u16 = 0x007F;
pub const HOLD_SEC_PER_STEP: f64 = 0.092;
pub const ATTEN_DB_PER_STEP: f64 = 0.375;
pub const RESONANCE_MAX_DB: f64 = 24.0;
pub const CUTOFF_BASE_CENTS: f64 = 4366.0;
pub const CUTOFF_CENTS_STEP: f64 = 29.3843;
pub const DELAY_NONE: u16 = 0x8000;
pub const PEFE_PITCH_OCTAVES: f64 = 1.0;
pub const PEFE_FILTER_OCTAVES: f64 = 6.0;
pub const FMMOD_FILTER_OCTAVES: f64 = 3.0;
pub const TREMOLO_MAX_DB: f64 = 6.0;
pub const LFO_HZ_PER_STEP: f64 = 0.042;
pub const DRAM_OFFSET: u32 = 0x20_0000;

// ---------------------------------------------------------------------------
// Awe32InitArrays.h
// ---------------------------------------------------------------------------

/// Awe32InitArrays.h `kInit1`
pub const INIT1: [u16; 128] = [
    0x03FF, 0x0030, 0x07FF, 0x0130, 0x0BFF, 0x0230, 0x0FFF, 0x0330, 0x13FF, 0x0430, 0x17FF, 0x0530,
    0x1BFF, 0x0630, 0x1FFF, 0x0730, 0x23FF, 0x0830, 0x27FF, 0x0930, 0x2BFF, 0x0A30, 0x2FFF, 0x0B30,
    0x33FF, 0x0C30, 0x37FF, 0x0D30, 0x3BFF, 0x0E30, 0x3FFF, 0x0F30, 0x43FF, 0x0030, 0x47FF, 0x0130,
    0x4BFF, 0x0230, 0x4FFF, 0x0330, 0x53FF, 0x0430, 0x57FF, 0x0530, 0x5BFF, 0x0630, 0x5FFF, 0x0730,
    0x63FF, 0x0830, 0x67FF, 0x0930, 0x6BFF, 0x0A30, 0x6FFF, 0x0B30, 0x73FF, 0x0C30, 0x77FF, 0x0D30,
    0x7BFF, 0x0E30, 0x7FFF, 0x0F30, 0x83FF, 0x0030, 0x87FF, 0x0130, 0x8BFF, 0x0230, 0x8FFF, 0x0330,
    0x93FF, 0x0430, 0x97FF, 0x0530, 0x9BFF, 0x0630, 0x9FFF, 0x0730, 0xA3FF, 0x0830, 0xA7FF, 0x0930,
    0xABFF, 0x0A30, 0xAFFF, 0x0B30, 0xB3FF, 0x0C30, 0xB7FF, 0x0D30, 0xBBFF, 0x0E30, 0xBFFF, 0x0F30,
    0xC3FF, 0x0030, 0xC7FF, 0x0130, 0xCBFF, 0x0230, 0xCFFF, 0x0330, 0xD3FF, 0x0430, 0xD7FF, 0x0530,
    0xDBFF, 0x0630, 0xDFFF, 0x0730, 0xE3FF, 0x0830, 0xE7FF, 0x0930, 0xEBFF, 0x0A30, 0xEFFF, 0x0B30,
    0xF3FF, 0x0C30, 0xF7FF, 0x0D30, 0xFBFF, 0x0E30, 0xFFFF, 0x0F30,
];

/// Awe32InitArrays.h `kInit2`
pub const INIT2: [u16; 128] = [
    0x03FF, 0x8030, 0x07FF, 0x8130, 0x0BFF, 0x8230, 0x0FFF, 0x8330, 0x13FF, 0x8430, 0x17FF, 0x8530,
    0x1BFF, 0x8630, 0x1FFF, 0x8730, 0x23FF, 0x8830, 0x27FF, 0x8930, 0x2BFF, 0x8A30, 0x2FFF, 0x8B30,
    0x33FF, 0x8C30, 0x37FF, 0x8D30, 0x3BFF, 0x8E30, 0x3FFF, 0x8F30, 0x43FF, 0x8030, 0x47FF, 0x8130,
    0x4BFF, 0x8230, 0x4FFF, 0x8330, 0x53FF, 0x8430, 0x57FF, 0x8530, 0x5BFF, 0x8630, 0x5FFF, 0x8730,
    0x63FF, 0x8830, 0x67FF, 0x8930, 0x6BFF, 0x8A30, 0x6FFF, 0x8B30, 0x73FF, 0x8C30, 0x77FF, 0x8D30,
    0x7BFF, 0x8E30, 0x7FFF, 0x8F30, 0x83FF, 0x8030, 0x87FF, 0x8130, 0x8BFF, 0x8230, 0x8FFF, 0x8330,
    0x93FF, 0x8430, 0x97FF, 0x8530, 0x9BFF, 0x8630, 0x9FFF, 0x8730, 0xA3FF, 0x8830, 0xA7FF, 0x8930,
    0xABFF, 0x8A30, 0xAFFF, 0x8B30, 0xB3FF, 0x8C30, 0xB7FF, 0x8D30, 0xBBFF, 0x8E30, 0xBFFF, 0x8F30,
    0xC3FF, 0x8030, 0xC7FF, 0x8130, 0xCBFF, 0x8230, 0xCFFF, 0x8330, 0xD3FF, 0x8430, 0xD7FF, 0x8530,
    0xDBFF, 0x8630, 0xDFFF, 0x8730, 0xE3FF, 0x8830, 0xE7FF, 0x8930, 0xEBFF, 0x8A30, 0xEFFF, 0x8B30,
    0xF3FF, 0x8C30, 0xF7FF, 0x8D30, 0xFBFF, 0x8E30, 0xFFFF, 0x8F30,
];

/// Awe32InitArrays.h `kInit3`
pub const INIT3: [u16; 128] = [
    0x0C10, 0x8470, 0x14FE, 0xB488, 0x167F, 0xA470, 0x18E7, 0x84B5, 0x1B6E, 0x842A, 0x1F1D, 0x852A,
    0x0DA3, 0x8F7C, 0x167E, 0xF254, 0x0000, 0x842A, 0x0001, 0x852A, 0x18E6, 0x8BAA, 0x1B6D, 0xF234,
    0x229F, 0x8429, 0x2746, 0x8529, 0x1F1C, 0x86E7, 0x229E, 0xF224, 0x0DA4, 0x8429, 0x2C29, 0x8529,
    0x2745, 0x87F6, 0x2C28, 0xF254, 0x383B, 0x8428, 0x320F, 0x8528, 0x320E, 0x8F02, 0x1341, 0xF264,
    0x3EB6, 0x8428, 0x3EB9, 0x8528, 0x383A, 0x8FA9, 0x3EB5, 0xF294, 0x3EB7, 0x8474, 0x3EBA, 0x8575,
    0x3EB8, 0xC4C3, 0x3EBB, 0xC5C3, 0x0000, 0xA404, 0x0001, 0xA504, 0x141F, 0x8671, 0x14FD, 0x8287,
    0x3EBC, 0xE610, 0x3EC8, 0x8C7B, 0x031A, 0x87E6, 0x3EC8, 0x86F7, 0x3EC0, 0x821E, 0x3EBE, 0xD280,
    0x3EBD, 0x821F, 0x3ECA, 0x8386, 0x3EC1, 0x8C03, 0x3EC9, 0x831E, 0x3ECA, 0x8C4C, 0x3EBF, 0x8C55,
    0x3EC9, 0xC280, 0x3EC4, 0xBC84, 0x3EC8, 0x8EAD, 0x3EC8, 0xD380, 0x3EC2, 0x8F7E, 0x3ECB, 0x8219,
    0x3ECB, 0xD2E6, 0x3EC5, 0x831F, 0x3EC6, 0xC380, 0x3EC3, 0xB27F, 0x3EC9, 0x8265, 0x3EC9, 0x8319,
    0x1342, 0xD3E6, 0x3EC7, 0xB37F, 0x0000, 0x8365, 0x1420, 0x9570,
];

/// Awe32InitArrays.h `kInit4`
pub const INIT4: [u16; 128] = [
    0x0C10, 0x8470, 0x14FE, 0xB488, 0x167F, 0xA470, 0x18E7, 0x84B5, 0x1B6E, 0x842A, 0x1F1D, 0x852A,
    0x0DA3, 0x0F7C, 0x167E, 0x7254, 0x0000, 0x842A, 0x0001, 0x852A, 0x18E6, 0x0BAA, 0x1B6D, 0x7234,
    0x229F, 0x8429, 0x2746, 0x8529, 0x1F1C, 0x06E7, 0x229E, 0x7224, 0x0DA4, 0x8429, 0x2C29, 0x8529,
    0x2745, 0x07F6, 0x2C28, 0x7254, 0x383B, 0x8428, 0x320F, 0x8528, 0x320E, 0x0F02, 0x1341, 0x7264,
    0x3EB6, 0x8428, 0x3EB9, 0x8528, 0x383A, 0x0FA9, 0x3EB5, 0x7294, 0x3EB7, 0x8474, 0x3EBA, 0x8575,
    0x3EB8, 0x44C3, 0x3EBB, 0x45C3, 0x0000, 0xA404, 0x0001, 0xA504, 0x141F, 0x0671, 0x14FD, 0x0287,
    0x3EBC, 0xE610, 0x3EC8, 0x0C7B, 0x031A, 0x07E6, 0x3EC8, 0x86F7, 0x3EC0, 0x821E, 0x3EBE, 0xD280,
    0x3EBD, 0x021F, 0x3ECA, 0x0386, 0x3EC1, 0x0C03, 0x3EC9, 0x031E, 0x3ECA, 0x8C4C, 0x3EBF, 0x0C55,
    0x3EC9, 0xC280, 0x3EC4, 0xBC84, 0x3EC8, 0x0EAD, 0x3EC8, 0xD380, 0x3EC2, 0x8F7E, 0x3ECB, 0x0219,
    0x3ECB, 0xD2E6, 0x3EC5, 0x031F, 0x3EC6, 0xC380, 0x3EC3, 0x327F, 0x3EC9, 0x0265, 0x3EC9, 0x8319,
    0x1342, 0xD3E6, 0x3EC7, 0x337F, 0x0000, 0x8365, 0x1420, 0x9570,
];

// ---------------------------------------------------------------------------
// Awe32Curves.h
// ---------------------------------------------------------------------------

/// Awe32Curves.h:47 (MDI 0x142D) - expression (CC11)
pub const EXPRESSION_DB: [u8; 128] = [
    127, 108, 98, 90, 84, 80, 75, 72, 69, 66, 64, 61, 59, 57, 56, 54, 52, 51, 49, 48, 47, 45, 44,
    43, 42, 41, 40, 39, 38, 37, 36, 36, 35, 34, 33, 33, 32, 31, 30, 30, 29, 29, 28, 27, 27, 26, 26,
    25, 24, 24, 23, 23, 22, 22, 21, 21, 21, 20, 20, 19, 19, 18, 18, 17, 17, 17, 16, 16, 15, 15, 15,
    14, 14, 14, 13, 13, 13, 12, 12, 12, 11, 11, 11, 10, 10, 10, 9, 9, 9, 9, 8, 8, 8, 7, 7, 7, 7, 6,
    6, 6, 6, 5, 5, 5, 4, 4, 4, 4, 4, 3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0,
];

/// Awe32Curves.h:59 (MDI 0x14AD) - channel volume (CC7)
pub const CHANNEL_VOLUME_DB: [u8; 128] = [
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 43, 41, 40, 39, 38, 37, 36, 35, 34, 33, 32, 31, 30,
    30, 29, 28, 27, 27, 26, 25, 25, 24, 23, 23, 22, 22, 21, 21, 20, 20, 19, 19, 19, 18, 18, 17, 17,
    17, 16, 16, 16, 15, 15, 15, 14, 14, 14, 14, 13, 13, 13, 12, 12, 12, 12, 12, 11, 11, 11, 11, 10,
    10, 10, 10, 9, 9, 9, 9, 9, 8, 8, 8, 8, 8, 7, 7, 7, 7, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5, 4, 4, 4, 4,
    4, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0,
];

/// Awe32Curves.h:71 (MDI 0x152D) - velocity
pub const VELOCITY_DB: [u8; 128] = [
    50, 49, 48, 47, 46, 45, 44, 43, 42, 42, 41, 40, 39, 38, 37, 36, 36, 35, 34, 33, 33, 32, 31, 30,
    30, 29, 28, 28, 27, 26, 26, 25, 25, 24, 24, 23, 22, 22, 21, 21, 20, 20, 19, 19, 19, 18, 18, 17,
    17, 16, 16, 16, 15, 15, 15, 14, 14, 14, 13, 13, 13, 12, 12, 12, 11, 11, 11, 11, 10, 10, 10, 10,
    9, 9, 9, 9, 9, 8, 8, 8, 8, 8, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5, 5, 5, 4, 4, 4, 4,
    4, 4, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0,
];

/// Awe32Curves.h:89 `VelocityDb` (the win95 exception is not ported).
#[inline]
pub fn velocity_db(velocity: i32, _drv: Driver) -> i32 {
    VELOCITY_DB[velocity.clamp(0, 127) as usize] as i32
}

/// Awe32Curves.h:97 `ComputeAttenuationMdi`
fn compute_attenuation_mdi(
    cc7: i32,
    velocity: i32,
    expression: i32,
    patch_atten_units: i32,
) -> i32 {
    let db = CHANNEL_VOLUME_DB[cc7 as usize] as i32 + velocity_db(velocity, Driver::Dos);
    let patch = (3 * patch_atten_units.clamp(0, 255)) & !7;
    let mut atten = (8 * db + patch) / 3;
    if atten >= 255 {
        return 255;
    }
    if expression < 127 {
        atten += EXPRESSION_DB[expression as usize] as i32 * (255 - atten) / 127;
    }
    atten.clamp(0, 255)
}

/// Awe32Curves.h:166 `ComputeAttenuationSdk`
fn compute_attenuation_sdk(
    cc7: i32,
    velocity: i32,
    expression: i32,
    patch_atten_units: i32,
) -> i32 {
    let db = CHANNEL_VOLUME_DB[cc7 as usize] as i32
        + velocity_db(velocity, Driver::Sdk)
        + patch_atten_units.clamp(0, 255) * 25 / 80;
    let mut atten = (db * 8) / 3;
    if atten > 255 {
        return 255;
    }
    if expression < 127 {
        atten += (EXPRESSION_DB[expression as usize] as i32 * (256 - atten)) >> 7;
    }
    atten.clamp(0, 255)
}

/// Awe32Curves.h:180 `ComputeAttenuation`
pub fn compute_attenuation(
    cc7: i32,
    velocity: i32,
    expression: i32,
    patch_atten_units: i32,
    drv: Driver,
) -> i32 {
    let cc7 = cc7.clamp(0, 127);
    let velocity = velocity.clamp(0, 127);
    let expression = expression.clamp(0, 127);
    if cc7 <= 10 {
        return 255;
    }
    match drv {
        Driver::Sdk => compute_attenuation_sdk(cc7, velocity, expression, patch_atten_units),
        Driver::Dos => compute_attenuation_mdi(cc7, velocity, expression, patch_atten_units),
    }
}

/// Awe32Curves.h:217 `kAttenToAmp16`
const ATTEN_TO_AMP16: [u16; 16] = [
    60096, 57544, 55104, 52768, 50528, 48392, 46336, 44376, 42488, 40688, 38960, 37312, 35728,
    34216, 32768, 31376,
];
/// Awe32Curves.h:227 `kAttenBeforeTable`
const ATTEN_BEFORE_TABLE: [u16; 15] = [
    1542, 1286, 1285, 1028, 1028, 772, 771, 515, 514, 258, 257, 257, 0, 0, 0,
];

/// Awe32Curves.h:247 `VolumeTarget`
pub fn volume_target(atten_units: i32) -> u16 {
    let index = atten_units % 16;
    let shift = atten_units / 16;
    let base = if index >= 0 {
        ATTEN_TO_AMP16[index as usize]
    } else {
        ATTEN_BEFORE_TABLE[(15 + index) as usize]
    };
    let count = (shift as u8 as u32) & 31;
    if count >= 16 { 0 } else { base >> count }
}
