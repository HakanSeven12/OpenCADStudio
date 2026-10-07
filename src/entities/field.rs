//! Host-side glue for dynamic text field evaluation.
//!
//! The field language (DIESEL + AcVar), the field structure/linkage, and all
//! date math live in the reader library ([`codec::fields`]). OCS only
//! supplies the **environment** — the current clock, OS login and environment
//! variables — through [`FieldContext`]. A DWG library must stay deterministic
//! and platform-neutral, so the one genuinely system-specific bit (reading the
//! clock / OS user) lives here in the app instead.

use codec::fields::FieldContext;
use codec::types::Handle;
use codec::CadDocument;

/// OCS's environment provider for field evaluation. With the document at
/// hand it also answers the system variables the engine asks the host for.
struct OcsFieldContext<'a>(Option<&'a CadDocument>);

impl FieldContext for OcsFieldContext<'_> {
    fn now_julian(&self) -> f64 {
        // Fields show local time. Unix epoch is Julian Day 2440587.5.
        now_utc_julian() + utc_offset_days()
    }

    fn file_times(&self) -> Option<(f64, f64)> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let meta = std::fs::metadata(self.0?.source_path.as_deref()?).ok()?;
            let julian = |t: std::time::SystemTime| -> Option<f64> {
                let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
                Some(secs / 86_400.0 + 2_440_587.5 + utc_offset_days())
            };
            return Some((julian(meta.created().ok()?)?, julian(meta.modified().ok()?)?));
        }
        #[cfg(target_arch = "wasm32")]
        None
    }

    fn login(&self) -> Option<String> {
        // The account's display name, as the reference shows it.
        if let Some(name) = display_name() {
            return Some(name);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            for var in ["USER", "LOGNAME", "USERNAME"] {
                if let Ok(v) = std::env::var(var) {
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
        }
        None
    }

    fn getvar(&self, name: &str) -> Option<String> {
        self.0.and_then(|document| sysvar(document, name))
    }

    fn file_size(&self) -> Option<u64> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.0?.source_path.as_deref()?;
            return std::fs::metadata(path).ok().map(|m| m.len());
        }
        #[cfg(target_arch = "wasm32")]
        None
    }

    fn date_locale(&self) -> codec::fields::DateLocale {
        os_date_locale()
    }

    fn sheet_sets(
        &self,
        f: &mut dyn FnMut(&codec::sheet_set::SheetSetDatabase) -> Option<String>,
    ) -> Option<String> {
        let sets = SHEET_SETS.read().ok()?;
        sets.iter().find_map(|db| f(db))
    }

    fn getenv(&self, name: &str) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            return std::env::var(name).ok().filter(|v| !v.is_empty());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            None
        }
    }
}

/// The sheet sets open in the Sheet Set Manager, for `\AcSm` fields. The
/// manager replaces the list whenever a set opens, closes or changes.
static SHEET_SETS: std::sync::RwLock<Vec<codec::sheet_set::SheetSetDatabase>> = std::sync::RwLock::new(Vec::new());

pub fn set_sheet_sets(sets: Vec<codec::sheet_set::SheetSetDatabase>) {
    if let Ok(mut guard) = SHEET_SETS.write() {
        *guard = sets;
    }
}

/// Re-evaluate the field hosted by entity `host` (usually an MTEXT), or `None`
/// to keep the cached text. Thin wrapper over the library engine.
pub fn resolve(document: &CadDocument, host: Handle) -> Option<String> {
    codec::fields::resolve(document, host, &OcsFieldContext(Some(document)))
}

/// FIELDEVAL of the drawing (kept in its variable dictionary; default 31):
/// the events that update fields — 1 open, 2 save, 4 plot, 8 eTransmit,
/// 16 regen.
pub fn fieldeval(document: &CadDocument) -> i32 {
    crate::io::drawing_variable(document, "FIELDEVAL")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(31)
}

/// Update the fields an event evaluates and store their values in the host
/// texts: `event` 1 open, 2 save or 16 regen (masked with FIELDEVAL), or 32
/// an explicit UPDATEFIELD. `hosts` limits it to those hosts. Returns the
/// hosts whose text changed and the number of fields the hosts hold.
pub fn update_fields(
    document: &mut CadDocument,
    event: i32,
    hosts: Option<&[Handle]>,
) -> (Vec<Handle>, usize) {
    let event = if event == 32 { event } else { event & fieldeval(document) };
    if event == 0 || document.fields.is_empty() {
        return (Vec::new(), 0);
    }
    let snapshot = document.clone();
    document.update_fields(&OcsFieldContext(Some(&snapshot)), event, hosts)
}

/// Recompute a table's cell formulas into its block after the table changed;
/// returns the cell texts that changed.
pub fn refresh_table_formulas(document: &mut CadDocument, table: Handle) -> Vec<Handle> {
    // Copying the drawing is only worth it for a table with a field.
    let has_field = matches!(
        document.get_entity(table),
        Some(codec::EntityType::Table(t)) if t
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .flat_map(|cell| &cell.contents)
            .any(|content| content.field_handle.is_some())
    );
    if !has_field {
        return Vec::new();
    }
    let snapshot = document.clone();
    document.refresh_table_formulas(&OcsFieldContext(Some(&snapshot)), table)
}

/// The text a table cell shows for its field: a formula is evaluated live
/// (it reads other cells), any other field shows its stored value.
pub fn cell_text(document: &CadDocument, field: Handle, table: Handle) -> Option<String> {
    codec::fields::cell_field_text(document, field, table, &OcsFieldContext(Some(document)))
}

/// Evaluate one field code (`\AcVar Date \f "yyyy-MM-dd"`, or the `%<…>%`
/// form) for a preview; `None` when it cannot be evaluated.
pub fn evaluate(
    document: &CadDocument,
    code: &str,
    objects: &[Handle],
    host: Option<Handle>,
) -> Option<String> {
    codec::fields::evaluate_code(document, code, objects, host, &OcsFieldContext(Some(document)))
}

/// The system variables the Field dialog lists, in its order (lower case).
pub const SYSVARS: &[&str] = &[
    "aunits", "auprec", "celtscale", "chamfera", "chamferb", "chamferc", "chamferd",
    "clayer", "dimscale", "dimtxt", "dwgname", "dwgprefix", "elevation", "filletrad",
    "insbase", "insunits", "limmax", "limmin", "ltscale", "lunits", "luprec",
    "measurement", "mirrtext", "pdmode", "pdsize", "textsize", "textstyle", "thickness",
    "useri1", "useri2", "useri3", "useri4", "useri5", "userr1", "userr2", "userr3",
    "userr4", "userr5",
];

/// Real system variables show six decimals, as the reference shows them
/// (TEXTSIZE 2.500000).
fn num(v: f64) -> String {
    format!("{v:.6}")
}

/// A system variable's value as a field shows it.
pub fn sysvar(document: &CadDocument, name: &str) -> Option<String> {
    let h = &document.header;
    let path = document.source_path.as_deref().unwrap_or("");
    let (folder, file) = match path.rfind(['/', '\\']) {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    };
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "aunits" => h.angular_unit_format.to_string(),
        "auprec" => h.angular_unit_precision.to_string(),
        "celtscale" => num(h.current_entity_linetype_scale),
        "chamfera" => num(h.chamfer_distance_a),
        "chamferb" => num(h.chamfer_distance_b),
        "chamferc" => num(h.chamfer_length),
        "chamferd" => num(h.chamfer_angle),
        "clayer" => h.current_layer_name.clone(),
        "dimscale" => num(h.dim_scale),
        "dimtxt" => num(h.dim_text_height),
        "dwgname" => if file.is_empty() { "Drawing1.dwg".to_string() } else { file.to_string() },
        "dwgprefix" => folder.to_string(),
        "elevation" => num(h.elevation),
        "filletrad" => num(h.fillet_radius),
        "insbase" => {
            let p = h.model_space_insertion_base;
            format!("{},{},{}", num(p.x), num(p.y), num(p.z))
        }
        "insunits" => h.insertion_units.to_string(),
        "limmax" => format!("{},{}", num(h.model_space_limits_max.x), num(h.model_space_limits_max.y)),
        "limmin" => format!("{},{}", num(h.model_space_limits_min.x), num(h.model_space_limits_min.y)),
        "ltscale" => num(h.linetype_scale),
        "lunits" => h.linear_unit_format.to_string(),
        "luprec" => h.linear_unit_precision.to_string(),
        "measurement" => h.measurement.to_string(),
        "mirrtext" => i16::from(h.mirror_text).to_string(),
        "pdmode" => h.point_display_mode.to_string(),
        "pdsize" => num(h.point_display_size),
        "textsize" => num(h.text_height),
        "textstyle" => h.current_text_style_name.clone(),
        "thickness" => num(h.thickness),
        "useri1" => h.user_int1.to_string(),
        "useri2" => h.user_int2.to_string(),
        "useri3" => h.user_int3.to_string(),
        "useri4" => h.user_int4.to_string(),
        "useri5" => h.user_int5.to_string(),
        "userr1" => num(h.user_real1),
        "userr2" => num(h.user_real2),
        "userr3" => num(h.user_real3),
        "userr4" => num(h.user_real4),
        "userr5" => num(h.user_real5),
        _ => return None,
    })
}

/// Seconds since the Unix epoch. Native uses the system clock; wasm uses the JS
/// `Date` clock. This platform-specific read stays in the app, not the library.
fn epoch_secs() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as i64
    }
}

/// Attach `field` (its code and referenced objects) to `host`, showing
/// `value`; `None` removes the host's field.
pub fn set_text_field(
    document: &mut CadDocument,
    host: Handle,
    field: Option<(String, Vec<Handle>)>,
    value: &str,
) {
    match field {
        Some((code, objects)) => {
            let mut child = codec::fields::NewField::new(code, value);
            child.objects = objects;
            document.set_text_field(host, "%<\\_FldIdx 0>%", vec![child]);
        }
        None => {
            document.remove_text_field(host);
        }
    }
}

/// Month and day names and the regional date / time pictures, as date
/// fields show them: the operating system's regional settings.
fn os_date_locale() -> codec::fields::DateLocale {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Globalization::GetLocaleInfoEx;
        let info = |kind: u32| -> Option<String> {
            let mut buffer = [0u16; 128];
            // A null locale name is the user's default locale.
            let len = unsafe { GetLocaleInfoEx(std::ptr::null(), kind, buffer.as_mut_ptr(), buffer.len() as i32) };
            (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
        };
        let mut locale = codec::fields::DateLocale::default();
        for k in 0..12u32 {
            if let Some(v) = info(0x38 + k) {
                locale.months[k as usize] = v; // LOCALE_SMONTHNAME1..
            }
            if let Some(v) = info(0x44 + k) {
                locale.months_abbr[k as usize] = v; // LOCALE_SABBREVMONTHNAME1..
            }
        }
        // LOCALE_SDAYNAME1 is Monday; the codec lists Sunday first.
        for k in 0..7u32 {
            let slot = ((k + 1) % 7) as usize;
            if let Some(v) = info(0x2A + k) {
                locale.days[slot] = v;
            }
            if let Some(v) = info(0x31 + k) {
                locale.days_abbr[slot] = v;
            }
        }
        if let Some(v) = info(0x28) {
            locale.am = v; // LOCALE_S1159
        }
        if let Some(v) = info(0x29) {
            locale.pm = v; // LOCALE_S2359
        }
        if let Some(v) = info(0x1F) {
            locale.short_date = v; // LOCALE_SSHORTDATE
        }
        if let Some(v) = info(0x20) {
            locale.long_date = v; // LOCALE_SLONGDATE
        }
        if let Some(v) = info(0x1003) {
            locale.long_time = v; // LOCALE_STIMEFORMAT
        }
        locale
    }
    #[cfg(not(target_os = "windows"))]
    codec::fields::DateLocale::default()
}

/// FIELDDISPLAY: whether fields show on a gray background (not plotted).
/// A profile setting, so one value for every drawing.
static FIELD_DISPLAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn display() -> bool {
    FIELD_DISPLAY.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_display(on: bool) {
    FIELD_DISPLAY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// FIELDDISPLAY box extent around a line of field text, in text heights:
/// from a third of the height below the baseline to 1.19 heights above it
/// (the reference's box, measured at 2.5 and 10 unit text).
pub const BOX_BELOW: f64 = 0.345;
pub const BOX_ABOVE: f64 = 1.19;

/// The gray the reference draws behind a field.
pub const BACKGROUND: [f32; 4] = [203.0 / 255.0, 203.0 / 255.0, 203.0 / 255.0, 1.0];

/// Whether `entity` hosts a field: its extension dictionary has ACAD_FIELD.
pub fn hosts_field(document: &CadDocument, entity: &codec::entities::EntityType) -> bool {
    common_hosts_field(document, entity.common())
}

/// [`hosts_field`] for an entity known only by its common data, such as an
/// attribute of a block reference.
pub fn common_hosts_field(document: &CadDocument, common: &codec::entities::EntityCommon) -> bool {
    let Some(xdict) = common.xdictionary_handle else {
        return false;
    };
    matches!(
        document.objects.get(&xdict),
        Some(codec::objects::ObjectType::Dictionary(d)) if d.get("ACAD_FIELD").is_some()
    )
}

fn now_utc_julian() -> f64 {
    epoch_secs() as f64 / 86_400.0 + 2_440_587.5
}

/// Local time minus UTC, in days (the zone's current offset).
pub fn utc_offset_days() -> f64 {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
        // 100 ns ticks since 1601-01-01.
        let ticks = (epoch_secs() as u64 + 11_644_473_600) * 10_000_000;
        let utc = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
        let mut local = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } != 0 {
            let local = ((local.dwHighDateTime as u64) << 32) | local.dwLowDateTime as u64;
            return (local as i64 - ticks as i64) as f64 / 864_000_000_000.0;
        }
        0.0
    }
    #[cfg(not(target_os = "windows"))]
    0.0
}

/// A DWG stores its header dates in universal time only; the local TDCREATE
/// and TDUPDATE come from the zone, as the reference derives them on load.
pub fn local_header_dates(document: &mut CadDocument) {
    let h = &mut document.header;
    let offset = utc_offset_days();
    if h.universal_create_date_julian != 0.0 && h.create_date_julian == h.universal_create_date_julian {
        h.create_date_julian = h.universal_create_date_julian + offset;
    }
    if h.universal_update_date_julian != 0.0 && h.update_date_julian == h.universal_update_date_julian {
        h.update_date_julian = h.universal_update_date_julian + offset;
    }
}

/// Stamp the header's update date (and the creation date of a new drawing)
/// in local and universal time before a save.
pub fn stamp_save_dates(document: &mut CadDocument) {
    let utc = now_utc_julian();
    let local = utc + utc_offset_days();
    let h = &mut document.header;
    if h.create_date_julian == 0.0 && h.universal_create_date_julian == 0.0 {
        h.create_date_julian = local;
        h.universal_create_date_julian = utc;
    }
    h.update_date_julian = local;
    h.universal_update_date_julian = utc;
}

/// The signed-in account's display name, when it has one.
fn display_name() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
        let mut buffer = [0u16; 256];
        let mut len = buffer.len() as u32;
        if unsafe { GetUserNameExW(NameDisplay, buffer.as_mut_ptr(), &mut len) } && len > 0 {
            return Some(String::from_utf16_lossy(&buffer[..len as usize])).filter(|n| !n.trim().is_empty());
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    None
}

/// Copy each attribute definition's field onto the matching attribute of a
/// new block reference (block placeholders resolve against the reference).
/// Returns the attributes that received a field.
pub fn attach_attribute_fields(document: &mut CadDocument, insert: Handle) -> Vec<Handle> {
    let context = OcsFieldContext(None);
    document.attach_attribute_fields(insert, &context)
}

/// Store fresh values for the sheet set fields (after a sheet set changed)
/// and return the hosts whose text changed. A field no open sheet set
/// resolves (`####`: its set was closed or not found) keeps its stored value.
pub fn refresh_sheet_set_fields(document: &mut CadDocument) -> Vec<Handle> {
    let context = OcsFieldContext(None);
    let unresolved: Vec<(Handle, String)> = document
        .fields
        .values()
        .filter(|f| f.evaluator.starts_with("AcSm"))
        .filter(|f| {
            let host = field_host(document, f.handle).unwrap_or(Handle::NULL);
            codec::fields::resolve_handle(document, f.handle, host, &context).is_none_or(|v| v == "####")
        })
        .map(|f| (f.handle, f.evaluator.clone()))
        .collect();
    // ponytail: the engine refreshes every sheet set field, so the unresolved
    // ones are hidden from it (evaluator cleared) for this one call; a
    // "skip unresolved" switch in the engine would replace this.
    for (h, _) in &unresolved {
        if let Some(f) = document.fields.get_mut(h) {
            f.evaluator.clear();
        }
    }
    let changed = document.refresh_sheet_set_fields(&context);
    for (h, evaluator) in unresolved {
        if let Some(f) = document.fields.get_mut(&h) {
            f.evaluator = evaluator;
        }
    }
    changed
}

/// The entity hosting field `field`: up its owner chain (fields, then the
/// field dictionary and extension dictionary) to the first non-object.
fn field_host(document: &CadDocument, field: Handle) -> Option<Handle> {
    let owner = |h: Handle| -> Option<Handle> {
        if let Some(f) = document.fields.get(&h) {
            return Some(f.owner);
        }
        match document.objects.get(&h)? {
            codec::objects::ObjectType::Dictionary(d) => Some(d.owner),
            codec::objects::ObjectType::Unknown { owner, .. } => Some(*owner),
            _ => None,
        }
    };
    let mut h = field;
    for _ in 0..12 {
        let o = owner(h)?;
        if owner(o).is_none() {
            return Some(o);
        }
        h = o;
    }
    None
}

/// Now as the OS long date, two spaces and the 24-hour time
/// (`Monday, October 5, 2026  09:26:39`).
pub fn long_date_time_now() -> String {
    let locale = os_date_locale();
    let picture = format!("{}  HH:mm:ss", locale.long_date);
    let now = now_utc_julian() + utc_offset_days();
    codec::fields::format_dt_in(codec::fields::julian_parts(now), &picture, &locale)
}

/// [`attach_attribute_fields`] with every field code passed through `map`.
pub fn attach_attribute_fields_mapped(document: &mut CadDocument, insert: Handle, map: &dyn Fn(&str) -> String) -> Vec<Handle> {
    let context = OcsFieldContext(None);
    document.attach_attribute_fields_mapped(insert, &context, map)
}

/// Whether any attribute definition of `block` hosts a field.
pub fn block_has_attribute_fields(document: &CadDocument, block: &str) -> bool {
    let Some(record) = document.block_records.iter().find(|r| r.name.eq_ignore_ascii_case(block)) else {
        return false;
    };
    record.entity_handles.iter().filter_map(|h| document.get_entity(*h)).any(|e| {
        matches!(e, codec::entities::EntityType::AttributeDefinition(_)) && hosts_field(document, e)
    })
}

/// Give plot-time fields (PlotDate) the plot's time; returns the hosts whose
/// text changed.
pub fn stamp_plot_fields(document: &mut CadDocument) -> Vec<Handle> {
    // The context reads a snapshot of the whole drawing; most drawings hold
    // no field, and a large one should not be copied for every plot.
    let has_fields = document
        .objects
        .values()
        .any(|object| matches!(object, codec::objects::ObjectType::Field(_)));
    if !has_fields || fieldeval(document) & 4 == 0 {
        return Vec::new();
    }
    let snapshot = document.clone();
    let context = PlotContext(OcsFieldContext(Some(&snapshot)));
    document.stamp_plot_fields(&context)
}

/// The app context while a plot is produced.
struct PlotContext<'a>(OcsFieldContext<'a>);

impl FieldContext for PlotContext<'_> {
    fn now_julian(&self) -> f64 {
        self.0.now_julian()
    }
    fn file_times(&self) -> Option<(f64, f64)> {
        self.0.file_times()
    }
    fn plotting(&self) -> bool {
        true
    }
    fn login(&self) -> Option<String> {
        self.0.login()
    }
    fn getvar(&self, name: &str) -> Option<String> {
        self.0.getvar(name)
    }
    fn file_size(&self) -> Option<u64> {
        self.0.file_size()
    }
    fn date_locale(&self) -> codec::fields::DateLocale {
        self.0.date_locale()
    }
    fn getenv(&self, name: &str) -> Option<String> {
        self.0.getenv(name)
    }
}
