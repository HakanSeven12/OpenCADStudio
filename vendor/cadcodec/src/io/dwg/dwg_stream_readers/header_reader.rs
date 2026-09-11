//! DWG Header section reader
//!
//! Reads the AcDb:Header section from a DWG file into `HeaderVariables`.
//! This is the inverse of `header_writer.rs`, reading ~200 fields with
//! extensive version-conditional logic.
//!
//! Based on the reference `DwgHeaderReader`.

use crate::document::HeaderVariables;
use crate::error::{DxfError, Result};
use crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::io::dwg::file_headers::section_definition::start_sentinels;
use crate::types::{DxfVersion, Handle};

// ════════════════════════════════════════════════════════════════════════════
//  Version-range helpers (same as header_writer)
// ════════════════════════════════════════════════════════════════════════════

#[inline]
fn r13_14_only(v: DxfVersion) -> bool {
    v >= DxfVersion::AC1012 && v <= DxfVersion::AC1014
}
#[inline]
fn r13_15_only(v: DxfVersion) -> bool {
    v >= DxfVersion::AC1012 && v <= DxfVersion::AC1015
}
#[inline]
fn r2000_plus(v: DxfVersion) -> bool {
    v >= DxfVersion::AC1015
}
#[inline]
fn r2004_plus(v: DxfVersion) -> bool {
    v >= DxfVersion::AC1018
}
#[inline]
fn r2007_plus(v: DxfVersion) -> bool {
    v >= DxfVersion::AC1021
}
#[inline]
fn r2010_plus(v: DxfVersion) -> bool {
    v >= DxfVersion::AC1024
}
#[inline]
fn r2013_plus(v: DxfVersion) -> bool {
    v >= DxfVersion::AC1027
}

// ════════════════════════════════════════════════════════════════════════════
//  Julian date helpers
// ════════════════════════════════════════════════════════════════════════════

fn day_ms_to_julian(day: i32, ms: i32) -> f64 {
    day as f64 + (ms as f64 / 86_400_000.0)
}

fn day_ms_to_timespan(days: i32, ms: i32) -> f64 {
    days as f64 + (ms as f64 / 86_400_000.0)
}

// ════════════════════════════════════════════════════════════════════════════
//  Reader abstraction (pre-R2007 = inline, R2007+ = three-stream merge)
// ════════════════════════════════════════════════════════════════════════════

/// Abstraction over single-stream (pre-R2007) and merged (R2007+) reading.
///
/// For R2007+ the header section uses the three-stream merge format
/// (main + text + handle), matching the writer's `DwgMergedWriter`.
/// Text reads are routed to the text sub-stream and handle reads to the
/// handle sub-stream, preserving bit alignment of the main data.
enum SectionReaderInner {
    /// Pre-R2007: single stream, everything inline
    BitReader(DwgBitReader),
    /// R2007+: three-stream merge (main + text + handle)
    MergedReader(DwgMergedReader),
}

struct SectionReader {
    inner: SectionReaderInner,
}

impl SectionReader {
    fn with_encoding(
        data: Vec<u8>,
        version: DxfVersion,
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<Self> {
        if version >= DxfVersion::AC1021 {
            // R2007+: three-stream merge.
            // The section data starts with an RL (total size in bits).
            // Create a DwgMergedReader in ThreeStream mode and set up
            // text/handle sub-streams from the RL.
            let mut merged = DwgMergedReader::new_with_encoding(data, version, 0, encoding);
            // Read the RL (total_size_bits) stored by save_position_for_size
            let total_size_bits = merged.main_mut().read_raw_long() as i64;
            merged.setup_text_and_handle(total_size_bits);
            Ok(SectionReader {
                inner: SectionReaderInner::MergedReader(merged),
            })
        } else {
            // Pre-R2007: single stream, everything inline
            let dwg = DwgVersion::from_dxf_version(version)?;
            let reader = DwgBitReader::with_encoding(data, dwg, version, encoding);
            Ok(SectionReader {
                inner: SectionReaderInner::BitReader(reader),
            })
        }
    }

    // Delegate all read methods to the underlying reader
    fn read_bit(&mut self) -> bool {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_bit(),
            SectionReaderInner::MergedReader(r) => r.read_bit(),
        }
    }
    fn read_byte(&mut self) -> u8 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_byte(),
            SectionReaderInner::MergedReader(r) => r.read_byte(),
        }
    }
    fn read_bit_short(&mut self) -> i16 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_bit_short(),
            SectionReaderInner::MergedReader(r) => r.read_bit_short(),
        }
    }
    fn read_bit_long(&mut self) -> i32 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_bit_long(),
            SectionReaderInner::MergedReader(r) => r.read_bit_long(),
        }
    }
    fn read_bit_long_long(&mut self) -> i64 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_bit_long_long(),
            SectionReaderInner::MergedReader(r) => r.read_bit_long_long(),
        }
    }
    fn read_bit_double(&mut self) -> f64 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_bit_double(),
            SectionReaderInner::MergedReader(r) => r.read_bit_double(),
        }
    }
    fn read_3bit_double(&mut self) -> crate::types::Vector3 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_3bit_double(),
            SectionReaderInner::MergedReader(r) => r.read_3bit_double(),
        }
    }
    fn read_2raw_double(&mut self) -> crate::types::Vector2 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_2raw_double(),
            SectionReaderInner::MergedReader(r) => r.read_2raw_double(),
        }
    }
    fn read_cm_color(&mut self) -> crate::types::Color {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_cm_color(),
            SectionReaderInner::MergedReader(r) => r.read_cm_color(),
        }
    }
    fn read_variable_text(&mut self) -> String {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_variable_text(),
            SectionReaderInner::MergedReader(r) => r.read_variable_text(),
        }
    }
    fn read_handle(&mut self) -> u64 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_handle(),
            SectionReaderInner::MergedReader(r) => r.read_handle(),
        }
    }

    /// Read a handle from the MAIN stream (not the handle sub-stream).
    /// Used for HANDSEED which is always written inline.
    fn read_handle_inline(&mut self) -> u64 {
        match &mut self.inner {
            SectionReaderInner::BitReader(r) => r.read_handle(),
            SectionReaderInner::MergedReader(r) => r.main_mut().read_handle(),
        }
    }

    fn read_datetime(&mut self) -> (i32, i32) {
        let day = self.read_bit_long();
        let ms = self.read_bit_long();
        (day, ms)
    }

    fn read_timespan(&mut self) -> (i32, i32) {
        let days = self.read_bit_long();
        let ms = self.read_bit_long();
        (days, ms)
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Public API
// ════════════════════════════════════════════════════════════════════════════

/// Read the complete Header section from raw bytes (including sentinels).
///
/// # Returns
/// `HeaderVariables` populated with all header variables.
pub fn read_header(
    data: &[u8],
    version: DxfVersion,
    maintenance_version: u8,
) -> Result<HeaderVariables> {
    read_header_with_encoding(
        data,
        version,
        maintenance_version,
        encoding_rs::WINDOWS_1252,
    )
}

pub fn read_header_with_encoding(
    data: &[u8],
    version: DxfVersion,
    maintenance_version: u8,
    encoding: &'static encoding_rs::Encoding,
) -> Result<HeaderVariables> {
    // ── Verify start sentinel ──
    if data.len() < 36 {
        return Err(DxfError::Parse("Header section too short".to_string()));
    }
    if &data[..16] != &start_sentinels::HEADER {
        return Err(DxfError::InvalidSentinel(
            "Header section start sentinel mismatch".to_string(),
        ));
    }

    // ── Read section size ──
    let mut size_offset = 16;
    let section_size = i32::from_le_bytes([
        data[size_offset],
        data[size_offset + 1],
        data[size_offset + 2],
        data[size_offset + 3],
    ]) as usize;
    size_offset += 4;

    // Extra 4 zero bytes when: (AC1024+ && maintenance > 3) || AC1032+
    if DwgVersion::has_section_extra_rl(version, maintenance_version) {
        size_offset += 4;
    }

    let section_data = &data[size_offset..size_offset + section_size];

    let mut r = SectionReader::with_encoding(section_data.to_vec(), version, encoding)?;
    let mut h = HeaderVariables::default();

    read_header_fields(&mut r, version, &mut h);

    Ok(h)
}

// ════════════════════════════════════════════════════════════════════════════
//  Header field reader — the big one (~200 fields, inverse of writer)
// ════════════════════════════════════════════════════════════════════════════

fn read_header_fields(r: &mut SectionReader, v: DxfVersion, h: &mut HeaderVariables) {
    // R2013+: BLL REQUIREDVERSIONS
    if r2013_plus(v) {
        h.required_versions = r.read_bit_long_long();
    }

    // ── Unknown defaults (Common) ──
    let _ = r.read_bit_double(); // 412148564080.0
    let _ = r.read_bit_double(); // 1.0
    let _ = r.read_bit_double(); // 1.0
    let _ = r.read_bit_double(); // 1.0

    let _ = r.read_variable_text(); // "m"
    let _ = r.read_variable_text(); // ""
    let _ = r.read_variable_text(); // ""
    let _ = r.read_variable_text(); // ""

    let _ = r.read_bit_long(); // 24
    let _ = r.read_bit_long(); // 0

    // R13-R14 Only: BS unknown
    if r13_14_only(v) {
        let _ = r.read_bit_short();
    }

    // Pre-2004: current viewport header handle
    if v < DxfVersion::AC1018 {
        h.current_vx_handle = Handle::new(r.read_handle());
    }

    // ── Drawing mode flags (Common) ──
    h.associate_dimensions = r.read_bit();
    h.update_dimensions_while_dragging = r.read_bit();

    if r13_14_only(v) {
        let _ = r.read_bit(); // DIMSAV undocumented
    }

    h.polyline_linetype_generation = r.read_bit();
    h.ortho_mode = r.read_bit();
    h.regen_mode = r.read_bit();
    h.fill_mode = r.read_bit();
    h.quick_text_mode = r.read_bit();
    h.paper_space_linetype_scaling = r.read_bit();
    h.limit_check = r.read_bit();

    if r13_14_only(v) {
        h.blip_mode = r.read_bit();
    }

    if r2004_plus(v) {
        let _ = r.read_bit(); // undocumented
    }

    h.user_timer = r.read_bit();
    h.sketch_type = if r.read_bit() { 1 } else { 0 }; // SKPOLY
    h.angle_direction = if r.read_bit() { 1 } else { 0 }; // ANGDIR
    h.spline_frame = r.read_bit(); // SPLFRAME

    if r13_14_only(v) {
        h.attribute_request = r.read_bit();
        h.attribute_dialog = r.read_bit();
    }

    h.mirror_text = r.read_bit();
    h.world_view = r.read_bit();

    if r13_14_only(v) {
        let _ = r.read_bit(); // WIREFRAME
    }

    h.show_model_space = r.read_bit();
    h.paper_space_limit_check = r.read_bit();
    h.retain_xref_visibility = r.read_bit();

    if r13_14_only(v) {
        h.delete_objects = r.read_bit();
    }

    h.display_silhouette = r.read_bit();
    let _ = r.read_bit(); // PELLIPSE
    h.proxy_graphics = r.read_bit_short();

    if r13_14_only(v) {
        h.drag_mode = r.read_bit_short();
    }

    // ── Unit settings (Common) ──
    h.tree_depth = r.read_bit_short();
    h.linear_unit_format = r.read_bit_short();
    h.linear_unit_precision = r.read_bit_short();
    h.angular_unit_format = r.read_bit_short();
    h.angular_unit_precision = r.read_bit_short();

    if r13_14_only(v) {
        h.object_snap_mode = r.read_bit_short() as i32;
    }

    h.attribute_visibility = r.read_bit_short();

    if r13_14_only(v) {
        h.coords_mode = r.read_bit_short();
    }

    h.point_display_mode = r.read_bit_short();

    if r13_14_only(v) {
        h.pick_style = r.read_bit_short();
    }

    if r2004_plus(v) {
        let _ = r.read_bit_long(); // unknown
        let _ = r.read_bit_long(); // unknown
        let _ = r.read_bit_long(); // unknown
    }

    h.user_int1 = r.read_bit_short();
    h.user_int2 = r.read_bit_short();
    h.user_int3 = r.read_bit_short();
    h.user_int4 = r.read_bit_short();
    h.user_int5 = r.read_bit_short();

    h.spline_segments = r.read_bit_short();
    h.surface_u_density = r.read_bit_short();
    h.surface_v_density = r.read_bit_short();
    h.surface_type = r.read_bit_short();
    h.surface_tab1 = r.read_bit_short();
    h.surface_tab2 = r.read_bit_short();
    h.spline_type = r.read_bit_short();
    h.shade_edge = r.read_bit_short();
    h.shade_diffuse = r.read_bit_short();
    let _ = r.read_bit_short(); // UNITMODE
    h.max_active_viewports = r.read_bit_short();
    h.isolines = r.read_bit_short();
    h.multiline_justification = r.read_bit_short();
    h.text_quality = r.read_bit_short();

    // ── Scale/size defaults (Common) ──
    h.linetype_scale = r.read_bit_double();
    h.text_height = r.read_bit_double();
    h.trace_width = r.read_bit_double();
    h.sketch_increment = r.read_bit_double();
    h.fillet_radius = r.read_bit_double();
    h.thickness = r.read_bit_double();
    h.angle_base = r.read_bit_double();
    h.point_display_size = r.read_bit_double();
    h.polyline_width = r.read_bit_double();
    h.user_real1 = r.read_bit_double();
    h.user_real2 = r.read_bit_double();
    h.user_real3 = r.read_bit_double();
    h.user_real4 = r.read_bit_double();
    h.user_real5 = r.read_bit_double();
    h.chamfer_distance_a = r.read_bit_double();
    h.chamfer_distance_b = r.read_bit_double();
    h.chamfer_length = r.read_bit_double();
    h.chamfer_angle = r.read_bit_double();
    h.facet_resolution = r.read_bit_double();
    h.multiline_scale = r.read_bit_double();
    h.current_entity_linetype_scale = r.read_bit_double();

    h.menu_name = r.read_variable_text();

    // ── Date/time (Common) ──
    let (cd, cms) = r.read_datetime();
    h.create_date_julian = day_ms_to_julian(cd, cms);
    let (ud, ums) = r.read_datetime();
    h.update_date_julian = day_ms_to_julian(ud, ums);

    if r2004_plus(v) {
        let _ = r.read_bit_long(); // unknown
        let _ = r.read_bit_long(); // unknown
        let _ = r.read_bit_long(); // unknown
    }

    let (ted, tems) = r.read_timespan();
    h.total_editing_time = day_ms_to_timespan(ted, tems);
    let (ued, uems) = r.read_timespan();
    h.user_elapsed_time = day_ms_to_timespan(ued, uems);

    // ── Current entity color ──
    h.current_entity_color = r.read_cm_color();

    // ── HANDSEED ──
    // HANDSEED is written to the main stream (not the handle sub-stream),
    // so we must read it inline from the main stream.
    h.handle_seed = r.read_handle_inline();

    // ── Style/layer/linetype handles ──
    h.current_layer_handle = Handle::new(r.read_handle());
    h.current_text_style_handle = Handle::new(r.read_handle());
    h.current_linetype_handle = Handle::new(r.read_handle());

    if r2007_plus(v) {
        h.current_material_handle = Handle::new(r.read_handle());
    }

    h.current_dimstyle_handle = Handle::new(r.read_handle());
    h.current_multiline_style_handle = Handle::new(r.read_handle());

    if r2000_plus(v) {
        h.viewport_scale_factor = r.read_bit_double();
    }

    // ── Paper space extents/limits/UCS ──
    h.paper_space_insertion_base = r.read_3bit_double();
    h.paper_space_extents_min = r.read_3bit_double();
    h.paper_space_extents_max = r.read_3bit_double();
    h.paper_space_limits_min = r.read_2raw_double();
    h.paper_space_limits_max = r.read_2raw_double();
    h.paper_elevation = r.read_bit_double();
    h.paper_space_ucs_origin = r.read_3bit_double();
    h.paper_space_ucs_x_axis = r.read_3bit_double();
    h.paper_space_ucs_y_axis = r.read_3bit_double();

    // UCSNAME (PSPACE)
    let _ = r.read_handle();

    if r2000_plus(v) {
        h.paper_ucs_ortho_ref = Handle::new(r.read_handle());
        h.paper_ucs_ortho_view = r.read_bit_short();
        let _ = r.read_handle(); // PUCSBASE

        // Paper space orthographic origins (6 × 3BD)
        let _ = r.read_3bit_double(); // PUCSORGTOP
        let _ = r.read_3bit_double(); // PUCSORGBOTTOM
        let _ = r.read_3bit_double(); // PUCSORGLEFT
        let _ = r.read_3bit_double(); // PUCSORGRIGHT
        let _ = r.read_3bit_double(); // PUCSORGFRONT
        let _ = r.read_3bit_double(); // PUCSORGBACK
    }

    // ── Model space extents/limits/UCS ──
    h.model_space_insertion_base = r.read_3bit_double();
    h.model_space_extents_min = r.read_3bit_double();
    h.model_space_extents_max = r.read_3bit_double();
    h.model_space_limits_min = r.read_2raw_double();
    h.model_space_limits_max = r.read_2raw_double();
    h.elevation = r.read_bit_double();
    h.model_space_ucs_origin = r.read_3bit_double();
    h.model_space_ucs_x_axis = r.read_3bit_double();
    h.model_space_ucs_y_axis = r.read_3bit_double();

    // UCSNAME (MSPACE)
    let _ = r.read_handle();

    if r2000_plus(v) {
        h.ucs_ortho_ref = Handle::new(r.read_handle());
        h.ucs_ortho_view = r.read_bit_short();
        let _ = r.read_handle(); // UCSBASE

        // Model space orthographic origins (6 × 3BD)
        let _ = r.read_3bit_double(); // UCSORGTOP
        let _ = r.read_3bit_double(); // UCSORGBOTTOM
        let _ = r.read_3bit_double(); // UCSORGLEFT
        let _ = r.read_3bit_double(); // UCSORGRIGHT
        let _ = r.read_3bit_double(); // UCSORGFRONT
        let _ = r.read_3bit_double(); // UCSORGBACK

        // DIMPOST, DIMAPOST
        h.dim_post = r.read_variable_text();
        h.dim_alt_post = r.read_variable_text();
    }

    // ── Dimension variables (R13-R14 Only block) ──
    if r13_14_only(v) {
        h.dim_tolerance = r.read_bit();
        h.dim_limits = r.read_bit();
        h.dim_text_inside_horizontal = r.read_bit();
        h.dim_text_outside_horizontal = r.read_bit();
        h.dim_suppress_ext1 = r.read_bit();
        h.dim_suppress_ext2 = r.read_bit();
        h.dim_alternate_units = r.read_bit();
        h.dim_force_line_inside = r.read_bit();
        h.dim_separate_arrows = r.read_bit();
        h.dim_force_text_inside = r.read_bit();
        h.dim_suppress_outside_ext = r.read_bit();
        h.dim_alt_decimal_places = r.read_byte() as i16;
        h.dim_zero_suppression = r.read_byte() as i16;
        h.dim_suppress_line1 = r.read_bit();
        h.dim_suppress_line2 = r.read_bit();
        h.dim_tolerance_justification = r.read_byte() as i16;
        h.dim_horizontal_justification = r.read_byte() as i16;
        h.dim_fit = r.read_byte() as i16;
        h.dim_user_positioned_text = r.read_bit();
        h.dim_tolerance_zero_suppression = r.read_byte() as i16;
        h.dim_alt_tolerance_zero_suppression = r.read_byte() as i16;
        h.dim_alt_tolerance_zero_tight = r.read_byte() as i16;
        h.dim_text_above = r.read_byte() as i16;
        let _ = r.read_bit_short(); // DIMUNIT
        h.dim_angular_decimal_places = r.read_bit_short();
        h.dim_decimal_places = r.read_bit_short();
        h.dim_tolerance_decimal_places = r.read_bit_short();
        h.dim_alt_units_format = r.read_bit_short();
        h.dim_alt_tolerance_decimal_places = r.read_bit_short();

        // DIMTXSTY handle
        h.dim_text_style_handle = Handle::new(r.read_handle());
    }

    // ── Dimension variables (Common) ──
    h.dim_scale = r.read_bit_double();
    h.dim_arrow_size = r.read_bit_double();
    h.dim_ext_line_offset = r.read_bit_double();
    h.dim_line_increment = r.read_bit_double();
    h.dim_ext_line_extension = r.read_bit_double();
    h.dim_rounding = r.read_bit_double();
    h.dim_line_extension = r.read_bit_double();
    h.dim_tolerance_plus = r.read_bit_double();
    h.dim_tolerance_minus = r.read_bit_double();

    // R2007+ dimension extras
    if r2007_plus(v) {
        let _ = r.read_bit_double(); // DIMFXL
        let _ = r.read_bit_double(); // DIMJOGANG
        let _ = r.read_bit_short(); // DIMTFILL
        let _ = r.read_cm_color(); // DIMTFILLCLR
    }

    // R2000+ dimension flags
    if r2000_plus(v) {
        h.dim_tolerance = r.read_bit();
        h.dim_limits = r.read_bit();
        h.dim_text_inside_horizontal = r.read_bit();
        h.dim_text_outside_horizontal = r.read_bit();
        h.dim_suppress_ext1 = r.read_bit();
        h.dim_suppress_ext2 = r.read_bit();
        h.dim_text_above = r.read_bit_short();
        h.dim_zero_suppression = r.read_bit_short();
        h.dim_alt_zero_suppression = r.read_bit_short();
    }

    if r2007_plus(v) {
        let _ = r.read_bit_short(); // DIMARCSYM
    }

    // ── Dimension sizes (Common) ──
    h.dim_text_height = r.read_bit_double();
    h.dim_center_mark = r.read_bit_double();
    h.dim_tick_size = r.read_bit_double();
    h.dim_alt_scale = r.read_bit_double();
    h.dim_linear_scale = r.read_bit_double();
    h.dim_text_vertical_pos = r.read_bit_double();
    h.dim_tolerance_scale = r.read_bit_double();
    h.dim_line_gap = r.read_bit_double();

    // R13-R14 only: dimension text strings
    if r13_14_only(v) {
        h.dim_post = r.read_variable_text();
        h.dim_alt_post = r.read_variable_text();
        h.dim_arrow_block = r.read_variable_text();
        h.dim_arrow_block1 = r.read_variable_text();
        h.dim_arrow_block2 = r.read_variable_text();
    }

    // R2000+ only: additional dimension settings
    if r2000_plus(v) {
        h.dim_alt_rounding = r.read_bit_double();
        h.dim_alternate_units = r.read_bit();
        h.dim_alt_decimal_places = r.read_bit_short();
        h.dim_force_line_inside = r.read_bit();
        h.dim_separate_arrows = r.read_bit();
        h.dim_force_text_inside = r.read_bit();
        h.dim_suppress_outside_ext = r.read_bit();
    }

    // ── Dimension colors (Common) ──
    h.dim_line_color = r.read_cm_color();
    h.dim_ext_line_color = r.read_cm_color();
    h.dim_text_color = r.read_cm_color();

    // R2000+ only: dimension unit settings
    if r2000_plus(v) {
        h.dim_angular_decimal_places = r.read_bit_short();
        h.dim_decimal_places = r.read_bit_short();
        h.dim_tolerance_decimal_places = r.read_bit_short();
        h.dim_alt_units_format = r.read_bit_short();
        h.dim_alt_tolerance_decimal_places = r.read_bit_short();
        h.dim_angular_units = r.read_bit_short();
        h.dim_fraction_format = r.read_bit_short();
        h.dim_linear_unit_format = r.read_bit_short();
        h.dim_decimal_separator = r.read_bit_short() as u8 as char;
        h.dim_text_movement = r.read_bit_short();
        h.dim_horizontal_justification = r.read_bit_short();
        h.dim_suppress_line1 = r.read_bit();
        h.dim_suppress_line2 = r.read_bit();
        h.dim_tolerance_justification = r.read_bit_short();
        h.dim_tolerance_zero_suppression = r.read_bit_short();
        h.dim_alt_tolerance_zero_suppression = r.read_bit_short();
        h.dim_alt_tolerance_zero_tight = r.read_bit_short();
        h.dim_user_positioned_text = r.read_bit();
        h.dim_fit = r.read_bit_short();
    }

    // R2007+: DIMFXLON
    if r2007_plus(v) {
        let _ = r.read_bit(); // DimensionIsExtensionLineLengthFixed
    }

    // R2010+: extra dimension fields
    if r2010_plus(v) {
        let _ = r.read_bit(); // DIMTXTDIRECTION
        let _ = r.read_bit_double(); // DIMALTMZF
        let _ = r.read_variable_text(); // DIMALTMZS
        let _ = r.read_bit_double(); // DIMMZF
        let _ = r.read_variable_text(); // DIMMZS
    }

    // R2000+ dimension handles
    if r2000_plus(v) {
        h.dim_text_style_handle = Handle::new(r.read_handle());
        let _ = r.read_handle(); // DIMLDRBLK
        let _ = r.read_handle(); // DIMBLK
        let _ = r.read_handle(); // DIMBLK1
        let _ = r.read_handle(); // DIMBLK2
    }

    // R2007+ dimension linetype handles
    if r2007_plus(v) {
        h.dim_linetype_handle = Handle::new(r.read_handle());
        h.dim_linetype1_handle = Handle::new(r.read_handle());
        h.dim_linetype2_handle = Handle::new(r.read_handle());
    }

    // R2000+ dimension line weights
    if r2000_plus(v) {
        h.dim_line_weight = r.read_bit_short();
        h.dim_ext_line_weight = r.read_bit_short();
    }

    // ── Table control object handles (Common) ──
    h.block_control_handle = Handle::new(r.read_handle());
    h.layer_control_handle = Handle::new(r.read_handle());
    h.style_control_handle = Handle::new(r.read_handle());
    h.linetype_control_handle = Handle::new(r.read_handle());
    h.view_control_handle = Handle::new(r.read_handle());
    h.ucs_control_handle = Handle::new(r.read_handle());
    h.vport_control_handle = Handle::new(r.read_handle());
    h.appid_control_handle = Handle::new(r.read_handle());
    h.dimstyle_control_handle = Handle::new(r.read_handle());

    // R13-R15 only: VPEntHdr control
    if r13_15_only(v) {
        h.vpent_hdr_control_handle = Handle::new(r.read_handle());
    }

    // ── Dictionary handles (Common) ──
    h.acad_group_dict_handle = Handle::new(r.read_handle());
    h.acad_mlinestyle_dict_handle = Handle::new(r.read_handle());
    h.named_objects_dict_handle = Handle::new(r.read_handle());

    // R2000+ dictionaries and flags
    if r2000_plus(v) {
        let _ = r.read_bit_short(); // TSTACKALIGN
        let _ = r.read_bit_short(); // TSTACKSIZE

        h.hyperlink_base = r.read_variable_text();
        h.stylesheet = r.read_variable_text();

        h.acad_layout_dict_handle = Handle::new(r.read_handle());
        h.acad_plotsettings_dict_handle = Handle::new(r.read_handle());
        h.acad_plotstylename_dict_handle = Handle::new(r.read_handle());
    }

    // R2004+ dictionaries
    if r2004_plus(v) {
        h.acad_material_dict_handle = Handle::new(r.read_handle());
        h.acad_color_dict_handle = Handle::new(r.read_handle());
    }

    // R2007+ dictionaries
    if r2007_plus(v) {
        h.acad_visualstyle_dict_handle = Handle::new(r.read_handle());
        if r2013_plus(v) {
            let _ = r.read_handle(); // unknown
        }
    }

    // R2000+ flags bitfield
    if r2000_plus(v) {
        let flags = r.read_bit_long();
        h.current_line_weight = (flags & 0x1F) as i16;
        h.end_caps = ((flags >> 5) & 0x03) as i16;
        h.join_style = ((flags >> 7) & 0x03) as i16;
        h.lineweight_display = (flags & 0x200) == 0;
        h.xedit = (flags & 0x400) == 0;
        h.extended_names = (flags & 0x800) != 0;
        h.plotstyle_mode = (flags & 0x2000) != 0;
        h.ole_startup = (flags & 0x4000) != 0;

        h.insertion_units = r.read_bit_short();
        h.current_plotstyle_type = r.read_bit_short();

        if h.current_plotstyle_type == 3 {
            let _ = r.read_handle(); // CPSNID
        }

        h.fingerprint_guid = r.read_variable_text();
        h.version_guid = r.read_variable_text();
    }

    // R2004+ extra entity settings
    if r2004_plus(v) {
        h.sort_entities = r.read_byte() as i16;
        h.index_control = r.read_byte() as i16;
        h.hide_text = r.read_byte() as i16;
        h.xclip_frame = r.read_byte() as i16;
        h.dimension_associativity = r.read_byte() as i16;
        h.halo_gap = r.read_byte() as i16;
        h.obscured_color = r.read_bit_short();
        h.intersection_color = r.read_bit_short();
        h.obscured_linetype = r.read_byte() as i16;
        h.intersection_display = r.read_byte() as i16;

        h.project_name = r.read_variable_text();
    }

    // ── Block record / linetype handles (Common) ──
    h.paper_space_block_handle = Handle::new(r.read_handle());
    h.model_space_block_handle = Handle::new(r.read_handle());
    h.bylayer_linetype_handle = Handle::new(r.read_handle());
    h.byblock_linetype_handle = Handle::new(r.read_handle());
    h.continuous_linetype_handle = Handle::new(r.read_handle());

    // ── R2007+ extended fields ──
    if r2007_plus(v) {
        h.camera_display = r.read_bit();
        let _ = r.read_bit_long(); // unknown
        let _ = r.read_bit_long(); // unknown
        let _ = r.read_bit_double(); // unknown

        h.steps_per_second = r.read_bit_double();
        h.step_size = r.read_bit_double();
        let _ = r.read_bit_double(); // 3DDWFPREC
        h.lens_length = r.read_bit_double();
        h.camera_height = r.read_bit_double();
        h.record_solid_history = r.read_byte() != 0;
        h.show_solid_history = (r.read_byte() as i16).clamp(0, 2);
        let _ = r.read_bit_double(); // PSOLWIDTH
        let _ = r.read_bit_double(); // PSOLHEIGHT
        h.loft_angle1 = r.read_bit_double();
        h.loft_angle2 = r.read_bit_double();
        h.loft_magnitude1 = r.read_bit_double();
        h.loft_magnitude2 = r.read_bit_double();
        h.loft_param = r.read_bit_short();
        h.loft_normals = r.read_byte() as i16;
        h.latitude = r.read_bit_double();
        h.longitude = r.read_bit_double();
        h.north_direction = r.read_bit_double();
        h.timezone = r.read_bit_long();
        let _ = r.read_byte(); // LIGHTGLYPHDISPLAY
        let _ = r.read_byte(); // TILEMODELIGHTSYNCH
        let _ = r.read_byte(); // DWFFRAME
        let _ = r.read_byte(); // DGNFRAME

        let _ = r.read_bit(); // unknown

        let _ = r.read_cm_color(); // INTERFERECOLOR

        let _ = r.read_handle(); // INTERFEREOBJVS
        let _ = r.read_handle(); // INTERFEREVPVS
        let _ = r.read_handle(); // DRAGVS

        let _ = r.read_byte(); // CSHADOW
        h.shadow_plane_location = r.read_bit_double();
    }

    // ── R14+ trailing fields ──
    if v >= DxfVersion::AC1014 {
        let _ = r.read_bit_short(); // -1
        let _ = r.read_bit_short(); // -1
        let _ = r.read_bit_short(); // -1
        let _ = r.read_bit_short(); // -1

        if r2004_plus(v) {
            let _ = r.read_bit_long();
            let _ = r.read_bit_long();
            let _ = r.read_bit();
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::dwg_stream_writers::header_writer;

    #[test]
    fn test_header_roundtrip_r2000() {
        let original = HeaderVariables::default();
        let written = header_writer::write_header(DxfVersion::AC1015, &original, 0);
        let read = read_header(&written, DxfVersion::AC1015, 0).unwrap();

        // Check sentinel verification worked
        assert_eq!(read.fill_mode, original.fill_mode);
        assert_eq!(read.ortho_mode, original.ortho_mode);
        assert_eq!(read.linear_unit_format, original.linear_unit_format);
        assert_eq!(read.angular_unit_format, original.angular_unit_format);
        assert!((read.linetype_scale - original.linetype_scale).abs() < 1e-10);
        assert!((read.text_height - original.text_height).abs() < 1e-10);
        assert!((read.dim_scale - original.dim_scale).abs() < 1e-10);
        assert!((read.dim_arrow_size - original.dim_arrow_size).abs() < 1e-10);
    }

    #[test]
    fn test_header_roundtrip_r2004() {
        let original = HeaderVariables::default();
        let written = header_writer::write_header(DxfVersion::AC1018, &original, 0);
        let read = read_header(&written, DxfVersion::AC1018, 0).unwrap();

        assert_eq!(read.fill_mode, original.fill_mode);
        assert_eq!(read.sort_entities, original.sort_entities);
        assert_eq!(read.insertion_units, original.insertion_units);
    }

    #[test]
    fn test_header_roundtrip_r2007() {
        // R2007+ uses three-stream merge (main + text + handle).
        // This test verifies the reader correctly splits the streams,
        // including TEXT values from the separate text sub-stream.
        let mut original = HeaderVariables::default();
        original.fingerprint_guid = "{TEST-GUID-1234}".to_string();
        original.version_guid = "{VERSION-GUID-5678}".to_string();
        original.current_layer_handle = Handle::new(98);
        let written = header_writer::write_header(DxfVersion::AC1021, &original, 0);
        let read = read_header(&written, DxfVersion::AC1021, 0).unwrap();

        // Verify numeric/boolean header variables
        assert_eq!(read.fill_mode, original.fill_mode);
        assert_eq!(read.ortho_mode, original.ortho_mode);
        assert_eq!(
            read.linear_unit_format, original.linear_unit_format,
            "LUNITS should survive roundtrip"
        );
        assert_eq!(read.angular_unit_format, original.angular_unit_format);
        assert!(
            (read.text_height - original.text_height).abs() < 1e-10,
            "TEXTSIZE should survive roundtrip: got {} expected {}",
            read.text_height,
            original.text_height
        );
        assert!((read.linetype_scale - original.linetype_scale).abs() < 1e-10);
        assert_eq!(
            read.attribute_visibility, original.attribute_visibility,
            "ATTMODE should survive roundtrip"
        );
        assert!(
            (read.current_entity_linetype_scale - original.current_entity_linetype_scale).abs()
                < 1e-10,
            "CELTSCALE should survive roundtrip"
        );
        assert_eq!(read.insertion_units, original.insertion_units);
        assert_eq!(read.spline_segments, original.spline_segments);
        assert_eq!(read.sort_entities, original.sort_entities);
        // Verify TEXT values survive the three-stream roundtrip (these go in the
        // separate text sub-stream in R2007+, not inline in main).
        assert_eq!(
            read.fingerprint_guid, original.fingerprint_guid,
            "FINGERPRINTGUID should survive three-stream roundtrip"
        );
        assert_eq!(
            read.version_guid, original.version_guid,
            "VERSIONGUID should survive three-stream roundtrip"
        );
        assert_eq!(read.current_layer_handle, original.current_layer_handle);
    }

    #[test]
    fn test_header_roundtrip_r2010() {
        let mut original = HeaderVariables::default();
        original.current_layer_handle = Handle::new(98);
        let written = header_writer::write_header(DxfVersion::AC1024, &original, 0);
        let read = read_header(&written, DxfVersion::AC1024, 0).unwrap();

        assert_eq!(read.fill_mode, original.fill_mode);
        assert_eq!(read.linear_unit_format, original.linear_unit_format);
        assert!((read.text_height - original.text_height).abs() < 1e-10);
        assert_eq!(read.attribute_visibility, original.attribute_visibility);
        assert_eq!(read.current_layer_handle, original.current_layer_handle);
    }

    #[test]
    fn test_header_bad_sentinel_fails() {
        let mut bad_data = vec![0u8; 50];
        bad_data[..16].fill(0xFF);
        let result = read_header(&bad_data, DxfVersion::AC1015, 0);
        assert!(result.is_err());
    }
}
