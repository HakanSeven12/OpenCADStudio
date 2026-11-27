//! Calculator engine tests.

use super::*;
use glam::DVec3;
use std::f64::consts::{E, FRAC_PI_2, FRAC_PI_4, PI};

fn ctx() -> CalcContext {
    CalcContext::default()
}

fn radians() -> CalcContext {
    CalcContext::new(AngleMode::Radians, 1.0)
}

/// Each input evaluates to its number in `ctx`.
fn numbers(ctx: &mut CalcContext, cases: &[(&str, f64)]) {
    for (input, expected) in cases {
        match evaluate(input, ctx) {
            Ok(Value::Number(n)) => assert!(
                (n - expected).abs() < 1e-9 * expected.abs().max(1.0),
                "{input:?} → {n}, expected {expected}"
            ),
            other => panic!("{input:?} → {other:?}"),
        }
    }
}

/// Each input fails, underlining the given text.
fn errors(ctx: &mut CalcContext, cases: &[(&str, &str)]) {
    for (input, bad) in cases {
        match evaluate(input, ctx) {
            Err(e) => assert_eq!(&&input[e.span.clone()], bad, "{input:?}: {}", e.message),
            ok => panic!("{input:?} should fail, got {ok:?}"),
        }
    }
}

#[test]
fn arithmetic() {
    numbers(
        &mut ctx(),
        &[
            ("2+3*4", 14.0),
            ("(2+3)*4", 20.0),
            ("10-4-3", 3.0),
            ("100/10/5", 2.0),
            ("2^3^2", 512.0),
            ("-2^2", -4.0),
            ("2^-1", 0.5),
            ("3*-2", -6.0),
            ("+-+5", -5.0),
            (".5", 0.5),
            ("2.5E-1", 0.25),
            ("3,5", 3.5),
            (" -3,25 ", -3.25),
            ("max(3,5)", 5.0),
            ("7 mod 3", 1.0),
            ("-7 mod 3", 2.0),
            ("7 mod -3", -2.0),
            ("1 + 7 mod 4", 4.0),
            ("10mod 4", 2.0),
        ],
    );
    let show = |s: &str| evaluate(s, &mut ctx()).unwrap().display();
    assert_eq!(show("0.1+0.2"), "0.3");
    assert_eq!(show("1/3"), "0.3333333333");
    assert_eq!(show("-0"), "0");
    assert_eq!(show("1e-9"), "1e-9");
    assert_eq!(show("[1,2,3.5]"), "[1,2,3.5]");
}

#[test]
fn points() {
    let point = |s: &str| match evaluate(s, &mut ctx()) {
        Ok(Value::Point(p)) => p,
        other => panic!("{s:?} → {other:?}"),
    };
    assert_eq!(point("[1,2]+[3,4,5]"), DVec3::new(4.0, 6.0, 5.0));
    assert_eq!(point("[5,5]-[1,2]"), DVec3::new(4.0, 3.0, 0.0));
    assert_eq!(point("2*[1,2]/4"), DVec3::new(0.5, 1.0, 0.0));
    assert_eq!(point("-[1,2]"), DVec3::new(-1.0, -2.0, 0.0));
    assert_eq!(point("mid([0,0],[4,2,6])"), DVec3::new(2.0, 1.0, 3.0));
    numbers(
        &mut ctx(),
        &[
            ("dist([0,0],[3,4])", 5.0),
            ("ang([1,1])", 45.0),
            ("ang([0,0],[0,-1])", 270.0),
            ("zof([1,2,3])", 3.0),
            ("zof([1,2])", 0.0),
        ],
    );
    errors(
        &mut ctx(),
        &[
            ("1 + [1,2]", "1 + [1,2]"),
            ("[1,2]*[3,4]", "[1,2]*[3,4]"),
            ("2/[1,2]", "[1,2]"),
            ("[[1,2],3]", "[1,2]"),
            ("[1]", "[1]"),
            ("xof(5)", "5"),
            ("[1,2] mod 2", "[1,2]"),
        ],
    );
}

#[test]
fn variables_and_ans() {
    let mut c = ctx();
    errors(&mut c, &[("ans", "ans"), ("*2", "*")]);
    numbers(
        &mut c,
        &[
            ("$wall = 3600", 3600.0),
            ("$Wall + 90", 3690.0),
            ("*2", 7380.0),
            ("ANS / 2", 3690.0),
            ("^0", 1.0),
            // The `$` namespace keeps variables apart from built-ins and units.
            ("$e = 5", 5.0),
            ("$e + e", 5.0 + E),
            ("$log = 100", 100.0),
            ("log($log)", 2.0),
            ("$m = 2", 2.0),
            ("3m * $m", 6000.0),
        ],
    );
    evaluate("$p = [1,2]", &mut c).unwrap();
    assert_eq!(c.vars["wall"], Value::Number(3600.0));
    assert_eq!(c.vars["p"], Value::Point(DVec3::new(1.0, 2.0, 0.0)));
    // A failed evaluation changes nothing.
    errors(&mut c, &[("$wall = 1/0", "0")]);
    assert_eq!(c.vars["wall"], Value::Number(3600.0));
    errors(
        &mut c,
        &[
            ("$nope + 1", "$nope"),
            ("wall = 1", "wall"),
            ("wall + 1", "wall"),
            ("$ + 1", "$"),
            ("$1 + 1", "$"),
            ("x + 1", "x"),
        ],
    );
    let message = |input: &str| evaluate(input, &mut c.clone()).unwrap_err().message;
    assert_eq!(
        message("wall = 1"),
        "Variable names start with $: $wall = …"
    );
    assert_eq!(message("WALL + 1"), "Variables start with $: use $wall");
    assert_eq!(message("$nope"), "Unknown variable $nope");
}

#[test]
fn variable_definitions() {
    let mut c = ctx();
    let errors = define_variables(
        &mut c,
        [
            ("module", Definition::Function("$brick + $joint")),
            ("course", Definition::Function("$module * 2")),
            ("broken", Definition::Function("$nope")),
            ("brick", Definition::Constant(Value::Number(210.0))),
            ("Joint", Definition::Constant(Value::Number(10.0))),
        ],
    );
    // Constants first, then functions in order, each seeing the earlier ones.
    assert_eq!(c.vars["module"], Value::Number(220.0));
    assert_eq!(c.vars["course"], Value::Number(440.0));
    assert!(!c.vars.contains_key("broken"));
    assert_eq!(errors["broken"].message, "Unknown variable $nope");
    // A function follows its inputs on the next definition.
    let mut c = ctx();
    define_variables(
        &mut c,
        [
            ("brick", Definition::Constant(Value::Number(300.0))),
            ("module", Definition::Function("$brick + 10")),
        ],
    );
    assert_eq!(c.vars["module"], Value::Number(310.0));
    // Helpers for the variable form.
    assert_eq!(
        assigned_variable("$Wall = 3"),
        Some(("wall".to_string(), 0..5))
    );
    assert_eq!(assigned_variable("$wall + 3"), None);
    assert!(uses_variable("2 * $MODULE", "module"));
    assert!(!uses_variable("2 * module", "module"));
    assert!(is_valid_name("_a1") && !is_valid_name("1a") && !is_valid_name("a-b"));
    assert_eq!(
        value_of("$x = 3", &ctx()).unwrap_err().message,
        "Leave out the $name = part"
    );
}

#[test]
fn getvar_reads_system_variables() {
    let mut c = ctx();
    c.sysvars = Some(SysVars(std::sync::Arc::new(|name: &str| match name {
        "luprec" => Some(SysVar::Number(4.0)),
        "extmax" => Some(SysVar::Point(DVec3::new(10.0, 20.0, 0.0))),
        "clayer" => Some(SysVar::Text),
        _ => None,
    })));
    numbers(
        &mut c,
        &[("getvar(LUPREC) + 1", 5.0), ("xof(getvar(extmax))", 10.0)],
    );
    errors(
        &mut c,
        &[
            ("getvar(clayer)", "getvar(clayer)"),
            ("getvar(nope)", "nope"),
            ("getvar(1)", "getvar(1)"),
            ("getvar($x)", "getvar($x)"),
        ],
    );
}

#[test]
fn units() {
    numbers(
        &mut ctx(),
        &[
            ("3.6m", 3600.0),
            ("3.6 m", 3600.0),
            ("1km + 10MM", 1_000_010.0),
            ("1in", 25.4),
            ("1ft", 304.8),
            ("90deg + 100grad", 180.0),
            ("1rad", 1f64.to_degrees()),
        ],
    );
    numbers(
        &mut CalcContext::new(AngleMode::Degrees, 1000.0),
        &[("1500mm", 1.5)],
    );
    numbers(
        &mut CalcContext::new(AngleMode::Degrees, 25.4),
        &[("2'9\"", 33.0), ("2'-9\"", 33.0), ("2'9\" + 3\"", 36.0)],
    );
    numbers(&mut radians(), &[("180deg", PI), ("sin(30deg)", 0.5)]);
    errors(&mut ctx(), &[("2x", "x"), ("2 x", "x")]);
}

#[test]
fn functions() {
    numbers(
        &mut ctx(),
        &[
            ("sin(30)", 0.5),
            ("tan(45)", 1.0),
            ("acos(0.5)", 60.0),
            ("atan2(1,-1)", 135.0),
            ("sec(60)", 2.0),
            ("csc(30)", 2.0),
            ("cot(45)", 1.0),
            ("asec(2)", 60.0),
            ("acsc(2)", 30.0),
            ("acot(0)", 90.0),
            ("sinh(1)", 1f64.sinh()),
            ("acosh(cosh(2))", 2.0),
            ("sqrt(16)", 4.0),
            ("cbrt(-8)", -2.0),
            ("root(-32, 5)", -2.0),
            ("fact(5)", 120.0),
            ("round(2.5)", 3.0),
            ("round(2.71828, 2)", 2.72),
            ("floor(-1.5)", -2.0),
            ("ceil(1.2)", 2.0),
            ("abs(-3)", 3.0),
            ("ln(e)", 1.0),
            ("log(1000)", 3.0),
            ("logb(8, 2)", 3.0),
            ("exp(0)", 1.0),
            ("pow(2,10)", 1024.0),
            ("min(4,2,8)", 2.0),
            ("d2r(180)", PI),
            ("r2d(pi)", 180.0),
            ("SQRT(9)", 3.0),
        ],
    );
    // Trig follows the angle mode; hyperbolic functions don't.
    numbers(
        &mut radians(),
        &[
            ("sin(pi/2)", 1.0),
            ("asin(1)", FRAC_PI_2),
            ("acot(1)", FRAC_PI_4),
            ("ang([0,0],[0,1])", FRAC_PI_2),
            ("sinh(1)", 1f64.sinh()),
        ],
    );
    errors(
        &mut ctx(),
        &[
            ("foo(1)", "foo(1)"),
            ("1 + sqrt(1, 2)", "sqrt(1, 2)"),
            ("1 + sqrt(-1)", "sqrt(-1)"),
            ("asin(2)", "asin(2)"),
            ("ln(0)", "ln(0)"),
            ("fact(171)", "fact(171)"),
            ("fact(2.5)", "fact(2.5)"),
            ("root(-16, 4)", "root(-16, 4)"),
            ("acosh(0)", "acosh(0)"),
        ],
    );
    // A wrong argument count is an error at the call, never a panic.
    for name in functions::FUNCTIONS {
        let input = format!("{name}()");
        errors(&mut ctx(), &[(&input, &input)]);
    }
}

#[test]
fn error_spans() {
    errors(
        &mut ctx(),
        &[
            ("2 + x", "x"),
            ("10 / (5-5)", "(5-5)"),
            ("5 mod 0", "0"),
            ("2 $ 3", "$"),
            ("2 + * 3", "*"),
            ("(2 + 3", "(2 + 3"),
            ("[1, 2", "[1, 2"),
            ("2 3", "3"),
            ("1.2.3", "1.2.3"),
            ("1e308 * 10", "1e308 * 10"),
            ("é + 1", "é"),
            ("1 + √4", "√"),
            ("2 +", ""),
            ("   ", "   "),
        ],
    );
}

#[test]
fn long_and_deep_input_is_bounded() {
    let parens = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
    numbers(&mut ctx(), &[(&parens(40), 1.0)]);
    let deep = evaluate(&parens(300), &mut ctx()).unwrap_err();
    assert_eq!(deep.message, "expression is too deeply nested");
    assert!(evaluate(&format!("{}1", "-".repeat(600)), &mut ctx()).is_err());
    numbers(&mut ctx(), &[(&vec!["1"; 500].join("+"), 500.0)]);
    let huge = vec!["1"; 1000].join("+");
    assert_eq!(evaluate(&huge, &mut ctx()).unwrap_err().span, 0..huge.len());
}

#[test]
fn random_input_never_panics() {
    const ALPHABET: &[&str] = &[
        "1", "2.5", "0", "e", "1e", "+", "-", "*", "/", "^", "(", ")", "[", "]", ",", "=", " ",
        "'", "\"", "pi", "ans", "x", "$x", "$", "sin", "dist", "mid", "m", "deg", "mod", "√", ".",
        "é", "getvar(", "luprec",
    ];
    let mut seed: u64 = 0x5eed_cafe;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    for _ in 0..20_000 {
        let len = next() % 24;
        let input: String = (0..len)
            .map(|_| ALPHABET[next() % ALPHABET.len()])
            .collect();
        let mut c = ctx();
        c.vars.insert("x".into(), Value::Number(2.0));
        if let Err(e) = evaluate(&input, &mut c) {
            assert!(e.span.end <= input.len(), "{input:?}");
            assert!(input.is_char_boundary(e.span.start), "{input:?}");
            assert!(input.is_char_boundary(e.span.end), "{input:?}");
            assert!(!e.message.is_empty());
        }
    }
}
