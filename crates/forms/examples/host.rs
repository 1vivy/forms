//! Runs the forms widget over two in-memory formsets in a real terminal.
//!
//! Start it without arguments for an interactive page. Start it with `--once`
//! to draw one frame to a test backend and print that frame; that path needs no
//! terminal and proves the first frame of the stack on a Linux host. Add
//! `--columns N` and `--rows N` to draw a frame at another size, and `--open`
//! to open the editor of the first question row.

use std::collections::HashMap;
use std::io::{self, Write};

use crossterm::event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers};
use forms::widgets::FormsWidget;
use forms::{
    ActionRequest, Choice, Error, Form, FormId, FormSet, Input, Provider, Question, QuestionFlags,
    QuestionId, RowKind, Session, Statement, Storage, Value, VarStore, VarStoreId, VarStoreKind,
    Width,
};
use ratatui::DefaultTerminal;
use ratatui::backend::TestBackend;

/// The in-memory provider behind the example.
struct Memory {
    sets: Vec<(u8, FormSet)>,
    values: HashMap<(u8, u16), Value>,
}

impl Memory {
    /// Builds two formsets and seeds every question value.
    fn new() -> Self {
        let sets = vec![(1u8, display_set()), (2u8, recovery_set())];
        let mut values = HashMap::new();
        for (handle, set) in &sets {
            for form in &set.forms {
                for item in &form.items {
                    let seed = match &item.statement {
                        Statement::Action { question } => Some((question, Value::Number(0))),
                        Statement::Checkbox { question, default } => {
                            Some((question, Value::Bool(*default)))
                        }
                        Statement::OneOf {
                            question, choices, ..
                        } => {
                            let value = choices
                                .iter()
                                .find(|choice| choice.default)
                                .or_else(|| choices.first())
                                .map_or(0, |choice| choice.value);
                            Some((question, Value::Number(value)))
                        }
                        Statement::Numeric {
                            question, minimum, ..
                        } => Some((question, Value::Number(*minimum))),
                        Statement::TextInput { question, .. } => {
                            Some((question, Value::Text(String::new())))
                        }
                        Statement::Subtitle { .. }
                        | Statement::Text { .. }
                        | Statement::Goto { .. } => None,
                    };
                    if let Some((question, value)) = seed {
                        values.insert((*handle, question.id.0), value);
                    }
                }
            }
        }
        Self { sets, values }
    }
}

impl Provider for Memory {
    type Handle = u8;

    fn formsets(&mut self) -> Result<Vec<(u8, FormSet)>, Error> {
        Ok(self.sets.clone())
    }

    fn read(&mut self, handle: u8, _set: &FormSet, question: &Question) -> Result<Value, Error> {
        self.values
            .get(&(handle, question.id.0))
            .cloned()
            .ok_or_else(|| Error {
                code: 1,
                reason: format!("no value for question {}", question.id.0),
            })
    }

    fn write(
        &mut self,
        handle: u8,
        _set: &FormSet,
        question: &Question,
        value: &Value,
    ) -> Result<ActionRequest, Error> {
        if let Value::Text(text) = value
            && text.is_empty()
        {
            return Err(Error {
                code: 2,
                reason: "the name needs at least one character".into(),
            });
        }
        self.values.insert((handle, question.id.0), value.clone());
        Ok(if question.prompt == "Device name" {
            ActionRequest::FormApply
        } else {
            ActionRequest::None
        })
    }

    fn action(
        &mut self,
        _handle: u8,
        _set: &FormSet,
        _question: &Question,
    ) -> Result<ActionRequest, Error> {
        Ok(ActionRequest::None)
    }
}

/// Builds the display formset.
fn display_set() -> FormSet {
    FormSet {
        guid: [0x11; 16],
        title: "Display".into(),
        help: "Screen and input behaviour.".into(),
        class_guids: Vec::new(),
        varstores: vec![VarStore {
            id: VarStoreId(1),
            guid: [0x21; 16],
            name: "DisplayVars".into(),
            size: 0,
            kind: VarStoreKind::NameValue,
        }],
        forms: vec![Form {
            id: FormId(1),
            title: "Display".into(),
            items: vec![
                Statement::Subtitle {
                    text: "Appearance".into(),
                }
                .into(),
                Statement::Checkbox {
                    question: question(1, "Night mode", "Use the dark palette.", "night"),
                    default: false,
                }
                .into(),
                Statement::OneOf {
                    question: question(2, "Scale", "Interface scale.", "scale"),
                    width: Width::U8,
                    choices: vec![
                        Choice {
                            text: "100%".into(),
                            value: 100,
                            default: true,
                        },
                        Choice {
                            text: "150%".into(),
                            value: 150,
                            default: false,
                        },
                        Choice {
                            text: "200%".into(),
                            value: 200,
                            default: false,
                        },
                    ],
                }
                .into(),
                Statement::Numeric {
                    question: question(3, "Brightness", "Backlight level.", "brightness"),
                    width: Width::U16,
                    minimum: 0,
                    maximum: 100,
                    step: 5,
                }
                .into(),
                Statement::TextInput {
                    question: question(4, "Device name", "Name shown on the network.", "name"),
                    minimum: 1,
                    maximum: 12,
                }
                .into(),
                Statement::Text {
                    prompt: "Panel".into(),
                    help: "The built-in display.".into(),
                    value: "1440 x 3120".into(),
                }
                .into(),
            ],
        }],
    }
}

/// Builds the recovery formset with two forms.
fn recovery_set() -> FormSet {
    FormSet {
        guid: [0x12; 16],
        title: "Recovery".into(),
        help: "Diagnostics and repair.".into(),
        class_guids: Vec::new(),
        varstores: Vec::new(),
        forms: vec![
            Form {
                id: FormId(1),
                title: "Recovery".into(),
                items: vec![
                    Statement::Action {
                        question: question(1, "Run self test", "Check memory and storage.", "test"),
                    }
                    .into(),
                    Statement::Goto {
                        prompt: "Advanced".into(),
                        help: "Logs and verbose output.".into(),
                        form: FormId(2),
                        question: None,
                    }
                    .into(),
                    Statement::Text {
                        prompt: "Firmware".into(),
                        help: "The running firmware revision.".into(),
                        value: "UEFI 2.10".into(),
                    }
                    .into(),
                ],
            },
            Form {
                id: FormId(2),
                title: "Advanced".into(),
                items: vec![
                    Statement::Checkbox {
                        question: question(2, "Verbose log", "Log every step.", "verbose"),
                        default: true,
                    }
                    .into(),
                    Statement::Action {
                        question: question(3, "Collect log", "Copy the log to the ESP.", "collect"),
                    }
                    .into(),
                ],
            },
        ],
    }
}

/// Builds one named question in the example store.
fn question(id: u16, prompt: &str, help: &str, name: &str) -> Question {
    Question {
        id: QuestionId(id),
        prompt: prompt.into(),
        help: help.into(),
        storage: Storage::Name {
            varstore: VarStoreId(1),
            name: name.into(),
        },
        flags: QuestionFlags::CALLBACK,
    }
}

/// Maps a terminal key to a front-end input.
fn map_key(code: KeyCode) -> Option<Input> {
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

/// Options of the `--once` frame.
struct Frame {
    /// Text columns of the test backend.
    columns: u16,
    /// Text rows of the test backend.
    rows: u16,
    /// Whether to open the first editor before the frame.
    open: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            columns: 80,
            rows: 24,
            open: false,
        }
    }
}

fn main() -> io::Result<()> {
    let mut provider = Memory::new();
    let mut session = Session::new(
        provider
            .formsets()
            .expect("in-memory formsets are readable"),
    );
    let mut once = false;
    let mut frame = Frame::default();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--once" => once = true,
            "--open" => frame.open = true,
            "--columns" => {
                frame.columns = arguments
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(frame.columns);
            }
            "--rows" => {
                frame.rows = arguments
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(frame.rows);
            }
            _ => {}
        }
    }
    if once {
        return render_once(&mut session, &mut provider, &frame);
    }
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut terminal, &mut session, &mut provider);
    ratatui::restore();
    outcome
}

/// Draws the page and dispatches keys until the person leaves.
fn event_loop(
    terminal: &mut DefaultTerminal,
    session: &mut Session<u8>,
    provider: &mut Memory,
) -> io::Result<()> {
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
            return Ok(());
        }
        let Some(input) = map_key(key.code) else {
            continue;
        };
        if session.input(input, provider) == forms::Event::Exit {
            return Ok(());
        }
    }
}

/// Selects the first row that opens an editor and presses Enter on it.
///
/// The `--open` frame needs a modal, and a toggle row changes a value instead
/// of opening one.
fn open_first_editor(session: &mut Session<u8>, provider: &mut Memory) {
    // The first page lists formsets; Enter on its first row opens one.
    if session
        .view(provider)
        .rows
        .first()
        .is_some_and(|row| row.kind == RowKind::FormSet)
    {
        session.input(Input::Enter, provider);
    }
    for _ in 0..session.view(provider).rows.len() {
        let view = session.view(provider);
        let Some(row) = view.selected.and_then(|index| view.rows.get(index)) else {
            return;
        };
        if matches!(row.kind, RowKind::Choice | RowKind::Number | RowKind::Text) {
            session.input(Input::Enter, provider);
            return;
        }
        if session.input(Input::Down, provider) == forms::Event::Exit {
            return;
        }
    }
}

/// Draws one frame to a test backend and prints it.
fn render_once(
    session: &mut Session<u8>,
    provider: &mut Memory,
    options: &Frame,
) -> io::Result<()> {
    if options.open {
        open_first_editor(session, provider);
    }
    let backend = TestBackend::new(options.columns.max(4), options.rows.max(4));
    let mut terminal = ratatui::Terminal::new(backend).expect("the test backend cannot fail");
    terminal
        .draw(|frame| {
            let view = session.view(provider);
            frame.render_widget(FormsWidget::new(&view), frame.area());
        })
        .expect("the test backend cannot fail");
    let buffer = terminal.backend().buffer();
    let mut stdout = std::io::stdout();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        writeln!(stdout, "{}", line.trim_end())?;
    }
    Ok(())
}
