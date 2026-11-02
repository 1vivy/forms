//! Encoding of the forms model as IFR and HII packages.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use forms::{
    Choice, Expr, Form, FormSet, Guid, Item, Question, Statement, Storage, VarStore, VarStoreKind,
    Width,
};

use hii_abi as abi;

/// A failure while encoding a formset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// An opcode, package or package list exceeded its length field.
    TooLong,
    /// A string contained a NUL character, which the string blocks cannot hold.
    BadString,
    /// A variable store name was not ASCII, as the IFR name field requires.
    NonAsciiName,
    /// A number did not fit the width of its statement.
    ValueOutOfRange,
    /// The model held a condition that IFR cannot encode.
    UnsupportedExpression,
    /// More than three class GUIDs cannot be encoded in the two-bit count.
    TooManyClassGuids,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::TooLong => "the encoded data exceeds a length field",
            Self::BadString => "a string contains a NUL character",
            Self::NonAsciiName => "a variable store name is not ASCII",
            Self::ValueOutOfRange => "a value does not fit its question width",
            Self::UnsupportedExpression => "a condition cannot be encoded",
            Self::TooManyClassGuids => "more than three class GUIDs cannot be encoded",
        };
        formatter.write_str(reason)
    }
}

impl core::error::Error for EncodeError {}

/// Encodes a formset and its strings as a package list.
pub(crate) fn encode(set: &FormSet, package_list: Guid) -> Result<Vec<u8>, EncodeError> {
    let mut strings = Strings::default();
    let mut body = Vec::new();

    let mut payload = Vec::new();
    payload.extend_from_slice(&set.guid);
    push_u16(&mut payload, strings.intern(&set.title)?);
    push_u16(&mut payload, strings.intern(&set.help)?);
    let count = u8::try_from(set.class_guids.len()).map_err(|_| EncodeError::TooManyClassGuids)?;
    if count > 3 {
        return Err(EncodeError::TooManyClassGuids);
    }
    payload.push(count);
    for guid in &set.class_guids {
        payload.extend_from_slice(guid);
    }
    push_op(&mut body, abi::op::FORM_SET, true, &payload)?;

    for store in &set.varstores {
        encode_varstore(store, &mut body)?;
    }
    for form in &set.forms {
        encode_form(form, &mut strings, &mut body)?;
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

/// The string table of one encoded formset.
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

/// Appends one form.
fn encode_form(form: &Form, strings: &mut Strings, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    let mut payload = Vec::new();
    push_u16(&mut payload, form.id.0);
    push_u16(&mut payload, strings.intern(&form.title)?);
    push_op(out, abi::op::FORM, true, &payload)?;
    for item in &form.items {
        encode_item(item, strings, out)?;
    }
    push_op(out, abi::op::END, false, &[])
}

/// Appends one item and the scopes of its conditions.
fn encode_item(item: &Item, strings: &mut Strings, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    let mut body = Vec::new();
    encode_statement(&item.statement, strings, &mut body)?;
    if let Some(expr) = &item.gray_out_if {
        body = wrap_condition(abi::op::GRAY_OUT_IF, expr, &body)?;
    }
    if let Some(expr) = &item.suppress_if {
        body = wrap_condition(abi::op::SUPPRESS_IF, expr, &body)?;
    }
    out.extend_from_slice(&body);
    Ok(())
}

/// Appends a condition scope with its expression and inner statements.
fn wrap_condition(code: u8, expr: &Expr, inner: &[u8]) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    push_op(&mut out, code, true, &[])?;
    out.extend_from_slice(&encode_expr(expr)?);
    out.extend_from_slice(inner);
    push_op(&mut out, abi::op::END, false, &[])?;
    Ok(out)
}

/// Encodes one expression as a tree of scoped opcodes.
fn encode_expr(expr: &Expr) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    match expr {
        Expr::True => push_op(&mut out, abi::op::TRUE, false, &[])?,
        Expr::False => push_op(&mut out, abi::op::FALSE, false, &[])?,
        Expr::Not(inner) => {
            push_op(&mut out, abi::op::NOT, true, &[])?;
            out.extend_from_slice(&encode_expr(inner)?);
            push_op(&mut out, abi::op::END, false, &[])?;
        }
        Expr::And(first, second) | Expr::Or(first, second) => {
            let code = if matches!(expr, Expr::And(..)) {
                abi::op::AND
            } else {
                abi::op::OR
            };
            push_op(&mut out, code, true, &[])?;
            out.extend_from_slice(&encode_expr(first)?);
            out.extend_from_slice(&encode_expr(second)?);
            push_op(&mut out, abi::op::END, false, &[])?;
        }
        Expr::EqualValue { question, value } => {
            let value = u16::try_from(*value).map_err(|_| EncodeError::ValueOutOfRange)?;
            let mut payload = Vec::new();
            push_u16(&mut payload, question.0);
            push_u16(&mut payload, value);
            push_op(&mut out, abi::op::EQ_ID_VAL, false, &payload)?;
        }
        Expr::EqualList { question, values } => {
            let count = u16::try_from(values.len()).map_err(|_| EncodeError::TooLong)?;
            let mut payload = Vec::new();
            push_u16(&mut payload, question.0);
            push_u16(&mut payload, count);
            for value in values {
                let value = u16::try_from(*value).map_err(|_| EncodeError::ValueOutOfRange)?;
                push_u16(&mut payload, value);
            }
            push_op(&mut out, abi::op::EQ_ID_VAL_LIST, false, &payload)?;
        }
        Expr::EqualQuestion { first, second } => {
            let mut payload = Vec::new();
            push_u16(&mut payload, first.0);
            push_u16(&mut payload, second.0);
            push_op(&mut out, abi::op::EQ_ID_ID, false, &payload)?;
        }
        Expr::Unsupported => return Err(EncodeError::UnsupportedExpression),
    }
    Ok(out)
}

/// Appends one statement.
fn encode_statement(
    statement: &Statement,
    strings: &mut Strings,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    match statement {
        Statement::Subtitle { text } => {
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(text)?);
            push_u16(&mut payload, 0);
            payload.push(0);
            push_op(out, abi::op::SUBTITLE, false, &payload)
        }
        Statement::Text {
            prompt,
            help,
            value,
        } => {
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(prompt)?);
            push_u16(&mut payload, strings.intern(help)?);
            push_u16(&mut payload, strings.intern(value)?);
            push_op(out, abi::op::TEXT, false, &payload)
        }
        Statement::Goto {
            prompt,
            help,
            form,
            question,
        } => {
            let id = question.map_or(0, |question| question.0);
            let mut payload = Vec::new();
            push_u16(&mut payload, strings.intern(prompt)?);
            push_u16(&mut payload, strings.intern(help)?);
            push_u16(&mut payload, id);
            push_u16(&mut payload, 0);
            push_u16(&mut payload, 0);
            payload.push(0);
            push_u16(&mut payload, form.0);
            if question.is_some() {
                push_u16(&mut payload, id);
            }
            push_op(out, abi::op::REF, false, &payload)
        }
        Statement::Action { question } => {
            let mut payload = question_header(question, strings)?;
            push_u16(&mut payload, 0);
            push_op(out, abi::op::ACTION, false, &payload)
        }
        Statement::Checkbox { question, default } => {
            let mut payload = question_header(question, strings)?;
            payload.push(if *default {
                abi::flags::CHECKBOX_DEFAULT
            } else {
                0
            });
            if !*default {
                return push_op(out, abi::op::CHECKBOX, false, &payload);
            }
            push_op(out, abi::op::CHECKBOX, true, &payload)?;
            let mut value = Vec::new();
            push_u16(&mut value, 0);
            value.push(abi::value_type::BOOLEAN);
            value.push(1);
            push_op(out, abi::op::DEFAULT, false, &value)?;
            push_op(out, abi::op::END, false, &[])
        }
        Statement::OneOf {
            question,
            width,
            choices,
        } => encode_choice_statement(question, *width, choices, strings, out),
        Statement::Numeric {
            question,
            width,
            minimum,
            maximum,
            step,
        } => {
            let mut payload = question_header(question, strings)?;
            payload.push(width_flags(*width) | abi::flags::DISPLAY_UINT_DEC);
            push_width(&mut payload, *width, *minimum)?;
            push_width(&mut payload, *width, *maximum)?;
            push_width(&mut payload, *width, *step)?;
            push_op(out, abi::op::NUMERIC, false, &payload)
        }
        Statement::TextInput {
            question,
            minimum,
            maximum,
        } => {
            let mut payload = question_header(question, strings)?;
            payload.push(*minimum);
            payload.push(*maximum);
            payload.push(0);
            push_op(out, abi::op::STRING, false, &payload)
        }
    }
}

/// Appends a one-of question with its options.
fn encode_choice_statement(
    question: &Question,
    width: Width,
    choices: &[Choice],
    strings: &mut Strings,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    let mut payload = question_header(question, strings)?;
    payload.push(width_flags(width));
    push_width(&mut payload, width, 0)?;
    push_width(&mut payload, width, width_max(width))?;
    push_width(&mut payload, width, 0)?;
    push_op(out, abi::op::ONE_OF, true, &payload)?;
    for choice in choices {
        let mut payload = Vec::new();
        push_u16(&mut payload, strings.intern(&choice.text)?);
        payload.push(if choice.default {
            abi::flags::OPTION_DEFAULT
        } else {
            0
        });
        payload.push(width_flags(width));
        push_width(&mut payload, width, choice.value)?;
        push_op(out, abi::op::ONE_OF_OPTION, false, &payload)?;
    }
    push_op(out, abi::op::END, false, &[])
}

/// Appends a variable store declaration.
fn encode_varstore(store: &VarStore, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    let mut payload = Vec::new();
    match store.kind {
        VarStoreKind::Buffer => {
            payload.extend_from_slice(&store.guid);
            push_u16(&mut payload, store.id.0);
            push_u16(&mut payload, store.size);
            push_ascii(&mut payload, &store.name)?;
            push_op(out, abi::op::VARSTORE, false, &payload)
        }
        VarStoreKind::Efi { attributes } => {
            push_u16(&mut payload, store.id.0);
            payload.extend_from_slice(&store.guid);
            payload.extend_from_slice(&attributes.to_le_bytes());
            push_u16(&mut payload, store.size);
            push_ascii(&mut payload, &store.name)?;
            push_op(out, abi::op::VARSTORE_EFI, false, &payload)
        }
        VarStoreKind::NameValue => {
            push_u16(&mut payload, store.id.0);
            payload.extend_from_slice(&store.guid);
            push_op(out, abi::op::VARSTORE_NAME_VALUE, false, &payload)
        }
    }
}

/// Appends the question part shared by question statements.
fn question_header(question: &Question, strings: &mut Strings) -> Result<Vec<u8>, EncodeError> {
    let mut payload = Vec::new();
    push_u16(&mut payload, strings.intern(&question.prompt)?);
    push_u16(&mut payload, strings.intern(&question.help)?);
    push_u16(&mut payload, question.id.0);
    match &question.storage {
        Storage::None => {
            push_u16(&mut payload, 0);
            push_u16(&mut payload, 0);
        }
        Storage::Offset { varstore, offset } => {
            push_u16(&mut payload, varstore.0);
            push_u16(&mut payload, *offset);
        }
        Storage::Name { varstore, name } => {
            push_u16(&mut payload, varstore.0);
            push_u16(&mut payload, strings.intern(name)?);
        }
    }
    payload.push(question.flags.0);
    Ok(payload)
}

/// Appends a NUL-terminated ASCII name.
fn push_ascii(out: &mut Vec<u8>, name: &str) -> Result<(), EncodeError> {
    if !name.is_ascii() || name.contains('\0') {
        return Err(EncodeError::NonAsciiName);
    }
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    Ok(())
}

/// Appends a number at a width, rejecting values that do not fit.
fn push_width(out: &mut Vec<u8>, width: Width, value: u64) -> Result<(), EncodeError> {
    let bytes = width.bytes();
    if bytes < 8 && value >> (bytes * 8) != 0 {
        return Err(EncodeError::ValueOutOfRange);
    }
    out.extend_from_slice(&value.to_le_bytes()[..bytes]);
    Ok(())
}

/// Returns the largest value of a width.
fn width_max(width: Width) -> u64 {
    match width {
        Width::U8 => u64::from(u8::MAX),
        Width::U16 => u64::from(u16::MAX),
        Width::U32 => u64::from(u32::MAX),
        Width::U64 => u64::MAX,
    }
}

/// Returns the numeric size flags of a width.
fn width_flags(width: Width) -> u8 {
    match width {
        Width::U8 => 0x00,
        Width::U16 => 0x01,
        Width::U32 => 0x02,
        Width::U64 => 0x03,
    }
}

/// Appends a little-endian 16-bit number.
fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a NUL-terminated UTF-16 string.
fn push_ucs2(out: &mut Vec<u8>, text: &str) {
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
}
