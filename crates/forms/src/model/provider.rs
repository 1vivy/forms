//! Where values live: the store and provider traits and the requests they
//! return.

use super::{Error, FormSet, Key, Row, Value};
use alloc::{collections::BTreeMap, vec::Vec};

/// The part of a set that an operation covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every page of the set.
    Set,
    /// One page.
    Page(Key),
    /// One row.
    Row(Key),
}

/// A request from an application.
///
/// A store or provider returns a request after a write or an action. The
/// session performs the request before it returns the event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Request {
    /// Continue browsing.
    #[default]
    None,
    /// Ask the host to restart.
    Restart,
    /// Commit a scope, and leave it when `leave` is true.
    Apply {
        /// The scope to commit.
        scope: Scope,
        /// Whether the session leaves the scope after the commit.
        leave: bool,
    },
    /// Restore a scope, and leave it when `leave` is true.
    Discard {
        /// The scope to restore.
        scope: Scope,
        /// Whether the session leaves the scope after the restore.
        leave: bool,
    },
    /// Ask the host to reconnect its service.
    Reconnect,
}

/// Reads and changes application values.
///
/// A write must leave the data unchanged when it returns an error, because the
/// session does not undo it.
pub trait Store {
    /// Reads a value.
    ///
    /// # Errors
    ///
    /// Returns the failure when the value cannot be read.
    fn read(&mut self, key: Key) -> Result<Value, Error>;

    /// Changes a value.
    ///
    /// # Errors
    ///
    /// Returns the failure when the value cannot be stored.
    fn write(&mut self, key: Key, value: &Value) -> Result<(), Error>;

    /// Runs a command of an action row.
    ///
    /// # Errors
    ///
    /// Returns the failure of the command. The default answers that the
    /// command is unknown.
    fn action(&mut self, key: Key) -> Result<Request, Error> {
        let _ = key;
        Err(Error::new("Unknown action"))
    }

    /// Commits the changes of a scope.
    ///
    /// # Errors
    ///
    /// Returns the failure of the commit.
    fn apply(&mut self, scope: Scope) -> Result<(), Error> {
        let _ = scope;
        Ok(())
    }

    /// Restores the changes of a scope.
    ///
    /// # Errors
    ///
    /// Returns the failure of the restore.
    fn discard(&mut self, scope: Scope) -> Result<(), Error> {
        let _ = scope;
        Ok(())
    }
}

/// An owned store with a restore record.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    /// The current values.
    pub values: BTreeMap<Key, Value>,
    /// The values from before their first change.
    originals: BTreeMap<Key, Value>,
}

impl Store for MemoryStore {
    fn read(&mut self, key: Key) -> Result<Value, Error> {
        self.values
            .get(&key)
            .cloned()
            .ok_or_else(|| Error::new("The value is missing"))
    }

    fn write(&mut self, key: Key, value: &Value) -> Result<(), Error> {
        if let Some(original) = self.values.get(&key).cloned() {
            self.originals.entry(key).or_insert(original);
        }
        self.values.insert(key, value.clone());
        Ok(())
    }

    fn apply(&mut self, _scope: Scope) -> Result<(), Error> {
        self.originals.clear();
        Ok(())
    }

    fn discard(&mut self, _scope: Scope) -> Result<(), Error> {
        self.values.append(&mut self.originals);
        Ok(())
    }
}

/// An external collection of sets and their values.
///
/// A HII adapter implements it over the firmware database. A consumer may also
/// implement it over its own services.
pub trait Provider {
    /// The identity of one published source.
    type Handle: Copy + Eq;

    /// Lists the published sets.
    ///
    /// # Errors
    ///
    /// Returns the failure when the list cannot be read.
    fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error>;

    /// Reads the value of a bound row.
    ///
    /// # Errors
    ///
    /// Returns the failure when the value cannot be read.
    fn read(&mut self, handle: Self::Handle, set: &FormSet, row: &Row) -> Result<Value, Error>;

    /// Changes the value of a bound row.
    ///
    /// # Errors
    ///
    /// Returns the failure when the value cannot be stored.
    fn write(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        row: &Row,
        value: &Value,
    ) -> Result<Request, Error>;

    /// Runs the command of an action row.
    ///
    /// # Errors
    ///
    /// Returns the failure of the command.
    fn action(&mut self, handle: Self::Handle, set: &FormSet, row: &Row) -> Result<Request, Error>;

    /// Commits the changes of a scope.
    ///
    /// # Errors
    ///
    /// Returns the failure of the commit.
    fn apply(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        let _ = (handle, set, scope);
        Ok(())
    }

    /// Restores the changes of a scope.
    ///
    /// # Errors
    ///
    /// Returns the failure of the restore.
    fn discard(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        let _ = (handle, set, scope);
        Ok(())
    }

    /// Reconnects the service behind the set.
    ///
    /// # Errors
    ///
    /// Returns the failure of the reconnect.
    fn reconnect(&mut self, handle: Self::Handle) -> Result<(), Error> {
        let _ = handle;
        Ok(())
    }
}
