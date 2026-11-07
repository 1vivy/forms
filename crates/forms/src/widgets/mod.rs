//! Setup-page rendering for a [`View`](crate::View) and a [`Modal`](crate::Modal).
//!
//! This module draws a page with a title bar, a breadcrumb path, a row list
//! with the label on the left and the value on the right, a selection
//! highlight, a help pane, a message line, a key-hint footer, an editor and a
//! modal. It reads the [`Decoration`](crate::Decoration) of each row from the
//! session, so a consumer control draws through a registered [`Paint`].
//!
//! It needs allocation only, so the same widget serves a UEFI application and
//! a Linux program. Only the ratatui backend and the input source differ.
//!
//! # Examples
//!
//! ```
//! use forms::View;
//! use forms::widgets::FormsWidget;
//! use ratatui_core::buffer::Buffer;
//! use ratatui_core::layout::Rect;
//! use ratatui_core::widgets::Widget;
//!
//! let view = View {
//!     path: Vec::new(),
//!     title: "Setup".into(),
//!     rows: Vec::new(),
//!     selected: None,
//!     help: "Choose a set.".into(),
//!     editor: None,
//!     message: None,
//!     modal: None,
//! };
//! let area = Rect::new(0, 0, 40, 12);
//! let mut buffer = Buffer::empty(area);
//! FormsWidget::new(&view).render(area, &mut buffer);
//! assert_eq!(buffer[(0, 0)].symbol(), "┌");
//! ```

mod modal;
mod page;
mod theme;

pub use modal::ModalWidget;
pub use page::{FormsWidget, Paint, Painters};
pub use theme::{Alignment, Theme};
