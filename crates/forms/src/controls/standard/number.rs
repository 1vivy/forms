//! A bounded number typed or stepped with Up and Down.

use super::super::builtin::number;
use super::super::{EditState, EditStep, Field, Handler};
use crate::model::{Control, Value};
use crate::session::{Editor, EditorKind, Input};
use alloc::{
    format,
    string::{String, ToString},
};

/// A bounded number.
pub(crate) struct Number;

impl Handler for Number {
    fn format(&self, field: &Field<'_>) -> String {
        format!("{}", number(field.value))
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        let text = format!("{}", number(field.value));
        EditState {
            cursor: text.chars().count(),
            text,
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::Number {
                text: state.text.clone(),
                cursor: state.cursor,
            },
        })
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (minimum, maximum, step) = match &field.row.control {
            Control::Number {
                minimum,
                maximum,
                step,
            } => (*minimum, *maximum, *step),
            _ => (0, u64::MAX, 0),
        };
        match input {
            Input::Char(character) if character.is_ascii_digit() => {
                if state.text.chars().count() >= 20 {
                    return EditStep::Ignored;
                }
                state.text.insert(state.cursor, character);
                state.cursor += 1;
                EditStep::Changed
            }
            Input::Erase => {
                if state.cursor == 0 {
                    return EditStep::Ignored;
                }
                state.cursor -= 1;
                state.text.remove(state.cursor);
                EditStep::Changed
            }
            // Up and Down step the value, so a device without a keyboard can
            // set a number; Left and Right move the cursor.
            Input::Up | Input::Down => {
                let current = state.text.parse::<u64>().unwrap_or(minimum);
                let step = step.max(1);
                let next = if input == Input::Up {
                    current.saturating_add(step).min(maximum)
                } else {
                    current.saturating_sub(step).max(minimum)
                };
                state.text = next.to_string();
                state.cursor = state.text.chars().count();
                EditStep::Changed
            }
            Input::Left => {
                state.cursor = state.cursor.saturating_sub(1);
                EditStep::Changed
            }
            Input::Right => {
                state.cursor = state
                    .cursor
                    .saturating_add(1)
                    .min(state.text.chars().count());
                EditStep::Changed
            }
            Input::Enter => {
                let Ok(value) = state.text.parse::<u64>() else {
                    return EditStep::Refused(String::from("Enter a number"));
                };
                if value < minimum || value > maximum {
                    return EditStep::Refused(format!("Enter a value from {minimum} to {maximum}"));
                }
                if step != 0 && (value - minimum) % step != 0 {
                    return EditStep::Refused(format!(
                        "Enter a value that is a multiple of {step} from {minimum}"
                    ));
                }
                EditStep::Accept(Value::Number(value))
            }
            _ => EditStep::Ignored,
        }
    }
}
