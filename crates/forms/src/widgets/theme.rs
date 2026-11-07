//! Colours, styles and presentation defaults of a page.

use crate::{Decoration, Truncation};
use ratatui_core::style::{Color, Modifier, Style};

/// Where a value sits in its column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    /// The value ends at the right edge.
    #[default]
    Right,
    /// The value starts at the left edge.
    Left,
}

/// The colours, styles and layout defaults of a page.
///
/// A consumer starts from [`Theme::default`] and changes single fields. The
/// widget reads every field per draw, so a changed theme takes effect on the
/// next frame.
///
/// # Examples
///
/// ```
/// use forms::widgets::Theme;
/// use ratatui_core::style::Color;
///
/// let mut theme = Theme::default();
/// theme.title = Color::Red;
/// assert_eq!(theme.split, 50);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The page background.
    pub background: Color,
    /// The normal text colour.
    pub foreground: Color,
    /// The colour of borders.
    pub border: Color,
    /// The colour of the page title.
    pub title: Color,
    /// The colour of headings.
    pub heading: Color,
    /// The colour of information.
    pub info: Color,
    /// The colour of links and the root list.
    pub link: Color,
    /// The colour of help text.
    pub help: Color,
    /// The colour of a message.
    pub message: Color,
    /// The colour of key hints.
    pub hint: Color,
    /// The style of the selected row.
    pub selection: Style,
    /// The style of a selected row that accepts no change.
    pub gray_selected: Style,
    /// The style of a row that accepts no change.
    pub disabled: Style,
    /// The style of a row with an unreadable value or condition.
    pub uncertain: Style,
    /// Where a value sits in its column.
    pub value_alignment: Alignment,
    /// The title colour of an information modal.
    pub modal_info: Color,
    /// The title colour of a warning modal.
    pub modal_warning: Color,
    /// The title colour of a danger modal.
    pub modal_danger: Color,
    /// The title colour of an error modal.
    pub modal_error: Color,
    /// Draws a rule after a heading.
    pub heading_rule: bool,
    /// The label column as a percentage of the width.
    pub split: u16,
    /// The largest label width in cells, or 0 for the split width.
    pub label_limit: u16,
    /// The overflow policy when a row does not choose one.
    pub truncation: Truncation,
    /// The mark of a cut value.
    pub clip: &'static str,
    /// The mark before an enabled switch.
    pub toggle_on: &'static str,
    /// The mark before a disabled switch.
    pub toggle_off: &'static str,
    /// The character of a divider.
    pub divider: char,
    /// The character of a filled bar cell.
    pub bar_full: char,
    /// The character of an empty bar cell.
    pub bar_empty: char,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color::Black,
            foreground: Color::Gray,
            border: Color::LightBlue,
            title: Color::White,
            heading: Color::Yellow,
            info: Color::Gray,
            link: Color::LightCyan,
            help: Color::Gray,
            message: Color::LightYellow,
            hint: Color::DarkGray,
            selection: Style::new()
                .fg(Color::Black)
                .bg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
            disabled: Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            gray_selected: Style::new()
                .fg(Color::Black)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
            value_alignment: Alignment::Right,
            uncertain: Style::new()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::ITALIC),
            modal_info: Color::LightCyan,
            modal_warning: Color::LightYellow,
            modal_danger: Color::LightRed,
            modal_error: Color::LightRed,
            heading_rule: true,
            split: 50,
            label_limit: 0,
            truncation: Truncation::End,
            clip: "~",
            toggle_on: "[X] ",
            toggle_off: "[ ] ",
            divider: '─',
            bar_full: '█',
            bar_empty: '░',
        }
    }
}

impl Theme {
    /// Returns the base style of a decoration.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Decoration, widgets::Theme};
    ///
    /// let theme = Theme::default();
    /// assert_eq!(theme.style(Decoration::Heading), theme.style(Decoration::Heading));
    /// ```
    pub fn style(&self, decoration: Decoration) -> Style {
        let colour = match &decoration {
            Decoration::Heading => self.heading,
            Decoration::Rule => self.border,
            Decoration::Bar { .. } | Decoration::Slider { .. } => self.link,
            Decoration::Toggle(_) | Decoration::Plain | Decoration::Custom { .. } => {
                self.foreground
            }
        };
        let style = Style::new().fg(colour);
        if matches!(decoration, Decoration::Heading) {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    /// Returns the style of the page background.
    ///
    /// # Examples
    ///
    /// ```
    /// let style = forms::widgets::Theme::default().page();
    /// assert_eq!(style.fg, Some(ratatui_core::style::Color::Gray));
    /// ```
    pub fn page(&self) -> Style {
        Style::new().fg(self.foreground).bg(self.background)
    }
}
