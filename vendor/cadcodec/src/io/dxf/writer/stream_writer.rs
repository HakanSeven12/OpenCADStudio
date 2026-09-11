//! DXF stream writer trait and common types

use crate::error::Result;
use crate::io::dxf::GroupCodeValueType;
use crate::types::{Color, Handle, Vector2, Vector3};

/// Trait for writing DXF code/value pairs
pub trait DxfStreamWriter {
    /// Write a code/value pair with a string value
    fn write_string(&mut self, code: i32, value: &str) -> Result<()>;

    /// Write an XRecord string without applying MTEXT-specific substitutions.
    fn write_xrecord_string(&mut self, code: i32, value: &str) -> Result<()>;

    /// Write a code/value pair with a byte value (for codes 280-289)
    fn write_byte(&mut self, code: i32, value: u8) -> Result<()>;

    /// Write a code/value pair with an integer value
    fn write_i16(&mut self, code: i32, value: i16) -> Result<()>;

    /// Write a code/value pair with an i32 value
    fn write_i32(&mut self, code: i32, value: i32) -> Result<()>;

    /// Write a code/value pair with an i64 value
    fn write_i64(&mut self, code: i32, value: i64) -> Result<()>;

    /// Write a code/value pair with a double value
    fn write_double(&mut self, code: i32, value: f64) -> Result<()>;

    /// Write a code/value pair with a boolean value
    fn write_bool(&mut self, code: i32, value: bool) -> Result<()>;

    /// Write a code/value pair with a handle value
    fn write_handle(&mut self, code: i32, handle: Handle) -> Result<()>;

    /// Write binary data
    fn write_binary(&mut self, code: i32, data: &[u8]) -> Result<()>;

    /// Flush the writer
    fn flush(&mut self) -> Result<()>;
}

/// Extension trait for convenient writing operations
pub trait DxfStreamWriterExt: DxfStreamWriter {
    /// Write a 2D point (codes 10/20 or similar)
    #[inline]
    fn write_point2d(&mut self, x_code: i32, point: Vector2) -> Result<()> {
        self.write_double(x_code, point.x)?;
        self.write_double(x_code + 10, point.y)?;
        Ok(())
    }

    /// Write a 3D point (codes 10/20/30 or similar)
    #[inline]
    fn write_point3d(&mut self, x_code: i32, point: Vector3) -> Result<()> {
        self.write_double(x_code, point.x)?;
        self.write_double(x_code + 10, point.y)?;
        self.write_double(x_code + 20, point.z)?;
        Ok(())
    }

    /// Write a color on the given code as an ACI index.
    ///
    /// For `Rgb` colors an approximate ACI index is written so that older
    /// readers can still display a reasonable colour.  The caller is
    /// responsible for also emitting code 420 (true colour) when required.
    #[inline]
    fn write_color(&mut self, code: i32, color: Color) -> Result<()> {
        match color {
            Color::ByLayer => self.write_i16(code, 256),
            Color::None => self.write_i16(code, 257),
            Color::ByBlock => self.write_i16(code, 0),
            Color::Index(index) => self.write_i16(code, index as i16),
            Color::Rgb { .. } => self.write_i16(code, color.approximate_index()),
        }
    }

    /// Write common entity header
    #[inline]
    fn write_entity_type(&mut self, entity_type: &str) -> Result<()> {
        self.write_string(0, entity_type)
    }

    /// Write a subclass marker
    #[inline]
    fn write_subclass(&mut self, marker: &str) -> Result<()> {
        self.write_string(100, marker)
    }

    /// Write section start
    #[inline]
    fn write_section_start(&mut self, section_name: &str) -> Result<()> {
        self.write_string(0, "SECTION")?;
        self.write_string(2, section_name)?;
        Ok(())
    }

    /// Write section end
    #[inline]
    fn write_section_end(&mut self) -> Result<()> {
        self.write_string(0, "ENDSEC")
    }

    /// Write end of file
    #[inline]
    fn write_eof(&mut self) -> Result<()> {
        self.write_string(0, "EOF")
    }
}

// Auto-implement the extension trait for all stream writers
impl<T: DxfStreamWriter> DxfStreamWriterExt for T {}

/// Helper to determine value type from code for writing
pub fn value_type_for_code(code: i32) -> GroupCodeValueType {
    use crate::io::dxf::DxfCode;
    let dxf_code = DxfCode::from_i32(code);
    GroupCodeValueType::from_code(dxf_code)
}
