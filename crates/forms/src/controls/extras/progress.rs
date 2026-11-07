//! A progress or busy row.

use super::super::{Decoration, Field, Handler};
use crate::model::Control;
use alloc::{format, string::String};

/// A progress or busy row.
pub(crate) struct Progress;

impl Handler for Progress {
    fn format(&self, field: &Field<'_>) -> String {
        match &field.row.control {
            Control::Progress { current, total } => match total {
                Some(total) if *total > 0 => {
                    format!("{}/{}", current.min(total), total)
                }
                _ => String::from("Working"),
            },
            _ => String::new(),
        }
    }

    fn decoration(&self, field: &Field<'_>) -> Decoration {
        match &field.row.control {
            Control::Progress { current, total } => Decoration::Bar {
                current: *current,
                total: *total,
            },
            _ => Decoration::Plain,
        }
    }
}
