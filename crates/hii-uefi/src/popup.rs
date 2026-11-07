//! The HII popup protocol over the native modal.
//!
//! Firmware publishes `EFI_HII_POPUP_PROTOCOL` so that a driver callback can
//! ask the setup browser for one decision. `Registration` installs that
//! protocol. Each `CreatePopup` call resolves the message, builds a
//! [`forms::Modal`] and asks the [`Presenter`] of the front end.
//!
//! The crate holds no display code. A front end supplies the [`Presenter`],
//! because display and input belong to the front end.
//!
//! A warning or an error question selects No first, so one Enter refuses it.
//! A modal with a Cancel button selects Cancel first.
//!
//! `ask` is the consumer for a caller that has no form set. It publishes its
//! own message package list, calls `CreatePopup` of the firmware browser and
//! removes the list again, as before.

use alloc::string::String;

use forms::{Button, ButtonRole, Error, Modal, ModalStyle};

pub use crate::map::PopupKind;

/// Shows a modal and returns the answer that the person gave.
///
/// # Examples
///
/// ```
/// use forms::{ButtonRole, Error, Modal};
/// use hii_uefi::popup::Presenter;
///
/// /// Accepts the highlighted button without a display.
/// struct Accept;
///
/// impl Presenter for Accept {
///     fn present(&mut self, modal: &mut Modal) -> Result<ButtonRole, Error> {
///         Ok(modal.buttons[modal.selected].role)
///     }
/// }
/// ```
pub trait Presenter {
    /// Shows `modal` and returns the role of the chosen button.
    ///
    /// # Errors
    ///
    /// Returns the reason when the modal cannot be shown.
    fn present(&mut self, modal: &mut Modal) -> Result<ButtonRole, Error>;
}

impl<P: Presenter + ?Sized> Presenter for alloc::boxed::Box<P> {
    fn present(&mut self, modal: &mut Modal) -> Result<ButtonRole, Error> {
        (**self).present(modal)
    }
}

/// Resolves the message text of a popup.
pub trait Strings {
    /// Returns the message text of `message` in the package list of `handle`.
    fn resolve(
        &mut self,
        handle: uefi_raw::protocol::hii::HiiHandle,
        message: u16,
    ) -> Option<String>;
}

impl<S: Strings + ?Sized> Strings for alloc::boxed::Box<S> {
    fn resolve(
        &mut self,
        handle: uefi_raw::protocol::hii::HiiHandle,
        message: u16,
    ) -> Option<String> {
        (**self).resolve(handle, message)
    }
}

/// Builds the modal of one popup request.
///
/// The function is pure: a front end and a test can call it without firmware.
///
/// # Examples
///
/// ```
/// use forms::{ButtonRole, ModalStyle};
/// use hii_uefi::popup::{PopupKind, modal_of};
///
/// let modal = modal_of(ModalStyle::Warning, PopupKind::YesNo, "Erase?");
/// assert_eq!(modal.buttons[modal.selected].role, ButtonRole::Other(0));
/// assert_eq!(modal.buttons[1].label, "No");
/// ```
pub fn modal_of(style: ModalStyle, kind: PopupKind, message: &str) -> Modal {
    let mut modal = Modal::new(style, "Message", message);
    match kind {
        PopupKind::Ok => {
            modal.push(Button::new("OK", ButtonRole::Default));
        }
        PopupKind::OkCancel => {
            modal.push(Button::new("OK", ButtonRole::Default));
            modal.push(Button::new("Cancel", ButtonRole::Cancel));
        }
        PopupKind::YesNo => {
            modal.push(Button::new("Yes", ButtonRole::Default));
            modal.push(Button::new("No", ButtonRole::Other(0)));
        }
        PopupKind::YesNoCancel => {
            modal.push(Button::new("Yes", ButtonRole::Default));
            modal.push(Button::new("No", ButtonRole::Other(0)));
            modal.push(Button::new("Cancel", ButtonRole::Cancel));
        }
    }
    if matches!(kind, PopupKind::YesNo | PopupKind::YesNoCancel)
        && !matches!(style, ModalStyle::Info)
    {
        // A warning or an error selects No, as the reference browser does.
        modal.selected = 1;
    }
    modal
}

/// Returns the answer that a role means for a popup kind.
///
/// # Examples
///
/// ```
/// use forms::ButtonRole;
/// use hii_uefi::popup::{PopupKind, answer_of};
///
/// assert_eq!(answer_of(PopupKind::YesNoCancel, ButtonRole::Cancel), Some(2));
/// assert_eq!(answer_of(PopupKind::YesNoCancel, ButtonRole::Other(0)), Some(1));
/// ```
pub fn answer_of(kind: PopupKind, role: ButtonRole) -> Option<usize> {
    match (kind, role) {
        (PopupKind::Ok | PopupKind::OkCancel, ButtonRole::Default) => Some(0),
        (PopupKind::OkCancel, ButtonRole::Cancel) => Some(1),
        (PopupKind::YesNo | PopupKind::YesNoCancel, ButtonRole::Default) => Some(0),
        (PopupKind::YesNo | PopupKind::YesNoCancel, ButtonRole::Other(0)) => Some(1),
        (PopupKind::YesNoCancel, ButtonRole::Cancel) => Some(2),
        _ => None,
    }
}

/// Shows one popup and returns its answer index.
///
/// This is the firmware-free decision path: the caller supplies the kind, the
/// style, the message and the presenter. The answer is checked against the
/// kind, so a presenter cannot invent an answer.
///
/// # Errors
///
/// Returns an error when the message is empty, when the presenter fails, or
/// when the presenter answers a role that the kind does not offer. The code of
/// such an error is [`INVALID_ANSWER`].
///
/// # Examples
///
/// ```
/// use forms::{ButtonRole, Error, Modal, ModalStyle};
/// use hii_uefi::popup::{PopupKind, Presenter, decide};
///
/// struct Refuse;
///
/// impl Presenter for Refuse {
///     fn present(&mut self, modal: &mut Modal) -> Result<ButtonRole, Error> {
///         Ok(modal.buttons[modal.selected].role)
///     }
/// }
///
/// let answer = decide(PopupKind::OkCancel, ModalStyle::Info, "Continue?", &mut Refuse);
/// assert_eq!(answer, Ok(1));
/// ```
pub fn decide(
    kind: PopupKind,
    style: ModalStyle,
    message: &str,
    presenter: &mut impl Presenter,
) -> Result<usize, Error> {
    if message.is_empty() {
        return Err(Error {
            code: INVALID_ANSWER,
            reason: String::from("The popup has no message"),
        });
    }
    let mut modal = modal_of(style, kind, message);
    let role = presenter.present(&mut modal)?;
    answer_of(kind, role).ok_or_else(|| Error {
        code: INVALID_ANSWER,
        reason: String::from("The answer is not one that the popup offers"),
    })
}

/// The error code of an answer that a popup does not offer.
pub const INVALID_ANSWER: u64 = 2;

/// The protocol table, the presenter and the resolver of one installation.
#[cfg(target_os = "uefi")]
mod serve {
    use core::cell::RefCell;

    use uefi_raw::Status;
    use uefi_raw::protocol::hii::popup::{
        HiiPopupProtocol, HiiPopupSelection, HiiPopupStyle, HiiPopupType,
    };
    use uefi_raw::protocol::hii::{HiiHandle, StringId};

    use crate::map::{kind_of, selection_of, style_of};

    use super::{Presenter, Strings, decide};

    /// One installed protocol table with its two services.
    #[repr(C)]
    pub(super) struct Context<P: Presenter, S: Strings> {
        pub(super) protocol: HiiPopupProtocol,
        presenter: RefCell<P>,
        strings: RefCell<S>,
        /// The device path of a handle that this context creates.
        pub(super) path: [u8; 24],
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
                path: vendor_path(),
            }
        }

        /// Resolves the message, shows the modal and checks the answer.
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
            let mut presenter = self
                .presenter
                .try_borrow_mut()
                .map_err(|_| Status::ACCESS_DENIED)?;
            let answer =
                decide(kind, style_of(style), &text, &mut *presenter).map_err(|error| {
                    let status = Status(error.code as usize);
                    if status.is_error() {
                        status
                    } else {
                        Status::ABORTED
                    }
                })?;
            Ok(selection_of(kind, answer))
        }
    }

    /// `EFI_HII_CREATE_POPUP`.
    ///
    /// # Safety
    ///
    /// `this` must point at the `protocol` field of a live `Context<P, S>`.
    /// `selection` is null or writable.
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

    /// Builds the vendor device path of a created handle.
    pub(super) fn vendor_path() -> [u8; 24] {
        let mut path = [0u8; 24];
        path[..4].copy_from_slice(&[1, 4, 20, 0]);
        path[4..20].copy_from_slice(&HiiPopupProtocol::GUID.to_bytes());
        path[20..].copy_from_slice(&[0x7f, 0xff, 4, 0]);
        path
    }
}

#[cfg(target_os = "uefi")]
pub use firmware::{HiiStrings, Registration, ask};

/// The firmware side of the popup protocol.
#[cfg(target_os = "uefi")]
mod firmware {
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::mem::{align_of, size_of};
    use core::ptr::{self, NonNull};

    use forms::{ButtonRole, Error, ModalStyle};
    use uefi_raw::protocol::device_path::DevicePathProtocol;
    use uefi_raw::protocol::hii::database::HiiDatabaseProtocol;
    use uefi_raw::protocol::hii::font::FontInfo;
    use uefi_raw::protocol::hii::popup::{HiiPopupProtocol, HiiPopupSelection, HiiPopupType};
    use uefi_raw::protocol::hii::string::HiiStringProtocol;
    use uefi_raw::protocol::hii::{HiiHandle, StringId};
    use uefi_raw::table::boot::InterfaceType;
    use uefi_raw::{Guid, Handle, Status};

    use super::serve::{Context, vendor_path};
    use super::{Presenter, Strings};
    use crate::raw::Boot;

    /// The language that the resolver uses when a package list reports none.
    const DEFAULT_LANGUAGE: &[u8] = b"eng\0";

    /// The package-list GUID of the message that `ask` publishes.
    const MESSAGE_LIST: Guid = uefi_raw::guid!("3d1b8a7c-5e42-4a91-8c0d-6f2e5b4a9c11");

    /// The string that holds the message. String 1 names the language.
    const MESSAGE_ID: StringId = 2;

    /// Resolves messages through `EFI_HII_STRING_PROTOCOL`.
    pub struct HiiStrings {
        protocol: NonNull<HiiStringProtocol>,
        languages: alloc::collections::BTreeMap<usize, Vec<u8>>,
    }

    impl HiiStrings {
        /// Opens the string protocol.
        ///
        /// # Errors
        ///
        /// Returns `EFI_NOT_FOUND` when the firmware publishes no string
        /// protocol.
        pub fn new(boot: Boot) -> Result<Self, Status> {
            let protocol = boot
                .locate::<HiiStringProtocol>(&HiiStringProtocol::GUID)
                .ok_or(Status::NOT_FOUND)?;
            Ok(Self {
                protocol,
                languages: alloc::collections::BTreeMap::new(),
            })
        }

        /// Returns the language of a package list.
        fn language(&mut self, handle: HiiHandle) -> Vec<u8> {
            let key = handle as usize;
            if let Some(language) = self.languages.get(&key) {
                return language.clone();
            }
            let mut language = Vec::from(DEFAULT_LANGUAGE);
            let mut size = 0usize;
            // SAFETY: The protocol belongs to the firmware.
            let status = unsafe {
                (self.protocol.as_ref().get_languages)(
                    self.protocol.as_ptr(),
                    handle,
                    ptr::null_mut(),
                    &mut size,
                )
            };
            if status == Status::BUFFER_TOO_SMALL && size > 0 {
                let mut buffer = alloc::vec![0u8; size];
                let mut length = size;
                // SAFETY: The buffer holds `size` bytes.
                let status = unsafe {
                    (self.protocol.as_ref().get_languages)(
                        self.protocol.as_ptr(),
                        handle,
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
            self.languages.insert(key, language.clone());
            language
        }
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    impl Strings for HiiStrings {
        fn resolve(&mut self, handle: HiiHandle, message: StringId) -> Option<String> {
            let language = self.language(handle);
            let mut buffer = alloc::vec![0u16; 256];
            let mut length = buffer.len();
            let mut font: *mut FontInfo = ptr::null_mut();
            // SAFETY: The buffers are writable and the language is NUL
            // terminated.
            let status = unsafe {
                (self.protocol.as_ref().get_string)(
                    self.protocol.as_ptr(),
                    language.as_ptr(),
                    handle,
                    message,
                    buffer.as_mut_ptr(),
                    &mut length,
                    &mut font,
                )
            };
            if status.is_error() {
                return None;
            }
            // SAFETY: The firmware filled the buffer with a NUL-terminated string.
            Some(unsafe { crate::text::from_ucs2(buffer.as_ptr()) })
        }
    }

    /// One installed `EFI_HII_POPUP_PROTOCOL`.
    pub struct Registration<P: Presenter + 'static, S: Strings + 'static> {
        boot: Boot,
        base: NonNull<u8>,
        context: NonNull<Context<P, S>>,
        handle: Handle,
        installed: bool,
    }

    impl<P: Presenter + 'static, S: Strings + 'static> Registration<P, S> {
        /// Installs the protocol on a new handle.
        ///
        /// # Safety
        ///
        /// The caller must keep the driver image resident until
        /// [`Self::remove`] succeeds, and boot services must stay valid.
        ///
        /// # Errors
        ///
        /// Returns the firmware status of a failed step.
        pub unsafe fn install(boot: Boot, presenter: P, strings: S) -> Result<Self, Status> {
            let size = size_of::<Context<P, S>>();
            let align = align_of::<Context<P, S>>();
            // SAFETY: The boot services table must be live for this call.
            let base = boot
                .allocate(size + align)
                .ok_or(Status::OUT_OF_RESOURCES)?;
            let address = align_pointer(base.as_ptr(), align);
            let context =
                NonNull::new(address.cast::<Context<P, S>>()).ok_or(Status::OUT_OF_RESOURCES)?;
            // SAFETY: The allocation keeps `size` bytes at the aligned address.
            unsafe {
                context.as_ptr().write(Context::new(presenter, strings));
            }
            let mut registration = Self {
                boot,
                base,
                context,
                handle: ptr::null_mut(),
                installed: false,
            };
            // SAFETY: The context is live and its `protocol` field is first.
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
                // SAFETY: No interface points at the context yet.
                unsafe {
                    ptr::drop_in_place(context.as_ptr());
                    boot.free(base);
                }
                return Err(status);
            }
            registration.installed = true;
            Ok(registration)
        }

        /// Removes the protocol and frees the context.
        ///
        /// # Errors
        ///
        /// Returns the first firmware refusal. A refused removal keeps the
        /// context, because firmware still holds its pointer.
        pub fn remove(&mut self) -> Result<(), Status> {
            if !self.installed {
                return Ok(());
            }
            // SAFETY: This interface was installed on this handle.
            let status = unsafe {
                (self.boot.services().uninstall_protocol_interface)(
                    self.handle,
                    &HiiPopupProtocol::GUID,
                    self.context.as_ptr().cast(),
                )
            };
            if status.is_error() {
                return Err(status);
            }
            self.installed = false;
            // SAFETY: The interface is removed.
            unsafe {
                ptr::drop_in_place(self.context.as_ptr());
                self.boot.free(self.base);
            }
            Ok(())
        }
    }

    impl<P: Presenter + 'static, S: Strings + 'static> Drop for Registration<P, S> {
        fn drop(&mut self) {
            let _ = self.remove();
        }
    }

    /// Shows one message through the firmware browser.
    ///
    /// The call publishes a package list that holds the message, calls
    /// `CreatePopup` of the installed browser and removes the list again, so a
    /// caller with no form set reaches the browser of the firmware.
    ///
    /// # Errors
    ///
    /// Returns the firmware status of a failed step as a forms error.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hii_uefi::{popup::ask, raw::Boot};
    /// # fn example(boot: Boot) {
    /// let answer = ask(boot, forms::ModalStyle::Info, "The write finished.");
    /// let _ = answer;
    /// # }
    /// ```
    pub fn ask(boot: Boot, style: ModalStyle, message: &str) -> Result<ButtonRole, Error> {
        let database = boot
            .locate::<HiiDatabaseProtocol>(&HiiDatabaseProtocol::GUID)
            .ok_or_else(|| error(Status::NOT_FOUND))?;
        let protocol = boot
            .locate::<HiiPopupProtocol>(&HiiPopupProtocol::GUID)
            .ok_or_else(|| error(Status::NOT_FOUND))?;
        let packages = hii_ifr::encode_message(message, MESSAGE_LIST.to_bytes())
            .map_err(|_| error(Status::INVALID_PARAMETER))?;

        let mut path = vendor_path();
        let mut handle: Handle = ptr::null_mut();
        // SAFETY: The path lives on the stack for this call and the firmware
        // copies it.
        let status = unsafe {
            (boot.services().install_protocol_interface)(
                &mut handle,
                &DevicePathProtocol::GUID,
                InterfaceType::NATIVE_INTERFACE,
                path.as_mut_ptr().cast(),
            )
        };
        if status.is_error() {
            return Err(error(status));
        }
        let mut package: HiiHandle = ptr::null_mut();
        // SAFETY: The codec produced a complete package list.
        let status = unsafe {
            (database.as_ref().new_package_list)(
                database.as_ptr(),
                packages.as_ptr().cast(),
                handle,
                &mut package,
            )
        };
        if status.is_error() {
            remove_path(&boot, handle, path.as_mut_ptr().cast());
            return Err(error(status));
        }
        let mut selection = HiiPopupSelection::CANCEL;
        // SAFETY: The protocol belongs to the firmware and the answer pointer
        // is writable.
        let status = unsafe {
            (protocol.as_ref().create_popup)(
                protocol.as_ptr(),
                abi_style(style),
                HiiPopupType::OK,
                package,
                MESSAGE_ID,
                &mut selection,
            )
        };
        // SAFETY: The list was installed on this handle.
        unsafe {
            let _ = (database.as_ref().remove_package_list)(database.as_ptr(), package);
        }
        remove_path(&boot, handle, path.as_mut_ptr().cast());
        if status.is_error() {
            return Err(error(status));
        }
        Ok(role_of(selection))
    }

    /// Maps a firmware style to the ABI style of this call.
    fn abi_style(style: ModalStyle) -> uefi_raw::protocol::hii::popup::HiiPopupStyle {
        match style {
            ModalStyle::Warning => uefi_raw::protocol::hii::popup::HiiPopupStyle::WARNING,
            ModalStyle::Error | ModalStyle::Danger => {
                uefi_raw::protocol::hii::popup::HiiPopupStyle::ERROR
            }
            ModalStyle::Info => uefi_raw::protocol::hii::popup::HiiPopupStyle::INFO,
        }
    }

    /// Maps a firmware answer to the role of a button.
    fn role_of(selection: HiiPopupSelection) -> ButtonRole {
        if selection == HiiPopupSelection::CANCEL {
            ButtonRole::Cancel
        } else if selection == HiiPopupSelection::NO {
            ButtonRole::Other(0)
        } else {
            ButtonRole::Default
        }
    }

    /// Removes the vendor device path of a created handle.
    fn remove_path(boot: &Boot, handle: Handle, path: *mut core::ffi::c_void) {
        // SAFETY: This path was installed on this handle.
        let _ = unsafe {
            (boot.services().uninstall_protocol_interface)(handle, &DevicePathProtocol::GUID, path)
        };
    }

    /// Builds a forms error from a firmware status.
    fn error(status: Status) -> Error {
        Error {
            code: status.0 as u64,
            reason: String::from("the HII popup protocol failed"),
        }
    }

    /// Aligns a pointer upwards.
    fn align_pointer(pointer: *mut u8, align: usize) -> *mut u8 {
        let address = pointer as usize;
        let mask = align - 1;
        ((address + mask) & !mask) as *mut u8
    }
}

#[cfg(test)]
mod tests {
    use super::{INVALID_ANSWER, PopupKind, Presenter, answer_of, decide, modal_of};
    use crate::map::{kind_of, map_request, request_value, selection_of, style_of};
    use alloc::string::String;
    use forms::{ButtonRole, Error, Key, Modal, ModalStyle, Request, Scope};
    use uefi_raw::protocol::hii::popup::{HiiPopupSelection, HiiPopupStyle, HiiPopupType};

    /// Accepts the highlighted button.
    struct Accept;

    impl Presenter for Accept {
        fn present(&mut self, modal: &mut Modal) -> Result<ButtonRole, Error> {
            Ok(modal.buttons[modal.selected].role)
        }
    }

    /// Answers a role that no popup offers.
    struct Invent;

    impl Presenter for Invent {
        fn present(&mut self, _modal: &mut Modal) -> Result<ButtonRole, Error> {
            Ok(ButtonRole::Other(9))
        }
    }

    /// Cannot show anything.
    struct Blind;

    impl Presenter for Blind {
        fn present(&mut self, _modal: &mut Modal) -> Result<ButtonRole, Error> {
            Err(Error::new("no display"))
        }
    }

    #[test]
    fn a_warning_question_selects_no_first() {
        let modal = modal_of(ModalStyle::Warning, PopupKind::YesNo, "Erase?");
        assert_eq!(modal.buttons[modal.selected].label, "No");
        let modal = modal_of(ModalStyle::Error, PopupKind::YesNoCancel, "Fail?");
        assert_eq!(modal.buttons[modal.selected].label, "No");
        let modal = modal_of(ModalStyle::Info, PopupKind::YesNo, "Continue?");
        assert_eq!(modal.buttons[modal.selected].label, "Yes");
    }

    #[test]
    fn a_cancel_button_is_the_safe_default() {
        let modal = modal_of(ModalStyle::Danger, PopupKind::OkCancel, "Apply?");
        assert_eq!(modal.buttons[modal.selected].role, ButtonRole::Cancel);
        let modal = modal_of(ModalStyle::Info, PopupKind::YesNoCancel, "Apply?");
        assert_eq!(modal.buttons[modal.selected].role, ButtonRole::Cancel);
        let modal = modal_of(ModalStyle::Info, PopupKind::Ok, "Done");
        assert_eq!(modal.buttons[modal.selected].role, ButtonRole::Default);
    }

    #[test]
    fn one_enter_refuses_a_warning() {
        assert_eq!(
            decide(PopupKind::YesNo, ModalStyle::Warning, "Erase?", &mut Accept),
            Ok(1)
        );
        assert_eq!(
            decide(PopupKind::OkCancel, ModalStyle::Info, "Apply?", &mut Accept),
            Ok(1)
        );
    }

    #[test]
    fn an_answer_outside_the_kind_is_refused() {
        assert_eq!(
            decide(PopupKind::Ok, ModalStyle::Info, "Done", &mut Invent),
            Err(Error {
                code: INVALID_ANSWER,
                reason: String::from("The answer is not one that the popup offers")
            })
        );
        assert_eq!(
            decide(PopupKind::Ok, ModalStyle::Info, "", &mut Accept),
            Err(Error {
                code: INVALID_ANSWER,
                reason: String::from("The popup has no message")
            })
        );
    }

    #[test]
    fn a_presenter_failure_is_reported() {
        let answer = decide(PopupKind::Ok, ModalStyle::Info, "Done", &mut Blind);
        assert!(answer.is_err());
    }

    #[test]
    fn every_style_and_kind_has_one_mapping() {
        assert_eq!(style_of(HiiPopupStyle::INFO), ModalStyle::Info);
        assert_eq!(style_of(HiiPopupStyle::WARNING), ModalStyle::Warning);
        assert_eq!(style_of(HiiPopupStyle::ERROR), ModalStyle::Error);
        assert_eq!(style_of(HiiPopupStyle(9)), ModalStyle::Info);
        assert_eq!(kind_of(HiiPopupType::OK), Some(PopupKind::Ok));
        assert_eq!(kind_of(HiiPopupType::OK_CANCEL), Some(PopupKind::OkCancel));
        assert_eq!(kind_of(HiiPopupType::YES_NO), Some(PopupKind::YesNo));
        assert_eq!(
            kind_of(HiiPopupType::YES_NO_CANCEL),
            Some(PopupKind::YesNoCancel)
        );
        assert_eq!(kind_of(HiiPopupType(9)), None);
    }

    #[test]
    fn every_answer_has_one_selection() {
        assert_eq!(selection_of(PopupKind::Ok, 0), HiiPopupSelection::OK);
        assert_eq!(
            selection_of(PopupKind::OkCancel, 1),
            HiiPopupSelection::CANCEL
        );
        assert_eq!(selection_of(PopupKind::YesNo, 1), HiiPopupSelection::NO);
        assert_eq!(
            selection_of(PopupKind::YesNoCancel, 0),
            HiiPopupSelection::YES
        );
        assert_eq!(
            selection_of(PopupKind::YesNoCancel, 2),
            HiiPopupSelection::CANCEL
        );
        assert_eq!(
            answer_of(PopupKind::YesNoCancel, ButtonRole::Cancel),
            Some(2)
        );
    }

    #[test]
    fn requests_round_trip_through_the_firmware_values() {
        let page = Key(4);
        for (value, request) in [
            (0u64, Request::None),
            (1, Request::Restart),
            (
                2,
                Request::Apply {
                    scope: Scope::Set,
                    leave: true,
                },
            ),
            (
                3,
                Request::Discard {
                    scope: Scope::Set,
                    leave: true,
                },
            ),
            (
                4,
                Request::Apply {
                    scope: Scope::Page(page),
                    leave: true,
                },
            ),
            (
                5,
                Request::Discard {
                    scope: Scope::Page(page),
                    leave: true,
                },
            ),
            (
                6,
                Request::Apply {
                    scope: Scope::Page(page),
                    leave: false,
                },
            ),
            (
                7,
                Request::Discard {
                    scope: Scope::Page(page),
                    leave: false,
                },
            ),
            (8, Request::Reconnect),
            (
                9,
                Request::Apply {
                    scope: Scope::Row(page),
                    leave: false,
                },
            ),
        ] {
            assert_eq!(map_request(value, page), Ok(request));
            assert_eq!(request_value(request), value);
        }
        assert!(map_request(42, page).is_err());
    }
}
