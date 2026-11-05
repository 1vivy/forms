//! A host-testable HII producer over a [`forms::Provider`].
//!
//! A UEFI driver publishes HII packages and answers configuration strings.
//! This module holds the part that needs no firmware. [`PreparedFormSet`]
//! rewrites one [`FormSet`] so that every value question uses one buffer
//! variable store. The same type then builds the package list and serves the
//! `<ConfigRequest>` and `<ConfigResp>` strings of the HII configuration
//! routing protocol through the provider.
//!
//! Question values use the codec in `hii_ifr`: numbers take their question
//! width, a checkbox takes one byte, and text takes UTF-16 code units padded
//! to the question width. A partial `<ConfigResp>` overlays the current
//! buffer. Only questions whose value changes go to [`Provider::write`].
//!
//! Action questions stay outside the buffer and keep no storage. The producer
//! does not run actions. The Config Access callback reports
//! `EFI_UNSUPPORTED` for them.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use forms::{FormSet, Provider, Question, Statement, Storage, VarStore, VarStoreId, VarStoreKind};
use hii_ifr::{EncodeError, config, value_bytes, value_from};
use uefi_raw::Status;

/// The identifier of the buffer store that the producer publishes.
pub const VARSTORE_ID: VarStoreId = VarStoreId(1);

/// The name of the buffer store that the producer publishes.
pub const VARSTORE_NAME: &str = "Forms";

/// A configuration failure with its firmware status and input position.
///
/// `progress` is the byte offset in the request or configuration string that
/// the producer rejected. It is 0 when the failure has no input position, for
/// example a package encoding failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError {
    /// The firmware status to return to the caller.
    pub status: Status,
    /// The byte offset in the input string, or 0 when none applies.
    pub progress: usize,
}

impl ConfigError {
    /// Builds a failure from a status and an input byte offset.
    pub const fn new(status: Status, progress: usize) -> Self {
        Self { status, progress }
    }

    /// Builds a failure from a provider error and an input byte offset.
    ///
    /// The provider status code is preserved.
    pub fn from_provider(error: &forms::Error, progress: usize) -> Self {
        let code = usize::try_from(error.code).unwrap_or(Status::DEVICE_ERROR.0);
        Self::new(Status(code), progress)
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "configuration error {} at byte {}",
            self.status, self.progress
        )
    }
}

impl core::error::Error for ConfigError {}

impl From<EncodeError> for ConfigError {
    fn from(_error: EncodeError) -> Self {
        Self::new(Status::INVALID_PARAMETER, 0)
    }
}

/// One value question and its slice of the buffer store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    offset: u16,
    width: u16,
}

/// A formset with every value question mapped to one buffer store.
///
/// [`Self::new`] removes the declared variable stores, assigns the questions
/// sequential byte offsets in publication order, and declares one buffer store
/// with identifier [`VARSTORE_ID`] and name [`VARSTORE_NAME`]. The store GUID
/// is the formset GUID.
#[derive(Debug)]
pub struct PreparedFormSet {
    set: FormSet,
    ranges: Vec<Range>,
    size: u16,
}

impl PreparedFormSet {
    /// Maps every value question of `set` to one buffer store.
    ///
    /// Checkbox values take one byte. Numbers take the width of their
    /// statement. Text takes `maximum * 2` bytes. Action questions keep no
    /// storage, because the producer does not run them.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] when the mapped values exceed the
    /// 16-bit store size, or when a text question accepts no characters.
    pub fn new(mut set: FormSet) -> Result<Self, ConfigError> {
        let mut ranges = Vec::new();
        let mut next = 0u32;
        for form in &mut set.forms {
            for item in &mut form.items {
                match &mut item.statement {
                    Statement::Checkbox { question, .. } => {
                        remap(question, 1, &mut next, &mut ranges)?;
                    }
                    Statement::OneOf {
                        question, width, ..
                    }
                    | Statement::Numeric {
                        question, width, ..
                    } => {
                        let width = u16::try_from(width.bytes()).map_err(|_| overflow())?;
                        remap(question, width, &mut next, &mut ranges)?;
                    }
                    Statement::TextInput {
                        question, maximum, ..
                    } => {
                        if *maximum == 0 {
                            return Err(ConfigError::new(Status::INVALID_PARAMETER, 0));
                        }
                        let width = u16::from(*maximum) * 2;
                        remap(question, width, &mut next, &mut ranges)?;
                    }
                    Statement::Action { question } => {
                        question.storage = Storage::None;
                    }
                    Statement::Subtitle { .. }
                    | Statement::Text { .. }
                    | Statement::Goto { .. } => {}
                }
            }
        }
        let size = u16::try_from(next).map_err(|_| overflow())?;
        set.varstores = vec![VarStore {
            id: VARSTORE_ID,
            guid: set.guid,
            name: String::from(VARSTORE_NAME),
            size,
            kind: VarStoreKind::Buffer,
        }];
        Ok(Self { set, ranges, size })
    }

    /// Returns the mapped formset.
    pub fn formset(&self) -> &FormSet {
        &self.set
    }

    /// Encodes the mapped formset as an HII package list.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] when the model cannot be encoded.
    /// The progress is 0.
    pub fn package_list(&self) -> Result<Vec<u8>, ConfigError> {
        Ok(hii_ifr::encode(&self.set, self.set.guid)?)
    }

    /// Builds the `<ConfigHdr>` of the buffer store for one device path.
    pub fn config_header(&self, path: &[u8]) -> String {
        config::header(&self.set.guid, VARSTORE_NAME, path)
    }

    /// Answers a `<ConfigRequest>` from the provider.
    ///
    /// `request` is `None` or empty for every question. A request with a
    /// `<ConfigHdr>` must name this producer's store, name and path. A request
    /// without a header holds only `OFFSET` and `WIDTH` elements.
    ///
    /// The answer is a `<ConfigResp>` with the requested blocks. Its header is
    /// the canonical header of this producer.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] for a malformed header, element,
    /// number, value, or a block outside the store. Returns the provider
    /// status when a read fails.
    /// `progress` is the byte offset of the rejected element, or 0 when the
    /// failure has no input position.
    pub fn extract<P: Provider>(
        &self,
        provider: &mut P,
        handle: P::Handle,
        request: Option<&str>,
        path: &[u8],
    ) -> Result<String, ConfigError> {
        let header = self.config_header(path);
        let ranges = match request {
            None => self.all_ranges(),
            Some(text) => {
                let (body, base) = self.body(text, path)?;
                let elements = elements(body, base)?;
                if elements.is_empty() {
                    self.all_ranges()
                } else {
                    parse_request(&elements, self.size)?
                }
            }
        };
        let buffer = self.read_buffer(provider, handle)?;
        let mut blocks = Vec::with_capacity(ranges.len());
        for (offset, width) in ranges {
            let start = usize::from(offset);
            let end = start + usize::from(width);
            blocks.push(config::Block {
                offset,
                width,
                bytes: buffer[start..end].to_vec(),
            });
        }
        Ok(config::response(&header, &blocks))
    }

    /// Routes a `<ConfigResp>` to the provider.
    ///
    /// The response must name this producer's store, name and path. Each
    /// block overlays the current buffer, so a block may cover part of a
    /// question. Only questions whose value changes go to the provider. The
    /// provider request in its answer is ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] for a malformed header, element,
    /// number, value, UTF-16 sequence, or a block outside the store. Returns
    /// the provider status when a read or write fails. `progress` is the byte
    /// offset of the rejected or failing block, or 0 when no block applies.
    pub fn route<P: Provider>(
        &self,
        provider: &mut P,
        handle: P::Handle,
        configuration: &str,
        path: &[u8],
    ) -> Result<(), ConfigError> {
        let (body, base) = self.body(configuration, path)?;
        let elements = elements(body, base)?;
        let blocks = parse_response(&elements, self.size)?;
        if blocks.is_empty() {
            return Ok(());
        }
        let current = self.read_buffer(provider, handle)?;
        let mut buffer = current.clone();
        for block in &blocks {
            let start = usize::from(block.offset);
            let end = start + usize::from(block.width);
            buffer[start..end].copy_from_slice(&block.bytes);
        }
        for (question, statement, offset, width) in self.value_questions() {
            let start = usize::from(offset);
            let end = start + usize::from(width);
            if buffer[start..end] == current[start..end] {
                continue;
            }
            let progress = block_progress(offset, width, &blocks);
            let value = decode_value(statement, &buffer[start..end])
                .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, progress))?;
            let encoded = value_bytes(statement, &value)
                .ok_or_else(|| ConfigError::new(Status::DEVICE_ERROR, progress))?;
            if encoded.len() != usize::from(width) {
                return Err(ConfigError::new(Status::DEVICE_ERROR, progress));
            }
            if encoded.as_slice() == &current[start..end] {
                continue;
            }
            provider
                .write(handle, &self.set, question, &value)
                .map_err(|error| ConfigError::from_provider(&error, progress))?;
        }
        Ok(())
    }

    /// Returns every question range in publication order.
    fn all_ranges(&self) -> Vec<(u16, u16)> {
        self.ranges
            .iter()
            .map(|range| (range.offset, range.width))
            .collect()
    }

    /// Returns every mapped value question with its statement and range.
    fn value_questions(&self) -> impl Iterator<Item = (&Question, &Statement, u16, u16)> + '_ {
        self.set
            .forms
            .iter()
            .flat_map(|form| form.items.iter())
            .filter_map(|item| {
                let width = stored_width(&item.statement)?;
                let (Statement::Checkbox { question, .. }
                | Statement::OneOf { question, .. }
                | Statement::Numeric { question, .. }
                | Statement::TextInput { question, .. }) = &item.statement
                else {
                    return None;
                };
                let Storage::Offset { varstore, offset } = &question.storage else {
                    return None;
                };
                (*varstore == VARSTORE_ID).then_some((question, &item.statement, *offset, width))
            })
    }

    /// Reads every value question into one buffer.
    fn read_buffer<P: Provider>(
        &self,
        provider: &mut P,
        handle: P::Handle,
    ) -> Result<Vec<u8>, ConfigError> {
        let mut buffer = vec![0u8; usize::from(self.size)];
        for (question, statement, offset, width) in self.value_questions() {
            let value = provider
                .read(handle, &self.set, question)
                .map_err(|error| ConfigError::from_provider(&error, 0))?;
            let bytes = value_bytes(statement, &value)
                .ok_or_else(|| ConfigError::new(Status::DEVICE_ERROR, 0))?;
            if bytes.len() != usize::from(width) {
                return Err(ConfigError::new(Status::DEVICE_ERROR, 0));
            }
            let start = usize::from(offset);
            let end = start + usize::from(width);
            buffer[start..end].copy_from_slice(&bytes);
        }
        Ok(buffer)
    }

    /// Returns the body of an input string and its byte offset.
    ///
    /// A string without a header must be empty or start with `&`. A string
    /// with a header must match the canonical header exactly, ignoring
    /// hexadecimal digit case.
    fn body<'a>(&self, input: &'a str, path: &[u8]) -> Result<(&'a str, usize), ConfigError> {
        if !input.starts_with("GUID=") {
            if input.is_empty() || input.starts_with('&') {
                return Ok((input, 0));
            }
            return Err(ConfigError::new(Status::INVALID_PARAMETER, 0));
        }
        let expected = self.config_header(path);
        let input_bytes = input.as_bytes();
        let expected_bytes = expected.as_bytes();
        for (index, (input_byte, expected_byte)) in
            input_bytes.iter().zip(expected_bytes).enumerate()
        {
            if !input_byte.eq_ignore_ascii_case(expected_byte) {
                return Err(ConfigError::new(Status::INVALID_PARAMETER, index));
            }
        }
        if input_bytes.len() < expected_bytes.len() {
            return Err(ConfigError::new(
                Status::INVALID_PARAMETER,
                input_bytes.len(),
            ));
        }
        let rest = &input[expected.len()..];
        if rest.is_empty() || rest.starts_with('&') {
            Ok((rest, expected.len()))
        } else {
            Err(ConfigError::new(Status::INVALID_PARAMETER, expected.len()))
        }
    }
}

/// Assigns the next buffer offset to one value question.
fn remap(
    question: &mut Question,
    width: u16,
    next: &mut u32,
    ranges: &mut Vec<Range>,
) -> Result<(), ConfigError> {
    let offset = u16::try_from(*next).map_err(|_| overflow())?;
    let end = *next + u32::from(width);
    if end > u32::from(u16::MAX) {
        return Err(overflow());
    }
    question.storage = Storage::Offset {
        varstore: VARSTORE_ID,
        offset,
    };
    ranges.push(Range { offset, width });
    *next = end;
    Ok(())
}

/// Returns the store size failure.
fn overflow() -> ConfigError {
    ConfigError::new(Status::INVALID_PARAMETER, 0)
}

/// Returns the stored width of a value statement.
fn stored_width(statement: &Statement) -> Option<u16> {
    match statement {
        Statement::Checkbox { .. } => Some(1),
        Statement::OneOf { width, .. } | Statement::Numeric { width, .. } => {
            u16::try_from(width.bytes()).ok()
        }
        Statement::TextInput { maximum, .. } => Some(u16::from(*maximum) * 2),
        _ => None,
    }
}

/// Decodes a question value from its stored bytes.
fn decode_value(statement: &Statement, bytes: &[u8]) -> Option<forms::Value> {
    if matches!(statement, Statement::TextInput { .. }) {
        decode_text(bytes)
    } else {
        value_from(statement, bytes)
    }
}

/// Decodes UTF-16 code units up to the first NUL unit.
///
/// Unpaired surrogates are rejected.
fn decode_text(bytes: &[u8]) -> Option<forms::Value> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units = bytes
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0);
    let mut text = String::new();
    for unit in core::char::decode_utf16(units) {
        match unit {
            Ok(character) => text.push(character),
            Err(_) => return None,
        }
    }
    Some(forms::Value::Text(text))
}

/// One `&NAME=VALUE` element and its byte offset in the input.
struct Element<'a> {
    name: &'a str,
    value: &'a str,
    start: usize,
}

/// One parsed configuration block.
struct InputBlock {
    offset: u16,
    width: u16,
    bytes: Vec<u8>,
    start: usize,
}

/// Splits an element body into `&NAME=VALUE` elements.
fn elements(input: &str, base: usize) -> Result<Vec<Element<'_>>, ConfigError> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < input.len() {
        if input.as_bytes()[at] != b'&' {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, base + at));
        }
        let start = at;
        at += 1;
        let end = input[at..]
            .find('&')
            .map_or(input.len(), |found| at + found);
        let (name, value) = input[at..end]
            .split_once('=')
            .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, base + start))?;
        if name.is_empty() {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, base + start));
        }
        out.push(Element {
            name,
            value,
            start: base + start,
        });
        at = end;
    }
    Ok(out)
}

/// Parses `OFFSET` and `WIDTH` pairs of a `<ConfigRequest>`.
fn parse_request(elements: &[Element<'_>], size: u16) -> Result<Vec<(u16, u16)>, ConfigError> {
    let mut ranges = Vec::new();
    for pair in elements.chunks(2) {
        let offset = &pair[0];
        if offset.name != "OFFSET" {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, offset.start));
        }
        let width = pair
            .get(1)
            .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, offset.start))?;
        if width.name != "WIDTH" {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, width.start));
        }
        let offset = parse_number(offset)?;
        let width = parse_number(width)?;
        check_range(offset, width, size, pair[0].start)?;
        ranges.push((offset, width));
    }
    Ok(ranges)
}

/// Parses `OFFSET`, `WIDTH` and `VALUE` triples of a `<ConfigResp>`.
fn parse_response(elements: &[Element<'_>], size: u16) -> Result<Vec<InputBlock>, ConfigError> {
    let mut blocks = Vec::new();
    for triple in elements.chunks(3) {
        let offset = &triple[0];
        if offset.name != "OFFSET" {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, offset.start));
        }
        let width = triple
            .get(1)
            .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, offset.start))?;
        if width.name != "WIDTH" {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, width.start));
        }
        let value = triple
            .get(2)
            .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, offset.start))?;
        if value.name != "VALUE" {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, value.start));
        }
        let offset = parse_number(offset)?;
        let width = parse_number(width)?;
        let bytes = parse_hex(value)?;
        if bytes.len() != usize::from(width) {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, value.start));
        }
        check_range(offset, width, size, triple[0].start)?;
        blocks.push(InputBlock {
            offset,
            width,
            bytes,
            start: triple[0].start,
        });
    }
    Ok(blocks)
}

/// Parses a 16-bit hexadecimal element value.
fn parse_number(element: &Element<'_>) -> Result<u16, ConfigError> {
    if element.value.is_empty()
        || element.value.len() > 4
        || !element.value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ConfigError::new(Status::INVALID_PARAMETER, element.start));
    }
    u16::from_str_radix(element.value, 16)
        .map_err(|_| ConfigError::new(Status::INVALID_PARAMETER, element.start))
}

/// Parses an even-length hexadecimal byte string.
fn parse_hex(element: &Element<'_>) -> Result<Vec<u8>, ConfigError> {
    if !element.value.len().is_multiple_of(2)
        || !element.value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ConfigError::new(Status::INVALID_PARAMETER, element.start));
    }
    (0..element.value.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(&element.value[at..at + 2], 16)
                .map_err(|_| ConfigError::new(Status::INVALID_PARAMETER, element.start))
        })
        .collect()
}

/// Checks that a block lies inside the store and has a nonzero width.
fn check_range(offset: u16, width: u16, size: u16, progress: usize) -> Result<(), ConfigError> {
    if width == 0 {
        return Err(ConfigError::new(Status::INVALID_PARAMETER, progress));
    }
    if u32::from(offset) + u32::from(width) > u32::from(size) {
        return Err(ConfigError::new(Status::INVALID_PARAMETER, progress));
    }
    Ok(())
}

/// Returns the input offset of the first block that overlaps a question.
fn block_progress(offset: u16, width: u16, blocks: &[InputBlock]) -> usize {
    let start = u32::from(offset);
    let end = start + u32::from(width);
    blocks
        .iter()
        .find(|block| {
            let block_start = u32::from(block.offset);
            let block_end = block_start + u32::from(block.width);
            block_start < end && start < block_end
        })
        .map_or(0, |block| block.start)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::format;
    use alloc::string::ToString;

    use forms::{Error, Form, FormId, Item, QuestionFlags, QuestionId, Response, Value, Width};

    use super::*;

    const PATH: [u8; 4] = [1, 4, 20, 0];

    /// Builds a question with the given storage.
    fn question(id: u16, storage: Storage) -> Question {
        Question {
            id: QuestionId(id),
            prompt: String::new(),
            help: String::new(),
            storage,
            flags: QuestionFlags::default(),
        }
    }

    /// Builds a formset around a list of statements.
    fn formset(items: Vec<Item>) -> FormSet {
        FormSet {
            guid: [0x42; 16],
            title: "Forms".to_string(),
            help: String::new(),
            class_guids: Vec::new(),
            varstores: Vec::new(),
            forms: vec![Form {
                id: FormId(1),
                title: "Main".to_string(),
                items,
            }],
        }
    }

    /// Returns the storage of the item with this question identifier.
    fn storage_of(set: &FormSet, id: u16) -> Storage {
        set.forms[0]
            .items
            .iter()
            .find_map(|item| {
                item.statement
                    .question()
                    .filter(|question| question.id == QuestionId(id))
                    .map(|question| question.storage.clone())
            })
            .expect("question exists")
    }

    /// Builds a formset with a checkbox, a `u16` numeric, a text and a length.
    fn sample() -> FormSet {
        formset(vec![
            Statement::Checkbox {
                question: question(
                    1,
                    Storage::Name {
                        varstore: VarStoreId(7),
                        name: "flag".to_string(),
                    },
                ),
                default: false,
            }
            .into(),
            Statement::Numeric {
                question: question(
                    2,
                    Storage::Offset {
                        varstore: VarStoreId(7),
                        offset: 9,
                    },
                ),
                width: Width::U16,
                minimum: 0,
                maximum: 1000,
                step: 1,
            }
            .into(),
            Statement::TextInput {
                question: question(3, Storage::None),
                minimum: 0,
                maximum: 4,
            }
            .into(),
        ])
    }

    /// Builds a provider that serves fixed values.
    #[derive(Default)]
    struct FakeProvider {
        values: Vec<(QuestionId, Value)>,
        writes: Vec<(QuestionId, Value)>,
        read_failure: Option<Status>,
        write_failure: Option<Status>,
    }

    impl FakeProvider {
        /// Builds a provider with the given values.
        fn with(values: Vec<(QuestionId, Value)>) -> Self {
            Self {
                values,
                ..Self::default()
            }
        }

        /// Returns the value of a question.
        ///
        /// The sample formset uses checkboxes, numbers and text in order, so
        /// an absent value has the type of that question.
        fn value(&self, id: QuestionId) -> Value {
            self.values
                .iter()
                .find(|(question, _)| *question == id)
                .map_or_else(
                    || match id.0 {
                        1 => Value::Bool(false),
                        2 => Value::Number(0),
                        3 => Value::Text(String::new()),
                        _ => Value::Bytes(Vec::new()),
                    },
                    |(_, value)| value.clone(),
                )
        }
    }

    impl Provider for FakeProvider {
        type Handle = u8;

        fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error> {
            Ok(Vec::new())
        }

        fn read(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
        ) -> Result<Value, Error> {
            if let Some(status) = self.read_failure {
                return Err(Error {
                    code: status.0 as u64,
                    reason: "read refused".to_string(),
                });
            }
            Ok(self.value(question.id))
        }

        fn write(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
            value: &Value,
        ) -> Result<Response, Error> {
            if let Some(status) = self.write_failure {
                return Err(Error {
                    code: status.0 as u64,
                    reason: "write refused".to_string(),
                });
            }
            self.writes.push((question.id, value.clone()));
            Ok(Response::None)
        }

        fn action(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            _question: &Question,
        ) -> Result<Response, Error> {
            Ok(Response::None)
        }
    }

    #[test]
    fn mapping_uses_one_buffer_store_with_sequential_offsets() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let set = prepared.formset();
        assert_eq!(set.varstores.len(), 1);
        let store = &set.varstores[0];
        assert_eq!(store.id, VARSTORE_ID);
        assert_eq!(store.name, VARSTORE_NAME);
        assert_eq!(store.guid, [0x42; 16]);
        assert_eq!(store.kind, VarStoreKind::Buffer);
        assert_eq!(store.size, 1 + 2 + 8);
        assert_eq!(
            storage_of(set, 1),
            Storage::Offset {
                varstore: VARSTORE_ID,
                offset: 0,
            }
        );
        assert_eq!(
            storage_of(set, 2),
            Storage::Offset {
                varstore: VARSTORE_ID,
                offset: 1,
            }
        );
        assert_eq!(
            storage_of(set, 3),
            Storage::Offset {
                varstore: VARSTORE_ID,
                offset: 3,
            }
        );
    }

    #[test]
    fn encoded_package_list_keeps_the_mapping() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let packages = prepared.package_list().expect("encoding succeeds");
        let decoded = hii_ifr::decode(&packages).expect("decoding succeeds");
        assert_eq!(decoded.len(), 1);
        let store = &decoded[0].varstores[0];
        assert_eq!(store.id, VARSTORE_ID);
        assert_eq!(store.name, VARSTORE_NAME);
        assert_eq!(store.size, 11);
        let numeric = decoded[0].forms[0]
            .items
            .iter()
            .find_map(|item| {
                item.statement
                    .question()
                    .filter(|question| question.id == QuestionId(2))
            })
            .expect("numeric question exists");
        assert_eq!(
            numeric.storage,
            Storage::Offset {
                varstore: VARSTORE_ID,
                offset: 1,
            }
        );
    }

    #[test]
    fn mapping_refuses_a_store_larger_than_16_bits() {
        let items: Vec<Item> = (1..=129)
            .map(|id| {
                Statement::TextInput {
                    question: question(id, Storage::None),
                    minimum: 0,
                    maximum: 255,
                }
                .into()
            })
            .collect();
        let error = PreparedFormSet::new(formset(items)).expect_err("mapping fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, 0);
    }

    #[test]
    fn mapping_leaves_action_questions_without_storage() {
        let set = formset(vec![
            Statement::Action {
                question: question(
                    9,
                    Storage::Name {
                        varstore: VarStoreId(7),
                        name: "run".to_string(),
                    },
                ),
            }
            .into(),
        ]);
        let prepared = PreparedFormSet::new(set).expect("mapping succeeds");
        assert_eq!(prepared.formset().varstores[0].size, 0);
        assert_eq!(storage_of(prepared.formset(), 9), Storage::None);
    }

    #[test]
    fn full_extract_answers_every_question() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![
            (QuestionId(1), Value::Bool(true)),
            (QuestionId(2), Value::Number(0x1234)),
            (QuestionId(3), Value::Text("abc".to_string())),
        ]);
        let answer = prepared
            .extract(&mut provider, 0, None, &PATH)
            .expect("extract succeeds");
        assert!(answer.starts_with(&prepared.config_header(&PATH)));
        let blocks = config::parse_blocks(&answer).expect("answer parses");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].offset, 0);
        assert_eq!(blocks[0].width, 1);
        assert_eq!(blocks[0].bytes, vec![1]);
        assert_eq!(blocks[1].offset, 1);
        assert_eq!(blocks[1].width, 2);
        assert_eq!(blocks[1].bytes, vec![0x34, 0x12]);
        assert_eq!(blocks[2].offset, 3);
        assert_eq!(blocks[2].width, 8);
        assert_eq!(blocks[2].bytes, vec![b'a', 0, b'b', 0, b'c', 0, 0, 0]);
    }

    #[test]
    fn extract_answers_only_the_requested_ranges() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![
            (QuestionId(1), Value::Bool(true)),
            (QuestionId(2), Value::Number(0x1234)),
            (QuestionId(3), Value::Text("abc".to_string())),
        ]);
        let header = prepared.config_header(&PATH);
        let request = config::request(&header, &[(1, 2), (3, 2)]);
        let answer = prepared
            .extract(&mut provider, 0, Some(&request), &PATH)
            .expect("extract succeeds");
        let blocks = config::parse_blocks(&answer).expect("answer parses");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].offset, 1);
        assert_eq!(blocks[0].bytes, vec![0x34, 0x12]);
        assert_eq!(blocks[1].offset, 3);
        assert_eq!(blocks[1].bytes, vec![b'a', 0]);
    }

    #[test]
    fn extract_accepts_a_body_without_a_header() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![(QuestionId(2), Value::Number(5))]);
        let answer = prepared
            .extract(&mut provider, 0, Some("&OFFSET=1&WIDTH=2"), &PATH)
            .expect("extract succeeds");
        let blocks = config::parse_blocks(&answer).expect("answer parses");
        assert_eq!(blocks[0].offset, 1);
        assert_eq!(blocks[0].bytes, vec![5, 0]);
    }

    #[test]
    fn extract_refuses_a_foreign_header_and_reports_its_position() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::default();
        let header = prepared.config_header(&PATH);
        let wrong = format!("GUID=X{}", &header[5..]);
        let request = config::request(&wrong, &[(0, 1)]);
        let error = prepared
            .extract(&mut provider, 0, Some(&request), &PATH)
            .expect_err("extract fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, 5);
    }

    #[test]
    fn extract_refuses_a_header_with_a_different_path() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::default();
        let header = prepared.config_header(&[0x55, 0xaa]);
        let request = config::request(&header, &[(0, 1)]);
        let error = prepared
            .extract(&mut provider, 0, Some(&request), &PATH)
            .expect_err("extract fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert!(error.progress > "GUID=".len());
    }

    #[test]
    fn malformed_ranges_report_the_element_position() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::default();
        let header = prepared.config_header(&PATH);
        let base = header.len();

        let missing = format!("{header}&OFFSET=0");
        let error = prepared
            .extract(&mut provider, 0, Some(&missing), &PATH)
            .expect_err("missing width fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, base);

        let bad_number = format!("{header}&OFFSET=zz&WIDTH=1");
        let error = prepared
            .extract(&mut provider, 0, Some(&bad_number), &PATH)
            .expect_err("bad number fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, base);

        let outside = format!("{header}&OFFSET=2&WIDTH=20");
        let error = prepared
            .extract(&mut provider, 0, Some(&outside), &PATH)
            .expect_err("outside range fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, base);

        let zero_width = format!("{header}&OFFSET=0&WIDTH=0");
        let error = prepared
            .extract(&mut provider, 0, Some(&zero_width), &PATH)
            .expect_err("zero width fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, base);
    }

    #[test]
    fn route_reports_the_value_position() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::default();
        let header = prepared.config_header(&PATH);
        let configuration = format!("{header}&OFFSET=0&WIDTH=2&VALUE=01");
        let value_start = header.len() + "&OFFSET=0&WIDTH=2".len();
        let error = prepared
            .route(&mut provider, 0, &configuration, &PATH)
            .expect_err("short value fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, value_start);

        let odd = format!("{header}&OFFSET=0&WIDTH=1&VALUE=0");
        let error = prepared
            .route(&mut provider, 0, &odd, &PATH)
            .expect_err("odd value fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, header.len() + "&OFFSET=0&WIDTH=1".len());
    }

    #[test]
    fn route_overlays_a_partial_range_and_writes_only_the_change() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![
            (QuestionId(1), Value::Bool(false)),
            (QuestionId(2), Value::Number(0x1234)),
            (QuestionId(3), Value::Text("ab".to_string())),
        ]);
        let header = prepared.config_header(&PATH);
        let configuration = format!("{header}&OFFSET=2&WIDTH=1&VALUE=ff");
        prepared
            .route(&mut provider, 0, &configuration, &PATH)
            .expect("route succeeds");
        assert_eq!(
            provider.writes,
            vec![(QuestionId(2), Value::Number(0xff34))]
        );
    }

    #[test]
    fn route_skips_values_that_do_not_change() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![(QuestionId(2), Value::Number(0x1234))]);
        let header = prepared.config_header(&PATH);
        let configuration = format!("{header}&OFFSET=1&WIDTH=2&VALUE=3412");
        prepared
            .route(&mut provider, 0, &configuration, &PATH)
            .expect("route succeeds");
        assert!(provider.writes.is_empty());
    }

    #[test]
    fn route_overlays_text_as_utf16() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![(QuestionId(3), Value::Text("ab".to_string()))]);
        let header = prepared.config_header(&PATH);
        let configuration = format!("{header}&OFFSET=3&WIDTH=2&VALUE=6500");
        prepared
            .route(&mut provider, 0, &configuration, &PATH)
            .expect("route succeeds");
        assert_eq!(
            provider.writes,
            vec![(QuestionId(3), Value::Text("eb".to_string()))]
        );
    }

    #[test]
    fn route_ignores_text_padding_changes() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![(QuestionId(3), Value::Text("ab".to_string()))]);
        let header = prepared.config_header(&PATH);
        let configuration = format!("{header}&OFFSET=0a&WIDTH=1&VALUE=ff");
        prepared
            .route(&mut provider, 0, &configuration, &PATH)
            .expect("route succeeds");
        assert!(provider.writes.is_empty());
    }

    #[test]
    fn strings_are_bounded_and_validated() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider =
            FakeProvider::with(vec![(QuestionId(3), Value::Text("abcde".to_string()))]);
        let error = prepared
            .extract(&mut provider, 0, None, &PATH)
            .expect_err("long text fails");
        assert_eq!(error.status, Status::DEVICE_ERROR);

        let mut provider = FakeProvider::with(vec![(QuestionId(3), Value::Text("😀".to_string()))]);
        let answer = prepared
            .extract(&mut provider, 0, None, &PATH)
            .expect("surrogate pair fits");
        let blocks = config::parse_blocks(&answer).expect("answer parses");
        assert_eq!(blocks[2].bytes[..4].to_vec(), vec![0x3d, 0xd8, 0x00, 0xde]);

        let header = prepared.config_header(&PATH);
        let unpaired = format!("{header}&OFFSET=3&WIDTH=4&VALUE=00d84100");
        let error = prepared
            .route(&mut provider, 0, &unpaired, &PATH)
            .expect_err("unpaired surrogate fails");
        assert_eq!(error.status, Status::INVALID_PARAMETER);
        assert_eq!(error.progress, header.len());
    }

    #[test]
    fn provider_failures_preserve_their_status() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider {
            read_failure: Some(Status::NOT_FOUND),
            ..FakeProvider::default()
        };
        let error = prepared
            .extract(&mut provider, 0, None, &PATH)
            .expect_err("read fails");
        assert_eq!(error.status, Status::NOT_FOUND);
        assert_eq!(error.progress, 0);

        let mut provider = FakeProvider {
            write_failure: Some(Status::ACCESS_DENIED),
            ..FakeProvider::with(vec![(QuestionId(2), Value::Number(0x1234))])
        };
        let header = prepared.config_header(&PATH);
        let configuration = format!("{header}&OFFSET=1&WIDTH=1&VALUE=ff");
        let error = prepared
            .route(&mut provider, 0, &configuration, &PATH)
            .expect_err("write fails");
        assert_eq!(error.status, Status::ACCESS_DENIED);
        assert_eq!(error.progress, header.len());
    }

    #[test]
    fn empty_body_requests_every_question() {
        let prepared = PreparedFormSet::new(sample()).expect("mapping succeeds");
        let mut provider = FakeProvider::with(vec![(QuestionId(2), Value::Number(7))]);
        let header = prepared.config_header(&PATH);
        let answer = prepared
            .extract(&mut provider, 0, Some(&header), &PATH)
            .expect("extract succeeds");
        let blocks = config::parse_blocks(&answer).expect("answer parses");
        assert_eq!(blocks.len(), 3);
    }
}
