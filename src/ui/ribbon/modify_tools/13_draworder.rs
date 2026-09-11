Tool {
    command: "DRAWORDER_FRONT",
    label: "Draw Order",
    icon: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/modify_draworder.svg")),
    options: &[("DRAWORDER_FRONT", "Bring to Front"), ("DRAWORDER_BACK", "Send to Back"), ("DRAWORDER_ABOVE", "Bring Above Objects"), ("DRAWORDER_UNDER", "Send Under Objects")],
}
