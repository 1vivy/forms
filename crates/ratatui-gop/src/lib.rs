//! A ratatui backend for the UEFI graphics output protocol.
//!
//! This crate draws a ratatui character grid on
//! `EFI_GRAPHICS_OUTPUT_PROTOCOL` with an integer-scaled bitmap font. On a
//! 1440-pixel-wide phone panel the backend picks the largest HII SYSFONT factor
//! that still leaves at least 40 columns, so a setup page is readable without a
//! display mode change.
//!
//! - [`GopBackend::new`] takes an opened graphics output protocol and an
//!   optional scale.
//! - [`GopBackend::with_insets`] keeps the grid inside a safe area with a top
//!   and bottom margin in physical pixels.
//! - [`GopBackend`] implements [`ratatui_core::backend::Backend`], so it works
//!   with `ratatui_core::Terminal` directly.
//!
//! The font path is ported from the owner's gobbl firmware console
//! (`legacy/gobbl/surfacer/console/src/render/font.rs`): HII SYSFONT lookup with
//! a bundled 5x7 bitmap fallback. The backend keeps glyphs as alpha masks and
//! composes each cell in the requested colours. It redraws only cells whose
//! content changed and blits each changed run of a row once, following the
//! dirty-cell approach of `legacy/gobbl/surfacer/console/src/render/text/renderer.rs`.
#![no_std]

extern crate alloc;

mod backend;
mod font;

pub use backend::GopBackend;
