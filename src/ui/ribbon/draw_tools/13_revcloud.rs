Tool {
    command: "REVCLOUD",
    label: "Revision Cloud",
    icon: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/revcloud.svg")),
    options: &[
        ("REVCLOUD_RECTANGULAR", "Rectangular"),
        ("REVCLOUD_POLYGONAL", "Polygonal"),
        ("REVCLOUD_FREEHAND", "Freehand"),
    ],
}
