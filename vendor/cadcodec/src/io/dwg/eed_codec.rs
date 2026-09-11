//! Encode / decode structured XDATA [`records`](crate::xdata::ExtendedData) to
//! and from the DWG EED (Extended Entity Data) byte payload.
//!
//! The DWG object reader keeps EED as opaque `(app_handle, payload_bytes)`
//! blobs (`raw_dwg_eed`), and the writer emits those blobs verbatim. Structured
//! records (the DXF-style group-coded values a plugin attaches via
//! `HostApi::write_record`) had no DWG representation, so they were dropped on
//! save. These functions bridge that gap: `encode_values` turns a record's
//! values into the payload bytes the writer frames with `[BS length][app
//! handle]`, and `decode_values` reverses it so a re-opened file surfaces the
//! same values through `ExtendedData::records`.
//!
//! Payload layout (ODA `.dwg` spec §28 "Extended Entity Data"), one item each:
//!   * `0`  string   — R2007+: `u16` code-unit count + UTF-16LE; earlier:
//!                      `u8` byte length + `u16` codepage + encoded bytes.
//!   * `2`  control  — one byte: `0` = `{`, `1` = `}`.
//!   * `3`  layer    — 8-byte little-endian layer table handle.
//!   * `4`  binary   — `u8` length + raw bytes.
//!   * `5`  handle   — 8-byte little-endian handle value.
//!   * `10..=13` point/pos/disp/dir — three `f64` (x, y, z).
//!   * `40..=42` real/distance/scale — one `f64`.
//!   * `70` int16    — one `i16`.
//!   * `71` int32    — one `i32`.
//!
//! The item code equals the DXF group code minus 1000. The application handle
//! is carried by the surrounding framing, not the payload, so no `0xFFFF` appid
//! marker is embedded here.

use crate::types::{Handle, Vector3};
use crate::xdata::XDataValue;

/// Encode a record's `values` into EED payload bytes.
///
/// `wide` selects the R2007+ UTF-16 string encoding. `layer_handle` resolves a
/// layer name to its table handle for [`XDataValue::LayerName`] items (returns
/// `0` when the layer is unknown).
#[cfg(test)]
fn encode_values(wide: bool, values: &[XDataValue], layer_handle: impl Fn(&str) -> u64) -> Vec<u8> {
    encode_values_with_encoding(wide, values, encoding_rs::WINDOWS_1252, 30, layer_handle)
}

pub(crate) fn encode_values_with_encoding(
    wide: bool,
    values: &[XDataValue],
    encoding: &'static encoding_rs::Encoding,
    code_page: u16,
    layer_handle: impl Fn(&str) -> u64,
) -> Vec<u8> {
    let mut b = Vec::new();
    let push_string = |b: &mut Vec<u8>, s: &str| {
        b.push(0);
        if wide {
            let units: Vec<u16> = s.encode_utf16().collect();
            b.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for u in units {
                b.extend_from_slice(&u.to_le_bytes());
            }
        } else {
            let encoded = crate::io::dxf::code_page::encode_legacy_string(s, encoding);
            b.push(encoded.len() as u8);
            b.extend_from_slice(&code_page.to_le_bytes());
            b.extend_from_slice(&encoded);
        }
    };
    let push_point = |b: &mut Vec<u8>, code: u8, p: &Vector3| {
        b.push(code);
        b.extend_from_slice(&p.x.to_le_bytes());
        b.extend_from_slice(&p.y.to_le_bytes());
        b.extend_from_slice(&p.z.to_le_bytes());
    };
    let push_real = |b: &mut Vec<u8>, code: u8, v: f64| {
        b.push(code);
        b.extend_from_slice(&v.to_le_bytes());
    };
    for value in values {
        match value {
            XDataValue::String(s) => push_string(&mut b, s),
            XDataValue::ControlString(s) => {
                b.push(2);
                b.push(if s == "}" { 1 } else { 0 });
            }
            XDataValue::LayerName(name) => {
                b.push(3);
                // Layer handles use the same big-endian form as code 5.
                b.extend_from_slice(&layer_handle(name).to_be_bytes());
            }
            XDataValue::BinaryData(data) => {
                // Each item length is a single byte; split oversized blobs into
                // consecutive code-4 chunks (matching the DXF multi-1004 form).
                for chunk in data.chunks(255) {
                    b.push(4);
                    b.push(chunk.len() as u8);
                    b.extend_from_slice(chunk);
                }
            }
            XDataValue::Handle(h) => {
                b.push(5);
                // DWG EED handles are 8-byte BIG-endian values.
                b.extend_from_slice(&h.value().to_be_bytes());
            }
            XDataValue::Point3D(p) => push_point(&mut b, 10, p),
            XDataValue::Position3D(p) => push_point(&mut b, 11, p),
            XDataValue::Displacement3D(p) => push_point(&mut b, 12, p),
            XDataValue::Direction3D(p) => push_point(&mut b, 13, p),
            XDataValue::Real(v) => push_real(&mut b, 40, *v),
            XDataValue::Distance(v) => push_real(&mut b, 41, *v),
            XDataValue::ScaleFactor(v) => push_real(&mut b, 42, *v),
            XDataValue::Integer16(v) => {
                b.push(70);
                b.extend_from_slice(&v.to_le_bytes());
            }
            XDataValue::Integer32(v) => {
                b.push(71);
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    b
}

/// Decode an EED payload byte block into XDATA values, reversing
/// [`encode_values`]. `wide` selects the R2007+ string encoding; `layer_name`
/// resolves a layer table handle back to its name. Returns `None` if the bytes
/// contain an unknown item code or are truncated mid-item, so the caller can
/// keep the verbatim `raw_dwg_eed` blob instead of a partial record.
pub(crate) fn decode_values(
    bytes: &[u8],
    wide: bool,
    layer_name: impl Fn(u64) -> Option<String>,
) -> Option<Vec<XDataValue>> {
    let mut values = Vec::new();
    let mut i = 0usize;
    let read_u16 = |b: &[u8], i: usize| -> Option<u16> {
        Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
    };
    let read_f64 = |b: &[u8], i: usize| -> Option<f64> {
        let s: [u8; 8] = b.get(i..i + 8)?.try_into().ok()?;
        Some(f64::from_le_bytes(s))
    };
    let _read_u64 = |b: &[u8], i: usize| -> Option<u64> {
        let s: [u8; 8] = b.get(i..i + 8)?.try_into().ok()?;
        Some(u64::from_le_bytes(s))
    };
    while i < bytes.len() {
        let code = bytes[i];
        i += 1;
        match code {
            0 => {
                let s = if wide {
                    let n = read_u16(bytes, i)? as usize;
                    i += 2;
                    let mut units = Vec::with_capacity(n);
                    for _ in 0..n {
                        units.push(read_u16(bytes, i)?);
                        i += 2;
                    }
                    String::from_utf16_lossy(&units)
                } else {
                    let n = *bytes.get(i)? as usize;
                    i += 1;
                    let code_page = read_u16(bytes, i)?;
                    i += 2;
                    let slice = bytes.get(i..i + n)?;
                    i += n;
                    let decoded = crate::io::dxf::code_page::encoding_from_dwg_code_page(code_page)
                        .decode(slice)
                        .0
                        .into_owned();
                    crate::io::dxf::code_page::decode_mif_escapes(&decoded)
                };
                values.push(XDataValue::String(s));
            }
            2 => {
                let c = *bytes.get(i)?;
                i += 1;
                values.push(XDataValue::ControlString(
                    if c == 1 { "}" } else { "{" }.to_string(),
                ));
            }
            3 => {
                // Layer handles use the same 8-byte big-endian form as
                // code 5 handles.
                let bytes = bytes.get(i..i + 8)?;
                let h = u64::from_be_bytes(bytes.try_into().ok()?);
                i += 8;
                values.push(XDataValue::LayerName(layer_name(h).unwrap_or_default()));
            }
            4 => {
                let n = *bytes.get(i)? as usize;
                i += 1;
                let slice = bytes.get(i..i + n)?;
                i += n;
                values.push(XDataValue::BinaryData(slice.to_vec()));
            }
            5 => {
                // DWG EED handles are 8-byte BIG-endian values. Decoding
                // them little-endian byte-reversed every handle and made
                // the DXF writer emit invalid `1005` values (issue #51
                // BricsCAD audit: XData handle (DA06000000000000)).
                let bytes = bytes.get(i..i + 8)?;
                let h = u64::from_be_bytes(bytes.try_into().ok()?);
                i += 8;
                values.push(XDataValue::Handle(Handle::new(h)));
            }
            10..=13 => {
                let x = read_f64(bytes, i)?;
                let y = read_f64(bytes, i + 8)?;
                let z = read_f64(bytes, i + 16)?;
                i += 24;
                let p = Vector3::new(x, y, z);
                values.push(match code {
                    10 => XDataValue::Point3D(p),
                    11 => XDataValue::Position3D(p),
                    12 => XDataValue::Displacement3D(p),
                    _ => XDataValue::Direction3D(p),
                });
            }
            40..=42 => {
                let v = read_f64(bytes, i)?;
                i += 8;
                values.push(match code {
                    40 => XDataValue::Real(v),
                    41 => XDataValue::Distance(v),
                    _ => XDataValue::ScaleFactor(v),
                });
            }
            70 => {
                let v = i16::from_le_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]);
                i += 2;
                values.push(XDataValue::Integer16(v));
            }
            71 => {
                let v = i32::from_le_bytes([
                    *bytes.get(i)?,
                    *bytes.get(i + 1)?,
                    *bytes.get(i + 2)?,
                    *bytes.get(i + 3)?,
                ]);
                i += 4;
                values.push(XDataValue::Integer32(v));
            }
            _ => return None,
        }
        if i > bytes.len() {
            return None;
        }
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eed_handle_is_big_endian() {
        // The DWG stream stores EED handles as 8-byte big-endian values:
        // entity 0x6DA's EED in the civil sample reads
        // `05 | 00 00 00 00 00 00 06 DA`. Decoding it little-endian
        // byte-reversed every handle and made the DXF writer emit invalid
        // `1005` values (issue #51 BricsCAD audit).
        let payload = [0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0xDA];
        let values = decode_values(&payload, false, |_| None).unwrap();
        match &values[0] {
            XDataValue::Handle(h) => assert_eq!(h.value(), 0x6DA),
            other => panic!("expected handle, got {:?}", other),
        }

        // Encoding round-trips through the big-endian form.
        let encoded = encode_values_with_encoding(
            false,
            &[XDataValue::Handle(Handle::new(0x6DA))],
            encoding_rs::WINDOWS_1252,
            30,
            |_| 0,
        );
        assert_eq!(
            &encoded[1..9],
            &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0xDA]
        );
        let back = decode_values(&encoded, false, |_| None).unwrap();
        match &back[0] {
            XDataValue::Handle(h) => assert_eq!(h.value(), 0x6DA),
            other => panic!("expected handle, got {:?}", other),
        }
    }

    fn sample() -> Vec<XDataValue> {
        vec![
            XDataValue::String("survey point 1 — ünïcödé".to_string()),
            XDataValue::ControlString("{".to_string()),
            XDataValue::Integer16(-42),
            XDataValue::Integer32(1_000_000),
            XDataValue::Real(3.141592653589793),
            XDataValue::Distance(2.5),
            XDataValue::ScaleFactor(0.5),
            XDataValue::Point3D(Vector3::new(1.0, 2.0, 3.0)),
            XDataValue::Position3D(Vector3::new(-4.0, 5.0, -6.0)),
            XDataValue::Handle(Handle::new(0xABCDEF)),
            XDataValue::BinaryData(vec![0, 1, 2, 250, 255]),
            XDataValue::ControlString("}".to_string()),
        ]
    }

    fn roundtrip(wide: bool) {
        let values = sample();
        let bytes = encode_values(wide, &values, |_| 0);
        let decoded = decode_values(&bytes, wide, |_| None).expect("decode");
        assert_eq!(decoded, values);
    }

    #[test]
    fn roundtrip_wide() {
        roundtrip(true);
    }

    #[test]
    fn roundtrip_narrow() {
        roundtrip(false);
    }

    #[test]
    fn layer_name_resolves_through_handle() {
        let values = vec![XDataValue::LayerName("WALLS".to_string())];
        let bytes = encode_values(true, &values, |n| if n == "WALLS" { 7 } else { 0 });
        let decoded =
            decode_values(&bytes, true, |h| (h == 7).then(|| "WALLS".to_string())).expect("decode");
        assert_eq!(decoded, values);
    }

    #[test]
    fn binary_over_255_bytes_splits_and_rejoins_as_chunks() {
        let values = vec![XDataValue::BinaryData((0..300).map(|n| n as u8).collect())];
        let bytes = encode_values(true, &values, |_| 0);
        let decoded = decode_values(&bytes, true, |_| None).expect("decode");
        // Re-joined chunks reconstruct the original byte sequence.
        let mut joined = Vec::new();
        for v in &decoded {
            if let XDataValue::BinaryData(d) = v {
                joined.extend_from_slice(d);
            }
        }
        assert_eq!(joined, (0..300).map(|n| n as u8).collect::<Vec<u8>>());
    }

    #[test]
    fn unknown_code_bails_out() {
        // 0x63 (99) is not a supported EED item code.
        assert_eq!(decode_values(&[0x63, 0, 0], true, |_| None), None);
    }
}
