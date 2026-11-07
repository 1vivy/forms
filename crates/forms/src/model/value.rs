//! Keys, values and the error a store or provider returns.

use alloc::{string::String, vec::Vec};

/// A stable application key.
///
/// The crate never derives meaning from the number. A consumer may use the
/// same key for a set, a page, a row, a value or a control kind, because each
/// namespace is separate. `Builder` allocates keys for a set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(pub u64);

/// A value in an application store.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    /// A logical value.
    Bool(bool),
    /// A nonnegative integer.
    Number(u64),
    /// Unicode text.
    Text(String),
    /// An ordered sequence of option values.
    List(Vec<u64>),
    /// Consumer-defined data.
    Bytes(Vec<u8>),
}

/// A service failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Consumer-defined error code.
    pub code: u64,
    /// A short reason for the person using the page.
    pub reason: String,
}

impl Error {
    /// Creates an error with a message and code 1.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Error::new("No value").code, 1);
    /// ```
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            code: 1,
            reason: reason.into(),
        }
    }
}
