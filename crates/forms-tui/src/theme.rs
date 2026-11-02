//! Colours and styles for the setup page.

use forms::RowKind;
use ratatui_core::style::{Color, Modifier, Style};

/// The colours and styles of a setup page.
///
/// [`Theme::default`] is a dark setup page with a light selection bar. A front
/// end may start from the default and change single fields.
///
/// # Examples
///
/// ```
/// use forms_tui::Theme;
/// use ratatui_core::style::Color;
///
/// let mut theme = Theme::default();
/// theme.title = Color::Red;
/// assert_eq!(theme.title, Color::Red);
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
    /// The colour of read-only information.
    pub info: Color,
    /// The colour of a link to another page.
    pub link: Color,
    /// The colour of the help text.
    pub help: Color,
    /// The colour of a message.
    pub message: Color,
    /// The colour of the key hints.
    pub hint: Color,
    /// The style of the selected row.
    pub selection: Style,
    /// The style of a row that cannot be selected.
    pub disabled: Style,
    /// The style of a row with an unsupported condition.
    pub uncertain: Style,
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
            uncertain: Style::new()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::ITALIC),
        }
    }
}

impl Theme {
    /// Returns the base style of a row of this kind.
    pub(crate) fn row_style(&self, kind: RowKind) -> Style {
        let colour = match kind {
            RowKind::FormSet => self.link,
            RowKind::Heading => self.heading,
            RowKind::Info => self.info,
            RowKind::Link | RowKind::Back => self.link,
            RowKind::Action | RowKind::Toggle | RowKind::Choice => self.foreground,
            RowKind::Number | RowKind::Text => self.foreground,
        };
        let style = Style::new().fg(colour);
        if matches!(kind, RowKind::Heading) {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    /// Returns the style of the page background.
    pub(crate) fn page(&self) -> Style {
        Style::new().fg(self.foreground).bg(self.background)
    }
}
