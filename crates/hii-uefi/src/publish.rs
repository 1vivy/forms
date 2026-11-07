//! Publication of a provider through UEFI HII Config Access.
//!
//! [`Registration`] encodes one form set with one buffer variable store,
//! creates a driver handle with a vendor device path, adds the package list to
//! the HII database and installs `EFI_HII_CONFIG_ACCESS_PROTOCOL` on that
//! handle.
//!
//! The protocol context lives in firmware pool memory. Firmware keeps its
//! address after a failed removal, so no Rust allocation can become stale.
//!
//! `ExtractConfig` reads the requested questions through [`forms::Provider`].
//! `RouteConfig` writes changed questions through the same trait. `Callback`
//! runs an action question through [`forms::Provider::action`].

use alloc::string::String;
use core::{
    cell::RefCell,
    mem::{align_of, size_of},
    ptr::{self, NonNull},
};

use forms::{FormSet, Key, Provider};
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::hii::config::{BrowserAction, HiiConfigAccessProtocol};
use uefi_raw::protocol::hii::database::HiiDatabaseProtocol;
use uefi_raw::protocol::hii::form_browser::BrowserActionRequest;
use uefi_raw::protocol::hii::ifr::IfrTypeValue;
use uefi_raw::protocol::hii::{HiiHandle, QuestionId};
use uefi_raw::table::boot::InterfaceType;
use uefi_raw::{Char16, Handle, Status};

use crate::producer::{ConfigError, PreparedFormSet};
use crate::raw::Boot;
use crate::text;

/// The vendor device path: type 1, sub-type 4, GUID, end node.
const PATH_LENGTH: usize = 24;

/// The length of the vendor node: four header bytes and one GUID.
const VENDOR_NODE_LENGTH: u8 = 20;

/// The protocol context of one registration.
#[repr(C)]
struct Context<P: Provider> {
    access: HiiConfigAccessProtocol,
    provider: RefCell<P>,
    provider_handle: P::Handle,
    prepared: PreparedFormSet,
    path: [u8; PATH_LENGTH],
    boot: Boot,
    /// The request of the last value write, for the next `Callback` call.
    pending: core::cell::Cell<usize>,
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
    installed: bool,
    removed: bool,
}

impl<P: Provider + 'static> Registration<P> {
    /// Publishes one form set over the given provider.
    ///
    /// The GUID identifies the form set in the HII database.
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
        guid: [u8; 16],
        provider: P,
        provider_handle: P::Handle,
    ) -> Result<Self, Status> {
        let database = boot
            .locate::<HiiDatabaseProtocol>(&HiiDatabaseProtocol::GUID)
            .ok_or(Status::NOT_FOUND)?;
        let prepared = PreparedFormSet::new(set, guid).map_err(|_| Status::INVALID_PARAMETER)?;
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
        // SAFETY: The pool allocation keeps `size` bytes at the aligned
        // address. Nothing is written yet.
        unsafe {
            context.as_ptr().write(Context {
                access: HiiConfigAccessProtocol {
                    extract_config: extract::<P>,
                    route_config: route::<P>,
                    callback: callback::<P>,
                },
                provider: RefCell::new(provider),
                provider_handle,
                prepared,
                path,
                boot,
                pending: core::cell::Cell::new(0),
            });
        }

        let mut registration = Self {
            boot,
            database,
            base,
            context,
            handle: ptr::null_mut(),
            package: ptr::null_mut(),
            installed: false,
            removed: false,
        };

        // SAFETY: The context was just written and the path field is live.
        let path_pointer = ptr::from_mut(unsafe { &mut (*context.as_ptr()).path });
        // SAFETY: The path lives in the retained pool context.
        let status = unsafe {
            (boot.services().install_protocol_interface)(
                &mut registration.handle,
                &DevicePathProtocol::GUID,
                InterfaceType::NATIVE_INTERFACE,
                path_pointer.cast(),
            )
        };
        if status.is_error() {
            registration.free_context();
            return Err(status);
        }
        // SAFETY: The context is live and `access` is its first field.
        let access = ptr::from_mut(unsafe { &mut (*context.as_ptr()).access });
        // SAFETY: The table lives in the retained pool context.
        let status = unsafe {
            (boot.services().install_protocol_interface)(
                &mut registration.handle,
                &HiiConfigAccessProtocol::GUID,
                InterfaceType::NATIVE_INTERFACE,
                access.cast(),
            )
        };
        if status.is_error() {
            let _ = registration.remove();
            return Err(status);
        }
        registration.installed = true;
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
            let _ = registration.remove();
            return Err(status);
        }
        Ok(registration)
    }

    /// Returns the driver handle that owns the package list.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hii_uefi::publish::Registration;
    /// # use hii_uefi::raw::Boot;
    /// # use uefi_raw::Handle;
    /// # fn example<R: forms::Provider + 'static>(registration: &Registration<R>) -> Handle {
    /// registration.driver_handle()
    /// # }
    /// ```
    pub fn driver_handle(&self) -> Handle {
        self.handle
    }

    /// Borrows the provider for host-side changes.
    ///
    /// # Errors
    ///
    /// Returns `ACCESS_DENIED` while a firmware callback runs, and
    /// `NOT_STARTED` after a successful removal.
    pub fn provider_mut(&mut self) -> Result<core::cell::RefMut<'_, P>, Status> {
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
        if self.installed {
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
            self.installed = false;
        }
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
        let _ = self.remove();
    }
}

/// Aligns a pointer upwards.
fn align_pointer(pointer: *mut u8, align: usize) -> *mut u8 {
    let address = pointer as usize;
    let mask = align - 1;
    ((address + mask) & !mask) as *mut u8
}

/// `EFI_HII_CONFIG_ACCESS_EXTRACT_CONFIG`.
///
/// # Safety
///
/// `this` must point at the `access` field of a live context.
unsafe extern "efiapi" fn extract<P: Provider>(
    this: *const HiiConfigAccessProtocol,
    request: *const Char16,
    progress: *mut *const Char16,
    results: *mut *mut Char16,
) -> Status {
    if this.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: The caller guarantees a live context whose first field is this
    // table.
    let context = unsafe { &*this.cast::<Context<P>>() };
    let Ok(mut provider) = context.provider.try_borrow_mut() else {
        return Status::ACCESS_DENIED;
    };
    let request = if request.is_null() {
        None
    } else {
        // SAFETY: The firmware passes a NUL-terminated string.
        Some(unsafe { text::from_ucs2(request) })
    };
    let answer = context.prepared.extract(
        &mut *provider,
        context.provider_handle,
        request.as_deref(),
        &context.path,
    );
    match answer {
        Ok(text_answer) => {
            let units = text::ucs2(&text_answer);
            let bytes = units.len() * 2;
            // SAFETY: The boot services allocate pool memory.
            let Some(buffer) = context.boot.allocate(bytes) else {
                return Status::OUT_OF_RESOURCES;
            };
            // SAFETY: The buffer holds `bytes` bytes.
            unsafe {
                ptr::copy_nonoverlapping(units.as_ptr().cast::<u8>(), buffer.as_ptr(), bytes);
            }
            if !results.is_null() {
                // SAFETY: The caller guarantees a writable output pointer.
                unsafe { results.write(buffer.as_ptr().cast::<Char16>()) };
            }
            Status::SUCCESS
        }
        Err(error) => {
            if !progress.is_null() {
                // SAFETY: The caller guarantees a writable output pointer.
                unsafe { progress.write(ptr::null()) };
            }
            let _ = error.progress;
            error.status
        }
    }
}

/// `EFI_HII_CONFIG_ACCESS_ROUTE_CONFIG`.
///
/// # Safety
///
/// `this` must point at the `access` field of a live context.
unsafe extern "efiapi" fn route<P: Provider>(
    this: *const HiiConfigAccessProtocol,
    configuration: *const Char16,
    progress: *mut *const Char16,
) -> Status {
    if this.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: The caller guarantees a live context whose first field is this
    // table.
    let context = unsafe { &*this.cast::<Context<P>>() };
    let Ok(mut provider) = context.provider.try_borrow_mut() else {
        return Status::ACCESS_DENIED;
    };
    // SAFETY: The firmware passes a NUL-terminated string or a null pointer.
    let Some(configuration) =
        (unsafe { configuration.as_ref() }).map(|_| unsafe { text::from_ucs2(configuration) })
    else {
        return Status::INVALID_PARAMETER;
    };
    let configuration: String = configuration;
    let answer = context.prepared.route(
        &mut *provider,
        context.provider_handle,
        &configuration,
        &context.path,
    );
    match answer {
        Ok(request) => {
            // The browser reads the request in its next Callback call.
            context
                .pending
                .set(crate::map::request_value(request) as usize);
            Status::SUCCESS
        }
        Err(error) => {
            let _ = progress;
            error.status
        }
    }
}

/// `EFI_HII_CONFIG_ACCESS_CALLBACK`.
///
/// # Safety
///
/// `this` must point at the `access` field of a live context.
unsafe extern "efiapi" fn callback<P: Provider>(
    this: *const HiiConfigAccessProtocol,
    action: BrowserAction,
    question_id: QuestionId,
    _kind: u8,
    _value: *mut IfrTypeValue,
    action_request: *mut BrowserActionRequest,
) -> Status {
    if this.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: The caller guarantees a live context whose first field is this
    // table.
    let context = unsafe { &*this.cast::<Context<P>>() };
    if action != BrowserAction::CHANGED {
        return Status::SUCCESS;
    }
    let Ok(mut provider) = context.provider.try_borrow_mut() else {
        return Status::ACCESS_DENIED;
    };
    let question = question_id as u64;
    let row = context
        .prepared
        .export()
        .questions
        .iter()
        .find(|entry| u64::from(entry.question) == question)
        .map(|entry| entry.row)
        .unwrap_or(Key::default());
    let Some(row) = context.prepared.formset().row(row).cloned() else {
        return Status::NOT_FOUND;
    };
    let answer = provider.action(context.provider_handle, context.prepared.formset(), &row);
    // A write reports its request first, because the browser calls Callback
    // after the value changed.
    let pending = context.pending.replace(0);
    let (value, status) = match answer {
        Ok(request) => {
            let value = if pending != 0 {
                pending
            } else {
                crate::map::request_value(request) as usize
            };
            (value, Status::SUCCESS)
        }
        Err(error) => (
            0,
            Status(usize::try_from(error.code).unwrap_or(Status::DEVICE_ERROR.0)),
        ),
    };
    if !action_request.is_null() && value != 0 {
        // SAFETY: The caller guarantees a writable request output.
        unsafe { action_request.write(BrowserActionRequest(value)) };
    }
    status
}

/// Returns the configuration error of one failure.
fn _config_error(error: ConfigError) -> Status {
    error.status
}
