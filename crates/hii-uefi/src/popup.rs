//! The HII popup protocol over the native popup model.
//!
//! Firmware publishes `EFI_HII_POPUP_PROTOCOL` so that a driver callback can
//! ask the setup browser for one decision. `Registration` installs that
//! protocol. Each `CreatePopup` call then runs four steps:
//!
//! 1. The adapter checks the popup type and the message identifier.
//! 2. [`Strings`] resolves the message through the HII string protocol. The
//!    firmware implementation is `HiiStrings`.
//! 3. The adapter builds a [`forms::Popup`] and asks [`Presenter`] to show it.
//!    The presentation owns every display and input decision.
//! 4. The adapter checks the answer against the popup type and writes the
//!    `EFI_HII_POPUP_SELECTION` value.
//!
//! The crate holds no display code. A front end supplies the [`Presenter`],
//! because display and input belong to the front end. This keeps `hii-uefi`
//! free of `forms-uefi`.
//!
//! # Status values
//!
//! | Condition | Status |
//! | --- | --- |
//! | The popup was shown | `EFI_SUCCESS` |
//! | Unknown popup type, null handle, message 0, unresolvable message, or an answer that the type does not offer | `EFI_INVALID_PARAMETER` |
//! | The presenter or the resolver is already busy | `EFI_ACCESS_DENIED` |
//! | The presenter failure carries an error code | that code |
//! | The presenter failure carries no error code | `EFI_ABORTED` |
//!
//! An unknown popup style shows the message as information, as the EDK II
//! display engine does with a style that it does not know.
//!
//! `Registration` and `HiiStrings` call firmware, so they exist on UEFI
//! targets only. The two traits and the protocol logic build on the host,
//! where the boundary tests drive the callback directly.

use alloc::string::String;

use forms::{Error, Popup, PopupSelection};
use uefi_raw::protocol::hii::{HiiHandle, StringId};

/// Shows a popup and returns the answer that the person gave.
///
/// A front end implements this trait over its own display and input. The
/// adapter calls it once per `CreatePopup` call, on the boot-services thread.
///
/// # Examples
///
/// ```
/// use forms::{Error, Popup, PopupSelection};
/// use hii_uefi::popup::Presenter;
///
/// /// Accepts the highlighted answer without a display.
/// struct Accept;
///
/// impl Presenter for Accept {
///     fn present(&mut self, popup: &mut Popup) -> Result<PopupSelection, Error> {
///         Ok(popup.kind.selections()[0])
///     }
/// }
/// ```
pub trait Presenter {
    /// Shows `popup` and returns the selection that the person made.
    ///
    /// The popup arrives with its first answer highlighted. The adapter reads
    /// no field after the call; the return value is the answer.
    ///
    /// # Errors
    ///
    /// Returns the reason when the popup cannot be shown. A code with the EFI
    /// error bit becomes the protocol status. Any other code becomes
    /// `EFI_ABORTED`.
    fn present(&mut self, popup: &mut Popup) -> Result<PopupSelection, Error>;
}

/// Resolves the message text of a popup.
///
/// The adapter resolves before it shows the popup, so a message that does not
/// resolve never reaches the display.
pub trait Strings {
    /// Returns the message text of `message` in the package list of `handle`.
    ///
    /// The answer is `None` when the firmware has no such string.
    fn resolve(&mut self, handle: HiiHandle, message: StringId) -> Option<String>;
}

impl<P: Presenter + ?Sized> Presenter for alloc::boxed::Box<P> {
    fn present(&mut self, popup: &mut Popup) -> Result<PopupSelection, Error> {
        (**self).present(popup)
    }
}

impl<S: Strings + ?Sized> Strings for alloc::boxed::Box<S> {
    fn resolve(&mut self, handle: HiiHandle, message: StringId) -> Option<String> {
        (**self).resolve(handle, message)
    }
}

/// The protocol table, the presenter and the resolver of one installation.
///
/// `protocol` is the first field, so the table address and the context address
/// are the same. Firmware reads the table through the pointer that it holds.
#[cfg(any(target_os = "uefi", test))]
mod serve {
    use core::cell::RefCell;

    use forms::{Popup, PopupSelection, PopupStyle, PopupType};
    use uefi_raw::Status;
    use uefi_raw::protocol::hii::popup::{
        HiiPopupProtocol, HiiPopupSelection, HiiPopupStyle, HiiPopupType,
    };
    use uefi_raw::protocol::hii::{HiiHandle, StringId};

    use super::{Presenter, Strings};

    /// The vendor device path: type 1, sub-type 4, GUID, end node.
    #[cfg(target_os = "uefi")]
    const PATH_LENGTH: usize = 24;

    /// The length of the vendor node: four header bytes and one GUID.
    #[cfg(target_os = "uefi")]
    const VENDOR_NODE_LENGTH: u8 = 20;

    /// One installed protocol table with its two services.
    #[repr(C)]
    pub(super) struct Context<P: Presenter, S: Strings> {
        pub(super) protocol: HiiPopupProtocol,
        presenter: RefCell<P>,
        strings: RefCell<S>,
        /// The device path of a handle that this context creates.
        #[cfg(target_os = "uefi")]
        pub(super) path: [u8; PATH_LENGTH],
    }

    impl<P: Presenter, S: Strings> Context<P, S> {
        /// Builds a context and its protocol table.
        pub(super) fn new(presenter: P, strings: S) -> Self {
            Self {
                protocol: HiiPopupProtocol {
                    revision: HiiPopupProtocol::REVISION,
                    create_popup: create::<P, S>,
                },
                presenter: RefCell::new(presenter),
                strings: RefCell::new(strings),
                #[cfg(target_os = "uefi")]
                path: vendor_path(),
            }
        }

        /// Resolves the message, shows the popup and checks the answer.
        pub(super) fn decide(
            &self,
            style: HiiPopupStyle,
            kind: HiiPopupType,
            handle: HiiHandle,
            message: StringId,
        ) -> Result<HiiPopupSelection, Status> {
            let kind = kind_of(kind).ok_or(Status::INVALID_PARAMETER)?;
            if handle.is_null() || message == 0 {
                return Err(Status::INVALID_PARAMETER);
            }
            let text = {
                let Ok(mut strings) = self.strings.try_borrow_mut() else {
                    return Err(Status::ACCESS_DENIED);
                };
                strings
                    .resolve(handle, message)
                    .ok_or(Status::INVALID_PARAMETER)?
            };
            let mut popup = Popup::new(style_of(style), kind, text);
            let answer = {
                let Ok(mut presenter) = self.presenter.try_borrow_mut() else {
                    return Err(Status::ACCESS_DENIED);
                };
                match presenter.present(&mut popup) {
                    Ok(answer) => answer,
                    Err(error) => return Err(status_of(&error)),
                }
            };
            if !kind.selections().contains(&answer) {
                return Err(Status::INVALID_PARAMETER);
            }
            Ok(selection_of(answer))
        }
    }

    /// `EFI_HII_CREATE_POPUP`.
    ///
    /// # Safety
    ///
    /// `this` must point at the `protocol` field of a live `Context<P, S>`.
    /// `selection` is null or writable. The context stays alive for the call.
    pub(super) unsafe extern "efiapi" fn create<P: Presenter, S: Strings>(
        this: *const HiiPopupProtocol,
        style: HiiPopupStyle,
        kind: HiiPopupType,
        handle: HiiHandle,
        message: StringId,
        selection: *mut HiiPopupSelection,
    ) -> Status {
        if this.is_null() {
            return Status::INVALID_PARAMETER;
        }
        // SAFETY: The caller guarantees a live context whose first field is
        // this table.
        let context = unsafe { &*this.cast::<Context<P, S>>() };
        match context.decide(style, kind, handle, message) {
            Ok(answer) => {
                if !selection.is_null() {
                    // SAFETY: The caller guarantees a writable answer output.
                    unsafe { selection.write(answer) };
                }
                Status::SUCCESS
            }
            Err(status) => status,
        }
    }

    /// Maps the ABI style to the model style.
    ///
    /// A style that the specification does not define shows information, as
    /// the EDK II display engine does with a style that it does not know.
    fn style_of(style: HiiPopupStyle) -> PopupStyle {
        if style == HiiPopupStyle::WARNING {
            PopupStyle::Warning
        } else if style == HiiPopupStyle::ERROR {
            PopupStyle::Error
        } else {
            PopupStyle::Info
        }
    }

    /// Maps the ABI type to the model type.
    fn kind_of(kind: HiiPopupType) -> Option<PopupType> {
        if kind == HiiPopupType::OK {
            Some(PopupType::Ok)
        } else if kind == HiiPopupType::OK_CANCEL {
            Some(PopupType::OkCancel)
        } else if kind == HiiPopupType::YES_NO {
            Some(PopupType::YesNo)
        } else if kind == HiiPopupType::YES_NO_CANCEL {
            Some(PopupType::YesNoCancel)
        } else {
            None
        }
    }

    /// Maps the model answer to the ABI answer.
    pub(super) fn selection_of(answer: PopupSelection) -> HiiPopupSelection {
        match answer {
            PopupSelection::Ok => HiiPopupSelection::OK,
            PopupSelection::Cancel => HiiPopupSelection::CANCEL,
            PopupSelection::Yes => HiiPopupSelection::YES,
            PopupSelection::No => HiiPopupSelection::NO,
        }
    }

    /// Turns a presenter failure into a protocol status.
    fn status_of(error: &forms::Error) -> Status {
        let status = Status(error.code as usize);
        if status.is_error() {
            status
        } else {
            Status::ABORTED
        }
    }

    /// Builds the vendor device path of a created handle.
    #[cfg(target_os = "uefi")]
    fn vendor_path() -> [u8; PATH_LENGTH] {
        let mut path = [0u8; PATH_LENGTH];
        path[..4].copy_from_slice(&[1, 4, VENDOR_NODE_LENGTH, 0]);
        path[4..20].copy_from_slice(&HiiPopupProtocol::GUID.to_bytes());
        path[20..].copy_from_slice(&[0x7f, 0xff, 4, 0]);
        path
    }
}

#[cfg(target_os = "uefi")]
pub use firmware::{HiiStrings, Registration};

/// The firmware side of the popup protocol.
///
/// # Examples
///
/// ```no_run
/// use hii_uefi::popup::{HiiStrings, Registration};
/// use hii_uefi::raw::Boot;
///
/// # fn serve(boot: Boot, presenter: impl hii_uefi::popup::Presenter + 'static)
/// #     -> Result<(), uefi_raw::Status> {
/// let strings = HiiStrings::new(boot)?;
/// // SAFETY: The driver stays resident while the protocol is installed.
/// let mut registration = unsafe { Registration::install(boot, presenter, strings)? };
/// // ... the HII browser calls CreatePopup ...
/// registration.remove()?;
/// # Ok(())
/// # }
/// ```
#[cfg(target_os = "uefi")]
mod firmware {
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::mem::{align_of, size_of};
    use core::ptr::{self, NonNull};

    use uefi_raw::protocol::device_path::DevicePathProtocol;
    use uefi_raw::protocol::hii::font::FontInfo;
    use uefi_raw::protocol::hii::popup::HiiPopupProtocol;
    use uefi_raw::protocol::hii::string::HiiStringProtocol;
    use uefi_raw::protocol::hii::{HiiHandle, StringId};
    use uefi_raw::table::boot::InterfaceType;
    use uefi_raw::{Handle, Status};

    use super::serve::Context;
    use super::{Presenter, Strings};
    use crate::raw::Boot;

    /// The language that the resolver uses when a package list reports none.
    const DEFAULT_LANGUAGE: &[u8] = b"eng\0";

    /// One installed `EFI_HII_POPUP_PROTOCOL`.
    ///
    /// Call [`Self::remove`] before driver unload or ExitBootServices. A
    /// dropped registration also tries removal. If firmware refuses a removal
    /// step, the context stays allocated and the presenter and the resolver
    /// stay alive, because firmware still holds the protocol pointer.
    pub struct Registration<P: Presenter + 'static, S: Strings + 'static> {
        boot: Boot,
        base: NonNull<u8>,
        context: NonNull<Context<P, S>>,
        handle: Handle,
        path_installed: bool,
        installed: bool,
        freed: bool,
    }

    impl<P: Presenter + 'static, S: Strings + 'static> Registration<P, S> {
        /// Installs the protocol on a handle that it creates.
        ///
        /// The new handle carries a vendor device path, so that
        /// `LocateHandleBuffer` finds the producer.
        ///
        /// # Safety
        ///
        /// The caller must keep the driver image resident until [`Self::remove`]
        /// succeeds. Boot services must stay valid. Firmware calls the presenter
        /// on its boot-services thread only.
        ///
        /// # Errors
        ///
        /// Returns the firmware status when the pool allocation or a protocol
        /// installation fails. A failed installation frees the context when
        /// firmware permits it.
        pub unsafe fn install(boot: Boot, presenter: P, strings: S) -> Result<Self, Status> {
            let size = size_of::<Context<P, S>>();
            let align = align_of::<Context<P, S>>();
            let base = boot
                .allocate(size + align)
                .ok_or(Status::OUT_OF_RESOURCES)?;
            let address = align_pointer(base.as_ptr(), align);
            let context =
                NonNull::new(address.cast::<Context<P, S>>()).ok_or(Status::OUT_OF_RESOURCES)?;
            // SAFETY: The allocation holds `size + align` bytes and the aligned
            // address keeps `size` bytes inside it. Nothing is written yet.
            unsafe {
                context.as_ptr().write(Context::new(presenter, strings));
            }
            let mut registration = Self {
                boot,
                base,
                context,
                handle: ptr::null_mut(),
                path_installed: false,
                installed: false,
                freed: false,
            };

            // SAFETY: The path lives in the retained pool context and the
            // handle output is writable.
            let status = unsafe {
                let path = ptr::from_mut(&mut (*context.as_ptr()).path);
                (boot.services().install_protocol_interface)(
                    &mut registration.handle,
                    &DevicePathProtocol::GUID,
                    InterfaceType::NATIVE_INTERFACE,
                    path.cast(),
                )
            };
            if status.is_error() {
                registration.free_context();
                return Err(status);
            }
            registration.path_installed = true;

            // SAFETY: The context is live and `protocol` is its first field.
            let protocol = unsafe { ptr::from_mut(&mut (*context.as_ptr()).protocol) };
            // SAFETY: The table lives in the retained pool context.
            let status = unsafe {
                (boot.services().install_protocol_interface)(
                    &mut registration.handle,
                    &HiiPopupProtocol::GUID,
                    InterfaceType::NATIVE_INTERFACE,
                    protocol.cast(),
                )
            };
            if status.is_error() {
                // Best effort: a refused removal retains the context for
                // firmware.
                let _ = registration.remove();
                return Err(status);
            }
            registration.installed = true;
            Ok(registration)
        }

        /// Returns the handle that owns the installed protocol.
        pub fn handle(&self) -> Handle {
            self.handle
        }

        /// Removes the installed protocol and frees the context.
        ///
        /// # Errors
        ///
        /// Returns the first firmware refusal. Steps that already succeeded are
        /// not repeated, so the caller may retry.
        pub fn remove(&mut self) -> Result<(), Status> {
            if self.installed {
                // SAFETY: The context is alive and `protocol` is its first
                // field, so its address is the context address.
                let protocol = unsafe { ptr::from_ref(&(*self.context.as_ptr()).protocol) };
                // SAFETY: This table was installed on this handle.
                let status = unsafe {
                    (self.boot.services().uninstall_protocol_interface)(
                        self.handle,
                        &HiiPopupProtocol::GUID,
                        protocol.cast(),
                    )
                };
                if status.is_error() {
                    return Err(status);
                }
                self.installed = false;
            }
            if self.path_installed {
                // SAFETY: The context is alive until removal.
                let path = unsafe { ptr::from_ref(&(*self.context.as_ptr()).path) };
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

        /// Drops the presenter, the resolver and the pool memory.
        fn free_context(&mut self) {
            if self.freed {
                return;
            }
            // SAFETY: Every interface that pointed into the context is removed.
            unsafe {
                ptr::drop_in_place(self.context.as_ptr());
                self.boot.free(self.base);
            }
            self.freed = true;
        }
    }

    impl<P: Presenter + 'static, S: Strings + 'static> Drop for Registration<P, S> {
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

    /// Resolves popup messages through `EFI_HII_STRING_PROTOCOL`.
    ///
    /// The resolver reads the primary language of the package list on every
    /// call. A package list without a usable language falls back to `eng`.
    pub struct HiiStrings {
        strings: NonNull<HiiStringProtocol>,
    }

    impl HiiStrings {
        /// Opens the HII string protocol.
        ///
        /// # Errors
        ///
        /// Returns `EFI_NOT_FOUND` when the firmware publishes no HII string
        /// protocol.
        pub fn new(boot: Boot) -> Result<Self, Status> {
            let strings = boot
                .locate::<HiiStringProtocol>(&HiiStringProtocol::GUID)
                .ok_or(Status::NOT_FOUND)?;
            Ok(Self { strings })
        }

        /// Returns the primary language of one package list, as a C string.
        ///
        /// The answer holds the first language only, because the string
        /// protocol takes one language at a time.
        fn language(&self, handle: HiiHandle) -> Option<Vec<u8>> {
            let protocol = self.strings;
            let mut size = 0usize;
            // SAFETY: The protocol belongs to the firmware and the size output
            // is writable.
            let status = unsafe {
                (protocol.as_ref().get_languages)(
                    protocol.as_ptr(),
                    handle,
                    ptr::null_mut(),
                    &mut size,
                )
            };
            if status != Status::BUFFER_TOO_SMALL || size == 0 {
                return None;
            }
            let mut buffer = vec![0u8; size];
            let mut length = size;
            // SAFETY: The buffer holds `size` bytes, which is what the firmware
            // asked for.
            let status = unsafe {
                (protocol.as_ref().get_languages)(
                    protocol.as_ptr(),
                    handle,
                    buffer.as_mut_ptr(),
                    &mut length,
                )
            };
            if status.is_error() {
                return None;
            }
            if let Some(end) = buffer.iter().position(|byte| matches!(*byte, 0 | b';')) {
                buffer.truncate(end);
            }
            if buffer.is_empty() {
                return None;
            }
            buffer.push(0);
            Some(buffer)
        }

        /// Reads one string token as Rust text.
        fn text(&self, language: &[u8], handle: HiiHandle, message: StringId) -> Option<String> {
            let protocol = self.strings;
            let mut size = 128usize;
            loop {
                let mut buffer = vec![0u16; size];
                let mut length = size;
                let mut font: *mut FontInfo = ptr::null_mut();
                // SAFETY: The buffer holds `size` code units and both outputs
                // are writable.
                let status = unsafe {
                    (protocol.as_ref().get_string)(
                        protocol.as_ptr(),
                        language.as_ptr(),
                        handle,
                        message,
                        buffer.as_mut_ptr(),
                        &mut length,
                        &mut font,
                    )
                };
                if status == Status::BUFFER_TOO_SMALL {
                    if length <= size {
                        return None;
                    }
                    size = length;
                    continue;
                }
                if status.is_error() {
                    return None;
                }
                let mut units = &buffer[..length.min(buffer.len())];
                while let [rest @ .., 0] = units {
                    units = rest;
                }
                return Some(String::from_utf16_lossy(units));
            }
        }
    }

    impl Strings for HiiStrings {
        fn resolve(&mut self, handle: HiiHandle, message: StringId) -> Option<String> {
            let language = self
                .language(handle)
                .unwrap_or_else(|| DEFAULT_LANGUAGE.to_vec());
            self.text(&language, handle, message)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::boxed::Box;
    use alloc::rc::Rc;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use core::cell::{Cell, RefCell};
    use core::ffi::c_void;
    use core::ptr;

    use forms::{Error, Input, Popup, PopupSelection, PopupStyle, PopupType};
    use uefi_raw::Status;
    use uefi_raw::protocol::hii::popup::{
        HiiPopupProtocol, HiiPopupSelection, HiiPopupStyle, HiiPopupType,
    };
    use uefi_raw::protocol::hii::{HiiHandle, StringId};

    use super::serve::{self, Context};
    use super::{Presenter, Strings};

    /// A message identifier that a package list can hold.
    const MESSAGE: StringId = 1;

    /// A handle value that is not null.
    fn handle() -> HiiHandle {
        ptr::without_provenance_mut::<c_void>(0x1000)
    }

    /// What the presenter does when the adapter shows a popup.
    enum Answer {
        /// Presses these keys on the real popup, in order.
        Keys(Vec<Input>),
        /// Returns this answer without input.
        Fixed(PopupSelection),
        /// Returns this failure code.
        Fail(u64),
    }

    /// What the adapter asked, for the test that owns the context.
    #[derive(Default)]
    struct Journal {
        shown: Vec<(PopupStyle, PopupType, String)>,
        first: Vec<PopupSelection>,
        asked: Vec<(HiiHandle, StringId)>,
        inner: Vec<Status>,
    }

    /// A presenter that reads its script and records the request.
    struct Script {
        answer: Answer,
        journal: Rc<RefCell<Journal>>,
        reenter: Option<Rc<Cell<*const HiiPopupProtocol>>>,
    }

    impl Presenter for Script {
        fn present(&mut self, popup: &mut Popup) -> Result<PopupSelection, Error> {
            let seen = (popup.style, popup.kind, popup.message.clone());
            {
                let mut journal = self.journal.borrow_mut();
                journal.shown.push(seen);
                journal.first.push(popup.selected);
            }
            if let Some(cell) = self.reenter.take() {
                let table = cell.get();
                let mut inner = HiiPopupSelection::OK;
                // SAFETY: The test keeps the context and its table alive, and
                // the table address is the context address.
                let status = unsafe {
                    ((*table).create_popup)(
                        table,
                        HiiPopupStyle::INFO,
                        HiiPopupType::OK,
                        handle(),
                        MESSAGE,
                        &mut inner,
                    )
                };
                self.journal.borrow_mut().inner.push(status);
            }
            match &self.answer {
                Answer::Fixed(answer) => Ok(*answer),
                Answer::Fail(code) => Err(Error {
                    code: *code,
                    reason: "refused".to_string(),
                }),
                Answer::Keys(inputs) => {
                    let answer = inputs.iter().find_map(|input| popup.input(*input));
                    answer.ok_or_else(|| Error {
                        code: Status::ABORTED.0 as u64,
                        reason: "no answer".to_string(),
                    })
                }
            }
        }
    }

    /// A resolver that returns one fixed text and records the request.
    struct Messages {
        text: Option<&'static str>,
        journal: Rc<RefCell<Journal>>,
    }

    impl Strings for Messages {
        fn resolve(&mut self, handle: HiiHandle, message: StringId) -> Option<String> {
            self.journal.borrow_mut().asked.push((handle, message));
            self.text.map(String::from)
        }
    }

    /// A context with one scripted presenter and one resolver.
    fn context(
        answer: Answer,
        text: Option<&'static str>,
    ) -> (Box<Context<Script, Messages>>, Rc<RefCell<Journal>>) {
        let journal = Rc::new(RefCell::new(Journal::default()));
        let presenter = Script {
            answer,
            journal: Rc::clone(&journal),
            reenter: None,
        };
        let strings = Messages {
            text,
            journal: Rc::clone(&journal),
        };
        (Box::new(Context::new(presenter, strings)), journal)
    }

    /// Calls the installed callback the way firmware does.
    fn call<K: Presenter, S: Strings>(
        context: &Context<K, S>,
        style: HiiPopupStyle,
        kind: HiiPopupType,
        handle: HiiHandle,
        message: StringId,
        selection: *mut HiiPopupSelection,
    ) -> Status {
        let table: *const HiiPopupProtocol = &context.protocol;
        // SAFETY: The context outlives the call and its table is the first
        // field of the live context.
        unsafe { ((*table).create_popup)(table, style, kind, handle, message, selection) }
    }

    #[test]
    fn one_answer_popup_returns_ok() {
        let (context, journal) = context(Answer::Keys([Input::Enter].to_vec()), Some("Saved"));
        let mut out = HiiPopupSelection::CANCEL;
        let status = call(
            &context,
            HiiPopupStyle::WARNING,
            HiiPopupType::OK,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::SUCCESS);
        assert_eq!(out, HiiPopupSelection::OK);
        let journal = journal.borrow();
        assert_eq!(journal.shown.len(), 1);
        assert_eq!(journal.shown[0].0, PopupStyle::Warning);
        assert_eq!(journal.shown[0].1, PopupType::Ok);
        assert_eq!(journal.shown[0].2, "Saved");
        assert_eq!(journal.asked, [(handle(), MESSAGE)]);
    }

    #[test]
    fn right_then_enter_answers_cancel() {
        let keys = [Input::Right, Input::Enter].to_vec();
        let (context, journal) = context(Answer::Keys(keys), Some("Apply?"));
        let mut out = HiiPopupSelection::OK;
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK_CANCEL,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::SUCCESS);
        assert_eq!(out, HiiPopupSelection::CANCEL);
        assert_eq!(journal.borrow().first, [PopupSelection::Ok]);
    }

    #[test]
    fn a_letter_key_answers_directly() {
        let keys = [Input::Char('n')].to_vec();
        let (context, journal) = context(Answer::Keys(keys), Some("Erase?"));
        let mut out = HiiPopupSelection::YES;
        let status = call(
            &context,
            HiiPopupStyle::ERROR,
            HiiPopupType::YES_NO,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::SUCCESS);
        assert_eq!(out, HiiPopupSelection::NO);
        assert_eq!(journal.borrow().shown[0].0, PopupStyle::Error);
    }

    #[test]
    fn back_answers_cancel_only_where_cancel_is_offered() {
        let keys = [Input::Back].to_vec();
        let (context, _) = context(Answer::Keys(keys), Some("Later?"));
        let mut out = HiiPopupSelection::YES;
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::YES_NO_CANCEL,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::SUCCESS);
        assert_eq!(out, HiiPopupSelection::CANCEL);
    }

    #[test]
    fn an_answer_outside_the_type_is_refused() {
        let (context, journal) = context(Answer::Fixed(PopupSelection::Cancel), Some("Erase?"));
        let mut out = HiiPopupSelection(0xff);
        let status = call(
            &context,
            HiiPopupStyle::WARNING,
            HiiPopupType::YES_NO,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::INVALID_PARAMETER);
        assert_eq!(out, HiiPopupSelection(0xff));
        assert_eq!(journal.borrow().shown.len(), 1);
    }

    #[test]
    fn a_null_selection_output_is_allowed() {
        let keys = [Input::Enter].to_vec();
        let (context, journal) = context(Answer::Keys(keys), Some("Done"));
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK,
            handle(),
            MESSAGE,
            ptr::null_mut(),
        );
        assert_eq!(status, Status::SUCCESS);
        assert_eq!(journal.borrow().shown.len(), 1);
    }

    #[test]
    fn an_unknown_type_is_refused_before_the_display() {
        let (context, journal) = context(Answer::Fixed(PopupSelection::Ok), Some("Done"));
        let mut out = HiiPopupSelection::OK;
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType(4),
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::INVALID_PARAMETER);
        assert!(journal.borrow().shown.is_empty());
    }

    #[test]
    fn a_null_handle_or_an_empty_identifier_is_refused() {
        let (context, journal) = context(Answer::Fixed(PopupSelection::Ok), Some("Done"));
        let mut out = HiiPopupSelection::OK;
        let null_handle = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK,
            ptr::null_mut(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(null_handle, Status::INVALID_PARAMETER);
        let no_message = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK,
            handle(),
            0,
            &mut out,
        );
        assert_eq!(no_message, Status::INVALID_PARAMETER);
        let journal = journal.borrow();
        assert!(journal.asked.is_empty());
        assert!(journal.shown.is_empty());
    }

    #[test]
    fn an_unresolvable_message_is_refused_before_the_display() {
        let (context, journal) = context(Answer::Fixed(PopupSelection::Ok), None);
        let mut out = HiiPopupSelection::OK;
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::INVALID_PARAMETER);
        assert_eq!(journal.borrow().asked, [(handle(), MESSAGE)]);
        assert!(journal.borrow().shown.is_empty());
    }

    #[test]
    fn an_unknown_style_shows_information() {
        let (context, journal) = context(Answer::Fixed(PopupSelection::Ok), Some("Hello"));
        let mut out = HiiPopupSelection::CANCEL;
        let status = call(
            &context,
            HiiPopupStyle(9),
            HiiPopupType::OK,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::SUCCESS);
        assert_eq!(journal.borrow().shown[0].0, PopupStyle::Info);
    }

    #[test]
    fn every_type_and_style_maps_to_the_model() {
        let kinds = [
            (HiiPopupType::OK, PopupType::Ok),
            (HiiPopupType::OK_CANCEL, PopupType::OkCancel),
            (HiiPopupType::YES_NO, PopupType::YesNo),
            (HiiPopupType::YES_NO_CANCEL, PopupType::YesNoCancel),
        ];
        for (abi_kind, model_kind) in kinds {
            let first = model_kind.selections()[0];
            let (context, journal) = context(Answer::Fixed(first), Some("Message"));
            let mut out = HiiPopupSelection(0xff);
            let status = call(
                &context,
                HiiPopupStyle::INFO,
                abi_kind,
                handle(),
                MESSAGE,
                &mut out,
            );
            assert_eq!(status, Status::SUCCESS);
            assert_eq!(out, super::serve::selection_of(first));
            assert_eq!(journal.borrow().shown[0].1, model_kind);
        }
        let styles = [
            (HiiPopupStyle::INFO, PopupStyle::Info),
            (HiiPopupStyle::WARNING, PopupStyle::Warning),
            (HiiPopupStyle::ERROR, PopupStyle::Error),
        ];
        for (abi_style, model_style) in styles {
            let (context, journal) = context(Answer::Fixed(PopupSelection::Ok), Some("Text"));
            let mut out = HiiPopupSelection(0xff);
            let status = call(
                &context,
                abi_style,
                HiiPopupType::OK,
                handle(),
                MESSAGE,
                &mut out,
            );
            assert_eq!(status, Status::SUCCESS);
            assert_eq!(journal.borrow().shown[0].0, model_style);
        }
    }

    #[test]
    fn a_presenter_error_becomes_the_protocol_status() {
        let code = Status::DEVICE_ERROR.0 as u64;
        let (context, _) = context(Answer::Fail(code), Some("Message"));
        let mut out = HiiPopupSelection::OK;
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::DEVICE_ERROR);
    }

    #[test]
    fn a_presenter_error_without_the_error_bit_aborts() {
        let (context, _) = context(Answer::Fail(0), Some("Message"));
        let mut out = HiiPopupSelection::OK;
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::ABORTED);
    }

    #[test]
    fn a_nested_call_is_refused_and_the_outer_popup_still_answers() {
        let journal = Rc::new(RefCell::new(Journal::default()));
        let cell = Rc::new(Cell::new(ptr::null::<HiiPopupProtocol>()));
        let presenter = Script {
            answer: Answer::Keys([Input::Enter].to_vec()),
            journal: Rc::clone(&journal),
            reenter: Some(Rc::clone(&cell)),
        };
        let strings = Messages {
            text: Some("Outer"),
            journal: Rc::clone(&journal),
        };
        let context = Box::new(Context::new(presenter, strings));
        cell.set(&context.protocol);
        let mut out = HiiPopupSelection::CANCEL;
        let status = call(
            &context,
            HiiPopupStyle::INFO,
            HiiPopupType::OK,
            handle(),
            MESSAGE,
            &mut out,
        );
        assert_eq!(status, Status::SUCCESS);
        assert_eq!(out, HiiPopupSelection::OK);
        assert_eq!(journal.borrow().inner, [Status::ACCESS_DENIED]);
    }

    #[test]
    fn the_table_carries_the_specification_revision() {
        let (context, _) = context(Answer::Fixed(PopupSelection::Ok), Some("Text"));
        assert_eq!(context.protocol.revision, HiiPopupProtocol::REVISION);
    }

    #[test]
    fn a_null_protocol_table_is_refused() {
        // SAFETY: A null table is the documented refusal, and no context is
        // read.
        let status = unsafe {
            serve::create::<Script, Messages>(
                ptr::null(),
                HiiPopupStyle::INFO,
                HiiPopupType::OK,
                handle(),
                MESSAGE,
                ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::INVALID_PARAMETER);
    }

    #[test]
    fn the_abi_copy_matches_uefi_raw() {
        use core::mem::size_of;
        use hii_abi::popup as copy;

        assert_eq!(copy::GUID, HiiPopupProtocol::GUID.to_bytes());
        assert_eq!(copy::REVISION, HiiPopupProtocol::REVISION);
        let table = size_of::<copy::HiiPopupProtocol>();
        assert_eq!(table, size_of::<HiiPopupProtocol>());
        for (abi, model) in [
            (copy::PopupStyle::INFO.0, HiiPopupStyle::INFO.0),
            (copy::PopupStyle::WARNING.0, HiiPopupStyle::WARNING.0),
            (copy::PopupStyle::ERROR.0, HiiPopupStyle::ERROR.0),
            (copy::PopupType::OK.0, HiiPopupType::OK.0),
            (copy::PopupType::OK_CANCEL.0, HiiPopupType::OK_CANCEL.0),
            (copy::PopupType::YES_NO.0, HiiPopupType::YES_NO.0),
            (
                copy::PopupType::YES_NO_CANCEL.0,
                HiiPopupType::YES_NO_CANCEL.0,
            ),
            (copy::PopupSelection::OK.0, HiiPopupSelection::OK.0),
            (copy::PopupSelection::CANCEL.0, HiiPopupSelection::CANCEL.0),
            (copy::PopupSelection::YES.0, HiiPopupSelection::YES.0),
            (copy::PopupSelection::NO.0, HiiPopupSelection::NO.0),
        ] {
            assert_eq!(abi, model);
        }
    }
}
