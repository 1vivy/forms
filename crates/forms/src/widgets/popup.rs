//! The modal popup that asks for a decision.

use alloc::{format, string::String};
use ratatui_core::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::{
    block::Block,
    clear::Clear,
    paragraph::{Paragraph, Wrap},
};

use crate::{Popup, PopupStyle, widgets::Theme};

/// Draws a [`Popup`] as a centered modal over an area.
///
/// The title names the style, the message wraps at word boundaries, and the
/// legal answers follow in one row. The selected answer is highlighted. The
/// widget never panics on a small area; it drops the parts that do not fit.
///
/// # Examples
///
/// ```
/// use forms::{Popup, PopupStyle, PopupType};
/// use forms::widgets::PopupWidget;
/// use ratatui_core::buffer::Buffer;
/// use ratatui_core::layout::Rect;
/// use ratatui_core::widgets::Widget;
///
/// let popup = Popup::new(PopupStyle::Warning, PopupType::YesNo, "Erase the log?");
/// let area = Rect::new(0, 0, 40, 12);
/// let mut buffer = Buffer::empty(area);
/// PopupWidget::new(&popup).render(area, &mut buffer);
/// assert!(buffer.content().iter().any(|cell| cell.symbol() == "Y"));
/// ```
pub struct PopupWidget<'a> {
    popup: &'a Popup,
    theme: Theme,
}

impl<'a> PopupWidget<'a> {
    /// Creates a widget that draws `popup` with [`Theme::default`].
    #[must_use]
    pub fn new(popup: &'a Popup) -> Self {
        Self {
            popup,
            theme: Theme::default(),
        }
    }

    /// Replaces the theme of the popup.
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for PopupWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        if area.width < 4 || area.height < 3 {
            return;
        }
        let popup = self.popup;
        let width = modal_width(popup, area);
        let lines = wrapped_lines(&popup.message, width.saturating_sub(4));
        let height = u16::try_from(lines.len())
            .unwrap_or(u16::MAX)
            .saturating_add(4)
            .min(area.height);
        let modal = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        Clear.render(modal, buf);
        let block = Block::bordered()
            .title(format!(" {} ", style_title(popup.style)))
            .title_style(style_colour(&theme, popup.style).add_modifier(Modifier::BOLD))
            .border_style(Style::new().fg(theme.border))
            .style(theme.page());
        let inner = block.inner(modal);
        block.render(modal, buf);
        if inner.is_empty() {
            return;
        }
        let buttons = u16::from(inner.height > 1);
        let message_area = Rect {
            height: inner.height - buttons,
            ..inner
        };
        if message_area.height > 0 {
            Paragraph::new(lines.join("\n"))
                .wrap(Wrap { trim: false })
                .style(Style::new().fg(theme.foreground))
                .render(message_area, buf);
        }
        if buttons == 1 {
            render_buttons(
                popup,
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..inner
                },
                buf,
                &theme,
            );
        }
    }
}

/// Draws the answers in one centered row, separated by two blank cells.
fn render_buttons(popup: &Popup, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let mut spans: alloc::vec::Vec<Span> = alloc::vec::Vec::new();
    for (index, selection) in popup.kind.selections().iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let style = if *selection == popup.selected {
            theme.selection
        } else {
            Style::new().fg(theme.foreground)
        };
        spans.push(Span::styled(selection.label(), style));
    }
    Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .render(area, buf);
}

/// Returns the width of the answer row.
fn buttons_width(popup: &Popup) -> u16 {
    let labels: u16 = popup
        .kind
        .selections()
        .iter()
        .map(|selection| Span::raw(selection.label()).width() as u16)
        .sum();
    labels.saturating_add(2 * (popup.kind.selections().len() as u16 - 1))
}

/// Returns the colour of one style.
fn style_colour(theme: &Theme, style: PopupStyle) -> Style {
    let colour = match style {
        PopupStyle::Info => theme.popup_info,
        PopupStyle::Warning => theme.popup_warning,
        PopupStyle::Error => theme.popup_error,
    };
    Style::new().fg(colour)
}

/// Returns the title of one style.
fn style_title(style: PopupStyle) -> &'static str {
    match style {
        PopupStyle::Info => "Information",
        PopupStyle::Warning => "Warning",
        PopupStyle::Error => "Error",
    }
}

/// Returns the width of the modal, no wider than the area.
fn modal_width(popup: &Popup, area: Rect) -> u16 {
    let message = popup
        .message
        .split('\n')
        .map(|line| Span::raw(line).width())
        .max()
        .unwrap_or(0);
    let title = Span::raw(style_title(popup.style)).width() + 2;
    let width = message.max(title).max(usize::from(buttons_width(popup)));
    let width = width.saturating_add(4);
    u16::try_from(width)
        .unwrap_or(u16::MAX)
        .min(area.width)
        .max(4)
}

/// Splits a message into lines no wider than `width`.
fn wrapped_lines(text: &str, width: u16) -> alloc::vec::Vec<String> {
    let width = usize::from(width).max(1);
    text.split('\n')
        .flat_map(|line| {
            let mut out = alloc::vec::Vec::new();
            let mut current = String::new();
            for word in line.split(' ') {
                if current.is_empty() {
                    current.push_str(word);
                } else if current.chars().count() + 1 + word.chars().count() <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    out.push(core::mem::take(&mut current));
                    current.push_str(word);
                }
            }
            out.push(current);
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Input, PopupSelection, PopupType};
    use alloc::{string::ToString, vec, vec::Vec};

    fn draw(popup: &Popup, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        PopupWidget::new(popup).render(area, &mut buffer);
        buffer
    }

    /// Returns the non-empty screen rows.
    fn rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|row| {
                let mut line = String::new();
                for x in buffer.area.x..buffer.area.right() {
                    line.push_str(buffer[(x, row)].symbol());
                }
                line.trim_end().to_string()
            })
            .filter(|line| !line.trim().is_empty())
            .collect()
    }

    fn row_of(lines: &[String], needle: &str) -> String {
        lines
            .iter()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no row contains {needle:?}: {lines:?}"))
            .clone()
    }

    #[test]
    fn title_names_the_style() {
        for (style, title) in [
            (PopupStyle::Info, "Information"),
            (PopupStyle::Warning, "Warning"),
            (PopupStyle::Error, "Error"),
        ] {
            let popup = Popup::new(style, PopupType::Ok, "Done");
            let buffer = draw(&popup, 40, 8);
            row_of(&rows(&buffer), title);
        }
    }

    #[test]
    fn every_type_draws_its_legal_answers() {
        for (kind, labels) in [
            (PopupType::Ok, vec!["OK"]),
            (PopupType::OkCancel, vec!["OK", "Cancel"]),
            (PopupType::YesNo, vec!["Yes", "No"]),
            (PopupType::YesNoCancel, vec!["Yes", "No", "Cancel"]),
        ] {
            let popup = Popup::new(PopupStyle::Info, kind, "Continue?");
            let buffer = draw(&popup, 60, 10);
            let buttons = row_of(&rows(&buffer), labels[0]);
            for label in labels {
                assert!(
                    buttons.contains(label),
                    "type {kind:?} misses {label} in {buttons:?}"
                );
            }
            let drawn = buttons.matches("Yes").count()
                + buttons.matches("No").count()
                + buttons.matches("OK").count()
                + buttons.matches("Cancel").count();
            assert_eq!(drawn, popup.kind.selections().len(), "row was {buttons:?}");
        }
    }

    #[test]
    fn selection_moves_the_highlight() {
        let mut popup = Popup::new(PopupStyle::Warning, PopupType::YesNo, "Erase?");
        let first = draw(&popup, 40, 10);
        popup.input(Input::Right);
        let second = draw(&popup, 40, 10);
        let changed = first
            .content()
            .iter()
            .zip(second.content())
            .filter(|(a, b)| a.style() != b.style())
            .count();
        assert!(changed >= 4, "styles changed in {changed} cells");
    }

    #[test]
    fn small_area_draws_nothing() {
        let popup = Popup::new(PopupStyle::Error, PopupType::Ok, "Failed");
        let buffer = draw(&popup, 3, 10);
        assert!(buffer.content().iter().all(|cell| cell.symbol() == " "));
    }

    #[test]
    fn a_long_message_wraps_and_the_modal_stays_inside() {
        let popup = Popup::new(
            PopupStyle::Error,
            PopupType::Ok,
            "A long message ".repeat(12),
        );
        let buffer = draw(&popup, 50, 20);
        let lines = rows(&buffer);
        let wrapped = lines.iter().filter(|line| line.contains("long")).count();
        assert_eq!(wrapped, 4, "rows were {lines:?}");
        for line in &lines {
            assert!(
                line.chars().count() <= 50,
                "a row leaves the area: {line:?}"
            );
        }
    }

    #[test]
    fn answers_sit_in_one_row_separated_by_two_cells() {
        let popup = Popup::new(PopupStyle::Info, PopupType::YesNoCancel, "Question");
        let buffer = draw(&popup, 60, 10);
        let buttons = row_of(&rows(&buffer), "Yes");
        assert!(buttons.contains("Yes  No  Cancel"), "row was {buttons:?}");
        assert_eq!(PopupSelection::Cancel.label(), "Cancel");
    }
}
