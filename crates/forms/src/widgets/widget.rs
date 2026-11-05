//! The widget that draws one setup page.

use alloc::{format, string::String, vec::Vec};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::{
    block::Block,
    clear::Clear,
    paragraph::{Paragraph, Wrap},
};

use crate::{Editor, EditorKind, Row, RowKind, View, widgets::Theme};

/// Draws a [`View`] as a setup page.
///
/// The page has a bordered title bar, a breadcrumb path, a row list with the
/// prompt left and the value right, a help pane, a message line, a key-hint
/// footer and an editor as a centered modal. A choice modal ends with a
/// `< Back` row, so a front end without an Esc key cancels it. The widget
/// never panics on a small area; it drops the parts that do not fit.
///
/// The prompt column and the value column never touch. One blank cell
/// separates them, and the value ends at the inner right edge. Text that does
/// not fit is cut at a grapheme boundary and ends with `~`, so a clipped value
/// is visible as clipped. A read-only `RowKind::Info` value wraps into the
/// value column at word boundaries instead, and takes one screen line per
/// wrapped part. A value on one line keeps its right alignment; a wrapped
/// value starts at the left edge of its column, and a word wider than the
/// column breaks at a grapheme boundary. Key hints are centered with at least
/// one blank cell at each edge, and the widget picks the longest hint form
/// that fits the width.
///
/// # Examples
///
/// ```
/// use forms::{Row, RowKind, View};
/// use forms::widgets::FormsWidget;
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
            render_footer(view.editor.as_ref(), footer, buf, &theme);
        }
        if let Some(editor) = &view.editor {
            let modal_area = Rect::new(
                inner.x,
                inner.y,
                inner.width,
                regions
                    .footer
                    .map_or(inner.height, |footer| footer.y.saturating_sub(inner.y)),
            );
            render_editor(editor, modal_area, buf, &theme);
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
    // Selection indexes remain model-row indexes. Only the viewport uses
    // physical lines, including wrapped read-only values.
    let selected = view
        .selected
        .unwrap_or(0)
        .min(view.rows.len().saturating_sub(1));
    let mut offset = 0;
    let mut height = 0usize;
    for row in view.rows.iter().take(selected + 1) {
        height = height.saturating_add(row_height(row, area.width));
    }
    while height > usize::from(area.height) && offset < selected {
        height = height.saturating_sub(row_height(&view.rows[offset], area.width));
        offset += 1;
    }
    let mut y = area.y;
    for (index, row) in view.rows.iter().enumerate().skip(offset) {
        if y >= area.bottom() {
            break;
        }
        let height = row_height(row, area.width).min(usize::from(area.bottom() - y)) as u16;
        render_row(
            row,
            Rect::new(area.x, y, area.width, height),
            buf,
            theme,
            view.selected == Some(index),
        );
        y += height;
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
        if theme.heading_rule {
            let text = Span::raw(row.prompt.as_str()).width();
            let start = text.saturating_add(1).min(usize::from(area.width)) as u16;
            for x in area.x + start..area.right() {
                buf[(x, area.y)].set_symbol("─").set_style(style);
            }
        }
        return;
    }
    // A row without a value uses the full width for its prompt.
    let (prompt_columns, value_columns) = if row.value.is_empty() {
        (area.width, 0)
    } else {
        columns(area.width)
    };
    let opens = matches!(row.kind, RowKind::FormSet | RowKind::Link);
    // An opening row keeps two cells for its arrow, so the arrow never
    // replaces the last prompt character.
    let label_columns = if opens {
        prompt_columns.saturating_sub(2)
    } else {
        prompt_columns
    };
    put(
        buf,
        Rect::new(area.x, area.y, label_columns, 1),
        &row.prompt,
        style,
    );
    if opens && prompt_columns >= 2 {
        put(
            buf,
            Rect::new(area.x + label_columns, area.y, 2, 1),
            " >",
            style,
        );
    }
    if !row.value.is_empty() && value_columns > 0 {
        let value_area = Rect::new(
            area.right() - value_columns,
            area.y,
            value_columns,
            area.height,
        );
        if row.kind == RowKind::Info {
            render_info_value(row, value_area, buf, style);
        } else {
            put_value(buf, value_area, &row.value, style);
        }
    }
}

/// Draws a read-only value, wrapped into the value column.
///
/// A value that fits on one line keeps its right alignment. A value that
/// wraps starts at the left edge of the column, so the block reads as one
/// paragraph. A row that runs out of lines ends with `~`, so clipped text
/// stays visible as clipped text.
fn render_info_value(row: &Row, area: Rect, buf: &mut Buffer, style: Style) {
    if area.is_empty() {
        return;
    }
    let lines: Vec<&str> = wrapped(&row.value, area.width).collect();
    let left = lines.len() > 1;
    for (index, text) in lines.iter().take(usize::from(area.height)).enumerate() {
        let line = index as u16;
        let clipped = index + 1 == usize::from(area.height) && lines.len() > index + 1;
        let width = if clipped {
            area.width.saturating_sub(2)
        } else {
            area.width
        };
        let target = Rect::new(area.x, area.y + line, width, 1);
        if left {
            put_left(buf, target, text, style);
        } else {
            put_clipped(buf, target, text, style);
        }
        if clipped && area.width >= 2 {
            put(
                buf,
                Rect::new(area.right() - 1, area.y + line, 1, 1),
                "~",
                style,
            );
        }
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

/// Draws centered hints with one spare cell at each edge.
fn render_footer(editor: Option<&Editor>, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let variants: &[&str] = match editor.map(|editor| &editor.kind) {
        None => &[
            "Up/Down Move  Enter Open  Esc Back",
            "Up/Down Move  Enter Open",
            "Up/Down  Enter",
            "Enter",
        ],
        Some(EditorKind::Choice { .. }) => &[
            "Up/Down Choose  Enter Set  Esc Cancel",
            "Up/Down Choose  Enter Set",
            "Up/Down  Enter",
            "Enter",
        ],
        Some(EditorKind::Number { .. } | EditorKind::Text { .. }) => &[
            "Enter Accept  Esc Cancel",
            "Enter Set  Esc Back",
            "Enter Set",
            "Enter",
        ],
    };
    buf.set_style(area, theme.page());
    let available = usize::from(area.width.saturating_sub(2));
    if let Some(text) = variants
        .iter()
        .copied()
        .find(|text| Span::raw(*text).width() <= available)
    {
        let width = Span::raw(text).width() as u16;
        put(
            buf,
            Rect::new(area.x + (area.width - width) / 2, area.y, width, 1),
            text,
            Style::new().fg(theme.hint),
        );
    }
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
            let rows = usize::from(inner.height);
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
    let label_area = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(1),
        1,
    );
    put(buf, label_area, label, style);
}

/// Returns the size of the modal for an editor inside `area`.
fn editor_size(editor: &Editor, area: Rect) -> (u16, u16) {
    let (content, height) = match &editor.kind {
        EditorKind::Choice { choices, .. } => {
            let mut content = Line::raw(editor.prompt.as_str()).width();
            for choice in choices {
                content = content.max(Line::raw(choice.as_str()).width() + 2);
            }
            content = content.max(Line::raw(BACK_PROMPT).width() + 2);
            // One line per choice, one for Back and two for the border.
            let lines = u16::try_from(choices.len() + 3).unwrap_or(u16::MAX);
            (content, lines)
        }
        EditorKind::Number { .. } => (Line::raw(editor.prompt.as_str()).width().max(24), 4),
        EditorKind::Text { text, .. } => (
            Line::raw(editor.prompt.as_str())
                .width()
                .max(Line::raw(text.as_str()).width() + 8),
            4,
        ),
    };
    let width = u16::try_from(content.saturating_add(4))
        .unwrap_or(u16::MAX)
        .clamp(area.width.min(20), area.width);
    (width, height.min(area.height))
}

/// Returns the area of one line of `inner`, if it exists.
fn line_at(inner: Rect, index: u16) -> Option<Rect> {
    (index < inner.height).then(|| Rect::new(inner.x, inner.y + index, inner.width, 1))
}

/// Returns prompt and value widths with a separating cell.
fn columns(width: u16) -> (u16, u16) {
    let value = width / 2;
    (width.saturating_sub(value).saturating_sub(1), value)
}

/// Splits text into display lines no wider than `width`.
///
/// A line breaks after the last word that fits, so words stay whole. A word
/// wider than the column breaks at a grapheme boundary. One grapheme wider
/// than `width` still takes a line of its own, so the split never stalls.
fn wrapped(text: &str, width: u16) -> impl Iterator<Item = &str> {
    let width = usize::from(width).max(1);
    text.split('\n').flat_map(move |line| {
        let mut remaining = Some(line);
        core::iter::from_fn(move || {
            let text = remaining.take()?;
            let (line, rest) = split_line(text, width);
            remaining = rest;
            Some(line)
        })
    })
}

/// Splits the first display line of `text` and returns it with the rest.
///
/// The line keeps whole words while one fits. The rest drops the spaces that
/// separated the words.
fn split_line(text: &str, width: usize) -> (&str, Option<&str>) {
    let end = wrap_line(text, width);
    if end >= text.len() {
        return (text, None);
    }
    let Some(at) = break_at(text, end) else {
        return (&text[..end], Some(&text[end..]));
    };
    let rest = text[at..].trim_start_matches(' ');
    if rest.is_empty() {
        (&text[..at], None)
    } else {
        (&text[..at], Some(rest))
    }
}

/// Returns the offset of the last space inside the first `end` bytes.
///
/// A space at offset zero is not a break, so the line keeps a grapheme and
/// the split always advances.
fn break_at(text: &str, end: usize) -> Option<usize> {
    text[..end].rfind(' ').filter(|at| *at > 0)
}

/// Returns the end byte offset of the first line of `text` that fits `width`.
///
/// The offset is always a grapheme boundary. One grapheme wider than `width`
/// still takes a line of its own, so the result never stalls.
fn wrap_line(text: &str, width: usize) -> usize {
    let mut end = 0;
    let mut columns = 0;
    let mut cursor = 0;
    let span = Span::raw(text);
    for grapheme in span.styled_graphemes(Style::new()) {
        let Some(offset) = text[cursor..].find(grapheme.symbol) else {
            break;
        };
        let start = cursor + offset;
        let stop = start + grapheme.symbol.len();
        let size = Span::raw(grapheme.symbol).width();
        if columns + size > width && end > 0 {
            break;
        }
        end = stop;
        columns += size;
        cursor = stop;
        if columns >= width {
            break;
        }
    }
    if end == 0 { text.len() } else { end }
}

/// Returns the physical height of a model row.
fn row_height(row: &Row, width: u16) -> usize {
    let (_, value) = columns(width);
    if row.kind == RowKind::Info && value > 0 && !row.value.is_empty() {
        wrapped(&row.value, value).count().max(1)
    } else {
        1
    }
}

/// Right-aligns a value inside its fixed column.
///
/// A value wider than the column loses its tail and ends with `~`.
fn put_value(buf: &mut Buffer, area: Rect, text: &str, style: Style) {
    let width = Span::raw(text).width().min(usize::from(area.width)) as u16;
    put(
        buf,
        Rect::new(area.right() - width, area.y, width, 1),
        text,
        style,
    );
}

/// Right-aligns text and drops the tail without a marker.
///
/// The caller draws its own marker, so a wrapped line shows one `~` only.
fn put_clipped(buf: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.is_empty() {
        return;
    }
    let width = Span::raw(text).width().min(usize::from(area.width)) as u16;
    buf.set_stringn(
        area.right() - width,
        area.y,
        text,
        usize::from(width),
        style,
    );
}

/// Left-aligns text and drops the tail without a marker.
///
/// The caller draws its own marker, so a wrapped block shows one `~` only.
fn put_left(buf: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.is_empty() {
        return;
    }
    let width = Span::raw(text).width().min(usize::from(area.width)) as u16;
    buf.set_stringn(area.x, area.y, text, usize::from(width), style);
}

/// Draws one line and marks clipped text with an ASCII tilde.
fn put(buf: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.is_empty() {
        return;
    }
    if Span::raw(text).width() > usize::from(area.width) {
        buf.set_stringn(area.x, area.y, text, usize::from(area.width - 1), style);
        buf.set_stringn(area.right() - 1, area.y, "~", 1, style);
    } else {
        buf.set_stringn(area.x, area.y, text, usize::from(area.width), style);
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    use crate::{Editor, EditorKind, Row, RowKind, View};
    use ratatui_core::backend::TestBackend;
    use ratatui_core::buffer::Buffer;
    use ratatui_core::terminal::Terminal;

    use crate::widgets::{FormsWidget, Theme};

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
        let height = 12;
        // Exceed the whole viewport, not an incidental footer row count.
        let choices: Vec<String> = (0..height * 2).map(|i| format!("Option {i}")).collect();
        let last_choice = choices.last().unwrap().clone();
        let selected = choices.len();
        let editor = choice_editor(choices, selected);
        let view = page(None, Some(editor));
        let buffer = render(&view, 50, height);
        let theme = Theme::default();
        let texts = lines(&buffer);
        let back = texts
            .iter()
            .position(|line| line.contains("< Back"))
            .expect("the Back row stays visible while the list scrolls");
        assert_eq!(selected_rows(&buffer, &theme), [back]);
        assert!(
            texts.iter().any(|line| line.contains(&last_choice)),
            "the last choice sits next to Back: {texts:?}"
        );
        assert!(
            texts.iter().all(|line| !line.contains("Option 0")),
            "the first choices scrolled out: {texts:?}"
        );
        let footer = &texts[usize::from(height) - 2];
        assert_eq!(
            footer.chars().filter(|c| *c == '\u{2502}').count(),
            2,
            "the modal must not cover the page footer: {footer:?}"
        );
        assert!(
            !footer.trim_matches(['\u{2502}', ' ']).is_empty(),
            "the active-editor hints remain visible: {footer:?}"
        );
    }

    /// The page width of a phone in text columns.
    const PHONE: u16 = 53;

    /// Returns `len` characters of `line`, starting at character `start`.
    fn cell_text(line: &str, start: usize, len: usize) -> String {
        line.chars().skip(start).take(len).collect()
    }

    /// Returns the row of the first line that contains `needle`.
    fn row_of(texts: &[String], needle: &str) -> usize {
        texts
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no line contains {needle:?}: {texts:?}"))
    }

    /// Builds a page whose read-only row holds the status text.
    fn status_page(selected: Option<usize>, rows_before: usize) -> View {
        let mut rows: Vec<Row> = (0..rows_before)
            .map(|index| row(&format!("Row {index}"), "set", RowKind::Info))
            .collect();
        rows.push(row(
            "T2 state",
            "T2 has not run in this boot",
            RowKind::Info,
        ));
        View {
            path: Vec::new(),
            title: "Recovery".into(),
            rows,
            selected,
            help: String::new(),
            editor: None,
            message: None,
        }
    }

    #[test]
    fn short_value_ends_at_the_inner_right_edge() {
        let view = page(Some(1), None);
        let buffer = render(&view, PHONE, 18);
        let texts = lines(&buffer);
        let y = row_of(&texts, "Night mode of the panel");
        assert_eq!(cell_text(&texts[y], 1, 2), "Ni", "the prompt starts left");
        let right = usize::from(PHONE) - 2;
        assert_eq!(
            cell_text(&texts[y], right - 2, 3),
            "Off",
            "the value ends at the inner right edge: {:?}",
            texts[y]
        );
        assert_eq!(
            texts[y].chars().nth(right + 1),
            Some('\u{2502}'),
            "the border is intact: {:?}",
            texts[y]
        );
        assert_eq!(
            texts[y].chars().nth(right - 3),
            Some(' '),
            "one blank cell separates the columns: {:?}",
            texts[y]
        );
    }

    #[test]
    fn long_value_is_marked_when_it_is_clipped() {
        let mut view = page(Some(1), None);
        view.rows[1] = row(
            "Night mode of the panel",
            "1440 x 3120 native panel size",
            RowKind::Toggle,
        );
        let buffer = render(&view, PHONE, 18);
        let texts = lines(&buffer);
        let y = row_of(&texts, "Night mode of the panel");
        let right = usize::from(PHONE) - 2;
        assert_eq!(
            texts[y].chars().nth(right),
            Some('~'),
            "a clipped value ends with a marker: {:?}",
            texts[y]
        );
        assert_eq!(
            texts[y].chars().nth(right + 1),
            Some('\u{2502}'),
            "a clipped value stays inside its column: {:?}",
            texts[y]
        );
        assert!(
            texts[y].contains("Night mode"),
            "the prompt keeps its column: {:?}",
            texts[y]
        );
    }

    /// Returns the text of the value column of `count` lines from `y`.
    fn value_lines(texts: &[String], y: usize, count: usize) -> Vec<String> {
        let value = (usize::from(PHONE) - 2) / 2;
        let start = usize::from(PHONE) - 1 - value;
        (0..count)
            .map(|line| cell_text(&texts[y + line], start, value).trim_end().into())
            .collect()
    }

    #[test]
    fn read_only_value_wraps_and_keeps_its_words() {
        let view = status_page(Some(0), 0);
        let buffer = render(&view, PHONE, 14);
        let texts = lines(&buffer);
        let y = row_of(&texts, "T2 state");
        let parts = value_lines(&texts, y, 2);
        assert_eq!(
            parts.join(" "),
            "T2 has not run in this boot",
            "the wrapped value breaks between whole words: {texts:?}"
        );
        let theme = Theme::default();
        assert_eq!(
            selected_rows(&buffer, &theme),
            [y, y + 1],
            "one wrapped row covers both of its lines: {texts:?}"
        );
    }

    #[test]
    fn wrapped_value_starts_at_the_left_edge() {
        let mut view = status_page(Some(0), 0);
        let status = "T2 has not run in this boot. The recovery page waits for it.";
        view.rows[0].value = status.into();
        let buffer = render(&view, PHONE, 14);
        let texts = lines(&buffer);
        let y = row_of(&texts, "T2 state");
        let parts = value_lines(&texts, y, 3);
        assert_eq!(
            parts.join(" "),
            status,
            "every break falls between words: {texts:?}"
        );
        let left = usize::from(PHONE) - 1 - (usize::from(PHONE) - 2) / 2;
        assert_eq!(
            parts[0].as_str(),
            "T2 has not run in this",
            "the first line starts at the left edge: {texts:?}"
        );
        assert_eq!(
            cell_text(&texts[y], left, 2),
            "T2",
            "the block shares one left edge: {texts:?}"
        );
        assert_eq!(
            cell_text(&texts[y + 1], left, 2),
            "bo",
            "the second line shares the left edge: {texts:?}"
        );
        assert!(
            value_lines(&texts, y + 3, 1)[0].is_empty(),
            "the block stops after its last line: {texts:?}"
        );
    }

    #[test]
    fn single_line_value_stays_right_aligned() {
        let mut view = status_page(Some(0), 0);
        view.rows[0].value = "Not run".into();
        let buffer = render(&view, PHONE, 14);
        let texts = lines(&buffer);
        let y = row_of(&texts, "T2 state");
        let right = usize::from(PHONE) - 2;
        assert_eq!(
            cell_text(&texts[y], right - 6, 7),
            "Not run",
            "a short value ends at the inner right edge: {:?}",
            texts[y]
        );
        assert_eq!(
            texts[y].chars().nth(right + 1),
            Some('\u{2502}'),
            "the border is intact: {:?}",
            texts[y]
        );
    }

    #[test]
    fn word_wider_than_the_column_breaks_at_graphemes() {
        let mut view = status_page(Some(0), 0);
        let word = "supercalifragilisticexpialidocious";
        view.rows[0].value = word.into();
        let buffer = render(&view, PHONE, 14);
        let texts = lines(&buffer);
        let y = row_of(&texts, "T2 state");
        let value = (usize::from(PHONE) - 2) / 2;
        let parts = value_lines(&texts, y, 2);
        assert_eq!(
            parts.concat(),
            word,
            "the long word keeps every character: {texts:?}"
        );
        assert_eq!(
            parts[0].chars().count(),
            value,
            "the long word fills the column first: {texts:?}"
        );
        let left = usize::from(PHONE) - 1 - value;
        assert_eq!(
            cell_text(&texts[y + 1], left, 2),
            "li",
            "the rest of the word starts at the left edge: {texts:?}"
        );
    }

    #[test]
    fn wrapped_row_keeps_its_selection_visible() {
        let mut view = status_page(Some(5), 5);
        view.rows[5].value = "T2 has not run in this boot. ".repeat(4);
        let buffer = render(&view, PHONE, 7);
        let theme = Theme::default();
        let texts = lines(&buffer);
        let selected = selected_rows(&buffer, &theme);
        assert!(
            !selected.is_empty(),
            "the selected row is visible: {texts:?}"
        );
        assert_eq!(
            selected.last().unwrap() - selected[0] + 1,
            selected.len(),
            "the selection covers one row: {texts:?}"
        );
        assert!(
            texts[selected[0]].contains("T2 state"),
            "the row starts at its prompt: {texts:?}"
        );
        let last = *selected.last().unwrap();
        assert_eq!(
            texts[last].chars().nth(usize::from(PHONE) - 2),
            Some('~'),
            "the row shows that its value is clipped: {:?}",
            texts[last]
        );
    }

    #[test]
    fn footer_hint_is_centered_with_padding() {
        let height = 18u16;
        let view = page(Some(1), None);
        let buffer = render(&view, PHONE, height);
        let texts = lines(&buffer);
        let width = usize::from(PHONE);
        let line = &texts[usize::from(height) - 2];
        let inner: Vec<char> = line.chars().skip(1).take(width - 2).collect();
        let start = inner
            .iter()
            .position(|cell| *cell != ' ')
            .expect("hints are drawn");
        let end = inner
            .iter()
            .rposition(|cell| *cell != ' ')
            .expect("hints are drawn");
        let left = start;
        let right = inner.len() - 1 - end;
        assert!(left >= 1 && right >= 1, "hints keep edge padding: {line:?}");
        assert!(
            left.abs_diff(right) <= 1,
            "hints are centered: left {left}, right {right} in {line:?}"
        );
        assert_eq!(
            texts[usize::from(height) - 1].chars().nth(1),
            Some('\u{2500}'),
            "the footer sits above the bottom border"
        );
    }

    #[test]
    fn heading_rule_reaches_the_right_edge() {
        let view = page(Some(1), None);
        let buffer = render(&view, PHONE, 18);
        let texts = lines(&buffer);
        let y = row_of(&texts, "Appearance");
        let start = 1 + "Appearance".len();
        assert_eq!(
            buffer[(start as u16, y as u16)].symbol(),
            " ",
            "one blank cell follows the heading text"
        );
        let right = PHONE - 2;
        assert_eq!(
            buffer[(right, y as u16)].symbol(),
            "\u{2500}",
            "the rule reaches the inner right edge: {:?}",
            texts[y]
        );
    }

    #[test]
    fn heading_rule_can_be_turned_off() {
        let theme = Theme {
            heading_rule: false,
            ..Theme::default()
        };
        let view = page(Some(1), None);
        let backend = TestBackend::new(PHONE, 18);
        let mut terminal = Terminal::new(backend).expect("the test backend cannot fail");
        terminal
            .draw(|frame| {
                frame.render_widget(FormsWidget::new(&view).with_theme(theme), frame.area());
            })
            .expect("the test backend cannot fail");
        let buffer = terminal.backend().buffer().clone();
        let texts = lines(&buffer);
        let y = row_of(&texts, "Appearance");
        let right = PHONE - 2;
        assert_eq!(
            buffer[(right, y as u16)].symbol(),
            " ",
            "no rule follows the heading: {:?}",
            texts[y]
        );
    }

    #[test]
    fn empty_value_row_uses_the_full_width() {
        let mut view = page(None, None);
        view.rows.push(row(
            "Restart the recovery menu of this device",
            "",
            RowKind::Action,
        ));
        let buffer = render(&view, PHONE, 22);
        let texts = lines(&buffer);
        let y = row_of(&texts, "Restart the recovery menu");
        assert!(
            texts[y].contains("of this device"),
            "the prompt keeps the columns of the value area: {:?}",
            texts[y]
        );
    }

    #[test]
    fn wide_value_uses_display_columns() {
        let mut view = page(None, None);
        view.rows[3] = row("Panel", "宽屏", RowKind::Info);
        let buffer = render(&view, PHONE, 18);
        let texts = lines(&buffer);
        let y = row_of(&texts, "Panel");
        let right = usize::from(PHONE) - 2;
        assert_eq!(buffer[(right as u16 - 3, y as u16)].symbol(), "宽");
        assert_eq!(buffer[(right as u16 - 1, y as u16)].symbol(), "屏");
        assert_eq!(
            buffer[(right as u16, y as u16)].symbol(),
            " ",
            "a double-width glyph keeps its trailing cell inside the column"
        );
        assert_eq!(buffer[(right as u16 + 1, y as u16)].symbol(), "\u{2502}");
    }
}
