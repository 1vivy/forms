//! Control handlers and the registry that dispatches them.
//!
//! A row holds a [`Control`]. The kind of the control selects a handler in a
//! [`Registry`]. The built-in kinds use handlers like any consumer kind, so a
//! consumer replaces a built-in handler or adds its own without changing the
//! crate. Only a [`Control::Custom`] row carries a kind that this crate does
//! not define.
//!
//! A handler formats the value column, declares how the value is drawn and
//! changes an edit state from one [`Input`]. It never draws and never touches
//! a store.

mod builtin;

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

use crate::model::{Control, Key, Row, Value};
use crate::session::{Editor, Input};

pub use builtin::{BUILTIN_KINDS, kind_of};

/// The reserved kinds of the built-in controls.
///
/// A consumer inserts a handler for one of these keys to replace the built-in
/// behavior of that kind.
pub mod kind {
    use crate::model::Key;

    /// A heading row.
    pub const HEADING: Key = Key(0xFFFF_0000_0000_0001);
    /// An information row.
    pub const TEXT: Key = Key(0xFFFF_0000_0000_0002);
    /// A link row.
    pub const LINK: Key = Key(0xFFFF_0000_0000_0003);
    /// An action row.
    pub const ACTION: Key = Key(0xFFFF_0000_0000_0004);
    /// A toggle row.
    pub const TOGGLE: Key = Key(0xFFFF_0000_0000_0005);
    /// A choice row.
    pub const CHOICE: Key = Key(0xFFFF_0000_0000_0006);
    /// A number row.
    pub const NUMBER: Key = Key(0xFFFF_0000_0000_0007);
    /// A bounded number row that two keys adjust.
    pub const SLIDER: Key = Key(0xFFFF_0000_0000_000F);
    /// A text row.
    pub const TEXT_INPUT: Key = Key(0xFFFF_0000_0000_0008);
    /// A password row.
    pub const PASSWORD: Key = Key(0xFFFF_0000_0000_0009);
    /// An ordered list row.
    pub const ORDERED_LIST: Key = Key(0xFFFF_0000_0000_000A);
    /// A reset row.
    pub const RESET: Key = Key(0xFFFF_0000_0000_000B);
    /// A progress row.
    pub const PROGRESS: Key = Key(0xFFFF_0000_0000_000C);
    /// A divider row.
    pub const DIVIDER: Key = Key(0xFFFF_0000_0000_000D);
    /// An imported control that the crate cannot show.
    pub const UNSUPPORTED: Key = Key(0xFFFF_0000_0000_000E);
}

/// One row and its current value, as a handler sees them.
#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    /// The row.
    pub row: &'a Row,
    /// The current value, when the row is bound and the read succeeded.
    pub value: Option<&'a Value>,
}

/// How a renderer draws the value of a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decoration {
    /// Text only.
    Plain,
    /// A heading with an optional rule.
    Heading,
    /// A divider.
    Rule,
    /// A switch, with its current state.
    Toggle(bool),
    /// A progress bar, determinate or indeterminate.
    Bar {
        /// Completed units.
        current: u64,
        /// Total units.
        total: Option<u64>,
    },
    /// A slider bar with its position.
    Slider {
        /// The current value.
        position: u64,
        /// The inclusive lower bound.
        minimum: u64,
        /// The inclusive upper bound.
        maximum: u64,
    },
    /// A consumer decoration, drawn by a painter that the consumer registers.
    Custom {
        /// The registered painter kind.
        kind: Key,
        /// Consumer-defined description.
        data: Vec<u8>,
    },
}

/// The result of one input against an open editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditStep {
    /// The input has no effect.
    Ignored,
    /// The editor state changed.
    Changed,
    /// The editor finished with this value.
    Accept(Value),
    /// The handler refuses the input with a reason.
    Refused(String),
    /// Run the command of the row without a value.
    Invoke,
}

/// The mutable state of one open editor.
///
/// A handler keeps all editing state here, so the session holds no per-control
/// knowledge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditState {
    /// The text of a number or text editor.
    pub text: String,
    /// The cursor in `text`, in characters.
    pub cursor: usize,
    /// The cursor in an option list, or the selected choice.
    pub selected: usize,
    /// The chosen values of an ordered list, in order.
    pub values: Vec<u64>,
    /// The position of a slider.
    pub value: u64,
    /// A warning was shown and this input confirms it.
    pub confirming: bool,
}

/// Formats a control, describes its drawing and edits its value.
pub trait Handler {
    /// Returns the value column text.
    fn format(&self, field: &Field<'_>) -> String;

    /// Returns the drawing of the value.
    fn decoration(&self, field: &Field<'_>) -> Decoration {
        let _ = field;
        Decoration::Plain
    }

    /// Applies one input to an open editor.
    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let _ = (field, state, input);
        EditStep::Ignored
    }

    /// Returns whether Enter opens an editor instead of running the row.
    fn opens_editor(&self) -> bool {
        false
    }

    /// Returns the initial editor state of a row.
    fn start(&self, field: &Field<'_>) -> EditState {
        let _ = field;
        EditState::default()
    }

    /// Returns the editor view of a row, or `None` when the row has no editor.
    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        let _ = (field, state);
        None
    }

    /// Returns the value and decoration of a row while its editor is open.
    ///
    /// A control that adjusts a value in the row itself implements this, so
    /// the row follows the editor state before the value is written. The
    /// default keeps the stored value.
    fn format_editing(&self, field: &Field<'_>, state: &EditState) -> Option<(String, Decoration)> {
        let _ = (field, state);
        None
    }
}

/// The handlers of one session.
///
/// [`Registry::builtin`] holds one handler for each built-in kind. A consumer
/// inserts another handler for a built-in kind to replace it, or for a
/// consumer kind to add a control.
pub struct Registry {
    handlers: BTreeMap<Key, Box<dyn Handler>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Registry {
    /// Creates a registry with the built-in handlers.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::Registry;
    ///
    /// assert!(forms::Registry::builtin().contains(forms::ext::kind::TOGGLE));
    /// ```
    pub fn builtin() -> Self {
        let mut registry = Self {
            handlers: BTreeMap::new(),
        };
        for (kind, handler) in builtin::handlers() {
            registry.handlers.insert(kind, handler);
        }
        registry
    }

    /// Inserts a handler for a kind, replacing a previous handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::ext::kind;
    /// use forms::{Decoration, Field, Handler, Registry};
    ///
    /// struct Bare;
    /// impl Handler for Bare {
    ///     fn format(&self, _field: &Field<'_>) -> String { String::from("bare") }
    ///     fn decoration(&self, _field: &Field<'_>) -> Decoration { Decoration::Plain }
    /// }
    ///
    /// let mut registry = Registry::builtin();
    /// registry.insert(kind::TOGGLE, Box::new(Bare));
    /// ```
    pub fn insert(&mut self, kind: Key, handler: Box<dyn Handler>) {
        self.handlers.insert(kind, handler);
    }

    /// Returns whether a handler is registered for a kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Registry, ext::kind};
    ///
    /// assert!(Registry::builtin().contains(kind::NUMBER));
    /// assert!(!Registry::builtin().contains(forms::Key(1)));
    /// ```
    pub fn contains(&self, kind: Key) -> bool {
        self.handlers.contains_key(&kind)
    }

    /// Returns the handler of a control.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Registry};
    ///
    /// assert!(Registry::builtin().handler(&Control::Toggle).is_some());
    /// ```
    pub fn handler(&self, control: &Control) -> Option<&dyn Handler> {
        let kind = kind_of(control);
        self.handlers.get(&kind).map(Box::as_ref)
    }

    /// Formats the value of a row and returns its decoration.
    ///
    /// A control without a handler formats as an imported control and draws as
    /// plain text, so a foreign row stays visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, Decoration, Key, Registry, Row, Value};
    ///
    /// let row = Row::new(Key(1), "Mode", Control::Toggle);
    /// let value = Value::Bool(true);
    /// let (text, decoration) = Registry::builtin().format(&row, Some(&value));
    /// assert_eq!(text, "On");
    /// assert_eq!(decoration, Decoration::Toggle(true));
    /// ```
    pub fn format(&self, row: &Row, value: Option<&Value>) -> (String, Decoration) {
        let field = Field { row, value };
        match self.handler(&row.control) {
            Some(handler) => (handler.format(&field), handler.decoration(&field)),
            None => (String::from("<unsupported>"), Decoration::Plain),
        }
    }

    /// Applies one input to the editor of a row.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Control, EditState, EditStep, Key, Registry, Row};
    ///
    /// let row = Row::new(Key(1), "Mode", Control::Toggle);
    /// let mut state = EditState::default();
    /// let step = Registry::builtin().edit(&row, None, &mut state, forms::Input::Enter);
    /// assert_eq!(step, EditStep::Accept(forms::Value::Bool(true)));
    /// ```
    pub fn edit(
        &self,
        row: &Row,
        value: Option<&Value>,
        state: &mut EditState,
        input: Input,
    ) -> EditStep {
        let field = Field { row, value };
        match self.handler(&row.control) {
            Some(handler) => handler.edit(&field, state, input),
            None => EditStep::Ignored,
        }
    }
}
