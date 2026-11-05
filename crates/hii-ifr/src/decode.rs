//! Decoding of IFR and HII packages into the forms model.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use forms::{
    Choice, Expr, Form, FormId, FormSet, Guid, Item, Question, QuestionFlags, QuestionId,
    Statement, Storage, VarStore, VarStoreId, VarStoreKind, Width,
};

use crate::utf16_to_string;
use hii_abi as abi;

/// A failure while decoding a package list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The package list or a package ends before its declared length.
    Truncated,
    /// A length field is smaller than its header or leaves its container.
    BadLength,
    /// A package extends past the package list, or uses a reserved type.
    BadPackage,
    /// A condition or statement scope is not closed by an end opcode.
    UnbalancedScope,
    /// A strings package or string block is malformed.
    BadStrings,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::Truncated => "the package list or a package is truncated",
            Self::BadLength => "a length field is inconsistent",
            Self::BadPackage => "a package is outside the list or has a reserved type",
            Self::UnbalancedScope => "a scope is not closed",
            Self::BadStrings => "a strings package is malformed",
        };
        formatter.write_str(reason)
    }
}

impl core::error::Error for DecodeError {}

/// One opcode with the opcodes of its scope.
struct Node<'a> {
    /// The opcode.
    code: u8,
    /// The bytes after the opcode header, within the opcode length.
    payload: &'a [u8],
    /// The opcodes of the scope, without the closing end opcode.
    children: Vec<Node<'a>>,
}

/// The strings of one strings package.
struct StringPackage {
    /// The language code, for example `en-US`.
    language: String,
    /// The strings by identifier.
    strings: BTreeMap<u16, String>,
}

/// A variable store and whether its questions name values instead of offsets.
struct StoreInfo {
    /// The declared store.
    store: VarStore,
    /// Named access, as for name/value stores and legacy EFI stores.
    named: bool,
}

/// Decodes a package list into every formset of its forms packages.
pub(crate) fn decode(package_list: &[u8]) -> Result<Vec<FormSet>, DecodeError> {
    if package_list.len() < abi::PACKAGE_LIST_HEADER_SIZE {
        return Err(DecodeError::Truncated);
    }
    let total = usize::try_from(read_u32(package_list, 16).ok_or(DecodeError::Truncated)?)
        .map_err(|_| DecodeError::Truncated)?;
    if total < abi::PACKAGE_LIST_HEADER_SIZE || total > package_list.len() {
        return Err(DecodeError::Truncated);
    }
    let list = &package_list[..total];

    let mut forms = Vec::new();
    let mut strings = Vec::new();
    let mut at = abi::PACKAGE_LIST_HEADER_SIZE;
    while at + abi::PACKAGE_HEADER_SIZE <= total {
        let header = read_u32(list, at).ok_or(DecodeError::Truncated)?;
        let length = usize::try_from(header & abi::MAX_PACKAGE_LENGTH)
            .map_err(|_| DecodeError::BadLength)?;
        let kind = (header >> 24) as u8;
        if length < abi::PACKAGE_HEADER_SIZE {
            return Err(DecodeError::BadLength);
        }
        let end = at.checked_add(length).ok_or(DecodeError::BadLength)?;
        if end > total {
            return Err(DecodeError::BadPackage);
        }
        match kind {
            abi::PACKAGE_FORMS => forms.push(&list[at + abi::PACKAGE_HEADER_SIZE..end]),
            abi::PACKAGE_STRINGS => strings.push(parse_string_package(&list[at..end])?),
            abi::PACKAGE_END => break,
            0xE0..=0xFF => return Err(DecodeError::BadPackage),
            _ => {}
        }
        at = end;
    }
    if forms.is_empty() {
        return Ok(Vec::new());
    }

    let empty = BTreeMap::new();
    let table = select_strings(&strings).unwrap_or(&empty);
    let mut sets = Vec::new();
    for data in forms {
        let nodes = parse_tree(data)?;
        for node in nodes.iter().filter(|node| node.code == abi::op::FORM_SET) {
            sets.push(build_formset(node, table)?);
        }
    }
    Ok(sets)
}

/// Splits an IFR stream into opcodes with their scopes.
fn parse_tree(data: &[u8]) -> Result<Vec<Node<'_>>, DecodeError> {
    let mut root = Vec::new();
    let mut stack: Vec<Node<'_>> = Vec::new();
    let mut at = 0;
    while at < data.len() {
        if at + 2 > data.len() {
            return Err(DecodeError::Truncated);
        }
        let code = data[at];
        let bits = data[at + 1];
        let length = usize::from(bits & 0x7F);
        let scope = bits & 0x80 != 0;
        if length < 2 {
            return Err(DecodeError::BadLength);
        }
        let end = at.checked_add(length).ok_or(DecodeError::BadLength)?;
        if end > data.len() {
            return Err(DecodeError::BadLength);
        }
        if code == abi::op::END {
            match stack.pop() {
                Some(node) => append(&mut root, &mut stack, node),
                None => break,
            }
        } else {
            let node = Node {
                code,
                payload: &data[at + 2..end],
                children: Vec::new(),
            };
            if scope {
                stack.push(node);
            } else {
                append(&mut root, &mut stack, node);
            }
        }
        at = end;
    }
    if !stack.is_empty() {
        return Err(DecodeError::UnbalancedScope);
    }
    Ok(root)
}

/// Appends a finished opcode to its parent scope.
fn append<'a>(root: &mut Vec<Node<'a>>, stack: &mut [Node<'a>], node: Node<'a>) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => root.push(node),
    }
}

/// Builds one formset from its opcode.
fn build_formset(node: &Node<'_>, strings: &BTreeMap<u16, String>) -> Result<FormSet, DecodeError> {
    let payload = node.payload;
    if payload.len() < 21 {
        return Err(DecodeError::BadLength);
    }
    let Some(guid) = guid_at(payload, 0) else {
        return Err(DecodeError::BadLength);
    };
    let title = text_of(strings, read_u16(payload, 16));
    let help = text_of(strings, read_u16(payload, 18));
    let count = usize::from(payload[20] & 0x03);
    let tail = &payload[21..];
    if tail.len() < count * 16 {
        return Err(DecodeError::BadLength);
    }
    let class_guids = tail
        .chunks_exact(16)
        .take(count)
        .filter_map(|chunk| guid_at(chunk, 0))
        .collect();

    let mut varstores = Vec::new();
    for child in &node.children {
        match child.code {
            abi::op::VARSTORE => varstores.push(decode_varstore_buffer(child)?),
            abi::op::VARSTORE_EFI => varstores.push(decode_varstore_efi(child)?),
            abi::op::VARSTORE_NAME_VALUE => varstores.push(decode_varstore_name_value(child)?),
            _ => {}
        }
    }

    let mut forms = Vec::new();
    for child in &node.children {
        if child.code == abi::op::FORM {
            forms.push(build_form(child, &varstores, strings)?);
        }
    }
    Ok(FormSet {
        guid,
        title,
        help,
        class_guids,
        varstores: varstores.iter().map(|info| info.store.clone()).collect(),
        forms,
    })
}

/// Builds one form from its opcode.
fn build_form(
    node: &Node<'_>,
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
) -> Result<Form, DecodeError> {
    let payload = node.payload;
    if payload.len() < 4 {
        return Err(DecodeError::BadLength);
    }
    let id = read_u16(payload, 0).ok_or(DecodeError::BadLength)?;
    let title = text_of(strings, read_u16(payload, 2));
    let mut items = Vec::new();
    decode_nodes(&node.children, None, None, varstores, strings, &mut items)?;
    Ok(Form {
        id: FormId(id),
        title,
        items,
    })
}

/// Decodes the statements of one scope, with the conditions that apply.
fn decode_nodes(
    nodes: &[Node<'_>],
    suppress: Option<Expr>,
    gray: Option<Expr>,
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
    items: &mut Vec<Item>,
) -> Result<(), DecodeError> {
    for node in nodes {
        match node.code {
            abi::op::SUPPRESS_IF => {
                let (expr, rest) = split_condition(node);
                let condition = merge(suppress.clone(), expr);
                decode_nodes(rest, condition, gray.clone(), varstores, strings, items)?;
            }
            abi::op::GRAY_OUT_IF => {
                let (expr, rest) = split_condition(node);
                let condition = merge(gray.clone(), expr);
                decode_nodes(rest, suppress.clone(), condition, varstores, strings, items)?;
            }
            code if is_statement(code) => {
                if code == abi::op::REF && node.payload.len() < 13 {
                    continue;
                }
                let statement = decode_statement(node, varstores, strings)?;
                items.push(Item {
                    statement,
                    suppress_if: suppress.clone(),
                    gray_out_if: gray.clone(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// Returns the condition expression of a condition scope and its statements.
fn split_condition<'a, 'b>(node: &'b Node<'a>) -> (Option<Expr>, &'b [Node<'a>]) {
    match node.children.first() {
        Some(first) if is_expression(first.code) => {
            (Some(decode_expression(first)), &node.children[1..])
        }
        _ => (None, &node.children[..]),
    }
}

/// Combines an outer and an inner condition.
fn merge(outer: Option<Expr>, inner: Option<Expr>) -> Option<Expr> {
    match (outer, inner) {
        (Some(outer), Some(inner)) => Some(Expr::And(Box::new(outer), Box::new(inner))),
        (outer, inner) => outer.or(inner),
    }
}

/// Decodes one expression tree.
fn decode_expression(node: &Node<'_>) -> Expr {
    match node.code {
        abi::op::TRUE => Expr::True,
        abi::op::FALSE => Expr::False,
        abi::op::NOT => match node.children.first() {
            Some(child) => Expr::Not(Box::new(decode_expression(child))),
            None => Expr::Unsupported,
        },
        abi::op::AND | abi::op::OR => {
            if node.children.len() != 2 {
                return Expr::Unsupported;
            }
            let first = Box::new(decode_expression(&node.children[0]));
            let second = Box::new(decode_expression(&node.children[1]));
            if node.code == abi::op::AND {
                Expr::And(first, second)
            } else {
                Expr::Or(first, second)
            }
        }
        abi::op::EQ_ID_VAL => match (read_u16(node.payload, 0), read_u16(node.payload, 2)) {
            (Some(question), Some(value)) => Expr::EqualValue {
                question: QuestionId(question),
                value: u64::from(value),
            },
            _ => Expr::Unsupported,
        },
        abi::op::EQ_ID_ID => match (read_u16(node.payload, 0), read_u16(node.payload, 2)) {
            (Some(first), Some(second)) => Expr::EqualQuestion {
                first: QuestionId(first),
                second: QuestionId(second),
            },
            _ => Expr::Unsupported,
        },
        abi::op::EQ_ID_VAL_LIST => {
            let Some(question) = read_u16(node.payload, 0) else {
                return Expr::Unsupported;
            };
            let Some(count) = read_u16(node.payload, 2) else {
                return Expr::Unsupported;
            };
            let count = usize::from(count);
            if node.payload.len() < 4 + count * 2 {
                return Expr::Unsupported;
            }
            let values = (0..count)
                .filter_map(|index| read_u16(node.payload, 4 + index * 2).map(u64::from))
                .collect();
            Expr::EqualList {
                question: QuestionId(question),
                values,
            }
        }
        _ => Expr::Unsupported,
    }
}

/// Decodes one statement.
fn decode_statement(
    node: &Node<'_>,
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
) -> Result<Statement, DecodeError> {
    let payload = node.payload;
    match node.code {
        abi::op::SUBTITLE => {
            if payload.len() < 5 {
                return Err(DecodeError::BadLength);
            }
            Ok(Statement::Subtitle {
                text: text_of(strings, read_u16(payload, 0)),
            })
        }
        abi::op::TEXT => {
            if payload.len() < 6 {
                return Err(DecodeError::BadLength);
            }
            Ok(Statement::Text {
                prompt: text_of(strings, read_u16(payload, 0)),
                help: text_of(strings, read_u16(payload, 2)),
                value: text_of(strings, read_u16(payload, 4)),
            })
        }
        abi::op::REF => {
            if payload.len() < 13 {
                return Err(DecodeError::BadLength);
            }
            let form = read_u16(payload, 11).ok_or(DecodeError::BadLength)?;
            let id = if payload.len() >= 15 {
                read_u16(payload, 13)
            } else {
                read_u16(payload, 4)
            };
            let question = id.filter(|id| *id != 0).map(QuestionId);
            Ok(Statement::Goto {
                prompt: text_of(strings, read_u16(payload, 0)),
                help: text_of(strings, read_u16(payload, 2)),
                form: FormId(form),
                question,
            })
        }
        abi::op::ACTION => Ok(Statement::Action {
            question: decode_question(payload, varstores, strings)?,
        }),
        abi::op::CHECKBOX => {
            if payload.len() < 12 {
                return Err(DecodeError::BadLength);
            }
            let mut default = payload[11] & abi::flags::CHECKBOX_DEFAULT != 0;
            for child in &node.children {
                if child.code == abi::op::DEFAULT
                    && child.payload.len() >= 4
                    && child.payload[2] == abi::value_type::BOOLEAN
                {
                    default = child.payload[3] != 0;
                }
            }
            Ok(Statement::Checkbox {
                question: decode_question(payload, varstores, strings)?,
                default,
            })
        }
        abi::op::ONE_OF => {
            if payload.len() < 12 {
                return Err(DecodeError::BadLength);
            }
            let width = width_of(payload[11]);
            let question = decode_question(payload, varstores, strings)?;
            let mut choices = Vec::new();
            let mut values = Vec::new();
            let mut default = None;
            for child in &node.children {
                match child.code {
                    abi::op::ONE_OF_OPTION => {
                        if let Some((choice, value)) = decode_choice(child, strings) {
                            values.push((choices.len(), value));
                            choices.push(choice);
                        }
                    }
                    abi::op::DEFAULT => {
                        if child.payload.len() >= 4
                            && let Some(width) = numeric_width(child.payload[2])
                        {
                            default = read_width(child.payload, 3, width);
                        }
                    }
                    _ => {}
                }
            }
            if let Some(default) = default {
                for (index, value) in values {
                    if value == default
                        && let Some(choice) = choices.get_mut(index)
                    {
                        choice.default = true;
                    }
                }
            }
            Ok(Statement::OneOf {
                question,
                width,
                choices,
            })
        }
        abi::op::NUMERIC => {
            if payload.len() < 12 {
                return Err(DecodeError::BadLength);
            }
            let width = width_of(payload[11]);
            let base = 12;
            let minimum = read_width(payload, base, width).ok_or(DecodeError::BadLength)?;
            let maximum =
                read_width(payload, base + width.bytes(), width).ok_or(DecodeError::BadLength)?;
            let step = read_width(payload, base + width.bytes() * 2, width)
                .ok_or(DecodeError::BadLength)?;
            Ok(Statement::Numeric {
                question: decode_question(payload, varstores, strings)?,
                width,
                minimum,
                maximum,
                step,
            })
        }
        abi::op::STRING => {
            if payload.len() < 14 {
                return Err(DecodeError::BadLength);
            }
            Ok(Statement::TextInput {
                question: decode_question(payload, varstores, strings)?,
                minimum: payload[11],
                maximum: payload[12],
            })
        }
        _ => Err(DecodeError::BadLength),
    }
}

/// Decodes a one-of option and its stored value.
fn decode_choice(node: &Node<'_>, strings: &BTreeMap<u16, String>) -> Option<(Choice, u64)> {
    let payload = node.payload;
    if payload.len() < 4 {
        return None;
    }
    let text = read_u16(payload, 0)?;
    let flags = payload[2];
    let width = numeric_width(payload[3])?;
    let value = read_width(payload, 4, width)?;
    let choice = Choice {
        text: text_of(strings, Some(text)),
        value,
        default: flags & abi::flags::OPTION_DEFAULT != 0,
    };
    Some((choice, value))
}

/// Decodes the question header of a question statement.
fn decode_question(
    payload: &[u8],
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
) -> Result<Question, DecodeError> {
    if payload.len() < 11 {
        return Err(DecodeError::BadLength);
    }
    let id = read_u16(payload, 4).ok_or(DecodeError::BadLength)?;
    let varstore = read_u16(payload, 6).ok_or(DecodeError::BadLength)?;
    let info = read_u16(payload, 8).ok_or(DecodeError::BadLength)?;
    let storage = storage_of(varstore, info, varstores, strings);
    Ok(Question {
        id: QuestionId(id),
        prompt: text_of(strings, read_u16(payload, 0)),
        help: text_of(strings, read_u16(payload, 2)),
        storage,
        flags: QuestionFlags(payload[10]),
    })
}

/// Resolves the storage of a question.
fn storage_of(
    varstore: u16,
    info: u16,
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
) -> Storage {
    if varstore == 0 {
        return Storage::None;
    }
    let Some(store) = varstores.iter().find(|store| store.store.id.0 == varstore) else {
        return Storage::None;
    };
    let varstore = VarStoreId(varstore);
    if store.named {
        Storage::Name {
            varstore,
            name: text_of(strings, Some(info)),
        }
    } else {
        Storage::Offset {
            varstore,
            offset: info,
        }
    }
}

/// Decodes a buffer store.
fn decode_varstore_buffer(node: &Node<'_>) -> Result<StoreInfo, DecodeError> {
    let payload = node.payload;
    if payload.len() < 20 {
        return Err(DecodeError::BadLength);
    }
    let guid = guid_at(payload, 0).ok_or(DecodeError::BadLength)?;
    let id = read_u16(payload, 16).ok_or(DecodeError::BadLength)?;
    let size = read_u16(payload, 18).ok_or(DecodeError::BadLength)?;
    Ok(StoreInfo {
        store: VarStore {
            id: VarStoreId(id),
            guid,
            name: ascii_name(&payload[20..]),
            size,
            kind: VarStoreKind::Buffer,
        },
        named: false,
    })
}

/// Decodes an EFI variable store.
fn decode_varstore_efi(node: &Node<'_>) -> Result<StoreInfo, DecodeError> {
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
            id: VarStoreId(id),
            guid,
            name,
            size,
            kind: VarStoreKind::Efi { attributes },
        },
        named: legacy,
    })
}

/// Decodes a name/value store.
fn decode_varstore_name_value(node: &Node<'_>) -> Result<StoreInfo, DecodeError> {
    let payload = node.payload;
    if payload.len() < 18 {
        return Err(DecodeError::BadLength);
    }
    let id = read_u16(payload, 0).ok_or(DecodeError::BadLength)?;
    let guid = guid_at(payload, 2).ok_or(DecodeError::BadLength)?;
    Ok(StoreInfo {
        store: VarStore {
            id: VarStoreId(id),
            guid,
            name: String::new(),
            size: 0,
            kind: VarStoreKind::NameValue,
        },
        named: true,
    })
}

/// Parses one strings package.
fn parse_string_package(package: &[u8]) -> Result<StringPackage, DecodeError> {
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
fn select_strings(packages: &[StringPackage]) -> Option<&BTreeMap<u16, String>> {
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
fn read_ucs2(package: &[u8], at: usize) -> Result<(String, usize), DecodeError> {
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
fn skip_scsu(package: &[u8], at: usize, count: u16) -> Result<usize, DecodeError> {
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
fn skip_block(package: &[u8], at: usize, length: usize) -> Result<usize, DecodeError> {
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
fn insert(strings: &mut BTreeMap<u16, String>, id: u32, text: String) {
    if let Ok(id) = u16::try_from(id) {
        strings.insert(id, text);
    }
}

/// Reads a null-terminated ASCII name.
fn ascii_name(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Reads a null-terminated ASCII string.
fn c_string(bytes: &[u8]) -> String {
    ascii_name(bytes)
}

/// Reads a little-endian 16-bit number.
fn read_u16(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Reads a little-endian 32-bit number.
fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Reads a number at a width.
fn read_width(data: &[u8], at: usize, width: Width) -> Option<u64> {
    let bytes = data.get(at..at + width.bytes())?;
    let mut number = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        number |= u64::from(*byte) << (index * 8);
    }
    Some(number)
}

/// Reads 16 GUID bytes.
fn guid_at(data: &[u8], at: usize) -> Option<Guid> {
    let bytes = data.get(at..at + 16)?;
    let mut guid = [0u8; 16];
    guid.copy_from_slice(bytes);
    Some(guid)
}

/// Returns the width of the numeric size flags.
fn width_of(flags: u8) -> Width {
    match flags & abi::flags::NUMERIC_SIZE {
        0 => Width::U8,
        1 => Width::U16,
        2 => Width::U32,
        _ => Width::U64,
    }
}

/// Returns the width of a numeric value type.
fn numeric_width(kind: u8) -> Option<Width> {
    match kind {
        abi::value_type::NUM_SIZE_8 => Some(Width::U8),
        abi::value_type::NUM_SIZE_16 => Some(Width::U16),
        abi::value_type::NUM_SIZE_32 => Some(Width::U32),
        abi::value_type::NUM_SIZE_64 => Some(Width::U64),
        _ => None,
    }
}

/// Returns the string of an identifier.
fn text_of(strings: &BTreeMap<u16, String>, id: Option<u16>) -> String {
    id.and_then(|id| strings.get(&id).cloned())
        .unwrap_or_default()
}

/// Tells whether an opcode is a condition expression.
fn is_expression(code: u8) -> bool {
    matches!(
        code,
        0x12..=0x17 | 0x2A..=0x2C | 0x2F..=0x59 | 0x5E | 0x20..=0x22 | 0x28 | 0x60 | 0x64
    )
}

/// Tells whether an opcode is a statement that the model represents.
fn is_statement(code: u8) -> bool {
    matches!(
        code,
        abi::op::SUBTITLE
            | abi::op::TEXT
            | abi::op::REF
            | abi::op::ACTION
            | abi::op::CHECKBOX
            | abi::op::ONE_OF
            | abi::op::NUMERIC
            | abi::op::STRING
    )
}
