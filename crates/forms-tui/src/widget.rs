//! The widget that draws one setup page.

use alloc::{format, string::String};
use forms::{Editor, EditorKind, Row, RowKind, View};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::Widget,
};
use ratatui_widgets::{
    block::Block,
    clear::Clear,
    paragraph::{Paragraph, Wrap},
};

use crate::Theme;

/// Draws a [`View`] as a setup page.
///
/// The page has a bordered title bar, a breadcrumb path, a row list with the
/// prompt left and the value right, a help pane, a message line, a key-hint
/// footer and an editor as a centered modal. A choice modal ends with a
/// `< Back` row, so a front end without an Esc key cancels it. The widget
/// never panics on a small area; it drops the parts that do not fit.
///
/// # Examples
///
/// ```
/// use forms::{Row, RowKind, View};
/// use forms_tui::FormsWidget;
/// use ratatui_core::buffer::Buffer;
/// use ratatui_core::layout::Rect;
/// use ratatui_core::widgets::Widget;
///
/// let view = View {
///     path: Vec::new(),
///     title: "Display".into(),
///     rows: vec![Row {
///         prompt: "Night mode".into(),
///         value: "Off".into(),
///         kind: RowKind::Toggle,
///         enabled: true,
///         uncertain: false,
///     }],
///     selected: Some(0),
///     help: "Turn the dark theme on.".into(),
///     editor: None,
///     message: None,
/// };
/// let area = Rect::new(0, 0, 40, 14);
/// let mut buffer = Buffer::empty(area);
/// FormsWidget::new(&view).render(area, &mut buffer);
/// ```
pub struct FormsWidget<'a> {
    view: &'a View,
    theme: Theme,
}

impl<'a> FormsWidget<'a> {
    /// Creates a widget that draws `view` with [`Theme::default`].
    #[must_use]
    pub fn new(view: &'a View) -> Self {
        Self {
            view,
            theme: Theme::default(),
        }
    }

    /// Replaces the theme of the page.
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for FormsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let theme = self.theme;
        buf.set_style(area, theme.page());
        if area.width < 4 || area.height < 4 {
            return;
        }
        let view = self.view;
        let frame = Block::bordered()
            .title(format!(" {} ", view.title))
            .title_style(Style::new().fg(theme.title).add_modifier(Modifier::BOLD))
            .border_style(Style::new().fg(theme.border))
            .style(theme.page());
        let inner = frame.inner(area);
        frame.render(area, buf);
        if inner.is_empty() {
            return;
        }
        let regions = Regions::split(inner, view.message.is_some(), !view.path.is_empty());
        if let Some(breadcrumb) = regions.breadcrumb {
            render_breadcrumb(view, breadcrumb, buf, &theme);
        }
        render_rows(view, regions.list, buf, &theme);
        if let Some(help) = regions.help {
            render_help(view, help, buf, &theme);
        }
        if let (Some(message), Some(line)) = (view.message.as_deref(), regions.message) {
            buf.set_style(line, theme.page());
            put(buf, line, message, Style::new().fg(theme.message));
        }
        if let Some(footer) = regions.footer {
            render_footer(view.editor.is_some(), footer, buf, &theme);
        }
        if let Some(editor) = &view.editor {
            render_editor(editor, area, buf, &theme);
        }
    }
}

/// The parts of the inner page area.
struct Regions {
    breadcrumb: Option<Rect>,
    list: Rect,
    help: Option<Rect>,
    message: Option<Rect>,
    footer: Option<Rect>,
}

impl Regions {
    /// Splits the inner area into one-line header, list, help and footer parts.
    fn split(inner: Rect, has_message: bool, has_breadcrumb: bool) -> Self {
        let mut top = inner.y;
        let mut bottom = inner.bottom();
        let mut take_line = |from_bottom: bool| -> Option<Rect> {
            if bottom.saturating_sub(top) < 2 {
                return None;
            }
            bottom -= 1;
            let y = if from_bottom { bottom } else { top };
            if !from_bottom {
                top += 1;
            }
            Some(Rect::new(inner.x, y, inner.width, 1))
        };
        let footer = take_line(true);
        let message = has_message.then(|| take_line(true)).flatten();
        let breadcrumb = has_breadcrumb.then(|| take_line(false)).flatten();
        let content = bottom.saturating_sub(top);
        let help_height = if content >= 7 {
            (content / 2).min(4)
        } else {
            0
        };
        let list_height = content - help_height;
        let list = Rect::new(inner.x, top, inner.width, list_height);
        let help = (help_height > 0)
            .then(|| Rect::new(inner.x, top + list_height, inner.width, help_height));
        Self {
            breadcrumb,
            list,
            help,
            message,
            footer,
        }
    }
}

/// Draws the path from the root page to the current page.
fn render_breadcrumb(view: &View, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let mut text = String::new();
    for (index, part) in view
        .path
        .iter()
        .chain(core::iter::once(&view.title))
        .enumerate()
    {
        if index > 0 {
            text.push_str(" > ");
        }
        text.push_str(part);
    }
    buf.set_style(area, theme.page());
    put(buf, area, &text, Style::new().fg(theme.help));
}

/// Draws the visible rows and the selection highlight.
fn render_rows(view: &View, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let height = area.height as usize;
    let offset = match view.selected {
        Some(selected) if selected >= height => selected + 1 - height,
        _ => 0,
    };
    for (index, row) in view.rows.iter().enumerate().skip(offset).take(height) {
        let line = Rect::new(area.x, area.y + (index - offset) as u16, area.width, 1);
        render_row(row, line, buf, theme, view.selected == Some(index));
    }
}

/// Draws one row: the prompt on the left and the value on the right.
fn render_row(row: &Row, area: Rect, buf: &mut Buffer, theme: &Theme, selected: bool) {
    if area.is_empty() {
        return;
    }
    let mut style = theme.row_style(row.kind);
    if !row.enabled {
        style = theme.disabled;
    } else if row.uncertain {
        style = style.patch(theme.uncertain);
    }
    if selected {
        style = style.patch(theme.selection);
    }
    buf.set_style(area, style);
    if matches!(row.kind, RowKind::Heading) {
        put(buf, area, &row.prompt, style);
        return;
    }
    let value_columns = area.width / 2;
    let prompt_columns = area.width - value_columns;
    let prompt = if matches!(row.kind, RowKind::FormSet | RowKind::Link) {
        format!("{} >", row.prompt)
    } else {
        row.prompt.clone()
    };
    put(
        buf,
        Rect::new(area.x, area.y, prompt_columns, 1),
        &prompt,
        style,
    );
    if !row.value.is_empty() && value_columns > 0 {
        let width = Line::raw(row.value.as_str())
            .width()
            .min(value_columns as usize);
        let value_area = Rect::new(area.x + area.width - width as u16, area.y, value_columns, 1);
        put(buf, value_area, &row.value, style);
    }
}

/// Draws the help of the selected row in a bordered pane.
fn render_help(view: &View, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let block = Block::bordered()
        .title(" Help ")
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border))
        .style(theme.page());
    let inner = block.inner(area);
    block.render(area, buf);
    if inner.is_empty() {
        return;
    }
    let help = if view.help.is_empty() {
        "No help for this item."
    } else {
        view.help.as_str()
    };
    Paragraph::new(help)
        .style(Style::new().fg(theme.help))
        .wrap(Wrap { trim: true })
        .render(inner, buf);
}

/// Draws the key hints that fit the current state.
fn render_footer(editing: bool, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let hints = if editing {
        "Up/Down Choose   Enter Accept   Esc Cancel"
    } else {
        "Up/Down Move   Enter Open   Esc Back"
    };
    buf.set_style(area, theme.page());
    put(buf, area, hints, Style::new().fg(theme.hint));
}

/// Draws the editor as a centered modal over the page.
fn render_editor(editor: &Editor, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let (width, height) = editor_size(editor, area);
    if width < 2 || height < 2 {
        return;
    }
    let modal = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    Clear.render(modal, buf);
    let block = Block::bordered()
        .title(format!(" {} ", editor.prompt))
        .title_style(Style::new().fg(theme.title).add_modifier(Modifier::BOLD))
        .border_style(Style::new().fg(theme.border))
        .style(theme.page());
    let inner = block.inner(modal);
    block.render(modal, buf);
    if inner.is_empty() {
        return;
    }
    match &editor.kind {
        EditorKind::Choice { choices, selected } => {
            let rows = inner.height.saturating_sub(1) as usize;
            let offset = if rows > 0 && *selected >= rows {
                selected + 1 - rows
            } else {
                0
            };
            for (line, choice) in choices.iter().enumerate().skip(offset).take(rows) {
                render_choice(buf, theme, inner, line, offset, choice, line == *selected);
            }
            // The Back row follows the choices; it is the last entry of the
            // list, so a phone cancels the editor with the volume keys alone.
            let back = choices.len();
            if back >= offset && back < offset.saturating_add(rows) {
                render_choice(
                    buf,
                    theme,
                    inner,
                    back,
                    offset,
                    BACK_PROMPT,
                    *selected == back,
                );
            }
            if let Some(last) = last_line(inner) {
                buf.set_style(last, theme.page());
                put(
                    buf,
                    last,
                    "Up/Down Choose   Enter Accept   Esc Cancel",
                    Style::new().fg(theme.hint),
                );
            }
        }
        EditorKind::Number {
            text,
            minimum,
            maximum,
        } => {
            if let Some(line) = line_at(inner, 0) {
                buf.set_style(line, theme.page());
                put(
                    buf,
                    line,
                    &format!(" {text}_"),
                    Style::new().fg(theme.foreground),
                );
            }
            if let Some(line) = line_at(inner, 1) {
                buf.set_style(line, theme.page());
                put(
                    buf,
                    line,
                    &format!(" {minimum}..{maximum}"),
                    Style::new().fg(theme.info),
                );
            }
            if inner.height >= 3
                && let Some(last) = last_line(inner)
            {
                buf.set_style(last, theme.page());
                put(
                    buf,
                    last,
                    "Enter Accept   Esc Cancel",
                    Style::new().fg(theme.hint),
                );
            }
        }
        EditorKind::Text { text, maximum } => {
            if let Some(line) = line_at(inner, 0) {
                buf.set_style(line, theme.page());
                put(
                    buf,
                    line,
                    &format!(" {text}_"),
                    Style::new().fg(theme.foreground),
                );
            }
            if let Some(line) = line_at(inner, 1) {
                buf.set_style(line, theme.page());
                put(
                    buf,
                    line,
                    &format!(" {}/{}", text.chars().count(), maximum),
                    Style::new().fg(theme.info),
                );
            }
            if inner.height >= 3
                && let Some(last) = last_line(inner)
            {
                buf.set_style(last, theme.page());
                put(
                    buf,
                    last,
                    "Enter Accept   Esc Cancel",
                    Style::new().fg(theme.hint),
                );
            }
        }
    }
}

/// The prompt of the row that cancels a choice editor.
const BACK_PROMPT: &str = "< Back";

/// Draws one line of a choice modal: the label and the selection bar.
///
/// `line` is the index of the entry in the whole list, `offset` the index of
/// the first visible line, so the drawn line is `line - offset`.
fn render_choice(
    buf: &mut Buffer,
    theme: &Theme,
    inner: Rect,
    line: usize,
    offset: usize,
    label: &str,
    selected: bool,
) {
    let Some(row) = line
        .checked_sub(offset)
        .filter(|row| *row < usize::from(inner.height))
    else {
        return;
    };
    let area = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
    let mut style = Style::new().fg(theme.foreground);
    buf.set_style(area, theme.page());
    if selected {
        style = style.patch(theme.selection);
        buf.set_style(area, theme.selection);
    }
    put(buf, area, &format!(" {label}"), style);
}

/// Returns the size of the modal for an editor inside `area`.
fn editor_size(editor: &Editor, area: Rect) -> (u16, u16) {
    let (mut content, height) = match &editor.kind {
        EditorKind::Choice { choices, .. } => {
            let mut content = Line::raw(editor.prompt.as_str()).width();
            for choice in choices {
                content = content.max(Line::raw(choice.as_str()).width() + 2);
            }
            content = content.max(Line::raw(BACK_PROMPT).width() + 2);
            // One line per choice, one for the Back row, one for the hints and
            // two for the border.
            let lines = u16::try_from(choices.len().max(1) + 4).unwrap_or(u16::MAX);
            (content, lines)
        }
        EditorKind::Number { .. } => (Line::raw(editor.prompt.as_str()).width().max(24), 5),
        EditorKind::Text { text, .. } => (
            Line::raw(editor.prompt.as_str())
                .width()
                .max(Line::raw(text.as_str()).width() + 8),
            5,
        ),
    };
    let width = u16::try_from(content.saturating_add(4))
        .unwrap_or(u16::MAX)
        .clamp(area.width.min(20), area.width);
    let _ = &mut content;
    (width, height.min(area.height))
}

/// Returns the area of one line of `inner`, if it exists.
fn line_at(inner: Rect, index: u16) -> Option<Rect> {
    (index < inner.height).then(|| Rect::new(inner.x, inner.y + index, inner.width, 1))
}

/// Returns the area of the last line of `inner`.
fn last_line(inner: Rect) -> Option<Rect> {
    inner
        .height
        .checked_sub(1)
        .map(|index| Rect::new(inner.x, inner.y + index, inner.width, 1))
}

/// Draws `text` in one line, clipped to `area`.
fn put(buf: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.is_empty() {
        return;
    }
    buf.set_stringn(area.x, area.y, text, area.width as usize, style);
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    use forms::{Editor, EditorKind, Row, RowKind, View};
    use ratatui_core::backend::TestBackend;
    use ratatui_core::buffer::Buffer;
    use ratatui_core::terminal::Terminal;

    use crate::{FormsWidget, Theme};

    /// Renders one view into an off-screen backend and returns the cells.
    fn render(view: &View, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("the test backend cannot fail");
        terminal
            .draw(|frame| frame.render_widget(FormsWidget::new(view), frame.area()))
            .expect("the test backend cannot fail");
        terminal.backend().buffer().clone()
    }

    /// Returns the screen row texts of a buffer.
    fn lines(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                let mut line = String::new();
                for x in 0..buffer.area.width {
                    line.push_str(buffer[(x, y)].symbol());
                }
                line
            })
            .collect()
    }

    /// Returns the row of every cell that carries the selection background.
    fn selected_rows(buffer: &Buffer, theme: &Theme) -> Vec<usize> {
        (0..usize::from(buffer.area.height))
            .filter(|&y| {
                (0..buffer.area.width).any(|x| {
                    theme
                        .selection
                        .bg
                        .is_some_and(|background| buffer[(x, y as u16)].bg == background)
                })
            })
            .collect()
    }

    /// Builds one row of the given kind.
    fn row(prompt: &str, value: &str, kind: RowKind) -> Row {
        Row {
            prompt: prompt.into(),
            value: value.into(),
            kind,
            enabled: true,
            uncertain: false,
        }
    }

    /// Builds a page with a heading, three rows and help text.
    fn page(selected: Option<usize>, editor: Option<Editor>) -> View {
        View {
            path: vec!["Recovery".into()],
            title: "Display".into(),
            rows: vec![
                row("Appearance", "", RowKind::Heading),
                row("Night mode of the panel", "Off", RowKind::Toggle),
                row("Scale of the interface", "150%", RowKind::Choice),
                row("Panel", "1440 x 3120", RowKind::Info),
            ],
            selected,
            help: "Choose a value and press Enter.".into(),
            editor,
            message: None,
        }
    }

    #[test]
    fn selected_row_is_highlighted_once() {
        let view = page(Some(2), None);
        let buffer = render(&view, 50, 16);
        let theme = Theme::default();
        let highlighted = selected_rows(&buffer, &theme);
        assert_eq!(highlighted.len(), 1, "one selection bar: {highlighted:?}");
        let texts = lines(&buffer);
        assert!(
            texts[highlighted[0]].contains("Scale of the interface"),
            "the selection follows the selected row: {:?}",
            texts[highlighted[0]]
        );
    }

    #[test]
    fn editor_without_selection_draws_no_bar() {
        let view = page(None, None);
        let buffer = render(&view, 50, 16);
        let theme = Theme::default();
        assert!(selected_rows(&buffer, &theme).is_empty());
    }

    /// Builds a choice editor over `choices` with `selected` pending.
    fn choice_editor(choices: Vec<String>, selected: usize) -> Editor {
        Editor {
            prompt: "Level".into(),
            kind: EditorKind::Choice { choices, selected },
        }
    }

    #[test]
    fn choice_modal_shows_a_back_row_and_highlights_it() {
        let editor = choice_editor(vec!["Low".into(), "High".into()], 2);
        let view = page(None, Some(editor));
        let buffer = render(&view, 50, 16);
        let theme = Theme::default();
        let texts = lines(&buffer);
        let back = texts
            .iter()
            .position(|line| line.contains("< Back"))
            .expect("the choice modal shows a Back row");
        assert_eq!(selected_rows(&buffer, &theme), [back]);
    }

    #[test]
    fn choice_modal_scrolls_the_back_row_into_view() {
        let choices: Vec<String> = (0..9).map(|i| format!("Option {i}")).collect();
        let selected = choices.len();
        let editor = choice_editor(choices, selected);
        let view = page(None, Some(editor));
        let buffer = render(&view, 50, 12);
        let theme = Theme::default();
        let texts = lines(&buffer);
        let back = texts
            .iter()
            .position(|line| line.contains("< Back"))
            .expect("the Back row stays visible while the list scrolls");
        assert_eq!(selected_rows(&buffer, &theme), [back]);
        assert!(
            texts.iter().any(|line| line.contains("Option 8")),
            "the last choice sits next to Back: {texts:?}"
        );
        assert!(
            texts.iter().all(|line| !line.contains("Option 0")),
            "the first choices scrolled out: {texts:?}"
        );
    }
}
