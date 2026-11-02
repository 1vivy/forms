//! Decoding of a compiled foreign formset and refusal of malformed input.

use forms::{
    Choice, Expr, FormId, QuestionFlags, QuestionId, Statement, Storage, VarStoreId, VarStoreKind,
    Width,
};
use hii_ifr::{DecodeError, decode};

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
    out.extend_from_slice(&(forms_size | (u32::from(0x02u8) << 24)).to_le_bytes());
    out.extend_from_slice(forms);
    out.extend_from_slice(&body);
    out
}

#[test]
fn compiled_formset_decodes() {
    let sets = decode(FIXTURE).expect("decode");
    assert_eq!(sets.len(), 1);
    let set = &sets[0];
    assert_eq!(
        set.guid,
        [
            0x0d, 0x24, 0x7a, 0x2b, 0xad, 0xd5, 0xd6, 0x4f, 0xbe, 0x1c, 0xdf, 0xa4, 0x41, 0x5f,
            0x55, 0x26
        ]
    );
    assert_eq!(set.title, "O/S Hardware Description Selection");
    assert_eq!(
        set.help,
        "Press <Enter> to choose between ACPI and DT hardware descriptions."
    );
    assert_eq!(
        set.class_guids,
        vec![[
            0x71, 0x99, 0x03, 0x93, 0x45, 0x85, 0x04, 0x4b, 0xb4, 0x5e, 0x32, 0xeb, 0x83, 0x26,
            0x04, 0x0e
        ]]
    );
    assert_eq!(set.varstores.len(), 1);
    assert_eq!(
        set.varstores[0],
        forms::VarStore {
            id: VarStoreId(1),
            guid: set.guid,
            name: "DtAcpiPref".into(),
            size: 4,
            kind: VarStoreKind::Efi { attributes: 3 },
        }
    );
    assert_eq!(set.forms.len(), 1);
    let form = &set.forms[0];
    assert_eq!(form.id, FormId(0x1000));
    assert_eq!(form.title, "O/S Hardware Description Selection");
    assert_eq!(form.items.len(), 2);
    let Statement::OneOf {
        question,
        width,
        choices,
    } = &form.items[0].statement
    else {
        panic!(
            "expected a one-of statement, got {:?}",
            form.items[0].statement
        );
    };
    assert_eq!(question.id, QuestionId(1));
    assert_eq!(question.prompt, "O/S Hardware Description");
    assert_eq!(
        question.help,
        "Select the hardware description that will be exposed to the O/S."
    );
    assert_eq!(
        question.storage,
        Storage::Offset {
            varstore: VarStoreId(1),
            offset: 0
        }
    );
    assert_eq!(question.flags, QuestionFlags(0x14));
    assert_eq!(*width, Width::U8);
    assert_eq!(
        choices,
        &vec![
            Choice {
                text: "Device Tree".into(),
                value: 0,
                default: true
            },
            Choice {
                text: "ACPI".into(),
                value: 1,
                default: false
            },
        ]
    );
    assert_eq!(
        form.items[1].statement,
        Statement::Subtitle {
            text: String::new()
        }
    );
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
fn ref5_statements_are_skipped() {
    let mut forms = op(0x0E, true, &[0u8; 21]);
    forms.extend_from_slice(&op(0x01, true, &[1, 0, 0, 0]));
    forms.extend_from_slice(&op(0x0F, false, &[0u8; 11]));
    forms.extend_from_slice(&op(0x03, false, &[0, 0, 0, 0, 0, 0]));
    forms.extend_from_slice(&op(0x29, false, &[]));
    forms.extend_from_slice(&op(0x29, false, &[]));

    let sets = decode(&list(&forms, &[])).expect("decode");
    let items = &sets[0].forms[0].items;
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].statement,
        Statement::Text {
            prompt: String::new(),
            help: String::new(),
            value: String::new(),
        }
    );
}

#[test]
fn unknown_statements_are_skipped_and_unknown_expressions_are_unsupported() {
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
    assert_eq!(sets[0].forms.len(), 1);
    let items = &sets[0].forms[0].items;
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].statement,
        Statement::Text {
            prompt: String::new(),
            help: String::new(),
            value: String::new()
        }
    );
    assert_eq!(items[0].suppress_if, Some(Expr::Unsupported));
}
