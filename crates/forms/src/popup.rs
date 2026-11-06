//! Firmware-independent modal decisions.

use crate::Input;
use alloc::string::String;

/// The importance of a popup message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupStyle {
    /// Information about an operation.
    Info,
    /// An operation needs attention.
    Warning,
    /// An operation failed.
    Error,
}

/// The available answers, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupType {
    /// Acknowledge a message.
    Ok,
    /// Accept or cancel an operation.
    OkCancel,
    /// Answer a question.
    YesNo,
    /// Answer a question or cancel it.
    YesNoCancel,
}

/// An explicit answer to a popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupSelection {
    /// Acknowledge or accept.
    Ok,
    /// Cancel without acceptance.
    Cancel,
    /// Affirm the question.
    Yes,
    /// Reject the question.
    No,
}

impl PopupType {
    /// Returns the legal answers in display order.
    ///
    /// # Examples
    /// ```
    /// assert_eq!(forms::PopupType::Ok.selections(), &[forms::PopupSelection::Ok]);
    /// ```
    pub fn selections(self) -> &'static [PopupSelection] {
        use PopupSelection::{Cancel, No, Ok, Yes};
        match self {
            Self::Ok => &[Ok],
            Self::OkCancel => &[Ok, Cancel],
            Self::YesNo => &[Yes, No],
            Self::YesNoCancel => &[Yes, No, Cancel],
        }
    }
}

impl PopupSelection {
    /// Returns the button label.
    ///
    /// # Examples
    /// ```
    /// assert_eq!(forms::PopupSelection::Cancel.label(), "Cancel");
    /// ```
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Cancel => "Cancel",
            Self::Yes => "Yes",
            Self::No => "No",
        }
    }
}

/// A modal message and its highlighted answer.
///
/// The first answer is selected initially, as in the EDK II display engine.
/// Left and Right wrap. Up and Down do the same for three-key front ends.
/// Enter accepts the highlighted answer. Letter keys select a legal answer.
/// Back cancels only a popup that offers Cancel; it never implies consent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Popup {
    /// The message importance.
    pub style: PopupStyle,
    /// The set of legal answers.
    pub kind: PopupType,
    /// The message text.
    pub message: String,
    /// The highlighted answer.
    pub selected: PopupSelection,
}

impl Popup {
    /// Creates a popup with its first answer selected.
    ///
    /// # Examples
    /// ```
    /// use forms::{Popup, PopupStyle, PopupType, PopupSelection};
    /// let popup = Popup::new(PopupStyle::Warning, PopupType::YesNo, "Continue?");
    /// assert_eq!(popup.selected, PopupSelection::Yes);
    /// ```
    pub fn new(style: PopupStyle, kind: PopupType, message: impl Into<String>) -> Self {
        Self {
            style,
            kind,
            message: message.into(),
            selected: kind.selections()[0],
        }
    }

    /// Handles a key and returns an answer only when the user selects one.
    ///
    /// # Examples
    /// ```
    /// use forms::{Input, Popup, PopupStyle, PopupType, PopupSelection};
    /// let mut popup = Popup::new(PopupStyle::Info, PopupType::OkCancel, "Apply?");
    /// popup.input(Input::Right);
    /// assert_eq!(popup.input(Input::Enter), Some(PopupSelection::Cancel));
    /// ```
    pub fn input(&mut self, input: Input) -> Option<PopupSelection> {
        let choices = self.kind.selections();
        let index = choices
            .iter()
            .position(|choice| *choice == self.selected)
            .unwrap_or(0);
        self.selected = choices[index];
        match input {
            Input::Up | Input::Left => {
                self.selected = choices[(index + choices.len() - 1) % choices.len()]
            }
            Input::Down | Input::Right => self.selected = choices[(index + 1) % choices.len()],
            Input::Home => self.selected = choices[0],
            Input::Enter => return Some(self.selected),
            Input::Back if choices.contains(&PopupSelection::Cancel) => {
                return Some(PopupSelection::Cancel);
            }
            Input::Char(ch) => {
                let answer = match ch.to_ascii_lowercase() {
                    'o' => PopupSelection::Ok,
                    'c' => PopupSelection::Cancel,
                    'y' => PopupSelection::Yes,
                    'n' => PopupSelection::No,
                    _ => return None,
                };
                if choices.contains(&answer) {
                    return Some(answer);
                }
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_navigates_and_accepts_only_legal_answers() {
        for kind in [
            PopupType::Ok,
            PopupType::OkCancel,
            PopupType::YesNo,
            PopupType::YesNoCancel,
        ] {
            let mut popup = Popup::new(PopupStyle::Info, kind, "Question");
            for answer in kind.selections() {
                assert_eq!(popup.input(Input::Enter), Some(*answer));
                assert_eq!(popup.input(Input::Right), None);
            }
            assert_eq!(popup.selected, kind.selections()[0]);
            popup.input(Input::Up);
            assert_eq!(popup.selected, *kind.selections().last().unwrap());
            popup.input(Input::Down);
            assert_eq!(popup.selected, kind.selections()[0]);
            for (key, answer) in [
                ('o', PopupSelection::Ok),
                ('C', PopupSelection::Cancel),
                ('y', PopupSelection::Yes),
                ('N', PopupSelection::No),
            ] {
                assert_eq!(
                    popup.input(Input::Char(key)),
                    kind.selections().contains(&answer).then_some(answer)
                );
            }
            assert_eq!(
                popup.input(Input::Back),
                kind.selections()
                    .contains(&PopupSelection::Cancel)
                    .then_some(PopupSelection::Cancel)
            );
            assert_eq!(popup.input(Input::Char('x')), None);
        }
    }
}
