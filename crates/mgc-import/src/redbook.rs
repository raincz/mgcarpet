//! Redbook audio rip: MC2's in-game music is CD audio — 27 short
//! tracks (~45 s each) following the data track inside `game.gog`
//! (a raw MODE1/2352 dump). The GOG install ships the cue sheet as
//! `game.ins` next to the image.
//!
//! Cue semantics used here: single-FILE sheet, `INDEX 01 mm:ss:ff`
//! positions are file-absolute MSF (75 frames/second, 2352 bytes per
//! frame); a `PREGAP` directive is virtual silence *not stored* in
//! the file, so it never shifts file offsets. Track N's audio runs
//! from its INDEX 01 to the next track's INDEX 01 (EOF for the last).
//! Audio frames are 588 stereo samples of little-endian s16 at
//! 44100 Hz.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const SECTOR: u64 = 2352;
pub const RATE: u32 = 44100;

/// One audio track: number and file-absolute sector range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioTrack {
    pub number: u32,
    pub start_sector: u64,
    /// Exclusive; patched to EOF for the last track at rip time.
    pub end_sector: u64,
}

/// Parse a cue sheet's AUDIO tracks. `image_sectors` bounds the last
/// track.
pub fn parse_cue(cue: &str, image_sectors: u64) -> Result<Vec<AudioTrack>, String> {
    let mut tracks: Vec<AudioTrack> = Vec::new();
    let mut current: Option<u32> = None;
    for line in cue.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("TRACK") => {
                let number: u32 = words
                    .next()
                    .ok_or("TRACK without number")?
                    .parse()
                    .map_err(|_| "bad TRACK number")?;
                let kind = words.next().ok_or("TRACK without type")?;
                current = (kind == "AUDIO").then_some(number);
            }
            Some("INDEX") => {
                // The index number is numeric: GOG's `game.ins` writes
                // `INDEX 01`, a retail CloneCD `.cue` writes `INDEX 1`.
                let idx: u32 = words
                    .next()
                    .ok_or("INDEX without number")?
                    .parse()
                    .map_err(|_| "bad INDEX number")?;
                let msf = words.next().ok_or("INDEX without MSF")?;
                if idx == 0 {
                    // A pregap start: the PREVIOUS track's audio ends
                    // HERE, not at the next INDEX 01 — otherwise the
                    // ~2 s pregap leaks into its tail. The GOG sheet
                    // uses PREGAP directives, so this is insurance for
                    // other rips.
                    let sector = msf_to_sector(msf)?;
                    if let Some(prev) = tracks.last_mut() {
                        if prev.end_sector == 0 {
                            prev.end_sector = sector;
                        }
                    }
                    continue;
                }
                if idx != 1 {
                    continue;
                }
                let sector = msf_to_sector(msf)?;
                if let Some(prev) = tracks.last_mut() {
                    if prev.end_sector == 0 {
                        prev.end_sector = sector;
                    }
                }
                if let Some(number) = current {
                    tracks.push(AudioTrack {
                        number,
                        start_sector: sector,
                        end_sector: 0,
                    });
                }
            }
            _ => {}
        }
    }
    if let Some(last) = tracks.last_mut() {
        if last.end_sector == 0 {
            last.end_sector = image_sectors;
        }
    }
    for t in &tracks {
        if t.start_sector >= t.end_sector || t.end_sector > image_sectors {
            return Err(format!(
                "track {}: bad sector range {}..{} (image has {image_sectors})",
                t.number, t.start_sector, t.end_sector
            ));
        }
    }
    Ok(tracks)
}

fn msf_to_sector(msf: &str) -> Result<u64, String> {
    let parts: Vec<u64> = msf
        .split(':')
        .map(|p| p.parse::<u64>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("bad MSF {msf}"))?;
    let [m, s, f] = parts[..] else {
        return Err(format!("bad MSF {msf}"));
    };
    Ok((m * 60 + s) * 75 + f)
}

/// Read one track's PCM (interleaved stereo s16) from the image.
pub fn read_track(image: &Path, track: AudioTrack) -> std::io::Result<Vec<i16>> {
    let mut f = std::fs::File::open(image)?;
    f.seek(SeekFrom::Start(track.start_sector * SECTOR))?;
    let bytes = (track.end_sector - track.start_sector) * SECTOR;
    let mut raw = vec![0u8; bytes as usize];
    f.read_exact(&mut raw)?;
    Ok(raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect())
}

/// One CD sector of interleaved stereo samples — the junk scan's
/// granularity.
const JUNK_SECTOR: usize = 588 * 2;

/// Silence the mastering junk ahead of a speech clip's voice.
///
/// The MC2 voiceover tracks carry digital garbage from the studio
/// mastering ahead of (and between) the voice takes — high-entropy
/// bytes pressed into the audio, heard as a loud crackle at each
/// narration start in retail too. The waveform shows sharply-bounded
/// full-band blocks with UNCORRELATED stereo channels — data-as-PCM,
/// not sound; the bytes self-match inside the audio track, not the
/// CD's data track.
///
/// Law: in the clip's head (before the first sustained voice run —
/// RMS ≥ 1500 with low ZCR
/// or stereo correlation ≥ 0.60), find runs of ≥4 consecutive
/// junk-CERTAIN sectors — RMS ≥ 5000, ZCR ≥ 0.38 AND |stereo
/// correlation| < 0.25 (confirmed junk measures 0.04-0.17; voice
/// and music never shed channel correlation). ZERO from the clip
/// start through the END of the last such run only — never up to
/// the detected onset — with a short fade after the cut. Durations
/// are preserved (the narration keeps its authored timing). The
/// caller additionally gates this to SEGMENT 0 (track heads): the
/// corruption lives at the start of the map narratives; hint
/// segments were never affected. Returns muted milliseconds,
/// None = untouched.
pub fn mute_leading_junk(pcm: &mut [i16]) -> Option<u32> {
    let stats: Vec<(f32, f32, f32)> = pcm
        .chunks_exact(JUNK_SECTOR)
        .take(300)
        .map(|seg| {
            let rms = (seg
                .iter()
                .map(|&x| f64::from(x) * f64::from(x))
                .sum::<f64>()
                / seg.len() as f64)
                .sqrt() as f32;
            let zc = seg.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count() as f32
                / seg.len() as f32;
            // Stereo correlation: the junk's channels are unrelated
            // (r≈0, σ≈0.04 at sector length), voice correlates hard
            // (~0.87 measured) — a detector ZCR can't fake, catching
            // syllable onsets still buried under fading junk (the
            // level-1 "Be[fore]", sector-38 r=+0.69).
            let (l, r): (Vec<i16>, Vec<i16>) = {
                let l: Vec<i16> = seg.iter().copied().step_by(2).collect();
                let r: Vec<i16> = seg.iter().copied().skip(1).step_by(2).collect();
                (l, r)
            };
            let n = l.len().min(r.len()) as f64;
            let (ml, mr) = (
                l.iter().map(|&x| f64::from(x)).sum::<f64>() / n,
                r.iter().map(|&x| f64::from(x)).sum::<f64>() / n,
            );
            let cov: f64 = (0..n as usize)
                .map(|i| (f64::from(l[i]) - ml) * (f64::from(r[i]) - mr))
                .sum();
            let (vl, vr): (f64, f64) = (
                l.iter().map(|&x| (f64::from(x) - ml).powi(2)).sum(),
                r.iter().map(|&x| (f64::from(x) - mr).powi(2)).sum(),
            );
            let corr = if vl * vr > 0.0 {
                (cov / (vl * vr).sqrt()) as f32
            } else {
                0.0
            };
            (rms, zc, corr)
        })
        .collect();
    let voicey = |i: usize| {
        stats
            .get(i)
            .is_some_and(|&(r, z, c)| r >= 1500.0 && (z < 0.30 || c >= 0.60))
    };
    let onset = (0..stats.len())
        .find(|&i| voicey(i) && (voicey(i + 1) as u8 + voicey(i + 2) as u8) >= 1)?;
    // Junk-CERTAIN sectors only: loud, white AND stereo-uncorrelated
    // (voice/music can reach the first two in bursts; it can never
    // shed its channel correlation — confirmed crackle heads sit at
    // |corr| 0.04-0.17). Mute only through the END of the last junk
    // run — never up to the detected onset, so late-detected voice
    // can't be zeroed.
    let mut run = 0usize;
    let mut last_run_end = None;
    for (i, s) in stats[..onset].iter().enumerate() {
        if s.0 >= 5000.0 && s.1 >= 0.38 && s.2.abs() < 0.25 {
            run += 1;
            if run >= 4 {
                last_run_end = Some(i + 1);
            }
        } else {
            run = 0;
        }
    }
    let cut = last_run_end? * JUNK_SECTOR;
    for x in &mut pcm[..cut] {
        *x = 0;
    }
    // ~3 ms linear fade into the onset sector: kills any residual
    // boundary click without touching audible voice.
    let fade = 256.min(pcm.len() - cut);
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        pcm[cut + i] = (f32::from(pcm[cut + i]) * g) as i16;
    }
    Some((cut as u64 / 2 * 1000 / u64::from(RATE)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic white noise (an LCG) at data-burst amplitude.
    fn noise(len: usize, seed: &mut u32) -> Vec<i16> {
        (0..len)
            .map(|_| {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (*seed >> 16) as i16 / 2
            })
            .collect()
    }

    /// A voicey signal: a low-frequency wave at speaking level
    /// (ZCR well under 0.30, RMS well over 1500).
    fn voice(len: usize) -> Vec<i16> {
        (0..len)
            .map(|i| (8000.0 * (i as f32 * 0.02).sin()) as i16)
            .collect()
    }

    #[test]
    fn junk_head_is_muted_voice_kept() {
        let mut seed = 1u32;
        let mut pcm = Vec::new();
        pcm.extend(vec![0i16; 4 * JUNK_SECTOR]); // lead silence
        pcm.extend(noise(12 * JUNK_SECTOR, &mut seed)); // the burst
        pcm.extend(vec![0i16; 2 * JUNK_SECTOR]); // gap
        pcm.extend(voice(30 * JUNK_SECTOR));
        let ms = mute_leading_junk(&mut pcm).expect("burst detected");
        assert!(ms > 0);
        // Everything before the voice is silent now.
        let onset = 18 * JUNK_SECTOR;
        assert!(pcm[..onset].iter().all(|&x| x == 0), "head muted");
        // The voice body survives untouched (past the 3 ms fade).
        assert!(pcm[onset + 512..].iter().any(|&x| x > 6000), "voice kept");
    }

    #[test]
    fn clean_clips_and_sibilant_onsets_stay_untouched() {
        // Silence + voice, no burst.
        let mut clean = vec![0i16; 6 * JUNK_SECTOR];
        clean.extend(voice(30 * JUNK_SECTOR));
        let before = clean.clone();
        assert_eq!(mute_leading_junk(&mut clean), None);
        assert_eq!(clean, before, "clean clip untouched");
        // A SHORT high-ZCR island (a sibilant "S", under 4 sectors)
        // right before voice must not trigger.
        let mut seed = 7u32;
        let mut sib = vec![0i16; 6 * JUNK_SECTOR];
        sib.extend(noise(2 * JUNK_SECTOR, &mut seed));
        sib.extend(voice(30 * JUNK_SECTOR));
        let before = sib.clone();
        assert_eq!(mute_leading_junk(&mut sib), None);
        assert_eq!(sib, before, "sibilant onset untouched");
    }

    #[test]
    fn parses_the_mc2_cue_shape() {
        let cue = "FILE \"game.gog\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    PREGAP 00:02:00\n    INDEX 01 18:57:63\n  TRACK 03 AUDIO\n    INDEX 01 19:48:68\n";
        let tracks = parse_cue(cue, 100_000).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].number, 2);
        assert_eq!(tracks[0].start_sector, (18 * 60 + 57) * 75 + 63);
        assert_eq!(tracks[0].end_sector, (19 * 60 + 48) * 75 + 68);
        assert_eq!(tracks[1].end_sector, 100_000);
    }

    /// A retail CloneCD `.cue`: unpadded index numbers (`INDEX 1`) and
    /// the pregap STORED in the file (`INDEX 0`), which ends the
    /// previous track and is skipped by the next.
    #[test]
    fn parses_a_retail_clonecd_cue() {
        let cue = "FILE \"Magic Carpet 2.img\" BINARY\n   TRACK 1 MODE1/2352\n   INDEX 1 00:00:00\n   TRACK 2 AUDIO\n   INDEX 0 18:57:63\n   INDEX 1 18:59:63\n   TRACK 3 AUDIO\n   INDEX 1 19:50:68\n";
        let tracks = parse_cue(cue, 100_000).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].number, 2);
        assert_eq!(tracks[0].start_sector, (18 * 60 + 59) * 75 + 63);
        assert_eq!(tracks[0].end_sector, (19 * 60 + 50) * 75 + 68);
    }

    #[test]
    fn data_track_index_bounds_nothing() {
        // The data track's INDEX must not open a phantom track.
        let cue = "TRACK 01 MODE1/2352\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 00:10:00\n";
        let tracks = parse_cue(cue, 10_000).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].start_sector, 750);
    }
}
