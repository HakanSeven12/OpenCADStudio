// Solid booleans and profile helpers.

use super::*;

impl<'a> HostSession<'a> {
    /// Kernel-backed solid create or transform. Every result is verified by
    /// `solid_to_sat` (the exported payload must lift back with no loss) and an
    /// existing payload is only transformed when it lifts losslessly, so a
    /// failure never changes the document.
    pub fn solid_operation(
        &mut self,
        operation: ocs_plugin_api::host::SolidOperation,
    ) -> Result<Handle, String> {
        use crate::scene::model::solid_model as model;
        use ocs_plugin_api::host::{SolidOperation, SolidPrimitive};
        let positive = |name: &str, value: f64| {
            if value.is_finite() && value > 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be finite and greater than zero"))
            }
        };
        let finite3 = |name: &str, value: [f64; 3]| {
            if value.iter().all(|v| v.is_finite()) {
                Ok(())
            } else {
                Err(format!("{name} must be finite"))
            }
        };
        match operation {
            SolidOperation::Create { primitive, layer } => {
                let body = match primitive {
                    SolidPrimitive::Box { center, size } => {
                        finite3("box center", center)?;
                        for (name, v) in
                            [("length", size[0]), ("width", size[1]), ("height", size[2])]
                        {
                            positive(&format!("box {name}"), v)?;
                        }
                        model::box_solid(center, size[0], size[1], size[2])
                    }
                    SolidPrimitive::Wedge { origin, size } => {
                        finite3("wedge origin", origin)?;
                        for (name, v) in
                            [("length", size[0]), ("width", size[1]), ("height", size[2])]
                        {
                            positive(&format!("wedge {name}"), v)?;
                        }
                        model::wedge_solid(origin, size[0], size[1], size[2])
                    }
                    SolidPrimitive::Cylinder {
                        center,
                        radius,
                        height,
                    } => {
                        finite3("cylinder center", center)?;
                        positive("cylinder radius", radius)?;
                        positive("cylinder height", height)?;
                        model::cylinder_solid(center, radius, height)
                    }
                    SolidPrimitive::Sphere { center, radius } => {
                        finite3("sphere center", center)?;
                        positive("sphere radius", radius)?;
                        model::sphere_solid(center, radius)
                    }
                    SolidPrimitive::Torus {
                        center,
                        major,
                        minor,
                    } => {
                        finite3("torus center", center)?;
                        positive("torus major radius", major)?;
                        positive("torus minor radius", minor)?;
                        if minor >= major {
                            return Err(
                                "torus minor radius must be smaller than the major radius".into()
                            );
                        }
                        model::torus_solid(center, major, minor)
                    }
                    SolidPrimitive::Pyramid {
                        center,
                        radius,
                        height,
                        sides,
                    } => {
                        finite3("pyramid center", center)?;
                        positive("pyramid radius", radius)?;
                        positive("pyramid height", height)?;
                        if !(3..=1024).contains(&sides) {
                            return Err("pyramid sides must be between 3 and 1024".into());
                        }
                        model::pyramid_solid(center, radius, height, sides as usize)
                    }
                }
                .ok_or("the geometry kernel refused these dimensions")?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the solid could not be exported losslessly")?;
                let mut solid = document::entities::Solid3D::new();
                solid.wires = model::edge_wires(&body);
                solid.set_sat_document(&sat);
                if let Some(layer) = layer {
                    if layer.trim().is_empty() {
                        return Err("layer name is empty".into());
                    }
                    solid.common.layer = layer;
                }
                self.push_undo("Create solid");
                let handle = self.add_entity(EntityType::Solid3D(solid));
                if handle.is_null() {
                    return Err("the solid could not be added".into());
                }
                Ok(handle)
            }
            SolidOperation::RegionFromProfile {
                source,
                layer,
                delete_source,
            } => {
                let (entity, plane, loops, closed, layer) =
                    self.profile_source(source, layer, delete_source)?;
                if matches!(entity, EntityType::Region(_)) {
                    return Err("the source is already a region".into());
                }
                if !closed {
                    return Err("the profile must be closed".into());
                }
                let body = ocs_solid::model::planar_region(plane, &loops)
                    .ok_or("the geometry kernel could not build a region from this profile")?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the region could not be exported losslessly")?;
                let mut region = document::entities::Region::new();
                region.point_of_reference = document::types::Vector3::new(
                    plane.origin[0],
                    plane.origin[1],
                    plane.origin[2],
                );
                region.wires = model::edge_wires(&body);
                region.set_sat_document(&sat);
                region.common.layer = layer;
                self.commit_profile_result(
                    EntityType::Region(region),
                    source,
                    delete_source,
                    "Create region",
                )
            }
            SolidOperation::SurfaceFromProfile {
                source,
                layer,
                delete_source,
            } => {
                let (entity, plane, loops, closed, layer) =
                    self.profile_source(source, layer, delete_source)?;
                if matches!(entity, EntityType::Region(_)) {
                    return Err("the source is already a region".into());
                }
                if !closed {
                    return Err("the profile must be closed".into());
                }
                let body = ocs_solid::model::planar_region(plane, &loops)
                    .ok_or("the geometry kernel could not build a surface from this profile")?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the surface could not be exported losslessly")?;
                let mut surface =
                    document::entities::Surface::new(document::entities::SurfaceKind::Plane);
                surface.point_of_reference = document::types::Vector3::new(
                    plane.origin[0],
                    plane.origin[1],
                    plane.origin[2],
                );
                surface.wires = model::edge_wires(&body);
                surface.acis_data = document::entities::AcisData::from_sat(&sat.to_sat_string());
                surface.common.layer = layer;
                self.commit_profile_result(
                    EntityType::Surface(Box::new(surface)),
                    source,
                    delete_source,
                    "Create surface",
                )
            }
            SolidOperation::Extrude {
                source,
                direction,
                layer,
                delete_source,
            } => {
                if direction.iter().any(|v| !v.is_finite()) {
                    return Err("extrusion direction must be finite".into());
                }
                if direction.iter().map(|v| v * v).sum::<f64>().sqrt() <= 1e-9 {
                    return Err("extrusion direction must be nonzero".into());
                }
                let (entity, _plane, _loops, closed, layer) =
                    self.profile_source(source, layer, delete_source)?;
                if matches!(entity, EntityType::Region(_)) {
                    return Err("extrude a curve profile; a region cannot be extruded here".into());
                }
                let body = crate::scene::model::presspull_model::extrusion_body(&entity, direction)
                    .ok_or(
                        "the geometry kernel could not extrude this profile in that direction",
                    )?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the extrusion could not be exported losslessly")?;
                let result = if closed {
                    let mut solid = document::entities::Solid3D::new();
                    solid.wires = model::edge_wires(&body);
                    solid.set_sat_document(&sat);
                    solid.common.layer = layer;
                    EntityType::Solid3D(solid)
                } else {
                    // An open profile sweeps to a surface. The generic kind
                    // carries only the payload, as OCS does after a boolean.
                    let mut surface =
                        document::entities::Surface::new(document::entities::SurfaceKind::Generic);
                    surface.wires = model::edge_wires(&body);
                    surface.acis_data =
                        document::entities::AcisData::from_sat(&sat.to_sat_string());
                    surface.common.layer = layer;
                    EntityType::Surface(Box::new(surface))
                };
                self.commit_profile_result(result, source, delete_source, "Extrude profile")
            }
            SolidOperation::EmbedPicture {
                path,
                origin,
                width,
                layer,
            } => {
                finite3("picture origin", origin)?;
                positive("picture width", width)?;
                if layer.as_ref().is_some_and(|name| name.trim().is_empty()) {
                    return Err("layer name is empty".into());
                }
                let picture =
                    crate::io::ole_embed::EmbeddedImage::from_file(std::path::Path::new(&path))
                        .map_err(|error| format!("cannot read the picture {path:?}: {error}"))?;
                let (upper_left, lower_right) = crate::io::ole_embed::corners_from_placement(
                    document::types::Vector3::new(origin[0], origin[1], origin[2]),
                    width,
                    picture.aspect(),
                );
                let mut frame =
                    crate::io::ole_embed::build_embedded_ole(&picture, upper_left, lower_right);
                if let Some(layer) = layer {
                    frame.common.layer = layer;
                }
                self.push_undo("Embed picture");
                let handle = self.add_entity(EntityType::Ole2Frame(Box::new(frame)));
                if handle.is_null() {
                    return Err("the picture frame could not be added".into());
                }
                Ok(handle)
            }
            SolidOperation::Boolean {
                first,
                second,
                operation,
                layer,
                keep_operands,
            } => {
                use ocs_plugin_api::host::SolidBoolean;
                if first == second {
                    return Err("a solid cannot be combined with itself".into());
                }
                if layer.as_ref().is_some_and(|name| name.trim().is_empty()) {
                    return Err("layer name is empty".into());
                }
                let operand = |host: &Self,
                               handle: Handle,
                               which: &str|
                 -> Result<document::entities::Solid3D, String> {
                    match host.document().get_entity(handle) {
                        Some(EntityType::Solid3D(solid)) => Ok(solid.clone()),
                        Some(_) => Err(format!("the {which} operand must be a Solid3D")),
                        None => Err(format!("entity {handle:?} does not exist")),
                    }
                };
                let a = operand(self, first, "first")?;
                let b = operand(self, second, "second")?;
                if !keep_operands {
                    for handle in [first, second] {
                        if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
                            return Err(format!("entity {handle:?} is on a locked layer"));
                        }
                    }
                }
                let lift = |solid: &document::entities::Solid3D, which: &str| {
                    crate::scene::convert::solid3d_tess::acis_record(&solid.acis_data).ok_or_else(|| {
                        format!("the {which} solid's payload cannot be lifted losslessly, so nothing was changed")
                    })
                };
                let (body_a, body_b) = (lift(&a, "first")?, lift(&b, "second")?);
                let kind = match operation {
                    SolidBoolean::Union => model::Bool::Union,
                    SolidBoolean::Subtract => model::Bool::Subtract,
                    SolidBoolean::Intersect => model::Bool::Intersect,
                };
                let combined = model::boolean_result(kind, &body_a, &body_b)
                    .map_err(|error| {
                        format!(
                            "the geometry kernel refused this {operation:?}: {}; nothing was changed",
                            crate::app::model_ops::modelling_error(&error)
                        )
                    })?
                    .ok_or_else(|| format!("nothing is left after this {operation:?}, so nothing was changed"))?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&combined)
                    .ok_or("the result could not be exported losslessly, so nothing was changed")?;
                let mut solid = document::entities::Solid3D::new();
                solid.common = a.common.clone();
                solid.common.handle = Handle::NULL;
                solid.common.owner_handle = Handle::NULL;
                solid.wires = model::edge_wires(&combined);
                solid.set_sat_document(&sat);
                if let Some(layer) = layer {
                    solid.common.layer = layer;
                }
                self.push_undo("Boolean solids");
                let handle = self.add_entity(EntityType::Solid3D(solid));
                if handle.is_null() {
                    return Err("the result could not be added".into());
                }
                if !keep_operands {
                    self.app.tabs[self.tab]
                        .scene
                        .erase_entities(&[first, second]);
                    self.publish_document_view();
                }
                Ok(handle)
            }
            SolidOperation::Transform { handle, matrix } => {
                if matrix.iter().any(|v| !v.is_finite()) {
                    return Err("transform matrix must be finite".into());
                }
                let column = |i: usize| [matrix[i], matrix[i + 1], matrix[i + 2]];
                let (x, y, z) = (column(0), column(4), column(8));
                let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let rigid = [x, y, z].iter().all(|c| (dot(*c, *c) - 1.0).abs() < 1e-6)
                    && dot(x, y).abs() < 1e-6
                    && dot(x, z).abs() < 1e-6
                    && dot(y, z).abs() < 1e-6
                    && matrix[3].abs() < 1e-9
                    && matrix[7].abs() < 1e-9
                    && matrix[11].abs() < 1e-9
                    && (matrix[15] - 1.0).abs() < 1e-9;
                if !rigid {
                    return Err(
                        "only rigid moves and mirrors are supported; scaling and shear are not"
                            .into(),
                    );
                }
                let mut entity = self
                    .document()
                    .get_entity(handle)
                    .cloned()
                    .ok_or_else(|| format!("entity {handle:?} does not exist"))?;
                if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
                    return Err(format!("entity {handle:?} is on a locked layer"));
                }
                macro_rules! retarget {
                    ($value:expr) => {{
                        let body = crate::scene::convert::solid3d_tess::acis_record(&$value.acis_data)
                            .ok_or("this body's payload cannot be lifted losslessly, so it is left untouched")?;
                        let moved = model::by_matrix(&body, matrix)
                            .ok_or("the geometry kernel refused this transform")?;
                        let sat = crate::scene::convert::acis_export::solid_to_sat(&moved)
                            .ok_or("the moved body could not be exported losslessly")?;
                        $value.wires = model::edge_wires(&moved);
                        $value.silhouettes.clear();
                        $value.history_handle = None;
                        $value.set_sat_document(&sat);
                    }};
                }
                match &mut entity {
                    EntityType::Solid3D(value) => retarget!(value),
                    EntityType::Body(value) => retarget!(value),
                    EntityType::Region(value) => retarget!(value),
                    EntityType::Surface(value) => {
                        let body = crate::scene::convert::solid3d_tess::acis_record(&value.acis_data)
                            .ok_or("this surface's payload cannot be lifted losslessly, so it is left untouched")?;
                        let moved = model::by_matrix(&body, matrix)
                            .ok_or("the geometry kernel refused this transform")?;
                        let sat = crate::scene::convert::acis_export::solid_to_sat(&moved)
                            .ok_or("the moved surface could not be exported losslessly")?;
                        value.wires = model::edge_wires(&moved);
                        value.silhouettes.clear();
                        value.history_handle = None;
                        value.acis_data =
                            document::entities::AcisData::from_sat(&sat.to_sat_string());
                        // A moved plane is still a plane; a swept surface no longer
                        // matches its stored sweep parameters, so it becomes generic.
                        if value.kind != document::entities::SurfaceKind::Plane {
                            value.kind = document::entities::SurfaceKind::Generic;
                            value.surface_data = document::entities::SurfaceData::Generic;
                        }
                    }
                    _ => {
                        return Err(
                            "transform applies to Solid3D, Body, Region and Surface entities"
                                .into(),
                        )
                    }
                }
                self.push_undo("Transform solid");
                if !self.update_entity(entity) {
                    return Err(format!("entity {handle:?} could not be updated"));
                }
                Ok(handle)
            }
        }
    }

    /// Look up a profile entity and its exact planar loops for the kernel.
    /// Returns the entity, its plane, loops and closed flag, and the layer the
    /// result should use (`layer`, or the profile's own).
    #[allow(clippy::type_complexity)]
    fn profile_source(
        &self,
        source: Handle,
        layer: Option<String>,
        delete_source: bool,
    ) -> Result<
        (
            EntityType,
            ocs_geom::space::Plane,
            Vec<Vec<ocs_geom::geom2d::Curve>>,
            bool,
            String,
        ),
        String,
    > {
        if layer.as_ref().is_some_and(|name| name.trim().is_empty()) {
            return Err("layer name is empty".into());
        }
        let entity = self
            .document()
            .get_entity(source)
            .cloned()
            .ok_or_else(|| format!("entity {source:?} does not exist"))?;
        if delete_source && self.app.tabs[self.tab].scene.is_layer_locked(source) {
            return Err(format!("entity {source:?} is on a locked layer"));
        }
        if matches!(
            entity,
            EntityType::Solid3D(_) | EntityType::Body(_) | EntityType::Surface(_)
        ) {
            return Err(
                "the source must be a curve profile, not an existing solid or surface".into(),
            );
        }
        let (plane, loops, closed) =
            crate::scene::model::presspull_model::profile_geometry(&entity)
                .ok_or("the source is not a planar profile")?;
        let layer = layer.unwrap_or_else(|| entity.common().layer.clone());
        Ok((entity, plane, loops, closed, layer))
    }

    /// Record one undo step, add `result` and optionally erase the profile.
    fn commit_profile_result(
        &mut self,
        result: EntityType,
        source: Handle,
        delete_source: bool,
        label: &str,
    ) -> Result<Handle, String> {
        self.push_undo(label);
        let handle = self.add_entity(result);
        if handle.is_null() {
            return Err("the result could not be added".into());
        }
        if delete_source {
            self.erase_entities_recording_designations(&[source]);
            self.publish_document_view();
        }
        Ok(handle)
    }
}
