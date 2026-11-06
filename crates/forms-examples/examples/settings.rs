//! Shows the templated settings formset and its typed provider.
//!
//! The template of the crate gives one list for the page and for the provider,
//! so a setting reaches both together. The provider owns the values, so the
//! session and the widget never hold a copy.
//!
//! ```sh
//! cargo run -p forms-examples --example settings
//! cargo run -p forms-examples --example settings -- --once
//! cargo run -p forms-examples --example settings -- --interactive
//! ```
//!
//! The scripted run enters the formset, raises the brightness by one step and
//! prints the draft and the committed value. The draft moves and the committed
//! value stays, because a change is a draft until the provider applies it.

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use forms::{Event, Input, Provider, Session};
use forms_examples::SettingsProvider;

use crate::support::Mode;

fn main() -> std::io::Result<()> {
    let mut provider = SettingsProvider::new();
    let mut session = Session::new(
        provider
            .formsets()
            .expect("the in-memory settings are readable"),
    );
    match support::mode(std::env::args().skip(1)) {
        Mode::Once { columns, rows } => {
            enter(&mut session, &mut provider);
            support::print_frame(&mut session, &mut provider, columns, rows)
        }
        Mode::Interactive => {
            enter(&mut session, &mut provider);
            let events = support::interactive(&mut session, &mut provider)?;
            report(&provider, &events);
            Ok(())
        }
        Mode::Script => {
            enter(&mut session, &mut provider);
            let before = provider.draft().brightness;
            if !support::select(&mut session, &mut provider, "Brightness") {
                println!("the page has no Brightness row");
                return Ok(());
            }
            // Enter opens the editor, Right adds one step, Enter writes it.
            let mut events = Vec::new();
            for input in [Input::Enter, Input::Right, Input::Enter] {
                match session.input(input, &mut provider) {
                    Event::None | Event::Changed => {}
                    event => events.push(event),
                }
            }
            println!(
                "a change is a draft: {before} -> {}",
                provider.draft().brightness
            );
            report(&provider, &events);
            Ok(())
        }
    }
}

/// Opens the entry form of the example formset.
fn enter(session: &mut Session<u8>, provider: &mut SettingsProvider) {
    let _ = session.input(Input::Enter, provider);
}

/// Prints the values of both records and the events that the session returned.
fn report(provider: &SettingsProvider, events: &[Event]) {
    println!("draft = {:?}", provider.draft());
    println!("committed = {:?}", provider.committed());
    println!("requests = {:?}", provider.requests());
    println!("events = {events:?}");
}
