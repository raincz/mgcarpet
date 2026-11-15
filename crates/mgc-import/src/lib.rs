//! The importer/baker: the only code in the project that understands
//! original Bullfrog data formats.
//!
//! Everything here runs at import time, once per machine. Output is baked
//! packages (`mgc-formats`) that the engine consumes without knowing
//! anything about RNC, DAT/TAB, seeds, or XMI.
//!
//! Reference implementations: remc2 (`reference/remc2`) for MC2 behavior,
//! remc1 (`reference/remc1`) for MC1 behavior, Moburma's tools and
//! michaelhoward's MagicCarpetFileFormat spec for MC1 formats.

pub mod adlib;
pub mod bake;
pub mod bundle;
pub mod cdtracks;
pub mod dattab;
pub mod flac;
pub mod flc;
pub mod fmv;
pub mod gamedata;
pub mod hmp;
pub mod hscreen;
pub mod hspr;
pub mod impostors;
pub mod iso;
pub mod level_mc1;
pub mod level_mc2;
pub mod mc1_terrain;
pub mod mc2_music;
pub mod mc2_terrain;
pub mod overlay;
pub mod redbook;
pub mod rnc;
pub mod smf;
pub mod sound;
pub mod sources;
pub mod sprites;
pub mod synth;
pub mod tmaps;
pub mod xmi;
