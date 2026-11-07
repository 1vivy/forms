//! The UEFI front end: one `forms` session on the firmware display.
//!
//! [`run`] owns the display for one session and nothing else:
//!
//! - It draws on the graphics output when the firmware publishes one, and on
//!   the inherited text console otherwise. It reads the live mode and never
//!   changes it.
//! - It opens the console input once, preferring the extended keyboard
//!   interface so that scan codes survive. The keys become `forms` inputs
//!   through a [`KeyMap`].
//! - It draws the page through the `forms` widget inside the margins of its
//!   [`Frame`], with the frame's theme. The text display keeps the same margins.
//! - It gives every turn to the [`Shell`], with or without a key stroke, so
//!   the shell can run a timer, execute a command or end the session.
//! - It clears the whole display when the session ends, so the next owner of
//!   the screen starts from a black display.
//!
//! The crate holds no page of its own. A consumer adds its pages to a
//! `forms` provider and supplies a [`Shell`].
//!
//! # Source
//!
//! The display, input and turn code ports the `setup` front end of
//! `odp-platform-qcom_bootstrap` and the `screen` crate of maOS. See
//! `README.md` in this crate for the source commits and the license.
//!
//! # Examples
//!
//! ```no_run
//! use forms::{Event, Provider, Session};
//! use forms_uefi::{Display, Shell, Turn};
//!
//! struct Timer;
//!
//! impl<P> Shell<u32, P> for Timer
//! where
//!     P: Provider<Handle = u32>,
//! {
//!     fn opened(&mut self, _session: &mut Session<u32>, _provider: &mut P, _display: &Display) {}
//!
//!     fn turn(
//!         &mut self,
//!         _session: &mut Session<u32>,
//!         _provider: &mut P,
//!         _pressed: bool,
//!         _event: Event,
//!     ) -> Turn {
//!         Turn::Continue
//!     }
//! }
//! ```

#![no_std]

extern crate alloc;

mod console;
mod graphics;
mod input;
mod text;

use alloc::boxed::Box;
use core::time::Duration;

use forms::render::{FormsWidget, Margins, Theme};
use forms::{
    ButtonRole, Event, FormSet, Modal, ModalOutcome, Provider, Request, Row, Session, Value,
};
use graphics::GopDisplay;
use input::Keys;
use mousefood::embedded_graphics::pixelcolor::Rgb888;
use mousefood::{
    EmbeddedBackend, EmbeddedBackendConfig, TerminalAlignment, fonts::mono_6x10_atlas,
};
use ratatui_core::backend::Backend;
use ratatui_core::layout::Rect;
use ratatui_core::terminal::Terminal;
use ratatui_core::widgets::Widget;
use uefi::boot;
use uefi::{Error, Status};

pub use input::{map as map_key, map_phone as map_key_phone};

/// Maps a firmware key stroke to a form input.
///
/// Scan codes win over the Unicode character. The default map follows a
/// keyboard: the arrows, the home key, Escape as Back, and the Qualcomm power
/// key as Enter. A front end for a phone with volume and power keys uses
/// [`map_key_phone`] instead.
pub type KeyMap = fn(u16, char) -> Option<forms::Input>;

/// What the shell asks the front end to do after one turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Turn {
    /// Take another turn.
    Continue,
    /// Draw the page again, then take another turn.
    Redraw,
    /// End the session and clear the display.
    End,
}

/// The display that the front end opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Display {
    /// What the page is drawn on.
    pub surface: Surface,
    /// The page width in cells.
    pub columns: u16,
    /// The page height in cells.
    pub rows: u16,
    /// The name of the open input protocol.
    pub input: &'static str,
}

/// What a page is drawn on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Surface {
    /// The graphics output, with its live mode in pixels and the page scale.
    Graphics {
        /// The live mode, width by height in pixels.
        mode: (u32, u32),
        /// How many mode pixels one page pixel covers.
        scale: u32,
    },
    /// The inherited text console.
    Text,
}

impl core::fmt::Display for Display {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.surface {
            Surface::Graphics { mode, scale } => write!(
                out,
                "Graphics output {}x{}, scale {scale}, {}x{} cells, {}",
                mode.0, mode.1, self.columns, self.rows, self.input
            ),
            Surface::Text => write!(
                out,
                "Text console {}x{} cells, {}",
                self.columns, self.rows, self.input
            ),
        }
    }
}

/// The owner of one session: it reacts to every turn.
pub trait Shell<H, P> {
    /// The front end opened its display. The page is drawn after this call.
    ///
    /// A consumer logs or records the display. This crate logs nothing.
    fn opened(&mut self, session: &mut Session<H>, provider: &mut P, display: &Display);

    /// One turn. `pressed` tells whether a key arrived. `event` is the result
    /// of its input on the session, or [`Event::None`].
    fn turn(
        &mut self,
        session: &mut Session<H>,
        provider: &mut P,
        pressed: bool,
        event: Event,
    ) -> Turn;

    /// Whether another owner of the screen wrote over the page since the last
    /// turn, such as a firmware console message. The front end then clears the
    /// display and draws the whole page again. The default never reports damage.
    fn damaged(&mut self) -> bool {
        false
    }
}

/// The pause between two turns: the console is polled 100 times a second.
pub const TURN: Duration = Duration::from_millis(10);

/// How one session is framed: the page grid, the margins, the theme and the
/// key map.
///
/// [`Frame::default`] is a keyboard front end with forty columns, sixteen
/// rows, one tenth of margin on every edge and the default theme. A consumer
/// changes single fields, so a phone states its own keys and margins without
/// a front end of its own.
///
/// # Examples
///
/// ```
/// use forms::render::{KeyNames, Margins};
///
/// let mut frame = forms_uefi::Frame::default();
/// frame.margins = Margins { top: 50, bottom: 50, left: 35, right: 35 };
/// frame.theme.keys = KeyNames::volume_and_power();
/// frame.key_map = forms_uefi::map_key_phone;
/// assert_eq!(frame.columns, 40);
/// ```
#[derive(Clone, Debug)]
pub struct Frame {
    /// The fewest columns that the page keeps on the graphics output.
    pub columns: u32,
    /// The fewest rows that the page keeps on the graphics output.
    pub rows: u32,
    /// The margin of each display edge.
    pub margins: Margins,
    /// The theme the page draws with.
    pub theme: Theme,
    /// The map from a firmware key stroke to a form input.
    pub key_map: KeyMap,
    /// How the pixels reach the graphics output.
    pub drawing: Drawing,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            columns: 40,
            rows: 16,
            margins: Margins::default(),
            theme: Theme::default(),
            key_map: map_key,
            drawing: Drawing::Auto,
        }
    }
}

/// How the pixels of the page reach the graphics output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Drawing {
    /// Write the memory-mapped frame buffer when the mode has one, and use
    /// `Blt` otherwise.
    #[default]
    Auto,
    /// Always use `Blt`, so the firmware copies every pixel. A platform whose
    /// frame buffer the firmware does not keep coherent with the panel needs
    /// this.
    Blt,
}

/// Shows one modal over an empty page and returns the role that the person
/// chose.
///
/// The modal takes every key stroke. The forms crate highlights the Cancel
/// button first, so one Enter refuses a question that offers it, and a
/// dismissal answers [`ButtonRole::Cancel`]. `idle` runs once per turn, so a
/// caller can poll a host interface while the modal waits; [`Turn::End`] from
/// it ends the wait with `EFI_ABORTED`, so a caller never reads a missing
/// answer as consent.
///
/// # Errors
///
/// Returns the same failures as [`run_with`], and `EFI_ABORTED` when `idle`
/// ended the wait before the modal was answered.
pub fn ask(modal: Modal, frame: &Frame, idle: impl FnMut() -> Turn) -> uefi::Result<ButtonRole> {
    let mut session = Session::<()>::new(alloc::vec::Vec::new());
    session.open_modal(modal);
    let mut shell = Ask { idle, answer: None };
    run_with(&mut session, &mut Empty, &mut shell, frame)?;
    shell.answer.ok_or_else(|| Error::from(Status::ABORTED))
}

/// The shell of [`ask`]: it ends on the modal's answer.
struct Ask<F> {
    idle: F,
    answer: Option<ButtonRole>,
}

impl<F: FnMut() -> Turn> Shell<(), Empty> for Ask<F> {
    fn opened(&mut self, _: &mut Session<()>, _: &mut Empty, _: &Display) {}

    fn turn(&mut self, _: &mut Session<()>, _: &mut Empty, _: bool, event: Event) -> Turn {
        if let Event::Modal(outcome) = event {
            self.answer = Some(match outcome {
                ModalOutcome::Chosen { role, .. } => role,
                ModalOutcome::Closed => ButtonRole::Cancel,
            });
            return Turn::End;
        }
        (self.idle)()
    }
}

/// The provider of a modal page: it publishes no set.
struct Empty;

impl Provider for Empty {
    type Handle = ();

    fn formsets(&mut self) -> Result<alloc::vec::Vec<((), FormSet)>, forms::Error> {
        Ok(alloc::vec::Vec::new())
    }

    fn read(&mut self, _: (), _: &FormSet, _: &Row) -> Result<Value, forms::Error> {
        Err(forms::Error::new("a modal page publishes no set"))
    }

    fn write(&mut self, _: (), _: &FormSet, _: &Row, _: &Value) -> Result<Request, forms::Error> {
        Err(forms::Error::new("a modal page publishes no set"))
    }

    fn action(&mut self, _: (), _: &FormSet, _: &Row) -> Result<Request, forms::Error> {
        Err(forms::Error::new("a modal page publishes no set"))
    }
}

/// Runs one session in the default [`Frame`] until the shell ends it.
///
/// # Errors
///
/// Returns the firmware failure when no display can be drawn on, when the
/// console input cannot be opened, or when the session cannot be drawn.
pub fn run<H, P, S>(session: &mut Session<H>, provider: &mut P, shell: &mut S) -> uefi::Result
where
    H: Copy + Eq,
    P: Provider<Handle = H>,
    S: Shell<H, P>,
{
    run_with(session, provider, shell, &Frame::default())
}

/// Runs one session in the consumer's [`Frame`] until the shell ends it.
///
/// The frame's key map turns one firmware key stroke, a scan code and a
/// character, into a form input. A key without a form meaning still counts as
/// a press, so the shell sees the turn.
///
/// # Errors
///
/// Returns the firmware failure when no display can be drawn on, when the
/// console input cannot be opened, or when the session cannot be drawn.
pub fn run_with<H, P, S>(
    session: &mut Session<H>,
    provider: &mut P,
    shell: &mut S,
    frame: &Frame,
) -> uefi::Result
where
    H: Copy + Eq,
    P: Provider<Handle = H>,
    S: Shell<H, P>,
{
    let (stdin, stdout) = console::handles()?;
    let mut keys = Keys::open(stdin, frame.key_map)?;
    match GopDisplay::open(frame) {
        Ok(display) => graphics(display, frame, &mut keys, session, provider, shell),
        Err(_) => text(stdout, frame, &mut keys, session, provider, shell),
    }
}

/// Serves the session on the graphics output.
fn graphics<H, P, S>(
    mut display: GopDisplay,
    frame: &Frame,
    keys: &mut Keys,
    session: &mut Session<H>,
    provider: &mut P,
    shell: &mut S,
) -> uefi::Result
where
    H: Copy + Eq,
    P: Provider<Handle = H>,
    S: Shell<H, P>,
{
    let (mode, scale) = display.geometry();
    let config = EmbeddedBackendConfig::<GopDisplay, Rgb888> {
        font_regular: mono_6x10_atlas(),
        // The page is centred inside the margins, so a mode whose size is not a
        // multiple of the scaled cell keeps an equal border on both sides.
        horizontal_alignment: TerminalAlignment::Center,
        vertical_alignment: TerminalAlignment::Center,
        flush_callback: Box::new(|target: &mut GopDisplay| target.present()),
        ..Default::default()
    };
    let served = {
        let backend = EmbeddedBackend::new(&mut display, config);
        match Terminal::new(backend).map_err(|_| Error::from(Status::DEVICE_ERROR)) {
            Ok(mut terminal) => {
                // A graphics page has no cursor.
                let _ = terminal.hide_cursor();
                let cells = terminal
                    .size()
                    .map_err(|_| Error::from(Status::DEVICE_ERROR))?;
                shell.opened(
                    session,
                    provider,
                    &Display {
                        surface: Surface::Graphics {
                            mode: (mode.width, mode.height),
                            scale,
                        },
                        columns: cells.width,
                        rows: cells.height,
                        input: keys.kind(),
                    },
                );
                serve(
                    &mut terminal,
                    |area| area,
                    &frame.theme,
                    keys,
                    session,
                    provider,
                    shell,
                )
            }
            Err(error) => Err(error),
        }
    };
    let closed = display.close();
    served.and(closed)
}

/// Serves the session on the text console, for a host without graphics output.
fn text<H, P, S>(
    stdout: uefi::Handle,
    frame: &Frame,
    keys: &mut Keys,
    session: &mut Session<H>,
    provider: &mut P,
    shell: &mut S,
) -> uefi::Result
where
    H: Copy + Eq,
    P: Provider<Handle = H>,
    S: Shell<H, P>,
{
    let backend = text::TextBackend::open(stdout)?;
    let mut terminal = Terminal::new(backend).map_err(|error| Error::from(error.status))?;
    // A console without cursor control still draws the page.
    let _ = terminal.hide_cursor();
    let cells = terminal.size().map_err(|error| Error::from(error.status))?;
    shell.opened(
        session,
        provider,
        &Display {
            surface: Surface::Text,
            columns: cells.width,
            rows: cells.height,
            input: keys.kind(),
        },
    );
    let page = |area: Rect| margins(area, frame.margins);
    let served = serve(
        &mut terminal,
        page,
        &frame.theme,
        keys,
        session,
        provider,
        shell,
    );
    let cleared = terminal.clear().map_err(|error| Error::from(error.status));
    let _ = terminal.show_cursor();
    served.and(cleared)
}

/// The page area of a text console, inside `margins`.
fn margins(area: Rect, margins: Margins) -> Rect {
    let (left, top, width, height) = margins.area((u32::from(area.width), u32::from(area.height)));
    Rect::new(
        area.x + u16::try_from(left).unwrap_or(0),
        area.y + u16::try_from(top).unwrap_or(0),
        u16::try_from(width).unwrap_or(area.width),
        u16::try_from(height).unwrap_or(area.height),
    )
}

/// A display that can report a failure that a draw recorded.
trait Fault: Backend {
    /// Returns the firmware status of a recorded failure.
    fn fault(&mut self) -> Option<Status>;
}

impl Fault for text::TextBackend {
    fn fault(&mut self) -> Option<Status> {
        None
    }
}

impl Fault for EmbeddedBackend<'_, GopDisplay, Rgb888> {
    fn fault(&mut self) -> Option<Status> {
        self.display_mut().take_failure()
    }
}

/// Serves the session on an open terminal until the shell ends it.
fn serve<B, H, P, S>(
    terminal: &mut Terminal<B>,
    page: impl Fn(Rect) -> Rect,
    theme: &Theme,
    keys: &mut Keys,
    session: &mut Session<H>,
    provider: &mut P,
    shell: &mut S,
) -> uefi::Result
where
    B: Fault,
    H: Copy + Eq,
    P: Provider<Handle = H>,
    S: Shell<H, P>,
{
    let mut dirty = true;
    loop {
        if dirty {
            let view = session.view(provider);
            terminal
                .draw(|frame| {
                    let area = page(frame.area());
                    FormsWidget::new(&view)
                        .with_theme(theme.clone())
                        .render(area, frame.buffer_mut());
                })
                .map_err(|_| Error::from(Status::DEVICE_ERROR))?;
            if let Some(status) = terminal.backend_mut().fault() {
                return Err(Error::from(status));
            }
            dirty = false;
        }
        let (pressed, event) = match keys.read() {
            Some(Some(input)) => (true, session.input(input, provider)),
            Some(None) => (true, Event::None),
            None => (false, Event::None),
        };
        if matches!(
            event,
            Event::Changed
                | Event::Failed(_)
                | Event::Response(_)
                | Event::Modal(_)
                | Event::Warning(_)
        ) {
            dirty = true;
        }
        match shell.turn(session, provider, pressed, event) {
            Turn::End => return Ok(()),
            Turn::Redraw => dirty = true,
            Turn::Continue => {}
        }
        // Another owner of the screen drew over the page, so the whole page
        // is drawn again from a cleared display.
        if shell.damaged() {
            terminal
                .clear()
                .map_err(|_| Error::from(Status::DEVICE_ERROR))?;
            dirty = true;
        }
        boot::stall(TURN);
    }
}
