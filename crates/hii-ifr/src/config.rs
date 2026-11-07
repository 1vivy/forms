//! ConfigHdr, ConfigRequest and ConfigResp strings (UEFI 2.10 chapter 35).
//!
//! A configuration string uses lower-case hexadecimal digits. The
//! `<ConfigHdr>` names a variable store by GUID, UCS-2 name and device path.
//! Buffer requests and responses add `&OFFSET`, `&WIDTH` and `&VALUE`
//! elements. Name/value requests and responses add bare names.
//!
//! Every function returns an owned `String`; callers convert it to UCS-2
//! before they pass it to firmware.
//!
//! # Examples
//!
//! ```
//! use hii_ifr::config::{self, Block};
//!
//! let header = config::header(&[0x42; 16], "Store", &[0x11, 0x22]);
//! let request = config::request(&header, &[(0, 2)]);
//! assert_eq!(request, format!("{header}&OFFSET=0&WIDTH=2"));
//! let response = config::response(&header, &[Block { offset: 0, width: 2, bytes: vec![0x34, 0x12] }]);
//! assert_eq!(config::parse_blocks(&response).unwrap()[0].bytes, vec![0x34, 0x12]);
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::{self, Write};

use crate::model::Guid;

/// One block of a `<ConfigResp>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The byte offset of the block in the variable store.
    pub offset: u16,
    /// The block size in bytes.
    pub width: u16,
    /// The block value.
    pub bytes: Vec<u8>,
}

/// A failure while building or parsing a configuration string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A required element was missing.
    MissingField(&'static str),
    /// A number was not hexadecimal or did not fit 16 bits.
    BadNumber(&'static str),
    /// A value was not an even-length hexadecimal byte string.
    BadHex(&'static str),
    /// An element had no `=` or was not `OFFSET`, `WIDTH` or `VALUE`.
    BadElement,
    /// A `VALUE` length did not match its `WIDTH`.
    WidthMismatch {
        /// The declared width in bytes.
        width: usize,
        /// The number of decoded value bytes.
        bytes: usize,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField(field) => write!(formatter, "missing field {field}"),
            Self::BadNumber(field) => write!(formatter, "bad number in {field}"),
            Self::BadHex(field) => write!(formatter, "bad hexadecimal value in {field}"),
            Self::BadElement => formatter.write_str("unsupported configuration element"),
            Self::WidthMismatch { width, bytes } => {
                write!(
                    formatter,
                    "value has {bytes} bytes but the width is {width}"
                )
            }
        }
    }
}

impl core::error::Error for ConfigError {}

/// Builds a `<ConfigHdr>`.
///
/// The GUID uses its EFI byte order. The name is written as four hex digits
/// per UTF-16 code unit. The device path is written as hex bytes. An empty
/// name or path leaves the element empty after `=`.
///
/// # Examples
///
/// ```
/// let header = hii_ifr::config::header(&[0; 16], "AB", &[]);
/// assert_eq!(header, format!("GUID={}&NAME=00410042&PATH=", "00".repeat(16)));
/// ```
pub fn header(guid: &Guid, name: &str, device_path: &[u8]) -> String {
    let mut out = String::from("GUID=");
    push_hex(&mut out, guid);
    out.push_str("&NAME=");
    for unit in name.encode_utf16() {
        push_hex_u16(&mut out, unit);
    }
    out.push_str("&PATH=");
    push_hex(&mut out, device_path);
    out
}

/// Builds a `<ConfigRequest>` for buffer offsets.
///
/// # Examples
///
/// ```
/// let request = hii_ifr::config::request("H", &[(0x10, 2), (0x20, 1)]);
/// assert_eq!(request, "H&OFFSET=10&WIDTH=2&OFFSET=20&WIDTH=1");
/// ```
pub fn request(header: &str, blocks: &[(u16, u16)]) -> String {
    let mut out = String::from(header);
    for (offset, width) in blocks {
        let _ = write!(out, "&OFFSET={offset:x}&WIDTH={width:x}");
    }
    out
}

/// Builds a `<ConfigResp>` for buffer offsets.
///
/// The `VALUE` element holds the bytes of each block. The declared width is
/// copied unchanged.
///
/// # Examples
///
/// ```
/// use hii_ifr::config::{response, Block};
///
/// let text = response("H", &[Block { offset: 1, width: 2, bytes: vec![0xab, 0xcd] }]);
/// assert_eq!(text, "H&OFFSET=1&WIDTH=2&VALUE=abcd");
/// ```
pub fn response(header: &str, blocks: &[Block]) -> String {
    let mut out = String::from(header);
    for block in blocks {
        let _ = write!(
            out,
            "&OFFSET={:x}&WIDTH={:x}&VALUE=",
            block.offset, block.width
        );
        push_hex(&mut out, &block.bytes);
    }
    out
}

/// Parses the `OFFSET`, `WIDTH` and `VALUE` elements of a `<ConfigResp>`.
///
/// A leading `<ConfigHdr>` is skipped. The elements must appear as
/// `&OFFSET=..&WIDTH=..&VALUE=..`, repeated for each block. Empty bodies are
/// accepted and return no blocks.
///
/// # Errors
///
/// Returns [`ConfigError`] for a missing element, a non-hexadecimal number, a
/// value with an odd number of digits, or a value whose length differs from
/// its width.
///
/// # Examples
///
/// ```
/// use hii_ifr::config;
///
/// assert!(config::parse_blocks("&OFFSET=0&WIDTH=1&VALUE=01").is_ok());
/// assert!(config::parse_blocks("&OFFSET=0&WIDTH=2&VALUE=01").is_err());
/// ```
pub fn parse_blocks(response: &str) -> Result<Vec<Block>, ConfigError> {
    let mut blocks = Vec::new();
    let mut offset = None;
    let mut width = None;
    for element in elements(response)
        .split('&')
        .filter(|element| !element.is_empty())
    {
        let (field, value) = element.split_once('=').ok_or(ConfigError::BadElement)?;
        match field {
            "OFFSET" => {
                if offset.is_some() || width.is_some() {
                    return Err(ConfigError::MissingField("VALUE"));
                }
                offset = Some(parse_hex_u16("OFFSET", value)?);
            }
            "WIDTH" => {
                if width.is_some() {
                    return Err(ConfigError::MissingField("OFFSET"));
                }
                width = Some(parse_hex_u16("WIDTH", value)?);
            }
            "VALUE" => {
                let block_offset = offset.ok_or(ConfigError::MissingField("OFFSET"))?;
                let block_width = width.ok_or(ConfigError::MissingField("WIDTH"))?;
                let bytes = parse_hex_bytes("VALUE", value)?;
                if bytes.len() != usize::from(block_width) {
                    return Err(ConfigError::WidthMismatch {
                        width: usize::from(block_width),
                        bytes: bytes.len(),
                    });
                }
                blocks.push(Block {
                    offset: block_offset,
                    width: block_width,
                    bytes,
                });
                offset = None;
                width = None;
            }
            _ => return Err(ConfigError::BadElement),
        }
    }
    if offset.is_some() || width.is_some() {
        return Err(ConfigError::MissingField("VALUE"));
    }
    Ok(blocks)
}

/// Builds a name/value `<ConfigRequest>`.
///
/// # Examples
///
/// ```
/// let request = hii_ifr::config::request_names("H", &["First", "Second"]);
/// assert_eq!(request, "H&First&Second");
/// ```
pub fn request_names(header: &str, names: &[&str]) -> String {
    let mut out = String::from(header);
    for name in names {
        out.push('&');
        out.push_str(name);
    }
    out
}

/// Parses the named elements of a name/value `<ConfigResp>`.
///
/// A leading `<ConfigHdr>` is skipped. Each element is `&NAME=VALUE`, where
/// `VALUE` is a hexadecimal byte string. The returned value is the
/// hexadecimal text without its leading `&`, exactly as published.
///
/// # Errors
///
/// Returns [`ConfigError`] for an element without `=`, an empty name, or a
/// value that is not an even-length hexadecimal byte string.
///
/// # Examples
///
/// ```
/// let header = hii_ifr::config::header(&[0; 16], "", &[]);
/// assert_eq!(hii_ifr::config::request_names(&header, &["First"]), format!("{header}&First"));
/// let names = hii_ifr::config::parse_names(&format!("{header}&First=00&Second=0102")).unwrap();
/// assert_eq!(names, vec![("First".into(), "00".into()), ("Second".into(), "0102".into())]);
/// ```
pub fn parse_names(response: &str) -> Result<Vec<(String, String)>, ConfigError> {
    let mut names = Vec::new();
    for element in elements(response)
        .split('&')
        .filter(|element| !element.is_empty())
    {
        let (name, value) = element.split_once('=').ok_or(ConfigError::BadElement)?;
        if name.is_empty() {
            return Err(ConfigError::BadElement);
        }
        parse_hex_bytes("VALUE", value)?;
        names.push((name.to_string(), value.to_string()));
    }
    Ok(names)
}

/// Returns the elements of a string, without a leading header.
fn elements(response: &str) -> &str {
    if !response.starts_with("GUID=") {
        return response;
    }
    let Some(path) = response.find("&PATH=") else {
        return response;
    };
    let rest = &response[path + "&PATH=".len()..];
    match rest.find('&') {
        Some(at) => &rest[at..],
        None => "",
    }
}

/// Parses a hexadecimal 16-bit number.
fn parse_hex_u16(field: &'static str, text: &str) -> Result<u16, ConfigError> {
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ConfigError::BadNumber(field));
    }
    u16::from_str_radix(text, 16).map_err(|_| ConfigError::BadNumber(field))
}

/// Parses a hexadecimal byte string.
fn parse_hex_bytes(field: &'static str, text: &str) -> Result<Vec<u8>, ConfigError> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ConfigError::BadHex(field));
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).map_err(|_| ConfigError::BadHex(field)))
        .collect()
}

/// Appends lower-case hexadecimal bytes.
fn push_hex(out: &mut String, bytes: &[u8]) {
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
}

/// Appends a lower-case hexadecimal 16-bit number as four digits.
fn push_hex_u16(out: &mut String, value: u16) {
    let _ = write!(out, "{value:04x}");
}
