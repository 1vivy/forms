//! A demo site: the example formset of `forms-examples` in a browser.
//!
//! The module is behind the `demo` feature. It runs the [`forms_examples`]
//! provider in a real [`forms::Session`] and draws each view on a [`Screen`] of
//! the default phone panel. A web page reads the pixels and sends key strokes
//! through the exports below, so the demo needs no other front end.
//!
//! # Examples
//!
//! ```
//! // The page opens on the example formset and the first view is drawn.
//! assert_eq!(forms_sim::demo::open(), 1);
//! assert!(forms_sim::demo::text_len() == 0);
//! ```

use core::cell::RefCell;

use forms::{Input, Session};
use forms_examples::{SET_GUID, SettingsProvider, formset};

use crate::{Config, Screen};

thread_local! {
    /// The one demo page of this thread.
    static DEMO: RefCell<Option<Demo>> = const { RefCell::new(None) };
}

/// One demo page: the session, its provider, the panel and the last event.
struct Demo {
    session: Session<u8>,
    provider: SettingsProvider,
    screen: Screen,
    text: String,
}

impl Demo {
    /// Returns a page on the entry form of the example formset.
    fn new() -> Self {
        let mut session = Session::new(vec![(forms_examples::HANDLE, formset())]);
        let _ = session.enter(&SET_GUID);
        let mut page = Self {
            session,
            provider: SettingsProvider::new(),
            screen: Screen::new(Config::default()),
            text: String::new(),
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
        let event = self.session.input(input, &mut self.provider);
        self.text = describe(&event);
        self.paint(true);
    }
}

pub use crate::{describe, input};

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
    use super::{describe, input, open, press};
    use forms::{ActionRequest, Event, Input};

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
            describe(&Event::Response(ActionRequest::Exit)),
            "request: Exit"
        );
        assert_eq!(describe(&Event::None), "");
        assert_eq!(
            describe(&Event::Popup(forms::PopupSelection::Yes)),
            "popup: Yes"
        );
    }

    #[test]
    fn every_named_key_maps_to_one_input() {
        for (code, key) in [
            (0, Input::Up),
            (1, Input::Down),
            (2, Input::Left),
            (3, Input::Right),
            (4, Input::Home),
            (5, Input::Enter),
            (6, Input::Back),
            (7, Input::Erase),
        ] {
            assert_eq!(input(code, 0), Some(key));
        }
    }
}
