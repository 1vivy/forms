//! Round trips of the complete model through the encoder and decoder.

use forms::{
    Builder, Choice, Control, Expr, FormSet, Key, Operand, Refresh, Target, Truncation, Validation,
    Value,
};
use hii_ifr::{Export, QuestionKind, Storage, Width, decode, descriptor, encode};

/// Builds a set that uses every control and condition.
fn model() -> FormSet {
    let mut builder = Builder::new(Key(1), "Platform").help("Platform settings");
    builder.page("Main");
    builder.row("State", Control::Heading);
    builder.row("Firmware", Control::Text(String::from("2.1")));
    builder
        .row("Advanced", Control::Link(Target::page(Key(0))))
        .set_help("Open the advanced page");
    builder.row("Apply", Control::Action);
    builder
        .row("Secure boot", Control::Toggle)
        .set_binding(Key(10));
    builder
        .row(
            "Mode",
            Control::Choice(vec![Choice::new("Fast", 1), Choice::new("Safe", 2)]),
        )
        .set_binding(Key(11));
    builder
        .row(
            "Cores",
            Control::Number {
                minimum: 1,
                maximum: 64,
                step: 1,
            },
        )
        .set_binding(Key(12));
    builder
        .row(
            "Name",
            Control::TextInput {
                minimum: 1,
                maximum: 16,
                secret: false,
            },
        )
        .set_binding(Key(13));
    builder
        .row(
            "Password",
            Control::TextInput {
                minimum: 4,
                maximum: 12,
                secret: true,
            },
        )
        .set_binding(Key(14));
    builder
        .row(
            "Boot order",
            Control::OrderedList {
                choices: vec![
                    Choice::new("Disk", 1),
                    Choice::new("Network", 2),
                    Choice::new("USB", 3),
                ],
                maximum: 3,
            },
        )
        .set_binding(Key(15));
    builder.row("Restore defaults", Control::Reset(Key(40)));
    builder.row(
        "Progress",
        Control::Progress {
            current: 2,
            total: Some(5),
        },
    );
    builder.row("Separator", Control::Divider);
    builder.row(
        "Consumer",
        Control::Custom {
            kind: Key(7),
            data: vec![1, 2, 3],
        },
    );
    builder.defaults(
        Key(40),
        "Factory",
        [(Key(10), Value::Bool(false)), (Key(11), Value::Number(2))],
    );
    builder.page("Advanced");
    builder.row("Back", Control::Link(Target::page(Key(0))));
    let mut set = builder.build();
    let main = set.pages[0].key;
    // Point both links at the advanced page.
    if let Some(advanced) = set.pages.get(1) {
        let advanced = advanced.key;
        for row in &mut set.pages[0].rows {
            if let Control::Link(target) = &mut row.control {
                target.page = advanced;
            }
        }
        let _ = main;
    }
    set
}

/// Encodes and decodes a set.
fn round_trip(set: &FormSet) -> (forms::FormSet, Export) {
    let export = descriptor(set, [0x42; 16]);
    let list = encode(set, &export, [0x17; 16]).expect("encode");
    let decoded = decode(&list).expect("decode");
    assert_eq!(decoded.len(), 1);
    let decoded = decoded.into_iter().next().unwrap();
    (decoded.set, decoded.export)
}

#[test]
fn every_control_survives_the_round_trip() {
    let set = model();
    let (decoded, export) = round_trip(&set);
    assert_eq!(decoded.title, "Platform");
    assert_eq!(decoded.pages.len(), 2);
    assert_eq!(decoded.pages[0].title, "Main");
    let rows = &decoded.pages[0].rows;
    assert!(matches!(rows[0].control, Control::Heading));
    assert_eq!(rows[1].control, Control::Text(String::from("2.1")));
    assert!(matches!(rows[2].control, Control::Link(_)));
    assert_eq!(rows[3].control, Control::Action);
    assert_eq!(rows[4].control, Control::Toggle);
    let Control::Choice(choices) = &rows[5].control else {
        panic!("expected a choice");
    };
    assert_eq!(choices.len(), 2);
    assert_eq!(choices[0].label, "Fast");
    let Control::Number {
        minimum,
        maximum,
        step,
    } = &rows[6].control
    else {
        panic!("expected a number");
    };
    assert_eq!((*minimum, *maximum, *step), (1, 64, 1));
    let Control::TextInput {
        minimum,
        maximum,
        secret,
    } = &rows[7].control
    else {
        panic!("expected text");
    };
    assert_eq!((*minimum, *maximum, *secret), (1, 16, false));
    let Control::TextInput { secret, .. } = &rows[8].control else {
        panic!("expected a password");
    };
    assert!(*secret);
    let Control::OrderedList { choices, maximum } = &rows[9].control else {
        panic!("expected an ordered list");
    };
    assert_eq!(*maximum, 3);
    assert_eq!(choices.len(), 3);
    assert!(matches!(rows[10].control, Control::Reset(_)));
    // The descriptor keeps the question identity and the storage offsets.
    assert_eq!(export.questions.len(), 7);
    assert_eq!(export.questions[0].kind, QuestionKind::Action);
    let toggle = export
        .questions
        .iter()
        .find(|question| question.kind == QuestionKind::Bool)
        .expect("toggle");
    assert_eq!(toggle.question, 2);
    assert_eq!(
        toggle.storage,
        Storage::Offset {
            store: 1,
            offset: 0
        }
    );
    assert_eq!(export.questions[2].kind, QuestionKind::Number(Width::U8));
    assert_eq!(export.questions[4].bytes, 32);
    assert_eq!(export.questions[5].bytes, 24);
    assert_eq!(export.questions[6].kind, QuestionKind::OrderedList);
    assert_eq!(export.guid, [0x42; 16]);
}

#[test]
fn a_decoded_set_encodes_again_unchanged() {
    let set = model();
    let (decoded, export) = round_trip(&set);
    let list = encode(&decoded, &export, [0x17; 16]).expect("re-encode");
    let again = decode(&list).expect("re-decode");
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].set, decoded);
}

#[test]
fn conditions_validate_and_refresh_round_trip() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder.row("Mode", Control::Toggle).set_binding(Key(10));
    let row = builder.row(
        "Cores",
        Control::Number {
            minimum: 1,
            maximum: 8,
            step: 1,
        },
    );
    row.set_binding(Key(11));
    row.hidden = Some(Expr::Equal(
        Operand::Field(Key(10)),
        Operand::Literal(Value::Bool(true)),
    ));
    row.disabled = Some(Expr::Equal(
        Operand::Field(Key(11)),
        Operand::Literal(Value::Number(1)),
    ));
    row.validation = vec![
        Validation::warn(Expr::Bool(true), "This erases data"),
        Validation::refuse(Expr::Bool(true), "Not allowed"),
        Validation::block_submit(Expr::Bool(true), "Fix the values"),
    ];
    row.refresh = Refresh::Periodic(2_500);
    let set = builder.build();
    let (decoded, _) = round_trip(&set);
    let checked = decoded.pages[0].rows.last().expect("row");
    assert!(matches!(checked.hidden, Some(Expr::Equal(..))));
    assert!(matches!(checked.disabled, Some(Expr::Equal(..))));
    assert_eq!(checked.validation.len(), 3);
    assert_eq!(checked.validation[0].message, "This erases data");
    assert_eq!(checked.validation[1].kind, forms::ValidationKind::Edit);
    assert_eq!(checked.validation[2].kind, forms::ValidationKind::Submit);
    assert_eq!(checked.refresh, Refresh::Periodic(3_000));
}

#[test]
fn an_ordered_list_keeps_its_values_and_limit() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Order",
            Control::OrderedList {
                choices: vec![Choice::new("A", 1), Choice::new("B", 2)],
                maximum: 2,
            },
        )
        .set_binding(Key(10));
    let set = builder.build();
    let (decoded, export) = round_trip(&set);
    let row = &decoded.pages[0].rows[0];
    let Control::OrderedList { choices, maximum } = &row.control else {
        panic!("expected an ordered list");
    };
    assert_eq!(*maximum, 2);
    assert_eq!(choices[1].label, "B");
    assert_eq!(export.questions[0].kind, QuestionKind::OrderedList);
}

#[test]
fn a_reset_button_names_the_decoded_default_set() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder.row("A", Control::Toggle).set_binding(Key(10));
    builder.row("Restore", Control::Reset(Key(40)));
    builder.defaults(Key(40), "Factory", [(Key(10), Value::Bool(false))]);
    let set = builder.build();
    let (decoded, _) = round_trip(&set);
    assert_eq!(decoded.defaults.len(), 1);
    assert_eq!(decoded.defaults[0].name, "Factory");
    let restore = &decoded.pages[0].rows[1];
    assert_eq!(
        restore.control.reset_default(),
        Some(decoded.defaults[0].key)
    );
}

#[test]
fn unsupported_controls_degrade_to_text_rows() {
    let set = model();
    let (decoded, _) = round_trip(&set);
    let rows = &decoded.pages[0].rows;
    let progress = &rows[11];
    assert_eq!(progress.control, Control::Text(String::from("2/5")));
    let divider = &rows[12];
    assert_eq!(divider.control, Control::Heading);
    let consumer = &rows[13];
    assert_eq!(
        consumer.control,
        Control::Text(String::from("Consumer control 7"))
    );
}

#[test]
fn presentation_hints_do_not_change_the_ifr_value() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    let row = builder.row("Info", Control::Text(String::from("Long text")));
    row.presentation = forms::Presentation {
        label_limit: Some(20),
        truncation: Truncation::Wrap,
    };
    let set = builder.build();
    let (decoded, _) = round_trip(&set);
    assert_eq!(
        decoded.pages[0].rows[0].control,
        Control::Text(String::from("Long text"))
    );
}

#[test]
fn a_slider_exports_as_a_numeric_question() {
    let mut builder = Builder::new(Key(1), "S");
    builder.page("Main");
    builder
        .row(
            "Timeout",
            Control::Slider {
                minimum: 0,
                maximum: 10,
                step: 1,
                unit: String::from("s"),
                labels: vec![Choice::new("Off", 0)],
            },
        )
        .set_binding(Key(10));
    let set = builder.build();
    let (decoded, export) = round_trip(&set);
    assert_eq!(export.questions[0].kind, QuestionKind::Number(Width::U8));
    let Control::Number {
        minimum,
        maximum,
        step,
    } = &decoded.pages[0].rows[0].control
    else {
        panic!("expected a numeric question");
    };
    assert_eq!((*minimum, *maximum, *step), (0, 10, 1));
}
