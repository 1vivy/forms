//! Value encoding at question widths.

use forms::{Question, QuestionFlags, QuestionId, Statement, Storage, Value, Width};
use hii_ifr::{value_bytes, value_from};

/// Builds a question with no storage.
fn question() -> Question {
    Question {
        id: QuestionId(1),
        prompt: String::new(),
        help: String::new(),
        storage: Storage::None,
        flags: QuestionFlags::default(),
    }
}

/// Builds a numeric statement of one width.
fn numeric(width: Width) -> Statement {
    Statement::Numeric {
        question: question(),
        width,
        minimum: 0,
        maximum: u64::MAX,
        step: 0,
    }
}

#[test]
fn numbers_use_the_question_width() {
    assert_eq!(
        value_bytes(&numeric(Width::U8), &Value::Number(0xFF)),
        Some(vec![0xFF])
    );
    assert_eq!(
        value_bytes(&numeric(Width::U16), &Value::Number(0x1234)),
        Some(vec![0x34, 0x12])
    );
    assert_eq!(
        value_bytes(&numeric(Width::U32), &Value::Number(0x1234_5678)),
        Some(vec![0x78, 0x56, 0x34, 0x12])
    );
    assert_eq!(
        value_bytes(&numeric(Width::U64), &Value::Number(0x0102_0304_0506_0708)),
        Some(vec![0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01])
    );
    assert_eq!(
        value_bytes(&numeric(Width::U8), &Value::Number(0x100)),
        None
    );
    assert_eq!(
        value_bytes(&numeric(Width::U16), &Value::Number(0x1_0000)),
        None
    );
    assert_eq!(
        value_bytes(&numeric(Width::U32), &Value::Number(0x1_0000_0000)),
        None
    );
}

#[test]
fn numbers_read_back_from_shorter_and_longer_buffers() {
    assert_eq!(
        value_from(&numeric(Width::U16), &[0x34, 0x12]),
        Some(Value::Number(0x1234))
    );
    assert_eq!(
        value_from(&numeric(Width::U16), &[0x34, 0x12, 0xFF]),
        Some(Value::Number(0x1234))
    );
    assert_eq!(value_from(&numeric(Width::U32), &[0x34, 0x12]), None);
}

#[test]
fn checkboxes_use_one_byte() {
    let statement = Statement::Checkbox {
        question: question(),
        default: false,
    };
    assert_eq!(value_bytes(&statement, &Value::Bool(true)), Some(vec![1]));
    assert_eq!(value_bytes(&statement, &Value::Bool(false)), Some(vec![0]));
    assert_eq!(value_from(&statement, &[7]), Some(Value::Bool(true)));
    assert_eq!(value_from(&statement, &[0]), Some(Value::Bool(false)));
    assert_eq!(value_from(&statement, &[]), None);
}

#[test]
fn text_is_utf16_and_padded_to_the_block_width() {
    let statement = Statement::TextInput {
        question: question(),
        minimum: 0,
        maximum: 4,
    };
    assert_eq!(
        value_bytes(&statement, &Value::Text("Hi".into())),
        Some(vec![0x48, 0x00, 0x69, 0x00, 0x00, 0x00, 0x00, 0x00])
    );
    assert_eq!(
        value_from(&statement, &[0x48, 0x00, 0x69, 0x00]),
        Some(Value::Text("Hi".into()))
    );
    assert_eq!(
        value_from(&statement, &[0x48, 0x00]),
        Some(Value::Text("H".into()))
    );
    assert_eq!(
        value_from(&statement, &[]),
        Some(Value::Text(String::new()))
    );
    assert_eq!(value_bytes(&statement, &Value::Text("Hello".into())), None);
    assert_eq!(
        value_bytes(
            &Statement::TextInput {
                question: question(),
                minimum: 0,
                maximum: 255
            },
            &Value::Text("Ω".into())
        ),
        Some({
            let mut bytes = vec![0xA9, 0x03];
            bytes.resize(255 * 2, 0);
            bytes
        })
    );
    assert_eq!(
        value_bytes(
            &Statement::TextInput {
                question: question(),
                minimum: 0,
                maximum: 2
            },
            &Value::Text("AB".into())
        )
        .map(|bytes| bytes.len()),
        Some(4)
    );
}

#[test]
fn raw_bytes_pass_through() {
    let statement = Statement::Subtitle {
        text: String::new(),
    };
    assert_eq!(
        value_bytes(&statement, &Value::Bytes(vec![1, 2, 3])),
        Some(vec![1, 2, 3])
    );
    assert_eq!(value_bytes(&statement, &Value::Text("x".into())), None);
    assert_eq!(value_from(&statement, &[1]), None);
}
