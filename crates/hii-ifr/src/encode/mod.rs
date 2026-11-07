//! Encoding of a form set as IFR and HII packages.

mod bytes;
mod expr;
mod store;

use self::{bytes::*, expr::*, store::*};

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use forms::{Choice, Control, Expr, FormSet, Operand, Page, Refresh, Row, ValidationKind, Value};

use hii_abi as abi;

use crate::model::{
    Export, Guid, QuestionKind, QuestionRef, Storage, VarStore, VarStoreKind, Width,
};

/// A failure while encoding a form set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// An opcode, package or package list exceeded its length field.
    TooLong,
    /// A string contained a NUL character.
    BadString,
    /// A variable store name was not ASCII.
    NonAsciiName,
    /// A number did not fit the width of its question.
    ValueOutOfRange,
    /// A condition or a value has no IFR form.
    UnsupportedExpression,
    /// More than three class GUIDs cannot be encoded.
    TooManyClassGuids,
    /// A value row has no descriptor entry.
    MissingQuestion,
    /// A page has no descriptor entry.
    MissingForm,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::TooLong => "the encoded data exceeds a length field",
            Self::BadString => "a string contains a NUL character",
            Self::NonAsciiName => "a variable store name is not ASCII",
            Self::ValueOutOfRange => "a value does not fit its question width",
            Self::UnsupportedExpression => "a condition or value cannot be encoded",
            Self::TooManyClassGuids => "more than three class GUIDs cannot be encoded",
            Self::MissingQuestion => "a value row has no descriptor entry",
            Self::MissingForm => "a page has no descriptor entry",
        };
        formatter.write_str(reason)
    }
}

impl core::error::Error for EncodeError {}

/// Encodes one message as string 2, after the language name.
pub(crate) fn encode_message(text: &str, guid: Guid) -> Result<Vec<u8>, EncodeError> {
    if text.contains('\0') {
        return Err(EncodeError::BadString);
    }
    let strings = Strings {
        ids: BTreeMap::new(),
        order: alloc::vec![String::from(text)],
    };
    let strings = strings.package()?;
    let end = package(abi::PACKAGE_END, &[])?;
    let size = u32::try_from(abi::PACKAGE_LIST_HEADER_SIZE + strings.len() + end.len())
        .map_err(|_| EncodeError::TooLong)?;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&guid);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&strings);
    out.extend_from_slice(&end);
    Ok(out)
}

/// Encodes a form set and its descriptor as a package list.
pub(crate) fn encode(
    set: &FormSet,
    export: &Export,
    package_list: Guid,
) -> Result<Vec<u8>, EncodeError> {
    let mut strings = Strings::default();
    let mut body = Vec::new();

    let mut payload = Vec::new();
    payload.extend_from_slice(&export.guid);
    push_u16(&mut payload, strings.intern(&set.title)?);
    push_u16(&mut payload, strings.intern(&set.help)?);
    let count =
        u8::try_from(export.class_guids.len()).map_err(|_| EncodeError::TooManyClassGuids)?;
    if count > 3 {
        return Err(EncodeError::TooManyClassGuids);
    }
    payload.push(count);
    for guid in &export.class_guids {
        payload.extend_from_slice(guid);
    }
    push_op(&mut body, abi::op::FORM_SET, true, &payload)?;

    for store in &export.varstores {
        encode_varstore(store, &mut body)?;
    }
    for (index, defaults) in set.defaults.iter().enumerate() {
        let id = u16::try_from(index + 1).map_err(|_| EncodeError::TooLong)?;
        let mut payload = Vec::new();
        push_u16(&mut payload, strings.intern(&defaults.name)?);
        push_u16(&mut payload, 0);
        push_u16(&mut payload, id);
        push_op(&mut body, abi::op::DEFAULTSTORE, false, &payload)?;
    }
    for (index, page) in set.pages.iter().enumerate() {
        encode_page(page, export, index, set, &mut strings, &mut body)?;
    }
    push_op(&mut body, abi::op::END, false, &[])?;

    let forms_package = package(abi::PACKAGE_FORMS, &body)?;
    let string_package = strings.package()?;
    let end_package = package(abi::PACKAGE_END, &[])?;

    let size = abi::PACKAGE_LIST_HEADER_SIZE
        + forms_package.len()
        + string_package.len()
        + end_package.len();
    let size = u32::try_from(size).map_err(|_| EncodeError::TooLong)?;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&package_list);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&forms_package);
    out.extend_from_slice(&string_package);
    out.extend_from_slice(&end_package);
    Ok(out)
}

/// The string table of one encoded form set.
#[derive(Default)]
struct Strings {
    ids: BTreeMap<String, u16>,
    order: Vec<String>,
}

impl Strings {
    /// Returns the identifier of a string, adding it when it is new.
    fn intern(&mut self, text: &str) -> Result<u16, EncodeError> {
        if text.is_empty() {
            return Ok(0);
        }
        if text.contains('\0') {
            return Err(EncodeError::BadString);
        }
        if let Some(id) = self.ids.get(text) {
            return Ok(*id);
        }
        let id = u16::try_from(self.order.len())
            .ok()
            .and_then(|count| count.checked_add(2))
            .ok_or(EncodeError::TooLong)?;
        self.ids.insert(String::from(text), id);
        self.order.push(String::from(text));
        Ok(id)
    }

    /// Builds the strings package.
    fn package(&self) -> Result<Vec<u8>, EncodeError> {
        let mut blocks = Vec::new();
        blocks.push(abi::sibt::STRING_UCS2);
        push_ucs2(&mut blocks, "English");
        for text in &self.order {
            blocks.push(abi::sibt::STRING_UCS2);
            push_ucs2(&mut blocks, text);
        }
        blocks.push(abi::sibt::END);

        let language = b"en-US\0";
        let header = abi::string_header::LANGUAGE + language.len();
        let size = u32::try_from(header + blocks.len()).map_err(|_| EncodeError::TooLong)?;
        if size > abi::MAX_PACKAGE_LENGTH {
            return Err(EncodeError::TooLong);
        }
        let mut out = Vec::with_capacity(size as usize);
        out.extend_from_slice(&(size | (u32::from(abi::PACKAGE_STRINGS) << 24)).to_le_bytes());
        out.extend_from_slice(&(header as u32).to_le_bytes());
        out.extend_from_slice(&(header as u32).to_le_bytes());
        out.resize(out.len() + abi::string_header::LANGUAGE_WINDOW_LEN * 2, 0);
        push_u16(&mut out, 1);
        out.extend_from_slice(language);
        out.extend_from_slice(&blocks);
        Ok(out)
    }
}

/// Wraps a package body in a package header.
fn package(kind: u8, body: &[u8]) -> Result<Vec<u8>, EncodeError> {
    let size = abi::PACKAGE_HEADER_SIZE + body.len();
    let size = u32::try_from(size).map_err(|_| EncodeError::TooLong)?;
    if size > abi::MAX_PACKAGE_LENGTH {
        return Err(EncodeError::TooLong);
    }
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&(size | (u32::from(kind) << 24)).to_le_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// Appends one IFR opcode.
fn push_op(out: &mut Vec<u8>, code: u8, scope: bool, payload: &[u8]) -> Result<(), EncodeError> {
    let length = payload.len() + 2;
    if length > abi::MAX_OPCODE_LENGTH {
        return Err(EncodeError::TooLong);
    }
    out.push(code);
    out.push((length as u8) | if scope { 0x80 } else { 0 });
    out.extend_from_slice(payload);
    Ok(())
}

/// Appends one page.
fn encode_page(
    page: &Page,
    export: &Export,
    index: usize,
    set: &FormSet,
    strings: &mut Strings,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    let form = export
        .form(page.key)
        .or_else(|| u16::try_from(index + 1).ok())
        .ok_or(EncodeError::MissingForm)?;
    let mut payload = Vec::new();
    push_u16(&mut payload, form);
    push_u16(&mut payload, strings.intern(&page.title)?);
    push_op(out, abi::op::FORM, true, &payload)?;
    for row in &page.rows {
        encode_row(row, export, set, strings, out)?;
    }
    push_op(out, abi::op::END, false, &[])
}

/// Appends one row and the scopes of its conditions.
fn encode_row(
    row: &Row,
    export: &Export,
    set: &FormSet,
    strings: &mut Strings,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    let mut body = Vec::new();
    encode_control(row, export, set, strings, &mut body)?;
    for rule in row.validation.iter().rev() {
        let code = match rule.kind {
            ValidationKind::Warning => abi::op::WARNING_IF,
            ValidationKind::Edit => abi::op::INCONSISTENT_IF,
            ValidationKind::Submit => abi::op::NO_SUBMIT_IF,
        };
        let mut payload = Vec::new();
        push_u16(&mut payload, strings.intern(&rule.message)?);
        if rule.kind == ValidationKind::Warning {
            payload.push(rule.timeout);
        }
        body = wrap_scoped(
            code,
            &payload,
            &encode_expr(&rule.condition, export)?,
            &body,
        )?;
    }
    if let Some(condition) = &row.disabled {
        let expr = encode_expr(condition, export)?;
        body = wrap_scoped(abi::op::GRAY_OUT_IF, &[], &expr, &body)?;
    }
    if let Some(condition) = &row.hidden {
        let expr = encode_expr(condition, export)?;
        body = wrap_scoped(abi::op::SUPPRESS_IF, &[], &expr, &body)?;
    }
    out.extend_from_slice(&body);
    Ok(())
}

/// Appends one control and the statements of its question scope.
fn encode_control(
    row: &Row,
    export: &Export,
    set: &FormSet,
    strings: &mut Strings,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    let question = question_of_row(export, row);
    match &row.control {
        Control::Heading => {
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(&row.label)?);
            push_u16(&mut payload, 0);
            payload.push(0);
            push_op(out, abi::op::SUBTITLE, false, &payload)
        }
        Control::Text(value) => {
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(&row.label)?);
            push_u16(&mut payload, strings.intern(&row.help)?);
            push_u16(&mut payload, strings.intern(value)?);
            push_op(out, abi::op::TEXT, false, &payload)
        }
        Control::Link(target) => {
            let form = export.form(target.page).unwrap_or(0);
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(&row.label)?);
            push_u16(&mut payload, strings.intern(&row.help)?);
            push_u16(&mut payload, 0);
            push_u16(&mut payload, 0);
            push_u16(&mut payload, 0);
            payload.push(0);
            push_u16(&mut payload, form);
            push_op(out, abi::op::REF, false, &payload)
        }
        Control::Action => {
            let question = question.ok_or(EncodeError::MissingQuestion)?;
            let mut payload = question_header(row, question, strings)?;
            push_u16(&mut payload, 0);
            push_op(out, abi::op::ACTION, false, &payload)
        }
        Control::Toggle => {
            let question = question.ok_or(EncodeError::MissingQuestion)?;
            let mut payload = question_header(row, question, strings)?;
            let default = default_of(set, row, 1) != 0;
            payload.push(if default {
                abi::flags::CHECKBOX_DEFAULT
            } else {
                0
            });
            push_op(out, abi::op::CHECKBOX, true, &payload)?;
            if default {
                push_default(out, 1, &[1])?;
            }
            encode_refresh(row, export, out)?;
            push_op(out, abi::op::END, false, &[])
        }
        Control::Choice(choices) => {
            let question = question.ok_or(EncodeError::MissingQuestion)?;
            let width = numeric_width(question)?;
            let mut payload = question_header(row, question, strings)?;
            payload.push(width_flags(width));
            push_width(&mut payload, width, 0)?;
            push_width(&mut payload, width, width_max(width))?;
            push_width(&mut payload, width, 0)?;
            push_op(out, abi::op::ONE_OF, true, &payload)?;
            let default = default_choice(set, row);
            for choice in choices {
                encode_option(choice, width, default == Some(choice.value), strings, out)?;
            }
            encode_refresh(row, export, out)?;
            push_op(out, abi::op::END, false, &[])
        }
        Control::Number {
            minimum,
            maximum,
            step,
        }
        | Control::Slider {
            minimum,
            maximum,
            step,
            ..
        } => {
            let question = question.ok_or(EncodeError::MissingQuestion)?;
            let width = numeric_width(question)?;
            let mut payload = question_header(row, question, strings)?;
            payload.push(width_flags(width) | abi::flags::DISPLAY_UINT_DEC);
            push_width(&mut payload, width, *minimum)?;
            push_width(&mut payload, width, *maximum)?;
            push_width(&mut payload, width, *step)?;
            push_op(out, abi::op::NUMERIC, true, &payload)?;
            let default = default_of(set, row, *minimum);
            if default != *minimum
                || set
                    .defaults
                    .iter()
                    .any(|de| de.values.contains_key(&row.binding.unwrap_or_default()))
            {
                let bytes = width_bytes(width, default)?;
                push_default(out, 1, &bytes)?;
            }
            encode_refresh(row, export, out)?;
            push_op(out, abi::op::END, false, &[])
        }
        Control::TextInput {
            minimum, maximum, ..
        } => {
            let question = question.ok_or(EncodeError::MissingQuestion)?;
            let (code, payload) = match question.kind {
                QuestionKind::Password { .. } => {
                    let mut payload = question_header(row, question, strings)?;
                    push_u16(&mut payload, u16::try_from(*minimum).unwrap_or(u16::MAX));
                    push_u16(&mut payload, u16::try_from(*maximum).unwrap_or(u16::MAX));
                    (abi::op::PASSWORD, payload)
                }
                _ => {
                    let mut payload = question_header(row, question, strings)?;
                    payload.push(u8::try_from(*minimum).unwrap_or(u8::MAX));
                    payload.push(u8::try_from(*maximum).unwrap_or(u8::MAX));
                    payload.push(0);
                    (abi::op::STRING, payload)
                }
            };
            push_op(out, code, true, &payload)?;
            encode_refresh(row, export, out)?;
            push_op(out, abi::op::END, false, &[])
        }
        Control::OrderedList { choices, maximum } => {
            let question = question.ok_or(EncodeError::MissingQuestion)?;
            let width = Width::U16;
            let mut payload = question_header(row, question, strings)?;
            payload.push(u8::try_from(*maximum).unwrap_or(u8::MAX));
            payload.push(abi::flags::ORDERED_UNIQUE_SET);
            push_op(out, abi::op::ORDERED_LIST, true, &payload)?;
            let default = default_choice(set, row);
            for choice in choices {
                encode_option(choice, width, default == Some(choice.value), strings, out)?;
            }
            encode_refresh(row, export, out)?;
            push_op(out, abi::op::END, false, &[])
        }
        Control::Reset(default) => {
            let id = set
                .defaults
                .iter()
                .position(|set| set.key == *default)
                .and_then(|index| u16::try_from(index + 1).ok())
                .unwrap_or(0);
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(&row.label)?);
            push_u16(&mut payload, strings.intern(&row.help)?);
            push_u16(&mut payload, id);
            push_op(out, abi::op::RESET_BUTTON, false, &payload)
        }
        Control::Progress { current, total } => {
            let value = match total {
                Some(total) => alloc::format!("{current}/{total}"),
                None => String::from("Working"),
            };
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(&row.label)?);
            push_u16(&mut payload, strings.intern(&row.help)?);
            push_u16(&mut payload, strings.intern(&value)?);
            push_op(out, abi::op::TEXT, false, &payload)
        }
        Control::Divider => {
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern("")?);
            push_u16(&mut payload, 0);
            payload.push(0);
            push_op(out, abi::op::SUBTITLE, false, &payload)
        }
        Control::Unsupported(reason) => {
            let value = alloc::format!("Unsupported: {reason}");
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(&row.label)?);
            push_u16(&mut payload, strings.intern(&row.help)?);
            push_u16(&mut payload, strings.intern(&value)?);
            push_op(out, abi::op::TEXT, false, &payload)
        }
        Control::Custom { kind, .. } => {
            let value = alloc::format!("Consumer control {}", kind.0);
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(&row.label)?);
            push_u16(&mut payload, strings.intern(&row.help)?);
            push_u16(&mut payload, strings.intern(&value)?);
            push_op(out, abi::op::TEXT, false, &payload)
        }
    }
}

/// Returns the descriptor entry of a row, which a consumer may key by binding.
fn question_of_row<'a>(export: &'a Export, row: &Row) -> Option<&'a QuestionRef> {
    export.question(row.binding.unwrap_or(row.key))
}

/// Appends the refresh statement of a row.
fn encode_refresh(row: &Row, export: &Export, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    match row.refresh {
        Refresh::OnView => Ok(()),
        Refresh::Periodic(milliseconds) => {
            let seconds = milliseconds.div_ceil(1000).clamp(1, u64::from(u8::MAX));
            push_op(out, abi::op::REFRESH, false, &[seconds as u8])
        }
        Refresh::Event(key) => match export.refresh_event(key) {
            Some(guid) => push_op(out, abi::op::REFRESH_ID, false, &guid),
            None => Ok(()),
        },
    }
}

/// Appends the question header shared by question statements.
fn question_header(
    row: &Row,
    question: &QuestionRef,
    strings: &mut Strings,
) -> Result<Vec<u8>, EncodeError> {
    let mut payload = Vec::new();
    push_u16(&mut payload, strings.intern(&row.label)?);
    push_u16(&mut payload, strings.intern(&row.help)?);
    push_u16(&mut payload, question.question);
    match &question.storage {
        Storage::None => {
            push_u16(&mut payload, 0);
            push_u16(&mut payload, 0);
        }
        Storage::Offset { store, offset } => {
            push_u16(&mut payload, *store);
            push_u16(&mut payload, *offset);
        }
        Storage::Name { store, name } => {
            push_u16(&mut payload, *store);
            push_u16(&mut payload, strings.intern(name)?);
        }
    }
    let mut flags = question.flags
        | match &question.kind {
            QuestionKind::Action => abi::flags::CALLBACK,
            _ => 0,
        };
    if row.locked || row.read_only.is_some() {
        flags |= abi::flags::READ_ONLY;
    }
    payload.push(flags);
    Ok(payload)
}

/// Appends a scoped condition with its expression and inner statements.
fn wrap_scoped(
    code: u8,
    payload: &[u8],
    expr: &[u8],
    inner: &[u8],
) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    push_op(&mut out, code, true, payload)?;
    out.extend_from_slice(expr);
    out.extend_from_slice(inner);
    push_op(&mut out, abi::op::END, false, &[])?;
    Ok(out)
}
