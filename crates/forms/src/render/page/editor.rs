//! Drawing the open editor and the key hints.

use super::row::fit;
use super::*;

impl FormsWidget<'_> {
    /// Draws the open editor as a centered panel.
    pub(super) fn editor_panel(&self, editor: &Editor, area: Rect, buf: &mut Buffer) {
        let theme = &self.theme;
        let lines = editor_lines(editor, theme.current_mark);
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
        // The highlighted line is filled across the panel, as a selected row
        // is on the page.
        let visible = usize::from(inner.height);
        let cursor = lines.iter().position(|line| line.1).unwrap_or(0);
        let first = cursor.saturating_sub(visible.saturating_sub(1));
        for (index, line) in lines.iter().skip(first).take(visible).enumerate() {
            let style = if line.1 {
                theme.selection
            } else {
                Style::new().fg(theme.foreground)
            };
            let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            buf.set_style(row, style);
            let text = fit(&line.0, inner.width, theme.clip, Truncation::End);
            put(buf, row, &text, style);
        }
    }
}

/// Returns the lines of an editor and whether each line is highlighted.
///
/// A choice list marks the stored value with `current`, and pads every other
/// entry by the same width, so the labels stay in one column.
pub(super) fn editor_lines(editor: &Editor, current: &str) -> Vec<(String, bool)> {
    match &editor.kind {
        EditorKind::Choice {
            items,
            selected,
            current: stored,
        } => {
            let pad = " ".repeat(current.chars().count());
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let mark = if index == *stored {
                        current
                    } else {
                        pad.as_str()
                    };
                    (format!("{mark}{item}"), index == *selected)
                })
                .collect()
        }
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
pub(super) fn with_cursor(text: &str, cursor: usize) -> String {
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

/// Returns the key hints of the current page in the keypad's key names.
///
/// Without a Back key the page hint points at the Back row, and an editor
/// names how to leave it unchanged: Enter on the stored value. An editor
/// names only what its own keys do.
pub(super) fn hints(view: &View, keys: Keypad, current: &str) -> String {
    let Keypad {
        move_keys,
        select,
        back,
        ..
    } = keys;
    let cancel = match back {
        Some(back) => format!("   {back} cancel"),
        None => String::new(),
    };
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
            EditorKind::Choice { .. } if back.is_none() => {
                format!(
                    "{move_keys} move   {select} choose   {} current",
                    current.trim_end()
                )
            }
            EditorKind::Choice { .. } => format!("{move_keys} move   {select} choose{cancel}"),
            EditorKind::Slider { .. } | EditorKind::Number { .. } => {
                format!("{move_keys} adjust   {select} accept{cancel}")
            }
            EditorKind::Text { .. } => format!("Type to edit   {select} accept{cancel}"),
        },
        None => match back {
            Some(back) => format!("{move_keys} move   {select} open   {back} back"),
            None => format!("{move_keys} move   {select} open   < Back row: back"),
        },
    }
}
