// Tests for text_width_factor.
use super::*;
use crate::t;

use super::*;
use document::tables::{DimStyle as DimStyleRecord, TextStyle};

#[test]
fn dimension_text_takes_its_style_width_factor() {
    // A drawing whose dimension style writes with a 0.6-wide text style:
    // AutoCAD 2025 draws the measurement 0.6 as wide; OCS drew it at 1.0.
    let mut doc = Document::default();
    let mut narrow = TextStyle::new("narrow");
    narrow.handle = doc.allocate_handle();
    narrow.width_factor = 0.6;
    let narrow_handle = narrow.handle;
    doc.text_styles.add(narrow).unwrap();
    let mut style = DimStyleRecord::new("NarrowDims");
    style.dimtxsty = "narrow".to_string();
    style.dimtxsty_handle = narrow_handle;
    doc.dim_styles.add(style).unwrap();

    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(200.0, 0.0, 0.0));
    d.base.style_name = "NarrowDims".to_string();
    let Some(EntityType::Text(text)) =
        baked_dimension_text_entity(&Dimension::Linear(d), &doc, 1.0)
    else {
        panic!("a plain measurement is synthesized as TEXT");
    };
    assert!(
        (text.width_factor - 0.6).abs() < 1.0e-6,
        "dimension text uses its style's width factor, got {}",
        text.width_factor
    );
}
