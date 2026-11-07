//! The mapping between firmware HII values and forms values.

use alloc::format;

use forms::{Error, Key, Request, Scope};
use hii_ifr::QuestionKind;
use uefi_raw::Status;
use uefi_raw::protocol::hii::popup::{HiiPopupSelection, HiiPopupStyle, HiiPopupType};

use forms::ModalStyle;

/// The available answers of one popup, in display order.
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

/// Maps a firmware action request to a forms request.
///
/// The ten specification values keep their meaning. A driver that writes an
/// undefined value breaks the protocol, so it becomes an error.
///
/// # Errors
///
/// Returns a device error for a value that the specification does not define.
///
/// # Examples
///
/// ```
/// use forms::{Key, Request, Scope};
///
/// assert_eq!(hii_uefi::map_request(0, Key(1)), Ok(Request::None));
/// assert_eq!(
///     hii_uefi::map_request(6, Key(1)),
///     Ok(Request::Apply {
///         scope: Scope::Page(Key(1)),
///         leave: false
///     })
/// );
/// assert!(hii_uefi::map_request(42, Key(1)).is_err());
/// ```
pub fn map_request(value: u64, page: Key) -> Result<Request, Error> {
    Ok(match value {
        0 => Request::None,
        1 => Request::Restart,
        2 => Request::Apply {
            scope: Scope::Set,
            leave: true,
        },
        3 => Request::Discard {
            scope: Scope::Set,
            leave: true,
        },
        4 => Request::Apply {
            scope: Scope::Page(page),
            leave: true,
        },
        5 => Request::Discard {
            scope: Scope::Page(page),
            leave: true,
        },
        6 => Request::Apply {
            scope: Scope::Page(page),
            leave: false,
        },
        7 => Request::Discard {
            scope: Scope::Page(page),
            leave: false,
        },
        8 => Request::Reconnect,
        9 => Request::Apply {
            scope: Scope::Row(page),
            leave: false,
        },
        other => {
            return Err(Error {
                code: Status::DEVICE_ERROR.0 as u64,
                reason: format!("the driver returned action request 0x{other:x}"),
            });
        }
    })
}

/// Returns the firmware value of a forms request.
///
/// This is the inverse of [`map_request`] for the ten specification values.
/// A request that the specification cannot express becomes `NONE`, so a
/// browser never acts on an invented number.
///
/// # Examples
///
/// ```
/// use forms::{Request, Scope};
///
/// assert_eq!(hii_uefi::request_value(Request::None), 0);
/// assert_eq!(hii_uefi::request_value(Request::Restart), 1);
/// assert_eq!(hii_uefi::request_value(Request::Reconnect), 8);
/// assert_eq!(
///     hii_uefi::request_value(Request::Apply {
///         scope: Scope::Set,
///         leave: true
///     }),
///     2
/// );
/// ```
pub fn request_value(request: Request) -> u64 {
    match request {
        Request::None => 0,
        Request::Restart => 1,
        Request::Reconnect => 8,
        Request::Apply { scope, leave } => match (scope, leave) {
            (Scope::Set, true) => 2,
            (Scope::Set, false) => 6,
            (Scope::Page(_), true) => 4,
            (Scope::Page(_), false) => 6,
            (Scope::Row(_), _) => 9,
        },
        Request::Discard { scope, leave } => match (scope, leave) {
            (Scope::Set, true) => 3,
            (Scope::Set, false) => 7,
            (Scope::Page(_), true) => 5,
            (Scope::Page(_), false) => 7,
            (Scope::Row(_), _) => 7,
        },
    }
}

/// Returns the IFR value type of a question kind.
///
/// # Examples
///
/// ```
/// use hii_ifr::QuestionKind;
///
/// assert_eq!(
///     hii_uefi::ifr_type(QuestionKind::Bool),
///     hii_abi::value_type::BOOLEAN
/// );
/// ```
pub fn ifr_type(kind: QuestionKind) -> u8 {
    match kind {
        QuestionKind::Bool => hii_abi::value_type::BOOLEAN,
        QuestionKind::Text { .. } | QuestionKind::Password { .. } => hii_abi::value_type::STRING,
        QuestionKind::OrderedList => hii_abi::value_type::BUFFER,
        QuestionKind::Action => hii_abi::value_type::ACTION,
        QuestionKind::Number(hii_ifr::Width::U8) => hii_abi::value_type::NUM_SIZE_8,
        QuestionKind::Number(hii_ifr::Width::U16) => hii_abi::value_type::NUM_SIZE_16,
        QuestionKind::Number(hii_ifr::Width::U32) => hii_abi::value_type::NUM_SIZE_32,
        QuestionKind::Number(hii_ifr::Width::U64) => hii_abi::value_type::NUM_SIZE_64,
    }
}

/// Maps a firmware popup style to a modal style.
///
/// A style that the specification does not define becomes information.
///
/// # Examples
///
/// ```
/// use forms::ModalStyle;
///
/// assert_eq!(
///     hii_uefi::style_of(uefi_raw::protocol::hii::popup::HiiPopupStyle::WARNING),
///     ModalStyle::Warning
/// );
/// assert_eq!(
///     hii_uefi::style_of(uefi_raw::protocol::hii::popup::HiiPopupStyle(9)),
///     ModalStyle::Info
/// );
/// ```
pub fn style_of(style: HiiPopupStyle) -> ModalStyle {
    if style == HiiPopupStyle::WARNING {
        ModalStyle::Warning
    } else if style == HiiPopupStyle::ERROR {
        ModalStyle::Error
    } else {
        ModalStyle::Info
    }
}

/// Maps a firmware popup type to a popup kind.
///
/// # Examples
///
/// ```
/// use hii_uefi::popup::PopupKind;
///
/// assert_eq!(
///     hii_uefi::kind_of(uefi_raw::protocol::hii::popup::HiiPopupType::YES_NO),
///     Some(PopupKind::YesNo)
/// );
/// assert!(
///     hii_uefi::kind_of(uefi_raw::protocol::hii::popup::HiiPopupType(9)).is_none()
/// );
/// ```
pub fn kind_of(kind: HiiPopupType) -> Option<PopupKind> {
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

/// Maps an answer index to the firmware answer.
///
/// # Examples
///
/// ```
/// use hii_uefi::popup::PopupKind;
///
/// assert_eq!(
///     hii_uefi::selection_of(PopupKind::YesNoCancel, 2),
///     uefi_raw::protocol::hii::popup::HiiPopupSelection::CANCEL
/// );
/// ```
pub fn selection_of(kind: PopupKind, index: usize) -> HiiPopupSelection {
    match (kind, index) {
        (PopupKind::Ok, _) => HiiPopupSelection::OK,
        (PopupKind::OkCancel, 0) => HiiPopupSelection::OK,
        (PopupKind::OkCancel, _) => HiiPopupSelection::CANCEL,
        (PopupKind::YesNo, 0) => HiiPopupSelection::YES,
        (PopupKind::YesNo, _) => HiiPopupSelection::NO,
        (PopupKind::YesNoCancel, 0) => HiiPopupSelection::YES,
        (PopupKind::YesNoCancel, 1) => HiiPopupSelection::NO,
        (PopupKind::YesNoCancel, _) => HiiPopupSelection::CANCEL,
    }
}
