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
//! `ask` is the consumer for a caller that has no form set. It shows the
//! modal through the presenter without a firmware call.

use alloc::string::String;

use forms::{Button, ButtonRole, Error, Modal, ModalStyle};

/// The available answers, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupKind {
    /// Acknowledge a message.
    Ok,
    /// Accept or cancel an operation.
    OkCancel,
    /// Answer a question.
    YesNo,
    /// Answer a question or cancel it.
    YesNoCancel,
}

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

/// The protocol table, the presenter and the resolver of one installation.
#[cfg(target_os = "uefi")]
mod serve {
    use core::cell::RefCell;

    use forms::ModalStyle;
    use uefi_raw::Status;
    use uefi_raw::protocol::hii::popup::{
        HiiPopupProtocol, HiiPopupSelection, HiiPopupStyle, HiiPopupType,
    };
    use uefi_raw::protocol::hii::{HiiHandle, StringId};

    use super::{PopupKind, Presenter, Strings, answer_of, modal_of};

    /// One installed protocol table with its two services.
    #[repr(C)]
    pub(super) struct Context<P: Presenter, S: Strings> {
        pub(super) protocol: HiiPopupProtocol,
        presenter: RefCell<P>,
        strings: RefCell<S>,
        /// The device path of a handle that this context creates.
        #[cfg(target_os = "uefi")]
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
                #[cfg(target_os = "uefi")]
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
            let mut modal = modal_of(style_of(style), kind, &text);
            let role = {
                let Ok(mut presenter) = self.presenter.try_borrow_mut() else {
                    return Err(Status::ACCESS_DENIED);
                };
                match presenter.present(&mut modal) {
                    Ok(role) => role,
                    Err(error) => {
                        let status = Status(error.code as usize);
                        return Err(if status.is_error() {
                            status
                        } else {
                            Status::ABORTED
                        });
                    }
                }
            };
            let answer = answer_of(kind, role).ok_or(Status::INVALID_PARAMETER)?;
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

    /// Maps the ABI style to the model style.
    pub(super) fn style_of(style: HiiPopupStyle) -> ModalStyle {
        if style == HiiPopupStyle::WARNING {
            ModalStyle::Warning
        } else if style == HiiPopupStyle::ERROR {
            ModalStyle::Error
        } else {
            ModalStyle::Info
        }
    }

    /// Maps the ABI type to the model kind.
    pub(super) fn kind_of(kind: HiiPopupType) -> Option<PopupKind> {
        if kind == HiiPopupType::OK {
            Some(PopupKind::Ok)
        } else if kind == HiiPopupType::OK_CANCEL {
            Some(PopupKind::OkCancel)
        } else if kind == HiiPopupType::YES_NO {
            Some(PopupKind::YesNo)
        } else if kind == HiiPopupType::YES_NO_CANCEL {
            Some(PopupKind::YesNoCancel)
        } else {
            None
        }
    }

    /// Maps an answer index to the ABI answer.
    pub(super) fn selection_of(kind: PopupKind, index: usize) -> HiiPopupSelection {
        match (kind, index) {
            (PopupKind::Ok | PopupKind::OkCancel, 0) => HiiPopupSelection::OK,
            (PopupKind::OkCancel, 1) => HiiPopupSelection::CANCEL,
            (PopupKind::YesNo | PopupKind::YesNoCancel, 0) => HiiPopupSelection::YES,
            (PopupKind::YesNo | PopupKind::YesNoCancel, 1) => HiiPopupSelection::NO,
            _ => HiiPopupSelection::CANCEL,
        }
    }

    /// Builds the vendor device path of a created handle.
    #[cfg(target_os = "uefi")]
    fn vendor_path() -> [u8; 24] {
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

    use forms::{Error, ModalStyle};
    use uefi_raw::protocol::device_path::DevicePathProtocol;
    use uefi_raw::protocol::hii::popup::{HiiPopupProtocol, HiiPopupSelection};
    use uefi_raw::protocol::hii::string::HiiStringProtocol;
    use uefi_raw::protocol::hii::{HiiHandle, StringId};
    use uefi_raw::table::boot::InterfaceType;
    use uefi_raw::{Handle, Status};

    use super::serve::{Context, create};
    use super::{PopupKind, Presenter, Strings};
    use crate::raw::Boot;

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
            let _ = boot;
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
            let mut language = Vec::from(b"eng\0" as &[u8]);
            let mut size = 0usize;
            // SAFETY: The protocol belongs to the firmware.
            let status = unsafe {
                // SAFETY: The table is live.
                let protocol = self.protocol.as_ref();
                (protocol.get_languages)(self.protocol.as_ptr(), handle, ptr::null_mut(), &mut size)
            };
            if status == Status::BUFFER_TOO_SMALL && size > 0 {
                let mut buffer = alloc::vec![0u8; size];
                let mut length = size;
                // SAFETY: The buffer holds `size` bytes.
                let status = unsafe {
                    let protocol = self.protocol.as_ref();
                    (protocol.get_languages)(
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

    impl Strings for HiiStrings {
        fn resolve(&mut self, handle: HiiHandle, message: StringId) -> Option<String> {
            let language = self.language(handle);
            let mut token = message;
            let mut buffer = alloc::vec![0u16; 256];
            let mut length = buffer.len();
            // SAFETY: The buffers are writable and the language is NUL
            // terminated.
            let status = unsafe {
                let protocol = self.protocol.as_ref();
                (protocol.get_string)(
                    self.protocol.as_ptr(),
                    language.as_ptr(),
                    handle,
                    &mut token,
                    &mut buffer,
                    &mut length,
                )
            };
            if status.is_error() {
                return None;
            }
            Some(crate::text::from_ucs2(buffer.as_ptr()))
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

    /// Shows one message through a presenter.
    ///
    /// The call needs no firmware because the caller supplies the text.
    ///
    /// # Errors
    ///
    /// Returns the presenter failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hii_uefi::popup::ask;
    /// # use forms::{ButtonRole, Error, Modal};
    /// # struct Accept;
    /// # impl hii_uefi::popup::Presenter for Accept {
    /// #     fn present(&mut self, _modal: &mut Modal) -> Result<ButtonRole, Error> { Ok(ButtonRole::Default) }
    /// # }
    /// # fn example() -> Result<(), Error> {
    /// let mut presenter = Accept;
    /// let answer = ask(&mut presenter, forms::ModalStyle::Info, "The write finished.")?;
    /// assert_eq!(answer, ButtonRole::Default);
    /// # Ok(())
    /// # }
    /// ```
    pub fn ask<P: Presenter>(
        presenter: &mut P,
        style: ModalStyle,
        message: &str,
    ) -> Result<forms::ButtonRole, Error> {
        let mut modal = super::modal_of(style, PopupKind::Ok, message);
        presenter.present(&mut modal)
    }

    /// Aligns a pointer upwards.
    fn align_pointer(pointer: *mut u8, align: usize) -> *mut u8 {
        let address = pointer as usize;
        let mask = align - 1;
        (address + mask) & !mask
    }
}
