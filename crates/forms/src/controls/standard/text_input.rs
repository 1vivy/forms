//! A text or password input with full keyboard editing.

use super::super::builtin::text;
use super::super::{EditState, EditStep, Field, Handler};
use crate::model::{Control, Value};
use crate::session::{Editor, EditorKind, Input};
use alloc::{
    format,
    string::{String, ToString},
};

/// A text or password value with full keyboard editing.
pub(crate) struct TextInput {
    /// Conceal the text.
    pub(crate) secret: bool,
}

impl TextInput {
    /// Returns the length limits of a row.
    fn limits(field: &Field<'_>) -> (usize, usize) {
        match &field.row.control {
            Control::TextInput {
                minimum, maximum, ..
            } => (*minimum, *maximum),
            _ => (0, usize::MAX),
        }
    }
}

impl Handler for TextInput {
    fn format(&self, field: &Field<'_>) -> String {
        let text = text(field.value);
        if self.secret {
            "*".repeat(text.chars().count())
        } else {
            text.to_string()
        }
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        let text = text(field.value).to_string();
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
            kind: EditorKind::Text {
                text: state.text.clone(),
                cursor: state.cursor,
                secret: self.secret,
            },
        })
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (minimum, maximum) = Self::limits(field);
        let length = state.text.chars().count();
        match input {
            Input::Char(character) => {
                if length >= maximum {
                    return EditStep::Refused(format!(
                        "The text accepts at most {maximum} characters"
                    ));
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
            Input::Left => {
                state.cursor = state.cursor.saturating_sub(1);
                EditStep::Changed
            }
            Input::Right => {
                state.cursor = state.cursor.saturating_add(1).min(length);
                EditStep::Changed
            }
            Input::Home => {
                state.cursor = 0;
                EditStep::Changed
            }
            Input::End => {
                state.cursor = length;
                EditStep::Changed
            }
            Input::Enter => {
                if length < minimum {
                    return EditStep::Refused(format!(
                        "The text needs at least {minimum} characters"
                    ));
                }
                EditStep::Accept(Value::Text(state.text.clone()))
            }
            _ => EditStep::Ignored,
        }
    }
}
