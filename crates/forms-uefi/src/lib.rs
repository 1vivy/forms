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
//! - It draws the page through the `forms` widget inside a margin of one tenth
//!   of the display on every edge. The text display keeps the same margin.
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
//! use forms_uefi::{Shell, Turn};
//!
//! struct Timer;
//!
//! impl<P> Shell<u32, P> for Timer
//! where
//!     P: Provider<Handle = u32>,
//! {
//!     fn opened(&mut self, _session: &mut Session<u32>, _provider: &mut P, _display: &str) {}
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
use alloc::format;
use core::time::Duration;

use forms::widgets::FormsWidget;
use forms::{Event, Provider, Session};
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

/// The owner of one session: it reacts to every turn.
pub trait Shell<H, P> {
    /// The front end opened its display. `display` describes it. The page is
    /// drawn after this call.
    ///
    /// The description names the backend, its size and the input protocol. A
    /// consumer logs it or records it. This crate logs nothing.
    fn opened(&mut self, session: &mut Session<H>, provider: &mut P, display: &str);

    /// One turn. `pressed` tells whether a key arrived. `event` is the result
    /// of its input on the session, or [`Event::None`].
    fn turn(
        &mut self,
        session: &mut Session<H>,
        provider: &mut P,
        pressed: bool,
        event: Event,
    ) -> Turn;
}

/// The pause between two turns: the console is polled 100 times a second.
pub const TURN: Duration = Duration::from_millis(10);

/// Runs one session with the default [`KeyMap`] until the shell ends it.
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
    run_with(session, provider, shell, map_key)
}

/// Runs one session with the key map of the consumer until the shell ends it.
///
/// The key map turns one firmware key stroke, a scan code and a character,
/// into a form input. A key without a form meaning still counts as a press, so
/// the shell sees the turn.
///
/// # Errors
///
/// Returns the firmware failure when no display can be drawn on, when the
/// console input cannot be opened, or when the session cannot be drawn.
pub fn run_with<H, P, S>(
    session: &mut Session<H>,
    provider: &mut P,
    shell: &mut S,
    key_map: KeyMap,
) -> uefi::Result
where
    H: Copy + Eq,
    P: Provider<Handle = H>,
    S: Shell<H, P>,
{
    let (stdin, stdout) = console::handles()?;
    let mut keys = Keys::open(stdin, key_map)?;
    match GopDisplay::open() {
        Ok(display) => graphics(display, &mut keys, session, provider, shell),
        Err(_) => text(stdout, &mut keys, session, provider, shell),
    }
}

/// Serves the session on the graphics output.
fn graphics<H, P, S>(
    mut display: GopDisplay,
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
                    &format!(
                        "Graphics output {}x{}, scale {scale}, {}x{} cells, {}",
                        mode.width,
                        mode.height,
                        cells.width,
                        cells.height,
                        keys.kind()
                    ),
                );
                serve(&mut terminal, |area| area, keys, session, provider, shell)
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
        &format!(
            "Text console {}x{} cells, {}",
            cells.width,
            cells.height,
            keys.kind()
        ),
    );
    let served = serve(&mut terminal, margins, keys, session, provider, shell);
    let cleared = terminal.clear().map_err(|error| Error::from(error.status));
    let _ = terminal.show_cursor();
    served.and(cleared)
}

/// The page area of a text console, inside the margins of
/// [`forms::page::Margins::default`].
fn margins(area: Rect) -> Rect {
    let (left, top, width, height) =
        forms::page::Margins::default().area((u32::from(area.width), u32::from(area.height)));
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
    page: fn(Rect) -> Rect,
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
                    FormsWidget::new(&view).render(area, frame.buffer_mut());
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
        boot::stall(TURN);
    }
}
