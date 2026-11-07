//! The mapping between firmware action requests and forms requests.

use alloc::format;

use forms::{Error, Key, Request, Scope};
use hii_ifr::QuestionKind;
use uefi_raw::Status;

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
