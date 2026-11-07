//! One row of a page, with its conditions, validation, refresh and
//! presentation hints.

use super::{Control, Expr, Key};
use alloc::{string::String, vec::Vec};

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
