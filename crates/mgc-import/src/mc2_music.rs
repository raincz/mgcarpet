//! MC2 `SOUND/MUSIC.DAT` container parser (trace
//! docs/traces/mc2-music-dat-xmi.md §A; `InitMusicBank_8EAD0`,
//! remc2 Sound.cpp:5499).
//!
//! Layout: trailer u32 @ EOF-4 → `datapos`; `int16 driverarray[4]` =
//! per-driver SONG-BANK counts (retail: 2 banks each). Record table
//! at `datapos+8`: per bank a 64-byte record of 4×16-byte slots, one
//! per driver (G=0 GM, R=1 MT-32, F=2 FM, W=3 AWE32) — slot =
//! `{u32 hdr_off, u32 xmi_off, u32 hdr_size, u32 xmi_size}`. The
//! header block (RNC-checked; retail ships it raw) is
//! `{8-byte prefix}{10-byte stub}{6 × 32-byte track slots}` with the
//! on-disk slot = filename[18] FIRST, then offset/stub/size/word12
//! (the decompile's "shadow" struct). The XMI blob is ONE
//! `FORM XDIR … CAT XMID` holding the 6 sub-songs, in slot order:
//! GAME1, GAME2, GAME3, SETUP, INTRO, CUTS.
//!
//! Gameplay music = driver G, **bank 0** (the "C2" = Magic Carpet 2
//! set — `musicChannel_E3814 = 0`, Sound.cpp:49, never reassigned).
//! Bank 1 is the "C1" MC1-set alternate behind the hidden `-music2`
//! flag (EF:39191/43023 gated by `setting_byte4_25 & 0x40`) — an
//! opt-in, NOT the default. MapType picks GAMEn (Night=1, Day=2,
//! Cave=3 → sub-song n-1), the menu plays SETUP (track 4 → sub-song
//! 3).

/// One named sub-song: (`C2GAME1.GEN`, parsed XMI, its own bytes).
pub struct SubSong {
    pub name: String,
    pub song: crate::xmi::Song,
    /// The sub-song's standalone `FORM XDIR … CAT XMID` container, as
    /// shipped — what the AWE32 renderer plays (it reads XMI itself).
    pub xmi: Vec<u8>,
}

/// Driver slots of a bank record (G, R, F, W).
pub const DRIVER_GM: usize = 0;
/// The Sound Blaster AWE32 scores (`C2*.WTB`) — the game's own AWE32
/// arrangement: GAME2/3 and CUTS equal the GM ones, GAME1 differs by
/// six bytes, SETUP and INTRO are genuinely re-arranged.
pub const DRIVER_AWE32: usize = 3;

fn le_u32(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .ok_or_else(|| format!("MUSIC.DAT truncated at {at}"))?
            .try_into()
            .unwrap(),
    ))
}

fn maybe_rnc(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.len() >= 3 && &raw[..3] == b"RNC" {
        crate::rnc::decompress(raw).map_err(|e| format!("RNC: {e:?}"))
    } else {
        Ok(raw.to_vec())
    }
}

/// Parse the GM section of the given bank (gameplay = 0, the "C2"
/// set; 1 = the hidden `-music2` "C1" alternate) into its six named
/// sub-songs.
pub fn parse_gm_bank(data: &[u8], bank: usize) -> Result<Vec<SubSong>, String> {
    parse_bank(data, bank, DRIVER_GM)
}

/// [`parse_gm_bank`] for any driver slot ([`DRIVER_GM`],
/// [`DRIVER_AWE32`], …).
pub fn parse_bank(data: &[u8], bank: usize, driver: usize) -> Result<Vec<SubSong>, String> {
    if driver > 3 {
        return Err(format!("driver slot {driver} out of range"));
    }
    if data.len() < 12 {
        return Err("MUSIC.DAT too short".into());
    }
    let datapos = le_u32(data, data.len() - 4)? as usize;
    let banks = i16::from_le_bytes(
        data.get(datapos + driver * 2..datapos + driver * 2 + 2)
            .ok_or("driver index out of range")?
            .try_into()
            .unwrap(),
    );
    // A corrupt/negative count must not wrap huge through the usize
    // cast.
    if banks < 0 || bank >= banks as usize {
        return Err(format!(
            "driver {driver} section has {banks} banks, wanted {bank}"
        ));
    }
    let rec = datapos + 8 + bank * 64 + driver * 16;
    let hdr_off = le_u32(data, rec)? as usize;
    let xmi_off = le_u32(data, rec + 4)? as usize;
    let hdr_size = le_u32(data, rec + 8)? as usize;
    let xmi_size = le_u32(data, rec + 12)? as usize;

    let hdr = maybe_rnc(
        data.get(hdr_off..hdr_off + hdr_size)
            .ok_or("header block out of range")?,
    )?;
    // 8-byte prefix + 10-byte stub, then 6 × 32-byte slots in the
    // in-memory field order: {u32 dataOff, u32 stub, u32 size,
    // u16 word12, u8 filename[18]} — the decompile's "shadow" struct
    // does NOT match the retail file.
    let mut names = Vec::new();
    for t in 0..6 {
        let at = 8 + 10 + t * 32 + 14;
        let Some(raw) = hdr.get(at..at + 18) else {
            break;
        };
        let name: String = raw
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
    }

    let blob = maybe_rnc(
        data.get(xmi_off..xmi_off + xmi_size)
            .ok_or("XMI blob out of range")?,
    )?;
    // The region is a CONCATENATION of single-song `FORM XDIR … CAT
    // XMID` containers (one per sub-song, possibly padded between).
    // The header's per-slot data offsets are shifted one slot (slot
    // 0 doubles as a whole-region aggregate) — the retail ±1
    // load/play skew — so split on the `FORM…XDIR` magics instead:
    // container order = name order (container 0 opens at 120 BPM =
    // GAME1).
    let mut starts = Vec::new();
    for at in 0..blob.len().saturating_sub(12) {
        if &blob[at..at + 4] == b"FORM" && &blob[at + 8..at + 12] == b"XDIR" {
            starts.push(at);
        }
    }
    if starts.len() != names.len() {
        return Err(format!(
            "header names {} vs XMI containers {}",
            names.len(),
            starts.len()
        ));
    }
    names
        .into_iter()
        .zip(starts.iter().enumerate())
        .map(|(name, (i, &start))| {
            let end = starts.get(i + 1).copied().unwrap_or(blob.len());
            let evnts = crate::xmi::split_container(&blob[start..end])
                .map_err(|e| format!("{name}: {e}"))?;
            let [evnt] = evnts[..] else {
                return Err(format!("{name}: {} EVNTs in one container", evnts.len()));
            };
            let song = crate::xmi::parse_evnt(evnt).map_err(|e| format!("{name}: {e}"))?;
            Ok(SubSong {
                name,
                song,
                xmi: blob[start..end].to_vec(),
            })
        })
        .collect()
}
