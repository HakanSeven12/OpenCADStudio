//! RGB-to-AutoCAD color mapping and material-layer utilities.

use crate::document::CadDocument;
use crate::tables::{Layer, TableEntry};
use crate::types::Color;

/// The standard AutoCAD Color Index (ACI) palette — 255 entries.
///
/// Each entry is `(r, g, b)`.  Index 0 is unused (ByBlock); indices 1–255 are
/// the standard palette.  We store them 0-indexed here, so ACI colour *n* is
/// at `ACI_PALETTE[n - 1]`.
static ACI_PALETTE: [(u8, u8, u8); 255] = [
    // 1-9: primary/secondary
    (255, 0, 0),     // 1 red
    (255, 255, 0),   // 2 yellow
    (0, 255, 0),     // 3 green
    (0, 255, 255),   // 4 cyan
    (0, 0, 255),     // 5 blue
    (255, 0, 255),   // 6 magenta
    (255, 255, 255), // 7 white
    (128, 128, 128), // 8 dark grey
    (192, 192, 192), // 9 light grey
    // 10-249: standard palette (sampled from AutoCAD)
    (255, 0, 0),     // 10
    (255, 127, 127), // 11
    (165, 0, 0),     // 12
    (165, 82, 82),   // 13
    (127, 0, 0),     // 14
    (127, 63, 63),   // 15
    (76, 0, 0),      // 16
    (76, 38, 38),    // 17
    (38, 0, 0),      // 18
    (38, 19, 19),    // 19
    (255, 63, 0),    // 20
    (255, 159, 127), // 21
    (165, 41, 0),    // 22
    (165, 103, 82),  // 23
    (127, 31, 0),    // 24
    (127, 79, 63),   // 25
    (76, 19, 0),     // 26
    (76, 47, 38),    // 27
    (38, 9, 0),      // 28
    (38, 23, 19),    // 29
    (255, 127, 0),   // 30
    (255, 191, 127), // 31
    (165, 82, 0),    // 32
    (165, 124, 82),  // 33
    (127, 63, 0),    // 34
    (127, 95, 63),   // 35
    (76, 38, 0),     // 36
    (76, 57, 38),    // 37
    (38, 19, 0),     // 38
    (38, 28, 19),    // 39
    (255, 191, 0),   // 40
    (255, 223, 127), // 41
    (165, 124, 0),   // 42
    (165, 145, 82),  // 43
    (127, 95, 0),    // 44
    (127, 111, 63),  // 45
    (76, 57, 0),     // 46
    (76, 66, 38),    // 47
    (38, 28, 0),     // 48
    (38, 33, 19),    // 49
    (255, 255, 0),   // 50
    (255, 255, 127), // 51
    (165, 165, 0),   // 52
    (165, 165, 82),  // 53
    (127, 127, 0),   // 54
    (127, 127, 63),  // 55
    (76, 76, 0),     // 56
    (76, 76, 38),    // 57
    (38, 38, 0),     // 58
    (38, 38, 19),    // 59
    (191, 255, 0),   // 60
    (223, 255, 127), // 61
    (124, 165, 0),   // 62
    (145, 165, 82),  // 63
    (95, 127, 0),    // 64
    (111, 127, 63),  // 65
    (57, 76, 0),     // 66
    (66, 76, 38),    // 67
    (28, 38, 0),     // 68
    (33, 38, 19),    // 69
    (127, 255, 0),   // 70
    (191, 255, 127), // 71
    (82, 165, 0),    // 72
    (124, 165, 82),  // 73
    (63, 127, 0),    // 74
    (95, 127, 63),   // 75
    (38, 76, 0),     // 76
    (57, 76, 38),    // 77
    (19, 38, 0),     // 78
    (28, 38, 19),    // 79
    (63, 255, 0),    // 80
    (159, 255, 127), // 81
    (41, 165, 0),    // 82
    (103, 165, 82),  // 83
    (31, 127, 0),    // 84
    (79, 127, 63),   // 85
    (19, 76, 0),     // 86
    (47, 76, 38),    // 87
    (9, 38, 0),      // 88
    (23, 38, 19),    // 89
    (0, 255, 0),     // 90
    (127, 255, 127), // 91
    (0, 165, 0),     // 92
    (82, 165, 82),   // 93
    (0, 127, 0),     // 94
    (63, 127, 63),   // 95
    (0, 76, 0),      // 96
    (38, 76, 38),    // 97
    (0, 38, 0),      // 98
    (19, 38, 19),    // 99
    (0, 255, 63),    // 100
    (127, 255, 159), // 101
    (0, 165, 41),    // 102
    (82, 165, 103),  // 103
    (0, 127, 31),    // 104
    (63, 127, 79),   // 105
    (0, 76, 19),     // 106
    (38, 76, 47),    // 107
    (0, 38, 9),      // 108
    (19, 38, 23),    // 109
    (0, 255, 127),   // 110
    (127, 255, 191), // 111
    (0, 165, 82),    // 112
    (82, 165, 124),  // 113
    (0, 127, 63),    // 114
    (63, 127, 95),   // 115
    (0, 76, 38),     // 116
    (38, 76, 57),    // 117
    (0, 38, 19),     // 118
    (19, 38, 28),    // 119
    (0, 255, 191),   // 120
    (127, 255, 223), // 121
    (0, 165, 124),   // 122
    (82, 165, 145),  // 123
    (0, 127, 95),    // 124
    (63, 127, 111),  // 125
    (0, 76, 57),     // 126
    (38, 76, 66),    // 127
    (0, 38, 28),     // 128
    (19, 38, 33),    // 129
    (0, 255, 255),   // 130
    (127, 255, 255), // 131
    (0, 165, 165),   // 132
    (82, 165, 165),  // 133
    (0, 127, 127),   // 134
    (63, 127, 127),  // 135
    (0, 76, 76),     // 136
    (38, 76, 76),    // 137
    (0, 38, 38),     // 138
    (19, 38, 38),    // 139
    (0, 191, 255),   // 140
    (127, 223, 255), // 141
    (0, 124, 165),   // 142
    (82, 145, 165),  // 143
    (0, 95, 127),    // 144
    (63, 111, 127),  // 145
    (0, 57, 76),     // 146
    (38, 66, 76),    // 147
    (0, 28, 38),     // 148
    (19, 33, 38),    // 149
    (0, 127, 255),   // 150
    (127, 191, 255), // 151
    (0, 82, 165),    // 152
    (82, 124, 165),  // 153
    (0, 63, 127),    // 154
    (63, 95, 127),   // 155
    (0, 38, 76),     // 156
    (38, 57, 76),    // 157
    (0, 19, 38),     // 158
    (19, 28, 38),    // 159
    (0, 63, 255),    // 160
    (127, 159, 255), // 161
    (0, 41, 165),    // 162
    (82, 103, 165),  // 163
    (0, 31, 127),    // 164
    (63, 79, 127),   // 165
    (0, 19, 76),     // 166
    (38, 47, 76),    // 167
    (0, 9, 38),      // 168
    (19, 23, 38),    // 169
    (0, 0, 255),     // 170
    (127, 127, 255), // 171
    (0, 0, 165),     // 172
    (82, 82, 165),   // 173
    (0, 0, 127),     // 174
    (63, 63, 127),   // 175
    (0, 0, 76),      // 176
    (38, 38, 76),    // 177
    (0, 0, 38),      // 178
    (19, 19, 38),    // 179
    (63, 0, 255),    // 180
    (159, 127, 255), // 181
    (41, 0, 165),    // 182
    (103, 82, 165),  // 183
    (31, 0, 127),    // 184
    (79, 63, 127),   // 185
    (19, 0, 76),     // 186
    (47, 38, 76),    // 187
    (9, 0, 38),      // 188
    (23, 19, 38),    // 189
    (127, 0, 255),   // 190
    (191, 127, 255), // 191
    (82, 0, 165),    // 192
    (124, 82, 165),  // 193
    (63, 0, 127),    // 194
    (95, 63, 127),   // 195
    (38, 0, 76),     // 196
    (57, 38, 76),    // 197
    (19, 0, 38),     // 198
    (28, 19, 38),    // 199
    (191, 0, 255),   // 200
    (223, 127, 255), // 201
    (124, 0, 165),   // 202
    (145, 82, 165),  // 203
    (95, 0, 127),    // 204
    (111, 63, 127),  // 205
    (57, 0, 76),     // 206
    (66, 38, 76),    // 207
    (28, 0, 38),     // 208
    (33, 19, 38),    // 209
    (255, 0, 255),   // 210
    (255, 127, 255), // 211
    (165, 0, 165),   // 212
    (165, 82, 165),  // 213
    (127, 0, 127),   // 214
    (127, 63, 127),  // 215
    (76, 0, 76),     // 216
    (76, 38, 76),    // 217
    (38, 0, 38),     // 218
    (38, 19, 38),    // 219
    (255, 0, 191),   // 220
    (255, 127, 223), // 221
    (165, 0, 124),   // 222
    (165, 82, 145),  // 223
    (127, 0, 95),    // 224
    (127, 63, 111),  // 225
    (76, 0, 57),     // 226
    (76, 38, 66),    // 227
    (38, 0, 28),     // 228
    (38, 19, 33),    // 229
    (255, 0, 127),   // 230
    (255, 127, 191), // 231
    (165, 0, 82),    // 232
    (165, 82, 124),  // 233
    (127, 0, 63),    // 234
    (127, 63, 95),   // 235
    (76, 0, 38),     // 236
    (76, 38, 57),    // 237
    (38, 0, 19),     // 238
    (38, 19, 28),    // 239
    (255, 0, 63),    // 240
    (255, 127, 159), // 241
    (165, 0, 41),    // 242
    (165, 82, 103),  // 243
    (127, 0, 31),    // 244
    (127, 63, 79),   // 245
    (76, 0, 19),     // 246
    (76, 38, 47),    // 247
    (38, 0, 9),      // 248
    (38, 19, 23),    // 249
    // 250-255: shades of grey
    (51, 51, 51),    // 250
    (91, 91, 91),    // 251
    (132, 132, 132), // 252
    (173, 173, 173), // 253
    (214, 214, 214), // 254
    (255, 255, 255), // 255
];

/// Map an RGB colour to the nearest AutoCAD Color Index (ACI 1–255).
///
/// Uses Euclidean distance in RGB space.  If you need true-colour fidelity,
/// use [`Color::Rgb`] directly instead.
pub fn rgb_to_aci(r: u8, g: u8, b: u8) -> Color {
    let mut best_idx: u8 = 7; // default to white
    let mut best_dist = u32::MAX;

    for (i, &(pr, pg, pb)) in ACI_PALETTE.iter().enumerate() {
        let dr = r as i32 - pr as i32;
        let dg = g as i32 - pg as i32;
        let db = b as i32 - pb as i32;
        let dist = (dr * dr + dg * dg + db * db) as u32;
        if dist < best_dist {
            best_dist = dist;
            best_idx = (i + 1) as u8; // ACI is 1-based
            if dist == 0 {
                break;
            }
        }
    }

    Color::Index(best_idx)
}

/// Map an RGB colour to a [`Color`], preferring true colour (`Rgb`) when the
/// DXF version supports it (R2004+), otherwise falling back to the nearest ACI.
pub fn rgb_to_color(r: u8, g: u8, b: u8, use_true_color: bool) -> Color {
    if use_true_color {
        Color::Rgb { r, g, b }
    } else {
        rgb_to_aci(r, g, b)
    }
}

/// Create a named layer in the document with the given colour.
///
/// If a layer with the same name already exists it is left unchanged and the
/// existing name is returned.  The `prefix` is prepended with an underscore
/// separator unless it is empty.
pub fn create_material_layer(
    doc: &mut CadDocument,
    prefix: &str,
    material_name: &str,
    color: Color,
) -> String {
    let layer_name = if prefix.is_empty() {
        sanitize_layer_name(material_name)
    } else {
        format!("{}_{}", prefix, sanitize_layer_name(material_name))
    };

    if doc.layers.get(&layer_name).is_none() {
        let mut layer = Layer::with_color(&layer_name, color);
        layer.set_handle(doc.allocate_handle());
        // Ignore error if layer already exists (race with case-insensitive check)
        let _ = doc.layers.add(layer);
    }

    layer_name
}

/// Sanitize a string for use as a DXF layer name.
///
/// Replaces characters that are invalid in AutoCAD layer names with `_`.
fn sanitize_layer_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | ',' | '=' | '`' => {
                out.push('_');
            }
            c if c.is_ascii_control() => {
                out.push('_');
            }
            c => out.push(c),
        }
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_aci_exact_red() {
        assert_eq!(rgb_to_aci(255, 0, 0), Color::Index(1));
    }

    #[test]
    fn test_rgb_to_aci_exact_yellow() {
        assert_eq!(rgb_to_aci(255, 255, 0), Color::Index(2));
    }

    #[test]
    fn test_rgb_to_aci_exact_white() {
        assert_eq!(rgb_to_aci(255, 255, 255), Color::Index(7));
    }

    #[test]
    fn test_rgb_to_aci_near_red() {
        // Should map to red (index 1)
        assert_eq!(rgb_to_aci(250, 5, 5), Color::Index(1));
    }

    #[test]
    fn test_sanitize_layer_name() {
        assert_eq!(sanitize_layer_name("Metal/Chrome"), "Metal_Chrome");
        assert_eq!(sanitize_layer_name("wood:oak"), "wood_oak");
        assert_eq!(sanitize_layer_name(""), "unnamed");
    }

    #[test]
    fn test_rgb_to_color_true_color() {
        assert_eq!(
            rgb_to_color(123, 45, 67, true),
            Color::Rgb {
                r: 123,
                g: 45,
                b: 67
            }
        );
    }

    #[test]
    fn test_rgb_to_color_aci_fallback() {
        let c = rgb_to_color(255, 0, 0, false);
        assert_eq!(c, Color::Index(1));
    }
}
