//! Publication of a provider through UEFI HII Config Access.
//!
//! [`Registration`] encodes one `forms::FormSet` with one buffer variable
//! store, creates a driver handle with a vendor device path, adds the package
//! list to the HII database and installs `EFI_HII_CONFIG_ACCESS_PROTOCOL` on
//! that handle.
//!
//! The protocol context lives in firmware pool memory. Firmware keeps its
//! address after a failed removal, so no Rust allocation can become stale.
//! The provider is dropped when removal succeeds.
//!
//! `ExtractConfig` reads every requested question through `Provider::read`.
//! `RouteConfig` writes changed questions through `Provider::write` and
//! returns the provider status on refusal. `Callback` returns
//! `EFI_UNSUPPORTED`, so action questions do not run through the HII browser.

use alloc::{string::String, vec::Vec};
use core::{
    cell::{RefCell, RefMut},
    mem::{align_of, size_of},
    ptr::{self, NonNull},
};

use forms::{FormSet, Provider};
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::hii::config::{BrowserAction, HiiConfigAccessProtocol};
use uefi_raw::protocol::hii::database::HiiDatabaseProtocol;
use uefi_raw::protocol::hii::form_browser::BrowserActionRequest;
use uefi_raw::protocol::hii::ifr::IfrTypeValue;
use uefi_raw::protocol::hii::{HiiHandle, QuestionId};
use uefi_raw::table::boot::InterfaceType;
use uefi_raw::{Char16, Handle, Status};

use crate::producer::PreparedFormSet;
use crate::raw::Boot;

/// The vendor device path: type 1, sub-type 4, GUID, end node.
const PATH_LENGTH: usize = 24;

/// The length of the vendor node: four header bytes and one GUID.
const VENDOR_NODE_LENGTH: u8 = 20;

/// The protocol context of one registration.
///
/// `access` is the first field, so the protocol pointer and the context
/// pointer have the same address. The type is `repr(C)` because firmware reads
/// the protocol table through the first field.
#[repr(C)]
struct Context<P: Provider> {
    access: HiiConfigAccessProtocol,
    provider: RefCell<P>,
    provider_handle: P::Handle,
    prepared: PreparedFormSet,
    path: [u8; PATH_LENGTH],
    boot: Boot,
}

/// One published package list with its Config Access protocol.
///
/// Call [`Self::remove`] before driver unload or ExitBootServices. A dropped
/// registration also tries removal. If firmware refuses a removal step, the
/// context stays allocated and the provider stays alive, because firmware
/// still holds the protocol and configuration pointers.
pub struct Registration<P: Provider + 'static> {
    boot: Boot,
    database: NonNull<HiiDatabaseProtocol>,
    base: NonNull<u8>,
    context: NonNull<Context<P>>,
    handle: Handle,
    package: HiiHandle,
    path_installed: bool,
    access_installed: bool,
    removed: bool,
}

impl<P: Provider + 'static> Registration<P> {
    /// Publishes one formset over the given provider.
    ///
    /// # Safety
    ///
    /// The caller must keep the driver image resident until [`Self::remove`]
    /// succeeds. Boot services must stay valid. Firmware calls the callbacks
    /// on its boot-services thread only.
    ///
    /// # Errors
    ///
    /// Returns the encoding or firmware status. A failed publication removes
    /// every installed interface when firmware permits it.
    pub unsafe fn new(
        boot: Boot,
        set: FormSet,
        provider: P,
        provider_handle: P::Handle,
    ) -> Result<Self, Status> {
        let database = boot
            .locate::<HiiDatabaseProtocol>(&HiiDatabaseProtocol::GUID)
            .ok_or(Status::NOT_FOUND)?;
        let guid = set.guid;
        let prepared = PreparedFormSet::new(set).map_err(|_| Status::INVALID_PARAMETER)?;
        let packages = prepared
            .package_list()
            .map_err(|_| Status::INVALID_PARAMETER)?;

        let mut path = [0u8; PATH_LENGTH];
        path[..4].copy_from_slice(&[1, 4, VENDOR_NODE_LENGTH, 0]);
        path[4..20].copy_from_slice(&guid);
        path[20..].copy_from_slice(&[0x7f, 0xff, 4, 0]);

        let size = size_of::<Context<P>>();
        let align = align_of::<Context<P>>();
        // SAFETY: The boot services table must be live for this call.
        let base = boot
            .allocate(size + align)
            .ok_or(Status::OUT_OF_RESOURCES)?;
        let address = align_pointer(base.as_ptr(), align);
        let context = NonNull::new(address.cast::<Context<P>>()).ok_or(Status::OUT_OF_RESOURCES)?;
        // SAFETY: The pool allocation is at least `size + align` bytes and the
        // aligned address keeps `size` bytes inside it. Nothing is written yet.
        unsafe {
            context.as_ptr().write(Context {
                access: HiiConfigAccessProtocol {
                    extract_config: extract::<P>,
                    route_config: route::<P>,
                    callback,
                },
                provider: RefCell::new(provider),
                provider_handle,
                prepared,
                path,
                boot,
            });
        }

        // SAFETY: The firmware handle may be written.
        let mut handle: Handle = ptr::null_mut();
        let mut registration = Self {
            boot,
            database,
            base,
            context,
            handle: ptr::null_mut(),
            package: ptr::null_mut(),
            path_installed: false,
            access_installed: false,
            removed: false,
        };

        // SAFETY: The context was just written and the path field is live.
        let path_pointer = ptr::from_mut(unsafe { &mut (*context.as_ptr()).path });
        // SAFETY: The path lives in the retained pool context and the handle is writable.
        let status = unsafe {
            (boot.services().install_protocol_interface)(
                &mut handle,
                &DevicePathProtocol::GUID,
                InterfaceType::NATIVE_INTERFACE,
                path_pointer.cast(),
            )
        };
        if status.is_error() {
            registration.free_context();
            return Err(status);
        }
        registration.handle = handle;
        registration.path_installed = true;

        // SAFETY: The context is live and `access` is its first field.
        let access = ptr::from_mut(unsafe { &mut (*context.as_ptr()).access });
        // SAFETY: The access table lives in the retained pool context.
        let status = unsafe {
            (boot.services().install_protocol_interface)(
                &mut handle,
                &HiiConfigAccessProtocol::GUID,
                InterfaceType::NATIVE_INTERFACE,
                access.cast(),
            )
        };
        if status.is_error() {
            // Best effort: a refused removal retains the context for firmware.
            let _ = registration.remove();
            return Err(status);
        }
        registration.access_installed = true;

        // SAFETY: The codec produced a complete package list. Firmware copies
        // it during this call.
        let status = unsafe {
            (database.as_ref().new_package_list)(
                database.as_ptr(),
                packages.as_ptr().cast(),
                registration.handle,
                &mut registration.package,
            )
        };
        if status.is_error() {
            // Best effort: a refused removal retains the context for firmware.
            let _ = registration.remove();
            return Err(status);
        }
        Ok(registration)
    }

    /// Returns the driver handle that owns the package list.
    pub fn driver_handle(&self) -> Handle {
        self.handle
    }

    /// Borrows the provider for host-side changes.
    ///
    /// # Errors
    ///
    /// Returns `ACCESS_DENIED` while a firmware callback runs, and
    /// `NOT_STARTED` after a successful removal.
    pub fn provider_mut(&mut self) -> Result<RefMut<'_, P>, Status> {
        if self.removed {
            return Err(Status::NOT_STARTED);
        }
        // SAFETY: The context is alive until removal.
        let context = unsafe { self.context.as_ref() };
        context
            .provider
            .try_borrow_mut()
            .map_err(|_| Status::ACCESS_DENIED)
    }

    /// Removes the package list and both installed protocols.
    ///
    /// # Errors
    ///
    /// Returns the first firmware refusal. Steps that already succeeded are
    /// not repeated, so the caller may retry.
    pub fn remove(&mut self) -> Result<(), Status> {
        if self.removed {
            return Ok(());
        }
        if !self.package.is_null() {
            // SAFETY: The database and package handle stay live until removal.
            let status = unsafe {
                (self.database.as_ref().remove_package_list)(self.database.as_ptr(), self.package)
            };
            if status.is_error() {
                return Err(status);
            }
            self.package = ptr::null_mut();
        }
        if self.access_installed {
            // The `access` field is first, so its address is the context address.
            // SAFETY: This interface was installed on this handle.
            let status = unsafe {
                (self.boot.services().uninstall_protocol_interface)(
                    self.handle,
                    &HiiConfigAccessProtocol::GUID,
                    self.context.as_ptr().cast(),
                )
            };
            if status.is_error() {
                return Err(status);
            }
            self.access_installed = false;
        }
        if self.path_installed {
            // SAFETY: The context is alive until removal.
            let path = ptr::from_ref(unsafe { &(*self.context.as_ptr()).path });
            // SAFETY: This path was installed on this handle.
            let status = unsafe {
                (self.boot.services().uninstall_protocol_interface)(
                    self.handle,
                    &DevicePathProtocol::GUID,
                    path.cast(),
                )
            };
            if status.is_error() {
                return Err(status);
            }
            self.path_installed = false;
        }
        self.free_context();
        Ok(())
    }

    /// Drops the provider and frees the pool memory.
    fn free_context(&mut self) {
        if self.removed {
            return;
        }
        // SAFETY: Every interface that pointed into the context is removed.
        unsafe {
            ptr::drop_in_place(self.context.as_ptr());
            self.boot.free(self.base);
        }
        self.removed = true;
    }
}

impl<P: Provider + 'static> Drop for Registration<P> {
    fn drop(&mut self) {
        // A failed removal keeps the context alive for firmware.
        let _ = self.remove();
    }
}

/// Returns the first address at or after `base` with `align` alignment.
fn align_pointer(base: *mut u8, align: usize) -> *mut u8 {
    let address = base as usize;
    ((address + align - 1) & !(align - 1)) as *mut u8
}

/// Reads an ASCII configuration string from a firmware string.
///
/// # Safety
///
/// `input` must point to a readable, NUL-terminated string for the duration
/// of the call.
unsafe fn config_string(input: *const Char16) -> Result<String, Status> {
    let mut bytes = Vec::new();
    let mut index = 0;
    loop {
        // SAFETY: The caller guarantees a terminated readable string.
        let unit = unsafe { *input.add(index) };
        if unit == 0 {
            break;
        }
        if unit > 0x7f {
            return Err(Status::INVALID_PARAMETER);
        }
        bytes.push(unit as u8);
        index += 1;
    }
    String::from_utf8(bytes).map_err(|_| Status::INVALID_PARAMETER)
}

/// Copies a configuration string into a firmware pool allocation.
fn copy_response(boot: Boot, text: &str) -> Result<*mut Char16, Status> {
    let units: Vec<Char16> = text.encode_utf16().chain(core::iter::once(0)).collect();
    let size = units.len() * size_of::<Char16>();
    let buffer = boot.allocate(size).ok_or(Status::OUT_OF_RESOURCES)?;
    for (index, unit) in units.iter().enumerate() {
        // SAFETY: The allocation holds every unit including the terminator.
        unsafe { buffer.as_ptr().cast::<Char16>().add(index).write(*unit) };
    }
    Ok(buffer.as_ptr().cast())
}

unsafe extern "efiapi" fn extract<P: Provider>(
    this: *const HiiConfigAccessProtocol,
    request: *const Char16,
    progress: *mut *const Char16,
    results: *mut *mut Char16,
) -> Status {
    if this.is_null() || progress.is_null() || results.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: Config Access supplies writable outputs.
    unsafe {
        *progress = request;
        *results = ptr::null_mut();
    }
    // SAFETY: `access` is the first field of the stable, correctly typed context.
    let context = unsafe { &*this.cast::<Context<P>>() };
    let request_text = if request.is_null() {
        None
    } else {
        // SAFETY: Firmware supplies a terminated request string.
        match unsafe { config_string(request) } {
            Ok(text) => Some(text),
            Err(status) => return status,
        }
    };
    let Ok(mut provider) = context.provider.try_borrow_mut() else {
        return Status::ACCESS_DENIED;
    };
    let response = match context.prepared.extract(
        &mut *provider,
        context.provider_handle,
        request_text.as_deref(),
        &context.path,
    ) {
        Ok(response) => response,
        Err(error) => {
            if !request.is_null() {
                // SAFETY: Request strings are ASCII, so byte and UCS-2 offsets agree.
                unsafe { *progress = request.add(error.progress) };
            }
            return error.status;
        }
    };
    let response = match copy_response(context.boot, &response) {
        Ok(response) => response,
        Err(status) => return status,
    };
    // SAFETY: The caller owns the returned pool allocation.
    unsafe {
        *results = response;
        *progress = if request.is_null() {
            ptr::null()
        } else {
            request.add(request_text.as_ref().map_or(0, String::len))
        };
    }
    Status::SUCCESS
}

unsafe extern "efiapi" fn route<P: Provider>(
    this: *const HiiConfigAccessProtocol,
    configuration: *const Char16,
    progress: *mut *const Char16,
) -> Status {
    if this.is_null() || configuration.is_null() || progress.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: Config Access supplies a writable progress output.
    unsafe { *progress = configuration };
    // SAFETY: `access` is the first field of the stable, correctly typed context.
    let context = unsafe { &*this.cast::<Context<P>>() };
    // SAFETY: Firmware supplies a readable terminated configuration string.
    let text = match unsafe { config_string(configuration) } {
        Ok(text) => text,
        Err(status) => return status,
    };
    let Ok(mut provider) = context.provider.try_borrow_mut() else {
        return Status::ACCESS_DENIED;
    };
    match context.prepared.route(
        &mut *provider,
        context.provider_handle,
        &text,
        &context.path,
    ) {
        Ok(()) => {
            // SAFETY: ASCII length equals the UCS-2 index of the terminator.
            unsafe { *progress = configuration.add(text.len()) };
            Status::SUCCESS
        }
        Err(error) => {
            // SAFETY: The parser reports an offset inside this ASCII string.
            unsafe { *progress = configuration.add(error.progress) };
            error.status
        }
    }
}

unsafe extern "efiapi" fn callback(
    _this: *const HiiConfigAccessProtocol,
    _action: BrowserAction,
    _question_id: QuestionId,
    _value_type: u8,
    _value: *mut IfrTypeValue,
    _action_request: *mut BrowserActionRequest,
) -> Status {
    Status::UNSUPPORTED
}
