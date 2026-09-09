//! The object-centric facade (plan §8.1, decision #12).
//!
//! Hierarchy: [`DocApi`] (root: session + transport) → [`Document`] →
//! collections ([`SolidCollection`], [`CurveCollection`], [`EntityCollection`])
//! → typed handles ([`Solid`], [`Line`], [`Circle`], [`Polyline`], [`Point`],
//! [`Entity`]) carrying methods. Every handle = `{ ObjectId, Arc<Session> }` —
//! `Clone`, `Send`, `Sync`; it never holds kernel geometry. Per-op autocommit:
//! each method is ONE atomic host call = one undo step (no transactions).

use std::sync::Arc;

use crate::envelope::{DocApiEnvelope, Receipt};
use crate::error::{ApiError, ApiResult};
use crate::gen::{Operation, Query};
use crate::id::ObjectId;
use crate::ops::{BoolOp, Curve2Spec, PlacementSpec, SolidPrimitive};
use crate::query::{Aabb, EntityView, QueryResult};
use crate::revision::GeometryRevision;
use crate::transport::Transport;

/// Shared session: the transport. Held by every handle. The transport already
/// carries the tab binding (IPC) or the backend (in-process), so the session
/// stores no per-tab field.
#[derive(Clone)]
pub struct Session {
    transport: Arc<dyn Transport>,
}

impl Session {
    fn apply_op(&self, op: Operation) -> ApiResult<Receipt> {
        self.transport.apply(DocApiEnvelope::op(op))
    }
    fn apply_queries(&self, queries: Vec<Query>) -> ApiResult<Receipt> {
        self.transport.apply(DocApiEnvelope::queries(queries))
    }
    fn one_query(&self, q: Query) -> ApiResult<QueryResult> {
        let mut r = self.apply_queries(vec![q])?;
        let result = r.query_results.drain(..).next();
        result.ok_or_else(|| ApiError::Transport("empty query result".into()))
    }
}

/// The API root: owns the session (transport + active tab). Entry point.
#[derive(Clone)]
pub struct DocApi {
    transport: Arc<dyn Transport>,
    active_tab: u64,
}

impl DocApi {
    /// Connect over an existing transport (IPC or in-process) bound to `active_tab`.
    pub fn connect(transport: Arc<dyn Transport>, active_tab: u64) -> Self {
        Self { transport, active_tab }
    }
    /// Connect in-process over a backend (host feature).
    #[cfg(feature = "host")]
    pub fn in_process<B: crate::backend::DocApiBackend + Send + 'static>(
        backend: B,
        active_tab: u64,
    ) -> Self {
        Self::connect(Arc::new(crate::transport::InProcess::new(backend)), active_tab)
    }

    /// The active tab id this root is bound to.
    pub fn active_tab(&self) -> u64 {
        self.active_tab
    }

    /// Liveness probe of the host/transport.
    pub fn alive(&self) -> bool {
        self.transport.alive()
    }

    /// Bind a `Document` to `tab`. In v1 the transport is already bound to the
    /// active tab (IPC carries `tab_id`; in-process carries the backend), so the
    /// argument is accepted for API forward-compat but not stored.
    pub fn document(&self, _tab: u64) -> Document {
        Document {
            session: Session { transport: Arc::clone(&self.transport) },
        }
    }

    // Host console passthroughs (UX feedback; map to PushInfo/PushError on the host).
    pub fn push_info(&self, msg: &str) {
        let _ = msg; // host console passthrough is a host-side concern; facade is silent.
    }
    pub fn push_error(&self, msg: &str) {
        let _ = msg;
    }
}

/// One open document tab: factory + lookup + queries + read-guards.
#[derive(Clone)]
pub struct Document {
    session: Session,
}

impl Document {
    pub fn solids(&self) -> SolidCollection {
        SolidCollection { session: self.session.clone() }
    }
    pub fn curves(&self) -> CurveCollection {
        CurveCollection { session: self.session.clone() }
    }
    pub fn entities(&self) -> EntityCollection {
        EntityCollection { session: self.session.clone() }
    }

    /// Current geometry revision (query, no bump).
    pub fn revision(&self) -> ApiResult<GeometryRevision> {
        match self.session.one_query(Query::GetGeometryRevision)? {
            QueryResult::Revision(r) => Ok(r),
            _ => Err(ApiError::Transport("unexpected revision result".into())),
        }
    }

    /// Standalone read-guard: fail with `ApiError::Validation` if the document's
    /// revision moved past `expected` (read-modify-write guard; NOT a batch pre-condition).
    pub fn assert_revision(&self, expected: GeometryRevision) -> ApiResult<()> {
        let now = self.revision()?;
        if now == expected {
            Ok(())
        } else {
            Err(ApiError::Validation {
                op: "assert_revision".to_string(),
                reason: format!("revision moved: expected {expected:?}, now {now:?}; re-read and retry"),
            })
        }
    }

    /// Read-only traversal of a block definition's entities (ids + kinds + bounds).
    pub fn block_entities(&self, block_name: &str) -> ApiResult<Vec<EntityView>> {
        match self.session.one_query(Query::GetBlockEntities { block_name: block_name.to_string() })? {
            QueryResult::BlockEntities(v) => Ok(v),
            _ => Err(ApiError::Transport("unexpected block-entities result".into())),
        }
    }

    /// Batch of read-only queries in ONE round-trip (safe: no mutation/undo).
    /// The closure records queries on a [`QueryBatch`]; the results are returned
    /// in the same order as a [`QueryResults`] view the caller destructures.
    pub fn query_batch<F: FnOnce(&mut QueryBatch)>(&self, f: F) -> ApiResult<QueryResults> {
        let mut qb = QueryBatch { queries: Vec::new() };
        f(&mut qb);
        let receipt = self.session.apply_queries(qb.queries)?;
        Ok(QueryResults { results: receipt.query_results })
    }
}

/// Records queries for [`Document::query_batch`]. Each method appends one query.
pub struct QueryBatch {
    queries: Vec<Query>,
}

impl QueryBatch {
    pub fn bounds(&mut self, e: &impl HasId) {
        self.queries.push(Query::GetBounds { id: e.id() });
    }
    pub fn volume(&mut self, e: &impl HasId) {
        self.queries.push(Query::GetVolume { id: e.id() });
    }
    pub fn centroid(&mut self, e: &impl HasId) {
        self.queries.push(Query::GetCentroid { id: e.id() });
    }
    pub fn intersects(&mut self, a: &impl HasId, b: &impl HasId) {
        self.queries.push(Query::GetIntersects { a: a.id(), b: b.id() });
    }
    pub fn revision(&mut self) {
        self.queries.push(Query::GetGeometryRevision);
    }
}

/// Ordered results of a [`Document::query_batch`], one per recorded query.
pub struct QueryResults {
    results: Vec<QueryResult>,
}

impl QueryResults {
    fn at(&self, i: usize) -> ApiResult<&QueryResult> {
        self.results.get(i).ok_or_else(|| ApiError::Transport("query result missing".into()))
    }
    pub fn bounds(&self, i: usize) -> ApiResult<Aabb> {
        match self.at(i)? {
            QueryResult::Bounds(b) => Ok(*b),
            _ => Err(ApiError::Transport("result is not bounds".into())),
        }
    }
    pub fn volume(&self, i: usize) -> ApiResult<f64> {
        match self.at(i)? {
            QueryResult::Volume(v) => Ok(*v),
            _ => Err(ApiError::Transport("result is not volume".into())),
        }
    }
    pub fn centroid(&self, i: usize) -> ApiResult<[f64; 3]> {
        match self.at(i)? {
            QueryResult::Centroid(c) => Ok(*c),
            _ => Err(ApiError::Transport("result is not centroid".into())),
        }
    }
    pub fn intersects(&self, i: usize) -> ApiResult<bool> {
        match self.at(i)? {
            QueryResult::Intersects(x) => Ok(*x),
            _ => Err(ApiError::Transport("result is not intersects".into())),
        }
    }
    pub fn revision(&self, i: usize) -> ApiResult<GeometryRevision> {
        match self.at(i)? {
            QueryResult::Revision(r) => Ok(*r),
            _ => Err(ApiError::Transport("result is not revision".into())),
        }
    }
}

/// Something with an `ObjectId` (all handles + raw ids).
pub trait HasId {
    fn id(&self) -> ObjectId;
}
impl HasId for ObjectId {
    fn id(&self) -> ObjectId {
        *self
    }
}

// ── Typed handles ────────────────────────────────────────────────────────────

macro_rules! handle {
    ($name:ident) => {
        #[derive(Clone)]
        pub struct $name {
            session: Session,
            id: ObjectId,
        }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "({:?})"), self.id)
            }
        }
        impl $name {
            fn new(session: Session, id: ObjectId) -> Self {
                Self { session, id }
            }
        }
        impl HasId for $name {
            fn id(&self) -> ObjectId {
                self.id
            }
        }
        impl $name {
            pub fn bounds(&self) -> ApiResult<Aabb> {
                match self.session.one_query(Query::GetBounds { id: self.id })? {
                    QueryResult::Bounds(b) => Ok(b),
                    _ => Err(ApiError::Transport("unexpected bounds result".into())),
                }
            }
            pub fn delete(&self) -> ApiResult<()> {
                self.session.apply_op(Operation::Delete { id: self.id })?;
                Ok(())
            }
            pub fn transform(&self, placement: PlacementSpec) -> ApiResult<()> {
                self.session.apply_op(Operation::Transform { id: self.id, placement })?;
                Ok(())
            }
        }
    };
}

handle!(Entity);
handle!(Solid);
handle!(Line);
handle!(Circle);
handle!(Polyline);
handle!(Point);
handle!(ArcCurve);
handle!(Ellipse);
handle!(Spline);
handle!(Ray);
handle!(XLine);
handle!(Text);
handle!(MText);
handle!(Dimension);

impl Dimension {
    /// The measured value of this dimension (distance for linear/radius, degrees
    /// for angular).
    pub fn measurement(&self) -> ApiResult<f64> {
        match self.session.one_query(Query::GetDimensionMeasurement { id: self.id })? {
            QueryResult::DimensionMeasurement(v) => Ok(v),
            _ => Err(ApiError::Transport("unexpected measurement result".into())),
        }
    }
}

impl Entity {
    pub fn view(&self) -> ApiResult<EntityView> {
        match self.session.one_query(Query::GetEntity { id: self.id })? {
            QueryResult::Entity(v) => Ok(v),
            _ => Err(ApiError::Transport("unexpected entity result".into())),
        }
    }
    /// This insert's attributes as (tag, value) pairs. Insert-only.
    pub fn attributes(&self) -> ApiResult<Vec<(String, String)>> {
        match self.session.one_query(Query::GetAttributes { id: self.id })? {
            QueryResult::Attributes(v) => Ok(v),
            _ => Err(ApiError::Transport("unexpected attributes result".into())),
        }
    }
    /// Set an attribute `value` for `tag` on this insert (adds if absent). One undo step.
    pub fn set_attribute(&self, tag: &str, value: &str) -> ApiResult<()> {
        self.session.apply_op(Operation::SetAttribute { id: self.id, tag: tag.to_string(), value: value.to_string() })?;
        Ok(())
    }
    /// This viewport's view (target WCS + zoom height). Viewport-only.
    pub fn viewport_view(&self) -> ApiResult<([f64; 3], f64)> {
        match self.session.one_query(Query::GetViewportView { id: self.id })? {
            QueryResult::ViewportView { target, height } => Ok((target, height)),
            _ => Err(ApiError::Transport("unexpected viewport-view result".into())),
        }
    }
    /// Retarget / re-zoom this viewport (one undo step). Viewport-only.
    pub fn set_view(&self, view_target: [f64; 3], view_height: f64) -> ApiResult<()> {
        self.session.apply_op(Operation::SetViewportView { id: self.id, view_target, view_height })?;
        Ok(())
    }
    pub fn as_solid(&self) -> Option<Solid> {
        // Typed downcast is validated by the view's kind.
        matches!(self.view().ok()?.kind.as_str(), "Solid3D").then(|| Solid::new(self.session.clone(), self.id))
    }
}

impl Solid {
    fn boolean(&self, op: BoolOp, other: &Solid) -> ApiResult<Solid> {
        let receipt = self.session.apply_op(Operation::SolidBoolean {
            op,
            a: self.id,
            b: other.id,
            erase_sources: true,
        })?;
        let id = receipt
            .outcome
            .and_then(|o| o.new_id())
            .ok_or_else(|| ApiError::Transport("boolean returned no id".into()))?;
        Ok(Solid::new(self.session.clone(), id))
    }
    pub fn intersect(&self, other: &Solid) -> ApiResult<Solid> {
        self.boolean(BoolOp::Intersection, other)
    }
    pub fn union(&self, other: &Solid) -> ApiResult<Solid> {
        self.boolean(BoolOp::Union, other)
    }
    pub fn subtract(&self, other: &Solid) -> ApiResult<Solid> {
        self.boolean(BoolOp::Difference, other)
    }
    pub fn volume(&self) -> ApiResult<f64> {
        match self.session.one_query(Query::GetVolume { id: self.id })? {
            QueryResult::Volume(v) => Ok(v),
            _ => Err(ApiError::Transport("unexpected volume result".into())),
        }
    }
    pub fn centroid(&self) -> ApiResult<[f64; 3]> {
        match self.session.one_query(Query::GetCentroid { id: self.id })? {
            QueryResult::Centroid(c) => Ok(c),
            _ => Err(ApiError::Transport("unexpected centroid result".into())),
        }
    }
    pub fn intersects(&self, other: &Solid) -> ApiResult<bool> {
        match self.session.one_query(Query::GetIntersects { a: self.id, b: other.id })? {
            QueryResult::Intersects(x) => Ok(x),
            _ => Err(ApiError::Transport("unexpected intersects result".into())),
        }
    }
}

impl Polyline {
    pub fn add_vertex(&self, at: usize, point: [f64; 3]) -> ApiResult<()> {
        self.session.apply_op(Operation::AddVertex { id: self.id, at, point })?;
        Ok(())
    }
}

macro_rules! text_handle {
    ($name:ident) => {
        impl $name {
            /// The text content of this annotation.
            pub fn content(&self) -> ApiResult<String> {
                match self.session.one_query(Query::GetTextContent { id: self.id })? {
                    QueryResult::TextContent(s) => Ok(s),
                    _ => Err(ApiError::Transport("unexpected content result".into())),
                }
            }
            /// Replace the text content (one undo step).
            pub fn set_content(&self, value: &str) -> ApiResult<()> {
                self.session.apply_op(Operation::SetTextContent { id: self.id, value: value.to_string() })?;
                Ok(())
            }
        }
    };
}
text_handle!(Text);
text_handle!(MText);

// ── Collections (construction / lookup) ─────────────────────────────────────

#[derive(Clone)]
pub struct SolidCollection {
    session: Session,
}

impl SolidCollection {
    fn create(&self, prim: SolidPrimitive) -> ApiResult<Solid> {
        let receipt = self.session.apply_op(Operation::CreateSolid(prim))?;
        let id = receipt
            .outcome
            .and_then(|o| o.new_id())
            .ok_or_else(|| ApiError::Transport("create_solid returned no id".into()))?;
        Ok(Solid::new(self.session.clone(), id))
    }
    pub fn create_cuboid(&self, origin: [f64; 3], size: [f64; 3]) -> ApiResult<Solid> {
        self.create(SolidPrimitive::Cuboid { origin, size })
    }
    pub fn create_sphere(&self, centre: [f64; 3], radius: f64) -> ApiResult<Solid> {
        self.create(SolidPrimitive::Sphere { centre, radius })
    }
    pub fn create_cylinder(&self, base: [f64; 3], radius: f64, height: f64) -> ApiResult<Solid> {
        self.create(SolidPrimitive::Cylinder { base, radius, height })
    }
    pub fn create_cone(&self, base: [f64; 3], radius: f64, height: f64) -> ApiResult<Solid> {
        self.create(SolidPrimitive::Cone { base, radius, height })
    }
    pub fn create_torus(&self, centre: [f64; 3], major_radius: f64, minor_radius: f64) -> ApiResult<Solid> {
        self.create(SolidPrimitive::Torus { centre, major_radius, minor_radius })
    }
    pub fn create_wedge(&self, origin: [f64; 3], size: [f64; 3]) -> ApiResult<Solid> {
        self.create(SolidPrimitive::Wedge { origin, size })
    }
    pub fn extrude(&self, profile: &impl HasId, direction: [f64; 3]) -> ApiResult<Solid> {
        let receipt = self.session.apply_op(Operation::Extrude { profile: profile.id(), direction })?;
        let id = receipt
            .outcome
            .and_then(|o| o.new_id())
            .ok_or_else(|| ApiError::Transport("extrude returned no id".into()))?;
        Ok(Solid::new(self.session.clone(), id))
    }
    pub fn revolve(&self, profile: &impl HasId, pivot: [f64; 3], axis: [f64; 3], angle: f64) -> ApiResult<Solid> {
        let receipt = self.session.apply_op(Operation::Revolve {
            profile: profile.id(),
            axis: (pivot, axis),
            angle,
        })?;
        let id = receipt
            .outcome
            .and_then(|o| o.new_id())
            .ok_or_else(|| ApiError::Transport("revolve returned no id".into()))?;
        Ok(Solid::new(self.session.clone(), id))
    }
    /// Loft a solid through >= 2 profile entities (polylines/circles/arcs).
    pub fn loft(&self, profiles: &[impl HasId]) -> ApiResult<Solid> {
        let ids: Vec<ObjectId> = profiles.iter().map(|p| p.id()).collect();
        let receipt = self.session.apply_op(Operation::Loft { profiles: ids })?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("loft returned no id".into()))?;
        Ok(Solid::new(self.session.clone(), id))
    }
}

#[derive(Clone)]
pub struct CurveCollection {
    session: Session,
}

impl CurveCollection {
    fn create_curve(&self, spec: Curve2Spec) -> ApiResult<(Session, ObjectId)> {
        let receipt = self.session.apply_op(Operation::CreateCurve(spec))?;
        let id = receipt
            .outcome
            .and_then(|o| o.new_id())
            .ok_or_else(|| ApiError::Transport("create_curve returned no id".into()))?;
        Ok((self.session.clone(), id))
    }
    pub fn create_line(&self, start: [f64; 3], end: [f64; 3]) -> ApiResult<Line> {
        let (s, id) = self.create_curve(Curve2Spec::Line { start, end })?;
        Ok(Line::new(s, id))
    }
    pub fn create_circle(&self, centre: [f64; 3], radius: f64) -> ApiResult<Circle> {
        let (s, id) = self.create_curve(Curve2Spec::Circle { centre, radius })?;
        Ok(Circle::new(s, id))
    }
    pub fn create_polyline(&self, points: &[[f64; 3]], closed: bool) -> ApiResult<Polyline> {
        let (s, id) = self.create_curve(Curve2Spec::Polyline { points: points.to_vec(), closed })?;
        Ok(Polyline::new(s, id))
    }
    pub fn create_point(&self, position: [f64; 3]) -> ApiResult<Point> {
        let (s, id) = self.create_curve(Curve2Spec::Point { position })?;
        Ok(Point::new(s, id))
    }
    pub fn create_arc(&self, centre: [f64; 3], radius: f64, start_angle: f64, end_angle: f64) -> ApiResult<ArcCurve> {
        let (s, id) = self.create_curve(Curve2Spec::Arc { centre, radius, start_angle, end_angle })?;
        Ok(ArcCurve::new(s, id))
    }
    pub fn create_ellipse(&self, centre: [f64; 3], major_axis: [f64; 3], ratio: f64, start: f64, end: f64) -> ApiResult<Ellipse> {
        let (s, id) = self.create_curve(Curve2Spec::Ellipse { centre, major_axis, ratio, start, end })?;
        Ok(Ellipse::new(s, id))
    }
    pub fn create_spline(&self, degree: i32, control_points: &[[f64; 3]], knots: &[f64], weights: &[f64]) -> ApiResult<Spline> {
        let (s, id) = self.create_curve(Curve2Spec::Spline {
            degree,
            control_points: control_points.to_vec(),
            knots: knots.to_vec(),
            weights: weights.to_vec(),
        })?;
        Ok(Spline::new(s, id))
    }
    pub fn create_ray(&self, origin: [f64; 3], direction: [f64; 3]) -> ApiResult<Ray> {
        let (s, id) = self.create_curve(Curve2Spec::Ray { origin, direction })?;
        Ok(Ray::new(s, id))
    }
    pub fn create_xline(&self, origin: [f64; 3], direction: [f64; 3]) -> ApiResult<XLine> {
        let (s, id) = self.create_curve(Curve2Spec::XLine { origin, direction })?;
        Ok(XLine::new(s, id))
    }
    /// A RASTER_IMAGE placed at `insertion_point` (host auto-registers the image definition).
    pub fn create_raster_image(&self, file_path: &str, insertion_point: [f64; 3], u_vector: [f64; 3], v_vector: [f64; 3], size: [f64; 2]) -> ApiResult<Entity> {
        let receipt = self.session.apply_op(Operation::CreateRasterImage(crate::ops::RasterImageSpec {
            file_path: file_path.to_string(),
            insertion_point,
            u_vector,
            v_vector,
            size,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_raster_image returned no id".into()))?;
        Ok(Entity::new(self.session.clone(), id))
    }

    /// A solid (or pattern) HATCH over a single closed polyline boundary.
    pub fn create_hatch(&self, boundary: &[[f64; 2]], solid: bool) -> ApiResult<Entity> {
        let receipt = self.session.apply_op(Operation::CreateHatch(crate::ops::HatchSpec {
            boundary: boundary.to_vec(),
            solid,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_hatch returned no id".into()))?;
        Ok(Entity::new(self.session.clone(), id))
    }

    /// A 2-line angular DIMENSION: angle between lines (vertex→first) and (vertex→second).
    pub fn create_dimension_angular2ln(&self, vertex: [f64; 3], first_point: [f64; 3], second_point: [f64; 3], arc_location: [f64; 3]) -> ApiResult<Dimension> {
        let receipt = self.session.apply_op(Operation::CreateDimensionAngular2Ln(crate::ops::DimensionAngularSpec {
            vertex, first_point, second_point, arc_location,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_dimension returned no id".into()))?;
        Ok(Dimension::new(self.session.clone(), id))
    }
    /// A radial DIMENSION for the circle centered at `center` through `point`.
    pub fn create_dimension_radius(&self, center: [f64; 3], point: [f64; 3]) -> ApiResult<Dimension> {
        self.create_dim_radial(Operation::CreateDimensionRadius, center, point)
    }
    /// A diameter DIMENSION for the circle with chord points `center`/`point`.
    pub fn create_dimension_diameter(&self, center: [f64; 3], point: [f64; 3]) -> ApiResult<Dimension> {
        self.create_dim_radial(Operation::CreateDimensionDiameter, center, point)
    }
    fn create_dim_radial(
        &self,
        op: impl Fn(crate::ops::DimensionRadialSpec) -> Operation,
        center: [f64; 3],
        point: [f64; 3],
    ) -> ApiResult<Dimension> {
        let receipt = self.session.apply_op(op(crate::ops::DimensionRadialSpec { center, point }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_dimension returned no id".into()))?;
        Ok(Dimension::new(self.session.clone(), id))
    }
    /// A 3-point angular DIMENSION (vertex + one point on each leg).
    pub fn create_dimension_angular(&self, vertex: [f64; 3], first_point: [f64; 3], second_point: [f64; 3], arc_location: [f64; 3]) -> ApiResult<Dimension> {
        let receipt = self.session.apply_op(Operation::CreateDimensionAngular(crate::ops::DimensionAngularSpec {
            vertex, first_point, second_point, arc_location,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_dimension returned no id".into()))?;
        Ok(Dimension::new(self.session.clone(), id))
    }
    /// A linear DIMENSION between `first_point` and `second_point`, with the
    /// dimension line placed at `definition_point`. `measurement()` reads the value.
    pub fn create_dimension_linear(&self, first_point: [f64; 3], second_point: [f64; 3], definition_point: [f64; 3]) -> ApiResult<Dimension> {
        let receipt = self.session.apply_op(Operation::CreateDimensionLinear(crate::ops::DimensionSpec {
            first_point, second_point, definition_point,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_dimension returned no id".into()))?;
        Ok(Dimension::new(self.session.clone(), id))
    }

    /// Single-line TEXT annotation.
    pub fn create_text(&self, value: &str, insertion_point: [f64; 3], height: f64, rotation: f64) -> ApiResult<Text> {
        let receipt = self.session.apply_op(Operation::CreateText(crate::ops::TextSpec {
            value: value.to_string(),
            insertion_point,
            height,
            rotation,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_text returned no id".into()))?;
        Ok(Text::new(self.session.clone(), id))
    }
    /// Multi-line MTEXT annotation.
    pub fn create_mtext(&self, value: &str, insertion_point: [f64; 3], height: f64) -> ApiResult<MText> {
        let receipt = self.session.apply_op(Operation::CreateMText(crate::ops::MTextSpec {
            value: value.to_string(),
            insertion_point,
            height,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_mtext returned no id".into()))?;
        Ok(MText::new(self.session.clone(), id))
    }
    /// Bulk-create many points in ONE op (plan §5.3): all-or-nothing, one undo step.
    pub fn create_points(&self, positions: &[[f64; 3]]) -> ApiResult<Vec<Point>> {
        let specs: Vec<crate::ops::EntitySpec> = positions
            .iter()
            .map(|&p| crate::ops::EntitySpec::Curve(Curve2Spec::Point { position: p }))
            .collect();
        let receipt = self.session.apply_op(Operation::CreateMany(specs))?;
        let outcome = receipt.outcome.ok_or_else(|| ApiError::Transport("create_many returned no outcome".into()))?;
        Ok(outcome
            .new_ids()
            .iter()
            .map(|&id| Point::new(self.session.clone(), id))
            .collect())
    }
}

#[derive(Clone)]
pub struct EntityCollection {
    session: Session,
}

impl EntityCollection {
    /// Create a paper-space `VIEWPORT` (a `width`×`height` viewport at `center`
    /// looking at `view_target` with `view_height` zoom).
    pub fn create_viewport(&self, center: [f64; 3], width: f64, height: f64, view_target: [f64; 3], view_height: f64) -> ApiResult<Entity> {
        let receipt = self.session.apply_op(Operation::CreateViewport(crate::ops::ViewportSpec {
            center, width, height, view_target, view_height,
        }))?;
        let id = receipt
            .outcome
            .and_then(|o| o.new_id())
            .ok_or_else(|| ApiError::Transport("create_viewport returned no id".into()))?;
        Ok(Entity::new(self.session.clone(), id))
    }

    /// Create a TABLE from a grid of cell strings (`data[row][column]`), at a point.
    pub fn create_table(&self, insertion_point: [f64; 3], data: &[Vec<String>]) -> ApiResult<Entity> {
        let receipt = self.session.apply_op(Operation::CreateTable(crate::ops::TableSpec {
            insertion_point,
            data: data.to_vec(),
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_table returned no id".into()))?;
        Ok(Entity::new(self.session.clone(), id))
    }

    /// Create an ATTDEF (in-block attribute definition): tag/prompt/default at a point.
    pub fn create_attribute_definition(&self, tag: &str, prompt: &str, default_value: &str, insertion_point: [f64; 3], height: f64, rotation: f64) -> ApiResult<Entity> {
        let receipt = self.session.apply_op(Operation::CreateAttributeDefinition(crate::ops::AttributeDefinitionSpec {
            tag: tag.to_string(),
            prompt: prompt.to_string(),
            default_value: default_value.to_string(),
            insertion_point,
            height,
            rotation,
        }))?;
        let id = receipt.outcome.and_then(|o| o.new_id()).ok_or_else(|| ApiError::Transport("create_attribute_definition returned no id".into()))?;
        Ok(Entity::new(self.session.clone(), id))
    }

    /// Place a block reference (`INSERT`) for `block_name` (must exist).
    pub fn create_insert(&self, block_name: &str, insert_point: [f64; 3], scale: f64, rotation: f64) -> ApiResult<Entity> {
        let receipt = self.session.apply_op(Operation::CreateInsert(crate::ops::InsertSpec {
            block_name: block_name.to_string(),
            insert_point,
            scale,
            rotation,
        }))?;
        let id = receipt
            .outcome
            .and_then(|o| o.new_id())
            .ok_or_else(|| ApiError::Transport("create_insert returned no id".into()))?;
        Ok(Entity::new(self.session.clone(), id))
    }

    /// Generic lookup by id (any family) → `Entity` (downcast via `as_solid()`, …).
    pub fn get(&self, id: ObjectId) -> ApiResult<Entity> {
        // Validate existence eagerly so `get` errors surface as `UnknownId`.
        self.session.one_query(Query::GetEntity { id })?;
        Ok(Entity::new(self.session.clone(), id))
    }
    pub fn delete(&self, id: ObjectId) -> ApiResult<()> {
        self.session.apply_op(Operation::Delete { id })?;
        Ok(())
    }
    /// Bulk transform (one op, all-or-nothing).
    pub fn transform_many(&self, ids: &[ObjectId], placement: PlacementSpec) -> ApiResult<()> {
        self.session.apply_op(Operation::TransformMany { ids: ids.to_vec(), placement })?;
        Ok(())
    }
    /// Bulk delete (one op, all-or-nothing). Used by `OpGroup::compensate`.
    pub fn delete_many(&self, ids: &[ObjectId]) -> ApiResult<()> {
        self.session.apply_op(Operation::DeleteMany(ids.to_vec()))?;
        Ok(())
    }
}

// ── OpGroup: client-side failure-cleanup (NOT a transaction) ────────────────

/// Tracks handles created during a logical operation so a later failure can be
/// cleaned up (best-effort, sequential delete ops — plan §8.1). No atomicity,
/// no host involvement, no rollback.
#[derive(Default)]
pub struct OpGroup {
    created: Vec<ObjectId>,
}

impl OpGroup {
    pub fn new() -> Self {
        Self { created: Vec::new() }
    }
    /// Record a created handle (propagating any construction error); returns it
    /// for fluent `grp.track(doc.solids().create_cuboid(..)?)?` use.
    pub fn track<T: HasId>(&mut self, handle: ApiResult<T>) -> ApiResult<T> {
        let handle = handle?;
        self.created.push(handle.id());
        Ok(handle)
    }
    /// Keep everything: drop the tracking without deleting.
    pub fn commit(self) {}
    /// Best-effort cleanup: delete every tracked entity (one `DeleteMany` op).
    /// Does not resurrect already-consumed/deleted entities (plan §13 note).
    pub fn compensate(self, doc: &Document) -> ApiResult<()> {
        if self.created.is_empty() {
            return Ok(());
        }
        doc.entities().delete_many(&self.created)
    }
}
