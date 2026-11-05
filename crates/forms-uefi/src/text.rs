//! A ratatui backend over the firmware text console.
//!
//! The text console has no useful colour depth and no cursor addressing beyond
//! `set_cursor_position`, so the backend writes every changed cell at its own
//! position. The current mode of the console is read once and kept: this
//! backend never changes the mode.

use alloc::vec::Vec;
use core::fmt;

use ratatui_core::backend::{Backend, ClearType, WindowSize};
use ratatui_core::buffer::Cell;
use ratatui_core::layout::{Position, Size};
use ratatui_core::style::Color;
use uefi::boot::ScopedProtocol;
use uefi::proto::console::text::{Color as TextColor, Output};
use uefi::{CStr16, Error, Handle, Status};

use crate::console;

/// A drawing failure on the text console.
#[derive(Debug)]
pub(crate) struct TextError {
    /// The firmware status of the failed call.
    pub(crate) status: Status,
}

impl fmt::Display for TextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "text console error: {}", self.status)
    }
}

impl core::error::Error for TextError {}

impl From<Error> for TextError {
    fn from(error: Error) -> Self {
        Self {
            status: error.status(),
        }
    }
}

/// Maps a ratatui colour to a firmware foreground colour.
fn foreground(color: Color) -> TextColor {
    match color {
        Color::Black => TextColor::Black,
        Color::Red => TextColor::Red,
        Color::Green => TextColor::Green,
        Color::Yellow | Color::LightYellow => TextColor::Yellow,
        Color::Blue => TextColor::Blue,
        Color::Magenta => TextColor::Magenta,
        Color::Cyan => TextColor::Cyan,
        Color::Gray => TextColor::LightGray,
        Color::DarkGray => TextColor::DarkGray,
        Color::LightRed => TextColor::LightRed,
        Color::LightGreen => TextColor::LightGreen,
        Color::LightBlue => TextColor::LightBlue,
        Color::LightMagenta => TextColor::LightMagenta,
        Color::LightCyan => TextColor::LightCyan,
        Color::White => TextColor::White,
        Color::Reset | Color::Indexed(_) | Color::Rgb(..) => TextColor::LightGray,
    }
}

/// Maps a ratatui colour to a firmware background colour.
///
/// The firmware accepts only the first eight colours as a background. Other
/// colours fall back to black so that text stays readable.
fn background(color: Color) -> TextColor {
    match color {
        Color::Black => TextColor::Black,
        Color::Red => TextColor::Red,
        Color::Green => TextColor::Green,
        Color::Yellow | Color::LightYellow => TextColor::Yellow,
        Color::Blue => TextColor::Blue,
        Color::Magenta => TextColor::Magenta,
        Color::Cyan => TextColor::Cyan,
        Color::Gray | Color::White => TextColor::LightGray,
        _ => TextColor::Black,
    }
}

/// A ratatui backend that writes to the firmware text console.
pub(crate) struct TextBackend {
    output: ScopedProtocol<Output>,
    size: Size,
    scratch: Vec<u16>,
}

impl TextBackend {
    /// Opens the console output and reads the mode the firmware already uses.
    ///
    /// # Errors
    ///
    /// Returns the firmware failure when the console cannot be opened, and
    /// [`Status::UNSUPPORTED`] when the console reports no current mode.
    pub(crate) fn open(handle: Handle) -> Result<Self, Error> {
        let output = console::open::<Output>(handle)?;
        let mode = output
            .current_mode()?
            .ok_or(Error::from(Status::UNSUPPORTED))?;
        let columns = u16::try_from(mode.columns()).unwrap_or(u16::MAX);
        let rows = u16::try_from(mode.rows()).unwrap_or(u16::MAX);
        Ok(Self {
            output,
            size: Size {
                width: columns,
                height: rows,
            },
            scratch: Vec::new(),
        })
    }

    fn blank(&mut self, column: u16, row: u16) -> Result<(), TextError> {
        self.output
            .set_cursor_position(usize::from(column), usize::from(row))?;
        self.output
            .set_color(TextColor::LightGray, TextColor::Black)?;
        self.output.output_string(uefi::cstr16!(" "))?;
        Ok(())
    }

    fn write_cell(&mut self, column: u16, row: u16, cell: &Cell) -> Result<(), TextError> {
        self.output
            .set_cursor_position(usize::from(column), usize::from(row))?;
        self.output
            .set_color(foreground(cell.fg), background(cell.bg))?;
        self.scratch.clear();
        self.scratch.extend(cell.symbol().encode_utf16());
        self.scratch.push(0);
        let text = CStr16::from_u16_with_nul(&self.scratch).map_err(|_| TextError {
            status: Status::INVALID_PARAMETER,
        })?;
        self.output.output_string(text)?;
        Ok(())
    }

    fn blank_row(&mut self, row: u16, first: u16, last: u16) -> Result<(), TextError> {
        for column in first..last.min(self.size.width) {
            self.blank(column, row)?;
        }
        Ok(())
    }
}

impl Backend for TextBackend {
    type Error = TextError;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (column, row, cell) in content {
            self.write_cell(column, row, cell)?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        cursor(self.output.enable_cursor(false))
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        cursor(self.output.enable_cursor(true))
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        let (column, row) = self.output.cursor_position();
        Ok(Position {
            x: u16::try_from(column).unwrap_or(u16::MAX),
            y: u16::try_from(row).unwrap_or(u16::MAX),
        })
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let position = position.into();
        self.output
            .set_cursor_position(usize::from(position.x), usize::from(position.y))?;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.output.clear()?;
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        let (columns, rows) = (self.size.width, self.size.height);
        let (column, row) = self.output.cursor_position();
        let (column, row) = (
            u16::try_from(column).unwrap_or(columns),
            u16::try_from(row).unwrap_or(rows),
        );
        match clear_type {
            ClearType::All => self.clear(),
            ClearType::CurrentLine => self.blank_row(row, 0, columns),
            ClearType::UntilNewLine => self.blank_row(row, column, columns),
            ClearType::BeforeCursor => {
                for line in 0..row.min(rows) {
                    self.blank_row(line, 0, columns)?;
                }
                self.blank_row(row, 0, column)
            }
            ClearType::AfterCursor => {
                self.blank_row(row, column, columns)?;
                for line in row.saturating_add(1)..rows {
                    self.blank_row(line, 0, columns)?;
                }
                Ok(())
            }
        }
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::ZERO,
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Accepts a console without cursor control.
///
/// `EnableCursor` may answer `EFI_UNSUPPORTED` (a serial console does). Ratatui
/// hides the cursor on every draw, so that status leaves the cursor as it is
/// instead of ending the page. Every other failure is reported.
fn cursor(result: uefi::Result) -> Result<(), TextError> {
    match result {
        Err(error) if error.status() == Status::UNSUPPORTED => Ok(()),
        other => Ok(other?),
    }
}
