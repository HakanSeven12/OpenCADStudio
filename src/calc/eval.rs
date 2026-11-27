//! Walks the syntax tree and computes a [`Value`].

use std::ops::Range;

use super::parser::{Expr, ExprKind};
use super::value::{CalcContext, SysVar, Value};
use super::{functions, units, CalcError};

pub(super) fn eval(expr: &Expr, ctx: &CalcContext) -> Result<Value, CalcError> {
    let value = match &expr.kind {
        ExprKind::Number(n) => Value::Number(*n),
        ExprKind::Quantity(n, unit) => {
            Value::Number(units::apply(*n, *unit, ctx.base_unit, ctx.angle_mode))
        }
        // Plain names are the built-in constants; variables carry a `$`.
        ExprKind::Ref(name) => {
            let lower = name.to_ascii_lowercase();
            match lower.as_str() {
                "pi" => Value::Number(std::f64::consts::PI),
                "e" => Value::Number(std::f64::consts::E),
                _ if ctx.vars.contains_key(&lower) => {
                    return Err(CalcError::new(
                        expr.span.clone(),
                        crate::tf!("Variables start with $: use ${}", lower),
                    ))
                }
                _ => {
                    return Err(CalcError::new(
                        expr.span.clone(),
                        crate::tf!("Unknown name {}", name),
                    ))
                }
            }
        }
        ExprKind::Var(name) => *ctx.vars.get(name).ok_or_else(|| {
            CalcError::new(expr.span.clone(), crate::tf!("Unknown variable ${}", name))
        })?,
        ExprKind::Ans => ctx.ans.ok_or_else(|| {
            CalcError::new(expr.span.clone(), crate::t!("no previous result yet"))
        })?,
        ExprKind::Neg(a) => match eval(a, ctx)? {
            Value::Number(n) => Value::Number(-n),
            Value::Point(p) => Value::Point(-p),
        },
        ExprKind::Add(a, b) => match (eval(a, ctx)?, eval(b, ctx)?) {
            (Value::Number(x), Value::Number(y)) => Value::Number(x + y),
            (Value::Point(p), Value::Point(q)) => Value::Point(p + q),
            _ => return Err(mixed(expr, "+")),
        },
        ExprKind::Sub(a, b) => match (eval(a, ctx)?, eval(b, ctx)?) {
            (Value::Number(x), Value::Number(y)) => Value::Number(x - y),
            (Value::Point(p), Value::Point(q)) => Value::Point(p - q),
            _ => return Err(mixed(expr, "-")),
        },
        ExprKind::Mul(a, b) => match (eval(a, ctx)?, eval(b, ctx)?) {
            (Value::Number(x), Value::Number(y)) => Value::Number(x * y),
            (Value::Point(p), Value::Number(k)) | (Value::Number(k), Value::Point(p)) => {
                Value::Point(p * k)
            }
            (Value::Point(_), Value::Point(_)) => {
                return Err(CalcError::new(
                    expr.span.clone(),
                    crate::t!("two points cannot be multiplied"),
                ))
            }
        },
        ExprKind::Div(a, b) => {
            let lhs = eval(a, ctx)?;
            let divisor = number(eval(b, ctx)?, b.span.clone())?;
            if divisor == 0.0 {
                return Err(CalcError::new(
                    b.span.clone(),
                    crate::t!("division by zero"),
                ));
            }
            match lhs {
                Value::Number(x) => Value::Number(x / divisor),
                Value::Point(p) => Value::Point(p / divisor),
            }
        }
        ExprKind::Mod(a, b) => {
            let x = number(eval(a, ctx)?, a.span.clone())?;
            let m = number(eval(b, ctx)?, b.span.clone())?;
            if m == 0.0 {
                return Err(CalcError::new(
                    b.span.clone(),
                    crate::t!("division by zero"),
                ));
            }
            Value::Number(x - m * (x / m).floor())
        }
        ExprKind::Pow(a, b) => {
            let base = number(eval(a, ctx)?, a.span.clone())?;
            let exponent = number(eval(b, ctx)?, b.span.clone())?;
            Value::Number(base.powf(exponent))
        }
        ExprKind::Call(name, args) if name.eq_ignore_ascii_case("getvar") => {
            getvar(args, expr.span.clone(), ctx)?
        }
        ExprKind::Call(name, args) => {
            let values = args
                .iter()
                .map(|arg| eval(arg, ctx))
                .collect::<Result<Vec<_>, _>>()?;
            functions::call(name, &values, args, expr.span.clone(), ctx)?
        }
        ExprKind::Point(coords) => {
            let mut xyz = [0.0; 3];
            for (slot, coord) in xyz.iter_mut().zip(coords) {
                *slot = number(eval(coord, ctx)?, coord.span.clone())?;
            }
            Value::Point(xyz.into())
        }
    };
    finite(value, expr.span.clone())
}

/// The number inside `value`, or an error pointing at `span`.
pub(super) fn number(value: Value, span: Range<usize>) -> Result<f64, CalcError> {
    match value {
        Value::Number(n) => Ok(n),
        Value::Point(_) => Err(CalcError::new(
            span,
            crate::t!("expected a number, found a point"),
        )),
    }
}

/// Infinity and NaN never leave the calculator: an overflow or a domain
/// error (`sqrt(-1)`, `asin(2)`) is reported where it happened.
pub(super) fn finite(value: Value, span: Range<usize>) -> Result<Value, CalcError> {
    let ok = match value {
        Value::Number(n) => n.is_finite(),
        Value::Point(p) => p.is_finite(),
    };
    if ok {
        Ok(value)
    } else {
        Err(CalcError::new(
            span,
            crate::t!("result is not a real number"),
        ))
    }
}

fn mixed(expr: &Expr, op: &str) -> CalcError {
    CalcError::new(
        expr.span.clone(),
        crate::tf!("'{}' does not work on a point and a number", op),
    )
}

/// `getvar(name)`: a numeric or point system variable, through the host's
/// reader. The argument is the bare name, as in `getvar(luprec)`.
fn getvar(args: &[Expr], span: Range<usize>, ctx: &CalcContext) -> Result<Value, CalcError> {
    let [Expr {
        kind: ExprKind::Ref(name),
        ..
    }] = args
    else {
        return Err(CalcError::new(
            span,
            crate::t!("getvar needs a system variable name, as in getvar(luprec)"),
        ));
    };
    let lower = name.to_ascii_lowercase();
    match ctx.sysvars.as_ref().and_then(|read| (read.0)(&lower)) {
        Some(SysVar::Number(n)) => Ok(Value::Number(n)),
        Some(SysVar::Point(p)) => Ok(Value::Point(p)),
        Some(SysVar::Text) => Err(CalcError::new(
            span,
            crate::tf!("getvar({}) is not a number", lower),
        )),
        None => Err(CalcError::new(
            args[0].span.clone(),
            crate::tf!("Unknown system variable {}", lower),
        )),
    }
}
