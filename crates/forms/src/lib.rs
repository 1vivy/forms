//! Platform-neutral form pages, their editing session and their rendering.
//!
//! This crate owns the description of a setup page. A *set* holds *pages*;
//! a page holds *rows*; a row has a stable [`Key`], a label, a [`Control`] and
//! an optional value binding. Values live in a [`Store`] or behind a
//! [`Provider`]; the crate never assigns meaning to a key.
//!
//! - [`Builder`] allocates keys and builds a [`FormSet`] as data.
//! - [`Session`] turns [`Input`] into store or provider calls and produces a
//!   [`View`] that a renderer draws. It draws nothing itself.
//! - [`Registry`] holds the handlers of the control kinds ([`controls`]). Built-in
//!   kinds use the same handlers, so a consumer replaces or adds a control without
//!   forking the crate.
//! - [`Modal`] is a general modal decision with its own buttons and roles.
//! - [`render`] draws a view and a modal through ratatui, places the page on a
//!   display, and holds the [`render::Theme`].
//!
//! | Module | Layer |
//! | --- | --- |
//! | [`model`] | The description: everything a HII form set can state, in forms' own types |
//! | [`session`] | Navigation, editing, conditions, validation, defaults, refresh |
//! | [`controls`] | Handlers: `standard` (HII controls) and `extras` (forms-only controls) |
//! | [`modal`] | Modal decisions |
//! | [`render`] | Drawing, theme and page placement; no firmware or display binding |
//!
//! No type in this crate carries a firmware identity, a firmware storage
//! location or a firmware opcode. A HII adapter maps the set to and from that
//! format in its own crate.
//!
//! # Examples
//!
//! ```
//! use forms::{Builder, Control, Key, MemoryProvider, Session, Value};
//!
//! let mut builder = Builder::new(Key(1), "Platform");
//! builder.page("Main");
//! builder.row("Secure boot", Control::Toggle).set_binding(Key(10));
//! let set = builder.build();
//!
//! let mut provider = MemoryProvider::new(vec![(0u8, set)]);
//! provider.set(Key(10), Value::Bool(false));
//! let mut session = Session::new(provider.sets.clone());
//! assert!(session.enter(Key(1)));
//! let view = session.view(&mut provider);
//! assert_eq!(view.rows[0].value, "Off");
//! ```
#![no_std]

extern crate alloc;

pub mod controls;
pub mod modal;
pub mod model;
pub mod render;
pub mod session;

pub use controls::{Decoration, EditState, EditStep, Field, Handler, Registry};
pub use modal::{Button, ButtonRole, Modal, ModalOutcome, ModalStyle, Progress};
pub use model::{
    Builder, Choice, Control, DefaultSet, Error, Expr, FieldBinding, FieldValue, FormSet, Key,
    MemoryProvider, MemoryStore, Operand, Page, Presentation, Provider, Refresh, Request, Row,
    Scope, Store, Target, Truncation, Validation, ValidationKind, Value,
};
pub use session::{Editor, EditorKind, Event, Input, Keypad, Location, RowView, Session, View};
