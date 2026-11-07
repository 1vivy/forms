//! A logical switch.

use super::super::{Decoration, EditState, EditStep, Field, Handler};
use crate::model::Value;
use crate::session::Input;
use alloc::string::String;

/// A logical switch.
pub(crate) struct Toggle;

impl Handler for Toggle {
    fn format(&self, field: &Field<'_>) -> String {
        let on = matches!(field.value, Some(Value::Bool(true)));
        if on {
            String::from("On")
        } else {
            String::from("Off")
        }
    }

    fn decoration(&self, field: &Field<'_>) -> Decoration {
        Decoration::Toggle(matches!(field.value, Some(Value::Bool(true))))
    }

    fn edit(&self, field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        let on = matches!(field.value, Some(Value::Bool(true)));
        match input {
            Input::Enter | Input::Char(' ') | Input::Left | Input::Right => {
                EditStep::Accept(Value::Bool(!on))
            }
            _ => EditStep::Ignored,
        }
    }
}
