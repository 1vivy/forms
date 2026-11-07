//! A [`forms::Provider`] over the UEFI HII database.
//!
//! `hii::HiiProvider` lists every formset in the HII database, decodes it
//! with `hii-ifr`, reads values through `EFI_HII_CONFIG_ROUTING_PROTOCOL` and
//! sends changes and actions to the owner's `EFI_HII_CONFIG_ACCESS_PROTOCOL`,
//! as a form browser does. The driver answers with a
//! [`forms::Request`], which the provider passes to the session, and the
//! provider commits or discards a scope when the session asks. HII
//! registration is additive, so stock formsets and formsets that a platform
//! adds appear in the same list.
//!
//! The crate uses only `uefi-raw` layouts and boot-services calls, so a uefi-rs
//! application (with or without `std`) and any other UEFI image can use it.
//!
//! - `hii` implements the provider.
//! - `producer` builds and parses configuration data for a published formset.
//! - `popup` serves `EFI_HII_POPUP_PROTOCOL` through the native `forms::Modal`.
//! - `map` maps firmware action requests and IFR value types.
//! - `publish` registers a provider through HII Config Access on UEFI.
//! - `raw` holds the boot-services calls that the provider needs.
//! - `text` converts strings at the firmware boundary.
//! - `hex` encodes and decodes hexadecimal byte strings for name/value stores.
//!
//! Modules that call firmware exist on UEFI targets only. The producer layout
//! and configuration codec build on the host, so their tests run with `cargo
//! test`. The popup traits and its protocol logic also build on the host.
#![no_std]

extern crate alloc;

pub mod hex;
pub mod map;
pub mod popup;
pub mod producer;
pub mod text;

pub use map::{
    ifr_type, kind_of, map_request, reported, request_for, request_value, selection_of, style_of,
};

#[cfg(target_os = "uefi")]
pub mod hii;
#[cfg(target_os = "uefi")]
pub mod publish;
#[cfg(target_os = "uefi")]
pub mod raw;
