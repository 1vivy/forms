//! Direct access to the firmware boot services.
//!
//! The `uefi` crate wraps the protocols this program draws with, but it does
//! not wrap the HII protocols or the extended text input protocol. This module
//! therefore holds the raw `EFI_BOOT_SERVICES` table from the UEFI system table
//! and gives the rest of the program a small checked surface.
//!
//! Every interface this module returns is owned by the firmware. The caller
//! must not free it unless the specification says so.

use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::table::boot::{BootServices, MemoryType};
use uefi_raw::table::system::SystemTable;
use uefi_raw::{Guid, Handle, Status};

/// The longest device path this module copies, in bytes.
const MAX_DEVICE_PATH: usize = 1 << 16;

/// A checked handle to the firmware boot services.
#[derive(Clone, Copy)]
pub struct Boot {
    services: NonNull<BootServices>,
}

impl Boot {
    /// Takes the boot services from the UEFI system table.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `table` stays valid for the whole program.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use core::ptr::NonNull;
    /// # use hii_uefi::raw::Boot;
    /// let raw = std::os::uefi::env::system_table();
    /// // SAFETY: The standard library keeps the system table mapped.
    /// let boot = unsafe { Boot::from_system_table(raw.cast()) }.expect("boot services");
    /// # let _ = boot;
    /// ```
    pub unsafe fn from_system_table(table: NonNull<SystemTable>) -> Option<Self> {
        // SAFETY: The caller guarantees that the table is valid.
        let services = unsafe { (*table.as_ptr()).boot_services };
        NonNull::new(services).map(|services| Self { services })
    }

    /// Returns the boot services table.
    pub fn services(&self) -> &BootServices {
        // SAFETY: The table lives for the whole program.
        unsafe { self.services.as_ref() }
    }

    /// Returns the first interface of the protocol with this GUID.
    pub fn locate<T>(&self, guid: &Guid) -> Option<NonNull<T>> {
        let mut interface: *mut c_void = ptr::null_mut();
        // SAFETY: The GUID is a valid protocol GUID and the output pointer is
        // writable.
        let status =
            unsafe { (self.services().locate_protocol)(guid, ptr::null(), &mut interface) };
        if status.is_error() {
            return None;
        }
        NonNull::new(interface.cast::<T>())
    }

    /// Returns the interface of this protocol on a handle.
    ///
    /// # Safety
    ///
    /// `handle` must be a live firmware handle.
    pub unsafe fn protocol<T>(&self, handle: Handle, guid: &Guid) -> Option<NonNull<T>> {
        let mut interface: *mut c_void = ptr::null_mut();
        // SAFETY: The caller guarantees the handle, and the output pointer is
        // writable.
        let status = unsafe { (self.services().handle_protocol)(handle, guid, &mut interface) };
        if status.is_error() {
            return None;
        }
        NonNull::new(interface.cast::<T>())
    }

    /// Allocates firmware pool memory.
    pub fn allocate(&self, size: usize) -> Option<NonNull<u8>> {
        let mut buffer: *mut u8 = ptr::null_mut();
        // SAFETY: The output pointer is writable.
        let status = unsafe {
            (self.services().allocate_pool)(MemoryType::BOOT_SERVICES_DATA, size, &mut buffer)
        };
        if status.is_error() {
            return None;
        }
        NonNull::new(buffer)
    }

    /// Releases firmware pool memory.
    ///
    /// # Safety
    ///
    /// `buffer` must come from [`Boot::allocate`] or from a firmware service
    /// that allocates pool memory.
    pub unsafe fn free(&self, buffer: NonNull<u8>) {
        // SAFETY: The caller guarantees that the buffer is pool memory.
        let _ = unsafe { (self.services().free_pool)(buffer.as_ptr()) };
    }

    /// Waits at least this many microseconds.
    pub fn stall(&self, microseconds: usize) {
        // SAFETY: The boot services table is live.
        let _ = unsafe { (self.services().stall)(microseconds) };
    }
}

/// Returns the numeric value of a firmware status code.
///
/// # Examples
///
/// ```
/// assert_eq!(hii_uefi::raw::status_code(uefi_raw::Status::SUCCESS), 0);
/// ```
pub fn status_code(status: Status) -> u64 {
    status.0 as u64
}

/// Copies a device path into a byte vector.
///
/// The copy stops at the end node or at an invalid node length.
///
/// # Safety
///
/// `path` must point to a device path that the firmware owns.
pub unsafe fn device_path_bytes(path: *const DevicePathProtocol) -> Vec<u8> {
    let mut size = 0usize;
    loop {
        // SAFETY: The caller guarantees a readable device path.
        let node = unsafe { path.cast::<u8>().add(size) };
        // SAFETY: A device path node starts with type, sub-type and length.
        let header = unsafe { ptr::read_unaligned(node.cast::<[u8; 4]>()) };
        let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
        if length < 4 || size + length > MAX_DEVICE_PATH {
            break;
        }
        size += length;
        if header[0] == 0x7f && header[1] == 0xff {
            break;
        }
    }
    // SAFETY: The bytes were just checked to lie inside the device path.
    unsafe { core::slice::from_raw_parts(path.cast::<u8>(), size) }.to_vec()
}
