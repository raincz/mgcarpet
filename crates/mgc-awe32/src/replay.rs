//! Port-write trace replay (`--replay`, main.cpp:737-796).

use crate::core::Emu8000Core;
use crate::driver::Driver;

/// One port write of a trace (`<frame> <port hex> <value hex>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceWrite {
    pub frame: u64,
    pub port: u16,
    pub value: u16,
}

/// Parses a trace as main.cpp:750-765 does: `#` and `R` lines are skipped,
/// byte writes (`... b`) are counted and skipped.
pub fn parse_trace(text: &str) -> (Vec<TraceWrite>, usize) {
    let mut evs = Vec::new();
    let mut byte_writes = 0;
    for line in text.lines() {
        let q = line.trim_start_matches([' ', '\t']);
        if q.is_empty() || q.starts_with('#') || q.starts_with('R') || q.starts_with('\r') {
            continue;
        }
        let mut it = q.split_whitespace();
        let (Some(t), Some(p), Some(v)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let (Ok(t), Ok(p), Ok(v)) = (
            t.parse::<u64>(),
            u32::from_str_radix(p, 16),
            u32::from_str_radix(v, 16),
        ) else {
            continue;
        };
        if let Some(w) = it.next() {
            if w.starts_with('b') || w.starts_with('B') {
                byte_writes += 1;
                continue;
            }
        }
        evs.push(TraceWrite {
            frame: t,
            port: p as u16,
            value: v as u16,
        });
    }
    (evs, byte_writes)
}

/// Replays port writes through the chip the way `AWE32Emu --replay` does:
/// the DRAM image is copied in first (the C++ gets it from the `--sf`
/// banks), no `PowerOnInit` (the trace carries its own), blocks of up to
/// 1024 frames between the writes, a tail of 2 s + latency, and the first
/// 980 frames (the chip latency) cut. Returns interleaved stereo i16.
pub fn replay(
    rom: &[u8],
    dram: &[i16],
    writes: &[TraceWrite],
    ram_kb: i32,
) -> Result<Vec<i16>, String> {
    const FRAMES_PER_BUFFER: u64 = 1024;
    let mut core = Emu8000Core::new(rom, ram_kb, Driver::Dos)?;
    core.dram = dram.to_vec();
    core.copy_dram_to_chip();

    let mut out = Vec::new();
    let mut buf = vec![0i16; FRAMES_PER_BUFFER as usize * 2];
    let mut skip = core.chip_latency_frames() as u64;
    let mut emit = |core: &mut Emu8000Core, frames: u64, out: &mut Vec<i16>| {
        let b = &mut buf[..frames as usize * 2];
        core.render_block(b);
        if skip >= frames {
            skip -= frames;
            return;
        }
        out.extend_from_slice(&b[skip as usize * 2..]);
        skip = 0;
    };
    let mut cur = 0u64;
    for ev in writes {
        while cur < ev.frame {
            let n = FRAMES_PER_BUFFER.min(ev.frame - cur);
            emit(&mut core, n, &mut out);
            cur += n;
        }
        core.port_out16(ev.port, ev.value);
    }
    let tail = 2 * 44100 + core.chip_latency_frames() as u64;
    let mut done = 0;
    while done < tail {
        emit(&mut core, FRAMES_PER_BUFFER, &mut out);
        done += FRAMES_PER_BUFFER;
    }
    Ok(out)
}
