//! A host-testable HII producer over a [`forms::Provider`].
//!
//! A UEFI driver publishes HII packages and answers configuration strings.
//! This module holds the part that needs no firmware. [`PreparedFormSet`]
//! builds the HII descriptor of one form set with [`hii_ifr::descriptor`], and
//! then builds the package list and serves the `<ConfigRequest>` and
//! `<ConfigResp>` strings of the configuration routing protocol through the
//! provider.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use forms::{FormSet, Key, Provider, Request, Row};
use hii_ifr::{
    EncodeError, Export, QuestionKind, Storage, config, descriptor, value_bytes, value_from,
};
use uefi_raw::Status;

/// The identifier of the buffer store that the producer publishes.
pub const VARSTORE_ID: u16 = 1;

/// The name of the buffer store that the producer publishes.
pub const VARSTORE_NAME: &str = "Forms";

/// A configuration failure with its firmware status and input position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError {
    /// The firmware status to return to the caller.
    pub status: Status,
    /// The byte offset in the input string, or 0 when none applies.
    pub progress: usize,
}

impl ConfigError {
    /// Builds a failure from a status and an input byte offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use hii_uefi::producer::ConfigError;
    /// use uefi_raw::Status;
    ///
    /// assert_eq!(ConfigError::new(Status::INVALID_PARAMETER, 4).progress, 4);
    /// ```
    pub const fn new(status: Status, progress: usize) -> Self {
        Self { status, progress }
    }

    /// Builds a failure from a provider error and an input byte offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use hii_uefi::producer::ConfigError;
    ///
    /// let error = forms::Error::new("refused");
    /// assert_eq!(ConfigError::from_provider(&error, 2).progress, 2);
    /// ```
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
pub struct Range {
    /// The row key of the question.
    pub row: Key,
    /// The value key that the row binds.
    pub binding: Key,
    /// The byte offset in the store.
    pub offset: u16,
    /// The block width in bytes.
    pub width: u16,
}

/// A form set with its HII identity and storage.
#[derive(Clone, Debug)]
pub struct PreparedFormSet {
    set: FormSet,
    export: Export,
}

impl PreparedFormSet {
    /// Builds the descriptor of a form set for one GUID.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] when a value row has no size.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// builder.row("Mode", Control::Toggle).set_binding(Key(10));
    /// let prepared = PreparedFormSet::new(builder.build(), [0x42; 16]).unwrap();
    /// assert_eq!(prepared.ranges().len(), 1);
    /// ```
    pub fn new(set: FormSet, guid: [u8; 16]) -> Result<Self, ConfigError> {
        let export = descriptor(&set, guid);
        Self::with_export(set, export)
    }

    /// Uses a descriptor that the caller built, so question flags such as
    /// `RESET_REQUIRED` and the caller's varstores reach the published IFR.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] when a value row has no size.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// builder.row("Mode", Control::Toggle).set_binding(Key(10));
    /// let set = builder.build();
    /// let mut export = hii_ifr::descriptor(&set, [0x42; 16]);
    /// export.questions[0].flags = 0x10;
    /// let prepared = PreparedFormSet::with_export(set, export).unwrap();
    /// assert_eq!(prepared.export().questions[0].flags, 0x10);
    /// ```
    pub fn with_export(set: FormSet, export: Export) -> Result<Self, ConfigError> {
        for question in &export.questions {
            if matches!(question.kind, QuestionKind::Action) {
                continue;
            }
            if question.bytes == 0 {
                return Err(ConfigError::new(Status::INVALID_PARAMETER, 0));
            }
        }
        Ok(Self { set, export })
    }

    /// Returns the mapped form set.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Key};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let prepared = PreparedFormSet::new(Builder::new(Key(1), "P").build(), [0; 16]).unwrap();
    /// assert_eq!(prepared.formset().title, "P");
    /// ```
    pub fn formset(&self) -> &FormSet {
        &self.set
    }

    /// Returns the HII descriptor of the set.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Key};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let prepared = PreparedFormSet::new(Builder::new(Key(1), "P").build(), [0x11; 16]).unwrap();
    /// assert_eq!(prepared.export().guid, [0x11; 16]);
    /// ```
    pub fn export(&self) -> &Export {
        &self.export
    }

    /// Returns every question range in publication order.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// builder.row("Mode", Control::Toggle).set_binding(Key(10));
    /// let prepared = PreparedFormSet::new(builder.build(), [0; 16]).unwrap();
    /// assert_eq!(prepared.ranges()[0].offset, 0);
    /// ```
    pub fn ranges(&self) -> Vec<Range> {
        self.export
            .questions
            .iter()
            .filter_map(|question| match question.storage {
                Storage::Offset { offset, .. } => Some(Range {
                    row: question.row,
                    binding: question.binding,
                    offset,
                    width: question.bytes,
                }),
                _ => None,
            })
            .collect()
    }

    /// Encodes the mapped form set as an HII package list.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] when the set cannot be encoded.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Key};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let prepared = PreparedFormSet::new(Builder::new(Key(1), "P").build(), [0; 16]).unwrap();
    /// assert!(!prepared.package_list().unwrap().is_empty());
    /// ```
    pub fn package_list(&self) -> Result<Vec<u8>, ConfigError> {
        Ok(hii_ifr::encode(&self.set, &self.export, self.export.guid)?)
    }

    /// Builds the `<ConfigHdr>` of the buffer store for one device path.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Key};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let prepared = PreparedFormSet::new(Builder::new(Key(1), "P").build(), [0; 16]).unwrap();
    /// assert!(prepared.config_header(&[]).starts_with("GUID="));
    /// ```
    pub fn config_header(&self, path: &[u8]) -> String {
        config::header(&self.export.guid, VARSTORE_NAME, path)
    }

    /// Answers a `<ConfigRequest>` from the provider.
    ///
    /// `request` is `None` or empty for every question.
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] for a malformed input and the
    /// provider status when a read fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key, MemoryProvider};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// builder.row("Mode", Control::Toggle).set_binding(Key(10));
    /// let set = builder.build();
    /// let prepared = PreparedFormSet::new(set.clone(), [0; 16]).unwrap();
    /// let mut provider = MemoryProvider::new(vec![(0u8, set)]);
    /// provider.set(Key(10), forms::Value::Bool(true));
    /// assert!(prepared.extract(&mut provider, 0, None, &[]).unwrap().contains("VALUE=01"));
    /// ```
    pub fn extract<P: Provider>(
        &self,
        provider: &mut P,
        handle: P::Handle,
        request: Option<&str>,
        path: &[u8],
    ) -> Result<String, ConfigError> {
        let header = self.config_header(path);
        let ranges = match request {
            None => self.ranges(),
            Some(text) => {
                let body = body(text, &header)?;
                let elements = elements(body)?;
                if elements.is_empty() {
                    self.ranges()
                } else {
                    self.requested(&elements)?
                }
            }
        };
        let mut blocks = Vec::with_capacity(ranges.len());
        for range in ranges {
            let row = self.row(range.row)?;
            let value = provider
                .read(handle, &self.set, row)
                .map_err(|error| ConfigError::from_provider(&error, 0))?;
            let bytes = value_bytes(row, &value)
                .ok_or_else(|| ConfigError::new(Status::DEVICE_ERROR, 0))?;
            blocks.push(config::Block {
                offset: range.offset,
                width: u16::try_from(bytes.len())
                    .map_err(|_| ConfigError::new(Status::DEVICE_ERROR, 0))?,
                bytes,
            });
        }
        Ok(config::response(&header, &blocks))
    }

    /// Routes a `<ConfigResp>` to the provider.
    ///
    /// Each block overlays the current value of the question that covers it.
    /// Only questions whose value changes go to [`forms::Provider::write`].
    ///
    /// # Errors
    ///
    /// Returns [`Status::INVALID_PARAMETER`] for a malformed input and the
    /// provider status when a write fails. The answer carries the request of
    /// the last changed question.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Builder, Control, Key, MemoryProvider, Request, Store, Value};
    /// use hii_uefi::producer::PreparedFormSet;
    ///
    /// let mut builder = Builder::new(Key(1), "P");
    /// builder.page("Main");
    /// builder.row("Mode", Control::Toggle).set_binding(Key(10));
    /// let set = builder.build();
    /// let prepared = PreparedFormSet::new(set.clone(), [0; 16]).unwrap();
    /// let mut provider = MemoryProvider::new(vec![(0u8, set)]);
    /// provider.set(Key(10), Value::Bool(false));
    /// let header = prepared.config_header(&[]);
    /// let input = format!("{header}&OFFSET=00&WIDTH=01&VALUE=01");
    /// assert_eq!(prepared.route(&mut provider, 0, &input, &[]), Ok(Request::None));
    /// assert_eq!(provider.store.read(Key(10)), Ok(Value::Bool(true)));
    /// ```
    pub fn route<P: Provider>(
        &self,
        provider: &mut P,
        handle: P::Handle,
        configuration: &str,
        path: &[u8],
    ) -> Result<Request, ConfigError> {
        let header = self.config_header(path);
        let body = body(configuration, &header)?;
        let elements = elements(body)?;
        let blocks = self.blocks(&elements)?;
        let mut request = Request::None;
        let mut cursor = 0usize;
        for range in self.ranges() {
            let row = self.row(range.row)?;
            let mut current = value_bytes(
                row,
                &provider
                    .read(handle, &self.set, row)
                    .map_err(|error| ConfigError::from_provider(&error, 0))?,
            )
            .ok_or_else(|| ConfigError::new(Status::DEVICE_ERROR, 0))?;
            let mut changed = false;
            for block in &blocks {
                let start = usize::from(block.offset);
                let end = start + usize::from(block.width);
                if start >= usize::from(range.offset)
                    && end <= usize::from(range.offset) + usize::from(range.width)
                {
                    let local = start - usize::from(range.offset);
                    current[local..local + block.width as usize].copy_from_slice(&block.bytes);
                    changed = true;
                }
            }
            if !changed {
                continue;
            }
            let Some(value) = value_from(row, &current) else {
                return Err(ConfigError::new(Status::INVALID_PARAMETER, cursor));
            };
            request = provider
                .write(handle, &self.set, row, &value)
                .map_err(|error| ConfigError::from_provider(&error, cursor))?;
            cursor += usize::from(range.width);
        }
        Ok(request)
    }

    /// Returns the row of a question, by its row key.
    fn row(&self, key: Key) -> Result<&Row, ConfigError> {
        self.set
            .row(key)
            .ok_or_else(|| ConfigError::new(Status::NOT_FOUND, 0))
    }

    /// Parses the requested ranges of a request.
    fn requested(&self, elements: &[Element]) -> Result<Vec<Range>, ConfigError> {
        let mut ranges = Vec::new();
        let mut offset = None;
        for element in elements {
            match element.key.as_str() {
                "OFFSET" => offset = Some(element.number()?),
                "WIDTH" => {
                    let width = element.number()?;
                    let offset = offset
                        .take()
                        .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, element.at))?;
                    let range = self
                        .ranges()
                        .into_iter()
                        .find(|range| {
                            u32::from(range.offset) == offset && u32::from(range.width) == width
                        })
                        .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, element.at))?;
                    ranges.push(range);
                }
                _ => {}
            }
        }
        if ranges.is_empty() {
            return Err(ConfigError::new(Status::INVALID_PARAMETER, 0));
        }
        Ok(ranges)
    }

    /// Parses the response blocks.
    fn blocks(&self, elements: &[Element]) -> Result<Vec<config::Block>, ConfigError> {
        let mut blocks = Vec::new();
        let mut offset = None;
        let mut width = None;
        for element in elements {
            match element.key.as_str() {
                "OFFSET" => offset = Some(element.number()?),
                "WIDTH" => width = Some(element.number()?),
                "VALUE" => {
                    let bytes = decode_hex(&element.value)
                        .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, element.at))?;
                    let offset = offset
                        .take()
                        .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, element.at))?;
                    let width = width
                        .take()
                        .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, element.at))?;
                    let offset = u16::try_from(offset)
                        .map_err(|_| ConfigError::new(Status::INVALID_PARAMETER, element.at))?;
                    let width = u16::try_from(width)
                        .map_err(|_| ConfigError::new(Status::INVALID_PARAMETER, element.at))?;
                    if usize::from(width) != bytes.len() {
                        return Err(ConfigError::new(Status::INVALID_PARAMETER, element.at));
                    }
                    blocks.push(config::Block {
                        offset,
                        width,
                        bytes,
                    });
                }
                _ => {}
            }
        }
        Ok(blocks)
    }
}

/// One `KEY=VALUE` element of a configuration string.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Element {
    key: String,
    value: String,
    at: usize,
}

impl Element {
    /// Reads the value as a hexadecimal number.
    fn number(&self) -> Result<u32, ConfigError> {
        u32::from_str_radix(&self.value, 16)
            .map_err(|_| ConfigError::new(Status::INVALID_PARAMETER, self.at))
    }
}

/// Returns the body of an input string after its header.
fn body<'a>(input: &'a str, header: &str) -> Result<&'a str, ConfigError> {
    if !input.starts_with("GUID=") {
        if input.is_empty() || input.starts_with('&') {
            return Ok(input.trim_start_matches('&'));
        }
        return Err(ConfigError::new(Status::INVALID_PARAMETER, 0));
    }
    if !input
        .get(..header.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(header))
    {
        return Err(ConfigError::new(Status::INVALID_PARAMETER, 0));
    }
    let rest = &input[header.len()..];
    Ok(rest.trim_start_matches('&'))
}

/// Splits a body into its elements.
fn elements(body: &str) -> Result<Vec<Element>, ConfigError> {
    let mut elements = Vec::new();
    let mut at = 0usize;
    for part in body.split('&').filter(|part| !part.is_empty()) {
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| ConfigError::new(Status::INVALID_PARAMETER, at))?;
        elements.push(Element {
            key: String::from(key),
            value: String::from(value),
            at,
        });
        at += part.len() + 1;
    }
    Ok(elements)
}

/// Decodes a hexadecimal byte string.
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        out.push((high * 16 + low) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::PreparedFormSet;
    use alloc::format;
    use alloc::vec;
    use forms::{Builder, Control, Key, MemoryProvider, Value};

    /// A binding that equals another row key must not move a value.
    #[test]
    fn a_binding_that_equals_another_row_key_does_not_collide() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        for index in 0..12u64 {
            let binding = if index == 7 {
                // Row key 7 belongs to row 5, which comes first.
                Key(7)
            } else {
                Key(100 + index)
            };
            let row = builder.row(
                format!("Row {index}"),
                Control::Number {
                    minimum: 0,
                    maximum: 255,
                    step: 1,
                },
            );
            row.set_binding(binding);
        }
        let set = builder.build();
        let prepared = PreparedFormSet::new(set.clone(), [0u8; 16]).expect("prepared");
        let mut provider = MemoryProvider::new(vec![(0u8, set)]);
        for index in 0..12u64 {
            let binding = if index == 7 { Key(7) } else { Key(100 + index) };
            let value = if index == 7 { 5 } else { index + 1 };
            provider.set(binding, Value::Number(value));
        }
        provider.set(Key(105), Value::Number(9));

        let answer = prepared
            .extract(&mut provider, 0, None, &[])
            .expect("the answer is built");
        let blocks = hii_ifr::config::parse_blocks(&answer).expect("the blocks parse");
        let range = prepared
            .ranges()
            .into_iter()
            .find(|range| range.binding == Key(7))
            .expect("the row that binds key 7");
        let block = blocks
            .iter()
            .find(|block| block.offset == range.offset)
            .expect("the block of that row");
        assert_eq!(block.bytes, vec![5]);
    }
}
