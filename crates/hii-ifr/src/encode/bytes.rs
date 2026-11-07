//! Little-endian writers and IFR widths.

use super::*;

/// Returns the numeric width of a question.
pub(super) fn numeric_width(question: &QuestionRef) -> Result<Width, EncodeError> {
    match question.kind {
        QuestionKind::Number(width) => Ok(width),
        QuestionKind::OrderedList => Ok(Width::U16),
        _ => Ok(Width::U16),
    }
}

/// Returns the value type flags of a width.
pub(super) fn value_type(width: Width) -> u8 {
    match width {
        Width::U8 => abi::value_type::NUM_SIZE_8,
        Width::U16 => abi::value_type::NUM_SIZE_16,
        Width::U32 => abi::value_type::NUM_SIZE_32,
        Width::U64 => abi::value_type::NUM_SIZE_64,
    }
}

/// Returns the width of a stored default.
pub(super) fn width_bytes(width: Width, value: u64) -> Result<Vec<u8>, EncodeError> {
    let bytes = width.bytes();
    if bytes < 8 && value >> (bytes * 8) != 0 {
        return Err(EncodeError::ValueOutOfRange);
    }
    Ok(value.to_le_bytes()[..bytes].to_vec())
}

/// Appends a number at a width, rejecting values that do not fit.
pub(super) fn push_width(out: &mut Vec<u8>, width: Width, value: u64) -> Result<(), EncodeError> {
    out.extend_from_slice(&width_bytes(width, value)?);
    Ok(())
}

/// Returns the largest value of a width.
pub(super) fn width_max(width: Width) -> u64 {
    match width {
        Width::U8 => u64::from(u8::MAX),
        Width::U16 => u64::from(u16::MAX),
        Width::U32 => u64::from(u32::MAX),
        Width::U64 => u64::MAX,
    }
}

/// Returns the numeric size flags of a width.
pub(super) fn width_flags(width: Width) -> u8 {
    match width {
        Width::U8 => 0x00,
        Width::U16 => 0x01,
        Width::U32 => 0x02,
        Width::U64 => 0x03,
    }
}

/// Appends a little-endian 16-bit number.
pub(super) fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a NUL-terminated UTF-16 string.
pub(super) fn push_ucs2(out: &mut Vec<u8>, text: &str) {
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
}
