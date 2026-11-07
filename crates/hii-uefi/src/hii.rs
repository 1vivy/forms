//! A forms provider over the UEFI HII database.
//!
//! [`HiiProvider`] lists the form sets that the HII database holds, reads
//! values through `EFI_HII_CONFIG_ROUTING_PROTOCOL`, writes changed values
//! through the same protocol and runs callbacks on the driver's
//! `EFI_HII_CONFIG_ACCESS_PROTOCOL`.

use alloc::collections::BTreeMap;
use alloc::{format, string::String, string::ToString, vec, vec::Vec};
use core::ptr::{self, NonNull};

use forms::{Control, Error, FormSet, Key, Provider, Request, Row, Scope, Value};
use hii_ifr::{Export, QuestionKind, Storage, config, decode, value_bytes, value_from};
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::hii::config::{
    BrowserAction, HiiConfigAccessProtocol, HiiConfigRoutingProtocol,
};
use uefi_raw::protocol::hii::database::HiiDatabaseProtocol;
use uefi_raw::protocol::hii::form_browser::BrowserActionRequest;
use uefi_raw::protocol::hii::ifr::IfrTypeValue;
use uefi_raw::protocol::hii::string::HiiStringProtocol;
use uefi_raw::protocol::hii::{HiiHandle, HiiPackageListHeader, StringId};
use uefi_raw::{Boolean, Char16, Handle, Status};

use crate::raw::{self, Boot};
use crate::text;
use hii_abi::value_type;

/// The language to use when a driver reports none.
const DEFAULT_LANGUAGE: &[u8] = b"eng\0";

/// The HII handle of one published package list.
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
    /// The driver handle that the controller services use.
    handle: Handle,
    /// The device path that the configuration header needs.
    device_path: Vec<u8>,
    /// The configuration access protocol of the driver, when it has one.
    access: Option<NonNull<HiiConfigAccessProtocol>>,
}

/// One decoded form set with its HII identity.
struct Entry {
    set: FormSet,
    export: Export,
}

/// A forms provider backed by the HII database.
///
/// # Examples
///
/// ```no_run
/// # use forms::Provider;
/// # use hii_uefi::{hii::HiiProvider, raw::Boot};
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
    entries: BTreeMap<HiiHandleId, Entry>,
    problems: Vec<String>,
    /// The value from before the first change of each row.
    originals: BTreeMap<(HiiHandleId, Key), Value>,
}

impl HiiProvider {
    /// Opens the HII protocols on the firmware.
    ///
    /// # Errors
    ///
    /// Returns a failure when the firmware publishes no HII database or no
    /// configuration routing protocol.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hii_uefi::{hii::HiiProvider, raw::Boot};
    /// # fn example(boot: Boot) { let _ = HiiProvider::new(boot); }
    /// ```
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
            entries: BTreeMap::new(),
            problems: Vec::new(),
            originals: BTreeMap::new(),
        })
    }

    /// Takes the problems that the last `formsets` call skipped.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use forms::Provider;
    /// # use hii_uefi::{hii::HiiProvider, raw::Boot};
    /// # fn example(boot: Boot) -> Result<(), forms::Error> {
    /// let mut provider = HiiProvider::new(boot)?;
    /// let _ = provider.formsets()?;
    /// assert!(provider.take_problems().is_empty() || true);
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
        // SAFETY: The protocol belongs to the firmware and the length pointer
        // is writable.
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
        // SAFETY: The buffer holds `length` bytes.
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
            // SAFETY: The bytes lie inside the buffer.
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
        // SAFETY: The protocol belongs to the firmware.
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
        // SAFETY: The buffer holds `size` bytes.
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
    fn remember_driver(&mut self, handle: HiiHandle, id: HiiHandleId) {
        let protocol = self.database();
        let mut driver: Handle = ptr::null_mut();
        // SAFETY: The protocol belongs to the firmware.
        let status = unsafe {
            (protocol.get_package_list_handle)(self.database.as_ptr(), handle, &mut driver)
        };
        let driver = if status.is_error() || driver.is_null() {
            handle
        } else {
            driver
        };
        let device_path = match unsafe {
            // SAFETY: `driver` is a live firmware handle.
            self.boot
                .protocol::<DevicePathProtocol>(driver, &DevicePathProtocol::GUID)
        } {
            // SAFETY: The firmware owns the device path.
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
                handle: driver,
                device_path,
                access,
            },
        );
    }

    /// Returns the device path of one package list.
    fn driver_path(&self, handle: HiiHandleId) -> Result<Vec<u8>, Error> {
        self.drivers
            .get(&handle)
            .map(|driver| driver.device_path.clone())
            .ok_or_else(|| Self::error(Status::NOT_FOUND, "the set is no longer published"))
    }

    /// Returns the configuration access protocol of one package list.
    fn access(&self, handle: HiiHandleId) -> Result<NonNull<HiiConfigAccessProtocol>, Error> {
        self.drivers
            .get(&handle)
            .and_then(|driver| driver.access)
            .ok_or_else(|| {
                Self::error(
                    Status::UNSUPPORTED,
                    "the driver of this set has no configuration access protocol",
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
            // SAFETY: The firmware returns a NUL-terminated string.
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
        // SAFETY: The string is NUL terminated.
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

    /// Returns the entry of one handle.
    fn entry(&self, handle: HiiHandleId) -> Result<&Entry, Error> {
        self.entries
            .get(&handle)
            .ok_or_else(|| Self::error(Status::NOT_FOUND, "the set is not loaded"))
    }

    /// Returns the question of one row.
    fn question(&self, handle: HiiHandleId, row: &Row) -> Result<hii_ifr::QuestionRef, Error> {
        let entry = self.entry(handle)?;
        entry
            .export
            .question_of_row(row.key)
            .cloned()
            .ok_or_else(|| Self::error(Status::NOT_FOUND, "the set declares no such question"))
    }

    /// Reads a stored value through the routing protocol.
    fn read_stored(
        &mut self,
        handle: HiiHandleId,
        row: &Row,
        store: u16,
        offset: u16,
        width: u16,
    ) -> Result<Value, Error> {
        let entry = self.entry(handle)?;
        let name = entry
            .export
            .varstores
            .iter()
            .find(|store_ref| store_ref.id == store)
            .map(|store_ref| store_ref.name.clone())
            .unwrap_or_default();
        let header = config::header(&entry.export.guid, &name, &self.driver_path(handle)?);
        let request = config::request(&header, &[(offset, width)]);
        let answer = self.extract_config(&request)?;
        let blocks = config::parse_blocks(&answer).map_err(|_| {
            Self::error(
                Status::DEVICE_ERROR,
                "cannot parse the configuration answer",
            )
        })?;
        let block = blocks
            .iter()
            .find(|block| block.offset == offset)
            .ok_or_else(|| Self::error(Status::NOT_FOUND, "the answer holds no value"))?;
        value_from(row, &block.bytes)
            .ok_or_else(|| Self::error(Status::DEVICE_ERROR, "the stored value does not fit"))
    }

    /// Writes a stored value through the routing protocol.
    fn write_stored(
        &mut self,
        handle: HiiHandleId,
        row: &Row,
        store: u16,
        offset: u16,
        value: &Value,
    ) -> Result<(), Error> {
        let entry = self.entry(handle)?;
        let name = entry
            .export
            .varstores
            .iter()
            .find(|store_ref| store_ref.id == store)
            .map(|store_ref| store_ref.name.clone())
            .unwrap_or_default();
        let bytes = value_bytes(row, value)
            .ok_or_else(|| Self::error(Status::INVALID_PARAMETER, "the value does not fit"))?;
        let width = u16::try_from(bytes.len())
            .map_err(|_| Self::error(Status::INVALID_PARAMETER, "the value is too wide"))?;
        let header = config::header(&entry.export.guid, &name, &self.driver_path(handle)?);
        let response = config::response(
            &header,
            &[config::Block {
                offset,
                width,
                bytes,
            }],
        );
        self.route_config(&response)
    }

    /// Converts a value to the firmware value of a question kind.
    fn ffi_value(
        &mut self,
        handle: HiiHandleId,
        kind: QuestionKind,
        value: &Value,
    ) -> Result<IfrTypeValue, Error> {
        let mut out = IfrTypeValue { u64: 0 };
        match (kind, value) {
            (QuestionKind::Bool, Value::Bool(flag)) => {
                out.b = Boolean(u8::from(*flag));
            }
            (QuestionKind::Number(width), Value::Number(number)) => match width {
                hii_ifr::Width::U8 => out.u8 = *number as u8,
                hii_ifr::Width::U16 => out.u16 = *number as u16,
                hii_ifr::Width::U32 => out.u32 = *number as u32,
                hii_ifr::Width::U64 => out.u64 = *number,
            },
            (QuestionKind::Text { .. } | QuestionKind::Password { .. }, Value::Text(text)) => {
                out.string = self.new_string(handle, text)?;
            }
            (QuestionKind::Action, _) => {}
            _ => {
                return Err(Self::error(
                    Status::INVALID_PARAMETER,
                    "the value does not fit the question",
                ));
            }
        }
        Ok(out)
    }

    /// Creates a string token in the package list of a handle.
    fn new_string(&mut self, handle: HiiHandleId, content: &str) -> Result<StringId, Error> {
        let language = self.language(handle)?;
        let protocol = self.strings.ok_or_else(|| {
            Self::error(
                Status::UNSUPPORTED,
                "the firmware has no HII string protocol",
            )
        })?;
        let units = text::ucs2(content);
        let mut token: StringId = 0;
        // SAFETY: The language and the string are NUL terminated and the
        // token pointer is writable.
        let status = unsafe {
            (protocol.as_ref().new_string)(
                protocol.as_ptr().cast_const(),
                handle.raw(),
                &mut token,
                language.as_ptr(),
                ptr::null(),
                units.as_ptr(),
                ptr::null(),
            )
        };
        if status.is_error() {
            return Err(Self::status_error(status, "cannot create a string"));
        }
        Ok(token)
    }

    /// Returns the primary language of one package list.
    fn language(&mut self, handle: HiiHandleId) -> Result<Vec<u8>, Error> {
        if let Some(language) = self.languages.get(&handle) {
            return Ok(language.clone());
        }
        let mut language = DEFAULT_LANGUAGE.to_vec();
        if let Some(protocol) = self.string_protocol() {
            let mut size = 0usize;
            // SAFETY: The size pointer is writable.
            let status = unsafe {
                (protocol.get_languages)(protocol, handle.raw(), ptr::null_mut(), &mut size)
            };
            if status == Status::BUFFER_TOO_SMALL && size > 0 {
                let mut buffer = vec![0u8; size];
                let mut length = size;
                // SAFETY: The buffer holds `size` bytes.
                let status = unsafe {
                    (protocol.get_languages)(
                        protocol,
                        handle.raw(),
                        buffer.as_mut_ptr(),
                        &mut length,
                    )
                };
                if !status.is_error() {
                    let end = buffer
                        .iter()
                        .position(|byte| *byte == 0)
                        .unwrap_or(buffer.len());
                    if end > 0 {
                        language = buffer[..=end].to_vec();
                    }
                }
            }
        }
        self.languages.insert(handle, language.clone());
        Ok(language)
    }

    /// Returns the string protocol.
    fn string_protocol(&self) -> Option<&HiiStringProtocol> {
        // SAFETY: The protocol lives as long as the firmware.
        self.strings.map(|strings| unsafe { strings.as_ref() })
    }

    /// Returns the questions that a scope covers.
    fn rows_of(set: &FormSet, scope: Scope) -> Vec<Row> {
        match scope {
            Scope::Set => set
                .pages
                .iter()
                .flat_map(|page| page.rows.iter().cloned())
                .filter(|row| row.binding.is_some() || matches!(row.control, Control::Action))
                .collect(),
            Scope::Page(key) => set.page(key).map_or_else(Vec::new, |page| {
                page.rows
                    .iter()
                    .filter(|row| row.binding.is_some() || matches!(row.control, Control::Action))
                    .cloned()
                    .collect()
            }),
            Scope::Row(key) => set.row(key).cloned().into_iter().collect(),
        }
    }

    /// Keeps the value from before the first change of a row.
    fn remember(&mut self, handle: HiiHandleId, set: &FormSet, row: &Row) {
        let key = row.binding.unwrap_or(row.key);
        if self.originals.contains_key(&(handle, key)) {
            return;
        }
        if let Ok(value) = self.read(handle, set, row) {
            self.originals.insert((handle, key), value);
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
        self.entries.clear();
        self.problems.clear();
        self.originals.clear();
        let mut sets = Vec::new();
        for handle in handles {
            let id = HiiHandleId::new(handle);
            self.remember_driver(handle, id);
            let package_list = self.export(handle)?;
            if package_list.is_empty() {
                continue;
            }
            match decode(&package_list) {
                Ok(decoded) => {
                    for decoded in decoded {
                        sets.push((id, decoded.set.clone()));
                        self.entries.insert(
                            id,
                            Entry {
                                set: decoded.set,
                                export: decoded.export,
                            },
                        );
                    }
                }
                Err(error) => {
                    self.problems.push(format!(
                        "cannot decode the package list at handle 0x{:x}: {error:?}",
                        id.0
                    ));
                }
            }
        }
        Ok(sets)
    }

    fn read(&mut self, handle: Self::Handle, set: &FormSet, row: &Row) -> Result<Value, Error> {
        let _ = set;
        let question = self.question(handle, row)?;
        match question.storage {
            Storage::Offset { store, offset } => {
                self.read_stored(handle, row, store, offset, question.bytes)
            }
            Storage::Name { store: _, name } => {
                let entry = self.entry(handle)?;
                let guid = entry.export.guid;
                let header = config::header(&guid, "", &self.driver_path(handle)?);
                let request = config::request_names(&header, &[name.as_str()]);
                let answer = self.extract_config(&request)?;
                let pairs = config::parse_names(&answer)
                    .map_err(|_| Self::error(Status::DEVICE_ERROR, "cannot parse the answer"))?;
                let (_, stored) = pairs
                    .iter()
                    .find(|(pair, _)| pair == &name)
                    .ok_or_else(|| Self::error(Status::NOT_FOUND, "the answer holds no value"))?;
                let bytes = crate::hex::decode(stored).ok_or_else(|| {
                    Self::error(Status::DEVICE_ERROR, "the value is not hexadecimal")
                })?;
                value_from(row, &bytes)
                    .ok_or_else(|| Self::error(Status::DEVICE_ERROR, "the value does not fit"))
            }
            Storage::None => Err(Self::error(
                Status::UNSUPPORTED,
                "the question stores no value",
            )),
        }
    }

    fn write(
        &mut self,
        handle: Self::Handle,
        set: &FormSet,
        row: &Row,
        value: &Value,
    ) -> Result<Request, Error> {
        let question = self.question(handle, row)?;
        self.remember(handle, set, row);
        match question.storage {
            Storage::Offset { store, offset } => {
                self.write_stored(handle, row, store, offset, value)?;
                Ok(crate::map::request_for(question.flags, Request::None))
            }
            Storage::Name { store, name } => {
                let entry = self.entry(handle)?;
                let guid = entry.export.guid;
                let _ = store;
                let bytes = value_bytes(row, value).ok_or_else(|| {
                    Self::error(Status::INVALID_PARAMETER, "the value does not fit")
                })?;
                let mut response = config::header(&guid, "", &self.driver_path(handle)?);
                response.push('&');
                response.push_str(&name);
                response.push('=');
                response.push_str(&crate::hex::encode(&bytes));
                self.route_config(&response)?;
                Ok(crate::map::request_for(question.flags, Request::None))
            }
            Storage::None => {
                let access = self.access(handle)?;
                let mut firmware = self.ffi_value(handle, question.kind, value)?;
                let mut request = BrowserActionRequest::NONE;
                // SAFETY: The protocol belongs to the firmware.
                let status = unsafe {
                    (access.as_ref().callback)(
                        access.as_ptr(),
                        BrowserAction::CHANGING,
                        question.question,
                        crate::map::ifr_type(question.kind),
                        &mut firmware,
                        &mut request,
                    )
                };
                if status.is_error() {
                    return Err(Self::status_error(status, "the driver refused the value"));
                }
                let request = self.respond(handle, request)?;
                Ok(crate::map::request_for(question.flags, request))
            }
        }
    }

    fn action(
        &mut self,
        handle: Self::Handle,
        _set: &FormSet,
        row: &Row,
    ) -> Result<Request, Error> {
        let access = self.access(handle)?;
        let question = self.question(handle, row)?;
        let mut value = IfrTypeValue { u64: 0 };
        let mut request = BrowserActionRequest::NONE;
        // SAFETY: The protocol belongs to the firmware.
        let status = unsafe {
            (access.as_ref().callback)(
                access.as_ptr(),
                BrowserAction::CHANGED,
                question.question,
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
        self.respond(handle, request)
    }

    fn apply(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        for row in Self::rows_of(set, scope) {
            self.originals
                .remove(&(handle, row.binding.unwrap_or(row.key)));
        }
        Ok(())
    }

    fn discard(&mut self, handle: Self::Handle, set: &FormSet, scope: Scope) -> Result<(), Error> {
        let mut restore = Vec::new();
        for row in Self::rows_of(set, scope) {
            if let Some(value) = self
                .originals
                .remove(&(handle, row.binding.unwrap_or(row.key)))
            {
                restore.push((row, value));
            }
        }
        for (row, value) in restore {
            self.write(handle, set, &row, &value)?;
        }
        Ok(())
    }

    fn reconnect(&mut self, handle: Self::Handle) -> Result<(), Error> {
        let driver = self
            .drivers
            .get(&handle)
            .map(|driver| driver.handle)
            .ok_or_else(|| Self::error(Status::NOT_FOUND, "the set is no longer published"))?;
        let services = self.boot.services();
        // SAFETY: The handle is the driver handle that the database returned.
        let status =
            unsafe { (services.disconnect_controller)(driver, ptr::null_mut(), ptr::null_mut()) };
        if status.is_error() && status != Status::NOT_FOUND {
            return Err(Self::status_error(
                status,
                "cannot disconnect the controller",
            ));
        }
        // SAFETY: The same live handle; every controller path is eligible.
        let status =
            unsafe { (services.connect_controller)(driver, ptr::null(), ptr::null(), Boolean(1)) };
        if status.is_error() {
            return Err(Self::status_error(status, "cannot connect the controller"));
        }
        Ok(())
    }
}

impl HiiProvider {
    /// Maps the firmware request and records it.
    fn respond(
        &mut self,
        handle: HiiHandleId,
        request: BrowserActionRequest,
    ) -> Result<Request, Error> {
        let page = self
            .entries
            .get(&handle)
            .and_then(|entry| entry.set.pages.first())
            .map_or(Key::default(), |page| page.key);
        crate::map::map_request(request.0 as u64, page)
    }
}
