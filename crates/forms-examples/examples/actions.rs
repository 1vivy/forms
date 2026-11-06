//! Shows how a provider answers with an `ActionRequest`.
//!
//! A provider answers a change or an action with a request. The session is the
//! browser, so it acts on the request before it returns [`Event::Response`].
//! A front end only has to act on the one request that leaves the engine:
//! `Reset` asks the platform for a restart.
//!
//! ```sh
//! cargo run -p forms-examples --example actions
//! cargo run -p forms-examples --example actions -- --once
//! cargo run -p forms-examples --example actions -- --interactive
//! ```
//!
//! The scripted run visits each action row with a draft change in place:
//!
//! - `Reset` keeps the page and asks for a restart. Nothing is committed.
//! - `Submit` calls `apply(FormSet)`, so the draft becomes committed, and then
//!   returns to the formset list.
//! - `Exit` calls `discard(FormSet)`, so the draft returns to the committed
//!   values, and then returns to the formset list.

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use forms::{ActionRequest, Input, Provider, Session};
use forms_examples::{ACTIONS, SettingsProvider};

use crate::support::Mode;

fn main() -> std::io::Result<()> {
    match support::mode(std::env::args().skip(1)) {
        Mode::Once { columns, rows } => {
            let (mut session, mut provider) = start();
            support::select(&mut session, &mut provider, "Reset to defaults");
            support::print_frame(&mut session, &mut provider, columns, rows)
        }
        Mode::Interactive => {
            let (mut session, mut provider) = start();
            let events = support::interactive(&mut session, &mut provider)?;
            println!("events = {events:?}");
            println!("requests = {:?}", provider.requests());
            Ok(())
        }
        Mode::Script => {
            for action in ACTIONS {
                run(action.prompt);
            }
            Ok(())
        }
    }
}

/// Creates a provider and a session on the entry form of the example formset.
fn start() -> (Session<u8>, SettingsProvider) {
    let mut provider = SettingsProvider::new();
    let mut session = Session::new(
        provider
            .formsets()
            .expect("the in-memory settings are readable"),
    );
    let _ = session.input(Input::Enter, &mut provider);
    (session, provider)
}

/// Changes one draft value, then runs the action row with this prompt.
fn run(prompt: &str) {
    let (mut session, mut provider) = start();
    if support::select(&mut session, &mut provider, "Brightness") {
        for input in [Input::Enter, Input::Right, Input::Enter] {
            let _ = session.input(input, &mut provider);
        }
    }
    let event = support::enter_row(&mut session, &mut provider, prompt);
    println!("{prompt}: {event:?}");
    println!(
        "  draft brightness = {}, committed brightness = {}",
        provider.draft().brightness,
        provider.committed().brightness
    );
    println!("  page = {}", session.view(&mut provider).title);
    println!("  requests = {:?}", provider.requests());
    println!("  reset asked for = {}", asks_for_reset(&provider));
}

/// Returns whether the provider asked the platform for a restart.
fn asks_for_reset(provider: &SettingsProvider) -> bool {
    provider.requests().contains(&ActionRequest::Reset)
}
