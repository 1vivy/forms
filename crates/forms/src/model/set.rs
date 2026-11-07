//! Pages, default sets and the published form set.

use super::{Key, Row, Value};
use alloc::{collections::BTreeMap, string::String, vec::Vec};

/// One page of a set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Stable page identity within the set.
    pub key: Key,
    /// The page title.
    pub title: String,
    /// Rows in display order.
    pub rows: Vec<Row>,
}

/// The values that one reset operation writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultSet {
    /// The identity that a [`Control::Reset`](super::Control::Reset) names.
    pub key: Key,
    /// The user-facing name.
    pub name: String,
    /// Value keys and their default values.
    pub values: BTreeMap<Key, Value>,
}

/// One published collection of pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormSet {
    /// Stable set identity within its provider.
    pub key: Key,
    /// The title that a front end lists.
    pub title: String,
    /// The help text for the title.
    pub help: String,
    /// Pages in publication order. The first page is the entry page.
    pub pages: Vec<Page>,
    /// Named default sets.
    pub defaults: Vec<DefaultSet>,
}

impl FormSet {
    /// Finds a page by its key.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{FormSet, Key};
    ///
    /// let set = FormSet {
    ///     key: Key(1),
    ///     title: String::new(),
    ///     help: String::new(),
    ///     pages: Vec::new(),
    ///     defaults: Vec::new(),
    /// };
    /// assert!(set.page(Key(2)).is_none());
    /// ```
    pub fn page(&self, key: Key) -> Option<&Page> {
        self.pages.iter().find(|page| page.key == key)
    }

    /// Finds a row by its key.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, FormSet, Key, Page, Row};
    ///
    /// let set = FormSet {
    ///     key: Key(1),
    ///     title: String::new(),
    ///     help: String::new(),
    ///     pages: vec![Page { key: Key(2), title: String::new(),
    ///         rows: vec![Row::new(Key(3), "Mode", Control::Toggle)] }],
    ///     defaults: Vec::new(),
    /// };
    /// assert!(set.row(Key(3)).is_some());
    /// ```
    pub fn row(&self, key: Key) -> Option<&Row> {
        self.pages
            .iter()
            .flat_map(|page| page.rows.iter())
            .find(|row| row.key == key)
    }

    /// Finds a default set by its key.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{FormSet, Key};
    ///
    /// let set = FormSet {
    ///     key: Key(1),
    ///     title: String::new(),
    ///     help: String::new(),
    ///     pages: Vec::new(),
    ///     defaults: Vec::new(),
    /// };
    /// assert!(set.defaults(Key(4)).is_none());
    /// ```
    pub fn defaults(&self, key: Key) -> Option<&DefaultSet> {
        self.defaults.iter().find(|set| set.key == key)
    }
}
