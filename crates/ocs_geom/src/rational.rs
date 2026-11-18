use crate::geom2d::{Curve, Ellipse, NurbsCurve};
use crate::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};
use std::f64::consts::{FRAC_PI_2, TAU};

/// A 2D curve as a rational B-spline: lines, arcs, ellipses and NURBS alike.
#[derive(Clone)]
pub struct RationalCurve2 {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub points: Vec<[f64; 2]>,
    pub weights: Vec<f64>,
}

/// A [`RationalCurve2`] lifted into a plane in space.
#[derive(Clone)]
pub struct RationalCurve3 {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub points: Vec<[f64; 3]>,
    pub weights: Vec<f64>,
}

impl RationalCurve2 {
    pub fn from_curve(curve: &Curve) -> Option<Self> {
        match curve {
            Curve::Line(line) => Some(Self {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![line.start, line.end],
                weights: vec![1.0; 2],
            }),
            Curve::Circle(circle) => conic(
                Ellipse {
                    centre: circle.centre,
                    major_radius: circle.radius,
                    minor_radius: circle.radius,
                    major_axis: [1.0, 0.0],
                },
                0.0,
                TAU,
            ),
            Curve::Arc(arc) => conic(
                Ellipse {
                    centre: arc.centre,
                    major_radius: arc.radius,
                    minor_radius: arc.radius,
                    major_axis: [1.0, 0.0],
                },
                arc.start_angle,
                arc.sweep(),
            ),
            Curve::Ellipse(arc) => conic(arc.ellipse, arc.start_parameter, arc.sweep()),
            Curve::Nurbs(curve) => from_nurbs(curve),
            _ => None,
        }
    }

    pub fn reversed(&self) -> Self {
        Self {
            degree: self.degree,
            knots: self.knots.iter().rev().map(|value| 1.0 - value).collect(),
            points: self.points.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
        }
    }

    pub fn curve(&self) -> Option<NurbsCurve> {
        NurbsCurve::new_strict(
            self.degree,
            self.points.clone(),
            self.knots.clone(),
            self.weights.clone(),
        )
    }

    pub fn unit_arc(sweep: f64) -> Option<Self> {
        conic(
            Ellipse {
                centre: [0.0, 0.0],
                major_radius: 1.0,
                minor_radius: 1.0,
                major_axis: [1.0, 0.0],
            },
            0.0,
            sweep,
        )
    }

    pub fn lifted(&self, plane: &Plane) -> RationalCurve3 {
        RationalCurve3 {
            degree: self.degree,
            knots: self.knots.clone(),
            points: self
                .points
                .iter()
                .map(|point| plane.point_at(*point))
                .collect(),
            weights: self.weights.clone(),
        }
    }
}

impl RationalCurve3 {
    pub fn reversed(&self) -> Self {
        Self {
            degree: self.degree,
            knots: self.knots.iter().rev().map(|value| 1.0 - value).collect(),
            points: self.points.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
        }
    }

    pub fn translated(&self, offset: Vec3) -> Self {
        Self {
            degree: self.degree,
            knots: self.knots.clone(),
            points: self
                .points
                .iter()
                .map(|point| (Vec3::from(*point) + offset).to_array())
                .collect(),
            weights: self.weights.clone(),
        }
    }

    pub fn curve(&self) -> Option<NurbsCurve3> {
        NurbsCurve3::new_strict(
            self.degree,
            self.points.clone(),
            self.knots.clone(),
            self.weights.clone(),
        )
    }

    pub fn compatible_with(&self, other: &Self) -> bool {
        self.degree == other.degree
            && self.points.len() == other.points.len()
            && self.knots.len() == other.knots.len()
            && self
                .knots
                .iter()
                .zip(&other.knots)
                .all(|(a, b)| (a - b).abs() <= 1e-10)
    }

    pub fn ruled_to(&self, other: &Self) -> Option<NurbsSurface3> {
        self.compatible_with(other).then_some(())?;
        let points = self
            .points
            .iter()
            .zip(&other.points)
            .map(|(a, b)| vec![*a, *b])
            .collect();
        let weights = self
            .weights
            .iter()
            .zip(&other.weights)
            .map(|(a, b)| vec![*a, *b])
            .collect();
        NurbsSurface3::new_strict(
            self.degree,
            1,
            points,
            self.knots.clone(),
            vec![0.0, 0.0, 1.0, 1.0],
            weights,
        )
    }
}

fn from_nurbs(curve: &NurbsCurve) -> Option<RationalCurve2> {
    let (start, end) = curve.domain();
    let span = end - start;
    if !span.is_finite() || span <= 0.0 {
        return None;
    }
    Some(RationalCurve2 {
        degree: curve.degree(),
        knots: curve
            .knots()
            .iter()
            .map(|value| (value - start) / span)
            .collect(),
        points: curve.control_points().to_vec(),
        weights: curve.weights().to_vec(),
    })
}

fn conic(ellipse: Ellipse, start: f64, sweep: f64) -> Option<RationalCurve2> {
    if !start.is_finite() || !sweep.is_finite() || sweep <= 0.0 {
        return None;
    }
    let spans = (sweep / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / spans as f64;
    if step > FRAC_PI_2 + 1e-12 {
        return None;
    }
    let mut points = Vec::with_capacity(spans * 2 + 1);
    let mut weights = Vec::with_capacity(spans * 2 + 1);
    for span in 0..spans {
        let a = start + step * span as f64;
        let b = a + step;
        let middle = (a + b) * 0.5;
        let weight = (step * 0.5).cos();
        if span == 0 {
            points.push(ellipse.point_at(a));
            weights.push(1.0);
        }
        let major = ellipse.major_axis;
        let minor = ellipse.minor_axis();
        points.push([
            ellipse.centre[0]
                + ellipse.major_radius * middle.cos() * major[0] / weight
                + ellipse.minor_radius * middle.sin() * minor[0] / weight,
            ellipse.centre[1]
                + ellipse.major_radius * middle.cos() * major[1] / weight
                + ellipse.minor_radius * middle.sin() * minor[1] / weight,
        ]);
        weights.push(weight);
        points.push(ellipse.point_at(b));
        weights.push(1.0);
    }
    let mut knots = vec![0.0; 3];
    for split in 1..spans {
        knots.extend([split as f64 / spans as f64; 2]);
    }
    knots.extend([1.0; 3]);
    Some(RationalCurve2 {
        degree: 2,
        knots,
        points,
        weights,
    })
}
