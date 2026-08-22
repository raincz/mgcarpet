//! The option registry: one declarative table describing every
//! user-facing option — its domain, class, how to toggle it, how to
//! read its current value out of [`Config`], how to WRITE it back
//! (the menu widget), and the hover text explaining it. It is the
//! single source of truth for the startup summary and the in-game
//! options menu: both are just *views* over this table, so a new
//! option is added in exactly one place.
//!
//! Two orthogonal axes describe each option:
//! - **domain** ([`Domain`]) — where it acts (mirrors the `Config`
//!   nesting: sim / render / controls / audio / gameplay / dev).
//! - **class** ([`Class`]) — how faithful it is. This drives the
//!   run-fidelity rollup: cheats and dev instruments make a run
//!   non-canonical ([`Fidelity::Modified`]); fair enhancements and
//!   harmless debug overlays make it [`Fidelity::Enhanced`]; neutral
//!   preferences leave it [`Fidelity::Faithful`].

use crate::config::Config;
use mgc_sim::ids::GameId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Sim,
    Render,
    Controls,
    Audio,
    Gameplay,
    Dev,
}

/// Menu tab order + labels.
pub const DOMAINS: [Domain; 6] = [
    Domain::Sim,
    Domain::Render,
    Domain::Controls,
    Domain::Audio,
    Domain::Gameplay,
    Domain::Dev,
];

impl Domain {
    pub fn title(self) -> &'static str {
        match self {
            Domain::Sim => "SIM",
            Domain::Render => "RENDER",
            Domain::Controls => "CONTROLS",
            Domain::Audio => "AUDIO",
            Domain::Gameplay => "GAMEPLAY",
            Domain::Dev => "DEV",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Neutral preference — no bearing on fidelity (volumes, bindings,
    /// mouse axis, HUD opacity).
    Preference,
    /// Fair opt-in that improves the game without impossible power.
    Enhancement,
    /// On-screen dev overlay that visualises the real sim — harmless to
    /// fidelity (it only lets you *see* more).
    Debug,
    /// Otherwise-impossible power (invulnerability, all spells).
    Cheat,
    /// Troubleshooting instrument that alters the run itself.
    Instrument,
    /// A retail-bug patch: one deliberate upstream bugfix with BOTH
    /// arms implemented (faithful = the retail arm). Counted apart in
    /// the rollup — a patched run is still the intended default
    /// experience, and fixture capture forces the retail arms
    /// structurally — so patches never flip the verdict.
    Patch,
}

/// The run-level fidelity rollup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fidelity {
    Faithful,
    Enhanced,
    Modified,
}

/// Whether an option can change during play.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    /// Takes effect immediately (the menu / runtime keys re-apply it).
    Live,
    /// Read once at startup / level-load; changing it mid-run is
    /// meaningless or would need a restart (e.g. the entity pool can't
    /// resurrect events already dropped; a plausible spellbook is
    /// seeded at level entry). The menu shows these greyed out.
    Startup,
}

impl Class {
    fn fidelity(self) -> Fidelity {
        match self {
            Class::Preference | Class::Patch => Fidelity::Faithful,
            Class::Enhancement | Class::Debug => Fidelity::Enhanced,
            Class::Cheat | Class::Instrument => Fidelity::Modified,
        }
    }
}

/// A resolved option value, carrying enough to render the current
/// selection, mark the faithful default, and detect deviation.
pub enum Val {
    Toggle {
        on: bool,
        faithful: bool,
    },
    /// An enum choice: `cur`/`faithful` index into `variants`.
    Choice {
        cur: usize,
        faithful: usize,
        variants: &'static [&'static str],
    },
    /// A numeric preference, pre-formatted with its faithful value.
    Scalar {
        text: String,
        faithful: &'static str,
    },
    /// An optional override (e.g. entity pool): `None` = the faithful
    /// per-game default described by `faithful`.
    Override {
        val: Option<String>,
        faithful: &'static str,
    },
}

impl Val {
    /// Does the current value differ from the faithful default?
    fn deviates(&self) -> bool {
        match self {
            Val::Toggle { on, faithful } => on != faithful,
            Val::Choice { cur, faithful, .. } => cur != faithful,
            Val::Scalar { text, faithful } => text != faithful,
            Val::Override { val, .. } => val.is_some(),
        }
    }

    /// The current value as displayed in the summary's value column.
    pub fn current_text(&self) -> String {
        match self {
            Val::Toggle { on, .. } => (if *on { "on" } else { "off" }).into(),
            Val::Choice { cur, variants, .. } => {
                variants.get(*cur).copied().unwrap_or("?").to_string()
            }
            Val::Scalar { text, .. } => text.clone(),
            // The hint column already spells out the faithful default,
            // so don't repeat it here.
            Val::Override { val, .. } => match val {
                Some(v) => v.clone(),
                None => "default".into(),
            },
        }
    }

    /// The parenthesised alternatives, faithful default marked `*`.
    fn choices_hint(&self) -> String {
        match self {
            Val::Toggle { faithful, .. } => {
                if *faithful {
                    "(*on, off)".into()
                } else {
                    "(*off, on)".into()
                }
            }
            Val::Choice {
                faithful, variants, ..
            } => {
                let inner = variants
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        if i == *faithful {
                            format!("*{v}")
                        } else {
                            (*v).to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({inner})")
            }
            Val::Scalar { faithful, .. } => format!("(faithful {faithful})"),
            Val::Override { faithful, .. } => format!("(faithful: {faithful})"),
        }
    }
}

/// The menu widget + write path for one option. Descriptions are
/// per-SELECTION hover text (the option-level text lives in
/// [`Spec::desc`]); placeholder drafts today — player-authored text
/// slots in here.
pub enum Ctl {
    /// Not adjustable from the menu (CLI/config only).
    ReadOnly,
    /// An on/off switch. `descs` = [off-text, on-text].
    Toggle {
        set: fn(&mut Config, bool),
        descs: [&'static str; 2],
    },
    /// An enum choice; `set` receives the index into the read
    /// `Val::Choice::variants`. `descs` aligns with the variants.
    Choice {
        set: fn(&mut Config, usize),
        descs: &'static [&'static str],
    },
    /// A continuous numeric slider, stepped to `step` granularity.
    Slider {
        get: fn(&Config) -> f32,
        set: fn(&mut Config, f32),
        min: f32,
        max: f32,
        step: f32,
    },
    /// A slider with a fixed set of stops: (value, tag). Clicks snap
    /// to the nearest stop.
    Stops {
        get: fn(&Config) -> u32,
        set: fn(&mut Config, u32),
        stops: &'static [(u32, &'static str)],
    },
}

/// One option's metadata + how to read/write it from [`Config`].
pub struct Spec {
    /// The acting domain — the menu tab this option lives under.
    pub domain: Domain,
    /// The `domain · group` heading this option lists under.
    pub group: &'static str,
    pub label: &'static str,
    pub class: Class,
    /// Runtime toggle key, if any (e.g. `"T"`, `"F1"`).
    pub key: Option<&'static str>,
    /// The `--flag` that sets it for one run, if any.
    pub cli: Option<&'static str>,
    /// The dotted `mgcarpet.json` path.
    pub cfg_path: &'static str,
    /// Read the live value out of the resolved config.
    pub read: fn(&Config) -> Val,
    /// The option-level hover explanation (the menu's info box; the
    /// per-selection texts live in [`Ctl`]).
    pub desc: &'static str,
    /// The menu widget + write path.
    pub ctl: Ctl,
}

impl Spec {
    /// Whether this option can change live, keyed by its config path so
    /// the registry literals stay uncluttered.
    pub fn mutability(&self) -> Mutability {
        match self.cfg_path {
            "sim.parameters.entity_pool_size"
            | "sim.parameters.awake_range"
            | "dev.plausible_spellbook" => Mutability::Startup,
            // Switching the music arrangement means reloading the
            // baked track set — no cheap re-apply path.
            "audio.arrangement" => Mutability::Startup,
            _ => Mutability::Live,
        }
    }

    /// Whether menu changes to this option are written to
    /// `mgcarpet.json`. Keyed by config path like [`mutability`].
    /// Game speed is a situational control that the game itself
    /// resets to normal at every level entry, so a persisted value
    /// would only ever be a stale surprise at the next launch.
    ///
    /// [`mutability`]: Spec::mutability
    pub fn persists(&self) -> bool {
        self.cfg_path != "sim.options.game_speed"
    }

    /// The trailing `[key T | --flag | cfg.path]` toggle comment.
    fn toggle_hint(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(k) = self.key {
            parts.push(format!("key {k}"));
        }
        if let Some(c) = self.cli {
            parts.push(c.to_string());
        }
        parts.push(self.cfg_path.to_string());
        format!("[{}]", parts.join(" | "))
    }
}

macro_rules! toggle {
    ($cfg:ident => $($path:tt)*) => {
        |c: &Config| Val::Toggle { on: c.$($path)*, faithful: false }
    };
}

/// The full registry. Order = summary order (grouped by heading) =
/// menu row order within each domain tab.
pub fn registry() -> Vec<Spec> {
    use Class::*;
    use Domain::*;
    vec![
        // ---- sim · parameters -------------------------------------------
        Spec {
            domain: Sim,
            group: "sim · parameters",
            label: "entity_pool_size",
            class: Cheat,
            key: None,
            cli: Some("--pool-slots N"),
            cfg_path: "sim.parameters.entity_pool_size",
            read: |c| Val::Override {
                val: c.sim.parameters.entity_pool_size.map(|n| n.to_string()),
                faithful: "per-game default 1000",
            },
            desc: "Entity pool capacity. Retail caps the world at 1000 things and \
                   silently drops spawns beyond it; enlarging the pool carries \
                   rosters the original would have shed. Set from the command \
                   line or config file; fixed for the run.",
            ctl: Ctl::ReadOnly,
        },
        Spec {
            domain: Sim,
            group: "sim · parameters",
            label: "awake_range",
            class: Cheat,
            key: None,
            cli: Some("--awake-range TILES"),
            cfg_path: "sim.parameters.awake_range",
            read: |c| Val::Override {
                val: c.sim.parameters.awake_range.map(|n| {
                    if n == 0 {
                        "off (always awake)".to_string()
                    } else {
                        format!("{n} tiles")
                    }
                }),
                faithful: "24 tiles (both retail engines)",
            },
            desc: "Creature wake radius in tiles. Retail sleeps creatures beyond \
                   24 tiles (a period CPU optimization); 0 keeps everything \
                   awake. Set from the command line or config file; fixed for \
                   the run.",
            ctl: Ctl::ReadOnly,
        },
        // ---- sim · options ----------------------------------------------
        Spec {
            domain: Sim,
            group: "sim · options",
            label: "game_speed",
            class: Preference,
            key: Some("F3"),
            cli: None,
            cfg_path: "sim.options.game_speed",
            read: |c| Val::Choice {
                cur: match c.sim.options.game_speed {
                    crate::config::GameSpeed::Slow => 0,
                    crate::config::GameSpeed::Normal => 1,
                    crate::config::GameSpeed::Fast => 2,
                    crate::config::GameSpeed::VeryFast => 3,
                },
                faithful: 1,
                variants: &["slow", "normal", "fast", "very-fast"],
            },
            desc: "How fast the world runs. Retail's F3 option: the whole \
                   simulation is paced up or down — everything moves, fights \
                   and regenerates at the multiplied rate. A situational \
                   control, not a preference: it resets to normal at every \
                   level start and is never saved.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.sim.options.game_speed = match i {
                        0 => crate::config::GameSpeed::Slow,
                        1 => crate::config::GameSpeed::Normal,
                        2 => crate::config::GameSpeed::Fast,
                        _ => crate::config::GameSpeed::VeryFast,
                    }
                },
                descs: &[
                    "Half speed (0.5x). Our addition — no retail equivalent.",
                    "The authentic pace: 24 simulation ticks per second.",
                    "Retail Fast: 4x, both games.",
                    "Retail's top speed, game-keyed: MC1 'Very Fast' = 16x, \
                     MC2 'Super Fast' = 8x.",
                ],
            },
        },
        // ---- render · preference ----------------------------------------
        Spec {
            domain: Render,
            group: "render · preference",
            label: "crosshair",
            class: Preference,
            key: Some("C"),
            cli: Some("--crosshair"),
            cfg_path: "render.preference.crosshair",
            // Faithful = ON: both retails steered by a live on-screen
            // cursor; a bare aim cross is the closest analog.
            read: |c| Val::Toggle {
                on: c.render.preference.crosshair,
                faithful: true,
            },
            desc: "The aim crosshair: a cross at the TRUE aim point (the \
                   faithful camera pitches at half the aim pitch, so aim is \
                   never screen center). Under enhanced thrust this IS the \
                   chase-the-pointer target \u{2014} steering is unreadable \
                   without it. Autoaim lock markers are a separate debug \
                   option.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.crosshair = v,
                descs: [
                    "No aim cursor (retail showed the live mouse pointer; \
                     the classic virtual stick hides it).",
                    "The white-edged aim cross \u{2014} and the enhanced \
                     steering target.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "sky",
            class: Preference,
            key: Some("F6"),
            cli: Some("--no-sky"),
            cfg_path: "render.preference.sky",
            // Faithful = ON (retail ships the Sky option enabled).
            read: |c| Val::Toggle {
                on: c.render.preference.sky,
                faithful: true,
            },
            desc: "The textured parallax cloud sky (retail's Sky option, F6). \
                   Caves never have one, exactly like retail.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.sky = v,
                descs: [
                    "Flat horizon-color fill (retail's sky-off look).",
                    "The per-environment cloud plane, scrolled by yaw and \
                     slid by pitch (retail default).",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "reflections",
            class: Preference,
            key: Some("F5"),
            cli: Some("--no-reflections"),
            cfg_path: "render.preference.reflections",
            // Faithful = ON (retail ships the Reflections option
            // enabled).
            read: |c| Val::Toggle {
                on: c.render.preference.reflections,
                faithful: true,
            },
            desc: "Water reflections (retail's Reflections option, F5): sea \
                   tiles mirror the landscape about the water plane, wobbling \
                   with the wave.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.reflections = v,
                descs: [
                    "Plain animated water.",
                    "Terrain mirrored in the water (retail default).",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "light_sources",
            class: Preference,
            key: None,
            cli: Some("--no-light-sources"),
            cfg_path: "render.preference.light_sources",
            // Faithful = ON (retail MC2 ships Dynamic Lighting
            // enabled; it self-gates to Night/Cave either way).
            read: |c| Val::Toggle {
                on: c.render.preference.light_sources,
                faithful: true,
            },
            desc: "Dynamic light sources (retail MC2's Dynamic Lighting): \
                   fireballs, explosions and standing fire brighten the \
                   terrain around them — night and cave levels only, exactly \
                   retail's gate.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.light_sources = v,
                descs: [
                    "No dynamic terrain lighting.",
                    "Fire lights up the night (retail default).",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "fog_distance",
            class: Preference,
            key: None,
            cli: Some("--fog-distance TILES"),
            cfg_path: "render.preference.fog_distance",
            read: |c| Val::Scalar {
                text: match c.render.preference.fog_distance {
                    0 => "off (no fog)".to_string(),
                    n => format!("{n} tiles"),
                },
                // Val::Scalar compares text == faithful for the
                // deviation mark, so this is the exact faithful text
                // (retail band 15..19, geometry cutoff 20).
                faithful: "20 tiles",
            },
            desc: "How far you can see before the distance fog fully occludes, \
                   in tiles. Retail drew 20 tiles for pure period-performance \
                   reasons; note the monsters' sight radii (15-20 tiles) were \
                   tuned so pop-in hides in that fog — long distances reveal \
                   creatures acting before you could faithfully see them. \
                   Capped at 90: the horizon silhouettes beyond the fog always \
                   melt into the sky across 95..125 tiles (hiding the world's \
                   wrap-around), and the fog may never reach into that band.",
            ctl: Ctl::Stops {
                get: |c| c.render.preference.fog_distance,
                set: |c, v| c.render.preference.fog_distance = v,
                stops: &crate::config::FOG_STOPS,
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "vsync",
            class: Preference,
            key: None,
            cli: Some("--no-vsync"),
            cfg_path: "render.preference.vsync",
            // "Faithful" = ON only in the sense that on is the sane
            // default; a display-device knob with no retail analogue,
            // hence Preference (fidelity-free either way).
            read: |c| Val::Toggle {
                on: c.render.preference.vsync,
                faithful: true,
            },
            desc: "Vertical sync: frames wait for the display refresh. Off \
                   trades tearing for an uncapped frame rate — only useful \
                   together with the fps overlay (render \u{b7} debug) to \
                   measure what the machine can actually render.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.vsync = v,
                descs: [
                    "Uncapped frame rate, may tear. For fps measurement.",
                    "Frames sync to the display refresh (default).",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "fullscreen",
            class: Preference,
            key: Some("Alt+Enter"),
            cli: Some("--fullscreen"),
            cfg_path: "render.preference.fullscreen",
            // DOS ran one exclusive full-screen video mode and offered
            // no window, so fullscreen IS the faithful presentation —
            // and now the default. Still classed Preference: a display
            // device knob cannot affect the simulation either way.
            read: |c| Val::Toggle {
                on: c.render.preference.fullscreen,
                faithful: true,
            },
            desc: "Borderless fullscreen: the window loses its frame and covers \
                   the monitor it sits on. No exclusive video-mode switch, so \
                   alt-tab stays instant. At aspects wider than 4:3 the HUD \
                   panels anchor to the screen edges (castle left, spells \
                   right) instead of stretching; narrower than 4:3 the whole \
                   HUD scales down to fit the width. The 3D view keeps square \
                   pixels either way — the field of view widens or narrows \
                   with the screen.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.fullscreen = v,
                descs: [
                    "Windowed, 4:3 by default (resizable).",
                    "Borderless fullscreen on the current monitor.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "anti_aliasing",
            class: Preference,
            key: None,
            cli: Some("--anti-aliasing"),
            cfg_path: "render.preference.anti_aliasing",
            // A display knob with no retail analogue — DOS drew one
            // 320x200 buffer and filtered nothing — so it is
            // fidelity-free, like vsync.
            read: |c| Val::Choice {
                cur: match c.render.preference.anti_aliasing {
                    crate::config::AntiAliasing::Off => 0,
                    crate::config::AntiAliasing::Msaa => 1,
                    crate::config::AntiAliasing::Ssaa15 => 2,
                    crate::config::AntiAliasing::Ssaa2 => 3,
                },
                faithful: 0,
                variants: &["off", "msaa", "1.5x", "2x"],
            },
            desc: "Smooth the 3D view's jagged edges. MSAA is cheap but only \
                   reaches true geometry — chiefly the landscape against the sky \
                   — because creatures and buildings are cut out of their sprites \
                   by a hard transparency test that multisampling cannot soften; \
                   it also needs a restart, being built into the render \
                   pipelines. 1.5x and 2x supersample the WHOLE frame instead, \
                   which is the only thing that smooths those sprite outlines, at \
                   2.25x and 4x the pixels respectively.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.render.preference.anti_aliasing = match i {
                        1 => crate::config::AntiAliasing::Msaa,
                        2 => crate::config::AntiAliasing::Ssaa15,
                        3 => crate::config::AntiAliasing::Ssaa2,
                        _ => crate::config::AntiAliasing::Off,
                    }
                },
                descs: &[
                    "No smoothing — the original's hard pixel edges.",
                    "4x multisampling: cheap, landscape edges only (restart).",
                    "Supersample at 1.5x: smooths everything, ~2.25x the pixels.",
                    "Supersample at 2x: smoothest, 4x the pixels.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "movies",
            class: Preference,
            key: None,
            cli: Some("--no-movies"),
            cfg_path: "render.preference.movies",
            // Retail always plays them and has no switch, so ON is
            // the faithful reading; it is a Preference rather than a
            // fidelity knob because nothing downstream can tell.
            read: |c| Val::Toggle {
                on: c.render.preference.movies,
                faithful: c.render.preference.movies,
            },
            desc: "Play the full-screen movies: the intro chain at launch, \
                   Magic Carpet 2's six cutscenes between levels, and the \
                   ending. Any key skips the rest of a chain while it plays, \
                   so turning this off only saves the keypress. The movies \
                   have no soundtrack of their own — the original scores them \
                   from MIDI, because the format cannot hold audio.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.movies = v,
                descs: [
                    "Skip straight past intro, cutscenes and ending.",
                    "Play them (as the original does).",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "movie_subtitles",
            class: Preference,
            key: None,
            cli: Some("--movie-subtitles"),
            cfg_path: "render.preference.movie_subtitles",
            // Faithful when OFF for this build: retail shows the strip
            // only in non-English builds or with no sound device.
            read: |c| Val::Toggle {
                on: c.render.preference.movie_subtitles,
                faithful: !c.render.preference.movie_subtitles,
            },
            desc: "Subtitle the movies' narration. The original ties this to \
                   language, not preference: the voice track is English only, so \
                   it subtitles every non-English build, and an English one only \
                   when the machine has no sound card. Turning it on here forces \
                   the strip open — which lifts the picture a little, as the \
                   original does, to clear a band for the text.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.movie_subtitles = v,
                descs: [
                    "No subtitles (an English machine with sound hears the narration).",
                    "Show the narration as text.",
                ],
            },
        },
        Spec {
            domain: Render,
            // A Preference (retail MC2 ships a shading toggle —
            // Shift+F7 "Flat Shading"); the cfg_path keeps the legacy
            // "enhancement" segment so saved configs stay valid.
            group: "render · preference",
            label: "smooth_shading",
            class: Preference,
            key: Some("T"),
            cli: Some("--smooth-shading"),
            cfg_path: "render.enhancement.smooth_shading",
            read: toggle!(c => render.enhancement.smooth_shading),
            desc: "Terrain shading style. Off = one shade level per tile (the \
                   original look); on = shade interpolated across tile centers.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.smooth_shading = v,
                descs: [
                    "Per-tile shading — the faceted original look.",
                    "Interpolated (gouraud-like) terrain shading.",
                ],
            },
        },
        Spec {
            domain: Render,
            // A Preference (visual, fidelity-neutral, deliberately
            // unscored) — the cfg_path keeps the legacy "enhancement"
            // segment so saved configs stay valid.
            group: "render · preference",
            label: "hud_transparency",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.hud_transparency",
            read: |c| Val::Toggle {
                on: c.render.enhancement.hud_transparency.transparent(),
                // Default off (opaque); fidelity deliberately unscored
                // here (MC1 is always-transparent, MC2 has the toggle).
                faithful: false,
            },
            desc: "HUD panel transparency. MC1 always blends the HUD over the \
                   sky; MC2 offers the toggle (Panel Transparency). Opaque \
                   reads best, especially the radar.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.render.enhancement.hud_transparency = if v {
                        crate::config::HudTransparency::On
                    } else {
                        crate::config::HudTransparency::Off
                    }
                },
                descs: [
                    "Solid panels and radar — best readability (default).",
                    "The HUD blends over the world, MC1-style.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "rival_tags",
            // A Preference with a per-game faithful reading: retail
            // MC2 draws the tag over every visible rival wizard and
            // ships its "Player Names" toggle ON; retail MC1 has no
            // tag at all. `auto` IS each game's faithful surface;
            // `on` deviates only in MC1 (the opt-in), `off` is retail
            // MC2's own toggle position.
            class: Preference,
            key: None,
            cli: Some("--rival-tags"),
            cfg_path: "render.preference.rival_tags",
            read: |c| Val::Choice {
                cur: match c.render.preference.rival_tags {
                    crate::config::RivalTags::Auto => 0,
                    crate::config::RivalTags::On => 1,
                    crate::config::RivalTags::Off => 2,
                },
                faithful: 0,
                variants: &["auto", "on", "off"],
            },
            desc: "The rival wizard tags in the 3D view: each visible rival \
                   wears a boxed name + health bar in its team color, exactly \
                   as MC2 draws them. Auto = MC2 only (each game's own retail \
                   surface); on = MC1 too (an opt-in — retail MC1 never tags \
                   its rivals); off = no tags (MC2's own Player Names \
                   toggle, off).",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.render.preference.rival_tags = match i {
                        1 => crate::config::RivalTags::On,
                        2 => crate::config::RivalTags::Off,
                        _ => crate::config::RivalTags::Auto,
                    }
                },
                descs: &[
                    "MC2 tags its rivals, MC1 doesn't — each game as retail \
                     shipped it (default).",
                    "Both games tag their rivals — MC2's tag brought to MC1.",
                    "No rival tags anywhere.",
                ],
            },
        },
        // ---- render · enhancement ---------------------------------------
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "smooth_motion",
            // Purely visual smoothing (the sim is untouched, nothing
            // interactable changes) — Preference, not a fidelity
            // event: cleanup/visual preferences never flag the run
            // (fog_distance). Lives in the
            // enhancement group + cfg segment (legacy placement).
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.smooth_motion",
            // Faithful = OFF (retail steps everything at sim rate);
            // ships ON as a deliberate default-on deviation.
            read: |c| Val::Toggle {
                on: c.render.enhancement.smooth_motion,
                faithful: false,
            },
            desc: "Entities move frame-smooth: rendered interpolated between \
                   the last two sim ticks (the camera always has been), so \
                   movement glides at any fps instead of stepping at tick \
                   rate. Presentation only — the simulation is untouched; \
                   the displayed world runs one tick (~40 ms) behind.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.smooth_motion = v,
                descs: [
                    "Entities step at sim tick rate, as retail drew them.",
                    "Entities glide — per-frame interpolation (default).",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "fire",
            // Purely visual (the sim is identical whichever is chosen,
            // exactly like smooth_motion) — Preference, not a fidelity
            // event; lives in the enhancement group + cfg segment.
            class: Preference,
            key: None,
            cli: Some("--fire"),
            cfg_path: "render.enhancement.fire",
            read: |c| Val::Choice {
                cur: match c.render.enhancement.fire {
                    crate::config::FireEffects::Classic => 0,
                    crate::config::FireEffects::Enhanced => 1,
                },
                faithful: 0,
                variants: &["classic", "enhanced"],
            },
            desc: "The fire look. Classic = the retail fire and explosion \
                   sprites, exactly as the running game draws them. Enhanced = \
                   procedural fire: the fireball becomes a flame with a comet \
                   trail (its core sprite hidden), the meteor blast an \
                   expanding two-wave flame front leaving lingering smoke, \
                   capped by a detaching shockwave ring. Presentation only — \
                   the simulation is identical either way. Needs smooth \
                   motion; with it off, classic draws regardless.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.render.enhancement.fire = match i {
                        1 => crate::config::FireEffects::Enhanced,
                        _ => crate::config::FireEffects::Classic,
                    }
                },
                descs: &[
                    "Retail fire/explosion sprites, as the original drew them \
                     (default).",
                    "Procedural flame, smoke and shockwave.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "lightning",
            // Purely visual, exactly like fire — Preference class.
            class: Preference,
            key: None,
            cli: Some("--lightning"),
            cfg_path: "render.enhancement.lightning",
            read: |c| Val::Choice {
                cur: match c.render.enhancement.lightning {
                    crate::config::LightningEffects::Classic => 0,
                    crate::config::LightningEffects::Enhanced => 1,
                },
                faithful: 0,
                variants: &["classic", "enhanced"],
            },
            desc: "The lightning look. Classic = the retail zigzag flash \
                   sprites, exactly as the running game draws them. Enhanced = \
                   a procedural bolt: a fractal main channel with small side \
                   branches, each strike playing a leader / return-stroke / \
                   decay envelope, successive strikes of a held stream \
                   overlapping into one continuous dancing arc. Presentation \
                   only — the simulation is identical either way. Needs \
                   smooth motion; with it off, classic draws regardless.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.render.enhancement.lightning = match i {
                        1 => crate::config::LightningEffects::Enhanced,
                        _ => crate::config::LightningEffects::Classic,
                    }
                },
                descs: &[
                    "Retail zigzag flash sprites, as the original drew them \
                     (default).",
                    "Procedural fractal bolt with branches and a strike \
                     envelope.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "map_owned_buildings",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.map_owned_buildings",
            read: toggle!(c => render.enhancement.map_owned_buildings),
            desc: "Mark dwellings on the overhead map the way MC2 does: \
                   unclaimed pink, claimed/possessed flashing in the owner's \
                   color — brought to MC1 as an opt-in (retail MC1 never \
                   marks buildings).",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_owned_buildings = v,
                descs: [
                    "Unmarked dwellings, as retail MC1 draws them.",
                    "MC2-style markers: unclaimed pink, owned flashing in \
                     the owner's color.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "map_marker_scale",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.map_marker_scale",
            read: |c| Val::Scalar {
                text: format!("{:.2}x", c.render.enhancement.map_marker_scale),
                // Retail's markers are surface-pixel-scale (near-
                // invisible at modern resolutions), so there is no
                // faithful size; 1.00x is the shipped baseline.
                faithful: "1.00x",
            },
            desc: "Overhead-map marker size (entity dots and icon stamps \
                   together, on the map screen and the radar). Off 1x, \
                   marker size stays constant as the radar zooms.",
            ctl: Ctl::Slider {
                get: |c| c.render.enhancement.map_marker_scale,
                set: |c, v| c.render.enhancement.map_marker_scale = v,
                min: 0.5,
                max: 3.0,
                step: 0.25,
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "map_marker_icons",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.map_marker_icons",
            read: toggle!(c => render.enhancement.map_marker_icons),
            desc: "Swap selected map dots for miniature pictures of the \
                   thing itself: spell jars, dolmens/shrines, statues. \
                   Drawn small — half the spell-stamp size — and scaled \
                   by the marker-size slider. (The expose-jar-spells \
                   debug option outranks this for jars.)",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_marker_icons = v,
                descs: [
                    "Plain dots, as retail draws them.",
                    "Jars, dolmens and statues wear miniature sprites.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "map_extent_fog",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.map_extent_fog",
            read: toggle!(c => render.enhancement.map_extent_fog),
            desc: "Fade the fullscreen map to black beyond the world's \
                   true extent. The world wraps, so the player-centered \
                   map repeats entities past half a world away — retail \
                   shows the duplicates; the fog hides them. The extent \
                   rectangle rotates with your heading.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_extent_fog = v,
                descs: [
                    "The map repeats past the world seam, as retail.",
                    "Soft black fog past the true extent hides the \
                     wrap-around duplicates.",
                ],
            },
        },
        // ---- render · debug ---------------------------------------------
        Spec {
            domain: Render,
            // A level-scouting instrument that only lets you SEE more
            // (the original never labels jars) — Debug, not
            // Enhancement; the cfg_path keeps its legacy segment so
            // saved configs stay valid.
            group: "render · debug",
            label: "expose_jar_spells",
            class: Debug,
            key: None,
            cli: Some("--expose-jar-spells"),
            cfg_path: "render.enhancement.expose_jar_spells",
            read: toggle!(c => render.enhancement.expose_jar_spells),
            desc: "Tag every pickable spell jar with its granted spell's icon — \
                   on the overhead map and floating over the jar in the main \
                   view. The original never labels jars; you learn by flying \
                   through.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.expose_jar_spells = v,
                descs: [
                    "Anonymous jars, as retail.",
                    "Every jar wears its spell icon.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "health_bars",
            class: Debug,
            key: Some("B"),
            cli: Some("--health-bars"),
            cfg_path: "render.debug.health_bars",
            read: toggle!(c => render.debug.health_bars),
            desc: "Red-on-black health bars floating above everything \
                   destroyable — monsters, dwellings and other structures. \
                   The original never shows this life — the combat-system \
                   debugging instrument.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.health_bars = v,
                descs: [
                    "No life shown, as retail.",
                    "Everything destroyable wears a life bar.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "autoaim_hints",
            class: Debug,
            key: None,
            cli: None,
            cfg_path: "render.debug.autoaim_hints",
            read: toggle!(c => render.debug.autoaim_hints),
            desc: "Autoaim lock markers: blinking per-hand markers on the \
                   target each equipped spell would acquire this instant \
                   (left +, right \u{d7}). A projectile-behavior predictor; \
                   the original shows no such UI. The plain aim crosshair \
                   is its own option (render \u{b7} preference).",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.autoaim_hints = v,
                descs: [
                    "No lock markers, as retail.",
                    "Blinking per-hand lock markers on the acquired target.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "map_trigger_areas",
            class: Debug,
            key: Some("V"),
            cli: Some("--map-triggers"),
            cfg_path: "render.debug.map_trigger_areas",
            read: toggle!(c => render.debug.map_trigger_areas),
            desc: "Overlay live trigger volumes / portals on the overhead map \
                   as tinted circles. The original never reveals trigger areas \
                   — the event-system debugging instrument.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.map_trigger_areas = v,
                descs: [
                    "No trigger overlay, as retail.",
                    "Trigger volumes tinted on the map.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "grace_meter",
            class: Debug,
            key: None,
            cli: Some("--grace-meter"),
            cfg_path: "render.debug.grace_meter",
            read: toggle!(c => render.debug.grace_meter),
            desc: "A thin bottom-center strip draining with the respawn \
                   invulnerability window. Retail shows nothing for spawn \
                   grace.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.grace_meter = v,
                descs: [
                    "No grace indicator, as retail.",
                    "The spawn-grace strip while invulnerable.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "fps",
            class: Debug,
            key: None,
            cli: Some("--fps"),
            cfg_path: "render.debug.fps",
            read: toggle!(c => render.debug.fps),
            desc: "Frame rate + frame time, bottom-right corner. The \
                   performance instrument for weighing effect costs; with \
                   vsync on it reads the display refresh, not the machine's \
                   limit — turn vsync off (render \u{b7} preference) to \
                   measure real headroom.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.fps = v,
                descs: [
                    "No frame-rate readout, as retail.",
                    "Live fps + ms per frame, refreshed twice a second.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "coords",
            class: Debug,
            key: Some("K"),
            cli: Some("--coords"),
            cfg_path: "render.debug.coords",
            read: toggle!(c => render.debug.coords),
            desc: "Carpet coordinates, bottom-left corner, engine units: \
                   \"x N, y N, z N (+E)\" \u{2014} z is the altitude, (+E) \
                   the elevation over the terrain underneath. The flight/\
                   altitude debugging instrument (the altitude bands speak \
                   engine units: clearance 128/256, band 1024/3072).",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.coords = v,
                descs: [
                    "No coordinate readout, as retail.",
                    "Live position + elevation over terrain.",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "entities",
            class: Debug,
            key: None,
            cli: Some("--entities"),
            cfg_path: "render.debug.entities",
            read: toggle!(c => render.debug.entities),
            desc: "Live entity-pool usage, \"ents used/capacity\", on its \
                   own line above the coordinate readout (bottom-left). \
                   In-use counts every allocated slot including corpses \
                   awaiting the reaper. The entity-pressure diagnosis \
                   instrument \u{2014} effect leaks, pool-exhaustion \
                   anomalies.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.entities = v,
                descs: [
                    "No entity readout, as retail.",
                    "Live pool usage above the coordinate line.",
                ],
            },
        },
        // ---- controls · preferences -------------------------------------
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "bindings",
            class: Preference,
            key: None,
            cli: Some("--bindings"),
            cfg_path: "controls.preferences.bindings",
            read: |c| Val::Choice {
                cur: match c.controls.preferences.bindings {
                    crate::config::Bindings::Classic => 0,
                    crate::config::Bindings::Wasd => 1,
                },
                faithful: 0,
                variants: &["classic", "wasd"],
            },
            desc: "Key-binding profile for movement.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.controls.preferences.bindings = if i == 0 {
                        crate::config::Bindings::Classic
                    } else {
                        crate::config::Bindings::Wasd
                    }
                },
                descs: &[
                    "The original scheme: mouse aims, Up/Down arrows \
                     accelerate/decelerate, Left/Right strafe.",
                    "W/S thrust, A/D strafe, mouse aims (arrows keep \
                     turn/pitch in the enhanced thrust model).",
                ],
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "mouse_sensitivity",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.mouse_sensitivity",
            read: |c| Val::Scalar {
                text: format!("{:.1}", c.controls.preferences.mouse_sensitivity),
                faithful: "1.0",
            },
            desc: "Mouse-to-stick / mouse-look sensitivity multiplier.",
            ctl: Ctl::Slider {
                get: |c| c.controls.preferences.mouse_sensitivity,
                set: |c, v| c.controls.preferences.mouse_sensitivity = v,
                min: 0.1,
                max: 3.0,
                step: 0.1,
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "mouse_sensitivity_x",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.mouse_sensitivity_x",
            read: |c| Val::Scalar {
                text: format!("{:.0}%", c.controls.preferences.mouse_sensitivity_x * 100.0),
                faithful: "50%",
            },
            desc: "Horizontal (turn) share of the mouse sensitivity, \
                   0-100%. Defaults to 50% — at full share the enhanced \
                   turn damper saturates on a flick (all-or-nothing \
                   turning); half keeps the whole turn-rate range \
                   reachable.",
            ctl: Ctl::Slider {
                get: |c| c.controls.preferences.mouse_sensitivity_x,
                set: |c, v| c.controls.preferences.mouse_sensitivity_x = v,
                min: 0.0,
                max: 1.0,
                step: 0.05,
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "mouse_sensitivity_y",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.mouse_sensitivity_y",
            read: |c| Val::Scalar {
                text: format!("{:.0}%", c.controls.preferences.mouse_sensitivity_y * 100.0),
                faithful: "100%",
            },
            desc: "Vertical (aim) share of the mouse sensitivity, \
                   0-100%.",
            ctl: Ctl::Slider {
                get: |c| c.controls.preferences.mouse_sensitivity_y,
                set: |c, v| c.controls.preferences.mouse_sensitivity_y = v,
                min: 0.0,
                max: 1.0,
                step: 0.05,
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "invert_y",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.invert_y",
            read: |c| Val::Toggle {
                on: c.controls.preferences.invert_y,
                // The flight-stick polarity both originals ship.
                faithful: true,
            },
            desc: "Mouse Y polarity. On = mouse up/forward dives (nose down), \
                   like a flight stick — the polarity both originals ship. \
                   Off = mouse up climbs (the FPS convention).",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.invert_y = v,
                descs: [
                    "Mouse up = nose up (FPS convention).",
                    "Mouse up = nose down, flight-stick style (the \
                     original polarity; default).",
                ],
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "fly_assistant",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.fly_assistant",
            read: |c| Val::Toggle {
                on: c.controls.preferences.fly_assistant.on(),
                faithful: false,
            },
            desc: "The retail MC2 Flight Assistance option: leave the mouse \
                   untouched for a couple of seconds and the steering stick \
                   recenters itself (level flight holds). Off by default, \
                   like retail MC2; MC1 never had it.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.controls.preferences.fly_assistant = if v {
                        crate::config::FlyAssistant::On
                    } else {
                        crate::config::FlyAssistant::Off
                    }
                },
                descs: [
                    "No auto-center — you trim your own drift (retail \
                     default).",
                    "Idle mouse recenters the steering stick.",
                ],
            },
        },
        // ---- controls · models ------------------------------------------
        Spec {
            domain: Controls,
            group: "controls · models",
            label: "thrust",
            class: Enhancement,
            key: None,
            cli: Some("--thrust"),
            cfg_path: "controls.models.thrust",
            read: |c| Val::Choice {
                cur: match c.controls.models.thrust {
                    crate::config::ThrustModel::Classic => 0,
                    crate::config::ThrustModel::Enhanced => 1,
                },
                faithful: 0,
                variants: &["classic", "enhanced"],
            },
            desc: "Thrust + steering model. Classic is the faithful law both \
                   originals share; enhanced is the modern hold-to-fly \
                   alternative.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.controls.models.thrust = if i == 0 {
                        crate::config::ThrustModel::Classic
                    } else {
                        crate::config::ThrustModel::Enhanced
                    }
                },
                descs: &[
                    "The faithful model: mouse offset = turn rate (airplane \
                     stick, recenter to fly straight); accelerate/decelerate \
                     impulses persist until countered.",
                    "Mouse look + hold-to-fly with automatic deceleration on \
                     release.",
                ],
            },
        },
        Spec {
            domain: Controls,
            group: "controls · models",
            label: "altitude",
            class: Enhancement,
            key: None,
            cli: Some("--altitude"),
            cfg_path: "controls.models.altitude",
            read: |c| Val::Choice {
                cur: match c.controls.models.altitude {
                    crate::config::AltitudeModel::Classic => 0,
                    crate::config::AltitudeModel::Enhanced => 1,
                },
                faithful: 0,
                variants: &["classic", "enhanced"],
            },
            desc: "Altitude model. Classic = terrain-follow only, as the \
                   originals; enhanced adds explicit float keys.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.controls.models.altitude = if i == 0 {
                        crate::config::AltitudeModel::Classic
                    } else {
                        crate::config::AltitudeModel::Enhanced
                    }
                },
                descs: &[
                    "Terrain-follow only: the carpet floats up along rising \
                     ground and settles by itself; no fly-up control exists.",
                    "Classic behavior plus E/Q float up/down, capped at the \
                     level's highest terrain.",
                ],
            },
        },
        // ---- audio ------------------------------------------------------
        Spec {
            domain: Audio,
            group: "audio",
            label: "sound",
            class: Preference,
            key: Some("F1"),
            cli: None,
            cfg_path: "audio.sound",
            read: |c| Val::Toggle {
                on: c.audio.sound,
                faithful: true,
            },
            desc: "Sample playback (the original's F1 toggle).",
            ctl: Ctl::Toggle {
                set: |c, v| c.audio.sound = v,
                descs: ["Silence the sound effects.", "Sound effects play."],
            },
        },
        Spec {
            domain: Audio,
            group: "audio",
            label: "music",
            class: Preference,
            key: Some("F2"),
            cli: None,
            cfg_path: "audio.music",
            read: |c| Val::Toggle {
                on: c.audio.music,
                faithful: true,
            },
            desc: "Music playback (the original's F2 toggle).",
            ctl: Ctl::Toggle {
                set: |c, v| c.audio.music = v,
                descs: ["No music.", "The level soundtrack plays."],
            },
        },
        Spec {
            domain: Audio,
            group: "audio",
            label: "sfx_volume",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "audio.sfx_volume",
            read: |c| Val::Scalar {
                text: format!("{:.1}", c.audio.sfx_volume),
                faithful: "1.0",
            },
            desc: "Sound-effect master gain.",
            ctl: Ctl::Slider {
                get: |c| c.audio.sfx_volume,
                set: |c, v| c.audio.sfx_volume = v,
                min: 0.0,
                max: 1.0,
                step: 0.1,
            },
        },
        Spec {
            domain: Audio,
            group: "audio",
            label: "music_volume",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "audio.music_volume",
            read: |c| Val::Scalar {
                text: format!("{:.1}", c.audio.music_volume),
                faithful: "1.0",
            },
            desc: "Music master gain.",
            ctl: Ctl::Slider {
                get: |c| c.audio.music_volume,
                set: |c, v| c.audio.music_volume = v,
                min: 0.0,
                max: 1.0,
                step: 0.1,
            },
        },
        Spec {
            domain: Audio,
            group: "audio",
            label: "arrangement",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "audio.arrangement",
            read: |c| Val::Choice {
                cur: match c.audio.arrangement {
                    crate::config::MusicArrangement::Auto => 0,
                    crate::config::MusicArrangement::Fm => 1,
                    crate::config::MusicArrangement::Gm => 2,
                },
                faithful: 0,
                variants: &["auto", "fm", "gm"],
            },
            desc: "Which MC1 music arrangement plays — the CD shipped one per \
                   sound-card family, so each is authentic. Applies at level \
                   load.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.audio.arrangement = match i {
                        0 => crate::config::MusicArrangement::Auto,
                        1 => crate::config::MusicArrangement::Fm,
                        _ => crate::config::MusicArrangement::Gm,
                    }
                },
                descs: &[
                    "The best-available render: General MIDI when baked, \
                     else FM.",
                    "The AdLib FM (OPL3) render.",
                    "The General MIDI render.",
                ],
            },
        },
        Spec {
            domain: Audio,
            group: "audio",
            label: "speech",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "audio.speech",
            read: |c| Val::Toggle {
                on: c.audio.speech,
                faithful: true,
            },
            desc: "MC2 objective voiceovers (the CD speech clips) — the \
                   original's in-game Speech option.",
            ctl: Ctl::Toggle {
                set: |c, v| c.audio.speech = v,
                descs: ["Objectives arrive silently.", "The narrator speaks."],
            },
        },
        Spec {
            domain: Audio,
            group: "audio",
            label: "subtitles",
            class: Preference,
            key: None,
            cli: Some("--subtitles"),
            cfg_path: "audio.subtitles",
            read: |c| Val::Toggle {
                on: c.audio.subtitles.on(),
                faithful: true,
            },
            desc: "Narration subtitles: the sentence behind each objective \
                   voiceover, drawn as a top-of-screen overtitle when the cue \
                   fires.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.audio.subtitles = if v {
                        crate::config::Subtitles::On
                    } else {
                        crate::config::Subtitles::Off
                    }
                },
                descs: [
                    "No narration text.",
                    "Every narration is subtitled (default).",
                ],
            },
        },
        // ---- gameplay · enhancement -------------------------------------
        Spec {
            domain: Gameplay,
            group: "gameplay · enhancement",
            label: "spell_selector",
            class: Enhancement,
            key: None,
            cli: Some("--spell-selector"),
            cfg_path: "gameplay.enhancement.spell_selector",
            read: |c| Val::Choice {
                cur: match c.gameplay.enhancement.spell_selector {
                    crate::config::SpellSelector::Auto => 0,
                    crate::config::SpellSelector::Mc1 => 1,
                    crate::config::SpellSelector::Mc2 => 2,
                    crate::config::SpellSelector::Mc1Mc2 => 3,
                },
                faithful: 0,
                variants: &["auto", "mc1", "mc2", "mc1+mc2"],
            },
            desc: "Which spell-selection interface is live — interface only, \
                   the spell economy underneath is untouched. Switchable \
                   mid-run; quick-key digit binds survive the round trip \
                   (MC2 always uses the CTRL pane).",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.gameplay.enhancement.spell_selector = match i {
                        0 => crate::config::SpellSelector::Auto,
                        1 => crate::config::SpellSelector::Mc1,
                        2 => crate::config::SpellSelector::Mc2,
                        _ => crate::config::SpellSelector::Mc1Mc2,
                    }
                },
                descs: &[
                    "Each game's own faithful surface: MC1 the map-screen \
                     spellbook, MC2 the CTRL pane.",
                    "Force the MC1 map-screen spellbook (MC1 only).",
                    "Force the MC2 CTRL-hold selector pane.",
                    "Both surfaces at once (MC1 only).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · enhancement",
            label: "wheel_spells",
            // Purely additive input (retail has no wheel binding at
            // all), so it does not flag the run.
            class: Preference,
            key: None,
            cli: Some("--no-wheel-spells"),
            cfg_path: "gameplay.enhancement.wheel_spells",
            read: |c| Val::Toggle {
                on: c.gameplay.enhancement.wheel_spells,
                faithful: false,
            },
            desc: "Mouse-wheel spell cycling (the remc2/MC2HD idiom): the \
                   wheel walks the left button's queued-spell ring, \
                   SHIFT+wheel the right. The faithful SHIFT/ALT+click \
                   rotation works either way.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.enhancement.wheel_spells = v,
                descs: [
                    "Wheel does nothing in flight, as retail.",
                    "Wheel cycles the queued spells (default).",
                ],
            },
        },
        // ---- gameplay · patches -----------------------------------------
        // Retail-bug patches (docs/DEVIATIONS.md "Patch options"):
        // every deliberate upstream bugfix, both arms implemented.
        // Faithful = the RETAIL arm (off); most ship ON as the
        // intended default experience. Fixture safety is structural —
        // goldens/tests/mgc-conform build retail-arm worlds and
        // --record/--replay force the retail arms — so a patched run
        // still rolls up FAITHFUL, with the patch count reported.
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "castle_recast_cost",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.castle_recast_cost",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.castle_recast_cost.on(),
                faithful: false,
            },
            desc: "Create Castle pricing after castle loss. Retail never re-prices \
                   when your castle dies: the recast keeps the last ladder \
                   price (the first-castle lockout) until collected mana \
                   raises your ceiling past it. The patch re-derives the \
                   live price, so a fresh castle always costs 1000.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.castle_recast_cost = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "The retail lockout - maybe a deliberate challenge (default).",
                    "Live pricing: a fresh castle is always affordable.",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "jar_ground_snap",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.jar_ground_snap",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.jar_ground_snap.on(),
                faithful: false,
            },
            desc: "Spell jars follow the ground. Retail's terrain reshaping skips \
                   jars, leaving them buried (Hidden Worlds ships some!) or \
                   floating after earth-shaping spells.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.jar_ground_snap = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Jars freeze in place through terrain edits, as retail.",
                    "Jars stay grounded and collectable (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "ball_ground_track",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.ball_ground_track",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.ball_ground_track.on(),
                faithful: false,
            },
            desc: "Settled mana balls follow the ground (MC1). Retail freezes a \
                   ball wherever its settle timer ran out - mid-hop balls \
                   hang in the air forever, and terrain edits bury grounded \
                   ones.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.ball_ground_track = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Settled balls freeze mid-air or get buried, as retail.",
                    "Settled balls land and surface (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "map_wide_ball_rolling",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.map_wide_ball_rolling",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.map_wide_ball_rolling.on(),
                faithful: false,
            },
            desc: "Mana balls roll downhill everywhere (MC1). Retail only rolls \
                   balls within 24 tiles of you - a period perf save - so \
                   approaching mana wakes it and it visibly runs away \
                   downhill. Balls only; creature wake-up is untouched.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.map_wide_ball_rolling = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Balls only roll near you and run away downhill, as retail.",
                    "Every ball rolls to rest, map-wide (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "possessed_footprint",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.possessed_footprint",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.possessed_footprint.on(),
                faithful: false,
            },
            desc: "A possessed dwelling keeps its true footprint (MC1). Retail \
                   shrinks it to the owner-flag sprite, so villagers and \
                   defenders spawn walled-in on the roof and their corpse \
                   flames destroy the house you just possessed.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.possessed_footprint = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Possessed villages slowly self-destruct, as retail.",
                    "Possessed villages keep working (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_downgrade_overflow",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_downgrade_overflow",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_downgrade_overflow.on(),
                faithful: false,
            },
            desc: "MC2 castle downgrade's 10% mana haircut, computed safely. \
                   Retail's 32-bit math overflows at the maxed level-7 rung: \
                   the downgrade RAISES capacity and scatters nothing.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_downgrade_overflow = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "The level-7 haircut overflows backwards, as retail.",
                    "The haircut is always 10% (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_magic_mine",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_magic_mine",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_magic_mine.on(),
                faithful: false,
            },
            desc: "MC2 Magic Mine proximity trigger. Retail ships the spell dead: \
                   nothing ever arms the trigger, so a mine floats, expires \
                   and sinks without detonating on anyone.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.patches.mc2_magic_mine = crate::config::PatchArm::from_on(v),
                descs: [
                    "Mines never detonate - a dead spell, as retail.",
                    "Mines detonate on approaching enemies (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_dweller_invisibility",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_dweller_invisibility",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_dweller_invisibility.on(),
                faithful: false,
            },
            desc: "MC2 mana dwellers fade into view only up close, like the \
                   zombie. Their sneaky mana stealing relied on retail's very \
                   short view range; with the port's long view they stand \
                   exposed to safe long-range meteors.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_dweller_invisibility =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Dwellers are plainly visible at any range, as retail draws them.",
                    "Dwellers materialize up close, restoring their stealth (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "win2_movie_score",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.win2_movie_score",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.win2_movie_score.on(),
                faithful: false,
            },
            desc: "The second win movie plays its own score. Retail points both \
                   win movies at one script, so levelw2 plays against the \
                   wrong sample bank; its own orphaned table (bank 7, win2) \
                   sits unreferenced in the binary.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.win2_movie_score = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "levelw2 plays the shared win1 score, as retail.",
                    "levelw2 plays its own win2 score (default).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "castle_latch_bug",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.castle_latch_bug",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.castle_latch_bug.on(),
                faithful: false,
            },
            desc: "Create Castle placement validation (MC1). Retail checks the \
                   tile beside your casting hand - steerable by aim - and \
                   skips the check when the ball hits terrain, so a cast \
                   glued to the right wall corner raises a castle inside a \
                   no-castle maze and carves its protected walls.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.castle_latch_bug = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "The skippable hand-side check - maze castles work, as retail.",
                    "The check anchors on you and always runs (default).",
                ],
            },
        },
        // ---- gameplay · cheat -------------------------------------------
        Spec {
            domain: Gameplay,
            group: "gameplay · cheat",
            label: "dev_spells",
            class: Cheat,
            key: Some("G"),
            cli: Some("--dev-spells"),
            cfg_path: "gameplay.cheat.dev_spells",
            read: toggle!(c => gameplay.cheat.dev_spells),
            desc: "All spells granted + infinite mana — the spell-track \
                   playtest instrument. The original ships the equivalent \
                   debug commands.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.dev_spells = v,
                descs: [
                    "Authentic acquisition and mana.",
                    "Every spell, bottomless mana (cheat).",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · cheat",
            label: "invincible",
            class: Cheat,
            key: Some("H"),
            cli: Some("--invincible"),
            cfg_path: "gameplay.cheat.invincible",
            read: toggle!(c => gameplay.cheat.invincible),
            desc: "Player invincibility: damage is totaled for display but \
                   never applied; no death. Playtest/accessibility \
                   instrument.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.invincible = v,
                descs: [
                    "Mortal, as the game intends.",
                    "Nothing can kill you (cheat).",
                ],
            },
        },
        // ---- dev --------------------------------------------------------
        Spec {
            domain: Dev,
            group: "dev",
            label: "plausible_spellbook",
            class: Instrument,
            key: None,
            cli: Some("--plausible-spellbook"),
            cfg_path: "dev.plausible_spellbook",
            read: toggle!(c => dev.plausible_spellbook),
            desc: "Seed the spellbook at level start with the spells a \
                   diligent player COULD legitimately hold entering this \
                   level (MC1 only). Applies at level load.",
            ctl: Ctl::Toggle {
                set: |c, v| c.dev.plausible_spellbook = v,
                descs: [
                    "Only what the level itself grants.",
                    "The campaign-plausible spell set at entry.",
                ],
            },
        },
        Spec {
            domain: Dev,
            group: "dev",
            label: "lift_unclamped",
            class: Instrument,
            key: None,
            cli: None,
            cfg_path: "dev.lift_unclamped",
            read: toggle!(c => dev.lift_unclamped),
            desc: "Unclamp the enhanced-altitude band: q/e may pin the \
                   desired altitude anywhere up to the level's highest \
                   terrain + a 4-tile margin, instead of the per-game \
                   ground-relative band. Altitude-system inspection; \
                   applies live.",
            ctl: Ctl::Toggle {
                set: |c, v| c.dev.lift_unclamped = v,
                descs: [
                    "The per-game band: 1024 over terrain (MC2 caves 3072).",
                    "Free climb to the global lift ceiling (instrument).",
                ],
            },
        },
    ]
}

/// Roll the whole config up to a single run-fidelity verdict, plus the
/// counts that back it.
pub fn rollup(cfg: &Config) -> (Fidelity, usize, usize, usize) {
    let mut enhancements = 0;
    let mut modifiers = 0;
    let mut patches = 0;
    for spec in registry() {
        let val = (spec.read)(cfg);
        if !val.deviates() {
            continue;
        }
        if spec.class == Class::Patch {
            // Counted apart: a patched arm never flips the verdict
            // (fixture capture forces the retail arms structurally).
            patches += 1;
            continue;
        }
        match spec.class.fidelity() {
            Fidelity::Enhanced => enhancements += 1,
            Fidelity::Modified => modifiers += 1,
            Fidelity::Faithful => {}
        }
    }
    let verdict = if modifiers > 0 {
        Fidelity::Modified
    } else if enhancements > 0 {
        Fidelity::Enhanced
    } else {
        Fidelity::Faithful
    };
    (verdict, enhancements, modifiers, patches)
}

/// Print the structured options summary at startup: one line per
/// option under its `domain · group` heading, current value pointed
/// out, alternatives (faithful `*`-marked) and the toggle comment
/// trailing. Non-faithful selections are flagged with a leading `•`.
pub fn print_summary(cfg: &Config, game: GameId, level_label: &str) {
    let (verdict, enh, modi, patches) = rollup(cfg);
    let mut banner = match verdict {
        Fidelity::Faithful => "FAITHFUL".to_string(),
        Fidelity::Enhanced => format!("ENHANCED ({enh} enhancement(s), 0 cheats)"),
        Fidelity::Modified => {
            format!("MODIFIED ({modi} modifier(s), {enh} enhancement(s)) — not a faithful run")
        }
    };
    if patches > 0 {
        banner.push_str(&format!(
            " ({patches} retail patch(es) on; recordings and fixtures run retail arms)"
        ));
    }
    let game_name = match game {
        GameId::Mc1 => "Magic Carpet",
        GameId::Mc1Hw => "Magic Carpet: Hidden Worlds",
        GameId::Mc2 => "Magic Carpet 2",
    };
    println!("\n{game_name} · {level_label}");
    println!("Run fidelity: {banner}");

    let specs = registry();
    // Column width for the value column (aligned across all options).
    let val_w = specs
        .iter()
        .map(|s| (s.read)(cfg).current_text().len())
        .max()
        .unwrap_or(0)
        .max(6);
    let hint_w = specs
        .iter()
        .map(|s| (s.read)(cfg).choices_hint().len())
        .max()
        .unwrap_or(0);

    let mut last_group = "";
    for spec in &specs {
        if spec.group != last_group {
            println!("{}", spec.group.to_uppercase());
            last_group = spec.group;
        }
        let val = (spec.read)(cfg);
        let mark = if val.deviates() { "•" } else { " " };
        let offline = if spec.mutability() == Mutability::Startup {
            "  (level load)"
        } else {
            ""
        };
        println!(
            "  {mark} {label:<20} {value:<val_w$}  {hint:<hint_w$}  {toggle}{offline}",
            label = spec.label,
            value = val.current_text(),
            hint = val.choices_hint(),
            toggle = spec.toggle_hint(),
        );
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn stock_run_is_faithful() {
        // The deliberate default deviations (fog 50, hud opaque) are
        // all Preference-class, so a stock run
        // rolls up FAITHFUL (cleanup/visual preferences must not flag
        // the run).
        let (verdict, enh, modi, patches) = rollup(&Config::default());
        assert_eq!(modi, 0, "no cheats/instruments on by default");
        assert_eq!(enh, 0, "no enhancement-class deviation by default");
        assert_eq!(verdict, Fidelity::Faithful);
        // The default-on retail patches count apart and never flip
        // the verdict (castle_recast_cost ships on its retail arm).
        // castle_death_mana/balloons retired 2026-08-12 — the mc1l0
        // corpus proved the scatter + fleet cull are retail law
        // (sub_470E0's wrapper), so they are unconditional now.
        assert_eq!(patches, 9, "nine of the ten patches ship on");
    }

    #[test]
    fn a_cheat_makes_the_run_modified() {
        let mut c = Config::default();
        c.gameplay.cheat.dev_spells = true;
        let (verdict, _, modi, _) = rollup(&c);
        assert_eq!(verdict, Fidelity::Modified);
        assert!(modi >= 1);
    }

    #[test]
    fn every_option_reads_and_the_summary_prints() {
        // Exercise every registry reader + the whole printer (no panic,
        // widths compute) and eyeball it under `--nocapture`.
        for spec in registry() {
            let _ = (spec.read)(&Config::default());
        }
        print_summary(&Config::default(), GameId::Mc2, "level-000 (smoke)");
    }

    #[test]
    fn every_ctl_setter_round_trips() {
        // Every widget setter lands where its reader looks: setting
        // each selectable value and reading it back must agree (guards
        // against a Spec whose `read` and `ctl` drift apart).
        for (i, spec) in registry().into_iter().enumerate() {
            let mut c = Config::default();
            match spec.ctl {
                Ctl::ReadOnly => {}
                Ctl::Toggle { set, .. } => {
                    for on in [true, false] {
                        set(&mut c, on);
                        match (spec.read)(&c) {
                            Val::Toggle { on: got, .. } => {
                                assert_eq!(got, on, "spec #{i} {} toggle", spec.label)
                            }
                            _ => panic!("spec #{i} {}: Toggle ctl but non-Toggle read", spec.label),
                        }
                    }
                }
                Ctl::Choice { set, descs } => {
                    let variants = match (spec.read)(&c) {
                        Val::Choice { variants, .. } => variants,
                        _ => panic!("spec #{i} {}: Choice ctl but non-Choice read", spec.label),
                    };
                    assert_eq!(
                        descs.len(),
                        variants.len(),
                        "spec #{i} {}: per-choice descs align with variants",
                        spec.label
                    );
                    for want in 0..variants.len() {
                        set(&mut c, want);
                        match (spec.read)(&c) {
                            Val::Choice { cur, .. } => {
                                assert_eq!(cur, want, "spec #{i} {} choice", spec.label)
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                Ctl::Slider {
                    get,
                    set,
                    min,
                    max,
                    step,
                } => {
                    assert!(min < max && step > 0.0, "spec #{i} {}", spec.label);
                    set(&mut c, min);
                    assert!((get(&c) - min).abs() < 1e-6, "spec #{i} {}", spec.label);
                    set(&mut c, max);
                    assert!((get(&c) - max).abs() < 1e-6, "spec #{i} {}", spec.label);
                }
                Ctl::Stops { get, set, stops } => {
                    assert!(!stops.is_empty(), "spec #{i} {}", spec.label);
                    for &(v, _) in stops {
                        set(&mut c, v);
                        assert_eq!(get(&c), v, "spec #{i} {} stop", spec.label);
                    }
                }
            }
        }
    }
}
