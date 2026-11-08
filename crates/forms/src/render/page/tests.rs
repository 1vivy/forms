//! Rendering decisions: rows, the slider bar and the editor panel.

mod rows {
    use super::super::FormsWidget;
    use crate::render::{Alignment, Painters, Theme};
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

mod slider {
    use super::super::FormsWidget;
    use crate::render::Theme;
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

mod editor {
    use super::super::FormsWidget;
    use crate::{Editor, EditorKind, Key, View};
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;
    use ratatui_core::buffer::Buffer;
    use ratatui_core::layout::Rect;
    use ratatui_core::widgets::Widget;

    /// The screen lines of a page with a choice editor of 40 options, the
    /// cursor on `selected` and the stored value on `current`, drawn into a
    /// panel shorter than the list.
    fn screen(selected: usize, current: usize) -> (Vec<String>, Buffer) {
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
                kind: EditorKind::Choice {
                    items,
                    selected,
                    current,
                },
            }),
            message: None,
            modal: None,
        };
        let area = Rect::new(0, 0, 40, 16);
        let mut buffer = Buffer::empty(area);
        FormsWidget::new(&view).render(area, &mut buffer);
        let lines = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| String::from(buffer[(x, y)].symbol()))
                    .collect()
            })
            .collect();
        (lines, buffer)
    }

    /// A cursor past the bottom of the panel scrolls the list so it is drawn.
    #[test]
    fn a_long_list_scrolls_to_the_cursor() {
        let (first, _) = screen(0, 0);
        assert!(first.iter().any(|line| line.contains("item00")));
        let (last, _) = screen(39, 0);
        assert!(
            last.iter().any(|line| line.contains("item39")),
            "the wrapped cursor is in view"
        );
        assert!(
            !last.iter().any(|line| line.contains("item00")),
            "the list scrolled"
        );
    }

    /// The stored value carries the current mark, and the cursor line is
    /// filled with the selection style, also where it holds no text.
    #[test]
    fn the_picker_marks_the_stored_value_and_fills_the_cursor() {
        let theme = crate::render::Theme::default();
        let (lines, buffer) = screen(2, 1);
        assert!(lines.iter().any(|line| line.contains("* item01")));
        assert!(lines.iter().any(|line| line.contains("  item02")));
        let y = lines
            .iter()
            .position(|line| line.contains("item02"))
            .expect("the cursor line");
        let line = &lines[y];
        let x = line
            .char_indices()
            .position(|(index, _)| line[index..].starts_with("item02"))
            .expect("the label")
            + 12;
        assert_eq!(buffer[(x as u16, y as u16)].bg, theme.selection.bg.unwrap());
    }
}

mod help {
    use super::super::FormsWidget;
    use crate::View;
    use alloc::string::String;
    use alloc::vec::Vec;
    use ratatui_core::buffer::Buffer;
    use ratatui_core::layout::Rect;
    use ratatui_core::widgets::Widget;

    /// Help longer than one line wraps at word boundaries in the help box.
    #[test]
    fn long_help_wraps() {
        let view = View {
            path: Vec::new(),
            title: String::from("Setup"),
            rows: Vec::new(),
            selected: None,
            help: String::from("Start something once, or change what starts by default"),
            editor: None,
            message: None,
            modal: None,
        };
        let area = Rect::new(0, 0, 30, 24);
        let mut buffer = Buffer::empty(area);
        FormsWidget::new(&view).render(area, &mut buffer);
        let lines: Vec<String> = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| String::from(buffer[(x, y)].symbol()))
                    .collect()
            })
            .collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Start something once, or"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("change what starts by"))
        );
        assert!(lines.iter().any(|line| line.contains("default")));
    }
}
