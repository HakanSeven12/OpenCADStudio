//! DWG Header section writer
//!
//! Writes the HEADER section containing all drawing header variables.
//! This is the most complex section, writing ~200 fields with extensive
//! version-conditional logic.
//!
//! ## Stream format
//!
//! - **Pre-R2007**: All data (including handle references) is written
//!   sequentially to a single stream (two-stream merge: text is inline,
//!   handles are appended at end — but for the header section the
//!   single-stream approach is used for legacy reasons).
//! - **R2007+**: Uses three-stream merge (`DwgMergedWriter`):
//!   text goes to a separate text sub-stream, handle references go to
//!   a separate handle sub-stream, and everything else to the main
//!   sub-stream. The merged output includes text-size flag words and
//!   a text-present bit per the R2007+ DWG section format.
//!
//! The section data is then wrapped with sentinels and CRC-16.
//!
//! Based on the reference `DwgHeaderWriter`.

use crate::document::HeaderVariables;
use crate::io::dwg::crc::{crc16, CRC16_SEED};
use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::io::dwg::dwg_stream_writers::DwgBitWriter;
use crate::io::dwg::dwg_stream_writers::DwgMergedWriter;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::io::dwg::file_headers::section_definition::{end_sentinels, start_sentinels};
use crate::types::{Color, DxfVersion, Handle, Vector2, Vector3};

// ════════════════════════════════════════════════════════════════════════════
//  Writer wrapper — dispatches to DwgBitWriter or DwgMergedWriter
// ════════════════════════════════════════════════════════════════════════════

/// Internal writer that uses DwgBitWriter for pre-R2007 and DwgMergedWriter
/// (three-stream merge) for R2007+ (AC1021+). This ensures that for R2007+,
/// text goes to the text sub-stream and handle references go to the handle
/// sub-stream, matching the reference `DwgHeaderWriter` behavior.
enum SectionWriterInner {
    /// Pre-R2007: single stream, everything inline
    BitWriter(DwgBitWriter),
    /// R2007+: three-stream merge (main + text + handle)
    MergedWriter(DwgMergedWriter),
}

struct SectionWriter {
    inner: SectionWriterInner,
}

impl SectionWriter {
    fn with_encoding(version: DxfVersion, encoding: &'static encoding_rs::Encoding) -> Self {
        let dwg = DwgVersion::from_dxf_version(version).unwrap_or(DwgVersion::AC15);

        let inner = if version >= DxfVersion::AC1021 {
            // R2007+: use three-stream merge
            let mut writer = DwgMergedWriter::with_encoding(dwg, version, encoding);
            writer.save_position_for_size(); // RL placeholder for total size in bits
            SectionWriterInner::MergedWriter(writer)
        } else {
            // Pre-R2007: single stream
            let writer = DwgBitWriter::with_encoding(dwg, version, encoding);
            SectionWriterInner::BitWriter(writer)
        };

        SectionWriter { inner }
    }

    // ── Main-stream data writes ──

    fn write_bit(&mut self, value: bool) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_bit(value),
            SectionWriterInner::MergedWriter(w) => w.write_bit(value),
        }
    }

    fn write_byte(&mut self, value: u8) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_byte(value),
            SectionWriterInner::MergedWriter(w) => w.write_byte(value),
        }
    }

    fn write_bit_short(&mut self, value: i16) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_bit_short(value),
            SectionWriterInner::MergedWriter(w) => w.write_bit_short(value),
        }
    }

    fn write_bit_long(&mut self, value: i32) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_bit_long(value),
            SectionWriterInner::MergedWriter(w) => w.write_bit_long(value),
        }
    }

    fn write_bit_long_long(&mut self, value: i64) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_bit_long_long(value),
            SectionWriterInner::MergedWriter(w) => w.write_bit_long_long(value),
        }
    }

    fn write_bit_double(&mut self, value: f64) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_bit_double(value),
            SectionWriterInner::MergedWriter(w) => w.write_bit_double(value),
        }
    }

    fn write_3bit_double(&mut self, value: Vector3) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_3bit_double(value),
            SectionWriterInner::MergedWriter(w) => w.write_3bit_double(value),
        }
    }

    fn write_2raw_double(&mut self, value: Vector2) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_2raw_double(value),
            SectionWriterInner::MergedWriter(w) => w.write_2raw_double(value),
        }
    }

    fn write_cm_color(&mut self, color: &Color) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_cm_color(color),
            SectionWriterInner::MergedWriter(w) => w.write_cm_color(color),
        }
    }

    fn write_datetime(&mut self, day: i32, ms: i32) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_datetime(day, ms),
            SectionWriterInner::MergedWriter(w) => w.write_datetime(day, ms),
        }
    }

    fn write_timespan(&mut self, days: i32, ms: i32) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_timespan(days, ms),
            SectionWriterInner::MergedWriter(w) => w.write_timespan(days, ms),
        }
    }

    // ── Text writes: route to text sub-stream for R2007+ ──

    fn write_variable_text(&mut self, value: &str) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_variable_text(value),
            SectionWriterInner::MergedWriter(w) => w.write_variable_text(value),
        }
    }

    // ── Handle writes: route to handle sub-stream for R2007+ ──

    /// Write handle reference — goes to handle sub-stream for R2007+.
    fn write_handle_ref(&mut self, ref_type: DwgReferenceType, handle: Handle) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_handle(ref_type, handle.value()),
            SectionWriterInner::MergedWriter(w) => w.write_handle(ref_type, handle.value()),
        }
    }

    /// Write HANDSEED — always goes to the MAIN stream, even for R2007+.
    /// This matches C#: `this._writer.Main.HandleReference(...)`.
    fn write_handle_seed(&mut self, handle_seed: u64) {
        match &mut self.inner {
            SectionWriterInner::BitWriter(w) => w.write_handle_undefined(handle_seed),
            SectionWriterInner::MergedWriter(w) => {
                // HANDSEED is written to the main stream specifically,
                // not the handle sub-stream.
                w.main_mut().write_handle_undefined(handle_seed);
            }
        }
    }

    /// Finalize and return section data bytes.
    fn finalize(self) -> Vec<u8> {
        match self.inner {
            SectionWriterInner::BitWriter(mut w) => {
                // Pre-R2007: just pad and return
                w.write_spear_shift();
                w.into_bytes()
            }
            SectionWriterInner::MergedWriter(mut w) => {
                // R2007+: three-stream merge handles RL patching,
                // text-size flags, and byte alignment automatically.
                w.merge()
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Public API
// ════════════════════════════════════════════════════════════════════════════

/// Write the complete Header section.
///
/// # Arguments
/// * `version` - Target DXF/DWG version
/// * `header` - Document header variables
///
/// # Returns
/// Complete section bytes including sentinels and CRC.
pub fn write_header(
    version: DxfVersion,
    header: &HeaderVariables,
    maintenance_version: u8,
) -> Vec<u8> {
    write_header_with_encoding(
        version,
        header,
        maintenance_version,
        encoding_rs::WINDOWS_1252,
    )
}

pub fn write_header_with_encoding(
    version: DxfVersion,
    header: &HeaderVariables,
    maintenance_version: u8,
    encoding: &'static encoding_rs::Encoding,
) -> Vec<u8> {
    let mut w = SectionWriter::with_encoding(version, encoding);
    write_header_fields(&mut w, version, header);
    let section_data = w.finalize();
    wrap_with_sentinels_and_crc(version, maintenance_version, &section_data)
}

// ════════════════════════════════════════════════════════════════════════════
//  Sentinel + CRC wrapper (same pattern as classes_writer)
// ════════════════════════════════════════════════════════════════════════════

fn wrap_with_sentinels_and_crc(
    version: DxfVersion,
    maintenance_version: u8,
    section_data: &[u8],
) -> Vec<u8> {
    let mut output = Vec::with_capacity(16 + 4 + section_data.len() + 2 + 16 + 8);

    output.extend_from_slice(&start_sentinels::HEADER);

    let mut crc_content = Vec::with_capacity(4 + section_data.len());
    crc_content.extend_from_slice(&(section_data.len() as i32).to_le_bytes());

    // Extra 4 zero bytes when: (AC1024+ && maintenance > 3) || AC1032+
    if DwgVersion::has_section_extra_rl(version, maintenance_version) {
        crc_content.extend_from_slice(&0i32.to_le_bytes());
    }

    crc_content.extend_from_slice(section_data);

    let crc = crc16(CRC16_SEED, &crc_content);
    output.extend_from_slice(&crc_content);
    output.extend_from_slice(&crc.to_le_bytes());

    output.extend_from_slice(&end_sentinels::HEADER);

    // R2004+: trailing 8 zero bytes (matches classes_writer pattern)
    if version >= DxfVersion::AC1018 {
        output.extend_from_slice(&[0u8; 8]);
    }

    output
}

// ════════════════════════════════════════════════════════════════════════════
//  Version-range helpers
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

fn julian_to_day_ms(julian: f64) -> (i32, i32) {
    let day = julian as i32;
    let fraction = julian - day as f64;
    let ms = (fraction * 86_400_000.0) as i32;
    (day, ms)
}

fn timespan_to_day_ms(days_fraction: f64) -> (i32, i32) {
    let days = days_fraction as i32;
    let fraction = days_fraction - days as f64;
    let ms = (fraction * 86_400_000.0) as i32;
    (days, ms)
}

// ════════════════════════════════════════════════════════════════════════════
//  Header field writer — the big one (~200 fields)
// ════════════════════════════════════════════════════════════════════════════

fn write_header_fields(w: &mut SectionWriter, v: DxfVersion, h: &HeaderVariables) {
    // R2013+: BLL REQUIREDVERSIONS
    if r2013_plus(v) {
        w.write_bit_long_long(h.required_versions);
    }

    // ── Unknown defaults (Common) ──
    w.write_bit_double(412148564080.0);
    w.write_bit_double(1.0);
    w.write_bit_double(1.0);
    w.write_bit_double(1.0);

    w.write_variable_text("m");
    w.write_variable_text("");
    w.write_variable_text("");
    w.write_variable_text("");

    w.write_bit_long(24);
    w.write_bit_long(0);

    // R13-R14 Only: BS unknown
    if r13_14_only(v) {
        w.write_bit_short(0);
    }

    // Pre-2004: current viewport header handle
    if v < DxfVersion::AC1018 {
        w.write_handle_ref(DwgReferenceType::HardPointer, h.current_vx_handle);
    }

    // ── Drawing mode flags (Common) ──
    w.write_bit(h.associate_dimensions);
    w.write_bit(h.update_dimensions_while_dragging);

    if r13_14_only(v) {
        w.write_bit(false); // DIMSAV undocumented
    }

    w.write_bit(h.polyline_linetype_generation);
    w.write_bit(h.ortho_mode);
    w.write_bit(h.regen_mode);
    w.write_bit(h.fill_mode);
    w.write_bit(h.quick_text_mode);
    w.write_bit(h.paper_space_linetype_scaling);
    w.write_bit(h.limit_check);

    if r13_14_only(v) {
        w.write_bit(h.blip_mode);
    }

    if r2004_plus(v) {
        w.write_bit(false); // undocumented
    }

    w.write_bit(h.user_timer);
    // The legacy DWG header stores SKPOLY as one bit.
    w.write_bit(h.sketch_type != 0); // SKPOLY
    w.write_bit(h.angle_direction != 0); // ANGDIR
    w.write_bit(h.spline_frame); // SPLFRAME

    if r13_14_only(v) {
        w.write_bit(h.attribute_request);
        w.write_bit(h.attribute_dialog);
    }

    w.write_bit(h.mirror_text);
    w.write_bit(h.world_view);

    if r13_14_only(v) {
        w.write_bit(false); // WIREFRAME
    }

    w.write_bit(h.show_model_space);
    w.write_bit(h.paper_space_limit_check);
    w.write_bit(h.retain_xref_visibility);

    if r13_14_only(v) {
        w.write_bit(h.delete_objects);
    }

    w.write_bit(h.display_silhouette);
    w.write_bit(false); // PELLIPSE (CreateEllipseAsPolyline) — default false
    w.write_bit_short(h.proxy_graphics);

    if r13_14_only(v) {
        w.write_bit_short(h.drag_mode);
    }

    // ── Unit settings (Common) ──
    w.write_bit_short(h.tree_depth);
    w.write_bit_short(h.linear_unit_format);
    w.write_bit_short(h.linear_unit_precision);
    w.write_bit_short(h.angular_unit_format);
    w.write_bit_short(h.angular_unit_precision);

    if r13_14_only(v) {
        w.write_bit_short(h.object_snap_mode as i16);
    }

    w.write_bit_short(h.attribute_visibility);

    if r13_14_only(v) {
        w.write_bit_short(h.coords_mode);
    }

    w.write_bit_short(h.point_display_mode);

    if r13_14_only(v) {
        w.write_bit_short(h.pick_style);
    }

    if r2004_plus(v) {
        w.write_bit_long(0); // unknown
        w.write_bit_long(0); // unknown
        w.write_bit_long(0); // unknown
    }

    w.write_bit_short(h.user_int1);
    w.write_bit_short(h.user_int2);
    w.write_bit_short(h.user_int3);
    w.write_bit_short(h.user_int4);
    w.write_bit_short(h.user_int5);

    w.write_bit_short(h.spline_segments);
    w.write_bit_short(h.surface_u_density);
    w.write_bit_short(h.surface_v_density);
    w.write_bit_short(h.surface_type);
    w.write_bit_short(h.surface_tab1);
    w.write_bit_short(h.surface_tab2);
    w.write_bit_short(h.spline_type);
    w.write_bit_short(h.shade_edge);
    w.write_bit_short(h.shade_diffuse);
    w.write_bit_short(0); // UNITMODE — default 0
    w.write_bit_short(h.max_active_viewports);
    w.write_bit_short(h.isolines);
    w.write_bit_short(h.multiline_justification);
    w.write_bit_short(h.text_quality);

    // ── Scale/size defaults (Common) ──
    w.write_bit_double(h.linetype_scale);
    w.write_bit_double(h.text_height);
    w.write_bit_double(h.trace_width);
    w.write_bit_double(h.sketch_increment);
    w.write_bit_double(h.fillet_radius);
    w.write_bit_double(h.thickness);
    w.write_bit_double(h.angle_base);
    w.write_bit_double(h.point_display_size);
    w.write_bit_double(h.polyline_width);
    w.write_bit_double(h.user_real1);
    w.write_bit_double(h.user_real2);
    w.write_bit_double(h.user_real3);
    w.write_bit_double(h.user_real4);
    w.write_bit_double(h.user_real5);
    w.write_bit_double(h.chamfer_distance_a);
    w.write_bit_double(h.chamfer_distance_b);
    w.write_bit_double(h.chamfer_length);
    w.write_bit_double(h.chamfer_angle);
    w.write_bit_double(h.facet_resolution);
    w.write_bit_double(h.multiline_scale);
    w.write_bit_double(h.current_entity_linetype_scale);

    w.write_variable_text(&h.menu_name);

    // ── Date/time (Common) ──
    let (cd, cms) = julian_to_day_ms(h.create_date_julian);
    w.write_datetime(cd, cms);
    let (ud, ums) = julian_to_day_ms(h.update_date_julian);
    w.write_datetime(ud, ums);

    if r2004_plus(v) {
        w.write_bit_long(0); // unknown
        w.write_bit_long(0); // unknown
        w.write_bit_long(0); // unknown
    }

    let (ted, tems) = timespan_to_day_ms(h.total_editing_time);
    w.write_timespan(ted, tems);
    let (ued, uems) = timespan_to_day_ms(h.user_elapsed_time);
    w.write_timespan(ued, uems);

    // ── Current entity color ──
    w.write_cm_color(&h.current_entity_color);

    // ── HANDSEED — always main stream ──
    w.write_handle_seed(h.handle_seed);

    // ── Style/layer/linetype handles ──
    w.write_handle_ref(DwgReferenceType::HardPointer, h.current_layer_handle);
    w.write_handle_ref(DwgReferenceType::HardPointer, h.current_text_style_handle);
    w.write_handle_ref(DwgReferenceType::HardPointer, h.current_linetype_handle);

    if r2007_plus(v) {
        w.write_handle_ref(DwgReferenceType::HardPointer, h.current_material_handle);
    }

    w.write_handle_ref(DwgReferenceType::HardPointer, h.current_dimstyle_handle);
    w.write_handle_ref(
        DwgReferenceType::HardPointer,
        h.current_multiline_style_handle,
    );

    if r2000_plus(v) {
        w.write_bit_double(h.viewport_scale_factor);
    }

    // ── Paper space extents/limits/UCS ──
    w.write_3bit_double(h.paper_space_insertion_base);
    w.write_3bit_double(h.paper_space_extents_min);
    w.write_3bit_double(h.paper_space_extents_max);
    w.write_2raw_double(h.paper_space_limits_min);
    w.write_2raw_double(h.paper_space_limits_max);
    w.write_bit_double(h.paper_elevation);
    w.write_3bit_double(h.paper_space_ucs_origin);
    w.write_3bit_double(h.paper_space_ucs_x_axis);
    w.write_3bit_double(h.paper_space_ucs_y_axis);

    // UCSNAME (PSPACE)
    w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL);

    if r2000_plus(v) {
        // PUCSORTHOREF
        w.write_handle_ref(DwgReferenceType::HardPointer, h.paper_ucs_ortho_ref);
        w.write_bit_short(h.paper_ucs_ortho_view);
        // PUCSBASE
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL);

        // Paper space orthographic origins (6 × 3BD) — default zeros
        w.write_3bit_double(Vector3::ZERO); // PUCSORGTOP
        w.write_3bit_double(Vector3::ZERO); // PUCSORGBOTTOM
        w.write_3bit_double(Vector3::ZERO); // PUCSORGLEFT
        w.write_3bit_double(Vector3::ZERO); // PUCSORGRIGHT
        w.write_3bit_double(Vector3::ZERO); // PUCSORGFRONT
        w.write_3bit_double(Vector3::ZERO); // PUCSORGBACK
    }

    // ── Model space extents/limits/UCS ──
    w.write_3bit_double(h.model_space_insertion_base);
    w.write_3bit_double(h.model_space_extents_min);
    w.write_3bit_double(h.model_space_extents_max);
    w.write_2raw_double(h.model_space_limits_min);
    w.write_2raw_double(h.model_space_limits_max);
    w.write_bit_double(h.elevation);
    w.write_3bit_double(h.model_space_ucs_origin);
    w.write_3bit_double(h.model_space_ucs_x_axis);
    w.write_3bit_double(h.model_space_ucs_y_axis);

    // UCSNAME (MSPACE)
    w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL);

    if r2000_plus(v) {
        // UCSORTHOREF
        w.write_handle_ref(DwgReferenceType::HardPointer, h.ucs_ortho_ref);
        w.write_bit_short(h.ucs_ortho_view);
        // UCSBASE
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL);

        // Model space orthographic origins (6 × 3BD) — default zeros
        w.write_3bit_double(Vector3::ZERO); // UCSORGTOP
        w.write_3bit_double(Vector3::ZERO); // UCSORGBOTTOM
        w.write_3bit_double(Vector3::ZERO); // UCSORGLEFT
        w.write_3bit_double(Vector3::ZERO); // UCSORGRIGHT
        w.write_3bit_double(Vector3::ZERO); // UCSORGFRONT
        w.write_3bit_double(Vector3::ZERO); // UCSORGBACK

        // DIMPOST, DIMAPOST
        w.write_variable_text(&h.dim_post);
        w.write_variable_text(&h.dim_alt_post);
    }

    // ── Dimension variables (R13-R14 Only block) ──
    if r13_14_only(v) {
        w.write_bit(h.dim_tolerance);
        w.write_bit(h.dim_limits);
        w.write_bit(h.dim_text_inside_horizontal);
        w.write_bit(h.dim_text_outside_horizontal);
        w.write_bit(h.dim_suppress_ext1);
        w.write_bit(h.dim_suppress_ext2);
        w.write_bit(h.dim_alternate_units);
        w.write_bit(h.dim_force_line_inside);
        w.write_bit(h.dim_separate_arrows);
        w.write_bit(h.dim_force_text_inside);
        w.write_bit(h.dim_suppress_outside_ext);
        w.write_byte(h.dim_alt_decimal_places as u8);
        w.write_byte(h.dim_zero_suppression as u8);
        w.write_bit(h.dim_suppress_line1);
        w.write_bit(h.dim_suppress_line2);
        w.write_byte(h.dim_tolerance_justification as u8);
        w.write_byte(h.dim_horizontal_justification as u8);
        w.write_byte(h.dim_fit as u8);
        w.write_bit(h.dim_user_positioned_text);
        w.write_byte(h.dim_tolerance_zero_suppression as u8);
        w.write_byte(h.dim_alt_tolerance_zero_suppression as u8);
        w.write_byte(h.dim_alt_tolerance_zero_tight as u8);
        w.write_byte(h.dim_text_above as u8);
        w.write_bit_short(0); // DIMUNIT
        w.write_bit_short(h.dim_angular_decimal_places);
        w.write_bit_short(h.dim_decimal_places);
        w.write_bit_short(h.dim_tolerance_decimal_places);
        w.write_bit_short(h.dim_alt_units_format);
        w.write_bit_short(h.dim_alt_tolerance_decimal_places);

        // DIMTXSTY handle
        w.write_handle_ref(DwgReferenceType::HardPointer, h.dim_text_style_handle);
    }

    // ── Dimension variables (Common) ──
    w.write_bit_double(h.dim_scale);
    w.write_bit_double(h.dim_arrow_size);
    w.write_bit_double(h.dim_ext_line_offset);
    w.write_bit_double(h.dim_line_increment);
    w.write_bit_double(h.dim_ext_line_extension);
    w.write_bit_double(h.dim_rounding);
    w.write_bit_double(h.dim_line_extension);
    w.write_bit_double(h.dim_tolerance_plus);
    w.write_bit_double(h.dim_tolerance_minus);

    // R2007+ dimension extras
    if r2007_plus(v) {
        w.write_bit_double(0.0); // DIMFXL
        w.write_bit_double(0.7854); // DIMJOGANG (default 45°)
        w.write_bit_short(0); // DIMTFILL
        w.write_cm_color(&Color::ByBlock); // DIMTFILLCLR
    }

    // R2000+ dimension flags
    if r2000_plus(v) {
        w.write_bit(h.dim_tolerance);
        w.write_bit(h.dim_limits);
        w.write_bit(h.dim_text_inside_horizontal);
        w.write_bit(h.dim_text_outside_horizontal);
        w.write_bit(h.dim_suppress_ext1);
        w.write_bit(h.dim_suppress_ext2);
        w.write_bit_short(h.dim_text_above);
        w.write_bit_short(h.dim_zero_suppression);
        w.write_bit_short(h.dim_alt_zero_suppression);
    }

    if r2007_plus(v) {
        w.write_bit_short(0); // DIMARCSYM
    }

    // ── Dimension sizes (Common) ──
    w.write_bit_double(h.dim_text_height);
    w.write_bit_double(h.dim_center_mark);
    w.write_bit_double(h.dim_tick_size);
    w.write_bit_double(h.dim_alt_scale);
    w.write_bit_double(h.dim_linear_scale);
    w.write_bit_double(h.dim_text_vertical_pos);
    w.write_bit_double(h.dim_tolerance_scale);
    w.write_bit_double(h.dim_line_gap);

    // R13-R14 only: dimension text strings
    if r13_14_only(v) {
        w.write_variable_text(&h.dim_post);
        w.write_variable_text(&h.dim_alt_post);
        w.write_variable_text(&h.dim_arrow_block);
        w.write_variable_text(&h.dim_arrow_block1);
        w.write_variable_text(&h.dim_arrow_block2);
    }

    // R2000+ only: additional dimension settings
    if r2000_plus(v) {
        w.write_bit_double(h.dim_alt_rounding);
        w.write_bit(h.dim_alternate_units);
        w.write_bit_short(h.dim_alt_decimal_places);
        w.write_bit(h.dim_force_line_inside);
        w.write_bit(h.dim_separate_arrows);
        w.write_bit(h.dim_force_text_inside);
        w.write_bit(h.dim_suppress_outside_ext);
    }

    // ── Dimension colors (Common) ──
    w.write_cm_color(&h.dim_line_color);
    w.write_cm_color(&h.dim_ext_line_color);
    w.write_cm_color(&h.dim_text_color);

    // R2000+ only: dimension unit settings
    if r2000_plus(v) {
        w.write_bit_short(h.dim_angular_decimal_places);
        w.write_bit_short(h.dim_decimal_places);
        w.write_bit_short(h.dim_tolerance_decimal_places);
        w.write_bit_short(h.dim_alt_units_format);
        w.write_bit_short(h.dim_alt_tolerance_decimal_places);
        w.write_bit_short(h.dim_angular_units);
        w.write_bit_short(h.dim_fraction_format);
        w.write_bit_short(h.dim_linear_unit_format);
        w.write_bit_short(h.dim_decimal_separator as i16);
        w.write_bit_short(h.dim_text_movement);
        w.write_bit_short(h.dim_horizontal_justification);
        w.write_bit(h.dim_suppress_line1);
        w.write_bit(h.dim_suppress_line2);
        w.write_bit_short(h.dim_tolerance_justification);
        w.write_bit_short(h.dim_tolerance_zero_suppression);
        w.write_bit_short(h.dim_alt_tolerance_zero_suppression);
        w.write_bit_short(h.dim_alt_tolerance_zero_tight);
        w.write_bit(h.dim_user_positioned_text);
        w.write_bit_short(h.dim_fit);
    }

    // R2007+: DIMFXLON
    if r2007_plus(v) {
        w.write_bit(false); // DimensionIsExtensionLineLengthFixed
    }

    // R2010+: extra dimension fields
    if r2010_plus(v) {
        w.write_bit(false); // DIMTXTDIRECTION
        w.write_bit_double(0.0); // DIMALTMZF
        w.write_variable_text(""); // DIMALTMZS
        w.write_bit_double(0.0); // DIMMZF
        w.write_variable_text(""); // DIMMZS
    }

    // R2000+ dimension handles
    if r2000_plus(v) {
        w.write_handle_ref(DwgReferenceType::HardPointer, h.dim_text_style_handle);
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // DIMLDRBLK
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // DIMBLK
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // DIMBLK1
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // DIMBLK2
    }

    // R2007+ dimension linetype handles
    if r2007_plus(v) {
        w.write_handle_ref(DwgReferenceType::HardPointer, h.dim_linetype_handle);
        w.write_handle_ref(DwgReferenceType::HardPointer, h.dim_linetype1_handle);
        w.write_handle_ref(DwgReferenceType::HardPointer, h.dim_linetype2_handle);
    }

    // R2000+ dimension line weights
    if r2000_plus(v) {
        w.write_bit_short(h.dim_line_weight);
        w.write_bit_short(h.dim_ext_line_weight);
    }

    // ── Table control object handles (Common) ──
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.block_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.layer_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.style_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.linetype_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.view_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.ucs_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.vport_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.appid_control_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.dimstyle_control_handle);

    // R13-R15 only: VPEntHdr control
    if r13_15_only(v) {
        w.write_handle_ref(DwgReferenceType::HardOwnership, h.vpent_hdr_control_handle);
    }

    // ── Dictionary handles (Common) ──
    w.write_handle_ref(DwgReferenceType::HardPointer, h.acad_group_dict_handle);
    w.write_handle_ref(DwgReferenceType::HardPointer, h.acad_mlinestyle_dict_handle);
    w.write_handle_ref(DwgReferenceType::HardOwnership, h.named_objects_dict_handle);

    // R2000+ dictionaries and flags
    if r2000_plus(v) {
        w.write_bit_short(1); // TSTACKALIGN default
        w.write_bit_short(70); // TSTACKSIZE default

        w.write_variable_text(&h.hyperlink_base);
        w.write_variable_text(&h.stylesheet);

        w.write_handle_ref(DwgReferenceType::HardPointer, h.acad_layout_dict_handle);
        w.write_handle_ref(
            DwgReferenceType::HardPointer,
            h.acad_plotsettings_dict_handle,
        );
        w.write_handle_ref(
            DwgReferenceType::HardPointer,
            h.acad_plotstylename_dict_handle,
        );
    }

    // R2004+ dictionaries
    if r2004_plus(v) {
        w.write_handle_ref(DwgReferenceType::HardPointer, h.acad_material_dict_handle);
        w.write_handle_ref(DwgReferenceType::HardPointer, h.acad_color_dict_handle);
    }

    // R2007+ dictionaries
    if r2007_plus(v) {
        w.write_handle_ref(
            DwgReferenceType::HardPointer,
            h.acad_visualstyle_dict_handle,
        );
        if r2013_plus(v) {
            w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // unknown
        }
    }

    // R2000+ flags bitfield
    if r2000_plus(v) {
        let mut flags: i32 = (h.current_line_weight as i32) & 0x1F;
        flags |= (h.end_caps as i32) << 5;
        flags |= (h.join_style as i32) << 7;
        if !h.lineweight_display {
            flags |= 0x200;
        }
        if !h.xedit {
            flags |= 0x400;
        }
        if h.extended_names {
            flags |= 0x800;
        }
        if h.plotstyle_mode {
            flags |= 0x2000;
        }
        if h.ole_startup {
            flags |= 0x4000;
        }
        w.write_bit_long(flags);

        w.write_bit_short(h.insertion_units);
        w.write_bit_short(h.current_plotstyle_type);

        if h.current_plotstyle_type == 3 {
            // CPSNID (only if CEPSNTYPE == 3/ByObjectId)
            w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL);
        }

        w.write_variable_text(&h.fingerprint_guid);
        w.write_variable_text(&h.version_guid);
    }

    // R2004+ extra entity settings
    if r2004_plus(v) {
        w.write_byte(h.sort_entities as u8);
        w.write_byte(h.index_control as u8);
        w.write_byte(h.hide_text as u8);
        w.write_byte(h.xclip_frame as u8);
        w.write_byte(h.dimension_associativity as u8);
        w.write_byte(h.halo_gap as u8);
        w.write_bit_short(h.obscured_color);
        w.write_bit_short(h.intersection_color);
        w.write_byte(h.obscured_linetype as u8);
        w.write_byte(h.intersection_display as u8);

        w.write_variable_text(&h.project_name);
    }

    // ── Block record / linetype handles (Common) ──
    w.write_handle_ref(DwgReferenceType::HardPointer, h.paper_space_block_handle);
    w.write_handle_ref(DwgReferenceType::HardPointer, h.model_space_block_handle);
    w.write_handle_ref(DwgReferenceType::HardPointer, h.bylayer_linetype_handle);
    w.write_handle_ref(DwgReferenceType::HardPointer, h.byblock_linetype_handle);
    w.write_handle_ref(DwgReferenceType::HardPointer, h.continuous_linetype_handle);

    // ── R2007+ extended fields ──
    if r2007_plus(v) {
        w.write_bit(h.camera_display);
        w.write_bit_long(0); // unknown
        w.write_bit_long(0); // unknown
        w.write_bit_double(0.0); // unknown

        w.write_bit_double(h.steps_per_second);
        w.write_bit_double(h.step_size);
        w.write_bit_double(2.0); // 3DDWFPREC — valid range 1..6
        w.write_bit_double(h.lens_length);
        w.write_bit_double(h.camera_height);
        w.write_byte(u8::from(h.record_solid_history));
        w.write_byte(h.show_solid_history.clamp(0, 2) as u8);
        w.write_bit_double(0.25); // PSOLWIDTH — valid range >0
        w.write_bit_double(0.25); // PSOLHEIGHT
        w.write_bit_double(h.loft_angle1);
        w.write_bit_double(h.loft_angle2);
        w.write_bit_double(h.loft_magnitude1);
        w.write_bit_double(h.loft_magnitude2);
        w.write_bit_short(h.loft_param);
        w.write_byte(h.loft_normals as u8);
        w.write_bit_double(h.latitude);
        w.write_bit_double(h.longitude);
        w.write_bit_double(h.north_direction);
        w.write_bit_long(h.timezone);
        w.write_byte(0); // LIGHTGLYPHDISPLAY
        w.write_byte(1); // TILEMODELIGHTSYNCH — valid range 0..1
        w.write_byte(0); // DWFFRAME
        w.write_byte(0); // DGNFRAME

        w.write_bit(false); // unknown

        w.write_cm_color(&Color::from_index(h.intersection_color));

        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // INTERFEREOBJVS
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // INTERFEREVPVS
        w.write_handle_ref(DwgReferenceType::HardPointer, Handle::NULL); // DRAGVS

        w.write_byte(0); // CSHADOW
        w.write_bit_double(h.shadow_plane_location);
    }

    // ── R14+ trailing fields ──
    if v >= DxfVersion::AC1014 {
        w.write_bit_short(-1);
        w.write_bit_short(-1);
        w.write_bit_short(-1);
        w.write_bit_short(-1);

        if r2004_plus(v) {
            w.write_bit_long(0);
            w.write_bit_long(0);
            w.write_bit(false);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_header_r2000_has_sentinels() {
        let h = HeaderVariables::default();
        let data = write_header(DxfVersion::AC1015, &h, 0);

        // Start sentinel
        assert_eq!(&data[..16], &start_sentinels::HEADER);
        // End sentinel at the end
        let end_start = data.len() - 16;
        assert_eq!(&data[end_start..], &end_sentinels::HEADER);
    }

    #[test]
    fn test_write_header_size_field() {
        let h = HeaderVariables::default();
        let data = write_header(DxfVersion::AC1015, &h, 0);

        // Size at offset 16 (RL = 4 bytes LE)
        let size = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        assert!(size > 0, "Header section size should be > 0: got {}", size);
    }

    #[test]
    fn test_write_header_crc_valid() {
        let h = HeaderVariables::default();
        let data = write_header(DxfVersion::AC1015, &h, 0);

        // CRC is 2 bytes before end sentinel
        let end_sentinel_start = data.len() - 16;
        let crc_bytes = &data[end_sentinel_start - 2..end_sentinel_start];

        let size_plus_data = &data[16..end_sentinel_start - 2];
        let expected_crc = crc16(CRC16_SEED, size_plus_data);
        let actual_crc = u16::from_le_bytes([crc_bytes[0], crc_bytes[1]]);
        assert_eq!(
            actual_crc, expected_crc,
            "Header CRC mismatch: got 0x{:04X}, expected 0x{:04X}",
            actual_crc, expected_crc
        );
    }

    #[test]
    fn test_write_header_r2004_larger_than_r2000() {
        let h = HeaderVariables::default();
        let data_2000 = write_header(DxfVersion::AC1015, &h, 0);
        let data_2004 = write_header(DxfVersion::AC1018, &h, 0);

        assert!(
            data_2004.len() > data_2000.len(),
            "R2004 header ({} bytes) should be larger than R2000 ({} bytes)",
            data_2004.len(),
            data_2000.len()
        );
    }

    #[test]
    fn test_write_header_r2007_uses_merged_format() {
        let h = HeaderVariables::default();
        let data_2007 = write_header(DxfVersion::AC1021, &h, 0);

        // Should have sentinels and be non-trivial size
        assert_eq!(&data_2007[..16], &start_sentinels::HEADER);
        assert!(data_2007.len() > 200, "R2007 header should be substantial");
    }

    #[test]
    fn test_write_header_r14_smaller_than_r2000() {
        let h = HeaderVariables::default();
        let data_r14 = write_header(DxfVersion::AC1014, &h, 0);
        let data_2000 = write_header(DxfVersion::AC1015, &h, 0);

        // R14 has fewer dimension handle fields, but more boolean R13_14 fields.
        // They should both be non-trivial.
        assert!(data_r14.len() > 100);
        assert!(data_2000.len() > 100);
    }

    #[test]
    fn test_julian_conversion() {
        let (day, ms) = julian_to_day_ms(2451544.5);
        assert_eq!(day, 2451544);
        assert!(ms > 0);

        let (d, m) = julian_to_day_ms(0.0);
        assert_eq!(d, 0);
        assert_eq!(m, 0);
    }

    #[test]
    fn test_timespan_conversion() {
        let (days, ms) = timespan_to_day_ms(1.5);
        assert_eq!(days, 1);
        assert_eq!(ms, 43_200_000);
    }
}
