//! The graphics display: the page drawn on the graphics output.
//!
//! `mousefood` renders the ratatui buffer with `embedded-graphics`, and this
//! module supplies the drawing target it renders into: one page buffer over the
//! live mode of `EFI_GRAPHICS_OUTPUT_PROTOCOL`.
//!
//! - [`GopDisplay`] owns the page buffer at the unscaled page size. It records
//!   the smallest rectangle that a draw changed, so one flush copies that
//!   rectangle and nothing else.
//! - The page sits inside the margins of the [`Frame`], measured in mode
//!   pixels, and is centred in the area that remains. Its own pixels are
//!   `scale` mode pixels wide and high, and [`page`] picks the largest integer
//!   scale that keeps the frame's columns and rows.
//! - A mode with a memory-mapped frame buffer receives the pixels through that
//!   buffer. A mode without one receives them from one `Blt` call per flush.
//!
//! The mode is read once and never changed, so the pixels keep their address
//! for the whole session.

use alloc::vec;
use alloc::vec::Vec;

use mousefood::embedded_graphics::Pixel;
use mousefood::embedded_graphics::draw_target::DrawTarget;
use mousefood::embedded_graphics::geometry::{Dimensions, Point, Size as PixelSize};
use mousefood::embedded_graphics::pixelcolor::{Rgb888, RgbColor};
use mousefood::embedded_graphics::primitives::Rectangle;
use uefi::boot::{self, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput, PixelFormat};
use uefi::{Error, Status};

use crate::{Frame, console};

/// The font cell in pixels, before the scale of [`page`]: the 6 by 10 atlas
/// that `mousefood` draws with.
const CELL: PixelSize = PixelSize::new(6, 10);

/// Where the page goes on one live mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Page {
    /// The top-left corner of the page, in mode pixels.
    pub(crate) origin: Point,
    /// How many mode pixels one page pixel covers, on each axis.
    pub(crate) scale: u32,
    /// The size of the page, in page pixels.
    pub(crate) size: PixelSize,
}

/// Returns the page of a mode of `mode` pixels for `frame`.
///
/// The scale is the largest integer that keeps the frame's columns and rows;
/// a mode too small for both keeps a scale of one. The page is centred in the
/// area inside the frame's margins.
pub(crate) fn page(mode: PixelSize, frame: &Frame) -> Page {
    let placement = forms::render::placement(
        (mode.width, mode.height),
        (CELL.width, CELL.height),
        frame.columns,
        frame.rows,
        frame.margins,
    );
    Page {
        origin: Point::new(placement.left as i32, placement.top as i32),
        scale: placement.scale,
        size: PixelSize::new(placement.width, placement.height),
    }
}

/// The page of one live mode, as a `mousefood` drawing target.
pub(crate) struct GopDisplay {
    output: ScopedProtocol<GraphicsOutput>,
    mode: PixelSize,
    page: Page,
    present: Present,
    buffer: Vec<Rgb888>,
    dirty: Option<Rectangle>,
    failure: Option<Status>,
}

/// How the pixels of a page reach the video signal.
enum Present {
    /// The mode has a memory-mapped frame buffer, so a copy writes the pixels.
    Memory(Framebuffer),
    /// The mode has no frame buffer, so the firmware copies the pixels.
    Firmware,
}

/// The memory-mapped frame buffer of a mode.
#[derive(Clone, Copy, Debug)]
struct Framebuffer {
    base: *mut u8,
    /// The number of pixels per scanline.
    stride: usize,
    /// The number of bytes the firmware reports for the frame buffer.
    bytes: usize,
    order: Order,
}

/// The byte order of one pixel of a frame buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Order {
    Rgb,
    Bgr,
}

impl Order {
    fn bytes(self, color: Rgb888) -> [u8; 4] {
        match self {
            Self::Rgb => [color.r(), color.g(), color.b(), 0],
            Self::Bgr => [color.b(), color.g(), color.r(), 0],
        }
    }
}

impl GopDisplay {
    /// Opens the live mode of the graphics output and blacks it.
    ///
    /// # Errors
    ///
    /// Returns the firmware failure when no handle publishes the graphics
    /// output, when the protocol cannot be opened, when the live mode has no
    /// pixel, and when the first copy fails.
    pub(crate) fn open(frame: &Frame) -> Result<Self, Error> {
        let handle = boot::get_handle_for_protocol::<GraphicsOutput>()?;
        let mut output = console::open::<GraphicsOutput>(handle)?;
        let info = output.current_mode_info();
        let (width, height) = info.resolution();
        let mode = PixelSize::new(
            u32::try_from(width).unwrap_or(u32::MAX),
            u32::try_from(height).unwrap_or(u32::MAX),
        );
        if mode.width == 0 || mode.height == 0 {
            return Err(Error::from(Status::UNSUPPORTED));
        }
        let present = match info.pixel_format() {
            PixelFormat::Rgb | PixelFormat::Bgr => {
                let order = if info.pixel_format() == PixelFormat::Rgb {
                    Order::Rgb
                } else {
                    Order::Bgr
                };
                let mut frame_buffer = output.frame_buffer();
                Present::Memory(Framebuffer {
                    base: frame_buffer.as_mut_ptr(),
                    stride: info.stride(),
                    bytes: frame_buffer.size(),
                    order,
                })
            }
            // A mode without a frame buffer, and a mode of a custom bitmask,
            // both go through the firmware.
            PixelFormat::Bitmask | PixelFormat::BltOnly => Present::Firmware,
        };
        let page = page(mode, frame);
        let mut display = Self {
            output,
            mode,
            page,
            present,
            buffer: vec![Rgb888::BLACK; page.size.width as usize * page.size.height as usize],
            dirty: None,
            failure: None,
        };
        display.black_out()?;
        Ok(display)
    }

    /// Returns the live mode and the scale of the page.
    pub(crate) fn geometry(&self) -> (PixelSize, u32) {
        (self.mode, self.page.scale)
    }

    /// Returns the failure of the last copy, if one failed.
    pub(crate) fn take_failure(&mut self) -> Option<Status> {
        self.failure.take()
    }

    /// Copies the changed part of the page to the video signal.
    ///
    /// The `mousefood` flush calls this. A failure is recorded, because the
    /// flush cannot return one, and the front end reports it after the draw.
    pub(crate) fn present(&mut self) {
        if let Err(error) = self.copy_dirty() {
            self.failure = Some(error.status());
        }
    }

    /// Blacks the whole mode and reports the failure of a draw.
    ///
    /// # Errors
    ///
    /// Returns the recorded failure of a copy, and the firmware failure of the
    /// clear itself.
    pub(crate) fn close(&mut self) -> uefi::Result {
        let recorded = match self.failure.take() {
            Some(status) => Err(Error::from(status)),
            None => Ok(()),
        };
        recorded.and(self.black_out())
    }

    fn copy_dirty(&mut self) -> uefi::Result {
        let Some(area) = self.dirty.take() else {
            return Ok(());
        };
        if area.size.width == 0 || area.size.height == 0 {
            return Ok(());
        }
        let scale = self.page.scale as usize;
        let dest = (
            self.page.origin.x as usize + area.top_left.x as usize * scale,
            self.page.origin.y as usize + area.top_left.y as usize * scale,
        );
        match &self.present {
            Present::Memory(_) => self.copy_framebuffer(area, dest),
            Present::Firmware => self.copy_firmware(area, dest),
        }
    }

    /// Writes the changed rectangle into the memory-mapped frame buffer.
    fn copy_framebuffer(&mut self, area: Rectangle, dest: (usize, usize)) -> uefi::Result {
        let Present::Memory(framebuffer) = &self.present else {
            return Err(Error::from(Status::UNSUPPORTED));
        };
        let Framebuffer {
            base,
            stride,
            bytes,
            order,
        } = *framebuffer;
        let scale = self.page.scale as usize;
        let mut line = vec![0u8; area.size.width as usize * scale * 4];
        for row in 0..area.size.height {
            self.fill_line(
                area.top_left.x as usize,
                area.top_left.y as u32 + row,
                &mut line,
                order,
            );
            for repeat in 0..scale {
                let y = dest.1 + row as usize * scale + repeat;
                let offset = y * stride * 4 + dest.0 * 4;
                if offset.saturating_add(line.len()) > bytes {
                    return Err(Error::from(Status::UNSUPPORTED));
                }
                // SAFETY: The mode reports a frame buffer of `bytes` bytes at
                // `base`, and the offset and length of this line stay inside
                // that range. The mode never changes during the session.
                unsafe {
                    core::ptr::copy_nonoverlapping(line.as_ptr(), base.add(offset), line.len())
                };
            }
        }
        Ok(())
    }

    /// Fills one line of `scale` copies of the pixels of one page row.
    fn fill_line(&self, first: usize, row: u32, line: &mut [u8], order: Order) {
        let scale = self.page.scale as usize;
        for (index, entry) in line.chunks_exact_mut(4).enumerate() {
            let column = first + index / scale;
            let color = self.pixel(column as i32, row as i32);
            entry.copy_from_slice(&order.bytes(color));
        }
    }

    /// Hands the changed rectangle, scaled, to the firmware.
    fn copy_firmware(&mut self, area: Rectangle, dest: (usize, usize)) -> uefi::Result {
        let scale = self.page.scale as usize;
        let dims = (
            area.size.width as usize * scale,
            area.size.height as usize * scale,
        );
        let mut packed: Vec<BltPixel> = Vec::with_capacity(dims.0 * dims.1);
        let mut line: Vec<BltPixel> = Vec::with_capacity(dims.0);
        for row in 0..area.size.height {
            line.clear();
            for column in 0..area.size.width {
                let color = self.pixel(
                    area.top_left.x + column as i32,
                    area.top_left.y + row as i32,
                );
                let pixel = BltPixel::new(color.r(), color.g(), color.b());
                for _ in 0..scale {
                    line.push(pixel);
                }
            }
            for _ in 0..scale {
                packed.extend_from_slice(&line);
            }
        }
        self.output.blt(BltOp::BufferToVideo {
            buffer: &packed,
            src: BltRegion::Full,
            dest,
            dims,
        })
    }

    /// Blacks the whole mode, margins included.
    fn black_out(&mut self) -> uefi::Result {
        match &self.present {
            Present::Memory(framebuffer) => {
                let length =
                    (framebuffer.stride * self.mode.height as usize * 4).min(framebuffer.bytes);
                // SAFETY: The mode reports the frame buffer in `bytes` bytes at
                // `base`, `length` stays inside that range, and a zero pixel is
                // black in both byte orders this crate accepts.
                unsafe { core::ptr::write_bytes(framebuffer.base, 0, length) };
                Ok(())
            }
            Present::Firmware => self.output.blt(BltOp::VideoFill {
                color: BltPixel::new(0, 0, 0),
                dest: (0, 0),
                dims: (self.mode.width as usize, self.mode.height as usize),
            }),
        }
    }

    /// Returns one pixel of the page, or black outside it.
    fn pixel(&self, x: i32, y: i32) -> Rgb888 {
        let (Ok(column), Ok(row)) = (usize::try_from(x), usize::try_from(y)) else {
            return Rgb888::BLACK;
        };
        if column >= self.page.size.width as usize || row >= self.page.size.height as usize {
            return Rgb888::BLACK;
        }
        let index = row * self.page.size.width as usize + column;
        self.buffer.get(index).copied().unwrap_or(Rgb888::BLACK)
    }

    /// Stores one pixel of the page and records it as changed.
    fn write(&mut self, point: Point, color: Rgb888) {
        let (Ok(column), Ok(row)) = (usize::try_from(point.x), usize::try_from(point.y)) else {
            return;
        };
        let width = self.page.size.width as usize;
        if column >= width || row >= self.page.size.height as usize {
            return;
        }
        let Some(slot) = self.buffer.get_mut(row * width + column) else {
            return;
        };
        if *slot == color {
            return;
        }
        *slot = color;
        let changed = Rectangle::new(point, PixelSize::new(1, 1));
        self.dirty = Some(match self.dirty {
            Some(area) => union(area, changed),
            None => changed,
        });
    }
}

impl Dimensions for GopDisplay {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), self.page.size)
    }
}

impl DrawTarget for GopDisplay {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

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
        let width = area.size.width as i32;
        for row in 0..area.size.height as i32 {
            for column in 0..width {
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

/// Returns the smallest rectangle that covers both `one` and `other`.
fn union(one: Rectangle, other: Rectangle) -> Rectangle {
    let left = one.top_left.x.min(other.top_left.x);
    let top = one.top_left.y.min(other.top_left.y);
    let right =
        (one.top_left.x + one.size.width as i32).max(other.top_left.x + other.size.width as i32);
    let bottom =
        (one.top_left.y + one.size.height as i32).max(other.top_left.y + other.size.height as i32);
    // `with_corners` names the last pixel of the rectangle, so the exclusive
    // right and bottom edges step back one pixel.
    Rectangle::with_corners(Point::new(left, top), Point::new(right - 1, bottom - 1))
}
