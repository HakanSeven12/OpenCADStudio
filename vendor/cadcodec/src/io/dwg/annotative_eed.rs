//! Encode / decode the annotative flag carried in `AcadAnnotative` EED.
//!
//! STYLE / DIMSTYLE / TABLESTYLE records have no native annotative field, so
//! AutoCAD stores it as extended data under the `AcadAnnotative` application,
//! exactly mirroring the DXF XDATA form `AnnotativeData { 1 <flag> }`.
//!
//! These functions produce / parse the raw EED data-item byte block (the bytes
//! that follow the `[length][app handle]` header), per the ODA `.dwg`
//! specification, §28 "Extended Entity Data":
//!   * code `0` string — R2007+: 2-byte char count then UTF-16LE; earlier:
//!     1-byte length, 2-byte codepage, then single-byte chars.
//!   * code `2` control — one byte: `0` = `{`, `1` = `}`.
//!   * code `70` short — 2 bytes little-endian.

const ANNOTATIVE_DATA: &str = "AnnotativeData";

/// Build the `AcadAnnotative` EED data-item bytes for `flag`.
/// `wide` selects the R2007+ UTF-16 string encoding.
pub(crate) fn encode(wide: bool, flag: bool) -> Vec<u8> {
    let mut b = Vec::new();

    // code 0: string "AnnotativeData"
    b.push(0);
    if wide {
        let units: Vec<u16> = ANNOTATIVE_DATA.encode_utf16().collect();
        b.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for u in units {
            b.extend_from_slice(&u.to_le_bytes());
        }
    } else {
        b.push(ANNOTATIVE_DATA.len() as u8);
        b.extend_from_slice(&0u16.to_le_bytes()); // codepage
        b.extend_from_slice(ANNOTATIVE_DATA.as_bytes());
    }

    // code 2: '{'
    b.push(2);
    b.push(0);
    // code 70: class version = 1
    b.push(70);
    b.extend_from_slice(&1i16.to_le_bytes());
    // code 70: annotative flag
    b.push(70);
    b.extend_from_slice(&(flag as i16).to_le_bytes());
    // code 2: '}'
    b.push(2);
    b.push(1);

    b
}

/// Walk an `AcadAnnotative` EED data-item block and return its annotative flag
/// (the last 16-bit short, after the version short). Returns `None` if the
/// bytes don't parse as the expected items.
pub(crate) fn decode_flag(bytes: &[u8], wide: bool) -> Option<bool> {
    let mut i = 0usize;
    let mut last_short: Option<i16> = None;
    while i < bytes.len() {
        let code = bytes[i];
        i += 1;
        match code {
            0 => {
                // string
                if wide {
                    let n = u16::from_le_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]) as usize;
                    i += 2 + n * 2;
                } else {
                    let n = *bytes.get(i)? as usize;
                    i += 1 + 2 + n; // length + codepage + chars
                }
            }
            2 => i += 1, // control string
            4 => {
                let n = *bytes.get(i)? as usize;
                i += 1 + n;
            }
            3 | 5 => i += 8,    // layer / entity handle
            10..=13 => i += 24, // points
            40..=42 => i += 8,  // reals
            70 => {
                last_short = Some(i16::from_le_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]));
                i += 2;
            }
            71 => i += 4,
            _ => return None,
        }
        if i > bytes.len() {
            return None;
        }
    }
    last_short.map(|v| v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_wide() {
        assert_eq!(decode_flag(&encode(true, true), true), Some(true));
        assert_eq!(decode_flag(&encode(true, false), true), Some(false));
    }

    #[test]
    fn roundtrip_narrow() {
        assert_eq!(decode_flag(&encode(false, true), false), Some(true));
        assert_eq!(decode_flag(&encode(false, false), false), Some(false));
    }
}
