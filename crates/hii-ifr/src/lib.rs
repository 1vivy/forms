//! IFR and HII package codec for the `forms` model.
//!
//! This crate is an adapter. It turns a [`forms::FormSet`] and an
//! [`Export`] descriptor into the HII packages that a UEFI driver publishes,
//! and it turns a package list back into a form set with its descriptor.
//!
//! The `forms` model has no GUID, variable store, question identifier or
//! opcode. Those HII facts live in [`Export`], which the consumer supplies on
//! encode and receives on decode. [`descriptor`] builds one for a set that
//! uses a single buffer store.
//!
//! Controls that IFR cannot express degrade on export:
//!
//! | Control | IFR form |
//! | --- | --- |
//! | Progress | `TEXT` with the current value |
//! | Divider | An empty `SUBTITLE` |
//! | Consumer control | `TEXT` with the page label |
//! | Event refresh | `REFRESH_ID` when the descriptor names a GUID, otherwise omitted |
//! | Warning, edit and submit rules | `WARNING_IF`, `INCONSISTENT_IF`, `NO_SUBMIT_IF` |
//! | Disabled condition | `GRAY_OUT_IF`; a foreign `DISABLE_IF` decodes as `disabled` |
//! | Hidden condition | `SUPPRESS_IF` |
//! | Read-only condition | A static `EFI_IFR_FLAG_READ_ONLY`, because IFR has no read-only condition |
//!
//! The read-only round trip loses the condition: a row with a `read_only`
//! expression exports as a question with the static flag and decodes as a
//! locked row, so a rebuilt page cannot restore the expression.
//!
//! An opcode that the decoder does not model becomes a
//! [`forms::Control::Unsupported`] row, so a foreign row stays visible instead
//! of disappearing.
//!
//! The crate needs no firmware. It builds for a UEFI driver (`no_std`), a UEFI
//! application and a Linux program.
//!
//! # Examples
//!
//! ```
//! use forms::{Builder, Control, Key};
//!
//! let mut builder = Builder::new(Key(1), "Platform");
//! builder.page("Main");
//! builder.row("Secure boot", Control::Toggle).set_binding(Key(10));
//! let set = builder.build();
//! let export = hii_ifr::descriptor(&set, [0x42; 16]);
//! let package_list = hii_ifr::encode(&set, &export, [0x17; 16]).unwrap();
//! let decoded = hii_ifr::decode(&package_list).unwrap();
//! assert_eq!(decoded.len(), 1);
//! assert_eq!(decoded[0].export.guid, [0x42; 16]);
//! ```
#![no_std]

extern crate alloc;

pub mod config;
mod decode;
mod encode;
mod model;

use alloc::{string::String, vec::Vec};
use forms::{Key, Row, Value};

pub use decode::{DecodeError, Decoded};
pub use encode::EncodeError;
pub use model::{
    Export, FormRef, Guid, QuestionKind, QuestionRef, Storage, VarStore, VarStoreKind, Width,
    descriptor,
};

/// Encodes one message as string 2 of an English package list.
///
/// # Errors
///
/// Returns [`EncodeError::BadString`] for NUL characters or
/// [`EncodeError::TooLong`] when the package exceeds its length field.
///
/// # Examples
///
/// ```
/// let bytes = hii_ifr::encode_message("Continue?", [0x42; 16]).unwrap();
/// assert!(hii_ifr::decode(&bytes).unwrap().is_empty());
/// assert!(hii_ifr::encode_message("bad\0text", [0; 16]).is_err());
/// ```
pub fn encode_message(text: &str, package_list: Guid) -> Result<Vec<u8>, EncodeError> {
    encode::encode_message(text, package_list)
}

/// Encodes a form set and its descriptor as an HII package list.
///
/// # Errors
///
/// Returns [`EncodeError`] when a part of the set cannot be represented: an
/// opcode or package would exceed its length field, a string contains NUL, a
/// store name is not ASCII, a value does not fit its width, a condition has no
/// IFR form, a page or a value row has no descriptor entry, or the descriptor
/// declares more than three class GUIDs.
///
/// # Examples
///
/// ```
/// use forms::{Builder, FormSet, Key};
///
/// let set = Builder::new(Key(1), "P").build();
/// let export = hii_ifr::Export {
///     guid: [0; 16],
///     class_guids: Vec::new(),
///     varstores: Vec::new(),
///     forms: Vec::new(),
///     questions: Vec::new(),
///     refresh_events: Vec::new(),
/// };
/// let bytes = hii_ifr::encode(&set, &export, [0; 16]).unwrap();
/// assert_eq!(hii_ifr::decode(&bytes).unwrap().len(), 1);
/// ```
pub fn encode(
    set: &forms::FormSet,
    export: &Export,
    package_list: Guid,
) -> Result<Vec<u8>, EncodeError> {
    encode::encode(set, export, package_list)
}

/// Decodes every forms package of an HII package list.
///
/// Strings come from the strings packages of the same list. The `en-US`
/// package wins, then `en`, then the first package. A legacy EFI variable
/// store without the size field decodes its questions as named values.
///
/// # Errors
///
/// Returns [`DecodeError`] when the package list is shorter than its header, a
/// package or opcode length leaves its container, a package uses a reserved
/// system type, a condition scope is not closed, or a strings package is
/// malformed.
///
/// # Examples
///
/// ```
/// assert!(hii_ifr::decode(&[0u8; 19]).is_err());
/// ```
pub fn decode(package_list: &[u8]) -> Result<Vec<Decoded>, DecodeError> {
    decode::decode(package_list)
}

/// Returns the stored bytes of a value at the question width.
///
/// Numbers use little-endian bytes of the question width; a checkbox uses one
/// byte; text uses UTF-16 code units; [`Value::List`] uses one width per
/// chosen value.
///
/// # Examples
///
/// ```
/// use forms::{Control, Key, Row, Value};
///
/// let row = Row::new(Key(1), "Count", Control::Number {
///     minimum: 0,
///     maximum: u64::from(u16::MAX),
///     step: 1,
/// });
/// let bytes = hii_ifr::value_bytes(&row, &Value::Number(0x1234)).unwrap();
/// assert_eq!(bytes, vec![0x34, 0x12]);
/// ```
pub fn value_bytes(row: &Row, value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Bytes(bytes) => Some(bytes.clone()),
        Value::Bool(flag) => Some(alloc::vec![u8::from(*flag)]),
        Value::Number(number) => {
            let bytes = numeric_bytes(row)?;
            let width = bytes;
            if width < 8 && *number >> (width * 8) != 0 {
                return None;
            }
            Some(number.to_le_bytes()[..width].to_vec())
        }
        Value::Text(text) => {
            let maximum = text_maximum(row)?;
            let mut out = Vec::with_capacity(maximum * 2);
            for unit in text.encode_utf16() {
                if out.len() >= maximum * 2 {
                    return None;
                }
                out.extend_from_slice(&unit.to_le_bytes());
            }
            out.resize(maximum * 2, 0);
            Some(out)
        }
        Value::List(values) => {
            let width = numeric_bytes(row)?;
            let mut out = Vec::with_capacity(values.len() * width);
            for value in values {
                if width < 8 && *value >> (width * 8) != 0 {
                    return None;
                }
                out.extend_from_slice(&value.to_le_bytes()[..width]);
            }
            Some(out)
        }
    }
}

/// Reads a value from stored bytes.
///
/// This is the inverse of [`value_bytes`]. Text ends at the first NUL code
/// unit.
///
/// # Examples
///
/// ```
/// use forms::{Control, Key, Row, Value};
///
/// let row = Row::new(Key(1), "Mode", Control::Toggle);
/// assert_eq!(hii_ifr::value_from(&row, &[1]), Some(Value::Bool(true)));
/// assert_eq!(hii_ifr::value_from(&row, &[]), None);
/// ```
pub fn value_from(row: &Row, bytes: &[u8]) -> Option<Value> {
    match &row.control {
        forms::Control::Toggle => bytes.first().map(|byte| Value::Bool(*byte != 0)),
        forms::Control::Choice(_)
        | forms::Control::Number { .. }
        | forms::Control::Slider { .. } => {
            let width = numeric_bytes(row)?;
            if bytes.len() < width {
                return None;
            }
            Some(Value::Number(read_width(bytes, width)?))
        }
        forms::Control::TextInput { .. } => {
            if !bytes.len().is_multiple_of(2) {
                return None;
            }
            let units = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
            Some(Value::Text(utf16_to_string(
                units.take_while(|unit| *unit != 0),
            )))
        }
        forms::Control::OrderedList { .. } => {
            let width = numeric_bytes(row)?;
            let mut values = Vec::new();
            for chunk in bytes.chunks_exact(width) {
                values.push(read_width(chunk, width)?);
            }
            Some(Value::List(values))
        }
        _ => None,
    }
}

/// Returns the stored byte width of a numeric question.
fn numeric_bytes(row: &Row) -> Option<usize> {
    match &row.control {
        forms::Control::Number { maximum, .. } | forms::Control::Slider { maximum, .. } => {
            Some(Width::of(*maximum).bytes())
        }
        forms::Control::Choice(choices) => {
            Some(Width::of(choices.iter().map(|choice| choice.value).max().unwrap_or(0)).bytes())
        }
        forms::Control::OrderedList { .. } => Some(Width::U16.bytes()),
        _ => None,
    }
}

/// Returns the largest text length of a text question.
fn text_maximum(row: &Row) -> Option<usize> {
    match &row.control {
        forms::Control::TextInput { maximum, .. } => Some(*maximum),
        _ => None,
    }
}

/// Reads a little-endian number of a width.
fn read_width(bytes: &[u8], width: usize) -> Option<u64> {
    let bytes = bytes.get(..width)?;
    let mut number = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        number |= u64::from(*byte) << (index * 8);
    }
    Some(number)
}

/// Converts UTF-16 code units to a string, replacing unpaired surrogates.
fn utf16_to_string(units: impl IntoIterator<Item = u16>) -> String {
    core::char::decode_utf16(units)
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Returns the key of a decoded row, for a consumer that keeps no keys.
///
/// # Examples
///
/// ```
/// assert_eq!(hii_ifr::row_key(&forms::Row::new(forms::Key(3), "X", forms::Control::Toggle)), forms::Key(3));
/// ```
pub fn row_key(row: &Row) -> Key {
    row.key
}
