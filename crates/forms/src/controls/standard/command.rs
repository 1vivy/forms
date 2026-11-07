//! Rows that run something: a link, an action and a reset.

use super::super::{EditState, EditStep, Field, Handler};
use crate::session::Input;
use alloc::string::String;

/// A link row.
pub(crate) struct LinkRow;

impl Handler for LinkRow {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn edit(&self, _field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        if input == Input::Enter {
            EditStep::Invoke
        } else {
            EditStep::Ignored
        }
    }
}

/// An action row.
pub(crate) struct ActionRow;

impl Handler for ActionRow {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn edit(&self, _field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        if input == Input::Enter {
            EditStep::Invoke
        } else {
            EditStep::Ignored
        }
    }
}

/// A reset command.
pub(crate) struct Reset;

impl Handler for Reset {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn edit(&self, _field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        if input == Input::Enter {
            EditStep::Invoke
        } else {
            EditStep::Ignored
        }
    }
}
