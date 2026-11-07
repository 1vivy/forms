//! A one-of choice: a cursor over the options, written on Enter.

use super::super::builtin::{choice_label, number};
use super::super::{EditState, EditStep, Field, Handler};
use crate::model::{Choice, Control, Value};
use crate::session::{Editor, EditorKind, Input};
use alloc::string::String;

/// One option of a choice list.
pub(crate) struct ChoiceOptions;

impl ChoiceOptions {
    /// Returns the options of a row.
    fn options<'a>(field: &'a Field<'_>) -> &'a [Choice] {
        match &field.row.control {
            Control::Choice(choices) => choices,
            _ => &[],
        }
    }

    /// Returns the index of the current value.
    fn index(field: &Field<'_>) -> usize {
        let options = Self::options(field);
        let value = number(field.value);
        options
            .iter()
            .position(|choice| choice.value == value)
            .unwrap_or(0)
    }
}

impl Handler for ChoiceOptions {
    fn format(&self, field: &Field<'_>) -> String {
        choice_label(Self::options(field), number(field.value))
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        EditState {
            selected: Self::index(field),
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        let items = Self::options(field)
            .iter()
            .map(|choice| choice.label.clone())
            .collect();
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::Choice {
                items,
                selected: state.selected,
            },
        })
    }

    /// Up and Down move the cursor through the options; Enter accepts the
    /// option under the cursor. A device with only volume keys and power can
    /// then look at every option before it writes one.
    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let options = Self::options(field);
        if options.is_empty() {
            return EditStep::Ignored;
        }
        let last = options.len() - 1;
        let index = state.selected.min(last);
        state.selected = match input {
            Input::Down | Input::Right => {
                if index == last {
                    0
                } else {
                    index + 1
                }
            }
            Input::Up | Input::Left => {
                if index == 0 {
                    last
                } else {
                    index - 1
                }
            }
            Input::Home => 0,
            Input::Enter => return EditStep::Accept(Value::Number(options[index].value)),
            _ => return EditStep::Ignored,
        };
        EditStep::Changed
    }
}
