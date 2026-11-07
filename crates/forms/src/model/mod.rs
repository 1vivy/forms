//! The form description and its value expressions.
//!
//! The model is the baseline of the crate: everything a HII form set can state,
//! in types that the crate owns. Each file holds one concept:
//!
//! | File | Content |
//! | --- | --- |
//! | `value` | [`Key`], [`Value`], [`Error`] |
//! | `expr` | [`Expr`] and its [`Operand`]s, for hidden, disabled and read-only conditions |
//! | `control` | [`Control`], [`Choice`], [`Target`] |
//! | `row` | [`Row`], its [`Validation`] rules, [`Refresh`] policy and [`Presentation`] hints |
//! | `set` | [`Page`], [`DefaultSet`], [`FormSet`] |
//! | `provider` | [`Store`], [`MemoryStore`], [`Provider`], [`Scope`], [`Request`] |
//! | `builder`, `binding`, `memory` | [`Builder`], [`FieldBinding`], [`MemoryProvider`] |

mod binding;
mod builder;
mod control;
mod expr;
mod memory;
mod provider;
mod row;
mod set;
mod value;

pub use binding::{FieldBinding, FieldValue};
pub use builder::Builder;
pub use control::{Choice, Control, Target};
pub use expr::{Expr, Operand};
pub use memory::MemoryProvider;
pub use provider::{MemoryStore, Provider, Request, Scope, Store};
pub use row::{Presentation, Refresh, Row, Truncation, Validation, ValidationKind};
pub use set::{DefaultSet, FormSet, Page};
pub use value::{Error, Key, Value};
