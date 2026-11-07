//! Typed bindings to consumer state.

use alloc::string::String;
use core::marker::PhantomData;

use super::{Error, Key, Store, Value};

/// A Rust type that a store value can hold.
pub trait FieldValue: Sized {
    /// Converts the value to a store value.
    fn into_value(self) -> Value;

    /// Reads the value from a store value.
    ///
    /// # Errors
    ///
    /// Returns a failure when the stored value has another type.
    fn from_value(value: &Value) -> Result<Self, Error>;
}

impl FieldValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }

    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            _ => Err(Error::new("The value is not a logical value")),
        }
    }
}

impl FieldValue for u64 {
    fn into_value(self) -> Value {
        Value::Number(self)
    }

    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Number(number) => Ok(*number),
            _ => Err(Error::new("The value is not a number")),
        }
    }
}

impl FieldValue for u16 {
    fn into_value(self) -> Value {
        Value::Number(u64::from(self))
    }

    fn from_value(value: &Value) -> Result<Self, Error> {
        u16::try_from(u64::from_value(value)?)
            .map_err(|_| Error::new("The value is larger than 16 bits"))
    }
}

impl FieldValue for u8 {
    fn into_value(self) -> Value {
        Value::Number(u64::from(self))
    }

    fn from_value(value: &Value) -> Result<Self, Error> {
        u8::try_from(u64::from_value(value)?)
            .map_err(|_| Error::new("The value is larger than 8 bits"))
    }
}

impl FieldValue for String {
    fn into_value(self) -> Value {
        Value::Text(self)
    }

    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Text(text) => Ok(text.clone()),
            _ => Err(Error::new("The value is not text")),
        }
    }
}

impl FieldValue for Value {
    fn into_value(self) -> Value {
        self
    }

    fn from_value(value: &Value) -> Result<Self, Error> {
        Ok(value.clone())
    }
}

/// A typed handle to one value key.
///
/// A page binds a row to a field, so the consumer code keeps its own record
/// types and no numeric id appears twice. The binding names a key only; it
/// holds no value.
///
/// # Examples
///
/// ```
/// use forms::{FieldBinding, Key, MemoryStore, Store, Value};
///
/// let field = FieldBinding::<bool>::new(Key(9));
/// let mut store = MemoryStore::default();
/// store.write(Key(9), &Value::Bool(true)).unwrap();
/// assert_eq!(field.get(&mut store), Ok(true));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldBinding<T> {
    key: Key,
    marker: PhantomData<fn() -> T>,
}

impl<T> FieldBinding<T> {
    /// Creates a binding for one key.
    ///
    /// # Examples
    ///
    /// ```
    /// let field = forms::FieldBinding::<u64>::new(forms::Key(3));
    /// assert_eq!(field.key(), forms::Key(3));
    /// ```
    pub const fn new(key: Key) -> Self {
        Self {
            key,
            marker: PhantomData,
        }
    }

    /// Returns the value key.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::FieldBinding::<u8>::new(forms::Key(1)).key(), forms::Key(1));
    /// ```
    pub const fn key(self) -> Key {
        self.key
    }
}

impl<T: FieldValue> FieldBinding<T> {
    /// Reads the value of the field.
    ///
    /// # Errors
    ///
    /// Returns the store failure or a type mismatch.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{FieldBinding, Key, MemoryStore, Store, Value};
    ///
    /// let field = FieldBinding::<u64>::new(Key(2));
    /// let mut store = MemoryStore::default();
    /// store.write(Key(2), &Value::Number(7)).unwrap();
    /// assert_eq!(field.get(&mut store), Ok(7));
    /// ```
    pub fn get(&self, store: &mut impl Store) -> Result<T, Error> {
        T::from_value(&store.read(self.key)?)
    }

    /// Writes the value of the field.
    ///
    /// # Errors
    ///
    /// Returns the store failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{FieldBinding, Key, MemoryStore, Store, Value};
    ///
    /// let field = FieldBinding::<bool>::new(Key(2));
    /// let mut store = MemoryStore::default();
    /// store.write(Key(2), &Value::Bool(false)).unwrap();
    /// field.set(&mut store, true).unwrap();
    /// assert_eq!(store.read(Key(2)), Ok(Value::Bool(true)));
    /// ```
    pub fn set(&self, store: &mut impl Store, value: T) -> Result<(), Error> {
        store.write(self.key, &value.into_value())
    }
}
