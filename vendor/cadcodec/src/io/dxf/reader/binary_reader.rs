//! DXF binary reader

use super::stream_reader::{CodePairValue, DxfCodePair, DxfStreamContext, DxfStreamReader};
use crate::error::{DxfError, Result};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

/// Sentinel for binary DXF files
pub const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\x00";

/// DXF binary file reader
pub struct DxfBinaryReader<R: Read + Seek> {
    reader: BufReader<R>,
    peeked_pair: Option<DxfCodePair>,
    /// True for pre-AC1012 format (single-byte group codes)
    /// False for AC1012+ format (two-byte group codes)
    use_single_byte_codes: bool,
    /// Reusable buffer for reading null-terminated strings.
    str_buf: Vec<u8>,
    context: DxfStreamContext,
}

impl<R: Read + Seek> DxfBinaryReader<R> {
    /// Create a new DXF binary reader
    pub fn new(mut reader: BufReader<R>) -> Result<Self> {
        // Verify sentinel using stack array
        let mut sentinel = [0u8; 22]; // BINARY_SENTINEL.len() == 22
        reader.read_exact(&mut sentinel)?;

        if sentinel != BINARY_SENTINEL {
            return Err(DxfError::Parse("Invalid binary DXF sentinel".to_string()));
        }

        // Detect format by checking the first group code
        // In pre-AC1012, after sentinel we have: [code_byte][string...]
        // In AC1012+, we have: [code_lo][code_hi][string...]
        // The first code should be 0 (for SECTION), so:
        // - Pre-AC1012: byte 0 = 0x00, byte 1 = 'S' (0x53)
        // - AC1012+: byte 0 = 0x00, byte 1 = 0x00, byte 2 = 'S' (0x53)
        let mut probe = [0u8; 2];
        reader.read_exact(&mut probe)?;
        reader.seek(SeekFrom::Start(BINARY_SENTINEL.len() as u64))?;

        // If second byte is printable ASCII (like 'S' for SECTION), it's pre-AC1012
        let use_single_byte_codes = probe[0] == 0 && probe[1] >= 0x20 && probe[1] < 0x7F;

        Ok(Self {
            reader,
            peeked_pair: None,
            use_single_byte_codes,
            str_buf: Vec::with_capacity(256),
            context: DxfStreamContext::default(),
        })
    }

    /// Read a code/value pair from the binary stream
    fn read_pair_internal(&mut self) -> Result<Option<DxfCodePair>> {
        let pair_offset = self.reader.stream_position().ok();
        self.context.source_offset = pair_offset;
        self.context.source_line = None;
        let code = if self.use_single_byte_codes {
            // Pre-AC1012: single byte codes, with 255 as escape for extended codes
            let mut code_byte = [0u8; 1];
            match self.reader.read_exact(&mut code_byte) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e.into()),
            }

            if code_byte[0] == 255 {
                // Extended code: next 2 bytes are the actual code
                let mut ext_code = [0u8; 2];
                self.reader.read_exact(&mut ext_code)?;
                i16::from_le_bytes(ext_code) as i32
            } else {
                code_byte[0] as i32
            }
        } else {
            // AC1012+: 2-byte codes, little-endian
            let mut code_bytes = [0u8; 2];
            match self.reader.read_exact(&mut code_bytes) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e.into()),
            }
            i16::from_le_bytes(code_bytes) as i32
        };

        // Read value based on code type and construct typed pair
        let pair = self.read_pair_for_code(code)?;
        if let Some(pair) = &pair {
            self.observe_pair(pair, pair_offset);
        }
        Ok(pair)
    }

    fn observe_pair(&mut self, pair: &DxfCodePair, pair_offset: Option<u64>) {
        self.context.source_offset = pair_offset;
        if pair.code == 0 {
            self.context.record_type = Some(pair.value_string.clone());
            self.context.record_handle = None;
        } else if pair.code == 105
            || (pair.code == 5 && self.context.record_type.as_deref() != Some("DIMSTYLE"))
        {
            self.context.record_handle = pair.as_handle();
        }
    }

    /// Read a null-terminated string from the binary stream, reusing str_buf.
    fn read_null_terminated_string(&mut self) -> Result<String> {
        self.str_buf.clear();
        self.reader.read_until(0, &mut self.str_buf)?;
        // read_until includes the delimiter in the buffer
        if self.str_buf.last() == Some(&0) {
            self.str_buf.pop();
        }
        // Try UTF-8 first (borrow check), then fall back to lossy conversion
        let s = match std::str::from_utf8(&self.str_buf) {
            Ok(s) => s.to_owned(),
            Err(_) => String::from_utf8_lossy(&self.str_buf).into_owned(),
        };
        self.str_buf.clear(); // keep the allocation for reuse
        Ok(s)
    }

    /// Read a code/value pair from the binary stream for a given group code.
    /// For numeric types, constructs DxfCodePair with pre-computed typed values
    /// to avoid redundant string→number parsing in DxfCodePair::new().
    fn read_pair_for_code(&mut self, code: i32) -> Result<Option<DxfCodePair>> {
        use crate::io::dxf::GroupCodeValueType;

        let value_type = GroupCodeValueType::from_raw_code(code);

        let pair = match value_type {
            GroupCodeValueType::String => {
                let s = self.read_null_terminated_string()?;
                DxfCodePair::new(code, s)
            }

            GroupCodeValueType::Double => {
                let mut bytes = [0u8; 8];
                self.reader.read_exact(&mut bytes)?;
                let value = f64::from_le_bytes(bytes);
                let mut buf = ryu::Buffer::new();
                let value_string = buf.format(value).to_owned();
                DxfCodePair::new_typed(code, value_string, CodePairValue::Double(value))
            }

            GroupCodeValueType::Int16 | GroupCodeValueType::Byte => {
                let mut bytes = [0u8; 2];
                self.reader.read_exact(&mut bytes)?;
                let value = i16::from_le_bytes(bytes);
                let mut buf = itoa::Buffer::new();
                let value_string = buf.format(value).to_owned();
                DxfCodePair::new_typed(code, value_string, CodePairValue::Int(value as i64))
            }

            GroupCodeValueType::Int32 => {
                let mut bytes = [0u8; 4];
                self.reader.read_exact(&mut bytes)?;
                let value = i32::from_le_bytes(bytes);
                let mut buf = itoa::Buffer::new();
                let value_string = buf.format(value).to_owned();
                DxfCodePair::new_typed(code, value_string, CodePairValue::Int(value as i64))
            }

            GroupCodeValueType::Int64 => {
                let mut bytes = [0u8; 8];
                self.reader.read_exact(&mut bytes)?;
                let value = i64::from_le_bytes(bytes);
                let mut buf = itoa::Buffer::new();
                let value_string = buf.format(value).to_owned();
                DxfCodePair::new_typed(code, value_string, CodePairValue::Int(value))
            }

            GroupCodeValueType::Bool => {
                let mut byte = [0u8; 1];
                self.reader.read_exact(&mut byte)?;
                let value = byte[0] != 0;
                let value_string = if value { "1" } else { "0" }.to_owned();
                DxfCodePair::new_typed(code, value_string, CodePairValue::Bool(value))
            }

            GroupCodeValueType::BinaryData => {
                let mut len_byte = [0u8; 1];
                self.reader.read_exact(&mut len_byte)?;
                let length = len_byte[0] as usize;
                let mut data = vec![0u8; length];
                if length > 0 {
                    self.reader.read_exact(&mut data)?;
                }
                // Convert raw bytes to uppercase hex string using lookup table
                const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";
                let mut hex = String::with_capacity(length * 2);
                for &b in &data {
                    hex.push(HEX_CHARS[(b >> 4) as usize] as char);
                    hex.push(HEX_CHARS[(b & 0x0F) as usize] as char);
                }
                DxfCodePair::new(code, hex)
            }

            GroupCodeValueType::Handle | _ => {
                let s = self.read_null_terminated_string()?;
                DxfCodePair::new(code, s)
            }
        };

        Ok(Some(pair))
    }
}

impl<R: Read + Seek> DxfStreamReader for DxfBinaryReader<R> {
    fn read_pair(&mut self) -> Result<Option<DxfCodePair>> {
        // If we have a peeked pair, return it
        if let Some(pair) = self.peeked_pair.take() {
            return Ok(Some(pair));
        }

        self.read_pair_internal()
    }

    fn peek_code(&mut self) -> Result<Option<i32>> {
        // If we already have a peeked pair, return its code
        if let Some(ref pair) = self.peeked_pair {
            return Ok(Some(pair.code));
        }

        // Read the next pair and store it
        if let Some(pair) = self.read_pair_internal()? {
            let code = pair.code;
            self.peeked_pair = Some(pair);
            Ok(Some(code))
        } else {
            Ok(None)
        }
    }

    fn push_back(&mut self, pair: DxfCodePair) {
        self.peeked_pair = Some(pair);
    }

    fn reset(&mut self) -> Result<()> {
        self.reader.seek(SeekFrom::Start(0))?;
        self.peeked_pair = None;
        self.context = DxfStreamContext::default();

        // Re-verify sentinel using stack array
        let mut sentinel = [0u8; 22];
        self.reader.read_exact(&mut sentinel)?;

        if sentinel != BINARY_SENTINEL {
            return Err(DxfError::Parse("Invalid binary DXF sentinel".to_string()));
        }

        // Re-detect format (should be same as before, but just re-skip the probe bytes)
        let mut probe = [0u8; 2];
        self.reader.read_exact(&mut probe)?;
        self.reader
            .seek(SeekFrom::Start(BINARY_SENTINEL.len() as u64))?;

        Ok(())
    }

    fn diagnostic_context(&self) -> DxfStreamContext {
        self.context.clone()
    }
}
