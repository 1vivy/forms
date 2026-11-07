//! The widget that draws one setup page.
//!
//! `mod.rs` holds the widget and its frame; `row` draws one row and fits its
//! text; `editor` draws the open editor and the key hints.

mod editor;
mod row;
#[cfg(test)]
mod tests;

use editor::hints;
use row::{put, row_height};

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
    render::{KeyNames, ModalWidget, Theme},
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
/// use forms::render::Painters;
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
    /// let _painters = forms::render::Painters::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a painter for one kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::render::Painters;
    /// use ratatui_core::buffer::Buffer;
    /// use ratatui_core::layout::Rect;
    ///
    /// fn paint(_data: &[u8], _row: &forms::RowView, _area: Rect, _buf: &mut Buffer, _theme: &forms::render::Theme) {}
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
    /// assert!(forms::render::Painters::new().get(forms::Key(1)).is_none());
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
/// use forms::render::FormsWidget;
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
    /// use forms::render::{FormsWidget, Theme};
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
    /// use forms::render::{FormsWidget, Painters};
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
}
