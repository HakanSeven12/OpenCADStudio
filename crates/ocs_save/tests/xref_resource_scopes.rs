use ocs_save::IfccadSession;
use std::sync::Arc;

#[test]
fn nested_bindings_are_owner_and_tab_scoped() {
    let a = IfccadSession::default();
    let b = IfccadSession::default();
    let id = a
        .bind_resource_bytes(
            2,
            1,
            "detail.ifccad",
            "same.ifccad".into(),
            Arc::new(vec![1]),
        )
        .unwrap();
    assert_eq!(
        a.resource_binding(2, 1, "detail.ifccad")
            .unwrap()
            .resource_id,
        id
    );
    assert!(a.resource_binding(3, 1, "detail.ifccad").is_none());
    assert!(b.resource_binding(2, 1, "detail.ifccad").is_none());
    assert!(a.resource_binding(2, 1, "other.ifccad").is_none());
}

#[test]
fn browser_explicit_reuse_preserves_resource_identity() {
    let session = IfccadSession::default();
    let id = session
        .bind_resource_bytes(1, 1, "B", "same.ifccad".into(), Arc::new(vec![1]))
        .unwrap();
    let other = session
        .bind_resource_bytes(1, 2, "C", "same.ifccad".into(), Arc::new(vec![1]))
        .unwrap();
    assert_ne!(id, other);
    session.reuse_source_resource(id, 1, "A", 1).unwrap();
    assert_eq!(session.resource_binding(id, 1, "A").unwrap().resource_id, 1);
    session.reuse_source_resource(other, 1, "B", id).unwrap();
    assert_eq!(
        session.resource_binding(other, 1, "B").unwrap().resource_id,
        id
    );
    assert!(IfccadSession::default()
        .reuse_source_resource(1, 1, "B", id)
        .is_err());
}
