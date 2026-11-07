//! A builder that allocates keys for a form set.

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use super::{Choice, Control, DefaultSet, FieldBinding, FormSet, Key, Page, Row, Target, Value};

/// Builds one set as data and allocates the keys.
///
/// The keys are sequential from 1, in creation order, so a rebuilt set has the
/// same keys every time. A consumer that owns persistent state binds the same
/// [`FieldBinding`] to a row and keeps its own keys.
///
/// # Examples
///
/// ```
/// use forms::{Builder, Control, Key};
///
/// let mut builder = Builder::new(Key(1), "Platform");
/// builder.page("Main");
/// builder.row("Secure boot", Control::Toggle).set_binding(Key(10));
/// builder.defaults(Key(20), "Manufacturing", [(Key(10), forms::Value::Bool(false))]);
/// let set = builder.build();
/// assert_eq!(set.pages.len(), 1);
/// assert_eq!(set.pages[0].rows.len(), 1);
/// ```
pub struct Builder {
    set: Key,
    title: String,
    help: String,
    next: u64,
    pages: Vec<Page>,
    defaults: Vec<DefaultSet>,
}

impl Builder {
    /// Creates a builder for a set key and title.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Builder::new(forms::Key(1), "Platform").build().title, "Platform");
    /// ```
    pub fn new(set: Key, title: impl Into<String>) -> Self {
        Self {
            set,
            title: title.into(),
            help: String::new(),
            next: 1,
            pages: Vec::new(),
            defaults: Vec::new(),
        }
    }

    /// Sets the help text of the set.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Builder::new(forms::Key(1), "P").help("Text").build().help, "Text");
    /// ```
    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    /// Allocates a key.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut builder = forms::Builder::new(forms::Key(1), "P");
    /// assert_eq!(builder.key(), forms::Key(1));
    /// assert_eq!(builder.key(), forms::Key(2));
    /// ```
    pub fn key(&mut self) -> Key {
        let key = Key(self.next);
        self.next = self.next.saturating_add(1);
        key
    }

    /// Allocates a key and returns a typed binding to it.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key};
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// let secure = builder.field::<bool>();
    /// builder.page("Main");
    /// builder.row("Secure boot", Control::Toggle).set_binding(secure.key());
    /// ```
    pub fn field<T>(&mut self) -> FieldBinding<T> {
        FieldBinding::new(self.key())
    }

    /// Starts a page with an allocated key.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut builder = forms::Builder::new(forms::Key(1), "P");
    /// builder.page("Main");
    /// assert_eq!(builder.build().pages[0].title, "Main");
    /// ```
    pub fn page(&mut self, title: impl Into<String>) {
        let key = self.key();
        self.pages.push(Page {
            key,
            title: title.into(),
            rows: Vec::new(),
        });
    }

    /// Adds a row to the active page with an allocated key.
    ///
    /// Returns the row so that a caller sets its binding, help, conditions and
    /// presentation. A row without a page is added to a page with an empty
    /// title.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key};
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// builder.row("Mode", Control::Toggle).set_help("Turn it on.");
    /// ```
    pub fn row(&mut self, label: impl Into<String>, control: Control) -> &mut Row {
        if self.pages.is_empty() {
            self.page("");
        }
        let key = self.key();
        let pages = self.pages.len();
        let page = &mut self.pages[pages - 1];
        page.rows.push(Row::new(key, label, control));
        let rows = page.rows.len();
        &mut page.rows[rows - 1]
    }

    /// Adds a link row to another page of the same set.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Key};
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// let other = builder.key();
    /// builder.link("Advanced", other);
    /// builder.page("Advanced");
    /// assert!(builder.build().pages[0].rows[0].control.target().is_some());
    /// ```
    pub fn link(&mut self, label: impl Into<String>, page: Key) -> &mut Row {
        self.row(label, Control::Link(Target::page(page)))
    }

    /// Adds a row with a list of choice values.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key};
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// builder.choice("Mode", [("Fast", 1u64), ("Safe", 2u64)]);
    /// ```
    pub fn choice(
        &mut self,
        label: impl Into<String>,
        choices: impl IntoIterator<Item = (impl Into<String>, u64)>,
    ) -> &mut Row {
        let choices = choices
            .into_iter()
            .map(|(label, value)| Choice::new(label, value))
            .collect();
        self.row(label, Control::Choice(choices))
    }

    /// Adds a named default set.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Key, Value};
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.defaults(Key(8), "Factory", [(Key(9), Value::Bool(true))]);
    /// assert!(builder.build().defaults(Key(8)).is_some());
    /// ```
    pub fn defaults(
        &mut self,
        key: Key,
        name: impl Into<String>,
        values: impl IntoIterator<Item = (Key, Value)>,
    ) {
        self.defaults.push(DefaultSet {
            key,
            name: name.into(),
            values: values.into_iter().collect::<BTreeMap<_, _>>(),
        });
    }

    /// Finishes the set.
    ///
    /// # Examples
    ///
    /// ```
    /// let set = forms::Builder::new(forms::Key(1), "P").build();
    /// assert!(set.pages.is_empty());
    /// ```
    #[must_use]
    pub fn build(self) -> FormSet {
        FormSet {
            key: self.set,
            title: self.title,
            help: self.help,
            pages: self.pages,
            defaults: self.defaults,
        }
    }
}
