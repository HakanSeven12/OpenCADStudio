//! STEP (ISO 10303) as data: the exchange structure a file is read into ([`p21`]), the units
//! of its representation contexts ([`units`]), its Part 42 curves and surfaces ([`geom`])
//! as the entity reader ([`read`]) maps them, its product structure ([`product`]), the
//! protocol its header declares ([`protocol`]), and the neutral plain-data B-rep model its bodies
//! are mapped to ([`model`]). Building bodies from it
//! is `solidcore-ops`' job (docs/layers.md §2, "Importers are solidcore's").
pub mod geom;
pub mod model;
pub mod p21;
pub mod product;
pub mod protocol;
pub mod read;
pub mod units;

pub use model::Model;
