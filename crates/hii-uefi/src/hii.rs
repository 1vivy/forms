//! A forms provider over the UEFI HII database.
//!
//! [`HiiProvider`] lists the formsets that the HII database holds, reads
//! question values through `EFI_HII_CONFIG_ROUTING_PROTOCOL`, writes changed
//! values back through the same protocol and runs callbacks on the driver's
//! `EFI_HII_CONFIG_ACCESS_PROTOCOL`.
//!
//! The module is self-contained: it needs the boot services, the string
//! conversion helper, the question shape helper and the forms model, and
//! nothing from the application loop. It therefore serves an application, a
//! driver and a test harness alike.
//!
//! # Protocol use
//!
//! | Task | Protocol |
//! | --- | --- |
//! | list and export package lists | `EFI_HII_DATABASE_PROTOCOL` |
//! | read and write stored values | `EFI_HII_CONFIG_ROUTING_PROTOCOL` |
//! | retrieve and change callback questions | `EFI_HII_CONFIG_ACCESS_PROTOCOL` |
//! | create and resolve string tokens | `EFI_HII_STRING_PROTOCOL` |

use alloc::{format, string::String, string::ToString, vec, vec::Vec};
use core::ptr::{self, NonNull};

use alloc::collections::BTreeMap;

use forms::{
    ActionRequest, Error, FormSet, Provider, Question, QuestionFlags, QuestionId, Scope, Statement,
    Storage, Value, VarStoreId, Width,
};
use hii_ifr::{config, decode, value_bytes, value_from};
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::hii::config::{
    BrowserAction, HiiConfigAccessProtocol, HiiConfigRoutingProtocol,
};
use uefi_raw::protocol::hii::database::HiiDatabaseProtocol;
use uefi_raw::protocol::hii::font::FontInfo;
use uefi_raw::protocol::hii::form_browser::BrowserActionRequest;
use uefi_raw::protocol::hii::ifr::IfrTypeValue;
use uefi_raw::protocol::hii::string::HiiStringProtocol;
use uefi_raw::protocol::hii::{HiiHandle, HiiPackageListHeader, StringId};
use uefi_raw::{Boolean, Char16, Handle, Status};

use crate::hex;
use crate::raw::{self, Boot};
use crate::shape;
use crate::text;
use hii_abi::value_type;

/// The language to use when a driver reports none.
const DEFAULT_LANGUAGE: &[u8] = b"eng\0";

/// The HII handle of one published package list.
///
/// Two package lists never share a handle, so the handle identifies a formset
/// group for the whole life of the HII database entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HiiHandleId(usize);

impl HiiHandleId {
    /// Wraps a raw HII handle.
    fn new(handle: HiiHandle) -> Self {
        Self(handle as usize)
    }

    /// Returns the raw HII handle.
    fn raw(self) -> HiiHandle {
        self.0 as HiiHandle
    }
}

/// The driver behind one package list.
struct Driver {
    /// The device path that the configuration header needs.
    device_path: Vec<u8>,
    /// The configuration access protocol of the driver, when it has one.
    access: Option<NonNull<HiiConfigAccessProtocol>>,
}

/// A forms provider backed by the HII database.
///
/// # Examples
///
/// ```no_run
/// # use forms::Provider;
/// # use oqcom_re::{hii::HiiProvider, raw::Boot};
/// # fn example(boot: Boot) -> Result<(), forms::Error> {
/// let mut provider = HiiProvider::new(boot)?;
/// for (handle, set) in provider.formsets()? {
///     let _ = (handle, set.title);
/// }
/// # Ok(())
/// # }
/// ```
pub struct HiiProvider {
    boot: Boot,
    database: NonNull<HiiDatabaseProtocol>,
    routing: NonNull<HiiConfigRoutingProtocol>,
    strings: Option<NonNull<HiiStringProtocol>>,
    drivers: BTreeMap<HiiHandleId, Driver>,
    languages: BTreeMap<HiiHandleId, Vec<u8>>,
    problems: Vec<String>,
    /// The value from before the first change of each question, keyed by
    /// handle and question.
    ///
    /// The browser writes every change at once, so this record is the only way
    /// that [`forms::Provider::discard`] can put a value back.
    originals: BTreeMap<(HiiHandleId, QuestionId), Value>,
}

impl HiiProvider {
    /// Opens the HII protocols on the firmware.
    ///
    /// # Errors
    ///
    /// Returns a failure when the firmware publishes no HII database or no
    /// configuration routing protocol. The provider cannot work without both.
    pub fn new(boot: Boot) -> Result<Self, Error> {
        let database = boot
            .locate::<HiiDatabaseProtocol>(&HiiDatabaseProtocol::GUID)
            .ok_or_else(|| Self::error(Status::NOT_FOUND, "no HII database protocol"))?;
        let routing = boot
            .locate::<HiiConfigRoutingProtocol>(&HiiConfigRoutingProtocol::GUID)
            .ok_or_else(|| {
                Self::error(Status::NOT_FOUND, "no HII configuration routing protocol")
            })?;
        let strings = boot.locate::<HiiStringProtocol>(&HiiStringProtocol::GUID);
        Ok(Self {
            boot,
            database,
            routing,
            strings,
            drivers: BTreeMap::new(),
            languages: BTreeMap::new(),
            problems: Vec::new(),
            originals: BTreeMap::new(),
        })
    }

    /// Takes the problems that the last `formsets` call skipped.
    ///
    /// A package list that this build cannot decode is skipped, so the rest of
    /// the pages stay usable. The caller logs the reasons.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use forms::Provider;
    /// # use oqcom_re::{hii::HiiProvider, raw::Boot};
    /// # fn example(boot: Boot) -> Result<(), forms::Error> {
    /// let mut provider = HiiProvider::new(boot)?;
    /// let _ = provider.formsets()?;
    /// for reason in provider.take_problems() {
    ///     let _ = reason;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn take_problems(&mut self) -> Vec<String> {
        core::mem::take(&mut self.problems)
    }

    /// Lists the HII handles that hold form packages.
    fn list_handles(&self) -> Result<Vec<HiiHandle>, Error> {
        let protocol = self.database();
        let mut length = 0usize;
        // SAFETY: The protocol belongs to the firmware and the length pointer is
        // writable.
        let status = unsafe {
            (protocol.list_package_lists)(
                self.database.as_ptr(),
                hii_abi::PACKAGE_FORMS,
                ptr::null(),
                &mut length,
                ptr::null_mut(),
            )
        };
        if status == Status::SUCCESS && length == 0 {
            return Ok(Vec::new());
        }
        if status.is_error() && status != Status::BUFFER_TOO_SMALL {
            return Err(Self::status_error(status, "cannot list HII package lists"));
        }
        let mut buffer = vec![0u8; length];
        let mut count = length;
        // SAFETY: The buffer holds `length` bytes, which is what the firmware
        // asked for.
        let status = unsafe {
            (protocol.list_package_lists)(
                self.database.as_ptr(),
                hii_abi::PACKAGE_FORMS,
                ptr::null(),
                &mut count,
                buffer.as_mut_ptr().cast::<HiiHandle>(),
            )
        };
        if status.is_error() {
            return Err(Self::status_error(status, "cannot list HII package lists"));
        }
        let handle_size = size_of::<HiiHandle>();
        if handle_size == 0 {
            return Ok(Vec::new());
        }
        let mut handles = Vec::with_capacity(count / handle_size);
        let mut offset = 0usize;
        while offset + handle_size <= count.min(buffer.len()) {
            // SAFETY: The bytes lie inside the buffer and a handle is plain
            // data.
            let handle = unsafe {
                buffer
                    .as_ptr()
                    .add(offset)
                    .cast::<HiiHandle>()
                    .read_unaligned()
            };
            handles.push(handle);
            offset += handle_size;
        }
        Ok(handles)
    }

    /// Exports the package list of one HII handle.
    fn export(&self, handle: HiiHandle) -> Result<Vec<u8>, Error> {
        let protocol = self.database();
        let mut size = 0usize;
        // SAFETY: The protocol belongs to the firmware and the size pointer is
        // writable.
        let status = unsafe {
            (protocol.export_package_lists)(
                self.database.as_ptr(),
                handle,
                &mut size,
                ptr::null_mut(),
            )
        };
        if status.is_error() && status != Status::BUFFER_TOO_SMALL {
            return Err(Self::status_error(
                status,
                "cannot size an HII package list",
            ));
        }
        if size == 0 {
            return Ok(Vec::new());
        }
        let mut buffer = vec![0u8; size];
        // SAFETY: The buffer holds `size` bytes, which is what the firmware asked
        // for. The package list header is byte aligned.
        let status = unsafe {
            (protocol.export_package_lists)(
                self.database.as_ptr(),
                handle,
                &mut size,
                buffer.as_mut_ptr().cast::<HiiPackageListHeader>(),
            )
        };
        if status.is_error() {
            return Err(Self::status_error(
                status,
                "cannot export an HII package list",
            ));
        }
        buffer.truncate(size.min(buffer.len()));
        Ok(buffer)
    }

    /// Records the driver and device path behind one package list.
    fn remember_driver(&mut self, handle: HiiHandle, id: HiiHandleId) -> Result<(), Error> {
        let protocol = self.database();
        let mut driver: Handle = ptr::null_mut();
        // SAFETY: The protocol belongs to the firmware and the handle pointer is
        // writable.
        let status = unsafe {
            (protocol.get_package_list_handle)(self.database.as_ptr(), handle, &mut driver)
        };
        let driver = if status.is_error() || driver.is_null() {
            handle
        } else {
            driver
        };
        let device_path = match unsafe {
            // SAFETY: `driver` is the HII handle or the driver handle that the
            // database just returned; both are live.
            self.boot
                .protocol::<DevicePathProtocol>(driver, &DevicePathProtocol::GUID)
        } {
            // SAFETY: The firmware owns the device path that it just returned.
            Some(path) => unsafe { raw::device_path_bytes(path.as_ptr()) },
            None => Vec::new(),
        };
        // SAFETY: `driver` is a live firmware handle.
        let access = unsafe {
            self.boot
                .protocol::<HiiConfigAccessProtocol>(driver, &HiiConfigAccessProtocol::GUID)
        };
        self.drivers.insert(
            id,
            Driver {
                device_path,
                access,
            },
        );
        Ok(())
    }

    /// Returns the driver record of one package list.
    fn driver(&self, handle: HiiHandleId) -> Result<&Driver, Error> {
        self.drivers
            .get(&handle)
            .ok_or_else(|| Self::error(Status::NOT_FOUND, "the formset is no longer published"))
    }

    /// Returns the device path of one package list.
    fn driver_path(&self, handle: HiiHandleId) -> Result<Vec<u8>, Error> {
        Ok(self.driver(handle)?.device_path.clone())
    }

    /// Returns the configuration access protocol of one package list.
    fn access(&self, handle: HiiHandleId) -> Result<NonNull<HiiConfigAccessProtocol>, Error> {
        self.driver(handle)?.access.ok_or_else(|| {
            Self::error(
                Status::UNSUPPORTED,
                "the driver of this formset has no configuration access protocol",
            )
        })
    }

    /// Returns the database protocol.
    fn database(&self) -> &HiiDatabaseProtocol {
        // SAFETY: The protocol lives as long as the firmware.
        unsafe { self.database.as_ref() }
    }

    /// Returns the routing protocol.
    fn routing(&self) -> &HiiConfigRoutingProtocol {
        // SAFETY: The protocol lives as long as the firmware.
        unsafe { self.routing.as_ref() }
    }

    /// Returns the string protocol.
    fn string_protocol(&self) -> Option<&HiiStringProtocol> {
        // SAFETY: The protocol lives as long as the firmware.
        self.strings.map(|strings| unsafe { strings.as_ref() })
    }

    /// Extracts a configuration answer for a request.
    fn extract_config(&self, request: &str) -> Result<String, Error> {
        let request_units = text::ucs2(request);
        let mut progress: *const Char16 = ptr::null();
        let mut results: *mut Char16 = ptr::null_mut();
        // SAFETY: Both strings are NUL terminated and the output pointers are
        // writable.
        let status = unsafe {
            (self.routing().extract_config)(
                self.routing.as_ptr(),
                request_units.as_ptr(),
                &mut progress,
                &mut results,
            )
        };
        let answer = if results.is_null() {
            None
        } else {
            // SAFETY: The firmware returns a NUL-terminated string that it
            // allocated.
            let answer = unsafe { text::from_ucs2(results) };
            // SAFETY: The string came from the firmware pool.
            unsafe { self.boot.free(NonNull::new_unchecked(results.cast::<u8>())) };
            Some(answer)
        };
        if status.is_error() {
            return Err(Self::status_error(
                status,
                "cannot extract the configuration",
            ));
        }
        answer.ok_or_else(|| Self::error(Status::NOT_FOUND, "the configuration answer was empty"))
    }

    /// Routes a configuration response to its variable store.
    fn route_config(&self, configuration: &str) -> Result<(), Error> {
        let configuration_units = text::ucs2(configuration);
        let mut progress: *const Char16 = ptr::null();
        // SAFETY: The string is NUL terminated and the progress pointer is
        // writable.
        let status = unsafe {
            (self.routing().route_config)(
                self.routing.as_ptr(),
                configuration_units.as_ptr(),
                &mut progress,
            )
        };
        if status.is_error() {
            return Err(Self::status_error(status, "cannot store the configuration"));
        }
        Ok(())
    }

    /// Returns the primary language of one package list, as an ASCII C string.
    fn language(&mut self, handle: HiiHandleId) -> Result<Vec<u8>, Error> {
        if let Some(language) = self.languages.get(&handle) {
            return Ok(language.clone());
        }
        let mut language = DEFAULT_LANGUAGE.to_vec();
        if let Some(protocol) = self.string_protocol() {
            let mut size = 0usize;
            // SAFETY: The protocol belongs to the firmware and the size pointer
            // is writable.
            let status = unsafe {
                (protocol.get_languages)(protocol, handle.raw(), ptr::null_mut(), &mut size)
            };
            if status == Status::BUFFER_TOO_SMALL && size > 0 {
                let mut buffer = vec![0u8; size];
                let mut length = size;
                // SAFETY: The buffer holds `size` bytes, which is what the
                // firmware asked for.
                let status = unsafe {
                    (protocol.get_languages)(
                        protocol,
                        handle.raw(),
                        buffer.as_mut_ptr(),
                        &mut length,
                    )
                };
                if !status.is_error() {
                    match buffer.iter().position(|byte| *byte == 0) {
                        Some(end) if end > 0 => {
                            buffer.truncate(end);
                            buffer.push(0);
                            language = buffer;
                        }
                        _ => {}
                    }
                }
            }
        }
        self.languages.insert(handle, language.clone());
        Ok(language)
    }

    /// Creates a string token that holds `content`.
    fn new_string(&mut self, handle: HiiHandleId, content: &str) -> Result<StringId, Error> {
        let language = self.language(handle)?;
        let protocol = self
            .string_protocol()
            .ok_or_else(|| Self::error(Status::UNSUPPORTED, "no HII string protocol"))?;
        let content_units = text::ucs2(content);
        let mut id: StringId = 0;
        // SAFETY: The language is a NUL-terminated ASCII string, the content is
        // NUL terminated, and the identifier pointer is writable.
        let status = unsafe {
            (protocol.new_string)(
                protocol,
                handle.raw(),
                &mut id,
                language.as_ptr(),
                ptr::null(),
                content_units.as_ptr(),
                ptr::null(),
            )
        };
        if status.is_error() {
            return Err(Self::status_error(status, "cannot create a string token"));
        }
        Ok(id)
    }

    /// Resolves a string token of one package list.
    fn string(&mut self, handle: HiiHandleId, id: StringId) -> Result<String, Error> {
        let language = self.language(handle)?;
        let protocol = self
            .string_protocol()
            .ok_or_else(|| Self::error(Status::UNSUPPORTED, "no HII string protocol"))?;
        let mut size = 128usize;
        loop {
            let mut buffer = vec![0u16; size];
            let mut length = size;
            let mut font: *mut FontInfo = ptr::null_mut();
            // SAFETY: The buffer holds `size` code units and the length pointer
            // is writable.
            let status = unsafe {
                (protocol.get_string)(
                    protocol,
                    language.as_ptr(),
                    handle.raw(),
                    id,
                    buffer.as_mut_ptr(),
                    &mut length,
                    &mut font,
                )
            };
            if status == Status::BUFFER_TOO_SMALL {
                if length <= size {
                    return Err(Self::status_error(status, "cannot size a string token"));
                }
                size = length;
                continue;
            }
            if status.is_error() {
                return Err(Self::status_error(status, "cannot read a string token"));
            }
            buffer.truncate(length.min(buffer.len()));
            return Ok(String::from_utf16_lossy(&buffer));
        }
    }

    /// Reads a question value through the driver callback.
    fn read_by_callback(
        &mut self,
        handle: HiiHandleId,
        question: &Question,
        statement: &Statement,
    ) -> Result<Value, Error> {
        let access = self.access(handle)?;
        let ifr_type = shape::ifr_type_of(statement).ok_or_else(|| {
            Self::error(
                Status::INVALID_PARAMETER,
                "the question cannot carry a value",
            )
        })?;
        let mut value = IfrTypeValue { u64: 0 };
        if ifr_type == value_type::STRING {
            let token = self.new_string(handle, "")?;
            value = IfrTypeValue { string: token };
        }
        let mut request = BrowserActionRequest::NONE;
        // SAFETY: The protocol belongs to the firmware and the value and request
        // pointers are writable.
        let status = unsafe {
            (access.as_ref().callback)(
                access.as_ptr(),
                BrowserAction::RETRIEVE,
                question.id.0,
                ifr_type,
                &mut value,
                &mut request,
            )
        };
        if status.is_error() {
            return Err(Self::status_error(
                status,
                "the driver cannot retrieve the value",
            ));
        }
        self.value_of(handle, statement, value)
    }

    /// Reads a stored value of a byte variable store.
    fn read_offset(
        &mut self,
        handle: HiiHandleId,
        set: &FormSet,
        statement: &Statement,
        varstore: VarStoreId,
        offset: u16,
    ) -> Result<Value, Error> {
        let store = set.varstore(varstore).ok_or_else(|| {
            Self::error(
                Status::NOT_FOUND,
                "the question names no known variable store",
            )
        })?;
        let width = shape::width_of(statement).ok_or_else(|| {
            Self::error(
                Status::INVALID_PARAMETER,
                "the question has no stored width",
            )
        })?;
        let header = config::header(&set.guid, &store.name, &self.driver_path(handle)?);
        let request = config::request(&header, &[(offset, width)]);
        let answer = self.extract_config(&request)?;
        let blocks = config::parse_blocks(&answer).map_err(|err| {
            Self::error(
                Status::DEVICE_ERROR,
                &format!("cannot parse the configuration answer: {err:?}"),
            )
        })?;
        let block = blocks
            .iter()
            .find(|block| block.offset == offset)
            .ok_or_else(|| {
                Self::error(Status::NOT_FOUND, "the configuration answer holds no value")
            })?;
        value_from(statement, &block.bytes).ok_or_else(|| {
            Self::error(
                Status::DEVICE_ERROR,
                "the stored value does not fit the question",
            )
        })
    }

    /// Reads a stored value of a name/value variable store.
    fn read_name(
        &mut self,
        handle: HiiHandleId,
        set: &FormSet,
        statement: &Statement,
        varstore: VarStoreId,
        name: &str,
    ) -> Result<Value, Error> {
        let _ = set.varstore(varstore).ok_or_else(|| {
            Self::error(
                Status::NOT_FOUND,
                "the question names no known variable store",
            )
        })?;
        let header = config::header(&set.guid, "", &self.driver_path(handle)?);
        let request = config::request_names(&header, &[name]);
        let answer = self.extract_config(&request)?;
        let pairs = config::parse_names(&answer).map_err(|err| {
            Self::error(
                Status::DEVICE_ERROR,
                &format!("cannot parse the configuration answer: {err:?}"),
            )
        })?;
        let (_, stored) = pairs.iter().find(|(pair, _)| pair == name).ok_or_else(|| {
            Self::error(Status::NOT_FOUND, "the configuration answer holds no value")
        })?;
        let bytes = hex::decode(stored).ok_or_else(|| {
            Self::error(Status::DEVICE_ERROR, "the stored value is not hexadecimal")
        })?;
        value_from(statement, &bytes).ok_or_else(|| {
            Self::error(
                Status::DEVICE_ERROR,
                "the stored value does not fit the question",
            )
        })
    }

    /// Converts a firmware value to a model value.
    fn value_of(
        &mut self,
        handle: HiiHandleId,
        statement: &Statement,
        value: IfrTypeValue,
    ) -> Result<Value, Error> {
        match shape::ifr_type_of(statement) {
            Some(value_type::BOOLEAN) => {
                // SAFETY: The driver set the member that matches the type.
                let flag = unsafe { value.b };
                Ok(Value::Bool(flag.0 != 0))
            }
            Some(value_type::STRING) => {
                // SAFETY: The driver set the member that matches the type.
                let id = unsafe { value.string };
                Ok(Value::Text(self.string(handle, id)?))
            }
            Some(_) => {
                let width = shape::width_of(statement).ok_or_else(|| {
                    Self::error(
                        Status::INVALID_PARAMETER,
                        "the question has no stored width",
                    )
                })?;
                let width = match width {
                    1 => Width::U8,
                    2 => Width::U16,
                    4 => Width::U32,
                    _ => Width::U64,
                };
                // SAFETY: The driver set the member that matches the type.
                let number = unsafe { number_of(value, width) };
                Ok(Value::Number(number))
            }
            None => Err(Self::error(
                Status::INVALID_PARAMETER,
                "the question cannot carry a value",
            )),
        }
    }

    /// Converts a model value to a firmware value.
    fn ffi_value(
        &mut self,
        handle: HiiHandleId,
        statement: &Statement,
        value: &Value,
    ) -> Result<IfrTypeValue, Error> {
        match (value, shape::ifr_type_of(statement)) {
            (Value::Bool(flag), Some(value_type::BOOLEAN)) => Ok(IfrTypeValue {
                b: Boolean(u8::from(*flag)),
            }),
            (Value::Number(number), Some(_)) => {
                let width = shape::width_of(statement).ok_or_else(|| {
                    Self::error(
                        Status::INVALID_PARAMETER,
                        "the question has no stored width",
                    )
                })?;
                let width = match width {
                    1 => Width::U8,
                    2 => Width::U16,
                    4 => Width::U32,
                    _ => Width::U64,
                };
                Ok(number_value(width, *number))
            }
            (Value::Text(content), Some(value_type::STRING)) => {
                let token = self.new_string(handle, content)?;
                Ok(IfrTypeValue { string: token })
            }
            _ => Err(Self::error(
                Status::INVALID_PARAMETER,
                "the value does not fit the question",
            )),
        }
    }

    /// Writes one firmware value to the stored configuration.
    ///
    /// The driver may replace the value during the `CHANGING` call, so this
    /// method converts the value that it receives.
    fn store_value(
        &mut self,
        handle: HiiHandleId,
        set: &FormSet,
        question: &Question,
        statement: &Statement,
        firmware_value: IfrTypeValue,
    ) -> Result<(), Error> {
        let stored = self.value_of(handle, statement, firmware_value)?;
        let bytes = value_bytes(statement, &stored).ok_or_else(|| {
            Self::error(
                Status::INVALID_PARAMETER,
                "the value does not fit the question",
            )
        })?;
        match &question.storage {
            Storage::None => {}
            Storage::Offset { varstore, offset } => {
                let store = set.varstore(*varstore).ok_or_else(|| {
                    Self::error(
                        Status::NOT_FOUND,
                        "the question names no known variable store",
                    )
                })?;
                let width = shape::width_of(statement).ok_or_else(|| {
                    Self::error(
                        Status::INVALID_PARAMETER,
                        "the question has no stored width",
                    )
                })?;
                let header = config::header(&set.guid, &store.name, &self.driver_path(handle)?);
                let response = config::response(
                    &header,
                    &[config::Block {
                        offset: *offset,
                        width,
                        bytes,
                    }],
                );
                self.route_config(&response)?;
            }
            Storage::Name { varstore, name } => {
                let _ = set.varstore(*varstore).ok_or_else(|| {
                    Self::error(
                        Status::NOT_FOUND,
                        "the question names no known variable store",
                    )
                })?;
                let mut response = config::header(&set.guid, "", &self.driver_path(handle)?);
                response.push('&');
                response.push_str(name);
                response.push('=');
                response.push_str(&hex::encode(&bytes));
                self.route_config(&response)?;
            }
        }
        Ok(())
    }

    /// Keeps the value from before the first change of a question.
    fn remember(&mut self, handle: HiiHandleId, set: &FormSet, question: &Question) {
        if matches!(question.storage, Storage::None) {
            return;
        }
        let key = (handle, question.id);
        if self.originals.contains_key(&key) {
            return;
        }
        if let Ok(value) = self.read(handle, set, question) {
            self.originals.insert(key, value);
        }
    }

    /// Forgets the values from before the first change of a scope.
    fn forget(&mut self, handle: HiiHandleId, set: &FormSet, scope: Scope) {
        for id in Self::questions_of(set, scope) {
            self.originals.remove(&(handle, id));
        }
    }

    /// Puts back the values from before the first change of a scope.
    fn restore(&mut self, handle: HiiHandleId, set: &FormSet, scope: Scope) -> Result<(), Error> {
        let mut restore = Vec::new();
        for id in Self::questions_of(set, scope) {
            if let Some(value) = self.originals.remove(&(handle, id)) {
                restore.push((id, value));
            }
        }
        for (id, value) in restore {
            let Some(statement) = shape::statement_of(set, id) else {
                continue;
            };
            let Some(question) = statement.question() else {
                continue;
            };
            let firmware_value = self.ffi_value(handle, statement, &value)?;
            self.store_value(handle, set, question, statement, firmware_value)?;
        }
        Ok(())
    }

    /// Returns the questions that a scope covers.
    fn questions_of(set: &FormSet, scope: Scope) -> Vec<QuestionId> {
        match scope {
            Scope::FormSet => set
                .forms
                .iter()
                .flat_map(|form| form.items.iter())
                .filter_map(|item| item.statement.question())
                .map(|question| question.id)
                .collect(),
            Scope::Form(form) => set.form(form).map_or_else(Vec::new, |form| {
                form.items
                    .iter()
                    .filter_map(|item| item.statement.question())
                    .map(|question| question.id)
                    .collect()
            }),
            Scope::Question(id) => vec![id],
        }
    }

    /// Builds a provider failure from a firmware status.
    fn status_error(status: Status, reason: &str) -> Error {
        Error {
            code: raw::status_code(status),
            reason: reason.to_string(),
        }
    }

    /// Builds a provider failure from a model inconsistency.
    fn error(status: Status, reason: &str) -> Error {
        Self::status_error(status, reason)
    }
}

impl Provider for HiiProvider {
    type Handle = HiiHandleId;

    fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error> {
        let handles = self.list_handles()?;
        self.drivers.clear();
        self.languages.clear();
        self.problems.clear();
        self.originals.clear();
        let mut sets = Vec::new();
        for handle in handles {
            let id = HiiHandleId::new(handle);
            self.remember_driver(handle, id)?;
            let package_list = self.export(handle)?;
            if package_list.is_empty() {
                continue;
            }
            match decode(&package_list) {
                Ok(decoded) => {
                    for set in decoded {
                        sets.push((id, set));
                    }
                }
                Err(error) => {
                    // One driver may publish opcodes that this build cannot
                    // read. Keep the pages that it can read.
                    self.problems.push(format!(
                        "cannot decode the package list at handle 0x{:x}: {error:?}",
                        id.0
                    ));
                }
            }
        }
        Ok(sets)
    }

    fn read(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        question: &Question,
    ) -> Result<Value, Error> {
        let statement = shape::statement_of(set, question.id).ok_or_else(|| {
            Self::error(
                Status::INVALID_PARAMETER,
                "the formset holds no such question",
            )
        })?;
        match &question.storage {
            Storage::None => self.read_by_callback(handle, question, statement),
            Storage::Offset { varstore, offset } => {
                self.read_offset(handle, set, statement, *varstore, *offset)
            }
            Storage::Name { varstore, name } => {
                self.read_name(handle, set, statement, *varstore, name)
            }
        }
    }

    fn write(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        question: &Question,
        value: &Value,
    ) -> Result<ActionRequest, Error> {
        let statement = shape::statement_of(set, question.id).ok_or_else(|| {
            Self::error(
                Status::INVALID_PARAMETER,
                "the formset holds no such question",
            )
        })?;
        let ifr_type = shape::ifr_type_of(statement).ok_or_else(|| {
            Self::error(
                Status::INVALID_PARAMETER,
                "the question cannot carry a value",
            )
        })?;
        let callback = question.flags.contains(QuestionFlags::CALLBACK);
        let access = self.driver(handle)?.access;
        if !callback && matches!(question.storage, Storage::None) {
            return Err(Self::error(
                Status::UNSUPPORTED,
                "the question stores no value and has no callback",
            ));
        }
        // Keep the value from before the first change of this question, so a
        // discard can put it back.
        self.remember(handle, set, question);
        let mut firmware_value = self.ffi_value(handle, statement, value)?;
        let mut request = BrowserActionRequest::NONE;
        if callback {
            let protocol = access.ok_or_else(|| {
                Self::error(
                    Status::UNSUPPORTED,
                    "the question needs a callback but its driver has none",
                )
            })?;
            // SAFETY: The protocol belongs to the firmware and the value and
            // request pointers are writable.
            let status = unsafe {
                (protocol.as_ref().callback)(
                    protocol.as_ptr(),
                    BrowserAction::CHANGING,
                    question.id.0,
                    ifr_type,
                    &mut firmware_value,
                    &mut request,
                )
            };
            if status.is_error() {
                return Err(Self::status_error(
                    status,
                    "the driver refused the new value",
                ));
            }
        }
        // The driver may replace the value during the call.
        self.store_value(handle, set, question, statement, firmware_value)?;
        if callback {
            let protocol = access.ok_or_else(|| {
                Self::error(
                    Status::UNSUPPORTED,
                    "the question needs a callback but its driver has none",
                )
            })?;
            // SAFETY: The protocol belongs to the firmware and the value and
            // request pointers are writable.
            let status = unsafe {
                (protocol.as_ref().callback)(
                    protocol.as_ptr(),
                    BrowserAction::CHANGED,
                    question.id.0,
                    ifr_type,
                    &mut firmware_value,
                    &mut request,
                )
            };
            if status.is_error() {
                return Err(Self::status_error(
                    status,
                    "the driver failed after the change",
                ));
            }
        }
        respond(request)
    }

    fn action(
        &mut self,
        handle: Self::Handle,
        _set: &FormSet,
        question: &Question,
    ) -> Result<ActionRequest, Error> {
        let access = self.access(handle)?;
        let mut value = IfrTypeValue { u64: 0 };
        let mut request = BrowserActionRequest::NONE;
        // SAFETY: The protocol belongs to the firmware and the value and request
        // pointers are writable.
        let status = unsafe {
            (access.as_ref().callback)(
                access.as_ptr(),
                BrowserAction::CHANGED,
                question.id.0,
                value_type::ACTION,
                &mut value,
                &mut request,
            )
        };
        if status.is_error() {
            return Err(Self::status_error(
                status,
                "the driver cannot run the action",
            ));
        }
        respond(request)
    }

    fn apply(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        self.forget(handle, set, scope);
        Ok(())
    }

    fn discard(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        self.restore(handle, set, scope)
    }
}

/// Reads a number from the firmware value union.
///
/// # Safety
///
/// The caller must ensure that the driver filled the member that `width`
/// names.
unsafe fn number_of(value: IfrTypeValue, width: Width) -> u64 {
    // SAFETY: The caller guarantees the active member.
    unsafe {
        match width {
            Width::U8 => u64::from(value.u8),
            Width::U16 => u64::from(value.u16),
            Width::U32 => u64::from(value.u32),
            Width::U64 => value.u64,
        }
    }
}

/// Writes a number into the firmware value union.
fn number_value(width: Width, number: u64) -> IfrTypeValue {
    let mut value = IfrTypeValue { u64: 0 };
    match width {
        Width::U8 => value.u8 = number as u8,
        Width::U16 => value.u16 = number as u16,
        Width::U32 => value.u32 = number as u32,
        Width::U64 => value.u64 = number,
    }
    value
}

/// Maps a driver action request to a session request.
///
/// The ten specification values keep their meaning. A driver that writes an
/// undefined value breaks the protocol, so it becomes an error and the browser
/// does not act on it.
fn respond(request: BrowserActionRequest) -> Result<ActionRequest, Error> {
    ActionRequest::from_raw(request.0 as u64).ok_or_else(|| Error {
        code: raw::status_code(Status::DEVICE_ERROR),
        reason: format!("the driver returned action request 0x{:x}", request.0),
    })
}
