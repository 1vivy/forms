//! Host support that the examples of this crate share.
//!
//! A directory under `examples` is not a cargo target unless it holds
//! `main.rs`, so each example includes this file with `#[path]`. The file
//! draws a page, maps terminal keys and prints one frame to a test backend,
//! so the examples stay about `forms` and not about a terminal.

use std::io::{self, Write};

use crossterm::event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers};
use forms::widgets::FormsWidget;
use forms::{Event, Input, Provider, Session};
use ratatui::DefaultTerminal;
use ratatui::backend::TestBackend;

/// How one example runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Runs the scripted demonstration and prints its result.
    Script,
    /// Prints one frame and leaves, so the gate needs no terminal.
    Once {
        /// Text columns of the test backend.
        columns: u16,
        /// Text rows of the test backend.
        rows: u16,
    },
    /// Runs a terminal loop until the person leaves the page.
    Interactive,
}

/// Reads the mode from the process arguments.
pub fn mode(arguments: impl Iterator<Item = String>) -> Mode {
    let mut mode = Mode::Script;
    let mut columns = 80;
    let mut rows = 22;
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--once" => mode = Mode::Once { columns, rows },
            "--interactive" => mode = Mode::Interactive,
            "--columns" => {
                if let Some(value) = arguments.next().and_then(|value| value.parse().ok()) {
                    columns = value;
                }
                if let Mode::Once { rows, .. } = mode {
                    mode = Mode::Once { columns, rows };
                }
            }
            "--rows" => {
                if let Some(value) = arguments.next().and_then(|value| value.parse().ok()) {
                    rows = value;
                }
                if let Mode::Once { columns, .. } = mode {
                    mode = Mode::Once { columns, rows };
                }
            }
            _ => {}
        }
    }
    mode
}

/// Maps a terminal key to a front-end input.
pub fn map_key(code: KeyCode) -> Option<Input> {
    Some(match code {
        KeyCode::Up => Input::Up,
        KeyCode::Down => Input::Down,
        KeyCode::Left => Input::Left,
        KeyCode::Right => Input::Right,
        KeyCode::Enter => Input::Enter,
        KeyCode::Esc => Input::Back,
        KeyCode::Home => Input::Home,
        KeyCode::Backspace => Input::Erase,
        KeyCode::Char(character) => Input::Char(character),
        _ => return None,
    })
}

/// Draws the page and answers keys until the person leaves it.
///
/// The events that a front end has to act on are returned, so the example can
/// print them after it restores the terminal.
pub fn interactive<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
) -> io::Result<Vec<Event>> {
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut terminal, session, provider);
    ratatui::restore();
    outcome
}

/// Draws the page and dispatches keys in a terminal.
fn event_loop<H: Copy + Eq, P: Provider<Handle = H>>(
    terminal: &mut DefaultTerminal,
    session: &mut Session<H>,
    provider: &mut P,
) -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    loop {
        terminal.draw(|frame| {
            let view = session.view(provider);
            frame.render_widget(FormsWidget::new(&view), frame.area());
        })?;
        let key = match event::read()? {
            TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => key,
            _ => continue,
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(events);
        }
        let Some(input) = map_key(key.code) else {
            continue;
        };
        match session.input(input, provider) {
            Event::Exit => return Ok(events),
            Event::None | Event::Changed | Event::Failed(_) => {}
            event => events.push(event),
        }
    }
}

/// Draws one frame to a test backend and prints it.
pub fn print_frame<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
    columns: u16,
    rows: u16,
) -> io::Result<()> {
    let backend = TestBackend::new(columns.max(4), rows.max(4));
    let mut terminal = ratatui::Terminal::new(backend).expect("the test backend cannot fail");
    terminal
        .draw(|frame| {
            let view = session.view(provider);
            frame.render_widget(FormsWidget::new(&view), frame.area());
        })
        .expect("the test backend cannot fail");
    let buffer = terminal.backend().buffer();
    let mut stdout = io::stdout();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        writeln!(stdout, "{}", line.trim_end())?;
    }
    Ok(())
}

/// Moves the selection to the first row with this prompt.
///
/// Returns `false` when no row of the page has the prompt.
pub fn select<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
    prompt: &str,
) -> bool {
    for _ in 0..session.view(provider).rows.len() {
        let view = session.view(provider);
        let Some(row) = view.selected.and_then(|index| view.rows.get(index)) else {
            return false;
        };
        if row.prompt == prompt {
            return true;
        }
        if session.input(Input::Down, provider) == Event::Exit {
            return false;
        }
    }
    false
}

/// Moves the selection to the first row with this prompt and presses Enter.
///
/// Returns the event of that Enter, or `None` when no row has the prompt.
pub fn enter_row<H: Copy + Eq, P: Provider<Handle = H>>(
    session: &mut Session<H>,
    provider: &mut P,
    prompt: &str,
) -> Option<Event> {
    if !select(session, provider, prompt) {
        return None;
    }
    Some(session.input(Input::Enter, provider))
}
