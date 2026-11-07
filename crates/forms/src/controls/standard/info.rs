//! Rows that only show: a heading, a text and the inert row of an unknown opcode.

use super::super::builtin::text;
use super::super::{Decoration, Field, Handler};
use crate::model::Control;
use alloc::string::{String, ToString};

/// A heading.
pub(crate) struct Heading;

impl Handler for Heading {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn decoration(&self, _field: &Field<'_>) -> Decoration {
        Decoration::Heading
    }
}

/// An information row.
pub(crate) struct Text;

impl Handler for Text {
    fn format(&self, field: &Field<'_>) -> String {
        let live = text(field.value);
        if live.is_empty() {
            match &field.row.control {
                Control::Text(text) => text.clone(),
                _ => String::new(),
            }
        } else {
            live.to_string()
        }
    }
}

/// An imported control that the crate cannot show.
pub(crate) struct Unsupported;

impl Handler for Unsupported {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }
}
