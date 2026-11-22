//! `Emu8000Box` - the wrapper around the 86Box chip with the one-block
//! latency design (`Emu8000Box.cpp` / `Emu8000Box.h`).
//!
//! 86Box drives the chip in blocks of `WTBUFLEN` = 980 frames: every port
//! write is applied at its own offset in the block, then one update renders
//! the block. The box queues the writes of the current block and outputs the
//! previous, finished block - a constant latency of exactly 980 frames.

use crate::chip::{Emu8k, WTBUFLEN};

/// Emu8000Box.h:30 `kBlockFrames`
pub const BLOCK_FRAMES: usize = WTBUFLEN;
/// Emu8000Box.h:31 `kLatencyFrames`
pub const LATENCY_FRAMES: usize = BLOCK_FRAMES;

#[derive(Clone, Copy)]
struct Write {
    offset: i32,
    port: u16,
    value: u16,
    is_byte: bool,
}

/// `Emu8000Box` (Emu8000Box.cpp:139 `Impl`).
pub struct Emu8000Box {
    emu: Box<Emu8k>,
    pending: Vec<Write>,
    fifo: Vec<i32>,
    fifo_pos: usize,
    frame_in_block: i32,
    have_block: bool,
}

impl Emu8000Box {
    /// `Emu8000Box::Init` (Emu8000Box.cpp:166): `rom` = the raw 1 MB wave
    /// ROM dump (awe32.raw), `ram_kb` = onboard DRAM (AWE32Emu default 8192).
    pub fn new(rom: &[u8], ram_kb: i32) -> Result<Self, String> {
        let emu = Emu8k::new(rom, ram_kb)?;
        Ok(Emu8000Box {
            emu,
            pending: Vec::new(),
            fifo: Vec::new(),
            fifo_pos: 0,
            frame_in_block: 0,
            have_block: false,
        })
    }

    /// Sound DRAM of the chip (`Ram()`, Emu8000Box.cpp:210), from 0x200000.
    pub fn ram(&mut self) -> &mut [i16] {
        let n = self.ram_words();
        &mut self.emu.ram_mut()[..n]
    }

    /// `RamWords()`, Emu8000Box.cpp:215
    pub fn ram_words(&self) -> usize {
        self.emu.ram_words()
    }

    /// `PortWrite`, Emu8000Box.cpp:222
    pub fn port_write(&mut self, port: u16, value: u16, is_byte: bool) {
        self.pending.push(Write {
            offset: self.frame_in_block,
            port,
            value,
            is_byte,
        });
    }

    fn apply_pending(&mut self) {
        for w in self.pending.drain(..) {
            if w.is_byte {
                self.emu.outb(w.port, w.value as u8, w.offset);
            } else {
                self.emu.outw(w.port, w.value, w.offset);
            }
        }
    }

    /// `PortRead`, Emu8000Box.cpp:229: the queued writes of this block are
    /// applied, the chip is run up to the current frame, then read.
    pub fn port_read(&mut self, port: u16) -> u16 {
        self.apply_pending();
        self.emu.update(self.frame_in_block);
        self.emu.inw(port)
    }

    /// `FlushBlock`, Emu8000Box.cpp:254
    fn flush_block(&mut self) {
        self.apply_pending();
        self.emu.update(BLOCK_FRAMES as i32);
        self.fifo.clear();
        self.fifo
            .extend_from_slice(&self.emu.buffer[..BLOCK_FRAMES * 2]);
        self.fifo_pos = 0;
        self.have_block = true;
        self.emu.reset_buffer();
    }

    /// `RenderFrame`, Emu8000Box.cpp:276: one frame at 44100 Hz; the first
    /// `LATENCY_FRAMES` calls return silence.
    pub fn render_frame(&mut self) -> (i32, i32) {
        let (l, r) = if self.have_block && self.fifo_pos + 1 < self.fifo.len() {
            let l = self.fifo[self.fifo_pos];
            let r = self.fifo[self.fifo_pos + 1];
            self.fifo_pos += 2;
            (l, r)
        } else {
            (0, 0)
        };
        self.frame_in_block += 1;
        if self.frame_in_block == BLOCK_FRAMES as i32 {
            self.flush_block();
            self.frame_in_block = 0;
        }
        (l, r)
    }
}
