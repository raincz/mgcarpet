//! Score input and timing: `MidiTypes.h`, `MidiFile.cpp` (SMF),
//! `XmiFile.cpp` (XMI) and `Sequencer.cpp`.
//!
//! XMI keeps AWE32Emu's (and AIL's) fixed clock: 60 ticks per quarter at a
//! constant 120 BPM = 120 ticks per second; `FF 51` tempo metas inside an
//! XMI are ignored (XmiFile.cpp:128-136).

use crate::synth::Synth;

/// MidiTypes.h:9
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiEventType {
    NoteOff,
    NoteOn,
    PolyPressure,
    ControlChange,
    ProgramChange,
    ChannelPressure,
    PitchBend,
    TempoChange,
    EndOfTrack,
}

/// MidiTypes.h:22
#[derive(Clone, Copy, Debug)]
pub struct MidiEvent {
    pub absolute_tick: u32,
    pub ty: MidiEventType,
    pub channel: u8,
    pub data1: u8,
    pub data2: u8,
    pub tempo_us_per_quarter: u32,
}

impl MidiEvent {
    fn new(absolute_tick: u32, ty: MidiEventType) -> Self {
        MidiEvent {
            absolute_tick,
            ty,
            channel: 0,
            data1: 0,
            data2: 0,
            tempo_us_per_quarter: 500000,
        }
    }
}

/// MidiTypes.h:34 `ParsedSequence`
#[derive(Clone, Debug)]
pub struct ParsedSequence {
    pub events: Vec<MidiEvent>,
    pub ticks_per_quarter_note: u16,
}

fn read_be32(p: &[u8]) -> u32 {
    ((p[0] as u32) << 24) | ((p[1] as u32) << 16) | ((p[2] as u32) << 8) | p[3] as u32
}

/// SMF VLQ (MidiFile.cpp:13 / XmiFile.cpp:31).
fn read_vlq(data: &[u8], pos: &mut usize) -> u32 {
    let mut value = 0u32;
    let mut i = 0;
    while i < 4 && *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        value = (value << 7) | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            break;
        }
        i += 1;
    }
    value
}

/// XmiFile.cpp:13 `ReadXmiInterval`
fn read_xmi_interval(data: &[u8], pos: &mut usize) -> u32 {
    let mut value = 0u32;
    while *pos < data.len() && data[*pos] < 0x80 {
        let b = data[*pos];
        value += b as u32;
        *pos += 1;
        if b != 0x7F {
            break;
        }
    }
    value
}

/// XmiFile.cpp:49 `FindChunk`: offset right after the first `tag`.
fn find_chunk(buf: &[u8], tag: &[u8; 4]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    (0..=buf.len() - 4)
        .find(|&i| &buf[i..i + 4] == tag)
        .map(|i| i + 4)
}

/// `XmiFile::Load` (XmiFile.cpp:62) on bytes. Takes the first `EVNT` chunk.
pub fn parse_xmi(buffer: &[u8]) -> Result<ParsedSequence, String> {
    if buffer.len() < 12 || &buffer[..4] != b"FORM" {
        return Err("Missing FORM header - not a valid XMI/IFF file".into());
    }
    let evnt_len_pos = find_chunk(buffer, b"EVNT")
        .ok_or("No EVNT chunk - not a valid XMI file (or an unsupported variant)")?;
    let mut pos = evnt_len_pos;
    if pos + 4 > buffer.len() {
        return Err("Damaged EVNT chunk (length missing)".into());
    }
    let evnt_len = read_be32(&buffer[pos..]);
    pos += 4;
    let evnt_end = (pos + evnt_len as usize).min(buffer.len());
    let data = &buffer[pos..evnt_end];

    let mut events = vec![MidiEvent::new(0, MidiEventType::TempoChange)];
    let mut p = 0usize;
    let mut absolute_tick = 0u32;
    while p < data.len() {
        absolute_tick = absolute_tick.wrapping_add(read_xmi_interval(data, &mut p));
        if p >= data.len() {
            break;
        }
        let status = data[p];
        if status < 0x80 {
            break;
        }
        p += 1;
        let hi = status & 0xF0;
        let channel = status & 0x0F;
        if status == 0xFF {
            if p >= data.len() {
                break;
            }
            let meta_type = data[p];
            p += 1;
            let len = read_vlq(data, &mut p);
            if meta_type == 0x2F {
                events.push(MidiEvent::new(absolute_tick, MidiEventType::EndOfTrack));
            }
            p += len as usize;
        } else if status == 0xF0 || status == 0xF7 {
            let len = read_vlq(data, &mut p);
            p += len as usize;
        } else if hi == 0x90 {
            if p + 2 > data.len() {
                break;
            }
            let note = data[p];
            let velocity = data[p + 1];
            p += 2;
            let duration = read_vlq(data, &mut p);
            let mut on = MidiEvent::new(absolute_tick, MidiEventType::NoteOn);
            on.channel = channel;
            on.data1 = note;
            on.data2 = velocity;
            events.push(on);
            let mut off =
                MidiEvent::new(absolute_tick.wrapping_add(duration), MidiEventType::NoteOff);
            off.channel = channel;
            off.data1 = note;
            events.push(off);
        } else if hi == 0x80 || hi == 0xA0 || hi == 0xB0 || hi == 0xE0 {
            if p + 2 > data.len() {
                break;
            }
            let ty = match hi {
                0x80 => MidiEventType::NoteOff,
                0xA0 => MidiEventType::PolyPressure,
                0xB0 => MidiEventType::ControlChange,
                _ => MidiEventType::PitchBend,
            };
            let mut ev = MidiEvent::new(absolute_tick, ty);
            ev.channel = channel;
            ev.data1 = data[p];
            ev.data2 = data[p + 1];
            p += 2;
            events.push(ev);
        } else if hi == 0xC0 || hi == 0xD0 {
            if p + 1 > data.len() {
                break;
            }
            let ty = if hi == 0xC0 {
                MidiEventType::ProgramChange
            } else {
                MidiEventType::ChannelPressure
            };
            let mut ev = MidiEvent::new(absolute_tick, ty);
            ev.channel = channel;
            ev.data1 = data[p];
            p += 1;
            events.push(ev);
        } else {
            break;
        }
    }
    // std::stable_sort by absoluteTick == Vec::sort_by_key (stable)
    events.sort_by_key(|e| e.absolute_tick);
    Ok(ParsedSequence {
        events,
        ticks_per_quarter_note: 60,
    })
}

/// MidiFile.cpp:31 `ParseTrack`
fn parse_track(data: &[u8], out: &mut Vec<MidiEvent>) {
    let mut pos = 0usize;
    let mut absolute_tick = 0u32;
    let mut running_status = 0u8;
    while pos < data.len() {
        let delta = read_vlq(data, &mut pos);
        absolute_tick = absolute_tick.wrapping_add(delta);
        if pos >= data.len() {
            break;
        }
        let mut status = data[pos];
        if status < 0x80 {
            status = running_status;
        } else {
            pos += 1;
            running_status = status;
        }
        let hi = status & 0xF0;
        let channel = status & 0x0F;
        if status == 0xFF {
            if pos >= data.len() {
                break;
            }
            let meta_type = data[pos];
            pos += 1;
            let len = read_vlq(data, &mut pos);
            if meta_type == 0x51 && len == 3 && pos + 3 <= data.len() {
                let us = ((data[pos] as u32) << 16)
                    | ((data[pos + 1] as u32) << 8)
                    | data[pos + 2] as u32;
                let mut ev = MidiEvent::new(absolute_tick, MidiEventType::TempoChange);
                ev.tempo_us_per_quarter = us;
                out.push(ev);
            } else if meta_type == 0x2F {
                out.push(MidiEvent::new(absolute_tick, MidiEventType::EndOfTrack));
            }
            pos += len as usize;
        } else if status == 0xF0 || status == 0xF7 {
            let len = read_vlq(data, &mut pos);
            pos += len as usize;
        } else if hi == 0x80 || hi == 0x90 || hi == 0xA0 || hi == 0xB0 || hi == 0xE0 {
            if pos + 2 > data.len() {
                break;
            }
            let d1 = data[pos];
            let d2 = data[pos + 1];
            pos += 2;
            let ty = match hi {
                0x80 => MidiEventType::NoteOff,
                0x90 => {
                    if d2 == 0 {
                        MidiEventType::NoteOff
                    } else {
                        MidiEventType::NoteOn
                    }
                }
                0xA0 => MidiEventType::PolyPressure,
                0xB0 => MidiEventType::ControlChange,
                _ => MidiEventType::PitchBend,
            };
            let mut ev = MidiEvent::new(absolute_tick, ty);
            ev.channel = channel;
            ev.data1 = d1;
            ev.data2 = d2;
            out.push(ev);
        } else if hi == 0xC0 || hi == 0xD0 {
            if pos + 1 > data.len() {
                break;
            }
            let ty = if hi == 0xC0 {
                MidiEventType::ProgramChange
            } else {
                MidiEventType::ChannelPressure
            };
            let mut ev = MidiEvent::new(absolute_tick, ty);
            ev.channel = channel;
            ev.data1 = data[pos];
            pos += 1;
            out.push(ev);
        } else {
            break;
        }
    }
}

/// `MidiFile::Load` (MidiFile.cpp:149) on bytes - SMF format 0/1.
pub fn parse_smf(buffer: &[u8]) -> Result<ParsedSequence, String> {
    if buffer.len() < 14 || &buffer[..4] != b"MThd" {
        return Err("Missing MThd header - not a valid SMF file".into());
    }
    let header_len = read_be32(&buffer[4..]);
    let num_tracks = ((buffer[10] as u16) << 8) | buffer[11] as u16;
    let division = ((buffer[12] as u16) << 8) | buffer[13] as u16;
    if division & 0x8000 != 0 {
        return Err("SMPTE division is not supported yet".into());
    }
    let mut pos = 8 + header_len as usize;
    let mut merged = Vec::new();
    let mut t = 0;
    while t < num_tracks && pos + 8 <= buffer.len() {
        if &buffer[pos..pos + 4] != b"MTrk" {
            return Err(format!("Expected MTrk chunk not found (track {})", t));
        }
        let track_len = read_be32(&buffer[pos + 4..]);
        let track_start = pos + 8;
        let track_end = track_start + track_len as usize;
        if track_end > buffer.len() {
            return Err(format!("Damaged track length {}", t));
        }
        parse_track(&buffer[track_start..track_end], &mut merged);
        pos = track_end;
        t += 1;
    }
    merged.sort_by_key(|e| e.absolute_tick);
    Ok(ParsedSequence {
        events: merged,
        ticks_per_quarter_note: division,
    })
}

/// `Sequencer` (Sequencer.cpp).
pub struct Sequencer {
    seq: ParsedSequence,
    next_event_index: usize,
    current_tick: f64,
    current_tempo_us: u32,
    trigger_mute: bool,
}

impl Sequencer {
    /// `Load`, Sequencer.cpp:3
    pub fn new(seq: ParsedSequence) -> Self {
        Sequencer {
            seq,
            next_event_index: 0,
            current_tick: 0.0,
            current_tempo_us: 500000,
            trigger_mute: false,
        }
    }

    /// Sequencer.h:30 `SetTriggerMute` - MC2's CC119 callback.
    pub fn set_trigger_mute(&mut self, on: bool) {
        self.trigger_mute = on;
    }

    /// Sequencer.cpp:11
    pub fn has_more_events(&self) -> bool {
        self.next_event_index < self.seq.events.len()
    }

    /// Sequencer.cpp:16
    fn ticks_per_second(&self) -> f64 {
        let quarters_per_second = 1000000.0 / self.current_tempo_us as f64;
        quarters_per_second * self.seq.ticks_per_quarter_note as f64
    }

    /// Sequencer.cpp:22
    fn dispatch_event(&mut self, synth: &mut Synth, ev: MidiEvent) {
        match ev.ty {
            MidiEventType::NoteOn => synth.note_on(ev.channel, ev.data1, ev.data2),
            MidiEventType::NoteOff => synth.note_off(ev.channel, ev.data1),
            MidiEventType::ProgramChange => synth.program_change(ev.channel, ev.data1),
            MidiEventType::ControlChange => {
                synth.control_change(ev.channel, ev.data1, ev.data2);
                if self.trigger_mute && ev.data1 == 119 && ev.data2 == 0 {
                    synth.control_change(ev.channel, 11, 0);
                }
            }
            MidiEventType::PitchBend => {
                let bend = ((((ev.data2 as i32) << 7) | ev.data1 as i32) - 8192) as i16;
                synth.pitch_bend(ev.channel, bend);
            }
            MidiEventType::TempoChange => self.current_tempo_us = ev.tempo_us_per_quarter,
            MidiEventType::ChannelPressure => synth.channel_pressure(ev.channel, ev.data1),
            MidiEventType::PolyPressure | MidiEventType::EndOfTrack => {}
        }
    }

    /// `RenderBlock`, Sequencer.cpp:60: events are dispatched before each
    /// frame, one frame is rendered, the tick advances.
    pub fn render_block(&mut self, synth: &mut Synth, out: &mut [i16], sample_rate: u32) {
        let frames = out.len() / 2;
        for frame in 0..frames {
            while self.next_event_index < self.seq.events.len()
                && self.seq.events[self.next_event_index].absolute_tick as f64 <= self.current_tick
            {
                let ev = self.seq.events[self.next_event_index];
                self.dispatch_event(synth, ev);
                self.next_event_index += 1;
            }
            synth.render_block(&mut out[frame * 2..frame * 2 + 2]);
            let ticks_per_sample = self.ticks_per_second() / sample_rate as f64;
            self.current_tick += ticks_per_sample;
        }
    }
}

/// Not part of AWE32Emu - MC2 container helpers for feeding it.
///
/// `MUSIC.DAT` (docs/traces/mc2-music-dat-xmi.md): `u32 datapos` at EOF-4;
/// at `datapos + 8 + bank * 64 + driver * 16` a record `{header offset, XMI
/// blob offset, header size, blob size}`; driver slots G=0, R=1, F=2, W=3
/// (W = AWE32, the `C2*.WTB` scores). Returns the XMI blob of that slot.
pub fn music_dat_slot(dat: &[u8], bank: usize, driver: usize) -> Option<&[u8]> {
    let rd = |p: usize| -> Option<u32> {
        dat.get(p..p + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let datapos = rd(dat.len().checked_sub(4)?)? as usize;
    let o = datapos + 8 + bank * 64 + driver * 16;
    let x = rd(o + 4)? as usize;
    let xs = rd(o + 12)? as usize;
    dat.get(x..x + xs)
}

/// Splits a MUSIC.DAT XMI blob into its standalone XMI files: the blob is
/// several complete `FORM....XDIR` files back to back, each one is cut at
/// the start of the next `FORM` whose type at +8 is `XDIR`.
pub fn split_xmi_bank(blob: &[u8]) -> Vec<&[u8]> {
    let starts: Vec<usize> = (0..blob.len().saturating_sub(11))
        .filter(|&i| &blob[i..i + 4] == b"FORM" && &blob[i + 8..i + 12] == b"XDIR")
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| &blob[s..starts.get(k + 1).copied().unwrap_or(blob.len())])
        .collect()
}
