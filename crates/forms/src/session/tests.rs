//! Session decisions: navigation, editing, validation and live rows.

use super::{BACK, Editor, EditorKind, Event, Input, Keypad, Session};
use crate::controls::Decoration;
use crate::model::{
    Builder, Choice, Control, Expr, FormSet, Key, MemoryProvider, Request, Store, Validation, Value,
};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Builds a provider over one set.
fn provider_of(set: &FormSet) -> MemoryProvider {
    MemoryProvider::new(vec![(0u8, set.clone())])
}

/// Builds a session over one set without a Back row.
fn session_of(set: &FormSet) -> Session<u8> {
    Session::new(vec![(0u8, set.clone())]).with_back_row(false)
}

/// Refuses a number outside its range and keeps the stored value.
#[test]
fn number_range_refuses_the_value() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Count",
            Control::Number {
                minimum: 0,
                maximum: 10,
                step: 1,
            },
        )
        .set_binding(Key(11));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(11), Value::Number(5));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
    assert_eq!(
        session.input(Input::Char('9'), &mut provider),
        Event::Changed
    );
    assert!(matches!(
        session.input(Input::Enter, &mut provider),
        Event::Failed(_)
    ));
    assert_eq!(provider.store.read(Key(11)), Ok(Value::Number(5)));
}

/// Refuses a value that an edit rule rejects and keeps the stored value.
#[test]
fn edit_rule_refuses_the_value() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Name",
            Control::TextInput {
                minimum: 0,
                maximum: 8,
                secret: false,
            },
        )
        .set_binding(Key(20))
        .push_validation(Validation::refuse(Expr::Bool(true), "Refused"));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(20), Value::Text(String::from("ok")));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    session.input(Input::Enter, &mut provider);
    session.input(Input::Char('!'), &mut provider);
    assert!(matches!(
        session.input(Input::Enter, &mut provider),
        Event::Failed(_)
    ));
    assert_eq!(
        provider.store.read(Key(20)),
        Ok(Value::Text(String::from("ok")))
    );
}

/// Asks for one confirmation before a warned value is written.
#[test]
fn warning_needs_a_second_enter() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row("Mode", Control::Toggle)
        .set_binding(Key(21))
        .push_validation(Validation::warn(Expr::Bool(true), "Careful"));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(21), Value::Bool(false));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    assert!(matches!(
        session.input(Input::Enter, &mut provider),
        Event::Warning(_)
    ));
    assert_eq!(provider.store.read(Key(21)), Ok(Value::Bool(false)));
    assert_eq!(
        session.input(Input::Enter, &mut provider),
        Event::Response(Request::None)
    );
    assert_eq!(provider.store.read(Key(21)), Ok(Value::Bool(true)));
}

/// Moves an entry of an ordered list and writes the new order.
#[test]
fn ordered_list_moves_and_commits() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Order",
            Control::OrderedList {
                choices: vec![
                    Choice::new("A", 1),
                    Choice::new("B", 2),
                    Choice::new("C", 3),
                ],
                maximum: 3,
            },
        )
        .set_binding(Key(12));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(12), Value::List(vec![1, 2]));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
    assert_eq!(session.input(Input::Down, &mut provider), Event::Changed);
    assert_eq!(session.input(Input::Left, &mut provider), Event::Changed);
    assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
    assert_eq!(
        session.input(Input::Enter, &mut provider),
        Event::Response(Request::None)
    );
    assert_eq!(provider.store.read(Key(12)), Ok(Value::List(vec![2, 1])));
}

/// Reorders and removes ordered-list entries with Up, Down and Enter only,
/// as a device with volume keys and power does.
#[test]
fn ordered_list_reorders_with_three_keys() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Order",
            Control::OrderedList {
                choices: vec![
                    Choice::new("A", 1),
                    Choice::new("B", 2),
                    Choice::new("C", 3),
                ],
                maximum: 3,
            },
        )
        .set_binding(Key(12));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(12), Value::List(vec![1, 2, 3]));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    // Open the editor, pick up A, move it down one, drop it: B A C.
    for input in [Input::Enter, Input::Enter, Input::Down, Input::Enter] {
        assert_eq!(session.input(input, &mut provider), Event::Changed);
    }
    // Pick up C and move it past the last chosen entry: removed.
    for input in [Input::Down, Input::Enter, Input::Down] {
        assert_eq!(session.input(input, &mut provider), Event::Changed);
    }
    // Walk to Done and accept.
    let Some(Editor {
        kind: EditorKind::List {
            items, selected, ..
        },
        ..
    }) = session.view(&mut provider).editor
    else {
        panic!("expected the list editor");
    };
    for _ in selected..items.len() - 1 {
        session.input(Input::Down, &mut provider);
    }
    assert_eq!(
        session.input(Input::Enter, &mut provider),
        Event::Response(Request::None)
    );
    assert_eq!(provider.store.read(Key(12)), Ok(Value::List(vec![2, 1])));
}

/// Without a Back key, Enter on an unchanged value leaves the editor without
/// a write; with a Back key, the same Enter writes the value.
#[test]
fn enter_on_an_unchanged_value_cancels_without_a_back_key() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Mode",
            Control::Choice(vec![Choice::new("A", 1), Choice::new("B", 2)]),
        )
        .set_binding(Key(14));
    let set = builder.build();
    for (keypad, expected) in [
        (Keypad::volume_and_power(), Event::Changed),
        (Keypad::keyboard(), Event::Response(Request::None)),
    ] {
        let mut provider = provider_of(&set);
        provider.set(Key(14), Value::Number(1));
        let mut session = session_of(&set).with_keypad(keypad);
        assert!(session.enter(Key(1)));
        session.input(Input::Enter, &mut provider);
        assert_eq!(
            session.input(Input::Enter, &mut provider),
            expected,
            "{keypad:?}"
        );
        assert!(session.view(&mut provider).editor.is_none());
    }
}

/// A device that cannot type keeps a text row visible but grayed out, states
/// why in its help, and opens no editor for it.
#[test]
fn a_text_row_needs_a_keyboard() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Name",
            Control::TextInput {
                minimum: 0,
                maximum: 8,
                secret: false,
            },
        )
        .set_binding(Key(16))
        .set_help("Set it from the host.");
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(16), Value::Text(String::from("a")));
    let mut session = session_of(&set).with_keypad(Keypad::volume_and_power());
    assert!(session.enter(Key(1)));
    let view = session.view(&mut provider);
    assert!(!view.rows[0].enabled);
    assert_eq!(view.help, "Needs a keyboard. Set it from the host.");
    assert!(matches!(
        session.input(Input::Enter, &mut provider),
        Event::Failed(_)
    ));
    assert!(session.view(&mut provider).editor.is_none());

    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
    assert!(session.view(&mut provider).editor.is_some());
}

/// Up on the first option moves the cursor to the last, and Down on the
/// last moves it to the first, as volume keys walk a list.
#[test]
fn a_choice_cursor_wraps_both_ways() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Target",
            Control::Choice(vec![
                Choice::new("A", 1),
                Choice::new("B", 2),
                Choice::new("C", 3),
            ]),
        )
        .set_binding(Key(15));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(15), Value::Number(1));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    session.input(Input::Enter, &mut provider);
    let cursor = |session: &mut Session<u8>, provider: &mut MemoryProvider| match session
        .view(provider)
        .editor
    {
        Some(Editor {
            kind: EditorKind::Choice { selected, .. },
            ..
        }) => selected,
        other => panic!("expected the choice editor, got {other:?}"),
    };
    assert_eq!(cursor(&mut session, &mut provider), 0);
    session.input(Input::Up, &mut provider);
    assert_eq!(cursor(&mut session, &mut provider), 2);
    session.input(Input::Down, &mut provider);
    assert_eq!(cursor(&mut session, &mut provider), 0);
}

/// Down moves the choice cursor without a write; Enter writes the option
/// under the cursor.
#[test]
fn a_choice_writes_only_on_enter() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Mode",
            Control::Choice(vec![Choice::new("A", 1), Choice::new("B", 2)]),
        )
        .set_binding(Key(14));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(14), Value::Number(1));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    session.input(Input::Enter, &mut provider);
    assert_eq!(session.input(Input::Down, &mut provider), Event::Changed);
    assert_eq!(provider.store.read(Key(14)), Ok(Value::Number(1)));
    assert_eq!(
        session.input(Input::Enter, &mut provider),
        Event::Response(Request::None)
    );
    assert_eq!(provider.store.read(Key(14)), Ok(Value::Number(2)));
}

/// A link pushes its page; the Back key and Enter on the Back row both
/// return to the linking page with the link row still selected.
#[test]
fn a_link_nests_and_back_returns_to_the_linking_row() {
    let mut builder = Builder::new(Key(1), "S");
    let inner = builder.key();
    builder.page("Main");
    builder.row("A", Control::Toggle).set_binding(Key(30));
    let link = builder.link("Inner", inner).key;
    builder.page_with(inner, "Inner");
    builder.row("B", Control::Toggle).set_binding(Key(31));
    let set = builder.build();
    // The Back key, then a phone's Up to the Back row and Enter.
    for leave in [&[Input::Back][..], &[Input::Up, Input::Enter][..]] {
        let mut provider = provider_of(&set);
        let mut session = Session::new(vec![(0u8, set.clone())]);
        assert!(session.enter(Key(1)));
        session.input(Input::Down, &mut provider);
        assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
        assert_eq!(session.view(&mut provider).title, "Inner");
        for input in leave {
            session.input(*input, &mut provider);
        }
        let view = session.view(&mut provider);
        assert_eq!(view.title, "Main", "{leave:?}");
        assert_eq!(view.selected.map(|index| view.rows[index].key), Some(link));
    }
}

/// Steps a number with Up and Down within its bounds.
#[test]
fn number_steps_with_up_and_down() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Count",
            Control::Number {
                minimum: 0,
                maximum: 10,
                step: 5,
            },
        )
        .set_binding(Key(13));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(13), Value::Number(5));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    for input in [Input::Enter, Input::Up, Input::Up] {
        session.input(input, &mut provider);
    }
    assert_eq!(
        session.input(Input::Enter, &mut provider),
        Event::Response(Request::None)
    );
    assert_eq!(provider.store.read(Key(13)), Ok(Value::Number(10)));
}

/// Down from the last row reaches the Back row, so a device without a Back
/// key can always leave the page.
#[test]
fn down_reaches_the_back_row() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder.row("A", Control::Toggle).set_binding(Key(30));
    builder.row("B", Control::Toggle).set_binding(Key(31));
    let set = builder.build();
    let mut provider = provider_of(&set);
    let mut session = Session::new(vec![(0u8, set.clone())]);
    assert!(session.enter(Key(1)));
    let mut labels = Vec::new();
    for _ in 0..3 {
        session.input(Input::Down, &mut provider);
        let view = session.view(&mut provider);
        labels.push(view.selected.map(|index| view.rows[index].label.clone()));
    }
    assert_eq!(
        labels,
        [
            Some(String::from("B")),
            Some(String::from(BACK)),
            Some(String::from("A"))
        ]
    );
}

/// Writes every value of the named default set.
#[test]
fn reset_writes_the_default_set() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder.row("A", Control::Toggle).set_binding(Key(30));
    builder.row("B", Control::Toggle).set_binding(Key(31));
    builder.row("Restore", Control::Reset(Key(40)));
    builder.defaults(
        Key(40),
        "Factory",
        [(Key(30), Value::Bool(false)), (Key(31), Value::Bool(false))],
    );
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(30), Value::Bool(true));
    provider.set(Key(31), Value::Bool(true));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    session.input(Input::Down, &mut provider);
    session.input(Input::Down, &mut provider);
    assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
    assert_eq!(provider.store.read(Key(30)), Ok(Value::Bool(false)));
    assert_eq!(provider.store.read(Key(31)), Ok(Value::Bool(false)));
}

/// Hides a row and refuses a read-only row.
#[test]
fn conditions_hide_and_lock_rows() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder.row("Kept", Control::Toggle).set_binding(Key(50));
    builder
        .row("Hidden", Control::Toggle)
        .set_binding(Key(51))
        .hidden = Some(Expr::Bool(true));
    builder
        .row("Fixed", Control::Toggle)
        .set_binding(Key(52))
        .read_only = Some(Expr::Bool(true));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(50), Value::Bool(false));
    provider.set(Key(51), Value::Bool(false));
    provider.set(Key(52), Value::Bool(false));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    let view = session.view(&mut provider);
    assert_eq!(view.rows.len(), 2);
    assert_eq!(view.rows[1].label, "Fixed");
    assert!(!view.rows[1].enabled);
    session.input(Input::Down, &mut provider);
    assert!(matches!(
        session.input(Input::Enter, &mut provider),
        Event::Failed(_)
    ));
    assert_eq!(provider.store.read(Key(52)), Ok(Value::Bool(false)));
}

/// Steps a slider, clamps it and commits the value.
#[test]
fn slider_steps_clamps_and_commits() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Timeout",
            Control::Slider {
                minimum: 0,
                maximum: 3,
                step: 1,
                unit: String::from("s"),
                labels: vec![Choice::new("Off", 0)],
            },
        )
        .set_binding(Key(70));
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(70), Value::Number(0));
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    let view = session.view(&mut provider);
    assert_eq!(view.rows[0].value, "Off");
    assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
    assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
    assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
    let view = session.view(&mut provider);
    let editor = view.editor.expect("editor");
    let crate::session::EditorKind::Slider { ref text, .. } = editor.kind else {
        panic!("expected a slider editor");
    };
    assert_eq!(text, "2 s");
    assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
    assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
    assert_eq!(
        session.input(Input::Enter, &mut provider),
        Event::Response(Request::None)
    );
    assert_eq!(provider.store.read(Key(70)), Ok(Value::Number(3)));
}

/// Reports a periodic refresh only after its interval.
#[test]
fn periodic_refresh_waits_for_its_interval() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row("Clock", Control::Text(String::new()))
        .set_binding(Key(60))
        .refresh = crate::model::Refresh::Periodic(1_000);
    let set = builder.build();
    let mut provider = provider_of(&set);
    provider.set(Key(60), Value::Text(String::from("00:00")));
    let mut session = session_of(&set).with_readable_rows(true);
    assert!(session.enter(Key(1)));
    session.view(&mut provider);
    assert!(!session.poll(500));
    assert!(session.poll(1_500));
}

/// A slider row follows the editor before the value is written.
#[test]
fn a_slider_row_tracks_its_editor() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Fan",
            Control::Slider {
                minimum: 0,
                maximum: 3,
                step: 1,
                unit: String::from("s"),
                labels: vec![Choice::new("Off", 0)],
            },
        )
        .set_binding(Key(80));
    let set: FormSet = builder.build();
    let mut provider = MemoryProvider::new(vec![(0u8, set.clone())]);
    provider.set(Key(80), Value::Number(0));
    let mut session = Session::new(vec![(0u8, set)]).with_back_row(false);
    assert!(session.enter(Key(1)));

    let view = session.view(&mut provider);
    assert_eq!(view.rows[0].value, "Off");
    assert_eq!(
        view.rows[0].decoration,
        Decoration::Slider {
            position: 0,
            minimum: 0,
            maximum: 3
        }
    );

    assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
    assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
    let view = session.view(&mut provider);
    assert_eq!(view.rows[0].value, "1 s");
    assert_eq!(
        view.rows[0].decoration,
        Decoration::Slider {
            position: 1,
            minimum: 0,
            maximum: 3
        }
    );
    assert_eq!(
        provider.store.read(Key(80)),
        Ok(Value::Number(0)),
        "the row follows the editor before the value is written"
    );
}

/// A resumed session lands on the page and the row that the last one left,
/// and on the deepest page that still exists when a rebuild removed a page.
#[test]
fn a_location_resumes_the_page_and_the_row() {
    let build = |with_detail: bool| {
        let mut builder = Builder::new(Key(1), "S");
        builder.page_with(Key(10), "Main");
        builder.link("Storage", Key(11));
        builder.page_with(Key(11), "Storage");
        builder.row("First", Control::Toggle).set_binding(Key(20));
        builder.row("Second", Control::Toggle).set_binding(Key(21));
        if with_detail {
            builder.link("Detail", Key(12));
            builder.page_with(Key(12), "Detail");
            builder.row("Size", Control::Toggle).set_binding(Key(22));
        }
        builder.build()
    };
    let set = build(true);
    let mut provider = provider_of(&set);
    let mut session = session_of(&set);
    assert!(session.enter(Key(1)));
    session.input(Input::Enter, &mut provider);
    session.input(Input::Down, &mut provider);
    let on_second = session.location();
    session.input(Input::Down, &mut provider);
    session.input(Input::Enter, &mut provider);
    let on_detail = session.location();

    let mut next = session_of(&set);
    assert!(next.enter(Key(1)));
    assert!(next.resume(&on_second));
    let view = next.view(&mut provider);
    assert_eq!(view.title, "Storage");
    assert_eq!(
        view.selected.map(|index| view.rows[index].label.as_str()),
        Some("Second")
    );

    let rebuilt = build(false);
    let mut provider = provider_of(&rebuilt);
    let mut next = session_of(&rebuilt);
    assert!(next.enter(Key(1)));
    assert!(!next.resume(&on_detail), "the detail page is gone");
    assert_eq!(next.view(&mut provider).title, "Storage");
}
