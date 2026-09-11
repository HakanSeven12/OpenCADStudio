//! DWG Auxiliary Header section writer
//!
//! Writes the AuxHeader section containing DWG-internal version info,
//! save counts, timestamps, and HANDSEED.
//!
//! Based on the reference `DwgAuxHeaderWriter`.

use crate::document::HeaderVariables;
use crate::io::dwg::dwg_stream_writers::DwgBitWriter;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::DxfVersion;

/// Map DxfVersion to the internal DWG version number used in AuxHeader.
///
/// These are the values from `ACadVersion` in C#:
/// AC1012=19, AC1014=21, AC1015=23, AC1018=25, AC1021=27,
/// AC1024=29, AC1027=31, AC1032=33
fn dwg_internal_version(version: DxfVersion) -> i16 {
    match version {
        DxfVersion::AC1012 => 19,
        DxfVersion::AC1014 => 21,
        DxfVersion::AC1015 => 23,
        DxfVersion::AC1018 => 25,
        DxfVersion::AC1021 => 27,
        DxfVersion::AC1024 => 29,
        DxfVersion::AC1027 => 31,
        DxfVersion::AC1032 => 33,
        _ => 23, // default to R2000
    }
}

/// Map DxfVersion to the maintenance release version used in AuxHeader.
///
/// Must match the file header metadata maintenance byte (0x0B, 0x12).
pub fn dwg_maintenance_version(version: DxfVersion) -> i16 {
    match version {
        DxfVersion::AC1021 => 25, // 0x19
        DxfVersion::AC1024 => 30, // 0x1E
        DxfVersion::AC1027 => 29, // 0x1D
        DxfVersion::AC1032 => 4,  // 0x04
        _ => 0,
    }
}

/// Write the AuxHeader section.
///
/// # Arguments
/// * `version` - Target DXF version
/// * `header` - Document header variables (for timestamps and HANDSEED)
///
/// # Returns
/// Raw section bytes.
pub fn write_aux_header(version: DxfVersion, header: &HeaderVariables) -> Vec<u8> {
    let dwg_version = DwgVersion::from_dxf_version(version).unwrap_or(DwgVersion::AC15);
    let mut writer = DwgBitWriter::new(dwg_version, version);

    let internal_ver = dwg_internal_version(version);
    let maintenance_ver: i16 = dwg_maintenance_version(version);

    // RC: 0xFF, 0x77, 0x01
    writer.write_byte(0xFF);
    writer.write_byte(0x77);
    writer.write_byte(0x01);

    // RS: DWG version
    writer.write_raw_short(internal_ver);

    // RS: Maintenance release version
    writer.write_raw_short(maintenance_ver);

    // RL: Number of saves (starts at 1)
    writer.write_raw_long(1);
    // RL: -1
    writer.write_raw_long(-1);

    // RS: Number of saves part 1 (= Number of saves - saves part 2)
    writer.write_raw_short(1);
    // RS: Number of saves part 2 (= Number of saves − 0x7fff if > 0x7fff, else 0)
    writer.write_raw_short(0);

    // RL: 0
    writer.write_raw_long(0);

    // RS: DWG version string (repeated)
    writer.write_raw_short(internal_ver);
    // RS: Maintenance version
    writer.write_raw_short(maintenance_ver);
    // RS: DWG version string (repeated again)
    writer.write_raw_short(internal_ver);
    // RS: Maintenance version (repeated)
    writer.write_raw_short(maintenance_ver);

    // RS: 0x0005
    writer.write_raw_short(0x0005);
    // RS: 0x0893
    writer.write_raw_short(0x0893);
    // RS: 0x0005
    writer.write_raw_short(0x0005);
    // RS: 0x0893
    writer.write_raw_short(0x0893);
    // RS: 0x0000
    writer.write_raw_short(0);
    // RS: 0x0001
    writer.write_raw_short(1);

    // RL: 0x0000 (×5)
    for _ in 0..5 {
        writer.write_raw_long(0);
    }

    // TD: TDCREATE (Julian date as 8 bytes: day + milliseconds)
    let (create_day, create_ms) = julian_from_f64(header.create_date_julian);
    writer.write_8bit_julian_date(create_day, create_ms);

    // TD: TDUPDATE (Julian date as 8 bytes)
    let (update_day, update_ms) = julian_from_f64(header.update_date_julian);
    writer.write_8bit_julian_date(update_day, update_ms);

    // RL: HANDSEED (if < 0x7FFFFFFF, else -1)
    let handseed = if header.handle_seed <= 0x7FFFFFFF {
        header.handle_seed as i32
    } else {
        -1
    };
    writer.write_raw_long(handseed);

    // RL: Educational plot stamp (default 0)
    writer.write_raw_long(0);

    // RS: 0
    writer.write_raw_short(0);
    // RS: Number of saves part 1 - part 2
    writer.write_raw_short(1);

    // RL: 0 (×3)
    writer.write_raw_long(0);
    writer.write_raw_long(0);
    writer.write_raw_long(0);

    // RL: Number of saves
    writer.write_raw_long(1);

    // RL: 0 (×4)
    writer.write_raw_long(0);
    writer.write_raw_long(0);
    writer.write_raw_long(0);
    writer.write_raw_long(0);

    // R2018+: 3 extra zero shorts
    if version >= DxfVersion::AC1032 {
        writer.write_raw_short(0);
        writer.write_raw_short(0);
        writer.write_raw_short(0);
    }

    writer.into_bytes()
}

/// Convert a Julian date f64 (day.fraction) into (day, milliseconds) pair.
fn julian_from_f64(julian: f64) -> (i32, i32) {
    let day = julian as i32;
    let fraction = julian - day as f64;
    let ms = (fraction * 86_400_000.0) as i32;
    (day, ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dwg_internal_version() {
        assert_eq!(dwg_internal_version(DxfVersion::AC1012), 19);
        assert_eq!(dwg_internal_version(DxfVersion::AC1015), 23);
        assert_eq!(dwg_internal_version(DxfVersion::AC1018), 25);
        assert_eq!(dwg_internal_version(DxfVersion::AC1032), 33);
    }

    #[test]
    fn test_julian_from_f64() {
        let (day, ms) = julian_from_f64(2451544.5);
        assert_eq!(day, 2451544);
        assert!(ms > 0);
    }

    #[test]
    fn test_write_aux_header_starts_with_magic() {
        let header = HeaderVariables::default();
        let data = write_aux_header(DxfVersion::AC1015, &header);

        // Should start with 0xFF, 0x77, 0x01
        assert_eq!(data[0], 0xFF);
        assert_eq!(data[1], 0x77);
        assert_eq!(data[2], 0x01);
    }

    #[test]
    fn test_write_aux_header_contains_version() {
        let header = HeaderVariables::default();
        let data = write_aux_header(DxfVersion::AC1015, &header);

        // Version 23 (AC1015) as LE i16 at offset 3
        assert_eq!(data[3], 23);
        assert_eq!(data[4], 0);
    }

    #[test]
    fn test_write_aux_header_r2018_longer() {
        let header = HeaderVariables::default();
        let data_2000 = write_aux_header(DxfVersion::AC1015, &header);
        let data_2018 = write_aux_header(DxfVersion::AC1032, &header);

        // R2018 should be 6 bytes longer (3 extra RS fields)
        assert_eq!(data_2018.len(), data_2000.len() + 6);
    }
}
