//! Host support that the examples of this crate share.
//!
//! A directory under `examples` is not a cargo target unless it holds
//! `main.rs`, so each example includes this file with `#[path]`. The file
//! draws a page, maps terminal keys and prints one frame to a test backend,
//! so the examples stay about `forms` and not about a terminal.

use std::io::{self, Write};

use crossterm::event::{self, Event as TerminalEvent, KeyCode};
use forms::widgets::{FormsWidget, Painters, Theme};
use forms::{Event, Input, Provider, Session, View};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::widgets::Widget;

/// How one example runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Draw one frame and print it.
    Once {
        /// The frame width in cells.
        columns: u16,
        /// The frame height in cells.
        rows: u16,
    },
    /// Accept keys in a terminal.
    Interactive,
    /// Run the scripted steps and print the result.
    Script,
}

/// Reads the mode from the process arguments.
pub fn mode(arguments: impl Iterator<Item = String>) -> Mode {
    let mut mode = Mode::Script;
    let mut columns = 56;
    let mut rows = 18;
    let mut pending = String::new();
    for argument in arguments {
        match argument.as_str() {
            "--once" => mode = Mode::Once { columns, rows },
            "--interactive" => mode = Mode::Interactive,
            "--columns" => pending = String::from("columns"),
            "--rows" => pending = String::from("rows"),
            other => {
                if let Ok(number) = other.parse::<u16>() {
                    match pending.as_str() {
                        "columns" => columns = number,
                        "rows" => rows = number,
                        _ => {}
                    }
                    if let Mode::Once {
                        columns: c,
                        rows: r,
                    } = mode
                    {
                        mode = Mode::Once {
                            columns: c,
                            rows: r,
                        };
                    }
                }
                pending.clear();
            }
        }
    }
    match mode {
        Mode::Once { .. } => Mode::Once { columns, rows },
        other => other,
    }
}

/// Maps a terminal key to a front-end input.
pub fn map_key(code: KeyCode) -> Option<Input> {
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

/// Draws the page and answers keys until the person leaves it.
///
/// The events that a front end has to act on are returned, so the example can
/// print them after it restores the terminal.
///
/// # Errors
///
/// Returns the terminal failure.
pub fn interactive<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
) -> io::Result<Vec<Event>> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, session, provider);
    ratatui::restore();
    result
}

/// Draws the page and dispatches keys in a terminal.
fn event_loop<H: Copy + Eq, P: Provider<Handle = H>>(
    terminal: &mut ratatui::DefaultTerminal,
    session: &mut Session<H>,
    provider: &mut P,
) -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    loop {
        let view = session.view(provider);
        terminal.draw(|frame| FormsWidget::new(&view).render(frame.area(), frame.buffer_mut()))?;
        let input = match event::read()? {
            TerminalEvent::Key(key) => map_key(key.code),
            _ => None,
        };
        let event = match input {
            Some(input) => session.input(input, provider),
            None => Event::None,
        };
        match &event {
            Event::Exit => return Ok(events),
            Event::None | Event::Changed => {}
            other => events.push(other.clone()),
        }
    }
}

/// Draws one frame to a test backend and prints it.
///
/// # Errors
///
/// Returns the terminal failure.
pub fn print_frame<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
    columns: u16,
    rows: u16,
) -> io::Result<()> {
    print_frame_styled(
        session,
        provider,
        columns,
        rows,
        &Theme::default(),
        &Painters::new(),
    )
}

/// Draws one frame with a theme and consumer painters, and prints it.
///
/// # Errors
///
/// Returns the terminal failure.
pub fn print_frame_styled<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
    columns: u16,
    rows: u16,
    theme: &Theme,
    painters: &Painters,
) -> io::Result<()> {
    let view = session.view(provider);
    frame(&view, columns, rows, theme, painters)
}

/// Draws one view to a test backend and prints it.
///
/// # Errors
///
/// Returns the terminal failure.
pub fn frame(
    view: &View,
    columns: u16,
    rows: u16,
    theme: &Theme,
    painters: &Painters,
) -> io::Result<()> {
    let backend = TestBackend::new(columns, rows);
    let mut terminal =
        Terminal::new(backend).map_err(|error| io::Error::other(error.to_string()))?;
    terminal
        .draw(|frame| {
            FormsWidget::new(view)
                .with_theme(theme.clone())
                .with_painters(painters.clone())
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

/// Moves the selection to the first row with this label.
///
/// Returns `false` when no row of the page has the label.
pub fn select<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
    label: &str,
) -> bool {
    for _ in 0..64 {
        let view = session.view(provider);
        let Some(index) = view.selected else {
            return false;
        };
        if view.rows.get(index).is_some_and(|row| row.label == label) {
            return true;
        }
        session.input(Input::Down, provider);
    }
    false
}

/// Moves the selection to the first row with this label and presses Enter.
///
/// Returns the event of that Enter, or `None` when no row has the label.
pub fn enter_row<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
    label: &str,
) -> Option<Event> {
    if !select(session, provider, label) {
        return None;
    }
    Some(session.input(Input::Enter, provider))
}
