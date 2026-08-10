use acadrust::{CadDocument, EntityType};

use crate::command::EntityTransform;
use crate::scene::convert::acad_to_render::RenderEntity;
use crate::scene::model::object::{
    GripApply, GripDef, GripMenuAction, GripMenuItem, PropSection,
};
use crate::scene::convert::tess_util::FallbackGeometry;

pub trait RenderConvertible {
    fn to_render(&self, document: &CadDocument) -> Option<RenderEntity>;
}

/// Fallback geometry for entities not routed through the render
/// pipeline (Viewport, Insert, Hatch outline, Ole2Frame). Returns absolute
/// WCS `f32` points + snap/key vertices the dispatcher wraps into a
/// `WireModel`.
pub trait FallbackTess {
    fn fallback_geometry(&self) -> FallbackGeometry;
}

pub trait Grippable {
    fn grips(&self) -> Vec<GripDef>;
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply);
    /// Items shown in the hover popup that opens when the cursor
    /// dwells on a grip. Default is a single `Stretch` entry — i.e.
    /// the popup just confirms the default drag behaviour. Override
    /// per entity to add Add Vertex / Convert to Arc / Reverse Arrows /
    /// etc.
    fn grip_menu(&self, _grip_id: usize) -> Vec<GripMenuItem> {
        vec![GripMenuItem { label: "Stretch", action: GripMenuAction::Stretch }]
    }
    /// React to a popup-menu commit. Default no-op — `Stretch` is the
    /// "do nothing extra" path, the normal drag still happens on click.
    /// Entities override this to mutate themselves when the user picks
    /// Add Vertex / Remove Vertex / Convert / Reverse / …
    fn apply_grip_menu(&mut self, _grip_id: usize, _action: GripMenuAction) {}
    /// Same as `apply_grip_menu` but for actions that need a numeric
    /// follow-up (Lengthen / Radius / Arc Length / Rotate Text / …).
    /// The app prompts the user for a value on the command line and
    /// calls this with the parsed `f64`.
    fn apply_grip_menu_value(
        &mut self,
        _grip_id: usize,
        _action: GripMenuAction,
        _value: f64,
    ) {}
    /// If the popup action this entity defines for `grip_id` needs a
    /// numeric prompt, return the prompt string and the action so the
    /// app can stash a pending-value state and route the next typed
    /// number into `apply_grip_menu_value`. `None` for one-shot
    /// actions handled by `apply_grip_menu`.
    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        _action: GripMenuAction,
    ) -> Option<&'static str> {
        None
    }
    /// Convert an interactive cursor point into the numeric value consumed by
    /// `apply_grip_menu_value`. Actions without a point-driven form return
    /// `None`.
    fn grip_menu_point_value(
        &self,
        _grip_id: usize,
        _action: GripMenuAction,
        _point: glam::DVec3,
    ) -> Option<f64> {
        None
    }
}

pub trait PropertyEditable {
    /// Type-specific property groups (Geometry, Misc, Text, …) shown after the
    /// common General group. May be several groups; empty means none.
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection>;
    fn apply_geom_prop(&mut self, field: &str, value: &str);
}

pub trait Transformable {
    fn apply_transform(&mut self, t: &EntityTransform);
}

/// Inquiry-time mass / area / perimeter properties for entities whose
/// 2D footprint has a meaningful area or perimeter (Circle, Arc, Line,
/// LwPolyline, Ellipse). Entities outside this set get `None` via the
/// dispatcher.
#[derive(Clone, Copy, Debug)]
pub struct MassProps {
    pub area: f64,
    pub perimeter: f64,
    pub cx: f64,
    pub cy: f64,
}

pub trait MassPropsCalc {
    fn mass_props(&self) -> MassProps;
}

/// Read / replace the visible string of text-like entities (Text, MText,
/// AttributeDefinition, AttributeEntity). `replace` rewrites every
/// occurrence of `search` with `rep`.
pub trait TextContent {
    fn text_content(&self) -> Option<String>;
    fn replace_text(&mut self, search: &str, rep: &str);
}

/// Human-readable variant name used by Quick Select / Select Similar
/// filtering. Stable across releases — the strings here are the canonical
/// type identifiers throughout the UI.
pub fn entity_type_name(et: &EntityType) -> &str {
    match et {
        EntityType::Point(_) => "Point",
        EntityType::Line(_) => "Line",
        EntityType::Circle(_) => "Circle",
        EntityType::Arc(_) => "Arc",
        EntityType::Ellipse(_) => "Ellipse",
        EntityType::Polyline(_) => "Polyline",
        EntityType::Polyline2D(_) => "Polyline2D",
        EntityType::Polyline3D(_) => "Polyline3D",
        EntityType::LwPolyline(_) => "LwPolyline",
        EntityType::Text(_) => "Text",
        EntityType::MText(_) => "MText",
        EntityType::Spline(_) => "Spline",
        EntityType::Helix(_) => "Helix",
        EntityType::Dimension(_) => "Dimension",
        EntityType::Hatch(_) => "Hatch",
        EntityType::Solid(_) => "Solid",
        EntityType::Face3D(_) => "3DFace",
        EntityType::Insert(_) => "Insert",
        EntityType::Block(_) => "Block",
        EntityType::BlockEnd(_) => "BlockEnd",
        EntityType::Ray(_) => "Ray",
        EntityType::XLine(_) => "XLine",
        EntityType::Viewport(_) => "Viewport",
        EntityType::AttributeDefinition(_) => "AttributeDefinition",
        EntityType::AttributeEntity(_) => "Attribute",
        EntityType::Leader(_) => "Leader",
        EntityType::MultiLeader(_) => "MultiLeader",
        EntityType::MLine(_) => "MLine",
        EntityType::Mesh(_) => "Mesh",
        EntityType::RasterImage(_) => "RasterImage",
        EntityType::Solid3D(_) => "Solid3D",
        EntityType::Region(_) => "Region",
        EntityType::Body(_) => "Body",
        EntityType::Surface(_) => "Surface",
        EntityType::Table(_) => "Table",
        EntityType::Tolerance(_) => "Tolerance",
        EntityType::PolyfaceMesh(_) => "PolyfaceMesh",
        EntityType::Wipeout(_) => "Wipeout",
        EntityType::Shape(_) => "Shape",
        EntityType::Underlay(_) => "Underlay",
        EntityType::Seqend(_) => "Seqend",
        EntityType::Ole2Frame(_) => "Ole2Frame",
        EntityType::PolygonMesh(_) => "PolygonMesh",
        EntityType::Light(_) => "Light",
        EntityType::SectionSymbol(_) => "SectionSymbol",
        EntityType::ViewBorder(_) => "ViewBorder",
        EntityType::Extended(entity) => entity.class_name(),
        EntityType::Unknown(_) => "Unknown",
    }
}

pub trait EntityTypeOps {
    fn to_render_entity(&self, document: &CadDocument) -> Option<RenderEntity>;
    fn grips(&self) -> Vec<GripDef>;
    fn grip_menu(&self, grip_id: usize) -> Vec<GripMenuItem>;
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection>;
    fn apply_geom_prop(&mut self, field: &str, value: &str);
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply);
    fn apply_grip_menu(&mut self, grip_id: usize, action: GripMenuAction);
    fn grip_menu_value_prompt(
        &self,
        grip_id: usize,
        action: GripMenuAction,
    ) -> Option<&'static str>;
    fn grip_menu_point_value(
        &self,
        grip_id: usize,
        action: GripMenuAction,
        point: glam::DVec3,
    ) -> Option<f64>;
    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: GripMenuAction,
        value: f64,
    );
    fn apply_transform(&mut self, t: &EntityTransform);
    fn mass_props(&self) -> Option<MassProps>;
    fn text_content(&self) -> Option<String>;
    fn replace_text(&mut self, search: &str, rep: &str);
}

/// Per-dispatch-function entity-variant lists. Adding a new entity that
/// participates in a dispatch = adding one identifier to one list here
/// (instead of one full match arm to each of five `match self` blocks).
///
/// `dispatch!` expands `EntityType::$Variant(e) => $body` for each name.
macro_rules! dispatch {
    ($self:expr, |$e:ident| $body:expr, [$($variant:ident),* $(,)?], _ => $default:expr $(,)?) => {
        match $self {
            $(EntityType::$variant($e) => $body,)*
            _ => $default,
        }
    };
}

/// Generates `Grippable`, `PropertyEditable`, `Transformable` trait impls
/// that delegate to identically-named free functions in the entity's own
/// module (`grips`, `apply_grip`, `properties`, `apply_geom_prop`,
/// `apply_transform`). `properties()` is called with `self` only — for
/// text-like entities that need the document's text-style list use
/// [`impl_entity_basics_with_text_styles!`] instead.
#[macro_export]
macro_rules! impl_entity_basics {
    ($T:ty) => {
        impl $crate::entities::traits::Grippable for $T {
            fn grips(&self) -> Vec<$crate::scene::model::object::GripDef> {
                grips(self)
            }
            fn apply_grip(
                &mut self,
                grip_id: usize,
                apply: $crate::scene::model::object::GripApply,
            ) {
                apply_grip(self, grip_id, apply);
            }
        }
        impl $crate::entities::traits::PropertyEditable for $T {
            fn geometry_properties(
                &self,
                _text_style_names: &[String],
            ) -> Vec<$crate::scene::model::object::PropSection> {
                properties(self)
            }
            fn apply_geom_prop(&mut self, field: &str, value: &str) {
                apply_geom_prop(self, field, value);
            }
        }
        impl $crate::entities::traits::Transformable for $T {
            fn apply_transform(&mut self, t: &$crate::command::EntityTransform) {
                apply_transform(self, t);
            }
        }
    };
}

/// Same as [`impl_entity_basics!`] but the entity's `properties(...)` free
/// function takes the document's text-style name list as a second
/// argument (Text, MText, …).
#[macro_export]
macro_rules! impl_entity_basics_with_text_styles {
    ($T:ty) => {
        impl $crate::entities::traits::Grippable for $T {
            fn grips(&self) -> Vec<$crate::scene::model::object::GripDef> {
                grips(self)
            }
            fn apply_grip(
                &mut self,
                grip_id: usize,
                apply: $crate::scene::model::object::GripApply,
            ) {
                apply_grip(self, grip_id, apply);
            }
        }
        impl $crate::entities::traits::PropertyEditable for $T {
            fn geometry_properties(
                &self,
                text_style_names: &[String],
            ) -> Vec<$crate::scene::model::object::PropSection> {
                properties(self, text_style_names)
            }
            fn apply_geom_prop(&mut self, field: &str, value: &str) {
                apply_geom_prop(self, field, value);
            }
        }
        impl $crate::entities::traits::Transformable for $T {
            fn apply_transform(&mut self, t: &$crate::command::EntityTransform) {
                apply_transform(self, t);
            }
        }
    };
}

impl EntityTypeOps for EntityType {
    fn to_render_entity(&self, document: &CadDocument) -> Option<RenderEntity> {
        dispatch!(self,
            |e| RenderConvertible::to_render(e, document),
            [
                Point, Line, Circle, Arc, Ellipse, Spline, LwPolyline,
                Polyline, Polyline2D, Polyline3D, Ray, XLine, RasterImage,
                Wipeout, AttributeDefinition, AttributeEntity, MLine,
                Tolerance, Solid, Face3D, PolygonMesh, PolyfaceMesh, Mesh,
                Table, Text, MText, Leader, MultiLeader, Underlay, Shape,
                Ole2Frame, Helix, Light, Extended,
            ],
            _ => None,
        )
    }

    fn grips(&self) -> Vec<GripDef> {
        dispatch!(self,
            |e| Grippable::grips(e),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Point, Spline, Text, MText,
                Viewport, Insert, Leader, MultiLeader, Dimension, Hatch,
                Underlay, Shape, Ole2Frame, Helix, Light, SectionSymbol,
                ViewBorder, Extended,
            ],
            _ => vec![],
        )
    }

    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
        dispatch!(self,
            |e| PropertyEditable::geometry_properties(e, text_style_names),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Hatch, Point, Spline, Text, MText,
                Viewport, Insert, Dimension, Leader, MultiLeader, Underlay,
                Shape, Ole2Frame, Helix, Light, SectionSymbol, ViewBorder,
                Extended,
            ],
            _ => vec![],
        )
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        dispatch!(self,
            |e| PropertyEditable::apply_geom_prop(e, field, value),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Hatch, Point, Spline, Text, MText,
                Viewport, Insert, Dimension, Leader, MultiLeader, Underlay,
                Shape, Ole2Frame, Helix, Light, SectionSymbol, ViewBorder,
                Extended,
            ],
            _ => {},
        )
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        dispatch!(self,
            |e| Grippable::apply_grip(e, grip_id, apply),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Point, Spline, Text, MText,
                Viewport, Insert, Leader, MultiLeader, Dimension, Hatch,
                Underlay, Shape, Ole2Frame, Helix, Light, SectionSymbol,
                ViewBorder, Extended,
            ],
            _ => {},
        )
    }

    fn grip_menu(&self, grip_id: usize) -> Vec<GripMenuItem> {
        dispatch!(self,
            |e| Grippable::grip_menu(e, grip_id),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Point, Spline, Text, MText,
                Viewport, Insert, Leader, MultiLeader, Dimension, Hatch,
                Underlay, Shape, Ole2Frame, SectionSymbol, ViewBorder,
                Extended,
            ],
            _ => vec![],
        )
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: GripMenuAction) {
        dispatch!(self,
            |e| Grippable::apply_grip_menu(e, grip_id, action),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Point, Spline, Text, MText,
                Viewport, Insert, Leader, MultiLeader, Dimension, Hatch,
                Underlay, Shape, Ole2Frame, SectionSymbol, ViewBorder,
                Extended,
            ],
            _ => {},
        )
    }

    fn grip_menu_value_prompt(
        &self,
        grip_id: usize,
        action: GripMenuAction,
    ) -> Option<&'static str> {
        dispatch!(self,
            |e| Grippable::grip_menu_value_prompt(e, grip_id, action),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Point, Spline, Text, MText,
                Viewport, Insert, Leader, MultiLeader, Dimension, Hatch,
                Underlay, Shape, Ole2Frame, SectionSymbol, ViewBorder,
                Extended,
            ],
            _ => None,
        )
    }

    fn grip_menu_point_value(
        &self,
        grip_id: usize,
        action: GripMenuAction,
        point: glam::DVec3,
    ) -> Option<f64> {
        dispatch!(self,
            |e| Grippable::grip_menu_point_value(e, grip_id, action, point),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Point, Spline, Text, MText,
                Viewport, Insert, Leader, MultiLeader, Dimension, Hatch,
                Underlay, Shape, Ole2Frame, SectionSymbol, ViewBorder,
                Extended,
            ],
            _ => None,
        )
    }

    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: GripMenuAction,
        value: f64,
    ) {
        dispatch!(self,
            |e| Grippable::apply_grip_menu_value(e, grip_id, action, value),
            [
                Line, Circle, Arc, Ellipse, LwPolyline, Polyline, Polyline2D,
                Polyline3D, Ray, XLine, RasterImage, Wipeout,
                AttributeDefinition, AttributeEntity, MLine, Tolerance,
                Solid, Solid3D, Region, Body, Surface, Face3D, PolygonMesh,
                PolyfaceMesh, Mesh, Table, Point, Spline, Text, MText,
                Viewport, Insert, Leader, MultiLeader, Dimension, Hatch,
                Underlay, Shape, Ole2Frame, SectionSymbol, ViewBorder,
            ],
            _ => {},
        )
    }

    fn apply_transform(&mut self, t: &EntityTransform) {
        dispatch!(self,
            |e| Transformable::apply_transform(e, t),
            [
                Arc, Circle, Ellipse, Hatch, Insert, Line, LwPolyline,
                Polyline, Polyline2D, Polyline3D, Ray, XLine, RasterImage,
                Wipeout, AttributeDefinition, AttributeEntity, MLine,
                Tolerance, Solid, Face3D, PolygonMesh, PolyfaceMesh, Mesh,
                Table, MText, Point, Spline, Text, Viewport, Dimension,
                Leader, MultiLeader, Underlay, Shape, Ole2Frame,
                Solid3D, Region, Body, Surface, Helix, Light, SectionSymbol,
                ViewBorder, Extended,
            ],
            _ => {},
        )
    }

    fn mass_props(&self) -> Option<MassProps> {
        dispatch!(self,
            |e| Some(MassPropsCalc::mass_props(e)),
            [Circle, Arc, Line, LwPolyline, Ellipse],
            _ => None,
        )
    }

    fn text_content(&self) -> Option<String> {
        dispatch!(self,
            |e| TextContent::text_content(e),
            [Text, MText, AttributeDefinition, AttributeEntity],
            _ => None,
        )
    }

    fn replace_text(&mut self, search: &str, rep: &str) {
        dispatch!(self,
            |e| TextContent::replace_text(e, search, rep),
            [Text, MText, AttributeDefinition, AttributeEntity],
            _ => {},
        )
    }
}
