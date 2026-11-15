//! `mgc-import` — command-line importer/baker.
//!
//! Current commands operate at the container level (RNC); format-aware
//! import (levels, sprites, sounds) will grow on top.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mgc_import::{bake, dattab, level_mc1, rnc};

/// Print one line to stdout; false when the reader went away (e.g.
/// piped into `head`), so bulk listings can stop instead of panicking
/// on the broken pipe.
fn out(line: std::fmt::Arguments) -> bool {
    writeln!(std::io::stdout(), "{line}").is_ok()
}

macro_rules! outln {
    ($($arg:tt)*) => {
        if !out(format_args!($($arg)*)) {
            return ExitCode::SUCCESS;
        }
    };
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("scan") if args.len() == 2 => scan(Path::new(&args[1])),
        Some("unpack") if args.len() == 2 || args.len() == 3 => {
            unpack(Path::new(&args[1]), args.get(2).map(PathBuf::from))
        }
        Some("archive") if args.len() == 3 => archive(Path::new(&args[1]), Path::new(&args[2])),
        Some("extract") if args.len() == 5 => extract(
            Path::new(&args[1]),
            Path::new(&args[2]),
            &args[3],
            Path::new(&args[4]),
        ),
        Some("level") if args.len() == 4 => {
            level(Path::new(&args[1]), Path::new(&args[2]), &args[3])
        }
        Some("bake") if args.len() == 3 => bake_cmd(Path::new(&args[1]), Path::new(&args[2])),
        Some("sources") if args.len() == 2 => sources_cmd(Path::new(&args[1])),
        Some("add") if args.len() == 3 || (args.len() == 4 && args[3] == "--replace") => {
            add_cmd(Path::new(&args[1]), Path::new(&args[2]), args.len() == 4)
        }
        _ => {
            eprintln!("mgc-import — Magic Carpet data importer/baker\n");
            eprintln!("Usage:");
            eprintln!(
                "  mgc-import scan <dir>             recursively find and verify RNC containers"
            );
            eprintln!(
                "  mgc-import unpack <file> [out]    decompress one RNC file (default: <file>.unpacked)"
            );
            eprintln!("  mgc-import archive <DAT> <TAB>    list a DAT/TAB archive's entries");
            eprintln!("  mgc-import extract <DAT> <TAB> <index> <out>  extract one archive entry");
            eprintln!("  mgc-import level <DAT> <TAB> <index>  inspect an MC1 level");
            eprintln!("  mgc-import bake <gamedata> <out>  bake all levels into .mgcl packages");
            eprintln!(
                "  mgc-import sources <gamedata>     list the game data found, in pecking order"
            );
            eprintln!(
                "  mgc-import add <gamedata> <path> [--replace]  import a GOG install,\n\
                 \x20                                    GOG installer or CD image"
            );
            ExitCode::from(2)
        }
    }
}

fn open_archive(dat_path: &Path, tab_path: &Path) -> Result<dattab::Archive, ExitCode> {
    let read = |p: &Path| {
        std::fs::read(p).map_err(|e| {
            eprintln!("error: cannot read {}: {e}", p.display());
            ExitCode::FAILURE
        })
    };
    dattab::Archive::open(&read(dat_path)?, &read(tab_path)?).map_err(|e| {
        eprintln!("error: {}: {e}", dat_path.display());
        ExitCode::FAILURE
    })
}

fn archive(dat_path: &Path, tab_path: &Path) -> ExitCode {
    let archive = match open_archive(dat_path, tab_path) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let mut failed = 0u32;
    let mut shown = 0u32;
    for entry in archive.non_empty() {
        shown += 1;
        let raw = archive.raw(entry);
        if rnc::is_rnc(raw) {
            match archive.extract(entry) {
                Ok(out) => outln!(
                    "  {:4}  @{:<8}  {:>8} -> {:>8} bytes (RNC)",
                    entry.index,
                    entry.offset,
                    entry.len,
                    out.len()
                ),
                Err(e) => {
                    failed += 1;
                    outln!(
                        "  {:4}  @{:<8}  {:>8} bytes  FAIL: {e}",
                        entry.index,
                        entry.offset,
                        entry.len
                    );
                }
            }
        } else {
            outln!(
                "  {:4}  @{:<8}  {:>8} bytes (raw)",
                entry.index,
                entry.offset,
                entry.len
            );
        }
    }
    println!(
        "\n{} entries ({} non-empty, {} extraction failures)",
        archive.entries().len(),
        shown,
        failed
    );
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn extract(dat_path: &Path, tab_path: &Path, index: &str, out_path: &Path) -> ExitCode {
    let archive = match open_archive(dat_path, tab_path) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let Ok(index) = index.parse::<usize>() else {
        eprintln!("error: bad index {index}");
        return ExitCode::FAILURE;
    };
    let Some(entry) = archive.entries().get(index).copied() else {
        eprintln!(
            "error: index {index} out of range (0..{})",
            archive.entries().len()
        );
        return ExitCode::FAILURE;
    };
    match archive.extract(entry) {
        Ok(out) => {
            if let Err(e) = std::fs::write(out_path, &out) {
                eprintln!("error: cannot write {}: {e}", out_path.display());
                return ExitCode::FAILURE;
            }
            println!(
                "entry {index} -> {} ({} bytes)",
                out_path.display(),
                out.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: entry {index}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn level(dat_path: &Path, tab_path: &Path, index: &str) -> ExitCode {
    let archive = match open_archive(dat_path, tab_path) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let Ok(index) = index.parse::<usize>() else {
        eprintln!("error: bad index {index}");
        return ExitCode::FAILURE;
    };
    let Some(entry) = archive.entries().get(index).copied() else {
        eprintln!("error: index {index} out of range");
        return ExitCode::FAILURE;
    };
    let payload = match archive.extract(entry) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: entry {index}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let level = match level_mc1::Mc1Level::parse(&payload) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: entry {index}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let g = &level.gen_map;
    println!(
        "GEN_MAP  seed={} off={} raise={} gnarl={}",
        g.seed, g.off, g.raise, g.gnarl
    );
    println!(
        "         river={} sourc={} snlin={} snflt={}",
        g.river, g.sourc, g.snlin, g.snflt
    );
    println!(
        "         bhlin={} bhflt={} rkste={} (pre-header {})",
        g.bhlin, g.bhflt, g.rkste, g.pre_header
    );
    println!("footer   {:?}", level.footer);
    if level.reserved_nonzero {
        println!("note: reserved block is NOT all zeros");
    }

    let mut census = std::collections::BTreeMap::<(u16, u16), u32>::new();
    for (_, thing) in level.active_things() {
        *census.entry((thing.class, thing.model)).or_default() += 1;
    }
    println!(
        "\nentities ({} active, {} markers, {} junk slots):",
        level.active_things().count(),
        level.markers().count(),
        level.junk().count()
    );
    for ((class, model), count) in &census {
        outln!("  {:>4}  {}", count, level_mc1::thing_name(*class, *model));
    }
    ExitCode::SUCCESS
}

fn bake_cmd(gamedata: &Path, out_dir: &Path) -> ExitCode {
    // The orchestration lives in the library (bake::bake_all_staged)
    // so the game shell's auto-bake shares this exact path.
    match bake::bake_all_staged(gamedata, out_dir) {
        Ok(summary) if summary.manifest.is_empty() => {
            println!(
                "0 packages baked — no game data found under {}",
                gamedata.display()
            );
            ExitCode::SUCCESS
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn sources_cmd(gamedata: &Path) -> ExitCode {
    let found = mgc_import::sources::survey(gamedata);
    if found.is_empty() {
        println!("no game data found under {}", gamedata.display());
    }
    for f in found {
        let mark = if f.used { "used  " } else { "unused" };
        println!("{}  {mark}  {}", f.game, f.origin);
    }
    ExitCode::SUCCESS
}

fn add_cmd(gamedata: &Path, path: &Path, replace: bool) -> ExitCode {
    use mgc_import::sources;
    let id = match sources::identify(path) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "{}: {} — {}",
        path.display(),
        id.game.title(),
        id.kind.label()
    );
    for n in &id.notes {
        println!("note: {n}");
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut last = (String::new(), u64::MAX);
    let mut show = |p: &sources::Progress| {
        let pct = (p.done * 100).checked_div(p.total).unwrap_or(0);
        if p.stage != last.0 || pct / 10 != last.1 / 10 {
            if p.total > 0 {
                eprintln!("{} {pct}%", p.stage);
            } else {
                eprintln!("{}", p.stage);
            }
            last = (p.stage.clone(), pct);
        }
    };
    match sources::import(&id, gamedata, replace, &cancel, &mut show) {
        Ok(dest) => {
            println!("imported into {}", dest.display());
            ExitCode::SUCCESS
        }
        Err(sources::ImportError::Exists(p)) => {
            eprintln!(
                "error: {} already exists — pass --replace to overwrite it",
                p.display()
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn out_scan_line(label: &str, data: &[u8], decompressed: &[u8]) -> bool {
    out(format_args!(
        "  OK     {label}  {} -> {} bytes (method {})",
        data.len(),
        decompressed.len(),
        rnc::parse_header(data).map(|h| h.method).unwrap_or(0),
    ))
}

fn scan(root: &Path) -> ExitCode {
    let mut files = Vec::new();
    if let Err(e) = collect_files(root, &mut files) {
        eprintln!("error: cannot read {}: {e}", root.display());
        return ExitCode::FAILURE;
    }
    files.sort();

    let (mut rnc_ok, mut rnc_bad, mut other) = (0u32, 0u32, 0u32);
    let mut check = |label: &str, data: &[u8]| -> bool {
        if !rnc::is_rnc(data) {
            other += 1;
            return true;
        }
        match rnc::decompress(data) {
            Ok(out) => {
                rnc_ok += 1;
                out_scan_line(label, data, &out)
            }
            Err(e) => {
                rnc_bad += 1;
                out(format_args!("  FAIL   {label}  {e}"))
            }
        }
    };
    for path in &files {
        let Ok(data) = std::fs::read(path) else {
            eprintln!("  ERROR  {} (unreadable)", path.display());
            continue;
        };
        if !check(&path.display().to_string(), &data) {
            return ExitCode::SUCCESS;
        }
        // CD images (the GOG installs' game.gog) are scanned inside-out
        // too — most game data lives there, not on the filesystem.
        if let Ok(image) = mgc_import::iso::IsoImage::open(path) {
            let inner: Vec<String> = image.paths().map(String::from).collect();
            for rel in inner {
                let Ok(data) = image.read(&rel) else {
                    eprintln!("  ERROR  {}!{rel} (unreadable)", path.display());
                    continue;
                };
                if !check(&format!("{}!{rel}", path.display()), &data) {
                    return ExitCode::SUCCESS;
                }
            }
        }
    }
    println!(
        "\n{} files scanned: {rnc_ok} RNC ok, {rnc_bad} RNC failed, {other} non-RNC",
        files.len()
    );
    if rnc_bad > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn unpack(input: &Path, output: Option<PathBuf>) -> ExitCode {
    let data = match std::fs::read(input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let out = match rnc::decompress(&data) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let output = output.unwrap_or_else(|| {
        let mut p = input.as_os_str().to_owned();
        p.push(".unpacked");
        PathBuf::from(p)
    });
    if let Err(e) = std::fs::write(&output, &out) {
        eprintln!("error: cannot write {}: {e}", output.display());
        return ExitCode::FAILURE;
    }
    println!(
        "{} -> {} ({} bytes)",
        input.display(),
        output.display(),
        out.len()
    );
    ExitCode::SUCCESS
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}
