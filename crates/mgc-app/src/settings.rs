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
    /// The tab title. Display only: the config keeps its own section
    /// names (`render.*` shows as VISUALS — player ruling 2026-09-25).
    pub fn title(self) -> &'static str {
        match self {
            Domain::Sim => "SIM",
            Domain::Render => "VISUALS",
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

/// A preset: one named mask over a group of options (player ruling
/// 2026-09-25). Choosing a preset writes every member option; the
/// group's current preset is never stored. It is READ BACK from the
/// member values: the preset whose mask they all match, or "custom"
/// when they match none (a hand-tuned member). So there is nothing new
/// to persist, and nothing to fall out of step with the options it
/// names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Enhanced,
    Classic,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::Enhanced, Preset::Classic];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Enhanced => "Enhanced",
            Preset::Classic => "Classic",
        }
    }
}

/// A group of options set together by [`Preset`]s.
pub struct PresetGroup {
    /// The group's registry row. A VIRTUAL path (no config field):
    /// persisting or applying it means doing so for every member.
    pub cfg_path: &'static str,
    /// The config paths a preset writes, and the only ones it reads
    /// back.
    pub members: &'static [&'static str],
    /// Write the preset's mask.
    pub apply: fn(&mut Config, Preset),
}

/// Controls (player ruling 2026-09-25). Enhanced is the default
/// config; Classic is how both originals fly.
pub const CONTROLS_PRESET: PresetGroup = PresetGroup {
    cfg_path: "controls.preset",
    members: &[
        "controls.preferences.bindings",
        "controls.preferences.mouse_sensitivity_x",
        "controls.preferences.invert_y",
        "controls.preferences.autofire",
        "controls.models.thrust",
        "controls.models.altitude",
    ],
    apply: |c, p| {
        use crate::config::{AltitudeModel, Bindings, ThrustModel};
        let (prefs, models) = (&mut c.controls.preferences, &mut c.controls.models);
        match p {
            Preset::Enhanced => {
                prefs.bindings = Bindings::Wasd;
                prefs.mouse_sensitivity_x = 1.0;
                prefs.invert_y = false;
                prefs.autofire = true;
                models.thrust = ThrustModel::Enhanced;
                models.altitude = AltitudeModel::Enhanced;
            }
            Preset::Classic => {
                prefs.bindings = Bindings::Classic;
                prefs.mouse_sensitivity_x = 0.5;
                prefs.invert_y = true;
                prefs.autofire = false;
                models.thrust = ThrustModel::Classic;
                models.altitude = AltitudeModel::Classic;
            }
        }
    },
};

/// Visuals — the `render` options that make up the look (player
/// ruling 2026-09-25). Enhanced is the default config; Classic is
/// retail's value for each, except two player rulings: Classic hides
/// the crosshair, and smooth motion is ON under both (it only removes
/// the 24 Hz stepping — the original's choppiness, not its look). The
/// display settings (vsync, fullscreen, anti-aliasing), the screens
/// (movies, stats, subtitles), the HUD and the debug overlays are not
/// members. Six members read the same under both presets: a preset
/// still enforces them.
pub const VISUALS_PRESET: PresetGroup = PresetGroup {
    cfg_path: "render.preset",
    members: &[
        "render.preference.crosshair",
        "render.preference.sky",
        "render.preference.reflections",
        "render.preference.horizon_cull",
        "render.preference.light_sources",
        "render.preference.fog_distance",
        "render.preference.rival_tags",
        "render.enhancement.smooth_motion",
        "render.enhancement.smooth_textures",
        "render.enhancement.segment_order",
        "render.enhancement.hotbar",
        "render.enhancement.fire",
        "render.enhancement.lightning",
        "render.enhancement.map_owned_buildings",
        "render.enhancement.map_marker_scale",
        "render.enhancement.map_marker_icons",
        "render.enhancement.autocontrasting_markers",
        "render.enhancement.map_extent_fog",
        "render.enhancement.map_beyond_sight_areas",
        "render.enhancement.mc2_fancy_exit",
    ],
    apply: |c, p| {
        use crate::config::{FireEffects, LightningEffects, RivalTags};
        let enhanced = p == Preset::Enhanced;
        let (pref, enh) = (&mut c.render.preference, &mut c.render.enhancement);
        pref.crosshair = enhanced;
        pref.sky = true;
        pref.reflections = true;
        pref.horizon_cull = true;
        pref.light_sources = true;
        pref.fog_distance = if enhanced { 90 } else { 20 };
        pref.rival_tags = if enhanced {
            RivalTags::On
        } else {
            RivalTags::Auto
        };
        enh.smooth_motion = true;
        enh.smooth_textures = enhanced;
        enh.segment_order = enhanced;
        enh.hotbar = enhanced;
        enh.fire = if enhanced {
            FireEffects::Enhanced
        } else {
            FireEffects::Classic
        };
        enh.lightning = if enhanced {
            LightningEffects::Enhanced
        } else {
            LightningEffects::Classic
        };
        enh.map_owned_buildings = enhanced;
        enh.map_marker_scale = 1.0;
        enh.map_marker_icons = enhanced;
        enh.autocontrasting_markers = enhanced;
        enh.map_extent_fog = enhanced;
        enh.map_beyond_sight_areas = enhanced;
        enh.mc2_fancy_exit = enhanced;
    },
};

/// Every preset group, in launcher row order.
pub const PRESET_GROUPS: [&PresetGroup; 2] = [&CONTROLS_PRESET, &VISUALS_PRESET];

/// The preset group whose registry row is `cfg_path`, if any.
pub fn preset_group(cfg_path: &str) -> Option<&'static PresetGroup> {
    PRESET_GROUPS.into_iter().find(|g| g.cfg_path == cfg_path)
}

/// The value at a dotted path of a serialized config.
fn json_at<'a>(v: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    path.split('.').fold(v, |v, seg| &v[seg])
}

/// Two serialized option values agree. Numbers compare within a
/// slider step's rounding (an f32 through JSON is not exact).
fn same_value(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-3,
        _ => a == b,
    }
}

impl PresetGroup {
    /// The preset every member matches, `None` = custom.
    pub fn current(&self, c: &Config) -> Option<Preset> {
        let have = serde_json::to_value(c).expect("config serializes");
        Preset::ALL.into_iter().find(|&p| {
            let mut t = c.clone();
            (self.apply)(&mut t, p);
            let want = serde_json::to_value(&t).expect("config serializes");
            self.members
                .iter()
                .all(|m| same_value(json_at(&have, m), json_at(&want, m)))
        })
    }

    /// Copy the members' values from `src` into `dst` (the launcher's
    /// "Custom": back to what was set before a preset was picked).
    pub fn copy_members(&self, dst: &mut Config, src: &Config) {
        let mut d = serde_json::to_value(&*dst).expect("config serializes");
        let from = serde_json::to_value(src).expect("config serializes");
        for m in self.members {
            let segs: Vec<&str> = m.split('.').collect();
            let slot = segs.iter().fold(&mut d, |v, seg| &mut v[*seg]);
            *slot = json_at(&from, m).clone();
        }
        *dst = serde_json::from_value(d).expect("members round-trip");
    }
}

macro_rules! toggle {
    ($cfg:ident => $($path:tt)*) => {
        |c: &Config| Val::Toggle { on: c.$($path)*, faithful: false }
    };
}

/// The full registry. Order = summary order (grouped by heading) =
/// menu row order within each domain tab.
/// Step `spec`'s value one notch (`forward` = next / up) — the lobby's
/// left-right rule rows: a toggle flips, a choice cycles past variants
/// that do not land (a preset's "custom"), a slider moves one `step`,
/// stops move to the neighbouring stop. False when nothing changed.
pub fn step(spec: &Spec, cfg: &mut Config, forward: bool) -> bool {
    match &spec.ctl {
        Ctl::ReadOnly => false,
        Ctl::Toggle { set, .. } => match (spec.read)(cfg) {
            Val::Toggle { on, .. } => {
                set(cfg, !on);
                true
            }
            _ => false,
        },
        Ctl::Choice { set, .. } => {
            let (cur, n) = match (spec.read)(cfg) {
                Val::Choice { cur, variants, .. } => (cur, variants.len()),
                _ => return false,
            };
            let mut next = cur;
            for _ in 1..n {
                next = if forward {
                    (next + 1) % n
                } else {
                    (next + n - 1) % n
                };
                set(cfg, next);
                if matches!((spec.read)(cfg), Val::Choice { cur, .. } if cur == next) {
                    return true;
                }
            }
            set(cfg, cur);
            false
        }
        Ctl::Slider {
            get,
            set,
            min,
            max,
            step,
        } => {
            let v = get(cfg);
            let to = (v + if forward { *step } else { -*step }).clamp(*min, *max);
            set(cfg, to);
            to != v
        }
        Ctl::Stops { get, set, stops } => {
            let v = get(cfg);
            let i = stops
                .iter()
                .enumerate()
                .min_by_key(|(_, (s, _))| s.abs_diff(v))
                .map_or(0, |(i, _)| i);
            let j = if forward {
                (i + 1).min(stops.len().saturating_sub(1))
            } else {
                i.saturating_sub(1)
            };
            match stops.get(j) {
                Some(&(to, _)) if to != v => {
                    set(cfg, to);
                    true
                }
                _ => false,
            }
        }
    }
}

pub fn registry() -> Vec<Spec> {
    use Class::*;
    use Domain::*;
    vec![
        // ---- sim · parameters -------------------------------------------
        Spec {
            domain: Sim,
            group: "sim · parameters",
            label: "entity_pool_size",
            // Enhancement, not Cheat, since the 20000 default landed
            // (player-ruled 2026-09-10): a bigger pool grants no
            // impossible power, it only stops the world shedding
            // things — but it IS a deviation from retail, so a stock
            // run rolls up ENHANCED, never FAITHFUL.
            class: Enhancement,
            key: None,
            cli: Some("--pool-slots N"),
            cfg_path: "sim.parameters.entity_pool_size",
            read: |c| Val::Override {
                val: c.sim.parameters.entity_pool_size.map(|n| n.to_string()),
                faithful: "per-game default 1000",
            },
            desc: "Sets how many things the world can hold at once. Retail Magic Carpet used 1000. \
                   The default is 20000. Can only be changed on the command line or in the config \
                   file.",
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
            desc: "Sets how close to you creatures stay active, in tiles. Sleeping creatures skip \
                   some behaviour, even dying when out of health. Retail uses 24. 0 keeps \
                   everything awake. Command line or config file only.",
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
            desc: "Sets how fast the game runs. Everything moves, fights and recovers at this \
                   pace. Resets to normal at the start of every level.",
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
                    "Half speed",
                    "The normal pace",
                    "Four times faster, retail's Fast",
                    "Retail's top speed: 16x in Magic Carpet, 8x in Magic Carpet 2",
                ],
            },
        },
        // ---- render · preset --------------------------------------------
        Spec {
            domain: Render,
            group: "render · preset",
            label: "preset",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "render.preset",
            read: |c| Val::Choice {
                cur: match VISUALS_PRESET.current(c) {
                    Some(Preset::Enhanced) => 0,
                    Some(Preset::Classic) => 1,
                    None => 2,
                },
                faithful: 1,
                variants: &["enhanced", "classic", "custom"],
            },
            desc: "Sets all the visual options below at once. Custom is a placeholder if some are \
                   changed by hand.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    if let Some(&p) = Preset::ALL.get(i) {
                        (VISUALS_PRESET.apply)(c, p);
                    }
                },
                descs: &[
                    "All visual improvements on (default)",
                    "The look of the original games, but with smooth motion",
                    "The options below match neither preset",
                ],
            },
        },
        // ---- render · preference ----------------------------------------
        Spec {
            domain: Render,
            group: "render · preference",
            label: "crosshair",
            class: Preference,
            key: None,
            cli: Some("--crosshair"),
            cfg_path: "render.preference.crosshair",
            // Faithful = ON: both retails steered by a live on-screen
            // cursor; a bare aim cross is the closest analog.
            read: |c| Val::Toggle {
                on: c.render.preference.crosshair,
                faithful: true,
            },
            desc: "Shows a cross where spells will aim at. With enhanced flight controls the \
                   carpet also steers toward it.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.crosshair = v,
                descs: ["No aim crosshair", "Shows the aim crosshair"],
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
            desc: "Draws the cloudy sky, unless the level is in a cave.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.sky = v,
                descs: ["A plain sky colour", "The cloudy sky"],
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
            desc: "Water reflecting the landscape, wobbling with the waves.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.reflections = v,
                descs: ["Plain water", "The landscape mirrored in the water"],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "horizon_cull",
            class: Preference,
            key: None,
            cli: Some("--no-horizon-cull"),
            cfg_path: "render.preference.horizon_cull",
            // Faithful = ON: retail drew a camera-relative grid cut
            // at 20 tiles; the whole-map draw is the port's excess.
            read: |c| Val::Toggle {
                on: c.render.preference.horizon_cull,
                faithful: true,
            },
            desc: "Skips drawing land that is hidden in the far fog anyway. This reduces the \
                   number of distant polygons, and should only be disabled if you know what you're \
                   doing.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.horizon_cull = v,
                descs: [
                    "Draws the whole map every frame",
                    "Draws only the land you can see (default)",
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
            desc: "Fireballs, explosions and fires light up the ground around them on night and \
                   cave levels.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.light_sources = v,
                descs: ["No light from fire", "Fire lights up the night"],
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
            desc: "Sets how far you can see, in tiles. Retail uses 20. Farther views look better, \
                   but reveal creatures that retail kept hidden in the fog. Shorter view also runs \
                   faster.",
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
            desc: "Synchronizes the frame rate with the display refresh. Turn it off to see the \
                   highest frame rate. Only disable this if you have trouble with visual tearing \
                   or are benchmarking the performance.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.vsync = v,
                descs: [
                    "Unlimited frame rate",
                    "Vsynced framerate, tear-free frames (default)",
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
            desc: "Fills the whole screen with no window frame. On widescreen displays the side \
                   panels move to the edges and you see more to the sides.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.fullscreen = v,
                descs: ["A window you can resize", "Fills the whole screen"],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "window_size",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "render.preference.window_size",
            read: |c| Val::Choice {
                cur: c.render.preference.window_size.index(),
                faithful: crate::config::WindowSize::W1280x960.index(),
                variants: &[
                    "640x480",
                    "1280x960",
                    "1600x1200",
                    "1920x1440",
                    "1920x1080",
                    "2560x1440",
                    "3840x2160",
                ],
            },
            desc: "The window size when not in fullscreen mode. You can still resize the window by \
                   hand.",
            ctl: Ctl::Choice {
                set: |c, i| c.render.preference.window_size = crate::config::WindowSize::ALL[i],
                descs: &[
                    "Retail's hires size",
                    "2x",
                    "2.5x",
                    "3x",
                    "Full HD",
                    "QHD widescreen",
                    "4K widescreen",
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
            desc: "Smooth jagged edges in the 3D view. The higher settings smooth everything but \
                   cost performance.",
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
                    "No AA",
                    "Cheap AA, smooths landscape edges only, needs a restart",
                    "1.5x supersampling, smooths everything, at a moderate cost",
                    "2x supersampling, smoothest, heavy performance cost",
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
            desc: "Play the movies and cutscenes: the intro, the Magic Carpet 2 intermission \
                   cutscenes between levels and the ending. Any key skips a movie.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.movies = v,
                descs: ["Skip all movies", "Play the movies"],
            },
        },
        Spec {
            domain: Render,
            group: "render · preference",
            label: "stats_screen",
            class: Preference,
            key: None,
            cli: Some("--no-stats"),
            cfg_path: "render.preference.stats_screen",
            // Retail always shows them, so ON is the faithful reading.
            read: |c| Val::Toggle {
                on: c.render.preference.stats_screen,
                faithful: c.render.preference.stats_screen,
            },
            desc: "Show result stats after each level.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.stats_screen = v,
                descs: [
                    "No stats. They are still calculated and can be viewed in MC2",
                    "Show stats after each level",
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
            desc: "Show the movie narration as subtitles.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.movie_subtitles = v,
                descs: ["No movie subtitles", "Narration shown as text (default)"],
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
            key: None,
            cli: Some("--smooth-shading"),
            cfg_path: "render.enhancement.smooth_shading",
            read: toggle!(c => render.enhancement.smooth_shading),
            desc: "Continuous landscape transition shading. Disabling this gives the landscape a \
                   faceted look and a small performance bump.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.smooth_shading = v,
                descs: ["Faceted shading", "Smooth shading"],
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
            desc: "Make the HUD panel transparent. Solid panels tend to be easier to view.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.render.enhancement.hud_transparency = if v {
                        crate::config::HudTransparency::On
                    } else {
                        crate::config::HudTransparency::Off
                    }
                },
                descs: ["Solid panels and radar (default)", "Transparent panels"],
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
            desc: "Show each rival wizard's name and health above them in their colour, Magic \
                   Carpet 2 style.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.render.preference.rival_tags = match i {
                        1 => crate::config::RivalTags::On,
                        2 => crate::config::RivalTags::Off,
                        _ => crate::config::RivalTags::Auto,
                    }
                },
                descs: &[
                    "Only show tags in Magic Carpet 2 (default)",
                    "Show tags in both games",
                    "No rival tags",
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
            desc: "Make the view and everything in it move smoothly at any frame rate, \
                   interpolating sim ticks. Does not affect the gameplay as such.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.smooth_motion = v,
                descs: [
                    "Movement in lockstep with sim",
                    "Interpolated smooth movement (default)",
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
            desc: "Enhanced fire effects. Draws procedurally generated flowing flames, smoke and \
                   shockwaves. Requires smooth motion.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.render.enhancement.fire = match i {
                        1 => crate::config::FireEffects::Enhanced,
                        _ => crate::config::FireEffects::Classic,
                    }
                },
                descs: &[
                    "The original fire and explosions",
                    "Enhanced flames, smoke and shockwaves (default)",
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
            desc: "Enhanced lightning effects. Draws branching bolts that dance while you hold the \
                   spell. Requires smooth motion.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.render.enhancement.lightning = match i {
                        1 => crate::config::LightningEffects::Enhanced,
                        _ => crate::config::LightningEffects::Classic,
                    }
                },
                descs: &[
                    "The original lightning projectiles",
                    "Enhanced branching bolts (default)",
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
            desc: "Mark houses on the map as Magic Carpet 2 does: pink when free, flashing in the \
                   owner's colour when claimed.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_owned_buildings = v,
                descs: [
                    "Houses unmarked, just like Magic Carpet",
                    "Houses marked by owner",
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
            desc: "Tweaks the size of the markers on the map and radar. At any size but 1x, \
                   markers keep their size as the radar zooms.",
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
            desc: "Shows small icons instead of dots on the map for several static objects: spell \
                   jars, shrines, statues, teleporters and dead wizard graves.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_marker_icons = v,
                descs: ["Plain dot markers", "Icon markers for important things"],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "autocontrasting_markers",
            // Preference, not Enhancement: a map-legibility aid with no
            // bearing on the run (like smooth shading / HUD opacity),
            // which is what lets it ship ON — the stock run must still
            // roll up Faithful (`stock_run_is_faithful`).
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.autocontrasting_markers",
            read: toggle!(c => render.enhancement.autocontrasting_markers),
            desc: "Highlights important map markers with a thin contrasting outline, so they stand \
                   out on any terrain.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.autocontrasting_markers = v,
                descs: ["Plain markers", "Outlined markers"],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "smooth_textures",
            class: Enhancement,
            key: None,
            cli: Some("--smooth-textures"),
            cfg_path: "render.enhancement.smooth_textures",
            read: toggle!(c => render.enhancement.smooth_textures),
            desc: "Smooths the pixels of the landscape and creatures. Disable to keep the original \
                   pixelated textures.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.smooth_textures = v,
                descs: ["Chunky pixels", "Smoothed textures"],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "segment_order",
            class: Enhancement,
            key: None,
            cli: Some("--segment-order"),
            cfg_path: "render.enhancement.segment_order",
            read: toggle!(c => render.enhancement.segment_order),
            desc: "Prevents long segmented creatures such as worms, dragons and hydras from \
                   flickering when viewed from the side.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.segment_order = v,
                descs: ["Bodies may flicker", "Forced segment order"],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "hotbar",
            class: Enhancement,
            key: None,
            cli: Some("--hotbar"),
            cfg_path: "render.enhancement.hotbar",
            read: toggle!(c => render.enhancement.hotbar),
            desc: "Display a spell bar with the quick keys 1..0. In the spell selector, \
                   double-click a spell to add it, drag to reorder, or drag it off the bar to \
                   remove it.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.hotbar = v,
                descs: [
                    "No bar, the quick keys still work",
                    "The spell bar is shown",
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
            desc: "The world wraps around the edges. This prevents the map from looping around and \
                   fades beyond the map edges.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_extent_fog = v,
                descs: [
                    "Things repeat on the map",
                    "The map has soft edges centered on the player",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "map_beyond_sight_areas",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.map_beyond_sight_areas",
            read: toggle!(c => render.enhancement.map_beyond_sight_areas),
            desc: "Enables the Beyond Sight spell to also show traps and some event places as \
                   faint colored ripples on the map: red for traps, cyan for the rest.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_beyond_sight_areas = v,
                descs: [
                    "Beyond Sight original functionality",
                    "Beyond Sight enhanced capabilities",
                ],
            },
        },
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "mc2_fancy_exit",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.mc2_fancy_exit",
            read: toggle!(c => render.enhancement.mc2_fancy_exit),
            desc: "Leaving a Magic Carpet 2 level plays an enhanced exit animation: a third-person \
                   view of the carpet flying into the portal.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.mc2_fancy_exit = v,
                descs: [
                    "Original fly into the portal",
                    "Enhanced level exit animation",
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
            desc: "Display which spell each jar holds, on the map and above the jar.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.expose_jar_spells = v,
                descs: ["Plain jars", "Every jar shows its spell"],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "health_bars",
            class: Debug,
            key: None,
            cli: Some("--health-bars"),
            cfg_path: "render.debug.health_bars",
            read: toggle!(c => render.debug.health_bars),
            desc: "Display a health bar above every creature and building that can be destroyed.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.health_bars = v,
                descs: ["No health bars", "Health bars on everything"],
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
            desc: "Marks the target each hand's spell would lock onto if it fired right now: + for \
                   the left hand, x for the right.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.autoaim_hints = v,
                descs: ["No markers", "Target markers for both hands"],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "map_trigger_areas",
            class: Debug,
            key: None,
            cli: Some("--map-triggers"),
            cfg_path: "render.debug.map_trigger_areas",
            read: toggle!(c => render.debug.map_trigger_areas),
            desc: "Outline the areas that set off traps and events as coloured circles on the map.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.map_trigger_areas = v,
                descs: ["No outlines", "Event places shown on the map"],
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
            desc: "After respawn (or level start) you cannot be hurt for a short while. This \
                   displays a countdown of this invincibility.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.grace_meter = v,
                descs: ["No bar", "Shows the safe time left"],
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
            desc: "Display the frame rate in the bottom-right corner. To properly benchmark \
                   performance, you need to disable vsync as well.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.fps = v,
                descs: ["No display", "Frames per second and time per frame"],
            },
        },
        Spec {
            domain: Render,
            group: "render · debug",
            label: "coords",
            class: Debug,
            key: None,
            cli: Some("--coords"),
            cfg_path: "render.debug.coords",
            read: toggle!(c => render.debug.coords),
            desc: "Display the internal map-native coordinates in the bottom-left corner.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.coords = v,
                descs: ["No display", "Your position and height displayed"],
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
            desc: "Display the current entity table status in the bottom-left corner.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.debug.entities = v,
                descs: ["No display", "Show how full the world is"],
            },
        },
        // ---- controls · preset ------------------------------------------
        Spec {
            domain: Controls,
            group: "controls · preset",
            label: "preset",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preset",
            read: |c| Val::Choice {
                cur: match CONTROLS_PRESET.current(c) {
                    Some(Preset::Enhanced) => 0,
                    Some(Preset::Classic) => 1,
                    None => 2,
                },
                faithful: 1,
                variants: &["enhanced", "classic", "custom"],
            },
            desc: "Sets all the control options below at once. Custom is a placeholder if some are \
                   changed by hand.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    if let Some(&p) = Preset::ALL.get(i) {
                        (CONTROLS_PRESET.apply)(c, p);
                    }
                },
                descs: &[
                    "WASD, mouse look, hold to fly, float keys (default)",
                    "Retail controls: arrow keys, flight-stick mouse",
                    "The options below match neither preset",
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
            desc: "Sets which keys move the carpet.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.controls.preferences.bindings = if i == 0 {
                        crate::config::Bindings::Classic
                    } else {
                        crate::config::Bindings::Wasd
                    }
                },
                descs: &[
                    "Retail keys: arrows speed up, slow down, strafe",
                    "W/S fly forward and back, A/D strafe",
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
            desc: "Sets how strongly the mouse turns and aims the carpet.",
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
            desc: "Sets the share of the mouse sensitivity used for turning left and right. The \
                   Enhanced controls preset uses 100%, Classic 50%.",
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
            desc: "Sets the share of the mouse sensitivity used for looking up and down.",
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
            desc: "Pushing the mouse forward points the carpet down, like a flight stick. Retail \
                   uses this in both games.",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.invert_y = v,
                descs: [
                    "Mouse forward climbs (default)",
                    "Mouse forward dives, flight-stick style",
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
            desc: "Magic Carpet 2's Flight Assistance: an idle mouse recenters the steering, so \
                   the carpet flies straight. Classic thrust only, retail Magic Carpet lacked it.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.controls.preferences.fly_assistant = if v {
                        crate::config::FlyAssistant::On
                    } else {
                        crate::config::FlyAssistant::Off
                    }
                },
                descs: [
                    "You trim the steering yourself",
                    "Idle mouse recenters the steering",
                ],
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "autofire",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.autofire",
            read: |c| Val::Toggle {
                on: c.controls.preferences.autofire,
                faithful: false,
            },
            desc: "Hold a fire button to keep casting single-shot spells such as Fireball or \
                   Meteor, about 4 times a second. Spells that already repeat are unchanged.",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.autofire = v,
                descs: [
                    "One click, one cast",
                    "Holding the button keeps casting (default)",
                ],
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "disable_suicide_key",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.disable_suicide_key",
            read: |c| Val::Toggle {
                on: c.controls.preferences.disable_suicide_key,
                faithful: false,
            },
            desc: "Disables the Shift+K suicide key, which is easy to hit by accident next to \
                   Shift+L.",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.disable_suicide_key = v,
                descs: ["Shift+K kills you (default)", "Shift+K does nothing"],
            },
        },
        Spec {
            domain: Controls,
            group: "controls · preferences",
            label: "disable_barrel_roll",
            class: Preference,
            key: None,
            cli: None,
            cfg_path: "controls.preferences.disable_barrel_roll",
            read: |c| Val::Toggle {
                on: c.controls.preferences.disable_barrel_roll,
                faithful: false,
            },
            desc: "Disables the Magic Carpet 2 barrel roll, done by pressing both strafe keys at \
                   once.",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.disable_barrel_roll = v,
                descs: [
                    "Both strafe keys roll the carpet (default)",
                    "Both strafe keys just strafe",
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
            desc: "Sets how the carpet steers and speeds up. Classic is the retail model of both \
                   games, enhanced is modern hold-to-fly.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.controls.models.thrust = if i == 0 {
                        crate::config::ThrustModel::Classic
                    } else {
                        crate::config::ThrustModel::Enhanced
                    }
                },
                descs: &[
                    "Retail: mouse steers like a stick, speed persists",
                    "Mouse look, hold a key to fly, release to slow",
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
            desc: "Sets how you control your height. Classic only follows the ground, as in \
                   retail. Enhanced adds float keys.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.controls.models.altitude = if i == 0 {
                        crate::config::AltitudeModel::Classic
                    } else {
                        crate::config::AltitudeModel::Enhanced
                    }
                },
                descs: &[
                    "Retail: the carpet follows the ground by itself",
                    "Also float up and down with E/Q, up to the highest terrain",
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
            desc: "Toggles sound effects, like retail's F1 key.",
            ctl: Ctl::Toggle {
                set: |c, v| c.audio.sound = v,
                descs: ["No sound effects", "Sound effects play"],
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
            desc: "Toggles music, like retail's F2 key.",
            ctl: Ctl::Toggle {
                set: |c, v| c.audio.music = v,
                descs: ["No music", "The level music plays"],
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
            desc: "Sets the volume of the sound effects. The Magic Carpet 2 narrator uses it too.",
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
            desc: "Sets the volume of the music.",
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
                    crate::config::MusicArrangement::Awe32 => 3,
                },
                faithful: 0,
                variants: &["auto", "fm", "gm", "awe32"],
            },
            desc: "Sets which music arrangement plays. Retail shipped one per sound card family. \
                   AWE32 is the one the music was tuned for.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    c.audio.arrangement = match i {
                        0 => crate::config::MusicArrangement::Auto,
                        1 => crate::config::MusicArrangement::Fm,
                        2 => crate::config::MusicArrangement::Gm,
                        _ => crate::config::MusicArrangement::Awe32,
                    }
                },
                descs: &[
                    "Best available: AWE32, GM, then FM",
                    "AdLib FM (OPL3)",
                    "General MIDI",
                    "Sound Blaster AWE32",
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
            desc: "Plays the narrator's spoken objectives in Magic Carpet 2. The narrator also \
                   follows the sound toggle and effects volume.",
            ctl: Ctl::Toggle {
                set: |c, v| c.audio.speech = v,
                descs: ["Objectives arrive silently", "The narrator speaks"],
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
            desc: "Shows the narrator's words as text at the top of the screen.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.audio.subtitles = if v {
                        crate::config::Subtitles::On
                    } else {
                        crate::config::Subtitles::Off
                    }
                },
                descs: ["No narration text", "Narration shown as text (default)"],
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
            desc: "Sets how you pick spells. Only the menu changes, not the spells, and you can \
                   switch at any time.",
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
                    "Each game's own: map spellbook or Ctrl menu",
                    "Always the map spellbook (Magic Carpet only)",
                    "Always the Magic Carpet 2 Ctrl menu",
                    "Both at once (Magic Carpet only)",
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
            desc: "The mouse wheel cycles the left hand's spells, Shift+wheel the right hand's. \
                   Same rotation as retail's Shift/Alt+click, which still works.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.enhancement.wheel_spells = v,
                descs: [
                    "The wheel does nothing",
                    "The wheel cycles spells (default)",
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
            label: "jar_ground_snap",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.jar_ground_snap",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.jar_ground_snap.on(),
                faithful: false,
            },
            desc: "Retail leaves spell jars in place when the land changes shape, so they end up \
                   buried or floating. Patched, jars follow the ground.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.jar_ground_snap = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Jars can be buried or float",
                    "Jars stay on the ground (default)",
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
            desc: "In retail Magic Carpet, settled mana balls can hang in mid-air or get buried \
                   when the land changes. Patched, they rest on the ground.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.ball_ground_track = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Balls can hang in the air",
                    "Balls rest on the ground (default)",
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
            desc: "Retail only rolls mana balls near you, so mana runs away downhill as you \
                   approach. Patched, balls roll to rest everywhere.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.map_wide_ball_rolling = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Balls roll only near you",
                    "Balls roll to rest everywhere (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "ball_owner_recolor",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.ball_owner_recolor",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.ball_owner_recolor.on(),
                faithful: false,
            },
            desc: "Retail recolours far-off mana balls and Magic Carpet 2 mana worms only when you \
                   come near. Patched, they always show the owner's colour.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.ball_owner_recolor = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Far mana keeps its old colour",
                    "Mana always shows its owner (default)",
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
            desc: "In retail Magic Carpet, possessed villages burn down because their people spawn \
                   stuck on the roofs. Patched, houses keep their true footprint.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.possessed_footprint = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Possessed villages destroy themselves",
                    "Possessed villages keep working (default)",
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
            desc: "In retail Magic Carpet 2, a top-level castle losing a level gains mana capacity \
                   and spills nothing, a maths overflow. Patched, it always loses 10%.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_downgrade_overflow = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A top castle gains from it",
                    "A castle always loses 10% (default)",
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
            desc: "Makes Magic Carpet 2 mana dwellers visible only up close. Their stealth relied \
                   on retail's short view, so the long view makes them easy targets.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_dweller_invisibility =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["Visible from afar", "They appear only up close (default)"],
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
            desc: "Retail Magic Carpet plays its second level-win movie with the first one's music \
                   by mistake. Patched, it plays its own music.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.win2_movie_score = crate::config::PatchArm::from_on(v)
                },
                descs: ["The first movie's music", "Its own music (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_leviathan_high_lunge",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_leviathan_high_lunge",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_leviathan_high_lunge.on(),
                faithful: false,
            },
            desc: "Retail's Magic Carpet 2 leviathan aims ahead of where you face, missing a \
                   hovering carpet. Patched, it leads by your real speed and leaps higher with \
                   enhanced altitude.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_leviathan_high_lunge =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Hovering or flying high is safe",
                    "Aims better, leaps higher (default)",
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
            desc: "In retail Magic Carpet, casting Castle into a wall corner can build a castle \
                   where none is allowed. Patched, the placement check always runs.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.castle_latch_bug = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "The corner trick works",
                    "Castles only where allowed (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "no_spell_loss",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.no_spell_loss",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.no_spell_loss.on(),
                faithful: false,
            },
            desc: "In retail, a held spell can rarely vanish for good, in a crowded level or near \
                   a volcano. Patched, spells are permanent; only the Magic Carpet 2 wraith can \
                   steal one.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.patches.no_spell_loss = crate::config::PatchArm::from_on(v),
                descs: [
                    "Spells can rarely be lost",
                    "Your spells are never lost (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc1_fix_dragon_tail",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc1_fix_dragon_tail",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc1_fix_dragon_tail.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet, a far-off dragon's tail bunches up on its head and \
                   snaps back as you come near. Patched, tails follow at any range.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_fix_dragon_tail = crate::config::PatchArm::from_on(v)
                },
                descs: ["Far tails bunch up", "Tails always follow (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc1_pack_speed_restore",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc1_pack_speed_restore",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc1_pack_speed_restore.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet, creatures in packs keep getting faster until they cross \
                   the map in seconds. Patched, a creature leaving a pack returns to its own \
                   speed.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_pack_speed_restore = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Creatures keep speeding up",
                    "Creatures keep their own speed (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_phantom_castle",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_phantom_castle",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_phantom_castle.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet 2, a dead wizard lying in a village raises stray castle \
                   walls at map position (0,0). Patched, only castles do.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_phantom_castle = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Stray castle walls appear",
                    "No stray castle walls (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "dual_wield_muzzle",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.dual_wield_muzzle",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.dual_wield_muzzle.on(),
                faithful: false,
            },
            desc: "Casting with both hands in retail, most spells leave the right hand, whichever \
                   hand cast them. Patched, each spell leaves its own hand.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.dual_wield_muzzle = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Spells may leave the wrong hand",
                    "Each hand fires its own spells (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "one_castle_per_wizard",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.one_castle_per_wizard",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.one_castle_per_wizard.on(),
                faithful: false,
            },
            desc: "In retail, two quick Castle casts can build two castles, leaving one useless. \
                   Patched, a wizard can only ever have one castle.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.one_castle_per_wizard = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Two castles possible",
                    "Never more than one castle (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_wyvern_alliance_brain",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_wyvern_alliance_brain",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_wyvern_alliance_brain.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet 2, a wyvern charmed mid-fight can fly in a straight line \
                   forever. Patched, it calms down like other creatures.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_wyvern_alliance_brain =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["It flies off forever", "It calms down (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_flyers_clear_terrain",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_flyers_clear_terrain",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_flyers_clear_terrain.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet 2, mana worms and moon dwellers get stuck on walls far \
                   below them. Patched, they fly over. Other creatures are unchanged.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_flyers_clear_terrain =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["They can get stuck", "They fly over low walls (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "skeletons_convert_traders",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.skeletons_convert_traders",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.skeletons_convert_traders.on(),
                faithful: false,
            },
            desc: "Retail skeletons turn villagers into skeletons but never traders. Patched, \
                   traders can be turned too. Castle guards and animals stay immune.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.skeletons_convert_traders =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["Traders are safe", "Traders can be turned too (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_fools_mana_skip",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_fools_mana_skip",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_fools_mana_skip.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet 2, a rival that sees through Fool's Mana stops \
                   collecting mana altogether. Patched, it skips the decoy and keeps collecting.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_fools_mana_skip = crate::config::PatchArm::from_on(v)
                },
                descs: ["Rivals give up", "Rivals keep collecting (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_orphan_balloon_reap",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_orphan_balloon_reap",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_orphan_balloon_reap.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet 2, a dead balloon whose castle is gone floats forever \
                   and cannot be destroyed. Patched, it turns into mana.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_orphan_balloon_reap = crate::config::PatchArm::from_on(v)
                },
                descs: ["It floats forever", "It turns into mana (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_house_flag_color",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_house_flag_color",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_house_flag_color.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet 2, four of the eight wizards fly a different colour on \
                   their houses than on their castle. Patched, houses match the castle.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_house_flag_color = crate::config::PatchArm::from_on(v)
                },
                descs: ["Some flags mismatch", "Flags match the castle (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_immediate_reap",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_immediate_reap",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_immediate_reap.on(),
                faithful: false,
            },
            desc: "Retail Magic Carpet 2 removes dead things at the start of the next frame, so \
                   they linger one frame. Patched, they go at once. No visible difference.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_immediate_reap = crate::config::PatchArm::from_on(v)
                },
                descs: ["They linger a frame", "They go at once (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc1_recycle_victim_revalidate",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc1_recycle_victim_revalidate",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc1_recycle_victim_revalidate.on(),
                faithful: false,
            },
            desc: "When a retail Magic Carpet level is full, it can delete a castle's helper by \
                   mistake, leaving the castle stuck forever. Patched, only spare things go.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_recycle_victim_revalidate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["Wrong things can go", "Only spare things go (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc1_castle_transform_watchdog",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc1_castle_transform_watchdog",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc1_castle_transform_watchdog.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet, a growing castle whose painter or ground-leveller is \
                   destroyed waits forever. Patched, it settles at once.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_castle_transform_watchdog =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["It can stay stuck", "It settles at once (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc1_crushed_site_collapse",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc1_crushed_site_collapse",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc1_crushed_site_collapse.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet, a half-built house crushed by a castle churns the \
                   ground forever. Patched, it collapses.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_crushed_site_collapse =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["It churns the ground", "It collapses (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc2_building_pad_saturate",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_building_pad_saturate",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_building_pad_saturate.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet 2, a building on very high ground sinks into deep pits. \
                   Patched, it sits flat at the height limit.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_building_pad_saturate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["It sinks into pits", "It sits flat (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc1_building_pad_saturate",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc1_building_pad_saturate",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc1_building_pad_saturate.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet, buildings on very high ground sink into deep pits. \
                   Patched, houses sit flat at the height limit and castles lower.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_building_pad_saturate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["They sink into pits", "They sit flat or lower (default)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "mc1_segment_chain_revalidate",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc1_segment_chain_revalidate",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc1_segment_chain_revalidate.on(),
                faithful: false,
            },
            desc: "In retail Magic Carpet, a sea monster killed twice can leave a stuck mana ball \
                   that swallows other mana. Patched, it leaves none.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_segment_chain_revalidate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A stuck mana ball can appear",
                    "No stuck mana ball (default)",
                ],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · patches",
            label: "volcano_register_revalidate",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.volcano_register_revalidate",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.volcano_register_revalidate.on(),
                faithful: false,
            },
            desc: "In retail, a new volcano can destroy random mana, a spell, or a Magic Carpet \
                   castle, which can freeze the game. Patched, it only touches the old volcano.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.volcano_register_revalidate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: ["Things can break", "Only volcanoes are affected (default)"],
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
            desc: "Gives you every spell and endless mana while on. Turning it off takes back only \
                   what the cheat gave, unlike retail's cheat.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.dev_spells = v,
                descs: [
                    "Normal spells and mana",
                    "Every spell, endless mana (cheat)",
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
            desc: "Nothing can hurt or kill you.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.invincible = v,
                descs: ["You can die", "Nothing can kill you (cheat)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · cheat",
            label: "ghost",
            class: Cheat,
            key: Some("J"),
            cli: Some("--ghost"),
            cfg_path: "gameplay.cheat.ghost",
            read: toggle!(c => gameplay.cheat.ghost),
            desc: "Nobody can see you, even when you cast, and walls do not stop you. You can \
                   still be hurt.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.ghost = v,
                descs: ["Seen and blocked", "Unseen, walls don't stop you (cheat)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · cheat",
            label: "inert",
            class: Cheat,
            key: Some("B"),
            cli: Some("--inert"),
            cfg_path: "gameplay.cheat.inert",
            read: toggle!(c => gameplay.cheat.inert),
            desc: "Flying over things sets nothing off: no pickups, teleporters, traps or \
                   switches.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.inert = v,
                descs: ["Things react", "Nothing you fly over reacts (cheat)"],
            },
        },
        Spec {
            domain: Gameplay,
            group: "gameplay · cheat",
            label: "weightless",
            class: Cheat,
            key: Some("N"),
            cli: Some("--weightless"),
            cfg_path: "gameplay.cheat.weightless",
            read: toggle!(c => gameplay.cheat.weightless),
            desc: "The float keys can take you up to just above the level's highest terrain, \
                   instead of a set height over the ground.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.weightless = v,
                descs: [
                    "The normal height limit",
                    "Rise to the top of the level (cheat)",
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
            desc: "Starts each Magic Carpet level with the spells a careful player could have \
                   collected by then.",
            ctl: Ctl::Toggle {
                set: |c, v| c.dev.plausible_spellbook = v,
                descs: [
                    "Only the spells the level gives you",
                    "The spells you would have by now",
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
    fn stock_run_is_enhanced_only_by_the_pool_and_the_presets() {
        // A stock run is ENHANCED, with no cheats, and every
        // enhancement it carries is either the 20000-slot entity pool
        // (player-ruled 2026-09-10; retail 1000) or a member of an
        // Enhanced preset (player-ruled 2026-09-25: the defaults ARE
        // the Enhanced presets). The retail pool plus the Classic
        // presets rolls up FAITHFUL.
        let (verdict, enh, modi, patches) = rollup(&Config::default());
        assert_eq!(modi, 0, "no cheats/instruments on by default");
        assert!(enh >= 1);
        assert_eq!(verdict, Fidelity::Enhanced);
        for spec in registry() {
            let deviates = (spec.read)(&Config::default()).deviates();
            if deviates && spec.class.fidelity() == Fidelity::Enhanced {
                assert!(
                    spec.cfg_path == "sim.parameters.entity_pool_size"
                        || PRESET_GROUPS
                            .iter()
                            .any(|g| g.members.contains(&spec.cfg_path)),
                    "{}: a stock enhancement outside the presets",
                    spec.cfg_path
                );
            }
        }
        let mut retail_pool = Config::default();
        retail_pool.sim.parameters.entity_pool_size = None;
        for g in PRESET_GROUPS {
            (g.apply)(&mut retail_pool, Preset::Classic);
        }
        let (verdict, enh, _, _) = rollup(&retail_pool);
        assert_eq!(
            (verdict, enh),
            (Fidelity::Faithful, 0),
            "retail pool + classic presets = faithful"
        );
        // The default-on retail patches count apart and never flip
        // the verdict (castle_recast_cost, the one retail-default
        // patch, was retired 2026-09-07).
        // castle_death_mana/balloons retired 2026-08-12 — the mc1l0
        // corpus proved the scatter + fleet cull are retail law
        // (sub_470E0's wrapper), so they are unconditional now.
        // mc2_magic_mine retired 2026-09-04 (session 97) on a player
        // ruling — the mine's trigger is retail's, so the whole
        // detonation column is unconditional too (DEVIATIONS.md).
        // ball_owner_recolor added 2026-09-06 (presentation-only),
        // no_spell_loss + mc1_fix_dragon_tail 2026-09-07,
        // mc2_phantom_castle 2026-09-10, dual_wield_muzzle and
        // one_castle_per_wizard 2026-09-15, mc2_wyvern_alliance_brain
        // and mc2_orphan_balloon_reap 2026-09-16, mc2_house_flag_color
        // 2026-09-17, mc2_immediate_reap 2026-09-18,
        // mc1_recycle_victim_revalidate + mc1_castle_transform_watchdog
        // 2026-09-18 (round 151), mc1_crushed_site_collapse and
        // mc2_building_pad_saturate 2026-09-19 (round 157), and
        // mc1_building_pad_saturate the same round (w157g),
        // mc1_segment_chain_revalidate 2026-09-19 (round 158, w158a), and
        // volcano_register_revalidate the same round (w158b; renamed
        // from mc1_volcano_register_revalidate when w158e gave it the
        // MC2 twin). mc2_troglodyte_sprite_crop 2026-09-30
        // (presentation-only; retired 2026-10-06 into a bake-time
        // asset correction), mc2_leviathan_high_lunge the same day
        // (a balance deviation riding the class for its pin), and
        // mc2_flyers_clear_terrain 2026-10-03,
        // mc1_pack_speed_restore 2026-10-10 (the pack-speed ratchet), and
        // skeletons_convert_traders 2026-10-05, and
        // mc2_fools_mana_skip 2026-11-22.
        assert_eq!(patches, 30, "all thirty patches ship on");
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
    fn presets_apply_and_read_back() {
        for g in PRESET_GROUPS {
            let spec = registry().into_iter().find(|s| s.cfg_path == g.cfg_path);
            assert!(spec.is_some(), "{}: a registry row", g.cfg_path);
            for p in Preset::ALL {
                let mut c = Config::default();
                (g.apply)(&mut c, p);
                assert_eq!(g.current(&c), Some(p), "{} {p:?}", g.cfg_path);
            }
            // Every member is a real option the preset actually moves.
            let (mut e, mut k) = (Config::default(), Config::default());
            (g.apply)(&mut e, Preset::Enhanced);
            (g.apply)(&mut k, Preset::Classic);
            let (e, k) = (
                serde_json::to_value(&e).unwrap(),
                serde_json::to_value(&k).unwrap(),
            );
            for m in g.members {
                assert!(!json_at(&e, m).is_null(), "{m}: not a config path");
            }
            assert!(
                g.members
                    .iter()
                    .any(|m| !same_value(json_at(&e, m), json_at(&k, m))),
                "{}: the presets differ somewhere",
                g.cfg_path
            );
        }
    }

    #[test]
    fn the_defaults_are_the_enhanced_presets() {
        for g in PRESET_GROUPS {
            assert_eq!(
                g.current(&Config::default()),
                Some(Preset::Enhanced),
                "{}",
                g.cfg_path
            );
        }
    }

    #[test]
    fn a_hand_tuned_member_reads_custom_and_copies_back() {
        let mut tuned = Config::default();
        tuned.controls.preferences.mouse_sensitivity_x = 0.8;
        assert_eq!(CONTROLS_PRESET.current(&tuned), None);
        let mut c = tuned.clone();
        (CONTROLS_PRESET.apply)(&mut c, Preset::Classic);
        assert_eq!(CONTROLS_PRESET.current(&c), Some(Preset::Classic));
        CONTROLS_PRESET.copy_members(&mut c, &tuned);
        assert_eq!(CONTROLS_PRESET.current(&c), None);
        assert!((c.controls.preferences.mouse_sensitivity_x - 0.8).abs() < 1e-6);
        assert_eq!(c.controls.models.thrust, tuned.controls.models.thrust);
        // A member off by less than a slider step still matches.
        let mut near = Config::default();
        near.controls.preferences.mouse_sensitivity_x = 1.0 - 1e-5;
        assert_eq!(CONTROLS_PRESET.current(&near), Some(Preset::Enhanced));
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
                // A preset row's "custom" is read back, never set:
                // see `presets_apply_and_read_back`.
                Ctl::Choice { .. } if preset_group(spec.cfg_path).is_some() => {}
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
