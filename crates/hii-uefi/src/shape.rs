//! Question shapes: stored widths and HII value types.
//!
//! The HII protocols need the width and the `EFI_IFR_TYPE_*` code of a
//! question before they can extract or store its value. The forms model keeps
//! both in the statement, so this module maps a statement to the values that
//! the firmware expects. It is pure, so the mapping is testable on the host.

use forms::{FormSet, QuestionId, Statement, Width};
use hii_abi::value_type;

/// Returns the statement that answers this question identifier.
///
/// # Examples
///
/// ```
/// use forms::{Form, FormId, FormSet, Item, QuestionFlags, Statement};
/// use hii_uefi::shape::statement_of;
///
/// let set = FormSet {
///     guid: [0; 16],
///     title: "Platform".into(),
///     help: String::new(),
///     class_guids: Vec::new(),
///     varstores: Vec::new(),
///     forms: vec![Form {
///         id: FormId(1),
///         title: "Platform".into(),
///         items: vec![Statement::Subtitle { text: "State".into() }.into()],
///     }],
/// };
/// assert!(statement_of(&set, forms::QuestionId(1)).is_none());
/// ```
pub fn statement_of(set: &FormSet, id: QuestionId) -> Option<&Statement> {
    set.forms
        .iter()
        .flat_map(|form| form.items.iter())
        .find_map(|item| match item.statement.question() {
            Some(question) if question.id == id => Some(&item.statement),
            _ => None,
        })
}

/// Returns the stored width of a question statement, in bytes.
///
/// A string is UTF-16LE, zero-padded to its maximum length, so its width is
/// twice that maximum. The codec writes no terminating NUL when the text fills
/// the maximum.
///
/// # Examples
///
/// ```
/// use forms::{Question, QuestionFlags, Statement, Storage};
/// use hii_uefi::shape::width_of;
///
/// let question = Question {
///     id: forms::QuestionId(1),
///     prompt: "Enabled".into(),
///     help: String::new(),
///     storage: Storage::None,
///     flags: QuestionFlags::default(),
/// };
/// let checkbox = Statement::Checkbox { question, default: false };
/// assert_eq!(width_of(&checkbox), Some(1));
/// ```
pub fn width_of(statement: &Statement) -> Option<u16> {
    match statement {
        Statement::Checkbox { .. } => Some(1),
        Statement::OneOf { width, .. } | Statement::Numeric { width, .. } => {
            Some(width.bytes() as u16)
        }
        Statement::TextInput { maximum, .. } => Some(u16::from(*maximum) * 2),
        Statement::Subtitle { .. }
        | Statement::Text { .. }
        | Statement::Goto { .. }
        | Statement::Action { .. } => None,
    }
}

/// Returns the `EFI_IFR_TYPE_*` code of a question statement.
///
/// # Examples
///
/// ```
/// use forms::{Question, QuestionFlags, Statement, Storage, Width};
/// use hii_abi::value_type;
/// use hii_uefi::shape::ifr_type_of;
///
/// let question = Question {
///     id: forms::QuestionId(1),
///     prompt: "Level".into(),
///     help: String::new(),
///     storage: Storage::None,
///     flags: QuestionFlags::default(),
/// };
/// let numeric = Statement::Numeric { question, width: Width::U16, minimum: 0, maximum: 9, step: 1 };
/// assert_eq!(ifr_type_of(&numeric), Some(value_type::NUM_SIZE_16));
/// ```
pub fn ifr_type_of(statement: &Statement) -> Option<u8> {
    match statement {
        Statement::Checkbox { .. } => Some(value_type::BOOLEAN),
        Statement::OneOf { width, .. } | Statement::Numeric { width, .. } => Some(match width {
            Width::U8 => value_type::NUM_SIZE_8,
            Width::U16 => value_type::NUM_SIZE_16,
            Width::U32 => value_type::NUM_SIZE_32,
            Width::U64 => value_type::NUM_SIZE_64,
        }),
        Statement::TextInput { .. } => Some(value_type::STRING),
        Statement::Action { .. } => Some(value_type::ACTION),
        Statement::Subtitle { .. } | Statement::Text { .. } | Statement::Goto { .. } => None,
    }
}
