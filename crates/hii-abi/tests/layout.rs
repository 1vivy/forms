//! Cross-checks of the raw layout constants against the firmware bindings.

use core::mem::size_of;
use uefi_raw::protocol::hii::ifr::IfrOpcode;
use uefi_raw::protocol::hii::{HiiPackageHeader, HiiPackageListHeader};

#[test]
fn header_sizes_match_the_firmware_bindings() {
    assert_eq!(
        hii_abi::PACKAGE_LIST_HEADER_SIZE,
        size_of::<HiiPackageListHeader>()
    );
    assert_eq!(hii_abi::PACKAGE_HEADER_SIZE, size_of::<HiiPackageHeader>());
}

#[test]
fn opcodes_match_the_firmware_bindings() {
    use hii_abi::op;
    assert_eq!(IfrOpcode::FORM.0, op::FORM);
    assert_eq!(IfrOpcode::SUBTITLE.0, op::SUBTITLE);
    assert_eq!(IfrOpcode::TEXT.0, op::TEXT);
    assert_eq!(IfrOpcode::ONE_OF.0, op::ONE_OF);
    assert_eq!(IfrOpcode::CHECKBOX.0, op::CHECKBOX);
    assert_eq!(IfrOpcode::NUMERIC.0, op::NUMERIC);
    assert_eq!(IfrOpcode::ONE_OF_OPTION.0, op::ONE_OF_OPTION);
    assert_eq!(IfrOpcode::SUPPRESS_IF.0, op::SUPPRESS_IF);
    assert_eq!(IfrOpcode::ACTION.0, op::ACTION);
    assert_eq!(IfrOpcode::FORM_SET.0, op::FORM_SET);
    assert_eq!(IfrOpcode::REF.0, op::REF);
    assert_eq!(IfrOpcode::EQ_ID_VAL.0, op::EQ_ID_VAL);
    assert_eq!(IfrOpcode::EQ_ID_ID.0, op::EQ_ID_ID);
    assert_eq!(IfrOpcode::EQ_ID_VAL_LIST.0, op::EQ_ID_VAL_LIST);
    assert_eq!(IfrOpcode::AND.0, op::AND);
    assert_eq!(IfrOpcode::OR.0, op::OR);
    assert_eq!(IfrOpcode::NOT.0, op::NOT);
    assert_eq!(IfrOpcode::GRAY_OUT_IF.0, op::GRAY_OUT_IF);
    assert_eq!(IfrOpcode::STRING.0, op::STRING);
    assert_eq!(IfrOpcode::VARSTORE.0, op::VARSTORE);
    assert_eq!(IfrOpcode::VARSTORE_NAME_VALUE.0, op::VARSTORE_NAME_VALUE);
    assert_eq!(IfrOpcode::VARSTORE_EFI.0, op::VARSTORE_EFI);
    assert_eq!(IfrOpcode::END.0, op::END);
    assert_eq!(IfrOpcode::DEFAULT.0, op::DEFAULT);
    assert_eq!(IfrOpcode::DEFAULTSTORE.0, op::DEFAULTSTORE);
    assert_eq!(IfrOpcode::GUID.0, op::GUID);
    assert_eq!(IfrOpcode::TRUE.0, op::TRUE);
    assert_eq!(IfrOpcode::FALSE.0, op::FALSE);
}
