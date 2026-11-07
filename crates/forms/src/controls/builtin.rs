//! The registration of the built-in control kinds and the value readers that
//! their handlers share.
//!
//! The handlers live in [`super::standard`] (the controls a HII form states)
//! and [`super::extras`] (the controls only this crate defines).

use alloc::{boxed::Box, format, string::String, vec::Vec};

use crate::model::{Choice, Control, Key, Value};

use super::extras::{Divider, Progress, Slider};
use super::standard::{
    ActionRow, ChoiceOptions, Heading, LinkRow, Number, OrderedList, Reset, Text, TextInput,
    Toggle, Unsupported,
};
use super::{Handler, kind};

/// Returns the kind of a control.
///
/// # Examples
///
/// ```
/// use forms::{Control, controls};
///
/// assert_eq!(controls::kind_of(&Control::Toggle), controls::kind::TOGGLE);
/// assert_eq!(
///     controls::kind_of(&Control::Custom { kind: forms::Key(5), data: Vec::new() }),
///     forms::Key(5)
/// );
/// ```
pub fn kind_of(control: &Control) -> Key {
    match control {
        Control::Heading => kind::HEADING,
        Control::Text(_) => kind::TEXT,
        Control::Link(_) => kind::LINK,
        Control::Action => kind::ACTION,
        Control::Toggle => kind::TOGGLE,
        Control::Choice(_) => kind::CHOICE,
        Control::Number { .. } => kind::NUMBER,
        Control::Slider { .. } => kind::SLIDER,
        Control::TextInput { secret: false, .. } => kind::TEXT_INPUT,
        Control::TextInput { secret: true, .. } => kind::PASSWORD,
        Control::OrderedList { .. } => kind::ORDERED_LIST,
        Control::Reset(_) => kind::RESET,
        Control::Progress { .. } => kind::PROGRESS,
        Control::Divider => kind::DIVIDER,
        Control::Unsupported(_) => kind::UNSUPPORTED,
        Control::Custom { kind, .. } => *kind,
    }
}

/// The kinds that [`super::Registry::builtin`] registers.
pub const BUILTIN_KINDS: [Key; 15] = [
    kind::HEADING,
    kind::TEXT,
    kind::LINK,
    kind::ACTION,
    kind::TOGGLE,
    kind::CHOICE,
    kind::NUMBER,
    kind::SLIDER,
    kind::TEXT_INPUT,
    kind::PASSWORD,
    kind::ORDERED_LIST,
    kind::RESET,
    kind::PROGRESS,
    kind::DIVIDER,
    kind::UNSUPPORTED,
];

/// Returns the handler of each built-in kind.
pub fn handlers() -> Vec<(Key, Box<dyn Handler>)> {
    alloc::vec![
        (kind::HEADING, Box::new(Heading) as Box<dyn Handler>),
        (kind::TEXT, Box::new(Text)),
        (kind::LINK, Box::new(LinkRow)),
        (kind::ACTION, Box::new(ActionRow)),
        (kind::TOGGLE, Box::new(Toggle)),
        (kind::CHOICE, Box::new(ChoiceOptions)),
        (kind::NUMBER, Box::new(Number)),
        (kind::SLIDER, Box::new(Slider)),
        (kind::TEXT_INPUT, Box::new(TextInput { secret: false })),
        (kind::PASSWORD, Box::new(TextInput { secret: true })),
        (kind::ORDERED_LIST, Box::new(OrderedList)),
        (kind::RESET, Box::new(Reset)),
        (kind::PROGRESS, Box::new(Progress)),
        (kind::DIVIDER, Box::new(Divider)),
        (kind::UNSUPPORTED, Box::new(Unsupported)),
    ]
}

/// Returns the text of a value, or an empty text.
pub(super) fn text(value: Option<&Value>) -> &str {
    match value {
        Some(Value::Text(text)) => text,
        _ => "",
    }
}

/// Returns the number of a value, or 0.
pub(super) fn number(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => *number,
        _ => 0,
    }
}

/// Returns the list of a value, or an empty list.
pub(super) fn list(value: Option<&Value>) -> Vec<u64> {
    match value {
        Some(Value::List(values)) => values.clone(),
        _ => Vec::new(),
    }
}

/// Returns the label of one choice value.
pub(super) fn choice_label(choices: &[Choice], value: u64) -> String {
    choices
        .iter()
        .find(|choice| choice.value == value)
        .map_or_else(|| format!("#{value}"), |choice| choice.label.clone())
}
