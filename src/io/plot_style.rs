//! Plot Style Table — CTB (color-based) and STB (named) file support.
//!
//! CTB files map indexed drawing colors (ACI, 1-255) to pen properties:
//! RGB color override, lineweight, and screening percentage.
//!
//! File format: a fixed 60-byte header followed by zlib-compressed text.
//!
//! STB files follow the same format but use named styles instead of
//! ACI indices; they are read into a `Vec<NamedPlotStyle>`.

use rustc_hash::FxHashMap as HashMap;
use std::io::Read;
use std::path::{Component, Path};

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

pub const DEFAULT_PLOT_STYLE: &str = "ocad.ctb";
pub const MONOCHROME_PLOT_STYLE: &str = "monochrome.ctb";
pub const GRAYSCALE_PLOT_STYLE: &str = "Grayscale.ctb";
pub const FILL_PATTERNS_PLOT_STYLE: &str = "Fill Patterns.ctb";
pub const SCREENING_100_PLOT_STYLE: &str = "Screening 100%.ctb";
pub const SCREENING_75_PLOT_STYLE: &str = "Screening 75%.ctb";
pub const SCREENING_50_PLOT_STYLE: &str = "Screening 50%.ctb";
pub const SCREENING_25_PLOT_STYLE: &str = "Screening 25%.ctb";

const STANDARD_PLOT_STYLES: &[&str] = &[
    GRAYSCALE_PLOT_STYLE,
    FILL_PATTERNS_PLOT_STYLE,
    SCREENING_100_PLOT_STYLE,
    SCREENING_75_PLOT_STYLE,
    SCREENING_50_PLOT_STYLE,
    SCREENING_25_PLOT_STYLE,
];

#[cfg(not(target_arch = "wasm32"))]
pub fn plot_styles_dir() -> Result<PathBuf, String> {
    crate::config::config_dir()
        .map(|path| path.join("plotstyles"))
        .ok_or_else(|| "Plot styles folder could not be resolved".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn ensure_plot_styles_dir() -> Result<PathBuf, String> {
    let dir = plot_styles_dir()?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    for (name, bytes) in [
        (
            DEFAULT_PLOT_STYLE,
            include_bytes!("../../assets/plotstyles/ocad.ctb").as_slice(),
        ),
        (
            MONOCHROME_PLOT_STYLE,
            include_bytes!("../../assets/plotstyles/monochrome.ctb").as_slice(),
        ),
    ] {
        let path = dir.join(name);
        if !path.exists() {
            std::fs::write(path, bytes).map_err(|error| error.to_string())?;
        }
    }
    Ok(dir)
}

/// CTB files available to the Plot dialog.
pub fn available_ctb_names() -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(dir) = ensure_plot_styles_dir() else {
            let mut names = vec![DEFAULT_PLOT_STYLE.into(), MONOCHROME_PLOT_STYLE.into()];
            names.extend(STANDARD_PLOT_STYLES.iter().map(|name| (*name).to_string()));
            return names;
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            let mut names = vec![DEFAULT_PLOT_STYLE.into(), MONOCHROME_PLOT_STYLE.into()];
            names.extend(STANDARD_PLOT_STYLES.iter().map(|name| (*name).to_string()));
            return names;
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| {
                let path = entry.path();
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ctb"))
                    .then(|| entry.file_name().to_string_lossy().into_owned())
            })
            .collect();
        names.extend(STANDARD_PLOT_STYLES.iter().map(|name| (*name).to_string()));
        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        names
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut names = vec![DEFAULT_PLOT_STYLE.into(), MONOCHROME_PLOT_STYLE.into()];
        names.extend(STANDARD_PLOT_STYLES.iter().map(|name| (*name).to_string()));
        names
    }
}

// ── Standard lineweight table (index → mm) ───────────────────────────────────

/// Lineweight table: index value → mm, matching the stored LWEIGHT codes.
/// Index 0 = 0.00 mm (hairline), others follow the DXF lineweight enum.
pub const LW_TABLE: &[f32] = &[
    0.00, 0.05, 0.09, 0.10, 0.13, 0.15, 0.18, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50,
    0.53, 0.60, 0.65, 0.70, 0.80, 0.90, 1.00, 1.06, 1.20, 1.40, 1.58, 2.00, 2.11,
];

// ── Per-color entry ───────────────────────────────────────────────────────────

/// A single entry in a CTB or STB plot style table.
#[derive(Debug, Clone)]
pub struct PlotStyleEntry {
    pub name: String,
    pub localized_name: String,
    pub description: String,
    /// If `Some([r,g,b])`, override the entity color with this RGB value (0..255).
    /// If `None`, use the object color.
    pub color: Option<[u8; 3]>,
    /// Lineweight index into the table. 0 (and legacy 255) = object lineweight.
    pub lineweight: u8,
    /// Screen percentage 0–100 (100 = opaque).
    pub screening: u8,
    pub color_policy: u8,
    pub physical_pen_number: u16,
    pub virtual_pen_number: u16,
    pub linepattern_size: f32,
    pub linetype: u8,
    pub adaptive_linetype: bool,
    pub fill_style: u8,
    pub end_style: u8,
    pub join_style: u8,
}

impl Default for PlotStyleEntry {
    fn default() -> Self {
        PlotStyleEntry {
            name: String::new(),
            localized_name: String::new(),
            description: String::new(),
            color: None,
            lineweight: 0,
            screening: 100,
            color_policy: 1,
            physical_pen_number: 0,
            virtual_pen_number: 0,
            linepattern_size: 0.5,
            linetype: 31,
            adaptive_linetype: true,
            fill_style: 73,
            end_style: 4,
            join_style: 5,
        }
    }
}

// ── Plot Style Table ──────────────────────────────────────────────────────────

/// A loaded CTB or STB plot style table.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PlotStyleTable {
    /// File name (without path), e.g. "monochrome.ctb".
    pub name: String,
    /// Whether this is a named-style (STB) table rather than color-based (CTB).
    pub is_stb: bool,
    pub description: String,
    pub scale_factor: f32,
    pub apply_factor: bool,
    pub custom_lineweight_display_units: u8,
    pub lineweights: Vec<f32>,
    /// For CTB: entries indexed by ACI (index 0 unused; 1..=255 are valid).
    pub aci_entries: Vec<PlotStyleEntry>, // 256 entries, index = ACI
    /// For STB: named style entries.
    pub named_entries: HashMap<String, PlotStyleEntry>,
}

impl PlotStyleTable {
    /// Create an identity CTB table (no overrides for any color).
    pub fn identity(name: impl Into<String>) -> Self {
        PlotStyleTable {
            name: name.into(),
            is_stb: false,
            description: String::new(),
            scale_factor: 1.0,
            apply_factor: false,
            custom_lineweight_display_units: 0,
            lineweights: LW_TABLE.to_vec(),
            aci_entries: (0..=255).map(|_| PlotStyleEntry::default()).collect(),
            named_entries: HashMap::default(),
        }
    }

    /// Load a CTB or STB file from disk.
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Self::from_bytes(name, &raw)
    }

    pub fn from_bytes(name: impl Into<String>, raw: &[u8]) -> Result<Self, String> {
        let name = name.into();
        let is_stb = name.to_ascii_lowercase().ends_with(".stb");
        let text = decompress_ctb(raw)?;
        parse_plot_style_text(&text, name, is_stb)
    }

    pub fn builtin(name: &str) -> Result<Self, String> {
        match name.to_ascii_lowercase().as_str() {
            DEFAULT_PLOT_STYLE => Self::from_bytes(
                DEFAULT_PLOT_STYLE,
                include_bytes!("../../assets/plotstyles/ocad.ctb"),
            ),
            MONOCHROME_PLOT_STYLE => Self::from_bytes(
                MONOCHROME_PLOT_STYLE,
                include_bytes!("../../assets/plotstyles/monochrome.ctb"),
            ),
            "grayscale.ctb" => {
                let mut table = Self::identity(GRAYSCALE_PLOT_STYLE);
                for aci in 1..=255u8 {
                    if let Some((r, g, b)) = acadrust::types::aci_to_rgb(aci) {
                        let gray = if aci == 7 {
                            0
                        } else {
                            (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
                                .round() as u8
                        };
                        table.aci_entries[aci as usize].color = Some([gray, gray, gray]);
                    }
                }
                Ok(table)
            }
            "fill patterns.ctb" => {
                let mut table = Self::identity(FILL_PATTERNS_PLOT_STYLE);
                for aci in 1..=9usize {
                    table.aci_entries[aci].fill_style = 63 + aci as u8;
                }
                Ok(table)
            }
            "screening 100%.ctb" => Ok(Self::screening(SCREENING_100_PLOT_STYLE, 100)),
            "screening 75%.ctb" => Ok(Self::screening(SCREENING_75_PLOT_STYLE, 75)),
            "screening 50%.ctb" => Ok(Self::screening(SCREENING_50_PLOT_STYLE, 50)),
            "screening 25%.ctb" => Ok(Self::screening(SCREENING_25_PLOT_STYLE, 25)),
            _ => Err(format!("Unknown built-in plot style: {name}")),
        }
    }

    fn screening(name: &str, percent: u8) -> Self {
        let mut table = Self::identity(name);
        for entry in table.aci_entries.iter_mut().skip(1) {
            entry.screening = percent;
        }
        table
    }

    /// Load one CTB by file name from the per-user plot styles folder.
    pub fn load_named(name: &str) -> Result<Self, String> {
        let path = Path::new(name);
        if path.components().count() != 1
            || !matches!(path.components().next(), Some(Component::Normal(_)))
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ctb"))
        {
            return Err(format!("Invalid plot style name: {name}"));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = ensure_plot_styles_dir()?;
            let matched = std::fs::read_dir(&dir)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(name)
                })
                .map(|entry| entry.path());
            return match matched {
                Some(path) => Self::load(&path),
                None => Self::builtin(name),
            };
        }

        #[cfg(target_arch = "wasm32")]
        Self::builtin(name)
    }

    /// Write this table to disk as a CTB/STB file.
    #[allow(dead_code)]
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = self.to_text();
        let compressed = compress_ctb(text.as_bytes())?;
        std::fs::write(path, compressed).map_err(|e| e.to_string())
    }

    /// Resolve the effective print RGB color for the given ACI index.
    /// Returns None if no override (use object color).
    pub fn resolve_color(&self, aci: u8) -> Option<[f32; 3]> {
        let entry = self.aci_entries.get(aci as usize)?;
        let mut color = entry
            .color
            .map(|[r, g, b]| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]);
        if entry.color_policy & 2 != 0 {
            let rgb = color.or_else(|| {
                if aci == 7 {
                    Some([0.0; 3])
                } else {
                    acadrust::types::aci_to_rgb(aci).map(|(r, g, b)| {
                        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
                    })
                }
            })?;
            let gray = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
            color = Some([gray; 3]);
        }
        color
    }

    /// Resolve the effective lineweight in mm for the given ACI index.
    /// Returns None if no override (use object lineweight).
    pub fn resolve_lineweight(&self, aci: u8) -> Option<f32> {
        let entry = self.aci_entries.get(aci as usize)?;
        if matches!(entry.lineweight, 0 | 255) {
            None
        } else {
            self.lineweights.get(entry.lineweight as usize).copied().map(|weight| {
                if self.apply_factor {
                    weight * self.scale_factor.max(0.0)
                } else {
                    weight
                }
            })
        }
    }

    /// Resolve the screening factor for the given ACI index.
    pub fn resolve_screening(&self, aci: u8) -> f32 {
        self.aci_entries
            .get(aci as usize)
            .map(|entry| entry.screening.min(100) as f32 / 100.0)
            .unwrap_or(1.0)
    }

    // ── Internal serialisation ────────────────────────────────────────────

    fn to_text(&self) -> String {
        let mut s = String::new();
        let description = self.description.replace(['\r', '\n'], " ");
        s.push_str(&format!("description=\"{description}\n"));
        s.push_str("aci_table_available=TRUE\n");
        s.push_str(&format!("scale_factor={:.1}\n", self.scale_factor));
        s.push_str(&format!(
            "apply_factor={}\n",
            if self.apply_factor { "TRUE" } else { "FALSE" }
        ));
        s.push_str(&format!(
            "custom_lineweight_display_units={}\n",
            self.custom_lineweight_display_units
        ));
        s.push_str("aci_table{\n");
        for index in 0..255 {
            s.push_str(&format!(" {index}=\"Color_{}\n", index + 1));
        }
        s.push_str("}\nplot_style{\n");
        for (index, entry) in self.aci_entries.iter().enumerate().skip(1).take(255) {
            let style_index = index - 1;
            let style_name = if entry.name.is_empty() {
                format!("Color_{index}")
            } else {
                entry.name.replace(['\r', '\n'], " ")
            };
            let localized_name = if entry.localized_name.is_empty() {
                style_name.clone()
            } else {
                entry.localized_name.replace(['\r', '\n'], " ")
            };
            let description = entry.description.replace(['\r', '\n'], " ");
            s.push_str(&format!(" {style_index}{{\n"));
            s.push_str(&format!("  name=\"{style_name}\n"));
            s.push_str(&format!("  localized_name=\"{localized_name}\n"));
            s.push_str(&format!("  description=\"{description}\n"));
            if let Some(rgb) = entry.color {
                let packed = packed_rgb(rgb);
                s.push_str(&format!("  color={packed}\n  mode_color={packed}\n"));
            } else {
                s.push_str("  color=-1\n");
            }
            s.push_str(&format!("  color_policy={}\n", entry.color_policy));
            s.push_str(&format!(
                "  physical_pen_number={}\n  virtual_pen_number={}\n",
                entry.physical_pen_number, entry.virtual_pen_number
            ));
            s.push_str(&format!("  screen={}\n", entry.screening));
            s.push_str(&format!(
                "  linepattern_size={}\n  linetype={}\n  adaptive_linetype={}\n",
                entry.linepattern_size,
                entry.linetype,
                if entry.adaptive_linetype { "TRUE" } else { "FALSE" }
            ));
            s.push_str(&format!("  lineweight={}\n", entry.lineweight));
            s.push_str(&format!(
                "  fill_style={}\n  end_style={}\n  join_style={}\n }}\n",
                entry.fill_style, entry.end_style, entry.join_style
            ));
        }
        s.push_str("}\ncustom_lineweight_table{\n");
        for (index, weight) in self.lineweights.iter().enumerate() {
            s.push_str(&format!(" {index}={weight:.2}\n"));
        }
        s.push_str("}\n");
        s
    }
}

// ── Deflate helpers ───────────────────────────────────────────────────────────

/// Decompress a CTB/STB file's raw bytes into the text content.
///
fn decompress_ctb(data: &[u8]) -> Result<String, String> {
    const PREFIX: &[u8] = b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec";
    let mut decoded = Vec::new();
    if data.starts_with(PREFIX) {
        if data.len() < 60 {
            return Err("CTB header is truncated".into());
        }
        let checksum = u32::from_le_bytes(data[48..52].try_into().unwrap());
        let text_len = u32::from_le_bytes(data[52..56].try_into().unwrap()) as usize;
        let compressed_len = u32::from_le_bytes(data[56..60].try_into().unwrap()) as usize;
        if compressed_len > data.len() - 60 {
            return Err("CTB compressed payload is truncated".into());
        }
        let payload = &data[60..60 + compressed_len];
        if adler32(payload) != checksum {
            return Err("CTB compressed payload checksum mismatch".into());
        }
        use flate2::read::ZlibDecoder;
        ZlibDecoder::new(payload)
            .read_to_end(&mut decoded)
            .map_err(|e| format!("CTB zlib decompress: {e}"))?;
        if decoded.len() != text_len {
            return Err(format!(
                "CTB content length mismatch: expected {text_len}, got {}",
                decoded.len()
            ));
        }
    } else {
        let split_at = data
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let payload = &data[split_at..];
        if payload.starts_with(&[0x78]) {
            use flate2::read::ZlibDecoder;
            ZlibDecoder::new(payload)
                .read_to_end(&mut decoded)
                .map_err(|e| format!("legacy CTB zlib decompress: {e}"))?;
        } else {
            use flate2::read::DeflateDecoder;
            DeflateDecoder::new(payload)
                .read_to_end(&mut decoded)
                .map_err(|e| format!("legacy CTB deflate decompress: {e}"))?;
        }
    }
    if decoded.last() == Some(&0) {
        decoded.pop();
    }
    String::from_utf8(decoded).map_err(|e| format!("CTB text is not UTF-8: {e}"))
}

/// Compress plot-style text content as a CTB/STB file.
fn compress_ctb(text: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    const PREFIX: &[u8] = b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec";
    let mut body = text.to_vec();
    body.push(0);
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&body).map_err(|e| e.to_string())?;
    let compressed = enc.finish().map_err(|e| e.to_string())?;
    let mut out = PREFIX.to_vec();
    out.extend_from_slice(&adler32(&compressed).to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

fn adler32(bytes: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in bytes {
        a = (a + u32::from(*byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

fn packed_rgb([r, g, b]: [u8; 3]) -> i32 {
    u32::from_be_bytes([0xC2, r, g, b]) as i32
}

// ── Text parser ───────────────────────────────────────────────────────────────

fn parse_plot_style_text(text: &str, name: String, is_stb: bool) -> Result<PlotStyleTable, String> {
    if text.lines().any(|line| line.trim() == "begin_plot_style") {
        return parse_legacy_plot_style_text(text, name, is_stb);
    }

    #[derive(Default)]
    struct PendingStyle {
        index: usize,
        name: String,
        entry: PlotStyleEntry,
        color: Option<i32>,
        mode_color: Option<i32>,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        Root,
        Other,
        PlotStyles,
        Lineweights,
    }

    let mut aci_entries: Vec<PlotStyleEntry> =
        (0..=255).map(|_| PlotStyleEntry::default()).collect();
    let mut named_entries: HashMap<String, PlotStyleEntry> = HashMap::default();
    let mut description = String::new();
    let mut scale_factor = 1.0f32;
    let mut apply_factor = false;
    let mut custom_lineweight_display_units = 0u8;
    let mut lineweights = Vec::<f32>::new();
    let mut section = Section::Root;
    let mut current: Option<PendingStyle> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "plot_style{" {
            section = Section::PlotStyles;
            continue;
        }
        if line == "custom_lineweight_table{" {
            section = Section::Lineweights;
            continue;
        }
        if line.ends_with('{') {
            if section == Section::PlotStyles && current.is_none() {
                if let Ok(index) = line.trim_end_matches('{').trim().parse::<usize>() {
                    let default_name = format!("Color_{}", index + 1);
                    let mut style = PendingStyle {
                        index,
                        name: default_name.clone(),
                        ..Default::default()
                    };
                    style.entry.name = default_name.clone();
                    style.entry.localized_name = default_name;
                    current = Some(style);
                    continue;
                }
            }
            section = Section::Other;
            continue;
        }
        if line == "}" {
            if let Some(mut style) = current.take() {
                let packed = style.mode_color.or(style.color);
                style.entry.color = packed.and_then(unpack_plot_color);
                if is_stb {
                    named_entries.insert(style.name, style.entry);
                } else if style.index < 255 {
                    aci_entries[style.index + 1] = style.entry;
                }
            } else {
                section = Section::Root;
            }
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_start_matches('"');
        if let Some(style) = current.as_mut() {
            match key {
                "name" => {
                    style.name = value.to_string();
                    style.entry.name = value.to_string();
                }
                "localized_name" => style.entry.localized_name = value.to_string(),
                "description" => {
                    style.entry.description = value.to_string();
                }
                "screen" => {
                    if let Ok(v) = value.parse::<u8>() {
                        style.entry.screening = v.min(100);
                    }
                }
                "lineweight" => {
                    if let Ok(v) = value.parse::<u8>() {
                        style.entry.lineweight = v;
                    }
                }
                "color" => style.color = value.parse::<i32>().ok(),
                "mode_color" => style.mode_color = value.parse::<i32>().ok(),
                "color_policy" => style.entry.color_policy = value.parse().unwrap_or(1),
                "physical_pen_number" => {
                    style.entry.physical_pen_number = value.parse().unwrap_or(0)
                }
                "virtual_pen_number" => {
                    style.entry.virtual_pen_number = value.parse().unwrap_or(0)
                }
                "linepattern_size" => {
                    style.entry.linepattern_size = value.parse().unwrap_or(0.5)
                }
                "linetype" => style.entry.linetype = value.parse().unwrap_or(31),
                "adaptive_linetype" => {
                    style.entry.adaptive_linetype = value.eq_ignore_ascii_case("TRUE")
                }
                "fill_style" => style.entry.fill_style = value.parse().unwrap_or(73),
                "end_style" => style.entry.end_style = value.parse().unwrap_or(4),
                "join_style" => style.entry.join_style = value.parse().unwrap_or(5),
                _ => {}
            }
            continue;
        }

        match section {
            Section::Root => match key {
                "description" => description = value.to_string(),
                "scale_factor" => scale_factor = value.parse().unwrap_or(1.0),
                "apply_factor" => apply_factor = value.eq_ignore_ascii_case("TRUE"),
                "custom_lineweight_display_units" => {
                    custom_lineweight_display_units = value.parse().unwrap_or(0)
                }
                _ => {}
            },
            Section::Lineweights => {
                if let (Ok(index), Ok(weight)) = (key.parse::<usize>(), value.parse::<f32>()) {
                    if lineweights.len() <= index {
                        lineweights.resize(index + 1, 0.0);
                    }
                    lineweights[index] = weight;
                }
            }
            _ => {}
        }
    }

    if lineweights.is_empty() {
        lineweights = LW_TABLE.to_vec();
    }

    Ok(PlotStyleTable {
        name,
        is_stb,
        description,
        scale_factor,
        apply_factor,
        custom_lineweight_display_units,
        lineweights,
        aci_entries,
        named_entries,
    })
}

fn unpack_plot_color(packed: i32) -> Option<[u8; 3]> {
    if matches!(packed, -1 | -1_006_632_961 | -1_056_964_608) {
        return None;
    }
    let bytes = (packed as u32).to_be_bytes();
    Some([bytes[1], bytes[2], bytes[3]])
}

fn parse_legacy_plot_style_text(
    text: &str,
    name: String,
    is_stb: bool,
) -> Result<PlotStyleTable, String> {
    let mut table = PlotStyleTable::identity(name);
    table.is_stb = is_stb;
    let mut style_index = 1usize;
    let mut current: Option<PlotStyleEntry> = None;
    let mut current_name = String::new();
    for line in text.lines().map(str::trim) {
        if line == "begin_plot_style" {
            current = Some(PlotStyleEntry::default());
            current_name = format!("Color_{style_index}");
            continue;
        }
        if line == "end_plot_style" {
            if let Some(entry) = current.take() {
                if is_stb {
                    table.named_entries.insert(current_name.clone(), entry);
                } else if style_index <= 255 {
                    table.aci_entries[style_index] = entry;
                    style_index += 1;
                }
            }
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "description" => {
                entry.description = value.to_string();
                if !value.is_empty() {
                    current_name = value.to_string();
                }
            }
            "screen" => entry.screening = value.parse::<u8>().unwrap_or(100).min(100),
            "lineweight" => entry.lineweight = value.parse().unwrap_or(0),
            "color1" if value.starts_with('#') && value.len() == 7 => {
                entry.color = Some([
                    u8::from_str_radix(&value[1..3], 16).unwrap_or(0),
                    u8::from_str_radix(&value[3..5], 16).unwrap_or(0),
                    u8::from_str_radix(&value[5..7], 16).unwrap_or(0),
                ]);
            }
            "color1" => entry.color = value.parse::<i32>().ok().and_then(unpack_plot_color),
            _ => {}
        }
    }
    Ok(table)
}
