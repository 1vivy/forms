//! The inherited console of the firmware.
//!
//! Both console protocols are opened for use, not exclusively: the consumer
//! keeps its own console writer, and taking the console away from it would
//! silence a launch line or a failure report.

use uefi::boot::{
    OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol, image_handle, open_protocol,
};
use uefi::proto::ProtocolPointer;
use uefi::table::system_table_raw;
use uefi::{Error, Handle, Status};

/// Opens a console protocol on `handle` without closing it for other owners.
///
/// # Errors
///
/// Returns the firmware failure when the protocol cannot be opened.
pub(crate) fn open<P>(handle: Handle) -> Result<ScopedProtocol<P>, Error>
where
    P: ProtocolPointer + ?Sized,
{
    let params = OpenProtocolParams {
        handle,
        agent: image_handle(),
        controller: None,
    };
    // SAFETY: The console protocols stay installed while the front end runs,
    // and the consumer writes no console text during a session.
    unsafe { open_protocol::<P>(params, OpenProtocolAttributes::GetProtocol) }
}

/// Returns the console input and output handles of the system table.
///
/// # Errors
///
/// Returns [`Status::UNSUPPORTED`] when the system table or a handle is
/// missing.
pub(crate) fn handles() -> Result<(Handle, Handle), Error> {
    let table = system_table_raw().ok_or(Error::from(Status::UNSUPPORTED))?;
    // SAFETY: The image inherits the system table from the firmware, and the
    // table stays live while the front end runs.
    let table = unsafe { table.as_ref() };
    Ok((handle(table.stdin_handle)?, handle(table.stdout_handle)?))
}

/// Returns a handle from a raw console pointer.
fn handle(raw: *mut core::ffi::c_void) -> Result<Handle, Error> {
    // SAFETY: The system table names live console handles.
    unsafe { Handle::from_ptr(raw) }.ok_or(Error::from(Status::UNSUPPORTED))
}
