//! The launcher's GAME-DATA dialog: its state and its workers (the
//! file browser, identification, the import) — no drawing; the
//! launcher composes it (src/launcher_screen.rs).
//!
//! The work is `mgc_import::sources`: [`identify`] what the player
//! picked, [`import`] it into its canonical home under `gamedata/`
//! (copying, or extracting a GOG installer and slimming it), and
//! [`survey`] what is there now. Everything slow runs on a thread and
//! is polled from [`DataDialog::tick`], so the launcher keeps drawing.
//!
//! The file browser is the launcher's OWN ([`Browser`]), not the
//! system's: a native picker on Linux goes through the desktop portal
//! or `zenity`, and a stock XFCE has neither (player report
//! 2026-11-16). Ours works the same everywhere and knows what it is
//! looking for — it lists folders and the file types a source can be,
//! and marks the folders that ARE game data.
//!
//! [`identify`]: mgc_import::sources::identify
//! [`import`]: mgc_import::sources::import
//! [`survey`]: mgc_import::sources::survey

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use mgc_import::sources::{self, Found, Identified, ImportError, Progress};

/// The kinds of game data the player can bring, each with its own
/// plain-words description and its own pre-set browser — a nudge for
/// players who do not know what they have (player ruling 2026-11-16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Installer,
    Installed,
    CdImage,
}

impl Want {
    pub const ALL: [Want; 3] = [Want::Installer, Want::Installed, Want::CdImage];

    pub const fn title(self) -> &'static str {
        match self {
            Want::Installer => "GOG installer (.exe)",
            Want::Installed => "Installed GOG game (folder)",
            Want::CdImage => "Original CD image (.cue with .img or .bin)",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Want::Installer => {
                "The setup file you downloaded from GOG, such as \
                 setup_magic_carpet_2_….exe. Nothing needs to be installed first."
            }
            Want::Installed => {
                "The folder the GOG installer put the game in: it contains \
                 game.gog (Magic Carpet 2) or a CARPET.CD folder (Magic Carpet \
                 Plus)."
            }
            Want::CdImage => {
                "A copy of the game CD: the .cue file, with its .img or .bin \
                 beside it."
            }
        }
    }

    /// The file types the browser lists for this kind (folders are
    /// always listed, to walk through).
    const fn exts(self) -> &'static [&'static str] {
        match self {
            Want::Installer => &["exe"],
            Want::Installed => &[],
            Want::CdImage => &["cue", "img", "bin"],
        }
    }
}

/// What the dialog is doing.
pub enum Step {
    /// Showing what is found, offering to add more.
    Idle,
    /// "What do you have?": one entry per [`Want`].
    Choosing,
    /// The file browser is open, set up for one kind.
    Browsing(Browser),
    /// Reading what the player picked.
    Identifying(JoinHandle<Result<Identified, String>>),
    /// Asking before the import: what it is, and whether its home is
    /// already taken (the import then replaces it).
    Confirm {
        id: Identified,
        dest: PathBuf,
        taken: bool,
    },
    /// The import is running.
    Importing {
        handle: JoinHandle<Result<PathBuf, ImportError>>,
        progress: Arc<Mutex<Progress>>,
        cancel: Arc<AtomicBool>,
    },
}

pub struct DataDialog {
    /// The `gamedata/` root imports land in (created on first import).
    pub root: PathBuf,
    pub step: Step,
    /// What `survey` found, refreshed after every import.
    pub found: Vec<Found>,
    /// The last outcome, shown under the source list: (is_error, text).
    pub outcome: Option<(bool, String)>,
    /// Set when an import lands: the launcher should (re)bake.
    pub bake_requested: bool,
    /// Where the browser was last, so it reopens there.
    last_dir: Option<PathBuf>,
}

impl DataDialog {
    pub fn new(root: PathBuf) -> Self {
        let found = sources::survey(&root);
        DataDialog {
            root,
            step: Step::Idle,
            found,
            outcome: None,
            bake_requested: false,
            last_dir: None,
        }
    }

    /// Something is running; the buttons and Close wait.
    pub fn busy(&self) -> bool {
        matches!(self.step, Step::Identifying(_) | Step::Importing { .. })
    }

    /// "Add game data…": ask what kind the player has.
    pub fn choose(&mut self) {
        if matches!(self.step, Step::Idle) {
            self.outcome = None;
            self.step = Step::Choosing;
        }
    }

    /// Open the file browser for one kind: where it was last, else
    /// Downloads for an installer (where GOG's lands) or home.
    pub fn browse(&mut self, want: Want) {
        if !matches!(self.step, Step::Idle | Step::Choosing) {
            return;
        }
        self.outcome = None;
        let downloads = (want == Want::Installer)
            .then(|| home().map(|h| h.join("Downloads")))
            .flatten()
            .filter(|d| d.is_dir());
        let start = self
            .last_dir
            .clone()
            .filter(|d| d.is_dir())
            .or(downloads)
            .or_else(home)
            .unwrap_or_else(|| PathBuf::from("/"));
        self.step = Step::Browsing(Browser::open(start, &self.root, want));
    }

    /// Leave the browser without picking (back to the kinds).
    pub fn close_browser(&mut self) {
        if let Step::Browsing(b) = &self.step {
            self.last_dir = Some(b.dir.clone());
            self.step = Step::Choosing;
        }
    }

    /// The browser picked `path` (a file, or "use this folder").
    pub fn pick(&mut self, path: PathBuf) {
        if let Step::Browsing(b) = &self.step {
            self.last_dir = Some(b.dir.clone());
        }
        self.identify(path);
    }

    /// Start reading a path.
    pub fn identify(&mut self, path: PathBuf) {
        self.outcome = None;
        let handle = std::thread::spawn(move || sources::identify(&path));
        self.step = Step::Identifying(handle);
    }

    /// The player said yes on the confirmation.
    pub fn confirm(&mut self) {
        let Step::Confirm { id, taken, .. } = std::mem::replace(&mut self.step, Step::Idle) else {
            return;
        };
        let progress = Arc::new(Mutex::new(Progress {
            stage: "Starting…".into(),
            done: 0,
            total: 0,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c, root) = (progress.clone(), cancel.clone(), self.root.clone());
        let handle = std::thread::spawn(move || {
            let mut report = |now: &Progress| {
                if let Ok(mut slot) = p.lock() {
                    *slot = now.clone();
                }
            };
            sources::import(&id, &root, taken, &c, &mut report)
        });
        self.step = Step::Importing {
            handle,
            progress,
            cancel,
        };
    }

    /// Back out of the browser or a confirmation, or ask a running
    /// import to stop.
    pub fn cancel(&mut self) {
        match &self.step {
            Step::Browsing(_) => self.close_browser(),
            Step::Choosing | Step::Confirm { .. } => self.step = Step::Idle,
            Step::Importing { cancel, .. } => cancel.store(true, Ordering::Relaxed),
            _ => {}
        }
    }

    /// The running import's progress, when it has one.
    pub fn progress(&self) -> Option<Progress> {
        match &self.step {
            Step::Importing { progress, .. } => progress.lock().ok().map(|p| p.clone()),
            _ => None,
        }
    }

    /// Poll the workers; true when the dialog changed.
    pub fn tick(&mut self) -> bool {
        let finished = match &self.step {
            Step::Identifying(h) => h.is_finished(),
            Step::Importing { handle, .. } => handle.is_finished(),
            _ => false,
        };
        if !finished {
            return matches!(self.step, Step::Importing { .. });
        }
        match std::mem::replace(&mut self.step, Step::Idle) {
            Step::Identifying(h) => match h.join() {
                Ok(Ok(id)) => {
                    let dest = sources::destination(&self.root, &id);
                    let taken = dest.exists();
                    self.step = Step::Confirm { id, dest, taken };
                }
                Ok(Err(e)) => self.fail(&e),
                Err(_) => self.fail("Reading the game data crashed."),
            },
            Step::Importing { handle, .. } => {
                match handle.join() {
                    Ok(Ok(dest)) => {
                        self.outcome = Some((
                            false,
                            format!(
                                "Added {}. The game now prepares its data.",
                                dest.file_name().unwrap_or_default().to_string_lossy()
                            ),
                        ));
                        self.bake_requested = true;
                    }
                    Ok(Err(ImportError::Cancelled)) => {
                        self.outcome = Some((false, "Cancelled; nothing was changed.".into()));
                    }
                    Ok(Err(e)) => self.fail(&e.to_string()),
                    Err(_) => self.fail("The import crashed; nothing was changed."),
                }
                self.found = sources::survey(&self.root);
            }
            _ => unreachable!("not a finished worker"),
        }
        true
    }

    fn fail(&mut self, message: &str) {
        self.outcome = Some((true, message.to_owned()));
    }

    /// The sources found, as display lines per product: (product,
    /// lines). A source the bake does not read says so.
    pub fn summary(&self) -> [(&'static str, Vec<String>); 2] {
        let lines = |game: &str| -> Vec<String> {
            let v: Vec<String> = self
                .found
                .iter()
                .filter(|f| f.game == game)
                .map(|f| {
                    if f.used {
                        f.origin.clone()
                    } else {
                        format!("{} (not used)", f.origin)
                    }
                })
                .collect();
            if v.is_empty() {
                vec!["Not found".into()]
            } else {
                v
            }
        };
        [
            ("Magic Carpet + Hidden Worlds", lines("mc1")),
            ("Magic Carpet 2", lines("mc2")),
        ]
    }
}

/// One row of the browser.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    /// A folder that looks like game data (a GOG install or a CD
    /// dump) — judged by cheap marker files, confirmed on pick.
    pub data: bool,
}

/// The launcher's own file browser: one folder's listing, a selection
/// and a scroll window, plus the quick places.
pub struct Browser {
    /// What it is looking for (which files it lists).
    pub want: Want,
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    /// First visible row.
    pub scroll: usize,
    pub selected: Option<usize>,
    /// Quick places: (label, folder).
    pub places: Vec<(String, PathBuf)>,
    /// When the folder itself is game data, what it is ("Use this
    /// folder" lights up).
    pub here: Option<String>,
    /// The folder could not be read.
    pub error: Option<String>,
}

/// Rows the browser shows at once (the launcher's list height).
pub const BROWSER_ROWS: usize = 14;

impl Browser {
    pub fn open(dir: PathBuf, gamedata_root: &Path, want: Want) -> Browser {
        let mut b = Browser {
            want,
            dir: PathBuf::new(),
            entries: Vec::new(),
            scroll: 0,
            selected: None,
            places: places(gamedata_root),
            here: None,
            error: None,
        };
        b.enter(dir);
        b
    }

    /// Show `dir`.
    pub fn enter(&mut self, dir: PathBuf) {
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        self.scroll = 0;
        self.selected = None;
        self.error = None;
        self.entries.clear();
        match std::fs::read_dir(&dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let path = e.path();
                    let dir = path.is_dir();
                    if !dir && !wanted_file(&path, self.want) {
                        continue;
                    }
                    let data = dir && looks_like_data(&path);
                    self.entries.push(Entry {
                        name,
                        path,
                        dir,
                        data,
                    });
                }
                self.entries.sort_by(|a, b| {
                    b.dir
                        .cmp(&a.dir)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            Err(e) => self.error = Some(format!("cannot read this folder: {e}")),
        }
        // "Use this folder" fits a folder that IS the source: an
        // install, or a CD dump's folder — never an installer's.
        self.here = (self.want != Want::Installer && looks_like_data(&dir))
            .then(|| sources::identify(&dir).ok())
            .flatten()
            .map(|id| format!("{} — {}", id.game.title(), id.kind.label()));
        self.dir = dir;
    }

    /// Go up one folder (keeping the one we left selected).
    pub fn up(&mut self) {
        let Some(parent) = self.dir.parent().map(Path::to_path_buf) else {
            return;
        };
        let left = self.dir.clone();
        self.enter(parent);
        if let Some(i) = self.entries.iter().position(|e| e.path == left) {
            self.selected = Some(i);
            self.reveal(i);
        }
    }

    /// Open row `i`: a folder is entered; a file is the pick.
    pub fn activate(&mut self, i: usize) -> Option<PathBuf> {
        let e = self.entries.get(i)?.clone();
        if e.dir {
            self.enter(e.path);
            None
        } else {
            Some(e.path)
        }
    }

    /// Move the selection by `delta` rows (keyboard).
    pub fn step(&mut self, delta: i64) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() as i64 - 1;
        let cur = self.selected.map_or(-1, |s| s as i64);
        let next = (cur + delta).clamp(0, last) as usize;
        self.selected = Some(next);
        self.reveal(next);
    }

    /// Scroll by `rows` (the wheel), within the listing.
    pub fn scroll_by(&mut self, rows: i64) {
        let max = self.entries.len().saturating_sub(BROWSER_ROWS) as i64;
        self.scroll = (self.scroll as i64 + rows).clamp(0, max) as usize;
    }

    fn reveal(&mut self, i: usize) {
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + BROWSER_ROWS {
            self.scroll = i + 1 - BROWSER_ROWS;
        }
    }
}

fn wanted_file(p: &Path, want: Want) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| want.exts().iter().any(|e| x.eq_ignore_ascii_case(e)))
}

/// Cheap markers only (no image is opened): a GOG MC2 install, a GOG
/// MC1 Plus install, or a folder holding a cue sheet.
fn looks_like_data(dir: &Path) -> bool {
    (dir.join("game.gog").is_file() && dir.join("GAME").is_dir())
        || dir.join("CARPET.CD").join("game.gog").is_file()
        || std::fs::read_dir(dir).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("cue"))
            })
        })
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// The browser's quick places: home and its usual download folders,
/// the game-data folder, removable media and mounts, and (Windows)
/// the drive letters.
fn places(gamedata_root: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut add = |label: String, p: PathBuf| {
        if p.is_dir() && !out.iter().any(|(_, q)| *q == p) {
            out.push((label, p));
        }
    };
    if let Some(h) = home() {
        add("Home".into(), h.clone());
        for d in ["Downloads", "Desktop", "Documents"] {
            add(d.into(), h.join(d));
        }
    }
    add(
        "Game data".into(),
        std::path::absolute(gamedata_root).unwrap_or_else(|_| gamedata_root.to_path_buf()),
    );
    let user = std::env::var("USER").unwrap_or_default();
    for base in [
        PathBuf::from("/media").join(&user),
        PathBuf::from("/run/media").join(&user),
        PathBuf::from("/mnt"),
        PathBuf::from("/Volumes"),
    ] {
        if let Ok(rd) = std::fs::read_dir(&base) {
            let mut mounts: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            mounts.sort();
            for m in mounts {
                let label = m
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                add(label, m);
            }
        }
    }
    if cfg!(windows) {
        for letter in b'A'..=b'Z' {
            let drive = format!("{}:\\", letter as char);
            add(format!("{}:", letter as char), PathBuf::from(drive));
        }
    } else {
        add("Computer".into(), PathBuf::from("/"));
    }
    out
}

/// Where imports go: the config's `gamedata`, else `MGC_GAMEDATA` /
/// `gamedata/` ([`crate::paths::gamedata_dir`]) — created on the first
/// import, so a packager's fresh data folder needs no setup.
pub fn gamedata_root(config_gamedata: Option<&Path>) -> PathBuf {
    config_gamedata
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::paths::gamedata_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(d: &mut DataDialog) {
        for _ in 0..6000 {
            d.tick();
            if !d.busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the dialog never settled");
    }

    /// The dialog's flow: identify → confirm → import → survey
    /// refreshed and a bake requested; a second import of the same
    /// source asks to REPLACE; a stray file is an error, not a
    /// confirmation. Uses the real CD dump when present.
    #[test]
    fn identify_confirm_import_flow() {
        let gd = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gamedata");
        let cd = gd.join("netherworlds-cd");
        let mut d = DataDialog::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-launcher-data"),
        );
        let _ = std::fs::remove_dir_all(&d.root);
        d.identify(gd.join("README.md"));
        wait(&mut d);
        assert!(matches!(d.step, Step::Idle));
        assert!(
            d.outcome.as_ref().is_some_and(|(err, _)| *err),
            "{:?}",
            d.outcome
        );
        if !cd.is_dir() {
            eprintln!("skip: no gamedata/netherworlds-cd");
            return;
        }
        d.identify(cd.clone());
        wait(&mut d);
        let Step::Confirm { taken, .. } = &d.step else {
            panic!("expected a confirmation, got {:?}", d.outcome)
        };
        assert!(!taken);
        d.confirm();
        wait(&mut d);
        assert!(d.bake_requested, "{:?}", d.outcome);
        assert!(
            d.summary()[1]
                .1
                .iter()
                .any(|l| l.contains("retail CD image"))
        );
        d.identify(cd);
        wait(&mut d);
        assert!(matches!(d.step, Step::Confirm { taken: true, .. }));
        d.cancel();
        assert!(matches!(d.step, Step::Idle));
        let _ = std::fs::remove_dir_all(&d.root);
    }

    /// The browser lists folders first, then only source-type files
    /// (any case), hides dotfiles, marks game-data folders, walks in
    /// and out, and scrolls/selects within the listing.
    #[test]
    fn the_browser_lists_and_walks_a_folder() {
        let t = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-browser");
        let _ = std::fs::remove_dir_all(&t);
        for d in ["b-dir", "A-dir", ".hidden", "dump"] {
            std::fs::create_dir_all(t.join(d)).unwrap();
        }
        for f in ["setup.EXE", "notes.txt", "x.cue", ".h.exe", "dump/mc2.cue"] {
            std::fs::write(t.join(f), b"").unwrap();
        }
        let names =
            |b: &Browser| -> Vec<String> { b.entries.iter().map(|e| e.name.clone()).collect() };
        // Each kind lists the folders and ITS files only (any case).
        let b = Browser::open(t.clone(), &t, Want::Installed);
        assert_eq!(names(&b), ["A-dir", "b-dir", "dump"]);
        let b = Browser::open(t.clone(), &t, Want::CdImage);
        assert_eq!(names(&b), ["A-dir", "b-dir", "dump", "x.cue"]);
        let mut b = Browser::open(t.clone(), &t, Want::Installer);
        assert_eq!(names(&b), ["A-dir", "b-dir", "dump", "setup.EXE"]);
        assert!(
            b.entries[2].data,
            "a folder with a cue sheet looks like data"
        );
        assert!(!b.entries[0].data);
        // Paths compare by what they NAME: `path::absolute` resolves
        // `..` on Windows but keeps it on Unix, so the spellings differ
        // across platforms (Windows CI 2026-11-16).
        let same = |a: &Path, b: &Path| {
            std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
        };
        let pick = b.activate(3).expect("a file is the pick");
        assert!(same(&pick, &t.join("setup.EXE")), "{pick:?}");
        assert_eq!(b.activate(2), None, "a folder is entered");
        assert!(b.here.is_none(), "an installer is never a folder pick");
        assert!(same(&b.dir, &t.join("dump")), "{:?}", b.dir);
        b.up();
        assert_eq!(
            b.selected,
            Some(2),
            "going up keeps the folder we left selected"
        );
        b.step(10);
        assert_eq!(b.selected, Some(3), "the selection stops at the end");
        b.scroll_by(5);
        assert_eq!(b.scroll, 0, "a short listing does not scroll");
        assert!(b.places.iter().any(|(l, _)| l == "Game data"));
        let _ = std::fs::remove_dir_all(&t);
    }

    /// A real GOG install folder reads as "here" (Use this folder).
    #[test]
    fn the_browser_knows_a_gog_install_when_inside_one() {
        let gd = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gamedata");
        if !gd.join("Magic Carpet 2").is_dir() {
            eprintln!("skip: no gamedata/Magic Carpet 2");
            return;
        }
        let mut b = Browser::open(gd.clone(), &gd, Want::Installed);
        let i = b
            .entries
            .iter()
            .position(|e| e.name == "Magic Carpet 2")
            .unwrap();
        assert!(b.entries[i].data);
        assert!(b.here.is_none(), "gamedata/ itself is not a source");
        b.activate(i);
        assert!(
            b.here.as_deref().is_some_and(|h| h.contains("GOG install")),
            "{:?}",
            b.here
        );
    }
}
