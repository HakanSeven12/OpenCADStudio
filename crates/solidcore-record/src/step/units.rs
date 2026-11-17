//! The units of a representation context (ISO 10303-41 measures and units, ISO 10303-43
//! representation contexts): `GLOBAL_UNIT_ASSIGNED_CONTEXT`'s length and plane angle units and
//! `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT`'s distance uncertainty, read from a parsed
//! [`Exchange`]. Missing or malformed units read as millimetres and radians; nothing here fails.
//!
//! A unit is a factor: millimetres per file length unit, radians per file plane angle unit. An
//! SI unit is its prefix times its base (`METRE` = 1000 mm, `RADIAN` = 1 rad); a
//! `CONVERSION_BASED_UNIT` is the value of its measure times the factor of the measure's own unit
//! (an inch is `LENGTH_MEASURE(25.4)` of millimetres, a degree `PLANE_ANGLE_MEASURE(0.01745…)`
//! of radians). Only multiplications: the factor of a degree is the number the file writes, not
//! a π computed here (the record computes no geometric quantity, docs/layers.md §4).
//!
//! Origin: SolidCraft's `crates/step/src/units.rs` and the `Units` type of
//! `crates/step/src/read.rs` at `a5b71ca2` (MIT OR Apache-2.0), carried over with their
//! behaviour unchanged.
//!
//! Deviations from SolidCraft: [`Units`] lives here (it was in `read.rs`, which is not carried
//! over yet). The nesting bound of a `CONVERSION_BASED_UNIT` chain is the named
//! `MAX_CONVERSION_DEPTH` (was the literal 4). Otherwise only formatting and doc comments on every
//! public item.

use super::p21::{Exchange, Param};

/// Most `CONVERSION_BASED_UNIT` links followed from one unit (a unit defined from a unit defined
/// from …); a deeper chain, or a cycle, reads as no unit.
const MAX_CONVERSION_DEPTH: usize = 4;

/// The units of a representation context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Units {
    /// Millimetres per file length unit.
    pub len: f64,
    /// Radians per file plane angle unit.
    pub ang: f64,
}

impl Default for Units {
    /// Millimetres and radians.
    fn default() -> Self {
        Units { len: 1.0, ang: 1.0 }
    }
}

/// What a representation context says about its numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Context {
    /// The length and plane angle units (millimetres and radians where the context names none
    /// that can be read).
    pub units: Units,
    /// The context's distance uncertainty in millimetres (the last `UNCERTAINTY_MEASURE_WITH_UNIT`
    /// it lists), if any.
    pub uncertainty: Option<f64>,
}

/// The factor of an SI prefix (`$` or an unknown prefix: 1).
fn si_prefix(p: Option<&Param>) -> f64 {
    match p.and_then(Param::as_enum) {
        Some("EXA") => 1e18,
        Some("PETA") => 1e15,
        Some("TERA") => 1e12,
        Some("GIGA") => 1e9,
        Some("MEGA") => 1e6,
        Some("KILO") => 1e3,
        Some("HECTO") => 1e2,
        Some("DECA") => 1e1,
        Some("DECI") => 1e-1,
        Some("CENTI") => 1e-2,
        Some("MILLI") => 1e-3,
        Some("MICRO") => 1e-6,
        Some("NANO") => 1e-9,
        Some("PICO") => 1e-12,
        Some("FEMTO") => 1e-15,
        Some("ATTO") => 1e-18,
        _ => 1.0,
    }
}

/// What a unit measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    /// A `LENGTH_UNIT`.
    Length,
    /// A `PLANE_ANGLE_UNIT`.
    Angle,
    /// Any other unit (solid angle, mass, …).
    Other,
}

/// A unit's kind and size: millimetres for lengths, radians for plane angles (1 for anything
/// else, or a unit that cannot be read). `None` when `#id` is not an entity, or is a
/// `CONVERSION_BASED_UNIT` whose measure cannot be read or is not finite and positive.
pub fn unit_factor(ex: &Exchange, id: u64) -> Option<(UnitKind, f64)> {
    factor(ex, id, 0)
}

fn factor(ex: &Exchange, id: u64, depth: usize) -> Option<(UnitKind, f64)> {
    if depth > MAX_CONVERSION_DEPTH {
        return None;
    }
    let e = ex.get(id)?;
    let kind = if e.has("LENGTH_UNIT") {
        UnitKind::Length
    } else if e.has("PLANE_ANGLE_UNIT") {
        UnitKind::Angle
    } else {
        UnitKind::Other
    };
    if let Some(si) = e.record("SI_UNIT") {
        let base = match si.get(1).and_then(Param::as_enum) {
            Some("METRE") => 1000.0,
            Some("RADIAN") => 1.0,
            _ => return Some((kind, 1.0)),
        };
        return Some((kind, si_prefix(si.first()) * base));
    }
    if let Some(cb) = e.record("CONVERSION_BASED_UNIT") {
        let m = ex.get(cb.get(1)?.as_ref_id()?)?;
        let mp = m.params();
        let v = mp.first()?.as_f64()?;
        let (_, f) = factor(ex, mp.get(1)?.as_ref_id()?, depth + 1)?;
        let x = v * f;
        return (x.is_finite() && x > 0.0).then_some((kind, x));
    }
    Some((kind, 1.0))
}

/// The units and uncertainty of the representation context `#id` (the defaults when `#id` is
/// not an entity or lists no readable unit).
///
/// Each unit `GLOBAL_UNIT_ASSIGNED_CONTEXT` lists that reads as a finite, positive length or
/// plane angle factor sets that unit; a later one wins. Each `UNCERTAINTY_MEASURE_WITH_UNIT` that
/// `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` lists sets the uncertainty to its value times the factor
/// of its unit (the context's length unit when that cannot be read), or clears it when the
/// product is not finite and positive; the last one wins.
pub fn context(ex: &Exchange, id: u64) -> Context {
    let mut c = Context::default();
    let Some(e) = ex.get(id) else { return c };
    if let Some(units) = e
        .record("GLOBAL_UNIT_ASSIGNED_CONTEXT")
        .and_then(<[Param]>::first)
        .and_then(Param::as_list)
    {
        for uid in units.iter().filter_map(Param::as_ref_id) {
            match unit_factor(ex, uid) {
                Some((UnitKind::Length, f)) if f.is_finite() && f > 0.0 => c.units.len = f,
                Some((UnitKind::Angle, f)) if f.is_finite() && f > 0.0 => c.units.ang = f,
                _ => {}
            }
        }
    }
    if let Some(unc) = e
        .record("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")
        .and_then(<[Param]>::first)
        .and_then(Param::as_list)
    {
        for uid in unc.iter().filter_map(Param::as_ref_id) {
            if let Some(m) = ex.get(uid)
                && let Some(v) = m.params().first().and_then(Param::as_f64)
            {
                let f = m
                    .params()
                    .get(1)
                    .and_then(Param::as_ref_id)
                    .and_then(|x| unit_factor(ex, x))
                    .map(|x| x.1);
                c.uncertainty =
                    Some(v * f.unwrap_or(c.units.len)).filter(|t| t.is_finite() && *t > 0.0);
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step::p21;
    use std::f64::consts::PI;
    use std::fmt::Write as _;

    /// Millimetres, radians and steradians as SolidCraft's writer and its topology tests
    /// (`crates/step/src/topology_tests.rs`, `file`) write them.
    const MM: &str = "#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\
         #2=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\
         #3=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());";

    fn parse(data: &str) -> Exchange {
        let t = format!(
            "ISO-10303-21;\nHEADER;FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
        );
        match p21::parse(&t) {
            Ok(ex) => ex,
            Err(e) => panic!("{e}\n{t}"),
        }
    }

    /// A context `#100` with the units `units` and the uncertainties `unc` (entity ids).
    fn ctx(data: &str, units: &str, unc: &str) -> Context {
        let c = format!(
            "#100=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({unc}))\
             GLOBAL_UNIT_ASSIGNED_CONTEXT(({units}))REPRESENTATION_CONTEXT('',''));"
        );
        context(&parse(&format!("{data}{c}")), 100)
    }

    #[test]
    fn solidcraft_file_context() {
        // SolidCraft's topology test file: mm, rad, sr and an uncertainty of 1e-7 mm.
        let c = ctx(
            &format!(
                "{MM}#4=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#1,'distance_accuracy_value','');"
            ),
            "#1,#2,#3",
            "#4",
        );
        assert_eq!(c.units, Units { len: 1.0, ang: 1.0 });
        // 1e-7 times MILLI (1e-3) times METRE (1000 mm).
        assert_eq!(c.uncertainty, Some(1e-7 * (1e-3 * 1000.0)));
    }

    #[test]
    fn si_prefixes() {
        for (prefix, mm) in [
            (".MILLI.", 1e-3 * 1000.0),
            ("$", 1000.0),
            (".MICRO.", 1e-6 * 1000.0),
            (".CENTI.", 1e-2 * 1000.0),
            (".KILO.", 1e3 * 1000.0),
        ] {
            let c = ctx(
                &format!("#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT({prefix},.METRE.));"),
                "#1",
                "",
            );
            assert_eq!(c.units.len, mm, "{prefix}");
            assert_eq!(c.units.ang, 1.0, "{prefix}");
            assert_eq!(c.uncertainty, None, "{prefix}");
        }
        // The products the table defines: a millimetre is exactly 1, a metre 1000.
        assert_eq!(1e-3 * 1000.0, 1.0);
        let ex = parse("#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MICRO.,.METRE.));");
        assert_eq!(unit_factor(&ex, 1), Some((UnitKind::Length, 1e-6 * 1000.0)));
        // A milliradian.
        let ex = parse("#1=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT(.MILLI.,.RADIAN.));");
        assert_eq!(unit_factor(&ex, 1), Some((UnitKind::Angle, 1e-3)));
    }

    #[test]
    fn inches_and_degrees() {
        // An inch as 25.4 mm, a degree as π/180 rad (written as the shortest decimal of
        // PI / 180, so it reads as exactly that double).
        let data = format!(
            "{MM}#10=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\
             #11=(CONVERSION_BASED_UNIT('INCH',#10)LENGTH_UNIT()NAMED_UNIT(#12));\
             #12=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\
             #13=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.017453292519943295),#2);\
             #14=(CONVERSION_BASED_UNIT('DEGREE',#13)NAMED_UNIT(#15)PLANE_ANGLE_UNIT());\
             #15=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);"
        );
        let c = ctx(&data, "#11,#14,#3", "");
        assert_eq!(
            c.units,
            Units {
                len: 25.4,
                ang: PI / 180.0
            }
        );
        // An inch defined from metres: 0.0254 times 1000 mm.
        let c = ctx(
            "#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\
             #10=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0254),#1);\
             #11=(CONVERSION_BASED_UNIT('INCH',#10)LENGTH_UNIT()NAMED_UNIT(*));",
            "#11",
            "",
        );
        assert_eq!(c.units.len, 0.0254 * 1000.0);
        // A foot defined from the inch: a chain of two conversions, 12 times 25.4 mm.
        let c = ctx(
            &format!(
                "{data}#20=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(12.),#11);\
                 #21=(CONVERSION_BASED_UNIT('FOOT',#20)LENGTH_UNIT()NAMED_UNIT(*));"
            ),
            "#21",
            "",
        );
        assert_eq!(c.units.len, 12.0 * 25.4);
    }

    #[test]
    fn missing_or_broken_units_read_as_mm_and_rad() {
        let mm_rad = Units { len: 1.0, ang: 1.0 };
        // No such context, or a context without units.
        assert_eq!(context(&parse(MM), 100), Context::default());
        assert_eq!(context(&parse(MM), 1), Context::default());
        assert_eq!(Context::default().units, mm_rad);
        for (data, units) in [
            // Dangling references.
            ("", "#7,#8"),
            // An SI unit with an unknown base: factor 1.
            (
                "#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.FURLONG.));",
                "#1",
            ),
            // A conversion whose measure is missing, unreadable, zero, negative or infinite.
            (
                "#1=(CONVERSION_BASED_UNIT('INCH',#9)LENGTH_UNIT()NAMED_UNIT(*));",
                "#1",
            ),
            (
                "#2=(NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.)LENGTH_UNIT());\
                 #3=LENGTH_MEASURE_WITH_UNIT($,#2);\
                 #1=(CONVERSION_BASED_UNIT('INCH',#3)LENGTH_UNIT()NAMED_UNIT(*));",
                "#1",
            ),
            (
                "#2=(NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.)LENGTH_UNIT());\
                 #3=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.),#2);\
                 #1=(CONVERSION_BASED_UNIT('INCH',#3)LENGTH_UNIT()NAMED_UNIT(*));",
                "#1",
            ),
            (
                "#2=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\
                 #3=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(-1.),#2);\
                 #1=(CONVERSION_BASED_UNIT('DEGREE',#3)PLANE_ANGLE_UNIT()NAMED_UNIT(*));",
                "#1",
            ),
            (
                "#2=(NAMED_UNIT(*)SI_UNIT(.EXA.,.METRE.)LENGTH_UNIT());\
                 #3=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E300),#2);\
                 #1=(CONVERSION_BASED_UNIT('BIG',#3)LENGTH_UNIT()NAMED_UNIT(*));",
                "#1",
            ),
            // A conversion that refers to itself: the depth bound ends it.
            (
                "#3=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(2.),#1);\
                 #1=(CONVERSION_BASED_UNIT('LOOP',#3)LENGTH_UNIT()NAMED_UNIT(*));",
                "#1",
            ),
            // Units that are neither lengths nor plane angles.
            (MM, "#3"),
            // An empty list.
            (MM, ""),
        ] {
            let c = ctx(data, units, "");
            assert_eq!(c.units, mm_rad, "{data} / {units}");
        }
        // A unit list that is not a list.
        let ex = parse(&format!(
            "{MM}#100=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT(#1)REPRESENTATION_CONTEXT('',''));"
        ));
        assert_eq!(context(&ex, 100), Context::default());
        // A broken length unit leaves the readable angle unit in place.
        let c = ctx(
            "#2=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT(.MILLI.,.RADIAN.));\
             #1=(CONVERSION_BASED_UNIT('INCH',#9)LENGTH_UNIT()NAMED_UNIT(*));",
            "#1,#2",
            "",
        );
        assert_eq!(
            c.units,
            Units {
                len: 1.0,
                ang: 1e-3
            }
        );
    }

    #[test]
    fn conversion_depth_is_bounded() {
        // #1 is millimetres; #2..=#7 each 2 of the one before, so #(1+k) is 2^k mm and
        // MAX_CONVERSION_DEPTH links are followed from the unit asked for.
        let mut data = String::from("#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));");
        for k in 2..=7 {
            write!(
                data,
                "#{m}=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(2.),#{p});\
                 #{k}=(CONVERSION_BASED_UNIT('U',#{m})LENGTH_UNIT()NAMED_UNIT(*));",
                m = 100 + k,
                p = k - 1
            )
            .unwrap();
        }
        let ex = parse(&data);
        assert_eq!(unit_factor(&ex, 5), Some((UnitKind::Length, 16.0)));
        assert_eq!(unit_factor(&ex, 6), None);
    }

    #[test]
    fn uncertainty_in_file_units() {
        let inch = format!(
            "{MM}#10=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\
             #11=(CONVERSION_BASED_UNIT('INCH',#10)LENGTH_UNIT()NAMED_UNIT(*));"
        );
        // 1e-4 inch is 1e-4 * 25.4 mm.
        let c = ctx(
            &format!("{inch}#20=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-04),#11,'d','');"),
            "#11,#2",
            "#20",
        );
        assert_eq!(c.units.len, 25.4);
        assert_eq!(c.uncertainty, Some(1e-4 * 25.4));
        // A measure in metres in an inch context: its own unit, 1e-6 * 1000 mm.
        let c = ctx(
            &format!(
                "{inch}#12=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\
                 #20=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#12,'d','');"
            ),
            "#11",
            "#20",
        );
        assert_eq!(c.uncertainty, Some(1e-6 * 1000.0));
        // A measure whose unit cannot be read is in the context's length unit.
        let c = ctx(
            &format!("{inch}#20=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-04),$,'d','');"),
            "#11",
            "#20",
        );
        assert_eq!(c.uncertainty, Some(1e-4 * 25.4));
        // The last one listed wins.
        let c = ctx(
            &format!(
                "{MM}#20=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-04),#1,'d','');\
                 #21=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(2.E-04),#1,'d','');"
            ),
            "#1",
            "#20,#21",
        );
        assert_eq!(c.uncertainty, Some(2e-4 * (1e-3 * 1000.0)));
    }

    #[test]
    fn uncertainty_zero_negative_or_infinite_is_none() {
        for v in ["0.", "-1.E-04", "1.E300"] {
            // In exametres (1e21 mm): 0, negative, and 1e300 * 1e21 mm, which is infinite.
            let c = ctx(
                &format!(
                    "{MM}#12=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.EXA.,.METRE.));\
                     #20=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({v}),#12,'d','');"
                ),
                "#1",
                "#20",
            );
            assert_eq!(c.uncertainty, None, "{v}");
        }
        // A bad one after a good one clears it (the last one wins).
        let c = ctx(
            &format!(
                "{MM}#20=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-04),#1,'d','');\
                 #21=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.),#1,'d','');"
            ),
            "#1",
            "#20,#21",
        );
        assert_eq!(c.uncertainty, None);
        // One without a readable value, or a dangling reference, is skipped.
        let c = ctx(
            &format!(
                "{MM}#20=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-04),#1,'d','');\
                 #21=UNCERTAINTY_MEASURE_WITH_UNIT($,#1,'d','');"
            ),
            "#1",
            "#20,#21,#99",
        );
        assert_eq!(c.uncertainty, Some(1e-4 * (1e-3 * 1000.0)));
    }
}
