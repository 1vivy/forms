//! Shows a native popup over the running page.
//!
//! A front end raises a confirmation without a second session:
//! [`Session::open_popup`] puts the popup over the current page, the page input
//! reaches the popup first, and the person answers it there. A front end acts
//! on [`Event::Popup`].
//!
//! ```sh
//! cargo run -p forms-examples --example popup
//! cargo run -p forms-examples --example popup -- --once
//! cargo run -p forms-examples --example popup -- --interactive
//! ```
//!
//! The scripted run shows three facts of the popup model:
//!
//! - Enter accepts the highlighted answer, which is the first answer.
//! - Back cancels only a popup that offers Cancel, so a Yes or No question
//!   never becomes consent.
//! - The popup leaves the page alone. The session still holds the formset.

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use forms::{Input, Popup, PopupStyle, PopupType, Provider, Session};
use forms_examples::SettingsProvider;

use crate::support::Mode;

fn main() -> std::io::Result<()> {
    let mut provider = SettingsProvider::new();
    let mut session = Session::new(
        provider
            .formsets()
            .expect("the in-memory settings are readable"),
    );
    let popup = Popup::new(
        PopupStyle::Warning,
        PopupType::YesNo,
        "Reset the display settings?",
    );
    match support::mode(std::env::args().skip(1)) {
        Mode::Once { columns, rows } => {
            enter(&mut session, &mut provider);
            session.open_popup(popup);
            support::print_frame(&mut session, &mut provider, columns, rows)
        }
        Mode::Interactive => {
            enter(&mut session, &mut provider);
            session.open_popup(popup);
            let events = support::interactive(&mut session, &mut provider)?;
            println!("events = {events:?}");
            Ok(())
        }
        Mode::Script => {
            enter(&mut session, &mut provider);
            session.open_popup(popup);
            let declined = session.input(Input::Back, &mut provider);
            let open = session.view(&mut provider).popup.is_some();
            println!("Back on a Yes or No popup returns {declined:?}, open = {open}");

            let accepted = session.input(Input::Enter, &mut provider);
            println!("Enter accepts the highlighted answer: {accepted:?}");

            session.open_popup(Popup::new(
                PopupStyle::Info,
                PopupType::OkCancel,
                "Erase the log?",
            ));
            let cancelled = session.input(Input::Back, &mut provider);
            println!("Back on an OK or Cancel popup returns {cancelled:?}");
            println!(
                "the page under the popup is {}",
                session.view(&mut provider).title
            );
            println!(
                "the formset and its values did not change: {:?}",
                provider.draft()
            );
            // A front end acts on the answer here: Yes starts the reset, and an
            // answer of Cancel leaves the platform alone.
            Ok(())
        }
    }
}

/// Opens the entry form of the example formset.
fn enter(session: &mut Session<u8>, provider: &mut SettingsProvider) {
    let _ = session.input(Input::Enter, provider);
}
