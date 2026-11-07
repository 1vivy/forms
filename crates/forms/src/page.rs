//! Where a page sits inside a display.
//!
//! A front end draws a setup page at a fixed cell size. The page is therefore a
//! fixed number of columns and rows, and a display of any size shows it at an
//! integer scale inside a margin. This module holds that arithmetic once, so a
//! firmware display and a host simulator place the page the same way.
//!
//! # Examples
//!
//! ```
//! use forms::page::placement;
//!
//! // A 1272 by 2772 panel keeps one tenth on every edge.
//! let page = placement((1272, 2772), (6, 10), 40, 16, 10);
//! assert_eq!(page.scale, 4);
//! assert_eq!(page.left, 128);
//! assert_eq!(page.top, 278);
//! ```

/// One page inside a display, in display pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The left edge of the page.
    pub left: u32,
    /// The top edge of the page.
    pub top: u32,
    /// How many display pixels one page pixel covers, on each axis.
    pub scale: u32,
    /// The width of the page, in page pixels.
    pub width: u32,
    /// The height of the page, in page pixels.
    pub height: u32,
}

/// Returns the page of a display, in page pixels.
///
/// `margin` is the divisor of the display that each edge keeps, so `10` keeps
/// one tenth and `0` keeps no margin. `columns` and `rows` are the fewest that
/// the page keeps. The scale is the largest integer that keeps both inside the
/// display, and the page is centred in the area that remains.
///
/// # Examples
///
/// ```
/// use forms::page::placement;
///
/// let page = placement((800, 600), (6, 10), 40, 16, 10);
/// assert_eq!(page.scale, 2);
/// assert!(page.left + page.width * page.scale <= 800);
/// assert!(page.top + page.height * page.scale <= 600);
/// ```
#[must_use]
pub fn placement(
    display: (u32, u32),
    cell: (u32, u32),
    columns: u32,
    rows: u32,
    margin: u32,
) -> Placement {
    let margin_x = display.0.checked_div(margin).unwrap_or(0);
    let margin_y = display.1.checked_div(margin).unwrap_or(0);
    let width = display.0.saturating_sub(margin_x.saturating_mul(2));
    let height = display.1.saturating_sub(margin_y.saturating_mul(2));
    let across = width
        .checked_div(columns.saturating_mul(cell.0))
        .unwrap_or(0);
    let down = height.checked_div(rows.saturating_mul(cell.1)).unwrap_or(0);
    let scale = across.min(down).max(1);
    let page_width = width / scale;
    let page_height = height / scale;
    let left = margin_x + (width - page_width.saturating_mul(scale)) / 2;
    let top = margin_y + (height - page_height.saturating_mul(scale)) / 2;
    Placement {
        left,
        top,
        scale,
        width: page_width,
        height: page_height,
    }
}

#[cfg(test)]
mod tests {
    use super::placement;

    #[test]
    fn a_panel_keeps_one_tenth_on_every_edge() {
        let page = placement((1272, 2772), (6, 10), 40, 16, 10);
        assert!(page.left >= 127);
        assert!(page.top >= 277);
        assert_eq!(page.width * page.scale + page.left * 2, 1272);
        assert_eq!(page.height * page.scale + page.top * 2, 2772);
    }

    #[test]
    fn a_panel_keeps_at_least_forty_columns() {
        for display in [(1272, 2772), (800, 600)] {
            let page = placement(display, (6, 10), 40, 16, 10);
            assert!(page.width / 6 >= 40, "display {display:?}");
            assert!(page.height / 10 >= 16, "display {display:?}");
        }
    }

    #[test]
    fn a_small_display_keeps_a_scale_of_one() {
        for display in [(1, 1), (100, 60), (160, 160), (240, 160)] {
            assert_eq!(
                placement(display, (6, 10), 40, 16, 10).scale,
                1,
                "display {display:?}"
            );
        }
    }

    #[test]
    fn the_page_stays_inside_the_display() {
        for width in [1, 10, 240, 640, 800, 1272, 1920, 4096] {
            for height in [1, 10, 160, 480, 600, 1080, 2772, 4096] {
                let page = placement((width, height), (6, 10), 40, 16, 10);
                assert!(
                    page.left + page.width * page.scale <= width,
                    "display {width}x{height}"
                );
                assert!(
                    page.top + page.height * page.scale <= height,
                    "display {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn no_margin_uses_the_whole_display() {
        let page = placement((240, 160), (6, 10), 40, 16, 0);
        assert_eq!(page.left, 0);
        assert_eq!(page.top, 0);
    }
}
