//! Hexadecimal byte strings for HII name/value configuration.
//!
//! A name/value variable store carries each value as a hexadecimal byte string
//! in the configuration string. This module encodes and decodes that form. It
//! is pure, so the round trip is testable on the host.

use alloc::{string::String, vec::Vec};

/// Decodes a hexadecimal byte string.
///
/// Returns `None` for odd length or for a character that is not a hex digit.
///
/// # Examples
///
/// ```
/// # use hii_uefi::hex::decode;
/// assert_eq!(decode("0102"), Some(vec![1, 2]));
/// assert_eq!(decode("012"), None);
/// assert_eq!(decode("0g"), None);
/// ```
pub fn decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let digits = text.as_bytes();
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for pair in digits.chunks_exact(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        bytes.push((high * 16 + low) as u8);
    }
    Some(bytes)
}

/// Encodes bytes as a lower-case hexadecimal string.
///
/// # Examples
///
/// ```
/// # use hii_uefi::hex::encode;
/// assert_eq!(encode(&[0x01, 0xab]), "01ab");
/// assert_eq!(encode(&[]), "");
/// ```
pub fn encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    use alloc::{vec, vec::Vec};

    #[test]
    fn round_trip_keeps_bytes() {
        let bytes = [0x00, 0x01, 0x0a, 0xab, 0xff];
        assert_eq!(decode(&encode(&bytes)), Some(bytes.to_vec()));
    }

    #[test]
    fn encoding_is_lower_case() {
        assert_eq!(encode(&[0xab, 0xcd]), "abcd");
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn decoding_rejects_broken_input() {
        assert_eq!(decode(""), Some(Vec::new()));
        assert_eq!(decode("0"), None);
        assert_eq!(decode("zz"), None);
        assert_eq!(decode("aB"), Some(vec![0xab]));
    }
}
