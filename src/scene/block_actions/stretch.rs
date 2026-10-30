//! A native Stretch action freezes its vertex membership at authoring time.
use super::*;
use codec::objects::{BlockPolarStretchAction, BlockStretchAction, BlockStretchHandle};
use codec::types::Vector2;

pub(crate) fn control_points(e: &EntityType) -> Result<Vec<DVec3>, &'static str> {
    let points = match e {
        EntityType::Line(l) => vec![v(l.start), v(l.end)],
        EntityType::Arc(a) => {
            if a.normal.x.abs() > 1e-10 || a.normal.y.abs() > 1e-10 {
                return Err("Stretch requires an arc parallel to the parameter's XY plane.");
            }
            vec![v(a.start_point_wcs()), v(a.end_point_wcs())]
        }
        EntityType::Circle(c) => {
            vec![v(codec::types::Matrix3::arbitrary_axis(c.normal) * c.center)]
        }
        EntityType::Ellipse(e) => vec![v(e.center)],
        EntityType::Point(p) => vec![v(p.location)],
        EntityType::Text(t) => vec![v(
            codec::types::Matrix3::arbitrary_axis(t.normal) * t.insertion_point
        )],
        EntityType::MText(t) => vec![v(t.insertion_point)],
        EntityType::LwPolyline(p) => {
            let p = crate::entities::curve::lwpolyline_world_xy(p)
                .ok_or("Stretch requires a planar XY lightweight polyline.")?;
            p.vertices
                .iter()
                .map(|v| DVec3::new(v.location.x, v.location.y, p.elevation))
                .collect()
        }
        _ => return Err("Select a supported Stretch object."),
    };
    if points.is_empty() || points.iter().any(|p| !p.is_finite()) {
        return Err("Stretch targets need finite vertices.");
    }
    Ok(points)
}

pub(crate) fn native_frame(corners: &[DVec3], plane: f64) -> Result<Vec<Vector2>, &'static str> {
    if corners.len() < 2
        || corners
            .iter()
            .any(|p| !p.is_finite() || (p.z - plane).abs() > 1e-8)
    {
        return Err("Specify finite stretch-frame corners in the parameter's XY plane.");
    }
    let points = if corners.len() == 2 {
        let low = corners[0].min(corners[1]);
        let high = corners[0].max(corners[1]);
        if high.x - low.x < 1e-9 || high.y - low.y < 1e-9 {
            return Err("The stretch frame needs a nonzero width and height.");
        }
        vec![Vector2::new(low.x, low.y), Vector2::new(high.x, high.y)]
    } else {
        corners
            .iter()
            .map(|point| Vector2::new(point.x, point.y))
            .collect()
    };
    validate_polygon(&outline(&points, plane))?;
    Ok(points)
}

fn validate_polygon(points: &[Vector3]) -> Result<(), &'static str> {
    let error = "Use a simple polygon with distinct vertices and nonzero area.";
    if points.len() < 3 {
        return Err(error);
    }
    let points = points
        .iter()
        .map(|point| v(*point) - v(points[0]))
        .collect::<Vec<_>>();
    let scale = points
        .iter()
        .map(|point| point.abs().max_element())
        .fold(0., f64::max);
    let coordinate_tolerance = f64::EPSILON * scale * 64.;
    let area_tolerance = coordinate_tolerance * scale;
    let cross = |a: DVec3, b: DVec3, c: DVec3| (b - a).cross(c - a).z;
    let on = |a: DVec3, b: DVec3, p: DVec3| {
        cross(a, b, p).abs() <= area_tolerance
            && p.x >= a.x.min(b.x) - coordinate_tolerance
            && p.x <= a.x.max(b.x) + coordinate_tolerance
            && p.y >= a.y.min(b.y) - coordinate_tolerance
            && p.y <= a.y.max(b.y) + coordinate_tolerance
    };
    let mut area = 0.;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        if a.distance(b) < 1e-9 {
            return Err(error);
        }
        area += a.cross(b).z;
        for j in i + 1..points.len() {
            if j == i + 1 || (i == 0 && j == points.len() - 1) {
                continue;
            }
            let (c, d) = (points[j], points[(j + 1) % points.len()]);
            let (ab_c, ab_d, cd_a, cd_b) = (
                cross(a, b, c),
                cross(a, b, d),
                cross(c, d, a),
                cross(c, d, b),
            );
            if (ab_c * ab_d < 0. && cd_a * cd_b < 0.)
                || on(a, b, c)
                || on(a, b, d)
                || on(c, d, a)
                || on(c, d, b)
            {
                return Err(error);
            }
        }
    }
    if !area.is_finite() || area.abs() <= area_tolerance {
        return Err(error);
    }
    Ok(())
}

pub(crate) fn bind_frame(
    doc: &Document,
    targets: &[Handle],
    frame: &[DVec3],
    plane: f64,
) -> Result<(Vec<Vector2>, Vec<BlockStretchHandle>), &'static str> {
    let points = native_frame(frame, plane)?;
    let bindings = bind_existing(doc, targets, &points, plane)?;
    Ok((points, bindings))
}

pub(crate) fn bind_existing(
    doc: &Document,
    targets: &[Handle],
    frame: &[Vector2],
    plane: f64,
) -> Result<Vec<BlockStretchHandle>, &'static str> {
    let polygon = outline(frame, plane);
    if polygon.len() < 3 || polygon.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err("The Stretch frame is invalid.");
    }
    let inside = |p: DVec3| {
        let mut crossing = false;
        for (a, b) in polygon
            .iter()
            .zip(polygon.iter().cycle().skip(1))
            .take(polygon.len())
        {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let cross = (p.x - a.x) * dy - (p.y - a.y) * dx;
            if cross.abs() < 1e-8
                && p.x >= a.x.min(b.x) - 1e-8
                && p.x <= a.x.max(b.x) + 1e-8
                && p.y >= a.y.min(b.y) - 1e-8
                && p.y <= a.y.max(b.y) + 1e-8
            {
                return true;
            }
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                crossing = !crossing;
            }
        }
        crossing
    };
    let mut bindings = vec![];
    let mut unique = targets.to_vec();
    unique.sort();
    unique.dedup();
    for handle in unique {
        let points = control_points(
            doc.get_entity(handle)
                .ok_or("A Stretch target is unavailable.")?,
        )?;
        if points.iter().any(|p| (p.z - plane).abs() > 1e-8) {
            return Err("Stretch targets must lie in the parameter's XY plane.");
        }
        let indexes = points
            .into_iter()
            .enumerate()
            .filter(|(_, p)| inside(*p))
            .map(|(i, _)| i as i32)
            .collect::<Vec<_>>();
        if !indexes.is_empty() {
            bindings.push(BlockStretchHandle { handle, indexes });
        }
    }
    Ok(bindings)
}

pub(crate) fn validate_options(a: &BlockStretchAction) -> Result<(), &'static str> {
    options::validate(&a.offsets)?;
    if !a.codes.is_empty()
        || !a.action.action_ids.is_empty()
        || a.connections[0].code != a.connections[1].code
        || !matches!(
            [
                a.connections[0].name.as_str(),
                a.connections[1].name.as_str()
            ],
            ["XDelta", "YDelta"]
                | ["BaseXDelta", "BaseYDelta"]
                | ["EndXDelta", "BaseYDelta"]
                | ["BaseXDelta", "EndYDelta"]
                | ["EndXDelta", "EndYDelta"]
        )
    {
        return Err("This Stretch action uses options or parameter bindings not supported yet.");
    }
    Ok(())
}

pub(crate) fn validate(doc: &Document, definition: Handle, a: &BlockStretchAction) -> Result<(), &'static str> {
    validate_options(a)?;
    if a.points.len() < 2
        || a.points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err("The Stretch frame is invalid.");
    }
    let mut seen = std::collections::BTreeSet::new();
    for binding in &a.handles {
        if !a.action.dependencies.contains(&binding.handle) || !seen.insert(binding.handle) {
            return Err("The Stretch action has invalid target bindings.");
        }
        let count = control_points(
            doc.get_entity(binding.handle)
                .ok_or("A Stretch target is unavailable.")?,
        )?
        .len();
        let unique = binding
            .indexes
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != binding.indexes.len()
            || binding
                .indexes
                .iter()
                .any(|i| *i < 0 || *i as usize >= count)
        {
            return Err("A Stretch vertex binding no longer matches its object.");
        }
    }
    // Selected objects without a partial vertex binding move as a whole.
    for h in super::targets::geometry(doc, definition, Kind::Stretch, a.connections[0].code, &a.action.dependencies)? {
        control_points(
            doc.get_entity(h)
                .ok_or("A Stretch target is unavailable.")?,
        )?;
    }
    Ok(())
}

pub(crate) fn polar_key_point(a: &BlockPolarStretchAction) -> Result<XyKeyPoint, &'static str> {
    let key = match [a.connections[0].name.as_str(), a.connections[1].name.as_str()] {
        ["BaseXDelta", "BaseYDelta"] => XyKeyPoint::Base,
        ["EndXDelta", "EndYDelta"] => XyKeyPoint::End,
        _ => return Err("This Polar Stretch action uses unsupported key-point connections."),
    };
    let connection_code = a.connections[0].code;
    if a.connections.iter().any(|c| c.code != connection_code)
        || a.connections[2..].iter().map(|c| c.name.as_str()).ne(["Base", "End", "UpdatedBase", "UpdatedEnd"]) {
        return Err("This Polar Stretch action uses unsupported frame connections.");
    }
    Ok(key)
}

pub(crate) fn validate_polar(
    doc: &Document,
    definition: Handle,
    a: &BlockPolarStretchAction,
) -> Result<(), &'static str> {
    polar_key_point(a)?;
    if !a.distance_multiplier.is_finite()
        || !a.angle_offset.is_finite()
        || !a.codes.is_empty()
        || !a.extra.is_empty()
        || !a.action.action_ids.is_empty()
        || a.points.len() < 2
        || a.points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err("This Polar Stretch action uses options not supported yet.");
    }
    let geometric = super::targets::geometry(doc, definition, Kind::PolarStretch, a.connections[0].code, &a.action.dependencies)?;
    let mut handles = a.handles.clone();
    handles.sort();
    handles.dedup();
    if handles.len() != a.handles.len()
        || handles.iter().any(|h| !geometric.contains(h))
    {
        return Err("The Polar Stretch action has invalid target bindings.");
    }
    let mut seen = std::collections::BTreeSet::new();
    for binding in &a.bindings {
        if !a.action.dependencies.contains(&binding.handle) || !seen.insert(binding.handle) {
            return Err("The Polar Stretch action has invalid vertex bindings.");
        }
        let count = control_points(
            doc.get_entity(binding.handle)
                .ok_or("A Polar Stretch target is unavailable.")?,
        )?
        .len();
        let unique = binding
            .indexes
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != binding.indexes.len()
            || binding
                .indexes
                .iter()
                .any(|i| *i < 0 || *i as usize >= count)
        {
            return Err("A Polar Stretch vertex binding no longer matches its object.");
        }
    }
    for h in geometric {
        control_points(
            doc.get_entity(h)
                .ok_or("A Polar Stretch target is unavailable.")?,
        )?;
    }
    Ok(())
}

pub(crate) fn apply(
    e: &mut EntityType,
    a: &BlockStretchAction,
    delta: DVec3,
) -> Result<(), &'static str> {
    if delta == DVec3::ZERO {
        return Ok(());
    }
    let Some(binding) = a
        .handles
        .iter()
        .find(|b| b.handle == e.common().handle && !b.indexes.is_empty())
    else {
        crate::scene::view::dispatch::apply_transform(
            e,
            &crate::command::EntityTransform::Translate(delta),
        );
        return Ok(());
    };
    match e {
        EntityType::Arc(arc) => {
            if !crate::modules::draw::modify::stretch::stretch_arc_endpoints(
                arc,
                binding.indexes.contains(&0),
                binding.indexes.contains(&1),
                delta,
            ) {
                return Err("The Stretch displacement collapses the arc chord.");
            }
        }
        EntityType::Circle(_)
        | EntityType::Ellipse(_)
        | EntityType::Point(_)
        | EntityType::Text(_)
        | EntityType::MText(_) => {
            crate::scene::view::dispatch::apply_transform(
                e,
                &crate::command::EntityTransform::Translate(delta),
            );
        }
        EntityType::Line(l) => {
            for index in &binding.indexes {
                let at = match index {
                    0 => &mut l.start,
                    1 => &mut l.end,
                    _ => return Err("Invalid Stretch line endpoint."),
                };
                *at = *at + native(delta);
            }
        }
        EntityType::LwPolyline(p) => {
            let mut world = crate::entities::curve::lwpolyline_world_xy(p)
                .ok_or("Stretch requires a planar XY lightweight polyline.")?;
            let before = world
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y])
                .collect::<Vec<_>>();
            for index in &binding.indexes {
                let v = world
                    .vertices
                    .get_mut(*index as usize)
                    .ok_or("Invalid Stretch polyline vertex.")?;
                v.location.x += delta.x;
                v.location.y += delta.y;
            }
            let after = world
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y])
                .collect::<Vec<_>>();
            for (i, v) in world.vertices.iter_mut().enumerate() {
                v.bulge = crate::modules::draw::modify::stretch::held_bulge(
                    v.bulge,
                    &before,
                    &after,
                    i,
                    world.is_closed,
                );
            }
            *p = world;
        }
        _ => return Err("Unsupported Stretch target."),
    }
    Ok(())
}

/// The bound key point is driven; the opposite endpoint anchors the action.
pub(crate) fn apply_polar_frame(
    entity: &mut EntityType,
    action: &BlockPolarStretchAction,
    old_base: DVec3,
    old_end: DVec3,
    new_base: DVec3,
    new_end: DVec3,
) -> Result<(), &'static str> {
    let (pivot, old_tip, new_pivot, new_tip) = match polar_key_point(action)? {
        XyKeyPoint::End => (old_base, old_end, new_base, new_end),
        XyKeyPoint::Base => (old_end, old_base, new_end, new_base),
        _ => unreachable!(),
    };
    apply_polar(entity, action, pivot, old_tip, pivot + (new_tip - new_pivot))?;
    super::super::view::dispatch::apply_transform(entity,
        &crate::command::EntityTransform::Translate(new_pivot - pivot));
    Ok(())
}

/// Rotate the selection first, then extend frozen vertices along the updated
/// polar direction. Explicit rotate-only targets retain their original size.
pub(crate) fn apply_polar(
    e: &mut EntityType,
    action: &BlockPolarStretchAction,
    base: DVec3,
    old_end: DVec3,
    new_end: DVec3,
) -> Result<(), &'static str> {
    let (old_vec, new_vec) = (old_end - base, new_end - base);
    let (old_length, new_length) = (old_vec.length(), new_vec.length());
    if !old_length.is_finite()
        || !new_length.is_finite()
        || old_length <= 1e-9
        || new_length <= 1e-9
    {
        return Err("The Polar parameter needs a nonzero distance.");
    }
    if old_end == new_end {
        return Ok(());
    }
    let angle = old_vec.cross(new_vec).z.atan2(old_vec.dot(new_vec));
    if angle != 0. {
        crate::scene::view::dispatch::apply_transform(
            e,
            &crate::command::EntityTransform::Rotate {
                center: base,
                axis: DVec3::Z,
                angle_rad: angle,
            },
        );
    }
    if action.handles.contains(&e.common().handle) {
        return Ok(());
    }
    let delta = options::displacement(
        &BlockActionOffsets {
            distance_multiplier: action.distance_multiplier,
            angle_offset: action.angle_offset,
            flags: 0,
        },
        new_vec / new_length * (new_length - old_length),
    )?;
    let stretch = BlockStretchAction {
        handles: action.bindings.clone(),
        ..Default::default()
    };
    apply(e, &stretch, delta)
}

pub(crate) fn outline(points: &[Vector2], z: f64) -> Vec<Vector3> {
    let points = if points.len() == 2 {
        let (a, b) = (points[0], points[1]);
        vec![a, Vector2::new(b.x, a.y), b, Vector2::new(a.x, b.y)]
    } else {
        points.to_vec()
    };
    points
        .into_iter()
        .map(|p| Vector3::new(p.x, p.y, z))
        .collect()
}
