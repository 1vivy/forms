//! Shows a consumer control, a consumer painter and a theme override.
//!
//! The crate has no privileged path for the built-in kinds: every kind is a
//! handler in a [`forms::Registry`], so a consumer adds a control by
//! registering a handler and a painter. This example adds a gauge, changes the
//! selection colours of the theme, and draws the page once.
//!
//! ```sh
//! cargo run -p forms-examples --example custom
//! ```

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use forms::RowView;
use forms::widgets::{Painters, Theme};
use forms::{Builder, Control, Decoration, Field, Handler, Key, Registry, Session};
use forms_examples::HANDLE;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Color;

use crate::support::Mode;

/// The registered kind of the gauge control.
const GAUGE: Key = Key(0x100);

/// Draws a percentage as a block bar.
struct GaugeHandler;

impl Handler for GaugeHandler {
    fn format(&self, field: &Field<'_>) -> String {
        match &field.row.control {
            Control::Custom { data, .. } => {
                let percent = data.first().copied().unwrap_or(0);
                format!("{percent}%")
            }
            _ => String::new(),
        }
    }

    fn decoration(&self, field: &Field<'_>) -> Decoration {
        match &field.row.control {
            Control::Custom { kind, data } => Decoration::Custom {
                kind: *kind,
                data: data.clone(),
            },
            _ => Decoration::Plain,
        }
    }
}

/// Draws the gauge bar of the value area.
fn paint(data: &[u8], _row: &RowView, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let percent = u64::from(data.first().copied().unwrap_or(0)).min(100);
    let width = usize::from(area.width);
    let filled = usize::try_from(percent * width as u64 / 100).unwrap_or(width);
    let bar: String = (0..width)
        .map(|index| {
            if index < filled {
                theme.bar_full
            } else {
                theme.bar_empty
            }
        })
        .collect();
    for (x, character) in (area.x..area.right()).zip(bar.chars()) {
        buf[(x, area.y)].set_char(character);
    }
}

/// Builds the page and the registry of the example.
fn page() -> (forms::FormSet, Registry) {
    let mut builder = Builder::new(Key(2), "Diagnostics");
    builder.page("Status");
    builder.row("Firmware", Control::Text(String::from("1.4")));
    builder.row(
        "Battery",
        Control::Custom {
            kind: GAUGE,
            data: vec![64],
        },
    );
    builder
        .row(
            "Fan",
            Control::Slider {
                minimum: 0,
                maximum: 10,
                step: 1,
                unit: String::from("s"),
                labels: Vec::new(),
            },
        )
        .set_binding(Key(90));
    let mut registry = Registry::builtin();
    registry.insert(GAUGE, Box::new(GaugeHandler));
    (builder.build(), registry)
}

fn main() -> std::io::Result<()> {
    let (set, registry) = page();
    let mut store = forms::MemoryProvider::new(vec![(HANDLE, set.clone())]);
    store.set(Key(90), forms::Value::Number(4));
    let mut session = Session::new(vec![(HANDLE, set)]).with_registry(registry);
    let _ = session.enter(Key(2));

    let mut painters = Painters::new();
    painters.insert(GAUGE, paint);
    let theme = Theme {
        selection: ratatui_core::style::Style::new()
            .fg(Color::Black)
            .bg(Color::LightGreen),
        ..Theme::default()
    };

    match support::mode(std::env::args().skip(1)) {
        Mode::Once { columns, rows } => {
            support::print_frame_styled(&mut session, &mut store, columns, rows, &theme, &painters)
        }
        _ => {
            support::print_frame_styled(&mut session, &mut store, 56, 18, &theme, &painters)?;
            let view = session.view(&mut store);
            println!("consumer rows = {}", view.rows.len());
            Ok(())
        }
    }
}
