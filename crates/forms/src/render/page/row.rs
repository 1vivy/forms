//! Drawing one row: its label, its value column and the helpers that fit text.

use super::*;

impl FormsWidget<'_> {
    /// Draws one row.
    pub(super) fn row(&self, row: &RowView, area: Rect, buf: &mut Buffer, selected: bool) {
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
}

/// Returns the label and value columns of a row.
pub(super) fn columns(row: &RowView, width: u16, theme: &Theme) -> (u16, u16) {
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
pub(super) fn row_height(row: &RowView, width: u16, theme: &Theme) -> u16 {
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
pub(super) fn draw_bar(
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
pub(super) fn draw_slider(
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
pub(super) fn put_fit(
    buf: &mut Buffer,
    area: Rect,
    text: &str,
    style: Style,
    row: &RowView,
    theme: &Theme,
) {
    let truncation = match row.presentation.truncation {
        Truncation::Theme => theme.truncation,
        other => other,
    };
    let text = fit(text, area.width, theme.clip, truncation);
    let width = u16::try_from(text.chars().count())
        .unwrap_or(0)
        .min(area.width);
    let left = match theme.value_alignment {
        crate::render::Alignment::Left => area.x,
        crate::render::Alignment::Right => area.right() - width,
    };
    put(buf, Rect::new(left, area.y, width, 1), &text, style);
}

/// Writes text at the start of an area.
pub(super) fn put(buf: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.is_empty() {
        return;
    }
    for (x, character) in (area.x..area.right()).zip(text.chars()) {
        buf[(x, area.y)].set_char(character).set_style(style);
    }
}

/// Cuts text to a width, at a character boundary.
pub(super) fn fit(text: &str, width: u16, clip: &str, truncation: Truncation) -> String {
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
pub(super) fn wrap(text: &str, width: u16) -> Vec<String> {
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
