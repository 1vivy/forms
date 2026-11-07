//! A browser front end for a [`forms`] session.
//!
//! The crate runs a real [`forms::Session`] and draws its [`forms::View`] with
//! [`forms::widgets::FormsWidget`] through `mousefood` and `embedded-graphics`,
//! exactly as a firmware front end does. The only difference is the pixel
//! target: a RAM framebuffer instead of a graphics output. A page therefore
//! looks in a browser exactly as it looks on the panel.
//!
//! - [`Config`] states the panel size, the font cell, the fewest columns and
//!   rows, and the margin. [`Config::placement`] returns where the page sits.
//! - [`Screen`] holds the RGB framebuffer of the panel and draws one view into
//!   it. [`Screen::pixels`] returns the pixels for a canvas.
//!
//! The crate holds no formset and no provider. A consumer supplies both, so an
//! outside crate renders its own pages with this front end.
//!
//! # Examples
//!
//! ```
//! use forms::widgets::FormsWidget;
//! use forms_sim::{Config, Screen};
//!
//! let config = Config { panel: (800, 600), ..Config::default() };
//! let screen = Screen::new(config);
//! assert_eq!(screen.pixels().len(), 800 * 600 * 3);
//! assert_eq!(config.placement().scale, 2);
//! // A consumer renders its own view:
//! let view = forms::View {
//!     path: Vec::new(),
//!     title: "Setup".into(),
//!     rows: Vec::new(),
//!     selected: None,
//!     help: String::new(),
//!     editor: None,
//!     message: None,
//!     modal: None,
//! };
//! let mut screen = Screen::new(config);
//! screen.draw(true, &view).expect("the view draws");
//! let _ = FormsWidget::new(&view);
//! ```

use core::convert::Infallible;

use forms::View;
use forms::page::{Placement, placement};
use forms::widgets::FormsWidget;
use mousefood::embedded_graphics::Pixel;
use mousefood::embedded_graphics::draw_target::DrawTarget;
use mousefood::embedded_graphics::geometry::{Dimensions, Point, Size};
use mousefood::embedded_graphics::pixelcolor::{Rgb888, RgbColor};
use mousefood::embedded_graphics::primitives::Rectangle;
use mousefood::{
    EmbeddedBackend, EmbeddedBackendConfig, TerminalAlignment, fonts::mono_6x10_atlas,
};
use ratatui_core::terminal::Terminal;
use ratatui_core::widgets::Widget;

mod sim;

#[cfg(feature = "demo")]
pub mod demo;

pub use sim::{Defaults, SHARED, Sim, describe, input, key, title};

/// Returns a sample set for examples and tests.
///
/// # Examples
///
/// ```
/// let set = forms_sim::sample();
/// assert_eq!(set.pages.len(), 1);
/// assert_eq!(set.pages[0].rows.len(), 3);
/// ```
#[must_use]
pub fn sample() -> forms::FormSet {
    use forms::{Builder, Choice, Control, Key};

    let mut builder = Builder::new(Key(0x33), "Sample").help("A sample page");
    builder.page("Sample");
    builder.row("Rows", Control::Heading);
    builder.row("Row", Control::Toggle).set_binding(Key(1));
    builder
        .row(
            "Mode",
            Control::Choice(vec![Choice::new("Fast", 1), Choice::new("Safe", 2)]),
        )
        .set_binding(Key(2));
    builder.build()
}

/// One panel and the page placement on it.
///
/// The defaults describe a phone panel of 1272 by 2772 pixels with the 6 by 10
/// cell that `mousefood` draws, forty columns, sixteen rows and a margin of one
/// tenth on every edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The panel size in pixels.
    pub panel: (u32, u32),
    /// One font cell in pixels.
    pub cell: (u32, u32),
    /// The fewest columns that the page keeps.
    pub columns: u32,
    /// The fewest rows that the page keeps.
    pub rows: u32,
    /// The divisor of the panel that each edge keeps. Zero keeps no margin.
    pub margin: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            panel: (1272, 2772),
            cell: (6, 10),
            columns: 40,
            rows: 16,
            margin: 10,
        }
    }
}

impl Config {
    /// Returns where the page sits on the panel.
    ///
    /// # Examples
    ///
    /// ```
    /// let config = forms_sim::Config::default();
    /// let page = config.placement();
    /// assert_eq!(page.scale, 4);
    /// ```
    #[must_use]
    pub fn placement(&self) -> Placement {
        placement(self.panel, self.cell, self.columns, self.rows, self.margin)
    }

    /// Returns the columns that the page shows.
    ///
    /// # Examples
    ///
    /// ```
    /// let config = forms_sim::Config::default();
    /// assert_eq!(config.placement().width / config.cell.0, 42);
    /// ```
    #[must_use]
    pub fn columns_shown(&self) -> u32 {
        self.placement().width / self.cell.0
    }
}

/// The RGB framebuffer of one panel.
///
/// The pixels run from the top-left corner, row after row, three bytes each, so
/// a canvas of the panel size shows them without conversion.
pub struct Screen {
    config: Config,
    placement: Placement,
    pixels: Vec<u8>,
}

impl Screen {
    /// Returns a black panel of the configured size.
    ///
    /// # Examples
    ///
    /// ```
    /// let screen = forms_sim::Screen::new(forms_sim::Config::default());
    /// assert_eq!(screen.pixels().len(), 1272 * 2772 * 3);
    /// assert_eq!(&screen.pixels()[..3], &[0, 0, 0]);
    /// ```
    #[must_use]
    pub fn new(config: Config) -> Self {
        let placement = config.placement();
        let pixels = vec![0u8; (config.panel.0 as usize) * (config.panel.1 as usize) * 3];
        Self {
            config,
            placement,
            pixels,
        }
    }

    /// Returns the configuration of the panel.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns where the page sits on the panel.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// Returns the RGB pixels of the whole panel.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns the panel size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.config.panel
    }

    /// Draws one view.
    ///
    /// `full` clears the whole page first. A front end that keeps the last
    /// frame sets it after the session changed the page or answered a popup,
    /// and clears it for a change of one row.
    ///
    /// # Errors
    ///
    /// Returns the backend failure of a terminal that cannot be created, of a
    /// clear, and of a draw.
    pub fn draw(&mut self, full: bool, view: &View) -> Result<(), mousefood::error::Error> {
        let mut surface = Surface {
            pixels: core::mem::take(&mut self.pixels),
            width: self.config.panel.0,
            height: self.config.panel.1,
            placement: self.placement,
        };
        let config = EmbeddedBackendConfig::<Surface, Rgb888> {
            font_regular: mono_6x10_atlas(),
            horizontal_alignment: TerminalAlignment::Center,
            vertical_alignment: TerminalAlignment::Center,
            ..Default::default()
        };
        let drawn =
            Terminal::new(EmbeddedBackend::new(&mut surface, config)).and_then(|mut terminal| {
                if full {
                    terminal.clear()?;
                }
                terminal
                    .draw(|frame| {
                        FormsWidget::new(view).render(frame.area(), frame.buffer_mut());
                    })
                    .map(|_| ())
            });
        self.pixels = surface.pixels;
        drawn?;
        Ok(())
    }

    /// Fills the whole panel with black.
    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }
}

/// One page drawn at a scale into the panel pixels.
struct Surface {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    placement: Placement,
}

impl Surface {
    /// Writes one page pixel as a `scale` by `scale` block of panel pixels.
    fn write(&mut self, point: Point, color: Rgb888) {
        let page = self.placement;
        let left = point.x;
        let top = point.y;
        if left < 0 || top < 0 {
            return;
        }
        let (left, top) = (left as u32, top as u32);
        if left >= page.width || top >= page.height {
            return;
        }
        let rgb = [color.r(), color.g(), color.b()];
        let scale = page.scale;
        for row in 0..scale {
            let y = page.top + top * scale + row;
            if y >= self.height {
                break;
            }
            for column in 0..scale {
                let x = page.left + left * scale + column;
                if x >= self.width {
                    continue;
                }
                let at = ((y as usize) * (self.width as usize) + (x as usize)) * 3;
                if let Some(pixel) = self.pixels.get_mut(at..at + 3) {
                    pixel.copy_from_slice(&rgb);
                }
            }
        }
    }
}

impl Dimensions for Surface {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(
            Point::zero(),
            Size::new(self.placement.width, self.placement.height),
        )
    }
}

impl DrawTarget for Surface {
    type Color = Rgb888;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            self.write(point, color);
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        // The colours run over `area` in row-major order, so every point of the
        // area consumes one colour even when the point is outside the page.
        let mut colors = colors.into_iter();
        for row in 0..area.size.height as i32 {
            for column in 0..area.size.width as i32 {
                let Some(color) = colors.next() else {
                    return Ok(());
                };
                let point = Point::new(area.top_left.x + column, area.top_left.y + row);
                self.write(point, color);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Screen, Surface};
    use forms::View;
    use mousefood::embedded_graphics::Pixel;
    use mousefood::embedded_graphics::draw_target::DrawTarget;
    use mousefood::embedded_graphics::geometry::Point;
    use mousefood::embedded_graphics::pixelcolor::Rgb888;

    fn view() -> View {
        View {
            path: Vec::new(),
            title: String::from("Setup"),
            rows: Vec::new(),
            selected: None,
            help: String::new(),
            editor: None,
            message: None,
            modal: None,
        }
    }

    #[test]
    fn one_page_pixel_becomes_a_block_of_panel_pixels() {
        let config = Config {
            panel: (800, 600),
            ..Config::default()
        };
        let page = config.placement();
        let mut surface = Surface {
            pixels: vec![0u8; 800 * 600 * 3],
            width: 800,
            height: 600,
            placement: page,
        };
        surface
            .draw_iter([Pixel(Point::new(1, 1), Rgb888::new(255, 0, 0))])
            .expect("the pixel writes");
        let pixels = surface.pixels;
        let x = page.left + page.scale;
        let y = page.top + page.scale;
        let at = ((y as usize) * 800 + (x as usize)) * 3;
        assert_eq!(&pixels[at..at + 3], &[255, 0, 0]);
        let beside = at + 3;
        assert_eq!(
            &pixels[beside..beside + 3],
            &[255, 0, 0],
            "the block is as wide as the scale"
        );
        let beyond = at + page.scale as usize * 3;
        assert_eq!(
            &pixels[beyond..beyond + 3],
            &[0, 0, 0],
            "the block stops at the scale"
        );
    }

    #[test]
    fn a_point_outside_the_page_writes_nothing() {
        let config = Config {
            panel: (800, 600),
            ..Config::default()
        };
        let page = config.placement();
        let mut pixels = vec![0u8; 800 * 600 * 3];
        {
            let mut surface = Surface {
                pixels,
                width: 800,
                height: 600,
                placement: page,
            };
            let outside = Point::new(page.width as i32 + 4, 0);
            surface
                .draw_iter([Pixel(outside, Rgb888::new(255, 255, 255))])
                .expect("the pixel writes");
            pixels = surface.pixels;
        }
        assert!(pixels.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_drawn_view_reaches_the_panel() {
        let mut screen = Screen::new(Config {
            panel: (800, 600),
            ..Config::default()
        });
        screen.draw(true, &view()).expect("the view draws");
        assert!(
            screen.pixels().iter().any(|byte| *byte != 0),
            "the page drew pixels"
        );
    }

    #[test]
    fn a_clear_returns_the_black_panel() {
        let mut screen = Screen::new(Config {
            panel: (800, 600),
            ..Config::default()
        });
        screen.draw(true, &view()).expect("the view draws");
        screen.clear();
        assert!(screen.pixels().iter().all(|byte| *byte == 0));
    }
}
