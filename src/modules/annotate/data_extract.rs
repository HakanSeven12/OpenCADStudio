// DATAEXTRACTION command — launches the Data Extraction wizard.

use crate::modules::{IconKind, ToolDef};

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/data_extract.svg"));

pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("DATAEXTRACTION")
}
