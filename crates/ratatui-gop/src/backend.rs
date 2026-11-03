//! A ratatui backend that draws on the UEFI graphics output protocol.
//!
//! The backend keeps an integer-scaled bitmap font. On a 1440-pixel-wide phone
//! panel it picks the largest SYSFONT factor that still leaves at least 40
//! columns, so text stays readable without a display mode change. Only cells
//! whose content changed since the last frame are composed, and each changed
//! run of one row reaches the display with a single `Blt` call. The dirty-cell
//! and row-blit approach follows the owner's gobbl text renderer
//! (`legacy/gobbl/surfacer/console/src/render/text/renderer.rs`).

use alloc::{vec, vec::Vec};
use ratatui_core::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
    style::{Color, Modifier},
};
use uefi::{
    Result, Status,
    boot::ScopedProtocol,
    proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput},
};

use crate::font::{self, Font, MAX_SCALE};

/// The grid keeps at least this many columns when the backend chooses a scale.
const MIN_COLUMNS: usize = 40;

/// A ratatui backend that draws a character grid on the UEFI graphics output.
///
/// The grid is a fixed whole number of glyph cells. Each cell is composed from
/// an alpha mask, so any foreground and background colour pair works.
///
/// # Examples
///
/// ```no_run
/// use ratatui_core::Terminal;
/// use ratatui_gop::GopBackend;
/// use uefi::boot;
///
/// let gop = boot::open_protocol_exclusive::<uefi::proto::console::gop::GraphicsOutput>(
///     boot::get_handle_for_protocol::<uefi::proto::console::gop::GraphicsOutput>()
///         .expect("no graphics output"),
/// )
/// .expect("cannot open the graphics output");
/// let backend = GopBackend::new(gop, None).expect("no usable mode");
/// let mut terminal = Terminal::new(backend).expect("cannot start the terminal");
/// assert!(terminal.size().expect("no size").height > 0);
/// ```
pub struct GopBackend {
    gop: ScopedProtocol<GraphicsOutput>,
    font: Font,
    columns: u16,
    rows: u16,
    pixels: (usize, usize),
    /// The top safe-area margin in physical pixels; the grid starts there.
    top: usize,
    cursor: Position,
    cursor_visible: bool,
    drawn_cursor: Option<Position>,
    screen: Vec<Option<CellSig>>,
    /// Reusable storage for the largest run of one character row.
    scratch: Vec<BltPixel>,
}

impl GopBackend {
    /// Starts a backend on `gop`.
    ///
    /// `scale` selects the SYSFONT factor, which sets the glyph cell to
    /// `8 * scale` by `19 * scale` pixels. `None` chooses the largest factor
    /// that keeps at least 40 columns and one row.
    ///
    /// The grid starts at the top-left corner of the mode. Use
    /// [`GopBackend::with_insets`] when the panel hides an edge.
    ///
    /// # Errors
    ///
    /// Returns [`Status::UNSUPPORTED`] when the current mode is smaller than
    /// one glyph cell.
    ///
    /// # Panics
    ///
    /// Does not panic. A mode with a `Blt`-only pixel format is supported.
    pub fn new(gop: ScopedProtocol<GraphicsOutput>, scale: Option<u8>) -> Result<Self> {
        Self::with_insets(gop, scale, 0, 0)
    }

    /// Starts a backend on `gop` inside a safe area.
    ///
    /// `top` and `bottom` are margins in physical pixels. The grid keeps the
    /// mode width and drops the rows that cross either margin. A full clear
    /// also clears the margins so previous console text does not remain there.
    /// An inset that would hide the last row is shrunk.
    ///
    /// Use this for a panel whose edges are hidden, and derive the margins from
    /// the live mode, never from a fixed panel geometry.
    ///
    /// # Errors
    ///
    /// Returns [`Status::UNSUPPORTED`] when the current mode is smaller than
    /// one glyph cell.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ratatui_core::Terminal;
    /// use ratatui_gop::GopBackend;
    /// use uefi::boot;
    ///
    /// let gop = boot::open_protocol_exclusive::<uefi::proto::console::gop::GraphicsOutput>(
    ///     boot::get_handle_for_protocol::<uefi::proto::console::gop::GraphicsOutput>()
    ///         .expect("no graphics output"),
    /// )
    /// .expect("cannot open the graphics output");
    /// // A 46/1000 top margin of a 2772-pixel panel, and four pixels at the bottom.
    /// let backend = GopBackend::with_insets(gop, None, 127, 4).expect("no usable mode");
    /// let mut terminal = Terminal::new(backend).expect("cannot start the terminal");
    /// assert!(terminal.size().expect("no size").height > 0);
    /// ```
    pub fn with_insets(
        gop: ScopedProtocol<GraphicsOutput>,
        scale: Option<u8>,
        top: usize,
        bottom: usize,
    ) -> Result<Self> {
        let info = gop.current_mode_info();
        let (width, height) = info.resolution();
        let scale = scale
            .map(|scale| scale.clamp(1, MAX_SCALE))
            .unwrap_or_else(|| choose_scale(width, height));
        let (cell_width, cell_height) = font::cell(scale);
        if width < cell_width || height < cell_height {
            return Err(Status::UNSUPPORTED.into());
        }
        // Keep one row: an inset that would hide the whole grid is shrunk.
        let top = top.min(height - cell_height);
        let bottom = bottom.min(height - top - cell_height);
        let columns = u16::try_from(width / cell_width).unwrap_or(u16::MAX);
        let rows = u16::try_from((height - top - bottom) / cell_height).unwrap_or(u16::MAX);
        let screen = vec![None; usize::from(columns) * usize::from(rows)];
        let mut backend = Self {
            gop,
            font: Font::new(scale),
            columns,
            rows,
            pixels: (width, height),
            top,
            cursor: Position { x: 0, y: 0 },
            cursor_visible: false,
            drawn_cursor: None,
            screen,
            scratch: vec![BltPixel::new(0, 0, 0); usize::from(columns) * cell_width * cell_height],
        };
        backend.clear()?;
        Ok(backend)
    }

    /// Returns the number of columns of the character grid.
    #[must_use]
    pub const fn columns(&self) -> u16 {
        self.columns
    }

    /// Returns the number of rows of the character grid.
    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows
    }

    /// Returns the index of one grid cell in the screen cache.
    fn index(&self, x: u16, y: u16) -> usize {
        usize::from(y) * usize::from(self.columns) + usize::from(x)
    }

    /// Composes and blits one run of one row.
    ///
    /// When `invert` names a column of the run, that cell is drawn with the
    /// foreground and background colours swapped; that is the cursor.
    fn paint_run(&mut self, y: u16, x0: u16, x1: u16, invert: Option<u16>) -> Result<()> {
        if x0 >= x1 {
            return Ok(());
        }
        let (cell_width, cell_height) = self.font.cell();
        let count = usize::from(x1 - x0);
        let width = count * cell_width;
        for (offset, x) in (x0..x1).enumerate() {
            let signature = self.screen[self.index(x, y)].unwrap_or_default();
            let glyph = self.font.glyph(signature.character);
            let inverted =
                invert.is_some_and(|column| usize::from(column) == usize::from(x0) + offset);
            let (foreground, background) = if inverted {
                (signature.background, signature.foreground)
            } else {
                (signature.foreground, signature.background)
            };
            let base = offset * cell_width;
            for row in 0..cell_height {
                for column in 0..cell_width {
                    let coverage = glyph.alpha[row * cell_width + column];
                    let mut pixel = blend(background, foreground, coverage);
                    if signature.underline && row + (cell_height / 19).max(1) >= cell_height {
                        pixel = BltPixel::new(foreground.red, foreground.green, foreground.blue);
                    }
                    self.scratch[row * width + base + column] = pixel;
                }
            }
        }
        self.gop.blt(BltOp::BufferToVideo {
            buffer: &self.scratch[..width * cell_height],
            src: BltRegion::Full,
            dest: (
                usize::from(x0) * cell_width,
                self.top + usize::from(y) * cell_height,
            ),
            dims: (width, cell_height),
        })
    }

    /// Restores the cell under a drawn cursor and forgets it.
    fn remove_cursor(&mut self) -> Result<()> {
        if let Some(position) = self.drawn_cursor.take() {
            self.paint_run(position.y, position.x, position.x + 1, None)?;
        }
        Ok(())
    }
}

impl Backend for GopBackend {
    type Error = uefi::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let cursor = self.drawn_cursor.take();
        if let Some(position) = cursor {
            self.paint_run(position.y, position.x, position.x + 1, None)?;
        }
        let mut dirty = vec![false; self.screen.len()];
        for (x, y, cell) in content {
            if x >= self.columns || y >= self.rows {
                continue;
            }
            let signature = CellSig::new(cell);
            let index = self.index(x, y);
            if self.screen[index] != Some(signature) {
                self.screen[index] = Some(signature);
                dirty[index] = true;
            }
        }
        for y in 0..self.rows {
            let mut x = 0;
            while x < self.columns {
                if !dirty[self.index(x, y)] {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < self.columns && dirty[self.index(x, y)] {
                    x += 1;
                }
                self.paint_run(y, start, x, None)?;
            }
        }
        if let Some(position) = cursor {
            self.cursor = position;
            self.show_cursor()?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<()> {
        self.remove_cursor()?;
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<()> {
        if self.cursor.x >= self.columns || self.cursor.y >= self.rows {
            return Ok(());
        }
        if self.drawn_cursor == Some(self.cursor) {
            self.cursor_visible = true;
            return Ok(());
        }
        self.remove_cursor()?;
        let position = self.cursor;
        self.paint_run(position.y, position.x, position.x + 1, Some(position.x))?;
        self.drawn_cursor = Some(position);
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<()> {
        self.cursor = position.into();
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        self.drawn_cursor = None;
        self.cursor_visible = false;
        let empty = CellSig::default();
        self.screen.fill(Some(empty));
        self.gop.blt(BltOp::VideoFill {
            color: BltPixel::new(
                empty.background.red,
                empty.background.green,
                empty.background.blue,
            ),
            dest: (0, 0),
            dims: self.pixels,
        })
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<()> {
        self.drawn_cursor = None;
        self.cursor_visible = false;
        let empty = CellSig::default();
        let cursor = self.cursor;
        let (first_row, last_row) = match clear_type {
            ClearType::All => (0, self.rows),
            ClearType::BeforeCursor => (0, cursor.y.saturating_add(1).min(self.rows)),
            ClearType::AfterCursor => (cursor.y.min(self.rows), self.rows),
            ClearType::CurrentLine | ClearType::UntilNewLine => (
                cursor.y.min(self.rows),
                cursor.y.saturating_add(1).min(self.rows),
            ),
        };
        for y in first_row..last_row {
            let (first_column, last_column) = match clear_type {
                ClearType::UntilNewLine => (cursor.x.min(self.columns), self.columns),
                ClearType::BeforeCursor if y == cursor.y => {
                    (0, cursor.x.saturating_add(1).min(self.columns))
                }
                ClearType::AfterCursor if y == cursor.y => {
                    (cursor.x.min(self.columns), self.columns)
                }
                _ => (0, self.columns),
            };
            if first_column >= last_column {
                continue;
            }
            let base = self.index(first_column, y);
            let end = self.index(last_column, y);
            self.screen[base..end].fill(Some(empty));
            self.paint_run(y, first_column, last_column, None)?;
        }
        Ok(())
    }

    fn size(&self) -> Result<Size> {
        Ok(Size {
            width: self.columns,
            height: self.rows,
        })
    }

    fn window_size(&mut self) -> Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: Size {
                width: self.columns,
                height: self.rows,
            },
            pixels: Size {
                width: u16::try_from(self.pixels.0).unwrap_or(u16::MAX),
                height: u16::try_from(self.pixels.1).unwrap_or(u16::MAX),
            },
        })
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// The largest SYSFONT factor that keeps at least [`MIN_COLUMNS`] columns.
fn choose_scale(width: usize, height: usize) -> u8 {
    let mut chosen = 1;
    for scale in 1..=MAX_SCALE {
        let (cell_width, cell_height) = font::cell(scale);
        if width / cell_width < MIN_COLUMNS || height / cell_height < 1 {
            break;
        }
        chosen = scale;
    }
    chosen
}

/// One RGB colour in the form the blit buffer takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rgb {
    red: u8,
    green: u8,
    blue: u8,
}

impl Rgb {
    /// White, the fallback foreground.
    const WHITE: Self = Self {
        red: 0xff,
        green: 0xff,
        blue: 0xff,
    };
    /// Black, the fallback background.
    const BLACK: Self = Self {
        red: 0,
        green: 0,
        blue: 0,
    };

    /// Returns `self` mixed with `other` by `other`'s weight in 0..=255.
    fn mix(self, other: Self, weight: u8) -> Self {
        let blend = |background: u8, foreground: u8| {
            let foreground = foreground as u32 * weight as u32;
            let background = background as u32 * (255 - weight as u32);
            ((foreground + background) / 255) as u8
        };
        Self {
            red: blend(self.red, other.red),
            green: blend(self.green, other.green),
            blue: blend(self.blue, other.blue),
        }
    }

    /// Returns a brightened variant for bold text.
    fn brighten(self) -> Self {
        let add = |value: u8| value.saturating_add(64);
        Self {
            red: add(self.red),
            green: add(self.green),
            blue: add(self.blue),
        }
    }

    /// Returns a darkened variant for dim text.
    fn dim(self) -> Self {
        let scale = |value: u8| (value as u32 * 2 / 3) as u8;
        Self {
            red: scale(self.red),
            green: scale(self.green),
            blue: scale(self.blue),
        }
    }
}

/// What one cell looked like when it was last drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellSig {
    character: char,
    foreground: Rgb,
    background: Rgb,
    underline: bool,
}

impl Default for Rgb {
    fn default() -> Self {
        Self::WHITE
    }
}

impl Default for CellSig {
    fn default() -> Self {
        Self {
            character: ' ',
            foreground: Rgb::WHITE,
            background: Rgb::BLACK,
            underline: false,
        }
    }
}

impl CellSig {
    /// Resolves the colours and modifiers of one buffer cell.
    fn new(cell: &Cell) -> Self {
        let mut foreground = colour(cell.fg, Rgb::WHITE);
        let mut background = colour(cell.bg, Rgb::BLACK);
        if cell.modifier.contains(Modifier::REVERSED) {
            core::mem::swap(&mut foreground, &mut background);
        }
        if cell.modifier.contains(Modifier::BOLD) {
            foreground = foreground.brighten();
        }
        if cell.modifier.contains(Modifier::DIM) {
            foreground = foreground.dim();
        }
        Self {
            character: cell.symbol().chars().next().unwrap_or(' '),
            foreground,
            background,
            underline: cell.modifier.contains(Modifier::UNDERLINED),
        }
    }
}

/// Returns a buffer colour as RGB; `fallback` is used for `Color::Reset`.
fn colour(color: Color, fallback: Rgb) -> Rgb {
    match color {
        Color::Reset => fallback,
        Color::Black => Rgb {
            red: 0,
            green: 0,
            blue: 0,
        },
        Color::Red => Rgb {
            red: 0xaa,
            green: 0,
            blue: 0,
        },
        Color::Green => Rgb {
            red: 0,
            green: 0xaa,
            blue: 0,
        },
        Color::Yellow => Rgb {
            red: 0xaa,
            green: 0x55,
            blue: 0,
        },
        Color::Blue => Rgb {
            red: 0,
            green: 0,
            blue: 0xaa,
        },
        Color::Magenta => Rgb {
            red: 0xaa,
            green: 0,
            blue: 0xaa,
        },
        Color::Cyan => Rgb {
            red: 0,
            green: 0xaa,
            blue: 0xaa,
        },
        Color::Gray => Rgb {
            red: 0xaa,
            green: 0xaa,
            blue: 0xaa,
        },
        Color::DarkGray => Rgb {
            red: 0x55,
            green: 0x55,
            blue: 0x55,
        },
        Color::LightRed => Rgb {
            red: 0xff,
            green: 0x55,
            blue: 0x55,
        },
        Color::LightGreen => Rgb {
            red: 0x55,
            green: 0xff,
            blue: 0x55,
        },
        Color::LightYellow => Rgb {
            red: 0xff,
            green: 0xff,
            blue: 0x55,
        },
        Color::LightBlue => Rgb {
            red: 0x55,
            green: 0x55,
            blue: 0xff,
        },
        Color::LightMagenta => Rgb {
            red: 0xff,
            green: 0x55,
            blue: 0xff,
        },
        Color::LightCyan => Rgb {
            red: 0x55,
            green: 0xff,
            blue: 0xff,
        },
        Color::White => Rgb::WHITE,
        Color::Rgb(red, green, blue) => Rgb { red, green, blue },
        Color::Indexed(index) => indexed(index),
    }
}

/// Returns an xterm 256-colour palette entry as RGB.
fn indexed(index: u8) -> Rgb {
    const BASIC: [Rgb; 16] = [
        Rgb {
            red: 0,
            green: 0,
            blue: 0,
        },
        Rgb {
            red: 0xaa,
            green: 0,
            blue: 0,
        },
        Rgb {
            red: 0,
            green: 0xaa,
            blue: 0,
        },
        Rgb {
            red: 0xaa,
            green: 0x55,
            blue: 0,
        },
        Rgb {
            red: 0,
            green: 0,
            blue: 0xaa,
        },
        Rgb {
            red: 0xaa,
            green: 0,
            blue: 0xaa,
        },
        Rgb {
            red: 0,
            green: 0xaa,
            blue: 0xaa,
        },
        Rgb {
            red: 0xaa,
            green: 0xaa,
            blue: 0xaa,
        },
        Rgb {
            red: 0x55,
            green: 0x55,
            blue: 0x55,
        },
        Rgb {
            red: 0xff,
            green: 0x55,
            blue: 0x55,
        },
        Rgb {
            red: 0x55,
            green: 0xff,
            blue: 0x55,
        },
        Rgb {
            red: 0xff,
            green: 0xff,
            blue: 0x55,
        },
        Rgb {
            red: 0x55,
            green: 0x55,
            blue: 0xff,
        },
        Rgb {
            red: 0xff,
            green: 0x55,
            blue: 0xff,
        },
        Rgb {
            red: 0x55,
            green: 0xff,
            blue: 0xff,
        },
        Rgb {
            red: 0xff,
            green: 0xff,
            blue: 0xff,
        },
    ];
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match index {
        0..=15 => BASIC[usize::from(index)],
        16..=231 => {
            let value = usize::from(index) - 16;
            Rgb {
                red: LEVELS[value / 36],
                green: LEVELS[(value % 36) / 6],
                blue: LEVELS[value % 6],
            }
        }
        _ => {
            let level = 8 + (usize::from(index) - 232) * 10;
            let level = u8::try_from(level).unwrap_or(u8::MAX);
            Rgb {
                red: level,
                green: level,
                blue: level,
            }
        }
    }
}

/// Mixes `foreground` over `background` with one coverage byte.
fn blend(background: Rgb, foreground: Rgb, coverage: u8) -> BltPixel {
    let colour = background.mix(foreground, coverage);
    BltPixel::new(colour.red, colour.green, colour.blue)
}
