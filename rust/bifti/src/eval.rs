use std::{
    ops::{Add, Div, Mul, Sub},
    str::FromStr,
};

use crate::{Volume, VolumeData, VolumeDataElement};

/// Dispatches `$data` to `eval_typed` once, based on its variant, so the arithmetic below runs
/// natively in that variant's element type rather than going through a common type per element.
macro_rules! eval_variant {
    ($data:expr, $ast:expr, $($variant:ident),* $(,)?) => {
        match $data {
            $(VolumeData::$variant(items) => VolumeData::$variant(eval_typed(items, $ast)),)*
        }
    };
}

/// Parses an expression `func` and applies it to all elements in `volume`, evaluating natively
/// in the volume's element type (a `Uint8` volume stays `Uint8`, a `Complex64` volume stays
/// `Complex64`, etc).
/// - Avaliable operators: + - * / ( )
/// - Available variables: x, x_min, x_max, x_mean, x_std
///
/// All variables are scalars, `x` is the current element of the data array that
/// is mapped while the other constants are pre-computed from the `data` array.
#[cfg_attr(feature = "tracing", tracing::instrument(skip_all, fields(func)))]
pub fn eval_mapping_func(mut volume: Volume, func: &str) -> Result<Volume, crate::Error> {
    let ast: Expr = func.parse()?;

    volume.data = eval_variant!(
        volume.data,
        &ast,
        Uint8,
        Uint16,
        Uint32,
        Uint64,
        Int8,
        Int16,
        Int32,
        Int64,
        Float32,
        Float64,
        Complex64,
        Complex128,
    );

    Ok(volume)
}

/// Evaluates `ast` over `items` using `T`'s own arithmetic. Only the aggregate stats
/// (`x_min`/`x_max`/`x_mean`/`x_std`) and literal constants round-trip through `f64` (via
/// [`VolumeDataElement::to_f64`]/[`VolumeDataElement::from_f64`]) - summing a `Vec<u8>`
/// natively, for example, would overflow long before reaching a meaningful mean.
fn eval_typed<T>(items: Vec<T>, ast: &Expr) -> Vec<T>
where
    T: VolumeDataElement + Add<Output = T> + Sub<Output = T> + Mul<Output = T> + Div<Output = T>,
{
    let input = Input::new(&items);
    match ast.eval(&input) {
        Array::Scalar(value) => vec![value],
        Array::Vector(items) => items,
    }
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
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        expr.parse(s).map_err(|e| crate::Error::EvalError {
            func: s.to_string(),
            error: e.to_string(),
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

/// Combines two already-evaluated `Array<T>`s element-wise with `$op`, broadcasting a `Scalar`
/// against a `Vector` where needed.
macro_rules! binary_op {
    ($lhs:expr, $rhs:expr, $op:tt) => {
        match ($lhs, $rhs) {
            (Array::Scalar(l), Array::Scalar(r)) => Array::Scalar(l $op r),
            (Array::Scalar(l), Array::Vector(r)) => {
                Array::Vector(r.into_iter().map(|r| l $op r).collect())
            }
            (Array::Vector(l), Array::Scalar(r)) => {
                Array::Vector(l.into_iter().map(|l| l $op r).collect())
            }
            (Array::Vector(l), Array::Vector(r)) => {
                Array::Vector(l.into_iter().zip(r).map(|(l, r)| l $op r).collect())
            }
        }
    };
}

impl Expr {
    fn eval<T>(&self, input: &Input<T>) -> Array<T>
    where
        T: VolumeDataElement
            + Add<Output = T>
            + Sub<Output = T>
            + Mul<Output = T>
            + Div<Output = T>,
    {
        match self {
            Expr::Input(name) => input.get(*name),
            Expr::Value(value) => Array::Scalar(T::from_f64(*value)),
            Expr::Add(lhs, rhs) => binary_op!(lhs.eval(input), rhs.eval(input), +),
            Expr::Sub(lhs, rhs) => binary_op!(lhs.eval(input), rhs.eval(input), -),
            Expr::Mul(lhs, rhs) => binary_op!(lhs.eval(input), rhs.eval(input), *),
            Expr::Div(lhs, rhs) => binary_op!(lhs.eval(input), rhs.eval(input), /),
            Expr::Paren(expr) => expr.eval(input),
        }
    }
}

enum Array<T> {
    Scalar(T),
    Vector(Vec<T>),
}

#[derive(Debug)]
struct Input<'a, T> {
    x: &'a [T],
    x_min: T,
    x_max: T,
    x_mean: T,
    x_std: T,
}

impl<'a, T: VolumeDataElement> Input<'a, T> {
    fn new(x: &'a [T]) -> Self {
        // Aggregate stats need real precision (and unbounded range) to be meaningful, so they're
        // computed over an `f64` copy rather than natively - `x` itself stays untouched.
        let x64: Vec<f64> = x.iter().map(|&v| T::to_f64(v)).collect();

        let x_min = *x64.iter().min_by(|a, b| a.total_cmp(b)).unwrap_or(&0.0);
        let x_max = *x64.iter().max_by(|a, b| a.total_cmp(b)).unwrap_or(&0.0);
        let n = x64.len() as f64;
        let x_mean = x64.iter().sum::<f64>() / n;
        let x_std = (x64.iter().map(|xi| (xi - x_mean).powi(2)).sum::<f64>() / n).sqrt();

        Self {
            x,
            x_min: T::from_f64(x_min),
            x_max: T::from_f64(x_max),
            x_mean: T::from_f64(x_mean),
            x_std: T::from_f64(x_std),
        }
    }

    fn get(&self, name: InputName) -> Array<T> {
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
