//! What a row does: its control, the choices it offers and the page it links.

use super::Key;
use alloc::{string::String, vec::Vec};

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
