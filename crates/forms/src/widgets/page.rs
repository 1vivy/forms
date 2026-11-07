//! The widget that draws one setup page.

use alloc::{collections::BTreeMap, format, string::String, string::ToString, vec, vec::Vec};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};
use ratatui_widgets::{block::Block, clear::Clear};

use crate::{
    Decoration, Editor, EditorKind, Key, RowView, Truncation, View,
    widgets::{KeyNames, ModalWidget, Theme},
};

/// Draws the value area of a consumer decoration.
///
/// The function receives the consumer data of the decoration, the row, the
/// value area, the buffer and the theme. It must not write outside the area.
pub type Paint = fn(&[u8], &RowView, Rect, &mut Buffer, &Theme);

/// The painters of consumer decorations, keyed by painter kind.
///
/// # Examples
///
/// ```
/// use forms::widgets::Painters;
///
/// assert!(Painters::new().get(forms::Key(1)).is_none());
/// ```
#[derive(Clone, Default)]
pub struct Painters {
    map: BTreeMap<Key, Paint>,
}

impl Painters {
    /// Creates an empty set of painters.
    ///
    /// # Examples
    ///
    /// ```
    /// let _painters = forms::widgets::Painters::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a painter for one kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::widgets::Painters;
    /// use ratatui_core::buffer::Buffer;
    /// use ratatui_core::layout::Rect;
    ///
    /// fn paint(_data: &[u8], _row: &forms::RowView, _area: Rect, _buf: &mut Buffer, _theme: &forms::widgets::Theme) {}
    /// let mut painters = Painters::new();
    /// painters.insert(forms::Key(4), paint);
    /// assert!(painters.get(forms::Key(4)).is_some());
    /// ```
    pub fn insert(&mut self, kind: Key, paint: Paint) {
        self.map.insert(kind, paint);
    }

    /// Returns the painter of one kind.
    ///
    /// # Examples
    ///
    /// ```
    /// assert!(forms::widgets::Painters::new().get(forms::Key(1)).is_none());
    /// ```
    pub fn get(&self, kind: Key) -> Option<Paint> {
        self.map.get(&kind).copied()
    }
}

/// Draws a [`View`] as a setup page.
///
/// A row with the [`Decoration::Heading`] decoration draws its label and a
/// rule. A row with [`Decoration::Toggle`] draws a switch mark. A row with
/// [`Decoration::Bar`] draws a progress bar. A row with a consumer decoration
/// draws through the painter that the consumer registered. A control without
/// a handler stays visible as an inert row.
///
/// The label column comes from the row [`Presentation`](crate::Presentation)
/// and the theme. A value that does not fit is cut at a character boundary and
/// ends with the clip mark of the theme, unless the row asks for wrapping.
///
/// # Examples
///
/// ```
/// use forms::View;
/// use forms::widgets::FormsWidget;
/// use ratatui_core::buffer::Buffer;
/// use ratatui_core::layout::Rect;
/// use ratatui_core::widgets::Widget;
///
/// let view = View {
///     path: Vec::new(),
///     title: "Display".into(),
///     rows: Vec::new(),
///     selected: None,
///     help: String::new(),
///     editor: None,
///     message: None,
///     modal: None,
/// };
/// let area = Rect::new(0, 0, 40, 12);
/// let mut buffer = Buffer::empty(area);
/// FormsWidget::new(&view).render(area, &mut buffer);
/// ```
pub struct FormsWidget<'a> {
    view: &'a View,
    theme: Theme,
    painters: Painters,
}

impl<'a> FormsWidget<'a> {
    /// Creates a widget with the default theme and no painters.
    ///
    /// # Examples
    ///
    /// ```
    /// let view = forms::Session::<u32>::new(Vec::new())
    ///     .with_back_row(false);
    /// ```
    #[must_use]
    pub fn new(view: &'a View) -> Self {
        Self {
            view,
            theme: Theme::default(),
            painters: Painters::default(),
        }
    }

    /// Replaces the theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::widgets::{FormsWidget, Theme};
    ///
    /// let view = forms::View {
    ///     path: Vec::new(),
    ///     title: String::new(),
    ///     rows: Vec::new(),
    ///     selected: None,
    ///     help: String::new(),
    ///     editor: None,
    ///     message: None,
    ///     modal: None,
    /// };
    /// let widget = FormsWidget::new(&view).with_theme(Theme::default());
    /// ```
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Replaces the painters of consumer decorations.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::widgets::{FormsWidget, Painters};
    ///
    /// let view = forms::View {
    ///     path: Vec::new(),
    ///     title: String::new(),
    ///     rows: Vec::new(),
    ///     selected: None,
    ///     help: String::new(),
    ///     editor: None,
    ///     message: None,
    ///     modal: None,
    /// };
    /// let widget = FormsWidget::new(&view).with_painters(Painters::new());
    /// ```
    #[must_use]
    pub fn with_painters(mut self, painters: Painters) -> Self {
        self.painters = painters;
        self
    }
}

impl Widget for FormsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_style(area, self.theme.page());
        if area.width < 4 || area.height < 4 {
            return;
        }
        let theme = &self.theme;
        let frame = Block::bordered()
            .title(format!(" {} ", self.view.title))
            .title_style(Style::new().fg(theme.title).add_modifier(Modifier::BOLD))
            .border_style(Style::new().fg(theme.border))
            .style(theme.page());
        let inner = frame.inner(area);
        frame.render(area, buf);
        if inner.is_empty() {
            return;
        }
        let mut list = inner;
        if !self.view.path.is_empty() && inner.height > 2 {
            let breadcrumb = Rect::new(inner.x, inner.y, inner.width, 1);
            let mut text = String::new();
            for part in &self.view.path {
                if !text.is_empty() {
                    text.push_str(" > ");
                }
                text.push_str(part);
            }
            put(buf, breadcrumb, &text, Style::new().fg(theme.help));
            list.y += 1;
            list.height -= 1;
        }
        let mut footer = None;
        if inner.height > 3 {
            let line = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
            put(
                buf,
                line,
                &hints(self.view, theme.keys),
                Style::new().fg(theme.hint),
            );
            footer = Some(line);
            list.height = list.height.saturating_sub(1);
        }
        if let Some(message) = self.view.message.as_deref()
            && list.height > 2
        {
            let line = Rect::new(inner.x, list.bottom() - 1, inner.width, 1);
            put(buf, line, message, Style::new().fg(theme.message));
            list.height = list.height.saturating_sub(1);
        }
        let help_height = if list.height >= 6 {
            (list.height / 3).min(3)
        } else {
            0
        };
        let rows_area = Rect::new(
            list.x,
            list.y,
            list.width,
            list.height.saturating_sub(help_height),
        );
        if help_height > 0 {
            let help = Rect::new(list.x, rows_area.bottom(), list.width, help_height);
            put(buf, help, &self.view.help, Style::new().fg(theme.help));
        }
        self.rows(rows_area, buf);
        let overlay = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            footer.map_or(inner.height, |line| line.y.saturating_sub(inner.y)),
        );
        if let Some(editor) = &self.view.editor {
            self.editor_panel(editor, overlay, buf);
        }
        if let Some(modal) = &self.view.modal {
            ModalWidget::new(modal)
                .with_theme(theme.clone())
                .render(overlay, buf);
        }
    }
}

impl FormsWidget<'_> {
    /// Draws the visible rows, scrolled to keep the selection visible.
    fn rows(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let theme = &self.theme;
        let selected = self.view.selected.unwrap_or(0);
        let heights: Vec<u16> = self
            .view
            .rows
            .iter()
            .map(|row| row_height(row, area.width, theme))
            .collect();
        let mut offset = 0;
        let mut height = 0u32;
        for row in heights.iter().take(selected.saturating_add(1)) {
            height += u32::from(*row);
        }
        while height > u32::from(area.height) && offset < selected {
            height -= u32::from(heights[offset]);
            offset += 1;
        }
        let mut y = area.y;
        for (index, row) in self.view.rows.iter().enumerate().skip(offset) {
            if y >= area.bottom() {
                break;
            }
            let height = heights[index].min(area.bottom() - y);
            let row_area = Rect::new(area.x, y, area.width, height);
            self.row(row, row_area, buf, self.view.selected == Some(index));
            y += height;
        }
    }

    /// Draws one row.
    fn row(&self, row: &RowView, area: Rect, buf: &mut Buffer, selected: bool) {
        if area.is_empty() {
            return;
        }
        let theme = &self.theme;
        let mut style = theme.style(row.decoration.clone());
        if !row.enabled {
            style = style.patch(theme.disabled);
        }
        if row.uncertain {
            style = style.patch(theme.uncertain);
        }
        if selected {
            style = style.patch(if row.enabled {
                theme.selection
            } else {
                theme.gray_selected
            });
        }
        buf.set_style(area, style);
        match &row.decoration {
            Decoration::Rule => {
                let line: String =
                    core::iter::repeat_n(theme.divider, usize::from(area.width)).collect();
                put(buf, area, &line, style);
                return;
            }
            Decoration::Heading => {
                put(buf, area, &row.label, style);
                if theme.heading_rule {
                    let start = row.label.chars().count().saturating_add(2);
                    if start < usize::from(area.width) {
                        let line: String =
                            core::iter::repeat_n(theme.divider, usize::from(area.width) - start)
                                .collect();
                        put(
                            buf,
                            Rect::new(area.x + start as u16, area.y, area.width - start as u16, 1),
                            &line,
                            style,
                        );
                    }
                }
                return;
            }
            _ => {}
        }
        // A link row ends with the link mark at the right edge; its value, if
        // any, sits before the mark. Other rows use the whole value column.
        let mark = if row.link { theme.link_mark } else { "" };
        let mark_width = u16::try_from(mark.chars().count())
            .unwrap_or(0)
            .min(area.width);
        let inner = Rect::new(area.x, area.y, area.width - mark_width, area.height);
        if mark_width > 0 {
            put(
                buf,
                Rect::new(inner.right(), area.y, mark_width, 1),
                mark,
                style,
            );
        }
        let (label_width, value_width) = columns(row, inner.width, theme);
        let label = fit(&row.label, label_width, theme.clip, Truncation::End);
        put(
            buf,
            Rect::new(area.x, area.y, label_width, area.height),
            &label,
            style,
        );
        if value_width == 0 {
            return;
        }
        let value_area = Rect::new(
            inner.right() - value_width,
            area.y,
            value_width,
            area.height,
        );
        match &row.decoration {
            Decoration::Toggle(on) => {
                let mark = if *on {
                    theme.toggle_on
                } else {
                    theme.toggle_off
                };
                let text = format!("{mark}{}", row.value);
                put_fit(buf, value_area, &text, style, row, theme);
            }
            Decoration::Bar { current, total } => {
                draw_bar(*current, *total, value_area, buf, style, theme);
            }
            Decoration::Slider {
                position,
                minimum,
                maximum,
            } => {
                draw_slider(*position, *minimum, *maximum, value_area, buf, style, theme);
                let text = fit(&row.value, value_area.width, theme.clip, Truncation::End);
                let width = u16::try_from(text.chars().count())
                    .unwrap_or(0)
                    .min(value_area.width);
                put(
                    buf,
                    Rect::new(value_area.right() - width, area.y, width, 1),
                    &text,
                    style,
                );
            }
            Decoration::Custom { kind, data } => match self.painters.get(*kind) {
                Some(paint) => paint(data, row, value_area, buf, theme),
                None => put_fit(buf, value_area, "<custom>", style, row, theme),
            },
            Decoration::Plain | Decoration::Heading | Decoration::Rule => {
                if row.presentation.truncation == Truncation::Wrap {
                    let lines: Vec<String> = wrap(&row.value, value_width);
                    for (index, line) in lines.iter().take(usize::from(area.height)).enumerate() {
                        put(
                            buf,
                            Rect::new(value_area.x, area.y + index as u16, value_width, 1),
                            line,
                            style,
                        );
                    }
                } else {
                    put_fit(buf, value_area, &row.value, style, row, theme);
                }
            }
        }
    }

    /// Draws the open editor as a centered panel.
    fn editor_panel(&self, editor: &Editor, area: Rect, buf: &mut Buffer) {
        let theme = &self.theme;
        let lines = editor_lines(editor);
        let width = area.width.saturating_sub(2).clamp(12, 56).min(area.width);
        let height = (lines.len() as u16 + 2).min(area.height);
        if width < 4 || height < 3 {
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
            .title(format!(" {} ", editor.label))
            .title_style(Style::new().fg(theme.title).add_modifier(Modifier::BOLD))
            .border_style(Style::new().fg(theme.border))
            .style(theme.page());
        let inner = block.inner(panel);
        block.render(panel, buf);
        // A list longer than the panel scrolls so the highlighted line stays in
        // view; a cursor that wrapped to the last entry is drawn at the bottom.
        let visible = usize::from(inner.height);
        let cursor = lines.iter().position(|line| line.1).unwrap_or(0);
        let first = cursor.saturating_sub(visible.saturating_sub(1));
        for (index, line) in lines.iter().skip(first).take(visible).enumerate() {
            let style = if line.1 {
                Style::new()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme.foreground)
            };
            let text = fit(&line.0, inner.width, theme.clip, Truncation::End);
            put(
                buf,
                Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
                &text,
                style,
            );
        }
    }
}

/// Returns the lines of an editor and whether each line is highlighted.
fn editor_lines(editor: &Editor) -> Vec<(String, bool)> {
    match &editor.kind {
        EditorKind::Choice { items, selected } => items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                (
                    format!("{} {}", if index == *selected { '>' } else { ' ' }, item),
                    index == *selected,
                )
            })
            .collect(),
        EditorKind::Number { text, cursor } => {
            vec![(with_cursor(text, *cursor), true)]
        }
        EditorKind::Text {
            text,
            cursor,
            secret,
        } => {
            let shown = if *secret {
                "*".repeat(text.chars().count())
            } else {
                text.clone()
            };
            vec![(with_cursor(&shown, *cursor), true)]
        }
        EditorKind::Slider {
            text,
            position,
            minimum,
            maximum,
        } => {
            let span = maximum.saturating_sub(*minimum);
            let offset = position.saturating_sub(*minimum);
            let cells = 16u64;
            let filled = offset
                .checked_mul(cells)
                .and_then(|product| product.checked_div(span))
                .unwrap_or(0);
            let bar = format!(
                "{}{}",
                "#".repeat(filled as usize),
                "-".repeat((cells - filled) as usize)
            );
            vec![(format!("[{bar}] {text}"), true)]
        }
        EditorKind::List {
            items,
            selected,
            chosen,
            held,
        } => items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let marker = if index < *chosen { '*' } else { ' ' };
                let cursor = match (index == *selected, *held) {
                    (true, true) => '=',
                    (true, false) => '>',
                    _ => ' ',
                };
                (format!("{cursor} {marker} {item}"), index == *selected)
            })
            .collect(),
    }
}

/// Returns the text with a cursor mark.
fn with_cursor(text: &str, cursor: usize) -> String {
    let mut out = String::new();
    for (index, character) in text.chars().enumerate() {
        if index == cursor {
            out.push('|');
        }
        out.push(character);
    }
    if cursor >= text.chars().count() {
        out.push('|');
    }
    out
}

/// Returns the key hints of the current page in the theme's key names.
///
/// Without a Back key the page hint points at the Back row, and an editor
/// names only what its own keys do.
fn hints(view: &View, keys: KeyNames) -> String {
    let KeyNames {
        move_keys,
        select,
        back,
    } = keys;
    let cancel = back.map_or_else(String::new, |back| format!("   {back} cancel"));
    if view.modal.is_some() {
        return format!("{move_keys} move   {select} choose{cancel}");
    }
    match &view.editor {
        Some(editor) => match editor.kind {
            EditorKind::List { held: true, .. } => {
                format!("{move_keys} move entry   {select} drop")
            }
            EditorKind::List { .. } => {
                format!("{move_keys} move   {select} pick up, add or done{cancel}")
            }
            EditorKind::Slider { .. } | EditorKind::Number { .. } => {
                format!("{move_keys} adjust   {select} accept{cancel}")
            }
            _ => format!("Type to edit   {select} accept{cancel}"),
        },
        None => match back {
            Some(back) => format!("{move_keys} move   {select} open   {back} back"),
            None => format!("{move_keys} move   {select} open   < Back row: back"),
        },
    }
}

/// Returns the label and value columns of a row.
fn columns(row: &RowView, width: u16, theme: &Theme) -> (u16, u16) {
    let limit = row
        .presentation
        .label_limit
        .filter(|limit| *limit > 0)
        .unwrap_or(theme.label_limit);
    let label = if limit > 0 {
        limit.min(width.saturating_sub(2))
    } else {
        u16::try_from(u32::from(width) * u32::from(theme.split) / 100).unwrap_or(width)
    };
    // A row without a value has no second column, so its label takes the
    // whole width instead of stopping at the split.
    if row.value.is_empty() && row.decoration == Decoration::Plain {
        return (width, 0);
    }
    let label = label.min(width.saturating_sub(2));
    (label, width.saturating_sub(label + 1))
}

/// Returns the height of a row in cells.
fn row_height(row: &RowView, width: u16, theme: &Theme) -> u16 {
    if row.presentation.truncation == Truncation::Wrap {
        let (_, value) = columns(row, width, theme);
        if value > 0 {
            let lines = wrap(&row.value, value);
            return u16::try_from(lines.len().max(1)).unwrap_or(1);
        }
    }
    1
}

/// Draws a progress bar in the value area.
fn draw_bar(
    current: u64,
    total: Option<u64>,
    area: Rect,
    buf: &mut Buffer,
    style: Style,
    theme: &Theme,
) {
    let width = usize::from(area.width);
    if width == 0 {
        return;
    }
    let (filled, label) = match total {
        Some(total) if total > 0 => {
            let filled =
                usize::try_from(current.min(total) * width as u64 / total).unwrap_or(width);
            (filled, format!("{}/{}", current.min(total), total))
        }
        _ => (width / 2, String::from("...")),
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
    put(buf, area, &bar, style);
    let label_width = u16::try_from(label.chars().count())
        .unwrap_or(0)
        .min(area.width);
    put(
        buf,
        Rect::new(area.right() - label_width, area.y, label_width, 1),
        &label,
        style,
    );
}

/// Draws a slider bar with its position in the value area.
fn draw_slider(
    position: u64,
    minimum: u64,
    maximum: u64,
    area: Rect,
    buf: &mut Buffer,
    style: Style,
    theme: &Theme,
) {
    let width = usize::from(area.width);
    if width == 0 {
        return;
    }
    let span = maximum.saturating_sub(minimum);
    let offset = position.clamp(minimum, maximum).saturating_sub(minimum);
    let filled = offset
        .checked_mul(width as u64)
        .and_then(|product| product.checked_div(span))
        .map_or(width, |value| usize::try_from(value).unwrap_or(width));
    let bar: String = (0..width)
        .map(|index| {
            if index < filled {
                theme.bar_full
            } else {
                theme.bar_empty
            }
        })
        .collect();
    put(buf, area, &bar, style);
}

/// Draws text in an area, cut to the width of the value and the row policy.
fn put_fit(buf: &mut Buffer, area: Rect, text: &str, style: Style, row: &RowView, theme: &Theme) {
    let truncation = match row.presentation.truncation {
        Truncation::Theme => theme.truncation,
        other => other,
    };
    let text = fit(text, area.width, theme.clip, truncation);
    let width = u16::try_from(text.chars().count())
        .unwrap_or(0)
        .min(area.width);
    let left = match theme.value_alignment {
        crate::widgets::Alignment::Left => area.x,
        crate::widgets::Alignment::Right => area.right() - width,
    };
    put(buf, Rect::new(left, area.y, width, 1), &text, style);
}

/// Writes text at the start of an area.
fn put(buf: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.is_empty() {
        return;
    }
    for (x, character) in (area.x..area.right()).zip(text.chars()) {
        buf[(x, area.y)].set_char(character).set_style(style);
    }
}

/// Cuts text to a width, at a character boundary.
fn fit(text: &str, width: u16, clip: &str, truncation: Truncation) -> String {
    let width = usize::from(width);
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let keep = match truncation {
        Truncation::Clip => width,
        _ => width.saturating_sub(clip.chars().count()).min(width),
    };
    let mut out: String = text.chars().take(keep).collect();
    if !matches!(truncation, Truncation::Clip) {
        out.push_str(clip);
    }
    out
}

/// Wraps text at word boundaries within a width.
fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width).max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(core::mem::take(&mut line));
            line.push_str(word);
        }
        while line.chars().count() > width {
            let mut rest = line.chars().collect::<String>();
            let head: String = rest.chars().take(width).collect();
            rest = rest.chars().skip(width).collect();
            lines.push(head);
            line = rest;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::FormsWidget;
    use crate::widgets::{Alignment, Painters, Theme};
    use crate::{Decoration, Presentation, RowView, Truncation, View};
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use ratatui_core::buffer::Buffer;
    use ratatui_core::layout::Rect;
    use ratatui_core::style::Color;
    use ratatui_core::widgets::Widget;

    /// Builds a page with one row.
    fn view(enabled: bool) -> View {
        View {
            path: Vec::new(),
            title: String::from("Setup"),
            rows: vec![RowView {
                key: crate::Key(1),
                label: String::from("Mode"),
                value: String::from("On"),
                help: String::new(),
                decoration: Decoration::Toggle(true),
                selectable: true,
                link: false,
                enabled,
                uncertain: false,
                presentation: Presentation {
                    label_limit: None,
                    truncation: Truncation::End,
                },
            }],
            selected: Some(0),
            help: String::new(),
            editor: None,
            message: None,
            modal: None,
        }
    }

    /// Returns the row that contains `text`.
    fn row_of(buffer: &Buffer, text: &str) -> Option<u16> {
        (0..buffer.area.height).find(|y| {
            let line: alloc::string::String = (0..buffer.area.width)
                .map(|x| buffer[(x, *y)].symbol())
                .collect();
            line.contains(text)
        })
    }

    /// Returns the column of the first value character of `text`.
    fn column(buffer: &Buffer, text: &str) -> Option<u16> {
        for y in 0..buffer.area.height {
            let line: alloc::string::String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if line.contains("Mode")
                && let Some(byte) = line.find(text)
            {
                return u16::try_from(byte).ok();
            }
        }
        None
    }

    #[test]
    fn the_default_theme_keeps_values_right_aligned() {
        let view = view(true);
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        FormsWidget::new(&view)
            .with_painters(Painters::new())
            .render(area, &mut buffer);
        // The value ends at the last cell before the right border.
        let y = row_of(&buffer, "Mode").expect("the row draws");
        let last = area.width - 2;
        assert_eq!(buffer[(last, y)].symbol(), "n");
    }

    /// Returns the text of the screen line that holds `text`.
    fn line_of(buffer: &Buffer, text: &str) -> String {
        let y = row_of(buffer, text).expect("the row draws");
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn only_a_link_row_ends_with_the_link_mark_at_the_right_edge() {
        let mut page = view(true);
        let mut link = page.rows[0].clone();
        link.key = crate::Key(2);
        link.label = String::from("Boot");
        link.value = String::new();
        link.decoration = Decoration::Plain;
        link.link = true;
        page.rows.push(link);
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        FormsWidget::new(&page).render(area, &mut buffer);
        let last = usize::from(area.width - 2);
        let boot: Vec<char> = line_of(&buffer, "Boot").chars().collect();
        assert_eq!(
            boot[last], '>',
            "the link mark sits before the right border"
        );
        let mode = line_of(&buffer, "Mode");
        assert!(!mode.contains('>'), "a toggle is not a link: {mode:?}");
    }

    #[test]
    fn a_long_label_ends_with_the_clip_mark() {
        let mut page = view(true);
        page.rows[0].label = String::from("A label much longer than its half of the row");
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        let theme = Theme {
            clip: "…",
            ..Theme::default()
        };
        FormsWidget::new(&page)
            .with_theme(theme)
            .render(area, &mut buffer);
        assert!(line_of(&buffer, "A label").contains('…'));
    }

    #[test]
    fn a_left_alignment_moves_the_value_to_its_column_start() {
        let view = view(true);
        let theme = Theme {
            value_alignment: Alignment::Left,
            ..Theme::default()
        };
        let area = Rect::new(0, 0, 40, 8);
        let mut right = Buffer::empty(area);
        let mut left = Buffer::empty(area);
        FormsWidget::new(&view).render(area, &mut right);
        FormsWidget::new(&view)
            .with_theme(theme)
            .render(area, &mut left);
        let right_column = column(&right, "On").expect("the value draws");
        let left_column = column(&left, "On").expect("the value draws");
        assert!(left_column < right_column);
        let y = row_of(&left, "Mode").expect("the row draws");
        assert_ne!(left[(area.width - 2, y)].symbol(), "n");
    }

    #[test]
    fn a_selected_row_that_accepts_no_change_uses_the_gray_style() {
        let view = view(false);
        let theme = Theme::default();
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        FormsWidget::new(&view)
            .with_theme(theme.clone())
            .render(area, &mut buffer);
        let y = (0..area.height)
            .find(|y| {
                let line: alloc::string::String =
                    (0..area.width).map(|x| buffer[(x, *y)].symbol()).collect();
                line.contains("Mode")
            })
            .expect("the row draws");
        assert_eq!(
            buffer[(1, y)].bg,
            theme.gray_selected.bg.unwrap_or(Color::Reset)
        );
        assert_ne!(buffer[(1, y)].bg, Color::Reset);
    }
}

#[cfg(test)]
mod slider_tests {
    use super::FormsWidget;
    use crate::widgets::Theme;
    use crate::{Decoration, Presentation, RowView, Truncation, View};
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use ratatui_core::buffer::Buffer;
    use ratatui_core::layout::Rect;
    use ratatui_core::widgets::Widget;

    /// Returns the line of the slider row.
    fn line(position: u64) -> Vec<String> {
        let view = View {
            path: Vec::new(),
            title: String::from("Setup"),
            rows: vec![RowView {
                key: crate::Key(1),
                label: String::from("Fan"),
                value: alloc::format!("{position} s"),
                help: String::new(),
                decoration: Decoration::Slider {
                    position,
                    minimum: 0,
                    maximum: 3,
                },
                selectable: true,
                link: false,
                enabled: true,
                uncertain: false,
                presentation: Presentation {
                    label_limit: None,
                    truncation: Truncation::End,
                },
            }],
            selected: Some(0),
            help: String::new(),
            editor: None,
            message: None,
            modal: None,
        };
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        FormsWidget::new(&view).render(area, &mut buffer);
        let y = (0..area.height)
            .find(|y| {
                let text: String = (0..area.width).map(|x| buffer[(x, *y)].symbol()).collect();
                text.contains("Fan")
            })
            .expect("the row draws");
        (0..area.width)
            .map(|x| String::from(buffer[(x, y)].symbol()))
            .collect()
    }

    /// The bar of the row follows the position.
    #[test]
    fn the_slider_bar_follows_the_position() {
        let low = line(0);
        let high = line(3);
        assert_ne!(low, high);
        let full = String::from(Theme::default().bar_full);
        let low_full = low.iter().filter(|symbol| **symbol == full).count();
        let high_full = high.iter().filter(|symbol| **symbol == full).count();
        assert!(high_full > low_full);
    }
}

#[cfg(test)]
mod editor_tests {
    use super::FormsWidget;
    use crate::{Editor, EditorKind, Key, View};
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;
    use ratatui_core::buffer::Buffer;
    use ratatui_core::layout::Rect;
    use ratatui_core::widgets::Widget;

    /// The screen text of a page with a choice editor of 40 options, the
    /// cursor on `selected`, drawn into a panel shorter than the list.
    fn screen(selected: usize) -> String {
        let items: Vec<String> = (0..40).map(|index| format!("item{index:02}")).collect();
        let view = View {
            path: Vec::new(),
            title: String::from("Setup"),
            rows: Vec::new(),
            selected: None,
            help: String::new(),
            editor: Some(Editor {
                row: Key(1),
                label: String::from("Target"),
                kind: EditorKind::Choice { items, selected },
            }),
            message: None,
            modal: None,
        };
        let area = Rect::new(0, 0, 40, 16);
        let mut buffer = Buffer::empty(area);
        FormsWidget::new(&view).render(area, &mut buffer);
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .map(|position| String::from(buffer[position].symbol()))
            .collect()
    }

    /// A cursor past the bottom of the panel scrolls the list so it is drawn.
    #[test]
    fn a_long_list_scrolls_to_the_cursor() {
        assert!(screen(0).contains("item00"));
        let last = screen(39);
        assert!(last.contains("item39"), "the wrapped cursor is in view");
        assert!(!last.contains("item00"), "the list scrolled");
    }
}
