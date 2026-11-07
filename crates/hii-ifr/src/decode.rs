//! Decoding of IFR and HII packages into the forms model.

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
            abi::op::SUPPRESS_IF => {
                let (expr, rest) = split_condition(node);
                let mut condition = condition.clone();
                condition.hidden = merge(condition.hidden, Some(expr));
                decode_nodes(
                    rest, condition, varstores, strings, decoding, forms, defaults, rows,
                )?;
            }
            abi::op::DISABLE_IF | abi::op::GRAY_OUT_IF => {
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

/// Returns the condition expression of a condition scope and its statements.
fn split_condition<'a, 'b>(node: &'b Node<'a>) -> (Expr, &'b [Node<'a>]) {
    match node.children.first() {
        Some(first) if is_expression(first.code) => (decode_expression(first), &node.children[1..]),
        _ => (Expr::Bool(true), &node.children[..]),
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
        abi::op::TRUE => Expr::Bool(true),
        abi::op::FALSE => Expr::Bool(false),
        abi::op::NOT => match node.children.first() {
            Some(child) => Expr::Not(Box::new(decode_expression(child))),
            None => Expr::Unavailable(String::from("NOT without an operand")),
        },
        abi::op::AND | abi::op::OR => {
            if node.children.len() != 2 {
                return Expr::Unavailable(String::from("binary operator without two operands"));
            }
            let first = Box::new(decode_expression(&node.children[0]));
            let second = Box::new(decode_expression(&node.children[1]));
            if node.code == abi::op::AND {
                Expr::And(first, second)
            } else {
                Expr::Or(first, second)
            }
        }
        abi::op::EQUAL | abi::op::LESS_THAN | abi::op::LESS_EQUAL => {
            if node.children.len() != 2 {
                return Expr::Unavailable(String::from("comparison without two operands"));
            }
            let first = decode_operand(&node.children[0]);
            let second = decode_operand(&node.children[1]);
            match node.code {
                abi::op::EQUAL => Expr::Equal(first, second),
                abi::op::LESS_THAN => Expr::Less(first, second),
                _ => Expr::LessEqual(first, second),
            }
        }
        abi::op::EQ_ID_VAL => match (read_u16(node.payload, 0), read_u16(node.payload, 2)) {
            (Some(question), Some(value)) => Expr::Equal(
                Operand::Field(Key(u64::from(question))),
                Operand::Literal(Value::Number(u64::from(value))),
            ),
            _ => Expr::Unavailable(String::from("EQ_ID_VAL is truncated")),
        },
        abi::op::EQ_ID_ID => match (read_u16(node.payload, 0), read_u16(node.payload, 2)) {
            (Some(first), Some(second)) => Expr::Equal(
                Operand::Field(Key(u64::from(first))),
                Operand::Field(Key(u64::from(second))),
            ),
            _ => Expr::Unavailable(String::from("EQ_ID_ID is truncated")),
        },
        abi::op::EQ_ID_VAL_LIST => {
            let Some(question) = read_u16(node.payload, 0) else {
                return Expr::Unavailable(String::from("EQ_ID_VAL_LIST is truncated"));
            };
            let Some(count) = read_u16(node.payload, 2) else {
                return Expr::Unavailable(String::from("EQ_ID_VAL_LIST is truncated"));
            };
            let count = usize::from(count);
            if node.payload.len() < 4 + count * 2 {
                return Expr::Unavailable(String::from("EQ_ID_VAL_LIST is truncated"));
            }
            let values = (0..count)
                .filter_map(|index| read_u16(node.payload, 4 + index * 2))
                .map(|value| Value::Number(u64::from(value)))
                .collect();
            Expr::In(Operand::Field(Key(u64::from(question))), values)
        }
        code => Expr::Unavailable(format!("opcode 0x{code:02X}")),
    }
}

/// Decodes one operand of a comparison.
fn decode_operand(node: &Node<'_>) -> Operand {
    match node.code {
        abi::op::QUESTION_REF1 => match read_u16(node.payload, 0) {
            Some(question) => Operand::Field(Key(u64::from(question))),
            None => Operand::Literal(Value::Bytes(Vec::new())),
        },
        abi::op::UINT8 => Operand::Literal(Value::Number(u64::from(
            node.payload.first().copied().unwrap_or(0),
        ))),
        abi::op::UINT16 => Operand::Literal(Value::Number(u64::from(
            read_u16(node.payload, 0).unwrap_or(0),
        ))),
        abi::op::UINT32 => Operand::Literal(Value::Number(u64::from(
            read_u32(node.payload, 0).unwrap_or(0),
        ))),
        abi::op::UINT64 => Operand::Literal(Value::Number(
            read_width(node.payload, 0, Width::U64).unwrap_or(0),
        )),
        abi::op::TRUE => Operand::Literal(Value::Bool(true)),
        abi::op::FALSE => Operand::Literal(Value::Bool(false)),
        _ => Operand::Literal(Value::Bytes(Vec::new())),
    }
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

/// Returns the storage that a question header names.
fn varstore_of(
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
fn decode_default(node: &Node<'_>) -> Option<Value> {
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
fn decode_choice(node: &Node<'_>, width: Width, strings: &BTreeMap<u16, String>) -> Option<Choice> {
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
fn decode_varstore_name_value(node: &Node<'_>) -> Result<StoreInfo, DecodeError> {
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

/// Tells whether an opcode is a question.
fn is_question(code: u8) -> bool {
    matches!(
        code,
        abi::op::CHECKBOX
            | abi::op::ONE_OF
            | abi::op::NUMERIC
            | abi::op::STRING
            | abi::op::PASSWORD
            | abi::op::ORDERED_LIST
            | abi::op::ACTION
    )
}

/// Tells whether an opcode is a condition expression.
fn is_expression(code: u8) -> bool {
    matches!(
        code,
        abi::op::TRUE
            | abi::op::FALSE
            | abi::op::NOT
            | abi::op::AND
            | abi::op::OR
            | abi::op::EQUAL
            | abi::op::LESS_THAN
            | abi::op::LESS_EQUAL
            | abi::op::EQ_ID_VAL
            | abi::op::EQ_ID_ID
            | abi::op::EQ_ID_VAL_LIST
            | abi::op::QUESTION_REF1
            | abi::op::UINT8
            | abi::op::UINT16
            | abi::op::UINT32
            | abi::op::UINT64
    )
}

/// Tells whether an opcode is a statement that the model represents.
fn is_statement(code: u8) -> bool {
    is_question(code)
        || matches!(
            code,
            abi::op::SUBTITLE | abi::op::TEXT | abi::op::REF | abi::op::RESET_BUTTON
        )
}
