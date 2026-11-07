//! Draws one setup page in a terminal.
//!
//! ```sh
//! cargo run -p forms --example host
//! cargo run -p forms --example host -- --once --columns 53 --rows 20
//! cargo run -p forms --example host -- --once --open
//! ```
//!
//! The example holds its values in a [`forms::MemoryProvider`], so it needs no
//! firmware. `--once` draws one frame and prints it. `--open` also opens the
//! menu timeout slider. Without a flag the program reads keys in a terminal.

use std::io::{self, Write};

use crossterm::event::{self, Event as TerminalEvent, KeyCode};
use forms::render::{FormsWidget, Theme};
use forms::{
    Builder, Button, ButtonRole, Choice, Control, Event, Input, Key, MemoryProvider, Modal,
    ModalStyle, Session, Value,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::widgets::Widget;

/// The handle of the example set.
const HANDLE: u8 = 0;
/// The value key of the secure boot row.
const SECURE_BOOT: Key = Key(10);
/// The value key of the boot mode row.
const MODE: Key = Key(11);
/// The value key of the timeout row.
const TIMEOUT: Key = Key(12);
/// The value key of the order row.
const ORDER: Key = Key(13);
/// The value key of the name row.
const NAME: Key = Key(14);

fn main() -> io::Result<()> {
    let mut provider = provider();
    let set = set();
    let mut session = Session::new(vec![(HANDLE, set.clone())]);
    let _ = session.enter(set.key);

    let mut once = false;
    let mut open = false;
    let mut columns = 56;
    let mut rows = 18;
    let mut argument = String::new();
    for value in std::env::args().skip(1) {
        match value.as_str() {
            "--once" => once = true,
            "--open" => open = true,
            "--columns" => argument = String::from("columns"),
            "--rows" => argument = String::from("rows"),
            other => {
                let number = other.parse::<u16>().unwrap_or(0);
                match argument.as_str() {
                    "columns" => columns = number,
                    "rows" => rows = number,
                    _ => {}
                }
                argument.clear();
            }
        }
    }
    if open {
        let _ = session.input(Input::Down, &mut provider);
        let _ = session.input(Input::Enter, &mut provider);
    }
    if once {
        return print_frame(&mut session, &mut provider, columns, rows);
    }
    interactive(&mut session, &mut provider)
}

/// Builds the example set.
fn set() -> forms::FormSet {
    let mut builder = Builder::new(Key(1), "Platform").help("Recovery settings");
    builder.page("Boot");
    builder.row("State", Control::Heading);
    builder.row("Firmware", Control::Text(String::from("1.4")));
    builder
        .row("Secure boot", Control::Toggle)
        .set_binding(SECURE_BOOT);
    builder
        .row(
            "Boot mode",
            Control::Choice(vec![Choice::new("Fast", 1), Choice::new("Safe", 2)]),
        )
        .set_binding(MODE);
    let row = builder.row(
        "Menu timeout",
        Control::Slider {
            minimum: 0,
            maximum: 10,
            step: 1,
            unit: String::from("s"),
            labels: vec![Choice::new("Forever", 0)],
        },
    );
    row.set_binding(TIMEOUT);
    row.set_help("Zero waits forever.");
    builder
        .row(
            "Machine name",
            Control::TextInput {
                minimum: 1,
                maximum: 16,
                secret: false,
            },
        )
        .set_binding(NAME);
    builder
        .row(
            "Boot order",
            Control::OrderedList {
                choices: vec![
                    Choice::new("Disk", 1),
                    Choice::new("Network", 2),
                    Choice::new("USB", 3),
                ],
                maximum: 3,
            },
        )
        .set_binding(ORDER);
    builder.row(
        "Progress",
        Control::Progress {
            current: 2,
            total: Some(5),
        },
    );
    builder.row("Separator", Control::Divider);
    builder.build()
}

/// Builds the in-memory values of the example.
fn provider() -> MemoryProvider {
    let mut provider = MemoryProvider::new(vec![(HANDLE, set())]);
    provider.set(SECURE_BOOT, Value::Bool(true));
    provider.set(MODE, Value::Number(1));
    provider.set(TIMEOUT, Value::Number(3));
    provider.set(NAME, Value::Text(String::from("board")));
    provider.set(ORDER, Value::List(vec![1, 3]));
    provider
}

/// Maps a terminal key to a front-end input.
fn map_key(code: KeyCode) -> Option<Input> {
    match code {
        KeyCode::Up => Some(Input::Up),
        KeyCode::Down => Some(Input::Down),
        KeyCode::Left => Some(Input::Left),
        KeyCode::Right => Some(Input::Right),
        KeyCode::Enter => Some(Input::Enter),
        KeyCode::Esc => Some(Input::Back),
        KeyCode::Backspace => Some(Input::Erase),
        KeyCode::Home => Some(Input::Home),
        KeyCode::End => Some(Input::End),
        KeyCode::Char(character) => Some(Input::Char(character)),
        _ => None,
    }
}

/// Draws one frame to a test backend and prints it.
fn print_frame(
    session: &mut Session<u8>,
    provider: &mut MemoryProvider,
    columns: u16,
    rows: u16,
) -> io::Result<()> {
    let view = session.view(provider);
    let backend = TestBackend::new(columns, rows);
    let mut terminal =
        Terminal::new(backend).map_err(|error| io::Error::other(error.to_string()))?;
    terminal
        .draw(|frame| {
            FormsWidget::new(&view)
                .with_theme(Theme::default())
                .render(frame.area(), frame.buffer_mut());
        })
        .map_err(|error| io::Error::other(error.to_string()))?;
    let buffer = terminal.backend().buffer().clone();
    let mut out = io::stdout();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// Reads keys and draws the page until the person leaves it.
fn interactive(session: &mut Session<u8>, provider: &mut MemoryProvider) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = serve(&mut terminal, session, provider);
    ratatui::restore();
    result
}

/// Draws and reads keys in an open terminal.
fn serve(
    terminal: &mut ratatui::DefaultTerminal,
    session: &mut Session<u8>,
    provider: &mut MemoryProvider,
) -> io::Result<()> {
    loop {
        let view = session.view(provider);
        terminal.draw(|frame| FormsWidget::new(&view).render(frame.area(), frame.buffer_mut()))?;
        let input = match event::read()? {
            TerminalEvent::Key(key) => map_key(key.code),
            _ => None,
        };
        if let Some(input) = input {
            match session.input(input, provider) {
                Event::Exit => return Ok(()),
                Event::None | Event::Changed => {}
                Event::Warning(message) => {
                    session.open_modal(
                        Modal::new(ModalStyle::Warning, "Confirm", message)
                            .button(Button::new("Continue", ButtonRole::Default))
                            .button(Button::new("Cancel", ButtonRole::Cancel)),
                    );
                }
                Event::Failed(error) => {
                    session.open_modal(
                        Modal::new(ModalStyle::Error, "Refused", error.reason)
                            .button(Button::new("OK", ButtonRole::Default)),
                    );
                }
                Event::Modal(_) | Event::Response(_) => {}
            }
        }
    }
}
