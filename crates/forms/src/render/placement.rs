//! Where a page sits inside a display.
//!
//! A front end draws a setup page at a fixed cell size. The page is therefore a
//! fixed number of columns and rows, and a display of any size shows it at an
//! integer scale inside its margins. This module holds that arithmetic once, so
//! a firmware display and a host simulator place the page the same way.
//!
//! # Examples
//!
//! ```
//! use forms::render::{Margins, placement};
//!
//! // A 1272 by 2772 panel keeps one tenth on every edge.
//! let page = placement((1272, 2772), (6, 10), 40, 16, Margins::default());
//! assert_eq!(page.scale, 4);
//! assert_eq!(page.left, 128);
//! assert_eq!(page.top, 278);
//! ```

/// The margin of each edge, in per-mille of the panel.
///
/// `100` is one tenth, which is the default on every edge. A value above
/// `1000` is clamped, so a margin never removes the whole display.
///
/// # Examples
///
/// ```
/// use forms::render::Margins;
///
/// let margins = Margins { top: 50, right: 100, bottom: 150, left: 200 };
/// assert_eq!(margins.top, 50);
/// assert_eq!(Margins::default(), Margins::uniform(100));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Margins {
    /// The margin of the top edge, in per-mille of the panel height.
    pub top: u32,
    /// The margin of the right edge, in per-mille of the panel width.
    pub right: u32,
    /// The margin of the bottom edge, in per-mille of the panel height.
    pub bottom: u32,
    /// The margin of the left edge, in per-mille of the panel width.
    pub left: u32,
}

impl Margins {
    /// Returns the same margin on every edge.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::render::Margins;
    ///
    /// assert_eq!(Margins::uniform(0).left, 0);
    /// assert_eq!(Margins::uniform(2_000).right, 1_000);
    /// ```
    pub const fn uniform(per_mille: u32) -> Self {
        let value = if per_mille > 1_000 { 1_000 } else { per_mille };
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Returns the area that the margins keep, as left, top, width and height.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::render::Margins;
    ///
    /// assert_eq!(Margins::uniform(0).area((800, 600)), (0, 0, 800, 600));
    /// assert_eq!(
    ///     Margins::default().area((800, 600)),
    ///     (80, 60, 640, 480)
    /// );
    /// ```
    pub fn area(self, display: (u32, u32)) -> (u32, u32, u32, u32) {
        let left = self.left_of(display.0);
        let right = self.right_of(display.0);
        let top = self.top_of(display.1);
        let bottom = self.bottom_of(display.1);
        (
            left,
            top,
            display.0.saturating_sub(left).saturating_sub(right),
            display.1.saturating_sub(top).saturating_sub(bottom),
        )
    }

    /// Returns the number of pixels that the left edge keeps.
    fn left_of(self, width: u32) -> u32 {
        fraction(width, self.left)
    }

    /// Returns the number of pixels that the right edge keeps.
    fn right_of(self, width: u32) -> u32 {
        fraction(width, self.right)
    }

    /// Returns the number of pixels that the top edge keeps.
    fn top_of(self, height: u32) -> u32 {
        fraction(height, self.top)
    }

    /// Returns the number of pixels that the bottom edge keeps.
    fn bottom_of(self, height: u32) -> u32 {
        fraction(height, self.bottom)
    }
}

impl Default for Margins {
    fn default() -> Self {
        Self::uniform(100)
    }
}

/// Returns the part of a length that a per-mille value keeps.
fn fraction(length: u32, per_mille: u32) -> u32 {
    let per_mille = per_mille.min(1_000);
    u32::try_from(u64::from(length) * u64::from(per_mille) / 1_000).unwrap_or(length)
}

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
/// `margins` states the part of each edge that stays free. `columns` and
/// `rows` are the fewest that the page keeps. The scale is the largest integer
/// that keeps both inside the area that remains, and the page is centred in
/// that area.
///
/// # Examples
///
/// ```
/// use forms::render::{Margins, placement};
///
/// let page = placement((800, 600), (6, 10), 40, 16, Margins::default());
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
    margins: Margins,
) -> Placement {
    let (left, top, width, height) = margins.area(display);
    let across = width
        .checked_div(columns.saturating_mul(cell.0))
        .unwrap_or(0);
    let down = height.checked_div(rows.saturating_mul(cell.1)).unwrap_or(0);
    let scale = across.min(down).max(1);
    let page_width = width / scale;
    let page_height = height / scale;
    let page_left = left + (width - page_width.saturating_mul(scale)) / 2;
    let page_top = top + (height - page_height.saturating_mul(scale)) / 2;
    Placement {
        left: page_left,
        top: page_top,
        scale,
        width: page_width,
        height: page_height,
    }
}

#[cfg(test)]
mod tests {
    use super::{Margins, placement};

    #[test]
    fn a_panel_keeps_one_tenth_on_every_edge() {
        let page = placement((1272, 2772), (6, 10), 40, 16, Margins::default());
        assert!(page.left >= 127);
        assert!(page.top >= 277);
        assert_eq!(page.width * page.scale + page.left * 2, 1272);
        assert_eq!(page.height * page.scale + page.top * 2, 2772);
    }

    #[test]
    fn a_panel_keeps_at_least_forty_columns() {
        for display in [(1272, 2772), (800, 600)] {
            let page = placement(display, (6, 10), 40, 16, Margins::default());
            assert!(page.width / 6 >= 40, "display {display:?}");
            assert!(page.height / 10 >= 16, "display {display:?}");
        }
    }

    #[test]
    fn a_small_display_keeps_a_scale_of_one() {
        for display in [(1, 1), (100, 60), (160, 160), (240, 160)] {
            assert_eq!(
                placement(display, (6, 10), 40, 16, Margins::default()).scale,
                1,
                "display {display:?}"
            );
        }
    }

    #[test]
    fn the_page_stays_inside_the_display() {
        for width in [1, 10, 240, 640, 800, 1272, 1920, 4096] {
            for height in [1, 10, 160, 480, 600, 1080, 2772, 4096] {
                let page = placement((width, height), (6, 10), 40, 16, Margins::default());
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
        let page = placement((240, 160), (6, 10), 40, 16, Margins::uniform(0));
        assert_eq!(page.left, 0);
        assert_eq!(page.top, 0);
    }

    #[test]
    fn the_area_of_a_margin_set_is_the_shared_rectangle() {
        let (left, top, width, height) = Margins::default().area((1272, 2772));
        assert_eq!((left, top), (127, 277));
        assert_eq!(left + width, 1272 - 127);
        assert_eq!(top + height, 2772 - 277);
        // `placement` draws inside that rectangle.
        let page = placement((1272, 2772), (6, 10), 40, 16, Margins::default());
        assert!(page.left >= left);
        assert!(page.top >= top);
        assert!(page.left + page.width * page.scale <= left + width);
        assert!(page.top + page.height * page.scale <= top + height);
    }

    #[test]
    fn each_edge_keeps_its_own_margin() {
        let margins = Margins {
            top: 0,
            right: 0,
            bottom: 0,
            left: 500,
        };
        let page = placement((800, 600), (6, 10), 40, 16, margins);
        // The left half of the panel stays free, so the page starts there.
        assert!(page.left >= 400);
        assert!(page.left + page.width * page.scale <= 800);
        let wide = Margins {
            top: 0,
            right: 0,
            bottom: 500,
            left: 0,
        };
        let page = placement((800, 600), (6, 10), 40, 16, wide);
        assert!(page.top + page.height * page.scale <= 320);
    }
}
