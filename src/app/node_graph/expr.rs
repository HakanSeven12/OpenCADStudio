//! A small expression language for the Expression and Filter nodes:
//! arithmetic (`+ - * / % ^`), comparisons (`< <= > >= == !=`), logic
//! (`&& || !`, also `and or not`), the conditional `c ? a : b`, named
//! variables and a few functions. Angles are degrees, as everywhere in the
//! library. True is 1 and false is 0, so conditions can be added up.

/// A value: a number, or the truth a comparison gives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Bool(bool),
}

impl Val {
    pub fn num(self) -> f64 {
        match self {
            Val::Num(x) => x,
            Val::Bool(b) => f64::from(u8::from(b)),
        }
    }

    pub fn truth(self) -> bool {
        match self {
            Val::Num(x) => x != 0.0 && !x.is_nan(),
            Val::Bool(b) => b,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f64),
    Name(String),
    Op(&'static str),
}

const OPS: &[&str] = &[
    "<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "^", "(", ")", ",", "<", ">", "=",
    "!", "?", ":",
];

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut rest = source;
    while let Some(ch) = rest.chars().next() {
        if ch.is_whitespace() {
            rest = &rest[ch.len_utf8()..];
        } else if ch.is_ascii_digit() || (ch == '.' && rest[1..].starts_with(|c: char| c.is_ascii_digit())) {
            let mut end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(rest.len());
            // An exponent: 1e3, 2.5e-4.
            if rest[end..].starts_with(['e', 'E']) {
                let after = &rest[end + 1..];
                let sign = usize::from(after.starts_with(['+', '-']));
                let digits = after[sign..].find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len() - sign);
                if digits > 0 {
                    end += 1 + sign + digits;
                }
            }
            let number = rest[..end].parse().map_err(|_| format!("bad number `{}`", &rest[..end]))?;
            tokens.push(Token::Num(number));
            rest = &rest[end..];
        } else if ch.is_alphabetic() || ch == '_' {
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            tokens.push(Token::Name(rest[..end].to_lowercase()));
            rest = &rest[end..];
        } else if let Some(op) = OPS.iter().find(|op| rest.starts_with(**op)) {
            tokens.push(Token::Op(op));
            rest = &rest[op.len()..];
        } else {
            return Err(format!("unexpected `{ch}`"));
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    var: &'a dyn Fn(&str) -> Option<f64>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, op: &str) -> bool {
        let found = match self.peek() {
            Some(Token::Op(o)) => *o == op,
            Some(Token::Name(name)) => matches!((op, name.as_str()), ("&&", "and") | ("||", "or") | ("!", "not")),
            _ => false,
        };
        if found {
            self.at += 1;
        }
        found
    }

    fn expect(&mut self, op: &str) -> Result<(), String> {
        if self.eat(op) { Ok(()) } else { Err(format!("expected `{op}`")) }
    }

    fn conditional(&mut self) -> Result<Val, String> {
        let test = self.or()?;
        if !self.eat("?") {
            return Ok(test);
        }
        let then = self.conditional()?;
        self.expect(":")?;
        let otherwise = self.conditional()?;
        Ok(if test.truth() { then } else { otherwise })
    }

    fn or(&mut self) -> Result<Val, String> {
        let mut left = self.and()?;
        while self.eat("||") {
            let right = self.and()?;
            left = Val::Bool(left.truth() || right.truth());
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Val, String> {
        let mut left = self.compare()?;
        while self.eat("&&") {
            let right = self.compare()?;
            left = Val::Bool(left.truth() && right.truth());
        }
        Ok(left)
    }

    fn compare(&mut self) -> Result<Val, String> {
        let left = self.sum()?;
        let op = match self.peek() {
            Some(Token::Op(op)) if matches!(*op, "<" | "<=" | ">" | ">=" | "==" | "=" | "!=") => *op,
            _ => return Ok(left),
        };
        self.at += 1;
        let (a, b) = (left.num(), self.sum()?.num());
        let close = (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
        Ok(Val::Bool(match op {
            "<" => a < b && !close,
            "<=" => a < b || close,
            ">" => a > b && !close,
            ">=" => a > b || close,
            "!=" => !close,
            _ => close,
        }))
    }

    fn sum(&mut self) -> Result<Val, String> {
        let mut left = self.product()?.num();
        loop {
            if self.eat("+") {
                left += self.product()?.num();
            } else if self.eat("-") {
                left -= self.product()?.num();
            } else {
                return Ok(Val::Num(left));
            }
        }
    }

    fn product(&mut self) -> Result<Val, String> {
        let first = self.unary()?;
        let mut left = first.num();
        let mut any = false;
        loop {
            if self.eat("*") {
                left *= self.unary()?.num();
            } else if self.eat("/") {
                left /= self.unary()?.num();
            } else if self.eat("%") {
                left = left.rem_euclid(self.unary()?.num());
            } else {
                return Ok(if any { Val::Num(left) } else { first });
            }
            any = true;
        }
    }

    fn unary(&mut self) -> Result<Val, String> {
        if self.eat("-") {
            return Ok(Val::Num(-self.unary()?.num()));
        }
        if self.eat("+") {
            return Ok(Val::Num(self.unary()?.num()));
        }
        if self.eat("!") {
            return Ok(Val::Bool(!self.unary()?.truth()));
        }
        self.power()
    }

    fn power(&mut self) -> Result<Val, String> {
        let base = self.atom()?;
        if self.eat("^") {
            // Right-associative, and binds tighter than a leading minus on
            // its right: 2^-1 = 0.5.
            return Ok(Val::Num(base.num().powf(self.unary()?.num())));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Val, String> {
        match self.tokens.get(self.at).cloned() {
            Some(Token::Num(x)) => {
                self.at += 1;
                Ok(Val::Num(x))
            }
            Some(Token::Op("(")) => {
                self.at += 1;
                let value = self.conditional()?;
                self.expect(")")?;
                Ok(value)
            }
            Some(Token::Name(name)) => {
                self.at += 1;
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.conditional()?);
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    return call(&name, &args);
                }
                if let Some(x) = (self.var)(&name) {
                    return Ok(Val::Num(x));
                }
                match name.as_str() {
                    "pi" => Ok(Val::Num(std::f64::consts::PI)),
                    "e" => Ok(Val::Num(std::f64::consts::E)),
                    "true" => Ok(Val::Bool(true)),
                    "false" => Ok(Val::Bool(false)),
                    _ => Err(format!("unknown name `{name}`")),
                }
            }
            Some(Token::Op(op)) => Err(format!("unexpected `{op}`")),
            None => Err("the expression ends too soon".to_owned()),
        }
    }
}

fn call(name: &str, args: &[Val]) -> Result<Val, String> {
    let x: Vec<f64> = args.iter().map(|arg| arg.num()).collect();
    let want = |count: usize| {
        if x.len() == count {
            Ok(())
        } else {
            Err(format!("`{name}` takes {count} values"))
        }
    };
    let value = match name {
        "if" => {
            want(3)?;
            return Ok(if args[0].truth() { args[1] } else { args[2] });
        }
        "min" | "max" if !x.is_empty() => {
            let pick: fn(f64, f64) -> f64 = if name == "min" { f64::min } else { f64::max };
            x.iter().copied().reduce(pick).unwrap_or(f64::NAN)
        }
        "round" if x.len() == 2 => {
            let scale = 10f64.powi(x[1] as i32);
            (x[0] * scale).round() / scale
        }
        "atan2" | "pow" | "hypot" => {
            want(2)?;
            match name {
                "atan2" => x[0].atan2(x[1]).to_degrees(),
                "pow" => x[0].powf(x[1]),
                _ => x[0].hypot(x[1]),
            }
        }
        "clamp" => {
            want(3)?;
            x[0].clamp(x[1].min(x[2]), x[2].max(x[1]))
        }
        _ => {
            want(1)?;
            let a = x[0];
            match name {
                "sin" => a.to_radians().sin(),
                "cos" => a.to_radians().cos(),
                "tan" => a.to_radians().tan(),
                "asin" => a.asin().to_degrees(),
                "acos" => a.acos().to_degrees(),
                "atan" => a.atan().to_degrees(),
                "sqrt" => a.sqrt(),
                "abs" => a.abs(),
                "floor" => a.floor(),
                "ceil" => a.ceil(),
                "round" => a.round(),
                "ln" => a.ln(),
                "log" => a.log10(),
                "exp" => a.exp(),
                "sign" => a.signum(),
                _ => return Err(format!("unknown function `{name}`")),
            }
        }
    };
    Ok(Val::Num(value))
}

/// Evaluates `source`, reading names through `var`.
pub fn eval(source: &str, var: &dyn Fn(&str) -> Option<f64>) -> Result<Val, String> {
    let mut parser = Parser { tokens: tokenize(source)?, at: 0, var };
    let value = parser.conditional()?;
    match parser.peek() {
        None => Ok(value),
        Some(Token::Op(op)) => Err(format!("unexpected `{op}`")),
        Some(Token::Name(name)) => Err(format!("unexpected `{name}`")),
        Some(Token::Num(x)) => Err(format!("unexpected `{x}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Val {
        eval(source, &|name| match name {
            "a" => Some(2.0),
            "b" => Some(3.0),
            _ => None,
        })
        .unwrap()
    }

    #[test]
    fn arithmetic_comparisons_and_conditions() {
        assert_eq!(run("a * 2 + b"), Val::Num(7.0));
        assert_eq!(run("-a ^ 2"), Val::Num(-4.0));
        assert_eq!(run("2 ^ -1"), Val::Num(0.5));
        assert_eq!(run("a < b && !(b == 4)"), Val::Bool(true));
        assert_eq!(run("a > b ? 10 : 20"), Val::Num(20.0));
        assert_eq!(run("if(a = 2, max(a, b, 9), 0)"), Val::Num(9.0));
        assert_eq!(run("round(10.256, 1)"), Val::Num(10.3));
        assert_eq!(run("(a > 1) + (b > 1)"), Val::Num(2.0));
        assert_eq!(run("1.5e2"), Val::Num(150.0));
        assert!((run("sin(30)").num() - 0.5).abs() < 1e-12);
        assert_eq!(run("0.1 + 0.2 == 0.3"), Val::Bool(true));
        assert!(eval("a +", &|_| None).is_err());
        assert!(eval("q", &|_| None).is_err());
    }
}
