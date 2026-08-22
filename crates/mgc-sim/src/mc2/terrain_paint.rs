//! MC2 terrain paint machinery — the texture-band painter and the
//! MC2-native retile/blend/shade pass. Everything here is a verbatim
//! port from remc2 (`reference/remc2/remc2/engine/Terrain.cpp` unless
//! noted); the tables are extracted programmatically from the
//! decompilation source.
//!
//! - [`CORNER_CLASSES_MC2`] = `unk_D47E0` (Unk_D47E0.cpp:3): 148
//!   textures x 4 corner classes. MC2's `sub_44580` (Terrain.cpp:1011)
//!   generates the runtime blend table `building_F2CD0x` from it with
//!   the SAME 8-dihedral-arrangement insertion MC1's `sub_32560` uses
//!   (all 8 permutation keys and orientation codes match
//!   [`crate::mc1::corners::arrangements`] exactly, verified line by
//!   line) — so the MC2 table is [`crate::mc1::corners::retile_table_for`]
//!   over this data, computed at world construction. It fills
//!   `Gen::retile` on MC2 worlds; the level file does NOT carry it
//!   (Level.cpp:231/364 is savegame serialization of the generated
//!   table).
//! - [`UNK_D4A30`] = `unk_D4A30` (Unk_D4A30.cpp:3): the texture-band
//!   bank, 144 x {terrain texture index, angle rotation nibble}.
//! - `classify_slope` = `sub_45BE0` (Terrain.cpp:1630): 2x2 heightmap
//!   quad -> slope-corner class 0..7 + the low-relief flag.
//! - `Gen::mc2_paint_cell` = `sub_45DC0` (Terrain.cpp:1783): the
//!   footprint paint-code interpreter (codes >= 8 = texture bands via
//!   UNK_D4A30, codes < 8 = blend-class nibble + retile).
//! - `Gen::mc2_retile_region` = `sub_462A0` (Terrain.cpp:1931): the
//!   MC2-native village-fill + blend + shade pass. Structurally MC1's
//!   `recompute_protected`/`retile_and_shade` with the MC2 blend table
//!   and the night/cave shading inversion (Terrain.cpp:2030-2033) —
//!   kept as its own handler per the five-tier taxonomy (no `if mc2`
//!   inside a handler).
//! - `Gen::mc2_pad_edge_ring` = `sub_48A20` (EventsFunctions.cpp:32348)
//!   plus `SetHeightmapByBuilding_48B90` (:32475): the pad-edge
//!   heightmap smoothing bands the building park state runs.
//!
//! The cave second-heightmap maintenance inside sub_45DC0
//! (:1875-1913), sub_462A0/46570 (:2034-2042), sub_46180
//! (EF:31061-31071) and 48B90 (EF:32531-32542) is live — each write
//! re-asserts the floor↔ceiling invariant ([`Gen::cave_seal_fixup`])
//! where retail's non-cave arm blind-clears bit3.

use crate::engine::features::{Gen, tile};

pub const CORNER_CLASSES_MC2: [[u8; 4]; 148] = [
    [0x00, 0x00, 0x00, 0x00], // 0
    [0x01, 0x01, 0x01, 0x01], // 1
    [0x02, 0x02, 0x02, 0x02], // 2
    [0x03, 0x03, 0x03, 0x03], // 3
    [0x04, 0x04, 0x04, 0x04], // 4
    [0x05, 0x05, 0x05, 0x05], // 5
    [0x06, 0x06, 0x06, 0x06], // 6
    [0xff, 0xff, 0xff, 0xff], // 7
    [0xff, 0xff, 0xff, 0xff], // 8
    [0xff, 0xff, 0xff, 0xff], // 9
    [0xff, 0xff, 0xff, 0xff], // 10
    [0xff, 0xff, 0xff, 0xff], // 11
    [0xff, 0xff, 0xff, 0xff], // 12
    [0xff, 0xff, 0xff, 0xff], // 13
    [0xff, 0xff, 0xff, 0xff], // 14
    [0xff, 0xff, 0xff, 0xff], // 15
    [0xff, 0xff, 0xff, 0xff], // 16
    [0xff, 0xff, 0xff, 0xff], // 17
    [0xff, 0xff, 0xff, 0xff], // 18
    [0xff, 0xff, 0xff, 0xff], // 19
    [0xff, 0xff, 0xff, 0xff], // 20
    [0xff, 0xff, 0xff, 0xff], // 21
    [0xff, 0xff, 0xff, 0xff], // 22
    [0xff, 0xff, 0xff, 0xff], // 23
    [0xff, 0xff, 0xff, 0xff], // 24
    [0xff, 0xff, 0xff, 0xff], // 25
    [0xff, 0xff, 0xff, 0xff], // 26
    [0xff, 0xff, 0xff, 0xff], // 27
    [0xff, 0xff, 0xff, 0xff], // 28
    [0xff, 0xff, 0xff, 0xff], // 29
    [0xff, 0xff, 0xff, 0xff], // 30
    [0xff, 0xff, 0xff, 0xff], // 31
    [0xff, 0xff, 0xff, 0xff], // 32
    [0xff, 0xff, 0xff, 0xff], // 33
    [0xff, 0xff, 0xff, 0xff], // 34
    [0x06, 0x00, 0x01, 0x04], // 35
    [0x01, 0x01, 0x00, 0x00], // 36
    [0x01, 0x00, 0x00, 0x00], // 37
    [0x01, 0x00, 0x01, 0x00], // 38
    [0x00, 0x01, 0x01, 0x01], // 39
    [0x06, 0x06, 0x04, 0x04], // 40
    [0x06, 0x04, 0x06, 0x04], // 41
    [0x06, 0x04, 0x06, 0x06], // 42
    [0x04, 0x06, 0x04, 0x04], // 43
    [0x04, 0x04, 0x00, 0x00], // 44
    [0x04, 0x00, 0x00, 0x00], // 45
    [0x00, 0x04, 0x04, 0x04], // 46
    [0x00, 0x04, 0x00, 0x04], // 47
    [0x01, 0x03, 0x03, 0x03], // 48
    [0x01, 0x03, 0x01, 0x03], // 49
    [0x03, 0x01, 0x01, 0x01], // 50
    [0x01, 0x01, 0x03, 0x03], // 51
    [0x05, 0x01, 0x01, 0x01], // 52
    [0x01, 0x01, 0x05, 0x05], // 53
    [0x01, 0x05, 0x01, 0x05], // 54
    [0x01, 0x05, 0x05, 0x05], // 55
    [0x02, 0x05, 0x02, 0x05], // 56
    [0x05, 0x02, 0x02, 0x02], // 57
    [0x02, 0x05, 0x05, 0x05], // 58
    [0x05, 0x05, 0x02, 0x02], // 59
    [0x04, 0x04, 0x03, 0x03], // 60
    [0x04, 0x03, 0x03, 0x03], // 61
    [0x03, 0x04, 0x03, 0x04], // 62
    [0x03, 0x04, 0x04, 0x04], // 63
    [0x04, 0x05, 0x05, 0x05], // 64
    [0x05, 0x04, 0x04, 0x04], // 65
    [0x05, 0x04, 0x05, 0x04], // 66
    [0x04, 0x04, 0x05, 0x05], // 67
    [0x01, 0x02, 0x01, 0x02], // 68
    [0x02, 0x01, 0x01, 0x01], // 69
    [0x01, 0x02, 0x02, 0x02], // 70
    [0x01, 0x01, 0x02, 0x02], // 71
    [0x04, 0x01, 0x01, 0x01], // 72
    [0x01, 0x04, 0x01, 0x04], // 73
    [0x01, 0x04, 0x04, 0x04], // 74
    [0x01, 0x01, 0x04, 0x04], // 75
    [0x01, 0x06, 0x01, 0x01], // 76
    [0x06, 0x06, 0x01, 0x01], // 77
    [0x06, 0x01, 0x06, 0x01], // 78
    [0x06, 0x01, 0x06, 0x06], // 79
    [0x06, 0x06, 0x00, 0x00], // 80
    [0x06, 0x00, 0x06, 0x00], // 81
    [0x06, 0x00, 0x06, 0x06], // 82
    [0x00, 0x06, 0x00, 0x00], // 83
    [0x02, 0x01, 0x05, 0x01], // 84
    [0x01, 0x01, 0x05, 0x02], // 85
    [0x05, 0x01, 0x05, 0x02], // 86
    [0x02, 0x01, 0x02, 0x05], // 87
    [0x02, 0x02, 0x01, 0x05], // 88
    [0x05, 0x05, 0x01, 0x02], // 89
    [0x03, 0x03, 0x04, 0x01], // 90
    [0x04, 0x03, 0x04, 0x01], // 91
    [0x01, 0x01, 0x04, 0x03], // 92
    [0x01, 0x04, 0x04, 0x03], // 93
    [0x03, 0x04, 0x03, 0x01], // 94
    [0x01, 0x03, 0x01, 0x04], // 95
    [0x01, 0x06, 0x04, 0x06], // 96
    [0x01, 0x06, 0x01, 0x04], // 97
    [0x01, 0x06, 0x06, 0x04], // 98
    [0x01, 0x04, 0x06, 0x04], // 99
    [0x01, 0x06, 0x04, 0x01], // 100
    [0x01, 0x06, 0x04, 0x04], // 101
    [0x06, 0x04, 0x00, 0x04], // 102
    [0x00, 0x04, 0x06, 0x06], // 103
    [0x00, 0x04, 0x00, 0x06], // 104
    [0x00, 0x00, 0x04, 0x06], // 105
    [0x00, 0x06, 0x04, 0x04], // 106
    [0x06, 0x00, 0x06, 0x04], // 107
    [0x06, 0x00, 0x06, 0x01], // 108
    [0x01, 0x00, 0x06, 0x00], // 109
    [0x01, 0x06, 0x00, 0x00], // 110
    [0x01, 0x06, 0x06, 0x00], // 111
    [0x01, 0x06, 0x01, 0x00], // 112
    [0x01, 0x01, 0x00, 0x06], // 113
    [0x01, 0x00, 0x04, 0x00], // 114
    [0x01, 0x04, 0x00, 0x04], // 115
    [0x01, 0x04, 0x00, 0x00], // 116
    [0x01, 0x01, 0x04, 0x00], // 117
    [0x04, 0x01, 0x00, 0x04], // 118
    [0x01, 0x04, 0x01, 0x00], // 119
    [0x01, 0x05, 0x05, 0x04], // 120
    [0x04, 0x05, 0x04, 0x01], // 121
    [0x01, 0x01, 0x04, 0x05], // 122
    [0x01, 0x05, 0x04, 0x05], // 123
    [0x01, 0x04, 0x01, 0x05], // 124
    [0x01, 0x04, 0x04, 0x05], // 125
    [0x01, 0x06, 0x00, 0x04], // 126
    [0x06, 0x01, 0x00, 0x04], // 127
    [0x06, 0x06, 0x05, 0x05], // 128
    [0x06, 0x05, 0x06, 0x05], // 129
    [0x06, 0x05, 0x06, 0x06], // 130
    [0x05, 0x06, 0x05, 0x05], // 131
    [0x06, 0x06, 0x03, 0x03], // 132
    [0x06, 0x03, 0x06, 0x03], // 133
    [0x06, 0x03, 0x06, 0x06], // 134
    [0x03, 0x06, 0x03, 0x03], // 135
    [0x01, 0x05, 0x05, 0x06], // 136
    [0x06, 0x05, 0x06, 0x01], // 137
    [0x01, 0x01, 0x06, 0x05], // 138
    [0x01, 0x05, 0x06, 0x05], // 139
    [0x01, 0x06, 0x01, 0x05], // 140
    [0x01, 0x06, 0x06, 0x05], // 141
    [0x01, 0x03, 0x03, 0x06], // 142
    [0x06, 0x03, 0x06, 0x01], // 143
    [0x01, 0x01, 0x06, 0x03], // 144
    [0x01, 0x03, 0x06, 0x03], // 145
    [0x01, 0x06, 0x01, 0x03], // 146
    [0x01, 0x06, 0x06, 0x03], // 147
];

pub const UNK_D4A30: [u8; 0x120] = [
    0x1b, 0x00, 0x1b, 0x50, 0x1b, 0x30, 0x1b, 0x60, 0x1a, 0x00, 0x1a, 0x50, 0x1a, 0x30, 0x1a,
    0x60, // 0x00
    0x0a, 0x00, 0x0a, 0x50, 0x0a, 0x30, 0x0a, 0x60, 0x0a, 0x00, 0x0a, 0x50, 0x0a, 0x30, 0x0a,
    0x60, // 0x10
    0x0b, 0x00, 0x0b, 0x50, 0x0b, 0x30, 0x0b, 0x60, 0x0b, 0x00, 0x0b, 0x50, 0x0b, 0x30, 0x0b,
    0x60, // 0x20
    0x0c, 0x00, 0x0c, 0x50, 0x0c, 0x30, 0x0c, 0x60, 0x0c, 0x00, 0x0c, 0x50, 0x0c, 0x30, 0x0c,
    0x60, // 0x30
    0x15, 0x00, 0x15, 0x50, 0x15, 0x30, 0x15, 0x60, 0x16, 0x00, 0x16, 0x50, 0x16, 0x30, 0x16,
    0x60, // 0x40
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00, 0x18, 0x50, 0x18, 0x30, 0x18,
    0x60, // 0x50
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x17, 0x00, 0x17, 0x50, 0x17, 0x30, 0x17,
    0x60, // 0x60
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x19, 0x00, 0x19, 0x50, 0x19, 0x30, 0x19,
    0x60, // 0x70
    0x10, 0x00, 0x10, 0x50, 0x10, 0x30, 0x10, 0x60, 0x0f, 0x00, 0x0f, 0x50, 0x0f, 0x30, 0x0f,
    0x60, // 0x80
    0x10, 0x00, 0x10, 0x50, 0x10, 0x30, 0x10, 0x60, 0x0f, 0x00, 0x0f, 0x50, 0x0f, 0x30, 0x0f,
    0x60, // 0x90
    0x1e, 0x00, 0x1e, 0x50, 0x1e, 0x30, 0x1d, 0x60, 0x1f, 0x00, 0x1d, 0x50, 0x1d, 0x30, 0x1d,
    0x60, // 0xa0
    0x1e, 0x00, 0x1e, 0x50, 0x1e, 0x30, 0x1e, 0x60, 0x1d, 0x00, 0x1d, 0x50, 0x1d, 0x30, 0x1d,
    0x60, // 0xb0
    0x21, 0x00, 0x21, 0x50, 0x21, 0x30, 0x21, 0x60, 0x22, 0x00, 0x20, 0x50, 0x20, 0x30, 0x20,
    0x60, // 0xc0
    0x21, 0x00, 0x21, 0x50, 0x21, 0x30, 0x21, 0x60, 0x20, 0x00, 0x20, 0x50, 0x20, 0x30, 0x20,
    0x60, // 0xd0
    0x13, 0x00, 0x13, 0x50, 0x13, 0x30, 0x13, 0x60, 0x14, 0x00, 0x12, 0x50, 0x12, 0x30, 0x12,
    0x60, // 0xe0
    0x13, 0x00, 0x13, 0x50, 0x13, 0x30, 0x13, 0x60, 0x12, 0x00, 0x12, 0x50, 0x12, 0x30, 0x12,
    0x60, // 0xf0
    0x13, 0x00, 0x13, 0x50, 0x13, 0x30, 0x13, 0x60, 0x14, 0x00, 0x12, 0x50, 0x12, 0x30, 0x12,
    0x60, // 0x100
    0x21, 0x00, 0x21, 0x50, 0x21, 0x30, 0x21, 0x60, 0x20, 0x00, 0x20, 0x50, 0x20, 0x30, 0x20,
    0x60, // 0x110
];

/// The MC2 runtime blend table (`building_F2CD0x`): `sub_44580`'s
/// generation collapsed through the shared bucket machinery
/// (first candidate per base-7 corner key, `[1, 0]` default —
/// Terrain.cpp:1093-1117). Pure static data; computed once.
pub fn retile_table_mc2() -> Vec<[u8; 2]> {
    crate::mc1::corners::retile_table_for(&CORNER_CLASSES_MC2)
}

/// `sub_45BE0` (Terrain.cpp:1630): classify the cell's 2x2 heightmap
/// quad into a slope-corner class. Returns `(class 0..7, low_diff)`
/// — `low_diff` is the original's `lowDiffHeightmap_D47DC` global
/// (quad relief <= 8), which codes 0x0A..0x0E consume as a +8 row
/// select. `in_type` seeds the second-corner register exactly like
/// the original's in-out argument.
fn classify_slope(height: &[u8], mut in_type: u8, cx: u8, cy: u8) -> (u8, bool) {
    let q = [
        height[tile(cx, cy)],
        height[tile(cx.wrapping_add(1), cy)],
        height[tile(cx.wrapping_add(1), cy.wrapping_add(1))],
        height[tile(cx, cy.wrapping_add(1))],
    ];
    let mut ty = 0u8;
    let mut min_h = 255u8;
    let mut max_h = 0u8;
    if q[0] != 0 {
        max_h = q[0];
        ty = 0;
    }
    if q[0] < 255 {
        min_h = q[0];
    }
    for (i, &h) in q.iter().enumerate().skip(1) {
        if h > max_h {
            max_h = h;
            ty = i as u8;
        }
        if h < min_h {
            min_h = h;
        }
    }
    // Second-highest corner, excluding `ty`'s — with the original's
    // per-corner quirks kept (corner 0 requires nonzero, corner 3
    // compares strictly greater).
    let mut max2 = 0u8;
    if ty != 0 && q[0] != 0 {
        max2 = q[0];
        in_type = 0;
    }
    if ty != 1 && q[1] > max2 {
        max2 = q[1];
        in_type = 1;
    }
    if ty != 2 && q[2] > max2 {
        max2 = q[2];
        in_type = 2;
    }
    if ty != 3 && q[3] > max2 {
        max2 = q[3];
        in_type = 3;
    }
    let low_diff = max_h - min_h <= 8;
    if max_h as i32 - max2 as i32 >= 8 {
        return (ty, low_diff);
    }
    let class = match ty {
        0 => {
            if in_type != 1 {
                7
            } else {
                4
            }
        }
        1 => {
            if in_type == 2 {
                5
            } else {
                4
            }
        }
        2 => {
            if in_type == 3 {
                6
            } else {
                5
            }
        }
        3 => {
            if in_type != 0 {
                6
            } else {
                7
            }
        }
        _ => 0,
    };
    (class, low_diff)
}

impl Gen {
    /// `sub_45DC0` (Terrain.cpp:1783): interpret one footprint paint
    /// code at a cell. Codes >= 8 select a texture-band family from
    /// [`UNK_D4A30`] (slope-variant row via `classify_slope`), lock
    /// the cell against village repaint (angle bit 7), and clear the
    /// cave bit over the quad; codes < 8 write the blend-class nibble
    /// and resolve through [`Gen::mc2_retile_region`]. `in_type` = the
    /// caller's column counter (the original passes the footprint
    /// column; the groove-castle path passes 7).
    pub(crate) fn mc2_paint_cell(&mut self, in_type: u8, cx: u8, cy: u8, code: u8) {
        let t = tile(cx, cy);
        if code < 8 {
            self.t.angle[t] = code | (self.t.angle[t] & 0xF0);
            self.mc2_retile_region(cx, cy, cx, cy);
            return;
        }
        // (family base into UNK_D4A30, class offset, +8 on low_diff)
        let band = |g: &mut Gen, base: usize, extra: u8, low_diff_rows: bool| {
            let (class, low_diff) = classify_slope(&g.t.height, in_type, cx, cy);
            let mut row = class + extra;
            if low_diff_rows && low_diff {
                row += 8;
            }
            g.t.tile_type[t] = UNK_D4A30[base + 2 * row as usize];
            g.t.angle[t] = (g.t.angle[t] & 0x8F) | UNK_D4A30[base + 2 * row as usize + 1];
        };
        match code {
            0x08 => self.t.tile_type[t] = 8,
            0x09 => self.t.tile_type[t] = 9,
            0x0A => band(self, 0x80, 0x00, true),
            0x0B => band(self, 0x80, 0x10, true),
            0x0C => band(self, 0x80, 0x20, true),
            0x0D => band(self, 0x80, 0x30, true),
            0x0E => band(self, 0x80, 0x40, true),
            0x0F => self.t.tile_type[t] = 11,
            0x10 => {
                // Roads/water types 10..12 win over the band (:1835).
                if !matches!(self.t.tile_type[t], 10..=12) {
                    band(self, 0x00, 0, false);
                }
            }
            0x11 => band(self, 0x40, 0, false),
            0x12 => band(self, 0x50, 8 * (cx.wrapping_add(cy) & 1), false),
            0x13 => band(self, 0x60, 8 * (cx.wrapping_add(cy) & 1), false),
            0x14 => band(self, 0x10, 0, false),
            0x15 => band(self, 0x20, 0, false),
            0x16 => band(self, 0x30, 0, false),
            _ => {}
        }
        // Lock the texture, then bit3 over the quad: non-cave clears
        // it (:1914-1923); a cave re-asserts the floor↔ceiling
        // invariant per quad cell instead (:1875-1912 — the early
        // return on the 4th cell's seal path is the same fixup).
        self.t.angle[t] |= 0x80;
        for (qx, qy) in [
            (cx, cy),
            (cx.wrapping_add(1), cy),
            (cx.wrapping_add(1), cy.wrapping_add(1)),
            (cx, cy.wrapping_add(1)),
        ] {
            let q = tile(qx, qy);
            if self.is_cave() {
                self.cave_seal_fixup(q);
            } else {
                self.t.angle[q] &= 0xF7;
            }
        }
    }

    /// `sub_462A0` (Terrain.cpp:1931): the MC2-native retile over the
    /// rect `(ax, ay)..=(bx, by)` — village fill (stencil type 1 onto
    /// each cell + its W/NW/N neighbors where not texture-locked),
    /// blend every type-1 cell of the rect grown one on -x/-y through
    /// the generated `building_F2CD0x` (= `Gen::retile` on MC2
    /// worlds), then shading over the rect grown once more, with the
    /// night/cave inversion (`shade = 64 - shade` when MapType != Day,
    /// :2030-2033) and the cave arm's seal/open invariant (:2034-2042).
    pub(crate) fn mc2_retile_region(&mut self, ax: u8, ay: u8, bx: u8, by: u8) {
        let w = bx.wrapping_sub(ax).wrapping_add(1);
        let h = by.wrapping_sub(ay).wrapping_add(1);
        // Pass 1: village fill (:1945-1965), gated on the texture
        // lock (mapAngle high bit clear).
        let mut cy = ay;
        for _ in 0..h {
            let mut cx = ax;
            for _ in 0..w {
                for (qx, qy) in [
                    (cx, cy),
                    (cx.wrapping_sub(1), cy),
                    (cx.wrapping_sub(1), cy.wrapping_sub(1)),
                    (cx, cy.wrapping_sub(1)),
                ] {
                    let t = tile(qx, qy);
                    if self.t.angle[t] & 0x80 == 0 {
                        self.t.tile_type[t] = 1;
                    }
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
        self.mc2_blend_shade_passes(ax, ay, w, h);
    }

    /// `AddBuildingToTerrain_46570` (EventsFunctions.cpp:31080-31206)
    /// — the terrain-write recompute the dome's height writer
    /// (`sub_570F0`, a4=0) triggers per cell. IDENTICAL to
    /// [`Gen::mc2_retile_region`] except pass A seeds the 2x2 quad's
    /// terrain type UNCONDITIONALLY (no texture-lock gate — the only
    /// behavioral difference, open-closure trace §3.2).
    pub(crate) fn mc2_add_building_region(&mut self, ax: u8, ay: u8, bx: u8, by: u8) {
        let w = bx.wrapping_sub(ax).wrapping_add(1);
        let h = by.wrapping_sub(ay).wrapping_add(1);
        let mut cy = ay;
        for _ in 0..h {
            let mut cx = ax;
            for _ in 0..w {
                for (qx, qy) in [
                    (cx, cy),
                    (cx.wrapping_sub(1), cy),
                    (cx.wrapping_sub(1), cy.wrapping_sub(1)),
                    (cx, cy.wrapping_sub(1)),
                ] {
                    self.t.tile_type[tile(qx, qy)] = 1;
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
        self.mc2_blend_shade_passes(ax, ay, w, h);
    }

    /// Passes B + C shared by `sub_462A0` and
    /// `AddBuildingToTerrain_46570` (byte-identical in retail —
    /// open-closure trace §3.2).
    fn mc2_blend_shade_passes(&mut self, ax: u8, ay: u8, w: u8, h: u8) {
        // Pass 2: blend (:1971-2002) — identical shape to MC1's
        // retile pass; the table differs (and is swapped in at world
        // construction), the LCG is the shared pseudoRand stream.
        let x_add = w.wrapping_add(1);
        let y_add = h.wrapping_add(1);
        let (sx, sy) = (ax.wrapping_sub(1), ay.wrapping_sub(1));
        let mut cy = sy;
        for _ in 0..y_add {
            let mut cx = sx;
            for _ in 0..x_add {
                let t = tile(cx, cy);
                if self.t.tile_type[t] == 1 {
                    let p1 = self.t.angle[t] & 7;
                    let p2 = self.t.angle[tile(cx.wrapping_add(1), cy)] & 7;
                    let p3 = self.t.angle[tile(cx.wrapping_add(1), cy.wrapping_add(1))] & 7;
                    let p4 = self.t.angle[tile(cx, cy.wrapping_add(1))] & 7;
                    let idx = p4 as usize + 7 * p3 as usize + 49 * p2 as usize + 343 * p1 as usize;
                    let [new_type, orient] = self.retile[idx];
                    self.t.tile_type[t] = new_type;
                    self.t.angle[t] = if new_type >= 8 {
                        orient.wrapping_add(self.t.angle[t] & 0x87)
                    } else {
                        self.pseudo = self.pseudo.wrapping_mul(9377).wrapping_add(9439);
                        (self.t.angle[t] & 0x87).wrapping_add(16 * (self.pseudo % 7) as u8)
                    };
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
        // Pass 3: shading (:2006-2048), NW-SE relief with the
        // non-day inversion; clear the cave bit (non-cave arm).
        let mut cy = sy;
        for _ in 0..y_add.wrapping_add(1) {
            let mut cx = sx;
            for _ in 0..x_add.wrapping_add(1) {
                let t = tile(cx, cy);
                let se = self.t.height[tile(cx.wrapping_add(1), cy.wrapping_add(1))];
                let nw = self.t.height[tile(cx.wrapping_sub(1), cy.wrapping_sub(1))];
                let mut s = nw.wrapping_sub(se).wrapping_add(32);
                if (s as i8) < 28 {
                    s = (s & 3) + 28;
                } else if (s as i8) > 40 {
                    s = (s & 7) + 40;
                }
                self.t.shading[t] = if self.mc2_night_shade.0 {
                    64u8.wrapping_sub(s)
                } else {
                    s
                };
                // Cave arm (:2034-2042): seal/open per the invariant
                // instead of the blind bit3 clear.
                if self.is_cave() {
                    self.cave_seal_fixup(t);
                } else {
                    self.t.angle[t] &= 0xF7;
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
    }

    /// `sub_46180` (EventsFunctions.cpp:31007) — the road RIDGE
    /// STAMP: terrain type on the 2x2 quad {(x,y),(x-1,y),(x-1,y-1),
    /// (x,y-1)}, then the NW-SE relief shading over the 3x3 centered
    /// on the cell (identical clamp bands + night inversion to
    /// [`Gen::mc2_retile_region`] pass 3), with the same bit3 law:
    /// clear off-cave, re-assert the invariant on caves
    /// (EF:31061-31071).
    pub(crate) fn mc2_ridge_stamp(&mut self, cx: u8, cy: u8, ty: u8) {
        for (qx, qy) in [
            (cx, cy),
            (cx.wrapping_sub(1), cy),
            (cx.wrapping_sub(1), cy.wrapping_sub(1)),
            (cx, cy.wrapping_sub(1)),
        ] {
            self.t.tile_type[tile(qx, qy)] = ty;
        }
        for dy in 0..3u8 {
            for dx in 0..3u8 {
                let sx = cx.wrapping_sub(1).wrapping_add(dx);
                let sy = cy.wrapping_sub(1).wrapping_add(dy);
                let t = tile(sx, sy);
                let nw = self.t.height[tile(sx.wrapping_sub(1), sy.wrapping_sub(1))];
                let se = self.t.height[tile(sx.wrapping_add(1), sy.wrapping_add(1))];
                let mut s = nw.wrapping_sub(se).wrapping_add(32);
                if (s as i8) < 28 {
                    s = (s & 3) + 28;
                } else if (s as i8) > 40 {
                    s = (s & 7) + 40;
                }
                self.t.shading[t] = if self.mc2_night_shade.0 {
                    64u8.wrapping_sub(s)
                } else {
                    s
                };
                if self.is_cave() {
                    self.cave_seal_fixup(t);
                } else {
                    self.t.angle[t] &= 0xF7;
                }
            }
        }
    }

    /// `sub_33F70` (Terrain.cpp:1744) — the road raise guard: a cell
    /// whose LEFT neighbor already carries type 8 is re-raised only
    /// if some probe exceeds center+30. The x+1 probe re-reads the
    /// CENTER height (a decompile-visible retail quirk — the
    /// comparison is vacuous) — kept faithful. This is what keeps
    /// overlapping road strips from double-stacking +48.
    fn mc2_road_water_ok(&self, cx: u8, cy: u8) -> bool {
        let c = tile(cx, cy);
        let left = tile(cx.wrapping_sub(1), cy);
        if self.t.tile_type[left] != 8 {
            return true;
        }
        let comp = self.t.height[c] as i32 + 30;
        if self.t.height[left] as i32 > comp {
            return true;
        }
        // The vacuous center re-read (x+1 probe) can never exceed
        // comp; the two Y probes follow, in retail's order.
        let down = tile(cx, cy.wrapping_add(1));
        if self.t.height[down] as i32 > comp {
            return true;
        }
        let up = tile(cx, cy.wrapping_sub(1));
        self.t.height[up] as i32 > comp
    }

    /// One road cell: the guarded +48 raise (u8-wrapping, verbatim)
    /// + the ridge stamp with type 8.
    fn mc2_road_cell(&mut self, cx: u8, cy: u8) {
        let t = tile(cx, cy);
        if self.t.tile_type[t] != 8 || self.mc2_road_water_ok(cx, cy) {
            self.t.height[t] = self.t.height[t].wrapping_add(48);
        }
        self.mc2_ridge_stamp(cx, cy, 8);
    }

    /// The (10,27) Y-strip walkers — action 28 `sub_34000`
    /// (EF:24863, adv >= 0: parity-snap X to even, +Y advance) and
    /// action 27 `sub_34110` (EF:24897, adv < 0: no snap, start at
    /// ty+2, -Y advance). Rows = width(2) + |run|; per row the left/
    /// right border cells get `mapAngle |= 0x80` (authored-locked).
    /// One-shot in retail (settle-ticked then despawned) — collapsed
    /// to a synchronous stamp like the waterpath's (10,30) segments
    /// (no RNG anywhere in the family).
    fn mc2_road_strip_y(&mut self, tx: u8, ty: u8, step: i32, rem: i32) {
        const W: u8 = 2; // (10,27) ctor life = strip width (EF:36156)
        // Retail picks the walker family from the STEP sign alone
        // (EV:5423-33 / 5468-77, v20/v12 — never the combined
        // advance); the first-step remainder folds into the SIGNED
        // row count. A degenerate step==0 && rem<0 leg stays in the
        // +Y snap family with FEWER rows — it does not flip to the
        // −Y walker.
        let (x0, mut cy, dir, rows) = if step >= 0 {
            let mut x0 = tx;
            if x0 & 1 == 1 {
                x0 = x0.wrapping_add(1);
            }
            (x0.wrapping_sub(W - 1), ty, 1i8, W as i32 + (step + rem))
        } else {
            (tx, ty.wrapping_add(2), -1i8, W as i32 + (-step - rem))
        };
        for _ in 0..rows {
            self.t.angle[tile(x0.wrapping_sub(1), cy)] |= 0x80;
            for k in 0..W {
                self.mc2_road_cell(x0.wrapping_add(k), cy);
            }
            self.t.angle[tile(x0.wrapping_add(W), cy)] |= 0x80;
            cy = cy.wrapping_add(dir as u8);
        }
    }

    /// The (10,27) X-run walker — action 29 `sub_34210` (EF:24929):
    /// parity-snap X on (x+y), lock the border row above, stamp
    /// width(2) rows of `run` cells in +X, lock the border row
    /// below.
    fn mc2_road_strip_x(&mut self, tx: u8, ty: u8, run: i32) {
        const W: u8 = 2;
        if run <= 0 {
            return;
        }
        let mut x0 = tx;
        if (tx.wrapping_add(ty)) & 1 == 1 {
            x0 = x0.wrapping_add(1);
        }
        for k in 0..run {
            self.t.angle[tile(x0.wrapping_add(k as u8), ty.wrapping_sub(1))] |= 0x80;
        }
        let mut cy = ty;
        for _ in 0..W {
            for k in 0..run {
                self.mc2_road_cell(x0.wrapping_add(k as u8), cy);
            }
            cy = cy.wrapping_add(1);
        }
        for k in 0..run {
            self.t.angle[tile(x0.wrapping_add(k as u8), cy)] |= 0x80;
        }
    }

    /// `sub_48400` (Events.cpp:5365) — the (10,28) ROAD leg: torus-
    /// shortest deltas (`shortestLenght_48370` EV:5753), endpoints
    /// swapped so the walk goes +X, then a coarse Bresenham of
    /// ~|major|/10 + 1 steps, each laying a Y strip and an X run
    /// (the axis order swaps between the Y-major and X-major
    /// branches; remainders fold into the first step only). The
    /// chain style byte passes through unused (EV:5427). Trace:
    /// docs/traces/mc2-terrain-author-painters.md §1-2.
    pub(crate) fn mc2_stamp_road_leg(&mut self, x1: u16, y1: u16, x2: u16, y2: u16) {
        let short = |a: u16, b: u16| -> i32 {
            let mut d = b as i32 - a as i32;
            if d > 128 {
                d -= 256;
            }
            if d < -128 {
                d += 256;
            }
            d
        };
        let mut dx = short(x1, x2);
        let mut dy = short(y1, y2);
        if dx == 0 && dy == 0 {
            return;
        }
        let (mut px, mut py) = (x1 as i32, y1 as i32);
        if dx < 0 {
            px = x2 as i32;
            py = y2 as i32;
            dx = -dx;
            dy = -dy;
        }
        if dx <= dy.abs() {
            // Y-major (EV:5407-5448).
            let steps = (dy / 10).abs() + 1;
            let step_y = dy / steps;
            let mut rem_y = dy - steps * step_y;
            let step_x = dx / steps;
            let mut rem_x = dx - steps * step_x;
            for _ in 0..steps {
                self.mc2_road_strip_y(px as u8, py as u8, step_y, rem_y);
                py += step_y + rem_y;
                let adv_x = rem_x + step_x;
                self.mc2_road_strip_x(px as u8, py as u8, adv_x);
                px += adv_x;
                rem_y = 0;
                rem_x = 0;
            }
        } else {
            // X-major (EV:5450-5487).
            let steps = dx / 10 + 1;
            let step_x = dx / steps;
            let step_y = dy / steps;
            let mut rem_x = dx - steps * step_x;
            let mut rem_y = dy - steps * step_y;
            for _ in 0..steps {
                let adv_x = rem_x + step_x;
                self.mc2_road_strip_x(px as u8, py as u8, adv_x);
                px += adv_x;
                self.mc2_road_strip_y(px as u8, py as u8, step_y, rem_y);
                py += step_y + rem_y;
                rem_x = 0;
                rem_y = 0;
            }
        }
    }

    /// `SetHeightmapByBuilding_48B90` (EventsFunctions.cpp:32475):
    /// smooth one cell's height to the 3x3 neighborhood average,
    /// counting only cells NOT carrying a building texture (types
    /// 6..=0x22), and only when the cell itself has a blend nibble, a
    /// nonzero height, and none of its quad-corner cells are
    /// building-textured. On caves the write re-asserts the invariant
    /// (EF:32531-32542; no off-cave else arm in retail).
    /// Shared by the pad-edge ring and the castle-unstamp finalizer.
    /// Neighbour indexing is retail's PACKED-WORD arithmetic
    /// (`word − 0x101` borrows across the y byte at x==0).
    /// ⚠ RETAIL USES **TWO** CONVENTIONS HERE AND THEY ARE NOT
    /// INTERCHANGEABLE: the four-way GATE passes a signed `int` to
    /// `fix_10B4E0_terraintype` (:32502-11), which pins a negative
    /// index to 0 = natural (:32465-70), while only the 3x3 kernel
    /// runs on a wrapping `uint16 i` (:32514) and is a true torus.
    /// The port modelled BOTH as a torus, so a footprint touching
    /// row 0 bailed on whatever sat at the far edge — mc2l3 t=5643,
    /// see the gate below. Fixed 2026-08-24e; the old text of this
    /// paragraph ("divergence only for footprints touching row 0")
    /// named the defect exactly and left it standing.
    pub(crate) fn mc2_smooth_pad_edge(&mut self, cx: u8, cy: u8) {
        let t = tile(cx, cy) as u16;
        let natural = |g: &Self, idx: u16| {
            let ty = g.t.tile_type[idx as usize];
            ty <= 5 || ty > 0x22
        };
        if self.t.angle[t as usize] & 7 == 0 || self.t.height[t as usize] == 0 {
            return;
        }
        for off in [0x101u16, 0x100, 0x1, 0] {
            // ⭐⭐ THE GATE IS A *SIGNED* READ; ONLY THE 3x3 LOOP IS
            // THE TORUS. `SetHeightmapByBuilding_48B90` hands
            // `-0x101 + axis.word` — an `int` — to
            // `fix_10B4E0_terraintype(int)` (:32502-11), and that
            // helper pins a NEGATIVE index to 0, a natural type
            // (:32465-70). So every cell whose tile word is below the
            // offset — all of row 0, plus (0,1) — passes the gate.
            // The 3x3 average below is the one that genuinely wraps
            // (`uint16 i`, :32514) and stays a torus.
            //
            // Collapsing the two cost mc2l3 t=5643: the (10,45) at
            // tile (198,12) completes, `mc2_pad_edge_ring` reaches
            // (198,0), and its WRAPPED gate cell (197,255) carries
            // type 6 — a building texture — so the port bailed and
            // left the height at 49 where retail smoothed it to 51.
            // Two height bytes are three engine units of
            // `interp_plane` at that quad, and the (9,0) fireball that
            // terrain-contacts there twenty ticks later burst at z
            // 2056 against retail's 2059 — the take's
            // `first=5663 sig=(9,0)slot235:z`.
            if t < off {
                continue;
            }
            if !natural(self, t.wrapping_sub(off)) {
                return;
            }
        }
        let mut sum = 0u32;
        let mut count = 0u32;
        let mut idx = t.wrapping_sub(0x101);
        for _ in 0..3 {
            for _ in 0..3 {
                if natural(self, idx) {
                    count += 1;
                    sum += self.t.height[idx as usize] as u32;
                }
                idx = idx.wrapping_add(1);
            }
            idx = idx.wrapping_add(0xFD);
        }
        if let Some(h) = sum.checked_div(count) {
            self.t.height[t as usize] = h as u8;
            self.cave_seal_fixup(t as usize);
        }
    }

    /// `sub_48A20` (EventsFunctions.cpp:32348): smooth the pad-edge
    /// heightmap in four border bands around the footprint —
    /// left/right verticals (width `thick+1`, `2*half_h` rows), then
    /// top/bottom horizontals (height `thick+1`, `2*half_w + 2*thick`
    /// columns). Offsets kept verbatim (the bands are anchored on the
    /// caller's TOP-LEFT corner minus the half extents — faithful,
    /// including the asymmetry).
    pub(crate) fn mc2_pad_edge_ring(&mut self, x: u8, y: u8, half_h: u8, half_w: u8, thick: u8) {
        let bx = x.wrapping_sub(half_w);
        let by = y.wrapping_sub(half_h);
        // Verticals (:32384-32405).
        let mut cy = by;
        for _ in 0..2 * half_h as u16 {
            let mut lx = bx.wrapping_sub(thick);
            let mut rx = x.wrapping_add(half_w);
            for _ in 0..=thick as u16 {
                self.mc2_smooth_pad_edge(lx, cy);
                self.mc2_smooth_pad_edge(rx, cy);
                lx = lx.wrapping_add(1);
                rx = rx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
        // Horizontals (:32418-32441).
        let mut cx = bx.wrapping_sub(thick);
        for _ in 0..(2 * thick as u16 + 2 * half_w as u16) {
            let mut ty_ = by.wrapping_sub(thick);
            let mut by_ = y.wrapping_add(half_h);
            for _ in 0..=thick as u16 {
                self.mc2_smooth_pad_edge(cx, ty_);
                self.mc2_smooth_pad_edge(cx, by_);
                ty_ = ty_.wrapping_add(1);
                by_ = by_.wrapping_add(1);
            }
            cx = cx.wrapping_add(1);
        }
    }
}
