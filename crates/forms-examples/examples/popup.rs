//! Shows a modal decision over the running page.
//!
//! `Session::open_modal` shows any modal over the page: a title, a body, a
//! style and its own buttons. The Cancel button is selected first, so one
//! Enter refuses a dangerous question.
//!
//! ```sh
//! cargo run -p forms-examples --example popup
//! cargo run -p forms-examples --example popup -- --once
//! ```

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use forms::{Button, ButtonRole, Event, Input, Modal, ModalOutcome, ModalStyle, Session};
use forms_examples::{HANDLE, SettingsProvider, formset};

use crate::support::Mode;

fn main() -> std::io::Result<()> {
    let mut provider = SettingsProvider::new();
    let mut session = Session::new(vec![(HANDLE, formset())]);
    let _ = session.enter(formset().key);
    match support::mode(std::env::args().skip(1)) {
        Mode::Once { columns, rows } => {
            session.open_modal(erase());
            support::print_frame(&mut session, &mut provider, columns, rows)
        }
        Mode::Interactive => {
            session.open_modal(erase());
            let events = support::interactive(&mut session, &mut provider)?;
            println!("events = {events:?}");
            Ok(())
        }
        Mode::Script => {
            session.open_modal(erase());
            let refused = session.input(Input::Enter, &mut provider);
            println!("one Enter refuses: {refused:?}");
            assert!(matches!(
                refused,
                Event::Modal(ModalOutcome::Chosen {
                    role: ButtonRole::Cancel,
                    ..
                })
            ));
            session.open_modal(erase());
            let moved = session.input(Input::Right, &mut provider);
            let accepted = match session.input(Input::Enter, &mut provider) {
                Event::Modal(outcome) => outcome,
                other => {
                    println!("unexpected event {other:?}");
                    return Ok(());
                }
            };
            println!("confirmed: {accepted:?} after {moved:?}");
            Ok(())
        }
    }
}

/// Builds the modal of the example.
fn erase() -> Modal {
    let mut modal = Modal::new(
        ModalStyle::Danger,
        "Erase settings",
        "This removes the saved values and the next boot uses the factory values.",
    );
    modal.push(Button::new("Erase", ButtonRole::Default));
    modal.push(Button::new("Cancel", ButtonRole::Cancel));
    modal
}
