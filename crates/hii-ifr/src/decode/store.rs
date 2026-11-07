//! Variable stores, defaults and one-of options.

use super::*;

/// Returns the storage that a question header names.
pub(super) fn varstore_of(
    payload: &[u8],
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
) -> Option<Storage> {
    if payload.len() < 11 {
        return None;
    }
    let store = read_u16(payload, 6)?;
    let info = read_u16(payload, 8)?;
    if store == 0 {
        return Some(Storage::None);
    }
    let named = varstores
        .iter()
        .find(|store_info| store_info.store.id == store)
        .is_some_and(|store_info| store_info.named);
    if named {
        Some(Storage::Name {
            store,
            name: text_of(strings, Some(info)),
        })
    } else {
        Some(Storage::Offset {
            store,
            offset: info,
        })
    }
}

/// Decodes one default value.
pub(super) fn decode_default(node: &Node<'_>) -> Option<Value> {
    let kind = *node.payload.get(2)?;
    let value = &node.payload[3..];
    match kind {
        abi::value_type::BOOLEAN => Some(Value::Bool(value.first().copied().unwrap_or(0) != 0)),
        kind => {
            let width = numeric_width(kind)?;
            Some(Value::Number(read_width(value, 0, width)?))
        }
    }
}

/// Decodes one option of a choice or an ordered list.
pub(super) fn decode_choice(
    node: &Node<'_>,
    width: Width,
    strings: &BTreeMap<u16, String>,
) -> Option<Choice> {
    let payload = node.payload;
    if payload.len() < 4 {
        return None;
    }
    let text = read_u16(payload, 0)?;
    let value = read_width(payload, 4, width)?;
    Some(Choice {
        label: text_of(strings, Some(text)),
        value,
    })
}

/// Decodes a buffer store.
pub(super) fn decode_varstore_buffer(node: &Node<'_>) -> Result<StoreInfo, DecodeError> {
    let payload = node.payload;
    if payload.len() < 20 {
        return Err(DecodeError::BadLength);
    }
    let guid = guid_at(payload, 0).ok_or(DecodeError::BadLength)?;
    let id = read_u16(payload, 16).ok_or(DecodeError::BadLength)?;
    let size = read_u16(payload, 18).ok_or(DecodeError::BadLength)?;
    Ok(StoreInfo {
        store: VarStore {
            id,
            guid,
            name: ascii_name(&payload[20..]),
            size,
            kind: VarStoreKind::Buffer,
        },
        named: false,
    })
}

/// Decodes an EFI variable store.
pub(super) fn decode_varstore_efi(node: &Node<'_>) -> Result<StoreInfo, DecodeError> {
    let payload = node.payload;
    if payload.len() < 18 {
        return Err(DecodeError::BadLength);
    }
    let id = read_u16(payload, 0).ok_or(DecodeError::BadLength)?;
    let guid = guid_at(payload, 2).ok_or(DecodeError::BadLength)?;
    let legacy = payload.len() < 24;
    let (attributes, size, name) = if legacy {
        (0, 0, String::new())
    } else {
        let attributes = read_u32(payload, 18).ok_or(DecodeError::BadLength)?;
        let size = read_u16(payload, 22).ok_or(DecodeError::BadLength)?;
        (attributes, size, ascii_name(&payload[24..]))
    };
    Ok(StoreInfo {
        store: VarStore {
            id,
            guid,
            name,
            size,
            kind: VarStoreKind::Efi { attributes },
        },
        named: legacy,
    })
}

/// Decodes a name/value store.
pub(super) fn decode_varstore_name_value(node: &Node<'_>) -> Result<StoreInfo, DecodeError> {
    let payload = node.payload;
    if payload.len() < 18 {
        return Err(DecodeError::BadLength);
    }
    let id = read_u16(payload, 0).ok_or(DecodeError::BadLength)?;
    let guid = guid_at(payload, 2).ok_or(DecodeError::BadLength)?;
    Ok(StoreInfo {
        store: VarStore {
            id,
            guid,
            name: String::new(),
            size: 0,
            kind: VarStoreKind::NameValue,
        },
        named: true,
    })
}
