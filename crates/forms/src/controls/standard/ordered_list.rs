//! An ordered list that a person reorders with three keys and selects from.

use super::super::builtin::{choice_label, list};
use super::super::{EditState, EditStep, Field, Handler};
use crate::model::{Choice, Control, Value};
use crate::session::{Editor, EditorKind, Input};
use alloc::{format, string::String, vec::Vec};

/// The label of the closing entry of an ordered list editor.
const DONE: &str = "Done";

/// An ordered list that a person reorders and selects from.
pub(crate) struct OrderedList;

impl OrderedList {
    /// Returns the options and the limit of a row.
    fn options<'a>(field: &'a Field<'_>) -> (&'a [Choice], usize) {
        match &field.row.control {
            Control::OrderedList { choices, maximum } => {
                let maximum = if *maximum == 0 {
                    choices.len()
                } else {
                    *maximum
                };
                (choices, maximum)
            }
            _ => (&[], 0),
        }
    }

    /// Returns the chosen values and the available option values.
    fn split(choices: &[Choice], chosen: &[u64]) -> (Vec<u64>, Vec<u64>) {
        let available: Vec<u64> = choices
            .iter()
            .map(|choice| choice.value)
            .filter(|value| !chosen.contains(value))
            .collect();
        (chosen.to_vec(), available)
    }

    /// Returns the number of entries, including the closing entry.
    fn entries(choices: &[Choice], chosen: &[u64]) -> usize {
        let (chosen, available) = Self::split(choices, chosen);
        chosen.len() + available.len() + 1
    }
}

impl Handler for OrderedList {
    fn format(&self, field: &Field<'_>) -> String {
        let (choices, _) = Self::options(field);
        let chosen = list(field.value);
        chosen
            .iter()
            .map(|value| choice_label(choices, *value))
            .collect::<Vec<String>>()
            .join(", ")
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        EditState {
            values: list(field.value),
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        let (choices, _) = Self::options(field);
        let (chosen, available) = Self::split(choices, &state.values);
        let mut items: Vec<String> = chosen
            .iter()
            .map(|value| choice_label(choices, *value))
            .collect();
        items.extend(available.iter().map(|value| choice_label(choices, *value)));
        items.push(String::from(DONE));
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::List {
                items,
                selected: state.selected.min(chosen.len() + available.len()),
                chosen: chosen.len(),
                held: state.held && state.selected < chosen.len(),
            },
        })
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (choices, maximum) = Self::options(field);
        let total = Self::entries(choices, &state.values);
        let closing = total - 1;
        if state.selected > closing {
            state.selected = closing;
        }
        let (chosen, available) = Self::split(choices, &state.values);
        // A device with only volume keys and power works the list with Up,
        // Down and Enter: Enter picks a chosen entry up, Up and Down move it,
        // Enter drops it, and Down past the last chosen entry removes it.
        // Left and Right stay as keyboard shortcuts for one move.
        if state.held && state.selected < chosen.len() {
            return match input {
                Input::Up | Input::Left => {
                    if state.selected > 0 {
                        state.values.swap(state.selected, state.selected - 1);
                        state.selected -= 1;
                    }
                    EditStep::Changed
                }
                Input::Down | Input::Right => {
                    if state.selected + 1 < chosen.len() {
                        state.values.swap(state.selected, state.selected + 1);
                        state.selected += 1;
                    } else if input == Input::Down {
                        state.values.remove(state.selected);
                        state.held = false;
                        state.selected = state.values.len();
                    }
                    EditStep::Changed
                }
                Input::Enter => {
                    state.held = false;
                    EditStep::Changed
                }
                _ => EditStep::Ignored,
            };
        }
        state.held = false;
        match input {
            Input::Up => {
                state.selected = if state.selected == 0 {
                    closing
                } else {
                    state.selected - 1
                };
                EditStep::Changed
            }
            Input::Down => {
                state.selected = if state.selected == closing {
                    0
                } else {
                    state.selected + 1
                };
                EditStep::Changed
            }
            Input::Left => {
                if state.selected == 0 || state.selected >= chosen.len() {
                    return EditStep::Ignored;
                }
                state.values.swap(state.selected, state.selected - 1);
                state.selected -= 1;
                EditStep::Changed
            }
            Input::Right => {
                if state.selected + 1 >= chosen.len() {
                    return EditStep::Ignored;
                }
                state.values.swap(state.selected, state.selected + 1);
                state.selected += 1;
                EditStep::Changed
            }
            Input::Enter if state.selected < chosen.len() => {
                state.held = true;
                EditStep::Changed
            }
            Input::Enter if state.selected < closing => {
                if state.values.len() >= maximum {
                    return EditStep::Refused(format!("Select at most {maximum} values"));
                }
                let index = state.selected - chosen.len();
                if let Some(value) = available.get(index) {
                    state.values.push(*value);
                    state.selected = state.values.len() - 1;
                }
                EditStep::Changed
            }
            Input::Enter => EditStep::Accept(Value::List(state.values.clone())),
            _ => EditStep::Ignored,
        }
    }
}
