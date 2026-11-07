//! The form description and its value expressions.

mod binding;
mod builder;
mod memory;

pub use binding::{FieldBinding, FieldValue};
pub use builder::Builder;
pub use memory::MemoryProvider;

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

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

/// A choice label and its stable stored value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The text that the row shows.
    pub label: String,
    /// The stored value.
    pub value: u64,
}

impl Choice {
    /// Creates a choice.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Choice::new("Fast", 1).value, 1);
    /// ```
    pub fn new(label: impl Into<String>, value: u64) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

/// A navigation destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Another set, or the current set when absent.
    pub set: Option<Key>,
    /// The destination page.
    pub page: Key,
    /// The row to select on arrival.
    pub row: Option<Key>,
}

impl Target {
    /// Creates a target in the current set.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Target::page(forms::Key(4)).page, forms::Key(4));
    /// ```
    pub fn page(page: Key) -> Self {
        Self {
            set: None,
            page,
            row: None,
        }
    }
}

/// What a row does and shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// A heading line.
    Heading,
    /// An information value. A binding supplies live text.
    Text(String),
    /// A link to another page.
    Link(Target),
    /// An application command.
    Action,
    /// A logical switch.
    Toggle,
    /// Select one option.
    Choice(Vec<Choice>),
    /// Edit a number within limits. Step 0 permits free entry.
    Number {
        /// Inclusive lower bound.
        minimum: u64,
        /// Inclusive upper bound.
        maximum: u64,
        /// Increment from the lower bound, or 0 for free entry.
        step: u64,
    },
    /// Adjust a bounded number with two keys. Step 0 is not allowed.
    Slider {
        /// Inclusive lower bound.
        minimum: u64,
        /// Inclusive upper bound.
        maximum: u64,
        /// The increment per key press. Must be 1 or more.
        step: u64,
        /// The unit shown after the value, for example `s`.
        unit: String,
        /// Labels for special values, for example 0 for `Off`.
        labels: Vec<Choice>,
    },
    /// Edit Unicode text.
    TextInput {
        /// Minimum character count.
        minimum: usize,
        /// Maximum character count.
        maximum: usize,
        /// Conceal the text in every view.
        secret: bool,
    },
    /// Reorder option values.
    OrderedList {
        /// The options that a person may choose.
        choices: Vec<Choice>,
        /// The largest number of chosen values.
        maximum: usize,
    },
    /// Restore one named default set.
    Reset(Key),
    /// A progress bar, or an indeterminate busy indicator without a total.
    Progress {
        /// Completed units.
        current: u64,
        /// Total units.
        total: Option<u64>,
    },
    /// A horizontal divider.
    Divider,
    /// An imported control that this crate cannot show.
    Unsupported(String),
    /// A registered consumer control.
    Custom {
        /// The registered kind.
        kind: Key,
        /// The consumer-defined description.
        data: Vec<u8>,
    },
}

impl Control {
    /// Returns the navigation target of a link.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Key, Target};
    ///
    /// let link = Control::Link(Target::page(Key(2)));
    /// assert_eq!(link.target().map(|target| target.page), Some(Key(2)));
    /// ```
    pub fn target(&self) -> Option<&Target> {
        match self {
            Self::Link(target) => Some(target),
            _ => None,
        }
    }

    /// Returns the default set of a reset row.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Key};
    ///
    /// assert_eq!(Control::Reset(Key(7)).reset_default(), Some(Key(7)));
    /// ```
    pub fn reset_default(&self) -> Option<Key> {
        match self {
            Self::Reset(key) => Some(*key),
            _ => None,
        }
    }

    /// Returns whether a person can act on the row.
    ///
    /// Headings, text, progress and dividers carry no action.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::Control;
    ///
    /// assert!(!Control::Heading.is_interactive());
    /// assert!(Control::Toggle.is_interactive());
    /// ```
    pub fn is_interactive(&self) -> bool {
        !matches!(
            self,
            Self::Heading
                | Self::Text(_)
                | Self::Progress { .. }
                | Self::Divider
                | Self::Unsupported(_)
        )
    }
}

/// When a validation rule applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationKind {
    /// Ask for confirmation before the edit is accepted.
    Warning,
    /// Refuse an edit that leaves inconsistent values.
    Edit,
    /// Refuse submission of the active scope.
    Submit,
}

/// A condition and the message that it produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validation {
    /// The point at which the rule applies.
    pub kind: ValidationKind,
    /// True means that the values are not acceptable.
    pub condition: Expr,
    /// The message shown on refusal or confirmation.
    pub message: String,
    /// Seconds before a warning accepts itself, or 0 for explicit input.
    pub timeout: u8,
}

impl Validation {
    /// Creates an edit rule that refuses the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Expr, Validation, ValidationKind};
    ///
    /// let rule = Validation::refuse(Expr::Bool(true), "Not allowed");
    /// assert_eq!(rule.kind, ValidationKind::Edit);
    /// ```
    pub fn refuse(condition: Expr, message: impl Into<String>) -> Self {
        Self {
            kind: ValidationKind::Edit,
            condition,
            message: message.into(),
            timeout: 0,
        }
    }

    /// Creates a warning rule that asks for confirmation.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Expr, Validation};
    ///
    /// let rule = Validation::warn(Expr::Bool(true), "This erases data");
    /// assert_eq!(rule.timeout, 0);
    /// ```
    pub fn warn(condition: Expr, message: impl Into<String>) -> Self {
        Self {
            kind: ValidationKind::Warning,
            condition,
            message: message.into(),
            timeout: 0,
        }
    }

    /// Creates a rule that refuses submission of the scope.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Expr, Validation};
    ///
    /// let rule = Validation::block_submit(Expr::Bool(true), "Fix the values");
    /// assert_eq!(rule.message, "Fix the values");
    /// ```
    pub fn block_submit(condition: Expr, message: impl Into<String>) -> Self {
        Self {
            kind: ValidationKind::Submit,
            condition,
            message: message.into(),
            timeout: 0,
        }
    }
}

/// When to re-read a bound value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Refresh {
    /// Read when the page is drawn.
    #[default]
    OnView,
    /// Re-read after this many milliseconds.
    Periodic(u64),
    /// Re-read when the consumer signals this key.
    Event(Key),
}

/// How a renderer handles a value that does not fit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Truncation {
    /// Use the policy of the theme.
    #[default]
    Theme,
    /// Cut the end and mark the cut.
    End,
    /// Wrap into the value column.
    Wrap,
    /// Cut without a mark.
    Clip,
}

/// Presentation hints for a renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Presentation {
    /// Maximum label width in cells, or the limit of the theme.
    pub label_limit: Option<u16>,
    /// The overflow policy for the value.
    pub truncation: Truncation,
}

/// One row of a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Stable row identity within the set.
    pub key: Key,
    /// The label shown beside the value.
    pub label: String,
    /// The context help of the selected row.
    pub help: String,
    /// The value key that the row reads and writes.
    pub binding: Option<Key>,
    /// The behavior and data of the row.
    pub control: Control,
    /// The row is hidden while this expression is true.
    pub hidden: Option<Expr>,
    /// The row cannot be selected while this expression is true.
    pub disabled: Option<Expr>,
    /// The row stays selectable but its value cannot change while true.
    pub read_only: Option<Expr>,
    /// The value cannot change, whatever the conditions say.
    pub locked: bool,
    /// The rules that apply to edits and submission.
    pub validation: Vec<Validation>,
    /// The re-read schedule.
    pub refresh: Refresh,
    /// Presentation hints.
    pub presentation: Presentation,
}

impl Row {
    /// Creates a row with default conditions.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Key, Row};
    ///
    /// let row = Row::new(Key(1), "Secure boot", Control::Toggle);
    /// assert!(!row.locked);
    /// ```
    pub fn new(key: Key, label: impl Into<String>, control: Control) -> Self {
        Self {
            key,
            label: label.into(),
            help: String::new(),
            binding: None,
            control,
            hidden: None,
            disabled: None,
            read_only: None,
            locked: false,
            validation: Vec::new(),
            refresh: Refresh::OnView,
            presentation: Presentation::default(),
        }
    }

    /// Binds the row to a value key.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Key, Row};
    ///
    /// let row = Row::new(Key(1), "Mode", Control::Toggle).bind(Key(2));
    /// assert_eq!(row.binding, Some(Key(2)));
    /// ```
    #[must_use]
    pub fn bind(mut self, key: Key) -> Self {
        self.binding = Some(key);
        self
    }

    /// Sets the context help.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Key, Row};
    ///
    /// assert_eq!(Row::new(Key(1), "Mode", Control::Toggle).help("Text").help, "Text");
    /// ```
    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    /// Adds a validation rule.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Expr, Key, Row, Validation};
    ///
    /// let row = Row::new(Key(1), "Mode", Control::Toggle)
    ///     .validate(Validation::refuse(Expr::Bool(false), "Never"));
    /// assert_eq!(row.validation.len(), 1);
    /// ```
    #[must_use]
    pub fn validate(mut self, rule: Validation) -> Self {
        self.validation.push(rule);
        self
    }

    /// Binds the row to a value key in place.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Key, Row};
    ///
    /// let mut row = Row::new(Key(1), "Mode", Control::Toggle);
    /// row.set_binding(Key(2));
    /// assert_eq!(row.binding, Some(Key(2)));
    /// ```
    pub fn set_binding(&mut self, key: Key) -> &mut Self {
        self.binding = Some(key);
        self
    }

    /// Sets the context help in place.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Key, Row};
    ///
    /// let mut row = Row::new(Key(1), "Mode", Control::Toggle);
    /// row.set_help("Text");
    /// assert_eq!(row.help, "Text");
    /// ```
    pub fn set_help(&mut self, help: impl Into<String>) -> &mut Self {
        self.help = help.into();
        self
    }

    /// Adds a validation rule in place.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Expr, Key, Row, Validation};
    ///
    /// let mut row = Row::new(Key(1), "Mode", Control::Toggle);
    /// row.push_validation(Validation::warn(Expr::Bool(true), "Careful"));
    /// assert_eq!(row.validation.len(), 1);
    /// ```
    pub fn push_validation(&mut self, rule: Validation) -> &mut Self {
        self.validation.push(rule);
        self
    }
}

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
    /// The identity that a [`Control::Reset`] names.
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
