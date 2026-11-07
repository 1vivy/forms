//! The HII string package: UCS-2, SCSU skips and the language choice.

use super::*;

/// Parses one strings package.
pub(super) fn parse_string_package(package: &[u8]) -> Result<StringPackage, DecodeError> {
    if package.len() < abi::string_header::MIN_HEADER_SIZE {
        return Err(DecodeError::BadStrings);
    }
    let header = usize::try_from(
        read_u32(package, abi::string_header::HDR_SIZE).ok_or(DecodeError::BadStrings)?,
    )
    .map_err(|_| DecodeError::BadStrings)?;
    let offset = usize::try_from(
        read_u32(package, abi::string_header::STRING_INFO_OFFSET).ok_or(DecodeError::BadStrings)?,
    )
    .map_err(|_| DecodeError::BadStrings)?;
    if header < abi::string_header::MIN_HEADER_SIZE
        || header > package.len()
        || offset < abi::string_header::MIN_HEADER_SIZE
        || offset > package.len()
    {
        return Err(DecodeError::BadStrings);
    }
    let language = c_string(&package[abi::string_header::LANGUAGE..]);
    let mut strings = BTreeMap::new();
    let mut id: u32 = 1;
    let mut at = offset;
    while at < package.len() {
        match package[at] {
            abi::sibt::END => break,
            abi::sibt::STRING_UCS2 => {
                let (text, next) = read_ucs2(package, at + 1)?;
                insert(&mut strings, id, text);
                id += 1;
                at = next;
            }
            abi::sibt::STRING_UCS2_FONT => {
                let (text, next) = read_ucs2(package, at + 2)?;
                insert(&mut strings, id, text);
                id += 1;
                at = next;
            }
            abi::sibt::STRINGS_UCS2 | abi::sibt::STRINGS_UCS2_FONT => {
                let (count, mut next) = if package[at] == abi::sibt::STRINGS_UCS2 {
                    (
                        read_u16(package, at + 1).ok_or(DecodeError::BadStrings)?,
                        at + 3,
                    )
                } else {
                    (
                        read_u16(package, at + 2).ok_or(DecodeError::BadStrings)?,
                        at + 4,
                    )
                };
                for _ in 0..count {
                    let (text, after) = read_ucs2(package, next)?;
                    insert(&mut strings, id, text);
                    id += 1;
                    next = after;
                }
                at = next;
            }
            block @ (abi::sibt::STRING_SCSU | abi::sibt::STRING_SCSU_FONT) => {
                let start = if block == abi::sibt::STRING_SCSU {
                    at + 1
                } else {
                    at + 2
                };
                at = skip_scsu(package, start, 1)?;
                id += 1;
            }
            block @ (abi::sibt::STRINGS_SCSU | abi::sibt::STRINGS_SCSU_FONT) => {
                let (count, start) = if block == abi::sibt::STRINGS_SCSU {
                    (
                        read_u16(package, at + 1).ok_or(DecodeError::BadStrings)?,
                        at + 3,
                    )
                } else {
                    (
                        read_u16(package, at + 2).ok_or(DecodeError::BadStrings)?,
                        at + 4,
                    )
                };
                at = skip_scsu(package, start, count)?;
                id += u32::from(count);
            }
            abi::sibt::DUPLICATE => {
                let source = read_u16(package, at + 1).ok_or(DecodeError::BadStrings)?;
                let text = strings.get(&source).cloned().unwrap_or_default();
                insert(&mut strings, id, text);
                id += 1;
                at += 3;
            }
            abi::sibt::SKIP2 => {
                let count = read_u16(package, at + 1).ok_or(DecodeError::BadStrings)?;
                id += u32::from(count);
                at += 3;
            }
            abi::sibt::SKIP1 => {
                let count = *package.get(at + 1).ok_or(DecodeError::BadStrings)?;
                id += u32::from(count);
                at += 2;
            }
            abi::sibt::EXT1 => {
                let length = usize::from(*package.get(at + 2).ok_or(DecodeError::BadStrings)?);
                at = skip_block(package, at, length)?;
            }
            abi::sibt::EXT2 => {
                let length = usize::from(read_u16(package, at + 2).ok_or(DecodeError::BadStrings)?);
                at = skip_block(package, at, length)?;
            }
            abi::sibt::EXT4 => {
                let length =
                    usize::try_from(read_u32(package, at + 2).ok_or(DecodeError::BadStrings)?)
                        .map_err(|_| DecodeError::BadStrings)?;
                at = skip_block(package, at, length)?;
            }
            _ => return Err(DecodeError::BadStrings),
        }
    }
    Ok(StringPackage { language, strings })
}

/// Returns the strings table of the preferred language.
pub(super) fn select_strings(packages: &[StringPackage]) -> Option<&BTreeMap<u16, String>> {
    for wanted in ["en-US", "en"] {
        if let Some(package) = packages
            .iter()
            .find(|package| package.language.eq_ignore_ascii_case(wanted))
        {
            return Some(&package.strings);
        }
    }
    packages.first().map(|package| &package.strings)
}

/// Reads a null-terminated UTF-16 string and returns the next block offset.
pub(super) fn read_ucs2(package: &[u8], at: usize) -> Result<(String, usize), DecodeError> {
    let mut units = Vec::new();
    let mut index = at;
    loop {
        let unit = read_u16(package, index).ok_or(DecodeError::BadStrings)?;
        index += 2;
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    Ok((utf16_to_string(units), index))
}

/// Skips a number of null-terminated single-byte strings.
pub(super) fn skip_scsu(package: &[u8], at: usize, count: u16) -> Result<usize, DecodeError> {
    let mut index = at;
    for _ in 0..count {
        loop {
            let byte = *package.get(index).ok_or(DecodeError::BadStrings)?;
            index += 1;
            if byte == 0 {
                break;
            }
        }
    }
    Ok(index)
}

/// Skips an extended block by its declared length.
pub(super) fn skip_block(package: &[u8], at: usize, length: usize) -> Result<usize, DecodeError> {
    if length < 3 {
        return Err(DecodeError::BadStrings);
    }
    let end = at.checked_add(length).ok_or(DecodeError::BadStrings)?;
    if end > package.len() {
        return Err(DecodeError::BadStrings);
    }
    Ok(end)
}

/// Adds a string when its identifier fits.
pub(super) fn insert(strings: &mut BTreeMap<u16, String>, id: u32, text: String) {
    if let Ok(id) = u16::try_from(id) {
        strings.insert(id, text);
    }
}
