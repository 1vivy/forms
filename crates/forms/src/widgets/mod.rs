//! Setup-page rendering for a [`View`](crate::View).
//!
//! This module draws a page the way a PC setup page looks: a title bar, a
//! breadcrumb path, a row list with the prompt on the left and the value on the
//! right, a selection highlight, a help pane, a message line, a key-hint footer
//! and a centered editor as a modal.
//!
//! It needs allocation only. The same widget therefore serves a UEFI
//! application and a Linux program; only the ratatui backend and the input
//! source differ.
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
//!     help: "Choose a formset.".into(),
//!     editor: None,
//!     message: None,
//! };
//! let area = Rect::new(0, 0, 40, 12);
//! let mut buffer = Buffer::empty(area);
//! FormsWidget::new(&view).render(area, &mut buffer);
//! assert_eq!(buffer[(0, 0)].symbol(), "┌");
//! ```

mod theme;
mod widget;

pub use theme::Theme;
pub use widget::FormsWidget;
