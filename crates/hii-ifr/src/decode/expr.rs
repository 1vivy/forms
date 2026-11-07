//! Conditions and their operands, from IFR expression opcodes.

use super::*;

/// Returns the condition expression of a condition scope and its statements.
pub(super) fn split_condition<'a, 'b>(node: &'b Node<'a>) -> (Expr, &'b [Node<'a>]) {
    match node.children.first() {
        Some(first) if is_expression(first.code) => (decode_expression(first), &node.children[1..]),
        _ => (Expr::Bool(true), &node.children[..]),
    }
}

/// Combines an outer and an inner condition.
pub(super) fn merge(outer: Option<Expr>, inner: Option<Expr>) -> Option<Expr> {
    match (outer, inner) {
        (Some(outer), Some(inner)) => Some(Expr::And(Box::new(outer), Box::new(inner))),
        (outer, inner) => outer.or(inner),
    }
}

/// Decodes one expression tree.
pub(super) fn decode_expression(node: &Node<'_>) -> Expr {
    match node.code {
        abi::op::TRUE => Expr::Bool(true),
        abi::op::FALSE => Expr::Bool(false),
        abi::op::NOT => match node.children.first() {
            Some(child) => Expr::Not(Box::new(decode_expression(child))),
            None => Expr::Unavailable(String::from("NOT without an operand")),
        },
        abi::op::AND | abi::op::OR => {
            if node.children.len() != 2 {
                return Expr::Unavailable(String::from("binary operator without two operands"));
            }
            let first = Box::new(decode_expression(&node.children[0]));
            let second = Box::new(decode_expression(&node.children[1]));
            if node.code == abi::op::AND {
                Expr::And(first, second)
            } else {
                Expr::Or(first, second)
            }
        }
        abi::op::EQUAL | abi::op::LESS_THAN | abi::op::LESS_EQUAL => {
            if node.children.len() != 2 {
                return Expr::Unavailable(String::from("comparison without two operands"));
            }
            let first = decode_operand(&node.children[0]);
            let second = decode_operand(&node.children[1]);
            match node.code {
                abi::op::EQUAL => Expr::Equal(first, second),
                abi::op::LESS_THAN => Expr::Less(first, second),
                _ => Expr::LessEqual(first, second),
            }
        }
        abi::op::EQ_ID_VAL => match (read_u16(node.payload, 0), read_u16(node.payload, 2)) {
            (Some(question), Some(value)) => Expr::Equal(
                Operand::Field(Key(u64::from(question))),
                Operand::Literal(Value::Number(u64::from(value))),
            ),
            _ => Expr::Unavailable(String::from("EQ_ID_VAL is truncated")),
        },
        abi::op::EQ_ID_ID => match (read_u16(node.payload, 0), read_u16(node.payload, 2)) {
            (Some(first), Some(second)) => Expr::Equal(
                Operand::Field(Key(u64::from(first))),
                Operand::Field(Key(u64::from(second))),
            ),
            _ => Expr::Unavailable(String::from("EQ_ID_ID is truncated")),
        },
        abi::op::EQ_ID_VAL_LIST => {
            let Some(question) = read_u16(node.payload, 0) else {
                return Expr::Unavailable(String::from("EQ_ID_VAL_LIST is truncated"));
            };
            let Some(count) = read_u16(node.payload, 2) else {
                return Expr::Unavailable(String::from("EQ_ID_VAL_LIST is truncated"));
            };
            let count = usize::from(count);
            if node.payload.len() < 4 + count * 2 {
                return Expr::Unavailable(String::from("EQ_ID_VAL_LIST is truncated"));
            }
            let values = (0..count)
                .filter_map(|index| read_u16(node.payload, 4 + index * 2))
                .map(|value| Value::Number(u64::from(value)))
                .collect();
            Expr::In(Operand::Field(Key(u64::from(question))), values)
        }
        code => Expr::Unavailable(format!("opcode 0x{code:02X}")),
    }
}

/// Decodes one operand of a comparison.
pub(super) fn decode_operand(node: &Node<'_>) -> Operand {
    match node.code {
        abi::op::QUESTION_REF1 => match read_u16(node.payload, 0) {
            Some(question) => Operand::Field(Key(u64::from(question))),
            None => Operand::Literal(Value::Bytes(Vec::new())),
        },
        abi::op::UINT8 => Operand::Literal(Value::Number(u64::from(
            node.payload.first().copied().unwrap_or(0),
        ))),
        abi::op::UINT16 => Operand::Literal(Value::Number(u64::from(
            read_u16(node.payload, 0).unwrap_or(0),
        ))),
        abi::op::UINT32 => Operand::Literal(Value::Number(u64::from(
            read_u32(node.payload, 0).unwrap_or(0),
        ))),
        abi::op::UINT64 => Operand::Literal(Value::Number(
            read_width(node.payload, 0, Width::U64).unwrap_or(0),
        )),
        abi::op::TRUE => Operand::Literal(Value::Bool(true)),
        abi::op::FALSE => Operand::Literal(Value::Bool(false)),
        _ => Operand::Literal(Value::Bytes(Vec::new())),
    }
}
