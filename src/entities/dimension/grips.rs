// Dimension grips and text-position capture/restore.

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct DimTextRelativePosition {
    pub(crate) along_fraction: f64,
    pub(crate) perpendicular_offset: f64,
    pub(crate) z_offset: f64,
}

pub(crate) fn capture_dim_text_relative_position(
    dim: &Dimension,
) -> Option<DimTextRelativePosition> {
    let base = dim.base();

    // Auto-positioned text is recomputed by the renderer from DIMSTYLE.
    // Only an explicitly moved text point needs to follow a grip deformation.
    if !base.text_user_positioned {
        return None;
    }

    let text = base.text_middle_point;
    if text.x * text.x + text.y * text.y + text.z * text.z <= 1e-16 {
        return None;
    }

    let (first, second, defpt, ax, ay) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            d.rotation.cos(),
            d.rotation.sin(),
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 1e-12 {
                return None;
            }

            (
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
            )
        }
        Dimension::Diameter(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let len = (dx * dx + dy * dy).sqrt();

            if len <= 1e-12 {
                return None;
            }

            (
                d.angle_vertex,
                d.definition_point,
                d.angle_vertex,
                dx / len,
                dy / len,
            )
        }
        _ => return None,
    };

    let px = -ay;
    let py = ax;

    let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
    let text_t = (text.x - defpt.x) * ax + (text.y - defpt.y) * ay;

    let span = t2 - t1;
    let along_fraction = if span.abs() > 1e-12 {
        (text_t - t1) / span
    } else {
        0.5
    };

    let along = t1 + span * along_fraction;
    let line_x = defpt.x + ax * along;
    let line_y = defpt.y + ay * along;

    let perpendicular_offset = (text.x - line_x) * px + (text.y - line_y) * py;

    Some(DimTextRelativePosition {
        along_fraction,
        perpendicular_offset,
        z_offset: text.z - defpt.z,
    })
}

pub(crate) fn restore_dim_text_relative_position(
    dim: &mut Dimension,
    saved: DimTextRelativePosition,
) {
    let (first, second, defpt, ax, ay) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            d.rotation.cos(),
            d.rotation.sin(),
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 1e-12 {
                return;
            }

            (
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
            )
        }
        Dimension::Diameter(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let len = (dx * dx + dy * dy).sqrt();

            if len <= 1e-12 {
                return;
            }

            (
                d.angle_vertex,
                d.definition_point,
                d.angle_vertex,
                dx / len,
                dy / len,
            )
        }
        _ => return,
    };

    let px = -ay;
    let py = ax;

    let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;

    let along = t1 + (t2 - t1) * saved.along_fraction;

    let base = dim.base_mut();
    base.text_middle_point = Vector3::new(
        defpt.x + ax * along + px * saved.perpendicular_offset,
        defpt.y + ay * along + py * saved.perpendicular_offset,
        defpt.z + saved.z_offset,
    );
}

pub(crate) fn dimension_line_grip_position(dim: &Dimension) -> Option<DVec3> {
    match dim {
        Dimension::Angular2Ln(d) => return Some(dv3(&d.dimension_arc)),
        Dimension::Angular3Pt(d) => return Some(dv3(&d.definition_point)),
        Dimension::Arc(d) => return Some(dv3(&d.definition_point)),
        _ => {}
    }
    dimension_line_endpoints(dim).map(|(first, second)| (first + second) * 0.5)
}

pub(crate) fn dimension_line_endpoints(dim: &Dimension) -> Option<(DVec3, DVec3)> {
    let (first, second, defpt, ax, ay) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            d.rotation.cos(),
            d.rotation.sin(),
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt();

            if len <= 1e-12 {
                return None;
            }

            (
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
            )
        }
        _ => return None,
    };

    let px = -ay;
    let py = ax;

    // Project both extension origins onto the current dimension line.
    let off1 = (defpt.x - first.x) * px + (defpt.y - first.y) * py;
    let off2 = (defpt.x - second.x) * px + (defpt.y - second.y) * py;

    let p1 = DVec3::new(first.x + px * off1, first.y + py * off1, defpt.z);

    let p2 = DVec3::new(second.x + px * off2, second.y + py * off2, defpt.z);

    Some((p1, p2))
}

impl Grippable for Dimension {
    fn grips(&self) -> Vec<GripDef> {
        // Auto-placed dimensions carry a zero text_middle_point sentinel; put
        // the text grip at the style-default placement (default metrics) instead
        // of the world origin, so it stays on the visible text and grabbable.
        let text = {
            let p = self.base().text_middle_point;
            if p.x * p.x + p.y * p.y + p.z * p.z > 1e-16 {
                dv3(&p)
            } else {
                dv3(&dimension_text_pos_f64(self, None, 2.5, 1.0, None))
            }
        };
        let mut grips = match self {
            Dimension::Linear(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(
                    2,
                    dimension_line_grip_position(self).unwrap_or_else(|| dv3(&d.definition_point)),
                ),
                center_grip(3, text),
            ],
            Dimension::Aligned(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(
                    2,
                    dimension_line_grip_position(self).unwrap_or_else(|| dv3(&d.definition_point)),
                ),
                center_grip(3, text),
            ],
            Dimension::Radius(d) => vec![
                square_grip(0, dv3(&d.angle_vertex)),
                center_grip(1, dv3(&d.definition_point)),
                center_grip(2, text),
            ],
            Dimension::Diameter(d) => vec![
                square_grip(0, dv3(&d.angle_vertex)),
                center_grip(1, dv3(&d.definition_point)),
                center_grip(2, text),
            ],
            Dimension::Angular2Ln(d) => vec![
                square_grip(0, dv3(&d.first_point)),
                center_grip(1, dv3(&d.second_point)),
                center_grip(2, dv3(&d.angle_vertex)),
                center_grip(3, dv3(&d.definition_point)),
                center_grip(4, dv3(&d.dimension_arc)),
                center_grip(5, text),
            ],
            Dimension::Angular3Pt(d) => vec![
                square_grip(0, dv3(&d.angle_vertex)),
                center_grip(1, dv3(&d.first_point)),
                center_grip(2, dv3(&d.second_point)),
                center_grip(3, dv3(&d.definition_point)),
                center_grip(4, text),
            ],
            Dimension::Ordinate(d) => vec![
                square_grip(0, dv3(&d.feature_location)),
                center_grip(1, dv3(&d.leader_endpoint)),
                center_grip(2, text),
            ],
            Dimension::Arc(d) => {
                let mut grips = vec![
                    square_grip(0, dv3(&d.center_point)),
                    center_grip(1, dv3(&d.first_extension_point)),
                    center_grip(2, dv3(&d.second_extension_point)),
                    center_grip(3, dv3(&d.definition_point)),
                ];
                if d.has_leader {
                    grips.push(center_grip(4, dv3(&d.first_leader_point)));
                    grips.push(center_grip(5, dv3(&d.second_leader_point)));
                    grips.push(center_grip(6, text));
                } else {
                    grips.push(center_grip(4, text));
                }
                grips
            }
            Dimension::LargeRadial(d) => vec![
                square_grip(0, dv3(&d.definition_point)),
                center_grip(1, dv3(&d.chord_point)),
                center_grip(2, dv3(&d.override_center)),
                center_grip(3, dv3(&d.jog_point)),
                center_grip(4, text),
            ],
        };
        if let Some((first, second)) = dimension_line_endpoints(self) {
            grips.push(center_grip(4, first));
            grips.push(center_grip(5, second));
        }
        grips
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if matches!(self, Dimension::Ordinate(_)) && grip_id == 2 {
            let old_text = dimension_text_pos_f64(self, None, 2.5, 1.0, None);
            let mut new_text = old_text;
            apply_to_v3(&mut new_text, &apply);
            let delta = new_text - old_text;
            if let Dimension::Ordinate(d) = self {
                d.leader_endpoint = d.leader_endpoint + delta;
                d.base.text_middle_point = new_text;
                d.base.text_user_positioned = true;
                d.refresh_measurement();
            }
            return;
        }
        // Text keeps its own grip ID even when arrow-tip grips follow it.
        let text_grip = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => 3,
            Dimension::Radius(_) | Dimension::Diameter(_) => 2,
            Dimension::Angular2Ln(_) => 5,
            Dimension::Angular3Pt(_) => 4,
            Dimension::Ordinate(_) => 2,
            Dimension::Arc(d) => {
                if d.has_leader {
                    6
                } else {
                    4
                }
            }
            Dimension::LargeRadial(_) => 4,
        };
        if grip_id == text_grip {
            apply_to_v3(&mut self.base_mut().text_middle_point, &apply);

            // A diametric dimension must remain collinear when its text/leader
            // grip is moved. Rotate both diameter endpoints around the fixed
            // center so the leader and diameter share the same radial axis.
            if let Dimension::Diameter(d) = self {
                let center = d.center();
                let radius = d.measurement() * 0.5;
                let offset = d.base.text_middle_point - center;

                if offset.length_squared() > 1e-24 && radius > 1e-12 {
                    let radial = offset.normalize() * radius;

                    // Keep the endpoint nearest the text on the text side.
                    d.angle_vertex = center + radial;
                    d.definition_point = center - radial;
                    d.base.definition_point = d.definition_point;
                    d.base.actual_measurement = d.measurement();
                }
            }

            // A radius turns about its centre the same way: the point on the
            // arc follows the text instead of the line kinking to reach it.
            if let Dimension::Radius(d) = self {
                let center = d.angle_vertex;
                let radius = d.measurement();
                let offset = d.base.text_middle_point - center;
                if offset.length_squared() > 1e-24 && radius > 1e-12 {
                    d.definition_point = center + offset.normalize() * radius;
                    d.base.definition_point = d.definition_point;
                    d.base.actual_measurement = d.measurement();
                }
            }

            // Dragging the text grip pins it to a user-defined location, so it
            // no longer follows the style (DIMTAD). See #94.
            self.base_mut().text_user_positioned = true;
            return;
        }

        // A manually positioned dimension text point is stored in the DWG as an
        // absolute coordinate. Capture its relation to the old dimension geometry
        // before moving a definition grip so that it can be reconstructed against
        // the new geometry afterwards.
        let relative_text = capture_dim_text_relative_position(self);

        match self {
            Dimension::Linear(d) => match grip_id {
                0 => apply_to_v3(&mut d.first_point, &apply),
                1 => apply_to_v3(&mut d.second_point, &apply),
                2 | 4 | 5 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Aligned(d) => match grip_id {
                0 => apply_to_v3(&mut d.first_point, &apply),
                1 => apply_to_v3(&mut d.second_point, &apply),
                2 | 4 | 5 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Radius(d) => match grip_id {
                0 => apply_to_v3(&mut d.angle_vertex, &apply),
                1 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Diameter(d) => {
                let center = d.center();
                let radius = d.measurement() * 0.5;
                let mut target = match grip_id {
                    0 => d.angle_vertex,
                    1 => d.definition_point,
                    _ => center,
                };
                if grip_id <= 1 {
                    apply_to_v3(&mut target, &apply);
                    let offset = target - center;
                    if offset.length_squared() > 1e-24 && radius > 1e-12 {
                        let radial = offset.normalize() * radius;
                        if grip_id == 0 {
                            d.angle_vertex = center + radial;
                            d.definition_point = center - radial;
                        } else {
                            d.definition_point = center + radial;
                            d.angle_vertex = center - radial;
                        }
                    }
                }
            }
            Dimension::Angular2Ln(d) => match grip_id {
                0 => apply_to_v3(&mut d.first_point, &apply),
                1 => apply_to_v3(&mut d.second_point, &apply),
                2 => apply_to_v3(&mut d.angle_vertex, &apply),
                3 => apply_to_v3(&mut d.definition_point, &apply),
                4 => apply_to_v3(&mut d.dimension_arc, &apply),
                _ => {}
            },
            Dimension::Angular3Pt(d) => match grip_id {
                0 => apply_to_v3(&mut d.angle_vertex, &apply),
                1 => apply_to_v3(&mut d.first_point, &apply),
                2 => apply_to_v3(&mut d.second_point, &apply),
                3 => apply_to_v3(&mut d.definition_point, &apply),
                _ => {}
            },
            Dimension::Ordinate(d) => match grip_id {
                0 => apply_to_v3(&mut d.feature_location, &apply),
                1 => {
                    let old = d.leader_endpoint;
                    apply_to_v3(&mut d.leader_endpoint, &apply);
                    if d.base.text_user_positioned {
                        d.base.text_middle_point =
                            d.base.text_middle_point + (d.leader_endpoint - old);
                    }
                }
                _ => {}
            },
            Dimension::Arc(d) => match grip_id {
                0 => apply_to_v3(&mut d.center_point, &apply),
                1 => apply_to_v3(&mut d.first_extension_point, &apply),
                2 => apply_to_v3(&mut d.second_extension_point, &apply),
                3 => apply_to_v3(&mut d.definition_point, &apply),
                4 => apply_to_v3(&mut d.first_leader_point, &apply),
                5 => apply_to_v3(&mut d.second_leader_point, &apply),
                _ => {}
            },
            Dimension::LargeRadial(d) => match grip_id {
                0 => apply_to_v3(&mut d.definition_point, &apply),
                1 => apply_to_v3(&mut d.chord_point, &apply),
                2 => apply_to_v3(&mut d.override_center, &apply),
                3 => apply_to_v3(&mut d.jog_point, &apply),
                _ => {}
            },
        }

        if let Some(saved) = relative_text {
            restore_dim_text_relative_position(self, saved);
        }

        let definition_point = dimension_definition_point(self);
        self.base_mut().definition_point = definition_point;
        self.base_mut().actual_measurement = self.stored_measurement();
    }

    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        let (dim_line_grip, text_grip) = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => (2, 3),
            Dimension::Radius(_) | Dimension::Diameter(_) => (1, 2),
            Dimension::Angular2Ln(_) => (4, 5),
            Dimension::Angular3Pt(_) => (3, 4),
            Dimension::Ordinate(_) => (1, 2),
            Dimension::Arc(d) => (3, if d.has_leader { 6 } else { 4 }),
            Dimension::LargeRadial(_) => (3, 4),
        };
        if grip_id == text_grip {
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Move with Dim Line",
                    action: GripMenuAction::MoveWithDimLine,
                },
                GripMenuItem {
                    label: "Move with Leader",
                    action: GripMenuAction::MoveWithLeader,
                },
                GripMenuItem {
                    label: "Move Independent",
                    action: GripMenuAction::MoveIndependent,
                },
                GripMenuItem {
                    label: "Reset Text",
                    action: GripMenuAction::ResetText,
                },
                GripMenuItem {
                    label: "Rotate Text",
                    action: GripMenuAction::RotateText,
                },
                GripMenuItem {
                    label: "Above Dim Line",
                    action: GripMenuAction::AboveDimLine,
                },
                GripMenuItem {
                    label: "Center",
                    action: GripMenuAction::Center,
                },
            ]
        } else if matches!(self, Dimension::Ordinate(_)) {
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        } else if arrow_end(self, grip_id).is_some() {
            // The grips at the two arrow ends flip their own arrow.
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Flip Arrow",
                    action: GripMenuAction::FlipArrow,
                },
            ]
        } else if grip_id == dim_line_grip {
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Reverse Arrows",
                    action: GripMenuAction::ReverseArrows,
                },
            ]
        } else {
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        }
    }

    fn apply_grip_menu(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        let (_dim_line_grip, text_grip) = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => (2, 3),
            Dimension::Radius(_) | Dimension::Diameter(_) => (1, 2),
            Dimension::Angular2Ln(_) => (4, 5),
            Dimension::Angular3Pt(_) => (3, 4),
            Dimension::Ordinate(_) => (1, 2),
            Dimension::Arc(d) => (3, if d.has_leader { 6 } else { 4 }),
            Dimension::LargeRadial(_) => (3, 4),
        };
        match action {
            A::ResetText if grip_id == text_grip => {
                // Drop any text-position override — leave it to the
                // renderer to recompute from the dim style.
                let b = self.base_mut();
                b.text_middle_point.x = 0.0;
                b.text_middle_point.y = 0.0;
                b.text_middle_point.z = 0.0;
                b.text_user_positioned = false;
            }
            A::Center if grip_id == text_grip => {
                if let Some(point) = dimension_line_grip_position(self) {
                    let base = self.base_mut();
                    base.text_middle_point = Vector3::new(point.x, point.y, point.z);
                    base.text_user_positioned = true;
                }
            }
            A::ReverseArrows => {
                let base = self.base_mut();
                base.flip_arrow1 = !base.flip_arrow1;
                base.flip_arrow2 = !base.flip_arrow2;
            }
            A::FlipArrow => match arrow_end(self, grip_id) {
                Some(1) => self.base_mut().flip_arrow1 ^= true,
                Some(_) => self.base_mut().flip_arrow2 ^= true,
                None => {}
            },
            // The caller sets DIMTAD = Above as an override; the text goes
            // back to automatic placement so DIMGAP and the text height set
            // how far above the line it sits, at any drawing scale. (#543)
            A::AboveDimLine if grip_id == text_grip => {
                let b = self.base_mut();
                b.text_middle_point = Vector3::new(0.0, 0.0, 0.0);
                b.text_user_positioned = false;
            }
            _ => {}
        }
    }

    fn grip_menu_value_prompt(
        &self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        let text_grip = match self {
            Dimension::Linear(_) | Dimension::Aligned(_) => 3,
            Dimension::Radius(_) | Dimension::Diameter(_) => 2,
            Dimension::Angular2Ln(_) => 5,
            Dimension::Angular3Pt(_) => 4,
            Dimension::Ordinate(_) => 2,
            Dimension::Arc(d) => {
                if d.has_leader {
                    6
                } else {
                    4
                }
            }
            Dimension::LargeRadial(_) => 4,
        };
        (grip_id == text_grip && matches!(action, A::RotateText)).then_some("Specify text rotation")
    }

    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        if self.grip_menu_value_prompt(grip_id, action).is_some() && matches!(action, A::RotateText)
        {
            self.base_mut().text_rotation = value.to_radians();
        }
    }
}

// ── Tessellation ─────────────────────────────────────────────────────────
//
// Per-entity tessellation entry for `Dimension`. The trait + impl live in
// this file so all dimension tess code stays alongside the entity
// definition. Shared dim machinery (`ArrowKind`, `DimGeom`, `append_arrow`,
// arrow blocks, colour resolution, `add_segment` / `add_polyline`,
// `normalized_or`, `entity_z`, `offset_snap_pts`) lives in
// `scene::convert::tessellate` and is reused by Leader / MultiLeader too.
