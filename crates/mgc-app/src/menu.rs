//! The in-game options menu, riding on pause (P): a darkened panel
//! over the live view, one tab per option domain, every registry
//! option as a row — toggles, choice cycles, sliders (0.1 steps) and
//! stop-bars, with a hover explanation box at the bottom. Entirely a
//! VIEW over the `settings::registry()` table: rows, widgets, hover
//! texts and write paths all come from the Specs; the menu adds only
//! geometry and ink.
//!
//! Ink rule: option labels are always
//! white; the default/non-default distinction lives on the WIDGET —
//! a value at its default draws quiet grey, a changed value draws
//! bright green. Group headings are dim, drop the redundant domain
//! prefix (the tab already says RENDER), and their rows indent under
//! them. Startup-mutability options are greyed darker and locked
//! (they apply at level load). The messaging FONT1 is the typeface at
//! notification size, so tall tabs scroll: mouse wheel, or click the
//! side bar.

use crate::config::Config;
use crate::settings::{self, Ctl, Mutability, Spec, Val};
use crate::ui::UiAssets;
use mgc_render::UiQuad;

/// Panel geometry fractions of the viewport.
const PANEL_X: f32 = 0.14;
const PANEL_Y: f32 = 0.15;
const PANEL_W: f32 = 0.72;
const PANEL_H: f32 = 0.64;

// Ink palette.
/// Option labels + the active tab: always white.
const INK_LABEL: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
const INK_WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Inactive tabs.
const INK_TAB_IDLE: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
/// Widget/value at its default: quiet grey.
const INK_VAL_DEFAULT: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
/// Widget/value moved off its default: bright green.
const INK_VAL_CHANGED: [f32; 4] = [0.30, 1.0, 0.45, 1.0];
const INK_LOCKED: [f32; 4] = [0.38, 0.38, 0.40, 1.0];
/// Group headings: dimmer than everything interactive.
const INK_HEADING: [f32; 4] = [0.38, 0.40, 0.44, 1.0];
const INK_DESC: [f32; 4] = [0.82, 0.82, 0.78, 1.0];
const INK_DESC_SEL: [f32; 4] = [0.62, 0.66, 0.72, 1.0];
const PANEL_BG: [f32; 4] = [0.02, 0.02, 0.04, 0.82];
const PANEL_EDGE: [f32; 4] = [0.75, 0.75, 0.75, 0.35];
const ROW_HOVER_BG: [f32; 4] = [1.0, 1.0, 1.0, 0.06];
const TAB_ACTIVE_BG: [f32; 4] = [1.0, 1.0, 1.0, 0.10];
const SCROLL_TRACK: [f32; 4] = [0.5, 0.5, 0.5, 0.25];
const SCROLL_THUMB: [f32; 4] = [0.75, 0.75, 0.75, 0.6];

pub struct MenuState {
    /// Active domain tab (index into [`settings::DOMAINS`]).
    pub tab: usize,
    /// Registry index of the slider/stop-bar being dragged.
    pub drag: Option<usize>,
    /// Scroll offset of the active tab, in whole rows (clamped by the
    /// layout pass).
    scroll: usize,
    /// Fractional wheel accumulator (trackpads send sub-row deltas).
    scroll_acc: f32,
    /// Each option's value text at `Config::default()` — the
    /// "is this at its default" baseline for the grey/green widget ink.
    default_texts: Vec<String>,
    /// Rows a netplay session holds fixed (the host's rules): drawn
    /// locked, never applied.
    pinned: Vec<bool>,
}

impl MenuState {
    pub fn new(specs: &[Spec]) -> Self {
        let d = Config::default();
        MenuState {
            tab: 0,
            drag: None,
            scroll: 0,
            scroll_acc: 0.0,
            default_texts: specs.iter().map(|s| (s.read)(&d).current_text()).collect(),
            pinned: vec![false; specs.len()],
        }
    }

    /// Lock the rows the netplay session pins ([`crate::rules`]); a
    /// preset row locks when any member it would set is pinned.
    pub fn pin_session_rules(&mut self, specs: &[Spec]) {
        self.pinned = specs
            .iter()
            .map(|s| match settings::preset_group(s.cfg_path) {
                Some(g) => g.members.iter().any(|m| crate::rules::locked_in_game(m)),
                None => crate::rules::locked_in_game(s.cfg_path),
            })
            .collect();
    }

    /// Switch tabs (resets the scroll).
    pub fn set_tab(&mut self, tab: usize) {
        self.tab = tab;
        self.scroll = 0;
        self.scroll_acc = 0.0;
    }

    /// Jump the scroll to a row offset (a scroll-track click; the
    /// layout pass clamps).
    pub fn scroll_to(&mut self, row: usize) {
        self.scroll = row;
        self.scroll_acc = 0.0;
    }
}

/// What a click landed on.
pub enum Hit {
    None,
    Tab(usize),
    /// Registry index of the row whose widget was clicked.
    Widget(usize),
    /// A click on the scroll track: jump to this row offset.
    ScrollTo(usize),
}

/// One VISIBLE laid-out row on the active tab.
struct Row {
    /// Registry index; None = a group heading.
    spec: Option<usize>,
    y: f32,
    /// Heading text (group rows only).
    heading: Option<String>,
}

struct Layout {
    s: f32,
    /// FONT1 scale — notification size (w/320), the nostalgia face.
    fs: f32,
    lh: f32,
    row_h: f32,
    panel: [f32; 4],
    tab_h: f32,
    /// The visible slice of this tab's rows, y assigned.
    rows: Vec<Row>,
    /// Widget column: x, width (the interactive strip of a row).
    widget_x: f32,
    widget_w: f32,
    /// Hover-explanation box top edge (it runs to the panel bottom).
    desc_y: f32,
    /// Scrolling: clamped offset, total rows, visible capacity.
    scroll: usize,
    total: usize,
    capacity: usize,
    /// The scroll track rect, when the tab overflows.
    scrollbar: Option<[f32; 4]>,
}

impl Layout {
    fn max_scroll(&self) -> usize {
        self.total.saturating_sub(self.capacity)
    }
}

fn layout(assets: &UiAssets, specs: &[Spec], st: &MenuState, w: f32, h: f32) -> Layout {
    let s = crate::ui::HudFrame::new(w, h).s.max(1.0);
    // Notification-size FONT1 (glyphs are 320-native).
    let fs = 2.0 * s;
    let lh = assets.font_line_height().max(8.0) * fs;
    let row_h = lh * 1.25;
    let panel = [PANEL_X * w, PANEL_Y * h, PANEL_W * w, PANEL_H * h];
    let tab_h = lh * 1.4;
    // Five lines of help: the description (up to ~3) and the current
    // choice's line (player ruling 2026-11-22 — three lines left room
    // for one short sentence, so the texts overflowed).
    let desc_h = lh * 5.4;
    let desc_y = panel[1] + panel[3] - desc_h;
    let pad = 8.0 * s;

    let domain = settings::DOMAINS[st.tab];
    // Group heading text: the tab already names the domain, so strip
    // it ("render · preference" → "PREFERENCE"); a group that IS just
    // the domain (audio, dev) gets no heading at all.
    let heading_text = |group: &str| -> Option<String> {
        let tail = group.split('·').next_back().unwrap_or(group).trim();
        let head = tail.to_uppercase();
        (head != domain.title()).then_some(head)
    };

    // This tab's full logical row list (headings + options).
    let mut logical: Vec<(Option<usize>, Option<String>)> = Vec::new();
    let mut last_group = "";
    for (i, spec) in specs.iter().enumerate() {
        if spec.domain != domain {
            continue;
        }
        if spec.group != last_group {
            if let Some(head) = heading_text(spec.group) {
                logical.push((None, Some(head)));
            }
            last_group = spec.group;
        }
        logical.push((Some(i), None));
    }

    let top = panel[1] + tab_h + pad;
    let avail = (desc_y - pad - top).max(row_h);
    let capacity = ((avail / row_h) as usize).max(1);
    let total = logical.len();
    let scroll = st.scroll.min(total.saturating_sub(capacity));

    let mut rows = Vec::new();
    let mut y = top;
    for (spec, heading) in logical.into_iter().skip(scroll).take(capacity) {
        rows.push(Row { spec, y, heading });
        y += row_h;
    }

    let scrollbar =
        (total > capacity).then(|| [panel[0] + panel[2] - 6.0 * s, top, 4.0 * s, avail - pad]);

    Layout {
        s,
        fs,
        lh,
        row_h,
        panel,
        tab_h,
        rows,
        widget_x: panel[0] + panel[2] * 0.42,
        widget_w: panel[2] * 0.32,
        desc_y,
        scroll,
        total,
        capacity,
        scrollbar,
    }
}

fn inside(r: [f32; 4], c: (f32, f32)) -> bool {
    c.0 >= r[0] && c.0 < r[0] + r[2] && c.1 >= r[1] && c.1 < r[1] + r[3]
}

fn tab_rect(l: &Layout, i: usize) -> [f32; 4] {
    let tw = l.panel[2] / settings::DOMAINS.len() as f32;
    [l.panel[0] + tw * i as f32, l.panel[1], tw, l.tab_h]
}

/// Is row `i`'s widget interactive in the menu?
fn row_live(st: &MenuState, specs: &[Spec], i: usize) -> bool {
    let spec = &specs[i];
    !matches!(spec.ctl, Ctl::ReadOnly)
        && spec.mutability() == Mutability::Live
        && !st.pinned.get(i).copied().unwrap_or(false)
}

/// Scroll the active tab by a (possibly fractional) number of rows —
/// the mouse-wheel path. Clamped against the tab's real overflow.
pub fn scroll_by(assets: &UiAssets, specs: &[Spec], st: &mut MenuState, w: f32, h: f32, rows: f32) {
    let l = layout(assets, specs, st, w, h);
    st.scroll_acc += rows;
    let whole = st.scroll_acc.trunc() as i64;
    if whole != 0 {
        st.scroll_acc -= whole as f32;
        st.scroll = (l.scroll as i64 + whole).clamp(0, l.max_scroll() as i64) as usize;
    } else {
        st.scroll = l.scroll;
    }
}

pub fn hit_test(
    assets: &UiAssets,
    specs: &[Spec],
    st: &MenuState,
    w: f32,
    h: f32,
    cursor: (f32, f32),
) -> Hit {
    let l = layout(assets, specs, st, w, h);
    for (i, _) in settings::DOMAINS.iter().enumerate() {
        if inside(tab_rect(&l, i), cursor) {
            return Hit::Tab(i);
        }
    }
    if let Some(track) = l.scrollbar {
        // A generous grab strip around the thin track.
        let grab = [
            track[0] - 4.0 * l.s,
            track[1],
            track[2] + 8.0 * l.s,
            track[3],
        ];
        if inside(grab, cursor) {
            let frac = ((cursor.1 - track[1]) / track[3]).clamp(0.0, 1.0);
            return Hit::ScrollTo((frac * l.max_scroll() as f32).round() as usize);
        }
    }
    for row in &l.rows {
        let Some(i) = row.spec else { continue };
        let widget = [l.widget_x, row.y, l.widget_w, l.row_h];
        if inside(widget, cursor) && row_live(st, specs, i) {
            return Hit::Widget(i);
        }
    }
    Hit::None
}

/// The slider/stop-bar track rect within a row's widget strip.
fn track_rect(l: &Layout, y: f32) -> [f32; 4] {
    [
        l.widget_x,
        y + l.row_h * 0.5 - 1.5 * l.s,
        80.0 * l.s,
        3.0 * l.s,
    ]
}

/// Apply a pointer position to spec `i`'s widget (click or drag).
/// Returns true when the config value changed. Sliders/stop-bars set
/// `st.drag` so motion keeps applying until release.
#[allow(clippy::too_many_arguments)]
pub fn pointer_apply(
    assets: &UiAssets,
    cfg: &mut Config,
    specs: &[Spec],
    st: &mut MenuState,
    w: f32,
    h: f32,
    cursor: (f32, f32),
    i: usize,
    click: bool,
) -> bool {
    let l = layout(assets, specs, st, w, h);
    let Some(row) = l.rows.iter().find(|r| r.spec == Some(i)) else {
        return false;
    };
    let spec = &specs[i];
    match &spec.ctl {
        Ctl::ReadOnly => false,
        Ctl::Toggle { set, .. } => {
            if !click {
                return false;
            }
            let on = match (spec.read)(cfg) {
                Val::Toggle { on, .. } => on,
                _ => return false,
            };
            set(cfg, !on);
            true
        }
        Ctl::Choice { set, .. } => {
            if !click {
                return false;
            }
            let (cur, n) = match (spec.read)(cfg) {
                Val::Choice { cur, variants, .. } => (cur, variants.len()),
                _ => return false,
            };
            // Left half of the widget strip = previous, right = next
            // (wrapping) — matching the drawn ‹ › chevrons. A variant
            // that does not land when set is shown, not chosen (a
            // preset row's "custom"), so the step moves past it.
            let back = cursor.0 < l.widget_x + l.widget_w * 0.5;
            let mut next = cur;
            for _ in 1..n {
                next = if back {
                    (next + n - 1) % n
                } else {
                    (next + 1) % n
                };
                set(cfg, next);
                if matches!((spec.read)(cfg), Val::Choice { cur: got, .. } if got == next) {
                    break;
                }
            }
            !matches!((spec.read)(cfg), Val::Choice { cur: got, .. } if got == cur)
        }
        Ctl::Slider {
            get,
            set,
            min,
            max,
            step,
        } => {
            let t = track_rect(&l, row.y);
            let frac = ((cursor.0 - t[0]) / t[2]).clamp(0.0, 1.0);
            let raw = min + frac * (max - min);
            let v = (raw / step).round() * step;
            if click {
                st.drag = Some(i);
            }
            if (get(cfg) - v).abs() < step * 0.5 {
                return false;
            }
            set(cfg, v);
            true
        }
        Ctl::Stops { get, set, stops } => {
            let t = track_rect(&l, row.y);
            let frac = ((cursor.0 - t[0]) / t[2]).clamp(0.0, 1.0);
            let k = (frac * (stops.len() - 1) as f32).round() as usize;
            let v = stops[k].0;
            if click {
                st.drag = Some(i);
            }
            if get(cfg) == v {
                return false;
            }
            set(cfg, v);
            true
        }
    }
}

/// Word-wrap `text` to `max_w` source-pixel units of the FONT1.
fn wrap(assets: &UiAssets, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let cand = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if !cur.is_empty() && assets.text_width(&cand) > max_w {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = cand;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// An axis-aligned chevron built from sliver rects (FONT1 may lack
/// `<`/`>` glyphs; rects are always there). `dir` -1 = left, 1 = right.
fn chevron(quads: &mut Vec<UiQuad>, x: f32, cy: f32, s: f32, dir: f32, ink: [f32; 4]) {
    let cols = 4;
    for c in 0..cols {
        let k = if dir < 0.0 { c } else { cols - 1 - c };
        let hh = (k as f32 + 1.0) * 1.5 * s;
        quads.push(crate::ui::solid(
            [x + c as f32 * 1.5 * s, cy - hh, 1.5 * s, hh * 2.0],
            ink,
        ));
    }
}

/// The per-selection hover text for the current value, if any.
fn selection_desc(spec: &Spec, cfg: &Config) -> Option<&'static str> {
    match (&spec.ctl, (spec.read)(cfg)) {
        (Ctl::Toggle { descs, .. }, Val::Toggle { on, .. }) => Some(descs[on as usize]),
        (Ctl::Choice { descs, .. }, Val::Choice { cur, .. }) => descs.get(cur).copied(),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    assets: &UiAssets,
    cfg: &Config,
    specs: &[Spec],
    st: &MenuState,
    w: f32,
    h: f32,
    cursor: (f32, f32),
) -> Vec<UiQuad> {
    let l = layout(assets, specs, st, w, h);
    let s = l.s;
    let fs = l.fs;
    let pad = 8.0 * s;
    // Option rows indent under their group heading.
    let indent = 14.0 * s;
    let mut quads = Vec::new();

    // Panel + border (1px edges).
    quads.push(crate::ui::solid(l.panel, PANEL_BG));
    let [px, py, pw, ph] = l.panel;
    for e in [
        [px, py, pw, s],
        [px, py + ph - s, pw, s],
        [px, py, s, ph],
        [px + pw - s, py, s, ph],
    ] {
        quads.push(crate::ui::solid(e, PANEL_EDGE));
    }

    // Tabs.
    for (i, d) in settings::DOMAINS.iter().enumerate() {
        let r = tab_rect(&l, i);
        let active = i == st.tab;
        if active {
            quads.push(crate::ui::solid(r, TAB_ACTIVE_BG));
            quads.push(crate::ui::solid(
                [r[0], r[1] + r[3] - 2.0 * s, r[2], 2.0 * s],
                INK_WHITE,
            ));
        } else if inside(r, cursor) {
            quads.push(crate::ui::solid(r, ROW_HOVER_BG));
        }
        let title = d.title();
        let tw = assets.text_width(title) * fs;
        let ink = if active { INK_WHITE } else { INK_TAB_IDLE };
        quads.extend(assets.text_quads(
            title,
            r[0] + (r[2] - tw) / 2.0,
            r[1] + (r[3] - l.lh) / 2.0,
            ink,
            fs,
        ));
    }
    // Tab bar underline.
    quads.push(crate::ui::solid([px, py + l.tab_h, pw, s], PANEL_EDGE));

    // The scroll bar (wheel scrolls; clicking the track jumps).
    if let Some(track) = l.scrollbar {
        quads.push(crate::ui::solid(track, SCROLL_TRACK));
        let frac = l.capacity as f32 / l.total as f32;
        let th = (track[3] * frac).max(12.0 * s);
        let max = l.max_scroll();
        let off = if max == 0 {
            0.0
        } else {
            (track[3] - th) * l.scroll as f32 / max as f32
        };
        quads.push(crate::ui::solid(
            [track[0], track[1] + off, track[2], th],
            SCROLL_THUMB,
        ));
    }

    // Rows.
    let mut hovered: Option<usize> = None;
    for row in &l.rows {
        let ty = row.y + (l.row_h - l.lh) / 2.0;
        if let Some(head) = &row.heading {
            quads.extend(assets.text_quads(head, px + pad, ty, INK_HEADING, fs));
            continue;
        }
        let i = row.spec.unwrap();
        let spec = &specs[i];
        let live = row_live(st, specs, i);
        let val = (spec.read)(cfg);
        let cur_text = val.current_text();
        let changed = cur_text != st.default_texts[i];
        let row_rect = [px, row.y, pw, l.row_h];
        if inside(row_rect, cursor) && cursor.1 < l.desc_y {
            hovered = Some(i);
            quads.push(crate::ui::solid(row_rect, ROW_HOVER_BG));
        }
        // Labels are always white; the default/changed distinction
        // lives on the widget ink alone (grey vs bright green).
        let label_ink = if live { INK_LABEL } else { INK_LOCKED };
        let val_ink = if !live {
            INK_LOCKED
        } else if changed {
            INK_VAL_CHANGED
        } else {
            INK_VAL_DEFAULT
        };
        quads.extend(assets.text_quads(spec.label, px + pad + indent, ty, label_ink, fs));

        // The widget strip.
        let wx = l.widget_x;
        let cy = row.y + l.row_h * 0.5;
        match &spec.ctl {
            Ctl::ReadOnly => {
                quads.extend(assets.text_quads(&cur_text, wx, ty, val_ink, fs));
                let note = "(set via cli/config)";
                let nx = wx + (assets.text_width(&cur_text) + 6.0) * fs;
                quads.extend(assets.text_quads(note, nx, ty, INK_LOCKED, fs));
            }
            Ctl::Toggle { .. } => {
                let on = matches!(val, Val::Toggle { on: true, .. });
                // Track + knob.
                let tw = 20.0 * s;
                let th = 8.0 * s;
                let track = [wx, cy - th / 2.0, tw, th];
                let track_ink = if on {
                    [val_ink[0], val_ink[1], val_ink[2], 0.55]
                } else {
                    [0.5, 0.5, 0.5, 0.25]
                };
                quads.push(crate::ui::solid(track, track_ink));
                let kw = 8.0 * s;
                let kx = if on { wx + tw - kw } else { wx };
                quads.push(crate::ui::solid(
                    [kx, cy - th / 2.0 - s, kw, th + 2.0 * s],
                    val_ink,
                ));
                quads.extend(assets.text_quads(
                    if on { "on" } else { "off" },
                    wx + tw + 6.0 * s,
                    ty,
                    val_ink,
                    fs,
                ));
            }
            Ctl::Choice { .. } => {
                chevron(&mut quads, wx, cy, s, -1.0, val_ink);
                quads.extend(assets.text_quads(&cur_text, wx + 10.0 * s, ty, val_ink, fs));
                chevron(&mut quads, wx + l.widget_w - 8.0 * s, cy, s, 1.0, val_ink);
            }
            Ctl::Slider { get, min, max, .. } => {
                let t = track_rect(&l, row.y);
                quads.push(crate::ui::solid(t, [0.5, 0.5, 0.5, 0.3]));
                let frac = ((get(cfg) - min) / (max - min)).clamp(0.0, 1.0);
                quads.push(crate::ui::solid(
                    [t[0], t[1], t[2] * frac, t[3]],
                    [val_ink[0], val_ink[1], val_ink[2], 0.7],
                ));
                quads.push(crate::ui::solid(
                    [
                        t[0] + t[2] * frac - 2.0 * s,
                        cy - 5.0 * s,
                        4.0 * s,
                        10.0 * s,
                    ],
                    val_ink,
                ));
                quads.extend(assets.text_quads(&cur_text, t[0] + t[2] + 8.0 * s, ty, val_ink, fs));
            }
            Ctl::Stops { get, stops, .. } => {
                let t = track_rect(&l, row.y);
                quads.push(crate::ui::solid(t, [0.5, 0.5, 0.5, 0.3]));
                // Nearest stop by value = the knob position.
                let cur = get(cfg);
                let k = stops
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, (v, _))| v.abs_diff(cur))
                    .map(|(k, _)| k)
                    .unwrap_or(0);
                for (j, _) in stops.iter().enumerate() {
                    let x = t[0] + t[2] * j as f32 / (stops.len() - 1) as f32;
                    quads.push(crate::ui::solid(
                        [x - s * 0.5, cy - 4.0 * s, s, 8.0 * s],
                        if j == k {
                            val_ink
                        } else {
                            [0.6, 0.6, 0.6, 0.5]
                        },
                    ));
                }
                let kx = t[0] + t[2] * k as f32 / (stops.len() - 1) as f32;
                quads.push(crate::ui::solid(
                    [kx - 2.0 * s, cy - 5.0 * s, 4.0 * s, 10.0 * s],
                    val_ink,
                ));
                // Value + the stop's tag when it matches exactly.
                let tag = stops
                    .iter()
                    .find(|(v, _)| *v == cur)
                    .map(|(_, t)| format!("{cur_text} ({t})"))
                    .unwrap_or(cur_text.clone());
                quads.extend(assets.text_quads(&tag, t[0] + t[2] + 8.0 * s, ty, val_ink, fs));
            }
        }

        // Key hint, right-aligned (clear of the scroll bar).
        if let Some(key) = spec.key {
            let kw = assets.text_width(key) * fs;
            quads.extend(assets.text_quads(key, px + pw - pad - 6.0 * s - kw, ty, INK_LOCKED, fs));
        }
    }

    // Hover explanation box.
    quads.push(crate::ui::solid([px, l.desc_y, pw, s], PANEL_EDGE));
    let max_w = (pw - 2.0 * pad) / fs;
    let mut dy = l.desc_y + 4.0 * s;
    let put = |quads: &mut Vec<UiQuad>, text: &str, ink: [f32; 4], dy: &mut f32| {
        for line in wrap(assets, text, max_w) {
            if *dy + l.lh > py + ph - 2.0 * s {
                return;
            }
            quads.extend(assets.text_quads(&line, px + pad, *dy, ink, fs));
            *dy += l.lh;
        }
    };
    match hovered {
        Some(i) => {
            let spec = &specs[i];
            let lock = if st.pinned.get(i).copied().unwrap_or(false) {
                " (fixed for this session)"
            } else if spec.mutability() == Mutability::Startup {
                " (applies at level load)"
            } else {
                ""
            };
            put(
                &mut quads,
                &format!("{}{}: {}", spec.label, lock, spec.desc),
                INK_DESC,
                &mut dy,
            );
            if let Some(sel) = selection_desc(spec, cfg) {
                let cur = (spec.read)(cfg).current_text();
                put(&mut quads, &format!("{cur}: {sel}"), INK_DESC_SEL, &mut dy);
            }
        }
        None => {
            put(
                &mut quads,
                "P or Esc resumes. Changes apply immediately and are saved to \
                 mgcarpet.json; the F-keys and letter toggles still work here.",
                INK_DESC_SEL,
                &mut dy,
            );
        }
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Under netplay the session's pinned rows lock — a preset too
    /// when it would set a pinned member — and the player's own rows
    /// stay live.
    /// EVERY OPTION'S HELP FITS ITS BOX (player ruling 2026-11-22: the
    /// texts overflowed and read like engineering notes). The box under
    /// the list holds three FONT1 lines at the narrowest common layout
    /// (4:3): the "label: description" paragraph plus the "choice:
    /// what it does" line for toggles and choices, each wrapped exactly
    /// as [`draw`] wraps them. Skips without a baked UI bundle.
    #[test]
    fn every_option_description_fits_its_box() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../baked/assets");
        let Ok(b) = mgc_formats::bundle::Bundle::load(&root.join("mc2-night")) else {
            eprintln!("skip: no baked mc2-night bundle");
            return;
        };
        let Some((idx, px)) = b.ui_sprites.as_ref() else {
            return;
        };
        let assets = UiAssets::build(
            idx.clone(),
            px,
            &b.palette,
            b.blend_lut.as_deref(),
            false,
            b.font.as_ref().map(|(i, p)| (i, p.as_slice())),
            b.web_sprites.as_ref().map(|(i, p)| (i, p.as_slice())),
        );
        let specs = settings::registry();
        let st = MenuState::new(&specs);
        let (w, h) = (1280.0, 960.0);
        let l = layout(&assets, &specs, &st, w, h);
        let max_w = (l.panel[2] - 2.0 * 8.0 * l.s) / l.fs;
        // The box's line budget, as `draw` spends it.
        let budget = ((l.panel[1] + l.panel[3] - 2.0 * l.s - (l.desc_y + 4.0 * l.s)) / l.lh).floor()
            as usize;
        let cfg = Config::default();
        let mut over = Vec::new();
        for spec in &specs {
            let suffix = if spec.mutability() == Mutability::Startup {
                " (applies at level load)"
            } else {
                ""
            };
            let head = wrap(
                &assets,
                &format!("{}{}: {}", spec.label, suffix, spec.desc),
                max_w,
            )
            .len();
            // The longest per-choice line this option can show.
            let choice = match (&spec.ctl, (spec.read)(&cfg)) {
                (Ctl::Toggle { descs, .. }, _) => ["off", "on"]
                    .iter()
                    .zip(descs.iter())
                    .map(|(c, d)| wrap(&assets, &format!("{c}: {d}"), max_w).len())
                    .max()
                    .unwrap_or(0),
                (Ctl::Choice { descs, .. }, Val::Choice { variants, .. }) => variants
                    .iter()
                    .zip(descs.iter())
                    .map(|(c, d)| wrap(&assets, &format!("{c}: {d}"), max_w).len())
                    .max()
                    .unwrap_or(0),
                _ => 0,
            };
            if head + choice > budget {
                over.push(format!(
                    "{} ({}): {head} + {choice} lines",
                    spec.label, spec.cfg_path
                ));
            }
        }
        assert!(
            over.is_empty(),
            "{} option texts overflow the {budget}-line box ({:.0} px wide):\n{}",
            over.len(),
            max_w,
            over.join("\n")
        );
    }

    #[test]
    fn session_pins_lock_their_rows() {
        let specs = settings::registry();
        let mut st = MenuState::new(&specs);
        let at = |path: &str| specs.iter().position(|s| s.cfg_path == path).unwrap();
        let fog = at("render.preference.fog_distance");
        let volume = at("audio.music_volume");
        assert!(row_live(&st, &specs, fog), "free outside netplay");
        st.pin_session_rules(&specs);
        assert!(!row_live(&st, &specs, fog), "a fairness pin locks");
        assert!(row_live(&st, &specs, volume), "a preference stays live");
        let presets: Vec<_> = specs
            .iter()
            .enumerate()
            .filter_map(|(i, s)| settings::preset_group(s.cfg_path).map(|g| (i, g)))
            .collect();
        assert!(!presets.is_empty());
        for (i, g) in presets {
            let pinned = g.members.iter().any(|m| crate::rules::locked_in_game(m));
            assert_eq!(!row_live(&st, &specs, i), pinned, "{}", g.cfg_path);
        }
    }
}
