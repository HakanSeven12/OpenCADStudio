//! Transparency representation for CAD entities

use std::fmt;

/// Transparency source and explicit amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Transparency {
    /// Use the owning layer's transparency.
    ByLayer,
    /// Use the containing block reference's transparency.
    ByBlock,
    /// Explicit transparency amount: 0 is opaque and 255 is transparent.
    Explicit(u8),
}

impl Transparency {
    pub const OPAQUE: Self = Self::Explicit(0);
    pub const TRANSPARENT: Self = Self::Explicit(255);
    pub const BY_LAYER: Self = Self::ByLayer;
    pub const BY_BLOCK: Self = Self::ByBlock;

    pub const fn new(alpha: u8) -> Self {
        Self::Explicit(alpha)
    }

    pub fn from_percent(percent: f64) -> Self {
        Self::Explicit((percent.clamp(0.0, 1.0) * 255.0) as u8)
    }

    /// Decode a packed DXF or DWG transparency value.
    pub fn from_alpha_value(value: u32) -> Self {
        match (value >> 24) as u8 {
            0 => Self::ByLayer,
            1 => Self::ByBlock,
            2 | 3 => Self::Explicit(255 - (value & 0xFF) as u8),
            _ => Self::ByLayer,
        }
    }

    /// Return the explicit amount, or zero for inherited values.
    pub const fn alpha(&self) -> u8 {
        match self {
            Self::Explicit(alpha) => *alpha,
            Self::ByLayer | Self::ByBlock => 0,
        }
    }

    pub const fn explicit_alpha(&self) -> Option<u8> {
        match self {
            Self::Explicit(alpha) => Some(*alpha),
            Self::ByLayer | Self::ByBlock => None,
        }
    }

    pub fn as_percent(&self) -> f64 {
        self.alpha() as f64 / 255.0
    }

    pub const fn is_by_layer(&self) -> bool {
        matches!(self, Self::ByLayer)
    }

    pub const fn is_by_block(&self) -> bool {
        matches!(self, Self::ByBlock)
    }

    pub const fn is_explicit(&self) -> bool {
        matches!(self, Self::Explicit(_))
    }

    pub const fn is_opaque(&self) -> bool {
        matches!(self, Self::Explicit(0))
    }

    pub const fn is_transparent(&self) -> bool {
        matches!(self, Self::Explicit(255))
    }

    pub const T_10: Self = Self::Explicit(26);
    pub const T_20: Self = Self::Explicit(51);
    pub const T_30: Self = Self::Explicit(77);
    pub const T_40: Self = Self::Explicit(102);
    pub const T_50: Self = Self::Explicit(128);
    pub const T_60: Self = Self::Explicit(153);
    pub const T_70: Self = Self::Explicit(179);
    pub const T_80: Self = Self::Explicit(204);
    pub const T_90: Self = Self::Explicit(230);

    /// Encode the DWG packed form.
    pub fn to_alpha_value(&self) -> i32 {
        match self {
            Self::ByLayer => 0,
            Self::ByBlock => (1u32 << 24) as i32,
            Self::Explicit(alpha) => ((3u32 << 24) | (255 - *alpha) as u32) as i32,
        }
    }

    /// Encode the DXF packed form.
    pub fn to_dxf_value(&self) -> i32 {
        match self {
            Self::ByLayer => 0,
            Self::ByBlock => (1u32 << 24) as i32,
            Self::Explicit(alpha) => ((2u32 << 24) | (255 - *alpha) as u32) as i32,
        }
    }
}

impl Default for Transparency {
    fn default() -> Self {
        Self::ByLayer
    }
}

impl From<u8> for Transparency {
    fn from(alpha: u8) -> Self {
        Self::Explicit(alpha)
    }
}

impl From<Transparency> for u8 {
    fn from(transparency: Transparency) -> Self {
        transparency.alpha()
    }
}

impl fmt::Display for Transparency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ByLayer => write!(f, "ByLayer"),
            Self::ByBlock => write!(f, "ByBlock"),
            Self::Explicit(_) => write!(f, "{:.1}%", self.as_percent() * 100.0),
        }
    }
}

#[cfg(feature = "serde")]
pub(crate) const fn opaque() -> Transparency {
    Transparency::OPAQUE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_amounts() {
        let transparency = Transparency::new(128);
        assert_eq!(transparency.explicit_alpha(), Some(128));
        assert_eq!(Transparency::from_percent(0.5).alpha(), 127);
        assert!(Transparency::OPAQUE.is_opaque());
        assert!(Transparency::TRANSPARENT.is_transparent());
    }

    #[test]
    fn packed_methods_roundtrip() {
        for value in [
            Transparency::BY_LAYER,
            Transparency::BY_BLOCK,
            Transparency::OPAQUE,
            Transparency::new(217),
            Transparency::TRANSPARENT,
        ] {
            assert_eq!(
                Transparency::from_alpha_value(value.to_dxf_value() as u32),
                value
            );
            assert_eq!(
                Transparency::from_alpha_value(value.to_alpha_value() as u32),
                value
            );
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_preserves_method() {
        for value in [
            Transparency::BY_LAYER,
            Transparency::BY_BLOCK,
            Transparency::OPAQUE,
            Transparency::T_50,
        ] {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(serde_json::from_str::<Transparency>(&json).unwrap(), value);
        }
    }
}
