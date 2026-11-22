//! XMI (AIL/Miles eXtended MIDI) parser + SMF encoder — the format
//! inside MC2's `SOUND/MUSIC.DAT` blobs (trace
//! docs/traces/mc2-music-dat-xmi.md).
//!
//! Container: `FORM XDIR … CAT XMID` holding one `FORM XMID` child
//! per sub-song (MC2: 6 — GAME1/2/3, SETUP, INTRO, CUTS); inside each
//! child, `TIMB` (patch preload — ignored, the EVNT's own program
//! changes are direct GM numbers), optional `RBRN` (interactive
//! branch table — ignored), and `EVNT` (the stream).
//!
//! EVNT quirks vs regular MIDI (all byte-verified against retail):
//! - Delta time = a RUN of bytes < 0x80, SUMMED (127+127+… for long
//!   waits) — NOT the SMF continuation VLQ. The first high-bit byte
//!   is the next status. XMI ticks run on AIL's FIXED 120 Hz clock:
//!   the XMIDI sequencer IGNORES `FF 51` tempo metas. We emit SMF
//!   division 60 PPQN with one 500 000 µs/quarter tempo at tick 0, so
//!   a tick is exactly 1/120 s, and drop the score's own tempo metas.
//!   (Until 2026-11-22 they were passed through and every baked MC2
//!   piece ran at the wrong speed — the cave track 361 s instead of
//!   237 s. Two witnesses: AWE32Emu's `XmiFile.cpp:139` ignores FF 51
//!   on purpose, checked against a real MC2 recording — C2GAME1 52755
//!   ticks / 120 = 439.6 s vs 441.9 s recorded, tempo-applied 493 s;
//!   and on the fixed clock `-music2`'s C1GAME1/2/3 last exactly as
//!   long as MC1's own HMP cgame1/2/3, the same notes.)
//! - Note-on carries an embedded duration as a STANDARD
//!   (continuation-bit) VLQ; there are no note-off events — the
//!   converter synthesizes them at `tick + duration`.
//! - Controllers 110-119 are AIL-private and must never reach a GM
//!   synth: 116/117 = FOR-loop start/next (MC2 wraps every sub-song
//!   in one infinite whole-song loop — we drop the pair and loop the
//!   baked FLAC), 119 = the trigger tag that DECLARES A WAR CHANNEL
//!   (the original zeroes cc11 on tagged channels at song start and
//!   ramps them up in combat — Sound.cpp:851/5880; the MC2 analog of
//!   MC1's channel-3/4/5 danger stems).

/// One event at an absolute XMI tick (fixed 120/s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub tick: u32,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    NoteOn {
        ch: u8,
        note: u8,
        vel: u8,
    },
    NoteOff {
        ch: u8,
        note: u8,
    },
    Program {
        ch: u8,
        prog: u8,
    },
    Control {
        ch: u8,
        ctrl: u8,
        val: u8,
    },
    PitchBend {
        ch: u8,
        value: u16,
    },
    ChannelPressure {
        ch: u8,
        val: u8,
    },
    Aftertouch {
        ch: u8,
        note: u8,
        val: u8,
    },
    /// `FF 51` — microseconds per quarter note. Parsed for the record;
    /// AIL ignores it and so does [`encode_smf`].
    Tempo {
        usec_per_quarter: u32,
    },
}

pub struct Song {
    /// Sorted by tick; synthesized note-offs precede same-tick events.
    pub events: Vec<Event>,
    pub end_tick: u32,
    /// Channels carrying a cc119 trigger tag — the war/danger layers.
    pub war_channels: u16,
}

impl Song {
    pub fn has_war_layer(&self) -> bool {
        self.war_channels != 0
    }
}

/// The SMF division making one XMI tick = 1/120 s at the default
/// 500 000 µs/quarter tempo (AIL's fixed 120 Hz sequencer clock).
pub const SMF_DIVISION: u16 = 60;

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Split a `FORM XMID` / `CAT XMID` / `FORM XDIR … CAT XMID`
/// container into its sub-songs' EVNT chunk bodies, in file order.
pub fn split_container(blob: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut at = 0usize;
    if blob.get(..4) == Some(b"FORM") && blob.get(8..12) == Some(b"XDIR") {
        // Skip the XDIR form; the CAT follows, word-aligned.
        let size = be_u32(blob, 4).ok_or("truncated XDIR")? as usize;
        at = 8 + size + (size & 1);
    }
    let songs: Vec<&[u8]> = match blob.get(at..at + 4) {
        Some(b"CAT ") => {
            let size = be_u32(blob, at + 4).ok_or("truncated CAT")? as usize;
            let body = blob
                .get(at + 8..at + 8 + size)
                .ok_or("CAT spans past the blob")?;
            if body.get(..4) != Some(b"XMID") {
                return Err("CAT is not XMID".into());
            }
            // Children: FORM XMID, back to back, word-aligned.
            let mut songs = Vec::new();
            let mut p = 4usize;
            while p + 8 <= body.len() {
                let id = &body[p..p + 4];
                let size = be_u32(body, p + 4).ok_or("truncated CAT child")? as usize;
                let child = body
                    .get(p + 8..p + 8 + size)
                    .ok_or("CAT child spans past the CAT")?;
                if id == b"FORM" && child.get(..4) == Some(b"XMID") {
                    songs.push(child);
                }
                p += 8 + size + (size & 1);
            }
            songs
        }
        Some(b"FORM") if blob.get(at + 8..at + 12) == Some(b"XMID") => {
            let size = be_u32(blob, at + 4).ok_or("truncated FORM")? as usize;
            vec![
                blob.get(at + 8..at + 8 + size)
                    .ok_or("FORM spans past the blob")?,
            ]
        }
        _ => return Err("not an XMI container".into()),
    };
    // Each song: the XMID form body; find its EVNT chunk.
    songs
        .into_iter()
        .map(|form| {
            let mut p = 4usize; // past "XMID"
            while p + 8 <= form.len() {
                let id = &form[p..p + 4];
                let size = be_u32(form, p + 4).ok_or("truncated XMID chunk")? as usize;
                let body = form
                    .get(p + 8..p + 8 + size)
                    .ok_or("XMID chunk spans past the form")?;
                if id == b"EVNT" {
                    return Ok(body);
                }
                p += 8 + size + (size & 1);
            }
            Err("XMID form has no EVNT".into())
        })
        .collect()
}

/// Standard continuation-bit VLQ (the note-duration / sysex-length
/// encoding — NOT the delta encoding).
fn read_vlq(data: &[u8], p: &mut usize) -> Result<u32, String> {
    let mut v = 0u32;
    for _ in 0..5 {
        let b = *data.get(*p).ok_or("truncated VLQ")?;
        *p += 1;
        v = (v << 7) | u32::from(b & 0x7F);
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("VLQ too long".into())
}

/// Parse one EVNT stream into a [`Song`].
pub fn parse_evnt(data: &[u8]) -> Result<Song, String> {
    let mut events: Vec<(u32, u8, EventKind)> = Vec::new(); // (tick, rank, kind)
    let mut offs: Vec<(u32, u8, u8)> = Vec::new(); // (end tick, ch, note)
    let mut war_channels = 0u16;
    let mut tick = 0u32;
    let mut p = 0usize;
    let mut end_tick = None;
    while end_tick.is_none() {
        let b = *data.get(p).ok_or("EVNT ended without end-of-track")?;
        if b < 0x80 {
            // XMI delta: a run of low bytes, SUMMED.
            tick += u32::from(b);
            p += 1;
            continue;
        }
        p += 1;
        let ch = b & 0x0F;
        match b & 0xF0 {
            0x90 => {
                let note = *data.get(p).ok_or("truncated note-on")?;
                let vel = *data.get(p + 1).ok_or("truncated note-on")?;
                p += 2;
                if vel == 0 {
                    // Defensive: retail streams never do this.
                    events.push((tick, 1, EventKind::NoteOff { ch, note }));
                } else {
                    let dur = read_vlq(data, &mut p)?;
                    events.push((tick, 1, EventKind::NoteOn { ch, note, vel }));
                    offs.push((tick + dur, ch, note));
                }
            }
            0x80 => {
                // Never present in XMI; tolerate.
                let note = *data.get(p).ok_or("truncated note-off")?;
                p += 2;
                events.push((tick, 1, EventKind::NoteOff { ch, note }));
            }
            0xB0 => {
                let ctrl = *data.get(p).ok_or("truncated control")?;
                let val = *data.get(p + 1).ok_or("truncated control")?;
                p += 2;
                match ctrl {
                    119 => war_channels |= 1 << ch,
                    // A FOR-loop start deep into a song is a real
                    // mid-song loop this strip FLATTENS (deliberate:
                    // plays through once, then the baked FLAC loops
                    // whole). Two bank-0 songs have one (C2GAME3 tick
                    // 1, C2INTRO tick 11876); note it audibly.
                    116 if tick > 1 => {
                        eprintln!(
                            "note: cc116 FOR-loop start at tick {tick} — \
                             mid-song loop flattened (plays through once)"
                        );
                    }
                    // The AIL-private band (channel lock, banks, FOR
                    // loops 116/117, callbacks): never emitted.
                    110..=119 => {}
                    _ => events.push((tick, 1, EventKind::Control { ch, ctrl, val })),
                }
            }
            0xC0 => {
                let prog = *data.get(p).ok_or("truncated program")?;
                p += 1;
                events.push((tick, 1, EventKind::Program { ch, prog }));
            }
            0xD0 => {
                let val = *data.get(p).ok_or("truncated pressure")?;
                p += 1;
                events.push((tick, 1, EventKind::ChannelPressure { ch, val }));
            }
            0xA0 => {
                let note = *data.get(p).ok_or("truncated aftertouch")?;
                let val = *data.get(p + 1).ok_or("truncated aftertouch")?;
                p += 2;
                events.push((tick, 1, EventKind::Aftertouch { ch, note, val }));
            }
            0xE0 => {
                let lo = *data.get(p).ok_or("truncated bend")?;
                let hi = *data.get(p + 1).ok_or("truncated bend")?;
                p += 2;
                events.push((
                    tick,
                    1,
                    EventKind::PitchBend {
                        ch,
                        value: u16::from(lo & 0x7F) | (u16::from(hi & 0x7F) << 7),
                    },
                ));
            }
            0xF0 => match b {
                0xFF => {
                    let ty = *data.get(p).ok_or("truncated meta")?;
                    p += 1;
                    let len = read_vlq(data, &mut p)? as usize;
                    let body = data.get(p..p + len).ok_or("truncated meta body")?;
                    p += len;
                    match ty {
                        0x2F => end_tick = Some(tick),
                        0x51 if len == 3 => {
                            let usec = u32::from(body[0]) << 16
                                | u32::from(body[1]) << 8
                                | u32::from(body[2]);
                            events.push((
                                tick,
                                1,
                                EventKind::Tempo {
                                    usec_per_quarter: usec,
                                },
                            ));
                        }
                        _ => {} // time-sig etc: timing-irrelevant in SMF
                    }
                }
                0xF0 | 0xF7 => {
                    let len = read_vlq(data, &mut p)? as usize;
                    p += len; // sysex: skip
                }
                _ => return Err(format!("unexpected status {b:#04x} at {p}")),
            },
            _ => unreachable!(),
        }
    }
    let mut end_tick = end_tick.unwrap();
    // Synthesized note-offs; rank 0 puts them ahead of same-tick
    // events so re-struck notes never cancel.
    for (t, ch, note) in offs {
        end_tick = end_tick.max(t);
        events.push((t, 0, EventKind::NoteOff { ch, note }));
    }
    events.sort_by_key(|&(t, rank, _)| (t, rank));
    Ok(Song {
        events: events
            .into_iter()
            .map(|(tick, _, kind)| Event { tick, kind })
            .collect(),
        end_tick,
        war_channels,
    })
}

/// Which channels a render keeps: the whole song, the ambient mix
/// (war channels silenced — retail zeroes their cc11 at song start),
/// or the war stem alone (retail ramps their cc11 up in combat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mix {
    Full,
    Ambient,
    WarStem,
}

fn vlq(out: &mut Vec<u8>, mut v: u32) {
    let mut stack = [0u8; 5];
    let mut n = 0;
    loop {
        stack[n] = (v & 0x7F) as u8;
        v >>= 7;
        n += 1;
        if v == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        out.push(stack[i] | if i > 0 { 0x80 } else { 0 });
    }
}

/// Encode as a type-0 SMF, division [`SMF_DIVISION`], one fixed
/// 500 000 µs/quarter tempo at tick 0 and the score's own tempo metas
/// DROPPED — AIL's XMIDI clock is a fixed 120 Hz (see the module doc).
pub fn encode_smf(song: &Song, mix: Mix) -> Vec<u8> {
    let keep = |ch: u8| -> bool {
        match mix {
            Mix::Full => true,
            Mix::Ambient => song.war_channels & (1 << ch) == 0,
            Mix::WarStem => song.war_channels & (1 << ch) != 0,
        }
    };
    // The fixed clock, explicit (it is also the SMF default).
    let mut track = vec![0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20];
    let mut last_tick = 0u32;
    for ev in &song.events {
        let bytes: Vec<u8> = match ev.kind {
            EventKind::NoteOn { ch, note, vel } if keep(ch) => vec![0x90 | ch, note, vel],
            EventKind::NoteOff { ch, note } if keep(ch) => vec![0x90 | ch, note, 0],
            EventKind::Program { ch, prog } if keep(ch) => vec![0xC0 | ch, prog],
            EventKind::Control { ch, ctrl, val } if keep(ch) => vec![0xB0 | ch, ctrl, val],
            EventKind::PitchBend { ch, value } if keep(ch) => {
                vec![0xE0 | ch, (value & 0x7F) as u8, (value >> 7) as u8]
            }
            EventKind::ChannelPressure { ch, val } if keep(ch) => vec![0xD0 | ch, val],
            EventKind::Aftertouch { ch, note, val } if keep(ch) => vec![0xA0 | ch, note, val],
            // AIL ignores tempo metas (fixed 120 Hz clock).
            EventKind::Tempo { .. } => continue,
            _ => continue, // filtered by the mix
        };
        vlq(&mut track, ev.tick - last_tick);
        last_tick = ev.tick;
        track.extend_from_slice(&bytes);
    }
    // End-of-track on the song end in every mix, so ambient and stem
    // renders keep the same musical length.
    vlq(&mut track, song.end_tick.saturating_sub(last_tick));
    track.extend_from_slice(&[0xFF, 0x2F, 0x00]);

    let mut out = Vec::with_capacity(track.len() + 22);
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&SMF_DIVISION.to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_sum_and_durations_synthesize_offs() {
        // delta 127+3, note-on ch1 dur 0x85 0x00 (=0x280), delta 5,
        // cc7, end.
        let evnt = [
            0x7F, 0x03, // delta 130
            0x91, 60, 100, 0x85, 0x00, // note on, dur 640
            0x05, // delta 5
            0xB1, 0x07, 0x40, // cc7=64
            0xFF, 0x2F, 0x00,
        ];
        let song = parse_evnt(&evnt).unwrap();
        assert_eq!(song.end_tick, 130 + 640);
        let kinds: Vec<_> = song.events.iter().map(|e| (e.tick, e.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (
                    130,
                    EventKind::NoteOn {
                        ch: 1,
                        note: 60,
                        vel: 100
                    }
                ),
                (
                    135,
                    EventKind::Control {
                        ch: 1,
                        ctrl: 7,
                        val: 64
                    }
                ),
                (770, EventKind::NoteOff { ch: 1, note: 60 }),
            ]
        );
    }

    #[test]
    fn ail_controllers_stripped_and_cc119_tags_war_channels() {
        let evnt = [
            0xB6, 119, 0, // cc119 ch6: war tag
            0xB6, 116, 0, // FOR start: stripped
            0xB0, 0x0A, 0x20, // pan: kept
            0xB6, 117, 127, // FOR next: stripped
            0xFF, 0x2F, 0x00,
        ];
        let song = parse_evnt(&evnt).unwrap();
        assert_eq!(song.war_channels, 1 << 6);
        assert_eq!(song.events.len(), 1);
        assert!(matches!(
            song.events[0].kind,
            EventKind::Control {
                ch: 0,
                ctrl: 0x0A,
                ..
            }
        ));
    }

    #[test]
    fn mixes_split_on_war_channels() {
        let evnt = [
            0xB6, 119, 0, // ch6 = war
            0x96, 60, 100, 10, // war note
            0x90, 62, 100, 10, // ambient note
            0xFF, 0x2F, 0x00,
        ];
        let song = parse_evnt(&evnt).unwrap();
        let full = encode_smf(&song, Mix::Full);
        let ambient = encode_smf(&song, Mix::Ambient);
        let stem = encode_smf(&song, Mix::WarStem);
        assert!(full.len() > ambient.len());
        assert!(full.len() > stem.len());
        // Same end-of-track tick in all three (length alignment).
        assert_eq!(&full[full.len() - 3..], &[0xFF, 0x2F, 0x00]);
        assert_eq!(&ambient[ambient.len() - 3..], &[0xFF, 0x2F, 0x00]);
        assert_eq!(&stem[stem.len() - 3..], &[0xFF, 0x2F, 0x00]);
    }

    #[test]
    fn tempo_metas_are_ignored_on_the_fixed_120hz_clock() {
        let evnt = [
            0xFF, 0x51, 0x03, 0x08, 0x8E, 0x6C, // 560748 µs/qn
            0x90, 60, 100, 10, //
            0xFF, 0x2F, 0x00,
        ];
        let song = parse_evnt(&evnt).unwrap();
        assert_eq!(
            song.events[0].kind,
            EventKind::Tempo {
                usec_per_quarter: 560748
            }
        );
        let smf = encode_smf(&song, Mix::Full);
        // division 60 in the header.
        assert_eq!(&smf[12..14], &SMF_DIVISION.to_be_bytes());
        // Exactly one tempo meta: the fixed 500 000 µs/quarter at tick
        // 0 (track data starts at 22) — the score's 560748 is gone.
        let metas: Vec<usize> = smf
            .windows(3)
            .enumerate()
            .filter(|(_, w)| *w == [0xFF, 0x51, 0x03])
            .map(|(i, _)| i)
            .collect();
        assert_eq!(metas, vec![23]);
        assert_eq!(&smf[22..30], &[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, 0x00]);
    }

    #[test]
    fn container_walk_finds_evnts() {
        // Minimal CAT XMID with one FORM XMID child holding an EVNT.
        let evnt_body = [0xFF, 0x2F, 0x00];
        let mut form = Vec::new();
        form.extend_from_slice(b"XMID");
        form.extend_from_slice(b"EVNT");
        form.extend_from_slice(&(evnt_body.len() as u32).to_be_bytes());
        form.extend_from_slice(&evnt_body);
        form.push(0); // word align (len 3)
        let mut cat_body = Vec::new();
        cat_body.extend_from_slice(b"XMID");
        cat_body.extend_from_slice(b"FORM");
        cat_body.extend_from_slice(&(form.len() as u32).to_be_bytes());
        cat_body.extend_from_slice(&form);
        let mut blob = Vec::new();
        blob.extend_from_slice(b"CAT ");
        blob.extend_from_slice(&(cat_body.len() as u32).to_be_bytes());
        blob.extend_from_slice(&cat_body);
        let songs = split_container(&blob).unwrap();
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0], &evnt_body[..]);
    }
}
