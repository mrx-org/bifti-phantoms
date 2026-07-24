use std::str::FromStr;

use crate::nifti_phantom::PhantomError;
use toolapi::{
    MessageFn,
    value::{structured::Volume, typed::TypedList},
};

/// Parses an expression `func` and applies it to all elements in `data`.
/// - Avaliable operators: + - * / ( )
/// - Available variables: x, x_min, x_max, x_mean, x_std
///
/// All variables are scalars, `x` is the current element of the data array that
/// is mapped while the other constants are pre-computed from the `data` array.
pub fn eval_mapping_func(
    mut volume: Volume,
    func: &str,
    send_msg: &mut MessageFn,
) -> Result<Volume, PhantomError> {
    if volume.data.is_empty() {
        send_msg("🧮 Data is empty, skipping mapping".to_string())?;
        return Ok(volume);
    }

    let data: Vec<f64> = match volume.data {
        TypedList::Float(floats) => floats,
        _ => {
            send_msg("🧮 Can currently only map `Float` NIfTIs".to_string())?;
            return Err(PhantomError::MappingFunction {
                func: func.to_string(),
                err: "Tried to map non-`Float` array".to_string(),
            });
        }
    };

    send_msg(format!("🧮 Parsing mapping function `{func}`"))?;
    let ast: Expr = func.parse()?;
    send_msg("🧮 Computing min, max, mean, std".to_string())?;
    let input = Input::new(&data);
    send_msg(format!("🧮 Applying mapping {ast:?}"))?;
    let output = ast.eval(&input);

    let Array::Vector(output) = output else {
        return Err(PhantomError::MappingFunction {
            func: func.to_string(),
            err: "Mapping function produced an scalar (didn't contain the input `x`)".to_string(),
        });
    };

    send_msg("🧮 Finished mapping".to_string())?;
    volume.data = TypedList::Float(output);
    Ok(volume)
}

#[derive(Debug, Clone)]
enum Expr {
    Input(InputName),
    Value(f64),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Paren(Box<Expr>),
}

impl FromStr for Expr {
    type Err = PhantomError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        expr.parse(s).map_err(|e| PhantomError::MappingFunction {
            func: s.to_string(),
            err: e.to_string(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum InputName {
    X,
    XMin,
    XMax,
    XMean,
    XStd,
}

impl Expr {
    /// TODO: This is not super performant - each collect as well as Array::get produces a clone at full map resolution!
    fn eval(&self, input: &Input) -> Array {
        match self {
            Expr::Input(name) => input.get(*name),
            Expr::Value(value) => Array::Scalar(*value),
            Expr::Add(lhs, rhs) => match (lhs.eval(input), rhs.eval(input)) {
                (Array::Scalar(lhs), Array::Scalar(rhs)) => Array::Scalar(lhs + rhs),
                (Array::Scalar(lhs), Array::Vector(rhs)) => {
                    Array::Vector(rhs.iter().map(|rhs| lhs + rhs).collect())
                }
                (Array::Vector(lhs), Array::Scalar(rhs)) => {
                    Array::Vector(lhs.iter().map(|lhs| lhs + rhs).collect())
                }
                (Array::Vector(lhs), Array::Vector(rhs)) => {
                    Array::Vector(lhs.iter().zip(rhs).map(|(lhs, rhs)| lhs + rhs).collect())
                }
            },
            Expr::Sub(lhs, rhs) => match (lhs.eval(input), rhs.eval(input)) {
                (Array::Scalar(lhs), Array::Scalar(rhs)) => Array::Scalar(lhs - rhs),
                (Array::Scalar(lhs), Array::Vector(rhs)) => {
                    Array::Vector(rhs.iter().map(|rhs| lhs - rhs).collect())
                }
                (Array::Vector(lhs), Array::Scalar(rhs)) => {
                    Array::Vector(lhs.iter().map(|lhs| lhs - rhs).collect())
                }
                (Array::Vector(lhs), Array::Vector(rhs)) => {
                    Array::Vector(lhs.iter().zip(rhs).map(|(lhs, rhs)| lhs - rhs).collect())
                }
            },
            Expr::Mul(lhs, rhs) => match (lhs.eval(input), rhs.eval(input)) {
                (Array::Scalar(lhs), Array::Scalar(rhs)) => Array::Scalar(lhs * rhs),
                (Array::Scalar(lhs), Array::Vector(rhs)) => {
                    Array::Vector(rhs.iter().map(|rhs| lhs * rhs).collect())
                }
                (Array::Vector(lhs), Array::Scalar(rhs)) => {
                    Array::Vector(lhs.iter().map(|lhs| lhs * rhs).collect())
                }
                (Array::Vector(lhs), Array::Vector(rhs)) => {
                    Array::Vector(lhs.iter().zip(rhs).map(|(lhs, rhs)| lhs * rhs).collect())
                }
            },
            Expr::Div(lhs, rhs) => match (lhs.eval(input), rhs.eval(input)) {
                (Array::Scalar(lhs), Array::Scalar(rhs)) => Array::Scalar(lhs / rhs),
                (Array::Scalar(lhs), Array::Vector(rhs)) => {
                    Array::Vector(rhs.iter().map(|rhs| lhs / rhs).collect())
                }
                (Array::Vector(lhs), Array::Scalar(rhs)) => {
                    Array::Vector(lhs.iter().map(|lhs| lhs / rhs).collect())
                }
                (Array::Vector(lhs), Array::Vector(rhs)) => {
                    Array::Vector(lhs.iter().zip(rhs).map(|(lhs, rhs)| lhs / rhs).collect())
                }
            },
            Expr::Paren(expr) => expr.eval(input),
        }
    }
}

enum Array {
    Scalar(f64),
    Vector(Vec<f64>),
}

impl FromIterator<f64> for Array {
    fn from_iter<T: IntoIterator<Item = f64>>(iter: T) -> Self {
        Self::Vector(iter.into_iter().collect())
    }
}

#[derive(Debug)]
struct Input<'a> {
    x: &'a [f64],
    x_min: f64,
    x_max: f64,
    x_mean: f64,
    x_std: f64,
}

impl<'a> Input<'a> {
    fn new(x: &'a [f64]) -> Self {
        let x_min = *x.iter().min_by(|a, b| a.total_cmp(b)).unwrap_or(&0.0);
        let x_max = *x.iter().max_by(|a, b| a.total_cmp(b)).unwrap_or(&0.0);
        let n = x.len() as f64;
        let x_mean = x.iter().sum::<f64>() / n;
        let x_std = (x.iter().map(|xi| (xi - x_mean).powi(2)).sum::<f64>() / n).sqrt();

        Self {
            x,
            x_min,
            x_max,
            x_mean,
            x_std,
        }
    }

    fn get(&self, name: InputName) -> Array {
        match name {
            InputName::X => Array::Vector(self.x.to_vec()),
            InputName::XMin => Array::Scalar(self.x_min),
            InputName::XMax => Array::Scalar(self.x_max),
            InputName::XMean => Array::Scalar(self.x_mean),
            InputName::XStd => Array::Scalar(self.x_std),
        }
    }
}

// =====================================
// Parse func string to AST using winnow
// =====================================

use winnow::{
    ascii::{digit1, multispace0},
    combinator::{alt, delimited, repeat},
    prelude::*,
    token::{literal, one_of},
};

fn parens(i: &mut &str) -> winnow::Result<Expr> {
    delimited("(", expr, ")")
        .map(|e| Expr::Paren(Box::new(e)))
        .parse_next(i)
}

fn value(i: &mut &str) -> winnow::Result<Expr> {
    digit1
        .try_map(FromStr::from_str)
        .map(Expr::Value)
        .parse_next(i)
}

fn input(i: &mut &str) -> winnow::Result<Expr> {
    alt((
        literal("x_min").value(Expr::Input(InputName::XMin)),
        literal("x_max").value(Expr::Input(InputName::XMax)),
        literal("x_mean").value(Expr::Input(InputName::XMean)),
        literal("x_std").value(Expr::Input(InputName::XStd)),
        literal("x").value(Expr::Input(InputName::X)),
    ))
    .parse_next(i)
}

fn factor(i: &mut &str) -> winnow::Result<Expr> {
    delimited(multispace0, alt((input, value, parens)), multispace0).parse_next(i)
}

fn term(i: &mut &str) -> winnow::Result<Expr> {
    let init = factor.parse_next(i)?;

    repeat(0.., (one_of(['*', '/']), factor))
        .fold(
            move || init.clone(),
            |acc, (op, val): (char, Expr)| {
                if op == '*' {
                    Expr::Mul(Box::new(acc), Box::new(val))
                } else {
                    Expr::Div(Box::new(acc), Box::new(val))
                }
            },
        )
        .parse_next(i)
}

fn expr(i: &mut &str) -> winnow::Result<Expr> {
    let init = term.parse_next(i)?;

    repeat(0.., (one_of(['+', '-']), term))
        .fold(
            move || init.clone(),
            |acc, (op, val): (char, Expr)| {
                if op == '+' {
                    Expr::Add(Box::new(acc), Box::new(val))
                } else {
                    Expr::Sub(Box::new(acc), Box::new(val))
                }
            },
        )
        .parse_next(i)
}
