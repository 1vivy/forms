//! A general modal decision over a page.

use alloc::{string::String, vec::Vec};

use crate::session::Input;

/// How much a modal message demands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalStyle {
    /// Information about an operation.
    Info,
    /// An operation needs attention.
    Warning,
    /// An operation is dangerous and needs confirmation.
    Danger,
    /// An operation failed.
    Error,
}

/// The meaning of one button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonRole {
    /// The primary answer.
    Default,
    /// The safe answer. A modal selects it first.
    Cancel,
    /// A consumer-defined answer.
    Other(u64),
}

/// One button of a modal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    /// The label of the button.
    pub label: String,
    /// The meaning of the button.
    pub role: ButtonRole,
}

impl Button {
    /// Creates a button.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Button, ButtonRole};
    ///
    /// assert_eq!(Button::new("Cancel", ButtonRole::Cancel).label, "Cancel");
    /// ```
    pub fn new(label: impl Into<String>, role: ButtonRole) -> Self {
        Self {
            label: label.into(),
            role,
        }
    }
}

/// A determinate or indeterminate progress state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Completed units.
    pub current: u64,
    /// Total units, or `None` while the work has no measure.
    pub total: Option<u64>,
}

/// The result of one modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalOutcome {
    /// A button was chosen.
    Chosen {
        /// The index of the button.
        index: usize,
        /// The role of the button.
        role: ButtonRole,
    },
    /// The person dismissed a modal that offers Cancel, or the modal closed.
    Closed,
}

/// A modal message with its own buttons.
///
/// The modal selects the Cancel button first, so one Enter refuses the
/// question. A modal without a Cancel button selects its default button. Back
/// closes a modal that offers Cancel and does nothing otherwise.
///
/// # Examples
///
/// ```
/// use forms::{Button, ButtonRole, Modal, ModalOutcome, ModalStyle};
///
/// let mut modal = Modal::new(ModalStyle::Danger, "Erase disk", "This removes all data.");
/// modal.push(Button::new("Erase", ButtonRole::Default));
/// modal.push(Button::new("Cancel", ButtonRole::Cancel));
/// assert_eq!(modal.selected, 1);
/// assert_eq!(
///     modal.input(forms::Input::Enter),
///     Some(ModalOutcome::Chosen { index: 1, role: ButtonRole::Cancel })
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modal {
    /// The importance of the message.
    pub style: ModalStyle,
    /// The title.
    pub title: String,
    /// The body text.
    pub body: String,
    /// The buttons in display order.
    pub buttons: Vec<Button>,
    /// The highlighted button.
    pub selected: usize,
    /// The progress state, when work is running.
    pub progress: Option<Progress>,
}

impl Modal {
    /// Creates a modal without buttons.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Modal, ModalStyle};
    ///
    /// assert!(Modal::new(ModalStyle::Info, "Done", "The write finished.").buttons.is_empty());
    /// ```
    pub fn new(style: ModalStyle, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            style,
            title: title.into(),
            body: body.into(),
            buttons: Vec::new(),
            selected: 0,
            progress: None,
        }
    }

    /// Adds a button and keeps the safe answer selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{ButtonRole, Modal, ModalStyle};
    ///
    /// let mut modal = Modal::new(ModalStyle::Info, "Title", "Body");
    /// modal.push(forms::Button::new("OK", ButtonRole::Default));
    /// assert_eq!(modal.selected, 0);
    /// ```
    pub fn push(&mut self, button: Button) {
        self.buttons.push(button);
        self.selected = self.safe_index();
    }

    /// Adds a button and returns the modal.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Button, ButtonRole, Modal, ModalStyle};
    ///
    /// let modal = Modal::new(ModalStyle::Info, "T", "B")
    ///     .button(Button::new("OK", ButtonRole::Default));
    /// assert_eq!(modal.buttons.len(), 1);
    /// ```
    #[must_use]
    pub fn button(mut self, button: Button) -> Self {
        self.push(button);
        self
    }

    /// Shows a progress state.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Modal, ModalStyle, Progress};
    ///
    /// let modal = Modal::new(ModalStyle::Info, "Copy", "")
    ///     .progress(Progress { current: 1, total: Some(4) });
    /// assert_eq!(modal.progress, Some(Progress { current: 1, total: Some(4) }));
    /// ```
    #[must_use]
    pub fn progress(mut self, progress: Progress) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Returns the index of the safe answer.
    ///
    /// The Cancel button wins, then the default button, then the last button.
    /// No button is a safe answer for a question without them.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Button, ButtonRole, Modal, ModalStyle};
    ///
    /// let modal = Modal::new(ModalStyle::Danger, "T", "B")
    ///     .button(Button::new("Erase", ButtonRole::Default))
    ///     .button(Button::new("Cancel", ButtonRole::Cancel));
    /// assert_eq!(modal.safe_index(), 1);
    /// ```
    pub fn safe_index(&self) -> usize {
        if let Some(index) = self
            .buttons
            .iter()
            .position(|button| button.role == ButtonRole::Cancel)
        {
            return index;
        }
        if let Some(index) = self
            .buttons
            .iter()
            .position(|button| button.role == ButtonRole::Default)
        {
            return index;
        }
        self.buttons.len().saturating_sub(1)
    }

    /// Handles one input.
    ///
    /// Up and Left select the previous button; Down and Right select the next
    /// button; Enter accepts the highlighted button. A modal with running work
    /// accepts no input until the work ends.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Button, ButtonRole, Input, Modal, ModalOutcome, ModalStyle};
    ///
    /// let mut modal = Modal::new(ModalStyle::Warning, "Erase?", "")
    ///     .button(Button::new("Erase", ButtonRole::Default))
    ///     .button(Button::new("Cancel", ButtonRole::Cancel));
    /// assert_eq!(modal.input(Input::Left), None);
    /// assert_eq!(modal.selected, 0);
    /// assert!(modal.input(Input::Enter).is_some());
    /// ```
    pub fn input(&mut self, input: Input) -> Option<ModalOutcome> {
        if self.buttons.is_empty() || self.progress.is_some() {
            return None;
        }
        let last = self.buttons.len() - 1;
        match input {
            Input::Up | Input::Left => {
                self.selected = if self.selected == 0 {
                    last
                } else {
                    self.selected - 1
                };
                None
            }
            Input::Down | Input::Right => {
                self.selected = if self.selected >= last {
                    0
                } else {
                    self.selected + 1
                };
                None
            }
            Input::Home => {
                self.selected = 0;
                None
            }
            Input::End => {
                self.selected = last;
                None
            }
            Input::Enter => {
                let index = self.selected.min(last);
                Some(ModalOutcome::Chosen {
                    index,
                    role: self.buttons[index].role,
                })
            }
            Input::Back => self
                .buttons
                .iter()
                .position(|button| button.role == ButtonRole::Cancel)
                .map(|_| ModalOutcome::Closed),
            _ => None,
        }
    }
}
