//! Decoding of IFR and HII packages into the forms model.

mod bytes;
mod expr;
mod store;
mod strings;

use self::{bytes::*, expr::*, store::*, strings::*};

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use forms::{
    Choice, Control, Expr, FormSet, Key, Operand, Page, Refresh, Row, Target, Validation,
    ValidationKind, Value,
};

use crate::model::{
    Export, FormRef, Guid, QuestionKind, QuestionRef, Storage, VarStore, VarStoreKind, Width,
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

/// One form set with the HII identity that it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// The application pages.
    pub set: FormSet,
    /// The HII identity and storage of the set.
    pub export: Export,
}

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

/// The state of one decoding pass.
struct Decoding {
    /// The next key to allocate.
    next: u64,
    /// The row key of each question identifier.
    questions: BTreeMap<u16, Key>,
    /// The row key of each page, in publication order.
    pages: Vec<Key>,
    /// The storage of each question identifier.
    storage: BTreeMap<u16, Storage>,
    /// The question header flags of each question identifier.
    flags: BTreeMap<u16, u8>,
}

impl Decoding {
    /// Allocates one key.
    fn key(&mut self) -> Key {
        let key = Key(self.next);
        self.next = self.next.saturating_add(1);
        key
    }
}

/// Decodes every form set of a package list.
pub(crate) fn decode(package_list: &[u8]) -> Result<Vec<Decoded>, DecodeError> {
    if package_list.len() < abi::PACKAGE_LIST_HEADER_SIZE {
        return Err(DecodeError::Truncated);
    }
    let total = usize::try_from(read_u32(package_list, 16).ok_or(DecodeError::Truncated)?)
        .map_err(|_| DecodeError::Truncated)?;
    if total < abi::PACKAGE_LIST_HEADER_SIZE || total > package_list.len() {
        return Err(DecodeError::Truncated);
    }
    let list = &package_list[..total];

    let mut forms: Vec<&[u8]> = Vec::new();
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
    let mut next = 1u64;
    for data in forms {
        let nodes = parse_tree(data)?;
        for node in nodes.iter().filter(|node| node.code == abi::op::FORM_SET) {
            let mut decoding = Decoding {
                next,
                questions: BTreeMap::new(),
                pages: Vec::new(),
                storage: BTreeMap::new(),
                flags: BTreeMap::new(),
            };
            sets.push(build_formset(node, table, &mut decoding)?);
            next = decoding.next;
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

/// Builds one form set from its opcode.
fn build_formset(
    node: &Node<'_>,
    strings: &BTreeMap<u16, String>,
    decoding: &mut Decoding,
) -> Result<Decoded, DecodeError> {
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
    let class_guids: Vec<Guid> = tail
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

    let set_key = decoding.key();
    let mut defaults = Vec::new();
    for child in &node.children {
        if child.code == abi::op::DEFAULTSTORE {
            let id = read_u16(child.payload, 4).unwrap_or(0);
            defaults.push((id, text_of(strings, read_u16(child.payload, 0))));
        }
    }

    let form_nodes: Vec<&Node<'_>> = node
        .children
        .iter()
        .filter(|child| child.code == abi::op::FORM)
        .collect();
    for _ in &form_nodes {
        let key = decoding.key();
        decoding.pages.push(key);
    }
    let mut form_refs = Vec::new();
    for (index, form) in form_nodes.iter().enumerate() {
        let page_key = decoding.pages[index];
        let id = read_u16(form.payload, 0).unwrap_or(0);
        form_refs.push(FormRef {
            page: page_key,
            form: id,
        });
    }

    for form in &form_nodes {
        collect_questions(&form.children, decoding);
    }

    let mut default_values: BTreeMap<u16, BTreeMap<Key, Value>> = BTreeMap::new();
    let mut pages = Vec::new();
    for (index, form) in form_nodes.iter().enumerate() {
        let page = build_page(
            form,
            &varstores,
            strings,
            decoding,
            &form_refs,
            &mut default_values,
        )?;
        debug_assert_eq!(page.key, decoding.pages[index]);
        pages.push(page);
    }

    let mut set = FormSet {
        key: set_key,
        title,
        help,
        pages,
        defaults: Vec::new(),
    };
    for (id, name) in defaults {
        let key = default_key(id);
        let values = default_values.remove(&id).unwrap_or_default();
        set.defaults.push(forms::DefaultSet { key, name, values });
    }

    let questions = questions_of(&set, decoding);
    let export = Export {
        guid,
        class_guids,
        varstores: varstores.into_iter().map(|info| info.store).collect(),
        forms: form_refs,
        questions,
        refresh_events: Vec::new(),
    };
    Ok(Decoded { set, export })
}

/// Returns the question descriptors of a decoded set.
fn questions_of(set: &FormSet, decoding: &Decoding) -> Vec<QuestionRef> {
    let mut questions = Vec::new();
    for page in &set.pages {
        for row in &page.rows {
            let Some(binding) = row.binding else {
                continue;
            };
            let Some((id, _)) = decoding.questions.iter().find(|(_, key)| **key == binding) else {
                continue;
            };
            let (kind, bytes) = match &row.control {
                Control::Toggle => (QuestionKind::Bool, 1),
                Control::Choice(choices) => {
                    let width =
                        Width::of(choices.iter().map(|choice| choice.value).max().unwrap_or(0));
                    (QuestionKind::Number(width), width.bytes() as u16)
                }
                Control::Number { maximum, .. } => {
                    let width = Width::of(*maximum);
                    (QuestionKind::Number(width), width.bytes() as u16)
                }
                Control::TextInput {
                    maximum, secret, ..
                } => {
                    let bytes = (maximum.saturating_mul(2)).min(usize::from(u16::MAX)) as u16;
                    if *secret {
                        (QuestionKind::Password { maximum: *maximum }, bytes)
                    } else {
                        (QuestionKind::Text { maximum: *maximum }, bytes)
                    }
                }
                Control::OrderedList { maximum, .. } => (
                    QuestionKind::OrderedList,
                    (2u32.saturating_mul(*maximum as u32 + 1)).min(u32::from(u16::MAX)) as u16,
                ),
                Control::Action => (QuestionKind::Action, 0),
                _ => continue,
            };
            questions.push(QuestionRef {
                row: row.key,
                binding: row.binding.unwrap_or(row.key),
                question: *id,
                storage: decoding.storage.get(id).cloned().unwrap_or(Storage::None),
                bytes,
                kind,
                flags: decoding.flags.get(id).copied().unwrap_or(0),
            });
        }
    }
    questions
}

/// Returns the default set key of a HII default identifier.
fn default_key(id: u16) -> Key {
    Key(0xDEFA_0000_0000_0000 | u64::from(id))
}

/// Allocates a row key for each question opcode.
fn collect_questions(nodes: &[Node<'_>], decoding: &mut Decoding) {
    for node in nodes {
        if is_question(node.code)
            && let Some(id) = read_u16(node.payload, 4)
        {
            let key = decoding.key();
            decoding.questions.entry(id).or_insert(key);
        }
        collect_questions(&node.children, decoding);
    }
}

/// Builds one page from its form opcode.
fn build_page(
    node: &Node<'_>,
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
    decoding: &mut Decoding,
    forms: &[FormRef],
    defaults: &mut BTreeMap<u16, BTreeMap<Key, Value>>,
) -> Result<Page, DecodeError> {
    if node.payload.len() < 4 {
        return Err(DecodeError::BadLength);
    }
    let id = read_u16(node.payload, 0).ok_or(DecodeError::BadLength)?;
    let title = text_of(strings, read_u16(node.payload, 2));
    let key = decoding
        .pages
        .iter()
        .zip(forms)
        .find(|(_, form)| form.form == id)
        .map_or(Key::default(), |(page, _)| *page);
    let mut rows = Vec::new();
    decode_nodes(
        &node.children,
        Condition::default(),
        varstores,
        strings,
        decoding,
        forms,
        defaults,
        &mut rows,
    )?;
    Ok(Page { key, title, rows })
}

/// The conditions that apply to a scope.
#[derive(Clone, Default)]
struct Condition {
    hidden: Option<Expr>,
    disabled: Option<Expr>,
    locked: bool,
    refresh: Option<Refresh>,
    validation: Vec<Validation>,
    read_only: bool,
}

/// Decodes the statements of one scope, with the conditions that apply.
#[allow(clippy::too_many_arguments)]
fn decode_nodes(
    nodes: &[Node<'_>],
    condition: Condition,
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
    decoding: &mut Decoding,
    forms: &[FormRef],
    defaults: &mut BTreeMap<u16, BTreeMap<Key, Value>>,
    rows: &mut Vec<Row>,
) -> Result<(), DecodeError> {
    for node in nodes {
        match node.code {
            abi::op::SUPPRESS_IF | abi::op::DISABLE_IF => {
                // EDK2 does not draw a disabled statement either
                // (SetupBrowserDxe/Presentation.c:615), so both opcodes hide
                // the row.
                let (expr, rest) = split_condition(node);
                let mut condition = condition.clone();
                condition.hidden = merge(condition.hidden, Some(expr));
                decode_nodes(
                    rest, condition, varstores, strings, decoding, forms, defaults, rows,
                )?;
            }
            abi::op::GRAY_OUT_IF => {
                let (expr, rest) = split_condition(node);
                let mut condition = condition.clone();
                condition.disabled = merge(condition.disabled, Some(expr));
                decode_nodes(
                    rest, condition, varstores, strings, decoding, forms, defaults, rows,
                )?;
            }
            abi::op::WARNING_IF | abi::op::INCONSISTENT_IF | abi::op::NO_SUBMIT_IF => {
                let (expr, rest) = split_condition(node);
                let message = text_of(strings, read_u16(node.payload, 0));
                let timeout = if node.code == abi::op::WARNING_IF {
                    node.payload.get(2).copied().unwrap_or(0)
                } else {
                    0
                };
                let kind = match node.code {
                    abi::op::WARNING_IF => ValidationKind::Warning,
                    abi::op::INCONSISTENT_IF => ValidationKind::Edit,
                    _ => ValidationKind::Submit,
                };
                let mut condition = condition.clone();
                condition.validation.push(Validation {
                    kind,
                    condition: expr,
                    message,
                    timeout,
                });
                decode_nodes(
                    rest, condition, varstores, strings, decoding, forms, defaults, rows,
                )?;
            }
            abi::op::LOCKED => {
                let mut condition = condition.clone();
                condition.locked = true;
                decode_nodes(
                    &node.children,
                    condition,
                    varstores,
                    strings,
                    decoding,
                    forms,
                    defaults,
                    rows,
                )?;
            }
            abi::op::REFRESH => {
                let seconds = node.payload.first().copied().unwrap_or(0);
                let mut condition = condition.clone();
                condition.refresh = Some(Refresh::Periodic(u64::from(seconds) * 1_000));
                decode_nodes(
                    &node.children,
                    condition,
                    varstores,
                    strings,
                    decoding,
                    forms,
                    defaults,
                    rows,
                )?;
            }
            abi::op::REFRESH_ID => {
                let mut condition = condition.clone();
                condition.refresh = Some(Refresh::OnView);
                decode_nodes(
                    &node.children,
                    condition,
                    varstores,
                    strings,
                    decoding,
                    forms,
                    defaults,
                    rows,
                )?;
            }
            code if is_statement(code) => {
                rows.push(build_row(
                    node, &condition, varstores, strings, decoding, forms, defaults,
                )?);
            }
            code => {
                // A foreign opcode stays visible as an inert row.
                let label = text_of(strings, read_u16(node.payload, 0));
                let mut row = Row::new(
                    decoding.key(),
                    if label.is_empty() {
                        format!("Opcode 0x{code:02X}")
                    } else {
                        label
                    },
                    Control::Unsupported(format!("opcode 0x{code:02X}")),
                );
                row.hidden = condition.hidden.clone();
                row.disabled = condition.disabled.clone();
                rows.push(row);
                if !node.children.is_empty() {
                    decode_nodes(
                        &node.children,
                        condition.clone(),
                        varstores,
                        strings,
                        decoding,
                        forms,
                        defaults,
                        rows,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Builds one row from a statement opcode.
fn build_row(
    node: &Node<'_>,
    condition: &Condition,
    varstores: &[StoreInfo],
    strings: &BTreeMap<u16, String>,
    decoding: &mut Decoding,
    forms: &[FormRef],
    defaults: &mut BTreeMap<u16, BTreeMap<Key, Value>>,
) -> Result<Row, DecodeError> {
    let payload = node.payload;
    let question = read_u16(payload, 4).filter(|id| *id != 0);
    let key = question
        .and_then(|id| decoding.questions.get(&id).copied())
        .unwrap_or_else(|| decoding.key());
    let label = text_of(strings, read_u16(payload, 0));
    let help = text_of(strings, read_u16(payload, 2));
    let mut row = Row::new(key, label, Control::Text(String::new()));
    row.help = help;
    row.hidden = condition.hidden.clone();
    row.disabled = condition.disabled.clone();
    row.locked = condition.locked || condition.read_only;
    row.validation = condition.validation.clone();
    if let Some(refresh) = &condition.refresh {
        row.refresh = refresh.clone();
    }
    match node.code {
        abi::op::SUBTITLE => {
            row.control = Control::Heading;
        }
        abi::op::TEXT => {
            row.control = Control::Text(text_of(strings, read_u16(payload, 4)));
        }
        abi::op::REF => {
            let form = read_u16(payload, 11).unwrap_or(0);
            let page = forms
                .iter()
                .find(|form_ref| form_ref.form == form)
                .map_or(Key::default(), |form_ref| form_ref.page);
            row.control = Control::Link(Target {
                set: None,
                page,
                row: None,
            });
        }
        abi::op::ACTION => {
            row.control = Control::Action;
            row.binding = question.map(|_| key);
        }
        abi::op::CHECKBOX => {
            row.control = Control::Toggle;
            row.binding = question.map(|_| key);
        }
        abi::op::ONE_OF => {
            let width = width_of(payload.get(11).copied().unwrap_or(0));
            let choices = node
                .children
                .iter()
                .filter(|child| child.code == abi::op::ONE_OF_OPTION)
                .filter_map(|child| decode_choice(child, width, strings))
                .collect();
            row.control = Control::Choice(choices);
            row.binding = question.map(|_| key);
        }
        abi::op::NUMERIC => {
            let width = width_of(payload.get(11).copied().unwrap_or(0));
            let base = 12;
            let minimum = read_width(payload, base, width).unwrap_or(0);
            let maximum = read_width(payload, base + width.bytes(), width).unwrap_or(0);
            let step = read_width(payload, base + width.bytes() * 2, width).unwrap_or(0);
            row.control = Control::Number {
                minimum,
                maximum,
                step,
            };
            row.binding = question.map(|_| key);
        }
        abi::op::STRING => {
            let minimum = usize::from(payload.get(11).copied().unwrap_or(0));
            let maximum = usize::from(payload.get(12).copied().unwrap_or(0));
            row.control = Control::TextInput {
                minimum,
                maximum,
                secret: false,
            };
            row.binding = question.map(|_| key);
        }
        abi::op::PASSWORD => {
            let minimum = usize::from(read_u16(payload, 11).unwrap_or(0));
            let maximum = usize::from(read_u16(payload, 13).unwrap_or(0));
            row.control = Control::TextInput {
                minimum,
                maximum,
                secret: true,
            };
            row.binding = question.map(|_| key);
        }
        abi::op::ORDERED_LIST => {
            let maximum = usize::from(payload.get(11).copied().unwrap_or(0));
            let choices = node
                .children
                .iter()
                .filter(|child| child.code == abi::op::ONE_OF_OPTION)
                .filter_map(|child| decode_choice(child, Width::U16, strings))
                .collect();
            row.control = Control::OrderedList { choices, maximum };
            row.binding = question.map(|_| key);
        }
        abi::op::RESET_BUTTON => {
            let id = read_u16(payload, 4).unwrap_or(0);
            row.read_only = None;
            row.control = Control::Reset(default_key(id));
        }
        _ => {}
    }
    for child in &node.children {
        match child.code {
            abi::op::REFRESH => {
                let seconds = child.payload.first().copied().unwrap_or(0);
                row.refresh = Refresh::Periodic(u64::from(seconds) * 1_000);
            }
            abi::op::LOCKED => row.locked = true,
            _ => {}
        }
    }
    if let Some(binding) = row.binding {
        for child in &node.children {
            if child.code == abi::op::DEFAULT && child.payload.len() >= 4 {
                let id = read_u16(child.payload, 0).unwrap_or(0);
                let value = decode_default(child);
                if let Some(value) = value {
                    defaults.entry(id).or_default().insert(binding, value);
                }
            }
        }
        if let (Some(id), Some(store)) = (question, varstore_of(payload, varstores, strings)) {
            decoding.storage.insert(id, store);
        }
        if let Some(id) = question {
            decoding
                .flags
                .insert(id, payload.get(10).copied().unwrap_or(0));
        }
    }
    Ok(row)
}
