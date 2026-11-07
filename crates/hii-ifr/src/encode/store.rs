//! Variable stores, defaults and one-of options.

use super::*;

/// Appends a variable store declaration.
pub(super) fn encode_varstore(store: &VarStore, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    let mut payload = Vec::new();
    match store.kind {
        VarStoreKind::Buffer => {
            payload.extend_from_slice(&store.guid);
            push_u16(&mut payload, store.id);
            push_u16(&mut payload, store.size);
            push_ascii(&mut payload, &store.name)?;
            push_op(out, abi::op::VARSTORE, false, &payload)
        }
        VarStoreKind::Efi { attributes } => {
            push_u16(&mut payload, store.id);
            payload.extend_from_slice(&store.guid);
            payload.extend_from_slice(&attributes.to_le_bytes());
            push_u16(&mut payload, store.size);
            push_ascii(&mut payload, &store.name)?;
            push_op(out, abi::op::VARSTORE_EFI, false, &payload)
        }
        VarStoreKind::NameValue => {
            push_u16(&mut payload, store.id);
            payload.extend_from_slice(&store.guid);
            push_op(out, abi::op::VARSTORE_NAME_VALUE, false, &payload)
        }
    }
}

/// Appends a NUL-terminated ASCII name.
pub(super) fn push_ascii(out: &mut Vec<u8>, name: &str) -> Result<(), EncodeError> {
    if !name.is_ascii() || name.contains('\0') {
        return Err(EncodeError::NonAsciiName);
    }
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    Ok(())
}

/// Returns the default value of a bound row, or the fallback.
pub(super) fn default_of(set: &FormSet, row: &Row, fallback: u64) -> u64 {
    let Some(binding) = row.binding else {
        return fallback;
    };
    set.defaults
        .iter()
        .find_map(|set| set.values.get(&binding))
        .map_or(fallback, crate::model::default_value)
}

/// Returns the default choice value of a row.
pub(super) fn default_choice(set: &FormSet, row: &Row) -> Option<u64> {
    let binding = row.binding?;
    set.defaults
        .iter()
        .find_map(|set| set.values.get(&binding))
        .map(crate::model::default_value)
}

/// Appends a default opcode for one value.
pub(super) fn push_default(out: &mut Vec<u8>, kind: u8, bytes: &[u8]) -> Result<(), EncodeError> {
    let mut payload = Vec::new();
    push_u16(&mut payload, 1);
    payload.push(kind);
    payload.extend_from_slice(bytes);
    push_op(out, abi::op::DEFAULT, false, &payload)
}

/// Appends one option of a choice or an ordered list.
pub(super) fn encode_option(
    choice: &Choice,
    width: Width,
    default: bool,
    strings: &mut Strings,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    let mut payload = Vec::new();
    push_u16(&mut payload, strings.intern(&choice.label)?);
    payload.push(if default {
        abi::flags::OPTION_DEFAULT
    } else {
        0
    });
    payload.push(value_type(width));
    push_width(&mut payload, width, choice.value)?;
    push_op(out, abi::op::ONE_OF_OPTION, false, &payload)
}
