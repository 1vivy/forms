//! The widget that draws a modal over a page.

use alloc::{format, string::String, vec::Vec};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};
use ratatui_widgets::{
    block::Block,
    clear::Clear,
    paragraph::{Paragraph, Wrap},
};

use crate::{ButtonRole, Modal, ModalStyle, Progress, widgets::Theme};

/// Draws a [`Modal`] as a centered box with its buttons.
///
/// The box holds the title, the body, an optional progress bar and one line of
/// buttons. The selected button is highlighted. The body wraps at word
/// boundaries. A small area keeps the title and drops the other parts.
///
/// # Examples
///
/// ```
/// use forms::{Button, ButtonRole, Modal, ModalStyle};
/// use forms::widgets::ModalWidget;
/// use ratatui_core::buffer::Buffer;
/// use ratatui_core::layout::Rect;
/// use ratatui_core::widgets::Widget;
///
/// let modal = Modal::new(ModalStyle::Info, "Done", "The write finished.")
///     .button(Button::new("OK", ButtonRole::Default));
/// let area = Rect::new(0, 0, 40, 12);
/// let mut buffer = Buffer::empty(area);
/// ModalWidget::new(&modal).render(area, &mut buffer);
/// ```
pub struct ModalWidget<'a> {
    modal: &'a Modal,
    theme: Theme,
}

impl<'a> ModalWidget<'a> {
    /// Creates a widget with the default theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Modal, ModalStyle};
    /// use forms::widgets::ModalWidget;
    ///
    /// let modal = Modal::new(ModalStyle::Info, "T", "B");
    /// let _widget = ModalWidget::new(&modal);
    /// ```
    #[must_use]
    pub fn new(modal: &'a Modal) -> Self {
        Self {
            modal,
            theme: Theme::default(),
        }
    }

    /// Replaces the theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Modal, ModalStyle};
    /// use forms::widgets::{ModalWidget, Theme};
    ///
    /// let modal = Modal::new(ModalStyle::Info, "T", "B");
    /// let _widget = ModalWidget::new(&modal).with_theme(Theme::default());
    /// ```
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Returns the title colour of the style of the modal.
    fn title_colour(&self) -> ratatui_core::style::Color {
        match self.modal.style {
            ModalStyle::Info => self.theme.modal_info,
            ModalStyle::Warning => self.theme.modal_warning,
            ModalStyle::Danger => self.theme.modal_danger,
            ModalStyle::Error => self.theme.modal_error,
        }
    }
}

impl Widget for ModalWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let theme = &self.theme;
        let width = area.width.saturating_sub(2).clamp(12, 56).min(area.width);
        let body = self.modal.body.clone();
        let inner_width = width.saturating_sub(2).max(1);
        let body_lines = u16::try_from(wrapped_lines(&body, inner_width)).unwrap_or(1);
        let has_buttons = !self.modal.buttons.is_empty();
        let has_progress = self.modal.progress.is_some();
        let height =
            (2 + body_lines + u16::from(has_progress) + u16::from(has_buttons)).min(area.height);
        if height < 3 {
            return;
        }
        let panel = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        Clear.render(panel, buf);
        let block = Block::bordered()
            .title(format!(" {} ", self.modal.title))
            .title_style(
                Style::new()
                    .fg(self.title_colour())
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::new().fg(self.title_colour()))
            .style(theme.page());
        let inner = block.inner(panel);
        block.render(panel, buf);
        if inner.is_empty() {
            return;
        }
        let mut cursor = inner.y;
        let body_area = Rect::new(inner.x, cursor, inner.width, body_lines.min(inner.height));
        Paragraph::new(body.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(theme.foreground))
            .render(body_area, buf);
        cursor += body_area.height;
        if let Some(progress) = self.modal.progress
            && cursor < inner.bottom()
        {
            draw_progress(
                progress,
                Rect::new(inner.x, cursor, inner.width, 1),
                buf,
                theme,
            );
            cursor += 1;
        }
        if has_buttons && cursor < inner.bottom() {
            let labels: Vec<String> = self
                .modal
                .buttons
                .iter()
                .map(|button| {
                    let safe = button.role == ButtonRole::Cancel;
                    if safe {
                        format!("[{}]", button.label)
                    } else {
                        format!(" {} ", button.label)
                    }
                })
                .collect();
            let text = labels.join("  ");
            let area = Rect::new(inner.x, cursor, inner.width, 1);
            let mut x = inner.x;
            for (index, label) in labels.iter().enumerate() {
                let width = u16::try_from(label.chars().count()).unwrap_or(0);
                if width == 0 || x + width > area.right() {
                    break;
                }
                let style = if index == self.modal.selected {
                    theme.selection
                } else {
                    Style::new().fg(theme.foreground)
                };
                for (cell, character) in (x..area.right()).zip(label.chars()) {
                    buf[(cell, area.y)].set_char(character).set_style(style);
                }
                x = x.saturating_add(width).saturating_add(2);
            }
            let _ = text;
        }
    }
}

/// Returns the number of lines that text needs within a width.
fn wrapped_lines(text: &str, width: u16) -> usize {
    let width = usize::from(width).max(1);
    let mut lines = 1;
    let mut used = 0usize;
    for word in text.split_whitespace() {
        let length = word.chars().count();
        if used == 0 {
            used = length;
        } else if used + 1 + length <= width {
            used += 1 + length;
        } else {
            lines += 1;
            used = length;
        }
        while used > width {
            lines += 1;
            used -= width;
        }
    }
    if used > width {
        lines += 1;
    }
    lines
}

/// Draws a progress bar in one line.
fn draw_progress(progress: Progress, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let width = usize::from(area.width);
    let filled = match progress.total {
        Some(total) if total > 0 => {
            usize::try_from(progress.current.min(total) * width as u64 / total).unwrap_or(width)
        }
        _ => width / 2,
    };
    let bar: String = (0..width)
        .map(|index| {
            if index < filled {
                theme.bar_full
            } else {
                theme.bar_empty
            }
        })
        .collect();
    for (cell, character) in (area.x..area.right()).zip(bar.chars()) {
        buf[(cell, area.y)]
            .set_char(character)
            .set_style(Style::new().fg(theme.link));
    }
}
