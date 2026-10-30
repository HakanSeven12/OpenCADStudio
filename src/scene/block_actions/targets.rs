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
        if !grips.as_ref().unwrap().contains(handle) {
            return Err("This action targets another parameter or an unsupported native object.");
        }
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
