//! Plot style tables in Open CAD Studio: the plot styles folder, the tables
//! registered this session, and the shipped ones. The table model and its
//! CTB/STB codec live in `document::plot_style`.

use rustc_hash::FxHashMap as HashMap;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

pub use document::plot_style::{decode_plot_style_text, PlotStyleEntry, PlotStyleTable, LW_TABLE};

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
static PLOT_STYLES_FOLDER: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// A folder chosen in Options to hold the plot style tables instead of the
/// default one, such as a share on the office network.
#[cfg(not(target_arch = "wasm32"))]
pub fn plot_styles_folder() -> Option<PathBuf> {
    PLOT_STYLES_FOLDER
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set_plot_styles_folder(folder: Option<PathBuf>) {
    *PLOT_STYLES_FOLDER
        .write()
        .unwrap_or_else(|e| e.into_inner()) = folder;
}

#[cfg(not(target_arch = "wasm32"))]
pub fn plot_styles_dir() -> Result<PathBuf, String> {
    if let Some(folder) = plot_styles_folder() {
        return Ok(folder);
    }
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

/// Plot style tables available to the Plot dialog: CTB and STB files in the
/// folder, the shipped tables, and the tables picked this session.
pub fn available_table_names() -> Vec<String> {
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
                    .is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("ctb") || ext.eq_ignore_ascii_case("stb")
                    })
                    .then(|| entry.file_name().to_string_lossy().into_owned())
            })
            .collect();
        names.extend(STANDARD_PLOT_STYLES.iter().map(|name| (*name).to_string()));
        // Empty on native (nothing registers there); on the web this is
        // where the tables picked this session surface.
        names.extend(session_table_names());
        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        names
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut names = vec![DEFAULT_PLOT_STYLE.into(), MONOCHROME_PLOT_STYLE.into()];
        names.extend(STANDARD_PLOT_STYLES.iter().map(|name| (*name).to_string()));
        // No folder to scan — the tables picked this session stand in.
        names.extend(session_table_names());
        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        names
    }
}

// ── Web session registry ────────────────────────────────────────────────────

/// Plot tables picked this session, keyed by file name. The browser keeps
/// no folder behind a picked name, so the web build remembers the bytes
/// instead and `load_named` resolves through here before falling back to
/// the built-ins — the plot-style counterpart of `pdf_raster::register_source`.
/// Native never writes it, so the lookup is always empty there and the
/// plot-styles folder stays authoritative.
fn session_tables() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
    static TABLES: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    TABLES.get_or_init(|| Mutex::new(HashMap::default()))
}

/// Remember a picked table's bytes so later `load_named` calls — the Plot
/// dialog's table list and APPLY, plot settings, display plot style —
/// resolve its name. Web only; on native, permanently registering a picked
/// table copies it into the plot-styles folder instead.
pub fn register_session_table(name: &str, bytes: Arc<Vec<u8>>) {
    session_tables()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(name.to_string(), bytes);
}

/// The bytes registered under `name` this session, matched
/// case-insensitively the way the folder scan matches file names. The web
/// ARCHIVE/ETRANSMIT packages a drawing's registered plot table into the
/// transmittal; `None` means the table was not picked this session (the
/// built-ins ship with the app and need no transmittal copy).
pub fn session_table(name: &str) -> Option<Arc<Vec<u8>>> {
    session_tables()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(registered, _)| registered.eq_ignore_ascii_case(name))
        .map(|(_, bytes)| Arc::clone(bytes))
}

/// The colour-dependent (.ctb) and named (.stb) tables picked this session,
/// for the Plot dialog's table list. Named tables now apply: objects resolve
/// through the table's `Normal` object style.
fn session_table_names() -> Vec<String> {
    session_tables()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .keys()
        .cloned()
        .collect()
}

/// The `Normal` object style of a named table, whether by exact or
/// case-insensitive name; the first entry when there is no `Normal`.
fn normal_entry(table: &PlotStyleTable) -> Option<&PlotStyleEntry> {
    table
        .named_entries
        .get("Normal")
        .or_else(|| {
            table
                .named_entries
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("Normal"))
                .map(|(_, entry)| entry)
        })
        .or_else(|| table.named_entries.values().next())
}

    // Names in a named table: the packed RGB the codec reads and writes.
    fn packed_rgb([r, g, b]: [u8; 3]) -> i32 {
        u32::from_be_bytes([0xC2, r, g, b]) as i32
    }

    /// Encode a named-style (STB) table as the file bytes the codec reads
    /// back. `PlotStyleTable::to_bytes` writes colour tables, so STB tables
    /// need their own writer.
    pub fn encode_stb(table: &PlotStyleTable) -> Result<Vec<u8>, String> {
        let mut body = String::new();
        let description = table.description.replace(['\r', '\n'], " ");
        body.push_str(&format!("description=\"{description}\n"));
        body.push_str("aci_table_available=TRUE\n");
        body.push_str(&format!(
            "scale_factor={}\n",
            full_real_stb(table.scale_factor)
        ));
        body.push_str(&format!(
            "apply_factor={}\n",
            if table.apply_factor { "TRUE" } else { "FALSE" }
        ));
        body.push_str(&format!(
            "custom_lineweight_display_units={}\n",
            table.custom_lineweight_display_units
        ));
        body.push_str("plot_style{\n");
        for (index, (name, entry)) in table.named_entries.iter().enumerate() {
            let localized_name = if entry.localized_name.is_empty() {
                name.as_str()
            } else {
                entry.localized_name.as_str()
            };
            body.push_str(&format!(" {index}{{\n"));
            body.push_str(&format!("  name=\"{name}\n"));
            body.push_str(&format!("  localized_name=\"{localized_name}\n"));
            body.push_str(&format!(
                "  description=\"{}\n",
                entry.description.replace(['\r', '\n'], " ")
            ));
            let color = entry.color.map(packed_rgb).unwrap_or(-1);
            body.push_str(&format!("  color={color}\n  mode_color={color}\n"));
            body.push_str(&format!("  color_policy={}\n", entry.color_policy));
            body.push_str(&format!(
                "  physical_pen_number={}\n  virtual_pen_number={}\n",
                entry.physical_pen_number, entry.virtual_pen_number
            ));
            body.push_str(&format!("  screen={}\n", entry.screening));
            body.push_str(&format!(
                "  linepattern_size={}\n  linetype={}\n  adaptive_linetype={}\n",
                entry.linepattern_size,
                entry.linetype,
                if entry.adaptive_linetype { "TRUE" } else { "FALSE" }
            ));
            body.push_str(&format!("  lineweight={}\n", entry.lineweight));
            body.push_str(&format!(
                "  fill_style={}\n  end_style={}\n  join_style={}\n }}\n",
                entry.fill_style, entry.end_style, entry.join_style
            ));
        }
        body.push_str("}\ncustom_lineweight_table{\n");
        for (index, weight) in table.lineweights.iter().enumerate() {
            body.push_str(&format!(" {index}={}\n", full_real_stb(*weight)));
        }
        body.push_str("}\n");
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write as _;
        encoder
            .write_all(body.as_bytes())
            .map_err(|error| error.to_string())?;
        encoder.finish().map_err(|error| error.to_string())
    }

    fn full_real_stb(value: f32) -> String {
        let rendered = format!("{value:.10}");
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }

/// A named table (STB) narrowed to one object style: the copy plots every
/// object with `style` the way the full table plots `Normal`, so the colour,
/// lineweight and screening resolvers apply it unchanged. `None` for a colour
/// table, or for a style the table does not define (the object then plots
/// with `Normal`, as the reference application does).
pub fn with_object_style(table: &PlotStyleTable, style: &str) -> Option<PlotStyleTable> {
    if !table.is_stb {
        return None;
    }
    let entry = table
        .named_entries
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(style))
        .map(|(_, entry)| entry.clone())?;
    let mut narrowed = table.clone();
    narrowed.named_entries.clear();
    narrowed.named_entries.insert("Normal".to_string(), entry);
    Some(narrowed)
}

/// Where Open CAD Studio finds plot style tables: on disk, in this session's
/// registry, and among the shipped ones.
pub trait PlotStyleTableExt: Sized {
    fn load(path: &Path) -> Result<Self, String>;
    fn builtin(name: &str) -> Result<Self, String>;
    fn load_named(name: &str) -> Result<Self, String>;
    fn save(&self, path: &Path) -> Result<(), String>;
    /// The plotted colour of an object, honouring a named table's `Normal`
    /// object style (STB) as well as a colour table's ACI row (CTB).
    fn resolve_style_color(&self, aci: u8) -> Option<[f32; 3]>;
    /// The plotted lineweight in millimetres, or `None` to keep the
    /// object's own lineweight.
    fn resolve_style_lineweight(&self, aci: u8) -> Option<f32>;
    /// Screening factor 0–1 for the style this object takes.
    fn resolve_style_screening(&self, aci: u8) -> f32;
}

impl PlotStyleTableExt for PlotStyleTable {
    /// Load a CTB or STB file from disk.
    fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Self::from_bytes(name, &raw).map_err(|e| e.to_string())
    }

    fn builtin(name: &str) -> Result<Self, String> {
        match name.to_ascii_lowercase().as_str() {
            DEFAULT_PLOT_STYLE => Self::from_bytes(
                DEFAULT_PLOT_STYLE,
                include_bytes!("../../assets/plotstyles/ocad.ctb"),
            )
            .map_err(|e| e.to_string()),
            MONOCHROME_PLOT_STYLE => Self::from_bytes(
                MONOCHROME_PLOT_STYLE,
                include_bytes!("../../assets/plotstyles/monochrome.ctb"),
            )
            .map_err(|e| e.to_string()),
            "grayscale.ctb" => {
                let mut table = Self::identity(GRAYSCALE_PLOT_STYLE);
                for aci in 1..=255u8 {
                    if let Some((r, g, b)) = document::types::aci_to_rgb(aci) {
                        let gray = if aci == 7 {
                            0
                        } else {
                            (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8
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
            "screening 100%.ctb" => Ok(screening(SCREENING_100_PLOT_STYLE, 100)),
            "screening 75%.ctb" => Ok(screening(SCREENING_75_PLOT_STYLE, 75)),
            "screening 50%.ctb" => Ok(screening(SCREENING_50_PLOT_STYLE, 50)),
            "screening 25%.ctb" => Ok(screening(SCREENING_25_PLOT_STYLE, 25)),
            // The named tables every reference install ships, so a layout set
            // up with one plots without a copy in the plot styles folder.
            "acad.stb" | "autodesk-color.stb" => Ok(named_normal(name, None)),
            "monochrome.stb" | "autodesk-mono.stb" => Ok(named_normal(name, Some([0, 0, 0]))),
            _ => Err(format!("Unknown built-in plot style: {name}")),
        }
    }

    /// Load one CTB by file name from the per-user plot styles folder.
    fn load_named(name: &str) -> Result<Self, String> {
        let path = Path::new(name);
        if path.components().count() != 1
            || !matches!(path.components().next(), Some(Component::Normal(_)))
        {
            return Err(format!("Invalid plot style name: {name}"));
        }
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("stb"))
            && !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ctb"))
        {
            return Err(format!("Invalid plot style name: {name}"));
        }

        // A table picked this session resolves through the registry first.
        // Empty on native, so the folder search below stays authoritative
        // there; on the web this is the only place a picked table lives.
        if let Some(bytes) = session_table(name) {
            return Self::from_bytes(name, &bytes).map_err(|e| e.to_string());
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
            match matched {
                Some(path) => Self::load(&path),
                None => Self::builtin(name).map_err(|_| {
                    // Not a built-in either: say where a copy would be found.
                    let folder = dir.display();
                    crate::tf!("{name} was not found in the plot styles folder ({folder}).")
                        .into_owned()
                }),
            }
        }

        #[cfg(target_arch = "wasm32")]
        Self::builtin(name)
    }

    /// Write this table to disk as a CTB/STB file.
    fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = self.to_bytes().map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    fn resolve_style_color(&self, aci: u8) -> Option<[f32; 3]> {
        if !self.is_stb {
            return self.resolve_color(aci);
        }
        let entry = normal_entry(self)?;
        let mut color = entry
            .color
            .map(|[r, g, b]| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]);
        if entry.color_policy & 2 != 0 {
            let rgb = color.or_else(|| {
                if aci == 7 {
                    Some([0.0; 3])
                } else {
                    document::types::aci_to_rgb(aci).map(|(r, g, b)| {
                        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
                    })
                }
            })?;
            let gray = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
            color = Some([gray; 3]);
        }
        color
    }

    fn resolve_style_lineweight(&self, aci: u8) -> Option<f32> {
        if !self.is_stb {
            return self.resolve_lineweight(aci);
        }
        let entry = normal_entry(self)?;
        if matches!(entry.lineweight, 0 | 255) {
            None
        } else {
            self.lineweights
                .get(entry.lineweight as usize)
                .copied()
                .map(|weight| {
                    if self.apply_factor {
                        weight * self.scale_factor.max(0.0)
                    } else {
                        weight
                    }
                })
        }
    }

    fn resolve_style_screening(&self, aci: u8) -> f32 {
        if self.is_stb {
            return normal_entry(self)
                .map(|entry| entry.screening.min(100) as f32 / 100.0)
                .unwrap_or(1.0);
        }
        self.resolve_screening(aci)
    }
}

/// A named table holding only `Normal`: `color` replaces every object's
/// colour, or `None` keeps it.
fn named_normal(name: &str, color: Option<[u8; 3]>) -> PlotStyleTable {
    let mut table = PlotStyleTable::identity(name);
    table.is_stb = true;
    let mut normal = PlotStyleEntry::default();
    normal.name = "Normal".to_string();
    normal.color = color;
    normal.color_policy = 0;
    normal.screening = 100;
    table.named_entries.insert("Normal".to_string(), normal);
    table
}

/// A table that screens every color to `percent`.
fn screening(name: &str, percent: u8) -> PlotStyleTable {
    let mut table = PlotStyleTable::identity(name);
    for entry in table.aci_entries.iter_mut().skip(1) {
        entry.screening = percent;
    }
    table
}

#[cfg(test)]
mod round_trip_tests {
    use super::{PlotStyleTable, PlotStyleTableExt};

    /// A table registered this session (the web stand-in for the plot-styles
    /// folder) is what `load_named` returns for its name — matched
    /// case-insensitively, the way the folder scan matches file names.
    #[test]
    fn load_named_resolves_a_session_registered_table() {
        let mut table = PlotStyleTable::identity("SessionPens.ctb");
        table.aci_entries[1].lineweight = 13; // 0.50 mm
        let bytes = table.to_bytes().expect("encode");
        super::register_session_table("SessionPens.ctb", std::sync::Arc::new(bytes));

        let loaded = PlotStyleTable::load_named("sessionpens.CTB").expect("registry hit");
        assert_eq!(loaded.name, "sessionpens.CTB");
        assert_eq!(loaded.resolve_lineweight(1), Some(0.50));

        // A name nothing was registered under still falls back to the built-ins.
        assert!(PlotStyleTable::load_named(super::MONOCHROME_PLOT_STYLE).is_ok());
    }

    /// The Plot dialog's table list includes the session-registered colour
    /// tables and the named (.stb) ones, since named tables now apply.
    #[test]
    fn available_names_include_session_registered_tables() {
        let bytes = PlotStyleTable::identity("x").to_bytes().expect("encode");
        let bytes = std::sync::Arc::new(bytes);
        super::register_session_table("DialogPens.ctb", std::sync::Arc::clone(&bytes));
        super::register_session_table("Named.stb", bytes);
        let names = super::available_table_names();
        assert!(
            names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("DialogPens.ctb")),
            "{names:?}"
        );
        assert!(
            names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("Named.stb")),
            "{names:?}"
        );
    }
}

#[cfg(test)]
mod lenient_loading_tests {
    use super::{PlotStyleTable, PlotStyleTableExt};

    #[test]
    fn standard_named_tables_load_without_a_file() {
        for name in ["acad.stb", "Autodesk-Color.stb"] {
            let table = PlotStyleTable::load_named(name).expect("shipped named table");
            assert!(table.is_stb);
            assert_eq!(table.resolve_style_color(1), None, "{name} keeps object colour");
        }
        let mono = PlotStyleTable::load_named("monochrome.stb").expect("monochrome.stb");
        assert_eq!(mono.resolve_style_color(1), Some([0.0; 3]));
        assert!(super::with_object_style(&mono, "Normal").is_some());
        assert!(super::with_object_style(&mono, "Nope").is_none());
    }

    #[test]
    fn named_tables_and_missing_files_get_a_reason() {
        // A named table now loads: the `Normal` object style, resolving
        // object colour/lineweight/screening, is what the plot honours.
        let mut table = PlotStyleTable::identity("Mono.stb");
        table.is_stb = true;
        let mut normal = crate::io::plot_style::PlotStyleEntry::default();
        normal.name = "Normal".to_string();
        normal.color = Some([0, 0, 0]);
        normal.color_policy = 0;
        normal.screening = 100;
        table.named_entries.insert("Normal".to_string(), normal);
        let bytes = super::encode_stb(&table).expect("encode STB");
        let loaded = PlotStyleTable::from_bytes("Mono.stb", &bytes).expect("STB round-trips");
        assert!(loaded.is_stb);
        assert_eq!(loaded.named_entries["Normal"].color, Some([0, 0, 0]));
        assert_eq!(loaded.resolve_style_color(1), Some([0.0, 0.0, 0.0]));
        assert_eq!(loaded.resolve_style_color(7), Some([0.0, 0.0, 0.0]));
        let error = PlotStyleTable::load_named("nowhere-to-be-found.stb").unwrap_err();
        assert!(
            error.to_lowercase().contains("stb"),
            "the refusal names the extension: {error}"
        );
        let error = PlotStyleTable::load_named("nowhere-to-be-found.ctb").unwrap_err();
        assert!(error.contains("nowhere-to-be-found.ctb"), "{error}");
        assert!(PlotStyleTable::load_named("../escape.ctb").is_err());
    }
}
