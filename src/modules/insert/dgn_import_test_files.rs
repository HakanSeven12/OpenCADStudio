//! DGN files for tests.

fn put_word(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

/// V7 stores a 32-bit integer as its high word followed by its low word,
/// with each word little-endian.
fn put_i32(bytes: &mut [u8], at: usize, value: i32) {
    let value = value as u32;
    put_word(bytes, at, (value >> 16) as u16);
    put_word(bytes, at + 2, value as u16);
}

/// A V7 design file (model "Default") with 1000 subunits per master unit
/// and one 2D line from (0,0) to (2000,3000) UORs; the same file as the
/// automation tests' `minimal_v7_line`.
pub(crate) fn minimal_v7_line() -> Vec<u8> {
    const HEADER: usize = 1256;
    const LINE: usize = 52;
    let mut bytes = vec![0u8; HEADER + LINE + 2];
    bytes[0] = 8; // 2D design-file header (type 9 below).
    bytes[1] = 9;
    put_word(&mut bytes, 2, ((HEADER - 4) / 2) as u16);
    put_i32(&mut bytes, 1112, 1000); // subunits / master unit
    put_i32(&mut bytes, 1116, 1); // UORs / subunit
    bytes[1120] = b'm'; // Two-byte fields are zero-padded.
    bytes[1122..1124].copy_from_slice(b"mm");
    let line = HEADER;
    bytes[line] = 1; // level 1
    bytes[line + 1] = 3; // line element
    put_word(&mut bytes, line + 2, ((LINE - 4) / 2) as u16);
    put_i32(&mut bytes, line + 44, 2000);
    put_i32(&mut bytes, line + 48, 3000);
    bytes[HEADER + LINE..].copy_from_slice(&[0xff, 0xff]);
    bytes
}
