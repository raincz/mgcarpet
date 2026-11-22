//! `Emu8000Core` (Emu8000.cpp / Emu8000.h) - only what the Box86 chip path
//! uses: the port/register write layer (pointer + data port, as a driver
//! does it), `ReadDriver`, `PowerOnInit`, the DRAM image the banks are
//! loaded into, the driver variant and the port-write trace.
//!
//! Left out, because it never reaches the PCM of the Box86 path:
//! the old "ours" software core (`RenderVoice`, `RenderNative`,
//! `Emu8000Effects.h`, the register mirror it keeps and
//! `UpdateRegistersFromState`). In the C++ it keeps running idle next to
//! the 86Box chip, but the `dos`/`sdk` driver families only read the chip
//! through `ReadDriver` (which goes to the 86Box chip), never the mirror,
//! so its state is unobservable here.

use crate::chip_box::{Emu8000Box, LATENCY_FRAMES};
use crate::driver::*;
use std::fmt::Write as _;

/// Emu8000Core, Box86 variant.
pub struct Emu8000Core {
    base_port: u16,
    driver: Driver,
    chip: Emu8000Box,
    /// Emu8000.h `m_dram`: the DRAM image the banks are uploaded into; it is
    /// copied into the chip before the first note (main.cpp:681).
    pub dram: Vec<i16>,
    trace: Option<String>,
    trace_frames: u64,
}

impl Emu8000Core {
    /// `Emu8000Core` + `UseBox86Chip` (Emu8000.cpp:1474). The constructor's
    /// `PowerOnInit` of the C++ runs before the chip exists, so it reaches
    /// nothing and is not repeated here; call [`Self::power_on_init`] where
    /// main.cpp does.
    pub fn new(rom: &[u8], ram_kb: i32, driver: Driver) -> Result<Self, String> {
        Ok(Emu8000Core {
            base_port: 0x220,
            driver,
            chip: Emu8000Box::new(rom, ram_kb)?,
            dram: Vec::new(),
            trace: None,
            trace_frames: 0,
        })
    }

    pub fn driver_variant(&self) -> Driver {
        self.driver
    }

    /// `ChipLatencyFrames`, Emu8000.cpp:1483
    pub fn chip_latency_frames(&self) -> u32 {
        LATENCY_FRAMES as u32
    }

    /// Copies the DRAM image into the chip (main.cpp:681-688).
    pub fn copy_dram_to_chip(&mut self) -> usize {
        let ram = self.chip.ram();
        let n = self.dram.len().min(ram.len());
        ram[..n].copy_from_slice(&self.dram[..n]);
        n
    }

    /// `OpenTrace`, Emu8000.cpp:220 (into memory).
    pub fn open_trace(&mut self) {
        let mut s = String::new();
        s.push_str("# EMU8000 port write trace, 44100 Hz timebase\n");
        s.push_str("# <frame> <port hex> <value hex>\n");
        self.trace = Some(s);
        self.trace_frames = 0;
    }

    pub fn take_trace(&mut self) -> Option<String> {
        self.trace.take()
    }

    /// `PortOut16`, Emu8000.cpp:241 (Box86 part).
    pub fn port_out16(&mut self, port: u16, value: u16) {
        if let Some(t) = &mut self.trace {
            let _ = writeln!(t, "{} {:03X} {:04X}", self.trace_frames, port, value);
        }
        self.chip.port_write(port, value, false);
    }

    /// `PortIn16`, Emu8000.cpp:432 -> `PortIn16Raw` (Box86).
    pub fn port_in16(&mut self, port: u16) -> u16 {
        let value = self.chip.port_read(port);
        if let Some(t) = &mut self.trace {
            let _ = writeln!(t, "R {} {:03X} {:04X}", self.trace_frames, port, value);
        }
        value
    }

    fn port_from_sel(sel: u16) -> Option<Port> {
        let port_sel = (sel >> 9) & 7;
        if !(2..=6).contains(&port_sel) {
            return None;
        }
        Some(match port_sel - 2 {
            0 => Port::Data0,
            1 => Port::Data0Hi,
            2 => Port::Data1,
            3 => Port::Data1Hi,
            _ => Port::Data3,
        })
    }

    fn port_offset(p: Port) -> u16 {
        match p {
            Port::Data0 => PORT_DATA0,
            Port::Data0Hi => PORT_DATA0_HI,
            Port::Data1 => PORT_DATA1,
            Port::Data1Hi => PORT_DATA1_HI,
            Port::Data3 => PORT_DATA3,
        }
    }

    /// `WriteReg16`, Emu8000.cpp:512
    pub fn write_reg16(&mut self, sel: u16, value: u16) {
        let Some(p) = Self::port_from_sel(sel) else {
            return;
        };
        self.port_out16(self.base_port + PORT_POINTER, sel_to_pointer(sel));
        self.port_out16(self.base_port + Self::port_offset(p), value);
    }

    /// `WriteReg32`, Emu8000.cpp:533
    pub fn write_reg32(&mut self, sel: u16, value: u32) {
        let Some(p) = Self::port_from_sel(sel) else {
            return;
        };
        let hi = match p {
            Port::Data0 => Port::Data0Hi,
            Port::Data1 => Port::Data1Hi,
            _ => {
                self.write_reg16(sel, value as u16);
                return;
            }
        };
        self.port_out16(self.base_port + PORT_POINTER, sel_to_pointer(sel));
        self.port_out16(
            self.base_port + Self::port_offset(p),
            (value & 0xFFFF) as u16,
        );
        self.port_out16(self.base_port + Self::port_offset(hi), (value >> 16) as u16);
    }

    /// `IsReg32`, Emu8000.cpp:581
    fn is_reg32(sel: u16) -> bool {
        let port_sel = (sel >> 9) & 7;
        if port_sel == 2 {
            return true;
        }
        if port_sel == 4 {
            return ((sel >> 12) & 7) <= 1;
        }
        false
    }

    /// `Write`, Emu8000.cpp:590
    pub fn write(&mut self, r: Reg, voice: usize, value: u32) {
        let s = sel(r, voice);
        if Self::is_reg32(s) {
            self.write_reg32(s, value);
        } else {
            self.write_reg16(s, value as u16);
        }
    }

    /// `ReadDriver`, Emu8000.cpp:604
    pub fn read_driver(&mut self, r: Reg, voice: usize) -> u32 {
        let s = sel(r, voice);
        let Some(p) = Self::port_from_sel(s) else {
            return 0;
        };
        self.port_out16(self.base_port + PORT_POINTER, sel_to_pointer(s));
        let lo = self.port_in16(self.base_port + Self::port_offset(p));
        if !Self::is_reg32(s) {
            return lo as u32;
        }
        let hi = if p == Port::Data0 {
            Port::Data0Hi
        } else {
            Port::Data1Hi
        };
        let hw = self.port_in16(self.base_port + Self::port_offset(hi));
        ((hw as u32) << 16) | lo as u32
    }

    /// `SendInitArray`, Emu8000.cpp:626 (the Win95 alternative values do not
    /// apply to the dos families).
    fn send_init_array(&mut self, data: &[u16; 128]) {
        for v in 0..32 {
            self.write(Reg::INIT1, v, data[v] as u32);
        }
        for v in 0..32 {
            self.write(Reg::INIT2, v, data[32 + v] as u32);
        }
        for v in 0..32 {
            self.write(Reg::INIT3, v, data[64 + v] as u32);
        }
        for v in 0..32 {
            self.write(Reg::INIT4, v, data[96 + v] as u32);
        }
    }

    /// `PowerOnInit`, Emu8000.cpp:641 (AWEUTIL.COM sequence + SBAWE32.MDI).
    pub fn power_on_init(&mut self) {
        self.write_reg16(make_sel(1, Port::Data1, hwcf::HWCF1), 0x0059);
        self.write_reg16(make_sel(1, Port::Data1, hwcf::HWCF2), 0x0020);
        self.write_reg16(make_sel(1, Port::Data1, hwcf::HWCF3), 0x0004);

        for v in 0..32 {
            self.write(Reg::DCYSUSV, v, DCYSUSV_OFF);
            self.write(Reg::ATKHLD, v, 0);
            self.write(Reg::DCYSUS, v, 0);
            self.write(Reg::IP, v, 0);
            self.write(Reg::IFATN, v, 0xFF00);
            self.write(Reg::PEFE, v, 0);
            self.write(Reg::FMMOD, v, 0);
            self.write(Reg::TREMFRQ, v, 0x0018);
            self.write(Reg::FM2FRQ2, v, 0x0018);
            self.write(Reg::Unk6C, v, 0);
            self.write(Reg::LFO2VAL, v, 0);
            self.write(Reg::LFO1VAL, v, 0);
            self.write(Reg::ATKHLDV, v, 0);
            self.write(Reg::ENVVOL, v, 0);
            self.write(Reg::ENVVAL, v, 0);
        }

        for v in 0..32 {
            self.write(Reg::PTRX, v, 0);
            self.write(Reg::VTFT, v, 0x0000_FFFF);
            self.write(Reg::PSST, v, 0);
            self.write(Reg::CSL, v, 0);
            self.write(Reg::CPF, v, 0);
            self.write(Reg::CVCF, v, 0x0000_FFFF);
            self.write(Reg::CCCA, v, 0);
            self.write(Reg::Unk0088, v, 0);
            self.write(Reg::Unk0080, v, 0);
        }

        self.write_reg16(make_sel(1, Port::Data1, hwcf::SMALR), 0);
        self.write_reg16(make_sel(1, Port::Data1, hwcf::SMARR), 0);
        self.write_reg16(make_sel(1, Port::Data1, hwcf::SMALW), 0);
        self.write_reg16(make_sel(1, Port::Data1, hwcf::SMARR), 0);

        self.send_init_array(&INIT1);
        self.send_init_array(&INIT2);
        self.send_init_array(&INIT3);

        self.write_reg32(make_sel(1, Port::Data1, hwcf::HWCF4), 0x0000_0000);
        self.write_reg32(make_sel(1, Port::Data1, hwcf::HWCF5), 0x0000_0083);
        self.write_reg32(make_sel(1, Port::Data1, hwcf::HWCF6), 0x0000_8000);
        self.write_reg32(make_sel(1, Port::Data1, hwcf::HWCF7), 0x0000_0000);

        self.send_init_array(&INIT4);

        self.write(Reg::PSST, 30, 0xFFFF_FFE0);
        self.write(Reg::CSL, 30, 0x00FF_FFE8);
        self.write(Reg::PTRX, 30, 0x0000_0000);
        self.write(Reg::CPF, 30, 0x0000_0000);
        self.write(Reg::CCCA, 30, 0x00FF_FFE3);
        self.write(Reg::PSST, 31, 0x00FF_FFF0);
        self.write(Reg::CSL, 31, 0x00FF_FFF8);
        self.write(Reg::PTRX, 31, 0x0000_00FF);
        self.write(Reg::CPF, 31, 0x0000_8000);
        self.write(Reg::CCCA, 31, 0x00FF_FFF3);

        self.write(Reg::PTRX, 30, 0x4828_0000);
        self.write_reg16(make_sel(1, Port::Data1, 28), 0x0000);

        self.write(Reg::VTFT, 30, 0x0000_FFFF);
        self.write(Reg::VTFT, 31, 0x0000_FFFF);

        // step 9: dos families end with HWCF3 = 4
        self.write_reg16(make_sel(1, Port::Data1, hwcf::HWCF3), 0x0004);

        if is_dos_like(self.driver) {
            for v in 0..32 {
                self.write(Reg::DCYSUS, v, 0x807F);
                self.write(Reg::DCYSUSV, v, 0x807F);
            }
        }
    }

    /// `RenderBlock`, Emu8000.cpp:1498 (Box86, native rate): out =
    /// interleaved stereo, `numFrames = out.len() / 2`.
    pub fn render_block(&mut self, out: &mut [i16]) {
        let frames = out.len() / 2;
        for i in 0..frames {
            let (l, r) = self.chip.render_frame();
            out[i * 2] = l.clamp(-32768, 32767) as i16;
            out[i * 2 + 1] = r.clamp(-32768, 32767) as i16;
        }
        // RenderNative: m_traceFrames += numFrames
        self.trace_frames += frames as u64;
    }
}
