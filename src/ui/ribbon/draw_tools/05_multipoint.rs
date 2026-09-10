Tool {
    command: "MULTIPOINT",
    label: "Multiple Points",
    icon: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/multipoint.svg")),
    options: &[("POINT", "Single Point"), ("MULTIPOINT", "Multiple Points"), ("DDPTYPE", "Point Style")],
}
