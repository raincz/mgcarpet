//! `.mgcs` save container — the native save file
//! (`docs/archive/DESIGN-SAVES.md` "Format").
//!
//! Same house pattern as [`crate::mgcl`]: a ZIP with a JSON index and
//! raw little-endian binary members. Two differences, both deliberate:
//!
//! - **Members are DEFLATEd.** `.mgcl` stores everything uncompressed
//!   so identical content yields identical bytes, which the committed
//!   level hashes depend on. Saves are pinned by nothing, and the
//!   payload is ~570 KiB of highly repetitive terrain and entity pool.
//! - **The sim payload is ONE member**, not the `world/*.bin` +
//!   `sim.bin` split the design sketched. The sim's own codec
//!   (`mgc_sim::snapshot`) emits a single versioned stream, and
//!   cutting it up here would put the field layout in two places —
//!   this crate cannot see `mgc-sim`'s types at all (it is the
//!   dependency, not the dependent).
//!
//! A slot always carries the campaign record, and carries the world
//! payload only when the save was taken mid-level. So a slot knows on
//! its own whether it resumes at the hub or drops straight into play,
//! and every slot is loadable from the main menu.

use std::io::{Read, Seek, Write};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{BAKE_EPOCH, Game};

/// Current `.mgcs` container version. Independent of `FORMAT_VERSION`
/// (levels) and of `mgc_sim::snapshot::SNAPSHOT_VERSION` (the payload
/// stream) — all three move for different reasons.
///
/// 2: `level` promoted to the header (every save sits at a level, not
///    just an in-level one) and `InLevel` gained `mana_pct`, so the
///    slot list can show "L3" and "L3 15%" without opening the payload.
pub const SAVE_VERSION: u32 = 2;

#[derive(Debug)]
pub enum MgcsError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Json(serde_json::Error),
    MissingMember(&'static str),
    /// `save.json` declares a container version this build cannot read.
    UnsupportedVersion(u32),
}

impl std::fmt::Display for MgcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Zip(e) => write!(f, "container: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
            Self::MissingMember(name) => write!(f, "missing required member {name}"),
            Self::UnsupportedVersion(v) => {
                write!(
                    f,
                    "save version {v} unsupported (this build knows {SAVE_VERSION})"
                )
            }
        }
    }
}

impl std::error::Error for MgcsError {}

impl From<std::io::Error> for MgcsError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<zip::result::ZipError> for MgcsError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::Zip(e)
    }
}
impl From<serde_json::Error> for MgcsError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// `save.json` — everything the slot list needs, without touching the
/// payload.
///
/// The rejection keys are [`Self::entry_sha256`] and the sim's own
/// identity fingerprint inside the payload. `bake_epoch` is recorded
/// but is deliberately NOT a rejection key: it bumps for audio
/// re-renders and UI assets, and gating on it would invalidate every
/// save for reasons that cannot affect a world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveHeader {
    pub save_version: u32,
    pub game: Game,
    /// Slot label as shown in the menu.
    pub label: String,
    /// Wall-clock seconds since the Unix epoch, for the menu's
    /// "saved" column. Zero when the clock was unavailable — display
    /// only, never compared.
    #[serde(default)]
    pub saved_unix: u64,
    /// Campaign position (the retail record's own level counter),
    /// carried out here so the menu can show it without decoding
    /// `campaign.bin`.
    pub campaign_level: u32,
    /// The level this slot SITS AT — the one a mid-level save resumes
    /// into, or the one a hub save is parked in front of. Present on
    /// every save, because every save has one: it is what the slot
    /// lists show as "L<n>".
    ///
    /// Deliberately NOT duplicated inside [`InLevel`]: two copies of a
    /// level number is two chances to disagree, and this is the one
    /// `resume` rebuilds from.
    #[serde(default)]
    pub level: u32,
    /// Bake epoch at save time. Informational — see the type docs.
    #[serde(default)]
    pub bake_epoch: u32,
    /// Present exactly when a world payload is present. `None` = a hub
    /// save, which resumes at the campaign screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<InLevel>,
    /// MC1/HW spell cycle-ring membership (0 = none / 1 = left ring /
    /// 2 = right, per spell) — NATIVE-ONLY campaign carry: the retail
    /// 142-byte `.gam` record has no room for it, so it lives here and
    /// is simply absent from the exported file. MC2 needs no twin (its
    /// ring sits inside the retail `str_611` blob at 0x3B5). `None` on
    /// MC2 slots and on pre-ring saves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc1_spell_ring: Option<[u8; 24]>,
    /// Quick keys 1..9,0 → spell id (`null` = unbound) — the same
    /// native-only campaign carry as the ring (player ask 2026-10-17:
    /// rebinding the digits every level was a chore). MC1/HW ids; MC2
    /// ids on MC2 slots since the hotbar (2026-11-21 — the name is
    /// historical; a save is one game's, so ids never mix). `None` on
    /// saves from before the carry and when nothing is bound — such a
    /// campaign seeds its keys from scratch at level start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc1_quick_keys: Option<[Option<u8>; 10]>,
    /// MC2: the stats tables the map still owes, oldest first — a
    /// secret level's parent, won through the alternate portal, whose
    /// table waits for the next map entry. Native-only; saved
    /// mid-level so a resume shows both tables. Empty (and omitted)
    /// otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc2_map_stats: Vec<u32>,
}

/// The mid-level half of the header. The level itself lives in
/// [`SaveHeader::level`], which both kinds of save carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InLevel {
    /// Asset bundle variant the level was loaded with, e.g.
    /// `mc1-temperate`. A save restored against a different bundle
    /// would re-supply different `Gen::assets`.
    pub bundle: String,
    /// Hex SHA-256 of the level package's source entry — THE
    /// rejection key. A repackaged level is a different world even at
    /// the same index.
    pub entry_sha256: String,
    /// `mgc_sim::snapshot::SNAPSHOT_VERSION` the payload was written
    /// with; checked by the sim on apply, surfaced here so an
    /// incompatible slot can be greyed out before it is opened.
    pub snapshot_version: u32,
    /// Sim tick at save time — the menu's play-time column.
    #[serde(default)]
    pub tick: u64,
    /// Percent of the world's mana the player held at save time (the
    /// HUD's own numerator over its denominator). This is what
    /// distinguishes "L3 15%" — a run in progress — from a bare "L3"
    /// hub save, and it doubles as a rough how-far-in marker.
    #[serde(default)]
    pub mana_pct: u8,
    /// G-class selectors, for DISPLAY only ("Enhanced flight"). The
    /// authoritative copies live in the payload and are restored from
    /// there; these are never read back into the sim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thrust_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_model: Option<String>,
}

/// A whole save slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePackage {
    pub header: SaveHeader,
    /// The retail campaign record verbatim (142 B MC1/HW, 1319 B
    /// MC2), including the opaque blobs the native format has no use
    /// for. Carried so the `.gam` export stays byte-exact. MC2 may
    /// append a native tail (the stats absolutes, `MGK3`), which the
    /// export trims.
    pub campaign: Vec<u8>,
    /// `mgc_sim::Simulation::snapshot()`. Absent on a hub save.
    pub snapshot: Option<Vec<u8>>,
}

impl SavePackage {
    /// Does this slot resume in play, or at the campaign screen?
    pub fn is_in_level(&self) -> bool {
        self.snapshot.is_some()
    }
}

/// Seconds since the Unix epoch, or 0 if the clock is before it.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

const SAVE_JSON: &str = "save.json";
const CAMPAIGN_BIN: &str = "campaign.bin";
const SNAPSHOT_BIN: &str = "snapshot.bin";

/// Deflated, with a fixed timestamp. The fixed timestamp is not
/// load-bearing the way it is for `.mgcl` — it just keeps a re-save
/// of unchanged state from churning bytes.
fn member_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
}

/// Write a save to any seekable sink.
pub fn write<W: Write + Seek>(sink: W, save: &SavePackage) -> Result<(), MgcsError> {
    debug_assert_eq!(
        save.header.resume.is_some(),
        save.snapshot.is_some(),
        "header.resume and the payload must agree: the header is what the \
         menu trusts to know whether a slot resumes in play"
    );
    let mut zip = ZipWriter::new(sink);
    let opts = member_options();

    zip.start_file(SAVE_JSON, opts)?;
    zip.write_all(serde_json::to_string_pretty(&save.header)?.as_bytes())?;
    zip.write_all(b"\n")?;

    zip.start_file(CAMPAIGN_BIN, opts)?;
    zip.write_all(&save.campaign)?;

    if let Some(snapshot) = &save.snapshot {
        zip.start_file(SNAPSHOT_BIN, opts)?;
        zip.write_all(snapshot)?;
    }

    zip.finish()?;
    Ok(())
}

/// Serialize to bytes — the shape the app writes, since a save is
/// staged in memory and written in one go.
pub fn to_bytes(save: &SavePackage) -> Result<Vec<u8>, MgcsError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    write(&mut buf, save)?;
    Ok(buf.into_inner())
}

fn member_bytes<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &'static str,
) -> Result<Option<Vec<u8>>, MgcsError> {
    match zip.by_name(name) {
        Ok(mut f) => {
            let mut v = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut v)?;
            Ok(Some(v))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Just enough of `save.json` to read the version, and nothing else.
///
/// The version MUST be checked before the full header is parsed. A
/// version bump is precisely the case where the rest of the schema has
/// changed shape, so deserializing `SaveHeader` first fails on some
/// unrelated field and buries the one error that would have explained
/// it — the v1→v2 bump moved `level` from an object to a number, and an
/// old save reported "invalid type: map, expected u32" instead of
/// "save version 1, this build reads 2".
#[derive(Deserialize)]
struct VersionProbe {
    save_version: u32,
}

/// What survives a container-version mismatch.
///
/// Only fields whose SHAPE is stable across versions may appear here —
/// that is the whole contract. `level` is deliberately absent: the
/// v1→v2 bump changed it from an object to a number, and declaring it
/// would reintroduce the parse failure this type exists to route
/// around. Unknown fields are ignored by serde, so the rest of an
/// older (or newer) `save.json` simply passes by.
#[derive(Deserialize)]
struct RecoveryProbe {
    save_version: u32,
    #[serde(default)]
    label: String,
    #[serde(default)]
    campaign_level: u32,
}

/// A campaign record salvaged from a save this build cannot otherwise
/// read. See [`recover`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// The version the file was written at — worth reporting, since
    /// this is a degraded read.
    pub save_version: u32,
    pub label: String,
    pub campaign_level: u32,
    /// The retail campaign record, verbatim.
    pub campaign: Vec<u8>,
}

/// Salvage the campaign record from a save of ANY container version.
///
/// `campaign.bin` is the retail byte layout — pinned by the decompile
/// and by `Mc1Save`/`Mc2Save`, not by this container's schema — so it
/// is readable at any version by construction. The world payload is
/// NOT: its field order is `SNAPSHOT_VERSION`'s business and a stale
/// one cannot be applied. So a version mismatch costs the player their
/// resume, never their progress.
///
/// Used only after the normal read has failed on the version; callers
/// must surface the degradation rather than pass it off as a clean
/// load.
pub fn recover<R: Read + Seek>(source: R) -> Result<Recovered, MgcsError> {
    let mut zip = ZipArchive::new(source)?;
    let raw = member_bytes(&mut zip, SAVE_JSON)?.ok_or(MgcsError::MissingMember(SAVE_JSON))?;
    let probe: RecoveryProbe = serde_json::from_slice(&raw)?;
    let campaign =
        member_bytes(&mut zip, CAMPAIGN_BIN)?.ok_or(MgcsError::MissingMember(CAMPAIGN_BIN))?;
    Ok(Recovered {
        save_version: probe.save_version,
        label: probe.label,
        campaign_level: probe.campaign_level,
        campaign,
    })
}

fn header_of<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<SaveHeader, MgcsError> {
    let raw = member_bytes(zip, SAVE_JSON)?.ok_or(MgcsError::MissingMember(SAVE_JSON))?;
    let probe: VersionProbe = serde_json::from_slice(&raw)?;
    if probe.save_version != SAVE_VERSION {
        return Err(MgcsError::UnsupportedVersion(probe.save_version));
    }
    Ok(serde_json::from_slice(&raw)?)
}

/// Read just the header — the cheap probe the slot list uses. Does
/// not touch the ~570 KiB payload.
pub fn read_header<R: Read + Seek>(source: R) -> Result<SaveHeader, MgcsError> {
    header_of(&mut ZipArchive::new(source)?)
}

/// Read a whole save.
pub fn read<R: Read + Seek>(source: R) -> Result<SavePackage, MgcsError> {
    let mut zip = ZipArchive::new(source)?;
    let header = header_of(&mut zip)?;
    let campaign =
        member_bytes(&mut zip, CAMPAIGN_BIN)?.ok_or(MgcsError::MissingMember(CAMPAIGN_BIN))?;
    let snapshot = member_bytes(&mut zip, SNAPSHOT_BIN)?;
    // A header promising a level with no payload behind it would send
    // the app down the resume path with nothing to apply.
    if header.resume.is_some() && snapshot.is_none() {
        return Err(MgcsError::MissingMember(SNAPSHOT_BIN));
    }
    Ok(SavePackage {
        header,
        campaign,
        snapshot,
    })
}

/// A fresh header for a hub save (campaign progress only).
pub fn hub_header(game: Game, label: String, campaign_level: u32, level: u32) -> SaveHeader {
    SaveHeader {
        save_version: SAVE_VERSION,
        game,
        label,
        saved_unix: now_unix(),
        campaign_level,
        level,
        bake_epoch: BAKE_EPOCH,
        resume: None,
        mc1_spell_ring: None,
        mc1_quick_keys: None,
        mc2_map_stats: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign() -> Vec<u8> {
        (0..142u32).map(|i| i as u8).collect()
    }

    fn in_level() -> InLevel {
        InLevel {
            bundle: "mc1-temperate".into(),
            entry_sha256: "abc123".into(),
            snapshot_version: 1,
            tick: 4242,
            mana_pct: 15,
            thrust_model: Some("Mc1".into()),
            altitude_model: Some("Faithful".into()),
        }
    }

    #[test]
    fn hub_save_round_trips() {
        let save = SavePackage {
            header: hub_header(Game::MagicCarpet1, "WIZARD".into(), 7, 7),
            campaign: campaign(),
            snapshot: None,
        };
        let bytes = to_bytes(&save).unwrap();
        let back = read(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(back, save);
        assert!(
            !back.is_in_level(),
            "a hub slot resumes at the campaign screen"
        );
    }

    /// The owed MC2 stats tables ride the header (a save taken inside
    /// a secret level), and a header without them keeps its old shape.
    #[test]
    fn owed_map_stats_ride_the_header() {
        let mut header = hub_header(Game::MagicCarpet2, "SECRET".into(), 4, 30);
        header.resume = Some(in_level());
        header.mc2_map_stats = vec![4];
        let save = SavePackage {
            header,
            campaign: campaign(),
            snapshot: Some(vec![1, 2, 3]),
        };
        let back = read(std::io::Cursor::new(&to_bytes(&save).unwrap())).unwrap();
        assert_eq!(back.header.mc2_map_stats, vec![4]);
        let plain = hub_header(Game::MagicCarpet2, "HUB".into(), 4, 4);
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("mc2_map_stats"), "omitted while empty");
    }

    /// The MC1 quick keys ride the header; a header without them keeps
    /// its old shape, and a save from before the carry (no field at
    /// all) reads as "no keys" — the level seeds them from scratch.
    #[test]
    fn mc1_quick_keys_ride_the_header() {
        let mut header = hub_header(Game::MagicCarpet1, "KEYS".into(), 3, 3);
        let mut keys = [None; 10];
        keys[0] = Some(0);
        keys[9] = Some(17);
        header.mc1_quick_keys = Some(keys);
        let save = SavePackage {
            header,
            campaign: campaign(),
            snapshot: None,
        };
        let back = read(std::io::Cursor::new(&to_bytes(&save).unwrap())).unwrap();
        assert_eq!(back.header.mc1_quick_keys, Some(keys));
        let plain = hub_header(Game::MagicCarpet1, "HUB".into(), 3, 3);
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("mc1_quick_keys"), "omitted while unbound");
        let old: SaveHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(old.mc1_quick_keys, None);
    }

    #[test]
    fn in_level_save_round_trips() {
        let mut header = hub_header(Game::MagicCarpet2, "MID LEVEL".into(), 3, 3);
        header.resume = Some(in_level());
        let save = SavePackage {
            header,
            campaign: campaign(),
            snapshot: Some(vec![7u8; 4096]),
        };
        let bytes = to_bytes(&save).unwrap();
        let back = read(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(back, save);
        assert!(back.is_in_level());
    }

    /// The listing path must not need the payload — that is the whole
    /// reason the header is a separate member.
    #[test]
    fn header_reads_without_the_payload() {
        let mut header = hub_header(Game::MagicCarpet1, "SLOT".into(), 2, 5);
        header.resume = Some(in_level());
        let save = SavePackage {
            header: header.clone(),
            campaign: campaign(),
            snapshot: Some(vec![0u8; 600_000]),
        };
        let bytes = to_bytes(&save).unwrap();
        assert_eq!(read_header(std::io::Cursor::new(&bytes)).unwrap(), header);
    }

    /// Deflate is the reason this container differs from `.mgcl`; if
    /// it ever silently reverts to Stored, a slot grows ~6x.
    #[test]
    fn payload_is_compressed() {
        let mut header = hub_header(Game::MagicCarpet1, "BIG".into(), 1, 1);
        header.resume = Some(in_level());
        let save = SavePackage {
            header,
            campaign: campaign(),
            // Repetitive like real terrain, which is what makes the
            // compression worth having.
            snapshot: Some(vec![0u8; 600_000]),
        };
        let n = to_bytes(&save).unwrap().len();
        assert!(n < 60_000, "payload was not compressed: {n} bytes");
    }

    /// A verbatim v1 `save.json`, from before `level` was promoted out
    /// of the resume block. The point is that the version gate fires on
    /// a header whose SHAPE this build can no longer parse — that is
    /// the only situation a version number is for.
    #[test]
    fn an_old_schema_reports_its_version_not_a_parse_error() {
        const V1: &str = r#"{
          "save_version": 1,
          "game": "mc1",
          "label": "RAIN",
          "saved_unix": 1784621017,
          "campaign_level": 3,
          "bake_epoch": 18,
          "level": {
            "index": 3,
            "bundle": "mc1-temperate",
            "entry_sha256": "320f0177",
            "snapshot_version": 1,
            "tick": 182,
            "thrust_model": "Enhanced",
            "altitude_model": "ExtendedLift"
          }
        }"#;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            zip.start_file(SAVE_JSON, member_options()).unwrap();
            zip.write_all(V1.as_bytes()).unwrap();
            zip.start_file(CAMPAIGN_BIN, member_options()).unwrap();
            zip.write_all(&campaign()).unwrap();
            zip.start_file(SNAPSHOT_BIN, member_options()).unwrap();
            zip.write_all(&[0u8; 16]).unwrap();
            zip.finish().unwrap();
        }
        let bytes = buf.into_inner();
        match read(std::io::Cursor::new(&bytes)) {
            Err(MgcsError::UnsupportedVersion(1)) => {}
            other => panic!("expected UnsupportedVersion(1), got {other:?}"),
        }
        // And the cheap listing probe agrees, so the slot lists as
        // incompatible rather than blowing up.
        assert!(matches!(
            read_header(std::io::Cursor::new(&bytes)),
            Err(MgcsError::UnsupportedVersion(1))
        ));
    }

    /// The salvage path: a v1 save this build cannot read still yields
    /// its campaign record, because that record is retail's byte
    /// layout rather than ours.
    #[test]
    fn a_v1_save_still_yields_its_campaign_record() {
        const V1: &str = r#"{
          "save_version": 1,
          "game": "mc1",
          "label": "RAIN",
          "saved_unix": 1784621017,
          "campaign_level": 3,
          "bake_epoch": 18,
          "level": { "index": 3, "bundle": "mc1-temperate", "tick": 182 }
        }"#;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            zip.start_file(SAVE_JSON, member_options()).unwrap();
            zip.write_all(V1.as_bytes()).unwrap();
            zip.start_file(CAMPAIGN_BIN, member_options()).unwrap();
            zip.write_all(&campaign()).unwrap();
            zip.start_file(SNAPSHOT_BIN, member_options()).unwrap();
            zip.write_all(&[0u8; 16]).unwrap();
            zip.finish().unwrap();
        }
        let bytes = buf.into_inner();
        // The ordinary read still refuses it, loudly and by version.
        assert!(matches!(
            read(std::io::Cursor::new(&bytes)),
            Err(MgcsError::UnsupportedVersion(1))
        ));
        // The salvage read gets the progress out regardless — note it
        // parses a `level` that is an OBJECT here, the very field whose
        // type changed.
        let r = recover(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(r.save_version, 1);
        assert_eq!(r.label, "RAIN");
        assert_eq!(r.campaign_level, 3);
        assert_eq!(r.campaign, campaign());
    }

    #[test]
    fn rejects_a_future_version() {
        let save = SavePackage {
            header: SaveHeader {
                save_version: SAVE_VERSION + 1,
                ..hub_header(Game::MagicCarpet1, "FUTURE".into(), 0, 0)
            },
            campaign: campaign(),
            snapshot: None,
        };
        // Written straight through `write`, which does not gate on the
        // version — reading is where the check belongs.
        let bytes = to_bytes(&save).unwrap();
        assert!(matches!(
            read(std::io::Cursor::new(&bytes)),
            Err(MgcsError::UnsupportedVersion(_))
        ));
    }

    /// A header that promises a mid-level resume with no payload
    /// behind it must not reach the app's resume path.
    #[test]
    fn rejects_a_header_without_its_payload() {
        let mut header = hub_header(Game::MagicCarpet1, "TORN".into(), 1, 1);
        header.resume = Some(in_level());
        // Build the ZIP by hand: `write` debug-asserts against this.
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            zip.start_file(SAVE_JSON, member_options()).unwrap();
            zip.write_all(serde_json::to_string(&header).unwrap().as_bytes())
                .unwrap();
            zip.start_file(CAMPAIGN_BIN, member_options()).unwrap();
            zip.write_all(&campaign()).unwrap();
            zip.finish().unwrap();
        }
        assert!(matches!(
            read(std::io::Cursor::new(buf.into_inner())),
            Err(MgcsError::MissingMember(SNAPSHOT_BIN))
        ));
    }
}
