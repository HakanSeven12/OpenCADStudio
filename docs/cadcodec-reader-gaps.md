# cadcodec DXF reader/writer gaps found by the OCS Python host audit

Reports on `HakanSeven12/cadcodec` (the `acadrust` crate). Every finding was made against
**cadcodec revision `5b682ed`** by saving a document with `DxfWriter` or `DwgWriter`,
reloading it, and comparing entity fields.

**Status (23 September 2026):** the fixes landed in cadcodec as
[#48](https://github.com/HakanSeven12/cadcodec/pull/48) (issues 1-6) and
[#51](https://github.com/HakanSeven12/cadcodec/pull/51) (the first three style findings), and
the block description with commit `dd1d7bf` (cadcodec issue #49). OCS pins `dd1d7bf`, and the
canaries below were flipped against it. What is still open:

- **ATTDEF `lock_position`** (issue 1): the reader is complete, but the DXF writer emits no
  group 280 at all, neither the version byte nor the lock flag the reader expects after it.
- **LEADER true-colour `override_color`** (issue 4): DXF group 77 holds an ACI index only, so
  a true colour cannot be written there, and the DWG writer does not store the field.
- **`TextStyle::true_type_font`** (style findings): still neither written nor read.

Each report names an executable canary in OCS (`src/app/plugin_host.rs`). The
canaries assert today's wrong result, so they fail loudly once cadcodec is
fixed; flip the marked expectation when that happens. To reproduce, from the
OCS repository:

```sh
STAGE="$PWD/work/bundled-plugins/opencad.python"
bash plugins/opencad-python/tools/stage-bundled.sh "$STAGE" --debug
cargo build --bin OpenCADStudio
OCS_TEST_PYTHON_PLUGIN="$STAGE/libopencad_python.dylib" \
OCS_PLUGIN_RUNNER_EXE="$PWD/target/debug/OpenCADStudio" \
cargo test --lib app::plugin_host::tests::<test name> -- --test-threads=1
```

| # | Area | Severity | Effect |
|---|---|---|---|
| 1 | `ATTDEF` reader | high | Most attribute-definition properties silently reset to defaults |
| 2 | Boolean group codes (290-299) | high | Booleans read through `as_i16()` are never applied |
| 3 | `UNDERLAY` rotation | high | Rotation is not converted back to radians; repeated saves compound it |
| 4 | `LEADER` | medium | Annotation link and three vectors are lost |
| 5 | `DRAWINGVIEW` / `SECTIONLINE` | medium | Typed entity not restored from the ENTITIES section |
| 6 | `TABLE` merged ranges | low | `merged_ranges` is empty after DXF reopen |

---

## 1. ATTDEF reader handles eight group codes; the writer emits all of them

**Where:** `src/io/dxf/reader/section_reader.rs`, `read_attdef` (around line
15647). The writer, `write_attdef` in `writer/section_writer.rs` (around line
4837), emits `7, 11, 41, 51, 70, 71, 72, 73, 74, 210` and `280` in addition to
the groups the reader consumes.

**What happens:** the reader has arms for groups `1, 2, 3, 10/20/30, 40, 50,
280` and `101` only; everything else falls through to `_ =>`. After a DXF round
trip these fields reset to defaults:

- text style name (7)
- alignment point (11/21/31) and both alignments (72, 74)
- width factor (41) and oblique angle (51)
- attribute flags (70): invisible, constant, verify, preset
- text generation flags (71) and field length (73)
- normal (210/220/230) and the position-lock state

Tag, prompt, default value, insertion point, height and rotation survive.
Losing the attribute flags is the worst part: an invisible or constant
attribute becomes visible and editable after a DXF save.

**Evidence:** `audit_python_attribute_definition_lifecycle_over_real_ipc`
compares every writable field through both formats. DWG round-trips the full
state; the DXF digest is pinned to the defaults (see the `BLOCKER` comment on
`expect_edited_dxf`).

**Suggested fix:** add reader arms for the missing groups, using the same
conversions the writer applies (rotation and oblique angle are written in
degrees). `TEXT` and `ATTRIB` already read most of these groups and can share
the implementation.

## 2. Boolean group codes are read with `as_i16()` and never applied

**Where:** `src/io/dxf/reader/stream_reader.rs` defines
`DxfCodePair::as_i16()` as `Some` only for `CodePairValue::Int`. Groups 290-299
are typed `Bool`, so `as_i16()` returns `None` for them and
`unwrap_or(0) != 0` yields `false`. The pattern appears in many arms of
`section_reader.rs`.

**Confirmed impact (round-trip tested):**

- `HELIX` group 290 (`handedness`): a clockwise helix reopens counter-clockwise.
  The reader arm is `290 => { if let Some(v) = pair.as_i16() { ... } }`
  (around line 12796).
- `LIGHT` groups 291, 292, 293 (`plot_glyph`, `use_attenuation_limits`,
  `cast_shadows`; around lines 18154-18182), and 290 for the photometric web
  file flag: `cast_shadows` reopens `false`.

**Same pattern, not individually tested** (line numbers in `section_reader.rs`):
13833, 13955-13969 (point-cloud style flags); 18194 (`has_web_file`);
18431-18438 and 18493-18494 (sweep, loft and revolve option flags); and several
`if let Some(v) = pair.as_i16()` arms on 290/292 near lines 9815, 10096, 10478
and 12861. Some other arms already use `as_bool()` correctly (for example lines
18376 and 18550), so the fix is mechanical.

**Evidence:** `audit_python_helix_lifecycle_over_real_ipc` (canary
`expect_edited_dxf`) and `audit_python_light_lifecycle_over_real_ipc`.

**Suggested fix:** read these groups with a helper that accepts either
representation, for example
`pair.as_bool().or_else(|| pair.as_i16().map(|v| v != 0))`, and audit every arm
whose code is in 290-299. Alternatively make `as_i16()` accept `Bool` values.

## 3. `UNDERLAY` rotation is written in degrees and read back unconverted

**Where:** `write_underlay` writes group 50 with `underlay.rotation.to_degrees()`
(around line 11038 of `writer/section_writer.rs`). `read_underlay` stores the
raw value: `50 => { if let Some(v) = pair.as_double() { underlay.rotation = v; } }`
(`reader/section_reader.rs`, around line 19470), with no `to_radians()`.

**What happens:** a rotation of `0.5` rad is written as 28.6479 degrees and
read back as 28.6479 rad. Because the writer converts again on every save, the
error compounds: a second save-and-reopen yields 1641.4. This silently corrupts
existing drawings each time they pass through cadcodec.

**Evidence:** `audit_python_underlay_lifecycle_over_real_ipc`; the canary
`expect_edited_dxf` / `expect_reedited_dxf` records 28.6 then 1641.4.

**Suggested fix:** `underlay.rotation = v.to_radians()` in the reader. Check
that `DGN`/`DWF`/`PDF` underlays share the code path.

## 4. `LEADER`: annotation link and vectors are lost in DXF

**a. The writer never emits group 340.** `write_leader` writes 3, 71-76, 10, 210,
211, 212 and 213 but no associated-annotation handle, while the reader has a
`340 =>` arm that fills `annotation_handle` (around line 16619). A DXF save
therefore unlinks every leader from its text, tolerance or block annotation.

**b. The coordinate mapping does not reassemble 211, 212 and 213.**
`GroupCodeValueType::coordinate_axis` in `src/io/dxf/group_code_value.rs` maps
X to `10..=18 | 110..=112 | 210 | 1010..=1013` (and the Y/Z equivalents), so
`add_coordinate` ignores the 211/221/231, 212/222/232 and 213/223/233 series.
`read_leader` uses a `PointReader` for `horizontal_direction`, `block_offset`
and `annotation_offset`, so all three reopen as their defaults even though the
writer emits them. (`normal`, group 210, works.)

**c. `override_color` is not persisted** in DXF or DWG: a leader coloured by
`override_color` reopens `ByLayer`. Observed, cause not traced.

**Evidence:** the canaries in `staged_python_leader_lifecycle_over_real_ipc`
(search for `BLOCKER`). DWG keeps the annotation link, offset and arrow flag.

**Suggested fix:** write group 340 for a non-null `annotation_handle`; extend
`coordinate_axis`/`coordinate_group` to cover the 211-213 series (or read those
groups explicitly); decide where the override colour belongs (group 62/420 on
the leader).

## 5. `DRAWINGVIEW` and `SECTIONLINE` are dispatched only inside blocks

**Where:** `section_reader.rs` has two entity dispatch tables: one for block
contents (around lines 3241-3390) and one for the ENTITIES section
(`read_entities`, from line 3482). `"DRAWINGVIEW"` (line 3386) and
`"SECTIONLINE"` (line 3382) appear only in the first; the ENTITIES table has no
arm for them. `write_view_border_dxf` writes `DRAWINGVIEW` correctly.

**What happens:** a `ViewBorder` saved to DXF in the drawing's entity list
reopens as a different, opaque entity kind instead of `ViewBorder`. DWG restores
the typed record. The same applies to `SectionSymbol`.

**Evidence:** `audit_python_view_border_lifecycle_over_real_ipc` (canary
`expect_edited_dxf: "wrong kind"`) and `audit_python_section_symbol_lifecycle_over_real_ipc`
(canary `wrong kind Unknown`).

**Suggested fix:** add the two arms to the ENTITIES dispatch, calling
`read_view_border_dxf` and `read_section_symbol_dxf`.

## 6. `TABLE` `merged_ranges` is empty after a DXF reopen

**What happens:** the writer emits the merged-range list (`AcDbFormattedTableData`,
group 90 then 91-94 per range; near line 10216 of `writer/section_writer.rs`),
and `reader/section_reader/table_content.rs` (around lines 627-690) appears to
parse it, yet `Table::merged_ranges` is empty after a DXF reopen. The merge
itself survives through the origin cell's `merge_width` and `merge_height`
(groups 175 and 176), and DWG restores `merged_ranges` but not the per-cell
dimensions.

**Impact:** low, because the merge is preserved in the cell fields; consumers
that read `merged_ranges` directly see nothing. Cause not traced; it may be that
the entity path does not call the table-content parser.

**Evidence:** `staged_python_table_lifecycle_over_real_ipc` (the DXF branch
asserts an empty `merged_ranges` and marks it as a blocker).

**Suggested fix:** rebuild `merged_ranges` from the cell merge dimensions after
reading, or make sure the `AcDbFormattedTableData` block is parsed for entities.

---

## Observed but not filed

These are format constraints or OCS-side issues, not reader bugs:

- **DWG R2010+ does not store** a leader's `text_height`, `text_width` or
  `hookline_enabled`, or a 3-D polyline's `elevation`; the writer stores them
  only for the older versions that define them.
- **Entity-level `MultiLeader.text_height`** is not stored by either writer
  (`context.text_height` is), so OCS exposes it read-only.
- **Legacy `Polyline` reopens as `Polyline3D`** from both formats and a 2-D
  polyline reopens as `LwPolyline` from DXF. These look like deliberate
  normalization; OCS made the legacy kind update-only and records the DXF
  conversion.
- **Spline weights in DXF** were briefly suspected but are correct: the writer
  emits group 41 only when `flags.rational` is set, so the host must set that
  flag when weights are present (fixed in OCS).

### Text and dimension style findings (2026-09-21)

Found by `audit_python_text_and_dim_styles_over_real_ipc`; each is pinned by a canary. The first
three are fixed in [HakanSeven12/cadcodec#51](https://github.com/HakanSeven12/cadcodec/pull/51)
(independent of #48); the last two are not filed, because fixing them needs the XDATA layout
AutoCAD expects and that cannot be checked here.

- **`STYLE` generation flags:** the DXF writer hard-codes group 71 to 0, so a text
  style's `flags.backward` and `flags.upside_down` are lost on every DXF save (DWG keeps them).
- **`STYLE` oblique angle units:** group 50 is written and read as the raw radian value, while DXF
  defines it in degrees; it round-trips inside cadcodec but AutoCAD would read 15 degrees as 0.26.
  DWG stores radians correctly.
- **`DIMSTYLE` text style name:** the DXF reader keeps only the text-style handle (group 340) and never
  resolves `dimtxsty` from it, so the name reopens as `Standard` (the handle is right; DWG resolves both).
- **`TextStyle::true_type_font`** is never written or read by either codec, so it lives only in memory.
- **`BLOCK_RECORD` description:** the DXF codec did not carry `BlockRecord::description`
  (found by `audit_python_blocks_over_real_ipc`). It needs no XDATA: it is BLOCK group 4, which
  the reader parsed onto the discarded BLOCK marker and the writer never emitted. Fixed in
  cadcodec `dd1d7bf` (cadcodec issue #49).

