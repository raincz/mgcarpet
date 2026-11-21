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
            desc: "Entity pool capacity, default 20000. Retail caps the world at \
                   1000 things and silently drops spawns beyond it; the larger \
                   pool carries rosters the original would have shed. Set to \
                   1000 for the retail limit. Set from the command line or \
                   config file; fixed for the run.",
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
            desc: "Sets the look below as a group: fog, fire and lightning, \
                   smooth motion, rival tags, the map markers and the rest. \
                   Custom means they have been tuned by hand and match neither \
                   preset; it is shown, not chosen.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    if let Some(&p) = Preset::ALL.get(i) {
                        (VISUALS_PRESET.apply)(c, p);
                    }
                },
                descs: &[
                    "Every visual enhancement on, fog pushed back to the \
                     90-tile maximum (default).",
                    "How the originals look: retail fog, fire and \
                     lightning, no crosshair, no map extras (motion \
                     stays smooth).",
                    "Hand-tuned: the options below match neither preset.",
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
            desc: "Stop drawing terrain past the horizon melt (which follows \
                   the fog distance): each visible tile once, in its nearest \
                   wrap copy, instead of the whole map nine times. A large \
                   frame-rate win at every fog distance — retail's 20 tiles \
                   becomes the cheapest view. Off = the old full draw.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.horizon_cull = v,
                descs: [
                    "The whole map, nine times, every frame.",
                    "Only the terrain inside the melt band (default).",
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
                   The horizon silhouettes beyond the fog melt into the sky \
                   across a band that follows this distance (1.2x .. +1/3), \
                   and the horizon cull stops drawing behind it, so shorter \
                   is also faster. Up to 90 nothing past the map's edge is \
                   drawn; extreme (120) reaches past it, where the world \
                   wraps around inside the fogged zone.",
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
            desc: "The window's size when not fullscreen. It can still be \
                   resized by hand; the next start opens at this size again. \
                   A size larger than the monitor is allowed (the window \
                   manager may clamp it).",
            ctl: Ctl::Choice {
                set: |c, i| c.render.preference.window_size = crate::config::WindowSize::ALL[i],
                descs: &[
                    "640 × 480, the original's own frame, pixel for pixel.",
                    "1280 × 960, twice the original 640 × 480 (default).",
                    "1600 × 1200, two and a half times 640 × 480.",
                    "1920 × 1440, three times 640 × 480.",
                    "1920 × 1080, Full HD (16:9).",
                    "2560 × 1440, QHD (16:9).",
                    "3840 × 2160, 4K UHD (16:9).",
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
            desc: "Show the end-of-level stats: the performance screen after \
                   a Magic Carpet level (it waits for a key), and the stats \
                   table on the Magic Carpet 2 map after a win.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.preference.stats_screen = v,
                descs: [
                    "Go straight on after a level.",
                    "Show them (as the original does).",
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
                    "Show the narration as text (default).",
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
            key: None,
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
            desc: "The view and the world move frame-smooth: rendered \
                   interpolated between the last two sim ticks, so movement \
                   glides at any fps instead of stepping at tick rate. Off, \
                   camera and entities alike step at tick rate. Presentation \
                   only — the simulation is untouched; the displayed world \
                   runs one tick (~40 ms) behind.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.smooth_motion = v,
                descs: [
                    "View and entities step at sim tick rate, as retail drew them.",
                    "View and entities glide — per-frame interpolation (default).",
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
                    "Retail fire/explosion sprites, as the original drew them.",
                    "Procedural flame, smoke and shockwave (default).",
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
                    "Retail zigzag flash sprites, as the original drew them.",
                    "Procedural fractal bolt with branches and a strike \
                     envelope (default).",
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
                   thing itself: spell jars, dolmens/shrines, statues, \
                   teleporters, and dead wizards' graves (the bones you \
                   possess to reclaim their mana - a dot is lost among \
                   the flags). \
                   Drawn small — half the spell-stamp size — and scaled \
                   by the marker-size slider. (The expose-jar-spells \
                   debug option outranks this for jars.)",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_marker_icons = v,
                descs: [
                    "Plain dots, as retail draws them.",
                    "Jars, dolmens, statues, teleporters and wizard graves wear miniature sprites.",
                ],
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
            desc: "Give the hunted map markers - creatures, mana, dwelling \
                   flags, spell jars, rival wizards and the marker \
                   miniatures - a one-pixel halo, black under a bright \
                   marker and white under a dark one, so they stay \
                   readable on any ground (bones on barren, mana on \
                   snow). Scenery, civilians and spell effects get none. \
                   Always one pixel, whatever the marker-size slider.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.autocontrasting_markers = v,
                descs: [
                    "Bare markers, as retail draws them.",
                    "Hunted markers wear a black-or-white contrast halo.",
                ],
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
            desc: "Texture filtering of the world art. Off = retail's \
                   chunky pixels; on = terrain textures and sprites \
                   smoothly filtered, sprite edges included.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.smooth_textures = v,
                descs: [
                    "Chunky pixels — the original look.",
                    "Smoothly filtered terrain and sprites.",
                ],
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
            desc: "Draw order of segmented creatures (worms, dragons, \
                   hydras). Off = retail's tile order, which flickers \
                   overlapping segments of a creature seen side-on; on = \
                   the segment nearer the head wins near-ties.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.segment_order = v,
                descs: [
                    "Retail's tile order — side-on bodies flicker.",
                    "Head-first segment order, no flicker.",
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
        Spec {
            domain: Render,
            group: "render · enhancement",
            label: "map_beyond_sight_areas",
            class: Enhancement,
            key: None,
            cli: None,
            cfg_path: "render.enhancement.map_beyond_sight_areas",
            read: toggle!(c => render.enhancement.map_beyond_sight_areas),
            desc: "While Beyond Sight is up, the level's trigger areas ripple \
                   on the map and the radar like a water surface, with a \
                   trace of tint: red where flying in springs a trap, cyan \
                   for any other effect. Faint on purpose — look for it. \
                   Triggers that are not a place (a mana goal, a kill \
                   count) are not shown, and nothing appears in the world \
                   view. Neither original reveals trigger areas.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.map_beyond_sight_areas = v,
                descs: [
                    "Beyond Sight shows what retail's does: rival wizards \
                     (and MC2's tiers).",
                    "Beyond Sight also reveals trigger areas as faint red / \
                     cyan ripples on the map.",
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
            desc: "MC2's level exit, filmed from behind: the view floats \
                   out to a chase camera and watches the carpet speed \
                   into the portal, which flashes as it vanishes. The \
                   flight itself is retail's.",
            ctl: Ctl::Toggle {
                set: |c, v| c.render.enhancement.mc2_fancy_exit = v,
                descs: [
                    "First-person exit flight, as retail (without its \
                     motion blur).",
                    "Chase-camera exit: the carpet flies into the portal \
                     and vanishes in a flash.",
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
            key: None,
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
            key: None,
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
            key: None,
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
            desc: "Sets the controls below as a group: key bindings, mouse turn \
                   share, Y polarity and the thrust and altitude models. Custom \
                   means they have been tuned by hand and match neither preset; \
                   it is shown, not chosen.",
            ctl: Ctl::Choice {
                set: |c, i| {
                    if let Some(&p) = Preset::ALL.get(i) {
                        (CONTROLS_PRESET.apply)(c, p);
                    }
                },
                descs: &[
                    "WASD, full mouse turn share, mouse up climbs, the \
                     hold-to-fly thrust model and altitude keys (default).",
                    "How both originals fly: arrow keys, half mouse turn \
                     share, flight-stick Y, the retail thrust and \
                     terrain-follow altitude.",
                    "Hand-tuned: the controls below match neither preset.",
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
                   0-100%. Set by the controls preset: 100% under \
                   Enhanced (the default), 50% under Classic.",
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
                    "Mouse up = nose up (FPS convention; the Enhanced \
                     preset and the default).",
                    "Mouse up = nose down, flight-stick style (the \
                     original polarity; the Classic preset).",
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
            desc: "Hold a fire button to keep casting a click-only projectile \
                   spell (Fireball, Possession, Meteor, Steal Mana, Duel, \
                   Alliance) at a gentle 4 casts a second — slower than fast \
                   clicking, there to spare fingers and mouse. Spells that \
                   already repeat when held, and every effect spell, are \
                   untouched. Neither original has it.",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.autofire = v,
                descs: [
                    "One click, one cast, as both originals play (the \
                     Classic preset).",
                    "A held button re-casts at 4 Hz, or as fast as the \
                     spell and your mana allow if that is slower (the \
                     Enhanced preset and the default).",
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
            desc: "Disable the Shift+K suicide key, which both originals \
                   have and which is easy to hit by accident next to \
                   Shift+L.",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.disable_suicide_key = v,
                descs: [
                    "Shift+K kills you, as in both originals (the default).",
                    "Shift+K does nothing.",
                ],
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
            desc: "Disable MC2's barrel roll (pressing both strafe keys \
                   together tumbles the carpet). MC2 only; MC1 has no roll.",
            ctl: Ctl::Toggle {
                set: |c, v| c.controls.preferences.disable_barrel_roll = v,
                descs: [
                    "Both strafe keys together roll the carpet, as in \
                     MC2 (the default).",
                    "Both strafe keys together just strafe; no roll.",
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
            desc: "Sound-effect master gain — the MC2 narration rides it too \
                   (the original has no narration volume).",
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
                   original's in-game Speech option. Narration also follows \
                   the sound toggle and the sfx volume (the original's \
                   sound/music switches leave it playing, at one volume).",
            ctl: Ctl::Toggle {
                set: |c, v| c.audio.speech = v,
                descs: [
                    "Objectives arrive silently.",
                    "The narrator speaks (when sound is on, at the sfx volume).",
                ],
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
            desc: "Mana balls roll downhill everywhere (both games). Retail only \
                   rolls balls within 24 tiles of you - a period perf save - so \
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
            label: "ball_owner_recolor",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.ball_owner_recolor",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.ball_owner_recolor.on(),
                faithful: false,
            },
            desc: "Mana balls and MC2 mana worms show their current owner's \
                   colour everywhere. Retail recolours a ball only when it \
                   moves, and a settled ball only moves within 24 tiles of \
                   you, so mana that changes hands out of range - a possessed \
                   corpse's spheres, a rival possessing yours - keeps the old \
                   colour until you approach it (the minimap is right at \
                   once). Drawing only for balls. A worm possessed out of \
                   range does not even change owner in retail until you \
                   approach; patched, the possess lands at once (a sim \
                   change, pinned to retail while recording or replaying).",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.ball_owner_recolor = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Far-off balls and worms keep the previous owner's colour, as retail.",
                    "Every ball and mana worm wears its owner's colour (default).",
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
            label: "mc2_leviathan_high_lunge",
            class: Patch,
            key: None,
            cli: None,
            cfg_path: "gameplay.patches.mc2_leviathan_high_lunge",
            read: |c| Val::Toggle {
                on: c.gameplay.patches.mc2_leviathan_high_lunge.on(),
                faithful: false,
            },
            desc: "Two leviathan changes, neither a bug fix. AIM: retail steers \
                   at a point a fixed 3 tiles ahead of wherever its target \
                   faces - right for a carpet in full flight, a miss to the \
                   side for one that hovers. Patched, the lead scales with \
                   the target's real speed, so a hovering carpet is lunged at \
                   directly. HEIGHT (only with the enhanced altitude model): \
                   lunges reach a random 768 to 1200 rolled per strike, where \
                   retail's stop at 768 and the enhanced lift parks at 1024 \
                   out of reach; parked straight overhead, about two strikes \
                   in five now land, and the strike is drawn as tall as it \
                   reaches.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_leviathan_high_lunge =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Retail: a fixed 3-tile lead and a 768 reach - hovering, \
                     or parking at 1024, is safe.",
                    "Lead by the target's speed; lunges reach 768..1200 \
                     under the enhanced lift (default).",
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
            desc: "Spells you hold stay yours for the whole level (both games). \
                   Retail's abandoned death-scatter mechanism can silently \
                   drop a spell when the entity pool is full, and a stale \
                   handle (the volcano's plume) can destroy a held spell's \
                   token while you are alive - after which no dropped jar of \
                   that spell can be picked up again. Only MC2's undead \
                   wraith, the one designed theft, still takes a spell.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.patches.no_spell_loss = crate::config::PatchArm::from_on(v),
                descs: [
                    "Death and stale handles can eat spells, as retail.",
                    "The spellbook is permanent; death drops cosmetic jars (default).",
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
            desc: "The MC1 dragon's tail stays in formation at any range. Retail \
                   only keeps body segments behind their head within 24 tiles of \
                   you; further out the tail piles onto the head and snaps back \
                   when you approach - visible with the port's long view. This \
                   fixes the tail without raising the awake range, which would \
                   wake every creature on the map.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_fix_dragon_tail = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Distant tails collapse onto the head, as retail.",
                    "Tails trail their head everywhere (default).",
                ],
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
            desc: "MC1 creatures no longer speed up for good. Retail's pack \
                   logic flies a follower one acceleration step faster than \
                   its leader and never slows it back down when the pack \
                   breaks up; the next pack it leads starts from that speed, \
                   so over a long level vultures, wyverns and worms end up \
                   crossing the map in seconds. With this on, a creature \
                   leaving a pack returns to its own top speed.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_pack_speed_restore = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Pack speed carries over and compounds, as retail.",
                    "Leaving a pack restores the creature's own top speed (default).",
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
            desc: "No phantom castle at the map origin (MC2). When a village \
                   building finishes, retail re-paints every wizard, corpse and \
                   balloon overlapping it as if it were a castle; a dead rival \
                   lying on a village then raises level-7 castle terrain at \
                   (0,0), at sea level, each time a hut completes under it. \
                   This re-paints castles only.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_phantom_castle = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Corpses on villages raise castle terrain at (0,0), as retail.",
                    "Only castles are re-painted (default).",
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
            desc: "Dual wield: every spell fires from the hand it was cast with \
                   (both games). Retail keeps one firing-hand register per \
                   wizard, stamped by the last successful cast and read when \
                   the projectile is born, so with both hands casting \
                   everything leaves the RIGHT muzzle and a stream can switch \
                   sides mid-burst. (Casting the same spell with both hands is \
                   a separate mechanism and is not changed.)",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.dual_wield_muzzle = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Both hands share one muzzle; the last cast picks it, as retail.",
                    "Each hand's casts keep their own muzzle (default).",
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
            desc: "One castle per wizard (all three games). Retail can leave a \
                   wizard holding TWO castles at once: the \"do I own one\" \
                   test reads a pointer written a tick after the castle \
                   appears (MC2), demands the castle already be transformed \
                   (MC1 delivery), or is missing altogether (MC1 create), so \
                   two castle balls landing far enough apart in quick \
                   succession both build. The second takes the pointer and \
                   the first is orphaned ALIVE — its balloons never find a \
                   home, cycle health and bank no mana — and when either \
                   castle falls the owner is left with none at all.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.one_castle_per_wizard = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A second castle ball can build a second castle, as retail.",
                    "A castle ball never builds a second castle (default).",
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
            desc: "Allied wyverns keep their wits (MC2). Retail's wyvern is the \
                   one creature whose attack state has no exit for a lost \
                   target: cast Alliance on a wyvern that is attacking, or let \
                   the charm run out while it fights, and it flies its last \
                   heading for the rest of its life - never shooting, never \
                   re-targeting, never waking, its ally tint blinking forever. \
                   Patched, it drops to idle like every other species and the \
                   charm resolves normally.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_wyvern_alliance_brain =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A charmed wyvern that loses its target flies on forever, as retail.",
                    "A charmed wyvern that loses its target drops to idle (default).",
                ],
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
            desc: "Mana worms and moon dwellers pass over steep walls they are \
                   well above (MC2). Retail stops every creature at a steep or \
                   forbidden tile whatever its altitude, so a mana worm heading \
                   straight for its castle can hang forever against a rampart \
                   far below it, until a crater clears the way. Patched, a worm \
                   or dweller that is already airborne and would pass clear over \
                   the tile is not stopped by it. Every other creature keeps \
                   retail's rule, so bees and wyverns still cannot follow you \
                   over a castle wall; caves keep retail's rule too.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_flyers_clear_terrain =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Worms and dwellers stall against steep tiles far below them, as retail.",
                    "Airborne worms and dwellers pass over steep tiles below them (default).",
                ],
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
            desc: "Skeletons can turn traders into skeletons too. A roaming \
                   skeleton converts the nearest civilian, hunting one kind at \
                   a time in turn: militia, settlers, villagers. Retail never \
                   puts the trader on that list, so traders alone are immune. \
                   Patched, the rotation includes them. Castle guards and \
                   animals are never converted either way.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.skeletons_convert_traders =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Skeletons never convert traders, as retail.",
                    "Skeletons convert traders like other civilians (default).",
                ],
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
            desc: "Dead mana balloons always pop (MC2). The only thing in retail \
                   that clears a dead mana balloon is its own castle's fleet \
                   pass, so destroy a wizard's castle before his balloon and \
                   what is left cannot be hurt, cannot be killed and never goes \
                   away: it keeps his colours, drifts after whatever has taken \
                   over the dead castle's slot - a hydra, a creature, you - and \
                   is still there when the level ends. Patched, a dead balloon \
                   with no castle left dissolves into its mana sphere the way \
                   the castle pass would have done.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_orphan_balloon_reap = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A castle-less dead balloon haunts the level forever, as retail.",
                    "A castle-less dead balloon dissolves into mana (default).",
                ],
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
            desc: "Claimed houses fly their owner's castle colour (MC2). Retail \
                   colours a claimed house's flag by the owner's raw player \
                   slot while the castle uses the proper team colour, so for \
                   four of the eight wizards a rival's houses and castle show \
                   different colours. Patched, houses match the castle.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_house_flag_color = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Houses fly the owner's raw slot colour, as retail.",
                    "Houses fly the owner's castle colour (default).",
                ],
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
            desc: "A dying MC2 record's pool slot returns within its own \
                   tick. Retail frees dead records in the NEXT frame's \
                   opening sweep, so a stale index can still reach a \
                   corpse for one more frame and the slot is re-popped by \
                   whatever spawns first. Patched, the slot is freed at the \
                   end of the record's own dispatch.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_immediate_reap = crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "Dead records are freed at the next frame's top, as retail.",
                    "Dead records are freed within their own tick (default).",
                ],
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
            desc: "When the MC1 entity pool runs dry, retail sacrifices the \
                   slots it listed as expendable at the last death — without \
                   checking what lives in them now. A slot re-used since (a \
                   castle's ground-leveler, a projectile) is eaten, and a \
                   castle whose leveler dies that way is parked mid-transform \
                   for the rest of the level: immune, stuck at its level, its \
                   dead balloon immortal, its owner unbanishable. Patched, a \
                   listed slot that is no longer expendable is skipped.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_recycle_victim_revalidate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "The dry-pool sacrifice eats whatever the listed slot holds, as retail.",
                    "A listed slot that is no longer expendable is skipped (default).",
                ],
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
            desc: "An MC1 castle mid-transformation waits for its painter or \
                   ground-leveler to finish; if that worker is destroyed from \
                   outside, retail waits forever. Patched, a castle waiting on \
                   a worker that no longer exists takes the leveler's own abort \
                   exit and settles next tick. Never fires while a worker \
                   stands at the site, so healthy transformations keep their \
                   full length.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_castle_transform_watchdog =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A castle whose worker is gone waits forever, as retail.",
                    "A castle whose worker is gone settles next tick (default).",
                ],
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
            desc: "An MC1 castle founded or upgraded over a dwelling still \
                   under construction crushes it without ending the \
                   construction: the site never finishes, its flag never \
                   goes up, it cannot be possessed, and it keeps reshaping \
                   its footprint the wrong way for the rest of the level \
                   (towers at the height ceiling beside pits at the floor). \
                   Patched, the crushed site collapses like a crushed \
                   finished house.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_crushed_site_collapse =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A crushed construction site reshapes its ground forever, as retail.",
                    "A crushed construction site collapses (default).",
                ],
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
            desc: "An MC2 building (or castle stage) raised on ground high \
                   enough that its pad would pass the height ceiling wraps \
                   around instead: its tallest parts come out as deep pits \
                   and the finished building sinks into the one under its \
                   centre (the ridge town of level 22). Patched, the pad \
                   tops out flat at the ceiling.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc2_building_pad_saturate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "An over-tall building pad wraps into pits, as retail.",
                    "An over-tall building pad tops out flat (default).",
                ],
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
            desc: "An MC1 dwelling built, or castle raised, on ground high \
                   enough that its tallest part would pass the height \
                   ceiling wraps around instead: a rising castle's towers \
                   drop into pits (climbing back out when it settles, \
                   unless the castle stands very high), and a dwelling \
                   keeps its pits and sinks into the one under its centre. \
                   Patched, a dwelling's pad tops out flat at the ceiling \
                   and a castle is raised no higher than its tallest tower \
                   can stand.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_building_pad_saturate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "An over-tall building or castle wraps into pits, as retail.",
                    "An over-tall building tops out, a castle sits lower (default).",
                ],
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
            desc: "An MC1 sea monster killed a second time while already \
                   dying can turn a nearby fire into a mana ball that can \
                   never be collected or possessed: it stays for the rest \
                   of the level, soaks up the mana of balls dropped onto \
                   it, and the map marks it magenta. Patched, the second \
                   death leaves the fire alone.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.mc1_segment_chain_revalidate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "A twice-killed sea monster can leave a phantom mana ball, as retail.",
                    "A twice-killed sea monster leaves no phantom ball (default).",
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
            desc: "A volcano (MC1 and MC2) that starts erupting knocks the \
                   previous volcano dormant and kills the previous lava \
                   plume or fire column by writing into their old slots \
                   without checking what lives there now. An MC1 castle \
                   there jumps to level 250, and a few downgrades later \
                   the game freezes; anything else there is killed (a \
                   rival's spell, which can end in hovering spell jars, or \
                   a loose mana sphere), and an MC2 fire column born in the \
                   old one's slot kills itself. Patched, the volcano only \
                   touches its own volcano and plume.",
            ctl: Ctl::Toggle {
                set: |c, v| {
                    c.gameplay.patches.volcano_register_revalidate =
                        crate::config::PatchArm::from_on(v)
                },
                descs: [
                    "An eruption can wreck whatever took the old volcano's slots (an MC1 castle freezes the game), as retail.",
                    "An eruption only touches the previous volcano and plume (default).",
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
            desc: "Every spell selectable + infinite mana while on — the \
                   spell-track playtest instrument. An overlay: jars stay \
                   collectable, and switching it off takes back only what \
                   the cheat lent (the original's own cheat grants for good).",
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
        Spec {
            domain: Gameplay,
            group: "gameplay · cheat",
            label: "ghost",
            class: Cheat,
            key: Some("J"),
            cli: Some("--ghost"),
            cfg_path: "gameplay.cheat.ghost",
            read: toggle!(c => gameplay.cheat.ghost),
            desc: "Ghost mode: permanently invisible to everything (casting \
                   does not break it, nothing sees through it) and flies \
                   over impassable walls as if they were terrain. Not \
                   invincibility. Neither original has it.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.ghost = v,
                descs: [
                    "Seen and walled in, as the game intends.",
                    "Unseen, walls are terrain (cheat).",
                ],
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
            desc: "Inert mode: the carpet trips no world trigger — no \
                   pickups, teleporters, trigger volumes or switches. \
                   Neither original has it.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.inert = v,
                descs: [
                    "Triggers fire, as the game intends.",
                    "Nothing you fly over fires (cheat).",
                ],
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
            desc: "Weightless mode (was dev.lift_unclamped): q/e may pin \
                   the desired altitude anywhere up to the level's highest \
                   terrain + a 4-tile margin, instead of the per-game \
                   ground-relative band. Applies live.",
            ctl: Ctl::Toggle {
                set: |c, v| c.gameplay.cheat.weightless = v,
                descs: [
                    "The per-game band: 1024 over terrain (MC2 caves 3072).",
                    "Free climb to the global lift ceiling (cheat).",
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
        // skeletons_convert_traders 2026-10-05.
        assert_eq!(patches, 29, "all twenty-nine patches ship on");
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
