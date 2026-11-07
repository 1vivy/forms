//! Shows the templated settings set and its typed provider.
//!
//! The template of the crate gives one list for the page and the provider, so
//! a setting reaches both together. The provider owns the values, so the
//! session and the widget hold no copy.
//!
//! ```sh
//! cargo run -p forms-examples --example settings
//! cargo run -p forms-examples --example settings -- --once
//! cargo run -p forms-examples --example settings -- --interactive
//! ```
//!
//! The scripted run enters the set, adjusts the slider by two steps and prints
//! the settings.

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use forms::{Event, Input, Session};
use forms_examples::{HANDLE, SettingsProvider, TIMEOUT, formset};

use crate::support::Mode;

fn main() -> std::io::Result<()> {
    let mut provider = SettingsProvider::new();
    let mut session = Session::new(vec![(HANDLE, formset())]);
    let _ = session.enter(formset().key);
    match support::mode(std::env::args().skip(1)) {
        Mode::Once { columns, rows } => {
            support::print_frame(&mut session, &mut provider, columns, rows)
        }
        Mode::Interactive => {
            let events = support::interactive(&mut session, &mut provider)?;
            report(&provider, &events);
            Ok(())
        }
        Mode::Script => {
            let before = provider.settings().timeout;
            if !support::select(&mut session, &mut provider, "Menu timeout") {
                println!("the page has no Menu timeout row");
                return Ok(());
            }
            let mut events = Vec::new();
            for input in [Input::Enter, Input::Up, Input::Up, Input::Enter] {
                let event = session.input(input, &mut provider);
                if !matches!(event, Event::None | Event::Changed) {
                    events.push(event);
                }
            }
            println!("timeout {before} -> {}", provider.settings().timeout);
            assert!(provider.read_setting(TIMEOUT) == forms::Value::Number(before.max(2)));
            report(&provider, &events);
            Ok(())
        }
    }
}

/// Prints the settings and the events that the session returned.
fn report(provider: &SettingsProvider, events: &[Event]) {
    println!("settings = {}", provider.settings().summary());
    println!("actions = {:?}", provider.actions());
    println!("events = {events:?}");
}
