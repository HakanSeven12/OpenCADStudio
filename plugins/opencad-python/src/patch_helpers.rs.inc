// Merge helpers for nested structs that scripts edit as a partial patch.

/// Apply `patch` (a dict of context fields) onto `existing`, so a script can
/// change one context field without resetting the rest. Unknown keys error.
fn merge_multileader_context(
    existing: &codec::entities::MultiLeaderAnnotContext,
    patch: PyObjectRef,
    vm: &VirtualMachine,
) -> PyResult<codec::entities::MultiLeaderAnnotContext> {
    let patch = patch.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
    let merged = multi_leader_annot_context_to_dict(vm, existing)?
        .try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
    for key in patch.keys_vec() {
        let name = key.clone().try_into_value::<String>(vm)?;
        if merged.get_item_opt(name.as_str(), vm)?.is_none() {
            return Err(vm.new_value_error(format!(
                "ocs: MultiLeader.context property {name:?} is outside the editable schema"
            )));
        }
        merged.set_item(name.as_str(), patch.get_item(name.as_str(), vm)?, vm)?;
    }
    dict_to_multi_leader_annot_context(merged.into(), vm)
}
