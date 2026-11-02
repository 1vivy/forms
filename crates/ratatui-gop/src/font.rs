//! Glyph source for the graphics backend.
//!
//! Ported from the owner's gobbl firmware console
//! (`legacy/gobbl/surfacer/console/src/render/font.rs`). The HII SYSFONT
//! qualification, the `sysfont<N>x` factor naming and the 5x7 bitmap fallback
//! patterns come from that file. This port keeps a glyph as an alpha mask, so
//! the caller can compose it in any pair of colours, and extends the fallback
//! with the box-drawing glyphs that a ratatui border needs.
//!
//! The text renderer in `legacy/gobbl/surfacer/console/src/render/text/renderer.rs`
//! contributed the ideas that a cell is only redrawn when its content changed
//! and that the cursor is drawn and removed around a repaint.

use alloc::{collections::BTreeMap, vec, vec::Vec};
use core::ptr::NonNull;
use uefi::{
    Identify,
    boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol, SearchType},
    proto::unsafe_protocol,
};
use uefi_raw::protocol::{
    console::GraphicsOutputBltPixel,
    hii::{
        font::{
            FontDisplayInfo, FontInfo, FontInfoMask, HiiFontProtocol, HiiFontStyle, HiiOutFlags,
        },
        image::{ImageOutput, ImageOutputDest},
    },
};

/// The glyph cell of the HII default font, in pixels (`sysfont<N>x` multiplies it).
pub(crate) const BASE_CELL_WIDTH: usize = 8;
/// The height of the HII default font cell, in pixels.
pub(crate) const BASE_CELL_HEIGHT: usize = 19;

/// The largest SYSFONT factor this backend tries.
pub(crate) const MAX_SCALE: u8 = 8;

/// `sysfont3x` is the canonical SYSFONT name; later factors change one digit.
const SYSFONT_3X: &[u16] = uefi::cstr16!("sysfont3x").to_u16_slice_with_nul();

/// The glyph cell of a font at `scale`.
pub(crate) const fn cell(scale: u8) -> (usize, usize) {
    (
        BASE_CELL_WIDTH * scale as usize,
        BASE_CELL_HEIGHT * scale as usize,
    )
}

/// One rasterized glyph.
pub(crate) struct Glyph {
    /// One coverage byte per pixel, row-major, `0` transparent and `255` opaque.
    pub(crate) alpha: Vec<u8>,
}

/// The glyph source of one backend: HII SYSFONT when the firmware qualifies it,
/// the bundled bitmap otherwise.
pub(crate) struct Font {
    scale: u8,
    cell: (usize, usize),
    hii: Option<HiiFont>,
    glyphs: BTreeMap<char, Glyph>,
}

impl Font {
    /// Prepares a font of one scale, with a cached glyph table.
    pub(crate) fn new(scale: u8) -> Self {
        let scale = scale.clamp(1, MAX_SCALE);
        Self {
            scale,
            cell: cell(scale),
            hii: HiiFont::qualify(scale),
            glyphs: BTreeMap::new(),
        }
    }

    /// Returns the glyph cell of this font, in pixels.
    pub(crate) const fn cell(&self) -> (usize, usize) {
        self.cell
    }

    /// Returns the glyph for `ch`, rendering it on first use.
    pub(crate) fn glyph(&mut self, ch: char) -> &Glyph {
        if !self.glyphs.contains_key(&ch) {
            let glyph = self.render(ch);
            self.glyphs.insert(ch, glyph);
        }
        self.glyphs.get(&ch).expect("the glyph was inserted above")
    }

    /// Renders one glyph from the firmware font, or from the bundled bitmap.
    fn render(&mut self, ch: char) -> Glyph {
        if let Some(hii) = &mut self.hii
            && let Some(glyph) = hii.render(ch, self.scale)
        {
            return glyph;
        }
        Glyph {
            alpha: fallback(ch, self.cell.0, self.cell.1, self.scale),
        }
    }
}

/// The HII font protocol wrapper. HII SYSFONT rendering stays explicit while
/// discovery and protocol lifetime use the standard boot services scope.
#[repr(transparent)]
#[unsafe_protocol("e9ca4775-8657-47fc-97e7-7ed65a084324")]
struct FontProtocol(HiiFontProtocol);

/// A located HII font provider.
struct HiiFont {
    protocol: ScopedProtocol<FontProtocol>,
}

/// Extra storage for the flexible `font_name` array of `EFI_FONT_INFO`.
#[repr(C)]
struct NamedDisplayInfo {
    info: FontDisplayInfo,
    tail: [u16; 10],
}

impl HiiFont {
    /// Locates an HII font provider and checks that it renders `scale`'s cell.
    fn qualify(scale: u8) -> Option<Self> {
        let mut font = Self {
            protocol: locate()?,
        };
        let expected = cell(scale);
        let usable = ['A', 'g'].into_iter().all(|ch| {
            font.render(ch, scale)
                .is_some_and(|glyph| glyph.alpha.len() == expected.0.saturating_mul(expected.1))
        });
        usable.then_some(font)
    }

    /// Renders one glyph at `scale`, or returns `None` when the provider does
    /// not produce exactly that cell.
    fn render(&mut self, ch: char, scale: u8) -> Option<Glyph> {
        let (width, height) = cell(scale);
        let code = u16::try_from(u32::from(ch)).ok()?;
        let name = sysfont_name(scale);
        let mut pixels = vec![
            GraphicsOutputBltPixel {
                blue: 0,
                green: 0,
                red: 0,
                reserved: 0,
            };
            width * height
        ];
        let mut output = ImageOutput {
            width: u16::try_from(width).ok()?,
            height: u16::try_from(height).ok()?,
            image: ImageOutputDest {
                bitmap: pixels.as_mut_ptr(),
            },
        };
        let original: *mut ImageOutput = &mut output;
        let mut output_pointer = original;
        let mut rows: *mut uefi_raw::protocol::hii::font::HiiRowInfo = core::ptr::null_mut();
        let mut row_count = 0usize;
        let mut text = [code, 0];
        let mut info = NamedDisplayInfo {
            info: FontDisplayInfo {
                foreground_color: white(),
                background_color: black(),
                font_mask_info: FontInfoMask::ANY_SIZE | FontInfoMask::ANY_STYLE,
                font_info: FontInfo {
                    font_style: HiiFontStyle::NORMAL,
                    font_size: 0,
                    font_name: [],
                },
            },
            tail: [0; 10],
        };
        // SAFETY: the flexible member starts within `info`'s trailing padding and
        // `tail` provides room for all ten initialized name units.
        unsafe {
            core::ptr::addr_of_mut!(info.info.font_info.font_name)
                .cast::<u16>()
                .copy_from_nonoverlapping(name.as_ptr(), name.len());
        }
        let protocol: &HiiFontProtocol = &self.protocol.0;
        // SAFETY: the protocol outlives this call, the text is NUL terminated, the
        // display info carries the whole font name, and every buffer is writable
        // for the declared extents.
        let status = unsafe {
            (protocol.string_to_image)(
                protocol,
                HiiOutFlags::CLIP
                    | HiiOutFlags::CLIP_CLEAN_X
                    | HiiOutFlags::CLIP_CLEAN_Y
                    | HiiOutFlags::IGNORE_LINE_BREAK,
                text.as_mut_ptr(),
                &info.info,
                &mut output_pointer,
                0,
                0,
                &mut rows,
                &mut row_count,
                core::ptr::null_mut(),
            )
        };
        // The output dimensions describe our buffer, not the selected font.
        // Firmware can substitute its unscaled default for a missing SYSFONT.
        let metrics_match = if !status.is_error() && row_count == 1 && !rows.is_null() {
            // SAFETY: Successful StringToImage returned one allocated row record.
            let row = unsafe { &*rows };
            row.line_width == width && row.line_height == height
        } else {
            false
        };
        if let Some(rows) = NonNull::new(rows.cast::<u8>()) {
            // SAFETY: StringToImage allocated the row array from the pool.
            let _ = unsafe { boot::free_pool(rows) };
        }
        if status.is_error()
            || output_pointer != original
            || !metrics_match
            || usize::from(output.width) != width
            || usize::from(output.height) != height
        {
            return None;
        }
        let alpha = pixels
            .into_iter()
            .map(|pixel| u8::max(u8::max(pixel.red, pixel.green), pixel.blue))
            .collect();
        Some(Glyph { alpha })
    }
}

/// Locates the first HII font provider on the system.
fn locate() -> Option<ScopedProtocol<FontProtocol>> {
    let handles = boot::locate_handle_buffer(SearchType::ByProtocol(&FontProtocol::GUID)).ok()?;
    for &handle in handles.iter() {
        let params = OpenProtocolParams {
            handle,
            agent: boot::image_handle(),
            controller: None,
        };
        // SAFETY: HII is shared with the firmware on the EFI CPU and is never
        // disconnected, so opening it for the lifetime of the backend is sound.
        let opened = unsafe {
            boot::open_protocol::<FontProtocol>(params, OpenProtocolAttributes::GetProtocol)
        };
        if let Ok(protocol) = opened {
            return Some(protocol);
        }
    }
    None
}

/// Returns the SYSFONT name of a factor; factor 1 is the default HII font.
fn sysfont_name(scale: u8) -> [u16; 10] {
    let mut name = [0; 10];
    name.copy_from_slice(SYSFONT_3X);
    if scale == 1 {
        name.fill(0);
    } else {
        name[7] = u16::from(b'0') + u16::from(scale);
    }
    name
}

/// Returns a white pixel.
const fn white() -> GraphicsOutputBltPixel {
    GraphicsOutputBltPixel {
        blue: 0xff,
        green: 0xff,
        red: 0xff,
        reserved: 0,
    }
}

/// Returns a black pixel.
const fn black() -> GraphicsOutputBltPixel {
    GraphicsOutputBltPixel {
        blue: 0,
        green: 0,
        red: 0,
        reserved: 0,
    }
}

/// Renders one glyph of the bundled bitmap fallback into `width` x `height`.
fn fallback(ch: char, width: usize, height: usize, scale: u8) -> Vec<u8> {
    let mut alpha = vec![0u8; width.saturating_mul(height)];
    if ch == '\u{2588}' {
        alpha.fill(0xff);
        return alpha;
    }
    if let Some(lines) = box_lines(ch) {
        draw_box(&mut alpha, width, height, scale as usize, lines);
        return alpha;
    }
    let rows = pattern(ch).unwrap_or_else(|| pattern('?').unwrap_or([0; 7]));
    // The canonical 3x cell (24 x 57) is 5 x 7 cells of 4 x 7 pixels with the
    // pattern centred; the same proportions scale to every factor.
    let step_x = (width / 6).max(1);
    let step_y = (height / 8).max(1);
    let x0 = (width - 5 * step_x) / 2;
    let y0 = (height - 7 * step_y) / 2;
    for (row, bits) in rows.into_iter().enumerate() {
        for column in 0..5 {
            if bits & (1 << (4 - column)) != 0 {
                let left = x0 + column * step_x;
                let top = y0 + row * step_y;
                fill(&mut alpha, width, left, top, left + step_x, top + step_y);
            }
        }
    }
    alpha
}

/// The half-lines of a box-drawing glyph: left, right, up, down.
type BoxLines = (bool, bool, bool, bool);

/// Returns the half-lines of a box-drawing glyph that a border uses.
fn box_lines(ch: char) -> Option<BoxLines> {
    Some(match ch {
        '\u{2500}' => (true, true, false, false),
        '\u{2502}' => (false, false, true, true),
        '\u{250c}' => (false, true, false, true),
        '\u{2510}' => (true, false, false, true),
        '\u{2514}' => (false, true, true, false),
        '\u{2518}' => (true, false, true, false),
        '\u{251c}' => (false, true, true, true),
        '\u{2524}' => (true, false, true, true),
        '\u{252c}' => (true, true, false, true),
        '\u{2534}' => (true, true, true, false),
        '\u{253c}' => (true, true, true, true),
        _ => return None,
    })
}

/// Draws the half-lines of a box glyph as strokes through the cell centre.
fn draw_box(alpha: &mut [u8], width: usize, height: usize, scale: usize, lines: BoxLines) {
    let (left, right, up, down) = lines;
    let stroke = scale.max(1);
    let x0 = width / 2 - stroke / 2;
    let x1 = x0 + stroke;
    let y0 = height / 2 - stroke / 2;
    let y1 = y0 + stroke;
    if left {
        fill(alpha, width, 0, y0, x1, y1);
    }
    if right {
        fill(alpha, width, x0, y0, width, y1);
    }
    if up {
        fill(alpha, width, x0, 0, x1, y1);
    }
    if down {
        fill(alpha, width, x0, y0, x1, height);
    }
}

/// Sets the alpha of one rectangle, clipped to the cell.
fn fill(alpha: &mut [u8], width: usize, x0: usize, y0: usize, x1: usize, y1: usize) {
    let height = alpha.len() / width.max(1);
    for y in y0..y1.min(height) {
        for x in x0..x1.min(width) {
            alpha[y * width + x] = 0xff;
        }
    }
}

/// The bundled 5x7 column-bit pattern of one glyph.
///
/// The repertoire is printable ASCII, U+0020 to U+007E, drawn as the uppercase
/// glyph for lowercase input, plus the box-drawing glyphs handled by
/// [`box_lines`]. `None` means the fallback has no glyph.
fn pattern(glyph: char) -> Option<[u8; 7]> {
    let ascii = u8::try_from(u32::from(glyph)).ok()?;
    let upper = ascii.to_ascii_uppercase();
    Some(match upper {
        b' ' => [0, 0, 0, 0, 0, 0, 0],
        b'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        b'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        b'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        b'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        b'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        b'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
        b'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'I' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1f],
        b'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0e],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        b'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        b'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        b'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        b'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        b'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        b'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        b'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        b'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        b'0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        b'1' => [0x04, 0x0c, 0x14, 0x04, 0x04, 0x04, 0x1f],
        b'2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        b'3' => [0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e],
        b'4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        b'5' => [0x1f, 0x10, 0x10, 0x1e, 0x01, 0x01, 0x1e],
        b'6' => [0x0e, 0x10, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        b'7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        b'9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x01, 0x0e],
        b'-' => [0, 0, 0, 0x1f, 0, 0, 0],
        b'_' => [0, 0, 0, 0, 0, 0, 0x1f],
        b'.' => [0, 0, 0, 0, 0, 0x0c, 0x0c],
        b':' => [0, 0x0c, 0x0c, 0, 0x0c, 0x0c, 0],
        b'/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        b'\\' => [0x10, 0x08, 0x08, 0x04, 0x02, 0x02, 0x01],
        b'>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        b'<' => [0x01, 0x02, 0x04, 0x08, 0x04, 0x02, 0x01],
        b'=' => [0, 0x1f, 0, 0x1f, 0, 0, 0],
        b'+' => [0, 0x04, 0x04, 0x1f, 0x04, 0x04, 0],
        b'!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0, 0x04],
        b'?' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
        b'[' => [0x0e, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0e],
        b']' => [0x0e, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0e],
        b'(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        b'|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'"' => [0x0a, 0x0a, 0x0a, 0, 0, 0, 0],
        b'\'' => [0x04, 0x04, 0x08, 0, 0, 0, 0],
        b'*' => [0, 0x15, 0x0e, 0x1f, 0x0e, 0x15, 0],
        b'#' => [0x0a, 0x0a, 0x1f, 0x0a, 0x1f, 0x0a, 0x0a],
        b'$' => [0x04, 0x0f, 0x14, 0x0e, 0x05, 0x1e, 0x04],
        b'%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        b'&' => [0x0c, 0x12, 0x14, 0x08, 0x15, 0x12, 0x0d],
        b',' => [0, 0, 0, 0, 0x0c, 0x04, 0x08],
        b';' => [0, 0x0c, 0x0c, 0, 0x0c, 0x04, 0x08],
        b'@' => [0x0e, 0x11, 0x17, 0x15, 0x17, 0x10, 0x0e],
        b'^' => [0x04, 0x0a, 0x11, 0, 0, 0, 0],
        b'`' => [0x08, 0x04, 0x02, 0, 0, 0, 0],
        b'{' => [0x02, 0x04, 0x04, 0x08, 0x04, 0x04, 0x02],
        b'}' => [0x08, 0x04, 0x04, 0x02, 0x04, 0x04, 0x08],
        b'~' => [0, 0, 0x09, 0x16, 0, 0, 0],
        _ => return None,
    })
}
