//! ASCII DXF writer

use super::stream_writer::DxfStreamWriter;
use crate::error::Result;
use crate::types::Handle;
use std::io::Write;

/// ASCII DXF stream writer.
///
/// Uses CR/LF (`\r\n`) line endings as required by the DXF text format
/// specification.
pub struct DxfTextWriter<W: Write> {
    writer: W,
    /// Reusable stack buffer for formatting numbers without heap allocation.
    fmt_buf: [u8; 64],
}

impl<W: Write> DxfTextWriter<W> {
    /// Create a new ASCII DXF writer
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            fmt_buf: [0u8; 64],
        }
    }

    /// Write a DXF group code (right-aligned in 3-character field) followed by CRLF.
    /// Uses itoa for zero-allocation integer formatting.
    #[inline]
    fn write_code(&mut self, code: i32) -> Result<()> {
        let mut ibuf = itoa::Buffer::new();
        let s = ibuf.format(code);
        // Right-align in 3-char field: pad with spaces
        let pad = if s.len() < 3 { 3 - s.len() } else { 0 };
        // Build the line in fmt_buf: spaces + digits + \r\n
        let len = pad + s.len() + 2;
        self.fmt_buf[..pad].fill(b' ');
        self.fmt_buf[pad..pad + s.len()].copy_from_slice(s.as_bytes());
        self.fmt_buf[pad + s.len()] = b'\r';
        self.fmt_buf[pad + s.len() + 1] = b'\n';
        self.writer.write_all(&self.fmt_buf[..len])?;
        Ok(())
    }

    /// Write a value string followed by CRLF.
    #[inline]
    fn write_value_crlf(&mut self, value: &[u8]) -> Result<()> {
        self.writer.write_all(value)?;
        self.writer.write_all(b"\r\n")?;
        Ok(())
    }

    /// Format an i16/i32 right-aligned in a 6-character field into fmt_buf.
    /// Returns the slice length.
    #[inline]
    fn format_right6(&mut self, value: i32) -> usize {
        let mut ibuf = itoa::Buffer::new();
        let s = ibuf.format(value);
        let slen = s.len();
        let pad = if slen < 6 { 6 - slen } else { 0 };
        let total = pad + slen + 2; // +2 for \r\n
        self.fmt_buf[..pad].fill(b' ');
        self.fmt_buf[pad..pad + slen].copy_from_slice(s.as_bytes());
        self.fmt_buf[pad + slen] = b'\r';
        self.fmt_buf[pad + slen + 1] = b'\n';
        total
    }

    /// Format f64 with 16 decimal places, trimming trailing zeros (keeping at
    /// least one digit after the decimal point). Written directly into fmt_buf
    /// to avoid heap allocation.
    #[inline]
    fn format_double(&mut self, value: f64) -> usize {
        use std::io::Cursor;
        // For extreme values (|v| >= 1e15), {:.16} may produce strings longer
        // than the stack buffer.  Fall back to 0.0 for non-finite values and
        // use a heap-allocated string for very large magnitudes.
        if !value.is_finite() {
            // NaN / Infinity → write 0.0
            self.fmt_buf[0] = b'0';
            self.fmt_buf[1] = b'.';
            self.fmt_buf[2] = b'0';
            self.fmt_buf[3] = b'\r';
            self.fmt_buf[4] = b'\n';
            return 5;
        }
        let abs = value.abs();
        if abs > 0.0 && abs < 1e-15 {
            // Very small magnitude: {:.16} would lose all significance.
            // Use scientific notation with full precision instead.
            let s = format!("{:.16e}", value);
            let bytes = s.as_bytes();
            let total = bytes.len() + 2; // +CRLF
            if total <= self.fmt_buf.len() {
                self.fmt_buf[..bytes.len()].copy_from_slice(bytes);
                self.fmt_buf[bytes.len()] = b'\r';
                self.fmt_buf[bytes.len() + 1] = b'\n';
                return total;
            }
        }
        if abs >= 1e15 && abs != 0.0 {
            // Scientific notation stays compact for every finite f64. Sixteen
            // fractional digits provide the 17 significant decimal digits
            // needed to round-trip an arbitrary binary64 value.
            let s = format!("{:.16e}", value);
            let bytes = s.as_bytes();
            self.fmt_buf[..bytes.len()].copy_from_slice(bytes);
            self.fmt_buf[bytes.len()] = b'\r';
            self.fmt_buf[bytes.len() + 1] = b'\n';
            return bytes.len() + 2;
        }
        let mut cursor = Cursor::new(&mut self.fmt_buf[..]);
        // Rust's `{}` Display for f64 emits the shortest decimal string that
        // round-trips back to the same value, and never uses exponent notation
        // for magnitudes in this branch's range (1e-15 ..= 1e15).  This matches
        // AutoCAD's compact output (e.g. "943.920153") instead of dumping the
        // full 16-digit binary expansion of the float ("943.9201530000000275"),
        // which trimming trailing zeros cannot clean up because the tail digits
        // are rounding noise, not zeros.  write! into a Cursor<&mut [u8]> does
        // not allocate.
        let _ = write!(cursor, "{}", value);
        let mut len = cursor.position() as usize;
        // `{}` prints whole numbers without a fractional part ("15" not "15.0");
        // DXF real values conventionally carry a decimal point, so add ".0".
        if !self.fmt_buf[..len].contains(&b'.') {
            self.fmt_buf[len] = b'.';
            self.fmt_buf[len + 1] = b'0';
            len += 2;
        }
        // Append CRLF
        self.fmt_buf[len] = b'\r';
        self.fmt_buf[len + 1] = b'\n';
        len + 2
    }

    /// Get the inner writer
    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> DxfStreamWriter for DxfTextWriter<W> {
    #[inline]
    fn write_string(&mut self, code: i32, value: &str) -> Result<()> {
        self.write_code(code)?;
        // DXF text format is line-based: literal newlines in string values
        // would corrupt the file.  Replace them with the MText paragraph
        // marker \P which is the standard convention in DXF/DWG ecosystems.
        if value.contains('\n') || value.contains('\r') {
            let sanitized = value
                .replace("\r\n", "\\P")
                .replace('\r', "\\P")
                .replace('\n', "\\P");
            self.write_value_crlf(sanitized.as_bytes())?;
        } else {
            self.write_value_crlf(value.as_bytes())?;
        }
        Ok(())
    }

    fn write_xrecord_string(&mut self, code: i32, value: &str) -> Result<()> {
        self.write_code(code)?;
        let escaped = value
            .replace('^', "^ ")
            .replace("\r\n", "^M^J")
            .replace('\r', "^M")
            .replace('\n', "^J")
            .replace('\t', "^I");
        self.write_value_crlf(escaped.as_bytes())?;
        Ok(())
    }

    #[inline]
    fn write_byte(&mut self, code: i32, value: u8) -> Result<()> {
        self.write_code(code)?;
        let mut ibuf = itoa::Buffer::new();
        let s = ibuf.format(value);
        self.write_value_crlf(s.as_bytes())?;
        Ok(())
    }

    #[inline]
    fn write_i16(&mut self, code: i32, value: i16) -> Result<()> {
        self.write_code(code)?;
        let len = self.format_right6(value as i32);
        self.writer.write_all(&self.fmt_buf[..len])?;
        Ok(())
    }

    #[inline]
    fn write_i32(&mut self, code: i32, value: i32) -> Result<()> {
        self.write_code(code)?;
        let len = self.format_right6(value);
        self.writer.write_all(&self.fmt_buf[..len])?;
        Ok(())
    }

    #[inline]
    fn write_i64(&mut self, code: i32, value: i64) -> Result<()> {
        self.write_code(code)?;
        let mut ibuf = itoa::Buffer::new();
        let s = ibuf.format(value);
        self.write_value_crlf(s.as_bytes())?;
        Ok(())
    }

    #[inline]
    fn write_double(&mut self, code: i32, value: f64) -> Result<()> {
        self.write_code(code)?;
        let len = self.format_double(value);
        self.writer.write_all(&self.fmt_buf[..len])?;
        Ok(())
    }

    #[inline]
    fn write_bool(&mut self, code: i32, value: bool) -> Result<()> {
        self.write_code(code)?;
        // "     0\r\n" or "     1\r\n" — always 8 bytes
        if value {
            self.writer.write_all(b"     1\r\n")?;
        } else {
            self.writer.write_all(b"     0\r\n")?;
        }
        Ok(())
    }

    fn write_handle(&mut self, code: i32, handle: Handle) -> Result<()> {
        self.write_code(code)?;
        // Format handle value as uppercase hex directly into fmt_buf
        let val = handle.value();
        if val == 0 {
            self.write_value_crlf(b"0")?;
        } else {
            // Upper-case hex: max u64 = 16 hex digits
            let mut pos = 16usize; // start from end of a 16-byte region
            let mut v = val;
            while v > 0 {
                pos -= 1;
                let digit = (v & 0xF) as u8;
                self.fmt_buf[pos] = if digit < 10 {
                    b'0' + digit
                } else {
                    b'A' + digit - 10
                };
                v >>= 4;
            }
            let hex_len = 16 - pos;
            // Shift to start of buffer and append CRLF
            self.fmt_buf.copy_within(pos..16, 0);
            self.fmt_buf[hex_len] = b'\r';
            self.fmt_buf[hex_len + 1] = b'\n';
            self.writer.write_all(&self.fmt_buf[..hex_len + 2])?;
        }
        Ok(())
    }

    fn write_binary(&mut self, code: i32, data: &[u8]) -> Result<()> {
        self.write_code(code)?;
        const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";
        // Write in chunks using fmt_buf to reduce write calls
        let chunk_bytes = self.fmt_buf.len() / 2; // 2 hex chars per byte
        for chunk in data.chunks(chunk_bytes) {
            let mut pos = 0;
            for &byte in chunk {
                self.fmt_buf[pos] = HEX_CHARS[(byte >> 4) as usize];
                self.fmt_buf[pos + 1] = HEX_CHARS[(byte & 0x0F) as usize];
                pos += 2;
            }
            self.writer.write_all(&self.fmt_buf[..pos])?;
        }
        self.writer.write_all(b"\r\n")?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::stream_writer::DxfStreamWriterExt;
    use super::*;
    use crate::types::Vector3;

    #[test]
    fn test_write_string() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfTextWriter::new(&mut buf);
            writer.write_string(0, "LINE").unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "  0\r\nLINE\r\n");
    }

    #[test]
    fn test_write_code_formatting() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfTextWriter::new(&mut buf);
            writer.write_i16(5, 100).unwrap();
            writer.write_i16(62, 7).unwrap();
            writer.write_i16(100, 1).unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        // Codes should be right-aligned in 3-character field
        assert!(output.starts_with("  5\r\n"));
        assert!(output.contains(" 62\r\n"));
        assert!(output.contains("100\r\n"));
    }

    #[test]
    fn test_write_point3d() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfTextWriter::new(&mut buf);
            writer
                .write_point3d(10, Vector3::new(1.0, 2.0, 3.0))
                .unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains(" 10\r\n"));
        assert!(output.contains("1.0\r\n"));
        assert!(output.contains(" 20\r\n"));
        assert!(output.contains("2.0\r\n"));
        assert!(output.contains(" 30\r\n"));
        assert!(output.contains("3.0\r\n"));
    }

    #[test]
    fn test_write_handle() {
        let mut buf = Vec::new();
        {
            let mut writer = DxfTextWriter::new(&mut buf);
            writer.write_handle(5, Handle::new(255)).unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("FF\r\n"));
    }

    #[test]
    fn test_write_double_compact() {
        // Regression: the writer used to emit the full 16-decimal expansion of
        // the f64 (e.g. "943.9201530000000275"), because trimming trailing
        // zeros cannot remove binary rounding noise.  It must emit the shortest
        // decimal that round-trips to the same value instead.
        let mut buf = Vec::new();
        {
            let mut writer = DxfTextWriter::new(&mut buf);
            writer.write_double(10, 943.920153).unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, " 10\r\n943.920153\r\n");
    }

    #[test]
    fn test_write_double_integer_keeps_point() {
        // Whole values must still carry a decimal point ("15.0", not "15"),
        // since `{}` prints integers without a fractional part.
        let mut buf = Vec::new();
        {
            let mut writer = DxfTextWriter::new(&mut buf);
            writer.write_double(40, 15.0).unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, " 40\r\n15.0\r\n");
    }
}
