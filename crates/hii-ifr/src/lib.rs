//! IFR and HII package codec for the shared forms model.
//!
//! This crate turns an [`forms::FormSet`] into the HII packages that a
//! UEFI driver publishes (a package list with one forms package, one `en-US`
//! strings package and an end package), and turns a package list back into
//! the model. It also builds and parses the ConfigHdr, ConfigRequest and
//! ConfigResp strings that read and write question values through the HII
//! configuration routing protocol.
//!
//! The crate needs no firmware: it builds for a UEFI driver (`no_std`), a
//! UEFI application and a Linux program. The raw layout constants are in the
//! `hii-abi` crate, so another crate can share them without the codec.
//!
//! # Examples
//!
//! ```
//! use forms::{Form, FormId, FormSet, Statement};
//!
//! let set = FormSet {
//!     guid: [0x42; 16],
//!     title: "Platform".into(),
//!     help: String::new(),
//!     class_guids: Vec::new(),
//!     varstores: Vec::new(),
//!     forms: vec![Form {
//!         id: FormId(1),
//!         title: "Platform".into(),
//!         items: vec![Statement::Subtitle { text: "State".into() }.into()],
//!     }],
//! };
//! let package_list = hii_ifr::encode(&set, [0x17; 16]).unwrap();
//! assert_eq!(hii_ifr::decode(&package_list).unwrap(), vec![set]);
//! ```
#![no_std]

extern crate alloc;

pub mod config;
mod decode;
mod encode;

use alloc::{string::String, vec, vec::Vec};
use forms::{FormSet, Guid, Statement, Value, Width};

pub use decode::DecodeError;
pub use encode::EncodeError;

/// Encodes a formset as an HII package list.
///
/// The package list holds the given list GUID, one forms package, one `en-US`
/// strings package with UCS-2 strings, and an end package. Strings are
/// deduplicated and numbered from 2; identifier 1 names the language.
///
/// # Errors
///
/// Returns [`EncodeError`] when a part of the model cannot be represented:
/// an opcode or package would exceed its length field, a string contains a
/// NUL character, a store name is not ASCII, a value does not fit its
/// question width, a condition uses [`forms::Expr::Unsupported`], or the
/// formset declares more than three class GUIDs.
///
/// # Examples
///
/// ```
/// use forms::{Form, FormId, FormSet};
///
/// let set = FormSet {
///     guid: [0; 16],
///     title: "T".into(),
///     help: String::new(),
///     class_guids: Vec::new(),
///     varstores: Vec::new(),
///     forms: vec![Form { id: FormId(1), title: "F".into(), items: Vec::new() }],
/// };
/// let bytes = hii_ifr::encode(&set, [0; 16]).unwrap();
/// assert_eq!(hii_ifr::decode(&bytes).unwrap().len(), 1);
/// ```
pub fn encode(set: &FormSet, package_list: Guid) -> Result<Vec<u8>, EncodeError> {
    encode::encode(set, package_list)
}

/// Decodes every forms package of an HII package list.
///
/// Strings are resolved from the strings packages of the same list. The
/// `en-US` package wins, then `en`, then the first package. Unknown package
/// types and opcodes are skipped: HII registration is additive, so a list may
/// hold data this crate does not model. SCSU string blocks are skipped with
/// their identifiers reserved, because only UCS-2 text is decoded. A legacy
/// EFI variable store without the size field decodes its questions as named
/// values; the UEFI 2.3.1 layout decodes them as byte offsets.
///
/// # Errors
///
/// Returns [`DecodeError`] when the package list is shorter than its header,
/// a package or opcode length leaves its container, a package uses a reserved
/// system type, a condition scope is not closed, or a strings package is
/// malformed. Values that reference a missing string or variable store decode
/// to empty text or [`forms::Storage::None`] instead of an error.
///
/// # Examples
///
/// ```
/// let empty = [0u8; 19];
/// assert!(hii_ifr::decode(&empty).is_err());
/// ```
pub fn decode(package_list: &[u8]) -> Result<Vec<FormSet>, DecodeError> {
    decode::decode(package_list)
}

/// Returns the stored bytes of a value at the question width.
///
/// Numbers use little-endian bytes of the question width; a checkbox uses one
/// byte; text uses UTF-16 code units, zero padded to `maximum * 2` bytes, which
/// is the block width that firmware uses for a string question;
/// [`Value::Bytes`] is returned unchanged.
///
/// # Examples
///
/// ```
/// use forms::{Question, QuestionFlags, QuestionId, Statement, Storage, Value, Width};
///
/// let statement = Statement::Numeric {
///     question: Question {
///         id: QuestionId(1),
///         prompt: String::new(),
///         help: String::new(),
///         storage: Storage::None,
///         flags: QuestionFlags::default(),
///     },
///     width: Width::U16,
///     minimum: 0,
///     maximum: 10,
///     step: 1,
/// };
/// assert_eq!(hii_ifr::value_bytes(&statement, &Value::Number(0x1234)), Some(vec![0x34, 0x12]));
/// assert_eq!(hii_ifr::value_bytes(&statement, &Value::Number(0x1_0000)), None);
/// ```
pub fn value_bytes(statement: &Statement, value: &Value) -> Option<Vec<u8>> {
    let Value::Bytes(bytes) = value else {
        return typed_bytes(statement, value);
    };
    Some(bytes.clone())
}

/// Reads a value from stored bytes.
///
/// This is the inverse of [`value_bytes`] for the typed values: it accepts
/// bytes at least as long as the value and ignores trailing bytes. Text ends
/// at the first NUL code unit.
///
/// # Examples
///
/// ```
/// use forms::{Question, QuestionFlags, QuestionId, Statement, Storage, Value, Width};
///
/// let statement = Statement::Checkbox {
///     question: Question {
///         id: QuestionId(1),
///         prompt: String::new(),
///         help: String::new(),
///         storage: Storage::None,
///         flags: QuestionFlags::default(),
///     },
///     default: false,
/// };
/// assert_eq!(hii_ifr::value_from(&statement, &[1]), Some(Value::Bool(true)));
/// assert_eq!(hii_ifr::value_from(&statement, &[]), None);
/// ```
pub fn value_from(statement: &Statement, bytes: &[u8]) -> Option<Value> {
    match statement {
        Statement::Checkbox { .. } => bytes.first().map(|byte| Value::Bool(*byte != 0)),
        Statement::OneOf { width, .. } | Statement::Numeric { width, .. } => {
            let width = width.bytes();
            if bytes.len() < width {
                return None;
            }
            let mut number = 0u64;
            for (index, byte) in bytes.iter().take(width).enumerate() {
                number |= u64::from(*byte) << (index * 8);
            }
            Some(Value::Number(number))
        }
        Statement::TextInput { .. } => {
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
        Statement::Subtitle { .. }
        | Statement::Text { .. }
        | Statement::Goto { .. }
        | Statement::Action { .. } => None,
    }
}

/// Returns the width a typed value is stored with.
fn typed_bytes(statement: &Statement, value: &Value) -> Option<Vec<u8>> {
    match (statement, value) {
        (Statement::Checkbox { .. }, Value::Bool(flag)) => Some(vec![u8::from(*flag)]),
        (
            Statement::OneOf { width, .. } | Statement::Numeric { width, .. },
            Value::Number(number),
        ) => width_bytes(*width, *number),
        (Statement::TextInput { maximum, .. }, Value::Text(text)) => text_bytes(text, *maximum),
        _ => None,
    }
}

/// Encodes a number at a width, rejecting values that do not fit.
fn width_bytes(width: Width, number: u64) -> Option<Vec<u8>> {
    let bytes = width.bytes();
    let bits = bytes * 8;
    if bits < 64 && number >> bits != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes);
    out.extend_from_slice(&number.to_le_bytes()[..bytes]);
    Some(out)
}

/// Encodes text as UTF-16 code units padded to the block width.
fn text_bytes(text: &str, maximum: u8) -> Option<Vec<u8>> {
    let maximum = usize::from(maximum);
    let mut out = Vec::with_capacity(maximum * 2);
    for (index, unit) in text.encode_utf16().enumerate() {
        if index == maximum {
            return None;
        }
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.resize(maximum * 2, 0);
    Some(out)
}

/// Converts UTF-16 code units to a string, replacing unpaired surrogates.
fn utf16_to_string(units: impl IntoIterator<Item = u16>) -> String {
    core::char::decode_utf16(units)
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}
