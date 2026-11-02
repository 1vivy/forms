//! UCS-2 conversion for the firmware boundary.
//!
//! Firmware strings are UTF-16 code unit sequences that end with a NUL. The
//! HII protocols call this encoding UCS-2; it covers the Basic Multilingual
//! Plane only. This module is the single place that converts between those
//! buffers and Rust text, so both sides of the boundary agree on the layout.

use alloc::{string::String, vec::Vec};
/// The longest firmware string this module converts, in code units.
const MAX_STRING: usize = 1 << 20;

/// Converts Rust text to a NUL-terminated UCS-2 buffer.
///
/// # Examples
///
/// ```
/// assert_eq!(hii_uefi::text::ucs2("Hi"), [0x48, 0x69, 0]);
/// ```
pub fn ucs2(text: &str) -> Vec<u16> {
    let mut buffer: Vec<u16> = text.encode_utf16().collect();
    buffer.push(0);
    buffer
}

/// Converts a NUL-terminated UCS-2 string to Rust text.
///
/// A null pointer yields an empty string. Characters that UCS-2 cannot hold are
/// replaced.
///
/// # Safety
///
/// `text` must be null, or point to a NUL-terminated UCS-2 string that stays
/// readable for the duration of the call.
///
/// # Examples
///
/// ```
/// let units = [0x48u16, 0x69, 0];
/// // SAFETY: The array is NUL terminated and readable.
/// assert_eq!(unsafe { hii_uefi::text::from_ucs2(units.as_ptr()) }, "Hi");
/// ```
pub unsafe fn from_ucs2(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    // SAFETY: The caller guarantees a readable NUL-terminated string.
    while length < MAX_STRING && unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` counts code units before the NUL.
    let units = unsafe { core::slice::from_raw_parts(text, length) };
    String::from_utf16_lossy(units)
}

#[cfg(test)]
mod tests {
    use super::{from_ucs2, ucs2};

    #[test]
    fn round_trip_keeps_text() {
        let units = ucs2("Platform setup");
        // SAFETY: `units` is NUL terminated and alive.
        assert_eq!(unsafe { from_ucs2(units.as_ptr()) }, "Platform setup");
    }

    #[test]
    fn from_ucs2_stops_at_nul() {
        let units = [0x41u16, 0x42, 0, 0x43];
        // SAFETY: The array is readable.
        assert_eq!(unsafe { from_ucs2(units.as_ptr()) }, "AB");
    }

    #[test]
    fn null_is_empty() {
        // SAFETY: A null pointer is allowed.
        assert_eq!(unsafe { from_ucs2(core::ptr::null()) }, "");
        assert_eq!(ucs2(""), [0]);
    }
}
