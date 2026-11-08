//! A demo site: the gallery of `forms-examples` in a browser.
//!
//! The module is behind the `demo` feature. It runs the [`forms_examples`]
//! gallery provider in a real [`forms::Session`] with the keys of a phone, and
//! draws each view on a [`Screen`] of the default phone panel. A web page
//! reads the pixels and sends key strokes through the exports below, so the
//! demo needs no other front end.
//!
//! The destination row opens a modal with one button for each destination.
//! The module reports the chosen button as the notice text.
//!
//! # Examples
//!
//! ```
//! // The page opens on the gallery and the first view is drawn.
//! assert_eq!(forms_sim::demo::open(), 1);
//! assert!(forms_sim::demo::text_len() == 0);
//! ```

use core::cell::RefCell;

use forms::{
    Button, ButtonRole, Control, Event, Input, Key, Keypad, MemoryProvider, Modal, ModalOutcome,
    ModalStyle, Session, render::Theme,
};
use forms_examples::{HANDLE, gallery, gallery_provider};

use crate::{Config, Screen};

/// The destinations that the picker modal lists.
const DESTINATIONS: [&str; 6] = [
    "Boot order",
    "Network",
    "Security",
    "Storage",
    "Devices",
    "Advanced",
];

thread_local! {
    /// The one demo page of this thread.
    static DEMO: RefCell<Option<Demo>> = const { RefCell::new(None) };
}

/// One demo page: the session, its provider, the panel and the last event.
struct Demo {
    session: Session<u8>,
    provider: MemoryProvider,
    screen: Screen,
    text: String,
    /// The row that opens the destination picker.
    picker: Key,
}

impl Demo {
    /// Returns a page on the entry form of the gallery, with the keys of a
    /// phone.
    fn new() -> Self {
        let set = gallery();
        let picker = set
            .pages
            .iter()
            .flat_map(|page| page.rows.iter())
            .find(|row| matches!(row.control, Control::Action))
            .map_or(Key::default(), |row| row.key);
        let mut session =
            Session::new(vec![(HANDLE, set.clone())]).with_keypad(Keypad::volume_and_power());
        let _ = session.enter(set.key);
        let mut screen = Screen::new(Config::default());
        let theme = Theme {
            keys: Keypad::volume_and_power(),
            ..Theme::default()
        };
        screen.set_theme(theme);
        let mut page = Self {
            session,
            provider: gallery_provider(),
            screen,
            text: String::new(),
            picker,
        };
        page.paint(true);
        page
    }

    /// Draws the current view.
    fn paint(&mut self, full: bool) {
        let view = self.session.view(&mut self.provider);
        if self.screen.draw(full, &view).is_err() {
            self.text = String::from("the view did not draw");
        }
    }

    /// Applies one input, records what it changed, and draws the result.
    fn press(&mut self, input: Input) {
        let actions = self.provider.actions.len();
        let event = self.session.input(input, &mut self.provider);
        if self.provider.actions.len() > actions
            && self.provider.actions.last() == Some(&self.picker)
        {
            self.session.open_modal(destination_modal());
            self.text = String::from("destination picker");
        } else {
            self.text = report(&event);
        }
        self.paint(true);
    }
}

pub use crate::describe;

/// Returns the input of one phone key code.
///
/// `code` is `0` for Volume Up, `1` for Volume Down, `2` for Home, `3` for
/// Power, `4` for Back, `5` for Erase and `6` for the character in
/// `character`. Every other code returns `None`.
///
/// # Examples
///
/// ```
/// use forms::Input;
/// use forms_sim::demo::input;
///
/// assert_eq!(input(0, 0), Some(Input::Up));
/// assert_eq!(input(3, 0), Some(Input::Enter));
/// assert_eq!(input(6, u32::from(b'k')), Some(Input::Char('k')));
/// assert_eq!(input(99, 0), None);
/// ```
#[must_use]
pub fn input(code: u32, character: u32) -> Option<Input> {
    match code {
        0 => Some(Input::Up),
        1 => Some(Input::Down),
        2 => Some(Input::Home),
        3 => Some(Input::Enter),
        4 => Some(Input::Back),
        5 => Some(Input::Erase),
        6 => core::char::from_u32(character)
            .filter(|character| !character.is_control())
            .map(Input::Char),
        _ => None,
    }
}

/// Returns the picker modal: one button for each destination, then Cancel.
fn destination_modal() -> Modal {
    let mut modal = Modal::new(ModalStyle::Info, "Destinations", "Choose the page to open.");
    for (index, destination) in DESTINATIONS.into_iter().enumerate() {
        modal.push(Button::new(
            destination,
            ButtonRole::Other(u64::try_from(index).unwrap_or(u64::MAX)),
        ));
    }
    modal.push(Button::new("Cancel", ButtonRole::Cancel));
    modal
}

/// Returns the notice text of one event.
fn report(event: &Event) -> String {
    match event {
        Event::Modal(ModalOutcome::Chosen { index, .. }) => destination_label(*index).map_or_else(
            || String::from("destination: ?"),
            |label| format!("destination: {label}"),
        ),
        Event::Modal(ModalOutcome::Closed) => String::from("destination: Cancel"),
        other => describe(other),
    }
}

/// Returns the label of one picker button.
fn destination_label(index: usize) -> Option<&'static str> {
    DESTINATIONS
        .get(index)
        .copied()
        .or_else(|| (index == DESTINATIONS.len()).then_some("Cancel"))
}

/// Opens the demo page and draws its first view.
///
/// Returns `1` when the page is ready.
#[unsafe(no_mangle)]
pub extern "C" fn open() -> u32 {
    DEMO.with(|demo| {
        *demo.borrow_mut() = Some(Demo::new());
    });
    1
}

/// Applies one browser key stroke and draws the result.
///
/// Returns `1` when the page drew again, and `0` when the key stroke carries no
/// form input.
#[unsafe(no_mangle)]
pub extern "C" fn press(code: u32, character: u32) -> u32 {
    let Some(key) = input(code, character) else {
        return 0;
    };
    DEMO.with(|demo| match demo.borrow_mut().as_mut() {
        Some(page) => {
            page.press(key);
            1
        }
        None => 0,
    })
}

/// Returns the RGB pixels of the panel.
///
/// The pointer stays valid until the next call of [`open`].
#[unsafe(no_mangle)]
pub extern "C" fn pixels() -> *const u8 {
    DEMO.with(|demo| {
        demo.borrow()
            .as_ref()
            .map_or(core::ptr::null(), |page| page.screen.pixels().as_ptr())
    })
}

/// Returns the panel width in pixels.
#[unsafe(no_mangle)]
pub extern "C" fn width() -> u32 {
    DEMO.with(|demo| {
        demo.borrow()
            .as_ref()
            .map_or(0, |page| page.screen.size().0)
    })
}

/// Returns the panel height in pixels.
#[unsafe(no_mangle)]
pub extern "C" fn height() -> u32 {
    DEMO.with(|demo| {
        demo.borrow()
            .as_ref()
            .map_or(0, |page| page.screen.size().1)
    })
}

/// Returns the UTF-8 text of the last event.
#[unsafe(no_mangle)]
pub extern "C" fn text() -> *const u8 {
    DEMO.with(|demo| {
        demo.borrow()
            .as_ref()
            .map_or(core::ptr::null(), |page| page.text.as_ptr())
    })
}

/// Returns the length of the last event text in bytes.
#[unsafe(no_mangle)]
pub extern "C" fn text_len() -> u32 {
    DEMO.with(|demo| {
        demo.borrow()
            .as_ref()
            .map_or(0, |page| u32::try_from(page.text.len()).unwrap_or(u32::MAX))
    })
}

#[cfg(test)]
mod tests {
    use super::{DESTINATIONS, describe, destination_modal, input, open, press};
    use forms::{Event, Input, ModalOutcome, Request};

    #[test]
    fn a_key_stroke_reaches_the_session() {
        assert_eq!(open(), 1);
        assert_eq!(press(1, 0), 1);
        assert_eq!(press(99, 0), 0);
        assert_eq!(input(99, 0), None);
    }

    #[test]
    fn an_event_states_its_request() {
        assert_eq!(
            describe(&Event::Response(Request::Restart)),
            "request: Restart"
        );
        assert_eq!(describe(&Event::None), "");
        assert_eq!(
            describe(&Event::Modal(ModalOutcome::Closed)),
            "modal: Closed"
        );
    }

    #[test]
    fn every_named_key_maps_to_one_input() {
        for (code, key) in [
            (0, Input::Up),
            (1, Input::Down),
            (2, Input::Home),
            (3, Input::Enter),
            (4, Input::Back),
            (5, Input::Erase),
        ] {
            assert_eq!(input(code, 0), Some(key));
        }
        assert_eq!(input(6, u32::from(b'k')), Some(Input::Char('k')));
        assert_eq!(input(6, 0), None);
    }

    #[test]
    fn the_picker_lists_every_destination_and_cancel() {
        let modal = destination_modal();
        assert_eq!(modal.buttons.len(), DESTINATIONS.len() + 1);
        assert_eq!(modal.buttons[0].label, "Boot order");
        assert_eq!(modal.buttons[5].label, "Advanced");
        assert_eq!(modal.buttons[6].label, "Cancel");
        assert_eq!(modal.selected, 6);
    }
}
