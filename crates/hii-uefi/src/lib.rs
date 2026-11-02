//! A [`forms::Provider`] over the UEFI HII database.
//!
//! [`hii::HiiProvider`] lists every formset in the HII database, decodes it
//! with `hii-ifr`, reads values through `EFI_HII_CONFIG_ROUTING_PROTOCOL` and
//! sends changes and actions to the owner's `EFI_HII_CONFIG_ACCESS_PROTOCOL`,
//! as a form browser does. HII registration is additive, so stock formsets and
//! formsets that a platform adds appear in the same list.
//!
//! The crate uses only `uefi-raw` layouts and boot-services calls, so a uefi-rs
//! application (with or without `std`) and any other UEFI image can use it.
//!
//! - [`hii`] implements the provider.
//! - [`raw`] holds the boot-services calls that the provider needs.
//! - [`text`] converts strings at the firmware boundary.
//! - [`hex`] encodes and decodes hexadecimal byte strings for name/value stores.
//! - [`shape`] maps a statement to the width and value type that firmware expects.
//!
//! Modules that call firmware exist on UEFI targets only.
#![no_std]

extern crate alloc;

pub mod hex;
pub mod shape;
pub mod text;

#[cfg(target_os = "uefi")]
pub mod hii;
#[cfg(target_os = "uefi")]
pub mod raw;
