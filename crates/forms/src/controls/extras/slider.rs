//! A bounded number that two keys adjust, with a unit and labels for special values.

use super::super::builtin::number;
use super::super::{Decoration, EditState, EditStep, Field, Handler};
use crate::model::{Choice, Control, Value};
use crate::session::{Editor, EditorKind, Input};
use alloc::{format, string::String, vec::Vec};

/// A bounded number that two keys adjust.
pub(crate) struct Slider;

impl Slider {
    /// Returns the limits, the step, the unit and the labels of a row.
    fn parts(field: &Field<'_>) -> (u64, u64, u64, String, Vec<Choice>) {
        match &field.row.control {
            Control::Slider {
                minimum,
                maximum,
                step,
                unit,
                labels,
            } => (
                *minimum,
                *maximum,
                (*step).max(1),
                unit.clone(),
                labels.clone(),
            ),
            _ => (0, u64::MAX, 1, String::new(), Vec::new()),
        }
    }

    /// Returns the displayed text of one value.
    fn text(value: u64, unit: &str, labels: &[Choice]) -> String {
        if let Some(choice) = labels.iter().find(|choice| choice.value == value) {
            return choice.label.clone();
        }
        if unit.is_empty() {
            format!("{value}")
        } else {
            format!("{value} {unit}")
        }
    }
}

impl Handler for Slider {
    fn format(&self, field: &Field<'_>) -> String {
        let (_, _, _, unit, labels) = Self::parts(field);
        Self::text(number(field.value), &unit, &labels)
    }

    fn decoration(&self, field: &Field<'_>) -> Decoration {
        let (minimum, maximum, _, _, _) = Self::parts(field);
        let position = number(field.value).clamp(minimum, maximum);
        Decoration::Slider {
            position,
            minimum,
            maximum,
        }
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        EditState {
            value: number(field.value),
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        let (minimum, maximum, _, unit, labels) = Self::parts(field);
        let position = state.value.clamp(minimum, maximum);
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::Slider {
                text: Self::text(position, &unit, &labels),
                position,
                minimum,
                maximum,
            },
        })
    }

    fn format_editing(&self, field: &Field<'_>, state: &EditState) -> Option<(String, Decoration)> {
        let (minimum, maximum, _, unit, labels) = Self::parts(field);
        let position = state.value.clamp(minimum, maximum);
        Some((
            Self::text(position, &unit, &labels),
            Decoration::Slider {
                position,
                minimum,
                maximum,
            },
        ))
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (minimum, maximum, step, _, _) = Self::parts(field);
        match input {
            Input::Up | Input::Right => {
                state.value = state.value.saturating_add(step).min(maximum);
                EditStep::Changed
            }
            Input::Down | Input::Left => {
                state.value = state.value.saturating_sub(step).max(minimum);
                EditStep::Changed
            }
            Input::Home => {
                state.value = minimum;
                EditStep::Changed
            }
            Input::End => {
                state.value = maximum;
                EditStep::Changed
            }
            Input::Enter => EditStep::Accept(Value::Number(state.value.clamp(minimum, maximum))),
            _ => EditStep::Ignored,
        }
    }
}
