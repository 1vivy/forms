//! Raw HII package and IFR layout constants.
//!
//! The values match the UEFI 2.10 structures (`EFI_HII_PACKAGE_HEADER`,
//! `EFI_HII_STRING_PACKAGE_HDR` and the `EFI_IFR_*` opcodes) and the EDK II
//! headers in `MdePkg/Include/Uefi/UefiInternalFormRepresentation.h`. The
//! crate has no logic and no dependency, so a Patina driver (r-efi), a uefi-rs
//! application and a host tool share one definition.
#![no_std]

/// Size of `EFI_HII_PACKAGE_LIST_HEADER` in bytes (a GUID and a length).
pub const PACKAGE_LIST_HEADER_SIZE: usize = 20;

/// Size of `EFI_HII_PACKAGE_HEADER` in bytes.
pub const PACKAGE_HEADER_SIZE: usize = 4;

/// Largest value of the 24-bit package length field.
pub const MAX_PACKAGE_LENGTH: u32 = 0x00FF_FFFF;

/// Largest value of the 7-bit IFR opcode length field.
pub const MAX_OPCODE_LENGTH: usize = 0x7F;

/// Package type of a forms package (`EFI_HII_PACKAGE_FORMS`).
pub const PACKAGE_FORMS: u8 = 0x02;

/// Package type of a strings package (`EFI_HII_PACKAGE_STRINGS`).
pub const PACKAGE_STRINGS: u8 = 0x04;

/// Package type that ends a package list (`EFI_HII_PACKAGE_END`).
pub const PACKAGE_END: u8 = 0xDF;

/// IFR opcodes (`EFI_IFR_*_OP`).
pub mod op {
    /// `EFI_IFR_FORM_OP`.
    pub const FORM: u8 = 0x01;
    /// `EFI_IFR_SUBTITLE_OP`.
    pub const SUBTITLE: u8 = 0x02;
    /// `EFI_IFR_TEXT_OP`.
    pub const TEXT: u8 = 0x03;
    /// `EFI_IFR_ONE_OF_OP`.
    pub const ONE_OF: u8 = 0x05;
    /// `EFI_IFR_CHECKBOX_OP`.
    pub const CHECKBOX: u8 = 0x06;
    /// `EFI_IFR_NUMERIC_OP`.
    pub const NUMERIC: u8 = 0x07;
    /// `EFI_IFR_ONE_OF_OPTION_OP`.
    pub const ONE_OF_OPTION: u8 = 0x09;
    /// `EFI_IFR_SUPPRESS_IF_OP`.
    pub const SUPPRESS_IF: u8 = 0x0A;
    /// `EFI_IFR_ACTION_OP`.
    pub const ACTION: u8 = 0x0C;
    /// `EFI_IFR_FORM_SET_OP`.
    pub const FORM_SET: u8 = 0x0E;
    /// `EFI_IFR_REF_OP` (forms 1 to 4).
    pub const REF: u8 = 0x0F;
    /// `EFI_IFR_EQ_ID_VAL_OP`.
    pub const EQ_ID_VAL: u8 = 0x12;
    /// `EFI_IFR_EQ_ID_ID_OP`.
    pub const EQ_ID_ID: u8 = 0x13;
    /// `EFI_IFR_EQ_ID_VAL_LIST_OP`.
    pub const EQ_ID_VAL_LIST: u8 = 0x14;
    /// `EFI_IFR_AND_OP`.
    pub const AND: u8 = 0x15;
    /// `EFI_IFR_OR_OP`.
    pub const OR: u8 = 0x16;
    /// `EFI_IFR_NOT_OP`.
    pub const NOT: u8 = 0x17;
    /// `EFI_IFR_GRAY_OUT_IF_OP`.
    pub const GRAY_OUT_IF: u8 = 0x19;
    /// `EFI_IFR_STRING_OP`.
    pub const STRING: u8 = 0x1C;
    /// `EFI_IFR_VARSTORE_OP`.
    pub const VARSTORE: u8 = 0x24;
    /// `EFI_IFR_VARSTORE_NAME_VALUE_OP`.
    pub const VARSTORE_NAME_VALUE: u8 = 0x25;
    /// `EFI_IFR_VARSTORE_EFI_OP`.
    pub const VARSTORE_EFI: u8 = 0x26;
    /// `EFI_IFR_END_OP`.
    pub const END: u8 = 0x29;
    /// `EFI_IFR_DEFAULT_OP`.
    pub const DEFAULT: u8 = 0x5B;
    /// `EFI_IFR_DEFAULTSTORE_OP`.
    pub const DEFAULTSTORE: u8 = 0x5C;
    /// `EFI_IFR_GUID_OP`.
    pub const GUID: u8 = 0x5F;
    /// `EFI_IFR_TRUE_OP`.
    pub const TRUE: u8 = 0x46;
    /// `EFI_IFR_FALSE_OP`.
    pub const FALSE: u8 = 0x47;
}

/// String information block types (`EFI_HII_SIBT_*`).
pub mod sibt {
    /// `EFI_HII_SIBT_END`.
    pub const END: u8 = 0x00;
    /// `EFI_HII_SIBT_STRING_SCSU`.
    pub const STRING_SCSU: u8 = 0x10;
    /// `EFI_HII_SIBT_STRING_SCSU_FONT`.
    pub const STRING_SCSU_FONT: u8 = 0x11;
    /// `EFI_HII_SIBT_STRINGS_SCSU`.
    pub const STRINGS_SCSU: u8 = 0x12;
    /// `EFI_HII_SIBT_STRINGS_SCSU_FONT`.
    pub const STRINGS_SCSU_FONT: u8 = 0x13;
    /// `EFI_HII_SIBT_STRING_UCS2`.
    pub const STRING_UCS2: u8 = 0x14;
    /// `EFI_HII_SIBT_STRING_UCS2_FONT`.
    pub const STRING_UCS2_FONT: u8 = 0x15;
    /// `EFI_HII_SIBT_STRINGS_UCS2`.
    pub const STRINGS_UCS2: u8 = 0x16;
    /// `EFI_HII_SIBT_STRINGS_UCS2_FONT`.
    pub const STRINGS_UCS2_FONT: u8 = 0x17;
    /// `EFI_HII_SIBT_DUPLICATE`.
    pub const DUPLICATE: u8 = 0x20;
    /// `EFI_HII_SIBT_SKIP2`.
    pub const SKIP2: u8 = 0x21;
    /// `EFI_HII_SIBT_SKIP1`.
    pub const SKIP1: u8 = 0x22;
    /// `EFI_HII_SIBT_EXT1`.
    pub const EXT1: u8 = 0x30;
    /// `EFI_HII_SIBT_EXT2`.
    pub const EXT2: u8 = 0x31;
    /// `EFI_HII_SIBT_EXT4`.
    pub const EXT4: u8 = 0x32;
    /// `EFI_HII_SIBT_FONT`.
    pub const FONT: u8 = 0x40;
}

/// Question header and statement flags (`EFI_IFR_FLAG_*`, `EFI_IFR_*_DEFAULT`).
pub mod flags {
    /// `EFI_IFR_FLAG_READ_ONLY`.
    pub const READ_ONLY: u8 = 0x01;
    /// `EFI_IFR_FLAG_CALLBACK`.
    pub const CALLBACK: u8 = 0x04;
    /// `EFI_IFR_FLAG_RESET_REQUIRED`.
    pub const RESET_REQUIRED: u8 = 0x10;
    /// `EFI_IFR_FLAG_RECONNECT_REQUIRED`.
    pub const RECONNECT_REQUIRED: u8 = 0x40;
    /// `EFI_IFR_CHECKBOX_DEFAULT`.
    pub const CHECKBOX_DEFAULT: u8 = 0x01;
    /// `EFI_IFR_OPTION_DEFAULT`.
    pub const OPTION_DEFAULT: u8 = 0x10;
    /// `EFI_IFR_NUMERIC_SIZE` mask.
    pub const NUMERIC_SIZE: u8 = 0x03;
    /// `EFI_IFR_DISPLAY` mask.
    pub const DISPLAY: u8 = 0x30;
    /// `EFI_IFR_DISPLAY_UINT_DEC`.
    pub const DISPLAY_UINT_DEC: u8 = 0x10;
}

/// Value types of `EFI_IFR_TYPE_VALUE` (`EFI_IFR_TYPE_*`).
pub mod value_type {
    /// `EFI_IFR_TYPE_NUM_SIZE_8`.
    pub const NUM_SIZE_8: u8 = 0x00;
    /// `EFI_IFR_TYPE_NUM_SIZE_16`.
    pub const NUM_SIZE_16: u8 = 0x01;
    /// `EFI_IFR_TYPE_NUM_SIZE_32`.
    pub const NUM_SIZE_32: u8 = 0x02;
    /// `EFI_IFR_TYPE_NUM_SIZE_64`.
    pub const NUM_SIZE_64: u8 = 0x03;
    /// `EFI_IFR_TYPE_BOOLEAN`.
    pub const BOOLEAN: u8 = 0x04;
    /// `EFI_IFR_TYPE_STRING`.
    pub const STRING: u8 = 0x07;
    /// `EFI_IFR_TYPE_ACTION`.
    pub const ACTION: u8 = 0x0A;
}

/// Byte offsets inside a `EFI_HII_STRING_PACKAGE_HDR`.
pub mod string_header {
    /// Offset of `HdrSize`.
    pub const HDR_SIZE: usize = 4;
    /// Offset of `StringInfoOffset`.
    pub const STRING_INFO_OFFSET: usize = 8;
    /// Offset of `LanguageWindow`.
    pub const LANGUAGE_WINDOW: usize = 12;
    /// Number of `CHAR16` entries in `LanguageWindow`.
    pub const LANGUAGE_WINDOW_LEN: usize = 16;
    /// Offset of `LanguageName`.
    pub const LANGUAGE_NAME: usize = 44;
    /// Offset of the null-terminated `Language` text.
    pub const LANGUAGE: usize = 46;
    /// Smallest valid `HdrSize` for a language text of one character.
    pub const MIN_HEADER_SIZE: usize = 48;
}
