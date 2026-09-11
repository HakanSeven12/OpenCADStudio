//! Binary DXF writer

use super::stream_writer::DxfStreamWriter;
use crate::error::Result;
use crate::types::Handle;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;

/// Binary DXF sentinel
const BINARY_DXF_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\x00";

/// Binary DXF stream writer
pub struct DxfBinaryWriter<W: Write> {
    writer: W,
    /// Stack buffer for formatting handles without heap allocation.
    hex_buf: [u8; 17],
}

impl<W: Write> DxfBinaryWriter<W> {
    /// Create a new binary DXF writer
    pub fn new(mut writer: W) -> Result<Self> {
        // Write the binary sentinel at the start
        writer.write_all(BINARY_DXF_SENTINEL)?;
        Ok(Self {
            writer,
            hex_buf: [0u8; 17],
        })
    }

    /// Write a DXF code as 16-bit little-endian
    fn write_code(&mut self, code: i32) -> Result<()> {
        self.writer.write_i16::<LittleEndian>(code as i16)?;
        Ok(())
    }

    /// Write a null-terminated string
    fn write_null_string(&mut self, value: &str) -> Result<()> {
        self.writer.write_all(value.as_bytes())?;
        self.writer.write_u8(0)?;
        Ok(())
    }

    /// Preserved DXF pairs store their values as text, but their binary
    /// representation still follows the group code's numeric or chunk type.
    /// Return false only for values that retain the null-string layout.
    fn write_typed_text(&mut self, code: i32, value: &str) -> Result<bool> {
        use crate::error::DxfError;
        use crate::io::dxf::GroupCodeValueType;

        let invalid = || DxfError::Parse(format!("Invalid text value for binary DXF group {code}"));
        let text = value.trim();
        match GroupCodeValueType::from_raw_code(code) {
            GroupCodeValueType::String | GroupCodeValueType::Handle => return Ok(false),
            GroupCodeValueType::Bool => {
                self.write_bool(code, text.parse::<i64>().map_err(|_| invalid())? != 0)?
            }
            GroupCodeValueType::Byte | GroupCodeValueType::Int16 => {
                self.write_i16(code, text.parse().map_err(|_| invalid())?)?
            }
            GroupCodeValueType::Int32 => {
                self.write_i32(code, text.parse().map_err(|_| invalid())?)?
            }
            GroupCodeValueType::Int64 => {
                self.write_i64(code, text.parse().map_err(|_| invalid())?)?
            }
            GroupCodeValueType::Double | GroupCodeValueType::Point3D => {
                self.write_double(code, text.parse().map_err(|_| invalid())?)?
            }
            GroupCodeValueType::BinaryData => {
                if text.len() % 2 != 0 {
                    return Err(invalid());
                }
                let data = text
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|pair| {
                        let high = (pair[0] as char).to_digit(16).ok_or_else(invalid)?;
                        let low = (pair[1] as char).to_digit(16).ok_or_else(invalid)?;
                        Ok(((high << 4) | low) as u8)
                    })
                    .collect::<Result<Vec<_>>>()?;
                self.write_binary(code, &data)?;
            }
            GroupCodeValueType::None => return Err(DxfError::InvalidDxfCode(code)),
        }
        Ok(true)
    }

    /// Get the inner writer
    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> DxfStreamWriter for DxfBinaryWriter<W> {
    fn write_string(&mut self, code: i32, value: &str) -> Result<()> {
        if self.write_typed_text(code, value)? {
            return Ok(());
        }
        self.write_code(code)?;
        // Sanitize embedded newlines to DXF paragraph markers, matching the
        // ASCII writer.  While binary DXF uses null-terminated strings (so raw
        // newlines won't structurally break the file), many AutoCAD consumers
        // still expect \P instead of literal line breaks in string values.
        if value.contains('\n') || value.contains('\r') {
            let sanitized = value
                .replace("\r\n", "\\P")
                .replace('\r', "\\P")
                .replace('\n', "\\P");
            self.write_null_string(&sanitized)?;
        } else {
            self.write_null_string(value)?;
        }
        Ok(())
    }

    fn write_xrecord_string(&mut self, code: i32, value: &str) -> Result<()> {
        if self.write_typed_text(code, value)? {
            return Ok(());
        }
        self.write_code(code)?;
        self.write_null_string(value)
    }

    fn write_byte(&mut self, code: i32, value: u8) -> Result<()> {
        self.write_code(code)?;
        // Group codes 280-289 are "Byte" type but written as Int16 in binary DXF
        self.writer.write_i16::<LittleEndian>(value as i16)?;
        Ok(())
    }

    fn write_i16(&mut self, code: i32, value: i16) -> Result<()> {
        self.write_code(code)?;
        self.writer.write_i16::<LittleEndian>(value)?;
        Ok(())
    }

    fn write_i32(&mut self, code: i32, value: i32) -> Result<()> {
        self.write_code(code)?;
        self.writer.write_i32::<LittleEndian>(value)?;
        Ok(())
    }

    fn write_i64(&mut self, code: i32, value: i64) -> Result<()> {
        self.write_code(code)?;
        self.writer.write_i64::<LittleEndian>(value)?;
        Ok(())
    }

    fn write_double(&mut self, code: i32, value: f64) -> Result<()> {
        self.write_code(code)?;
        self.writer.write_f64::<LittleEndian>(value)?;
        Ok(())
    }

    fn write_bool(&mut self, code: i32, value: bool) -> Result<()> {
        use crate::io::dxf::GroupCodeValueType;

        // Logical flags also occur in integer groups, including history
        // flags 280/281. Their binary width follows the group code, not the
        // Rust value type; only groups 290-299 use a single byte.
        let value = u8::from(value);
        match GroupCodeValueType::from_raw_code(code) {
            GroupCodeValueType::Bool => {
                self.write_code(code)?;
                self.writer.write_u8(value)?;
                Ok(())
            }
            GroupCodeValueType::Byte | GroupCodeValueType::Int16 => {
                self.write_i16(code, i16::from(value))
            }
            GroupCodeValueType::Int32 => self.write_i32(code, i32::from(value)),
            GroupCodeValueType::Int64 => self.write_i64(code, i64::from(value)),
            GroupCodeValueType::Double => self.write_double(code, f64::from(value)),
            _ => Err(crate::error::DxfError::InvalidDxfCode(code)),
        }
    }

    fn write_handle(&mut self, code: i32, handle: Handle) -> Result<()> {
        self.write_code(code)?;
        // Handles are written as hex strings even in binary DXF
        // Format directly into stack buffer to avoid heap allocation
        let val = handle.value();
        if val == 0 {
            self.writer.write_all(b"0\0")?;
        } else {
            let mut pos = 16usize;
            let mut v = val;
            while v > 0 {
                pos -= 1;
                let digit = (v & 0xF) as u8;
                self.hex_buf[pos] = if digit < 10 {
                    b'0' + digit
                } else {
                    b'A' + digit - 10
                };
                v >>= 4;
            }
            let hex_len = 16 - pos;
            self.hex_buf.copy_within(pos..16, 0);
            self.hex_buf[hex_len] = 0; // null terminator
            self.writer.write_all(&self.hex_buf[..hex_len + 1])?;
        }
        Ok(())
    }

    fn write_binary(&mut self, code: i32, data: &[u8]) -> Result<()> {
        // Each binary group has a one-byte length. Repeat the group instead
        // of truncating the length when a caller supplies a larger payload.
        if data.is_empty() {
            self.write_code(code)?;
            self.writer.write_u8(0)?;
        }
        for chunk in data.chunks(255) {
            self.write_code(code)?;
            self.writer.write_u8(chunk.len() as u8)?;
            self.writer.write_all(chunk)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_sentinel() {
        let mut buf = Vec::new();
        {
            let _writer = DxfBinaryWriter::new(&mut buf).unwrap();
        }
        assert!(buf.starts_with(BINARY_DXF_SENTINEL));
    }

    #[test]
    fn test_write_string() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfBinaryWriter::new(&mut buf).unwrap();
            writer.write_string(0, "LINE").unwrap();
        }
        let sentinel_len = BINARY_DXF_SENTINEL.len();
        // After sentinel: code (2 bytes) + string + null
        assert_eq!(buf[sentinel_len..sentinel_len + 2], [0, 0]); // code 0 as little-endian
        assert_eq!(&buf[sentinel_len + 2..sentinel_len + 6], b"LINE");
        assert_eq!(buf[sentinel_len + 6], 0); // null terminator
    }

    #[test]
    fn test_write_double() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfBinaryWriter::new(&mut buf).unwrap();
            writer.write_double(10, 1.5).unwrap();
        }
        let sentinel_len = BINARY_DXF_SENTINEL.len();
        // code (2 bytes) + f64 (8 bytes)
        assert_eq!(buf[sentinel_len..sentinel_len + 2], [10, 0]); // code 10 as little-endian
                                                                  // 1.5 as f64 little-endian
        let expected: [u8; 8] = 1.5f64.to_le_bytes();
        assert_eq!(&buf[sentinel_len + 2..sentinel_len + 10], &expected);
    }

    #[test]
    fn test_write_i16() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfBinaryWriter::new(&mut buf).unwrap();
            writer.write_i16(62, 7).unwrap();
        }
        let sentinel_len = BINARY_DXF_SENTINEL.len();
        assert_eq!(buf[sentinel_len..sentinel_len + 2], [62, 0]); // code 62
        assert_eq!(buf[sentinel_len + 2..sentinel_len + 4], [7, 0]); // value 7
    }

    #[test]
    fn test_write_string_newline_sanitization() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfBinaryWriter::new(&mut buf).unwrap();
            writer.write_string(1, "Hello\r\nWorld\nFoo\rBar").unwrap();
        }
        let sentinel_len = BINARY_DXF_SENTINEL.len();
        // code (2 bytes) + sanitized string + null
        let str_start = sentinel_len + 2;
        let str_end = buf[str_start..].iter().position(|&b| b == 0).unwrap() + str_start;
        let written = std::str::from_utf8(&buf[str_start..str_end]).unwrap();
        assert_eq!(written, "Hello\\PWorld\\PFoo\\PBar");
    }
}
