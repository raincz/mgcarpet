//! Mid-level world snapshots — the sim half of the save format
//! (`docs/archive/DESIGN-SAVES.md`).
//!
//! Both retail engines snapshot by dumping their master struct to
//! disk as a raw RAM image (MC1 `save/gam00199.dat`, MC2 `SLEV*.DAT`).
//! Those images carry absolute 32-bit pointers, so neither is
//! readable by anything but the executable that wrote it, and no
//! interop is possible in either direction. This is our own format:
//! an explicit little-endian byte stream, written field by field.
//!
//! # Why it is hand-written
//!
//! `mgc-sim` depends on nothing but `mgc-formats`, deliberately. That
//! stance is worth more here than the typing it saves: a derived
//! codec follows declaration order implicitly, so adding a field to
//! `Gen` would silently start round-tripping it (or, worse, silently
//! stop). Every struct below is written through an exhaustive
//! DESTRUCTURE and read back through an exhaustive struct LITERAL, so
//! both directions fail to compile when a field appears — the same
//! discipline `World::state_hash` and `Simulation::state_hash` use.
//!
//! # What is not in the stream
//!
//! Three things are excluded and re-supplied by the caller, because
//! they are properties of the LEVEL PACKAGE rather than of the run:
//! `Gen::assets`, `Gen::retile`, and `ChassisParams`'s
//! `&'static [u8]`. Restore is consequently an APPLY onto an
//! already-built world, not a constructor — resolve the level, build
//! it the way a fresh start would, then overwrite the state. That is
//! also exactly the shape of the app's existing restart path.
//!
//! To make that safe, the header carries an IDENTITY fingerprint (the
//! game, the chassis scalars, the verb column, the pool and table
//! sizes). Restoring into a world that disagrees is refused rather
//! than half-applied: a pool of a different size would renumber every
//! slot handle in the stream.

use crate::{AltitudeModel, Flyer, Simulation, ThrustModel};

/// `MGCS`, little-endian.
const MAGIC: u32 = 0x5343_474D;

/// Stream version. Bump on any layout change; there is no
/// forward compatibility and none is wanted — a snapshot is a
/// short-lived artifact pinned to a level package.
///
/// 2: `Gen::pal_flash` (the Global Death palette wash) joined the
///    stream. A v1 save read as v2 would pass the identity gate — that
///    is written ahead of the payload — and then apply every field
///    after the insertion point SHIFTED, so the version check is what
///    stands between an old save and a silently mangled world.
/// 3: the enhanced-flight steering/altitude state (`turn_rate`,
///    `turn_grace`, `lift_desired`) joined the flight tier.
/// 4: chase-the-pointer steering replaced the turn-rate damper —
///    `turn_grace: u8` left the stream, `aim_lead: f32` joined (any
///    v3 saves from the one damper playtest round refuse cleanly).
/// 7: `Gen::rival_wanted` (the per-rival village-wanted timers, so
///    militia and griffons turn on hostile rival wizards, not only the
///    human) joined the stream after `player_aggro`.
/// 8: the teleport family's retail z hand-off — `Player::
///    teleport_return` widened to the full saved axis (x, y, z),
///    `World::pending_teleport` gained the arrival altitude,
///    `World::pending_speed_zero` joined the stream after it.
/// 10: `Gen::mc1_guard_reg` — the per-owner castle-guard register
///    (wizext+84) appended to the Gen stream.
/// 11: the speed-token flight seam — `World::pending_speed_base` (the
///    burst-end signed-base restore mail, ±80) and `World::mc1_v14`
///    (retail's Type_160 v_14 speed-touched latch) joined the stream
///    after `pending_speed_zero`.
/// 12: `Rival::life_rate` — the AI life-regen rate REGISTER (retail
///    u16_341, applied-then-selected) joined the rival record after
///    `regen_stall`.
/// 13: `Rival::knock_dir` / `Rival::knock_mag` — the pending
///    knockback impulse (retail Type_160 v_24/v_22) joined the rival
///    record after `jink`. A live rival never spends it; its death
///    fall does.
pub const SNAPSHOT_VERSION: u32 = 13;

/// Why a snapshot could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// Not a snapshot stream at all.
    BadMagic(u32),
    /// Written by a different version of the codec.
    Version(u32),
    /// The stream ended mid-field, or a length ran past its end.
    Truncated,
    /// A tag byte named a variant this build does not have.
    BadTag { what: &'static str, tag: u8 },
    /// A string member was not valid UTF-8.
    BadString,
    /// The snapshot describes a different world than the one it is
    /// being applied to — a different game, chassis, verb column, or
    /// pool geometry. Slot handles would not survive the mismatch.
    Identity { what: &'static str },
    /// Bytes remained after the last field. Always a codec bug (a
    /// writer and reader that disagree), never bad input alone, so it
    /// is worth failing loudly on.
    Trailing(usize),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic(m) => write!(f, "not a snapshot (magic {m:#010x})"),
            Self::Version(v) => write!(
                f,
                "snapshot version {v}, this build reads {SNAPSHOT_VERSION}"
            ),
            Self::Truncated => write!(f, "snapshot ends mid-field"),
            Self::BadTag { what, tag } => write!(f, "snapshot has no {what} variant {tag}"),
            Self::BadString => write!(f, "snapshot string is not UTF-8"),
            Self::Identity { what } => {
                write!(f, "snapshot is for a different world ({what} differs)")
            }
            Self::Trailing(n) => write!(f, "{n} bytes left over after the snapshot"),
        }
    }
}

impl std::error::Error for SnapshotError {}

// ------------------------------------------------------------ stream

/// Append-only little-endian byte sink.
pub(crate) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    /// The finished stream — used by the conformance world dump, which
    /// serializes one section at a time rather than one whole sim.
    pub(crate) fn into_buf(self) -> Vec<u8> {
        self.buf
    }

    pub(crate) fn raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Write any [`Snap`] value. Named short because the struct
    /// bodies below are nothing but long runs of these.
    pub(crate) fn put<T: Snap>(&mut self, v: &T) {
        v.put(self);
    }
}

/// Cursor over a snapshot stream.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub(crate) fn raw(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        if self.remaining() < n {
            return Err(SnapshotError::Truncated);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Read any [`Snap`] value.
    pub(crate) fn get<T: Snap>(&mut self) -> Result<T, SnapshotError> {
        T::get(self)
    }

    /// Read an identity field and require it to match the live value.
    pub(crate) fn expect<T: Snap + PartialEq>(
        &mut self,
        what: &'static str,
        want: T,
    ) -> Result<(), SnapshotError> {
        if self.get::<T>()? == want {
            Ok(())
        } else {
            Err(SnapshotError::Identity { what })
        }
    }
}

/// A value that round-trips through the stream.
///
/// Implemented for the primitives and containers here, and for the
/// sim's own types beside their definitions (privacy: `World`'s
/// fields are not visible from this module, and should not be).
pub(crate) trait Snap: Sized {
    fn put(&self, w: &mut Writer);
    fn get(r: &mut Reader) -> Result<Self, SnapshotError>;
}

macro_rules! snap_int {
    ($($t:ty),*) => {$(
        impl Snap for $t {
            fn put(&self, w: &mut Writer) {
                w.raw(&self.to_le_bytes());
            }
            fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
                let n = std::mem::size_of::<$t>();
                Ok(<$t>::from_le_bytes(r.raw(n)?.try_into().unwrap()))
            }
        }
    )*};
}
snap_int!(u8, u16, u32, u64, i8, i16, i32, i64);

impl Snap for bool {
    fn put(&self, w: &mut Writer) {
        w.put(&(*self as u8));
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        // Any nonzero reads as true rather than erroring: this is the
        // one field shape where a stricter rule buys nothing.
        Ok(r.get::<u8>()? != 0)
    }
}

/// Floats go by BIT PATTERN, never by decimal text — the same rule
/// the hashes follow. `-0.0` must not come back as `0.0`, and a NaN
/// payload must survive.
impl Snap for f32 {
    fn put(&self, w: &mut Writer) {
        w.put(&self.to_bits());
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(f32::from_bits(r.get::<u32>()?))
    }
}

/// `usize` is narrowed to `u32` on the wire so a snapshot is not
/// pointer-width dependent. Nothing in the sim is anywhere near 4 G.
impl Snap for usize {
    fn put(&self, w: &mut Writer) {
        w.put(&(*self as u32));
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(r.get::<u32>()? as usize)
    }
}

impl<T: Snap> Snap for Option<T> {
    fn put(&self, w: &mut Writer) {
        match self {
            None => w.put(&0u8),
            Some(v) => {
                w.put(&1u8);
                w.put(v);
            }
        }
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        match r.get::<u8>()? {
            0 => Ok(None),
            1 => Ok(Some(r.get()?)),
            tag => Err(SnapshotError::BadTag {
                what: "Option",
                tag,
            }),
        }
    }
}

impl<T: Snap> Snap for Vec<T> {
    fn put(&self, w: &mut Writer) {
        w.put(&(self.len() as u32));
        for v in self {
            w.put(v);
        }
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        let n = r.get::<u32>()? as usize;
        // Every element costs at least one byte, so a length past the
        // remaining stream is corrupt. Checked BEFORE reserving, or a
        // truncated file turns into a multi-gigabyte allocation.
        if n > r.remaining() {
            return Err(SnapshotError::Truncated);
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(r.get()?);
        }
        Ok(v)
    }
}

impl<T: Snap, const N: usize> Snap for [T; N] {
    fn put(&self, w: &mut Writer) {
        for v in self {
            w.put(v);
        }
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        let mut v = Vec::with_capacity(N);
        for _ in 0..N {
            v.push(r.get()?);
        }
        // Length is N by construction; the map_err is for the type,
        // not for a case that can happen.
        v.try_into().map_err(|_| SnapshotError::Truncated)
    }
}

impl<A: Snap, B: Snap> Snap for (A, B) {
    fn put(&self, w: &mut Writer) {
        w.put(&self.0);
        w.put(&self.1);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok((r.get()?, r.get()?))
    }
}

impl<A: Snap, B: Snap, C: Snap> Snap for (A, B, C) {
    fn put(&self, w: &mut Writer) {
        w.put(&self.0);
        w.put(&self.1);
        w.put(&self.2);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok((r.get()?, r.get()?, r.get()?))
    }
}

impl Snap for String {
    fn put(&self, w: &mut Writer) {
        w.put(&(self.len() as u32));
        w.raw(self.as_bytes());
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        let n = r.get::<u32>()? as usize;
        let b = r.raw(n)?;
        std::str::from_utf8(b)
            .map(str::to_owned)
            .map_err(|_| SnapshotError::BadString)
    }
}

impl Snap for std::collections::BTreeMap<u16, u16> {
    fn put(&self, w: &mut Writer) {
        w.put(&(self.len() as u32));
        for (k, v) in self {
            w.put(k);
            w.put(v);
        }
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        let n = r.get::<u32>()? as usize;
        // Four bytes per entry, so this bound is generous but still
        // keeps a corrupt length from pre-allocating wildly.
        if n > r.remaining() {
            return Err(SnapshotError::Truncated);
        }
        let mut m = std::collections::BTreeMap::new();
        for _ in 0..n {
            let k = r.get()?;
            m.insert(k, r.get()?);
        }
        Ok(m)
    }
}

impl Snap for std::collections::BTreeMap<u16, Vec<u16>> {
    fn put(&self, w: &mut Writer) {
        w.put(&(self.len() as u32));
        for (k, v) in self {
            w.put(k);
            w.put(v);
        }
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        let n = r.get::<u32>()? as usize;
        if n > r.remaining() {
            return Err(SnapshotError::Truncated);
        }
        let mut m = std::collections::BTreeMap::new();
        for _ in 0..n {
            let k = r.get()?;
            m.insert(k, r.get()?);
        }
        Ok(m)
    }
}

/// Field-less enums travel as a tag byte. The `put` match is
/// exhaustive and the `get` match names every tag, so a new variant
/// breaks the build on both sides.
macro_rules! snap_enum {
    ($t:ty, $name:literal, $($tag:literal => $variant:path),+ $(,)?) => {
        impl $crate::snapshot::Snap for $t {
            fn put(&self, w: &mut $crate::snapshot::Writer) {
                let tag: u8 = match self { $($variant => $tag),+ };
                w.put(&tag);
            }
            fn get(
                r: &mut $crate::snapshot::Reader,
            ) -> Result<Self, $crate::snapshot::SnapshotError> {
                match r.get::<u8>()? {
                    $($tag => Ok($variant),)+
                    tag => Err($crate::snapshot::SnapshotError::BadTag { what: $name, tag }),
                }
            }
        }
    };
}
pub(crate) use snap_enum;

snap_enum!(ThrustModel, "ThrustModel", 0 => ThrustModel::Mc1, 1 => ThrustModel::Enhanced);
snap_enum!(
    AltitudeModel,
    "AltitudeModel",
    0 => AltitudeModel::Faithful,
    1 => AltitudeModel::ExtendedLift,
);

impl Snap for Flyer {
    fn put(&self, w: &mut Writer) {
        let Flyer {
            x,
            y,
            z,
            vx,
            vy,
            vz,
            yaw,
            pitch,
            roll,
        } = self;
        for f in [x, y, z, vx, vy, vz, yaw, pitch, roll] {
            w.put(f);
        }
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Flyer {
            x: r.get()?,
            y: r.get()?,
            z: r.get()?,
            vx: r.get()?,
            vy: r.get()?,
            vz: r.get()?,
            yaw: r.get()?,
            pitch: r.get()?,
            roll: r.get()?,
        })
    }
}

// ------------------------------------------------------- the top level

impl Simulation {
    /// Serialize the whole sim: the flight tier out here plus the
    /// attached world, if any.
    ///
    /// Excludes the level package's own contribution (`Gen::assets`,
    /// `Gen::retile`) — see the module docs. The result is meaningful
    /// only against the level it was taken in, which the header's
    /// identity fingerprint enforces as far as it can and the save
    /// container's `entry_sha256` enforces the rest of the way.
    pub fn snapshot(&self) -> Vec<u8> {
        let Simulation {
            tick,
            flyer,
            thrust_model,
            altitude_model,
            carpet,
            carpet_mc2,
            accel_was_active,
            turn_rate,
            aim_lead,
            lift_desired,
            // Dev config, not game state: re-armed by the app from
            // `dev.lift_unclamped`, never carried through a save.
            lift_unclamped: _,
            broll,
            terrain_height,
            world,
        } = self;

        let mut w = Writer::new();
        w.put(&MAGIC);
        w.put(&SNAPSHOT_VERSION);
        // Identity first, so a mismatched stream is refused before a
        // single field is applied.
        match world {
            Some(world) => {
                w.put(&1u8);
                world.snap_identity(&mut w);
            }
            None => w.put(&0u8),
        }

        w.put(tick);
        w.put(flyer);
        w.put(thrust_model);
        w.put(altitude_model);
        w.put(carpet);
        w.put(carpet_mc2);
        w.put(accel_was_active);
        w.put(turn_rate);
        w.put(aim_lead);
        w.put(lift_desired);
        w.put(broll);
        w.put(terrain_height);
        // The world is written inline rather than through `Snap`: it
        // has no `get` side (it cannot be built from the stream, only
        // applied onto a live one), so giving it an impl would mean an
        // impl with an unreachable half.
        match world {
            Some(world) => {
                w.put(&1u8);
                world.snap_write(&mut w);
            }
            None => w.put(&0u8),
        }
        w.buf
    }

    /// Apply a snapshot onto this sim IN PLACE.
    ///
    /// The sim must already hold a world built for the same level —
    /// the snapshot carries no assets, so there is nothing to
    /// construct from. Build the level exactly as a fresh start
    /// would, then call this.
    ///
    /// Nothing is written until the identity fingerprint checks out,
    /// so a rejected snapshot leaves the sim untouched and playable.
    /// A snapshot that fails PART WAY through (a truncated stream)
    /// does not: the sim is left half-applied and the caller must
    /// rebuild the level. Callers treat any error that way.
    pub fn restore(&mut self, bytes: &[u8]) -> Result<(), SnapshotError> {
        let mut r = Reader::new(bytes);
        let magic = r.get::<u32>()?;
        if magic != MAGIC {
            return Err(SnapshotError::BadMagic(magic));
        }
        let version = r.get::<u32>()?;
        if version != SNAPSHOT_VERSION {
            return Err(SnapshotError::Version(version));
        }
        // The identity gate. Both sides must agree on whether a world
        // is attached at all, and on its geometry if so.
        let had_world = r.get::<u8>()? != 0;
        match (had_world, self.world.as_ref()) {
            (true, Some(world)) => world.snap_check_identity(&mut r)?,
            (false, None) => {}
            _ => {
                return Err(SnapshotError::Identity {
                    what: "world presence",
                });
            }
        }

        self.tick = r.get()?;
        self.flyer = r.get()?;
        self.thrust_model = r.get()?;
        self.altitude_model = r.get()?;
        self.carpet = r.get()?;
        self.carpet_mc2 = r.get()?;
        self.accel_was_active = r.get()?;
        self.turn_rate = r.get()?;
        self.aim_lead = r.get()?;
        self.lift_desired = r.get()?;
        self.broll = r.get()?;
        self.terrain_height = r.get()?;
        // The world applies onto itself, keeping assets and retile.
        if r.get::<u8>()? != 0 {
            let Some(world) = self.world.as_mut() else {
                return Err(SnapshotError::Identity {
                    what: "world presence",
                });
            };
            world.snap_apply(&mut r)?;
        }

        if r.remaining() != 0 {
            return Err(SnapshotError::Trailing(r.remaining()));
        }
        Ok(())
    }
}
