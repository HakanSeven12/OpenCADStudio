// Core ribbon module registry — one boxed instance of every built-in CAD
// module, in ribbon-tab display order. External add-ons are appended on top
// of this list at runtime by `plugin::registry`.
//
// To add a core module:
//   1. Create src/modules/my_name/mod.rs  (implement CadModule as MyNameModule)
//   2. Add `pub mod my_name;` to src/modules/mod.rs
//   3. Add one `Box::new(super::my_name::MyNameModule)` line below, in the
//      position you want its tab to appear.
use crate::modules::CadModule;

/// Returns one boxed instance of every registered core CAD module.
/// Called once at startup by `Ribbon::new()`.
pub fn all_modules() -> Vec<Box<dyn CadModule>> {
    vec![
        Box::new(super::draw::DrawModule),
        Box::new(super::parametric::ParametricModule),
        Box::new(super::model::ModelModule),
        Box::new(super::insert::InsertModule),
        Box::new(super::annotate::AnnotateModule),
        Box::new(super::view::ViewModule),
        Box::new(super::manage::ManageModule),
        Box::new(super::layout::LayoutModule),
    ]
}
