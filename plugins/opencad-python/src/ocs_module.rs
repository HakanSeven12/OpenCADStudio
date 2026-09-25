//! The `ocs` Python module: read-only entity access (Phase 1.2) plus 2D write
//! access (Phase 1.3) of `docs/python-scripting-roadmap.md`.

pub(crate) use ocs::module_def;

#[rustpython_vm::pymodule]
mod ocs {
    use std::io::Write;

    #[cfg(feature = "experimental-host-settings")]
    use ocs_plugin_api::host::HostSettingValue;
    #[cfg(feature = "experimental-host-settings")]
    use rustpython_vm::convert::TryFromObject;
    use rustpython_vm::{function::ArgIntoFloat, PyObjectRef, PyResult, VirtualMachine};

    use codec::entities::{
        Arc, LwPolyline, LwVertex, MText, TableBuilder, Text, TextHorizontalAlignment,
        TextVerticalAlignment,
    };
    use codec::objects::ObjectType;
    use codec::types::{Transform, Vector2, Vector3};
    use ocs_plugin_api::host::{
        codec, EntityType, ExtendedDataRecord, Handle, ReaderEntityKind, XDataValue,
    };

    use crate::host_ctx;

    /// Write one new UTF-8 report file. The caller must provide an explicit
    /// path; existing files are never overwritten and parent directories are
    /// not created.
    #[pyfunction]
    fn write_new_text_file(path: String, content: String, vm: &VirtualMachine) -> PyResult<()> {
        if host_ctx::with_host(|_| ()).is_none() {
            return Err(vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()));
        }
        if path.is_empty() {
            return Err(vm.new_value_error("report path must not be empty".to_owned()));
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                vm.new_runtime_error(format!("cannot create report {path}: {error}"))
            })?;
        if let Err(error) = file.write_all(content.as_bytes()) {
            return Err(vm.new_runtime_error(format!(
                "cannot finish report {path}: {error}; a partial file may remain"
            )));
        }
        Ok(())
    }

    fn top_level_blocks(document: &codec::CadDocument) -> std::collections::HashSet<Handle> {
        let mut blocks: std::collections::HashSet<Handle> = document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                    Some(layout.block_record)
                }
                _ => None,
            })
            .collect();
        if !document.header.model_space_block_handle.is_null() {
            blocks.insert(document.header.model_space_block_handle);
        }
        if let Some(model) = document.block_records.get("*Model_Space") {
            blocks.insert(model.handle);
        }
        blocks
    }

    /// Handles of the entities selected as of the last `SelectionChangedV4`
    /// notification (see `selection_cache.rs` — a best-effort cache, not a
    /// live query).
    #[pyfunction]
    fn selection(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        #[cfg(feature = "experimental-host-model")]
        let handles = host_ctx::with_host(|host| host.selection()).ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })?;
        #[cfg(not(feature = "experimental-host-model"))]
        let handles = crate::selection_cache::get();
        let items = handles
            .into_iter()
            .map(|h| vm.new_pyobj(h.value()))
            .collect();
        Ok(vm.ctx.new_list(items).into())
    }

    /// Drain best-effort drawing, selection, and command-state notifications.
    /// Events accumulated since the previous poll, up to a bounded queue.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn poll_events(tab_id: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        use crate::event_cache::Event;
        let current_tab = host_ctx::with_host(|host| host.tab_id()).ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })?;
        if tab_id != current_tab {
            return Err(
                vm.new_value_error("ocs.poll_events: tab_id is not the active document".to_owned())
            );
        }
        let mut items = Vec::new();
        for event in crate::event_cache::drain_for(tab_id) {
            let row = vm.ctx.new_dict();
            match event {
                Event::Drawing { tab_id, version } => {
                    row.set_item("type", vm.new_pyobj("drawing"), vm)?;
                    row.set_item("tab_id", vm.new_pyobj(tab_id), vm)?;
                    row.set_item("version", vm.new_pyobj(version), vm)?;
                }
                Event::Selection { tab_id, handles } => {
                    row.set_item("type", vm.new_pyobj("selection"), vm)?;
                    row.set_item("tab_id", vm.new_pyobj(tab_id), vm)?;
                    row.set_item(
                        "handles",
                        vm.ctx
                            .new_list(handles.into_iter().map(|h| vm.new_pyobj(h)).collect())
                            .into(),
                        vm,
                    )?;
                }
                Event::Command { tab_id, command } => {
                    row.set_item("type", vm.new_pyobj("command"), vm)?;
                    row.set_item("tab_id", vm.new_pyobj(tab_id), vm)?;
                    row.set_item("command", vm.new_pyobj(command), vm)?;
                }
                Event::Overflow { tab_id, dropped } => {
                    row.set_item("type", vm.new_pyobj("overflow"), vm)?;
                    row.set_item("tab_id", vm.new_pyobj(tab_id), vm)?;
                    row.set_item("dropped", vm.new_pyobj(dropped), vm)?;
                }
            }
            items.push(row.into());
        }
        Ok(vm.ctx.new_list(items).into())
    }

    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn tab_id(vm: &VirtualMachine) -> PyResult<u64> {
        host_ctx::with_host(|host| host.tab_id())
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Start a host-driven point or entity pick. Read the result in a later
    /// script invocation with `poll_input(token)`.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn request_input(prompt: String, entity: bool, vm: &VirtualMachine) -> PyResult<u64> {
        if prompt.trim().is_empty() {
            return Err(vm.new_value_error("prompt must not be empty".to_owned()));
        }
        host_ctx::with_host(|host| {
            let (token, command) = crate::input_cache::begin(host.tab_id(), prompt, entity);
            host.start_interactive(Box::new(command));
            token
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn poll_input(token: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        use crate::input_cache::PickResult;
        let tab_id = host_ctx::with_host(|host| host.tab_id()).ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })?;
        let Some(result) = crate::input_cache::take(tab_id, token) else {
            return Ok(vm.ctx.none());
        };
        let row = vm.ctx.new_dict();
        match result {
            PickResult::Pending => row.set_item("status", vm.new_pyobj("pending"), vm)?,
            PickResult::Cancelled => row.set_item("status", vm.new_pyobj("cancelled"), vm)?,
            PickResult::Point(point) => {
                row.set_item("status", vm.new_pyobj("point"), vm)?;
                row.set_item(
                    "point",
                    vm.ctx
                        .new_list(point.into_iter().map(|v| vm.new_pyobj(v)).collect())
                        .into(),
                    vm,
                )?;
            }
            PickResult::Entity { handle, point } => {
                row.set_item("status", vm.new_pyobj("entity"), vm)?;
                row.set_item("handle", vm.new_pyobj(handle), vm)?;
                row.set_item(
                    "point",
                    vm.ctx
                        .new_list(point.into_iter().map(|v| vm.new_pyobj(v)).collect())
                        .into(),
                    vm,
                )?;
            }
        }
        Ok(row.into())
    }

    /// Read-only geometry/properties for one entity handle, or `None` if it
    /// doesn't exist. Backed by `HostApi::document_reader()` (zero-copy read).
    #[pyfunction]
    fn get(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let target = Handle::new(handle);
        let found = host_ctx::with_host(|host| {
            let reader = host.document_reader();
            let mut found = None;
            reader.for_each_entity(&mut |entity| {
                if found.is_none() && entity.handle == target {
                    found = Some((entity.kind, entity.layer_name.to_string(), entity.point));
                }
            });
            found
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;

        let Some((kind, layer, point)) = found else {
            return Ok(vm.ctx.none());
        };

        let dict = vm.ctx.new_dict();
        dict.set_item("handle", vm.new_pyobj(handle), vm)?;
        dict.set_item("kind", vm.new_pyobj(kind_name(kind)), vm)?;
        dict.set_item("layer", vm.new_pyobj(layer), vm)?;
        let point_obj = match point {
            Some(p) => {
                let pd = vm.ctx.new_dict();
                pd.set_item("x", vm.new_pyobj(p.x), vm)?;
                pd.set_item("y", vm.new_pyobj(p.y), vm)?;
                pd.set_item("z", vm.new_pyobj(p.z), vm)?;
                pd.into()
            }
            None => vm.ctx.none(),
        };
        dict.set_item("point", point_obj, vm)?;
        Ok(dict.into())
    }

    /// Full single-line TEXT geometry from the host's document snapshot.
    /// `point` is the active DXF alignment point (10 for left/baseline,
    /// otherwise 11), in the entity's OCS coordinate system.
    #[pyfunction]
    fn get_text(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found =
            host_ctx::with_host(
                |host| match host.document().get_entity(Handle::new(handle)) {
                    Some(EntityType::Text(text)) => Some(text.clone()),
                    _ => None,
                },
            )
            .ok_or_else(|| {
                vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
            })?;
        let Some(text) = found else {
            return Ok(vm.ctx.none());
        };
        let point = active_text_point(&text);
        let dict = vm.ctx.new_dict();
        dict.set_item("handle", vm.new_pyobj(handle), vm)?;
        dict.set_item(
            "point",
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(point.x),
                    vm.new_pyobj(point.y),
                    vm.new_pyobj(point.z),
                ])
                .into(),
            vm,
        )?;
        dict.set_item(
            "normal",
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(text.normal.x),
                    vm.new_pyobj(text.normal.y),
                    vm.new_pyobj(text.normal.z),
                ])
                .into(),
            vm,
        )?;
        dict.set_item("rotation", vm.new_pyobj(text.rotation), vm)?;
        dict.set_item(
            "direction",
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(text.rotation.cos()),
                    vm.new_pyobj(text.rotation.sin()),
                ])
                .into(),
            vm,
        )?;
        dict.set_item("height", vm.new_pyobj(text.height), vm)?;
        dict.set_item("value", vm.new_pyobj(text.value), vm)?;
        dict.set_item("style", vm.new_pyobj(text.style), vm)?;
        Ok(dict.into())
    }

    /// Approximate XY box of left/baseline TEXT, using height and character count.
    #[pyfunction]
    fn text_box_vertices(
        handle: u64,
        padding: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        let padding = padding.into_float();
        if !padding.is_finite() || padding < 0.0 {
            return Err(vm.new_value_error(
                "ocs.text_box_vertices: padding must be finite and nonnegative".to_owned(),
            ));
        }
        let found = host_ctx::with_host(|host| {
            let Some(EntityType::Text(text)) = host.document().get_entity(Handle::new(handle))
            else {
                return None;
            };
            if text.normal != Vector3::UNIT_Z
                || text.horizontal_alignment != TextHorizontalAlignment::Left
                || text.vertical_alignment != TextVerticalAlignment::Baseline
                || text.value.is_empty()
                || ![
                    text.height,
                    text.width_factor,
                    text.rotation,
                    text.insertion_point.x,
                    text.insertion_point.y,
                    text.insertion_point.z,
                ]
                .iter()
                .all(|value| value.is_finite())
                || text.height <= 0.0
                || text.width_factor <= 0.0
            {
                return None;
            }
            let width = text.value.chars().count() as f64 * text.height * 0.6 * text.width_factor;
            let local = [
                (-padding, -padding),
                (width + padding, -padding),
                (width + padding, text.height + padding),
                (-padding, text.height + padding),
            ];
            let (sin, cos) = text.rotation.sin_cos();
            let vertices = local.map(|(x, y)| {
                [
                    text.insertion_point.x + x * cos - y * sin,
                    text.insertion_point.y + x * sin + y * cos,
                ]
            });
            Some((vertices, text.insertion_point.z))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let Some((vertices, elevation)) = found else {
            return Ok(vm.ctx.none());
        };
        let dict = vm.ctx.new_dict();
        let points = vertices
            .into_iter()
            .map(|point| {
                vm.ctx
                    .new_list(vec![vm.new_pyobj(point[0]), vm.new_pyobj(point[1])])
                    .into()
            })
            .collect();
        dict.set_item("vertices", vm.ctx.new_list(points).into(), vm)?;
        dict.set_item("elevation", vm.new_pyobj(elevation), vm)?;
        Ok(dict.into())
    }

    /// Add a top-left anchored MTEXT entity in the world XY plane.
    #[pyfunction]
    fn add_mtext(
        value: rustpython_vm::builtins::PyStrRef,
        point: Vec<f64>,
        height: ArgIntoFloat,
        width: ArgIntoFloat,
        rotation: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let (height, width, rotation) = (
            height.into_float(),
            width.into_float(),
            rotation.into_float(),
        );
        if point.len() != 3
            || point.iter().any(|v| !v.is_finite())
            || ![height, width, rotation].iter().all(|v| v.is_finite())
            || height <= 0.0
            || width <= 0.0
        {
            return Err(vm.new_value_error(
                "ocs.add_mtext: coordinates must be finite; height and width must be positive"
                    .to_owned(),
            ));
        }
        let mut text = MText::with_value(
            value.to_string(),
            Vector3::new(point[0], point[1], point[2]),
        )
        .with_height(height)
        .with_width(width);
        text.rotation = rotation;
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::MText(text)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Build a kernel solid. `values` are the primitive's numbers in a fixed
    /// order (see `document_model.py`); the host validates and builds it.
    #[pyfunction]
    fn solid_create(
        primitive: String,
        values: Vec<f64>,
        layer: Option<String>,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        use ocs_plugin_api::host::{SolidOperation, SolidPrimitive};
        let expect = |count: usize| -> PyResult<()> {
            if values.len() == count {
                Ok(())
            } else {
                Err(vm.new_value_error(format!(
                    "ocs.solid_create: {primitive} takes {count} numbers, got {}",
                    values.len()
                )))
            }
        };
        let v3 = |i: usize| [values[i], values[i + 1], values[i + 2]];
        let primitive = match primitive.as_str() {
            "box" => { expect(6)?; SolidPrimitive::Box { center: v3(0), size: v3(3) } }
            "wedge" => { expect(6)?; SolidPrimitive::Wedge { origin: v3(0), size: v3(3) } }
            "cylinder" => { expect(5)?; SolidPrimitive::Cylinder { center: v3(0), radius: values[3], height: values[4] } }
            "sphere" => { expect(4)?; SolidPrimitive::Sphere { center: v3(0), radius: values[3] } }
            "torus" => { expect(5)?; SolidPrimitive::Torus { center: v3(0), major: values[3], minor: values[4] } }
            "pyramid" => {
                expect(6)?;
                if values[5] < 0.0 || values[5].fract() != 0.0 || values[5] > f64::from(u32::MAX) {
                    return Err(vm.new_value_error("ocs.solid_create: sides must be a whole number".to_owned()));
                }
                SolidPrimitive::Pyramid { center: v3(0), radius: values[3], height: values[4], sides: values[5] as u32 }
            }
            other => return Err(vm.new_value_error(format!("ocs.solid_create: unknown primitive {other:?}"))),
        };
        // The host records its own undo step once the operation validates, like
        // an entity transaction, so a refused operation leaves no empty step.
        let result = host_ctx::with_host(|host| {
            host.solid_operation(SolidOperation::Create { primitive, layer })
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.solid_create: {error}")))
    }

    /// Build a planar region from a closed planar profile entity.
    #[pyfunction]
    fn solid_region(source: u64, layer: Option<String>, delete_source: bool, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::SolidOperation;
        let result = host_ctx::with_host(|host| {
            host.solid_operation(SolidOperation::RegionFromProfile {
                source: Handle::new(source),
                layer,
                delete_source,
            })
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.solid_region: {error}")))
    }

    /// Embed a picture file as an OLE frame.
    #[pyfunction]
    fn embed_picture(
        path: String,
        origin: Vec<f64>,
        width: f64,
        layer: Option<String>,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        use ocs_plugin_api::host::SolidOperation;
        let origin: [f64; 3] = origin.try_into().map_err(|_| {
            vm.new_value_error("ocs.embed_picture: the origin needs 3 numbers".to_owned())
        })?;
        let result = host_ctx::with_host(|host| {
            host.solid_operation(SolidOperation::EmbedPicture { path, origin, width, layer })
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.embed_picture: {error}")))
    }

    /// Combine two solids ("union", "subtract" or "intersect").
    #[pyfunction]
    fn solid_boolean(
        first: u64,
        second: u64,
        operation: String,
        layer: Option<String>,
        keep_operands: bool,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        use ocs_plugin_api::host::{SolidBoolean, SolidOperation};
        let operation = match operation.as_str() {
            "union" => SolidBoolean::Union,
            "subtract" => SolidBoolean::Subtract,
            "intersect" => SolidBoolean::Intersect,
            other => {
                return Err(vm.new_value_error(format!("ocs.solid_boolean: unknown operation {other:?}")))
            }
        };
        let result = host_ctx::with_host(|host| {
            host.solid_operation(SolidOperation::Boolean {
                first: Handle::new(first),
                second: Handle::new(second),
                operation,
                layer,
                keep_operands,
            })
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.solid_boolean: {error}")))
    }

    /// Build a plane surface from a closed planar profile entity.
    #[pyfunction]
    fn solid_surface(source: u64, layer: Option<String>, delete_source: bool, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::SolidOperation;
        let result = host_ctx::with_host(|host| {
            host.solid_operation(SolidOperation::SurfaceFromProfile {
                source: Handle::new(source),
                layer,
                delete_source,
            })
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.solid_surface: {error}")))
    }

    /// Extrude a planar profile: a solid if closed, a surface if open.
    #[pyfunction]
    fn solid_extrude(
        source: u64,
        direction: Vec<f64>,
        layer: Option<String>,
        delete_source: bool,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        use ocs_plugin_api::host::SolidOperation;
        let direction: [f64; 3] = direction.try_into().map_err(|_| {
            vm.new_value_error("ocs.solid_extrude: the direction needs 3 numbers".to_owned())
        })?;
        let result = host_ctx::with_host(|host| {
            host.solid_operation(SolidOperation::Extrude {
                source: Handle::new(source),
                direction,
                layer,
                delete_source,
            })
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.solid_extrude: {error}")))
    }

    /// Apply a column-major 4x4 rigid transform to a solid, body or region.
    #[pyfunction]
    fn solid_transform(handle: u64, matrix: Vec<f64>, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::SolidOperation;
        let matrix: [f64; 16] = matrix.try_into().map_err(|_| {
            vm.new_value_error("ocs.solid_transform: the matrix needs 16 numbers".to_owned())
        })?;
        let result = host_ctx::with_host(|host| {
            host.solid_operation(SolidOperation::Transform { handle: Handle::new(handle), matrix })
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.solid_transform: {error}")))
    }

    /// Column-major matrix that turns about a world axis (0=X, 1=Y, 2=Z)
    /// through the point `about`. The sandbox has no `math` module.
    #[pyfunction]
    fn rotation_matrix(axis: usize, angle: f64, about: Vec<f64>, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        if axis > 2 || about.len() != 3 || !angle.is_finite() || about.iter().any(|v| !v.is_finite()) {
            return Err(vm.new_value_error("ocs.rotation_matrix: axis 0-2, a finite angle and a 3-point".to_owned()));
        }
        let (sin, cos) = angle.sin_cos();
        let (x, y, z) = match axis {
            0 => ([1.0, 0.0, 0.0], [0.0, cos, sin], [0.0, -sin, cos]),
            1 => ([cos, 0.0, -sin], [0.0, 1.0, 0.0], [sin, 0.0, cos]),
            _ => ([cos, sin, 0.0], [-sin, cos, 0.0], [0.0, 0.0, 1.0]),
        };
        Ok(matrix_to_py(vm, frame_matrix(x, y, z, &about)))
    }

    /// Column-major matrix that reflects across the plane normal to a world
    /// axis through the point `about`.
    #[pyfunction]
    fn mirror_matrix(axis: usize, about: Vec<f64>, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        if axis > 2 || about.len() != 3 || about.iter().any(|v| !v.is_finite()) {
            return Err(vm.new_value_error("ocs.mirror_matrix: axis 0-2 and a finite 3-point".to_owned()));
        }
        let mut columns = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        columns[axis][axis] = -1.0;
        let [x, y, z] = columns;
        Ok(matrix_to_py(vm, frame_matrix(x, y, z, &about)))
    }

    fn matrix_to_py(vm: &VirtualMachine, matrix: Vec<f64>) -> PyObjectRef {
        vm.ctx.new_list(matrix.into_iter().map(|value| vm.new_pyobj(value)).collect()).into()
    }

    /// A transform about `about` has origin `about - M * about`.
    fn frame_matrix(x: [f64; 3], y: [f64; 3], z: [f64; 3], about: &[f64]) -> Vec<f64> {
        let mut origin = [about[0], about[1], about[2]];
        for axis in 0..3 {
            origin[axis] -= x[axis] * about[0] + y[axis] * about[1] + z[axis] * about[2];
        }
        vec![
            x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0,
            origin[0], origin[1], origin[2], 1.0,
        ]
    }

    /// Delete one entity in the script's shared undo group.
    #[pyfunction]
    fn remove_entity(handle: u64, vm: &VirtualMachine) -> PyResult<()> {
        let removed = host_ctx::with_host(|host| {
            if host.document().get_entity(Handle::new(handle)).is_none() {
                return false;
            }
            host_ctx::ensure_undo_started(host);
            host.remove_entity(Handle::new(handle))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !removed {
            return Err(vm.new_runtime_error(format!(
                "ocs.remove_entity: entity {handle} was not deleted"
            )));
        }
        Ok(())
    }

    /// Read the XDATA record for `app_name` on entity `handle`, or `None` if
    /// the entity doesn't exist or carries no such record. Each value is a
    /// `{"kind": ..., "value": ...}` dict — see `write_record` for the
    /// supported kinds.
    ///
    /// The `{app_name, values}`/`{kind, value}` shape mirrors
    /// `schoeller/ocs_python_repl`'s `ocs.doc.read_record(...)` (its `_ocs`
    /// PyO3 extension) so a script's XDATA handling ports between the two
    /// plugins unchanged.
    #[pyfunction]
    fn read_record(handle: u64, app_name: String, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let record =
            host_ctx::with_host(|host| host.read_record(Handle::new(handle), &app_name).cloned())
                .ok_or_else(|| {
                vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
            })?;
        let Some(record) = record else {
            return Ok(vm.ctx.none());
        };
        xdata_record_to_py(vm, &record)
    }

    /// Attach an XDATA record to `handle` for `app_name`, replacing any
    /// existing record for the same application. `values` is a list of
    /// `{"kind": ..., "value": ...}` dicts; supported kinds: `String`,
    /// `ControlString`, `LayerName`, `BinaryData` (Python `bytes`), `Handle`
    /// (`int`), `Point3D`/`Position3D`/`Displacement3D`/`Direction3D`
    /// (`[x, y, z]`), `Real`/`Distance`/`ScaleFactor` (`float`), and
    /// `Integer16`/`Integer32` (`int`). Raises if `handle` does not exist.
    #[pyfunction]
    fn write_record(
        handle: u64,
        app_name: String,
        values: Vec<PyObjectRef>,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let mut record = ExtendedDataRecord::new(app_name);
        for value in values {
            let dict = value.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
            record.add_value(py_to_xdata_value(&dict, vm)?);
        }
        let written = host_ctx::with_host(|host| {
            if host.document().get_entity(Handle::new(handle)).is_none() {
                return false;
            }
            host_ctx::ensure_undo_started(host);
            host.write_record(Handle::new(handle), record)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !written {
            return Err(
                vm.new_runtime_error(format!("ocs.write_record: entity {handle} does not exist"))
            );
        }
        Ok(())
    }

    /// Remove the XDATA record for `app_name` from `handle`, if any. Returns
    /// `True` if a record was actually removed.
    #[pyfunction]
    fn remove_record(handle: u64, app_name: String, vm: &VirtualMachine) -> PyResult<bool> {
        host_ctx::with_host(|host| {
            if host.read_record(Handle::new(handle), &app_name).is_none() {
                return false;
            }
            host_ctx::ensure_undo_started(host);
            host.remove_record(Handle::new(handle), &app_name)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    fn xdata_record_to_py(
        vm: &VirtualMachine,
        record: &ExtendedDataRecord,
    ) -> PyResult<PyObjectRef> {
        let dict = vm.ctx.new_dict();
        dict.set_item(
            "app_name",
            vm.new_pyobj(record.application_name.clone()),
            vm,
        )?;
        let mut values = Vec::with_capacity(record.values.len());
        for value in &record.values {
            values.push(xdata_value_to_py(vm, value)?);
        }
        dict.set_item("values", vm.ctx.new_list(values).into(), vm)?;
        Ok(dict.into())
    }

    fn xdata_value_to_py(vm: &VirtualMachine, value: &XDataValue) -> PyResult<PyObjectRef> {
        let dict = vm.ctx.new_dict();
        let (kind, py_value): (&str, PyObjectRef) = match value {
            XDataValue::String(s) => ("String", vm.new_pyobj(s.clone())),
            XDataValue::ControlString(s) => ("ControlString", vm.new_pyobj(s.clone())),
            XDataValue::LayerName(s) => ("LayerName", vm.new_pyobj(s.clone())),
            XDataValue::BinaryData(b) => ("BinaryData", vm.ctx.new_bytes(b.clone()).into()),
            XDataValue::Handle(h) => ("Handle", vm.new_pyobj(h.value())),
            XDataValue::Point3D(v) => ("Point3D", vector3_to_py(vm, v)),
            XDataValue::Position3D(v) => ("Position3D", vector3_to_py(vm, v)),
            XDataValue::Displacement3D(v) => ("Displacement3D", vector3_to_py(vm, v)),
            XDataValue::Direction3D(v) => ("Direction3D", vector3_to_py(vm, v)),
            XDataValue::Real(r) => ("Real", vm.new_pyobj(*r)),
            XDataValue::Distance(d) => ("Distance", vm.new_pyobj(*d)),
            XDataValue::ScaleFactor(s) => ("ScaleFactor", vm.new_pyobj(*s)),
            XDataValue::Integer16(i) => ("Integer16", vm.new_pyobj(*i)),
            XDataValue::Integer32(i) => ("Integer32", vm.new_pyobj(*i)),
        };
        dict.set_item("kind", vm.new_pyobj(kind), vm)?;
        dict.set_item("value", py_value, vm)?;
        Ok(dict.into())
    }

    fn vector3_to_py(vm: &VirtualMachine, v: &Vector3) -> PyObjectRef {
        vm.ctx
            .new_list(vec![
                vm.new_pyobj(v.x),
                vm.new_pyobj(v.y),
                vm.new_pyobj(v.z),
            ])
            .into()
    }

    fn py_to_xdata_value(
        dict: &rustpython_vm::builtins::PyDictRef,
        vm: &VirtualMachine,
    ) -> PyResult<XDataValue> {
        let kind = dict.get_item("kind", vm)?.try_into_value::<String>(vm)?;
        let value = dict.get_item("value", vm)?;
        match kind.as_str() {
            "String" => Ok(XDataValue::String(value.try_into_value(vm)?)),
            "ControlString" => Ok(XDataValue::ControlString(value.try_into_value(vm)?)),
            "LayerName" => Ok(XDataValue::LayerName(value.try_into_value(vm)?)),
            "BinaryData" => Ok(XDataValue::BinaryData(value.try_into_value(vm)?)),
            "Handle" => Ok(XDataValue::Handle(Handle::new(value.try_into_value(vm)?))),
            "Point3D" => Ok(XDataValue::Point3D(py_to_vector3(value, vm)?)),
            "Position3D" => Ok(XDataValue::Position3D(py_to_vector3(value, vm)?)),
            "Displacement3D" => Ok(XDataValue::Displacement3D(py_to_vector3(value, vm)?)),
            "Direction3D" => Ok(XDataValue::Direction3D(py_to_vector3(value, vm)?)),
            "Real" => Ok(XDataValue::Real(value.try_into_value(vm)?)),
            "Distance" => Ok(XDataValue::Distance(value.try_into_value(vm)?)),
            "ScaleFactor" => Ok(XDataValue::ScaleFactor(value.try_into_value(vm)?)),
            "Integer16" => Ok(XDataValue::Integer16(value.try_into_value(vm)?)),
            "Integer32" => Ok(XDataValue::Integer32(value.try_into_value(vm)?)),
            other => Err(vm.new_value_error(format!(
                "unsupported XDATA value kind: {other}; supported: String, ControlString, \
                 LayerName, BinaryData, Handle, Point3D, Position3D, Displacement3D, \
                 Direction3D, Real, Distance, ScaleFactor, Integer16, Integer32"
            ))),
        }
    }

    fn py_to_vector3(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<Vector3> {
        let coords: Vec<f64> = value.try_into_value(vm)?;
        if coords.len() != 3 || coords.iter().any(|v| !v.is_finite()) {
            return Err(vm.new_value_error(
                "XDATA Point3D/Position3D/Displacement3D/Direction3D value must be \
                 [finite x, y, z]"
                    .to_owned(),
            ));
        }
        Ok(Vector3::new(coords[0], coords[1], coords[2]))
    }

    /// All top-level single- and multiline text in the current document.
    #[pyfunction]
    fn text_entities(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let items = host_ctx::with_host(|host| {
            host.document()
                .entities()
                .filter_map(|entity| match entity {
                    EntityType::Text(text) => {
                        Some((text.common.handle.value(), "TEXT", text.value.clone()))
                    }
                    EntityType::MText(text) => {
                        Some((text.common.handle.value(), "MTEXT", text.value.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(items.len());
        for (handle, kind, value) in items {
            let dict = vm.ctx.new_dict();
            dict.set_item("handle", vm.new_pyobj(handle), vm)?;
            dict.set_item("kind", vm.new_pyobj(kind), vm)?;
            dict.set_item("value", vm.new_pyobj(value), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// All top-level block references in the active document.
    #[pyfunction]
    fn block_references(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let items = host_ctx::with_host(|host| {
            let document = host.document();
            document
                .entities()
                .filter_map(|entity| match entity {
                    EntityType::Insert(insert) => {
                        let raw_name = insert.block_name.clone();
                        let record = document
                            .block_records
                            .iter()
                            .find(|block| block.name == raw_name);
                        let is_xref = record.is_some_and(|block| block.flags.is_xref);
                        let dynamic_definition = document
                            .dynamic_definition_for_insert(insert.common.handle)
                            .and_then(|handle| {
                                document
                                    .block_records
                                    .iter()
                                    .find(|block| block.handle == handle)
                            });
                        let effective_name = dynamic_definition
                            .map_or_else(|| raw_name.clone(), |block| block.name.clone());
                        let is_dynamic = effective_name != raw_name
                            || document
                                .dynamic_visibility_for_insert(insert.common.handle)
                                .is_some();
                        let visibility = document
                            .dynamic_visibility_for_insert(insert.common.handle)
                            .and_then(|(definition, parameter)| {
                                let def_record = document
                                    .block_records
                                    .iter()
                                    .find(|block| block.handle == definition)?;
                                let anon_record = record?;
                                let current: std::collections::HashSet<usize> = anon_record
                                    .entity_handles
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, handle)| {
                                        document
                                            .get_entity(**handle)
                                            .is_some_and(|entity| !entity.common().invisible)
                                    })
                                    .map(|(index, _)| index)
                                    .collect();
                                parameter
                                    .states
                                    .iter()
                                    .find(|state| {
                                        let visible: std::collections::HashSet<usize> = def_record
                                            .entity_handles
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, handle)| {
                                                state.visible_blocks.contains(handle)
                                            })
                                            .map(|(index, _)| index)
                                            .collect();
                                        visible == current
                                    })
                                    .map(|state| state.name.clone())
                            });
                        Some((
                            insert.common.handle.value(),
                            raw_name,
                            effective_name,
                            insert.row_count,
                            insert.column_count,
                            is_xref,
                            is_dynamic,
                            visibility,
                        ))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(items.len());
        for (handle, name, effective_name, rows, columns, is_xref, is_dynamic, visibility) in items
        {
            let dict = vm.ctx.new_dict();
            dict.set_item("handle", vm.new_pyobj(handle), vm)?;
            dict.set_item("name", vm.new_pyobj(name), vm)?;
            dict.set_item("effective_name", vm.new_pyobj(effective_name), vm)?;
            dict.set_item("is_xref", vm.new_pyobj(is_xref), vm)?;
            dict.set_item("is_dynamic", vm.new_pyobj(is_dynamic), vm)?;
            dict.set_item(
                "visibility",
                visibility.map_or_else(|| vm.ctx.none(), |state| vm.new_pyobj(state)),
                vm,
            )?;
            dict.set_item(
                "count",
                vm.new_pyobj(u64::from(rows) * u64::from(columns)),
                vm,
            )?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    #[cfg(feature = "experimental-host-model")]
    fn layer_options(
        name: String,
        options: &rustpython_vm::builtins::PyDictRef,
        vm: &VirtualMachine,
    ) -> PyResult<ocs_plugin_api::host::LayerConfig> {
        use codec::types::{Color, LineWeight, Transparency};
        ensure_known_entity_keys(
            options,
            "layer",
            &["color", "linetype", "lineweight", "off", "frozen", "locked", "plottable", "transparency", "description"],
            vm,
        )?;
        let present = |key: &str| -> PyResult<Option<PyObjectRef>> {
            Ok(options.get_item_opt(key, vm)?.filter(|v| !vm.is_none(v)))
        };
        let mut config = ocs_plugin_api::host::LayerConfig { name, ..Default::default() };
        if let Some(value) = present("color")? {
            config.color = Some(if let Ok(index) = value.clone().try_into_value::<i64>(vm) {
                let index = u8::try_from(index)
                    .ok()
                    .filter(|i| *i != 0)
                    .ok_or_else(|| vm.new_value_error("ocs: a layer color index must be 1..=255".to_owned()))?;
                Color::Index(index)
            } else if let Ok(rgb) = value.clone().try_into_value::<Vec<i64>>(vm) {
                let byte = |v: i64| {
                    u8::try_from(v).map_err(|_| vm.new_value_error("ocs: RGB components must be 0..=255".to_owned()))
                };
                match rgb.as_slice() {
                    [r, g, b] => Color::Rgb { r: byte(*r)?, g: byte(*g)?, b: byte(*b)? },
                    _ => return Err(vm.new_value_error("ocs: an RGB color needs three numbers".to_owned())),
                }
            } else {
                py_to_color_dict(value, vm)?
            });
        }
        if let Some(value) = present("linetype")? {
            config.linetype = Some(value.try_into_value::<String>(vm)?);
        }
        if let Some(value) = present("lineweight")? {
            let weight = value.try_into_value::<i64>(vm)?;
            let weight = i16::try_from(weight)
                .map_err(|_| vm.new_value_error("ocs: lineweight is out of range".to_owned()))?;
            config.lineweight = Some(LineWeight::from_value(weight));
        }
        for (key, slot) in [
            ("off", &mut config.off),
            ("frozen", &mut config.frozen),
            ("locked", &mut config.locked),
            ("plottable", &mut config.plottable),
        ] {
            if let Some(value) = present(key)? {
                *slot = Some(value.try_into_value::<bool>(vm)?);
            }
        }
        if let Some(value) = present("transparency")? {
            let percent = py_number_to_f64(value, vm)?;
            if !(0.0..=90.0).contains(&percent) {
                return Err(vm.new_value_error("ocs: layer transparency is a percentage from 0 to 90".to_owned()));
            }
            config.transparency = Some(if percent == 0.0 { Transparency::ByLayer } else { Transparency::from_percent(percent) });
        }
        if let Some(value) = present("description")? {
            config.description = Some(value.try_into_value::<String>(vm)?);
        }
        Ok(config)
    }

    /// Layer-table change through the host: `op` is `create`, `modify`,
    /// `rename` (`options={"to": ...}`), `delete` (`options={"erase_objects": bool}`)
    /// or `set_current`. Returns the layer's handle.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn layer_operation(op: String, name: String, options: PyObjectRef, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::TableOperation;
        let options = if vm.is_none(&options) {
            vm.ctx.new_dict()
        } else {
            options.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?
        };
        let operation = match op.as_str() {
            "create" => TableOperation::LayerCreate { config: layer_options(name, &options, vm)? },
            "modify" => TableOperation::LayerModify { config: layer_options(name, &options, vm)? },
            "rename" => {
                ensure_known_entity_keys(&options, "layer rename", &["to"], vm)?;
                let to = options
                    .get_item_opt("to", vm)?
                    .ok_or_else(|| vm.new_value_error("ocs: rename needs the new name".to_owned()))?
                    .try_into_value::<String>(vm)?;
                TableOperation::LayerRename { from: name, to }
            }
            "delete" => {
                ensure_known_entity_keys(&options, "layer delete", &["erase_objects"], vm)?;
                let erase_objects = get_opt_bool(&options, "erase_objects", vm)?;
                TableOperation::LayerDelete { name, erase_objects }
            }
            "set_current" => TableOperation::LayerSetCurrent { name },
            other => return Err(vm.new_value_error(format!("ocs.layer_operation: unknown operation {other:?}"))),
        };
        let result = host_ctx::with_host(|host| host.table_operation(operation))
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.layer_operation: {error}")))
    }

    /// Convert a Python value (None, bool, int, float, str, list/tuple, dict
    /// with string keys) to JSON.
    #[cfg(feature = "experimental-host-model")]
    fn py_to_json(value: &PyObjectRef, vm: &VirtualMachine) -> PyResult<serde_json::Value> {
        use rustpython_vm::builtins::{PyDict, PyFloat, PyInt, PyList, PyStr, PyTuple};
        use rustpython_vm::AsObject;
        if vm.is_none(value) {
            return Ok(serde_json::Value::Null);
        }
        if let Ok(flag) = value.clone().try_into_value::<bool>(vm) {
            if value.class().is(vm.ctx.types.bool_type) {
                return Ok(serde_json::Value::Bool(flag));
            }
        }
        if value.downcast_ref::<PyInt>().is_some() {
            return Ok(serde_json::Value::from(value.clone().try_into_value::<i64>(vm)?));
        }
        if value.downcast_ref::<PyFloat>().is_some() {
            let number = value.clone().try_into_value::<f64>(vm)?;
            return serde_json::Number::from_f64(number)
                .map(serde_json::Value::Number)
                .ok_or_else(|| vm.new_value_error("ocs: numbers must be finite".to_owned()));
        }
        if value.downcast_ref::<PyStr>().is_some() {
            return Ok(serde_json::Value::String(value.clone().try_into_value::<String>(vm)?));
        }
        if let Some(list) = value.downcast_ref::<PyList>() {
            let items: Vec<PyObjectRef> = list.borrow_vec().to_vec();
            return items.iter().map(|item| py_to_json(item, vm)).collect::<PyResult<Vec<_>>>().map(serde_json::Value::Array);
        }
        if let Some(tuple) = value.downcast_ref::<PyTuple>() {
            return tuple.iter().map(|item| py_to_json(item, vm)).collect::<PyResult<Vec<_>>>().map(serde_json::Value::Array);
        }
        if let Some(dict) = value.downcast_ref::<PyDict>() {
            let mut map = serde_json::Map::new();
            for key in dict.keys_vec() {
                let name = key.clone().try_into_value::<String>(vm)?;
                let item = dict.get_item(&*key, vm)?;
                map.insert(name, py_to_json(&item, vm)?);
            }
            return Ok(serde_json::Value::Object(map));
        }
        Err(vm.new_type_error("ocs: unsupported value in a style property".to_owned()))
    }

    #[cfg(feature = "experimental-host-model")]
    fn style_kind(kind: &str, vm: &VirtualMachine) -> PyResult<ocs_plugin_api::host::TableStyleKind> {
        match kind {
            "text" => Ok(ocs_plugin_api::host::TableStyleKind::Text),
            "dim" => Ok(ocs_plugin_api::host::TableStyleKind::Dim),
            other => Err(vm.new_value_error(format!("ocs.style_operation: unknown style kind {other:?}"))),
        }
    }

    /// Text/dimension style change through the host. `kind` is `text` or `dim`;
    /// `op` is `create`, `modify`, `rename` (`{"to": ...}`), `delete` or
    /// `set_current`. Text options: `height`, `width_factor`, `oblique`
    /// (degrees), `font`, `big_font`, `backward`,
    /// `upside_down`, `vertical`, `annotative`. Dimension options are DimStyle
    /// field names (plus `copy_from` on create). Returns the style's handle.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn style_operation(kind: String, op: String, name: String, options: PyObjectRef, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::{TableOperation, TextStyleConfig};
        let style_kind = style_kind(&kind, vm)?;
        let options = if vm.is_none(&options) {
            vm.ctx.new_dict()
        } else {
            options.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?
        };
        let operation = match (op.as_str(), kind.as_str()) {
            ("rename", _) => {
                ensure_known_entity_keys(&options, "style rename", &["to"], vm)?;
                let to = options
                    .get_item_opt("to", vm)?
                    .ok_or_else(|| vm.new_value_error("ocs: rename needs the new name".to_owned()))?
                    .try_into_value::<String>(vm)?;
                TableOperation::StyleRename { kind: style_kind, from: name, to }
            }
            ("delete", _) => {
                ensure_known_entity_keys(&options, "style delete", &[], vm)?;
                TableOperation::StyleDelete { kind: style_kind, name }
            }
            ("set_current", _) => TableOperation::StyleSetCurrent { kind: style_kind, name },
            ("create" | "modify", "text") => {
                ensure_known_entity_keys(
                    &options,
                    "text style",
                    &["height", "width_factor", "oblique", "font", "big_font", "backward", "upside_down", "vertical", "annotative"],
                    vm,
                )?;
                let present = |key: &str| -> PyResult<Option<PyObjectRef>> {
                    Ok(options.get_item_opt(key, vm)?.filter(|v| !vm.is_none(v)))
                };
                let number = |key: &str| -> PyResult<Option<f64>> {
                    present(key)?.map(|v| py_number_to_f64(v, vm)).transpose()
                };
                let text = |key: &str| -> PyResult<Option<String>> {
                    present(key)?.map(|v| v.try_into_value::<String>(vm)).transpose()
                };
                let flag = |key: &str| -> PyResult<Option<bool>> {
                    present(key)?.map(|v| v.try_into_value::<bool>(vm)).transpose()
                };
                let config = TextStyleConfig {
                    name,
                    height: number("height")?,
                    width_factor: number("width_factor")?,
                    oblique_angle: number("oblique")?.map(f64::to_radians),
                    font_file: text("font")?,
                    big_font_file: text("big_font")?,
                    backward: flag("backward")?,
                    upside_down: flag("upside_down")?,
                    vertical: flag("vertical")?,
                    annotative: flag("annotative")?,
                    ..Default::default()
                };
                if op == "create" {
                    TableOperation::TextStyleCreate { config }
                } else {
                    TableOperation::TextStyleModify { config }
                }
            }
            ("create" | "modify", "dim") => {
                let copy_from = if op == "create" {
                    options.get_item_opt("copy_from", vm)?.filter(|v| !vm.is_none(v))
                        .map(|v| v.try_into_value::<String>(vm)).transpose()?
                } else {
                    None
                };
                let properties = vm.ctx.new_dict();
                for key in options.keys_vec() {
                    if key.clone().try_into_value::<String>(vm)? != "copy_from" {
                        properties.set_item(&*key, options.get_item(&*key, vm)?, vm)?;
                    }
                }
                let properties = py_to_json(&PyObjectRef::from(properties), vm)?.to_string();
                if op == "create" {
                    TableOperation::DimStyleCreate { name, copy_from, properties }
                } else {
                    TableOperation::DimStyleModify { name, properties }
                }
            }
            (other, _) => return Err(vm.new_value_error(format!("ocs.style_operation: unknown operation {other:?}"))),
        };
        let result = host_ctx::with_host(|host| host.table_operation(operation))
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.style_operation: {error}")))
    }

    /// Every text style with its properties (`oblique` in degrees).
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn text_style_records(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let rows = host_ctx::with_host(|host| {
            let document = host.document();
            let current = document.header.current_text_style_name.to_uppercase();
            document
                .text_styles
                .iter()
                .map(|style| (style.clone(), style.name.to_uppercase() == current))
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(rows.len());
        for (style, is_current) in rows {
            let dict = vm.ctx.new_dict();
            dict.set_item("handle", vm.new_pyobj(style.handle.value()), vm)?;
            dict.set_item("name", vm.new_pyobj(style.name.clone()), vm)?;
            dict.set_item("height", vm.new_pyobj(style.height), vm)?;
            dict.set_item("width_factor", vm.new_pyobj(style.width_factor), vm)?;
            dict.set_item("oblique", vm.new_pyobj(style.oblique_angle.to_degrees()), vm)?;
            dict.set_item("font", vm.new_pyobj(style.font_file.clone()), vm)?;
            dict.set_item("big_font", vm.new_pyobj(style.big_font_file.clone()), vm)?;
            dict.set_item("backward", vm.new_pyobj(style.flags.backward), vm)?;
            dict.set_item("upside_down", vm.new_pyobj(style.flags.upside_down), vm)?;
            dict.set_item("vertical", vm.new_pyobj(style.is_vertical), vm)?;
            dict.set_item("annotative", vm.new_pyobj(style.annotative), vm)?;
            dict.set_item("current", vm.new_pyobj(is_current), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// Every dimension style with all of its DimStyle fields plus `current`.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn dim_style_records(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let rows = host_ctx::with_host(|host| {
            let document = host.document();
            let current = document.header.current_dimstyle_name.to_uppercase();
            document
                .dim_styles
                .iter()
                .map(|style| {
                    let mut value = serde_json::to_value(style).unwrap_or(serde_json::Value::Null);
                    if let Some(object) = value.as_object_mut() {
                        object.insert("current".to_owned(), (style.name.to_uppercase() == current).into());
                    }
                    value
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let output = rows
            .into_iter()
            .map(|value| snapshot_value_to_py(value, vm))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(vm.ctx.new_list(output).into())
    }

    /// Block-definition change through the host. `op` is `create`
    /// (`entities`, `base_point`, `erase_originals`, `description`), `modify`
    /// (`description`, `explodable`, `scale_uniformly`), `rename` (`to`) or
    /// `delete`. Returns the block record's handle.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn block_operation(op: String, name: String, options: PyObjectRef, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::TableOperation;
        let options = if vm.is_none(&options) {
            vm.ctx.new_dict()
        } else {
            options.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?
        };
        let present = |key: &str| -> PyResult<Option<PyObjectRef>> {
            Ok(options.get_item_opt(key, vm)?.filter(|v| !vm.is_none(v)))
        };
        let operation = match op.as_str() {
            "create" => {
                ensure_known_entity_keys(&options, "block", &["entities", "base_point", "erase_originals", "description"], vm)?;
                let handles = present("entities")?
                    .ok_or_else(|| vm.new_value_error("ocs: a block needs entities".to_owned()))?
                    .try_into_value::<Vec<u64>>(vm)?;
                let base_point = match present("base_point")? {
                    Some(value) => py_to_f64_array::<3>(value, vm)?,
                    None => [0.0; 3],
                };
                TableOperation::BlockCreate {
                    name,
                    entities: handles.into_iter().map(Handle::new).collect(),
                    base_point,
                    erase_originals: get_opt_bool(&options, "erase_originals", vm)?,
                    description: present("description")?.map(|v| v.try_into_value::<String>(vm)).transpose()?,
                }
            }
            "modify" => {
                ensure_known_entity_keys(&options, "block", &["description", "explodable", "scale_uniformly"], vm)?;
                TableOperation::BlockModify {
                    name,
                    description: present("description")?.map(|v| v.try_into_value::<String>(vm)).transpose()?,
                    explodable: present("explodable")?.map(|v| v.try_into_value::<bool>(vm)).transpose()?,
                    scale_uniformly: present("scale_uniformly")?.map(|v| v.try_into_value::<bool>(vm)).transpose()?,
                }
            }
            "rename" => {
                ensure_known_entity_keys(&options, "block rename", &["to"], vm)?;
                let to = options
                    .get_item_opt("to", vm)?
                    .ok_or_else(|| vm.new_value_error("ocs: rename needs the new name".to_owned()))?
                    .try_into_value::<String>(vm)?;
                TableOperation::BlockRename { from: name, to }
            }
            "delete" => {
                ensure_known_entity_keys(&options, "block delete", &[], vm)?;
                TableOperation::BlockDelete { name }
            }
            other => return Err(vm.new_value_error(format!("ocs.block_operation: unknown operation {other:?}"))),
        };
        let result = host_ctx::with_host(|host| host.table_operation(operation))
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.block_operation: {error}")))
    }

    /// Every user block definition (layouts and anonymous blocks are omitted).
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn block_records(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let rows = host_ctx::with_host(|host| {
            let document = host.document();
            let mut inserts = std::collections::HashMap::<String, usize>::new();
            for entity in document.entities() {
                if let codec::entities::EntityType::Insert(insert) = entity {
                    *inserts.entry(insert.block_name.to_uppercase()).or_default() += 1;
                }
            }
            document
                .block_records
                .iter()
                .filter(|record| !record.is_layout() && !record.is_anonymous() && !record.flags.is_xref)
                .map(|record| {
                    let members: Vec<(u64, String)> = record
                        .entity_handles
                        .iter()
                        .filter_map(|handle| document.get_entity(*handle))
                        .map(|entity| {
                            let kind = ocs_plugin_api::entity_coverage::entity_snapshot(entity)
                                .ok()
                                .and_then(|value| value.as_object().and_then(|o| o.keys().next().cloned()))
                                .unwrap_or_else(|| "Unknown".to_owned());
                            (entity.common().handle.value(), kind)
                        })
                        .collect();
                    (
                        record.clone(),
                        members,
                        inserts.get(&record.name.to_uppercase()).copied().unwrap_or(0),
                    )
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(rows.len());
        for (record, members, insert_count) in rows {
            let dict = vm.ctx.new_dict();
            dict.set_item("handle", vm.new_pyobj(record.handle.value()), vm)?;
            dict.set_item("name", vm.new_pyobj(record.name.clone()), vm)?;
            dict.set_item("description", vm.new_pyobj(record.description.clone()), vm)?;
            dict.set_item("explodable", vm.new_pyobj(record.explodable), vm)?;
            dict.set_item("scale_uniformly", vm.new_pyobj(record.scale_uniformly), vm)?;
            let base = [record.base_point.x, record.base_point.y, record.base_point.z];
            dict.set_item("base_point", PyObjectRef::from(vm.ctx.new_list(base.iter().map(|v| vm.new_pyobj(*v)).collect())), vm)?;
            dict.set_item("insert_count", vm.new_pyobj(insert_count), vm)?;
            let entities = members
                .into_iter()
                .map(|(handle, kind)| {
                    let member = vm.ctx.new_dict();
                    member.set_item("handle", vm.new_pyobj(handle), vm)?;
                    member.set_item("kind", vm.new_pyobj(kind), vm)?;
                    Ok(PyObjectRef::from(member))
                })
                .collect::<PyResult<Vec<_>>>()?;
            dict.set_item("entities", PyObjectRef::from(vm.ctx.new_list(entities)), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// Linetype change through the host. `op` is `create` or `modify`
    /// (`description`, `pattern`: signed lengths, positive dash, negative gap,
    /// zero dot), `rename` (`to`) or `delete`. Returns the linetype's handle.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn linetype_operation(op: String, name: String, options: PyObjectRef, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::TableOperation;
        let options = if vm.is_none(&options) {
            vm.ctx.new_dict()
        } else {
            options.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?
        };
        let present = |key: &str| -> PyResult<Option<PyObjectRef>> {
            Ok(options.get_item_opt(key, vm)?.filter(|v| !vm.is_none(v)))
        };
        let pattern = |value: PyObjectRef| -> PyResult<Vec<f64>> {
            let items: Vec<PyObjectRef> = value.try_into_value(vm)?;
            items.into_iter().map(|item| py_number_to_f64(item, vm)).collect()
        };
        let operation = match op.as_str() {
            "create" => {
                ensure_known_entity_keys(&options, "linetype", &["description", "pattern"], vm)?;
                TableOperation::LinetypeCreate {
                    name,
                    description: get_opt_string(&options, "description", "", vm)?,
                    pattern: pattern(present("pattern")?.ok_or_else(|| vm.new_value_error("ocs: a linetype needs a pattern".to_owned()))?)?,
                }
            }
            "modify" => {
                ensure_known_entity_keys(&options, "linetype", &["description", "pattern"], vm)?;
                TableOperation::LinetypeModify {
                    name,
                    description: present("description")?.map(|v| v.try_into_value::<String>(vm)).transpose()?,
                    pattern: present("pattern")?.map(pattern).transpose()?,
                }
            }
            "rename" => {
                ensure_known_entity_keys(&options, "linetype rename", &["to"], vm)?;
                let to = options
                    .get_item_opt("to", vm)?
                    .ok_or_else(|| vm.new_value_error("ocs: rename needs the new name".to_owned()))?
                    .try_into_value::<String>(vm)?;
                TableOperation::LinetypeRename { from: name, to }
            }
            "delete" => {
                ensure_known_entity_keys(&options, "linetype delete", &[], vm)?;
                TableOperation::LinetypeDelete { name }
            }
            other => return Err(vm.new_value_error(format!("ocs.linetype_operation: unknown operation {other:?}"))),
        };
        let result = host_ctx::with_host(|host| host.table_operation(operation))
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.linetype_operation: {error}")))
    }

    /// Every linetype: pattern as signed lengths plus how many layers and
    /// entities use it.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn linetype_records(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let rows = host_ctx::with_host(|host| {
            let document = host.document();
            let mut uses = std::collections::HashMap::<String, usize>::new();
            for layer in document.layers.iter() {
                *uses.entry(layer.line_type.to_uppercase()).or_default() += 1;
            }
            for entity in document.entities() {
                *uses.entry(entity.common().linetype.to_uppercase()).or_default() += 1;
            }
            document
                .line_types
                .iter()
                .map(|lt| (lt.clone(), uses.get(&lt.name.to_uppercase()).copied().unwrap_or(0)))
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(rows.len());
        for (lt, used) in rows {
            let dict = vm.ctx.new_dict();
            dict.set_item("handle", vm.new_pyobj(lt.handle.value()), vm)?;
            dict.set_item("name", vm.new_pyobj(lt.name.clone()), vm)?;
            dict.set_item("description", vm.new_pyobj(lt.description.clone()), vm)?;
            let pattern: Vec<PyObjectRef> = lt.elements.iter().map(|e| vm.new_pyobj(e.length)).collect();
            dict.set_item("pattern", PyObjectRef::from(vm.ctx.new_list(pattern)), vm)?;
            dict.set_item("pattern_length", vm.new_pyobj(lt.pattern_length), vm)?;
            dict.set_item("complex", vm.new_pyobj(lt.elements.iter().any(|e| e.complex.is_some())), vm)?;
            dict.set_item("used_by", vm.new_pyobj(used), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// Layout change through the host. `op` is `create`, `rename` (`to`),
    /// `delete`, `set_current` or `set_page` (`paper_size`: (width, height) mm,
    /// `rotation`: 0/90/180/270, `scale`: (numerator, denominator)).
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn layout_operation(op: String, name: String, options: PyObjectRef, vm: &VirtualMachine) -> PyResult<u64> {
        use ocs_plugin_api::host::TableOperation;
        let options = if vm.is_none(&options) {
            vm.ctx.new_dict()
        } else {
            options.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?
        };
        let present = |key: &str| -> PyResult<Option<PyObjectRef>> {
            Ok(options.get_item_opt(key, vm)?.filter(|v| !vm.is_none(v)))
        };
        let operation = match op.as_str() {
            "create" => TableOperation::LayoutCreate { name },
            "delete" => TableOperation::LayoutDelete { name },
            "set_current" => TableOperation::LayoutSetCurrent { name },
            "rename" => {
                ensure_known_entity_keys(&options, "layout rename", &["to"], vm)?;
                let to = options
                    .get_item_opt("to", vm)?
                    .ok_or_else(|| vm.new_value_error("ocs: rename needs the new name".to_owned()))?
                    .try_into_value::<String>(vm)?;
                TableOperation::LayoutRename { from: name, to }
            }
            "set_page" => {
                ensure_known_entity_keys(&options, "layout page", &["paper_size", "rotation", "scale"], vm)?;
                TableOperation::LayoutSetPage {
                    name,
                    paper_size: present("paper_size")?.map(|v| py_to_f64_array::<2>(v, vm)).transpose()?,
                    rotation: present("rotation")?
                        .map(|v| -> PyResult<u16> {
                            let degrees = v.try_into_value::<i64>(vm)?;
                            u16::try_from(degrees).map_err(|_| vm.new_value_error("ocs: the rotation must be 0, 90, 180 or 270".to_owned()))
                        })
                        .transpose()?,
                    scale: present("scale")?.map(|v| py_to_f64_array::<2>(v, vm)).transpose()?,
                }
            }
            other => return Err(vm.new_value_error(format!("ocs.layout_operation: unknown operation {other:?}"))),
        };
        let result = host_ctx::with_host(|host| host.table_operation(operation))
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.layout_operation: {error}")))
    }

    /// `Model` and every paper-space layout in tab order with its page setup.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn layout_records(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        use codec::objects::ObjectType;
        let rows = host_ctx::with_host(|host| {
            let current = match host.system_variable("CTAB") {
                Some(ocs_plugin_api::host::HostSettingValue::Text(name)) => name,
                _ => "Model".to_owned(),
            };
            let document = host.document();
            let mut owned = std::collections::HashMap::<u64, usize>::new();
            let mut viewports = std::collections::HashMap::<u64, usize>::new();
            for entity in document.entities() {
                let owner = entity.common().owner_handle.value();
                *owned.entry(owner).or_default() += 1;
                if matches!(entity, codec::entities::EntityType::Viewport(_)) {
                    *viewports.entry(owner).or_default() += 1;
                }
            }
            let mut layouts: Vec<_> = document
                .objects
                .values()
                .filter_map(|object| match object {
                    ObjectType::Layout(layout) if !layout.block_record.is_null() => Some(layout.clone()),
                    _ => None,
                })
                .collect();
            layouts.sort_by_key(|layout| (layout.name != "Model", layout.tab_order));
            layouts
                .into_iter()
                .map(|layout| {
                    let count = owned.get(&layout.block_record.value()).copied().unwrap_or(0);
                    let vps = viewports.get(&layout.block_record.value()).copied().unwrap_or(0);
                    let is_current = layout.name == current;
                    (layout, count, vps, is_current)
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(rows.len());
        for (layout, count, vps, is_current) in rows {
            let dict = vm.ctx.new_dict();
            dict.set_item("handle", vm.new_pyobj(layout.handle.value()), vm)?;
            dict.set_item("name", vm.new_pyobj(layout.name.clone()), vm)?;
            dict.set_item("current", vm.new_pyobj(is_current), vm)?;
            dict.set_item("tab_order", vm.new_pyobj(i64::from(layout.tab_order)), vm)?;
            dict.set_item("entity_count", vm.new_pyobj(count), vm)?;
            dict.set_item("viewport_count", vm.new_pyobj(vps), vm)?;
            dict.set_item("paper_size", PyObjectRef::from(vm.ctx.new_list(vec![vm.new_pyobj(layout.paper_width), vm.new_pyobj(layout.paper_height)])), vm)?;
            dict.set_item("paper_name", vm.new_pyobj(layout.paper_size.clone()), vm)?;
            let rotation = match layout.plot_rotation {
                1 => 90,
                2 => 180,
                3 => 270,
                _ => 0,
            };
            dict.set_item("rotation", vm.new_pyobj(rotation), vm)?;
            dict.set_item("scale", PyObjectRef::from(vm.ctx.new_list(vec![vm.new_pyobj(layout.plot_scale_numerator), vm.new_pyobj(layout.plot_scale_denominator)])), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// One step of driving a real OCS command. `kind` is `run` (`line`),
    /// `start` (`name`), `point` (`point`), `text` (`text`), `token` (`text`),
    /// `entity` (`handle`, `point`), `selection`, `enter` or `cancel`. Returns
    /// where the command stands: `status`, `blocked_by`, `command`, `prompt`,
    /// `accepts`, `options`, `entities`, `added`, `unconsumed`, `error`.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn command_step(kind: String, options: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        use ocs_plugin_api::host::CommandRequest as R;
        let options = if vm.is_none(&options) {
            vm.ctx.new_dict()
        } else {
            options.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?
        };
        let required = |key: &str| -> PyResult<PyObjectRef> {
            options
                .get_item_opt(key, vm)?
                .ok_or_else(|| vm.new_value_error(format!("ocs.command_step: {kind} needs {key}")))
        };
        let request = match kind.as_str() {
            "run" => R::Run { line: required("line")?.try_into_value::<String>(vm)? },
            "start" => R::Start { name: required("name")?.try_into_value::<String>(vm)? },
            "point" => R::Point { point: py_to_f64_array::<3>(required("point")?, vm)? },
            "text" => R::Text { text: required("text")?.try_into_value::<String>(vm)? },
            "token" => R::Token { text: required("text")?.try_into_value::<String>(vm)? },
            "entity" => R::Entity {
                handle: Handle::new(required("handle")?.try_into_value::<u64>(vm)?),
                point: py_to_f64_array::<3>(required("point")?, vm)?,
            },
            "selection" => R::Selection,
            "enter" => R::Enter,
            "cancel" => R::Cancel,
            other => return Err(vm.new_value_error(format!("ocs.command_step: unknown step {other:?}"))),
        };
        let outcome = host_ctx::with_host(|host| host.run_command(request))
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
            .map_err(|error| vm.new_runtime_error(format!("ocs.command_step: {error}")))?;
        let dict = vm.ctx.new_dict();
        let strings = |items: &[String]| -> PyObjectRef {
            PyObjectRef::from(vm.ctx.new_list(items.iter().map(|v| vm.new_pyobj(v.clone())).collect()))
        };
        dict.set_item("status", vm.new_pyobj(outcome.status.clone()), vm)?;
        dict.set_item("blocked_by", outcome.blocked_by.clone().map_or_else(|| vm.ctx.none(), |v| vm.new_pyobj(v)), vm)?;
        dict.set_item("command", vm.new_pyobj(outcome.command.clone()), vm)?;
        dict.set_item("prompt", vm.new_pyobj(outcome.prompt.clone()), vm)?;
        dict.set_item("accepts", strings(&outcome.accepts), vm)?;
        dict.set_item("options", strings(&outcome.options), vm)?;
        dict.set_item("entities", vm.new_pyobj(outcome.entities), vm)?;
        dict.set_item("added", vm.new_pyobj(outcome.added), vm)?;
        dict.set_item("unconsumed", strings(&outcome.unconsumed), vm)?;
        dict.set_item("error", outcome.error.clone().map_or_else(|| vm.ctx.none(), |v| vm.new_pyobj(v)), vm)?;
        Ok(dict.into())
    }

    /// Every layer with its full properties, in table order.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn layer_records(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        use codec::types::{LineWeight, Transparency};
        let rows = host_ctx::with_host(|host| {
            let document = host.document();
            let mut counts = std::collections::HashMap::<String, usize>::new();
            for entity in document.entities() {
                *counts.entry(entity.common().layer.to_uppercase()).or_default() += 1;
            }
            let current = document.header.current_layer_name.to_uppercase();
            document
                .layers
                .iter()
                .map(|layer| {
                    (
                        layer.clone(),
                        counts.get(&layer.name.to_uppercase()).copied().unwrap_or(0),
                        layer.name.to_uppercase() == current,
                    )
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(rows.len());
        for (layer, count, is_current) in rows {
            let dict = vm.ctx.new_dict();
            dict.set_item("handle", vm.new_pyobj(layer.handle.value()), vm)?;
            dict.set_item("name", vm.new_pyobj(layer.name.clone()), vm)?;
            dict.set_item("color", color_to_py_dict(vm, &layer.color)?, vm)?;
            dict.set_item("linetype", vm.new_pyobj(layer.line_type.clone()), vm)?;
            let weight = match layer.line_weight {
                LineWeight::ByLayer => -1,
                LineWeight::ByBlock => -2,
                LineWeight::Default => -3,
                LineWeight::Value(v) => i64::from(v),
            };
            dict.set_item("lineweight", vm.new_pyobj(weight), vm)?;
            dict.set_item("off", vm.new_pyobj(layer.flags.off), vm)?;
            dict.set_item("frozen", vm.new_pyobj(layer.flags.frozen), vm)?;
            dict.set_item("locked", vm.new_pyobj(layer.flags.locked), vm)?;
            dict.set_item("plottable", vm.new_pyobj(layer.is_plottable), vm)?;
            let transparency = match layer.transparency {
                Transparency::Explicit(_) => layer.transparency.as_percent(),
                _ => 0.0,
            };
            dict.set_item("transparency", vm.new_pyobj(transparency), vm)?;
            dict.set_item("description", vm.new_pyobj(layer.description.clone()), vm)?;
            dict.set_item("entity_count", vm.new_pyobj(count), vm)?;
            dict.set_item("current", vm.new_pyobj(is_current), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// Layer names, visibility flags, and top-level entity counts.
    #[pyfunction]
    fn layers(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let rows = host_ctx::with_host(|host| {
            let document = host.document();
            let layout_blocks = top_level_blocks(document);
            let mut counts = std::collections::HashMap::<String, usize>::new();
            for entity in document.entities() {
                if layout_blocks.contains(&entity.common().owner_handle) {
                    *counts.entry(entity.common().layer.clone()).or_default() += 1;
                }
            }
            document
                .layers
                .iter()
                .map(|layer| {
                    (
                        layer.name.clone(),
                        layer.flags.off,
                        layer.flags.frozen,
                        layer.flags.locked,
                        counts.get(&layer.name).copied().unwrap_or(0),
                    )
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let mut output = Vec::with_capacity(rows.len());
        for (name, off, frozen, locked, count) in rows {
            let dict = vm.ctx.new_dict();
            dict.set_item("name", vm.new_pyobj(name), vm)?;
            dict.set_item("off", vm.new_pyobj(off), vm)?;
            dict.set_item("frozen", vm.new_pyobj(frozen), vm)?;
            dict.set_item("locked", vm.new_pyobj(locked), vm)?;
            dict.set_item("entity_count", vm.new_pyobj(count), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// Move a top-level entity to an existing layer without changing its owner.
    #[pyfunction]
    fn set_entity_layer(handle: u64, layer_name: String, vm: &VirtualMachine) -> PyResult<()> {
        let changed = host_ctx::with_host(|host| {
            let document = host.document();
            if document.layers.get(&layer_name).is_none() {
                return false;
            }
            let Some(mut entity) = document.get_entity(Handle::new(handle)).cloned() else {
                return false;
            };
            if !top_level_blocks(document).contains(&entity.common().owner_handle) {
                return false;
            }
            if entity.common().layer == layer_name {
                return true;
            }
            entity.common_mut().layer = layer_name;
            host_ctx::ensure_undo_started(host);
            host.update_entity(entity)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_value_error(format!(
                "ocs.set_entity_layer: top-level entity {handle} or target layer not found"
            )));
        }
        Ok(())
    }

    /// Export one layer's top-level entities as a new DWG, never overwriting.
    #[pyfunction]
    fn export_layer_dwg(
        layer_name: String,
        output_path: String,
        vm: &VirtualMachine,
    ) -> PyResult<usize> {
        let path = std::path::Path::new(&output_path);
        if path
            .extension()
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("dwg"))
            || path.file_name().is_none()
            || path.parent().is_none_or(|parent| !parent.is_dir())
        {
            return Err(vm.new_value_error(
                "ocs.export_layer_dwg: output must be a .dwg file in an existing directory"
                    .to_owned(),
            ));
        }
        let prepared = host_ctx::with_host(|host| {
            let source = host.document();
            source.layers.get(&layer_name)?;
            let layout_blocks = top_level_blocks(source);
            let mut snapshot = source.clone();
            let removed: Vec<Handle> = source
                .entities()
                .filter(|entity| {
                    layout_blocks.contains(&entity.common().owner_handle)
                        && entity.common().layer != layer_name
                })
                .map(|entity| entity.common().handle)
                .collect();
            for handle in removed {
                snapshot.remove_entity(handle);
            }
            let kept: std::collections::HashSet<Handle> = snapshot
                .entities()
                .map(|entity| entity.common().handle)
                .collect();
            for record in snapshot.block_records.iter_mut() {
                record.entity_handles.retain(|handle| kept.contains(handle));
            }
            let count = snapshot
                .entities()
                .filter(|entity| {
                    layout_blocks.contains(&entity.common().owner_handle)
                        && entity.common().layer == layer_name
                })
                .count();
            Some((snapshot, count))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_value_error(format!(
                "ocs.export_layer_dwg: layer {layer_name:?} does not exist"
            ))
        })?;
        let (snapshot, count) = prepared;
        if count == 0 {
            return Err(vm.new_value_error(format!(
                "ocs.export_layer_dwg: layer {layer_name:?} has no top-level entities"
            )));
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                vm.new_runtime_error(format!(
                    "ocs.export_layer_dwg: cannot create {output_path:?}: {error}"
                ))
            })?;
        if let Err(error) = codec::DwgWriter::write_to_writer(&mut file, &snapshot) {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(
                vm.new_runtime_error(format!("ocs.export_layer_dwg: DWG write failed: {error}"))
            );
        }
        Ok(count)
    }

    /// Export a planned set of layers as new DWGs. If any export fails,
    /// remove only files created by this call, leaving pre-existing files
    /// untouched. No source drawing changes are made.
    #[pyfunction]
    fn export_layer_dwgs(exports: Vec<Vec<String>>, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let mut paths = std::collections::HashSet::new();
        if exports
            .iter()
            .any(|row| row.len() != 2 || !paths.insert(row[1].clone()))
        {
            return Err(vm.new_value_error(
                "ocs.export_layer_dwgs: expected unique [layer, path] pairs".to_owned(),
            ));
        }
        let mut created = Vec::with_capacity(exports.len());
        let mut counts = Vec::with_capacity(exports.len());
        for row in exports {
            match export_layer_dwg(row[0].clone(), row[1].clone(), vm) {
                Ok(count) => {
                    created.push(row[1].clone());
                    counts.push(count);
                }
                Err(error) => {
                    let failed_cleanup: Vec<String> = created
                        .into_iter()
                        .filter(|path| std::fs::remove_file(path).is_err())
                        .collect();
                    if !failed_cleanup.is_empty() {
                        return Err(vm.new_runtime_error(format!(
                            "ocs.export_layer_dwgs: export failed ({error:?}); cleanup failed for {failed_cleanup:?}"
                        )));
                    }
                    return Err(error);
                }
            }
        }
        Ok(vm
            .ctx
            .new_list(
                counts
                    .into_iter()
                    .map(|count| vm.new_pyobj(count))
                    .collect(),
            )
            .into())
    }

    /// Name of the active document's current layer (CLAYER).
    #[pyfunction]
    fn current_layer(vm: &VirtualMachine) -> PyResult<String> {
        host_ctx::with_host(|host| host.document().header.current_layer_name.clone())
            .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Read a host-managed system variable without invoking a CAD command.
    #[cfg(feature = "experimental-host-settings")]
    #[pyfunction]
    fn system_variable(name: String, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let value = host_ctx::with_host(|host| host.system_variable(&name)).ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })?;
        Ok(match value {
            Some(HostSettingValue::Text(text)) => vm.new_pyobj(text),
            Some(HostSettingValue::Number(number)) => vm.new_pyobj(number),
            None => vm.ctx.none(),
        })
    }

    /// Set a host-managed system variable without nested command dispatch.
    #[cfg(feature = "experimental-host-settings")]
    #[pyfunction]
    fn set_system_variable(
        name: String,
        value: PyObjectRef,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        let setting = match value.clone().try_into_value::<String>(vm) {
            Ok(text) => HostSettingValue::Text(text),
            Err(_) => {
                HostSettingValue::Number(ArgIntoFloat::try_from_object(vm, value)?.into_float())
            }
        };
        let changed = host_ctx::with_host(|host| host.set_system_variable(&name, setting))
            .ok_or_else(|| {
                vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
            })?
            .map_err(|error| vm.new_value_error(error))?;
        Ok(match changed {
            HostSettingValue::Text(text) => vm.new_pyobj(text),
            HostSettingValue::Number(number) => vm.new_pyobj(number),
        })
    }

    /// Owning layout of a top-level entity, including sheet order/count.
    #[pyfunction]
    fn layout_for_entity(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found = host_ctx::with_host(|host| {
            let document = host.document();
            let entity = document.get_entity(Handle::new(handle))?;
            let block = document
                .block_records
                .iter()
                .find(|record| record.handle == entity.common().owner_handle)?;
            let ObjectType::Layout(layout) = document.objects.get(&block.layout)? else {
                return None;
            };
            let sheet_count = document
                .objects
                .values()
                .filter(|object| matches!(object, ObjectType::Layout(item) if item.name != "Model"))
                .count();
            Some((layout.name.clone(), layout.tab_order, sheet_count))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let Some((name, order, sheet_count)) = found else {
            return Ok(vm.ctx.none());
        };
        let dict = vm.ctx.new_dict();
        dict.set_item("name", vm.new_pyobj(name), vm)?;
        dict.set_item("order", vm.new_pyobj(order), vm)?;
        dict.set_item("sheet_count", vm.new_pyobj(sheet_count), vm)?;
        Ok(dict.into())
    }

    /// Tag/value pairs attached to one INSERT, or None for other entities.
    #[pyfunction]
    fn block_attributes(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found =
            host_ctx::with_host(
                |host| match host.document().get_entity(Handle::new(handle)) {
                    Some(EntityType::Insert(insert)) => Some(
                        insert
                            .attributes
                            .iter()
                            .map(|attribute| (attribute.tag.clone(), attribute.value.clone()))
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                },
            )
            .ok_or_else(|| {
                vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
            })?;
        let Some(attributes) = found else {
            return Ok(vm.ctx.none());
        };
        let mut output = Vec::with_capacity(attributes.len());
        for (tag, value) in attributes {
            let dict = vm.ctx.new_dict();
            dict.set_item("tag", vm.new_pyobj(tag), vm)?;
            dict.set_item("value", vm.new_pyobj(value), vm)?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// Replace one INSERT attribute's value by tag, preserving other data.
    #[pyfunction]
    fn set_block_attribute(
        handle: u64,
        tag: rustpython_vm::builtins::PyStrRef,
        value: rustpython_vm::builtins::PyStrRef,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let tag = tag.to_string();
        let value = value.to_string();
        let changed = host_ctx::with_host(|host| {
            let Some(EntityType::Insert(mut insert)) =
                host.document().get_entity(Handle::new(handle)).cloned()
            else {
                return false;
            };
            let Some(attribute) = insert
                .attributes
                .iter_mut()
                .find(|attribute| attribute.tag.eq_ignore_ascii_case(&tag))
            else {
                return false;
            };
            attribute.value = value;
            host_ctx::ensure_undo_started(host);
            host.update_entity(EntityType::Insert(insert))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_runtime_error(format!(
                "ocs.set_block_attribute: INSERT {handle} or tag {tag} was not updated"
            )));
        }
        Ok(())
    }

    /// Replace the content of one top-level TEXT or MTEXT entity in place.
    #[pyfunction]
    fn set_text_value(
        handle: u64,
        value: rustpython_vm::builtins::PyStrRef,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let value = value.to_string();
        let changed = host_ctx::with_host(|host| {
            let Some(entity) = host.document().get_entity(Handle::new(handle)).cloned() else {
                return false;
            };
            let updated = match entity {
                EntityType::Text(mut text) => {
                    text.value = value;
                    EntityType::Text(text)
                }
                EntityType::MText(mut text) => {
                    text.value = value;
                    EntityType::MText(text)
                }
                _ => return false,
            };
            host_ctx::ensure_undo_started(host);
            host.update_entity(updated)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_runtime_error(format!(
                "ocs.set_text_value: TEXT/MTEXT entity {handle} was not updated"
            )));
        }
        Ok(())
    }

    /// Set an MTEXT background mode (off, mask, or fill) and border scale.
    #[pyfunction]
    fn set_mtext_mask(
        handle: u64,
        mode: rustpython_vm::builtins::PyStrRef,
        scale: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let scale = scale.into_float();
        let mode = mode.to_string();
        if !scale.is_finite() || scale < 1.0 || !["off", "mask", "fill"].contains(&mode.as_str()) {
            return Err(vm.new_value_error(
                "ocs.set_mtext_mask: mode must be off/mask/fill and scale at least 1".to_owned(),
            ));
        }
        let changed = host_ctx::with_host(|host| {
            let Some(EntityType::MText(mut text)) =
                host.document().get_entity(Handle::new(handle)).cloned()
            else {
                return false;
            };
            text.background_fill_flags &= !0x03;
            text.background_fill_flags |= match mode.as_str() {
                "mask" => 0x02,
                "fill" => 0x01,
                _ => 0,
            };
            text.background_scale = scale;
            host_ctx::ensure_undo_started(host);
            host.update_entity(EntityType::MText(text))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_runtime_error(format!(
                "ocs.set_mtext_mask: MTEXT entity {handle} was not updated"
            )));
        }
        Ok(())
    }

    /// Change only the active insertion/alignment point of a TEXT entity,
    /// preserving its style, rotation, content and other properties.
    #[pyfunction]
    fn move_text(
        handle: u64,
        x: ArgIntoFloat,
        y: ArgIntoFloat,
        z: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let (x, y, z) = (x.into_float(), y.into_float(), z.into_float());
        if ![x, y, z].iter().all(|value| value.is_finite()) {
            return Err(vm.new_value_error("ocs.move_text: coordinates must be finite".to_owned()));
        }
        let changed = host_ctx::with_host(|host| {
            let Some(EntityType::Text(mut text)) =
                host.document().get_entity(Handle::new(handle)).cloned()
            else {
                return false;
            };
            let point = Vector3::new(x, y, z);
            if uses_text_alignment_point(&text) {
                text.alignment_point = Some(point);
            } else {
                text.insertion_point = point;
            }
            host_ctx::ensure_undo_started(host);
            host.update_entity(EntityType::Text(text))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_runtime_error(format!(
                "ocs.move_text: TEXT entity {handle} was not updated"
            )));
        }
        Ok(())
    }

    /// Move and rotate one XY TEXT entity, preserving its other properties.
    #[pyfunction]
    fn set_text_pose(
        handle: u64,
        x: ArgIntoFloat,
        y: ArgIntoFloat,
        z: ArgIntoFloat,
        rotation: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let (x, y, z, rotation) = (
            x.into_float(),
            y.into_float(),
            z.into_float(),
            rotation.into_float(),
        );
        if ![x, y, z, rotation].iter().all(|v| v.is_finite()) {
            return Err(vm.new_value_error(
                "ocs.set_text_pose: coordinates and rotation must be finite".to_owned(),
            ));
        }
        let changed = host_ctx::with_host(|host| {
            let Some(EntityType::Text(mut text)) =
                host.document().get_entity(Handle::new(handle)).cloned()
            else {
                return false;
            };
            if text.normal != Vector3::UNIT_Z {
                return false;
            }
            let point = Vector3::new(x, y, z);
            if uses_text_alignment_point(&text) {
                text.alignment_point = Some(point);
            } else {
                text.insertion_point = point;
            }
            text.rotation = rotation;
            host_ctx::ensure_undo_started(host);
            host.update_entity(EntityType::Text(text))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_runtime_error(format!(
                "ocs.set_text_pose: XY TEXT entity {handle} was not updated"
            )));
        }
        Ok(())
    }

    fn uses_text_alignment_point(text: &Text) -> bool {
        !(text.horizontal_alignment == TextHorizontalAlignment::Left
            && text.vertical_alignment == TextVerticalAlignment::Baseline)
    }

    fn active_text_point(text: &Text) -> Vector3 {
        if uses_text_alignment_point(text) {
            text.alignment_point.unwrap_or(text.insertion_point)
        } else {
            text.insertion_point
        }
    }

    /// Area and label point for a circle or a straight, closed lightweight
    /// polyline in the world XY plane. Unsupported geometry returns None.
    #[pyfunction]
    fn get_area(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found = host_ctx::with_host(|host| {
            let document = host.document();
            match document.get_entity(Handle::new(handle)) {
                Some(EntityType::Circle(circle))
                    if circle.normal == Vector3::UNIT_Z && circle.radius > 0.0 =>
                {
                    Some((circle.area(), circle.center))
                }
                Some(EntityType::LwPolyline(polyline))
                    if polyline.is_closed
                        && polyline.normal == Vector3::UNIT_Z
                        && polyline.vertices.len() >= 3
                        && polyline.vertices.iter().all(|v| v.bulge == 0.0) =>
                {
                    let mut double_area = 0.0;
                    let mut cx = 0.0;
                    let mut cy = 0.0;
                    for index in 0..polyline.vertices.len() {
                        let a = polyline.vertices[index].location;
                        let b = polyline.vertices[(index + 1) % polyline.vertices.len()].location;
                        let cross = a.x * b.y - b.x * a.y;
                        double_area += cross;
                        cx += (a.x + b.x) * cross;
                        cy += (a.y + b.y) * cross;
                    }
                    if double_area.abs() <= f64::EPSILON {
                        None
                    } else {
                        Some((
                            double_area.abs() / 2.0,
                            Vector3::new(
                                cx / (3.0 * double_area),
                                cy / (3.0 * double_area),
                                polyline.elevation,
                            ),
                        ))
                    }
                }
                _ => None,
            }
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let Some((area, point)) = found else {
            return Ok(vm.ctx.none());
        };
        let dict = vm.ctx.new_dict();
        dict.set_item("area", vm.new_pyobj(area), vm)?;
        dict.set_item(
            "point",
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(point.x),
                    vm.new_pyobj(point.y),
                    vm.new_pyobj(point.z),
                ])
                .into(),
            vm,
        )?;
        Ok(dict.into())
    }

    /// Length, halfway point, and local tangent for an XY line, circle, arc,
    /// or straight-segment lightweight polyline.
    #[pyfunction]
    fn get_curve_measure(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found = host_ctx::with_host(|host| {
            let document = host.document();
            match document.get_entity(Handle::new(handle)) {
                Some(EntityType::Line(line))
                    if line.normal == Vector3::UNIT_Z
                        && (line.start.z - line.end.z).abs() <= 1e-9
                        && line.length() > 0.0 =>
                {
                    Some((
                        line.length(),
                        line.midpoint(),
                        (line.end.y - line.start.y).atan2(line.end.x - line.start.x),
                    ))
                }
                Some(EntityType::Circle(circle))
                    if circle.normal == Vector3::UNIT_Z && circle.radius > 0.0 =>
                {
                    Some((
                        2.0 * std::f64::consts::PI * circle.radius,
                        Vector3::new(
                            circle.center.x - circle.radius,
                            circle.center.y,
                            circle.center.z,
                        ),
                        std::f64::consts::FRAC_PI_2,
                    ))
                }
                Some(EntityType::Arc(arc))
                    if arc.normal == Vector3::UNIT_Z
                        && arc.radius > 0.0
                        && arc.arc_length() > 0.0 =>
                {
                    let angle = arc.start_angle + arc.sweep_angle() / 2.0;
                    Some((
                        arc.arc_length(),
                        arc.midpoint(),
                        angle + std::f64::consts::FRAC_PI_2,
                    ))
                }
                Some(EntityType::LwPolyline(polyline))
                    if polyline.normal == Vector3::UNIT_Z
                        && polyline.vertices.len() >= 2
                        && polyline.vertices.iter().all(|v| v.bulge == 0.0) =>
                {
                    let segment_count = if polyline.is_closed {
                        polyline.vertices.len()
                    } else {
                        polyline.vertices.len() - 1
                    };
                    let mut segments = Vec::with_capacity(segment_count);
                    let mut total = 0.0;
                    for index in 0..segment_count {
                        let a = polyline.vertices[index].location;
                        let b = polyline.vertices[(index + 1) % polyline.vertices.len()].location;
                        let length = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
                        segments.push((a, b, length));
                        total += length;
                    }
                    if total <= 0.0 {
                        return None;
                    }
                    let mut remaining = total / 2.0;
                    for (a, b, length) in segments {
                        if length == 0.0 {
                            continue;
                        }
                        if remaining <= length {
                            let fraction = remaining / length;
                            let point = Vector3::new(
                                a.x + (b.x - a.x) * fraction,
                                a.y + (b.y - a.y) * fraction,
                                polyline.elevation,
                            );
                            return Some((total, point, (b.y - a.y).atan2(b.x - a.x)));
                        }
                        remaining -= length;
                    }
                    None
                }
                _ => None,
            }
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let Some((length, point, rotation)) = found else {
            return Ok(vm.ctx.none());
        };
        let dict = vm.ctx.new_dict();
        dict.set_item("length", vm.new_pyobj(length), vm)?;
        dict.set_item(
            "point",
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(point.x),
                    vm.new_pyobj(point.y),
                    vm.new_pyobj(point.z),
                ])
                .into(),
            vm,
        )?;
        dict.set_item("rotation", vm.new_pyobj(rotation), vm)?;
        dict.set_item(
            "tangent",
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(rotation.cos()),
                    vm.new_pyobj(rotation.sin()),
                ])
                .into(),
            vm,
        )?;
        Ok(dict.into())
    }

    /// Start and end points of a nonzero XY line, or None.
    #[pyfunction]
    fn line_endpoints(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found =
            host_ctx::with_host(
                |host| match host.document().get_entity(Handle::new(handle)) {
                    Some(EntityType::Line(line))
                        if line.normal == Vector3::UNIT_Z
                            && (line.start.z - line.end.z).abs() <= 1e-9
                            && line.length() > 0.0 =>
                    {
                        Some((line.start, line.end))
                    }
                    _ => None,
                },
            )
            .ok_or_else(|| {
                vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
            })?;
        let Some((start, end)) = found else {
            return Ok(vm.ctx.none());
        };
        let output = vm.ctx.new_list(vec![
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(start.x),
                    vm.new_pyobj(start.y),
                    vm.new_pyobj(start.z),
                ])
                .into(),
            vm.ctx
                .new_list(vec![
                    vm.new_pyobj(end.x),
                    vm.new_pyobj(end.y),
                    vm.new_pyobj(end.z),
                ])
                .into(),
        ]);
        Ok(output.into())
    }

    /// Insertion point of one top-level block reference, or None.
    #[pyfunction]
    fn block_insert_point(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found =
            host_ctx::with_host(
                |host| match host.document().get_entity(Handle::new(handle)) {
                    Some(EntityType::Insert(insert)) => Some(insert.insert_point),
                    _ => None,
                },
            )
            .ok_or_else(|| {
                vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
            })?;
        let Some(point) = found else {
            return Ok(vm.ctx.none());
        };
        Ok(vm
            .ctx
            .new_list(vec![
                vm.new_pyobj(point.x),
                vm.new_pyobj(point.y),
                vm.new_pyobj(point.z),
            ])
            .into())
    }

    /// Axis-aligned XY footprint of a simple unrotated block reference.
    /// Rejects nested/unsupported definition entities rather than guessing.
    #[pyfunction]
    fn block_bounds_xy(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found = host_ctx::with_host(|host| {
            let document = host.document();
            let Some(EntityType::Insert(insert)) = document.get_entity(Handle::new(handle)) else {
                return None;
            };
            if insert.normal != Vector3::UNIT_Z
                || insert.rotation.abs() > 1e-9
                || insert.row_count != 1
                || insert.column_count != 1
            {
                return None;
            }
            let record = document.block_records.get(&insert.block_name)?;
            if record.entity_handles.is_empty() {
                return None;
            }
            let base = record.base_point;
            let transform = Transform::from_translation(Vector3::new(-base.x, -base.y, -base.z))
                .then(&Transform::from_scaling(Vector3::new(
                    insert.x_scale(),
                    insert.y_scale(),
                    insert.z_scale(),
                )))
                .then(&Transform::from_translation(insert.insert_point));
            let mut xmin = f64::INFINITY;
            let mut ymin = f64::INFINITY;
            let mut xmax = f64::NEG_INFINITY;
            let mut ymax = f64::NEG_INFINITY;
            for entity_handle in &record.entity_handles {
                let mut entity = document.get_entity(*entity_handle)?.clone();
                if !matches!(
                    entity,
                    EntityType::Line(_)
                        | EntityType::Arc(_)
                        | EntityType::Circle(_)
                        | EntityType::LwPolyline(_)
                ) {
                    return None;
                }
                entity.apply_transform(&transform);
                let bounds = entity.as_entity().bounding_box();
                if ![bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
                    .iter()
                    .all(|value| value.is_finite())
                {
                    return None;
                }
                xmin = xmin.min(bounds.min.x);
                ymin = ymin.min(bounds.min.y);
                xmax = xmax.max(bounds.max.x);
                ymax = ymax.max(bounds.max.y);
            }
            if xmax - xmin <= 1e-9 || ymax - ymin <= 1e-9 {
                return None;
            }
            Some([xmin, ymin, xmax, ymax])
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        match found {
            Some(bounds) => Ok(vm
                .ctx
                .new_list(
                    bounds
                        .into_iter()
                        .map(|value| vm.new_pyobj(value))
                        .collect(),
                )
                .into()),
            None => Ok(vm.ctx.none()),
        }
    }

    /// Segment metrics for one XY lightweight polyline, including bulge arcs.
    #[pyfunction]
    fn polyline_segments(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found = host_ctx::with_host(|host| {
            let document = host.document();
            let Some(EntityType::LwPolyline(polyline)) = document.get_entity(Handle::new(handle))
            else {
                return None;
            };
            if polyline.normal != Vector3::UNIT_Z || polyline.vertices.len() < 2 {
                return None;
            }
            let count = if polyline.is_closed {
                polyline.vertices.len()
            } else {
                polyline.vertices.len() - 1
            };
            let mut segments = Vec::with_capacity(count);
            for index in 0..count {
                let a = polyline.vertices[index];
                let b = polyline.vertices[(index + 1) % polyline.vertices.len()];
                let chord = ((b.location.x - a.location.x).powi(2)
                    + (b.location.y - a.location.y).powi(2))
                .sqrt();
                let radius = if a.bulge.abs() > f64::EPSILON && chord > 0.0 {
                    Some(chord * (1.0 + a.bulge * a.bulge) / (4.0 * a.bulge.abs()))
                } else {
                    None
                };
                let center = radius.map(|_| {
                    let offset = (1.0 - a.bulge * a.bulge) / (4.0 * a.bulge);
                    (
                        (a.location.x + b.location.x) / 2.0
                            - (b.location.y - a.location.y) * offset,
                        (a.location.y + b.location.y) / 2.0
                            + (b.location.x - a.location.x) * offset,
                    )
                });
                let length = radius.map_or(chord, |r| r * (4.0 * a.bulge.atan()).abs());
                segments.push((
                    a.location,
                    b.location,
                    a.start_width,
                    a.end_width,
                    length,
                    center,
                    radius,
                ));
            }
            Some(segments)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let Some(segments) = found else {
            return Ok(vm.ctx.none());
        };
        let mut output = Vec::with_capacity(segments.len());
        for (start, end, start_width, end_width, length, center, radius) in segments {
            let dict = vm.ctx.new_dict();
            dict.set_item(
                "start",
                vm.ctx
                    .new_list(vec![vm.new_pyobj(start.x), vm.new_pyobj(start.y)])
                    .into(),
                vm,
            )?;
            dict.set_item(
                "end",
                vm.ctx
                    .new_list(vec![vm.new_pyobj(end.x), vm.new_pyobj(end.y)])
                    .into(),
                vm,
            )?;
            dict.set_item("start_width", vm.new_pyobj(start_width), vm)?;
            dict.set_item("end_width", vm.new_pyobj(end_width), vm)?;
            dict.set_item("length", vm.new_pyobj(length), vm)?;
            dict.set_item(
                "center",
                center.map_or_else(
                    || vm.ctx.none(),
                    |(x, y)| {
                        vm.ctx
                            .new_list(vec![vm.new_pyobj(x), vm.new_pyobj(y)])
                            .into()
                    },
                ),
                vm,
            )?;
            dict.set_item(
                "radius",
                radius.map_or_else(|| vm.ctx.none(), |r| vm.new_pyobj(r)),
                vm,
            )?;
            output.push(dict.into());
        }
        Ok(vm.ctx.new_list(output).into())
    }

    /// Equal-distance samples for an XY line, arc, or circle.
    #[pyfunction]
    fn curve_samples(handle: u64, segments: usize, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        if !(2..=4096).contains(&segments) {
            return Err(vm.new_value_error(
                "ocs.curve_samples: segments must be between 2 and 4096".to_owned(),
            ));
        }
        let found =
            host_ctx::with_host(
                |host| match host.document().get_entity(Handle::new(handle)) {
                    Some(EntityType::Line(line))
                        if line.normal == Vector3::UNIT_Z
                            && (line.start.z - line.end.z).abs() <= 1e-9
                            && line.length() > 0.0 =>
                    {
                        let points = (0..=segments)
                            .map(|i| {
                                let t = i as f64 / segments as f64;
                                Vector2::new(
                                    line.start.x + (line.end.x - line.start.x) * t,
                                    line.start.y + (line.end.y - line.start.y) * t,
                                )
                            })
                            .collect::<Vec<_>>();
                        Some((points, line.start.z, false))
                    }
                    Some(EntityType::Circle(circle))
                        if circle.normal == Vector3::UNIT_Z && circle.radius > 0.0 =>
                    {
                        let points = (0..segments)
                            .map(|i| {
                                let angle = 2.0 * std::f64::consts::PI * i as f64 / segments as f64;
                                Vector2::new(
                                    circle.center.x + circle.radius * angle.cos(),
                                    circle.center.y + circle.radius * angle.sin(),
                                )
                            })
                            .collect::<Vec<_>>();
                        Some((points, circle.center.z, true))
                    }
                    Some(EntityType::Arc(arc))
                        if arc.normal == Vector3::UNIT_Z
                            && arc.radius > 0.0
                            && arc.arc_length() > 0.0 =>
                    {
                        let points = (0..=segments)
                            .map(|i| {
                                let angle = arc.start_angle
                                    + arc.sweep_angle() * i as f64 / segments as f64;
                                Vector2::new(
                                    arc.center.x + arc.radius * angle.cos(),
                                    arc.center.y + arc.radius * angle.sin(),
                                )
                            })
                            .collect::<Vec<_>>();
                        Some((points, arc.center.z, false))
                    }
                    _ => None,
                },
            )
            .ok_or_else(|| {
                vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
            })?;
        let Some((points, elevation, closed)) = found else {
            return Ok(vm.ctx.none());
        };
        let vertices = points
            .into_iter()
            .map(|point| {
                vm.ctx
                    .new_list(vec![vm.new_pyobj(point.x), vm.new_pyobj(point.y)])
                    .into()
            })
            .collect();
        let dict = vm.ctx.new_dict();
        dict.set_item("vertices", vm.ctx.new_list(vertices).into(), vm)?;
        dict.set_item("elevation", vm.new_pyobj(elevation), vm)?;
        dict.set_item("closed", vm.new_pyobj(closed), vm)?;
        Ok(dict.into())
    }

    /// Modelspace XY outline of a rectangular, unclipped +Z paperspace viewport.
    #[pyfunction]
    fn viewport_model_outline(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found = host_ctx::with_host(|host| {
            let Some(EntityType::Viewport(viewport)) =
                host.document().get_entity(Handle::new(handle))
            else {
                return None;
            };
            if viewport.id == 1
                || !viewport.clip_boundary_handle.is_null()
                || viewport.view_direction != Vector3::UNIT_Z
                || ![
                    viewport.width,
                    viewport.height,
                    viewport.view_height,
                    viewport.twist_angle,
                    viewport.view_center.x,
                    viewport.view_center.y,
                    viewport.view_target.x,
                    viewport.view_target.y,
                    viewport.view_target.z,
                ]
                .iter()
                .all(|value| value.is_finite())
                || viewport.width <= 0.0
                || viewport.height <= 0.0
                || viewport.view_height <= 0.0
            {
                return None;
            }
            let scale = viewport.view_height / viewport.height;
            let (sin, cos) = (-viewport.twist_angle).sin_cos();
            let corners = [
                (-viewport.width / 2.0, -viewport.height / 2.0),
                (viewport.width / 2.0, -viewport.height / 2.0),
                (viewport.width / 2.0, viewport.height / 2.0),
                (-viewport.width / 2.0, viewport.height / 2.0),
            ];
            let points = corners.map(|(x, y)| {
                let dx = x * scale + viewport.view_center.x;
                let dy = y * scale + viewport.view_center.y;
                [
                    viewport.view_target.x + dx * cos - dy * sin,
                    viewport.view_target.y + dx * sin + dy * cos,
                ]
            });
            Some((points, viewport.view_target.z))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let Some((points, elevation)) = found else {
            return Ok(vm.ctx.none());
        };
        let dict = vm.ctx.new_dict();
        let vertices = points
            .into_iter()
            .map(|point| {
                vm.ctx
                    .new_list(vec![vm.new_pyobj(point[0]), vm.new_pyobj(point[1])])
                    .into()
            })
            .collect();
        dict.set_item("vertices", vm.ctx.new_list(vertices).into(), vm)?;
        dict.set_item("elevation", vm.new_pyobj(elevation), vm)?;
        Ok(dict.into())
    }

    /// Copy supported modelspace XY curves through a rectangular +Z viewport.
    #[pyfunction]
    fn copy_model_to_paper(
        handles: Vec<u64>,
        viewport_handle: u64,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        if handles.is_empty() {
            return Ok(vm.ctx.new_list(Vec::<PyObjectRef>::new()).into());
        }
        let copies = host_ctx::with_host(|host| {
            let document = host.document();
            let Some(EntityType::Viewport(viewport)) =
                document.get_entity(Handle::new(viewport_handle))
            else {
                return None;
            };
            if viewport.id == 1
                || !viewport.clip_boundary_handle.is_null()
                || viewport.view_direction != Vector3::UNIT_Z
                || ![
                    viewport.width,
                    viewport.height,
                    viewport.view_height,
                    viewport.twist_angle,
                    viewport.center.x,
                    viewport.center.y,
                    viewport.view_center.x,
                    viewport.view_center.y,
                    viewport.view_target.x,
                    viewport.view_target.y,
                    viewport.view_target.z,
                ]
                .iter()
                .all(|value| value.is_finite())
                || viewport.width <= 0.0
                || viewport.height <= 0.0
                || viewport.view_height <= 0.0
            {
                return None;
            }
            let paper_block = viewport.common.owner_handle;
            let block = document
                .block_records
                .iter()
                .find(|record| record.handle == paper_block)?;
            let ObjectType::Layout(layout) = document.objects.get(&block.layout)? else {
                return None;
            };
            if layout.name == "Model" {
                return None;
            }
            let scale = viewport.height / viewport.view_height;
            let transform = Transform::from_translation(Vector3::new(
                -viewport.view_target.x,
                -viewport.view_target.y,
                -viewport.view_target.z,
            ))
            .then(&Transform::from_rotation(
                Vector3::UNIT_Z,
                viewport.twist_angle,
            ))
            .then(&Transform::from_scaling(Vector3::new(scale, scale, 1.0)))
            .then(&Transform::from_translation(Vector3::new(
                viewport.center.x - viewport.view_center.x * scale,
                viewport.center.y - viewport.view_center.y * scale,
                0.0,
            )));
            let mut copies = Vec::with_capacity(handles.len());
            for handle in handles {
                let mut entity = document.get_entity(Handle::new(handle))?.clone();
                if entity.common().owner_handle != document.header.model_space_block_handle {
                    return None;
                }
                let planar = match &entity {
                    EntityType::Line(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.start.z - viewport.view_target.z).abs() <= 1e-9
                            && (item.end.z - viewport.view_target.z).abs() <= 1e-9
                    }
                    EntityType::Arc(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.center.z - viewport.view_target.z).abs() <= 1e-9
                    }
                    EntityType::Circle(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.center.z - viewport.view_target.z).abs() <= 1e-9
                    }
                    EntityType::LwPolyline(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.elevation - viewport.view_target.z).abs() <= 1e-9
                    }
                    _ => false,
                };
                if !planar {
                    return None;
                }
                entity.as_entity_mut().apply_transform(&transform);
                entity.common_mut().handle = Handle::NULL;
                entity.common_mut().owner_handle = paper_block;
                copies.push(entity);
            }
            Some(copies)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_value_error(
                "ocs.copy_model_to_paper: unsupported viewport or source geometry".to_owned(),
            )
        })?;
        let new_handles = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entities(copies)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if new_handles.iter().any(|handle| handle.is_null()) {
            return Err(vm.new_runtime_error(
                "ocs.copy_model_to_paper: host did not add every entity".to_owned(),
            ));
        }
        Ok(vm
            .ctx
            .new_list(
                new_handles
                    .into_iter()
                    .map(|handle| vm.new_pyobj(handle.value()))
                    .collect(),
            )
            .into())
    }

    /// Mirror supported planar curves across an existing XY LINE, optionally
    /// removing the originals. The axis itself is never mirrored or removed.
    #[pyfunction]
    fn mirror_xy_entities(
        handles: Vec<u64>,
        axis_handle: u64,
        erase_source: bool,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        if handles.is_empty() {
            return Ok(vm.ctx.new_list(Vec::<PyObjectRef>::new()).into());
        }
        let unique: std::collections::HashSet<u64> = handles.iter().copied().collect();
        if unique.len() != handles.len() || unique.contains(&axis_handle) {
            return Err(vm.new_value_error(
                "ocs.mirror_xy_entities: handles must be unique and exclude the axis".to_owned(),
            ));
        }
        let prepared = host_ctx::with_host(|host| {
            let document = host.document();
            let Some(EntityType::Line(axis)) = document.get_entity(Handle::new(axis_handle)) else {
                return None;
            };
            if axis.normal != Vector3::UNIT_Z
                || (axis.start.z - axis.end.z).abs() > 1e-9
                || axis.length() <= 0.0
            {
                return None;
            }
            let transform = Transform::from_mirror_line(axis.start, axis.end);
            let mut copies = Vec::with_capacity(handles.len());
            for handle in &handles {
                let mut entity = document.get_entity(Handle::new(*handle))?.clone();
                let planar = match &entity {
                    EntityType::Line(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.start.z - axis.start.z).abs() <= 1e-9
                            && (item.end.z - axis.start.z).abs() <= 1e-9
                    }
                    EntityType::Arc(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.center.z - axis.start.z).abs() <= 1e-9
                    }
                    EntityType::Circle(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.center.z - axis.start.z).abs() <= 1e-9
                    }
                    EntityType::LwPolyline(item) => {
                        item.normal == Vector3::UNIT_Z
                            && (item.elevation - axis.start.z).abs() <= 1e-9
                    }
                    _ => false,
                };
                if !planar {
                    return None;
                }
                entity.apply_mirror(&transform);
                entity.common_mut().handle = Handle::NULL;
                copies.push(entity);
            }
            Some(copies)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_value_error(
                "ocs.mirror_xy_entities: axis or source must be supported planar geometry"
                    .to_owned(),
            )
        })?;
        let (added, removed_all) = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            let added = host.add_entities(prepared);
            let mut removed_all = true;
            if erase_source && added.iter().all(|handle| !handle.is_null()) {
                for handle in &handles {
                    removed_all &= host.remove_entity(Handle::new(*handle));
                }
            }
            (added, removed_all)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if added.iter().any(|handle| handle.is_null()) {
            return Err(vm.new_runtime_error(
                "ocs.mirror_xy_entities: host did not add every entity".to_owned(),
            ));
        }
        if !removed_all {
            return Err(vm.new_runtime_error(
                "ocs.mirror_xy_entities: a source could not be removed; inspect the drawing"
                    .to_owned(),
            ));
        }
        Ok(vm
            .ctx
            .new_list(
                added
                    .into_iter()
                    .map(|handle| vm.new_pyobj(handle.value()))
                    .collect(),
            )
            .into())
    }

    /// Mirror planar curves across the XY line through `point` in `tangent`
    /// direction. The calling script can derive these from a curve midpoint.
    #[pyfunction]
    fn mirror_xy_about_line(
        handles: Vec<u64>,
        point: Vec<f64>,
        tangent: Vec<f64>,
        erase_source: bool,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        if point.len() != 3
            || tangent.len() != 2
            || point
                .iter()
                .chain(tangent.iter())
                .any(|value| !value.is_finite())
            || tangent[0].hypot(tangent[1]) <= 1e-9
            || handles.len()
                != handles
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
        {
            return Err(vm.new_value_error(
                "ocs.mirror_xy_about_line: invalid point, tangent, or duplicate handles".to_owned(),
            ));
        }
        if handles.is_empty() {
            return Ok(vm.ctx.new_list(Vec::<PyObjectRef>::new()).into());
        }
        let start = Vector3::new(point[0], point[1], point[2]);
        let end = Vector3::new(point[0] + tangent[0], point[1] + tangent[1], point[2]);
        let transform = Transform::from_mirror_line(start, end);
        let prepared = host_ctx::with_host(|host| {
            let mut copies = Vec::with_capacity(handles.len());
            for handle in &handles {
                let mut entity = host.document().get_entity(Handle::new(*handle))?.clone();
                let planar = match &entity {
                    EntityType::Line(item) => item.normal == Vector3::UNIT_Z
                        && (item.start.z - start.z).abs() <= 1e-9
                        && (item.end.z - start.z).abs() <= 1e-9,
                    EntityType::Arc(item) => item.normal == Vector3::UNIT_Z
                        && (item.center.z - start.z).abs() <= 1e-9,
                    EntityType::Circle(item) => item.normal == Vector3::UNIT_Z
                        && (item.center.z - start.z).abs() <= 1e-9,
                    EntityType::LwPolyline(item) => item.normal == Vector3::UNIT_Z
                        && (item.elevation - start.z).abs() <= 1e-9,
                    _ => false,
                };
                if !planar {
                    return None;
                }
                entity.apply_mirror(&transform);
                entity.common_mut().handle = Handle::NULL;
                copies.push(entity);
            }
            Some(copies)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| vm.new_value_error(
            "ocs.mirror_xy_about_line: only planar LINE/ARC/CIRCLE/LWPolyline sources are supported".to_owned(),
        ))?;
        let (added, removed_all) = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            let added = host.add_entities(prepared);
            let mut removed_all = true;
            if erase_source && added.iter().all(|handle| !handle.is_null()) {
                for handle in &handles {
                    removed_all &= host.remove_entity(Handle::new(*handle));
                }
            }
            (added, removed_all)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if added.iter().any(|handle| handle.is_null()) || !removed_all {
            return Err(vm.new_runtime_error(
                "ocs.mirror_xy_about_line: host could not mirror every entity; inspect the drawing"
                    .to_owned(),
            ));
        }
        Ok(vm
            .ctx
            .new_list(
                added
                    .into_iter()
                    .map(|handle| vm.new_pyobj(handle.value()))
                    .collect(),
            )
            .into())
    }

    /// Remove the span between two points strictly inside an XY LINE, keeping
    /// its first fragment under the original handle and adding the second.
    #[pyfunction]
    fn break_xy_line(
        handle: u64,
        first: Vec<f64>,
        second: Vec<f64>,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        if first.len() != 2
            || second.len() != 2
            || first
                .iter()
                .chain(second.iter())
                .any(|value| !value.is_finite())
        {
            return Err(vm.new_value_error(
                "ocs.break_xy_line: break points must be finite XY pairs".to_owned(),
            ));
        }
        let prepared = host_ctx::with_host(|host| {
            let Some(EntityType::Line(line)) = host.document().get_entity(Handle::new(handle))
            else {
                return None;
            };
            if line.normal != Vector3::UNIT_Z || (line.start.z - line.end.z).abs() > 1e-9 {
                return None;
            }
            let dx = line.end.x - line.start.x;
            let dy = line.end.y - line.start.y;
            let length_sq = dx * dx + dy * dy;
            if length_sq <= 1e-18 {
                return None;
            }
            let parameter = |point: &[f64]| -> Option<f64> {
                let px = point[0] - line.start.x;
                let py = point[1] - line.start.y;
                let t = (px * dx + py * dy) / length_sq;
                let deviation = (px * dy - py * dx).abs() / length_sq.sqrt();
                if t <= 1e-9 || t >= 1.0 - 1e-9 || deviation > 1e-6 {
                    None
                } else {
                    Some(t)
                }
            };
            let a = parameter(&first)?;
            let b = parameter(&second)?;
            let before = a.min(b);
            let after = a.max(b);
            let at =
                |t: f64| Vector3::new(line.start.x + t * dx, line.start.y + t * dy, line.start.z);
            let mut initial = line.clone();
            let mut final_part = line.clone();
            initial.end = at(before);
            final_part.start = at(after);
            final_part.common.handle = Handle::NULL;
            Some((EntityType::Line(initial), EntityType::Line(final_part)))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_value_error(
                "ocs.break_xy_line: target must be an XY LINE and both points strictly on it"
                    .to_owned(),
            )
        })?;
        let result = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            let added = host.add_entity(prepared.1);
            if added.is_null() {
                return None;
            }
            if !host.update_entity(prepared.0) {
                host.remove_entity(added);
                return None;
            }
            Some(added.value())
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_runtime_error("ocs.break_xy_line: host could not edit the line".to_owned())
        })?;
        Ok(result)
    }

    /// Remove the angular span between two interior XY points on an ARC,
    /// retaining the first fragment's original handle.
    #[pyfunction]
    fn break_xy_arc(
        handle: u64,
        first: Vec<f64>,
        second: Vec<f64>,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        if first.len() != 2
            || second.len() != 2
            || first
                .iter()
                .chain(second.iter())
                .any(|value| !value.is_finite())
        {
            return Err(
                vm.new_value_error("ocs.break_xy_arc: points must be finite XY pairs".to_owned())
            );
        }
        let prepared = host_ctx::with_host(|host| {
            let Some(EntityType::Arc(arc)) = host.document().get_entity(Handle::new(handle)) else {
                return None;
            };
            if arc.normal != Vector3::UNIT_Z
                || arc.radius <= 0.0
                || arc.sweep_angle() <= 1e-9
                || arc.sweep_angle() >= std::f64::consts::TAU - 1e-9
            {
                return None;
            }
            let parameter = |point: &[f64]| -> Option<f64> {
                let dx = point[0] - arc.center.x;
                let dy = point[1] - arc.center.y;
                if (dx.hypot(dy) - arc.radius).abs() > 1e-6 {
                    return None;
                }
                let angle = dy.atan2(dx);
                let t = (angle - arc.start_angle).rem_euclid(std::f64::consts::TAU);
                if t <= 1e-9 || t >= arc.sweep_angle() - 1e-9 {
                    None
                } else {
                    Some(t)
                }
            };
            let a = parameter(&first)?;
            let b = parameter(&second)?;
            let mut initial = arc.clone();
            let mut final_part = arc.clone();
            initial.end_angle = arc.start_angle + a.min(b);
            final_part.start_angle = arc.start_angle + a.max(b);
            final_part.common.handle = Handle::NULL;
            Some((EntityType::Arc(initial), EntityType::Arc(final_part)))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_value_error(
                "ocs.break_xy_arc: target must be a nonfull XY ARC with interior points on it"
                    .to_owned(),
            )
        })?;
        let result = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            let added = host.add_entity(prepared.1);
            if added.is_null() {
                return None;
            }
            if !host.update_entity(prepared.0) {
                host.remove_entity(added);
                return None;
            }
            Some(added.value())
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_runtime_error("ocs.break_xy_arc: host could not edit the arc".to_owned())
        })?;
        Ok(result)
    }

    /// Replace an XY CIRCLE with the complementary ARC after removing the
    /// counterclockwise span from `first` to `second`.
    #[pyfunction]
    fn break_xy_circle(
        handle: u64,
        first: Vec<f64>,
        second: Vec<f64>,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        if first.len() != 2
            || second.len() != 2
            || first
                .iter()
                .chain(second.iter())
                .any(|value| !value.is_finite())
        {
            return Err(vm.new_value_error(
                "ocs.break_xy_circle: points must be finite XY pairs".to_owned(),
            ));
        }
        let prepared = host_ctx::with_host(|host| {
            let Some(EntityType::Circle(circle)) = host.document().get_entity(Handle::new(handle))
            else {
                return None;
            };
            if circle.normal != Vector3::UNIT_Z || circle.radius <= 0.0 {
                return None;
            }
            let angle = |point: &[f64]| -> Option<f64> {
                let dx = point[0] - circle.center.x;
                let dy = point[1] - circle.center.y;
                if (dx.hypot(dy) - circle.radius).abs() > 1e-6 {
                    None
                } else {
                    Some(dy.atan2(dx).rem_euclid(std::f64::consts::TAU))
                }
            };
            let start = angle(&first)?;
            let end = angle(&second)?;
            if (end - start).rem_euclid(std::f64::consts::TAU) <= 1e-9 {
                return None;
            }
            let mut arc = Arc::from_center_radius_angles(circle.center, circle.radius, end, start);
            arc.common = circle.common.clone();
            arc.thickness = circle.thickness;
            arc.normal = circle.normal;
            Some(EntityType::Arc(arc))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_value_error(
                "ocs.break_xy_circle: target must be an XY CIRCLE with distinct points on it"
                    .to_owned(),
            )
        })?;
        let updated = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.update_entity(prepared)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !updated {
            return Err(vm.new_runtime_error(
                "ocs.break_xy_circle: host could not edit the circle".to_owned(),
            ));
        }
        Ok(())
    }

    /// Remove the path interval between two interior points on a straight,
    /// zero-width XY lightweight polyline. A closed polyline becomes one open
    /// complementary path, under its original handle.
    #[pyfunction]
    fn break_xy_polyline(
        handle: u64,
        first: Vec<f64>,
        second: Vec<f64>,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        if first.len() != 2
            || second.len() != 2
            || first
                .iter()
                .chain(second.iter())
                .any(|value| !value.is_finite())
        {
            return Err(vm.new_value_error(
                "ocs.break_xy_polyline: points must be finite XY pairs".to_owned(),
            ));
        }
        let prepared = host_ctx::with_host(|host| {
            let Some(EntityType::LwPolyline(polyline)) = host.document().get_entity(Handle::new(handle)) else {
                return None;
            };
            if polyline.normal != Vector3::UNIT_Z
                || polyline.vertices.len() < (if polyline.is_closed { 3 } else { 2 })
                || polyline.vertices.iter().any(|vertex| vertex.bulge != 0.0
                    || vertex.start_width != 0.0 || vertex.end_width != 0.0) {
                return None;
            }
            let segment_count = polyline.vertices.len() - if polyline.is_closed { 0 } else { 1 };
            let mut lengths = Vec::with_capacity(segment_count);
            let mut total = 0.0;
            for index in 0..segment_count {
                let a = polyline.vertices[index].location;
                let b = polyline.vertices[(index + 1) % polyline.vertices.len()].location;
                let length = (b.x - a.x).hypot(b.y - a.y);
                lengths.push(length);
                total += length;
            }
            if total <= 1e-9 {
                return None;
            }
            let locate = |point: &[f64]| -> Option<(usize, f64)> {
                let mut walked = 0.0;
                for (index, length) in lengths.iter().copied().enumerate() {
                    if length <= 1e-9 {
                        continue;
                    }
                    let a = polyline.vertices[index].location;
                    let b = polyline.vertices[(index + 1) % polyline.vertices.len()].location;
                    let dx = b.x - a.x;
                    let dy = b.y - a.y;
                    let px = point[0] - a.x;
                    let py = point[1] - a.y;
                    let fraction = (px * dx + py * dy) / (length * length);
                    let deviation = (px * dy - py * dx).abs() / length;
                    let position = walked + fraction * length;
                    if fraction > 1e-9 && fraction < 1.0 - 1e-9
                        && deviation <= 1e-6 && position > 1e-9
                        && position < total - 1e-9 {
                        return Some((index, position));
                    }
                    walked += length;
                }
                None
            };
            let a = locate(&first)?;
            let b = locate(&second)?;
            if (first[0] - second[0]).hypot(first[1] - second[1]) <= 1e-9 {
                return None;
            }
            if polyline.is_closed {
                let mut complement = polyline.clone();
                let mut vertices = vec![LwVertex::new(Vector2::new(second[0], second[1]))];
                if b.0 != a.0 || b.1 > a.1 {
                    let mut index = (b.0 + 1) % polyline.vertices.len();
                    loop {
                        vertices.push(polyline.vertices[index]);
                        if index == a.0 {
                            break;
                        }
                        index = (index + 1) % polyline.vertices.len();
                    }
                }
                vertices.push(LwVertex::new(Vector2::new(first[0], first[1])));
                complement.vertices = vertices;
                complement.is_closed = false;
                return Some((EntityType::LwPolyline(complement), None));
            }
            let ((before_index, before_point), (after_index, after_point)) =
                if a.1 <= b.1 { ((a.0, &first), (b.0, &second)) }
                else { ((b.0, &second), (a.0, &first)) };
            let mut initial = polyline.clone();
            initial.vertices.truncate(before_index + 1);
            initial.vertices.push(LwVertex::new(Vector2::new(before_point[0], before_point[1])));
            let mut final_part = polyline.clone();
            final_part.vertices = vec![LwVertex::new(Vector2::new(after_point[0], after_point[1]))];
            final_part.vertices.extend_from_slice(&polyline.vertices[after_index + 1..]);
            final_part.common.handle = Handle::NULL;
            Some((EntityType::LwPolyline(initial), Some(EntityType::LwPolyline(final_part))))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| vm.new_value_error(
            "ocs.break_xy_polyline: target must be a straight zero-width XY polyline with distinct interior points".to_owned(),
        ))?;
        let result = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            if let Some(final_part) = prepared.1 {
                let added = host.add_entity(final_part);
                if added.is_null() {
                    return None;
                }
                if !host.update_entity(prepared.0) {
                    host.remove_entity(added);
                    return None;
                }
                Some(added.value())
            } else if host.update_entity(prepared.0) {
                Some(handle)
            } else {
                None
            }
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| {
            vm.new_runtime_error(
                "ocs.break_xy_polyline: host could not edit the polyline".to_owned(),
            )
        })?;
        Ok(result)
    }

    /// Rigidly move and rotate planar curves, mapping one source XY point to
    /// one destination point. Optionally add transformed copies instead of
    /// changing the originals. This intentionally does not scale geometry.
    #[pyfunction]
    fn align_xy_entities(
        handles: Vec<u64>,
        source: Vec<f64>,
        destination: Vec<f64>,
        source_direction: Vec<f64>,
        target_direction: Vec<f64>,
        copy_mode: bool,
        vm: &VirtualMachine,
    ) -> PyResult<usize> {
        if source.len() != 3
            || destination.len() != 3
            || source_direction.len() != 2
            || target_direction.len() != 2
            || source
                .iter()
                .chain(destination.iter())
                .chain(source_direction.iter())
                .chain(target_direction.iter())
                .any(|value| !value.is_finite())
            || handles.len()
                != handles
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
        {
            return Err(vm.new_value_error(
                "ocs.align_xy_entities: invalid points, directions, or duplicate handles"
                    .to_owned(),
            ));
        }
        let source_length = source_direction[0].hypot(source_direction[1]);
        let target_length = target_direction[0].hypot(target_direction[1]);
        if source_length <= 0.0 || target_length <= 0.0 {
            return Err(
                vm.new_value_error("ocs.align_xy_entities: directions must be nonzero".to_owned())
            );
        }
        if handles.is_empty() {
            return Ok(0);
        }
        let rotation = target_direction[1].atan2(target_direction[0])
            - source_direction[1].atan2(source_direction[0]);
        let source = Vector3::new(source[0], source[1], source[2]);
        let destination = Vector3::new(destination[0], destination[1], destination[2]);
        let transform = Transform::from_translation(Vector3::new(-source.x, -source.y, -source.z))
            .then(&Transform::from_rotation(Vector3::UNIT_Z, rotation))
            .then(&Transform::from_translation(destination));
        let mut prepared = host_ctx::with_host(|host| {
            let mut entities = Vec::with_capacity(handles.len());
            for handle in &handles {
                let mut entity = host.document().get_entity(Handle::new(*handle))?.clone();
                let planar = match &entity {
                    EntityType::Line(item) => item.normal == Vector3::UNIT_Z
                        && (item.start.z - item.end.z).abs() <= 1e-9,
                    EntityType::Arc(item) => item.normal == Vector3::UNIT_Z,
                    EntityType::Circle(item) => item.normal == Vector3::UNIT_Z,
                    EntityType::LwPolyline(item) => item.normal == Vector3::UNIT_Z,
                    _ => false,
                };
                if !planar {
                    return None;
                }
                entity.apply_transform(&transform);
                entities.push(entity);
            }
            Some(entities)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
        .ok_or_else(|| vm.new_value_error(
            "ocs.align_xy_entities: only planar LINE/ARC/CIRCLE/LWPolyline sources are supported".to_owned(),
        ))?;
        if copy_mode {
            for entity in &mut prepared {
                entity.common_mut().handle = Handle::NULL;
            }
        }
        let updated = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            if copy_mode {
                host.add_entities(prepared)
                    .iter()
                    .all(|handle| !handle.is_null())
            } else {
                prepared
                    .into_iter()
                    .all(|entity| host.update_entity(entity))
            }
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !updated {
            return Err(vm.new_runtime_error(
                "ocs.align_xy_entities: host could not write every entity; inspect the drawing"
                    .to_owned(),
            ));
        }
        Ok(handles.len())
    }

    /// Replace an XY line, arc, or circle with a lightweight polyline,
    /// retaining its handle, layer, and common drawing properties.
    #[pyfunction]
    fn replace_with_polyline(
        handle: u64,
        vertices: Vec<Vec<f64>>,
        elevation: ArgIntoFloat,
        closed: bool,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let elevation = elevation.into_float();
        if vertices.len() < (if closed { 3 } else { 2 })
            || vertices
                .iter()
                .any(|point| point.len() != 2 || point.iter().any(|value| !value.is_finite()))
            || !elevation.is_finite()
        {
            return Err(vm.new_value_error(
                "ocs.replace_with_polyline: invalid vertices or elevation".to_owned(),
            ));
        }
        let changed = host_ctx::with_host(|host| {
            let Some(entity) = host.document().get_entity(Handle::new(handle)).cloned() else {
                return false;
            };
            let supported = match &entity {
                EntityType::Line(line) => {
                    line.normal == Vector3::UNIT_Z && (line.start.z - line.end.z).abs() <= 1e-9
                }
                EntityType::Arc(arc) => arc.normal == Vector3::UNIT_Z,
                EntityType::Circle(circle) => circle.normal == Vector3::UNIT_Z,
                _ => false,
            };
            if !supported {
                return false;
            }
            let mut polyline = LwPolyline::new();
            polyline.common = entity.common().clone();
            polyline.elevation = elevation;
            polyline.is_closed = closed;
            for point in vertices {
                polyline
                    .vertices
                    .push(LwVertex::new(Vector2::new(point[0], point[1])));
            }
            host_ctx::ensure_undo_started(host);
            host.update_entity(EntityType::LwPolyline(polyline))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_runtime_error(format!(
                "ocs.replace_with_polyline: entity {handle} was not updated"
            )));
        }
        Ok(())
    }

    /// Create a native OCS table from a rectangular grid of cell strings.
    #[pyfunction]
    fn add_table(
        rows: Vec<Vec<String>>,
        x: ArgIntoFloat,
        y: ArgIntoFloat,
        z: ArgIntoFloat,
        row_height: ArgIntoFloat,
        column_width: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let (x, y, z, row_height, column_width) = (
            x.into_float(),
            y.into_float(),
            z.into_float(),
            row_height.into_float(),
            column_width.into_float(),
        );
        let columns = rows.first().map_or(0, Vec::len);
        if rows.is_empty()
            || columns == 0
            || rows.iter().any(|row| row.len() != columns)
            || ![x, y, z, row_height, column_width]
                .iter()
                .all(|v| v.is_finite())
            || row_height <= 0.0
            || column_width <= 0.0
        {
            return Err(vm.new_value_error("ocs.add_table: rows must be rectangular and nonempty; dimensions must be positive and finite".to_owned()));
        }
        let mut builder = TableBuilder::new(rows.len(), columns)
            .at(Vector3::new(x, y, z))
            .row_height(row_height)
            .column_width(column_width);
        for (row_index, row) in rows.iter().enumerate() {
            for (column_index, value) in row.iter().enumerate() {
                builder = builder.cell_text(row_index, column_index, value);
            }
        }
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::Table(builder.build())).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Add a 2D line from `(x1, y1)` to `(x2, y2)` on layer `"0"`, returning
    /// its handle. Starts the script's shared undo group (see
    /// `host_ctx::ensure_undo_started`) if this is the first write this call.
    ///
    /// Coordinates take `ArgIntoFloat` rather than a bare `f64` so plain
    /// Python ints work too (`ocs.add_line(0, 0, 10, 10)`) — a bare `f64`
    /// parameter in RustPython, unlike CPython's C-function argument parsing,
    /// does *not* implicitly coerce an `int`, and rejects it with a `TypeError`.
    #[pyfunction]
    fn add_line(
        x1: ArgIntoFloat,
        y1: ArgIntoFloat,
        x2: ArgIntoFloat,
        y2: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let line = codec::Line::from_coords(
            x1.into_float(),
            y1.into_float(),
            0.0,
            x2.into_float(),
            y2.into_float(),
            0.0,
        );
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::Line(line)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Add a 2D circle centered at `(x, y)` with the given `radius`, on layer
    /// `"0"`, returning its handle. Same undo-grouping and int/float argument
    /// handling as `add_line`.
    #[pyfunction]
    fn add_circle(
        x: ArgIntoFloat,
        y: ArgIntoFloat,
        radius: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let circle =
            codec::Circle::from_coords(x.into_float(), y.into_float(), 0.0, radius.into_float());
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::Circle(circle)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Add a 2D arc centered at `(x, y)`, from `start_deg` to `end_deg`
    /// (degrees, counterclockwise from the +X axis — matching how OCS's own
    /// `ARC`/angle fields are conventionally read), on layer `"0"`. Returns
    /// its handle. Same undo-grouping and int/float argument handling as
    /// `add_line`.
    #[pyfunction]
    fn add_arc(
        x: ArgIntoFloat,
        y: ArgIntoFloat,
        radius: ArgIntoFloat,
        start_deg: ArgIntoFloat,
        end_deg: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let center = codec::Vector3::new(x.into_float(), y.into_float(), 0.0);
        let arc = codec::Arc::from_center_radius_angles(
            center,
            radius.into_float(),
            start_deg.into_float().to_radians(),
            end_deg.into_float().to_radians(),
        );
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::Arc(arc)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Add an open 2D lightweight polyline. Each vertex is
    /// [x, y, start_width, end_width]; widths belong to the segment starting
    /// at that vertex. The polyline uses a single explicit elevation.
    #[pyfunction]
    fn add_polyline(
        vertices: Vec<Vec<f64>>,
        elevation: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let elevation = elevation.into_float();
        if vertices.len() < 2 || !elevation.is_finite() {
            return Err(vm.new_value_error(
                "ocs.add_polyline: need at least two vertices and a finite elevation".to_owned(),
            ));
        }
        let mut polyline = LwPolyline::new();
        polyline.elevation = elevation;
        for vertex in vertices {
            if vertex.len() != 4
                || vertex.iter().any(|value| !value.is_finite())
                || vertex[2] < 0.0
                || vertex[3] < 0.0
            {
                return Err(vm.new_value_error("ocs.add_polyline: each vertex must be [finite x, y, nonnegative start_width, end_width]".to_owned()));
            }
            let mut item = LwVertex::new(Vector2::new(vertex[0], vertex[1]));
            item.start_width = vertex[2];
            item.end_width = vertex[3];
            polyline.vertices.push(item);
        }
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::LwPolyline(polyline)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Add a closed XY lightweight polyline with vertices `[x, y]`.
    #[pyfunction]
    fn add_closed_polyline(
        vertices: Vec<Vec<f64>>,
        elevation: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let elevation = elevation.into_float();
        if vertices.len() < 3
            || !elevation.is_finite()
            || vertices
                .iter()
                .any(|point| point.len() != 2 || point.iter().any(|v| !v.is_finite()))
        {
            return Err(vm.new_value_error("ocs.add_closed_polyline: need at least three finite [x, y] vertices and finite elevation".to_owned()));
        }
        let mut polyline = LwPolyline::new();
        polyline.elevation = elevation;
        polyline.is_closed = true;
        for point in vertices {
            polyline
                .vertices
                .push(LwVertex::new(Vector2::new(point[0], point[1])));
        }
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::LwPolyline(polyline)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Replace a closed lightweight polyline's shape, preserving handle/properties/XDATA.
    #[pyfunction]
    fn update_closed_polyline(
        handle: u64,
        vertices: Vec<Vec<f64>>,
        elevation: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        let elevation = elevation.into_float();
        if vertices.len() < 3
            || !elevation.is_finite()
            || vertices
                .iter()
                .any(|point| point.len() != 2 || point.iter().any(|v| !v.is_finite()))
        {
            return Err(vm.new_value_error("ocs.update_closed_polyline: need at least three finite [x, y] vertices and finite elevation".to_owned()));
        }
        let changed = host_ctx::with_host(|host| {
            let Some(EntityType::LwPolyline(mut polyline)) =
                host.document().get_entity(Handle::new(handle)).cloned()
            else {
                return false;
            };
            if !polyline.is_closed {
                return false;
            }
            polyline.elevation = elevation;
            polyline.vertices = vertices
                .into_iter()
                .map(|point| LwVertex::new(Vector2::new(point[0], point[1])))
                .collect();
            host_ctx::ensure_undo_started(host);
            host.update_entity(EntityType::LwPolyline(polyline))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        if !changed {
            return Err(vm.new_runtime_error(format!("ocs.update_closed_polyline: entity {handle} is not a writable closed lightweight polyline")));
        }
        Ok(())
    }

    /// Add many 3D points in one host round trip, each `[x, y, z]`. Returns
    /// the new handles in the same order as `points`.
    ///
    /// Ported from `schoeller/ocs_python_repl`'s `ocs.doc.add_many(...)`,
    /// which batches thousands of points into a single request instead of
    /// one per point. That plugin is out-of-process (real round-trip cost
    /// per call); this one is in-process, so the win here is different but
    /// still real: `HostApi::add_entities` lets the host coalesce the
    /// undo/dirty/geometry-rebuild bookkeeping across the whole batch
    /// instead of repeating it once per point (its default implementation
    /// merely loops `add_entity`, but hosts are expected to override it for
    /// batch efficiency — see its doc comment in `ocs_plugin_api`).
    #[pyfunction]
    fn add_points(points: Vec<Vec<f64>>, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        if points.is_empty() {
            return Err(vm.new_value_error("ocs.add_points: need at least one point".to_owned()));
        }
        let mut entities = Vec::with_capacity(points.len());
        for point in &points {
            if point.len() != 3 || point.iter().any(|value| !value.is_finite()) {
                return Err(vm.new_value_error(
                    "ocs.add_points: each point must be [finite x, y, z]".to_owned(),
                ));
            }
            entities.push(EntityType::Point(codec::Point::from_coords(
                point[0], point[1], point[2],
            )));
        }
        let handles = host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entities(entities)
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        let items = handles
            .into_iter()
            .map(|h| vm.new_pyobj(h.value()))
            .collect();
        Ok(vm.ctx.new_list(items).into())
    }

    /// Add one left/baseline-aligned single-line TEXT entity.
    /// Its insertion point is the supplied point, which also makes it
    /// eligible for `text_box_vertices` and the associated-textbox port.
    #[pyfunction]
    fn add_text_left(
        value: rustpython_vm::builtins::PyStrRef,
        x: ArgIntoFloat,
        y: ArgIntoFloat,
        z: ArgIntoFloat,
        height: ArgIntoFloat,
        rotation_radians: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let (x, y, z, height, rotation_radians) = (
            x.into_float(),
            y.into_float(),
            z.into_float(),
            height.into_float(),
            rotation_radians.into_float(),
        );
        if ![x, y, z, height, rotation_radians]
            .iter()
            .all(|v| v.is_finite())
            || height <= 0.0
        {
            return Err(vm.new_value_error("ocs.add_text_left: coordinates and rotation must be finite; height must be positive".to_owned()));
        }
        let mut text = Text::with_value(value.to_string(), Vector3::new(x, y, z))
            .with_height(height)
            .with_rotation(rotation_radians);
        text.horizontal_alignment = TextHorizontalAlignment::Left;
        text.vertical_alignment = TextVerticalAlignment::Baseline;
        text.alignment_point = None;
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::Text(text)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Add one centered, middle-aligned single-line TEXT entity.
    #[pyfunction]
    fn add_text_centered(
        value: rustpython_vm::builtins::PyStrRef,
        x: ArgIntoFloat,
        y: ArgIntoFloat,
        z: ArgIntoFloat,
        height: ArgIntoFloat,
        rotation_radians: ArgIntoFloat,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        let (x, y, z, height, rotation_radians) = (
            x.into_float(),
            y.into_float(),
            z.into_float(),
            height.into_float(),
            rotation_radians.into_float(),
        );
        if ![x, y, z, height, rotation_radians]
            .iter()
            .all(|v| v.is_finite())
            || height <= 0.0
        {
            return Err(vm.new_value_error("ocs.add_text_centered: coordinates and rotation must be finite; height must be positive".to_owned()));
        }
        let point = Vector3::new(x, y, z);
        let mut text = Text::with_value(value.to_string(), point)
            .with_height(height)
            .with_rotation(rotation_radians);
        text.alignment_point = Some(point);
        text.horizontal_alignment = TextHorizontalAlignment::Center;
        text.vertical_alignment = TextVerticalAlignment::Middle;
        host_ctx::with_host(|host| {
            host_ctx::ensure_undo_started(host);
            host.add_entity(EntityType::Text(text)).value()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))
    }

    /// Run a full command line (built-in or plugin) to completion, exactly
    /// as if typed — including any inline point/keyword/handle tokens an
    /// interactive command needs (e.g. `ocs.command("LINE 0,0 10,10")`, or
    /// `ocs.command(f"PCONSTRAINT {h1:X} {h2:X}")` with two entity handles
    /// for a constraint command's object picks). Raises a Python exception
    /// on failure — including on hosts that predate `HostApi::run_command`
    /// (Phase 1, `ocs.command()` — see `docs/python-scripting-roadmap.md`
    /// in the OpenCADStudio repo for what this can and can't do).
    #[cfg(feature = "experimental-command-replay")]
    #[pyfunction]
    fn command(cmd: rustpython_vm::builtins::PyStrRef, vm: &VirtualMachine) -> PyResult<()> {
        let cmd = cmd.to_string();
        let result = host_ctx::with_host(|host| host.run_command(&cmd)).ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })?;
        result.map_err(|e| vm.new_runtime_error(format!("ocs.command({cmd:?}): {e}")))
    }

    /// Set the document's selection to exactly these entity handles
    /// (clearing any existing selection first). The missing piece for
    /// driving a constraint command via `ocs.command()`: those read a prior
    /// selection rather than picks fed as command-line tokens. Errors,
    /// without partially applying, if any handle doesn't exist.
    #[cfg(any(
        feature = "experimental-command-replay",
        feature = "experimental-host-model"
    ))]
    #[pyfunction]
    fn select(handles: Vec<u64>, vm: &VirtualMachine) -> PyResult<()> {
        let handles: Vec<Handle> = handles.into_iter().map(Handle::new).collect();
        let result = host_ctx::with_host(|host| host.set_selection(&handles)).ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })?;
        result.map_err(|e| vm.new_runtime_error(format!("ocs.select(...): {e}")))
    }

    fn kind_name(kind: ReaderEntityKind) -> &'static str {
        match kind {
            ReaderEntityKind::Point => "point",
            ReaderEntityKind::Line => "line",
            ReaderEntityKind::Circle => "circle",
            ReaderEntityKind::Arc => "arc",
            ReaderEntityKind::Polyline => "polyline",
            ReaderEntityKind::Text => "text",
            ReaderEntityKind::Other => "other",
        }
    }

    // ════════════════════════════════════════════════════════════════════
    // Generic entity CRUD (Phase 0 — see this session's
    // `project_opencad_python_generic_entity_crud_plan.md` memory).
    //
    // `dict_to_entity`/`entity_to_dict` below are generated at build time
    // (`build.rs` + `build/generate.rs`, driven by `entity_manifest.json`
    // plus the type registry `ocs_plugin_api` embeds) from the registry's
    // 12 currently-traced entity kinds, wired into `ocs.add` / `ocs.update`
    // and the feature-gated document model. Round trips are exercised by
    // `entity_crud_tests` below.
    //
    // A handful of leaf conversions the generated code calls into are
    // hand-written here rather than generated, matching how `vector3_to_py`
    // /`py_to_vector3` above are already hand-written for XDATA: `Vector3`/
    // `Vector2`/`Handle` have no registry-driven struct shape worth
    // generating (they're represented as `{"x":..,"y":..,"z":..}` /
    // `{"x":..,"y":..}` dicts, matching `get()`'s existing `point` dict —
    // not the `[x, y, z]` list XDATA uses), and `get_opt_*` are tiny,
    // reused by every generated setter.

    fn vector2_to_py(vm: &VirtualMachine, v: &Vector2) -> PyResult<PyObjectRef> {
        let dict = vm.ctx.new_dict();
        dict.set_item("x", vm.new_pyobj(v.x), vm)?;
        dict.set_item("y", vm.new_pyobj(v.y), vm)?;
        Ok(dict.into())
    }

    fn py_to_vector2(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<Vector2> {
        let dict = value.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
        let x = get_opt_f64(&dict, "x", vm)?;
        let y = get_opt_f64(&dict, "y", vm)?;
        Ok(Vector2::new(x, y))
    }

    fn vector3_to_py_dict(vm: &VirtualMachine, v: &Vector3) -> PyResult<PyObjectRef> {
        let dict = vm.ctx.new_dict();
        dict.set_item("x", vm.new_pyobj(v.x), vm)?;
        dict.set_item("y", vm.new_pyobj(v.y), vm)?;
        dict.set_item("z", vm.new_pyobj(v.z), vm)?;
        Ok(dict.into())
    }

    fn py_to_vector3_dict(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<Vector3> {
        let dict = value.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
        let x = get_opt_f64(&dict, "x", vm)?;
        let y = get_opt_f64(&dict, "y", vm)?;
        let z = get_opt_f64(&dict, "z", vm)?;
        Ok(Vector3::new(x, y, z))
    }

    fn get_opt_f64(
        dict: &rustpython_vm::builtins::PyDictRef,
        key: &str,
        vm: &VirtualMachine,
    ) -> PyResult<f64> {
        match dict.get_item_opt(key, vm)? {
            None => Ok(0.0),
            Some(v) if vm.is_none(&v) => Ok(0.0),
            Some(v) => py_number_to_f64(v, vm),
        }
    }

    /// `Color` as `{"kind": "ByLayer" | "None" | "ByBlock"}`,
    /// `{"kind": "Index", "value": n}` or
    /// `{"kind": "Rgb", "value": {"r": .., "g": .., "b": ..}}`.
    fn color_to_py_dict(vm: &VirtualMachine, color: &codec::types::Color) -> PyResult<PyObjectRef> {
        use codec::types::Color;
        let dict = vm.ctx.new_dict();
        match color {
            Color::ByLayer => dict.set_item("kind", vm.new_pyobj("ByLayer"), vm)?,
            Color::None => dict.set_item("kind", vm.new_pyobj("None"), vm)?,
            Color::ByBlock => dict.set_item("kind", vm.new_pyobj("ByBlock"), vm)?,
            Color::Index(index) => {
                dict.set_item("kind", vm.new_pyobj("Index"), vm)?;
                dict.set_item("value", vm.new_pyobj(i64::from(*index)), vm)?;
            }
            Color::Rgb { r, g, b } => {
                dict.set_item("kind", vm.new_pyobj("Rgb"), vm)?;
                let rgb = vm.ctx.new_dict();
                rgb.set_item("r", vm.new_pyobj(i64::from(*r)), vm)?;
                rgb.set_item("g", vm.new_pyobj(i64::from(*g)), vm)?;
                rgb.set_item("b", vm.new_pyobj(i64::from(*b)), vm)?;
                dict.set_item("value", PyObjectRef::from(rgb), vm)?;
            }
        }
        Ok(dict.into())
    }

    fn py_to_color_dict(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<codec::types::Color> {
        use codec::types::Color;
        let dict = value.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
        let kind = dict.get_item("kind", vm)?.try_into_value::<String>(vm)?;
        let byte = |v: PyObjectRef, what: &str| -> PyResult<u8> {
            let n = v.try_into_value::<i64>(vm)?;
            u8::try_from(n).map_err(|_| vm.new_value_error(format!("Color {what} must be 0..=255")))
        };
        Ok(match kind.as_str() {
            "ByLayer" => Color::ByLayer,
            "None" => Color::None,
            "ByBlock" => Color::ByBlock,
            "Index" => {
                let index = byte(dict.get_item("value", vm)?, "index")?;
                if index == 0 {
                    return Err(vm.new_value_error("Color index must be 1..=255; use ByBlock for 0".to_owned()));
                }
                Color::Index(index)
            }
            "Rgb" => {
                let rgb = dict.get_item("value", vm)?.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
                Color::Rgb {
                    r: byte(rgb.get_item("r", vm)?, "r")?,
                    g: byte(rgb.get_item("g", vm)?, "g")?,
                    b: byte(rgb.get_item("b", vm)?, "b")?,
                }
            }
            other => return Err(vm.new_value_error(format!("ocs: unsupported Color kind: {other}"))),
        })
    }

    fn py_to_f64_array<const N: usize>(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<[f64; N]> {
        let items: Vec<PyObjectRef> = value.try_into_value(vm)?;
        if items.len() != N {
            return Err(vm.new_value_error(format!("expected {N} numbers, got {}", items.len())));
        }
        let mut out = [0.0; N];
        for (slot, item) in out.iter_mut().zip(items) {
            *slot = py_number_to_f64(item, vm)?;
        }
        Ok(out)
    }

    /// Accept a Python int or float; `try_into_value::<f64>` rejects ints.
    fn py_number_to_f64(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<f64> {
        Ok(value.try_float(vm)?.to_f64())
    }

    fn get_opt_bool(
        dict: &rustpython_vm::builtins::PyDictRef,
        key: &str,
        vm: &VirtualMachine,
    ) -> PyResult<bool> {
        match dict.get_item_opt(key, vm)? {
            None => Ok(false),
            Some(v) if vm.is_none(&v) => Ok(false),
            Some(v) => v.try_into_value::<bool>(vm),
        }
    }

    fn get_opt_u64(
        dict: &rustpython_vm::builtins::PyDictRef,
        key: &str,
        vm: &VirtualMachine,
    ) -> PyResult<u64> {
        match dict.get_item_opt(key, vm)? {
            None => Ok(0),
            Some(v) if vm.is_none(&v) => Ok(0),
            Some(v) => v.try_into_value::<u64>(vm),
        }
    }

    fn get_opt_string(
        dict: &rustpython_vm::builtins::PyDictRef,
        key: &str,
        default: &str,
        vm: &VirtualMachine,
    ) -> PyResult<String> {
        match dict.get_item_opt(key, vm)? {
            None => Ok(default.to_owned()),
            Some(v) if vm.is_none(&v) => Ok(default.to_owned()),
            Some(v) => v.try_into_value::<String>(vm),
        }
    }

    fn ensure_known_entity_keys(
        dict: &rustpython_vm::builtins::PyDictRef,
        kind: &str,
        allowed: &[&str],
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        for key in dict.keys_vec() {
            let key = key.try_into_value::<String>(vm)?;
            if !allowed.contains(&key.as_str()) {
                return Err(vm.new_value_error(format!(
                    "ocs: {kind} property {key:?} is outside the editable schema"
                )));
            }
        }
        Ok(())
    }

    include!(concat!(env!("OUT_DIR"), "/entity_crud.rs"));
    include!("dimension_model.rs.inc");
    include!("patch_helpers.rs.inc");

    // ════════════════════════════════════════════════════════════════════
    // Phase 1 — wiring the generic conversion functions above into `ocs`.
    //
    // `get_entity` is deliberately a *new*, separate function rather than a
    // change to `get()` above: existing scripts already depend on `get()`'s
    // small `{handle, kind, layer, point}` shape (Phase 1 review feedback —
    // don't break that compatibility to hand out the fuller schema).

    /// Full entity schema for `handle` (every field the type registry knows
    /// about, not just `get()`'s small `{handle, kind, layer, point}`), or
    /// `None` if the entity doesn't exist or isn't one of the kinds this
    /// generic API covers yet.
    #[pyfunction]
    fn get_entity(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found =
            host_ctx::with_host(|host| host.document().get_entity(Handle::new(handle)).cloned())
                .ok_or_else(|| {
                    vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
                })?;
        match found {
            Some(entity) => entity_to_dict(vm, &entity),
            None => Ok(vm.ctx.none()),
        }
    }

    /// API v7 document-model view: every entity has identity, kind and layer;
    /// only manifest-covered kinds also expose editable geometry properties.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn entity_descriptor(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found =
            host_ctx::with_host(|host| host.document().get_entity(Handle::new(handle)).cloned())
                .ok_or_else(|| {
                    vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
                })?;
        let Some(entity) = found else {
            return Ok(vm.ctx.none());
        };
        if let Ok(full) = entity_to_dict(vm, &entity) {
            let dict = full
                .clone()
                .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
            dict.set_item("_editable", vm.new_pyobj(true), vm)?;
            return Ok(full);
        }
        let dict = vm.ctx.new_dict();
        dict.set_item("handle", vm.new_pyobj(handle), vm)?;
        // `entity_type()` is the DXF name (for example `HATCH`), while the
        // coverage catalog uses the Rust `EntityType` variant (`Hatch`).
        let kind = ocs_plugin_api::entity_coverage::entity_snapshot(&entity)
            .ok()
            .and_then(|value| {
                value
                    .as_object()
                    .and_then(|object| object.keys().next().cloned())
            })
            .ok_or_else(|| {
                vm.new_runtime_error("ocs: cannot identify entity variant".to_owned())
            })?;
        dict.set_item("kind", vm.new_pyobj(kind), vm)?;
        dict.set_item("layer", vm.new_pyobj(entity.common().layer.clone()), vm)?;
        dict.set_item(
            "owner_handle",
            vm.new_pyobj(entity.common().owner_handle.value()),
            vm,
        )?;
        dict.set_item("_editable", vm.new_pyobj(false), vm)?;
        Ok(dict.into())
    }

    #[cfg(feature = "experimental-host-model")]
    fn snapshot_value_to_py(
        value: serde_json::Value,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        Ok(match value {
            serde_json::Value::Null => vm.ctx.none(),
            serde_json::Value::Bool(value) => vm.new_pyobj(value),
            serde_json::Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    vm.new_pyobj(value)
                } else if let Some(value) = value.as_u64() {
                    vm.new_pyobj(value)
                } else {
                    vm.new_pyobj(value.as_f64().expect("JSON number"))
                }
            }
            serde_json::Value::String(value) => vm.new_pyobj(value),
            serde_json::Value::Array(values) => vm
                .ctx
                .new_list(
                    values
                        .into_iter()
                        .map(|value| snapshot_value_to_py(value, vm))
                        .collect::<PyResult<Vec<_>>>()?,
                )
                .into(),
            serde_json::Value::Object(values) => {
                let dict = vm.ctx.new_dict();
                for (key, value) in values {
                    dict.set_item(&key, snapshot_value_to_py(value, vm)?, vm)?;
                }
                dict.into()
            }
        })
    }

    /// Copy every serializable field of an entity, including kinds outside
    /// the editable Python schema. The result is detached and read-only with
    /// respect to the drawing; mutations to the returned dict do not commit.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn entity_snapshot(handle: u64, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let found =
            host_ctx::with_host(|host| host.document().get_entity(Handle::new(handle)).cloned())
                .ok_or_else(|| {
                    vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
                })?;
        let Some(entity) = found else {
            return Ok(vm.ctx.none());
        };
        let value = ocs_plugin_api::entity_coverage::entity_snapshot(&entity).map_err(|error| {
            vm.new_runtime_error(format!("ocs: entity snapshot failed: {error}"))
        })?;
        snapshot_value_to_py(value, vm)
    }

    /// Host-owned coverage for a variant. `unmapped` properties exist in the
    /// typed snapshot but have no Python document-model getter or setter.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn entity_coverage(kind: String, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        use ocs_plugin_api::entity_coverage::{EntityCoverageCatalog, EntityScope, ModelAccess};
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(ocs_plugin_api::get_embedded_entity_coverage_json())
                .map_err(|error| vm.new_runtime_error(error.to_string()))?;
        let Some(entry) = catalog
            .entity_kinds
            .into_iter()
            .find(|entry| entry.kind == kind)
        else {
            return Ok(vm.ctx.none());
        };
        let row = vm.ctx.new_dict();
        row.set_item("kind", vm.new_pyobj(entry.kind), vm)?;
        let scope = match entry.scope {
            EntityScope::Canvas => "canvas",
            EntityScope::Internal => "internal",
            EntityScope::Opaque => "opaque",
        };
        row.set_item("scope", vm.new_pyobj(scope), vm)?;
        let mut readable = Vec::new();
        let mut editable = Vec::new();
        let mut unmapped = Vec::new();
        let mut properties = Vec::new();
        for property in entry.properties {
            let access = match property.model_access {
                ModelAccess::ReadWrite => {
                    readable.push(property.name.clone());
                    editable.push(property.name.clone());
                    "read_write"
                }
                ModelAccess::ReadOnly => {
                    readable.push(property.name.clone());
                    "read_only"
                }
                ModelAccess::Unmapped => {
                    unmapped.push(property.name.clone());
                    "unmapped"
                }
            };
            let item = vm.ctx.new_dict();
            item.set_item("name", vm.new_pyobj(property.name), vm)?;
            item.set_item("source_path", vm.new_pyobj(property.source_path), vm)?;
            item.set_item("type", vm.new_pyobj(property.type_id), vm)?;
            item.set_item("optional", vm.new_pyobj(property.optional), vm)?;
            item.set_item("sequence", vm.new_pyobj(property.is_sequence), vm)?;
            item.set_item(
                "snapshot_readable",
                vm.new_pyobj(property.snapshot_readable),
                vm,
            )?;
            item.set_item("access", vm.new_pyobj(access), vm)?;
            item.set_item("validation", vm.new_pyobj(property.validation), vm)?;
            properties.push(item.into());
        }
        let list = |values: Vec<String>| {
            vm.ctx.new_list(
                values
                    .into_iter()
                    .map(|value| vm.new_pyobj(value))
                    .collect(),
            )
        };
        row.set_item("readable", list(readable).into(), vm)?;
        row.set_item("editable", list(editable).into(), vm)?;
        row.set_item("unmapped", list(unmapped).into(), vm)?;
        row.set_item("properties", vm.ctx.new_list(properties).into(), vm)?;
        Ok(row.into())
    }

    /// Enumerate every kind in the host-owned coverage catalog.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn entity_kinds(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        use ocs_plugin_api::entity_coverage::EntityCoverageCatalog;
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(ocs_plugin_api::get_embedded_entity_coverage_json())
                .map_err(|error| vm.new_runtime_error(error.to_string()))?;
        Ok(vm
            .ctx
            .new_list(
                catalog
                    .entity_kinds
                    .into_iter()
                    .map(|entry| vm.new_pyobj(entry.kind))
                    .collect(),
            )
            .into())
    }

    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn entity_handles(vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let handles = host_ctx::with_host(|host| {
            host.document()
                .entities()
                .map(|entity| entity.common().handle.value())
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?;
        Ok(vm
            .ctx
            .new_list(
                handles
                    .into_iter()
                    .map(|handle| vm.new_pyobj(handle))
                    .collect(),
            )
            .into())
    }

    /// Add a new entity from a dict with a `"kind"` key (see `entity_to_dict`
    /// for the shape `get_entity`/this function share) — a generic
    /// alternative to `add_line`/`add_circle`/etc. covering every entity kind
    /// the type registry currently traces. Returns the new handle. Errors if
    /// `entity` is missing the geometry that actually defines the shape (a
    /// line's `start`/`end`, a circle's `center`/`radius`, ...) rather than
    /// silently defaulting it to zero.
    #[pyfunction]
    fn add(entity: PyObjectRef, vm: &VirtualMachine) -> PyResult<u64> {
        let dict = entity.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
        let built = dict_to_entity(&dict, vm)?;
        #[cfg(feature = "experimental-host-model")]
        ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&built)
            .map_err(|error| vm.new_value_error(error))?;
        host_ctx::with_host(|host| -> PyResult<u64> {
            #[allow(unused_mut)]
            let mut built = built;
            #[cfg(feature = "experimental-host-model")]
            {
                ocs_plugin_api::entity_coverage::bind_canvas_entity_references(
                    host.document(),
                    &mut built,
                )
                .map_err(|error| vm.new_value_error(error))?;
            }
            host_ctx::ensure_undo_started(host);
            Ok(host.add_entity(built).value())
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))?
    }

    /// Add a new entity, built from `entity` like `add`, to the block definition
    /// `block`. Returns the new entity's handle.
    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn add_to_block(block: String, entity: PyObjectRef, vm: &VirtualMachine) -> PyResult<u64> {
        let dict = entity.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
        let mut built = dict_to_entity(&dict, vm)?;
        let result = host_ctx::with_host(|host| -> PyResult<Result<Handle, String>> {
            let owner = host
                .document()
                .block_records
                .get(block.trim())
                .map(|record| record.handle)
                .ok_or_else(|| vm.new_runtime_error(format!("ocs.add_to_block: block {block:?} does not exist")))?;
            built.common_mut().owner_handle = owner;
            ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&built)
                .map_err(|error| vm.new_value_error(error))?;
            Ok(host.table_operation(ocs_plugin_api::host::TableOperation::BlockEntityAdd { block, entity: built }))
        })
        .ok_or_else(|| vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned()))??;
        result
            .map(|handle| handle.value())
            .map_err(|error| vm.new_runtime_error(format!("ocs.add_to_block: {error}")))
    }

    /// Update `handle` by merging `entity`'s keys onto its *current* value —
    /// a key `entity` doesn't mention is left exactly as it was, not reset to
    /// a type default (Phase 1 review feedback: reconstructing from a partial
    /// dict could silently drop properties this generic API can't represent
    /// at all, e.g. color/line-weight/XDATA). Errors if `handle` doesn't
    /// exist or isn't a supported kind.
    #[pyfunction]
    fn update(handle: u64, entity: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        let dict = entity.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
        let changed = host_ctx::with_host(|host| -> PyResult<bool> {
            let Some(existing) = host.document().get_entity(Handle::new(handle)).cloned() else {
                return Ok(false);
            };
            #[allow(unused_mut)]
            let mut updated = apply_dict_to_entity(&existing, &dict, vm)?;
            #[cfg(feature = "experimental-host-model")]
            {
                ocs_plugin_api::entity_coverage::bind_canvas_entity_references(
                    host.document(),
                    &mut updated,
                )
                .map_err(|error| vm.new_value_error(error))?;
                ocs_plugin_api::entity_coverage::validate_entity_mutation(&existing, &updated)
                    .map_err(|error| vm.new_value_error(error))?;
            }
            host_ctx::ensure_undo_started(host);
            Ok(host.update_entity(updated))
        })
        .ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })??;
        if !changed {
            return Err(vm.new_runtime_error(format!("ocs.update: entity {handle} does not exist")));
        }
        Ok(())
    }

    /// API v7: merge a list of property dictionaries and commit one undo step.
    /// The host validates the entire batch before changing the drawing.
    #[cfg(feature = "experimental-host-model")]
    fn apply_layer_only_patch(
        existing: &EntityType,
        dict: &rustpython_vm::builtins::PyDictRef,
        vm: &VirtualMachine,
    ) -> PyResult<EntityType> {
        if dict.keys_vec().len() != 2 || dict.get_item_opt("layer", vm)?.is_none() {
            return Err(vm.new_value_error(
                "ocs.update_many: this kind supports only a layer patch".to_owned(),
            ));
        }
        let layer = dict.get_item("layer", vm)?.try_into_value::<String>(vm)?;
        ocs_plugin_api::entity_coverage::patch_canvas_layer(existing, &layer)
            .map_err(|error| vm.new_value_error(format!("ocs.update_many: {error}")))
    }

    #[cfg(feature = "experimental-host-model")]
    #[pyfunction]
    fn update_many(label: String, updates: Vec<PyObjectRef>, vm: &VirtualMachine) -> PyResult<()> {
        let result = host_ctx::with_host(|host| -> PyResult<Result<(), String>> {
            let mut entities = Vec::with_capacity(updates.len());
            for patch in updates {
                let dict = patch.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
                let handle = dict.get_item("handle", vm)?.try_into_value::<u64>(vm)?;
                let existing = host
                    .document()
                    .get_entity(Handle::new(handle))
                    .cloned()
                    .ok_or_else(|| {
                        vm.new_runtime_error(format!("entity {handle} does not exist"))
                    })?;
                let changed = if entity_to_dict(vm, &existing).is_ok() {
                    apply_dict_to_entity(&existing, &dict, vm)?
                } else {
                    apply_layer_only_patch(&existing, &dict, vm)?
                };
                entities.push(changed);
            }
            Ok(host.update_entities_transaction(&label, entities))
        })
        .ok_or_else(|| {
            vm.new_runtime_error("ocs: not running inside a PY_ command".to_owned())
        })??;
        result.map_err(|error| vm.new_runtime_error(format!("ocs.update_many: {error}")))
    }

    #[cfg(test)]
    mod entity_crud_tests {
        use super::*;
        #[cfg(feature = "experimental-host-model")]
        use rustpython_vm::AsObject;

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn snapshot_conversion_preserves_nested_values() {
            with_vm(|vm| {
                let source = serde_json::json!({"Hatch": {"common": {"layer": "0"},
                    "angles": [1.5, 2.0], "optional": null, "enabled": true}});
                let converted = snapshot_value_to_py(source, vm).unwrap();
                let dict = converted
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                let hatch = dict
                    .get_item("Hatch", vm)
                    .unwrap()
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                let common = hatch
                    .get_item("common", vm)
                    .unwrap()
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert_eq!(
                    common
                        .get_item("layer", vm)
                        .unwrap()
                        .try_into_value::<String>(vm)
                        .unwrap(),
                    "0"
                );
                assert!(hatch.get_item("optional", vm).unwrap().is(&vm.ctx.none()));
                assert!(hatch
                    .get_item("enabled", vm)
                    .unwrap()
                    .try_into_value::<bool>(vm)
                    .unwrap());
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn unmapped_canvas_kind_accepts_only_layer_patch() {
            with_vm(|vm| {
                let entity = EntityType::MLine(codec::entities::MLine::default());
                let patch = vm.ctx.new_dict();
                patch.set_item("handle", vm.new_pyobj(1_u64), vm).unwrap();
                patch
                    .set_item("layer", vm.new_pyobj("HATCHES"), vm)
                    .unwrap();
                let changed = apply_layer_only_patch(&entity, &patch, vm).unwrap();
                assert_eq!(changed.common().layer, "HATCHES");
                patch.set_item("scale", vm.new_pyobj(2.0), vm).unwrap();
                assert!(apply_layer_only_patch(&entity, &patch, vm).is_err());
            });
        }

        fn with_vm<R>(f: impl FnOnce(&VirtualMachine) -> R) -> R {
            let interp = rustpython_vm::Interpreter::without_stdlib(Default::default());
            interp.enter(f)
        }

        fn set_vector3(
            vm: &VirtualMachine,
            dict: &rustpython_vm::builtins::PyDictRef,
            key: &str,
            x: f64,
            y: f64,
            z: f64,
        ) {
            let v = vm.ctx.new_dict();
            v.set_item("x", vm.new_pyobj(x), vm).unwrap();
            v.set_item("y", vm.new_pyobj(y), vm).unwrap();
            v.set_item("z", vm.new_pyobj(z), vm).unwrap();
            dict.set_item(key, v.into(), vm).unwrap();
        }

        #[cfg(feature = "experimental-host-model")]
        fn vector2(vm: &VirtualMachine, x: f64, y: f64) -> PyObjectRef {
            let value = vm.ctx.new_dict();
            value.set_item("x", vm.new_pyobj(x), vm).unwrap();
            value.set_item("y", vm.new_pyobj(y), vm).unwrap();
            value.into()
        }

        #[cfg(feature = "experimental-host-model")]
        fn hatch_boundary(vm: &VirtualMachine, size: f64) -> PyObjectRef {
            let edges = [
                (0.0, 0.0, size, 0.0),
                (size, 0.0, size, size),
                (size, size, 0.0, size),
                (0.0, size, 0.0, 0.0),
            ]
            .into_iter()
            .map(|(x1, y1, x2, y2)| {
                let line = vm.ctx.new_dict();
                line.set_item("start", vector2(vm, x1, y1), vm).unwrap();
                line.set_item("end", vector2(vm, x2, y2), vm).unwrap();
                let edge = vm.ctx.new_dict();
                edge.set_item("kind", vm.new_pyobj("Line"), vm).unwrap();
                edge.set_item("value", line.into(), vm).unwrap();
                edge.into()
            })
            .collect();
            let path = vm.ctx.new_dict();
            path.set_item("flags", vm.new_pyobj(1_i64), vm).unwrap();
            path.set_item("edges", vm.ctx.new_list(edges).into(), vm)
                .unwrap();
            path.set_item("boundary_handles", vm.ctx.new_list(Vec::new()).into(), vm)
                .unwrap();
            path.into()
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn phase_seven_kinds_create_and_preserve_unmentioned_properties() {
            with_vm(|vm| {
                for kind in ["Ray", "XLine", "Solid", "Face3D"] {
                    let input = vm.ctx.new_dict();
                    input.set_item("kind", vm.new_pyobj(kind), vm).unwrap();
                    input
                        .set_item("layer", vm.new_pyobj("Geometry"), vm)
                        .unwrap();
                    match kind {
                        "Ray" | "XLine" => {
                            set_vector3(vm, &input, "base_point", 1.0, 2.0, 0.0);
                            set_vector3(vm, &input, "direction", 1.0, 0.0, 0.0);
                        }
                        _ => {
                            for (name, x, y) in [
                                ("first_corner", 0.0, 0.0),
                                ("second_corner", 1.0, 0.0),
                                ("third_corner", 0.0, 1.0),
                                ("fourth_corner", 1.0, 1.0),
                            ] {
                                set_vector3(vm, &input, name, x, y, 0.0);
                            }
                        }
                    }
                    if kind == "Solid" {
                        input.set_item("is_trace", vm.new_pyobj(true), vm).unwrap();
                    }
                    if kind == "Face3D" {
                        input
                            .set_item("invisible_edges", vm.new_pyobj(5_i64), vm)
                            .unwrap();
                    }
                    let original = dict_to_entity(&input, vm).unwrap();
                    ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&original).unwrap();
                    let rendered = entity_to_dict(vm, &original)
                        .unwrap()
                        .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                        .unwrap();
                    assert_eq!(
                        rendered
                            .get_item("kind", vm)
                            .unwrap()
                            .try_into_value::<String>(vm)
                            .unwrap(),
                        kind
                    );
                    let patch = vm.ctx.new_dict();
                    let changed_field = if matches!(kind, "Ray" | "XLine") {
                        "base_point"
                    } else {
                        "first_corner"
                    };
                    set_vector3(vm, &patch, changed_field, 7.0, 8.0, 0.0);
                    let updated = apply_dict_to_entity(&original, &patch, vm).unwrap();
                    ocs_plugin_api::entity_coverage::validate_entity_mutation(&original, &updated)
                        .unwrap();
                    let updated_dict = entity_to_dict(vm, &updated)
                        .unwrap()
                        .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                        .unwrap();
                    assert_eq!(
                        updated_dict
                            .get_item("layer", vm)
                            .unwrap()
                            .try_into_value::<String>(vm)
                            .unwrap(),
                        "Geometry"
                    );
                    if kind == "Solid" {
                        assert!(updated_dict
                            .get_item("is_trace", vm)
                            .unwrap()
                            .try_into_value::<bool>(vm)
                            .unwrap());
                    }
                    if kind == "Face3D" {
                        assert_eq!(
                            updated_dict
                                .get_item("invisible_edges", vm)
                                .unwrap()
                                .try_into_value::<i64>(vm)
                                .unwrap(),
                            5
                        );
                    }
                }
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn face3d_flags_reject_integer_overflow() {
            with_vm(|vm| {
                let input = vm.ctx.new_dict();
                input.set_item("kind", vm.new_pyobj("Face3D"), vm).unwrap();
                for name in [
                    "first_corner",
                    "second_corner",
                    "third_corner",
                    "fourth_corner",
                ] {
                    set_vector3(vm, &input, name, 0.0, 0.0, 0.0);
                }
                input
                    .set_item("invisible_edges", vm.new_pyobj(256_i64), vm)
                    .unwrap();
                assert!(dict_to_entity(&input, vm).is_err());
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn insert_is_created_by_block_name_and_preserves_block_identity() {
            with_vm(|vm| {
                let mut insert =
                    codec::entities::Insert::new("DOOR", Vector3::new(1.0, 2.0, 0.0));
                insert.set_x_scale(2.0);
                let original = EntityType::Insert(insert);
                let rendered = entity_to_dict(vm, &original)
                    .unwrap()
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert_eq!(
                    rendered
                        .get_item("block_name", vm)
                        .unwrap()
                        .try_into_value::<String>(vm)
                        .unwrap(),
                    "DOOR"
                );
                assert_eq!(
                    rendered
                        .get_item("x_scale", vm)
                        .unwrap()
                        .try_into_value::<f64>(vm)
                        .unwrap(),
                    2.0
                );
                // A round-tripped dict names an existing block, so it creates one.
                let recreated = dict_to_entity(&rendered, vm).unwrap();
                assert!(matches!(&recreated, EntityType::Insert(value)
                    if value.block_name == "DOOR" && value.x_scale() == 2.0));
                // Creation must name the block.
                let unnamed = vm.ctx.new_dict();
                unnamed.set_item("kind", vm.new_pyobj("Insert"), vm).unwrap();
                assert!(dict_to_entity(&unnamed, vm).is_err());

                let patch = vm.ctx.new_dict();
                set_vector3(vm, &patch, "insert_point", 4.0, 5.0, 0.0);
                patch.set_item("y_scale", vm.new_pyobj(3.0), vm).unwrap();
                let updated = apply_dict_to_entity(&original, &patch, vm).unwrap();
                ocs_plugin_api::entity_coverage::validate_entity_mutation(&original, &updated)
                    .unwrap();
                let EntityType::Insert(updated) = updated else {
                    panic!("expected Insert");
                };
                assert_eq!(updated.block_name, "DOOR");
                assert_eq!(updated.insert_point, Vector3::new(4.0, 5.0, 0.0));
                assert_eq!(updated.x_scale(), 2.0);
                assert_eq!(updated.y_scale(), 3.0);

                patch
                    .set_item("block_name", vm.new_pyobj("OTHER"), vm)
                    .unwrap();
                // The adapter only converts; the host mutation check refuses a
                // different block, so an edit cannot swap block identity.
                let renamed = apply_dict_to_entity(&original, &patch, vm).unwrap();
                assert!(ocs_plugin_api::entity_coverage::validate_entity_mutation(&original, &renamed).is_err());
                patch.del_item("block_name", vm).unwrap();
                patch.set_item("z_scale", vm.new_pyobj(0.0), vm).unwrap();
                assert!(apply_dict_to_entity(&original, &patch, vm).is_err());
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn tolerance_converts_and_style_change_clears_stale_handle() {
            with_vm(|vm| {
                let input = vm.ctx.new_dict();
                input
                    .set_item("kind", vm.new_pyobj("Tolerance"), vm)
                    .unwrap();
                set_vector3(vm, &input, "insertion_point", 1.0, 2.0, 0.0);
                set_vector3(vm, &input, "direction", 1.0, 0.0, 0.0);
                input
                    .set_item("text", vm.new_pyobj("{\\Fgdt;p}%%v0.1"), vm)
                    .unwrap();
                input
                    .set_item("dimension_style_name", vm.new_pyobj("Standard"), vm)
                    .unwrap();
                let original = dict_to_entity(&input, vm).unwrap();
                ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&original).unwrap();
                let EntityType::Tolerance(mut with_handle) = original.clone() else {
                    panic!("expected Tolerance");
                };
                with_handle.dimension_style_handle = Some(Handle::new(42));
                let original = EntityType::Tolerance(with_handle);
                let patch = vm.ctx.new_dict();
                patch
                    .set_item("text", vm.new_pyobj("{\\Fgdt;p}%%v0.2"), vm)
                    .unwrap();
                patch
                    .set_item("dimension_style_name", vm.new_pyobj("ISO-25"), vm)
                    .unwrap();
                let updated = apply_dict_to_entity(&original, &patch, vm).unwrap();
                let EntityType::Tolerance(updated) = updated else {
                    panic!("expected Tolerance");
                };
                assert_eq!(updated.text, "{\\Fgdt;p}%%v0.2");
                assert_eq!(updated.dimension_style_name, "ISO-25");
                assert_eq!(updated.dimension_style_handle, None);
                assert_eq!(updated.insertion_point, Vector3::new(1.0, 2.0, 0.0));
                let rendered = entity_to_dict(vm, &EntityType::Tolerance(updated))
                    .unwrap()
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert!(rendered
                    .get_item_opt("dimension_style_handle", vm)
                    .unwrap()
                    .is_some());
                assert!(rendered
                    .get_item_opt("dwg_unknown_short", vm)
                    .unwrap()
                    .is_none());
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn shape_converts_and_style_change_clears_stale_handle() {
            with_vm(|vm| {
                let input = vm.ctx.new_dict();
                input.set_item("kind", vm.new_pyobj("Shape"), vm).unwrap();
                set_vector3(vm, &input, "insertion_point", 1.0, 2.0, 0.0);
                input.set_item("size", vm.new_pyobj(2.0), vm).unwrap();
                input
                    .set_item("shape_name", vm.new_pyobj("ARROW"), vm)
                    .unwrap();
                input
                    .set_item("shape_number", vm.new_pyobj(1_i32), vm)
                    .unwrap();
                input
                    .set_item("style_name", vm.new_pyobj("Symbols"), vm)
                    .unwrap();
                let original = dict_to_entity(&input, vm).unwrap();
                ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&original).unwrap();
                let EntityType::Shape(mut with_handle) = original.clone() else {
                    panic!("expected Shape");
                };
                with_handle.style_handle = Some(Handle::new(42));
                let original = EntityType::Shape(with_handle);
                let patch = vm.ctx.new_dict();
                patch.set_item("size", vm.new_pyobj(3.0), vm).unwrap();
                patch
                    .set_item("style_name", vm.new_pyobj("OtherSymbols"), vm)
                    .unwrap();
                let updated = apply_dict_to_entity(&original, &patch, vm).unwrap();
                let EntityType::Shape(updated) = updated else {
                    panic!("expected Shape");
                };
                assert_eq!(updated.size, 3.0);
                assert_eq!(updated.shape_name, "ARROW");
                assert_eq!(updated.shape_number, 1);
                assert_eq!(updated.style_name, "OtherSymbols");
                assert_eq!(updated.style_handle, None);
                assert_eq!(updated.insertion_point, Vector3::new(1.0, 2.0, 0.0));
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn attribute_definition_converts_owner_and_preserves_embedded_mtext() {
            with_vm(|vm| {
                let input = vm.ctx.new_dict();
                input
                    .set_item("kind", vm.new_pyobj("AttributeDefinition"), vm)
                    .unwrap();
                input
                    .set_item("owner_handle", vm.new_pyobj(42_u64), vm)
                    .unwrap();
                input.set_item("tag", vm.new_pyobj("PART_NO"), vm).unwrap();
                input
                    .set_item("prompt", vm.new_pyobj("Part number"), vm)
                    .unwrap();
                input
                    .set_item("default_value", vm.new_pyobj("PN-001"), vm)
                    .unwrap();
                set_vector3(vm, &input, "insertion_point", 1.0, 2.0, 0.0);
                input.set_item("height", vm.new_pyobj(2.5), vm).unwrap();
                let entity = dict_to_entity(&input, vm).unwrap();
                let EntityType::AttributeDefinition(mut definition) = entity else {
                    panic!("expected AttributeDefinition");
                };
                assert_eq!(definition.common.owner_handle, Handle::new(42));
                assert_eq!(definition.tag, "PART_NO");
                assert_eq!(definition.text_style, "Standard");

                let mut embedded = codec::entities::MText::default();
                embedded.value = "first\\Psecond".into();
                definition.embedded_mtext = Some(Box::new(embedded));
                let entity = EntityType::AttributeDefinition(definition);
                let patch = vm.ctx.new_dict();
                patch
                    .set_item("default_value", vm.new_pyobj("PN-002"), vm)
                    .unwrap();
                set_vector3(vm, &patch, "insertion_point", 4.0, 5.0, 0.0);
                let updated = apply_dict_to_entity(&entity, &patch, vm).unwrap();
                let EntityType::AttributeDefinition(updated) = updated else {
                    panic!("expected AttributeDefinition");
                };
                assert_eq!(updated.default_value, "PN-002");
                assert_eq!(updated.insertion_point, Vector3::new(4.0, 5.0, 0.0));
                assert_eq!(
                    updated.embedded_mtext.as_ref().unwrap().value,
                    "first\\Psecond"
                );

                let rendered = entity_to_dict(vm, &EntityType::AttributeDefinition(updated))
                    .unwrap()
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert_eq!(
                    rendered
                        .get_item("owner_handle", vm)
                        .unwrap()
                        .try_into_value::<u64>(vm)
                        .unwrap(),
                    42
                );
                assert!(rendered
                    .get_item_opt("embedded_mtext", vm)
                    .unwrap()
                    .is_none());

                let owner_patch = vm.ctx.new_dict();
                owner_patch
                    .set_item("owner_handle", vm.new_pyobj(43_u64), vm)
                    .unwrap();
                assert!(apply_dict_to_entity(&entity, &owner_patch, vm)
                    .unwrap_err()
                    .args()
                    .as_slice()[0]
                    .clone()
                    .try_into_value::<String>(vm)
                    .unwrap()
                    .contains("read-only after creation"));
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn attribute_entity_converts_nested_identity_and_preserves_embedded_mtext() {
            with_vm(|vm| {
                let input = vm.ctx.new_dict();
                input
                    .set_item("kind", vm.new_pyobj("AttributeEntity"), vm)
                    .unwrap();
                input
                    .set_item("owner_handle", vm.new_pyobj(50_u64), vm)
                    .unwrap();
                input.set_item("tag", vm.new_pyobj("PART_NO"), vm).unwrap();
                input.set_item("value", vm.new_pyobj("PN-101"), vm).unwrap();
                set_vector3(vm, &input, "insertion_point", 10.0, 2.0, 0.0);
                input.set_item("height", vm.new_pyobj(2.5), vm).unwrap();
                let entity = dict_to_entity(&input, vm).unwrap();
                let EntityType::AttributeEntity(mut attribute) = entity else {
                    panic!("expected AttributeEntity");
                };
                assert_eq!(attribute.common.owner_handle, Handle::new(50));
                attribute.attdef_handle = Handle::new(42);
                let mut embedded = codec::entities::MText::default();
                embedded.value = "first\\Psecond".into();
                attribute.embedded_mtext = Some(Box::new(embedded));
                let entity = EntityType::AttributeEntity(attribute);

                let patch = vm.ctx.new_dict();
                patch.set_item("value", vm.new_pyobj("PN-102"), vm).unwrap();
                set_vector3(vm, &patch, "insertion_point", 12.0, 3.0, 0.0);
                let updated_entity = apply_dict_to_entity(&entity, &patch, vm).unwrap();
                let EntityType::AttributeEntity(updated) = &updated_entity else {
                    unreachable!()
                };
                assert_eq!(updated.value, "PN-102");
                assert_eq!(updated.attdef_handle, Handle::new(42));
                assert_eq!(
                    updated.embedded_mtext.as_ref().unwrap().value,
                    "first\\Psecond"
                );

                let tag_patch = vm.ctx.new_dict();
                tag_patch
                    .set_item("tag", vm.new_pyobj("SERIAL"), vm)
                    .unwrap();
                let retagged = apply_dict_to_entity(&updated_entity, &tag_patch, vm).unwrap();
                assert!(matches!(retagged, EntityType::AttributeEntity(value)
                    if value.tag == "SERIAL" && value.attdef_handle == Handle::new(42)));
                let rendered = entity_to_dict(vm, &updated_entity)
                    .unwrap()
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert_eq!(
                    rendered
                        .get_item("attdef_handle", vm)
                        .unwrap()
                        .try_into_value::<u64>(vm)
                        .unwrap(),
                    42
                );
                assert!(rendered
                    .get_item_opt("embedded_mtext", vm)
                    .unwrap()
                    .is_none());
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn hatch_converts_tagged_boundaries_patterns_and_preserves_gradient() {
            with_vm(|vm| {
                let input = vm.ctx.new_dict();
                input.set_item("kind", vm.new_pyobj("Hatch"), vm).unwrap();
                input
                    .set_item(
                        "paths",
                        vm.ctx.new_list(vec![hatch_boundary(vm, 10.0)]).into(),
                        vm,
                    )
                    .unwrap();
                let entity = dict_to_entity(&input, vm).unwrap();
                ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&entity).unwrap();
                let EntityType::Hatch(mut hatch) = entity else {
                    panic!("expected Hatch")
                };
                hatch.gradient_color.enabled = true;
                hatch.gradient_color.name = "LINEAR".into();
                let entity = EntityType::Hatch(hatch);

                let pattern_line = vm.ctx.new_dict();
                pattern_line
                    .set_item("angle", vm.new_pyobj(0.0), vm)
                    .unwrap();
                pattern_line
                    .set_item("base_point", vector2(vm, 0.0, 0.0), vm)
                    .unwrap();
                pattern_line
                    .set_item("offset", vector2(vm, 0.0, 2.0), vm)
                    .unwrap();
                pattern_line
                    .set_item(
                        "dash_lengths",
                        vm.ctx
                            .new_list(vec![vm.new_pyobj(1.0), vm.new_pyobj(-1.0)])
                            .into(),
                        vm,
                    )
                    .unwrap();
                let pattern = vm.ctx.new_dict();
                pattern.set_item("name", vm.new_pyobj("TEST"), vm).unwrap();
                pattern
                    .set_item("description", vm.new_pyobj("test pattern"), vm)
                    .unwrap();
                pattern
                    .set_item(
                        "lines",
                        vm.ctx.new_list(vec![pattern_line.into()]).into(),
                        vm,
                    )
                    .unwrap();
                let patch = vm.ctx.new_dict();
                patch.set_item("is_solid", vm.new_pyobj(false), vm).unwrap();
                patch.set_item("pattern", pattern.into(), vm).unwrap();
                patch
                    .set_item("pattern_scale", vm.new_pyobj(2.0), vm)
                    .unwrap();
                patch
                    .set_item(
                        "paths",
                        vm.ctx.new_list(vec![hatch_boundary(vm, 12.0)]).into(),
                        vm,
                    )
                    .unwrap();
                let updated = apply_dict_to_entity(&entity, &patch, vm).unwrap();
                ocs_plugin_api::entity_coverage::validate_entity_mutation(&entity, &updated)
                    .unwrap();
                let EntityType::Hatch(updated) = updated else {
                    unreachable!()
                };
                assert!(!updated.is_solid);
                assert_eq!(updated.pattern.name, "TEST");
                assert_eq!(updated.pattern.lines.len(), 1);
                assert_eq!(updated.paths[0].edges.len(), 4);
                assert!(updated.gradient_color.enabled);
                let rendered = entity_to_dict(vm, &EntityType::Hatch(updated))
                    .unwrap()
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert!(rendered
                    .get_item_opt("gradient_color", vm)
                    .unwrap()
                    .is_none());
                assert_eq!(
                    rendered
                        .get_item("paths", vm)
                        .unwrap()
                        .try_into_value::<Vec<PyObjectRef>>(vm)
                        .unwrap()
                        .len(),
                    1
                );
                let forbidden = vm.ctx.new_dict();
                forbidden
                    .set_item("gradient_color", vm.ctx.new_dict().into(), vm)
                    .unwrap();
                assert!(apply_dict_to_entity(&entity, &forbidden, vm).is_err());
            });
        }

        #[cfg(feature = "experimental-host-model")]
        #[test]
        fn phase_seven_kinds_survive_dwg_round_trip() {
            with_vm(|vm| {
                let mut doc = codec::CadDocument::new();
                let mut handles = Vec::new();
                for kind in ["Ray", "XLine", "Solid", "Face3D"] {
                    let input = vm.ctx.new_dict();
                    input.set_item("kind", vm.new_pyobj(kind), vm).unwrap();
                    if matches!(kind, "Ray" | "XLine") {
                        set_vector3(vm, &input, "base_point", 2.0, 3.0, 0.0);
                        set_vector3(vm, &input, "direction", 1.0, 0.0, 0.0);
                    } else {
                        for (name, x, y) in [
                            ("first_corner", 0.0, 0.0),
                            ("second_corner", 1.0, 0.0),
                            ("third_corner", 0.0, 1.0),
                            ("fourth_corner", 1.0, 1.0),
                        ] {
                            set_vector3(vm, &input, name, x, y, 0.0);
                        }
                    }
                    let entity = dict_to_entity(&input, vm).unwrap();
                    ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&entity).unwrap();
                    handles.push((kind, doc.add_entity(entity).unwrap()));
                }
                let path = std::env::temp_dir()
                    .join(format!("ocs_phase_seven_{}.dwg", std::process::id()));
                codec::DwgWriter::write_to_file(&path, &doc).unwrap();
                let reopened = codec::DwgReader::from_file(&path)
                    .unwrap()
                    .read()
                    .unwrap();
                let _ = std::fs::remove_file(&path);
                for (kind, handle) in handles {
                    let entity = reopened.get_entity(handle).expect("entity survives reopen");
                    let rendered = entity_to_dict(vm, entity)
                        .unwrap()
                        .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                        .unwrap();
                    assert_eq!(
                        rendered
                            .get_item("kind", vm)
                            .unwrap()
                            .try_into_value::<String>(vm)
                            .unwrap(),
                        kind
                    );
                }
            });
        }

        #[test]
        fn text_is_in_editable_schema_and_keeps_unmentioned_fields() {
            with_vm(|vm| {
                let mut text = codec::entities::Text::new();
                text.value = "Before".into();
                text.height = 3.5;
                text.insertion_point = Vector3::new(1.0, 2.0, 0.0);
                let entity = EntityType::Text(text);
                let data = entity_to_dict(vm, &entity).unwrap();
                let data = data
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert_eq!(
                    data.get_item("text", vm)
                        .unwrap()
                        .try_into_value::<String>(vm)
                        .unwrap(),
                    "Before"
                );
                let patch = vm.ctx.new_dict();
                patch.set_item("text", vm.new_pyobj("After"), vm).unwrap();
                let updated = apply_dict_to_entity(&entity, &patch, vm).unwrap();
                let EntityType::Text(updated) = updated else {
                    panic!("expected text");
                };
                assert_eq!(updated.value, "After");
                assert_eq!(updated.height, 3.5);
                assert_eq!(updated.insertion_point, Vector3::new(1.0, 2.0, 0.0));
            });
        }

        #[test]
        fn circle_round_trips_through_dict_to_entity_and_back() {
            with_vm(|vm| {
                let dict = vm.ctx.new_dict();
                dict.set_item("kind", vm.new_pyobj("Circle"), vm).unwrap();
                dict.set_item("layer", vm.new_pyobj("Walls"), vm).unwrap();
                set_vector3(vm, &dict, "center", 1.0, 2.0, 0.0);
                dict.set_item("radius", vm.new_pyobj(5.0), vm).unwrap();

                let entity = dict_to_entity(&dict, vm).expect("dict_to_entity");
                let codec::EntityType::Circle(circle) = &entity else {
                    panic!("expected Circle, got {entity:?}");
                };
                assert_eq!(circle.common.layer, "Walls");
                assert_eq!(circle.center, Vector3::new(1.0, 2.0, 0.0));
                assert_eq!(circle.radius, 5.0);

                let round_tripped = entity_to_dict(vm, &entity).expect("entity_to_dict");
                let round_tripped = round_tripped
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert_eq!(
                    round_tripped
                        .get_item("kind", vm)
                        .unwrap()
                        .try_into_value::<String>(vm)
                        .unwrap(),
                    "Circle"
                );
                assert_eq!(
                    round_tripped
                        .get_item("radius", vm)
                        .unwrap()
                        .try_into_value::<f64>(vm)
                        .unwrap(),
                    5.0
                );
            });
        }

        #[test]
        fn add_rejects_missing_required_geometry_instead_of_defaulting_to_zero() {
            with_vm(|vm| {
                // Phase 1 review feedback: a Circle with no center/radius must
                // error, not silently become a zero-radius circle at the origin.
                let dict = vm.ctx.new_dict();
                dict.set_item("kind", vm.new_pyobj("Circle"), vm).unwrap();
                let err = dict_to_entity(&dict, vm).expect_err("missing radius/center must error");
                let message = err.args().as_slice()[0]
                    .clone()
                    .try_into_value::<String>(vm)
                    .unwrap();
                assert!(
                    message.contains("requires"),
                    "unexpected message: {message}"
                );

                let dict = vm.ctx.new_dict();
                dict.set_item("kind", vm.new_pyobj("Polyline2D"), vm).unwrap();
                let err = dict_to_entity(&dict, vm).expect_err("missing vertices must error");
                let message = err.args().as_slice()[0]
                    .clone()
                    .try_into_value::<String>(vm)
                    .unwrap();
                assert!(
                    message.contains("at least one"),
                    "unexpected message: {message}"
                );

                // An explicitly empty vertex list is exactly as invalid as an
                // absent one — both parse to the same empty Vec.
                let dict = vm.ctx.new_dict();
                dict.set_item("kind", vm.new_pyobj("Polyline2D"), vm).unwrap();
                dict.set_item("vertices", vm.ctx.new_list(vec![]).into(), vm)
                    .unwrap();
                assert!(dict_to_entity(&dict, vm).is_err());
            });
        }

        #[test]
        fn legacy_polyline_creation_is_refused_with_a_hint() {
            with_vm(|vm| {
                let dict = vm.ctx.new_dict();
                dict.set_item("kind", vm.new_pyobj("Polyline"), vm).unwrap();
                let err = dict_to_entity(&dict, vm).expect_err("legacy Polyline is update-only");
                let message = err.args().as_slice()[0]
                    .clone()
                    .try_into_value::<String>(vm)
                    .unwrap();
                assert!(message.contains("Polyline2D or Polyline3D"), "{message}");
            });
        }

        #[test]
        fn polyline_flags_override_exposes_closed_as_a_plain_bool() {
            with_vm(|vm| {
                let dict = vm.ctx.new_dict();
                dict.set_item("kind", vm.new_pyobj("Polyline2D"), vm).unwrap();
                dict.set_item("closed", vm.new_pyobj(true), vm).unwrap();
                let vertex = vm.ctx.new_dict();
                let location = vm.ctx.new_dict();
                location.set_item("x", vm.new_pyobj(0.0), vm).unwrap();
                location.set_item("y", vm.new_pyobj(0.0), vm).unwrap();
                location.set_item("z", vm.new_pyobj(0.0), vm).unwrap();
                vertex.set_item("location", location.into(), vm).unwrap();
                dict.set_item("vertices", vm.ctx.new_list(vec![vertex.into()]).into(), vm)
                    .unwrap();

                let entity = dict_to_entity(&dict, vm).expect("dict_to_entity");
                let codec::EntityType::Polyline2D(polyline) = &entity else {
                    panic!("expected Polyline2D, got {entity:?}");
                };
                assert!(polyline.flags.is_closed());
                assert_eq!(polyline.vertices.len(), 1);

                let round_tripped = entity_to_dict(vm, &entity).expect("entity_to_dict");
                let round_tripped = round_tripped
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert!(round_tripped
                    .get_item("closed", vm)
                    .unwrap()
                    .try_into_value::<bool>(vm)
                    .unwrap());
            });
        }

        #[test]
        fn mtext_round_trips_unit_enum_and_excluded_fields() {
            with_vm(|vm| {
                let dict = vm.ctx.new_dict();
                dict.set_item("kind", vm.new_pyobj("MText"), vm).unwrap();
                dict.set_item("text", vm.new_pyobj("hello"), vm).unwrap();
                set_vector3(vm, &dict, "insertion", 0.0, 0.0, 0.0);
                dict.set_item("attachment_point", vm.new_pyobj("MiddleCenter"), vm)
                    .unwrap();

                let entity = dict_to_entity(&dict, vm).expect("dict_to_entity");
                let codec::EntityType::MText(text) = &entity else {
                    panic!("expected MText, got {entity:?}");
                };
                assert_eq!(text.value, "hello");
                assert_eq!(
                    text.attachment_point,
                    codec::entities::AttachmentPoint::MiddleCenter
                );

                let round_tripped = entity_to_dict(vm, &entity).expect("entity_to_dict");
                let round_tripped = round_tripped
                    .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                    .unwrap();
                assert_eq!(
                    round_tripped
                        .get_item("attachment_point", vm)
                        .unwrap()
                        .try_into_value::<String>(vm)
                        .unwrap(),
                    "MiddleCenter"
                );
                assert!(round_tripped
                    .get_item_opt("background_color", vm)
                    .unwrap()
                    .is_none());
            });
        }

        #[test]
        fn update_preserves_fields_the_dict_does_not_mention() {
            with_vm(|vm| {
                let add_dict = vm.ctx.new_dict();
                add_dict.set_item("kind", vm.new_pyobj("Line"), vm).unwrap();
                add_dict
                    .set_item("layer", vm.new_pyobj("Geometry"), vm)
                    .unwrap();
                set_vector3(vm, &add_dict, "start", 0.0, 0.0, 0.0);
                set_vector3(vm, &add_dict, "end", 10.0, 0.0, 0.0);
                let existing = dict_to_entity(&add_dict, vm).expect("dict_to_entity");

                // Only "end" is mentioned — "start" and "layer" must survive
                // untouched, unlike a fresh `dict_to_entity` from this same
                // partial dict (which would reset them to their type defaults).
                let update_dict = vm.ctx.new_dict();
                set_vector3(vm, &update_dict, "end", 20.0, 5.0, 0.0);
                let updated = apply_dict_to_entity(&existing, &update_dict, vm)
                    .expect("apply_dict_to_entity");

                let codec::EntityType::Line(line) = &updated else {
                    panic!("expected Line, got {updated:?}");
                };
                assert_eq!(line.common.layer, "Geometry", "layer must be preserved");
                assert_eq!(
                    line.start,
                    Vector3::new(0.0, 0.0, 0.0),
                    "start must be preserved"
                );
                assert_eq!(
                    line.end,
                    Vector3::new(20.0, 5.0, 0.0),
                    "end must be updated"
                );

                // A second update, this time touching the layer, must not
                // disturb the geometry it doesn't mention either.
                let update_dict = vm.ctx.new_dict();
                update_dict
                    .set_item("layer", vm.new_pyobj("Renamed"), vm)
                    .unwrap();
                let updated =
                    apply_dict_to_entity(&updated, &update_dict, vm).expect("apply_dict_to_entity");
                let codec::EntityType::Line(line) = &updated else {
                    panic!("expected Line, got {updated:?}");
                };
                assert_eq!(line.common.layer, "Renamed");
                assert_eq!(line.start, Vector3::new(0.0, 0.0, 0.0));
                assert_eq!(line.end, Vector3::new(20.0, 5.0, 0.0));
            });
        }

        /// The concrete Phase 1 acceptance test: create, read,
        /// edit, delete, save, and reopen a Line and an Ellipse, with common
        /// properties preserved — exercised against a real
        /// `codec::CadDocument` and a real DWG round trip (not a fake
        /// `HostApi`; `dict_to_entity`/`apply_dict_to_entity`/`entity_to_dict`
        /// are exactly what a live `HostApi`-backed `ocs.add`/`ocs.update`/
        /// `ocs.get_entity` call into, so this covers the same logic without
        /// needing a from-scratch test double for the whole `HostApi` trait).
        #[test]
        fn line_and_ellipse_survive_create_read_edit_delete_save_reopen() {
            with_vm(|vm| {
                let mut doc = codec::CadDocument::new();
                // A layer must exist in the document's layer table before an
                // entity can reference it — same requirement `set_entity_layer`
                // above already enforces for a live document. Without this,
                // the DWG writer silently falls back the entity to layer "0"
                // instead of erroring, which this test caught the first time
                // around (it used "Geometry" without registering it first).
                doc.layers.add(codec::Layer::new("Geometry")).unwrap();

                // --- CREATE ---
                let add_line = vm.ctx.new_dict();
                add_line.set_item("kind", vm.new_pyobj("Line"), vm).unwrap();
                add_line
                    .set_item("layer", vm.new_pyobj("Geometry"), vm)
                    .unwrap();
                set_vector3(vm, &add_line, "start", 0.0, 0.0, 0.0);
                set_vector3(vm, &add_line, "end", 10.0, 5.0, 0.0);
                let line_handle = doc
                    .add_entity(dict_to_entity(&add_line, vm).expect("dict_to_entity Line"))
                    .expect("doc.add_entity Line");

                let add_ellipse = vm.ctx.new_dict();
                add_ellipse
                    .set_item("kind", vm.new_pyobj("Ellipse"), vm)
                    .unwrap();
                add_ellipse
                    .set_item("layer", vm.new_pyobj("Geometry"), vm)
                    .unwrap();
                set_vector3(vm, &add_ellipse, "center", 1.0, 2.0, 0.0);
                set_vector3(vm, &add_ellipse, "major_axis", 5.0, 0.0, 0.0);
                add_ellipse
                    .set_item("minor_axis_ratio", vm.new_pyobj(0.5), vm)
                    .unwrap();
                let ellipse_handle = doc
                    .add_entity(dict_to_entity(&add_ellipse, vm).expect("dict_to_entity Ellipse"))
                    .expect("doc.add_entity Ellipse");

                // --- READ ---
                let line_dict =
                    entity_to_dict(vm, doc.get_entity(line_handle).expect("line exists"))
                        .expect("entity_to_dict Line");
                assert_eq!(
                    line_dict
                        .clone()
                        .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)
                        .unwrap()
                        .get_item("layer", vm)
                        .unwrap()
                        .try_into_value::<String>(vm)
                        .unwrap(),
                    "Geometry"
                );

                // --- EDIT (update-merge: only "end" changes) ---
                let existing = doc.get_entity(line_handle).unwrap().clone();
                let edit_dict = vm.ctx.new_dict();
                set_vector3(vm, &edit_dict, "end", 20.0, 5.0, 0.0);
                let updated = apply_dict_to_entity(&existing, &edit_dict, vm)
                    .expect("apply_dict_to_entity Line");
                *doc.get_entity_mut(line_handle).unwrap() = updated;

                let codec::EntityType::Line(line) = doc.get_entity(line_handle).unwrap() else {
                    panic!("expected Line");
                };
                assert_eq!(
                    line.common.layer, "Geometry",
                    "layer survives an unrelated edit"
                );
                assert_eq!(
                    line.start,
                    Vector3::new(0.0, 0.0, 0.0),
                    "start survives an unrelated edit"
                );
                assert_eq!(
                    line.end,
                    Vector3::new(20.0, 5.0, 0.0),
                    "end reflects the edit"
                );

                // --- DELETE ---
                assert!(doc.remove_entity(ellipse_handle).is_some());
                assert!(doc.get_entity(ellipse_handle).is_none());

                // --- SAVE / REOPEN ---
                let path = std::env::temp_dir().join(format!(
                    "ocs_entity_crud_acceptance_test_{}.dwg",
                    std::process::id()
                ));
                codec::DwgWriter::write_to_file(&path, &doc).expect("write dwg");
                let reopened = codec::DwgReader::from_file(&path)
                    .expect("open dwg")
                    .read()
                    .expect("read dwg");
                let _ = std::fs::remove_file(&path);

                let codec::EntityType::Line(reopened_line) = reopened
                    .get_entity(line_handle)
                    .expect("line survives save/reopen")
                else {
                    panic!("expected Line");
                };
                assert_eq!(
                    reopened_line.common.layer, "Geometry",
                    "layer survives a real DWG round trip"
                );
                assert_eq!(
                    reopened_line.start,
                    Vector3::new(0.0, 0.0, 0.0),
                    "start survives a real DWG round trip"
                );
                assert_eq!(
                    reopened_line.end,
                    Vector3::new(20.0, 5.0, 0.0),
                    "the edit survives a real DWG round trip"
                );
                assert!(
                    reopened.get_entity(ellipse_handle).is_none(),
                    "the deleted ellipse must not reappear after reopen"
                );
            });
        }
    }
}

/// These run a real RustPython interpreter with the `ocs` module registered
/// (same setup as `lib.rs`'s `run_eval`), but with no `HostGuard` active — so
/// `host_ctx::with_host` always returns `None`. `add_points` and
/// `write_record` both finish all of their own argument/dict parsing
/// *before* calling `host_ctx::with_host`, so getting exactly the sentinel
/// "not running inside a PY_ command" `RuntimeError` (rather than a
/// `TypeError`/`ValueError` from the parsing itself) confirms the new
/// RustPython glue code — `Vec<PyObjectRef>` extraction, `PyDictRef`
/// downcasting, `try_into_value` for every XDATA value kind — actually works
/// against the real interpreter, without needing a live `HostApi`.
#[cfg(test)]
mod tests {
    use super::module_def;

    #[cfg(feature = "experimental-host-model")]
    #[test]
    fn host_coverage_matches_generated_python_keys() {
        use ocs_plugin_api::entity_coverage::{EntityCoverageCatalog, ModelAccess};
        use std::collections::{BTreeMap, BTreeSet};

        let catalog: EntityCoverageCatalog =
            serde_json::from_str(ocs_plugin_api::get_embedded_entity_coverage_json()).unwrap();
        let generated = include_str!(concat!(env!("OUT_DIR"), "/entity_crud.rs"));
        let converter = generated
            .split("pub(crate) fn entity_to_dict")
            .nth(1)
            .unwrap()
            .split("pub(crate) fn dict_to_entity")
            .next()
            .unwrap();
        let mut emitted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut current = None;
        for line in converter.lines() {
            let line = line.trim();
            if let Some(arm) = line.strip_prefix("codec::EntityType::") {
                if let Some((kind, _)) = arm.split_once("(value) => {") {
                    current = Some(kind.to_owned());
                    emitted.entry(kind.to_owned()).or_default();
                }
            }
            if let (Some(kind), Some(key)) = (&current, line.strip_prefix("dict.set_item(\"")) {
                let key = key.split('"').next().unwrap();
                emitted.get_mut(kind).unwrap().insert(key.to_owned());
            }
            if line == "}" {
                current = None;
            }
        }
        // Hand-written kinds: keys come from the include file's dict writes
        // and per-subtype field table.
        let manual = include_str!("dimension_model.rs.inc");
        let dimension_keys = emitted.entry("Dimension".to_owned()).or_default();
        for line in manual.lines() {
            let line = line.trim();
            let key = line
                .strip_prefix("dict.set_item(\"")
                .or_else(|| {
                    (line.contains("\", V(") || line.contains("\", F(") || line.contains("\", B("))
                        .then(|| line.strip_prefix("(\""))
                        .flatten()
                });
            if let Some(key) = key {
                dimension_keys.insert(key.split('"').next().unwrap().to_owned());
            }
        }
        for entry in &catalog.entity_kinds {
            let expected: BTreeSet<_> = entry
                .properties
                .iter()
                .filter(|property| property.model_access != ModelAccess::Unmapped)
                .map(|property| property.name.clone())
                .collect();
            if let Some(actual) = emitted.get(&entry.kind) {
                assert_eq!(&expected, actual, "{}", entry.kind);
            } else {
                assert_eq!(
                    expected,
                    BTreeSet::from([
                        "handle".to_owned(),
                        "kind".to_owned(),
                        "layer".to_owned(),
                        "owner_handle".to_owned()
                    ]),
                    "{}",
                    entry.kind
                );
            }
        }
        assert_eq!(emitted.len(), 43);
    }

    #[cfg(feature = "experimental-host-model")]
    #[test]
    fn entity_snapshot_uses_catalog_variant_name() {
        use ocs_plugin_api::host::codec;
        let hatch = codec::EntityType::Hatch(codec::entities::Hatch::default());
        let value = serde_json::to_value(hatch).unwrap();
        let kind = value.as_object().unwrap().keys().next().unwrap();
        assert_eq!(kind, "Hatch");
        assert_ne!(kind, "HATCH");
    }

    #[cfg(feature = "experimental-host-model")]
    #[test]
    fn python_can_query_host_coverage_without_document() {
        assert_eq!(eval("len(ocs.entity_kinds())"), Ok("48".to_owned()));
        assert_eq!(
            eval("'paths' in ocs.entity_coverage('Hatch')['editable']"),
            Ok("True".to_owned())
        );
        assert_eq!(
            eval("ocs.entity_coverage('Line')['scope']"),
            Ok("canvas".to_owned())
        );
    }

    #[cfg(feature = "experimental-host-model")]
    #[test]
    fn update_many_binding_parses_python_batch() {
        let error = eval("ocs.update_many('Move', [{'handle': 1, 'layer': '0'}])").unwrap_err();
        assert!(
            error.contains("not running inside a PY_ command"),
            "{error}"
        );
    }

    // Runs `source` (a hardcoded literal in each test below, never external
    // input) as a single expression inside a fresh, stdlib-free RustPython
    // interpreter with only the `ocs` module registered — the same sandboxed
    // `Mode::Eval` setup `lib.rs`'s `run_eval` uses for real `PY_EVAL`
    // commands. Not a generic "eval untrusted string" helper.
    fn eval(source: &str) -> Result<String, String> {
        let builder = rustpython_vm::Interpreter::builder(Default::default());
        let ocs_def = module_def(&builder.ctx);
        let interp = builder.add_native_module(ocs_def).build();
        interp.enter(|vm| {
            let scope = vm.new_scope_with_builtins();
            let ocs = vm.import("ocs", 0).map_err(|exc| exc_string(vm, &exc))?;
            scope
                .globals
                .set_item("ocs", ocs, vm)
                .map_err(|exc| exc_string(vm, &exc))?;
            let code_obj = vm
                .compile(
                    source,
                    rustpython_vm::compiler::Mode::Eval,
                    "<test>".to_string(),
                )
                .map_err(|err| err.to_string())?;
            match vm.run_code_obj(code_obj, scope) {
                Ok(result) => result
                    .str(vm)
                    .map(|s| s.to_string())
                    .map_err(|exc| exc_string(vm, &exc)),
                Err(exc) => Err(exc_string(vm, &exc)),
            }
        })
    }

    fn exc_string(
        vm: &rustpython_vm::VirtualMachine,
        exc: &rustpython_vm::builtins::PyBaseExceptionRef,
    ) -> String {
        let obj: rustpython_vm::PyObjectRef = exc.clone().into();
        let name = obj.class().name().to_string();
        let message = obj.str(vm).map(|s| s.to_string()).unwrap_or_default();
        format!("{name}: {message}")
    }

    const NO_HOST: &str = "RuntimeError: ocs: not running inside a PY_ command";

    #[test]
    fn add_points_parses_before_reaching_the_host() {
        let result = eval("ocs.add_points([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])");
        assert_eq!(result, Err(NO_HOST.to_string()));
    }

    #[test]
    fn add_points_rejects_wrong_point_shape() {
        let result = eval("ocs.add_points([[1.0, 2.0]])");
        assert!(matches!(result, Err(ref msg) if msg.starts_with("ValueError: ocs.add_points")));
    }

    #[test]
    fn add_points_rejects_empty_list() {
        let result = eval("ocs.add_points([])");
        assert!(matches!(result, Err(ref msg) if msg.starts_with("ValueError: ocs.add_points")));
    }

    #[test]
    fn write_record_parses_every_xdata_kind_before_reaching_the_host() {
        let result = eval(
            r#"ocs.write_record(1, "TEST", [
                {"kind": "String", "value": "hi"},
                {"kind": "ControlString", "value": "{"},
                {"kind": "LayerName", "value": "0"},
                {"kind": "BinaryData", "value": b"\x01\x02"},
                {"kind": "Handle", "value": 7},
                {"kind": "Point3D", "value": [1.0, 2.0, 3.0]},
                {"kind": "Position3D", "value": [1.0, 2.0, 3.0]},
                {"kind": "Displacement3D", "value": [1.0, 2.0, 3.0]},
                {"kind": "Direction3D", "value": [1.0, 2.0, 3.0]},
                {"kind": "Real", "value": 1.5},
                {"kind": "Distance", "value": 2.5},
                {"kind": "ScaleFactor", "value": 1.0},
                {"kind": "Integer16", "value": 16},
                {"kind": "Integer32", "value": 32},
            ])"#,
        );
        assert_eq!(result, Err(NO_HOST.to_string()));
    }

    #[test]
    fn write_record_rejects_unknown_kind() {
        let result = eval(r#"ocs.write_record(1, "TEST", [{"kind": "Nope", "value": 1}])"#);
        assert!(matches!(
            result,
            Err(ref msg) if msg.starts_with("ValueError: unsupported XDATA value kind: Nope")
        ));
    }

    #[test]
    fn write_record_rejects_malformed_point_value() {
        let result =
            eval(r#"ocs.write_record(1, "TEST", [{"kind": "Point3D", "value": [1.0, 2.0]}])"#);
        assert!(matches!(
            result,
            Err(ref msg) if msg.starts_with("ValueError: XDATA Point3D")
        ));
    }

    #[test]
    fn read_record_and_remove_record_parse_before_reaching_the_host() {
        assert_eq!(
            eval(r#"ocs.read_record(1, "TEST")"#),
            Err(NO_HOST.to_string())
        );
        assert_eq!(
            eval(r#"ocs.remove_record(1, "TEST")"#),
            Err(NO_HOST.to_string())
        );
    }

    #[test]
    fn add_parses_before_reaching_the_host() {
        let result = eval(
            r#"ocs.add({"kind": "Line", "start": {"x": 0.0, "y": 0.0, "z": 0.0}, "end": {"x": 1.0, "y": 1.0, "z": 0.0}})"#,
        );
        assert_eq!(result, Err(NO_HOST.to_string()));
    }

    #[test]
    fn add_rejects_missing_required_geometry_before_reaching_the_host() {
        let result = eval(r#"ocs.add({"kind": "Circle"})"#);
        assert!(
            matches!(result, Err(ref msg) if msg.starts_with("ValueError: ocs.add: Circle requires")),
            "unexpected result: {result:?}"
        );
    }

    #[test]
    fn add_rejects_unknown_kind() {
        let result = eval(r#"ocs.add({"kind": "Nope"})"#);
        assert!(matches!(
            result,
            Err(ref msg) if msg.starts_with("ValueError: ocs: unsupported entity kind: Nope")
        ));
    }

    #[test]
    fn update_and_get_entity_hit_the_host_sentinel() {
        assert_eq!(
            eval(r#"ocs.update(1, {"layer": "X"})"#),
            Err(NO_HOST.to_string())
        );
        assert_eq!(eval("ocs.get_entity(1)"), Err(NO_HOST.to_string()));
    }

    #[test]
    fn add_text_left_validates_before_reaching_host() {
        assert_eq!(
            eval(r#"ocs.add_text_left("Note", 0.0, 0.0, 0.0, 2.0, 0.0)"#),
            Err(NO_HOST.to_string())
        );
        let bad = eval(r#"ocs.add_text_left("Note", 0.0, 0.0, 0.0, 0.0, 0.0)"#);
        assert!(matches!(bad, Err(ref msg) if msg.starts_with("ValueError: ocs.add_text_left:")));
    }
}
