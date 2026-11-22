//! `mgcarpet --help`: the command-line reference, laid out for an
//! 80-column terminal — one flag per entry, its explanation wrapped
//! under a fixed column (player ruling 2026-11-22: the old one-line
//! usage string was impossible to read).
//!
//! The settings that also live in the in-game options menu take their
//! explanation from that menu's own text (`settings::registry`, by
//! config path), so the two never drift apart. Everything else is
//! written here. The tests keep this list and the argument parser in
//! step: every flag the parser accepts is listed, and every listed flag
//! is accepted.

use crate::settings::Spec;

/// The terminal width the help is laid out for.
pub const WIDTH: usize = 80;
/// The column the explanations start at.
const TEXT_COL: usize = 28;

/// Where an entry's explanation comes from.
enum Text {
    /// Written here.
    Own(&'static str),
    /// The in-game option with this config path (its menu text).
    Setting(&'static str),
}

struct Entry {
    flags: &'static str,
    text: Text,
}

const fn own(flags: &'static str, text: &'static str) -> Entry {
    Entry {
        flags,
        text: Text::Own(text),
    }
}

const fn setting(flags: &'static str, path: &'static str) -> Entry {
    Entry {
        flags,
        text: Text::Setting(path),
    }
}

struct Section {
    title: &'static str,
    note: Option<&'static str>,
    entries: &'static [Entry],
}

const SECTIONS: &[Section] = &[
    Section {
        title: "PLAYING",
        note: None,
        entries: &[
            own(
                "--campaign mc1|mc1hw|mc2",
                "Play a campaign: Magic Carpet, Hidden Worlds or Magic Carpet 2.",
            ),
            own(
                "--slot N",
                "The campaign save slot to continue or start. Slot 0 is a run \
                 that is only saved from the in-game menu.",
            ),
            own("--new-game", "Start the campaign over in that slot."),
            own(
                "--level GAME:N",
                "Fly a single level, for example mc2:0 (or give the path of a \
                 baked level file).",
            ),
            own(
                "--seat 1-8",
                "With --level: fly as that wizard instead of the first one.",
            ),
            own(
                "--tileset 0|1",
                "Magic Carpet levels: force the temperate (0) or arctic (1) look.",
            ),
            own(
                "--config FILE",
                "Read and save the settings in FILE instead of mgcarpet.json.",
            ),
        ],
    },
    Section {
        title: "MULTIPLAYER",
        note: Some("Games use port 7777 unless an address says otherwise."),
        entries: &[
            own("--host [ADDR]", "Host a game and play in it."),
            own("--join ADDR", "Join a game someone else is hosting."),
            own(
                "--serve [ADDR]",
                "Run a dedicated server without playing (needs --game).",
            ),
            own("--game mc1|mc1hw|mc2", "The game a dedicated server runs."),
            own(
                "--passphrase WORD",
                "Only let in players who give the same word.",
            ),
        ],
    },
    Section {
        title: "DISPLAY",
        note: Some("These are also in the in-game options menu."),
        entries: &[
            setting("--fullscreen, --windowed", "render.preference.fullscreen"),
            setting("--vsync, --no-vsync", "render.preference.vsync"),
            setting(
                "--anti-aliasing off|msaa|1.5x|2x",
                "render.preference.anti_aliasing",
            ),
            setting("--fog-distance TILES", "render.preference.fog_distance"),
            setting("--sky, --no-sky", "render.preference.sky"),
            setting(
                "--reflections, --no-reflections",
                "render.preference.reflections",
            ),
            setting(
                "--horizon-cull, --no-horizon-cull",
                "render.preference.horizon_cull",
            ),
            setting(
                "--light-sources, --no-light-sources",
                "render.preference.light_sources",
            ),
            setting(
                "--smooth-shading, --no-smooth-shading",
                "render.enhancement.smooth_shading",
            ),
            setting(
                "--smooth-textures, --no-smooth-textures",
                "render.enhancement.smooth_textures",
            ),
            setting(
                "--segment-order, --no-segment-order",
                "render.enhancement.segment_order",
            ),
            setting("--crosshair, --no-crosshair", "render.preference.crosshair"),
            setting("--hotbar, --no-hotbar", "render.enhancement.hotbar"),
            setting("--rival-tags auto|on|off", "render.preference.rival_tags"),
            setting("--movies, --no-movies", "render.preference.movies"),
            setting(
                "--movie-subtitles, --no-movie-subtitles",
                "render.preference.movie_subtitles",
            ),
            setting("--stats, --no-stats", "render.preference.stats_screen"),
        ],
    },
    Section {
        title: "AUDIO",
        note: None,
        entries: &[setting("--subtitles on|off", "audio.subtitles")],
    },
    Section {
        title: "CONTROLS",
        note: None,
        entries: &[
            setting("--bindings classic|wasd", "controls.preferences.bindings"),
            setting("--thrust classic|enhanced", "controls.models.thrust"),
            setting("--altitude classic|enhanced", "controls.models.altitude"),
            setting(
                "--spell-selector auto|mc1|mc2|mc1+mc2",
                "gameplay.enhancement.spell_selector",
            ),
            setting(
                "--wheel-spells, --no-wheel-spells",
                "gameplay.enhancement.wheel_spells",
            ),
        ],
    },
    Section {
        title: "CHEATS AND TESTING",
        note: None,
        entries: &[
            own(
                "--dev-mode",
                "Turn on the whole testing kit for this run: spell labels on \
                 jars, health bars, crosshair, trigger areas and the grace meter.",
            ),
            setting("--dev-spells, --no-dev-spells", "gameplay.cheat.dev_spells"),
            setting("--invincible, --no-invincible", "gameplay.cheat.invincible"),
            setting("--ghost, --no-ghost", "gameplay.cheat.ghost"),
            setting("--inert, --no-inert", "gameplay.cheat.inert"),
            setting("--weightless, --no-weightless", "gameplay.cheat.weightless"),
            setting(
                "--plausible-spellbook, --no-plausible-spellbook",
                "dev.plausible_spellbook",
            ),
            own(
                "--set-spellbook ID[:XP],...",
                "Start the level with exactly these spells (by spell number; \
                 --set-spellbook list prints the numbers).",
            ),
            setting(
                "--expose-jar-spells, --no-expose-jar-spells",
                "render.enhancement.expose_jar_spells",
            ),
            setting(
                "--health-bars, --no-health-bars",
                "render.debug.health_bars",
            ),
            setting(
                "--map-triggers, --no-map-triggers",
                "render.debug.map_trigger_areas",
            ),
            setting(
                "--grace-meter, --no-grace-meter",
                "render.debug.grace_meter",
            ),
            setting("--coords, --no-coords", "render.debug.coords"),
            setting("--fps, --no-fps", "render.debug.fps"),
            setting("--pool-slots N", "sim.parameters.entity_pool_size"),
            setting("--awake-range TILES", "sim.parameters.awake_range"),
            own(
                "--no-terrain-features",
                "Skip the level's terrain shaping at load (testing only).",
            ),
        ],
    },
    Section {
        title: "RECORDINGS",
        note: None,
        entries: &[
            own("--record FILE", "Record this session to FILE."),
            own(
                "--replay FILE",
                "Play a recording back (the level comes from the recording).",
            ),
            own(
                "--replay-check FILE",
                "Run a whole recording without a window and report whether \
                 the game still plays it the same way.",
            ),
            own(
                "--firstperson",
                "Watch a replay through the carpet's own eyes (default).",
            ),
            own("--thirdperson", "Watch a replay from behind the carpet."),
            own(
                "--film DIR",
                "Save the session as numbered PNG frames in DIR, one per game \
                 turn by default (turn them into a video with tools/film.py).",
            ),
            own(
                "--film-from TURN",
                "With --film: start filming at that game turn.",
            ),
            own("--film-to TURN", "With --film: stop at that game turn."),
            own(
                "--film-rate N",
                "With --film: frames per game turn (default 1).",
            ),
        ],
    },
    Section {
        title: "DEVELOPER TOOLS",
        note: None,
        entries: &[
            own(
                "--screenshot FILE",
                "Render one frame of the level to a PNG and exit.",
            ),
            own(
                "--camera X,Y,Z,YAW,PITCH",
                "With --screenshot: where the camera stands and looks.",
            ),
            own("--map-view", "With --screenshot: show the map screen."),
            own(
                "--anim-turn N",
                "With --screenshot: the animation turn to show.",
            ),
            own(
                "--map FILE",
                "Render the level's overhead map to a PNG and exit.",
            ),
            own("--map-scale N", "With --map: pixels per map tile."),
            own(
                "--map-settle TICKS",
                "With --map: run the level this many game ticks first (Magic \
                 Carpet 2 builds its terrain during play).",
            ),
            own(
                "--flock-probe FILE",
                "Write a CSV of how creatures move, for tuning (no window).",
            ),
            own(
                "--probe-ticks N",
                "With --flock-probe: how many ticks to run.",
            ),
            own(
                "--probe-every N",
                "With --flock-probe: sample every N ticks.",
            ),
            own(
                "--probe-pose POSE",
                "With --flock-probe: where the carpet waits: far, start, \
                 hover[:ALT], approach[:ALT] or orbit[:ALT].",
            ),
            own(
                "--probe-species CLASS,MODEL",
                "With --flock-probe: only this kind of creature.",
            ),
            own("--probe-strip", "With --flock-probe: a shorter CSV."),
            own(
                "--probe-dis N,N...",
                "With --flock-probe: also trigger these level events.",
            ),
        ],
    },
];

/// Word-wrap `text` to `width` columns.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// The whole `--help` text for a `width`-column terminal.
pub fn render(specs: &[Spec], width: usize) -> String {
    let mut out = String::new();
    out.push_str(
        "mgcarpet - Magic Carpet and Magic Carpet 2, remade\n\n\
         Usage: mgcarpet [OPTIONS]\n\
         With no options the launcher opens. Settings are saved in mgcarpet.json\n\
         and most of them can also be changed in the game (press P).\n",
    );
    let body = width.saturating_sub(TEXT_COL).max(20);
    for section in SECTIONS {
        out.push('\n');
        out.push_str(section.title);
        out.push('\n');
        if let Some(note) = section.note {
            for line in wrap(note, width - 2) {
                out.push_str(&format!("  {line}\n"));
            }
        }
        for e in section.entries {
            let text = match e.text {
                Text::Own(t) => t.to_string(),
                Text::Setting(path) => specs
                    .iter()
                    .find(|s| s.cfg_path == path)
                    .map(|s| s.desc.to_string())
                    .unwrap_or_default(),
            };
            let lines = wrap(&text, body);
            let head = format!("  {}", e.flags);
            let indent = " ".repeat(TEXT_COL);
            if head.chars().count() + 2 <= TEXT_COL {
                // Flags and the first line of text share a row.
                let first = lines.first().map(String::as_str).unwrap_or("");
                out.push_str(&format!("{head:<TEXT_COL$}{first}\n"));
                for line in lines.iter().skip(1) {
                    out.push_str(&format!("{indent}{line}\n"));
                }
            } else {
                // Long flags get their own row.
                out.push_str(&head);
                out.push('\n');
                for line in &lines {
                    out.push_str(&format!("{indent}{line}\n"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every flag token the argument parser matches on (the
    /// `"--x" =>` / `"--x" | "--y" =>` arms of `parse_args`).
    fn parser_flags() -> Vec<String> {
        let src = include_str!("lib.rs");
        let start = src
            .find("fn parse_args() -> Result<Args, String> {")
            .unwrap();
        let end = start + src[start..].find("\n    Ok(Args {").unwrap();
        let mut flags = Vec::new();
        for line in src[start..end].lines() {
            let line = line.trim();
            let Some(arm) = line.split("=>").next().filter(|_| line.contains("=>")) else {
                continue;
            };
            for tok in arm.split('|') {
                let tok = tok.trim().trim_matches('"');
                if tok.starts_with("--") && !tok.contains(' ') {
                    flags.push(tok.to_string());
                }
            }
        }
        flags.sort();
        flags.dedup();
        flags
    }

    /// Every flag a help entry names.
    fn help_flags() -> Vec<String> {
        let mut flags: Vec<String> = SECTIONS
            .iter()
            .flat_map(|s| s.entries.iter())
            .flat_map(|e| e.flags.split([',', ' ']))
            .filter(|t| t.starts_with("--"))
            .map(str::to_string)
            .collect();
        flags.sort();
        flags.dedup();
        flags
    }

    #[test]
    fn every_parser_flag_is_documented() {
        let listed = help_flags();
        let missing: Vec<_> = parser_flags()
            .into_iter()
            .filter(|f| f != "--help" && !listed.contains(f))
            .collect();
        assert!(missing.is_empty(), "flags missing from --help: {missing:?}");
    }

    #[test]
    fn every_documented_flag_is_accepted() {
        let accepted = parser_flags();
        let bogus: Vec<_> = help_flags()
            .into_iter()
            .filter(|f| !accepted.contains(f))
            .collect();
        assert!(
            bogus.is_empty(),
            "--help names flags the parser rejects: {bogus:?}"
        );
    }

    #[test]
    fn every_setting_entry_resolves_and_the_text_fits_80_columns() {
        let specs = crate::settings::registry();
        for e in SECTIONS.iter().flat_map(|s| s.entries.iter()) {
            if let Text::Setting(path) = e.text {
                let spec = specs.iter().find(|s| s.cfg_path == path);
                assert!(
                    spec.is_some_and(|s| !s.desc.is_empty()),
                    "{}: no option at {path}",
                    e.flags
                );
            }
        }
        let text = render(&specs, WIDTH);
        for line in text.lines() {
            assert!(line.chars().count() <= WIDTH, "too wide: {line:?}");
        }
    }
}
