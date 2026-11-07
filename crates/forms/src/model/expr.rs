//! The condition language of hidden, disabled and read-only rows.

use super::{Error, Key, Value};
use alloc::{boxed::Box, string::String, vec::Vec};

/// A source for an expression operand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operand {
    /// A value read from the active store or provider.
    Field(Key),
    /// A literal value.
    Literal(Value),
}

/// A small expression language over row values.
///
/// Evaluation returns an error for a missing value, a type mismatch and
/// [`Expr::Unavailable`]. It never treats an unknown result as `false`, so a
/// condition that cannot be evaluated does not silently enable a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// A constant truth value.
    Bool(bool),
    /// Equal operands.
    Equal(Operand, Operand),
    /// The first operand is smaller than the second, with the same type.
    Less(Operand, Operand),
    /// The first operand is smaller than or equal to the second.
    LessEqual(Operand, Operand),
    /// The operand is one of the listed values.
    In(Operand, Vec<Value>),
    /// Logical conjunction.
    And(Box<Expr>, Box<Expr>),
    /// Logical disjunction.
    Or(Box<Expr>, Box<Expr>),
    /// Logical negation.
    Not(Box<Expr>),
    /// An imported expression that this crate cannot evaluate.
    Unavailable(String),
}

impl Expr {
    /// Evaluates the expression against a value reader.
    ///
    /// # Errors
    ///
    /// Returns the reader failure, a type mismatch, or the reason of
    /// [`Expr::Unavailable`].
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Expr, Operand, Value};
    ///
    /// let expr = Expr::Equal(
    ///     Operand::Field(forms::Key(1)),
    ///     Operand::Literal(Value::Number(2)),
    /// );
    /// let mut read = |_| Ok(Value::Number(2));
    /// assert_eq!(expr.evaluate(&mut read), Ok(true));
    /// ```
    pub fn evaluate(
        &self,
        read: &mut impl FnMut(Key) -> Result<Value, Error>,
    ) -> Result<bool, Error> {
        match self {
            Self::Bool(value) => Ok(*value),
            Self::Equal(left, right) => Ok(operand(left, read)? == operand(right, read)?),
            Self::Less(left, right) | Self::LessEqual(left, right) => {
                let left = operand(left, read)?;
                let right = operand(right, read)?;
                if core::mem::discriminant(&left) != core::mem::discriminant(&right) {
                    return Err(Error::new("Cannot compare different value types"));
                }
                Ok(if matches!(self, Self::Less(..)) {
                    left < right
                } else {
                    left <= right
                })
            }
            Self::In(value, list) => Ok(list.contains(&operand(value, read)?)),
            Self::And(left, right) => Ok(left.evaluate(read)? && right.evaluate(read)?),
            Self::Or(left, right) => Ok(left.evaluate(read)? || right.evaluate(read)?),
            Self::Not(value) => Ok(!value.evaluate(read)?),
            Self::Unavailable(reason) => Err(Error::new(reason.clone())),
        }
    }
}

/// Reads one operand.
fn operand(
    operand: &Operand,
    read: &mut impl FnMut(Key) -> Result<Value, Error>,
) -> Result<Value, Error> {
    match operand {
        Operand::Field(key) => read(*key),
        Operand::Literal(value) => Ok(value.clone()),
    }
}
