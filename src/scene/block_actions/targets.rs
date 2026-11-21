//! Separate geometric targets from the driving parameter's native grip links.
use super::*;

pub(super) fn geometry(
    doc: &Document,
    definition: Handle,
    kind: Kind,
    parameter_id: i32,
    dependencies: &[Handle],
) -> Result<Vec<Handle>, &'static str> {
    let mut geometric = Vec::new();
    let mut grips = None;
    for handle in dependencies {
        if let Some(entity) = doc.get_entity(*handle) {
            if entity.common().owner_handle != definition || !supported(entity) {
                return Err("Select supported action targets belonging to this block.");
            }
            geometric.push(*handle);
            continue;
        }
        if !doc.objects.contains_key(handle) {
            return Err("An action target is unavailable.");
        }
        // The base point parameter only places the insertion grip; the
        // geometry an action moves does not depend on it.
        if matches!(kind, Kind::Stretch | Kind::Move)
            && super::super::block_basepoint::parameter(doc, definition)
                .is_some_and(|(basepoint, _)| basepoint == *handle)
        {
            continue;
        }
        if grips.is_none() {
            let parameter = parameters(doc, definition, kind)
                .into_iter()
                .find(|p| p.id == parameter_id)
                .ok_or("An action has an unsupported parameter binding.")?;
            grips = Some(
                super::super::block_grips::config::locations(doc, definition, parameter.handle)?
                    .into_iter()
                    .map(|grip| grip.handle)
                    .collect::<std::collections::BTreeSet<_>>(),
            );
        }
        if kind == Kind::Move
            && chains::frame_target(doc, definition, parameter_id, *handle).is_some()
        {
            continue;
        }
        if grips.as_ref().unwrap().contains(handle) {
            continue;
        }
        // Visibility, lookup and alignment grips, the alignment and visibility
        // parameters, and other actions carry no frame another parameter
        // reads: a selection that sweeps them in only keeps the link.
        let bookkeeping = match doc.objects.get(handle) {
            Some(ObjectType::BlockVisibilityParameter(_)) => true,
            Some(ObjectType::DynamicBlock(o)) => {
                matches!(
                    o.data,
                    DynamicBlockData::VisibilityGrip(_)
                        | DynamicBlockData::LookupGrip(_)
                        | DynamicBlockData::LookupParameter(_)
                        | DynamicBlockData::LookupAction(_)
                        | DynamicBlockData::AlignmentGrip(_)
                        | DynamicBlockData::AlignmentParameter(_)
                        | DynamicBlockData::MoveAction(_)
                        | DynamicBlockData::StretchAction(_)
                        | DynamicBlockData::PolarStretchAction(_)
                        | DynamicBlockData::FlipAction(_)
                        | DynamicBlockData::ArrayAction(_)
                        | DynamicBlockData::RotateAction(_)
                        | DynamicBlockData::ScaleAction(_)
                )
            }
            _ => false,
        };
        if bookkeeping {
            continue;
        }
        // Flip-linked parameters need frame transport; silently treating them
        // as bookkeeping would leave later grips and edits in the old frame.
        if kind == Kind::Flip {
            return Err("This action targets another parameter or an unsupported native object.");
        }
        let all_params = all_parameters(doc, definition);
        let is_block_param_or_grip = all_params.iter().any(|p| {
            // Move links to Linear parameters and their grips were taken as
            // frame transport above; anything else Linear would drop a move.
            !(kind == Kind::Move && p.linear)
                && (p.handle == *handle
                    || p.grip == *handle
                    || super::super::block_grips::config::locations(doc, definition, p.handle)
                        .map(|locs| locs.iter().any(|g| g.handle == *handle))
                        .unwrap_or(false))
        });
        if is_block_param_or_grip {
            continue;
        }
        return Err("This action targets another parameter or an unsupported native object.");
        // The current per-reference parameter frame already determines its own
        // grips. Keep the native link without transforming definition objects.
    }
    Ok(geometric)
}

pub(super) fn action_mut(data: &mut DynamicBlockData) -> Option<&mut BlockAction> {
    Some(match data {
        DynamicBlockData::MoveAction(a) => &mut a.action,
        DynamicBlockData::StretchAction(a) => &mut a.action,
        DynamicBlockData::PolarStretchAction(a) => &mut a.action,
        DynamicBlockData::FlipAction(a) => &mut a.action,
        DynamicBlockData::ArrayAction(a) => &mut a.action,
        DynamicBlockData::RotateAction(a) | DynamicBlockData::ScaleAction(a) => {
            &mut a.action.action
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
