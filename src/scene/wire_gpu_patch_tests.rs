use super::{compose_wire_gpu_patches, ChangeKind, WireGpuPatch, WireIndexEdit};
use document::Handle;
use std::sync::Arc;

fn patch(handle: Handle, kind: ChangeKind, edit: WireIndexEdit) -> WireGpuPatch {
    WireGpuPatch {
        changes: Arc::new(vec![(handle, kind)]),
        runs: Arc::new(rustc_hash::FxHashMap::default()),
        index_edits: Arc::new(vec![edit]),
        new_handles_are_suffix: true,
        face_pass_changed: false,
        text_pass_changed: false,
    }
}

#[test]
fn consecutive_add_then_constraint_edit_stays_one_added_handle() {
    let handle = Handle::new(41);
    let combined = compose_wire_gpu_patches(
        &patch(
            handle,
            ChangeKind::Added,
            WireIndexEdit {
                handle,
                start: 100,
                old_len: 0,
                new_len: 1,
                visible: true,
            },
        ),
        patch(
            handle,
            ChangeKind::Modified,
            WireIndexEdit {
                handle,
                start: 100,
                old_len: 1,
                new_len: 2,
                visible: true,
            },
        ),
    );

    assert_eq!(combined.changes.as_slice(), &[(handle, ChangeKind::Added)]);
    assert_eq!(combined.index_edits[0].start, 100);
    assert_eq!(combined.index_edits[0].old_len, 0);
    assert_eq!(combined.index_edits[0].new_len, 2);
    assert!(combined.index_edits[0].visible);
}

#[test]
fn consecutive_modification_then_removal_is_removed_from_base() {
    let handle = Handle::new(52);
    let combined = compose_wire_gpu_patches(
        &patch(
            handle,
            ChangeKind::Modified,
            WireIndexEdit {
                handle,
                start: 200,
                old_len: 2,
                new_len: 3,
                visible: true,
            },
        ),
        patch(
            handle,
            ChangeKind::Removed,
            WireIndexEdit {
                handle,
                start: 200,
                old_len: 3,
                new_len: 0,
                visible: false,
            },
        ),
    );

    assert_eq!(
        combined.changes.as_slice(),
        &[(handle, ChangeKind::Removed)]
    );
    assert_eq!(combined.index_edits[0].old_len, 2);
    assert_eq!(combined.index_edits[0].new_len, 0);
    assert!(!combined.index_edits[0].visible);
}
