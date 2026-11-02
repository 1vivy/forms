//! Platform-neutral setup forms.
//!
//! This crate is the model that a firmware driver, a recovery application and
//! an operating-system tool share. It follows the UEFI HII forms model
//! (UEFI 2.10 chapter 33): a *formset* holds *forms*, a form holds
//! *statements*, and a *question* is a statement with a value in a
//! *variable store*. The model has no firmware dependency, so it compiles for
//! a UEFI driver, a UEFI application and a Linux program.
//!
//! - [`FormSet`] and its parts describe what a provider publishes.
//! - [`Provider`] reads and changes question values and runs actions. A UEFI
//!   application implements it over the HII database and Config Access; a
//!   Linux program implements it over its own services.
//! - [`Session`] is the navigation state for one front end. It turns
//!   [`Input`] into provider calls and produces a [`View`] that a renderer
//!   draws. It draws nothing itself.
//!
//! `hii-ifr` encodes and decodes this model as IFR and string packages.
//!
//! # Examples
//!
//! ```
//! use forms::{Form, FormId, FormSet, Statement};
//!
//! let set = FormSet {
//!     guid: [0x11; 16],
//!     title: "Platform".into(),
//!     help: "Platform settings".into(),
//!     class_guids: Vec::new(),
//!     varstores: Vec::new(),
//!     forms: vec![Form {
//!         id: FormId(1),
//!         title: "Platform".into(),
//!         items: vec![Statement::Subtitle { text: "State".into() }.into()],
//!     }],
//! };
//! assert_eq!(set.form(FormId(1)).map(|form| form.items.len()), Some(1));
//! ```
#![no_std]

extern crate alloc;

mod session;

use alloc::{boxed::Box, string::String, vec::Vec};

pub use session::{Editor, EditorKind, Event, Input, Row, RowKind, Session, View};

/// A GUID in the UEFI byte order (`EFI_GUID`: little-endian first three fields).
pub type Guid = [u8; 16];

/// The identifier of a form inside its formset (`EFI_FORM_ID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormId(pub u16);

/// The identifier of a question inside its formset (`EFI_QUESTION_ID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuestionId(pub u16);

/// The identifier of a variable store inside its formset (`EFI_VARSTORE_ID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarStoreId(pub u16);

/// One published formset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormSet {
    /// The formset GUID. A provider chooses its own; two providers never share one.
    pub guid: Guid,
    /// The title that a front end lists.
    pub title: String,
    /// The help text for the title.
    pub help: String,
    /// Class GUIDs (`EFI_IFR_FORM_SET.ClassGuid`); a front end may filter on them.
    pub class_guids: Vec<Guid>,
    /// The variable stores that questions refer to.
    pub varstores: Vec<VarStore>,
    /// The forms, in publication order. The first form is the entry form.
    pub forms: Vec<Form>,
}

impl FormSet {
    /// Returns the form with this identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{FormId, FormSet};
    ///
    /// let set = FormSet {
    ///     guid: [0; 16],
    ///     title: String::new(),
    ///     help: String::new(),
    ///     class_guids: Vec::new(),
    ///     varstores: Vec::new(),
    ///     forms: Vec::new(),
    /// };
    /// assert!(set.form(FormId(1)).is_none());
    /// ```
    pub fn form(&self, id: FormId) -> Option<&Form> {
        self.forms.iter().find(|form| form.id == id)
    }

    /// Returns the variable store with this identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{FormSet, VarStoreId};
    ///
    /// let set = FormSet {
    ///     guid: [0; 16],
    ///     title: String::new(),
    ///     help: String::new(),
    ///     class_guids: Vec::new(),
    ///     varstores: Vec::new(),
    ///     forms: Vec::new(),
    /// };
    /// assert!(set.varstore(VarStoreId(1)).is_none());
    /// ```
    pub fn varstore(&self, id: VarStoreId) -> Option<&VarStore> {
        self.varstores.iter().find(|store| store.id == id)
    }
}

/// One form: a page of statements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    /// The form identifier.
    pub id: FormId,
    /// The form title.
    pub title: String,
    /// The statements, in display order, with their conditions.
    pub items: Vec<Item>,
}

/// One statement and the conditions that hide or disable it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The statement.
    pub statement: Statement,
    /// The item is not shown while this expression is true (`SUPPRESS_IF`).
    pub suppress_if: Option<Expr>,
    /// The item is shown but cannot be selected while this is true (`GRAY_OUT_IF`).
    pub gray_out_if: Option<Expr>,
}

impl From<Statement> for Item {
    fn from(statement: Statement) -> Self {
        Self {
            statement,
            suppress_if: None,
            gray_out_if: None,
        }
    }
}

/// A statement of a form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    /// A heading line (`EFI_IFR_SUBTITLE`).
    Subtitle {
        /// The heading text.
        text: String,
    },
    /// A read-only prompt and value pair (`EFI_IFR_TEXT`).
    Text {
        /// The prompt.
        prompt: String,
        /// The help text.
        help: String,
        /// The value text (`TextTwo`); empty when absent.
        value: String,
    },
    /// A link to another form of the same formset (`EFI_IFR_REF`).
    Goto {
        /// The prompt.
        prompt: String,
        /// The help text.
        help: String,
        /// The target form.
        form: FormId,
        /// The question that the link carries, if it has callback semantics.
        question: Option<QuestionId>,
    },
    /// A question whose selection runs a provider action (`EFI_IFR_ACTION`).
    Action {
        /// The question.
        question: Question,
    },
    /// A boolean question (`EFI_IFR_CHECKBOX`).
    Checkbox {
        /// The question.
        question: Question,
        /// The default value.
        default: bool,
    },
    /// A choice among listed values (`EFI_IFR_ONE_OF` with `EFI_IFR_ONE_OF_OPTION`).
    OneOf {
        /// The question.
        question: Question,
        /// The stored width of the value.
        width: Width,
        /// The choices, in display order.
        choices: Vec<Choice>,
    },
    /// A bounded number (`EFI_IFR_NUMERIC`).
    Numeric {
        /// The question.
        question: Question,
        /// The stored width of the value.
        width: Width,
        /// The smallest accepted value.
        minimum: u64,
        /// The largest accepted value.
        maximum: u64,
        /// The increment; 0 means free entry.
        step: u64,
    },
    /// A text value (`EFI_IFR_STRING`).
    TextInput {
        /// The question.
        question: Question,
        /// The smallest accepted length in characters.
        minimum: u8,
        /// The largest accepted length in characters.
        maximum: u8,
    },
}

impl Statement {
    /// Returns the question of this statement, if it has one.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::Statement;
    ///
    /// assert!(Statement::Subtitle { text: "x".into() }.question().is_none());
    /// ```
    pub fn question(&self) -> Option<&Question> {
        match self {
            Self::Action { question }
            | Self::Checkbox { question, .. }
            | Self::OneOf { question, .. }
            | Self::Numeric { question, .. }
            | Self::TextInput { question, .. } => Some(question),
            Self::Subtitle { .. } | Self::Text { .. } | Self::Goto { .. } => None,
        }
    }
}

/// A question: the part of a statement that has an identity, a value and storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// The question identifier; unique in the formset.
    pub id: QuestionId,
    /// The prompt.
    pub prompt: String,
    /// The help text.
    pub help: String,
    /// Where the value is kept.
    pub storage: Storage,
    /// The question flags.
    pub flags: QuestionFlags,
}

/// Where a question value is kept (`EFI_IFR_QUESTION_HEADER.VarStoreId` and `VarStoreInfo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    /// The question has no stored value; the provider answers it through callbacks.
    None,
    /// A byte offset in a buffer or EFI variable store.
    Offset {
        /// The variable store.
        varstore: VarStoreId,
        /// The byte offset of the value.
        offset: u16,
    },
    /// A named value in a name/value store.
    Name {
        /// The variable store.
        varstore: VarStoreId,
        /// The value name.
        name: String,
    },
}

/// Question flags (`EFI_IFR_FLAG_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuestionFlags(pub u8);

impl QuestionFlags {
    /// The value cannot be changed (`EFI_IFR_FLAG_READ_ONLY`).
    pub const READ_ONLY: Self = Self(0x01);
    /// The provider receives callbacks for this question (`EFI_IFR_FLAG_CALLBACK`).
    pub const CALLBACK: Self = Self(0x04);
    /// A change takes effect after a reset (`EFI_IFR_FLAG_RESET_REQUIRED`).
    pub const RESET_REQUIRED: Self = Self(0x10);
    /// A change takes effect after a reconnect (`EFI_IFR_FLAG_RECONNECT_REQUIRED`).
    pub const RECONNECT_REQUIRED: Self = Self(0x40);

    /// Returns whether every flag of `other` is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::QuestionFlags;
    ///
    /// let flags = QuestionFlags(QuestionFlags::READ_ONLY.0 | QuestionFlags::CALLBACK.0);
    /// assert!(flags.contains(QuestionFlags::CALLBACK));
    /// assert!(!flags.contains(QuestionFlags::RESET_REQUIRED));
    /// ```
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The stored width of a numeric or choice value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    /// One byte.
    U8,
    /// Two bytes.
    U16,
    /// Four bytes.
    U32,
    /// Eight bytes.
    U64,
}

impl Width {
    /// Returns the width in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Width::U32.bytes(), 4);
    /// ```
    pub const fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 => 8,
        }
    }
}

/// One choice of a [`Statement::OneOf`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The choice text.
    pub text: String,
    /// The stored value.
    pub value: u64,
    /// This choice is the default (`EFI_IFR_OPTION_DEFAULT`).
    pub default: bool,
}

/// A variable store declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarStore {
    /// The identifier that questions use.
    pub id: VarStoreId,
    /// The store GUID; for an EFI store, the variable vendor GUID.
    pub guid: Guid,
    /// The store name; for an EFI store, the variable name.
    pub name: String,
    /// The store size in bytes; 0 for a name/value store.
    pub size: u16,
    /// The store kind.
    pub kind: VarStoreKind,
}

/// The kind of a variable store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarStoreKind {
    /// A buffer that the provider routes (`EFI_IFR_VARSTORE`).
    Buffer,
    /// An EFI variable (`EFI_IFR_VARSTORE_EFI`) with these attributes.
    Efi {
        /// The variable attributes.
        attributes: u32,
    },
    /// Named values (`EFI_IFR_VARSTORE_NAME_VALUE`).
    NameValue,
}

/// A question value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A checkbox value.
    Bool(bool),
    /// A numeric or choice value.
    Number(u64),
    /// A text value.
    Text(String),
    /// Raw bytes of a question that the model does not type.
    Bytes(Vec<u8>),
}

/// A condition expression (IFR expression opcodes), limited to the forms that
/// compare question values.
///
/// [`Expr::Unsupported`] stands for an expression that a decoder could not
/// represent. [`Session`] evaluates it as false, so the item stays visible and
/// selectable, and the item's [`Row`] has `uncertain` set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// Always true (`EFI_IFR_TRUE`).
    True,
    /// Always false (`EFI_IFR_FALSE`).
    False,
    /// Logical negation (`EFI_IFR_NOT`).
    Not(Box<Expr>),
    /// Logical conjunction (`EFI_IFR_AND`).
    And(Box<Expr>, Box<Expr>),
    /// Logical disjunction (`EFI_IFR_OR`).
    Or(Box<Expr>, Box<Expr>),
    /// A question equals a constant (`EFI_IFR_EQ_ID_VAL`).
    EqualValue {
        /// The question.
        question: QuestionId,
        /// The constant.
        value: u64,
    },
    /// A question equals one of the constants (`EFI_IFR_EQ_ID_VAL_LIST`).
    EqualList {
        /// The question.
        question: QuestionId,
        /// The constants.
        values: Vec<u64>,
    },
    /// Two questions are equal (`EFI_IFR_EQ_ID_ID`).
    EqualQuestion {
        /// The first question.
        first: QuestionId,
        /// The second question.
        second: QuestionId,
    },
    /// An expression the decoder could not represent.
    Unsupported,
}

/// What a provider asks the front end to do after a change or an action
/// (`EFI_BROWSER_ACTION_REQUEST_*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    /// Nothing further.
    None,
    /// Read the values of the current form again.
    Refresh,
    /// Leave the formset.
    Exit,
    /// A reset is needed for the change to take effect.
    Reset,
    /// Show this message to the person.
    Message(String),
}

/// A provider failure: the firmware or service status and a short reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The EFI status or service error code.
    pub code: u64,
    /// A short reason.
    pub reason: String,
}

/// Reads and changes question values, and runs actions.
///
/// A provider owns the formset list and the storage behind it. The front end
/// owns no value; it reads a value before it shows it and asks the provider
/// for every change. A UEFI implementation uses `EFI_HII_CONFIG_ROUTING_PROTOCOL`
/// and `EFI_HII_CONFIG_ACCESS_PROTOCOL`; another implementation may use any
/// store with the same semantics.
pub trait Provider {
    /// The provider's identity for one published formset (for example an HII handle).
    type Handle: Copy + Eq;

    /// Lists the published formsets.
    ///
    /// # Errors
    ///
    /// Returns the provider failure when the list cannot be read.
    fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error>;

    /// Reads the current value of a question.
    ///
    /// # Errors
    ///
    /// Returns the provider failure when the value cannot be read.
    fn read(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        question: &Question,
    ) -> Result<Value, Error>;

    /// Changes the value of a question and returns the provider's request.
    ///
    /// # Errors
    ///
    /// Returns the provider failure when the provider refuses or cannot store the value.
    fn write(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        question: &Question,
        value: &Value,
    ) -> Result<Response, Error>;

    /// Runs the action of an action question and returns the provider's request.
    ///
    /// # Errors
    ///
    /// Returns the provider failure when the action fails.
    fn action(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        question: &Question,
    ) -> Result<Response, Error>;
}
