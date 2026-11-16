//! `.mgcl` container I/O: a ZIP archive with all members stored
//! uncompressed, written deterministically (fixed timestamps) so that
//! identical content produces identical bytes — a requirement for the
//! committed-hash pinning strategy. See docs/FORMAT.md.

use std::io::{Read, Seek, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{
    FORMAT_VERSION, GenParams, LevelHeader, LevelPackage, Meta, Stages, TERRAIN_GRID_BYTES,
    Terrain, Things, Wizards,
};

#[derive(Debug)]
pub enum MgclError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Json(serde_json::Error),
    /// A required member is missing.
    MissingMember(&'static str),
    /// `meta.json` declares a format version this build doesn't know.
    UnsupportedVersion(u32),
}

impl std::fmt::Display for MgclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Zip(e) => write!(f, "container: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
            Self::MissingMember(name) => write!(f, "missing required member {name}"),
            Self::UnsupportedVersion(v) => {
                write!(
                    f,
                    "format version {v} unsupported (this build knows {FORMAT_VERSION})"
                )
            }
        }
    }
}

impl std::error::Error for MgclError {}

impl From<std::io::Error> for MgclError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<zip::result::ZipError> for MgclError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::Zip(e)
    }
}
impl From<serde_json::Error> for MgclError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Stored, deterministic member options: compression off, timestamp
/// fixed to the ZIP epoch (1980-01-01).
fn member_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
}

/// Write a package to any seekable sink.
pub fn write<W: Write + Seek>(sink: W, package: &LevelPackage) -> Result<(), MgclError> {
    let mut zip = ZipWriter::new(sink);
    let opts = member_options();

    let member = |zip: &mut ZipWriter<W>, name: &str, json: String| -> Result<(), MgclError> {
        zip.start_file(name, opts)?;
        zip.write_all(json.as_bytes())?;
        zip.write_all(b"\n")?;
        Ok(())
    };

    member(
        &mut zip,
        "meta.json",
        serde_json::to_string_pretty(&package.meta)?,
    )?;
    member(
        &mut zip,
        "things.json",
        serde_json::to_string_pretty(&package.things)?,
    )?;
    if let Some(gen_params) = &package.gen_params {
        member(
            &mut zip,
            "genparams.json",
            serde_json::to_string_pretty(gen_params)?,
        )?;
    }
    if let Some(header) = &package.header {
        member(
            &mut zip,
            "level.json",
            serde_json::to_string_pretty(header)?,
        )?;
    }
    if let Some(wizards) = &package.wizards {
        member(
            &mut zip,
            "wizards.json",
            serde_json::to_string_pretty(wizards)?,
        )?;
    }
    if let Some(stages) = &package.stages {
        member(
            &mut zip,
            "stages.json",
            serde_json::to_string_pretty(stages)?,
        )?;
    }
    if let Some(terrain) = &package.terrain {
        zip.start_file("terrain/height.bin", opts)?;
        zip.write_all(&terrain.height)?;
        zip.start_file("terrain/type.bin", opts)?;
        zip.write_all(&terrain.tile_type)?;
        if let Some(shading) = &terrain.shading {
            zip.start_file("terrain/shading.bin", opts)?;
            zip.write_all(shading)?;
        }
        if let Some(angle) = &terrain.angle {
            zip.start_file("terrain/angle.bin", opts)?;
            zip.write_all(angle)?;
        }
        if let Some(ceiling) = &terrain.ceiling {
            zip.start_file("terrain/ceiling.bin", opts)?;
            zip.write_all(ceiling)?;
        }
    }

    zip.finish()?;
    Ok(())
}

/// Read only `meta.json` from a package — the cheap staleness probe
/// (version/epoch checks) that avoids parsing the whole package. Does
/// NOT reject unsupported versions; the caller inspects the fields.
pub fn read_meta<R: Read + Seek>(source: R) -> Result<Meta, MgclError> {
    let mut zip = ZipArchive::new(source)?;
    let mut file = match zip.by_name("meta.json") {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => {
            return Err(MgclError::MissingMember("meta.json"));
        }
        Err(e) => return Err(e.into()),
    };
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)?;
    Ok(serde_json::from_slice(&buf)?)
}

/// Read a package from any seekable source. Unknown members are ignored
/// here; tools that rewrite packages must copy them through (docs/FORMAT.md).
pub fn read<R: Read + Seek>(source: R) -> Result<LevelPackage, MgclError> {
    let mut zip = ZipArchive::new(source)?;

    fn member_bytes<R: Read + Seek>(
        zip: &mut ZipArchive<R>,
        name: &'static str,
    ) -> Result<Option<Vec<u8>>, MgclError> {
        match zip.by_name(name) {
            Ok(mut file) => {
                let mut buf = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut buf)?;
                Ok(Some(buf))
            }
            Err(zip::result::ZipError::FileNotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    let meta_bytes =
        member_bytes(&mut zip, "meta.json")?.ok_or(MgclError::MissingMember("meta.json"))?;
    let meta: Meta = serde_json::from_slice(&meta_bytes)?;
    if meta.format_version > FORMAT_VERSION {
        return Err(MgclError::UnsupportedVersion(meta.format_version));
    }

    let things_bytes =
        member_bytes(&mut zip, "things.json")?.ok_or(MgclError::MissingMember("things.json"))?;
    let things: Things = serde_json::from_slice(&things_bytes)?;

    let gen_params: Option<GenParams> = member_bytes(&mut zip, "genparams.json")?
        .map(|b| serde_json::from_slice(&b))
        .transpose()?;
    let header: Option<LevelHeader> = member_bytes(&mut zip, "level.json")?
        .map(|b| serde_json::from_slice(&b))
        .transpose()?;
    let wizards: Option<Wizards> = member_bytes(&mut zip, "wizards.json")?
        .map(|b| serde_json::from_slice(&b))
        .transpose()?;
    let stages: Option<Stages> = member_bytes(&mut zip, "stages.json")?
        .map(|b| serde_json::from_slice(&b))
        .transpose()?;

    let height = member_bytes(&mut zip, "terrain/height.bin")?;
    let tile_type = member_bytes(&mut zip, "terrain/type.bin")?;
    let shading =
        member_bytes(&mut zip, "terrain/shading.bin")?.filter(|s| s.len() == TERRAIN_GRID_BYTES);
    let angle =
        member_bytes(&mut zip, "terrain/angle.bin")?.filter(|s| s.len() == TERRAIN_GRID_BYTES);
    let ceiling =
        member_bytes(&mut zip, "terrain/ceiling.bin")?.filter(|s| s.len() == TERRAIN_GRID_BYTES);
    let terrain = match (height, tile_type) {
        (Some(height), Some(tile_type))
            if height.len() == TERRAIN_GRID_BYTES && tile_type.len() == TERRAIN_GRID_BYTES =>
        {
            Some(Terrain {
                height,
                tile_type,
                shading,
                angle,
                ceiling,
            })
        }
        (None, None) => None,
        _ => {
            return Err(MgclError::MissingMember(
                "terrain/*.bin (pair, 65536 B each)",
            ));
        }
    };

    Ok(LevelPackage {
        meta,
        things,
        gen_params,
        header,
        wizards,
        stages,
        terrain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Game, Importer, Source, Thing, ThingKind};
    use std::io::Cursor;

    fn sample() -> LevelPackage {
        LevelPackage {
            meta: Meta {
                format_version: FORMAT_VERSION,
                bake_epoch: crate::BAKE_EPOCH,
                game: Game::MagicCarpet1,
                level: 0,
                source: Some(Source {
                    archive: "LEVELS.DAT".into(),
                    entry: 0,
                    entry_sha256: "00".repeat(32),
                }),
                overlay: None,
                importer: Importer {
                    name: "mgc-import".into(),
                    version: "0.1.0".into(),
                },
            },
            things: Things {
                things: vec![Thing {
                    slot: 7,
                    kind: ThingKind::Entity,
                    class: 5,
                    model: 0,
                    x: 100,
                    y: 200,
                    dis_id: 0xFFFF,
                    swi_sz: 0,
                    swi_id: 0xFFFF,
                    parent: 0,
                    child: 0,
                    par3: None,
                }],
            },
            header: None,
            wizards: None,
            stages: None,
            terrain: None,
            gen_params: Some(GenParams {
                pre_header: Some(135538),
                seed: 1921,
                off: 41339,
                raise: -1010,
                gnarl: 0,
                river: 1,
                lriver: None,
                sourc: 0,
                snlin: 200,
                snflt: 50,
                bhlin: 30,
                bhflt: 16,
                rkste: 18,
                footer: Some([35, 1, 0, 0, 0, 0]),
            }),
        }
    }

    fn write_to_vec(package: &LevelPackage) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        write(&mut buf, package).unwrap();
        buf.into_inner()
    }

    #[test]
    fn round_trips() {
        let package = sample();
        let bytes = write_to_vec(&package);
        let loaded = read(Cursor::new(&bytes)).unwrap();
        assert_eq!(loaded, package);
    }

    #[test]
    fn output_is_deterministic() {
        assert_eq!(write_to_vec(&sample()), write_to_vec(&sample()));
    }

    #[test]
    fn members_are_stored_uncompressed() {
        let bytes = write_to_vec(&sample());
        let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        for i in 0..zip.len() {
            let file = zip.by_index(i).unwrap();
            assert_eq!(
                file.compression(),
                CompressionMethod::Stored,
                "member {} is compressed",
                file.name().unwrap_or_default()
            );
        }
    }

    #[test]
    fn terrain_round_trips() {
        use crate::{TERRAIN_GRID_BYTES, Terrain};
        let mut package = sample();
        package.terrain = Some(Terrain {
            height: (0..TERRAIN_GRID_BYTES).map(|i| (i % 251) as u8).collect(),
            tile_type: vec![7; TERRAIN_GRID_BYTES],
            shading: None,
            angle: None,
            ceiling: None,
        });
        let bytes = write_to_vec(&package);
        let loaded = read(Cursor::new(&bytes)).unwrap();
        assert_eq!(loaded, package);

        package.terrain.as_mut().unwrap().shading = Some(vec![33; TERRAIN_GRID_BYTES]);
        package.terrain.as_mut().unwrap().angle = Some(vec![0x50; TERRAIN_GRID_BYTES]);
        package.terrain.as_mut().unwrap().ceiling = Some(vec![44; TERRAIN_GRID_BYTES]);
        let bytes = write_to_vec(&package);
        let loaded = read(Cursor::new(&bytes)).unwrap();
        assert_eq!(loaded, package);
    }

    #[test]
    fn genparams_is_optional() {
        let mut package = sample();
        package.gen_params = None;
        let bytes = write_to_vec(&package);
        let loaded = read(Cursor::new(&bytes)).unwrap();
        assert_eq!(loaded.gen_params, None);
    }

    #[test]
    fn rejects_future_version() {
        let mut package = sample();
        package.meta.format_version = FORMAT_VERSION + 1;
        let bytes = write_to_vec(&package);
        assert!(matches!(
            read(Cursor::new(&bytes)),
            Err(MgclError::UnsupportedVersion(_))
        ));
    }
}
