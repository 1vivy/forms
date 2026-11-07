//! Shows the requests that a provider returns to the session.
//!
//! A provider answers a change or an action with a [`forms::Request`]. The
//! session acts on it before it returns the event, so a front end only obeys a
//! restart. This example runs the apply action and prints the requests.
//!
//! ```sh
//! cargo run -p forms-examples --example actions
//! ```

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use forms::{Event, Input, Request, Scope, Session, Value};
use forms_examples::{CORES, HANDLE, SettingsProvider, formset};

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
            println!("events = {events:?}");
            Ok(())
        }
        Mode::Script => {
            provider.set(CORES, Value::Number(8)).expect("cores");
            let applied = support::enter_row(&mut session, &mut provider, "Apply settings");
            println!("apply returned {applied:?}");
            assert!(matches!(
                applied,
                Some(Event::Response(Request::Apply {
                    scope: Scope::Set,
                    leave: false
                }))
            ));
            // The restore row writes the factory values through the provider.
            let restore = support::enter_row(&mut session, &mut provider, "Restore defaults");
            println!("restore returned {restore:?}");
            println!("settings = {}", provider.settings().summary());
            assert_eq!(provider.settings().timeout, 5);
            let _ = Input::Enter;
            Ok(())
        }
    }
}
