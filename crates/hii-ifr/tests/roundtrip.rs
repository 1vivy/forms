//! Round trips of the complete model through the encoder and decoder.

use forms::{
    Choice, Expr, Form, FormId, FormSet, Item, Question, QuestionFlags, QuestionId, Statement,
    Storage, VarStore, VarStoreId, VarStoreKind, Width,
};
use hii_ifr::{decode, encode};

/// Returns a question with a storage and no flags.
fn question(id: u16, storage: Storage) -> Question {
    Question {
        id: QuestionId(id),
        prompt: format!("Prompt {id}"),
        help: format!("Help {id}"),
        storage,
        flags: QuestionFlags::default(),
    }
}

/// Builds a formset that uses every supported statement and condition.
fn model() -> FormSet {
    let buffer = VarStore {
        id: VarStoreId(1),
        guid: [0x01; 16],
        name: "Buffer".into(),
        size: 32,
        kind: VarStoreKind::Buffer,
    };
    let efi = VarStore {
        id: VarStoreId(2),
        guid: [0x02; 16],
        name: "DtAcpiPref".into(),
        size: 4,
        kind: VarStoreKind::Efi { attributes: 3 },
    };
    let named = VarStore {
        id: VarStoreId(3),
        guid: [0x03; 16],
        name: String::new(),
        size: 0,
        kind: VarStoreKind::NameValue,
    };
    let offset = |id, offset| Storage::Offset {
        varstore: VarStoreId(id),
        offset,
    };
    let name = |id: u16, name: &str| Storage::Name {
        varstore: VarStoreId(id),
        name: name.into(),
    };

    let list = Expr::EqualList {
        question: QuestionId(0x21),
        values: vec![1, 4, 9],
    };
    let conditions = [
        Expr::True,
        Expr::False,
        Expr::Not(Box::new(Expr::False)),
        Expr::And(Box::new(Expr::True), Box::new(Expr::False)),
        Expr::Or(Box::new(list.clone()), Box::new(Expr::True)),
        Expr::EqualValue {
            question: QuestionId(0x30),
            value: 0x1234,
        },
        list,
        Expr::EqualQuestion {
            first: QuestionId(0x30),
            second: QuestionId(0x31),
        },
    ];

    let mut items = vec![
        Item::from(Statement::Subtitle {
            text: "State".into(),
        }),
        Item::from(Statement::Text {
            prompt: "Read only".into(),
            help: "A value shown to the person".into(),
            value: "Enabled".into(),
        }),
        Item::from(Statement::Goto {
            prompt: "Next".into(),
            help: "Open the next page".into(),
            form: FormId(2),
            question: None,
        }),
        Item::from(Statement::Goto {
            prompt: "Callback".into(),
            help: "Open with a question".into(),
            form: FormId(2),
            question: Some(QuestionId(0x0F)),
        }),
        Item::from(Statement::Action {
            question: question(0x10, offset(2, 0)),
        }),
        Item::from(Statement::Checkbox {
            question: question(0x11, offset(1, 0)),
            default: true,
        }),
        Item::from(Statement::Checkbox {
            question: question(0x12, offset(1, 1)),
            default: false,
        }),
        Item::from(Statement::OneOf {
            question: question(0x13, offset(1, 2)),
            width: Width::U8,
            choices: vec![
                Choice {
                    text: "One".into(),
                    value: 1,
                    default: false,
                },
                Choice {
                    text: "Two".into(),
                    value: 2,
                    default: true,
                },
                Choice {
                    text: "Three".into(),
                    value: 3,
                    default: false,
                },
            ],
        }),
        Item::from(Statement::Numeric {
            question: question(0x14, offset(2, 0)),
            width: Width::U8,
            minimum: 1,
            maximum: 9,
            step: 2,
        }),
        Item::from(Statement::Numeric {
            question: question(0x15, offset(2, 1)),
            width: Width::U16,
            minimum: 0x100,
            maximum: 0xFFFF,
            step: 0,
        }),
        Item::from(Statement::Numeric {
            question: question(0x16, offset(2, 2)),
            width: Width::U32,
            minimum: 0x10000,
            maximum: u32::MAX as u64,
            step: 1,
        }),
        Item::from(Statement::Numeric {
            question: question(0x17, offset(2, 3)),
            width: Width::U64,
            minimum: 0,
            maximum: u64::MAX,
            step: 1,
        }),
        Item::from(Statement::TextInput {
            question: question(0x18, name(3, "Title")),
            minimum: 1,
            maximum: 8,
        }),
    ];

    for (index, condition) in conditions.iter().enumerate() {
        if index % 2 == 0 {
            items[index].suppress_if = Some(condition.clone());
        } else {
            items[index].gray_out_if = Some(condition.clone());
        }
    }
    items[12].suppress_if = Some(Expr::And(
        Box::new(Expr::EqualValue {
            question: QuestionId(0x13),
            value: 2,
        }),
        Box::new(Expr::Not(Box::new(Expr::EqualQuestion {
            first: QuestionId(0x14),
            second: QuestionId(0x15),
        }))),
    ));

    FormSet {
        guid: [0x42; 16],
        title: "Platform".into(),
        help: "Platform settings".into(),
        class_guids: vec![[0x11; 16], [0x12; 16]],
        varstores: vec![buffer, efi, named],
        forms: vec![
            Form {
                id: FormId(1),
                title: "Main".into(),
                items,
            },
            Form {
                id: FormId(2),
                title: "Second".into(),
                items: vec![Item::from(Statement::Subtitle {
                    text: "Empty".into(),
                })],
            },
        ],
    }
}

#[test]
fn round_trip_keeps_the_model() {
    let set = model();
    let bytes = encode(&set, [0x55; 16]).expect("encode");
    let sets = decode(&bytes).expect("decode");
    assert_eq!(sets, vec![set]);
}

#[test]
fn encode_writes_a_forms_strings_and_end_package() {
    let bytes = encode(&model(), [0x55; 16]).expect("encode");
    assert_eq!(&bytes[..16], &[0x55; 16]);
    assert_eq!(
        u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize,
        bytes.len()
    );
    let mut kinds = Vec::new();
    let mut at = 20;
    while at + 4 <= bytes.len() {
        let header = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        kinds.push((header >> 24) as u8);
        at += (header & 0x00FF_FFFF) as usize;
    }
    assert_eq!(kinds, vec![0x02, 0x04, 0xDF]);
}

#[test]
fn unsupported_conditions_are_refused() {
    let mut set = model();
    set.forms[0].items[0].suppress_if = Some(Expr::Unsupported);
    assert!(encode(&set, [0; 16]).is_err());
}

#[test]
fn a_value_wider_than_its_question_is_refused() {
    let mut set = model();
    set.forms[0].items[7] = Item::from(Statement::OneOf {
        question: question(
            0x13,
            Storage::Offset {
                varstore: VarStoreId(1),
                offset: 2,
            },
        ),
        width: Width::U8,
        choices: vec![Choice {
            text: "Too wide".into(),
            value: 0x100,
            default: false,
        }],
    });
    assert!(encode(&set, [0; 16]).is_err());
}

#[test]
fn a_non_ascii_store_name_is_refused() {
    let mut set = model();
    set.varstores[0].name = "Störe".into();
    assert!(encode(&set, [0; 16]).is_err());
}

#[test]
fn three_class_guids_round_trip_and_four_are_refused() {
    let mut set = model();
    set.class_guids = vec![[0x01; 16], [0x02; 16], [0x03; 16]];
    let decoded = decode(&encode(&set, [0; 16]).expect("encode")).expect("decode");
    assert_eq!(decoded[0].class_guids, set.class_guids);
    set.class_guids.push([0x04; 16]);
    assert_eq!(
        encode(&set, [0; 16]),
        Err(hii_ifr::EncodeError::TooManyClassGuids)
    );
}
