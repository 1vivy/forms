//! Key strokes from the firmware console.
//!
//! [`Keys`] prefers `EFI_SIMPLE_TEXT_INPUT_EX_PROTOCOL`, which reports the full
//! scan code, and falls back to `EFI_SIMPLE_TEXT_INPUT_PROTOCOL` for firmware
//! that publishes only the basic console input protocol.
//!
//! [`Keys::read`] never waits. The runner polls the console and stalls between
//! turns, so one place owns the idle delay.

use forms::Input;
use uefi::boot::ScopedProtocol;
use uefi::proto::console::text::{Input as Basic, InputEx, Key};
use uefi::{Error, Handle, Status};

use crate::KeyMap;
use crate::console;

/// The UEFI scan code of the up arrow and of the volume-up key.
const SCAN_UP: u16 = 0x01;
/// The UEFI scan code of the down arrow and of the volume-down key.
const SCAN_DOWN: u16 = 0x02;
/// The UEFI scan code of the right arrow.
const SCAN_RIGHT: u16 = 0x03;
/// The UEFI scan code of the left arrow.
const SCAN_LEFT: u16 = 0x04;
/// The UEFI scan code of the home key.
const SCAN_HOME: u16 = 0x05;
/// The UEFI scan code of the escape key.
const SCAN_ESCAPE: u16 = 0x17;
/// The UEFI scan code of the volume-up key on a phone keypad.
const SCAN_VOLUME_UP: u16 = 0x80;
/// The UEFI scan code of the volume-down key on a phone keypad.
const SCAN_VOLUME_DOWN: u16 = 0x81;
/// The scan code that firmware reports for the power or suspend key.
const SCAN_POWER: u16 = 0x0102;

/// Maps a firmware key stroke to a form input. This is the default map.
///
/// Scan codes win over the Unicode character. The arrows move the selection
/// and change a choice, the home key selects the first row, and Escape goes
/// back. The Qualcomm power key reports `0x0102` and selects, so a phone
/// without a volume keypad can still open a page.
///
/// A front end for a phone with volume and power keys uses
/// [`map_phone`] instead.
///
/// # Examples
///
/// ```
/// use forms::Input;
/// use forms_uefi::map_key;
///
/// assert_eq!(map_key(0x01, '\0'), Some(Input::Up));
/// assert_eq!(map_key(0x03, '\0'), Some(Input::Right));
/// assert_eq!(map_key(0x17, '\0'), Some(Input::Back));
/// assert_eq!(map_key(0x0102, '\0'), Some(Input::Enter));
/// assert_eq!(map_key(0, 'a'), Some(Input::Char('a')));
/// assert_eq!(map_key(0, '\u{1}'), None);
/// ```
pub fn map(scan: u16, unicode: char) -> Option<Input> {
    match scan {
        SCAN_UP => return Some(Input::Up),
        SCAN_DOWN => return Some(Input::Down),
        SCAN_RIGHT => return Some(Input::Right),
        SCAN_LEFT => return Some(Input::Left),
        SCAN_HOME => return Some(Input::Home),
        SCAN_ESCAPE => return Some(Input::Back),
        SCAN_POWER => return Some(Input::Enter),
        _ => {}
    }
    match unicode {
        '\r' | '\n' => Some(Input::Enter),
        '\u{8}' | '\u{7f}' => Some(Input::Erase),
        c if c == ' ' || c.is_ascii_graphic() => Some(Input::Char(c)),
        _ => None,
    }
}

/// Maps a firmware key stroke for a phone with volume, power and no keyboard.
///
/// Scan codes win over the Unicode character. The volume keys move the
/// selection, and the right arrow and the power key select, so three keys work
/// every page. The left arrow and Escape go back.
///
/// # Examples
///
/// ```
/// use forms::Input;
/// use forms_uefi::map_key_phone;
///
/// assert_eq!(map_key_phone(0x80, '\0'), Some(Input::Up));
/// assert_eq!(map_key_phone(0x81, '\0'), Some(Input::Down));
/// assert_eq!(map_key_phone(0x03, '\0'), Some(Input::Enter));
/// assert_eq!(map_key_phone(0x04, '\0'), Some(Input::Back));
/// assert_eq!(map_key_phone(0x0102, '\0'), Some(Input::Enter));
/// ```
pub fn map_phone(scan: u16, unicode: char) -> Option<Input> {
    match scan {
        SCAN_UP | SCAN_VOLUME_UP => return Some(Input::Up),
        SCAN_DOWN | SCAN_VOLUME_DOWN => return Some(Input::Down),
        SCAN_RIGHT | SCAN_POWER => return Some(Input::Enter),
        SCAN_LEFT | SCAN_ESCAPE => return Some(Input::Back),
        SCAN_HOME => return Some(Input::Home),
        _ => {}
    }
    match unicode {
        '\r' | '\n' => Some(Input::Enter),
        '\u{1b}' => Some(Input::Back),
        '\u{8}' | '\u{7f}' => Some(Input::Erase),
        c if c == ' ' || c.is_ascii_graphic() => Some(Input::Char(c)),
        _ => None,
    }
}

/// A non-blocking source of form inputs.
pub(crate) struct Keys {
    extended: Option<ScopedProtocol<InputEx>>,
    basic: Option<ScopedProtocol<Basic>>,
    map: KeyMap,
}

impl Keys {
    /// Opens the console input that the firmware publishes on `handle`.
    ///
    /// The extended protocol is preferred. The basic protocol is opened only
    /// when the firmware does not publish the extended one, so a console
    /// reports each key stroke once.
    ///
    /// # Errors
    ///
    /// Returns [`Status::UNSUPPORTED`] when neither protocol is openable.
    pub(crate) fn open(handle: Handle, map: KeyMap) -> Result<Self, Error> {
        let extended = console::open::<InputEx>(handle).ok();
        let basic = if extended.is_none() {
            console::open::<Basic>(handle).ok()
        } else {
            None
        };
        if extended.is_none() && basic.is_none() {
            return Err(Error::from(Status::UNSUPPORTED));
        }
        Ok(Self {
            extended,
            basic,
            map,
        })
    }

    /// The name of the open input protocol.
    pub(crate) fn kind(&self) -> &'static str {
        if self.extended.is_some() {
            "Text Input Ex"
        } else {
            "Simple Text Input"
        }
    }

    /// Returns the next key stroke, or `None` when no key waits.
    ///
    /// The outer `Option` tells whether a key arrived. The inner one is its
    /// form input, `None` for a key without a form meaning.
    pub(crate) fn read(&mut self) -> Option<Option<Input>> {
        if let Some(extended) = &mut self.extended {
            return match extended.read_key() {
                Ok(Some(data)) => Some(key_input(data.key, self.map)),
                _ => None,
            };
        }
        let basic = self.basic.as_mut()?;
        match basic.read_key() {
            Ok(Some(key)) => Some(key_input(key, self.map)),
            _ => None,
        }
    }
}

/// Maps one decoded key stroke to a form input.
fn key_input(key: Key, map: KeyMap) -> Option<Input> {
    match key {
        Key::Printable(character) => map(0, char::from(character)),
        Key::Special(scan) => map(scan.0, '\0'),
    }
}

#[cfg(test)]
mod tests {
    use super::{map, map_phone};
    use forms::Input;

    #[test]
    fn navigation_keys() {
        assert_eq!(map(0x01, '\0'), Some(Input::Up));
        assert_eq!(map(0x02, '\0'), Some(Input::Down));
        assert_eq!(map(0x03, '\0'), Some(Input::Right));
        assert_eq!(map(0x04, '\0'), Some(Input::Left));
        assert_eq!(map(0x05, '\0'), Some(Input::Home));
        assert_eq!(map(0x17, '\0'), Some(Input::Back));
    }

    #[test]
    fn phone_keys_without_a_keyboard() {
        // The power key reports its own scan code, so the three keys reach every
        // page while the volume keys report the arrow scan codes above.
        assert_eq!(map(0x0102, '\0'), Some(Input::Enter));
    }

    #[test]
    fn editing_keys() {
        assert_eq!(map(0, '\r'), Some(Input::Enter));
        assert_eq!(map(0, '\n'), Some(Input::Enter));
        assert_eq!(map(0, '\u{8}'), Some(Input::Erase));
        assert_eq!(map(0, '\u{7f}'), Some(Input::Erase));
        assert_eq!(map(0, '7'), Some(Input::Char('7')));
        assert_eq!(map(0, ' '), Some(Input::Char(' ')));
    }

    #[test]
    fn keys_without_a_form_meaning_are_dropped() {
        assert_eq!(map(0, '\0'), None);
        assert_eq!(map(0, '\u{1}'), None);
        assert_eq!(map(0, '\u{1b}'), None);
        assert_eq!(map(0, 'é'), None);
        assert_eq!(map(0x0b, '\0'), None);
    }

    #[test]
    fn a_scan_code_wins_over_the_character() {
        assert_eq!(map(0x02, 'x'), Some(Input::Down));
    }

    #[test]
    fn phone_volume_keys_move() {
        assert_eq!(map_phone(0x80, '\0'), Some(Input::Up));
        assert_eq!(map_phone(0x81, '\0'), Some(Input::Down));
        assert_eq!(map_phone(0x01, '\0'), Some(Input::Up));
        assert_eq!(map_phone(0x02, '\0'), Some(Input::Down));
        assert_eq!(map_phone(0x05, '\0'), Some(Input::Home));
    }

    #[test]
    fn phone_keys_select_and_go_back() {
        assert_eq!(map_phone(0x03, '\0'), Some(Input::Enter));
        assert_eq!(map_phone(0x04, '\0'), Some(Input::Back));
        assert_eq!(map_phone(0x17, '\0'), Some(Input::Back));
        assert_eq!(map_phone(0x0102, '\0'), Some(Input::Enter));
    }

    #[test]
    fn phone_keys_type_and_erase() {
        assert_eq!(map_phone(0, '\r'), Some(Input::Enter));
        assert_eq!(map_phone(0, '\n'), Some(Input::Enter));
        assert_eq!(map_phone(0, '\u{1b}'), Some(Input::Back));
        assert_eq!(map_phone(0, '\u{8}'), Some(Input::Erase));
        assert_eq!(map_phone(0, '\u{7f}'), Some(Input::Erase));
        assert_eq!(map_phone(0, 'a'), Some(Input::Char('a')));
        assert_eq!(map_phone(0, ' '), Some(Input::Char(' ')));
        assert_eq!(map_phone(0, 'é'), None);
        assert_eq!(map_phone(0x02, 'x'), Some(Input::Down));
    }
}
