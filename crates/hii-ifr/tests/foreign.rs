//! Decoding of a compiled foreign formset and refusal of malformed input.

use forms::{Control, Expr};
use hii_ifr::{DecodeError, QuestionKind, Storage, Width, decode};

/// The compiled `DtPlatformDxe` formset from the Mu-Silicium build.
const FIXTURE: &[u8] = include_bytes!("fixtures/dt_platform_hii.bin");

/// Appends an IFR opcode.
fn op(code: u8, scope: bool, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![
        code,
        (payload.len() + 2) as u8 | if scope { 0x80 } else { 0 },
    ];
    out.extend_from_slice(payload);
    out
}

/// Builds a package list with a forms package and optional extra packages.
fn list(forms: &[u8], packages: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (kind, data) in packages {
        let size = (data.len() + 4) as u32;
        body.extend_from_slice(&(size | (u32::from(*kind) << 24)).to_le_bytes());
        body.extend_from_slice(data);
    }
    let forms_size = (forms.len() + 4) as u32;
    let mut out = vec![0u8; 16];
    let total = 20 + 4 + forms.len() + body.len();
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(forms_size | (0x02u32 << 24)).to_le_bytes());
    out.extend_from_slice(forms);
    out.extend_from_slice(&body);
    out
}

#[test]
fn compiled_formset_decodes() {
    let sets = decode(FIXTURE).expect("decode");
    assert_eq!(sets.len(), 1);
    let decoded = &sets[0];
    assert_eq!(
        decoded.export.guid,
        [
            0x0d, 0x24, 0x7a, 0x2b, 0xad, 0xd5, 0xd6, 0x4f, 0xbe, 0x1c, 0xdf, 0xa4, 0x41, 0x5f,
            0x55, 0x26
        ]
    );
    assert_eq!(decoded.set.title, "O/S Hardware Description Selection");
    assert_eq!(
        decoded.set.help,
        "Press <Enter> to choose between ACPI and DT hardware descriptions."
    );
    assert_eq!(
        decoded.export.class_guids,
        vec![[
            0x71, 0x99, 0x03, 0x93, 0x45, 0x85, 0x04, 0x4b, 0xb4, 0x5e, 0x32, 0xeb, 0x83, 0x26,
            0x04, 0x0e
        ]]
    );
    assert_eq!(decoded.export.varstores.len(), 1);
    assert_eq!(decoded.export.varstores[0].name, "DtAcpiPref");
    assert_eq!(decoded.export.varstores[0].size, 4);
    assert_eq!(decoded.set.pages.len(), 1);
    let page = &decoded.set.pages[0];
    assert_eq!(page.title, "O/S Hardware Description Selection");
    assert_eq!(page.rows.len(), 2);
    let row = &page.rows[0];
    let Control::Choice(choices) = &row.control else {
        panic!("expected a choice, got {:?}", row.control);
    };
    assert_eq!(row.label, "O/S Hardware Description");
    assert_eq!(
        row.help,
        "Select the hardware description that will be exposed to the O/S."
    );
    assert_eq!(
        choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Device Tree", "ACPI"]
    );
    assert_eq!(choices[1].value, 1);
    let question = &decoded.export.questions[0];
    assert_eq!(question.question, 1);
    assert_eq!(
        question.storage,
        Storage::Offset {
            store: 1,
            offset: 0
        }
    );
    assert_eq!(question.kind, QuestionKind::Number(Width::U8));
    assert_eq!(page.rows[1].control, Control::Heading);
}

#[test]
fn reserved_package_types_are_refused() {
    let mut list = vec![0u8; 16];
    list.extend_from_slice(&26u32.to_le_bytes());
    list.extend_from_slice(&(6u32 | (0xE0u32 << 24)).to_le_bytes());
    list.extend_from_slice(&[0, 0]);
    assert_eq!(decode(&list), Err(DecodeError::BadPackage));
}

#[test]
fn class_guid_count_larger_than_the_data_is_refused() {
    let mut payload = vec![0u8; 20];
    payload.push(3);
    payload.extend_from_slice(&[0u8; 16]);
    let mut forms = op(0x0E, true, &payload);
    forms.extend_from_slice(&op(0x29, false, &[]));
    assert_eq!(decode(&list(&forms, &[])), Err(DecodeError::BadLength));
}

#[test]
fn truncated_header_is_refused() {
    assert_eq!(decode(&[0u8; 19]), Err(DecodeError::Truncated));
}

#[test]
fn length_beyond_the_slice_is_refused() {
    let mut list = vec![0u8; 16];
    list.extend_from_slice(&100u32.to_le_bytes());
    assert_eq!(decode(&list), Err(DecodeError::Truncated));
}

#[test]
fn package_beyond_the_list_is_refused() {
    let mut list = vec![0u8; 16];
    list.extend_from_slice(&24u32.to_le_bytes());
    list.extend_from_slice(&(0x100u32 | (0x02u32 << 24)).to_le_bytes());
    assert_eq!(decode(&list), Err(DecodeError::BadPackage));
}

#[test]
fn package_shorter_than_its_header_is_refused() {
    let mut list = vec![0u8; 16];
    list.extend_from_slice(&24u32.to_le_bytes());
    list.extend_from_slice(&(3u32 | (0x02u32 << 24)).to_le_bytes());
    assert_eq!(decode(&list), Err(DecodeError::BadLength));
}

#[test]
fn opcode_shorter_than_its_header_is_refused() {
    let forms = vec![0x02, 0x01];
    assert_eq!(decode(&list(&forms, &[])), Err(DecodeError::BadLength));
}

#[test]
fn unclosed_scope_is_refused() {
    let forms = op(0x0E, true, &[0u8; 21]);
    assert_eq!(
        decode(&list(&forms, &[])),
        Err(DecodeError::UnbalancedScope)
    );
}

#[test]
fn malformed_strings_package_is_refused() {
    let mut list = vec![0u8; 16];
    let forms = op(0x2F, false, &[]);
    let forms_size = (forms.len() + 4) as u32;
    let strings = [0u8; 8];
    let strings_size = (strings.len() + 4) as u32;
    let total = 20 + forms.len() + 4 + strings.len() + 4;
    list.extend_from_slice(&(total as u32).to_le_bytes());
    list.extend_from_slice(&(forms_size | (0x02u32 << 24)).to_le_bytes());
    list.extend_from_slice(&forms);
    list.extend_from_slice(&(strings_size | (0x04u32 << 24)).to_le_bytes());
    list.extend_from_slice(&strings);
    assert_eq!(decode(&list), Err(DecodeError::BadStrings));
}

#[test]
fn unknown_packages_are_skipped() {
    let mut list = FIXTURE.to_vec();
    let end = list.len() - 4;
    list.truncate(end);
    let extra = [1u8, 2, 3, 4];
    let size = (extra.len() + 4) as u32;
    list.extend_from_slice(&(size | (0x01u32 << 24)).to_le_bytes());
    list.extend_from_slice(&extra);
    list.extend_from_slice(&(4u32 | (0xDFu32 << 24)).to_le_bytes());
    let total = list.len() as u32;
    list[16..20].copy_from_slice(&total.to_le_bytes());
    assert_eq!(decode(&list).expect("decode").len(), 1);
}

#[test]
fn a_short_ref_keeps_a_link_row() {
    let mut forms = op(0x0E, true, &[0u8; 21]);
    forms.extend_from_slice(&op(0x01, true, &[1, 0, 0, 0]));
    forms.extend_from_slice(&op(0x0F, false, &[0u8; 11]));
    forms.extend_from_slice(&op(0x03, false, &[0, 0, 0, 0, 0, 0]));
    forms.extend_from_slice(&op(0x29, false, &[]));
    forms.extend_from_slice(&op(0x29, false, &[]));

    let sets = decode(&list(&forms, &[])).expect("decode");
    let rows = &sets[0].set.pages[0].rows;
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0].control, Control::Link(_)));
    assert_eq!(rows[1].control, Control::Text(String::new()));
}

#[test]
fn unknown_statements_stay_visible_and_unknown_expressions_are_unavailable() {
    let mut forms = op(0x0E, true, &[0u8; 21]);
    forms.extend_from_slice(&op(0x01, true, &[1, 0, 0, 0]));
    forms.extend_from_slice(&op(0x99, true, &[]));
    forms.extend_from_slice(&op(0x03, false, &[0, 0, 0, 0, 0, 0]));
    forms.extend_from_slice(&op(0x29, false, &[]));
    forms.extend_from_slice(&op(0x0A, true, &[]));
    forms.extend_from_slice(&op(0x42, false, &[7]));
    forms.extend_from_slice(&op(0x03, false, &[0, 0, 0, 0, 0, 0]));
    forms.extend_from_slice(&op(0x29, false, &[]));
    forms.extend_from_slice(&op(0x29, false, &[]));
    forms.extend_from_slice(&op(0x29, false, &[]));

    let sets = decode(&list(&forms, &[])).expect("decode");
    assert_eq!(sets.len(), 1);
    let rows = &sets[0].set.pages[0].rows;
    // The foreign scope stays visible, and so does the text inside it.
    assert_eq!(rows.len(), 3);
    assert!(matches!(rows[0].control, Control::Unsupported(_)));
    assert_eq!(rows[1].control, Control::Text(String::new()));
    assert!(rows[1].hidden.is_none());
    assert!(matches!(rows[2].hidden, Some(Expr::Unavailable(_))));
}

/// Appends one form around the given statements.
fn form(statements: &[u8]) -> Vec<u8> {
    let mut forms = op(0x0E, true, &[0u8; 21]);
    forms.extend_from_slice(&op(0x01, true, &[1, 0, 0, 0]));
    forms.extend_from_slice(statements);
    forms.extend_from_slice(&op(0x29, false, &[]));
    forms.extend_from_slice(&op(0x29, false, &[]));
    forms
}

#[test]
fn a_disable_if_scope_hides_its_row() {
    let text = op(0x03, false, &[0, 0, 0, 0, 0, 0]);
    let mut statements = op(0x1E, true, &[]);
    statements.extend_from_slice(&op(0x46, false, &[]));
    statements.extend_from_slice(&text);
    statements.extend_from_slice(&op(0x29, false, &[]));

    let sets = decode(&list(&form(&statements), &[])).expect("decode");
    let rows = &sets[0].set.pages[0].rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].hidden, Some(Expr::Bool(true)));
    assert!(rows[0].disabled.is_none());
}

#[test]
fn a_gray_out_if_scope_disables_its_row() {
    let text = op(0x03, false, &[0, 0, 0, 0, 0, 0]);
    let mut statements = op(0x19, true, &[]);
    statements.extend_from_slice(&op(0x46, false, &[]));
    statements.extend_from_slice(&text);
    statements.extend_from_slice(&op(0x29, false, &[]));

    let sets = decode(&list(&form(&statements), &[])).expect("decode");
    let rows = &sets[0].set.pages[0].rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].disabled, Some(Expr::Bool(true)));
    assert!(rows[0].hidden.is_none());
}
