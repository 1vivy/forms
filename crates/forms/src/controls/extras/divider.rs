//! A divider row: a rule between groups of rows.

use super::super::{Decoration, Field, Handler};
use alloc::string::String;

/// A divider.
pub(crate) struct Divider;

impl Handler for Divider {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn decoration(&self, _field: &Field<'_>) -> Decoration {
        Decoration::Rule
    }
}
