//! An owned provider for tests, examples and small applications.

use alloc::vec::Vec;

use super::{Error, FormSet, Key, MemoryStore, Provider, Request, Row, Scope, Store, Value};

/// A provider that holds its sets and values in memory.
///
/// The handle is the position of the set in [`Self::sets`]. The connected
/// store is [`Self::store`]. An action row records its key in
/// [`Self::actions`].
///
/// # Examples
///
/// ```
/// use forms::{Builder, Control, Key, MemoryProvider, Provider, Value};
///
/// let mut builder = Builder::new(Key(1), "Platform");
/// builder.page("Main");
/// builder.row("Secure boot", Control::Toggle).set_binding(Key(10));
/// let mut provider = MemoryProvider::new(vec![(0u8, builder.build())]);
/// provider.store.values.insert(Key(10), Value::Bool(true));
/// let sets = provider.formsets().unwrap();
/// assert_eq!(sets.len(), 1);
/// ```
#[derive(Clone, Debug, Default)]
pub struct MemoryProvider {
    /// The published sets and their handles.
    pub sets: Vec<(u8, FormSet)>,
    /// The current values.
    pub store: MemoryStore,
    /// The action keys that the session ran, in order.
    pub actions: Vec<Key>,
}

impl MemoryProvider {
    /// Creates a provider over published sets.
    ///
    /// # Examples
    ///
    /// ```
    /// let provider = forms::MemoryProvider::new(Vec::new());
    /// assert!(provider.sets.is_empty());
    /// ```
    pub fn new(sets: Vec<(u8, FormSet)>) -> Self {
        Self {
            sets,
            store: MemoryStore::default(),
            actions: Vec::new(),
        }
    }

    /// Adds a value to the store.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut provider = forms::MemoryProvider::new(Vec::new());
    /// provider.set(forms::Key(3), forms::Value::Number(2));
    /// assert_eq!(provider.store.values.len(), 1);
    /// ```
    pub fn set(&mut self, key: Key, value: Value) {
        self.store.values.insert(key, value);
    }
}

impl Provider for MemoryProvider {
    type Handle = u8;

    fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error> {
        Ok(self.sets.clone())
    }

    fn read(&mut self, handle: Self::Handle, set: &FormSet, row: &Row) -> Result<Value, Error> {
        let _ = (handle, set);
        let key = row
            .binding
            .ok_or_else(|| Error::new("The row has no value"))?;
        self.store.read(key)
    }

    fn write(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        row: &Row,
        value: &Value,
    ) -> Result<Request, Error> {
        let _ = (handle, set);
        let key = row
            .binding
            .ok_or_else(|| Error::new("The row has no value"))?;
        self.store.write(key, value)?;
        Ok(Request::None)
    }

    fn action(&mut self, handle: Self::Handle, set: &FormSet, row: &Row) -> Result<Request, Error> {
        let _ = (handle, set);
        self.actions.push(row.key);
        Ok(Request::None)
    }

    fn apply(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        let _ = (handle, set);
        self.store.apply(scope)
    }

    fn discard(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        let _ = (handle, set);
        self.store.discard(scope)
    }
}

impl MemoryProvider {
    /// Returns the title of the set behind a handle.
    ///
    /// # Examples
    ///
    /// ```
    /// let provider = forms::MemoryProvider::new(Vec::new());
    /// assert!(provider.title(0).is_none());
    /// ```
    pub fn title(&self, handle: u8) -> Option<&str> {
        self.sets
            .iter()
            .find(|(other, _)| *other == handle)
            .map(|(_, set)| set.title.as_str())
    }

    /// Returns the first set with this key.
    ///
    /// # Examples
    ///
    /// ```
    /// let provider = forms::MemoryProvider::new(Vec::new());
    /// assert!(provider.set_with(forms::Key(1)).is_none());
    /// ```
    pub fn set_with(&self, key: Key) -> Option<&FormSet> {
        self.sets
            .iter()
            .find(|(_, set)| set.key == key)
            .map(|(_, set)| set)
    }
}
