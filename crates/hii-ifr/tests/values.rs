//! Value encoding at question widths.

use forms::{Choice, Control, Key, Row, Value};
use hii_ifr::{value_bytes, value_from};

/// Builds a numeric row with a maximum value.
fn numeric(maximum: u64) -> Row {
    Row::new(
        Key(1),
        "Count",
        Control::Number {
            minimum: 0,
            maximum,
            step: 0,
        },
    )
}

#[test]
fn numbers_use_the_question_width() {
    assert_eq!(
        value_bytes(&numeric(u64::from(u8::MAX)), &Value::Number(0xFF)),
        Some(vec![0xFF])
    );
    assert_eq!(
        value_bytes(&numeric(u64::from(u16::MAX)), &Value::Number(0x1234)),
        Some(vec![0x34, 0x12])
    );
    assert_eq!(
        value_bytes(&numeric(u64::from(u32::MAX)), &Value::Number(0x1234_5678)),
        Some(vec![0x78, 0x56, 0x34, 0x12])
    );
    assert_eq!(
        value_bytes(&numeric(u64::MAX), &Value::Number(0x0102_0304_0506_0708)),
        Some(vec![0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01])
    );
    assert_eq!(
        value_bytes(&numeric(u64::from(u8::MAX)), &Value::Number(0x100)),
        None
    );
    assert_eq!(
        value_bytes(&numeric(u64::from(u16::MAX)), &Value::Number(0x1_0000)),
        None
    );
    assert_eq!(
        value_bytes(&numeric(u64::from(u32::MAX)), &Value::Number(0x1_0000_0000)),
        None
    );
}

#[test]
fn numbers_read_back_from_shorter_and_longer_buffers() {
    let row = numeric(u64::from(u16::MAX));
    assert_eq!(value_from(&row, &[0x34, 0x12]), Some(Value::Number(0x1234)));
    assert_eq!(
        value_from(&row, &[0x34, 0x12, 0xFF]),
        Some(Value::Number(0x1234))
    );
    let wide = numeric(u64::from(u32::MAX));
    assert_eq!(value_from(&wide, &[0x34, 0x12]), None);
}

#[test]
fn checkboxes_use_one_byte() {
    let row = Row::new(Key(1), "Mode", Control::Toggle);
    assert_eq!(value_bytes(&row, &Value::Bool(true)), Some(vec![1]));
    assert_eq!(value_bytes(&row, &Value::Bool(false)), Some(vec![0]));
    assert_eq!(value_from(&row, &[7]), Some(Value::Bool(true)));
    assert_eq!(value_from(&row, &[0]), Some(Value::Bool(false)));
    assert_eq!(value_from(&row, &[]), None);
}

#[test]
fn text_is_utf16_and_padded_to_the_block_width() {
    let row = Row::new(
        Key(1),
        "Name",
        Control::TextInput {
            minimum: 0,
            maximum: 4,
            secret: false,
        },
    );
    assert_eq!(
        value_bytes(&row, &Value::Text("Hi".into())),
        Some(vec![0x48, 0x00, 0x69, 0x00, 0x00, 0x00, 0x00, 0x00])
    );
    assert_eq!(
        value_from(&row, &[0x48, 0x00, 0x69, 0x00]),
        Some(Value::Text("Hi".into()))
    );
    assert_eq!(
        value_from(&row, &[0x48, 0x00]),
        Some(Value::Text("H".into()))
    );
    assert_eq!(value_from(&row, &[]), Some(Value::Text(String::new())));
    assert_eq!(value_bytes(&row, &Value::Text("Hello".into())), None);
    let wide = Row::new(
        Key(1),
        "Name",
        Control::TextInput {
            minimum: 0,
            maximum: 255,
            secret: false,
        },
    );
    assert_eq!(
        value_bytes(&wide, &Value::Text("Ω".into())),
        Some({
            let mut bytes = vec![0xA9, 0x03];
            bytes.resize(255 * 2, 0);
            bytes
        })
    );
}

#[test]
fn ordered_lists_store_one_width_per_value() {
    let row = Row::new(
        Key(1),
        "Order",
        Control::OrderedList {
            choices: vec![Choice::new("A", 1), Choice::new("B", 2)],
            maximum: 2,
        },
    );
    assert_eq!(
        value_bytes(&row, &Value::List(vec![2, 1])),
        Some(vec![2, 0, 1, 0])
    );
    assert_eq!(
        value_from(&row, &[2, 0, 1, 0]),
        Some(Value::List(vec![2, 1]))
    );
}

#[test]
fn a_password_stores_text_like_a_text_row() {
    let row = Row::new(
        Key(1),
        "Secret",
        Control::TextInput {
            minimum: 0,
            maximum: 3,
            secret: true,
        },
    );
    assert_eq!(
        value_bytes(&row, &Value::Text("ab".into())),
        Some(vec![0x61, 0x00, 0x62, 0x00, 0x00, 0x00])
    );
    assert_eq!(
        value_from(&row, &[0x61, 0x00, 0x62, 0x00]),
        Some(Value::Text("ab".into()))
    );
}
