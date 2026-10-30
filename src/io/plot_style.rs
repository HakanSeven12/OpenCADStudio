//! Plot style tables in Open CAD Studio: the plot styles folder, the tables
//! registered this session, and the shipped ones. The table model and its
//! CTB/STB codec live in `codec::plot_style`.

use rustc_hash::FxHashMap as HashMap;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

pub use codec::plot_style::{decode_plot_style_text, PlotStyleEntry, PlotStyleTable, LW_TABLE};

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
        // Empty on native (nothing registers there); on the web this is
        // where the tables picked this session surface.
        names.extend(session_ctb_names());
        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        names
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut names = vec![DEFAULT_PLOT_STYLE.into(), MONOCHROME_PLOT_STYLE.into()];
        names.extend(STANDARD_PLOT_STYLES.iter().map(|name| (*name).to_string()));
        // No folder to scan — the tables picked this session stand in.
        names.extend(session_ctb_names());
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

/// The colour-dependent (.ctb) names picked this session, for the Plot
/// dialog's table list. Named (.stb) tables are left out: the plotter is
/// color-dependent, so they cannot be applied anyway.
fn session_ctb_names() -> Vec<String> {
    session_tables()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .keys()
        .filter(|name| name.to_ascii_lowercase().ends_with(".ctb"))
        .cloned()
        .collect()
}

/// Where Open CAD Studio finds plot style tables: on disk, in this session's
/// registry, and among the shipped ones.
pub trait PlotStyleTableExt: Sized {
    fn load(path: &Path) -> Result<Self, String>;
    fn builtin(name: &str) -> Result<Self, String>;
    fn load_named(name: &str) -> Result<Self, String>;
    fn save(&self, path: &Path) -> Result<(), String>;
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
                    if let Some((r, g, b)) = codec::types::aci_to_rgb(aci) {
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
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("stb"))
        {
            return Err(
                crate::tf!("Named plot style tables (.stb) are not supported yet: {name}")
                    .into_owned(),
            );
        }
        if !path
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
            return match matched {
                Some(path) => Self::load(&path),
                None => Self::builtin(name).map_err(|_| {
                    // Not a built-in either: say where a copy would be found.
                    let folder = dir.display();
                    crate::tf!("{name} was not found in the plot styles folder ({folder}).")
                        .into_owned()
                }),
            };
        }

        #[cfg(target_arch = "wasm32")]
        Self::builtin(name)
    }

    /// Write this table to disk as a CTB/STB file.
    fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = self.to_bytes().map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }
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
    /// tables, but never a named (.stb) one.
    #[test]
    fn available_names_include_session_registered_tables() {
        let bytes = PlotStyleTable::identity("x").to_bytes().expect("encode");
        let bytes = std::sync::Arc::new(bytes);
        super::register_session_table("DialogPens.ctb", std::sync::Arc::clone(&bytes));
        super::register_session_table("Named.stb", bytes);
        let names = super::available_ctb_names();
        assert!(
            names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("DialogPens.ctb")),
            "{names:?}"
        );
        assert!(
            !names
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
    fn named_tables_and_missing_files_get_a_reason() {
        let error = PlotStyleTable::load_named("styles.stb").unwrap_err();
        assert!(error.contains(".stb"), "{error}");
        let error = PlotStyleTable::load_named("nowhere-to-be-found.ctb").unwrap_err();
        assert!(error.contains("nowhere-to-be-found.ctb"), "{error}");
        assert!(PlotStyleTable::load_named("../escape.ctb").is_err());
    }
}

