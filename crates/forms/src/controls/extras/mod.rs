//! Handlers of the controls only this crate defines.
//!
//! IFR cannot state them, so an HII export degrades them (see the `hii-ifr`
//! degrade table): a slider becomes a numeric question, a progress row and a
//! divider become text.

mod divider;
mod progress;
mod slider;

pub(super) use divider::Divider;
pub(super) use progress::Progress;
pub(super) use slider::Slider;
