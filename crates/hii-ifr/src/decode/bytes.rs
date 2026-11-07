//! Little-endian readers, IFR widths and opcode classes.

use super::*;

/// Reads a null-terminated ASCII name.
pub(super) fn ascii_name(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Reads a null-terminated ASCII string.
pub(super) fn c_string(bytes: &[u8]) -> String {
    ascii_name(bytes)
}

/// Reads a little-endian 16-bit number.
pub(super) fn read_u16(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Reads a little-endian 32-bit number.
pub(super) fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Reads a number at a width.
pub(super) fn read_width(data: &[u8], at: usize, width: Width) -> Option<u64> {
    let bytes = data.get(at..at + width.bytes())?;
    let mut number = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        number |= u64::from(*byte) << (index * 8);
    }
    Some(number)
}

/// Reads 16 GUID bytes.
pub(super) fn guid_at(data: &[u8], at: usize) -> Option<Guid> {
    let bytes = data.get(at..at + 16)?;
    let mut guid = [0u8; 16];
    guid.copy_from_slice(bytes);
    Some(guid)
}

/// Returns the width of the numeric size flags.
pub(super) fn width_of(flags: u8) -> Width {
    match flags & abi::flags::NUMERIC_SIZE {
        0 => Width::U8,
        1 => Width::U16,
        2 => Width::U32,
        _ => Width::U64,
    }
}

/// Returns the width of a numeric value type.
pub(super) fn numeric_width(kind: u8) -> Option<Width> {
    match kind {
        abi::value_type::NUM_SIZE_8 => Some(Width::U8),
        abi::value_type::NUM_SIZE_16 => Some(Width::U16),
        abi::value_type::NUM_SIZE_32 => Some(Width::U32),
        abi::value_type::NUM_SIZE_64 => Some(Width::U64),
        _ => None,
    }
}

/// Returns the string of an identifier.
pub(super) fn text_of(strings: &BTreeMap<u16, String>, id: Option<u16>) -> String {
    id.and_then(|id| strings.get(&id).cloned())
        .unwrap_or_default()
}

/// Tells whether an opcode is a question.
pub(super) fn is_question(code: u8) -> bool {
    matches!(
        code,
        abi::op::CHECKBOX
            | abi::op::ONE_OF
            | abi::op::NUMERIC
            | abi::op::STRING
            | abi::op::PASSWORD
            | abi::op::ORDERED_LIST
            | abi::op::ACTION
    )
}

/// Tells whether an opcode is a condition expression.
pub(super) fn is_expression(code: u8) -> bool {
    matches!(
        code,
        abi::op::TRUE
            | abi::op::FALSE
            | abi::op::NOT
            | abi::op::AND
            | abi::op::OR
            | abi::op::EQUAL
            | abi::op::LESS_THAN
            | abi::op::LESS_EQUAL
            | abi::op::EQ_ID_VAL
            | abi::op::EQ_ID_ID
            | abi::op::EQ_ID_VAL_LIST
            | abi::op::QUESTION_REF1
            | abi::op::UINT8
            | abi::op::UINT16
            | abi::op::UINT32
            | abi::op::UINT64
    )
}

/// Tells whether an opcode is a statement that the model represents.
pub(super) fn is_statement(code: u8) -> bool {
    is_question(code)
        || matches!(
            code,
            abi::op::SUBTITLE | abi::op::TEXT | abi::op::REF | abi::op::RESET_BUTTON
        )
}
