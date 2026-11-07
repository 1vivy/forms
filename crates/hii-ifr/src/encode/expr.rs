//! Conditions and their operands, as IFR expression opcodes.

use super::*;

/// Encodes one expression as a tree of scoped opcodes.
pub(super) fn encode_expr(expr: &Expr, export: &Export) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    match expr {
        Expr::Bool(true) => push_op(&mut out, abi::op::TRUE, false, &[])?,
        Expr::Bool(false) => push_op(&mut out, abi::op::FALSE, false, &[])?,
        Expr::Not(inner) => {
            push_op(&mut out, abi::op::NOT, true, &[])?;
            out.extend_from_slice(&encode_expr(inner, export)?);
            push_op(&mut out, abi::op::END, false, &[])?;
        }
        Expr::And(first, second) | Expr::Or(first, second) => {
            let code = if matches!(expr, Expr::And(..)) {
                abi::op::AND
            } else {
                abi::op::OR
            };
            push_op(&mut out, code, true, &[])?;
            out.extend_from_slice(&encode_expr(first, export)?);
            out.extend_from_slice(&encode_expr(second, export)?);
            push_op(&mut out, abi::op::END, false, &[])?;
        }
        Expr::Equal(first, second) | Expr::Less(first, second) | Expr::LessEqual(first, second) => {
            let code = match expr {
                Expr::Equal(..) => abi::op::EQUAL,
                Expr::Less(..) => abi::op::LESS_THAN,
                _ => abi::op::LESS_EQUAL,
            };
            push_op(&mut out, code, true, &[])?;
            out.extend_from_slice(&encode_operand(first, export)?);
            out.extend_from_slice(&encode_operand(second, export)?);
            push_op(&mut out, abi::op::END, false, &[])?;
        }
        Expr::In(value, list) => {
            let Operand::Field(key) = value else {
                return Err(EncodeError::UnsupportedExpression);
            };
            let question = export.question(*key).ok_or(EncodeError::MissingQuestion)?;
            let count = u16::try_from(list.len()).map_err(|_| EncodeError::TooLong)?;
            let mut payload = Vec::new();
            push_u16(&mut payload, question.question);
            push_u16(&mut payload, count);
            for value in list {
                let number = match value {
                    Value::Number(number) => *number,
                    Value::Bool(flag) => u64::from(*flag),
                    _ => return Err(EncodeError::UnsupportedExpression),
                };
                let number = u16::try_from(number).map_err(|_| EncodeError::ValueOutOfRange)?;
                push_u16(&mut payload, number);
            }
            push_op(&mut out, abi::op::EQ_ID_VAL_LIST, false, &payload)?;
        }
        Expr::Unavailable(_) => return Err(EncodeError::UnsupportedExpression),
    }
    Ok(out)
}

/// Encodes one operand.
pub(super) fn encode_operand(operand: &Operand, export: &Export) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    match operand {
        Operand::Field(key) => {
            let question = export.question(*key).ok_or(EncodeError::MissingQuestion)?;
            push_op(
                &mut out,
                abi::op::QUESTION_REF1,
                false,
                &question.question.to_le_bytes(),
            )?;
        }
        Operand::Literal(value) => {
            let (code, number) = match value {
                Value::Bool(flag) => (abi::op::UINT8, u64::from(*flag)),
                Value::Number(number) => match Width::of(*number) {
                    Width::U8 => (abi::op::UINT8, *number),
                    Width::U16 => (abi::op::UINT16, *number),
                    Width::U32 => (abi::op::UINT32, *number),
                    Width::U64 => (abi::op::UINT64, *number),
                },
                _ => return Err(EncodeError::UnsupportedExpression),
            };
            let width = match code {
                abi::op::UINT8 => Width::U8,
                abi::op::UINT16 => Width::U16,
                abi::op::UINT32 => Width::U32,
                _ => Width::U64,
            };
            push_op(
                &mut out,
                code,
                false,
                &number.to_le_bytes()[..width.bytes()],
            )?;
        }
    }
    Ok(out)
}
