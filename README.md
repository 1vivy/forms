# forms

A setup-forms engine, its navigation session and its ratatui setup page, in
Rust.

The engine describes setup pages (formsets, forms, questions, conditions) and
keeps the navigation state of one front end. One widget draws that state as a
PC setup page. A front end supplies the key strokes and the ratatui backend.

The core crates have no firmware dependency, so a Linux program and a UEFI
image use the same engine. `forms-uefi` supplies the UEFI display and input
front end. The `hii-*` crates are optional HII adapters.

## Structure

| Item | Layer | Content | Builds for |
| --- | --- | --- | --- |
| `forms` | model | Formsets, forms, statements, questions, values, the `Provider` trait | `no_std` + `alloc`, any target |
| `forms::Session` | session | Navigation state: `Input` in, `Event` and `View` out | `no_std` + `alloc`, any target |
| `forms::widgets` | view | `FormsWidget` and `Theme`: draw a `View` as a setup page | `no_std` + `alloc`, any target |
| `forms-uefi` | front end | UEFI display and input: graphics output with a text console fallback, `KeyMap`, `Shell` | UEFI targets, host build for the key map tests |
| `hii-abi` | HII layouts | UEFI HII package and IFR layout constants, with no dependency | `no_std`, any target |
| `hii-ifr` | HII codec | IFR and HII string package codec for the forms model | `no_std` + `alloc`, any target |
| `hii-uefi` | HII provider | `forms::Provider` over the UEFI HII database | UEFI targets |

The crate is one unit because the parts are useless apart: a session without a
model has nothing to navigate, and a widget without a view has nothing to draw.
The session draws nothing, and the widget holds no state.

HII is an optional capability, not a foundation. `forms` and `forms-uefi` name
no HII type. A front end checks the HII database once and adds a `hii-uefi`
provider when the firmware publishes one. maOS browses the firmware formsets in
its Firmware Settings page. ODP `opq` initializes and repairs the inherited HII
stack, then shows its own pages through its native provider. A host without HII
keeps its native pages.

## Navigation

`forms::Session` ends every page with a `< Back` row, so a front end with only
three keys (a phone: volume up, volume down, power) can return to the previous
page; `with_back_row(false)` removes it for a front end with an Esc key. Up and
Down wrap at both ends of the selectable rows. A choice editor ends with the
same `< Back` row and wraps the same way, so Enter on that row cancels the
editor and leaves the committed value unchanged.
`Session::enter(guid)` opens one formset and pins it: Back on its entry form
ends the session instead of showing the list.
`with_entry_back_row(false)` removes only the pinned entry form's Back row.
`with_readable_rows(true)` lets Up and Down select information rows, so long
information pages scroll and show their help. Enter on these rows has no effect.

Navigation and refresh preserve both the provider handle and the formset GUID.
Different providers can publish the same GUID without sharing question state.
`enter(guid)` selects the first matching provider and retains that identity.

## The page

`FormsWidget` keeps the usual setup-browser columns at any width. The prompt is
on the left, the value is on the right, and one blank cell separates them. A
value that does not fit its column loses its tail and ends with `~`, so clipped
text is visible as clipped. A read-only value (`RowKind::Info`) wraps into the
value column instead, and the row takes one screen line per wrapped part. The
selection still follows rows, not lines. Key hints are centered with one blank
cell at each edge, and the widget uses the longest hint that fits the width. The
page footer changes with the active editor; popups do not repeat those hints.
Rows with no value use the full width for their prompt. Headings draw a rule
to the right edge. Set `Theme::heading_rule` to `false` to omit the rule.

`Theme` holds the colours and styles of one page. A front end starts from
`Theme::default()` and changes single fields.

## Use on Linux

A Linux program builds `forms::FormSet` values from its own settings, implements
`forms::Provider` over its own storage (`formsets`, `read`, `write`, `action`),
feeds key strokes to a `forms::Session` and draws each `forms::View` with
`forms::widgets::FormsWidget` on a ratatui terminal backend.

`cargo run -p forms --example host` runs that loop in a terminal over an
in-memory provider. `--once` draws one frame and prints it.

Draw a frame at the width of a phone screen, and a frame with an open editor:

```sh
cargo run -p forms --example host -- --once --columns 53 --rows 20
cargo run -p forms --example host -- --once --columns 53 --rows 20 --open
```

Check three things in the printed frame:

- The key hints are centered and keep one blank cell at each edge.
- Opening an editor changes the page footer without adding hints to its border.
- A long read-only value wraps, and a clipped value ends with `~`.

## Use on UEFI

A UEFI image implements `forms::Provider` over its own facts and a
`forms_uefi::Shell`. The shell receives one description of the display and
every turn, so it can run a boot timer and act on page commands. The image
calls `forms_uefi::run` with the default keyboard map, or
`forms_uefi::run_with` with another map such as `forms_uefi::map_key_phone` on
a phone.

The front end draws on the graphics output when the firmware publishes one, and
on the inherited text console otherwise. It prefers Text Input Ex and falls
back to basic console input on the same handle. Display and input are
independent: graphics without Text Input Ex, or text without graphics output,
still works.

## Build and check

Rust 1.95.0 stable (`rust-toolchain.toml`). The `forms` crate uses no feature
gate, because Patina firmware links it at the same pin.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

`--all-targets` also covers the host example of the `forms` crate.

Check the front end for the UEFI targets that rustup has installed:

```sh
cargo check -p forms-uefi --target x86_64-unknown-uefi
cargo check -p forms-uefi --target aarch64-unknown-uefi
```
