//! Built-in functions. Names are case-insensitive; trig follows the angle mode.

use std::f64::consts::TAU;
use std::ops::Range;

use glam::DVec3;

use super::eval::{finite, number};
use super::parser::Expr;
use super::value::{CalcContext, Value};
use super::CalcError;

/// Every built-in function name; the tests call each one.
#[cfg(test)]
pub(super) const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sec", "csc", "cot", "asec", "acsc",
    "acot", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "sqrt", "cbrt", "root", "abs",
    "round", "floor", "ceil", "fact", "ln", "log", "logb", "exp", "pow", "min", "max", "d2r",
    "r2d", "dist", "ang", "mid", "xof", "yof", "zof", "getvar",
];

pub(super) fn call(
    name: &str,
    values: &[Value],
    args: &[Expr],
    span: Range<usize>,
    ctx: &CalcContext,
) -> Result<Value, CalcError> {
    let lower = name.to_ascii_lowercase();
    let mode = ctx.angle_mode;
    let arity = |range: std::ops::RangeInclusive<usize>, shown: &str| {
        if range.contains(&values.len()) {
            Ok(())
        } else {
            Err(CalcError::new(
                span.clone(),
                crate::tf!(
                    "{}() expects {} argument(s), found {}",
                    lower,
                    shown,
                    values.len()
                ),
            ))
        }
    };
    let num = |i: usize| number(values[i], args[i].span.clone());
    let point = |i: usize| match values[i] {
        Value::Point(p) => Ok(p),
        Value::Number(_) => Err(CalcError::new(
            args[i].span.clone(),
            crate::t!("expected a point, found a number"),
        )),
    };
    let unary = |f: fn(f64) -> f64| -> Result<Value, CalcError> {
        arity(1..=1, "1")?;
        Ok(Value::Number(f(num(0)?)))
    };

    let value = match lower.as_str() {
        "sin" | "cos" | "tan" | "sec" | "csc" | "cot" => {
            arity(1..=1, "1")?;
            let a = mode.to_radians(num(0)?);
            Value::Number(match lower.as_str() {
                "sin" => a.sin(),
                "cos" => a.cos(),
                "tan" => a.tan(),
                "sec" => 1.0 / a.cos(),
                "csc" => 1.0 / a.sin(),
                _ => 1.0 / a.tan(),
            })
        }
        "asin" | "acos" | "atan" | "asec" | "acsc" | "acot" => {
            arity(1..=1, "1")?;
            let x = num(0)?;
            let a = match lower.as_str() {
                "asin" => x.asin(),
                "acos" => x.acos(),
                "atan" => x.atan(),
                "asec" => (1.0 / x).acos(),
                "acsc" => (1.0 / x).asin(),
                // acot(0) is a quarter turn, from either side.
                _ if x == 0.0 => std::f64::consts::FRAC_PI_2,
                _ => (1.0 / x).atan(),
            };
            Value::Number(mode.radians_to_mode(a))
        }
        // Hyperbolic functions take and give plain numbers, whatever the
        // angle mode.
        "sinh" => unary(f64::sinh)?,
        "cosh" => unary(f64::cosh)?,
        "tanh" => unary(f64::tanh)?,
        "asinh" => unary(f64::asinh)?,
        "acosh" => unary(f64::acosh)?,
        "atanh" => unary(f64::atanh)?,
        "cbrt" => unary(f64::cbrt)?,
        "fact" => unary(factorial)?,
        "root" => {
            arity(2..=2, "2")?;
            Value::Number(nth_root(num(0)?, num(1)?))
        }
        "logb" => {
            arity(2..=2, "2")?;
            Value::Number(num(0)?.ln() / num(1)?.ln())
        }
        "atan2" => {
            arity(2..=2, "2")?;
            Value::Number(mode.radians_to_mode(num(0)?.atan2(num(1)?)))
        }
        "sqrt" => unary(f64::sqrt)?,
        "abs" => unary(f64::abs)?,
        "floor" => unary(f64::floor)?,
        "ceil" => unary(f64::ceil)?,
        "ln" => unary(f64::ln)?,
        "log" => unary(f64::log10)?,
        "exp" => unary(f64::exp)?,
        "d2r" => unary(f64::to_radians)?,
        "r2d" => unary(f64::to_degrees)?,
        "round" => {
            arity(1..=2, "1-2")?;
            let x = num(0)?;
            if values.len() == 1 {
                Value::Number(x.round())
            } else {
                let places = num(1)?.round().clamp(-15.0, 15.0);
                let scale = 10f64.powf(places);
                Value::Number((x * scale).round() / scale)
            }
        }
        "pow" => {
            arity(2..=2, "2")?;
            Value::Number(num(0)?.powf(num(1)?))
        }
        "min" | "max" => {
            arity(1..=usize::MAX, "1+")?;
            let mut best = num(0)?;
            for i in 1..values.len() {
                let x = num(i)?;
                best = if lower == "min" {
                    best.min(x)
                } else {
                    best.max(x)
                };
            }
            Value::Number(best)
        }
        "dist" => {
            arity(2..=2, "2")?;
            Value::Number(point(0)?.distance(point(1)?))
        }
        "ang" => {
            // ang(v): direction of a vector; ang(p1,p2): from p1 towards p2.
            // Measured in the XY plane, counter-clockwise from +X.
            arity(1..=2, "1-2")?;
            let v: DVec3 = if values.len() == 1 {
                point(0)?
            } else {
                point(1)? - point(0)?
            };
            Value::Number(mode.radians_to_mode(v.y.atan2(v.x).rem_euclid(TAU)))
        }
        "mid" => {
            arity(2..=2, "2")?;
            Value::Point(point(0)?.midpoint(point(1)?))
        }
        "xof" | "yof" | "zof" => {
            arity(1..=1, "1")?;
            let p = point(0)?;
            Value::Number(match lower.as_str() {
                "xof" => p.x,
                "yof" => p.y,
                _ => p.z,
            })
        }
        _ => {
            return Err(CalcError::new(
                span,
                crate::tf!("unknown function '{}'", name),
            ))
        }
    };
    finite(value, span)
}

/// `n!` for whole numbers 0..=170 (171! overflows); anything else has no
/// real result.
fn factorial(n: f64) -> f64 {
    if n < 0.0 || n.fract() != 0.0 || n > 170.0 {
        return f64::NAN;
    }
    (1..=n as u32).map(f64::from).product()
}

/// The `n`-th root of `x`; a negative `x` has one only for odd whole `n`.
fn nth_root(x: f64, n: f64) -> f64 {
    if n == 0.0 {
        return f64::NAN;
    }
    let odd = n.fract() == 0.0 && n.rem_euclid(2.0) == 1.0;
    if x < 0.0 && odd {
        -(-x).powf(1.0 / n)
    } else {
        x.powf(1.0 / n)
    }
}
